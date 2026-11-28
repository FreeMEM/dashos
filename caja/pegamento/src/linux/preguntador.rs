//! La caja Amiga en Linux (docs/CAJA_LINUX_LOOP.md): sin `sesiones` ni `guardian` a quien preguntar por los ficheros
//! protegidos. Lo que la guardia no deja cambiar, no se cambia.

use caja_dos::guardia::{Clase, Preguntador};

/// Lo que dice `cuenta` cuando no se ha preguntado nada (como en DashOS).
pub const NADA: u32 = 0xff;

pub fn reiniciar_cuenta() {}

pub fn cuenta() -> (u32, u32) {
    (0, NADA)
}

pub struct PorSesiones;

impl Preguntador for PorSesiones {
    fn preguntar(&mut self, _programa: &str, _clase: Clase, _nombre: &str) -> bool {
        false
    }
}

pub fn apuntar(_que: &str, _motivo: &str) {}
