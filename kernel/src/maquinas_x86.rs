//! Las máquinas virtuales en x86-64 (X10), vistas como objetos con llave: la misma interfaz que `maquinas.rs` de ARM
//! (llamadas 45 a 48), para que el monitor de máquinas sirva en las dos arquitecturas. Lo que es de la CPU está en
//! `base/x86_64/hiper.rs` (AMD-V o VT-x); aquí, qué se le dice al servicio y lo que el kernel hace por él.
//!
//! Lo mismo que en ARM: cada llave es una vCPU (hasta 8, cada una con su ID de APIC); "correr" entra en el invitado
//! hasta que sale y lo dice ya interpretado (`PALABRAS`); "patear" la saca (una interrupción a su núcleo, o la despierta
//! si duerme en un HLT); "parar" mata la máquina. Lo que cambia:
//!
//! - HVC es VMMCALL (AMD) o VMCALL (Intel): valor = rax, y x1, x2 y x3 son rbx, rcx y rdx.
//! - Las lecturas y escrituras en una dirección que no es su memoria (los aparatos que emula el servicio): x86 no dice
//!   qué instrucción fue (salvo AMD con su ayuda), así que el kernel la lee de la memoria del invitado (recorriendo sus
//!   tablas de páginas) y la decodifica: MOV entre registro y memoria, MOV de un inmediato, MOVZX y MOVSX, de 1 a 8
//!   bytes. Otra instrucción sobre un aparato es un fallo.
//! - WFI es HLT. REGISTRO es RDMSR o WRMSR (dirección = la MSR; el valor, edx:eax).
//! - Motivo nuevo, 8 (`PUERTO`): IN u OUT. Dirección = el puerto, tamaño 1, 2 o 4, x1 = 1 si es IN (lo que diga el
//!   servicio va a al, ax o eax). INS y OUTS (de cadena) son un fallo.
//! - Las interrupciones: el servicio emula el APIC (y el IOAPIC) y dice qué vectores entregar, en el bloque de
//!   interrupciones (`IRQ_PALABRAS`, el mismo tamaño que en ARM): hasta 8 huecos, cada uno con su vector (bits 7:0) y
//!   el bit 8 si está pendiente. El kernel entrega el mayor en cuanto el invitado lo acepta y deja ese hueco a 0. El
//!   temporizador lo lleva el kernel, como el virtual de ARM: el plazo del TSC (IA32_TSC_DEADLINE) que escribe el
//!   invitado, con su vector y si está encendido en la palabra 1 (lo pone el servicio, que emula la LVT); cuando vence
//!   lo entrega y lo dice con `TEMPORIZADOR_ENTREGADO`.

use crate::base::hiper::{self, Espacio, Salida, Vcpu};
use crate::cerrojo::Cerrojo;
use crate::memoria::Memoria;
use crate::planificador::{self, ColaEspera};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

pub const HVC: u64 = 1;
pub const LECTURA: u64 = 2;
pub const ESCRITURA: u64 = 3;
pub const WFI: u64 = 4;
pub const INTERRUMPIDO: u64 = 5;
pub const FALLO: u64 = 6;
pub const REGISTRO: u64 = 7;
/// x86: IN u OUT.
pub const PUERTO: u64 = 8;

/// Palabras del informe, como en ARM: motivo, RIP de la instrucción, dirección (física del invitado, puerto o MSR),
/// valor, tamaño, el código de salida de la CPU (el de AMD o la razón de Intel), x1 (rbx en una hipercall; 1 si lee en
/// PUERTO y REGISTRO), x2 (rcx), las cuentas del contador fuera del kernel, x3 (rdx), el plazo de su temporizador (el
/// del TSC, si está puesto y el servicio lo tiene encendido; si no, `u64::MAX`), señales (bit 0: algo pendiente que
/// entregar) y los µs que durmió en el kernel en sus HLT.
pub const PALABRAS: usize = 13;

pub const IRQ_LRS: usize = 8;
/// El bloque de interrupciones: [0] cuántos huecos usa el servicio (lo pone el kernel: 8; 0 sin hipervisor); [1] lo pone
/// el servicio: bit 0 temporizador encendido, [15:8] su vector, bit 17 `ESPERAR_WFI`; el kernel añade
/// `TEMPORIZADOR_ENTREGADO`; desde [2], los huecos: vector y `PENDIENTE`. Los entregados vuelven a 0.
pub const IRQ_PALABRAS: usize = 2 + IRQ_LRS;
/// En la palabra 1: un HLT sin nada que entregar duerme el hilo en el kernel (hasta el plazo del temporizador, una
/// patada o `TOPE_WFI_US`).
pub const ESPERAR_WFI: u64 = 1 << 17;
/// En la palabra 1, de vuelta: se entregó la interrupción del temporizador (el servicio la apunta como en servicio en su
/// APIC, para el EOI).
pub const TEMPORIZADOR_ENTREGADO: u64 = 1 << 18;
/// En un hueco: hay que entregar su vector.
pub const PENDIENTE: u64 = 1 << 8;
pub const TOPE_WFI_US: u64 = 100_000;

