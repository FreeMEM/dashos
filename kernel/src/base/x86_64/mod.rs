//! Todo lo inseguro del kernel en x86-64 (S21, X1). Mismas reglas que `base` en ARM (`../mod.rs`): cada función
//! pública es segura de llamar con cualquier argumento, o es `unsafe fn`.
//!
//! De momento solo el arranque: la entrada desde el cargador UEFI (`arranque.s`), lo que deja el cargador
//! (`arranque.rs`), el puerto serie COM1 (`serie.rs`) y parar la CPU. Corre con las tablas de páginas del firmware
//! (memoria física 1:1). La MMU propia, las excepciones y los núcleos llegan en X2 y X3.

pub mod arranque;
pub mod cpu;
pub mod serie;

core::arch::global_asm!(include_str!("arranque.s"));

/// Entrada desde arranque.s, con la dirección de la `InfoArranque` que dejó el cargador.
#[unsafe(no_mangle)]
extern "C" fn kmain_x86(info: usize) -> ! {
    arranque::guardar(info);
    crate::kmain()
}
