//! El PCIe que ve un dominio de controladores: un puente ECAM genérico (`pci-host-ecam-generic`) con un solo aparato,
//! el que DashOS le presta (S35, paso 2; `docs/bitacoras/dominio-red.md`).
//!
//! El espacio de configuración es una copia de la cabecera del aparato de verdad (fabricante, aparato, clase,
//! subsistema), con lo que el dominio no debe tocar escondido o fijo:
//!
//! - **La BAR 0** es la de verdad y no se mueve: el hipervisor proyecta esos registros en la misma dirección en la
//!   etapa 2 del dominio. El dominio puede medirla (escribir unos y leer la máscara); lo que escriba en ella no cambia
//!   nada. Las demás BAR (y la ROM) no existen para él.
//! - **Sin capacidades** (puntero 0 y el bit de la lista apagado): ni MSI, ni MSI-X, ni gestión de energía. El aparato
//!   avisa por su INTx, que el servicio recibe de DashOS y pasa al dominio (`EstadoPci::intx`).
//! - **El registro de mando**: el dominio enciende la memoria y el maestro de bus; el servicio lo copia al aparato de
//!   verdad (`EstadoPci::mando_nuevo`) y apaga el maestro de bus cuando el dominio muere, para que el aparato deje de
//!   escribir en una memoria que se va a soltar.
//!
//! Solo el bus 0, aparato 0, función 0 existe: lo demás se lee como unos (no hay nada), como en un bus de verdad.
//!
//! **Los puertos de la e1000** (`PuertosE1000`): el controlador de Linux de la Intel 8254x hace el reinicio de algunos
//! modelos por su BAR de puertos (la 1), que es solo otra puerta a los mismos registros (manual de la familia 8254x,
//! "I/O-Mapped Access to Internal Registers": IOADDR en el 0 y IODATA en el 4). El dominio la ve como una BAR de
//! puertos de verdad (`TAM_PUERTOS` bytes, donde la ponga) en el espacio de puertos del PCIe, que en la máquina es una
//! ventana de MMIO (`dominio::PUERTOS`) que no se proyecta: cada acceso sale al servicio, que lo hace en la BAR 0 de
//! verdad (`Registros`) si cae dentro. Es propio de este aparato y de este paso intermedio.

use alloc::rc::Rc;
use core::cell::RefCell;

use crate::memoria::MemoriaInvitado;
use crate::mmio::{AparatoMmio, Clase};

/// El ECAM del bus 0: 32 aparatos × 8 funciones × 4 KB.
pub const TAM_REGION: u64 = 1 << 20;

/// Bits del registro de mando que el dominio puede poner: memoria (1), maestro de bus (2) e INTx apagada (10).
pub const MANDO_MEMORIA: u16 = 1 << 1;
pub const MANDO_MAESTRO: u16 = 1 << 2;
pub const MANDO_SIN_INTX: u16 = 1 << 10;
/// Los puertos (bit 0): solo cuentan para la BAR de puertos emulada; no se copian al aparato.
pub const MANDO_PUERTOS: u16 = 1 << 0;
const MANDO_DEJADOS: u16 = MANDO_PUERTOS | MANDO_MEMORIA | MANDO_MAESTRO | MANDO_SIN_INTX;
/// El tamaño de la BAR de puertos de la e1000.
pub const TAM_PUERTOS: u32 = 0x40;
/// El espacio de puertos del PCIe del dominio: 64 KB.
pub const TAM_ESPACIO_PUERTOS: u64 = 0x1_0000;

/// El aparato prestado, visto desde el dominio.
#[derive(Debug)]
pub struct EstadoPci {
    /// Las primeras 16 palabras de su cabecera de verdad (tipo 0).
    real: [u32; 16],
    bar: u64,
    tam_bar: u64,
    /// La BAR 0 es de 64 bits (ocupa también la 1).
    bar64: bool,
    mando: u16,
    /// El dominio escribió unos en la BAR (la está midiendo): las palabras 0 y 1 dicen la máscara.
    midiendo: [bool; 2],
    /// La BAR de puertos (la 1), si el aparato la tiene: dónde la puso el dominio en el espacio de puertos.
    puertos: Option<u32>,
    midiendo_puertos: bool,
    /// La INTx del aparato de verdad: la sube el servicio cuando llega su interrupción y la baja cuando el dominio la
    /// termina.
    pub intx: bool,
    /// El mando cambió y el servicio aún no lo ha copiado al aparato.
    pub mando_nuevo: Option<u16>,
    /// Accesos raros (tamaños o sitios que no existen): se cuentan.
    pub raros: u64,
}

