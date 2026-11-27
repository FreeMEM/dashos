//! Funciones de texto de dos: Fault, PrintFault, FilePart, PathPart, AddPart, SplitName, StrToLong, y el formato de
//! VPrintf/VFPrintf (el de `RawDoFmt` de exec: `%[-][0][ancho][.límite][l]tipo`, con `d u x X s b c` y `%%`; sin `l`,
//! los números y `c` son WORD de 16 bits en la tabla de argumentos).

use alloc::vec::Vec;

use crate::errores;
use crate::fs::SistemaArchivos;
use crate::*;

/// Lo más largo que sale de un formato (un formato roto no llena la memoria).
const MAX_FORMATO: usize = 64 * 1024;

impl<F: SistemaArchivos> Dos<F> {
    /// El texto de `Fault`: "cabecera: mensaje", o el mensaje solo.
    pub(crate) fn texto_de_fallo<C: Caja>(c: &C, codigo: u32, cabecera: u32) -> R<Vec<u8>> {
        let mut t = Vec::new();
        if cabecera != 0 {
            t.extend(Self::cadena(c, cabecera)?);
            t.extend_from_slice(b": ");
        }
        match errores::texto(codigo) {
            Some(m) => t.extend_from_slice(m.as_bytes()),
            None => t.extend_from_slice(alloc::format!("Error code {}", codigo as i32).as_bytes()),
        }
        Ok(t)
    }

    /// Fault(code, header, buffer, len): cuántos bytes dejó en el búfer (sin el 0).
    pub(crate) fn fault<C: Caja>(c: &mut C, codigo: u32, cabecera: u32, buf: u32, largo: u32) -> R<u32> {
        if (largo as i32) <= 0 {
            return Ok(0);
        }
        Self::tramo(c, buf, largo)?;
        if codigo == 0 {
            Self::e8(c, buf, 0)?;
            return Ok(0);
        }
        let mut t = Self::texto_de_fallo(c, codigo, cabecera)?;
        t.truncate(largo as usize - 1);
        Self::poner(c, buf, &t)?;
        Self::e8(c, buf + t.len() as u32, 0)?;
        Ok(t.len() as u32)
    }

    /// FilePart(path): lo de después de la última `/` o `:`.
    pub(crate) fn file_part<C: Caja>(c: &C, ruta: u32) -> R<u32> {
        let s = Self::cadena(c, ruta)?;
        Ok(ruta + s.iter().rposition(|&b| b == b'/' || b == b':').map_or(0, |i| i as u32 + 1))
    }

    /// PathPart(path): donde acaba el cajón (la última `/`, o tras `:`, o el principio).
    pub(crate) fn path_part<C: Caja>(c: &C, ruta: u32) -> R<u32> {
        let s = Self::cadena(c, ruta)?;
        let i = match s.iter().rposition(|&b| b == b'/') {
            Some(i) => i,
            None => s.iter().rposition(|&b| b == b':').map_or(0, |i| i + 1),
        };
        Ok(ruta + i as u32)
    }

    /// AddPart(dirname, filename, size): añade un nombre a una ruta, con `/` si hace falta. Si el nombre lleva `:`,
    /// sustituye a la ruta. `false` si no cabe en `size` (con el 0); entonces no cambia nada.
    pub(crate) fn add_part<C: Caja>(c: &mut C, dir: u32, nombre: u32, tam: u32) -> R<bool> {
        let d = Self::cadena(c, dir)?;
        let n = Self::cadena(c, nombre)?;
        let mut t = if n.contains(&b':') {
            Vec::new()
        } else {
            let mut t = d.clone();
            if !t.is_empty() && !t.ends_with(b":") && !t.ends_with(b"/") && !n.is_empty() {
                t.push(b'/');
            }
            t
        };
        t.extend_from_slice(&n);
        Self::poner_cadena(c, dir, tam, &t)
    }

    /// SplitName(name, separator, buf, oldpos, size): copia en `buf` la parte desde `oldpos` hasta el separador.
    /// Devuelve la posición tras el separador, o -1 si se acabó la cadena.
    pub(crate) fn split_name<C: Caja>(c: &mut C, nombre: u32, sep: u32, buf: u32, pos: u32, tam: u32) -> R<u32> {
        let s = Self::cadena(c, nombre)?;
        let sep = sep as u8;
        let desde = (pos as usize).min(s.len());
        let resto = &s[desde..];
        let (parte, siguiente) = match resto.iter().position(|&b| b == sep) {
            Some(i) => (&resto[..i], (desde + i + 1) as u32),
            None => (resto, u32::MAX),
        };
        if tam > 0 {
            Self::tramo(c, buf, tam)?;
            let k = parte.len().min(tam as usize - 1);
            Self::poner(c, buf, &parte[..k])?;
            Self::e8(c, buf + k as u32, 0)?;
        }
        Ok(siguiente)
    }

