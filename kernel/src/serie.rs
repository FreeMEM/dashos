//! Puerto serie PL011 (UART0) de la Pi 4.

use crate::mmio::RegLe32;
use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};

/// Dirección del PL011 con los periféricos en modo bajo. En C4 se leerá del árbol de dispositivos.
const BASE: usize = crate::mmu::virt(crate::placa::SERIE);

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

/// Espera a que haya hueco, pero no para siempre: el controlador serie de EL0 comparte este PL011 y podría apagarlo.
/// Entonces el byte se pierde y el kernel sigue (auditoría, hallazgo 9). A 115200 baudios un byte tarda unos 87 µs;
/// el límite da para varios.
///
/// El plazo es para la consola, no para cada byte: tras agotarlo una vez, la consola se da por muerta y los bytes se
/// tiran sin esperar hasta que vuelva a tener hueco. Si no, un texto largo esperaba el plazo en cada byte, minutos con
/// las IRQ enmascaradas (auditoría de cierre de la fase 1, N2).
pub fn escribir_byte(b: u8) {
    if MUERTA.load(Ordering::Relaxed) {
        if FR.leer() & FR_TXFF != 0 {
            return;
        }
        MUERTA.store(false, Ordering::Relaxed);
    }
    let mut vueltas = 0u32;
    while FR.leer() & FR_TXFF != 0 {
        vueltas += 1;
        if vueltas > 200_000 {
            MUERTA.store(true, Ordering::Relaxed);
            return;
        }
    }
    DR.escribir(b as u32);
}

static MUERTA: AtomicBool = AtomicBool::new(false);

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

/// Un cerrojo para que las líneas de varios núcleos no se mezclen (C9). Enmascara las IRQ: así se puede imprimir desde
/// una interrupción sin colgar el núcleo cuando el hilo interrumpido estaba imprimiendo (auditoría, bajos).
static CERROJO: crate::cerrojo::CerrojoIrq<()> = crate::cerrojo::CerrojoIrq::new(());

/// Escribe en el registro (C48) y, mientras la consola no lo haya tomado, también directo al PL011.
pub fn imprimir(args: fmt::Arguments) {
    let _g = CERROJO.tomar();
    let _ = fmt::Write::write_fmt(&mut AlRegistro, args);
    if crate::registro::directo() {
        let _ = fmt::Write::write_fmt(&mut Serie, args);
    }
}

/// Directo al PL011, aunque la consola haya tomado el registro: para el pánico, que puede ser lo último que se vea.
pub fn imprimir_directo(args: fmt::Arguments) {
    let _ = fmt::Write::write_fmt(&mut Serie, args);
}

struct AlRegistro;

impl fmt::Write for AlRegistro {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        crate::registro::poner(s.as_bytes());
        Ok(())
    }
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
