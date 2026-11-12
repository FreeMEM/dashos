//! Las máquinas virtuales de la caja Linux (H9, `docs/bitacoras/caja-linux.md`), vistas como objetos con llave.
//!
//! Crear una pide el permiso de máquinas virtuales (lo da el kernel a `inicio`, y `inicio` al servicio `caja-linux`) y
//! un objeto de memoria del cupo de quien la pide, que es la RAM del invitado. "Correr" entra en el invitado hasta que
//! sale y dice por qué, ya interpretado (`Informe`): una llamada al hipervisor (HVC), un acceso a una dirección que no
//! es su memoria (los aparatos que emula el servicio, o un fallo, según el servicio), un WFI, una interrupción del
//! anfitrión (el reloj saca al invitado cada tic: nunca se queda con el núcleo) o un fallo. El kernel no emula nada:
//! eso es del servicio.
//!
//! Lo que toca el hardware (EL2, la etapa 2, el cambio de mundo) está en `base::hiper`.

use crate::base::hiper::{self, Vcpu};
use crate::cerrojo::Cerrojo;
use crate::memoria::Memoria;
use alloc::sync::Arc;

/// Por qué volvió el invitado (el primer número del informe; lo devuelve también la llamada).
pub const HVC: u64 = 1;
/// Leyó en una dirección que no es su memoria: el valor que dé el servicio en la siguiente vuelta va a su registro.
pub const LECTURA: u64 = 2;
/// Escribió en una dirección que no es su memoria.
pub const ESCRITURA: u64 = 3;
pub const WFI: u64 = 4;
/// Una interrupción del anfitrión lo sacó (el reloj, un aparato): no ha pasado nada suyo.
pub const INTERRUMPIDO: u64 = 5;
/// Una trampa o un fallo que el kernel no sabe pasar al servicio de otra forma (SMC, la coma flotante, un fallo de la
/// etapa 2 sin datos, un SError): la máquina queda muerta.
pub const FALLO: u64 = 6;
/// Tocó un registro del sistema atrapado (depuración, contadores de rendimiento, el temporizador físico...): en la
/// dirección, su codificación (op0, op1, CRn, CRm y op2, como en el ESR, sin el registro de destino ni el sentido);
/// en x1, 1 si lee (el valor de la siguiente vuelta va a su registro) y 0 si escribe (el valor, en el informe).
pub const REGISTRO: u64 = 7;

/// Palabras del informe: motivo, PC de la instrucción, dirección (física del invitado), valor, tamaño en bytes (o el
/// número del HVC), ESR, x1 y x2 (en un HVC), las cuentas del contador que pasó la CPU fuera del kernel (ida y vuelta
/// por EL2 con el invitado), x3 (en un HVC), el plazo del temporizador virtual del invitado (CNTV_CVAL, si está
/// encendido y sin enmascarar; si no, `u64::MAX`) y señales: bit 0, hay una interrupción pendiente en sus registros de
/// lista (un WFI no tiene que esperar).
pub const PALABRAS: usize = 12;

/// Registros de lista del servicio en el bloque de interrupciones.
pub const IRQ_LRS: usize = 8;
/// El bloque de interrupciones que va y viene con `maquina_correr` (palabras de 64 bits): [0] cuántos registros de
/// lista puede llenar el servicio (lo escribe el kernel; 0 sin GIC virtual), [1] cómo tiene el invitado el temporizador
/// virtual en su distribuidor (lo escribe el servicio: bit 0 habilitado, [15:8] prioridad, bit 16 grupo 1) y desde [2]
/// los registros de lista (GICH_LRn del GICv2, sin el bit HW): los pone el servicio y vuelven como los dejó el
/// invitado.
pub const IRQ_PALABRAS: usize = 2 + IRQ_LRS;

pub enum Error {
    NoHay,
    /// Otro hilo la está corriendo.
    Ocupada,
    /// Murió en un fallo.
    Muerta,
}

struct Estado {
    vcpu: Vcpu,
    /// Una lectura de aparato a medias: el registro (31 = xzr), el tamaño en bytes, si se extiende el signo (SSE) y si
    /// el registro es de 64 bits (SF).
    lectura: Option<(usize, u32, bool, bool)>,
    /// El último acceso a un aparato, para inyectar un aborto en su lugar: PC de la instrucción, dirección virtual y si
    /// escribía. Solo hasta la siguiente vuelta.
    acceso: Option<(u64, u64, bool)>,
    muerta: bool,
}

