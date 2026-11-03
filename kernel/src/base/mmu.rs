//! Memoria virtual del kernel.
//!
//! arranque.s enciende la MMU con la memoria física entera (los primeros 4 GB por ahora) vista a partir de
//! `KERNEL_VA`, y entra en Rust ya en direcciones altas. Todo acceso del kernel a una dirección física (memoria o
//! periférico) pasa por `virt()`.

use core::sync::atomic::{AtomicUsize, Ordering};

/// Inicio de la mitad alta: 39 bits de dirección virtual (T1SZ = 25).
pub const KERNEL_VA: usize = 0xFFFF_FF80_0000_0000;

/// Dirección virtual del kernel para una dirección física.
#[inline]
pub const fn virt(fisica: usize) -> usize {
    fisica + KERNEL_VA
}

/// Dirección física de una dirección virtual del kernel. La imagen del kernel se ve en `KERNEL_VA + 0x80000`, donde
/// la cargó el firmware; desde C38 está en otro sitio, por encima del primer GB (mapa.rs), y sus direcciones se
/// traducen con `IMAGEN`. Lo demás es el mapa lineal.
#[inline]
pub fn fisica(virtual_: usize) -> usize {
    let p = IMAGEN.load(Ordering::Relaxed);
    if p != 0 {
        let (inicio, fin) = imagen_va();
        if (inicio..fin).contains(&virtual_) {
            return virtual_ - inicio + p;
        }
    }
    virtual_ - KERNEL_VA
}

/// Física del primer byte de la imagen tras el traslado (C38), o 0 si sigue donde la cargó el firmware.
static IMAGEN: AtomicUsize = AtomicUsize::new(0);

pub(in crate::base) fn imagen_trasladada(fisica: usize) {
    IMAGEN.store(fisica, Ordering::Relaxed);
}

/// Direcciones virtuales del principio y el final de la imagen (con `.bss`, tablas y pilas de arranque).
pub fn imagen_va() -> (usize, usize) {
    unsafe extern "C" {
        static __kernel_inicio: u8;
        static __kernel_fin: u8;
    }
    unsafe { (&__kernel_inicio as *const u8 as usize, &__kernel_fin as *const u8 as usize) }
}

/// Estado de la MMU, para la prueba.
pub fn estado() -> (u64, u64, u64) {
    let (sctlr, tcr, ttbr1): (u64, u64, u64);
    unsafe {
        core::arch::asm!("mrs {}, sctlr_el1", out(reg) sctlr);
        core::arch::asm!("mrs {}, tcr_el1", out(reg) tcr);
        core::arch::asm!("mrs {}, ttbr1_el1", out(reg) ttbr1);
    }
    (sctlr, tcr, ttbr1)
}