pub enum Error {
    NoHay,
    Ocupada,
    Muerta,
}

const NINGUNO: u32 = u32::MAX;

struct Patada {
    salir: AtomicBool,
    nucleo: AtomicU32,
    cola: Cerrojo<ColaEspera>,
}

impl Patada {
    fn dar(&self) {
        // Como en ARM: primero la petición y luego mirar el núcleo; la vCPU, al revés.
        self.salir.store(true, Ordering::SeqCst);
        let n = self.nucleo.load(Ordering::SeqCst);
        if n != NINGUNO && n != crate::nucleos::numero() {
            crate::nucleos::avisar_vcpu(n as usize);
        }
        self.cola.tomar().despertar_todos();
    }
}

struct Comun {
    espacio: Arc<Espacio>,
    muerta: AtomicBool,
    vcpus: Cerrojo<Vec<Arc<Patada>>>,
    numeros: AtomicU32,
}

impl Comun {
    fn parar(&self) {
        self.muerta.store(true, Ordering::SeqCst);
        for p in self.vcpus.tomar().iter() {
            p.dar();
        }
    }
}

/// Una lectura a medias: adónde va lo que conteste el servicio.
#[derive(Clone, Copy)]
enum Lectura {
    /// Al registro `r`: `bytes` leídos, extendidos (con signo o sin él) a `destino` bytes; `alto` es AH, CH, DH o BH.
    Reg { r: u8, bytes: u8, destino: u8, signo: bool, alto: bool },
    /// RDMSR: a edx:eax.
    Msr,
}

/// La última salida a un aparato, para `aborto`.
#[derive(Clone, Copy)]
enum Acceso {
    Aparato,
    /// La MSR que no existe: #GP en la instrucción, que vuelve a estar en `pc`.
    Msr { pc: u64 },
}

struct Estado {
    vcpu: Vcpu,
    lectura: Option<Lectura>,
    acceso: Option<Acceso>,
}

pub struct Maquina {
    comun: Arc<Comun>,
    patada: Arc<Patada>,
    numero: u8,
    estado: Cerrojo<Estado>,
}

enum Despertar {
    Vencido,
    Pateada,
}

/// De dónde sale la interrupción que se entrega.
#[derive(Clone, Copy)]
enum Fuente {
    Hueco(usize),
    Temporizador,
}

impl Maquina {
    /// Una máquina nueva con su vCPU 0, que empieza en `entrada` (modo protegido de 32 bits, plano) con `x0` en rsi, y
    /// `memoria` vista desde `gpa`.
    pub fn nueva(memoria: Arc<Memoria>, gpa: u64, entrada: u64, x0: u64, cupo: &Arc<crate::cupo::Cupo>) -> Result<Self, hiper::Error> {
        let espacio = Espacio::nuevo(cupo)?;
        espacio.anadir(memoria, gpa, cupo)?;
        let comun = Arc::try_new(Comun { espacio, muerta: AtomicBool::new(false), vcpus: Cerrojo::new(Vec::new()), numeros: AtomicU32::new(1) })
            .map_err(|_| hiper::Error::Memoria)?;
        Self::con(comun, 0, entrada, x0, cupo)
    }

    fn con(comun: Arc<Comun>, numero: u8, entrada: u64, x0: u64, cupo: &Arc<crate::cupo::Cupo>) -> Result<Self, hiper::Error> {
        let hecha = (|| {
            let vcpu = Vcpu::nueva(&comun.espacio, numero, entrada, x0, cupo)?;
            let patada = Arc::try_new(Patada { salir: AtomicBool::new(false), nucleo: AtomicU32::new(NINGUNO), cola: Cerrojo::new(ColaEspera::nueva()) })
                .map_err(|_| hiper::Error::Memoria)?;
            let mut l = comun.vcpus.tomar();
            l.try_reserve(1).map_err(|_| hiper::Error::Memoria)?;
            l.push(patada.clone());
            Ok((vcpu, patada))
        })();
        match hecha {
            Ok((vcpu, patada)) => Ok(Self { comun, patada, numero, estado: Cerrojo::new(Estado { vcpu, lectura: None, acceso: None }) }),
            Err(e) => {
                comun.numeros.fetch_and(!(1 << numero), Ordering::AcqRel);
                Err(e)
            }
        }
    }

