//! Las máquinas virtuales de la caja Linux (H9, `docs/bitacoras/caja-linux.md`), vistas como objetos con llave.
//!
//! Crear una pide el permiso de máquinas virtuales (lo reparte `sesiones`) y un objeto de memoria del cupo de quien la
//! pide, que es la RAM del invitado. Cada llave es una vCPU: la de `maquina_crear` es la 0, y con ella se piden las
//! demás (hasta 8, cada una con su número en el MPIDR), que comparten la memoria y la etapa 2 de la máquina. Cada vCPU
//! se corre desde un hilo del servicio, y varias a la vez en núcleos distintos.
//!
//! "Correr" entra en el invitado hasta que sale y dice por qué, ya interpretado (`Informe`): una llamada al hipervisor
//! (HVC), un acceso a una dirección que no es su memoria (los aparatos que emula el servicio, o un fallo, según el
//! servicio), un WFI, una interrupción del anfitrión (el reloj saca al invitado cada tic: nunca se queda con el núcleo;
//! o la patada de otro hilo) o un fallo. El kernel no emula nada: eso es del servicio.
//!
//! "Patear" una vCPU la saca de su invitado en cuanto puede: si corre en otro núcleo, con una interrupción por software
//! a ese núcleo (`gic::SGI_VCPU`); si duerme en un WFI, despertándola; si no, la siguiente vez que vaya a entrar vuelve
//! sin entrar. Es lo que hace el servicio cuando deja algo para ella (una SGI de otra vCPU, una interrupción de un
//! aparato). "Parar" la máquina la mata entera: todas sus vCPU salen y ya no vuelven a entrar.
//!
//! Lo que toca el hardware (EL2, la etapa 2, el cambio de mundo) está en `base::hiper`.

use crate::base::hiper::{self, Espacio, Vcpu};
use crate::cerrojo::Cerrojo;
use crate::memoria::Memoria;
use crate::planificador::{self, ColaEspera};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Por qué volvió el invitado (el primer número del informe; lo devuelve también la llamada).
pub const HVC: u64 = 1;
/// Leyó en una dirección que no es su memoria: el valor que dé el servicio en la siguiente vuelta va a su registro.
pub const LECTURA: u64 = 2;
/// Escribió en una dirección que no es su memoria.
pub const ESCRITURA: u64 = 3;
/// Un WFI (o WFE). Con `ESPERAR_WFI`, el kernel ya esperó lo que tocaba: hasta el plazo de su temporizador, como mucho
/// `TOPE_WFI_US`, o hasta que alguien la pateó.
pub const WFI: u64 = 4;
/// Una interrupción del anfitrión lo sacó (el reloj, un aparato, la patada de otro hilo): no ha pasado nada suyo.
pub const INTERRUMPIDO: u64 = 5;
/// Una trampa o un fallo que el kernel no sabe pasar al servicio de otra forma (SMC, la coma flotante, un fallo de la
/// etapa 2 sin datos, un SError): la máquina queda muerta, con todas sus vCPU.
pub const FALLO: u64 = 6;
/// Tocó un registro del sistema atrapado (depuración, contadores de rendimiento, el temporizador físico...): en la
/// dirección, su codificación (op0, op1, CRn, CRm y op2, como en el ESR, sin el registro de destino ni el sentido);
/// en x1, 1 si lee (el valor de la siguiente vuelta va a su registro) y 0 si escribe (el valor, en el informe).
pub const REGISTRO: u64 = 7;

/// Palabras del informe: motivo, PC de la instrucción, dirección (física del invitado), valor, tamaño en bytes (o el
/// número del HVC), ESR, x1 y x2 (en un HVC), las cuentas del contador que pasó la CPU fuera del kernel (ida y vuelta
/// por EL2 con el invitado), x3 (en un HVC), el plazo del temporizador virtual del invitado (CNTV_CVAL, si está
/// encendido y sin enmascarar; si no, `u64::MAX`), señales (bit 0, hay una interrupción pendiente en sus registros de
/// lista: un WFI no tiene que esperar) y los microsegundos que durmió en el kernel en sus WFI.
pub const PALABRAS: usize = 13;

