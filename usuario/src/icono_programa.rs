//! S27: el icono de un programa, para el dock (y luego el lanzador). Es el punto donde enchufar la colección propia de
//! iconos y la de las cajas (las ramas `iconos` e `iconos-cajas`): hoy no hay ninguna, así que contesta que no hay, y
//! quien lo pide pinta la letra, como DMS cuando un programa no trae icono.

extern crate alloc;

use alloc::vec::Vec;

/// Un icono cuadrado de `lado` × `lado`, en ARGB premultiplicado, fila a fila.
pub struct Icono {
    pub lado: u32,
    pub px: Vec<u32>,
}

/// El icono de `programa` (su nombre en `Programas/`, o "linux:..." y "amiga:..." para los de las cajas) a `lado`
/// píxeles. `None` si no tiene.
pub fn icono(programa: &str, lado: u32) -> Option<Icono> {
    let _ = (programa, lado);
    None
}