    /// Otra vCPU de la misma máquina, con el ID de APIC `numero` (menor que `hiper::MAX_VCPUS`, sin otra con él).
    pub fn otra(&self, numero: u64, entrada: u64, x0: u64, cupo: &Arc<crate::cupo::Cupo>) -> Result<Self, hiper::Error> {
        if numero >= hiper::MAX_VCPUS as u64 || self.comun.muerta.load(Ordering::Acquire) {
            return Err(hiper::Error::Argumento);
        }
        let bit = 1u32 << numero;
        if self.comun.numeros.fetch_or(bit, Ordering::AcqRel) & bit != 0 {
            return Err(hiper::Error::Argumento);
        }
        Self::con(self.comun.clone(), numero as u8, entrada, x0, cupo)
    }

    pub fn anadir(&self, memoria: Arc<Memoria>, gpa: u64, cupo: &Arc<crate::cupo::Cupo>) -> Result<(), hiper::Error> {
        self.comun.espacio.anadir(memoria, gpa, cupo)
    }

    /// El globo (llamada 55): aún no en x86.
    pub fn globo(&self, _ipas: &[u64], _soltar: bool, _proceso: &mut crate::espacio::Espacio, _llaves_de: &dyn Fn(&Arc<Memoria>) -> usize) -> Result<usize, hiper::Error> {
        Err(hiper::Error::NoHay)
    }

    /// La GICV es de ARM: en x86 no hay.
    pub fn proyectar_gicv(&self, _ipa: u64, _cupo: &Arc<crate::cupo::Cupo>) -> Result<(), hiper::Error> {
        Err(hiper::Error::NoHay)
    }

    pub fn patear(&self) {
        self.patada.dar();
    }

    pub fn parar(&self) {
        self.comun.parar();
    }

    /// Entra en el invitado. `valor` contesta a una lectura pendiente (de un aparato, un puerto o una MSR). `irq`, el
    /// bloque de interrupciones; con `ESPERAR_WFI`, un HLT sin nada que entregar duerme aquí.
    pub fn correr(&self, valor: u64, mut irq: Option<&mut [u64; IRQ_PALABRAS]>) -> Result<[u64; PALABRAS], Error> {
        let esperar = irq.as_deref().is_some_and(|b| b[1] & ESPERAR_WFI != 0);
        if let Some(b) = irq.as_deref_mut() {
            b[1] &= !TEMPORIZADOR_ENTREGADO;
        }
        let mut valor = valor;
        let mut dormido = 0u64;
        let fr = crate::reloj::frecuencia().max(1);
        let tope = TOPE_WFI_US * fr / 1_000_000;
        loop {
            let mut inf = self.correr_una(valor, irq.as_deref_mut())?;
            valor = 0;
            inf[12] = (dormido as u128 * 1_000_000 / fr as u128) as u64;
            if !esperar || inf[0] != WFI || inf[11] & 1 != 0 {
                return Ok(inf);
            }
            // Un HLT sin nada que entregar: hasta el plazo del temporizador (si está encendido), como mucho el tope.
            let ahora = crate::reloj::ahora();
            let plazo = inf[10];
            if plazo <= ahora || dormido >= tope {
                return Ok(inf);
            }
            let hasta = plazo.min(ahora.saturating_add(tope - dormido));
            let d = self.dormir(hasta, fr);
            dormido += crate::reloj::ahora().saturating_sub(ahora);
            inf[12] = (dormido as u128 * 1_000_000 / fr as u128) as u64;
            match d {
                Despertar::Vencido if hasta == plazo => continue,
                _ => return Ok(inf),
            }
        }
    }

    fn dormir(&self, hasta: u64, fr: u64) -> Despertar {
        let p = &self.patada;
        let mut g = p.cola.tomar();
        if p.salir.swap(false, Ordering::SeqCst) || self.comun.muerta.load(Ordering::SeqCst) || planificador::debe_morir() {
            return Despertar::Pateada;
        }
        if !g.apuntarme() {
            return Despertar::Pateada;
        }
        let us = (hasta.saturating_sub(crate::reloj::ahora()) as u128 * 1_000_000 / fr as u128) as u64;
        planificador::bloquear_soltando_hasta(g, us.max(1));
        p.cola.tomar().quitarme();
        if p.salir.swap(false, Ordering::SeqCst) || self.comun.muerta.load(Ordering::SeqCst) || planificador::debe_morir() {
            return Despertar::Pateada;
        }
        if crate::reloj::ahora() >= hasta { Despertar::Vencido } else { Despertar::Pateada }
    }

