//! Los cuatro núcleos (C9).
//!
//! El firmware de la Pi (y QEMU) deja los núcleos 1 a 3 esperando en la tabla de espera ("spin table"): cada uno
//! lee una dirección de 64 bits en 0xe0, 0xe8 y 0xf0 y salta a ella cuando deja de ser cero. Esos núcleos
//! siguen en little-endian y sin MMU, así que la dirección se escribe en little-endian, se saca de la caché y se
//! avisa con `sev`.

use crate::{excepciones, gic, kprintln, mmu};
use core::sync::atomic::{AtomicU32, Ordering};

static EN_MARCHA: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" {
    static _secundario: u8;
}

pub fn numero() -> u32 {
    let mpidr: u64;
    unsafe { core::arch::asm!("mrs {}, mpidr_el1", out(reg) mpidr) };
    (mpidr & 0xff) as u32
}

/// Despierta los núcleos 1 a 3 y espera a que se presenten. Devuelve cuántos lo hicieron.
pub fn despertar() -> u32 {
    let entrada = mmu::fisica(unsafe { &_secundario as *const u8 as usize }) as u64;
    for n in 1..4u64 {
        let ranura = mmu::virt(0xd8 + 8 * n as usize) as *mut u64;
        unsafe {
            ranura.write_volatile(entrada.to_le());
            core::arch::asm!("dc civac, {}", in(reg) ranura);
        }
    }
    unsafe { core::arch::asm!("dsb sy", "sev") };
    let inicio = crate::reloj::ahora();
    let limite = crate::reloj::frecuencia(); // un segundo
    while EN_MARCHA.load(Ordering::Acquire) < 3 && crate::reloj::ahora() - inicio < limite {
        core::hint::spin_loop();
    }
    EN_MARCHA.load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
extern "C" fn kmain_secundario(nucleo: u64) -> ! {
    excepciones::iniciar();
    gic::iniciar_cpu();
    crate::planificador::iniciar_nucleo();
    crate::reloj::iniciar_nucleo();
    kprintln!("  núcleo {} en marcha (MPIDR {})", nucleo, numero());
    EN_MARCHA.fetch_add(1, Ordering::Release);
    crate::planificador::ocioso();
}
