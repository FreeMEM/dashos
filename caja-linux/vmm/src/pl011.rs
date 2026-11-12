//! Un PL011 mínimo (Arm PrimeCell UART, TRM DDI 0183), para `earlycon` y como `ttyAMA0` si el invitado lo quiere.
//!
//! La transmisión sale entera al momento: la FIFO de salida siempre está vacía y nunca ocupada. La de entrada guarda
//! hasta 16 bytes que llegan de DashOS por [`AparatoMmio::sondear`]. No hay líneas de módem, ni DMA, ni errores de
//! trama. Los registros de baudios y de línea se guardan y se leen, pero no cambian nada.
//!
//! Interrupciones (por nivel, las enmascara UARTIMSC):
//! - RX cuando la FIFO de entrada llega al nivel de UARTIFLS, y RT (tiempo agotado) mientras no esté vacía: la
//!   máquina virtual no espera 32 bits de tiempo;
//! - TX al escribir en UARTDR (la FIFO vuelve a quedar vacía, por debajo del nivel), hasta que se borra en UARTICR.

use alloc::boxed::Box;
use alloc::collections::VecDeque;

use crate::memoria::MemoriaInvitado;
use crate::mmio::{AparatoMmio, Clase};
use crate::virtio::consola::ExtremoConsola;

pub const DR: u64 = 0x000;
pub const RSR: u64 = 0x004;
pub const FR: u64 = 0x018;
pub const ILPR: u64 = 0x020;
pub const IBRD: u64 = 0x024;
pub const FBRD: u64 = 0x028;
pub const LCR_H: u64 = 0x02c;
pub const CR: u64 = 0x030;
pub const IFLS: u64 = 0x034;
pub const IMSC: u64 = 0x038;
pub const RIS: u64 = 0x03c;
pub const MIS: u64 = 0x040;
pub const ICR: u64 = 0x044;
pub const DMACR: u64 = 0x048;

pub const FR_BUSY: u32 = 1 << 3;
pub const FR_RXFE: u32 = 1 << 4;
pub const FR_TXFF: u32 = 1 << 5;
pub const FR_RXFF: u32 = 1 << 6;
pub const FR_TXFE: u32 = 1 << 7;

pub const INT_RX: u32 = 1 << 4;
pub const INT_TX: u32 = 1 << 5;
pub const INT_RT: u32 = 1 << 6;

const LCR_FEN: u32 = 1 << 4;
const CR_UARTEN: u32 = 1 << 0;
const CR_TXE: u32 = 1 << 8;
const CR_RXE: u32 = 1 << 9;
const FIFO: usize = 16;

/// Identificación PrimeCell (0xfe0..0xffc): PL011 r1p5 y la marca de PrimeCell. El bus AMBA de Linux la lee para
/// saber qué controlador cargar.
const ID: [u32; 8] = [0x11, 0x10, 0x34, 0x00, 0x0d, 0xf0, 0x05, 0xb1];

pub const TAM_REGION: u64 = 0x1000;

pub struct Pl011 {
    extremo: Box<dyn ExtremoConsola>,
    rx: VecDeque<u8>,
    ibrd: u32,
    fbrd: u32,
    lcr: u32,
    cr: u32,
    ifls: u32,
    imsc: u32,
    tx_ris: bool,
    ilpr: u32,
    dmacr: u32,
}

impl Pl011 {
    pub fn nuevo(extremo: Box<dyn ExtremoConsola>) -> Self {
        Self {
            extremo,
            rx: VecDeque::with_capacity(FIFO),
            ibrd: 0,
            fbrd: 0,
            lcr: 0,
            cr: CR_TXE | CR_RXE, // valor tras reinicio (TRM 3.3.8)
            ifls: 0x12,
            imsc: 0,
            tx_ris: false,
            ilpr: 0,
            dmacr: 0,
        }
    }

    fn hondo(&self) -> usize {
        if self.lcr & LCR_FEN != 0 { FIFO } else { 1 }
    }