    /// Lo que toca entregar ahora: el mayor vector entre los huecos pendientes y el temporizador, si venció.
    fn elegir(huecos: &[u64; IRQ_LRS], temporizador: Option<u8>, plazo: Option<u64>) -> Option<(u8, Fuente)> {
        let mut mejor: Option<(u8, Fuente)> = None;
        for (i, &h) in huecos.iter().enumerate() {
            if h & PENDIENTE != 0 && h as u8 >= 16 && mejor.is_none_or(|(v, _)| h as u8 > v) {
                mejor = Some((h as u8, Fuente::Hueco(i)));
            }
        }
        if let (Some(t), Some(p)) = (temporizador, plazo)
            && t >= 16
            && crate::reloj::ahora() >= p
            && mejor.is_none_or(|(v, _)| t > v)
        {
            mejor = Some((t, Fuente::Temporizador));
        }
        mejor
    }

    fn correr_una(&self, valor: u64, mut irq: Option<&mut [u64; IRQ_PALABRAS]>) -> Result<[u64; PALABRAS], Error> {
        let Some(mut e) = self.estado.intentar() else { return Err(Error::Ocupada) };
        if self.comun.muerta.load(Ordering::Acquire) {
            return Err(Error::Muerta);
        }
        e.acceso = None;
        if let Some(l) = e.lectura.take() {
            responder(&mut e.vcpu, l, valor);
        }
        let mut huecos = [0u64; IRQ_LRS];
        let mut temporizador = None;
        if let Some(b) = irq.as_deref() {
            if b[1] & 1 != 0 {
                temporizador = Some((b[1] >> 8) as u8);
            }
            huecos.copy_from_slice(&b[2..]);
        }
        // Si el plazo del temporizador del invitado vence antes que el tic de este núcleo, el tic se adelanta: así lo
        // saca a tiempo (como el temporizador virtual de ARM, que es de verdad).
        if temporizador.is_some()
            && let Some(p) = e.vcpu.plazo_tsc()
        {
            let ahora = crate::reloj::ahora();
            if p > ahora && p < crate::reloj::proximo() {
                crate::reloj::programar(p - ahora);
            }
        }
        let antes = crate::reloj::ahora();
        let yo = crate::nucleos::numero();
        let mut entregado_t = false;
        let v = loop {
            self.patada.nucleo.store(yo, Ordering::SeqCst);
            if self.patada.salir.swap(false, Ordering::SeqCst) || self.comun.muerta.load(Ordering::SeqCst) {
                self.patada.nucleo.store(NINGUNO, Ordering::SeqCst);
                break None;
            }
            let elegido = Self::elegir(&huecos, temporizador, e.vcpu.plazo_tsc());
            let v = e.vcpu.correr(elegido.map(|x| x.0));
            self.patada.nucleo.store(NINGUNO, Ordering::SeqCst);
            let Some(v) = v else { return Err(Error::NoHay) };
            if v.entregada {
                match elegido.map(|x| x.1) {
                    Some(Fuente::Hueco(i)) => huecos[i] = 0,
                    Some(Fuente::Temporizador) => {
                        e.vcpu.plazo_cumplido();
                        entregado_t = true;
                    }
                    None => {}
                }
            }
            // Un HLT con algo que entregar y las interrupciones abiertas: no espera, se le da.
            if let Salida::Hlt = v.salida
                && e.vcpu.acepta_interrupciones()
                && Self::elegir(&huecos, temporizador, e.vcpu.plazo_tsc()).is_some()
            {
                continue;
            }
            break Some(v);
        };
        self.patada.salir.store(false, Ordering::SeqCst);
        if self.comun.muerta.load(Ordering::Acquire) {
            return Err(Error::Muerta);
        }
        let cuentas = crate::reloj::ahora().wrapping_sub(antes);
        if let Some(b) = irq.as_deref_mut() {
            b[0] = IRQ_LRS as u64;
            if entregado_t {
                b[1] |= TEMPORIZADOR_ENTREGADO;
            }
            b[2..].copy_from_slice(&huecos);
        }
        let mut inf = [0u64; PALABRAS];
        inf[8] = cuentas;
        let plazo = e.vcpu.plazo_tsc().filter(|_| temporizador.is_some());
        inf[10] = plazo.unwrap_or(u64::MAX);
        inf[11] = Self::elegir(&huecos, temporizador, e.vcpu.plazo_tsc()).is_some() as u64;
        inf[1] = e.vcpu.rip();
        let Some(v) = v else {
            inf[0] = INTERRUMPIDO;
            return Ok(inf);
        };
        inf[1] = v.pc;
        inf[5] = v.codigo;
        match v.salida {
            Salida::Interrumpida => inf[0] = INTERRUMPIDO,
            Salida::Hlt => inf[0] = WFI,
            Salida::Hipercall => {
                inf[0] = HVC;
                inf[3] = e.vcpu.reg(0);
                inf[6] = e.vcpu.reg(3);
                inf[7] = e.vcpu.reg(1);
                inf[9] = e.vcpu.reg(2);
            }
            Salida::Puerto { puerto, bytes, entrada, cadena } => {
                if cadena || !matches!(bytes, 1 | 2 | 4) {
                    inf[0] = FALLO;
                    inf[2] = puerto as u64;
                    drop(e);
                    self.comun.parar();
                    return Ok(inf);
                }
                inf[0] = PUERTO;
                inf[2] = puerto as u64;
                inf[4] = bytes as u64;
                inf[6] = entrada as u64;
                if entrada {
                    e.lectura = Some(Lectura::Reg { r: 0, bytes, destino: bytes, signo: false, alto: false });
                } else {
                    inf[3] = e.vcpu.reg(0) & mascara(bytes);
                }
                e.acceso = Some(Acceso::Aparato);
            }
            Salida::Memoria { gpa, escritura } => {
                let (bytes, n) = leer_instruccion(&e.vcpu);
                let (largo64, def32, _) = e.vcpu.modo_cs();
                match decodificar(&bytes[..n], largo64, def32) {
                    Some(d) if d.escritura == escritura => {
                        inf[2] = gpa;
                        inf[4] = d.bytes as u64;
                        if d.escritura {
                            inf[0] = ESCRITURA;
                            inf[3] = match d.origen {
                                Origen::Inmediato(v) => v,
                                Origen::Reg { r, alto } => leer_reg(&e.vcpu, r, alto),
                            } & mascara(d.bytes);
                        } else {
                            inf[0] = LECTURA;
                            e.lectura = Some(d.destino);
                        }
                        e.acceso = Some(Acceso::Aparato);
                        e.vcpu.saltar(d.largo as u64);
                    }
                    _ => {
                        inf[0] = FALLO;
                        inf[2] = gpa;
                        inf[3] = u64::from_le_bytes(bytes[..8].try_into().unwrap_or([0; 8]));
                        drop(e);
                        self.comun.parar();
                    }
                }
            }
            Salida::Msr { msr, escritura, valor } => {
                inf[0] = REGISTRO;
                inf[2] = msr as u64;
                inf[4] = 8;
                if escritura {
                    inf[3] = valor;
                } else {
                    inf[6] = 1;
                    e.lectura = Some(Lectura::Msr);
                }
                e.acceso = Some(Acceso::Msr { pc: v.pc });
            }
            Salida::Fallo { codigo, info1, info2 } => {
                inf[0] = FALLO;
                inf[2] = info2;
                inf[3] = info1;
                inf[5] = codigo;
                drop(e);
                self.comun.parar();
            }
        }
        Ok(inf)
    }

