//! Prueba S1: un kernel mínimo en Rust para AArch64 big-endian que escribe por el
//! puerto serie de la Pi 4 (PL011) en QEMU raspi4b y lee el orden de bytes de la CPU.
#![no_std]
#![no_main]

use core::arch::global_asm;
use core::panic::PanicInfo;

// La Pi 4 en modo de periféricos bajos: PL011 (UART0) en 0xFE201000.
const UART0: usize = 0xFE20_1000;

// El firmware de la Pi (y QEMU) entra en EL2 con la MMU apagada y en little-endian.
// Ponemos big-endian en EL2 (SCTLR_EL2.EE, bit 25) antes de tocar memoria con datos.
global_asm!(
    ".section .text.boot",
    ".globl _start",
    "_start:",
    "  mrs  x1, mpidr_el1",
    "  and  x1, x1, #3",
    "  cbnz x1, 2f",
    "  mrs  x1, sctlr_el2",
    "  orr  x1, x1, #(1 << 25)",
    "  msr  sctlr_el2, x1",
    "  isb",
    "  ldr  x1, =__pila",
    "  mov  sp, x1",
    "  bl   inicio",
    "2: wfe",
    "  b    2b",
);

fn escribe(b: u8) {
    unsafe {
        while core::ptr::read_volatile((UART0 + 0x18) as *const u32) & (1 << 5) != 0 {}
        // El PL011 es little-endian: con la CPU en big-endian hay que darle la vuelta al dato.
        core::ptr::write_volatile(UART0 as *mut u32, (b as u32).swap_bytes());
    }
}

fn texto(s: &str) {
    for b in s.bytes() {
        if b == b'\n' { escribe(b'\r'); }
        escribe(b);
    }
}

fn hex(v: u64) {
    for i in (0..16).rev() {
        let d = ((v >> (i * 4)) & 0xf) as u8;
        escribe(if d < 10 { b'0' + d } else { b'a' + d - 10 });
    }
}

#[unsafe(no_mangle)]
extern "C" fn inicio() -> ! {
    texto("DashOS: Rust en AArch64 big-endian\n");
    let x: u32 = 0x1122_3344;
    let primer = unsafe { *(&x as *const u32 as *const u8) };
    texto("Primer byte de 0x11223344 en memoria: ");
    hex(primer as u64);
    texto(if primer == 0x11 { " (big-endian)\n" } else { " (little-endian)\n" });
    let mmfr0: u64;
    unsafe { core::arch::asm!("mrs {}, id_aa64mmfr0_el1", out(reg) mmfr0) };
    texto("ID_AA64MMFR0_EL1 = ");
    hex(mmfr0);
    texto("\nBigEnd (bits 11:8) = ");
    hex((mmfr0 >> 8) & 0xf);
    texto(", BigEndEL0 (bits 19:16) = ");
    hex((mmfr0 >> 16) & 0xf);
    texto("\n");
    // S7: ¿se puede poner little-endian solo para EL0 (SCTLR_EL1.E0E, bit 24)?
    let antes: u64;
    let despues: u64;
    unsafe {
        core::arch::asm!("mrs {}, sctlr_el1", out(reg) antes);
        core::arch::asm!("msr sctlr_el1, {}", "isb", in(reg) antes | (1 << 24));
        core::arch::asm!("mrs {}, sctlr_el1", out(reg) despues);
        core::arch::asm!("msr sctlr_el1, {}", "isb", in(reg) despues & !(1 << 24));
    }
    texto("SCTLR_EL1.E0E: antes ");
    hex((antes >> 24) & 1);
    texto(", tras ponerlo a 1 se lee ");
    hex((despues >> 24) & 1);
    texto(" (EE, bit 25 = ");
    hex((despues >> 25) & 1);
    texto(")\n");
    loop { unsafe { core::arch::asm!("wfe") } }
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }
