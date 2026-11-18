//! Los tooltypes (`NOMBRE=valor|otro`) y los nombres de las copias, sin memoria del 68k: lo que hacen
//! `FindToolType`, `MatchToolValue` y `BumpRevision` sobre bytes. Desde sus autodocs: las comparaciones no miran
//! mayúsculas, y desde la versión 44 se saltan los blancos de alrededor del `=` y de cada valor.

use alloc::vec::Vec;

use crate::mem::{iguales_sin_caja, mayuscula};

fn blanco(c: u8) -> bool {
    c == b' ' || c == b'\t'
}

fn recortar(mut s: &[u8]) -> &[u8] {
    while let [c, r @ ..] = s
        && blanco(*c)
    {
        s = r;
    }
    while let [r @ .., c] = s
        && blanco(*c)
    {
        s = r;
    }
    s
}

/// Si el tooltype `tt` es el de `nombre`, dónde empieza su valor (tras el `=` y los blancos), o su largo si no
/// tiene `=` (un `DONOTWAIT` suelto: se encuentra, con el valor vacío).
pub fn valor_de(tt: &[u8], nombre: &[u8]) -> Option<usize> {
    let igual = tt.iter().position(|&c| c == b'=');
    let clave = recortar(&tt[..igual.unwrap_or(tt.len())]);
    let nombre = recortar(nombre);
    if nombre.is_empty() || !iguales_sin_caja(clave, nombre) {
        return None;
    }
    match igual {
        None => Some(tt.len()),
        Some(i) => {
            let mut j = i + 1;
            while j < tt.len() && blanco(tt[j]) {
                j += 1;
            }
            Some(j)
        }
    }
}

/// `MatchToolValue`: si `valor` es una de las opciones de `tipo` (separadas por `|`), sin mirar mayúsculas.
pub fn casa_valor(tipo: &[u8], valor: &[u8]) -> bool {
    let valor = recortar(valor);
    tipo.split(|&c| c == b'|').any(|op| iguales_sin_caja(recortar(op), valor))
}

const COPIA: &[u8] = b"copy ";
const DE: &[u8] = b"of ";

/// `BumpRevision`: "copy of X" para X, "copy 2 of X" para "copy of X", "copy N+1 of X" para "copy N of X". Se
/// recorta a `max` letras (30 en `BumpRevision`).
pub fn siguiente_copia(viejo: &[u8], max: usize) -> Vec<u8> {
    let mut n = 0u32;
    let mut resto = viejo;
    if viejo.len() >= COPIA.len() && iguales_sin_caja(&viejo[..COPIA.len()], COPIA) {
        let tras = &viejo[COPIA.len()..];
        if tras.len() >= DE.len() && iguales_sin_caja(&tras[..DE.len()], DE) {
            n = 1;
            resto = &tras[DE.len()..];
        } else {
            let cifras = tras.iter().take_while(|c| c.is_ascii_digit()).count();
            let tras_num = &tras[cifras..];
            if cifras > 0
                && cifras <= 9
                && tras_num.len() > DE.len()
                && tras_num[0] == b' '
                && iguales_sin_caja(&tras_num[1..1 + DE.len()], DE)
            {
                n = tras[..cifras].iter().fold(0u32, |a, c| a * 10 + (c - b'0') as u32);
                resto = &tras_num[1 + DE.len()..];
            }
        }
    }
    let mut v = Vec::with_capacity(max + 1);
    v.extend_from_slice(b"copy ");
    if n >= 1 {
        let mut cifras = Vec::new();
        let mut k = n.saturating_add(1);
        while k > 0 {
            cifras.push(b'0' + (k % 10) as u8);
            k /= 10;
        }
        cifras.reverse();
        v.extend_from_slice(&cifras);
        v.push(b' ');
    }
    v.extend_from_slice(b"of ");
    v.extend_from_slice(resto);
    v.truncate(max);
    v
}

/// Las letras en mayúsculas (para comparar nombres de iconos de serie).
pub fn mayusculas(s: &[u8]) -> Vec<u8> {
    s.iter().map(|&c| mayuscula(c)).collect()
}