/// Registros de lista del servicio en el bloque de interrupciones.
pub const IRQ_LRS: usize = 8;
/// El bloque de interrupciones que va y viene con `maquina_correr` (palabras de 64 bits): [0] cuántos registros de
/// lista puede llenar el servicio (lo escribe el kernel; 0 sin GIC virtual), [1] lo escribe el servicio: cómo tiene el
/// invitado el temporizador virtual en su distribuidor (bit 0 habilitado, [15:8] prioridad, bit 16 grupo 1) y, en el
/// bit 17 (`ESPERAR_WFI`), que los WFI esperen en el kernel; y desde [2] los registros de lista (GICH_LRn del GICv2,
/// sin el bit HW): los pone el servicio y vuelven como los dejó el invitado.
pub const IRQ_PALABRAS: usize = 2 + IRQ_LRS;
/// En la palabra 1 del bloque: un WFI sin nada pendiente duerme el hilo en el kernel (hasta el plazo del temporizador
/// del invitado, una patada o `TOPE_WFI_US`) en vez de volver al servicio. Si venció el temporizador, vuelve a entrar
/// sin pasar por el servicio.
pub const ESPERAR_WFI: u64 = 1 << 17;
/// Lo más que espera un WFI en el kernel antes de volver al servicio (que así mira su plazo y lo demás).
pub const TOPE_WFI_US: u64 = 100_000;

pub enum Error {
    NoHay,
    /// Otro hilo la está corriendo.
    Ocupada,
    /// Murió en un fallo o la pararon.
    Muerta,
}

/// Ningún núcleo: la vCPU no está dentro de su invitado.
const NINGUNO: u32 = u32::MAX;

/// Lo que hace falta para sacar a una vCPU de su invitado desde otro hilo, sin tocar su estado.
struct Patada {
    /// Que salga en cuanto pueda (o que no entre la próxima vez).
    salir: AtomicBool,
    /// El núcleo donde está dentro del invitado, o `NINGUNO`.
    nucleo: AtomicU32,
    /// Su hilo, si duerme en un WFI.
    cola: Cerrojo<ColaEspera>,
}

impl Patada {
    fn dar(&self) {
        // Primero la petición y luego mirar el núcleo; la vCPU, al revés (`Maquina::correr_una`): con orden total, o
        // ella ve la petición y no entra, o aquí se ve su núcleo y se le manda el aviso.
        self.salir.store(true, Ordering::SeqCst);
        let n = self.nucleo.load(Ordering::SeqCst);
        if n != NINGUNO && n != crate::nucleos::numero() {
            let sgi = if hiper::hay_gic_virtual() { crate::gic::SGI_VCPU } else { crate::gic::SGI_PLANIFICAR };
            crate::gic::avisar(n, sgi);
        }
        self.cola.tomar().despertar_todos();
    }
}

/// Lo que comparten las vCPU de una máquina.
struct Comun {
    espacio: Arc<Espacio>,
    /// Murió en un fallo o la pararon: ninguna vCPU vuelve a entrar.
    muerta: AtomicBool,
    /// Las vCPU vivas, para sacarlas a todas.
    vcpus: Cerrojo<Vec<Arc<Patada>>>,
    /// Los números de vCPU en uso (un bit cada uno).
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

struct Estado {
    vcpu: Vcpu,
    /// Una lectura de aparato a medias: el registro (31 = xzr), el tamaño en bytes, si se extiende el signo (SSE) y si
    /// el registro es de 64 bits (SF).
    lectura: Option<(usize, u32, bool, bool)>,
    /// El último acceso a un aparato, para inyectar un aborto en su lugar: PC de la instrucción, dirección virtual y si
    /// escribía. Solo hasta la siguiente vuelta.
    acceso: Option<(u64, u64, bool)>,
}

/// Una vCPU de una máquina virtual (lo que hay detrás de una llave de máquina).
pub struct Maquina {
    comun: Arc<Comun>,
    patada: Arc<Patada>,
    numero: u8,
    estado: Cerrojo<Estado>,
}

/// Cómo acabó la espera de un WFI.
enum Despertar {
    /// Llegó al momento pedido.
    Vencido,
    /// Una patada, la máquina parada o el hilo muriendo: hay que volver al servicio.
    Pateada,
}

impl Maquina {
    /// Una máquina nueva con su vCPU 0, que empieza en `entrada` con `x0`, y `memoria` vista desde `ipa`.
    pub fn nueva(memoria: Arc<Memoria>, ipa: u64, entrada: u64, x0: u64, cupo: &Arc<crate::cupo::Cupo>) -> Result<Self, hiper::Error> {
        let espacio = Espacio::nuevo(cupo)?;
        espacio.anadir(memoria, ipa, cupo)?;
        let comun = Arc::try_new(Comun { espacio, muerta: AtomicBool::new(false), vcpus: Cerrojo::new(Vec::new()), numeros: AtomicU32::new(1) })
            .map_err(|_| hiper::Error::Memoria)?;
        Self::con(comun, 0, entrada, x0, cupo)
    }