impl EstadoPci {
    /// `real`: los primeros 64 bytes del espacio de configuración del aparato. `bar`, `tam_bar`: su BAR 0 (memoria).
    pub fn nuevo(real: [u32; 16], bar: u64, tam_bar: u64) -> Self {
        let bar64 = real[4] & 0x6 == 0x4;
        Self { real, bar, tam_bar, bar64, mando: 0, midiendo: [false; 2], puertos: None, midiendo_puertos: false, intx: false, mando_nuevo: None, raros: 0 }
    }

    /// Con la BAR de puertos de la e1000 (la 1, de `TAM_PUERTOS` bytes), empezando en `desde` del espacio de puertos.
    /// Solo para un aparato con la BAR 0 de 32 bits.
    pub fn con_puertos(mut self, desde: u32) -> Self {
        if !self.bar64 {
            self.puertos = Some(desde & !(TAM_PUERTOS - 1));
        }
        self
    }

    /// Dónde tiene el dominio la BAR de puertos (si la hay y tiene los puertos encendidos en su mando).
    pub fn puertos(&self) -> Option<u32> {
        self.puertos.filter(|_| self.mando & MANDO_PUERTOS != 0)
    }

    pub fn mando(&self) -> u16 {
        self.mando
    }

    /// La línea que ve el dominio: la INTx, si no la apagó en su mando.
    pub fn linea(&self) -> bool {
        self.intx && self.mando & MANDO_SIN_INTX == 0
    }

    fn palabra(&self, reg: usize) -> u32 {
        let mascara = !(self.tam_bar.max(16) - 1);
        match reg {
            0 => self.real[0],
            // Estado: sin lista de capacidades; el bit 3 dice si la INTx está arriba.
            1 => self.mando as u32 | (self.intx as u32) << 19,
            2 => self.real[2],
            // Tipo de cabecera 0, de una sola función (sin el bit 7).
            3 => self.real[3] & 0xff7f_ffff,
            4 => {
                let marcas = self.real[4] & 0xf;
                if self.midiendo[0] { (mascara as u32 & !0xf) | marcas } else { (self.bar as u32 & !0xf) | marcas }
            }
            5 if self.bar64 => {
                if self.midiendo[1] { (mascara >> 32) as u32 } else { (self.bar >> 32) as u32 }
            }
            5 => match self.puertos {
                Some(_) if self.midiendo_puertos => !(TAM_PUERTOS - 1) | 1,
                Some(p) => p | 1,
                None => 0,
            },
            // Subsistema.
            11 => self.real[11],
            // Línea 0 y patilla INTA.
            15 => 1 << 8,
            _ => 0,
        }
    }

    fn escribir_palabra(&mut self, reg: usize, v: u32, bytes: u32) {
        match reg {
            1 if bytes & 0b11 != 0 => {
                let m = (v as u16) & MANDO_DEJADOS;
                if m != self.mando {
                    self.mando = m;
                    self.mando_nuevo = Some(m);
                }
            }
            4 => self.midiendo[0] = v | 0xf == u32::MAX,
            5 if self.bar64 => self.midiendo[1] = v == u32::MAX,
            5 if self.puertos.is_some() => {
                self.midiendo_puertos = v | 3 == u32::MAX;
                if !self.midiendo_puertos {
                    // El dominio la mueve donde quiera dentro de su espacio de puertos (64 KB).
                    self.puertos = Some(v & 0xffff & !(TAM_PUERTOS - 1));
                }
            }
            _ => {}
        }
    }
}

/// El puente ECAM. El servicio guarda el mismo `Rc`.
pub struct PciDominio(pub Rc<RefCell<EstadoPci>>);

impl PciDominio {
    pub fn nuevo(e: EstadoPci) -> (Self, Rc<RefCell<EstadoPci>>) {
        let e = Rc::new(RefCell::new(e));
        (Self(e.clone()), e)
    }
}