    /// Los registros de la vCPU parada (`hiper::REGISTROS`).
    pub fn registros(&self) -> Result<[u64; hiper::REGISTROS], Error> {
        let e = self.estado.intentar().ok_or(Error::Ocupada)?;
        Ok(e.vcpu.registros())
    }

    /// Cambia los generales, rip y rflags de la vCPU parada. Una lectura pendiente se olvida.
    pub fn poner_registros(&self, r: &[u64; hiper::REGISTROS]) -> Result<(), Error> {
        let mut e = self.estado.intentar().ok_or(Error::Ocupada)?;
        if self.comun.muerta.load(Ordering::Acquire) {
            return Err(Error::Muerta);
        }
        e.lectura = None;
        e.vcpu.poner_registros(r);
        Ok(())
    }

    /// La vCPU parada, como recién encendida en `entrada` con `x0` en rsi, en el modo `modo` (`hiper::MODO_PROTEGIDO`
    /// o `hiper::MODO_REAL`: lo que hace una SIPI).
    pub fn reiniciar(&self, entrada: u64, x0: u64, modo: u64) -> Result<(), Error> {
        let mut e = self.estado.intentar().ok_or(Error::Ocupada)?;
        if self.comun.muerta.load(Ordering::Acquire) {
            return Err(Error::Muerta);
        }
        e.lectura = None;
        e.acceso = None;
        if e.vcpu.reiniciar(entrada, x0, modo) { Ok(()) } else { Err(Error::NoHay) }
    }

