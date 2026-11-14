//! Memoria virtual del kernel en x86-64 (S21, X2). La misma interfaz que `mmu.rs` de ARM.
//!
//! El reparto de la mitad alta es el de ARM (39 bits, `KERNEL_VA`), dentro de la última entrada del PML4:
//!
//! - el mapa lineal: la memoria física desde `KERNEL_VA` (hasta 256 GB);
//! - las pilas de los hilos: el GB 256 (X3);
//! - la imagen del kernel: en los 2 GB de arriba (`IMAGEN_VA` + física), donde la quiere el modelo de código
//!   "kernel" de x86-64. Es como la imagen trasladada de ARM (C38): sus páginas no se ven por el mapa lineal.
//!
//! La mitad baja (PML4[0..256]) queda para los procesos (X4).

/// Inicio de la mitad alta, el mismo que en ARM: PML4[511].
pub const KERNEL_VA: usize = 0xFFFF_FF80_0000_0000;
/// Dónde se ve la imagen del kernel: `IMAGEN_VA` + física (enlace_x86.ld).
pub const IMAGEN_VA: usize = 0xFFFF_FFFF_8000_0000;

/// Dirección virtual del kernel para una dirección física (mapa lineal).
#[inline]
pub const fn virt(fisica: usize) -> usize {
    fisica + KERNEL_VA
}

/// Dirección física de una dirección virtual del kernel: de la imagen o del mapa lineal.
#[inline]
pub fn fisica(virtual_: usize) -> usize {
    if virtual_ >= IMAGEN_VA { virtual_ - IMAGEN_VA } else { virtual_ - KERNEL_VA }
}

/// Direcciones virtuales del principio y el final de la imagen (con `.bss`, tablas y pilas de arranque).
pub fn imagen_va() -> (usize, usize) {
    unsafe extern "C" {
        static __kernel_inicio: u8;
        static __kernel_fin: u8;
    }
    unsafe { (&__kernel_inicio as *const u8 as usize, &__kernel_fin as *const u8 as usize) }
}

/// Estado de la MMU, para las pruebas: CR0, CR3, CR4 y EFER.
pub fn estado() -> (u64, u64, u64, u64) {
    use super::cpu;
    (cpu::cr0(), cpu::cr3(), cpu::cr4(), cpu::leer_msr(cpu::EFER))
}
