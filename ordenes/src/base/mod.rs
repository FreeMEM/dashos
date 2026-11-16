//! Las órdenes de base de `SYS:C`: cada una en su módulo, con su `principal`. Se reparten de dos formas
//! (`docs/bitacoras/shell-nativa.md`, "Dónde están las órdenes"): juntas en un programa (`SYS:C/Ordenes`, que hace la
//! que le diga quien lo lanza) o cada una en el suyo (`src/bin/sueltas/`).

use crate::Orden;

pub mod ajuste;
pub mod ask;
pub mod assign;
pub mod cd;
pub mod copy;
pub mod date;
pub mod delete;
pub mod dir;
pub mod echo;
pub mod filenote;
pub mod info;
pub mod list;
pub mod makedir;
pub mod permiso;
pub mod protect;
pub mod registro;
pub mod rename;
pub mod type_;
pub mod version;
pub mod wait;

/// Las órdenes por su nombre (sin distinguir mayúsculas al buscar).
pub const BASE: &[(&str, fn(&mut Orden) -> u64)] = &[
    ("Ajuste", ajuste::principal),
    ("Ask", ask::principal),
    ("Assign", assign::principal),
    ("Cd", cd::principal),
    ("Copy", copy::principal),
    ("Date", date::principal),
    ("Delete", delete::principal),
    ("Dir", dir::principal),
    ("Echo", echo::principal),
    ("Filenote", filenote::principal),
    ("Info", info::principal),
    ("List", list::principal),
    ("MakeDir", makedir::principal),
    ("Permiso", permiso::principal),
    ("Protect", protect::principal),
    ("Registro", registro::principal),
    ("Rename", rename::principal),
    ("Type", type_::principal),
    ("Version", version::principal),
    ("Wait", wait::principal),
];

/// Sus nombres, sin las funciones: para quien solo quiere saber si una orden es de base (el Shell, que no las lleva
/// dentro).
pub const NOMBRES: &[&str] = &["Ajuste", "Ask", "Assign", "Cd", "Copy", "Date", "Delete", "Dir", "Echo", "Filenote", "Info", "List", "MakeDir", "Permiso", "Protect", "Registro", "Rename", "Type", "Version", "Wait"];

/// Si `nombre` es una orden de base.
pub fn es_de_base(nombre: &str) -> bool {
    NOMBRES.iter().any(|n| n.eq_ignore_ascii_case(nombre))
}

/// La orden de base que se llama `nombre`: su nombre bien escrito y su función.
pub fn buscar(nombre: &str) -> Option<(&'static str, fn(&mut Orden) -> u64)> {
    BASE.iter().find(|(n, _)| n.eq_ignore_ascii_case(nombre)).copied()
}
