//! Memoria virtual del kernel.
//!
//! arranque.s enciende la MMU con la memoria física entera (los primeros 4 GB por ahora) vista a partir de
//! `KERNEL_VA`, y entra en Rust ya en direcciones altas. Todo acceso del kernel a una dirección física (memoria o
//! periférico) pasa por `virt()`.

/// Inicio de la mitad alta: 39 bits de dirección virtual (T1SZ = 25).
pub const KERNEL_VA: usize = 0xFFFF_FF80_0000_0000;

/// Dirección virtual del kernel para una dirección física.
#[inline]
pub const fn virt(fisica: usize) -> usize {
    fisica + KERNEL_VA
}

/// Dirección física de una dirección virtual del kernel.
#[inline]
pub const fn fisica(virtual_: usize) -> usize {
    virtual_ - KERNEL_VA
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
