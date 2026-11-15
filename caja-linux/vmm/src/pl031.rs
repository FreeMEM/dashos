//! Un PL031 (Arm PrimeCell Real Time Clock, TRM DDI 0224): el reloj de calendario del invitado. systemd y pacman
//! quieren la hora de verdad; sin reloj, Linux arranca en 1970.
//!
//! El contador cuenta segundos. Su valor es el del [`Reloj`] de DashOS más un desfase, que cambia cuando el invitado
//! escribe en RTCLR (cargar el contador): el invitado cambia su hora, no la de DashOS.
//!
//! La alarma: RTCMR guarda un valor; cuando el contador llega a él, RTCRIS se pone a 1 hasta que se borra en RTCICR.
//! La línea (por nivel) es RTCRIS y RTCIMSC. Como el aparato solo se mira cuando el invitado lo toca o cuando el
//! servicio sondea, la alarma puede llegar tarde; el contador, nunca.

use alloc::boxed::Box;

use crate::memoria::MemoriaInvitado;
use crate::mmio::{AparatoMmio, Clase};

pub const DR: u64 = 0x000;
pub const MR: u64 = 0x004;
pub const LR: u64 = 0x008;
pub const CR: u64 = 0x00c;
pub const IMSC: u64 = 0x010;
pub const RIS: u64 = 0x014;
pub const MIS: u64 = 0x018;
pub const ICR: u64 = 0x01c;

pub const TAM_REGION: u64 = 0x1000;

/// Identificación PrimeCell (0xfe0..0xffc): pieza 0x031 de Arm (0x41), revisión 0, y la marca de PrimeCell. El bus AMBA
/// de Linux la lee para saber qué controlador cargar.
const ID: [u32; 8] = [0x31, 0x10, 0x04, 0x00, 0x0d, 0xf0, 0x05, 0xb1];

/// La hora del calendario de quien emula: segundos desde 1970 (UTC). 0 si no la sabe.
pub trait Reloj {
    fn segundos(&self) -> u64;
}

pub struct Pl031 {
    reloj: Box<dyn Reloj>,
    /// Lo que se suma a la hora de DashOS (32 bits, con vuelta, como el contador).
    desfase: u32,
    mr: u32,
    imsc: bool,
    ris: bool,
    /// La alarma está armada (se escribió RTCMR y no ha saltado).
    armada: bool,
}

impl Pl031 {
    pub fn nuevo(reloj: Box<dyn Reloj>) -> Self {
        Self { reloj, desfase: 0, mr: 0, imsc: false, ris: false, armada: false }
    }

    /// El contador que ve el invitado.
    pub fn contador(&self) -> u32 {
        (self.reloj.segundos() as u32).wrapping_add(self.desfase)
    }

    /// Si la alarma armada ya llegó (el contador alcanzó RTCMR; con vuelta, como resta de 32 bits).
    fn mirar_alarma(&mut self) {
        if self.armada && (self.contador().wrapping_sub(self.mr) as i32) >= 0 {
            self.ris = true;
            self.armada = false;
        }
    }
}

impl AparatoMmio for Pl031 {
    fn tam_region(&self) -> u64 {
        TAM_REGION
    }

    fn clase(&self) -> Clase {
        Clase::Pl031
    }

    fn leer(&mut self, desp: u64, tam: u8, _mem: &mut dyn MemoriaInvitado) -> u64 {
        // Registros de 32 bits: un acceso de otro tamaño o desalineado se lee como 0.
        if tam != 4 || desp % 4 != 0 {
            return 0;
        }
        self.mirar_alarma();
        (match desp {
            DR => self.contador(),
            MR => self.mr,
            LR => self.contador(),
            CR => 1, // siempre en marcha (RTCCR.RTCStart)
            IMSC => self.imsc as u32,
            RIS => self.ris as u32,
            MIS => (self.ris && self.imsc) as u32,
            0xfe0..=0xffc => ID[((desp - 0xfe0) / 4) as usize],
            _ => 0,
        }) as u64
    }

