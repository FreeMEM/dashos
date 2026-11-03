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
    if suma == 5050 && primero == 0x11 { 5 } else { 1 }
}