    /// En lugar del último acceso a un aparato, "no hay nada": una lectura de un aparato o un puerto da todo unos y una
    /// escritura se pierde (como en un PC); una MSR que no existe da #GP. Falso si la última salida no fue de esas.
    pub fn aborto(&self) -> Result<bool, Error> {
        let mut e = self.estado.intentar().ok_or(Error::Ocupada)?;
        if self.comun.muerta.load(Ordering::Acquire) {
            return Err(Error::Muerta);
        }
        match e.acceso.take() {
            Some(Acceso::Aparato) => {
                if let Some(l) = e.lectura.take() {
                    responder(&mut e.vcpu, l, u64::MAX);
                }
                Ok(true)
            }
            Some(Acceso::Msr { pc }) => {
                e.lectura = None;
                e.vcpu.poner_rip(pc);
                e.vcpu.excepcion(13, Some(0));
                Ok(true)
            }
            None => Ok(false),
        }
    }
}

impl Drop for Maquina {
    fn drop(&mut self) {
        let p = Arc::as_ptr(&self.patada);
        self.comun.vcpus.tomar().retain(|q| Arc::as_ptr(q) != p);
        self.comun.numeros.fetch_and(!(1 << self.numero), Ordering::AcqRel);
    }
}

fn mascara(bytes: u8) -> u64 {
    if bytes >= 8 { u64::MAX } else { (1u64 << (8 * bytes)) - 1 }
}

/// Un registro de origen: `alto` es AH, CH, DH o BH (los bits 15:8 del registro `r`).
fn leer_reg(v: &Vcpu, r: u8, alto: bool) -> u64 {
    if alto { v.reg(r as usize) >> 8 } else { v.reg(r as usize) }
}

/// Escribe en un registro como lo haría la CPU: con 4 bytes, el resto a cero; con 1 o 2, el resto como estaba.
fn escribir_reg(v: &mut Vcpu, r: u8, destino: u8, valor: u64, alto: bool) {
    let r = r as usize;
    let viejo = v.reg(r);
    let nuevo = match destino {
        8 => valor,
        4 => valor & 0xffff_ffff,
        2 => (viejo & !0xffff) | (valor & 0xffff),
        _ if alto => (viejo & !0xff00) | ((valor & 0xff) << 8),
        _ => (viejo & !0xff) | (valor & 0xff),
    };
    v.poner_reg(r, nuevo);
}

/// Lo que contesta el servicio a una lectura, en su registro.
fn responder(v: &mut Vcpu, l: Lectura, valor: u64) {
    match l {
        Lectura::Reg { r, bytes, destino, signo, alto } => {
            let bits = 8 * bytes.min(8) as u32;
            let mut x = valor & mascara(bytes);
            if signo && bits < 64 && x & (1 << (bits - 1)) != 0 {
                x |= u64::MAX << bits;
            }
            escribir_reg(v, r, destino, x, alto);
        }
        Lectura::Msr => {
            v.poner_reg(0, valor & 0xffff_ffff);
            v.poner_reg(2, valor >> 32);
        }
    }
}

// --- La instrucción que tocó un aparato ---

/// Hasta 15 bytes desde el RIP del invitado: los que dio la CPU (AMD con su ayuda) o leídos de su memoria, recorriendo
/// sus tablas de páginas. Devuelve los bytes y cuántos se pudieron leer.
fn leer_instruccion(v: &Vcpu) -> ([u8; 16], usize) {
    let mut b = [0u8; 16];
    let dados = v.instruccion();
    if !dados.is_empty() {
        b[..dados.len()].copy_from_slice(dados);
        return (b, dados.len());
    }
    let (largo, _, base) = v.modo_cs();
    let lineal = if largo { v.rip() } else { base.wrapping_add(v.rip()) & 0xffff_ffff };
    let mut n = 0;
    while n < 15 {
        let va = lineal.wrapping_add(n as u64);
        let Some(gpa) = traducir(v, va) else { break };
        // Hasta el final de la página, de una vez.
        let k = (4096 - (va & 0xfff) as usize).min(15 - n);
        if !v.leer_fisica(gpa, &mut b[n..n + k]) {
            break;
        }
        n += k;
    }
    (b, n)
}