    fn escribir(&mut self, desp: u64, tam: u8, valor: u64, _mem: &mut dyn MemoriaInvitado) {
        if tam != 4 || desp % 4 != 0 {
            return;
        }
        let v = valor as u32;
        match desp {
            MR => {
                self.mr = v;
                self.armada = true;
            }
            LR => self.desfase = v.wrapping_sub(self.reloj.segundos() as u32),
            IMSC => self.imsc = v & 1 != 0,
            ICR if v & 1 != 0 => self.ris = false,
            // RTCCR: escribir 0 no para el reloj (TRM: una vez en marcha, solo lo para un reinicio).
            _ => {}
        }
        self.mirar_alarma();
    }

    fn irq(&self) -> bool {
        self.ris && self.imsc
    }

    fn sondear(&mut self, _mem: &mut dyn MemoriaInvitado) {
        self.mirar_alarma();
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use crate::memoria::Ram;
    use alloc::rc::Rc;
    use alloc::vec;
    use core::cell::Cell;

    struct RelojFalso(Rc<Cell<u64>>);
    impl Reloj for RelojFalso {
        fn segundos(&self) -> u64 {
            self.0.get()
        }
    }

    fn nuevo(t: u64) -> (Pl031, Rc<Cell<u64>>, Ram<alloc::vec::Vec<u8>>) {
        let c = Rc::new(Cell::new(t));
        (Pl031::nuevo(Box::new(RelojFalso(c.clone()))), c, Ram { base: 0, datos: vec![0; 16] })
    }

    #[test]
    fn cuenta_la_hora_de_dashos() {
        let (mut r, t, mut m) = nuevo(1_800_000_000);
        assert_eq!(r.leer(DR, 4, &mut m), 1_800_000_000);
        t.set(1_800_000_007);
        assert_eq!(r.leer(DR, 4, &mut m), 1_800_000_007);
        assert_eq!(r.leer(CR, 4, &mut m), 1);
    }

    #[test]
    fn cargar_cambia_solo_la_del_invitado() {
        let (mut r, t, mut m) = nuevo(1000);
        r.escribir(LR, 4, 50, &mut m);
        assert_eq!(r.leer(DR, 4, &mut m), 50);
        t.set(1010);
        assert_eq!(r.leer(DR, 4, &mut m), 60);
        // Hacia atrás y con vuelta de 32 bits.
        r.escribir(LR, 4, u32::MAX as u64, &mut m);
        t.set(1012);
        assert_eq!(r.leer(DR, 4, &mut m), 1);
    }

    #[test]
    fn identificacion_primecell() {
        let (mut r, _, mut m) = nuevo(0);
        let id: u32 = (0..4).map(|i| (r.leer(0xfe0 + 4 * i, 4, &mut m) as u32) << (8 * i)).sum();
        assert_eq!(id & 0x000f_ffff, 0x0004_1031);
        assert_eq!(r.leer(0xff0, 4, &mut m), 0x0d);
        assert_eq!(r.leer(0xffc, 4, &mut m), 0xb1);
    }

    #[test]
    fn alarma() {
        let (mut r, t, mut m) = nuevo(100);
        r.escribir(MR, 4, 105, &mut m);
        r.escribir(IMSC, 4, 1, &mut m);
        assert!(!r.irq());
        t.set(105);
        r.sondear(&mut m);
        assert!(r.irq());
        assert_eq!(r.leer(MIS, 4, &mut m), 1);
        r.escribir(ICR, 4, 1, &mut m);
        assert!(!r.irq());
        // No vuelve a saltar sin otra escritura en RTCMR.
        t.set(200);
        r.sondear(&mut m);
        assert!(!r.irq());
        // Enmascarada: RIS sí, la línea no.
        r.escribir(IMSC, 4, 0, &mut m);
        r.escribir(MR, 4, 150, &mut m);
        assert_eq!(r.leer(RIS, 4, &mut m), 1);
        assert!(!r.irq());
    }

    #[test]
    fn accesos_raros() {
        let (mut r, _, mut m) = nuevo(77);
        assert_eq!(r.leer(DR, 1, &mut m), 0);
        assert_eq!(r.leer(DR + 2, 4, &mut m), 0);
        assert_eq!(r.leer(0x800, 4, &mut m), 0);
        r.escribir(LR, 2, 5, &mut m);
        r.escribir(CR, 4, 0, &mut m);
        assert_eq!(r.leer(DR, 4, &mut m), 77);
        assert_eq!(r.leer(CR, 4, &mut m), 1);
    }
}