    /// La vCPU `numero` de `comun`, con el número ya apartado; si falla, lo devuelve.
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

    /// Otra vCPU de la misma máquina, con el número `numero` (su Aff0 en el MPIDR, menor que `hiper::MAX_VCPUS` y que no
    /// tenga ya otra), que empieza en `entrada` con `x0`. Comparte la memoria, la etapa 2 y el VMID.
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

    /// Más memoria para el invitado, desde `ipa`. Puede ser con sus vCPU en marcha.
    pub fn anadir(&self, memoria: Arc<Memoria>, ipa: u64, cupo: &Arc<crate::cupo::Cupo>) -> Result<(), hiper::Error> {
        self.comun.espacio.anadir(memoria, ipa, cupo)
    }

    /// Globo (llamada 55): suelta o repone las páginas del invitado en `ipas` (ver `hiper::Espacio::globo`). `proceso`
    /// es el espacio de quien llama y `llaves_de` cuenta las llaves de su tabla que tienen un objeto de memoria.
    pub fn globo(&self, ipas: &[u64], soltar: bool, proceso: &mut crate::espacio::Espacio, llaves_de: &dyn Fn(&Arc<Memoria>) -> usize) -> Result<usize, hiper::Error> {
        self.comun.espacio.globo(ipas, soltar, proceso, llaves_de)
    }

    /// Proyecta la GICV del GIC virtual donde el invitado espera la interfaz de CPU de su GIC (para todas sus vCPU).
    pub fn proyectar_gicv(&self, ipa: u64, cupo: &Arc<crate::cupo::Cupo>) -> Result<(), hiper::Error> {
        self.comun.espacio.proyectar_gicv(ipa, cupo)
    }

    /// Saca a esta vCPU de su invitado en cuanto pueda (ver arriba).
    pub fn patear(&self) {
        self.patada.dar();
    }

    /// Mata la máquina entera: todas sus vCPU salen y no vuelven a entrar (`Error::Muerta`).
    pub fn parar(&self) {
        self.comun.parar();
    }

    /// Entra en el invitado. `valor` es la respuesta a una lectura de aparato pendiente (si no la hay, no se usa).
    /// `irq`, el bloque de interrupciones del servicio (`IRQ_PALABRAS`), si lo da. Con `ESPERAR_WFI` en él, un WFI sin
    /// nada pendiente duerme aquí (ver `ESPERAR_WFI`).
    pub fn correr(&self, valor: u64, mut irq: Option<&mut [u64; IRQ_PALABRAS]>) -> Result<[u64; PALABRAS], Error> {
        let esperar = irq.as_deref().is_some_and(|b| b[1] & ESPERAR_WFI != 0);
        let temporizador = irq.as_deref().is_some_and(|b| b[1] & 1 != 0);
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
            // Un WFI sin nada en sus registros de lista: hasta el plazo de su temporizador (si se lo entregan), como mucho
            // el tope. Si ya venció, vuelve al servicio: puede que el invitado no acepte la interrupción ahora.
            let ahora = crate::reloj::ahora();
            let plazo = if temporizador { inf[10] } else { u64::MAX };
            if plazo <= ahora || dormido >= tope {
                return Ok(inf);
            }
            let hasta = plazo.min(ahora.saturating_add(tope - dormido));
            let d = self.dormir(hasta, fr);
            dormido += crate::reloj::ahora().saturating_sub(ahora);
            inf[12] = (dormido as u128 * 1_000_000 / fr as u128) as u64;
            match d {
                // Venció su temporizador: otra vuelta, que lo lleva en su registro de lista.
                Despertar::Vencido if hasta == plazo => continue,
                _ => return Ok(inf),
            }
        }
    }

    /// Duerme el hilo hasta `hasta` (cuentas del contador) o una patada. Sin cerrojos tomados (el de la vCPU ya se
    /// soltó: mientras tanto se pueden leer sus registros).
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

