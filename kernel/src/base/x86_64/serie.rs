//! El puerto serie COM1 de un PC: una UART 16550 en el puerto de E/S 0x3F8.
//!
//! Escrito con la hoja de datos de la 16550 (registros THR, IER, IIR, FCR, LCR, MCR, LSR y el divisor con DLAB). Solo
//! escribe, sin interrupciones salvo la de la prueba del IOAPIC (`aviso_de_hueco`). El puerto es fijo: nadie de fuera
//! elige a qué puerto se escribe.

use core::fmt;

const COM1: u16 = 0x3f8;
const IER: u16 = COM1 + 1;
const FCR: u16 = COM1 + 2;
const IIR: u16 = COM1 + 2;
const LCR: u16 = COM1 + 3;
const MCR: u16 = COM1 + 4;
const LSR: u16 = COM1 + 5;
/// LSR: el registro de transmisión está vacío.
const LSR_THRE: u8 = 1 << 5;

fn salida(puerto: u16, v: u8) {
    unsafe { core::arch::asm!("out dx, al", in("dx") puerto, in("al") v, options(nomem, nostack, preserves_flags)) };
}

fn entrada(puerto: u16) -> u8 {
    let v: u8;
    unsafe { core::arch::asm!("in al, dx", out("al") v, in("dx") puerto, options(nomem, nostack, preserves_flags)) };
    v
}

/// 115200 baudios, 8 bits, sin paridad, un bit de parada; las FIFO encendidas y sin interrupciones.
pub fn iniciar() {
    salida(IER, 0);
    salida(LCR, 0x80); // DLAB: los dos primeros registros pasan a ser el divisor
    salida(COM1, 1); // 115200 = 1843200 / 16 / 1
    salida(COM1 + 1, 0);
    salida(LCR, 0x03); // 8N1, DLAB fuera
    salida(FCR, 0xc7); // FIFO encendidas y vacías, aviso a 14 bytes
    salida(MCR, 0x03); // DTR y RTS; OUT2 apagado: la UART no pide interrupciones
}

/// Pide (o deja de pedir) la interrupción de "registro de transmisión vacío" (IER bit 1, con OUT2 en MCR, que en un PC
/// une la UART a su línea, la IRQ 4). Para probar el IOAPIC (X3).
pub fn aviso_de_hueco(encender: bool) {
    salida(MCR, if encender { 0x0b } else { 0x03 });
    salida(IER, if encender { 0x02 } else { 0 });
}

/// Lee el IIR, que reconoce la interrupción de la UART: bit 0 a 1 si no había ninguna; bits 3:1, el motivo (1, hueco
/// en la transmisión).
pub fn reconocer() -> u8 {
    entrada(IIR)
}

/// Escribe un byte. Espera hueco, pero no para siempre: si no hay UART (un PC sin COM1), el byte se pierde.
pub fn byte(b: u8) {
    for _ in 0..100_000 {
        if entrada(LSR) & LSR_THRE != 0 {
            break;
        }
        core::hint::spin_loop();
    }
    salida(COM1, b);
}

/// Para `write!`: pasa "\n" a "\r\n".
pub struct Serie;

impl fmt::Write for Serie {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for b in s.bytes() {
            if b == b'\n' {
                byte(b'\r');
            }
            byte(b);
        }
        Ok(())
    }
}