/// Dirección lineal del invitado a física, con sus tablas: sin paginación, de 32 bits (con PSE), PAE o de 4 niveles.
fn traducir(v: &Vcpu, va: u64) -> Option<u64> {
    let (cr0, cr3, cr4, efer) = v.control();
    if cr0 & (1 << 31) == 0 {
        return Some(va & 0xffff_ffff);
    }
    let leer64 = |gpa: u64| -> Option<u64> {
        let mut b = [0u8; 8];
        v.leer_fisica(gpa, &mut b).then(|| u64::from_le_bytes(b))
    };
    let leer32 = |gpa: u64| -> Option<u64> {
        let mut b = [0u8; 4];
        v.leer_fisica(gpa, &mut b).then(|| u32::from_le_bytes(b) as u64)
    };
    const DIR: u64 = 0x000f_ffff_ffff_f000;
    if efer & (1 << 10) != 0 {
        // Modo largo, 4 niveles (5 niveles no se da: LA57 está quitado del CPUID).
        let mut tabla = cr3 & DIR;
        for nivel in (0..4).rev() {
            let e = leer64(tabla + 8 * ((va >> (12 + 9 * nivel)) & 511))?;
            if e & 1 == 0 {
                return None;
            }
            if nivel == 0 || (nivel <= 2 && e & 0x80 != 0) {
                let tam = 1u64 << (12 + 9 * nivel);
                return Some((e & DIR & !(tam - 1)) | (va & (tam - 1)));
            }
            tabla = e & DIR;
        }
        None
    } else if cr4 & (1 << 5) != 0 {
        // PAE de 32 bits: 4 PDPTE, directorios y tablas de entradas de 8 bytes.
        let pdpte = leer64((cr3 & 0xffff_ffe0) + 8 * ((va >> 30) & 3))?;
        if pdpte & 1 == 0 {
            return None;
        }
        let pde = leer64((pdpte & DIR) + 8 * ((va >> 21) & 511))?;
        if pde & 1 == 0 {
            return None;
        }
        if pde & 0x80 != 0 {
            return Some((pde & DIR & !0x1f_ffff) | (va & 0x1f_ffff));
        }
        let pte = leer64((pde & DIR) + 8 * ((va >> 12) & 511))?;
        (pte & 1 != 0).then(|| (pte & DIR) | (va & 0xfff))
    } else {
        // 32 bits: directorio y tablas de entradas de 4 bytes; con PSE, páginas de 4 MB.
        let pde = leer32((cr3 & 0xffff_f000) + 4 * ((va >> 22) & 1023))?;
        if pde & 1 == 0 {
            return None;
        }
        if pde & 0x80 != 0 && cr4 & (1 << 4) != 0 {
            return Some((pde & 0xffc0_0000) | (va & 0x3f_ffff));
        }
        let pte = leer32((pde & 0xffff_f000) + 4 * ((va >> 12) & 1023))?;
        (pte & 1 != 0).then(|| (pte & 0xffff_f000) | (va & 0xfff))
    }
}

#[derive(Clone, Copy)]
enum Origen {
    Reg { r: u8, alto: bool },
    Inmediato(u64),
}

struct Decodificada {
    largo: usize,
    escritura: bool,
    bytes: u8,
    origen: Origen,
    destino: Lectura,
}

