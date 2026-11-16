//! Un cambio de un plan, en una línea: lo que se enseña al administrador en la ventana de `guardian` (una acción de la
//! tanda por línea, S6b) y lo único que hace después el servicio `paq` ("un plan, una contraseña": Paq hace solo lo que
//! estaba en la lista). Las mismas palabras lee `usuario/src/tanda.rs`, que solo mira su forma.
//!
//! - `instalar <nombre> <versión> <repositorio> <explicit|dependency>`
//! - `actualizar <nombre> <versión de ahora> <versión nueva> <repositorio>`
//! - `volver <nombre> <versión de ahora> <versión de antes> <repositorio>` (a una más vieja, guardada en el almacén)
//! - `quitar <nombre> <versión de ahora>`
//!
//! Las versiones, enteras en una palabra (`[epoch:]versión-release`).

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use pkg2::formato::{Version, nombre_valido};

use crate::generacion::Motivo;
use crate::texto::{bonita, leer_version, version};

/// Lo más que mide una línea (lo de la tanda de `sesiones`).
pub const MAX_LINEA: usize = 120;
/// Cuántos cambios caben en una confirmación (las acciones de una tanda).
pub const MAX_CAMBIOS: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cambio {
    Instalar { nombre: String, version: Version, repo: String, motivo: Motivo },
    Actualizar { nombre: String, de: Version, a: Version, repo: String },
    Volver { nombre: String, de: Version, a: Version, repo: String },
    Quitar { nombre: String, version: Version },
}

impl Cambio {
    pub fn nombre(&self) -> &str {
        match self {
            Cambio::Instalar { nombre, .. } | Cambio::Actualizar { nombre, .. } | Cambio::Volver { nombre, .. } | Cambio::Quitar { nombre, .. } => nombre,
        }
    }

    /// La versión con que queda (ninguna si se quita).
    pub fn queda(&self) -> Option<(&Version, &str)> {
        match self {
            Cambio::Instalar { version, repo, .. } => Some((version, repo)),
            Cambio::Actualizar { a, repo, .. } | Cambio::Volver { a, repo, .. } => Some((a, repo)),
            Cambio::Quitar { .. } => None,
        }
    }

    pub fn linea(&self) -> String {
        match self {
            Cambio::Instalar { nombre, version: v, repo, motivo } => format!("instalar {nombre} {} {repo} {}", version(v), motivo.palabra()),
            Cambio::Actualizar { nombre, de, a, repo } => format!("actualizar {nombre} {} {} {repo}", version(de), version(a)),
            Cambio::Volver { nombre, de, a, repo } => format!("volver {nombre} {} {} {repo}", version(de), version(a)),
            Cambio::Quitar { nombre, version: v } => format!("quitar {nombre} {}", version(v)),
        }
    }

    pub fn leer(l: &str) -> Option<Cambio> {
        let p: Vec<&str> = l.trim_end_matches('\r').split(' ').collect();
        let n = |x: &str| nombre_valido(x).then(|| x.to_string());
        Some(match p.as_slice() {
            ["instalar", nombre, v, repo, m] => Cambio::Instalar { nombre: n(nombre)?, version: leer_version(v)?, repo: n(repo)?, motivo: Motivo::de_palabra(m)? },
            ["actualizar", nombre, de, a, repo] => Cambio::Actualizar { nombre: n(nombre)?, de: leer_version(de)?, a: leer_version(a)?, repo: n(repo)? },
            ["volver", nombre, de, a, repo] => Cambio::Volver { nombre: n(nombre)?, de: leer_version(de)?, a: leer_version(a)?, repo: n(repo)? },
            ["quitar", nombre, v] => Cambio::Quitar { nombre: n(nombre)?, version: leer_version(v)? },
            _ => return None,
        })
    }

    /// En llano, para la orden (`guardian` tiene la suya, en `tanda.rs`, con las mismas palabras).
    pub fn frase(&self) -> String {
        match self {
            Cambio::Instalar { nombre, version, repo, motivo: Motivo::Pedido } => format!("Instalar {nombre} {} (de {repo})", bonita(version)),
            Cambio::Instalar { nombre, version, repo, motivo: Motivo::Dependencia } => format!("Instalar {nombre} {} (de {repo}; lo necesita otro)", bonita(version)),
            Cambio::Actualizar { nombre, de, a, repo } => format!("Actualizar {nombre} de {} a {} (de {repo})", bonita(de), bonita(a)),
            Cambio::Volver { nombre, de, a, .. } => format!("Volver {nombre} de {} a {}", bonita(de), bonita(a)),
            Cambio::Quitar { nombre, version } => format!("Quitar {nombre} {}", bonita(version)),
        }
    }
}

/// Las líneas de un plan; si una no se entiende, ninguna.
pub fn leer_lineas(t: &str) -> Option<Vec<Cambio>> {
    t.lines().filter(|l| !l.trim().is_empty()).map(Cambio::leer).collect()
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn lineas() {
        for l in ["instalar saludo 1.0-1 prueba explicit", "actualizar saludo 1.0-1 2:1.1-3 prueba", "volver saludo 1.1-1 1.0-1 prueba", "quitar saludo 1.0-1"] {
            let c = Cambio::leer(l).unwrap();
            assert_eq!(c.linea(), l);
            assert!(c.linea().len() <= MAX_LINEA);
        }
        for mala in ["instalar saludo 1.0 prueba explicit", "instalar Saludo 1.0-1 prueba explicit", "quitar", "borrar saludo 1.0-1", "quitar saludo 1.0-1 extra", "instalar saludo 1.0-1 prueba porque"] {
            assert_eq!(Cambio::leer(mala), None, "{mala}");
        }
        assert_eq!(Cambio::leer("instalar letras 1.0-1 prueba dependency").unwrap().frase(), "Instalar letras 1.0 (de prueba; lo necesita otro)");
    }
}