    /// Una vuelta: entra en el invitado y vuelve con lo que pasó.
    fn correr_una(&self, valor: u64, mut irq: Option<&mut [u64; IRQ_PALABRAS]>) -> Result<[u64; PALABRAS], Error> {
        // Sin esperar: dos hilos no corren a la vez la misma vCPU.
        let Some(mut e) = self.estado.intentar() else { return Err(Error::Ocupada) };
        if self.comun.muerta.load(Ordering::Acquire) {
            return Err(Error::Muerta);
        }
        e.acceso = None;
        if let Some((r, bytes, signo, x64)) = e.lectura.take() {
            let bits = 8 * bytes.min(8);
            let mut v = if bits >= 64 { valor } else { valor & ((1u64 << bits) - 1) };
            if signo && bits < 64 && v & (1 << (bits - 1)) != 0 {
                v |= u64::MAX << bits;
            }
            if !x64 {
                v &= 0xffff_ffff;
            }
            e.vcpu.poner_x(r, v);
        }
        let lrs = hiper::lrs_servicio().min(IRQ_LRS);
        if let Some(b) = irq.as_deref() {
            let t = b[1];
            e.vcpu.poner_temporizador((t & 1 != 0).then_some(((t >> 8) as u8, t & 1 << 16 != 0)));
            for i in 0..lrs {
                e.vcpu.poner_lr(i, b[2 + i] as u32);
            }
        }
        let antes = crate::reloj::ahora();
        // Las interrupciones del kernel siguen enmascaradas (es una llamada al sistema): el hilo no cambia de núcleo y
        // una patada que llegue desde ahora queda pendiente y lo saca nada más entrar.
        let yo = crate::nucleos::numero();
        let s = loop {
            self.patada.nucleo.store(yo, Ordering::SeqCst);
            if self.patada.salir.swap(false, Ordering::SeqCst) || self.comun.muerta.load(Ordering::SeqCst) {
                self.patada.nucleo.store(NINGUNO, Ordering::SeqCst);
                break None;
            }
            let s = e.vcpu.correr();
            self.patada.nucleo.store(NINGUNO, Ordering::SeqCst);
            let Some(s) = s else { return Err(Error::NoHay) };
            // La primera vez que usa la FPU: desde ahora tiene la suya, y se repite la instrucción.
            if s.tipo == hiper::SALIDA_SINCRONA && s.esr >> 26 == 0x07 && !e.vcpu.tiene_fp() && e.vcpu.usar_fp() {
                continue;
            }
            break Some(s);
        };
        // Sale por lo que sea: una patada de antes ya está atendida (el servicio mira todo antes de volver a entrar).
        self.patada.salir.store(false, Ordering::SeqCst);
        if self.comun.muerta.load(Ordering::Acquire) {
            return Err(Error::Muerta);
        }
        let cuentas = crate::reloj::ahora().wrapping_sub(antes);
        if let Some(b) = irq.as_deref_mut() {
            b[0] = lrs as u64;
            for i in 0..IRQ_LRS {
                b[2 + i] = if i < lrs { e.vcpu.lr(i) as u64 } else { 0 };
            }
        }
        let mut inf = [0u64; PALABRAS];
        inf[8] = cuentas;
        inf[10] = e.vcpu.plazo_temporizador().unwrap_or(u64::MAX);
        inf[11] = e.vcpu.lr_pendiente() as u64;
        let pc = e.vcpu.pc();
        inf[1] = pc;
        let Some(s) = s else {
            inf[0] = INTERRUMPIDO;
            return Ok(inf);
        };
        inf[5] = s.esr;
        if s.tipo == hiper::SALIDA_IRQ || s.tipo == hiper::SALIDA_FIQ {
            inf[0] = INTERRUMPIDO;
            return Ok(inf);
        }
        if s.tipo != hiper::SALIDA_SINCRONA {
            inf[0] = FALLO;
            drop(e);
            self.comun.parar();
            return Ok(inf);
        }
        let clase = s.esr >> 26;
        let iss = s.esr & 0x1ff_ffff;
        match clase {
            // HVC de 64 bits: el PC ya apunta a la siguiente.
            0x16 => {
                inf[0] = HVC;
                inf[1] = pc.wrapping_sub(4);
                inf[3] = e.vcpu.x(0);
                inf[4] = iss & 0xffff;
                inf[6] = e.vcpu.x(1);
                inf[7] = e.vcpu.x(2);
                inf[9] = e.vcpu.x(3);
            }
            // WFI o WFE.
            0x01 => {
                inf[0] = WFI;
                e.vcpu.poner_pc(pc + 4);
            }
            // Fallo de datos desde el invitado. Con ISV (la instrucción se describe sola), en la etapa 2, sin ser al
            // recorrer sus tablas y siendo de traducción: un acceso a una dirección que no es su memoria.
            0x24 if iss & (1 << 24) != 0 && iss & (1 << 7) == 0 && iss & 0x3c == 0x04 => {
                let ipa = ((s.hpfar >> 4) << 12) | (s.far & 0xfff);
                let bytes = 1u32 << ((iss >> 22) & 3);
                let r = ((iss >> 16) & 31) as usize;
                inf[2] = ipa;
                inf[4] = bytes as u64;
                if iss & (1 << 6) != 0 {
                    let mascara = if bytes >= 8 { u64::MAX } else { (1u64 << (8 * bytes)) - 1 };
                    inf[0] = ESCRITURA;
                    inf[3] = if r == 31 { 0 } else { e.vcpu.x(r) & mascara };
                } else {
                    inf[0] = LECTURA;
                    e.lectura = Some((r, bytes, iss & (1 << 21) != 0, iss & (1 << 15) != 0));
                }
                e.acceso = Some((pc, s.far, iss & (1 << 6) != 0));
                e.vcpu.poner_pc(pc + 4);
            }
            // MSR o MRS de un registro atrapado: lo decide el servicio.
            0x18 => {
                let r = ((iss >> 5) & 31) as usize;
                inf[0] = REGISTRO;
                inf[2] = iss & 0x3f_fc1e;
                inf[4] = 8;
                if iss & 1 != 0 {
                    inf[6] = 1;
                    e.lectura = Some((r, 8, false, true));
                } else {
                    inf[3] = if r == 31 { 0 } else { e.vcpu.x(r) };
                }
                e.vcpu.poner_pc(pc + 4);
            }
            _ => {
                inf[0] = FALLO;
                // En un fallo de la etapa 2 (de instrucción o de datos), la dirección física del invitado.
                inf[2] = if clase == 0x20 || clase == 0x24 { ((s.hpfar >> 4) << 12) | (s.far & 0xfff) } else { s.far };
                drop(e);
                self.comun.parar();
            }
        }
        Ok(inf)
    }

