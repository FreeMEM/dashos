//! Los juegos de caracteres de los ficheros del Amiga (8 bits) y UTF-8.

use alloc::string::String;
use alloc::vec::Vec;

/// El juego de caracteres de un catálogo, según su trozo `CSET` (números de la IANA, como AmigaOS 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Juego {
    /// ISO 8859-1, el del Amiga de siempre (0, o sin `CSET`; 3 es ASCII y 4 es Latin-1).
    Latin1,
    /// ISO 8859-15 (111): Latin-1 con el euro.
    Latin9,
    /// UTF-8 (106): el de los catálogos nativos de DashOS.
    Utf8,
}

impl Juego {
    /// El juego del número de `CSET`; `None` si no lo sabemos leer (el catálogo no vale: saldría basura).
    pub fn desde(numero: u32) -> Option<Juego> {
        match numero {
            0 | 3 | 4 => Some(Juego::Latin1),
            111 => Some(Juego::Latin9),
            106 => Some(Juego::Utf8),
            _ => None,
        }
    }

    pub fn numero(self) -> u32 {
        match self {
            Juego::Latin1 => 0,
            Juego::Latin9 => 111,
            Juego::Utf8 => 106,
        }
    }
}

fn latin9(b: u8) -> char {
    match b {
        0xa4 => '€',
        0xa6 => 'Š',
        0xa8 => 'š',
        0xb4 => 'Ž',
        0xb8 => 'ž',
        0xbc => 'Œ',
        0xbd => 'œ',
        0xbe => 'Ÿ',
        _ => b as char,
    }
}

/// Añade a `salida` el texto `bytes` (sin el 0 final), que está en `juego`. `false` si es UTF-8 y no es válido: no se
/// añade nada.
pub fn a_utf8(bytes: &[u8], juego: Juego, salida: &mut String) -> bool {
    match juego {
        Juego::Utf8 => match core::str::from_utf8(bytes) {
            Ok(t) => salida.push_str(t),
            Err(_) => return false,
        },
        Juego::Latin1 => salida.extend(bytes.iter().map(|&b| b as char)),
        Juego::Latin9 => salida.extend(bytes.iter().map(|&b| latin9(b))),
    }
    true
}

/// Un texto de las preferencias del Amiga (`locale.prefs`, un `.country`): acaba en el primer 0 o al final del campo.
/// Se lee como Latin-9 (ISO 8859-15), que es Latin-1 con el euro en el sitio de `¤`: así el símbolo de la moneda de
/// España cabe en un fichero que también lee el Amiga.
pub fn de_campo(campo: &[u8]) -> String {
    let fin = campo.iter().position(|&b| b == 0).unwrap_or(campo.len());
    let mut s = String::new();
    a_utf8(&campo[..fin], Juego::Latin9, &mut s);
    s
}

/// El texto en Latin-1, para un catálogo del Amiga. Lo que no cabe en Latin-1 sale como `?`.
pub fn a_latin1(texto: &str) -> Vec<u8> {
    texto.chars().map(|c| if (c as u32) < 256 { c as u32 as u8 } else { b'?' }).collect()
}

/// El texto en Latin-9, para un campo de las preferencias (lo contrario de `de_campo`). Lo que no cabe, `?`.
pub fn a_latin9(texto: &str) -> Vec<u8> {
    texto
        .chars()
        .map(|c| match c {
            '€' => 0xa4,
            'Š' => 0xa6,
            'š' => 0xa8,
            'Ž' => 0xb4,
            'ž' => 0xb8,
            'Œ' => 0xbc,
            'œ' => 0xbd,
            'Ÿ' => 0xbe,
            '¤' | '¦' | '¨' | '´' | '¸' | '¼' | '½' | '¾' => b'?',
            c if (c as u32) < 256 => c as u32 as u8,
            _ => b'?',
        })
        .collect()
}

/// Escribe `texto` en un campo de `campo.len()` bytes, con su 0 final: lo que no cabe se corta.
pub fn en_campo(texto: &str, campo: &mut [u8]) {
    campo.fill(0);
    let b = a_latin9(texto);
    let n = b.len().min(campo.len().saturating_sub(1));
    campo[..n].copy_from_slice(&b[..n]);
}

/// Si dos nombres son el mismo sin mirar mayúsculas (también las de Latin-1: `Español` y `español`), como en el Amiga.
pub fn mismo_nombre(a: &str, b: &str) -> bool {
    let mut x = a.chars().flat_map(char::to_lowercase);
    let mut y = b.chars().flat_map(char::to_lowercase);
    loop {
        match (x.next(), y.next()) {
            (None, None) => return true,
            (p, q) if p != q => return false,
            _ => {}
        }
    }
}

/// Si `nombre` vale como nombre de idioma, de país o de programa: va a formar parte de un camino
/// (`LOCALE:Catalogs/<idioma>/<programa>.catalog`), así que no puede traer barras, dos puntos, bytes de control ni
/// empezar por un punto, y cabe en los 29 bytes de Latin-1 de un campo del Amiga.
pub fn nombre_valido(nombre: &str) -> bool {
    !nombre.is_empty()
        && nombre.chars().count() <= 29
        && !nombre.starts_with('.')
        && nombre.chars().all(|c| !c.is_control() && !matches!(c, '/' | ':' | '\\' | '*' | '?' | '"' | '|' | '<' | '>') && (c as u32) < 256)
}
