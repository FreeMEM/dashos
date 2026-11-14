//! C32: dos copias en el mismo núcleo. Cada una pone su TPIDR_EL0 y comprueba durante 300 ms que nadie se lo
//! cambia; al empezar, que no ve el de otro. Antes, TPIDR_EL0 no se guardaba por hilo (auditoría, hallazgo 5).
//! Usa el contador virtual, que es lo único del temporizador que EL0 puede leer (hallazgo 16). Sale con 32.
//! En x86-64 (X4), lo mismo con las bases de FS y GS.

#![no_std]
#![no_main]

use core::arch::asm;

dashos::programa!(principal);

#[cfg(target_arch = "aarch64")]
fn tpidr() -> u64 {
    let v: u64;
    unsafe { asm!("mrs {}, tpidr_el0", out(reg) v) };
    v
}

fn contador() -> (u64, u64) {
    (dashos::contador(), dashos::frecuencia())
}

#[cfg(target_arch = "aarch64")]
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

/// x86-64 (X4): la base de FS es el TLS, como TPIDR_EL0 (con FSGSBASE, el proceso la lee y la pone). La de GS también
/// es suya: el kernel usa la propia con `swapgs`, y la del proceso tiene que seguir igual tras cada interrupción.
#[cfg(target_arch = "x86_64")]
fn bases() -> (u64, u64) {
    let (fs, gs): (u64, u64);
    unsafe { asm!("rdfsbase {}", "rdgsbase {}", out(reg) fs, out(reg) gs, options(nostack, nomem)) };
    (fs, gs)
}

#[cfg(target_arch = "x86_64")]
fn principal() -> u64 {
    if bases() != (0, 0) {
        return 1;
    }
    let (ahora, frecuencia) = contador();
    // Direcciones canónicas de la mitad baja (si no, wrfsbase falla), distintas en cada copia.
    let mio = ((ahora & 0x7f_ffff_fff0) | 0x10, (ahora & 0x7f_ffff_fff0) | 0x20);
    unsafe { asm!("wrfsbase {}", "wrgsbase {}", in(reg) mio.0, in(reg) mio.1, options(nostack, nomem)) };
    let fin = ahora + frecuencia * 3 / 10;
    while contador().0 < fin {
        if bases() != mio {
            return 3;
        }
    }
    // Una llamada al sistema con las bases puestas: el kernel entra y sale con las suyas.
    dashos::escribir("");
    if bases() != mio {
        return 4;
    }
    32
}
