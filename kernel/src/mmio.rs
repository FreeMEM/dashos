//! Registros de periférico.
//!
//! Los periféricos de la Pi son little-endian y la CPU de DashOS es big-endian (S1). Todo acceso a un
//! registro pasa por aquí, que da la vuelta a los bytes. Nunca se lee un registro con un puntero suelto.

use core::ptr::{read_volatile, write_volatile};

/// Registro de 32 bits little-endian en una dirección fija.
#[derive(Clone, Copy)]
pub struct RegLe32(usize);

impl RegLe32 {
    pub const fn new(direccion: usize) -> Self {
        Self(direccion)
    }

    #[inline]
    pub fn leer(self) -> u32 {
        u32::from_le(unsafe { read_volatile(self.0 as *const u32) })
    }

    #[inline]
    pub fn escribir(self, valor: u32) {
        unsafe { write_volatile(self.0 as *mut u32, valor.to_le()) }
    }
}
