//! Rutas del Amiga sobre un cajón actual: `VOL:cajón/fichero`, sin nombre es desde el cajón actual, `:x` desde la raíz
//! del volumen actual, y una `/` al principio (o dos seguidas) sube un cajón. Lo que sale es siempre `Nombre:a/b`, sin
//! barras de más, para `dashos::asignaciones`, que no admite subir.
//!
//! No se sube por encima de un nombre: `Home:` no tiene padre (S6b: por encima de una raíz no se llega).

use alloc::string::String;
use alloc::vec::Vec;

/// Lo más que mide una ruta entera.
pub const MAX_RUTA: usize = 255;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Sube por encima del nombre.
    SobreLaRaiz,
    /// Dos puntos de más, un nombre vacío donde no puede, o letras de control.
    MalFormada,
    /// Más de `MAX_RUTA` bytes.
    Larga,
}

/// El nombre (sin los dos puntos) y el resto de una ruta absoluta.
pub fn nombre_y_resto(ruta: &str) -> Option<(&str, &str)> {
    ruta.split_once(':')
}

/// Junta un cajón y un nombre: `Home:` + `x` es `Home:x`; `Home:a` + `x`, `Home:a/x`.
pub fn unir(cajon: &str, nombre: &str) -> String {
    let mut s = String::from(cajon);
    if !cajon.ends_with(':') && !cajon.ends_with('/') && !cajon.is_empty() {
        s.push('/');
    }
    s.push_str(nombre);
    s
}

/// Parte una ruta absoluta en su cajón y su último nombre: `Home:a/b` en `Home:a` y `b`; `Home:b` en `Home:` y `b`;
/// `Home:` en `Home:` y nada.
pub fn partir(ruta: &str) -> (String, String) {
    match ruta.rfind(['/', ':']) {
        Some(i) if ruta.as_bytes()[i] == b'/' => (String::from(&ruta[..i]), String::from(&ruta[i + 1..])),
        Some(i) => (String::from(&ruta[..=i]), String::from(&ruta[i + 1..])),
        None => (String::new(), String::from(ruta)),
    }
}

/// El último nombre de una ruta (lo de detrás de la última `/` o de los dos puntos).
pub fn ultimo(ruta: &str) -> &str {
    ruta.rsplit(['/', ':']).next().unwrap_or(ruta)
}