    /// StrToLong(string, value): cuántos caracteres se leyeron (blancos de delante incluidos), o -1 si no hay número.
    pub(crate) fn str_to_long<C: Caja>(c: &mut C, s: u32, valor: u32) -> R<u32> {
        let t = Self::cadena(c, s)?;
        let Some((v, n)) = numero(&t) else { return Ok(u32::MAX) };
        Self::e32(c, valor, v as u32)?;
        Ok(n as u32)
    }

    /// El texto de un formato de `RawDoFmt` con su tabla de argumentos en la memoria de la caja.
    pub(crate) fn formatear<C: Caja>(c: &C, formato: u32, args: u32) -> R<Vec<u8>> {
        let f = Self::cadena(c, formato)?;
        let mut out = Vec::new();
        let mut a = args;
        let mut i = 0;
        while i < f.len() && out.len() < MAX_FORMATO {
            if f[i] != b'%' {
                out.push(f[i]);
                i += 1;
                continue;
            }
            i += 1;
            let mut izquierda = false;
            let mut ceros = false;
            while i < f.len() && (f[i] == b'-' || f[i] == b'0') {
                if f[i] == b'-' {
                    izquierda = true;
                } else {
                    ceros = true;
                }
                i += 1;
            }
            let mut ancho = 0usize;
            while i < f.len() && f[i].is_ascii_digit() {
                ancho = (ancho * 10 + (f[i] - b'0') as usize).min(1024);
                i += 1;
            }
            let mut limite = usize::MAX;
            if i < f.len() && f[i] == b'.' {
                i += 1;
                let mut l = 0usize;
                while i < f.len() && f[i].is_ascii_digit() {
                    l = (l * 10 + (f[i] - b'0') as usize).min(1 << 16);
                    i += 1;
                }
                limite = l;
            }
            let mut largo = false;
            if i < f.len() && f[i] == b'l' {
                largo = true;
                i += 1;
            }
            let Some(&tipo) = f.get(i) else { break };
            i += 1;
            let mut leer = |c: &C, largo: bool| -> R<u32> {
                if largo {
                    let v = Self::l32(c, a)?;
                    a = a.wrapping_add(4);
                    Ok(v)
                } else {
                    let v = Self::l16(c, a)?;
                    a = a.wrapping_add(2);
                    Ok(v as u32)
                }
            };
            let mut campo: Vec<u8> = match tipo {
                b'%' => {
                    out.push(b'%');
                    continue;
                }
                b'd' | b'D' => {
                    let v = leer(c, largo)?;
                    let v = if largo { v as i32 as i64 } else { v as u16 as i16 as i64 };
                    alloc::format!("{v}").into_bytes()
                }
                b'u' | b'U' => alloc::format!("{}", leer(c, largo)?).into_bytes(),
                b'x' => alloc::format!("{:x}", leer(c, largo)?).into_bytes(),
                b'X' => alloc::format!("{:X}", leer(c, largo)?).into_bytes(),
                b'c' => alloc::vec![leer(c, largo)? as u8],
                b's' => {
                    let p = leer(c, true)?;
                    if p == 0 { Vec::new() } else { Self::cadena(c, p)? }
                }
                b'b' => {
                    let p = Self::baddr(leer(c, true)?)?;
                    if p == 0 {
                        Vec::new()
                    } else {
                        let n = Self::l8(c, p)? as u32;
                        Self::bytes(c, p + 1, n)?
                    }
                }
                otro => {
                    // Un tipo que no se conoce sale tal cual.
                    alloc::vec![b'%', otro]
                }
            };
            if matches!(tipo, b's' | b'b') {
                campo.truncate(limite);
            }
            if campo.len() < ancho {
                let relleno = ancho - campo.len();
                if izquierda {
                    campo.extend(core::iter::repeat_n(b' ', relleno));
                } else if ceros && !matches!(tipo, b's' | b'b' | b'c') {
                    let signo = usize::from(campo.first() == Some(&b'-'));
                    let mut v = campo[..signo].to_vec();
                    v.extend(core::iter::repeat_n(b'0', relleno));
                    v.extend_from_slice(&campo[signo..]);
                    campo = v;
                } else {
                    let mut v: Vec<u8> = core::iter::repeat_n(b' ', relleno).collect();
                    v.extend_from_slice(&campo);
                    campo = v;
                }
            }
            out.extend_from_slice(&campo);
        }
        out.truncate(MAX_FORMATO);
        Ok(out)
    }
}

/// Lee un número decimal con signo tras blancos: (valor, caracteres leídos).
pub(crate) fn numero(t: &[u8]) -> Option<(i32, usize)> {
    let mut i = 0;
    while i < t.len() && (t[i] == b' ' || t[i] == b'\t') {
        i += 1;
    }
    let mut neg = false;
    if i < t.len() && (t[i] == b'-' || t[i] == b'+') {
        neg = t[i] == b'-';
        i += 1;
    }
    let inicio = i;
    let mut v: i64 = 0;
    while i < t.len() && t[i].is_ascii_digit() {
        v = (v * 10 + (t[i] - b'0') as i64).min(1 << 40);
        i += 1;
    }
    if i == inicio {
        return None;
    }
    let v = if neg { -v } else { v };
    Some((v.clamp(i32::MIN as i64, i32::MAX as i64) as i32, i))
}
