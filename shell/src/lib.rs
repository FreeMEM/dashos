//! El Shell nativo de DashOS, al estilo del de AmigaOS (`docs/bitacoras/shell-nativa.md`): lo que no depende del
//! sistema, para probarlo en el PC con `cargo test`.
//!
//! - `patron`: los patrones de AmigaDOS (`#?`, `?`, `(a|b)`, `~`, `[a-z]`) y cómo se expanden sobre un cajón.
//! - `plantilla`: las plantillas de `ReadArgs` (`FROM/A,TO/K,ALL/S`), para las órdenes.
//! - `linea`: una línea del Shell: comillas, variables, redirecciones y tuberías.
//! - `ruta`: rutas del Amiga (`VOL:cajón/fichero`, `/` sube) sobre un cajón actual.
//! - `interprete`: los guiones (`If`, `Skip`, `Lab`, `FailAt`, `Execute`, variables) como una máquina de estados.
//! - `consola`: el texto de la ventana (líneas, desplazamiento, selección) y el editor de la línea, con su historial.
//! - `protocolo`: lo que hablan una orden y el Shell que la lanza.

#![no_std]

extern crate alloc;

pub mod consola;
pub mod fecha;
pub mod completar;
pub mod historial;
pub mod indicador;
pub mod interprete;
pub mod linea;
pub mod ordenes;
pub mod paneles;
pub mod patron;
pub mod perfil;
pub mod plantilla;
pub mod protocolo;
pub mod resaltar;
pub mod ruta;

/// Los códigos de retorno de AmigaDOS.
pub const RC_BIEN: i32 = 0;
pub const RC_AVISO: i32 = 5;
pub const RC_ERROR: i32 = 10;
pub const RC_FALLO: i32 = 20;

/// Lo más que mide una línea de órdenes (y los argumentos de una orden), en bytes. Más, es un error, no un corte.
pub const MAX_LINEA: usize = 2048;