/// La ruta absoluta de `camino` desde el cajón actual `actual` (que ya es absoluta).
pub fn absoluta(actual: &str, camino: &str) -> Result<String, Error> {
    if camino.len() > MAX_RUTA || camino.chars().any(|c| (c as u32) < 0x20 || c as u32 == 0x7f) {
        return Err(if camino.len() > MAX_RUTA { Error::Larga } else { Error::MalFormada });
    }
    let (nombre, partes_base, resto): (&str, Vec<&str>, &str) = match camino.split_once(':') {
        Some((n, r)) => {
            if r.contains(':') {
                return Err(Error::MalFormada);
            }
            // `:x`: desde la raíz del volumen del cajón actual.
            let n = if n.is_empty() { actual.split_once(':').ok_or(Error::MalFormada)?.0 } else { n };
            if n.contains('/') {
                return Err(Error::MalFormada);
            }
            (n, Vec::new(), r)
        }
        None => {
            let (n, r) = actual.split_once(':').ok_or(Error::MalFormada)?;
            (n, r.split('/').filter(|p| !p.is_empty()).collect(), camino)
        }
    };
    if nombre.is_empty() {
        return Err(Error::MalFormada);
    }
    let mut partes = partes_base;
    // Las barras del principio suben, una cada una. Después, una barra al final no cuenta (`Fotos/` es `Fotos`) y dos
    // seguidas en medio suben (`a//b` es `b` al lado de `a`).
    let sin_subir = resto.trim_start_matches('/');
    for _ in 0..resto.len() - sin_subir.len() {
        partes.pop().ok_or(Error::SobreLaRaiz)?;
    }
    let resto = sin_subir.strip_suffix('/').unwrap_or(sin_subir);
    if !resto.is_empty() {
        for p in resto.split('/') {
            match p {
                "" | ".." => {
                    partes.pop().ok_or(Error::SobreLaRaiz)?;
                }
                "." => {}
                p => partes.push(p),
            }
        }
    }
    let mut s = String::from(nombre);
    s.push(':');
    for (i, p) in partes.iter().enumerate() {
        if i > 0 {
            s.push('/');
        }
        s.push_str(p);
    }
    if s.len() > MAX_RUTA {
        return Err(Error::Larga);
    }
    Ok(s)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn relativas_y_absolutas() {
        assert_eq!(absoluta("Home:", "nota.txt").unwrap(), "Home:nota.txt");
        assert_eq!(absoluta("Home:Trabajo", "a/b").unwrap(), "Home:Trabajo/a/b");
        assert_eq!(absoluta("Home:Trabajo", "SYS:C/Dir").unwrap(), "SYS:C/Dir");
        assert_eq!(absoluta("Home:Trabajo/x", ":otro").unwrap(), "Home:otro");
        assert_eq!(absoluta("Home:Trabajo", "").unwrap(), "Home:Trabajo");
        assert_eq!(absoluta("Home:Trabajo", "Fotos/").unwrap(), "Home:Trabajo/Fotos");
        assert_eq!(absoluta("Home:", "C:").unwrap(), "C:");
    }

    #[test]
    fn subir() {
        assert_eq!(absoluta("Home:a/b", "/").unwrap(), "Home:a");
        assert_eq!(absoluta("Home:a/b", "//").unwrap(), "Home:");
        assert_eq!(absoluta("Home:a/b", "/c").unwrap(), "Home:a/c");
        assert_eq!(absoluta("Home:a/b", "c//d").unwrap(), "Home:a/b/d");
        assert_eq!(absoluta("Home:a/b", "../c").unwrap(), "Home:a/c");
        assert_eq!(absoluta("Home:a", "./c").unwrap(), "Home:a/c");
        assert_eq!(absoluta("Home:a", "Work:x/y//z").unwrap(), "Work:x/z");
    }

    #[test]
    fn hostiles() {
        assert_eq!(absoluta("Home:", "/"), Err(Error::SobreLaRaiz));
        assert_eq!(absoluta("Home:a", "///"), Err(Error::SobreLaRaiz));
        assert_eq!(absoluta("Home:", "SYS:/Libs"), Err(Error::SobreLaRaiz));
        assert_eq!(absoluta("Home:", "../../x"), Err(Error::SobreLaRaiz));
        assert_eq!(absoluta("Home:", "a:b:c"), Err(Error::MalFormada));
        assert_eq!(absoluta("Home:", "a/b:c"), Err(Error::MalFormada));
        assert_eq!(absoluta("Home:", "a\nb"), Err(Error::MalFormada));
        assert_eq!(absoluta("sin nombre", "x"), Err(Error::MalFormada));
        assert_eq!(absoluta("Home:", &"x".repeat(300)), Err(Error::Larga));
        assert_eq!(absoluta(&alloc::format!("Home:{}", "y".repeat(200)), &"x".repeat(100)), Err(Error::Larga));
    }

    #[test]
    fn partir_y_unir() {
        assert_eq!(partir("Home:a/b"), ("Home:a".into(), "b".into()));
        assert_eq!(partir("Home:b"), ("Home:".into(), "b".into()));
        assert_eq!(partir("Home:"), ("Home:".into(), "".into()));
        assert_eq!(unir("Home:", "x"), "Home:x");
        assert_eq!(unir("Home:a", "x"), "Home:a/x");
        assert_eq!(ultimo("Home:a/b"), "b");
        assert_eq!(ultimo("Home:"), "");
    }
}
