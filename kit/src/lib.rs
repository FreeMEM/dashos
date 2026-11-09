//! El kit de interfaz de DashOS (S11): el modelo de MUI (objetos con atributos y avisos, grupos que se colocan
//! solos) con el aspecto de Material 3 (S10). Dibuja en memoria; la ventana de Wayland va aparte (K7).

#![no_std]

extern crate alloc;

pub mod controles;
pub mod grupos;
pub mod lienzo;
pub mod objetos;
pub mod color;
pub mod tema;
pub mod teclado;
pub mod texto;
