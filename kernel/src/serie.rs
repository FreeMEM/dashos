//! Puerto serie PL011 (UART0) de la Pi 4.

use crate::mmio::RegLe32;
use core::fmt;

/// Dirección del PL011 con los periféricos en modo bajo. En C4 se leerá del árbol de dispositivos.
const BASE: usize = crate::mmu::virt(0xFE20_1000);

const DR: RegLe32 = RegLe32::new(BASE + 0x00);
const FR: RegLe32 = RegLe32::new(BASE + 0x18);
const IBRD: RegLe32 = RegLe32::new(BASE + 0x24);
const FBRD: RegLe32 = RegLe32::new(BASE + 0x28);
const LCRH: RegLe32 = RegLe32::new(BASE + 0x2C);
const CR: RegLe32 = RegLe32::new(BASE + 0x30);
const ICR: RegLe32 = RegLe32::new(BASE + 0x44);

const FR_TXFF: u32 = 1 << 5;

/// 115200 baudios con el reloj de 48 MHz que pone el firmware de la Pi 4.
pub fn iniciar() {
    CR.escribir(0);
    ICR.escribir(0x7ff);
    IBRD.escribir(26);
    FBRD.escribir(3);
    LCRH.escribir((1 << 4) | (3 << 5)); // FIFO y 8 bits
    CR.escribir((1 << 0) | (1 << 8) | (1 << 9)); // UART, TX y RX
}

pub fn escribir_byte(b: u8) {
    while FR.leer() & FR_TXFF != 0 {}
    DR.escribir(b as u32);
}

pub struct Serie;

impl fmt::Write for Serie {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for b in s.bytes() {
            if b == b'\n' {
                escribir_byte(b'\r');
            }
            escribir_byte(b);
        }
        Ok(())
    }
}

/// Un cerrojo para que las líneas de varios núcleos no se mezclen (C9). No se imprime desde interrupciones.
static CERROJO: crate::cerrojo::Cerrojo<()> = crate::cerrojo::Cerrojo::new(());

pub fn imprimir(args: fmt::Arguments) {
    let _g = CERROJO.tomar();
    let _ = fmt::Write::write_fmt(&mut Serie, args);
}

#[macro_export]
macro_rules! kprint {
    ($($arg:tt)*) => ($crate::serie::imprimir(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! kprintln {
    () => ($crate::kprint!("\n"));
    ($($arg:tt)*) => ($crate::kprint!("{}\n", format_args!($($arg)*)));
}
