//! El kit de interfaz de DashOS (S11): el modelo de MUI (objetos con atributos y avisos, grupos que se colocan
//! solos) con el aspecto de Material 3 (S10). Dibuja en memoria; la ventana de Wayland va aparte (K7).

#![no_std]

extern crate alloc;

pub mod controles;
pub mod grupos;
pub mod lienzo;
pub mod menus;
pub mod objetos;
pub mod color;
pub mod fondos;
pub mod iconos;
pub mod imagen;
pub mod png;
pub mod semilla;
pub mod tema;
pub mod teclado;
pub mod texto;
pub mod textos;
mod trazos;

/// Idiomas y catálogos (el crate `idioma`), para los programas que usan el kit.
pub use idioma;