    /// x0..x30, PC y PSTATE de la vCPU parada.
    pub fn registros(&self) -> Result<[u64; hiper::REGISTROS], Error> {
        let e = self.estado.intentar().ok_or(Error::Ocupada)?;
        Ok(e.vcpu.registros())
    }

    /// Cambia x0..x30, PC y PSTATE de la vCPU parada (PSTATE, saneado: ver `Vcpu::poner_registros`). Una lectura de
    /// aparato pendiente se olvida: lo que diga el servicio manda.
    pub fn poner_registros(&self, r: &[u64; hiper::REGISTROS]) -> Result<(), Error> {
        let mut e = self.estado.intentar().ok_or(Error::Ocupada)?;
        if self.comun.muerta.load(Ordering::Acquire) {
            return Err(Error::Muerta);
        }
        e.lectura = None;
        e.vcpu.poner_registros(r);
        Ok(())
    }

    /// La vCPU parada, como recién encendida en `entrada` con `x0` (PSCI CPU_ON: ver `Vcpu::reiniciar`).
    pub fn reiniciar(&self, entrada: u64, x0: u64) -> Result<(), Error> {
        let mut e = self.estado.intentar().ok_or(Error::Ocupada)?;
        if self.comun.muerta.load(Ordering::Acquire) {
            return Err(Error::Muerta);
        }
        e.lectura = None;
        e.acceso = None;
        e.vcpu.reiniciar(entrada, x0);
        Ok(())
    }

    /// En lugar del último acceso a un aparato (el de la última salida), un aborto externo para el invitado: no hay
    /// nada en esa dirección. Falso si la última salida no fue un acceso.
    pub fn aborto(&self) -> Result<bool, Error> {
        let mut e = self.estado.intentar().ok_or(Error::Ocupada)?;
        if self.comun.muerta.load(Ordering::Acquire) {
            return Err(Error::Muerta);
        }
        let Some((pc, far, escritura)) = e.acceso.take() else { return Ok(false) };
        e.lectura = None;
        e.vcpu.inyectar_aborto(pc, far, escritura);
        Ok(true)
    }
}

impl Drop for Maquina {
    fn drop(&mut self) {
        // Nadie la corre (era la última referencia): fuera de la lista y su número, libre.
        let p = Arc::as_ptr(&self.patada);
        self.comun.vcpus.tomar().retain(|q| Arc::as_ptr(q) != p);
        self.comun.numeros.fetch_and(!(1 << self.numero), Ordering::AcqRel);
    }
}