/// Decodifica un acceso a memoria de los que usan los controladores con sus aparatos (Intel SDM vol. 2, tablas de
/// opcodes y 2.1 a 2.2: prefijos, REX, ModRM y SIB): MOV r/m, r (88, 89), MOV r, r/m (8A, 8B), MOV r/m, imm (C6, C7),
/// MOV con desplazamiento absoluto (A0 a A3), MOVZX (0F B6, 0F B7) y MOVSX (0F BE, 0F BF). `largo64` es CS de 64 bits;
/// `def32`, CS de 32 bits por defecto (fuera del modo largo).
fn decodificar(b: &[u8], largo64: bool, def32: bool) -> Option<Decodificada> {
    let mut i = 0;
    let mut op16 = false;
    let mut dir_cambio = false;
    // Prefijos: tamaño de operando, de dirección, LOCK, REP y segmentos (la dirección física ya la dio la CPU).
    while i < b.len() && i < 14 {
        match b[i] {
            0x66 => op16 = true,
            0x67 => dir_cambio = true,
            0xf0 | 0xf2 | 0xf3 | 0x2e | 0x36 | 0x3e | 0x26 | 0x64 | 0x65 => {}
            _ => break,
        }
        i += 1;
    }
    let mut rex = 0u8;
    if largo64 && i < b.len() && b[i] & 0xf0 == 0x40 {
        rex = b[i];
        i += 1;
    }
    let w = rex & 8 != 0;
    let dir_bytes: usize = if largo64 {
        if dir_cambio { 4 } else { 8 }
    } else if def32 != dir_cambio {
        4
    } else {
        2
    };
    // Tamaño de operando: 8 con REX.W; 2 o 4 según el modo y el prefijo 0x66.
    let opsize: u8 = if w {
        8
    } else if (largo64 || def32) != op16 {
        4
    } else {
        2
    };
    let op = *b.get(i)?;
    i += 1;
    // Registro de 8 bits: sin REX, del 4 al 7 son AH, CH, DH y BH.
    let reg8 = |r: u8| if rex == 0 && (4..8).contains(&r) { (r - 4, true) } else { (r, false) };
    let (dos, op) = if op == 0x0f {
        let o = *b.get(i)?;
        i += 1;
        (true, o)
    } else {
        (false, op)
    };
    // MOV con desplazamiento absoluto (moffs): sin ModRM.
    if !dos && (0xa0..=0xa3).contains(&op) {
        let largo = i + dir_bytes;
        if largo > b.len() {
            return None;
        }
        let bytes = if op & 1 == 0 { 1 } else { opsize };
        let escritura = op >= 0xa2;
        return Some(Decodificada {
            largo,
            escritura,
            bytes,
            origen: Origen::Reg { r: 0, alto: false },
            destino: Lectura::Reg { r: 0, bytes, destino: bytes, signo: false, alto: false },
        });
    }
    let modrm = *b.get(i)?;
    i += 1;
    let modo = modrm >> 6;
    let reg = ((modrm >> 3) & 7) | if rex & 4 != 0 { 8 } else { 0 };
    let rm = modrm & 7;
    if modo == 3 {
        return None;
    }
    // Lo que ocupa el operando de memoria: SIB y desplazamiento.
    if dir_bytes == 2 {
        i += match (modo, rm) {
            (0, 6) => 2,
            (1, _) => 1,
            (2, _) => 2,
            _ => 0,
        };
    } else {
        let mut base5 = false;
        if rm == 4 {
            let sib = *b.get(i)?;
            i += 1;
            base5 = sib & 7 == 5;
        }
        i += match modo {
            0 if rm == 5 || base5 => 4,
            1 => 1,
            2 => 4,
            _ => 0,
        };
    }
    let mut imm = |n: usize| -> Option<u64> {
        let s = b.get(i..i + n)?;
        i += n;
        let mut x = [0u8; 8];
        x[..n].copy_from_slice(s);
        Some(u64::from_le_bytes(x))
    };
    let d = match (dos, op) {
        (false, 0x88) | (false, 0x89) => {
            let bytes = if op == 0x88 { 1 } else { opsize };
            let (r, alto) = if bytes == 1 { reg8(reg) } else { (reg, false) };
            Decodificada { largo: 0, escritura: true, bytes, origen: Origen::Reg { r, alto }, destino: Lectura::Msr }
        }
        (false, 0x8a) | (false, 0x8b) => {
            let bytes = if op == 0x8a { 1 } else { opsize };
            let (r, alto) = if bytes == 1 { reg8(reg) } else { (reg, false) };
            Decodificada { largo: 0, escritura: false, bytes, origen: Origen::Inmediato(0), destino: Lectura::Reg { r, bytes, destino: bytes, signo: false, alto } }
        }
        (false, 0xc6) if reg & 7 == 0 => {
            let v = imm(1)?;
            Decodificada { largo: 0, escritura: true, bytes: 1, origen: Origen::Inmediato(v), destino: Lectura::Msr }
        }
        (false, 0xc7) if reg & 7 == 0 => {
            let v = if opsize == 2 { imm(2)? } else { imm(4)? as u32 as i32 as i64 as u64 };
            Decodificada { largo: 0, escritura: true, bytes: opsize, origen: Origen::Inmediato(v), destino: Lectura::Msr }
        }
        (true, 0xb6) | (true, 0xb7) | (true, 0xbe) | (true, 0xbf) => {
            let bytes = if op & 1 == 0 { 1 } else { 2 };
            Decodificada { largo: 0, escritura: false, bytes, origen: Origen::Inmediato(0), destino: Lectura::Reg { r: reg, bytes, destino: opsize, signo: op >= 0xbe, alto: false } }
        }
        _ => return None,
    };
    if i > b.len() {
        return None;
    }
    Some(Decodificada { largo: i, ..d })
}
