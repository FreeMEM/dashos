//! Las órdenes de base de `SYS:C`: cada una en su módulo, con su `principal`. Se reparten de dos formas
//! (`docs/bitacoras/shell-nativa.md`, "Dónde están las órdenes"): juntas en un programa (`SYS:C/Ordenes`, que hace la
//! que le diga quien lo lanza) o cada una en el suyo (`src/bin/sueltas/`).

use crate::Orden;

pub mod abrir;
pub mod administrar;
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
/// S32: el instalador (solo en vivo).
pub mod instalar;
pub mod list;
pub mod makedir;
pub mod permiso;
pub mod protect;
pub mod registro;
pub mod rename;
pub mod type_;
pub mod usuario;
pub mod version;
pub mod wait;

/// Las órdenes por su nombre (sin distinguir mayúsculas al buscar).
pub const BASE: &[(&str, fn(&mut Orden) -> u64)] = &[
    ("Abrir", abrir::principal),
    ("Administrar", administrar::principal),
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
    ("Instalar", instalar::principal),
    ("List", list::principal),
    ("MakeDir", makedir::principal),
    ("Permiso", permiso::principal),
    ("Protect", protect::principal),
    ("Registro", registro::principal),
    ("Rename", rename::principal),
    ("Type", type_::principal),
    ("Usuario", usuario::principal),
    ("Version", version::principal),
    ("Wait", wait::principal),
];

/// Sus nombres, sin las funciones: para quien solo quiere saber si una orden es de base (el Shell, que no las lleva
/// dentro).
pub const NOMBRES: &[&str] = &["Abrir", "Administrar", "Ajuste", "Ask", "Assign", "Cd", "Copy", "Date", "Delete", "Dir", "Echo", "Filenote", "Info", "Instalar", "List", "MakeDir", "Permiso", "Protect", "Registro", "Rename", "Type", "Usuario", "Version", "Wait"];

/// Si `nombre` es una orden de base.
pub fn es_de_base(nombre: &str) -> bool {
    NOMBRES.iter().any(|n| n.eq_ignore_ascii_case(nombre))
}

/// La orden de base que se llama `nombre`: su nombre bien escrito y su función.
pub fn buscar(nombre: &str) -> Option<(&'static str, fn(&mut Orden) -> u64)> {
    BASE.iter().find(|(n, _)| n.eq_ignore_ascii_case(nombre)).copied()
}