pub struct Maquina {
    estado: Cerrojo<Estado>,
}

impl Maquina {
    pub fn nueva(memoria: Arc<Memoria>, ipa: u64, entrada: u64, x0: u64, cupo: &Arc<crate::cupo::Cupo>) -> Result<Self, hiper::Error> {
        let vcpu = Vcpu::nueva(memoria, ipa, entrada, x0, cupo)?;
        Ok(Self { estado: Cerrojo::new(Estado { vcpu, lectura: None, acceso: None, muerta: false }) })
    }

    /// Más memoria para el invitado, desde `ipa`. `None` si otro hilo la está corriendo.
    pub fn anadir(&self, memoria: Arc<Memoria>, ipa: u64, cupo: &Arc<crate::cupo::Cupo>) -> Option<Result<(), hiper::Error>> {
        let mut e = self.estado.intentar()?;
        Some(e.vcpu.anadir(memoria, ipa, cupo))
    }

    /// Entra en el invitado. `valor` es la respuesta a una lectura de aparato pendiente (si no la hay, no se usa).
    /// `irq`, el bloque de interrupciones del servicio (`IRQ_PALABRAS`), si lo da.
    pub fn correr(&self, valor: u64, mut irq: Option<&mut [u64; IRQ_PALABRAS]>) -> Result<[u64; PALABRAS], Error> {
        // Sin esperar: dos hilos no corren a la vez la misma vCPU.
        let Some(mut e) = self.estado.intentar() else { return Err(Error::Ocupada) };
        if e.muerta {
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
        let s = loop {
            let Some(s) = e.vcpu.correr() else { return Err(Error::NoHay) };
            // La primera vez que usa la FPU: desde ahora tiene la suya, y se repite la instrucción.
            if s.tipo == hiper::SALIDA_SINCRONA && s.esr >> 26 == 0x07 && !e.vcpu.tiene_fp() && e.vcpu.usar_fp() {
                continue;
            }
            break s;
        };
        let cuentas = crate::reloj::ahora().wrapping_sub(antes);
        if let Some(b) = irq.as_deref_mut() {
            b[0] = lrs as u64;
            for i in 0..IRQ_LRS {
                b[2 + i] = if i < lrs { e.vcpu.lr(i) as u64 } else { 0 };
            }
        }
        let mut inf = [0u64; PALABRAS];
        inf[8] = cuentas;
        inf[5] = s.esr;
        inf[10] = e.vcpu.plazo_temporizador().unwrap_or(u64::MAX);
        inf[11] = e.vcpu.lr_pendiente() as u64;
        let pc = e.vcpu.pc();
        inf[1] = pc;
        if s.tipo == hiper::SALIDA_IRQ || s.tipo == hiper::SALIDA_FIQ {
            inf[0] = INTERRUMPIDO;
            return Ok(inf);
        }
        if s.tipo != hiper::SALIDA_SINCRONA {
            e.muerta = true;
            inf[0] = FALLO;
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
                e.muerta = true;
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
        if e.muerta {
            return Err(Error::Muerta);
        }
        e.lectura = None;
        e.vcpu.poner_registros(r);
        Ok(())
    }

    /// En lugar del último acceso a un aparato (el de la última salida), un aborto externo para el invitado: no hay
    /// nada en esa dirección. Falso si la última salida no fue un acceso.
    pub fn aborto(&self) -> Result<bool, Error> {
        let mut e = self.estado.intentar().ok_or(Error::Ocupada)?;
        if e.muerta {
            return Err(Error::Muerta);
        }
        let Some((pc, far, escritura)) = e.acceso.take() else { return Ok(false) };
        e.lectura = None;
        e.vcpu.inyectar_aborto(pc, far, escritura);
        Ok(true)
    }

    /// Proyecta la GICV del GIC virtual donde el invitado espera la interfaz de CPU de su GIC.
    pub fn proyectar_gicv(&self, ipa: u64, cupo: &Arc<crate::cupo::Cupo>) -> Result<Result<(), hiper::Error>, Error> {
        let mut e = self.estado.intentar().ok_or(Error::Ocupada)?;
        Ok(e.vcpu.proyectar_gicv(ipa, cupo))
    }
}