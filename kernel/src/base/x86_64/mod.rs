//! Todo lo inseguro del kernel en x86-64 (S21). Mismas reglas que `base` en ARM (`../mod.rs`): cada función pública
//! es segura de llamar con cualquier argumento, o es `unsafe fn`.
//!
//! - X1: la entrada desde el cargador UEFI (`arranque.s`), lo que deja el cargador (`arranque.rs`), el puerto serie
//!   COM1 (`serie.rs`) y la CPU (`cpu.rs`).
//! - X2: GDT, TSS e IDT (`excepciones.rs`, `vectores.s`), las tablas de páginas propias (`mmu.rs`, `mapa.rs`) y las
//!   defensas (SMEP, SMAP, NX). El repartidor de páginas, el montón y los cerrojos son los mismos ficheros que en ARM.
//!
//! Los núcleos, el APIC y el temporizador llegan en X3; los espacios de los procesos (`espacio`), en X4.

pub mod arranque;
#[path = "../cerrojo.rs"]
pub mod cerrojo;
pub mod cpu;
pub mod excepciones;
pub mod mapa;
#[path = "../memoria.rs"]
pub mod memoria;
pub mod mmu;
#[path = "../monton.rs"]
pub mod monton;
pub mod serie;

core::arch::global_asm!(include_str!("arranque.s"));

/// Entrada desde arranque.s, ya en la mitad alta, con la dirección física de la `InfoArranque` que dejó el cargador.
#[unsafe(no_mangle)]
extern "C" fn kmain_x86(info: usize) -> ! {
    arranque::guardar(info);
    crate::kmain()
}
