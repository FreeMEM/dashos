//! Paq, el gestor de paquetes de DashOS (`docs/diseño/S8-paquetes.md`; bitácora en `docs/bitacoras/paq.md`).
//!
//! Lo que no toca el disco (huellas, firmas, gzip, tar, el índice, los ficheros de texto) va en sus módulos; lo que sí,
//! en `motor`, sobre el rasgo `Disco`: así lo mismo sirve para la orden `Paq` (lo de cada usuario, en `Home:Paquetes`),
//! para el servicio `paq` (lo de todos, en `Paquetes:`) y para las pruebas del PC, con un disco en memoria que se puede
//! cortar en cualquier paso.
//!
//! El formato de paquetes 2 (versiones, requisitos, manifiesto, resolutor y comprobador de planes) es `pkg2`, de Hangar.
//!
//! Las pruebas, en el PC: `cargo test` en `paq/` (antes, `tools/traer_pkg2.sh`).

#![no_std]
extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod cambio;
pub mod config;
pub mod disco;
pub mod fallo;
pub mod ficha;
pub mod firma;
pub mod generacion;
pub mod gz;
pub mod huella;
pub mod indice;
pub mod motor;
pub mod reconocer;
pub mod tar;
pub mod texto;

pub use fallo::Fallo;

/// La arquitectura de los paquetes nativos (formato 2, sección 3).
pub const ARQUITECTURA: &str = "aarch64-dashos";
