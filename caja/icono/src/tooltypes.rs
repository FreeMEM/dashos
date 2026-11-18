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


/// `BumpRevision` (lo comprobado contra el oráculo, `caja/pruebas68k/icono.c` con el Kickstart 40.68): "Copy_of_X"
/// para X, y "Copy_<n+1>_of_X" si ya era la copia `n`, escrita con espacios o con rayas ("copy of X", "Copy_3_of_X"...,
/// sin distinguir mayúsculas). Un número raro (de más de 9 cifras, o 0) no cuenta: "Copy_of_" delante de todo. Como
/// mucho `max` letras.
pub fn siguiente_copia(viejo: &[u8], max: usize) -> Vec<u8> {
    let sep = |b: u8| b == b' ' || b == b'_';
    let mut n = 1u32;
    let mut resto = viejo;
    if viejo.len() > 5 && viejo[..4].eq_ignore_ascii_case(b"copy") && sep(viejo[4]) {
        let mut i = 5;
        let mut num = 0u32;
        let mut digitos = 0;
        while i < viejo.len() && viejo[i].is_ascii_digit() && digitos < 9 {
            num = num * 10 + (viejo[i] - b'0') as u32;
            i += 1;
            digitos += 1;
        }
        let mut vale = true;
        if digitos > 0 {
            if i < viejo.len() && sep(viejo[i]) {
                i += 1;
            } else {
                vale = false;
            }
        }
        if vale && viejo.len() > i + 3 && viejo[i..i + 2].eq_ignore_ascii_case(b"of") && sep(viejo[i + 2]) && (digitos == 0 || num >= 1) {
            n = if digitos == 0 { 1 } else { num };
            resto = &viejo[i + 3..];
        }
    }
    let mut v = Vec::with_capacity(max + 1);
    if resto.len() == viejo.len() {
        v.extend_from_slice(b"Copy_of_");
    } else {
        v.extend_from_slice(b"Copy_");
        let mut cifras = Vec::new();
        let mut k = n.saturating_add(1);
        while k > 0 {
            cifras.push(b'0' + (k % 10) as u8);
            k /= 10;
        }
        cifras.reverse();
        v.extend_from_slice(&cifras);
        v.extend_from_slice(b"_of_");
    }
    v.extend_from_slice(resto);
    v.truncate(max);
    v
}

/// Las letras en mayúsculas (para comparar nombres de iconos de serie).
pub fn mayusculas(s: &[u8]) -> Vec<u8> {
    s.iter().map(|&c| mayuscula(c)).collect()
}