/// (aparato, función, registro de 32 bits, desplazamiento dentro de él) de un desplazamiento del ECAM.
fn partir(desp: u64) -> (u64, u64, usize, u32) {
    ((desp >> 15) & 31, (desp >> 12) & 7, ((desp & 0xfff) / 4) as usize, (desp & 3) as u32)
}

impl AparatoMmio for PciDominio {
    fn tam_region(&self) -> u64 {
        TAM_REGION
    }

    fn clase(&self) -> Clase {
        Clase::PciDominio
    }

    fn leer(&mut self, desp: u64, tam: u8, _mem: &mut dyn MemoriaInvitado) -> u64 {
        let mut e = self.0.borrow_mut();
        let (ap, func, reg, dentro) = partir(desp);
        if !matches!(tam, 1 | 2 | 4) || dentro % tam as u32 != 0 {
            e.raros += 1;
            return 0;
        }
        if ap != 0 || func != 0 {
            return u64::MAX;
        }
        let w = if reg < 16 { e.palabra(reg) } else { 0 };
        (w >> (8 * dentro)) as u64
    }

    fn escribir(&mut self, desp: u64, tam: u8, valor: u64, _mem: &mut dyn MemoriaInvitado) {
        let mut e = self.0.borrow_mut();
        let (ap, func, reg, dentro) = partir(desp);
        if !matches!(tam, 1 | 2 | 4) || dentro % tam as u32 != 0 {
            e.raros += 1;
            return;
        }
        if ap != 0 || func != 0 || reg >= 16 {
            return;
        }
        // Lo escrito, encima de lo que había en esa palabra; `bytes` dice qué bytes tocó.
        let bytes = ((1u32 << tam) - 1) << dentro;
        let antes = e.palabra(reg);
        let mut v = antes;
        for b in 0..4 {
            if bytes & (1 << b) != 0 {
                v = (v & !(0xff << (8 * b))) | (((valor >> (8 * (b - dentro))) as u32 & 0xff) << (8 * b));
            }
        }
        e.escribir_palabra(reg, v, bytes);
    }

    fn irq(&self) -> bool {
        self.0.borrow().linea()
    }
}

/// Los registros de verdad del aparato (su BAR 0), que el servicio tiene proyectados.
pub trait Registros {
    fn leer32(&mut self, desp: u64) -> u32;
    fn escribir32(&mut self, desp: u64, v: u32);
}

/// La BAR de puertos de la e1000, en el espacio de puertos del PCIe del dominio (ver arriba).
pub struct PuertosE1000 {
    pci: Rc<RefCell<EstadoPci>>,
    regs: alloc::boxed::Box<dyn Registros>,
    ioaddr: u32,
}

impl PuertosE1000 {
    pub fn nuevo(pci: Rc<RefCell<EstadoPci>>, regs: alloc::boxed::Box<dyn Registros>) -> Self {
        Self { pci, regs, ioaddr: 0 }
    }

    /// El registro que dice IOADDR, si cae dentro de la BAR 0 y está alineado.
    fn destino(&self) -> Option<u64> {
        let tam = self.pci.borrow().tam_bar;
        let d = self.ioaddr as u64;
        (d % 4 == 0 && d + 4 <= tam).then_some(d)
    }
}

impl AparatoMmio for PuertosE1000 {
    fn tam_region(&self) -> u64 {
        TAM_ESPACIO_PUERTOS
    }

    fn clase(&self) -> Clase {
        Clase::PciDominio
    }

    fn leer(&mut self, desp: u64, tam: u8, _mem: &mut dyn MemoriaInvitado) -> u64 {
        let Some(base) = self.pci.borrow().puertos() else { return u64::MAX };
        let Some(d) = desp.checked_sub(base as u64).filter(|&d| d < TAM_PUERTOS as u64) else { return u64::MAX };
        match (d, tam) {
            (0, 4) => self.ioaddr as u64,
            (4, 4) => match self.destino() {
                Some(r) => self.regs.leer32(r) as u64,
                None => {
                    self.pci.borrow_mut().raros += 1;
                    0
                }
            },
            _ => {
                self.pci.borrow_mut().raros += 1;
                0
            }
        }
    }

