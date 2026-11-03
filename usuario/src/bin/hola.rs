//! C19: el primer programa de DashOS escrito en Rust.

#![no_std]
#![no_main]

use dashos::println;

dashos::programa!(principal);

fn principal() -> u64 {
    println!("  hola desde Rust en EL0 (DashOS, big-endian)");
    let suma: u64 = (1..=100u64).sum();
    println!("  la suma de 1 a 100 es {}", suma);
    // Un objeto de memoria propio, proyectado y usado.
    let m = dashos::memoria_crear(4096);
    let r = dashos::memoria_proyectar(m, 0x0080_0000, true);
    let p = 0x0080_0000 as *mut u32;
    unsafe { p.write_volatile(0x1122_3344) };
    let primero = unsafe { *(0x0080_0000 as *const u8) };
    println!("  memoria propia proyectada ({}); primer byte de 0x11223344: {:#x}", r, primero);
    // Usar 40 KB de pila: con la pila de 4 KB de antes esto fallaba (la pide Dash).
    let mut grande = [0u8; 40 * 1024];
    for (i, b) in grande.iter_mut().enumerate() {
        *b = i as u8;
    }
    let total: u64 = core::hint::black_box(&grande).iter().map(|&b| b as u64).sum();
    println!("  40 KB en la pila: suma {}", total);
    if suma == 5050 && primero == 0x11 && total == 5_222_400 { 5 } else { 1 }
}
