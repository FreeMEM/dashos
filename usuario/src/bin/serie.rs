//! C20: un controlador fuera del kernel. Maneja el puerto serie PL011 con sus registros proyectados y recibe su
//! interrupción como mensaje.
//!
//! El kernel le da dos llaves al arrancar: la 1, los registros del PL011; la 2, su interrupción.

#![no_std]
#![no_main]

use dashos::println;

dashos::programa!(principal);

const LLAVE_REGISTROS: u32 = 1;
const LLAVE_IRQ: u32 = 2;
const BASE: usize = 0x0090_0000;

// Registros del PL011 (desplazamientos públicos del manual de ARM).
const DR: usize = 0x00;
const FR: usize = 0x18;
const IMSC: usize = 0x38;
const MIS: usize = 0x40;
const ICR: usize = 0x44;
const FR_TXFF: u32 = 1 << 5;
const INT_TX: u32 = 1 << 5;

// Los registros son little-endian; el proceso, big-endian.
fn leer(off: usize) -> u32 {
    u32::from_le(unsafe { ((BASE + off) as *const u32).read_volatile() })
}
fn escribir(off: usize, v: u32) {
    unsafe { ((BASE + off) as *mut u32).write_volatile(v.to_le()) }
}

fn poner(texto: &str) {
    for b in texto.bytes() {
        while leer(FR) & FR_TXFF != 0 {}
        escribir(DR, b as u32);
    }
}

fn principal() -> u64 {
    if dashos::registros_proyectar(LLAVE_REGISTROS, BASE) != 0 {
        return 1;
    }
    poner("  [serie] escrito por un controlador en EL0, sin pasar por el kernel\n");

    let (a, b) = dashos::canal_crear();
    if dashos::interrupcion_asociar(LLAVE_IRQ, a) != 0 {
        return 2;
    }
    // Pedir la interrupción de "cola de envío vacía": llega en cuanto el aparato no tiene nada que mandar.
    escribir(IMSC, leer(IMSC) | INT_TX);
    let mut bufer = [0u8; 4];
    let (n, _) = dashos::canal_leer(b, &mut bufer, &mut []);
    let mis = leer(MIS);
    // Atenderla en el aparato y devolverla al kernel.
    escribir(IMSC, leer(IMSC) & !INT_TX);
    escribir(ICR, INT_TX);
    dashos::interrupcion_terminar(LLAVE_IRQ);
    if n != 4 {
        return 3;
    }
    let intid = u32::from_le_bytes(bufer); // lo que escribe el kernel va en little-endian (C30)
    println!("  [serie] interrupción {} recibida como mensaje (MIS={:#x})", intid, mis);
    if mis & INT_TX != 0 { 7 } else { 4 }
}
