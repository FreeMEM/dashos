//! C32: dos copias en el mismo núcleo. Cada una pone su TPIDR_EL0 y comprueba durante 300 ms que nadie se lo
//! cambia; al empezar, que no ve el de otro. Antes, TPIDR_EL0 no se guardaba por hilo (auditoría, hallazgo 5).
//! Usa el contador virtual, que es lo único del temporizador que EL0 puede leer (hallazgo 16). Sale con 32.

#![no_std]
#![no_main]

use core::arch::asm;

dashos::programa!(principal);

fn tpidr() -> u64 {
    let v: u64;
    unsafe { asm!("mrs {}, tpidr_el0", out(reg) v) };
    v
}

fn contador() -> (u64, u64) {
    let (v, f): (u64, u64);
    unsafe { asm!("isb", "mrs {}, cntvct_el0", "mrs {}, cntfrq_el0", out(reg) v, out(reg) f) };
    (v, f)
}

fn principal() -> u64 {
    if tpidr() != 0 {
        return 1;
    }
    let ro: u64;
    unsafe { asm!("mrs {}, tpidrro_el0", out(reg) ro) };
    if ro != 0 {
        return 2;
    }
    let (ahora, frecuencia) = contador();
    let mio = ahora | 1;
    unsafe { asm!("msr tpidr_el0, {}", in(reg) mio) };
    let fin = ahora + frecuencia * 3 / 10;
    while contador().0 < fin {
        if tpidr() != mio {
            return 3;
        }
    }
    32
}
