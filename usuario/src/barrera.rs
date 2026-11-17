//! Barreras de memoria para los controladores que comparten memoria con un aparato por DMA (X5): lo que cambia de una
//! arquitectura a otra. Los registros y la memoria DMA se tocan con `read_volatile` y `write_volatile`, en
//! little-endian, igual en las dos.
//!
//! - ARM: `dmb` con dominio exterior (`osh`), que es el que comparten la CPU y los aparatos; `dsb sy` para esperar a
//!   que todo lo anterior acabe antes de tocar un registro.
//! - x86-64: la memoria normal (WB) ya guarda el orden entre escrituras y entre lecturas, y una escritura a un registro
//!   sin caché (UC) no adelanta a las de antes (SDM, volumen 3, 8.2.2). Basta con que el compilador no las mueva; solo
//!   una escritura seguida de una lectura necesita `mfence`.

#[cfg(target_arch = "x86_64")]
use core::sync::atomic::{compiler_fence, Ordering};

/// Las escrituras de antes se ven antes que las de después (en el aparato).
#[inline(always)]
pub fn escrituras() {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("dmb oshst", options(nostack, preserves_flags))
    };
    #[cfg(target_arch = "x86_64")]
    compiler_fence(Ordering::SeqCst);
}

/// Las lecturas de antes se hacen antes que las de después (lo que escribió el aparato).
#[inline(always)]
pub fn lecturas() {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("dmb oshld", options(nostack, preserves_flags))
    };
    #[cfg(target_arch = "x86_64")]
    compiler_fence(Ordering::SeqCst);
}

/// Todo lo de antes (lecturas y escrituras) antes que todo lo de después.
#[inline(always)]
pub fn total() {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("dmb osh", options(nostack, preserves_flags))
    };
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("mfence", options(nostack, preserves_flags))
    };
}

/// Espera a que acabe todo lo de antes (también en el aparato) antes de seguir: antes de un timbre.
#[inline(always)]
pub fn completa() {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("dsb sy", options(nostack, preserves_flags))
    };
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("mfence", options(nostack, preserves_flags))
    };
}

/// Lo escrito en una pantalla con combinación de escritura (x86: WC, que junta las escrituras en búferes propios de la
/// CPU) sale ya hacia ella, y antes que lo de después (`sfence`). En ARM la pantalla es memoria normal sin caché: basta
/// con el orden de las escrituras.
#[inline(always)]
pub fn pantalla() {
    #[cfg(target_arch = "aarch64")]
    escrituras();
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("sfence", options(nostack, preserves_flags))
    };
}