    /// Nivel de la FIFO de entrada para RX, según UARTIFLS[5:3]: 1/8, 1/4, 1/2, 3/4, 7/8.
    fn nivel_rx(&self) -> usize {
        if self.lcr & LCR_FEN == 0 {
            return 1;
        }
        match (self.ifls >> 3) & 7 {
            0 => FIFO / 8,
            1 => FIFO / 4,
            2 => FIFO / 2,
            3 => FIFO * 3 / 4,
            _ => FIFO * 7 / 8,
        }
    }

    fn ris(&self) -> u32 {
        let mut r = 0;
        if !self.rx.is_empty() {
            r |= INT_RT;
            if self.rx.len() >= self.nivel_rx() {
                r |= INT_RX;
            }
        }
        if self.tx_ris {
            r |= INT_TX;
        }
        r
    }

    fn fr(&self) -> u32 {
        let mut f = FR_TXFE;
        if self.rx.is_empty() {
            f |= FR_RXFE;
        }
        if self.rx.len() >= self.hondo() {
            f |= FR_RXFF;
        }
        f
    }

    fn recibir(&mut self) {
        if self.cr & CR_UARTEN == 0 || self.cr & CR_RXE == 0 {
            return;
        }
        let mut b = [0u8; FIFO];
        while self.rx.len() < self.hondo() && self.extremo.hay_entrada() {
            let hueco = self.hondo() - self.rx.len();
            let n = self.extremo.entrada(&mut b[..hueco]).min(hueco);
            if n == 0 {
                break;
            }
            self.rx.extend(&b[..n]);
        }
    }
}

impl AparatoMmio for Pl011 {
    fn tam_region(&self) -> u64 {
        TAM_REGION
    }

    fn clase(&self) -> Clase {
        Clase::Pl011
    }

    fn leer(&mut self, desp: u64, _tam: u8, _mem: &mut dyn MemoriaInvitado) -> u64 {
        if !desp.is_multiple_of(4) {
            return 0;
        }
        let v = match desp {
            DR => {
                let b = self.rx.pop_front().unwrap_or(0) as u32;
                self.recibir();
                b
            }
            RSR => 0,
            FR => self.fr(),
            ILPR => self.ilpr,
            IBRD => self.ibrd,
            FBRD => self.fbrd,
            LCR_H => self.lcr,
            CR => self.cr,
            IFLS => self.ifls,
            IMSC => self.imsc,
            RIS => self.ris(),
            MIS => self.ris() & self.imsc,
            DMACR => self.dmacr,
            0xfe0..=0xffc => ID[((desp - 0xfe0) / 4) as usize],
            _ => 0,
        };
        v as u64
    }

    fn escribir(&mut self, desp: u64, _tam: u8, valor: u64, _mem: &mut dyn MemoriaInvitado) {
        if !desp.is_multiple_of(4) {
            return;
        }
        let v = valor as u32;
        match desp {
            DR => {
                // Con la UART o la transmisión apagadas, el TRM no garantiza nada; para earlycon conviene sacarlo
                // igual (un invitado puede escribir sin haberla configurado).
                self.extremo.salida(&[v as u8]);
                self.tx_ris = true;
            }
            ILPR => self.ilpr = v & 0xff,
            IBRD => self.ibrd = v & 0xffff,
            FBRD => self.fbrd = v & 0x3f,
            LCR_H => {
                self.lcr = v & 0xff;
                // Cambiar FEN vacía las FIFO; sin FIFO solo cabe un byte.
                while self.rx.len() > self.hondo() {
                    self.rx.pop_back();
                }
            }
            CR => {
                self.cr = v & 0xffff;
                self.recibir();
            }
            IFLS => self.ifls = v & 0x3f,
            IMSC => self.imsc = v & 0x7ff,
            ICR => {
                if v & INT_TX != 0 {
                    self.tx_ris = false;
                }
                // RX y RT siguen el estado de la FIFO: vuelven si sigue con datos.
            }
            DMACR => self.dmacr = v & 7,
            RSR => {}
            _ => {}
        }
    }

    fn irq(&self) -> bool {
        self.ris() & self.imsc != 0
    }

    fn sondear(&mut self, _mem: &mut dyn MemoriaInvitado) {
        self.recibir();
    }
}