    fn escribir(&mut self, desp: u64, tam: u8, valor: u64, _mem: &mut dyn MemoriaInvitado) {
        let Some(base) = self.pci.borrow().puertos() else { return };
        let Some(d) = desp.checked_sub(base as u64).filter(|&d| d < TAM_PUERTOS as u64) else { return };
        match (d, tam) {
            (0, 4) => self.ioaddr = valor as u32,
            (4, 4) => match self.destino() {
                Some(r) => self.regs.escribir32(r, valor as u32),
                None => self.pci.borrow_mut().raros += 1,
            },
            _ => self.pci.borrow_mut().raros += 1,
        }
    }

    fn irq(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use crate::memoria::Ram;
    use alloc::vec;

    fn e1000() -> EstadoPci {
        let mut real = [0u32; 16];
        real[0] = 0x100e_8086;
        real[1] = 0x0010_0007;
        real[2] = 0x0200_0003;
        real[3] = 0x0080_0000;
        real[4] = 0x1004_0000;
        real[5] = 0xc001;
        real[11] = 0x1100_1af4;
        real[13] = 0xdc;
        real[15] = 0x0000_0123;
        EstadoPci::nuevo(real, 0x1004_0000, 0x2_0000)
    }

    #[test]
    fn cabecera_vista_desde_el_dominio() {
        let (mut p, e) = PciDominio::nuevo(e1000());
        let mut m = Ram { base: 0, datos: vec![0u8; 16] };
        assert_eq!(p.leer(0, 4, &mut m), 0x100e_8086);
        assert_eq!(p.leer(2, 2, &mut m), 0x100e);
        // Otro aparato o función: nada.
        assert_eq!(p.leer(1 << 15, 4, &mut m) as u32, u32::MAX);
        assert_eq!(p.leer(1 << 12, 2, &mut m) as u16, u16::MAX);
        // Sin capacidades ni multifunción; la BAR 1 (puertos) no existe; la patilla A.
        assert_eq!(p.leer(4, 4, &mut m), 0);
        assert_eq!(p.leer(0xc, 4, &mut m), 0);
        assert_eq!(p.leer(0x14, 4, &mut m), 0);
        assert_eq!(p.leer(0x34, 1, &mut m), 0);
        assert_eq!(p.leer(0x3d, 1, &mut m), 1);
        // Medir la BAR 0 y volver a ponerla en otro sitio: no se mueve.
        p.escribir(0x10, 4, 0xffff_ffff, &mut m);
        assert_eq!(p.leer(0x10, 4, &mut m), 0xfffe_0000);
        p.escribir(0x10, 4, 0x2000_0000, &mut m);
        assert_eq!(p.leer(0x10, 4, &mut m), 0x1004_0000);
        // El mando: solo memoria, maestro e INTx apagada; el servicio se entera.
        p.escribir(4, 2, 0xffff, &mut m);
        assert_eq!(p.leer(4, 2, &mut m), (MANDO_PUERTOS | MANDO_MEMORIA | MANDO_MAESTRO | MANDO_SIN_INTX) as u64);
        assert_eq!(e.borrow_mut().mando_nuevo.take(), Some(MANDO_PUERTOS | MANDO_MEMORIA | MANDO_MAESTRO | MANDO_SIN_INTX));
        // Con la INTx apagada en el mando, la línea no sube.
        e.borrow_mut().intx = true;
        assert!(!p.irq());
        assert_eq!(p.leer(6, 1, &mut m) & 8, 8);
        p.escribir(4, 1, 0x06, &mut m);
        assert_eq!(e.borrow().mando(), MANDO_MEMORIA | MANDO_MAESTRO | MANDO_SIN_INTX);
        p.escribir(5, 1, 0, &mut m);
        assert!(p.irq());
        // El mismo valor otra vez no es un cambio.
        p.escribir(4, 2, 0x06, &mut m);
        assert_eq!(e.borrow_mut().mando_nuevo.take(), Some(MANDO_MEMORIA | MANDO_MAESTRO));
        p.escribir(4, 2, 0x06, &mut m);
        assert_eq!(e.borrow_mut().mando_nuevo.take(), None);
    }

    #[test]
    fn bar_de_64_bits() {
        let mut real = [0u32; 16];
        real[4] = 0x4;
        let (mut p, _) = PciDominio::nuevo(EstadoPci::nuevo(real, 0x8_1000_0000, 0x4000));
        let mut m = Ram { base: 0, datos: vec![0u8; 16] };
        assert_eq!(p.leer(0x10, 4, &mut m), 0x1000_0004);
        assert_eq!(p.leer(0x14, 4, &mut m), 0x8);
        p.escribir(0x10, 4, 0xffff_ffff, &mut m);
        p.escribir(0x14, 4, 0xffff_ffff, &mut m);
        assert_eq!(p.leer(0x10, 4, &mut m), 0xffff_c004);
        assert_eq!(p.leer(0x14, 4, &mut m), 0xffff_ffff);
    }

    struct Falsos(Rc<RefCell<alloc::vec::Vec<(u64, Option<u32>)>>>);
    impl Registros for Falsos {
        fn leer32(&mut self, desp: u64) -> u32 {
            self.0.borrow_mut().push((desp, None));
            0xabcd_0000 | desp as u32
        }
        fn escribir32(&mut self, desp: u64, v: u32) {
            self.0.borrow_mut().push((desp, Some(v)));
        }
    }

    #[test]
    fn puertos_de_la_e1000() {
        let (mut p, e) = PciDominio::nuevo(e1000().con_puertos(0x1000));
        let mut m = Ram { base: 0, datos: vec![0u8; 16] };
        let hecho = Rc::new(RefCell::new(alloc::vec::Vec::new()));
        let mut io = PuertosE1000::nuevo(e.clone(), alloc::boxed::Box::new(Falsos(hecho.clone())));
        assert_eq!(p.leer(0x14, 4, &mut m), 0x1001);
        p.escribir(0x14, 4, 0xffff_ffff, &mut m);
        assert_eq!(p.leer(0x14, 4, &mut m), 0xffff_ffc1);
        p.escribir(0x14, 4, 0x2040, &mut m);
        assert_eq!(p.leer(0x14, 4, &mut m), 0x2041);
        // Sin los puertos encendidos en el mando, no contesta.
        assert_eq!(io.leer(0x2044, 4, &mut m), u64::MAX);
        p.escribir(4, 2, 0x7, &mut m);
        io.escribir(0x2040, 4, 0x8, &mut m);
        assert_eq!(io.leer(0x2044, 4, &mut m), 0xabcd_0008);
        io.escribir(0x2044, 4, 0x1234, &mut m);
        // Fuera de la BAR 0, desalineado o con otro tamaño: no llega a los registros.
        io.escribir(0x2040, 4, 0x2_0000, &mut m);
        assert_eq!(io.leer(0x2044, 4, &mut m), 0);
        io.escribir(0x2040, 4, 0x6, &mut m);
        io.escribir(0x2044, 4, 1, &mut m);
        io.escribir(0x2040, 2, 0x8, &mut m);
        assert_eq!(io.leer(0x1000, 4, &mut m), u64::MAX);
        assert_eq!(*hecho.borrow(), [(8, None), (8, Some(0x1234))]);
        assert_eq!(e.borrow().raros, 3);
        // El mando del aparato de verdad no lleva el bit de los puertos.
        assert_eq!(e.borrow_mut().mando_nuevo.take(), Some(0x7));
    }

    #[test]
    fn accesos_raros() {
        let (mut p, e) = PciDominio::nuevo(e1000());
        let mut m = Ram { base: 0, datos: vec![0u8; 16] };
        assert_eq!(p.leer(1, 4, &mut m), 0);
        assert_eq!(p.leer(0, 8, &mut m), 0);
        p.escribir(3, 2, 0xffff, &mut m);
        p.escribir(0x800, 4, 0xffff_ffff, &mut m);
        assert_eq!(e.borrow().raros, 3);
        // Todo el espacio, al azar: ni pánico ni cambios fuera del mando.
        let mut x = 0x1234_5678_9abc_def0u64;
        for _ in 0..200_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let desp = x % TAM_REGION;
            let tam = [1u8, 2, 4, 8, 3][(x >> 40) as usize % 5];
            if x & 1 == 0 {
                p.leer(desp, tam, &mut m);
            } else {
                p.escribir(desp, tam, x.rotate_left(17), &mut m);
            }
        }
        assert_eq!(e.borrow().mando() & !MANDO_DEJADOS, 0);
        assert_eq!(p.leer(0, 4, &mut m), 0x100e_8086);
    }
}
