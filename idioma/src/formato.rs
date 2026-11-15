//! Meter valores en un texto traducible: `{}` (por orden) y `{0}`, `{1}`… (por posición, para que una traducción los
//! cambie de sitio). `{{` y `}}` son llaves de verdad. Sin dependencias: lo incluyen también las pruebas de PC de los
//! programas (`#[path]`).
//!
//! La plantilla viene de un catálogo, que puede estar mal: un hueco de más sale vacío, una llave suelta sale tal cual,
//! y nunca se sale de los argumentos.

extern crate alloc;

use alloc::string::String;
use core::fmt::{Display, Write};

/// Un pedazo de una plantilla.
enum Parte<'a> {
    Texto(&'a str),
    Letra(char),
    /// El hueco del valor número tal.
    Hueco(usize),
}

fn recorrer<'a>(plantilla: &'a str, mut cada: impl FnMut(Parte<'a>)) {
    let mut siguiente = 0;
    let mut resto = plantilla;
    while let Some(i) = resto.find(['{', '}']) {
        cada(Parte::Texto(&resto[..i]));
        let abre = resto.as_bytes()[i] == b'{';
        resto = &resto[i + 1..];
        if !abre {
            // `}}` es una llave; una sola, también.
            resto = resto.strip_prefix('}').unwrap_or(resto);
            cada(Parte::Letra('}'));
            continue;
        }
        if let Some(r) = resto.strip_prefix('{') {
            cada(Parte::Letra('{'));
            resto = r;
            continue;
        }
        match resto.find('}') {
            Some(fin) if fin <= 2 && resto[..fin].bytes().all(|b| b.is_ascii_digit()) => {
                let cual = if fin == 0 {
                    siguiente += 1;
                    siguiente - 1
                } else {
                    resto[..fin].parse().unwrap_or(usize::MAX)
                };
                cada(Parte::Hueco(cual));
                resto = &resto[fin + 1..];
            }
            // No es un hueco: la llave sale tal cual.
            _ => cada(Parte::Letra('{')),
        }
    }
    cada(Parte::Texto(resto));
}

/// `plantilla` con sus huecos llenos con `valores`.
pub fn poner(plantilla: &str, valores: &[&dyn Display]) -> String {
    let mut salida = String::with_capacity(plantilla.len() + 16 * valores.len());
    recorrer(plantilla, |p| match p {
        Parte::Texto(t) => salida.push_str(t),
        Parte::Letra(c) => salida.push(c),
        Parte::Hueco(n) => {
            if let Some(v) = valores.get(n) {
                let _ = write!(salida, "{v}");
            }
        }
    });
    salida
}

/// Qué valores usa una plantilla: el bit n, el valor n (los de 64 en adelante cuentan como el 63).
pub fn usa(plantilla: &str) -> u64 {
    let mut bits = 0;
    recorrer(plantilla, |p| {
        if let Parte::Hueco(n) = p {
            bits |= 1 << n.min(63);
        }
    });
    bits
}

/// Si la traducción se puede poner en lugar del original: no pide ningún valor que el original no dé. (Puede dejar
/// alguno sin usar, o cambiarlos de sitio.)
pub fn casa(original: &str, traduccion: &str) -> bool {
    usa(traduccion) & !usa(original) == 0
}
