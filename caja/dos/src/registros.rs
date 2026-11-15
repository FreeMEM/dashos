//! Los registros del 68k con que llega cada función. Misma forma que `Registros` de exec (rama `caja-amiga-exec`):
//! la caja los saca de la copia del JIT una vez y se los pasa a la biblioteca que toque.

/// D0-D7 y A0-A7 de una tarea 68k.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Registros {
    pub d: [u32; 8],
    pub a: [u32; 8],
}
