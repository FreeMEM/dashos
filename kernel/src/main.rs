//! DashOS: el kernel.

#![no_std]
#![no_main]

mod excepciones;
mod mmio;
mod serie;

use core::panic::PanicInfo;

core::arch::global_asm!(include_str!("arranque.s"));

fn nivel_excepcion() -> u64 {
    let el: u64;
    unsafe { core::arch::asm!("mrs {}, CurrentEL", out(reg) el) };
    (el >> 2) & 3
}

#[unsafe(no_mangle)]
extern "C" fn kmain(dtb: usize) -> ! {
    serie::iniciar();
    kprintln!();
    kprintln!("DashOS {}", env!("CARGO_PKG_VERSION"));
    kprintln!("Nivel de excepción: EL{}", nivel_excepcion());
    kprintln!("Árbol de dispositivos en {:#x}", dtb);
    let x: u32 = 0x1122_3344;
    let primero = unsafe { *(&x as *const u32 as *const u8) };
    kprintln!("Orden de bytes: {}", if primero == 0x11 { "big-endian" } else { "little-endian" });
    kprintln!("C2: esqueleto en marcha");

    excepciones::iniciar();
    unsafe { core::arch::asm!("brk #0x2a") };
    kprintln!("C3: de vuelta de la excepción");
    loop {
        unsafe { core::arch::asm!("wfe") };
    }
}

#[panic_handler]
fn panico(info: &PanicInfo) -> ! {
    kprintln!("PÁNICO: {}", info);
    loop {
        unsafe { core::arch::asm!("wfe") };
    }
}
