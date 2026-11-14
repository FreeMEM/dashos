//! La salida del kernel en x86-64 (S21, X2): `kprint!` y `kprintln!` por COM1, como `serie.rs` en ARM.
//!
//! Un cerrojo para que las líneas de dos núcleos no se mezclen (X3). El pánico y las excepciones escriben directo
//! (`base::serie::Serie`), sin él: pueden llegar con el cerrojo tomado.

use crate::cerrojo::Cerrojo;
use core::fmt;

static SALIDA: Cerrojo<()> = Cerrojo::new(());

pub fn imprimir(args: fmt::Arguments) {
    let _g = SALIDA.tomar();
    let _ = fmt::Write::write_fmt(&mut crate::base::serie::Serie, args);
}

/// Directo al puerto, sin cerrojo: para el pánico.
pub fn imprimir_directo(args: fmt::Arguments) {
    let _ = fmt::Write::write_fmt(&mut crate::base::serie::Serie, args);
}

#[macro_export]
macro_rules! kprint {
    ($($arg:tt)*) => ($crate::serie::imprimir(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! kprintln {
    () => ($crate::kprint!("\n"));
    ($($arg:tt)*) => ($crate::kprint!("{}\n", format_args!($($arg)*)));
}
