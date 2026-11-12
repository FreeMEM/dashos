//! Nombres y rutas del Amiga.
//!
//! En la caja, los nombres son de 8 bits (Latin-1, como en el Amiga); el servicio guarda UTF-8. Aquí se pasa de unos
//! a otros y se parte una ruta del Amiga:
//!
//! - `Nombre:` delante es un volumen, una asignación o `PROGDIR:`; `:` solo, la raíz del volumen del cajón actual.
//!   Un segundo `:` no vale.
//! - Las partes van separadas por `/`. Una `/` al principio, o dos seguidas, sube un cajón (`/` es "el de arriba", no
//!   la raíz). Una `/` al final no hace nada.
//! - `.` y `..` no son nada especial en el Amiga; en DashOS no se pueden usar como nombres (el servicio no los
//!   admite), así que no valen.

use alloc::string::String;
use alloc::vec::Vec;

/// Lo más largo que puede ser un nombre: lo que cabe en `fib_FileName` (108 bytes con el 0).
pub const LARGO_NOMBRE: usize = 107;
/// Lo más largo que puede ser un comentario: lo que cabe en `fib_Comment` (80 bytes con el 0).
pub const LARGO_COMENTARIO: usize = 79;

/// Latin-1 a texto de Rust (cada byte es el carácter con ese número).
pub fn a_utf8(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

/// Texto a Latin-1, si todos sus caracteres caben en 8 bits.
pub fn a_latin1(s: &str) -> Option<Vec<u8>> {
    s.chars().map(|c| u8::try_from(c as u32).ok()).collect()
}

/// Mayúscula de un carácter Latin-1, como `ToUpper` de utility.library con el idioma de fábrica.
pub fn mayuscula(c: u8) -> u8 {
    match c {
        b'a'..=b'z' => c - 32,
        0xE0..=0xFE if c != 0xF7 => c - 32,
        _ => c,
    }
}

/// Un nombre Latin-1 en mayúsculas, para comparar sin distinguirlas.
pub fn clave_latin1(b: &[u8]) -> Vec<u8> {
    b.iter().map(|&c| mayuscula(c)).collect()
}

/// Un nombre UTF-8 en minúsculas, para comparar sin distinguirlas (como hace el servicio).
pub fn clave(s: &str) -> String {
    s.chars().flat_map(|c| c.to_lowercase()).collect()
}

/// Un paso de una ruta.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Paso {
    /// Subir un cajón.
    Subir,
    /// Entrar en un nombre (ya pasado a UTF-8).
    Nombre(String),
}

/// Una ruta del Amiga, partida.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ruta {
    /// Lo de antes de `:`, si hay `:`. Vacío es "la raíz del volumen actual".
    pub dispositivo: Option<Vec<u8>>,
    pub pasos: Vec<Paso>,
}

/// Si una parte puede ser un nombre de DashOS visto desde la caja.
pub fn nombre_valido(b: &[u8]) -> bool {
    !(b.is_empty() || b.len() > LARGO_NOMBRE || b == b"." || b == b".." || b.contains(&0))
}

/// Una ruta que no vale: un `:` de más o un nombre que no puede ser (`ERROR_INVALID_COMPONENT_NAME`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RutaMala;

/// Parte una ruta.
pub fn partir(ruta: &[u8]) -> Result<Ruta, RutaMala> {
    let (dispositivo, resto) = match ruta.iter().position(|&c| c == b':') {
        Some(i) => (Some(ruta[..i].to_vec()), &ruta[i + 1..]),
        None => (None, ruta),
    };
    if resto.contains(&b':') {
        return Err(RutaMala);
    }
    if let Some(d) = &dispositivo
        && d.contains(&b'/')
    {
        return Err(RutaMala);
    }
    let partes: Vec<&[u8]> = resto.split(|&c| c == b'/').collect();
    let mut pasos = Vec::new();
    let n = partes.len();
    for (i, p) in partes.iter().enumerate() {
        if p.is_empty() {
            if i + 1 < n {
                pasos.push(Paso::Subir);
            }
        } else if nombre_valido(p) {
            pasos.push(Paso::Nombre(a_utf8(p)));
        } else {
            return Err(RutaMala);
        }
    }
    Ok(Ruta { dispositivo, pasos })
}
