//! Lo común de los ficheros de texto de Paq: bloques de campos (`Campo: valor`), la suma del final (`Sum:`), las fechas
//! (`2026-11-16T12:00:00Z`) y las versiones enteras escritas en una palabra (`[epoch:]versión-release`).

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use pkg2::formato::{Version, numero, version_valida};

use crate::huella;

/// Los campos de un bloque: nombre en minúsculas y valor sin espacios a los lados. Las líneas que empiezan por `#` y
/// las que no son `Campo: valor` no cuentan.
pub fn campos(bloque: &str) -> Vec<(String, String)> {
    bloque
        .lines()
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once(':'))
        .filter(|(c, _)| !c.is_empty() && !c.contains(' '))
        .map(|(c, v)| (c.to_ascii_lowercase(), v.trim().to_string()))
        .collect()
}

/// El primer valor del campo `c` (en minúsculas).
pub fn campo<'a>(cs: &'a [(String, String)], c: &str) -> Option<&'a str> {
    cs.iter().find(|x| x.0 == c).map(|x| x.1.as_str())
}

/// Los bloques de un texto, separados por líneas en blanco.
pub fn bloques(t: &str) -> impl Iterator<Item = &str> {
    t.split("\n\n").flat_map(|b| b.split("\r\n\r\n")).filter(|b| b.lines().any(|l| !l.trim().is_empty() && !l.starts_with('#')))
}

/// La suma de un texto: los 16 primeros del SHA-256 (en hexadecimal) de todo lo de antes de la línea `Sum:`.
pub fn suma(t: &str) -> String {
    huella::hex(&huella::sha256(t.as_bytes())[..8])
}

/// `t` con su línea `Sum:` al final.
pub fn con_suma(t: &str) -> String {
    format!("{t}Sum: {}\n", suma(t))
}

/// Si la última línea de `t` es su `Sum:` y cuadra: lo de antes. Si falta o no cuadra, `None` (cambiado o cortado).
pub fn sin_suma(t: &str) -> Option<&str> {
    let cuerpo = t.strip_suffix('\n')?;
    let i = cuerpo.rfind('\n').map_or(0, |i| i + 1);
    let valor = cuerpo[i..].strip_prefix("Sum: ")?;
    let antes = &t[..i];
    (valor == suma(antes)).then_some(antes)
}

/// Una versión entera en una palabra: `[epoch:]versión-release` (el epoch solo si no es 0; el release siempre).
pub fn version(v: &Version) -> String {
    if v.epoch > 0 { format!("{}:{}-{}", v.epoch, v.version, v.release) } else { format!("{}-{}", v.version, v.release) }
}

/// Lee lo que escribe `version`.
pub fn leer_version(t: &str) -> Option<Version> {
    let (epoch, resto) = match t.split_once(':') {
        Some((e, r)) => (numero(e, u32::MAX as u64)? as u32, r),
        None => (0, t),
    };
    let (v, r) = resto.rsplit_once('-')?;
    let release = numero(r, u32::MAX as u64)? as u32;
    version_valida(v).then(|| Version { epoch, version: v.to_string(), release })
}

/// Una versión para la gente: sin el epoch 0 ni el release 1 (`1.0`, `1.0-2`, `1:2.0`).
pub fn bonita(v: &Version) -> String {
    let mut t = if v.epoch > 0 { format!("{}:{}", v.epoch, v.version) } else { v.version.clone() };
    if v.release != 1 {
        t += &format!("-{}", v.release);
    }
    t
}

/// El cajón de una versión en el almacén: `epoch-versión-release` (único: el epoch y el release son números).
pub fn hueco(v: &Version) -> String {
    format!("{}-{}-{}", v.epoch, v.version, v.release)
}

// ---------------------------------------------------------------------------------------------------------------
// Fechas, en UTC.

fn dias_desde_civil(a: i64, m: i64, d: i64) -> i64 {
    let a = if m <= 2 { a - 1 } else { a };
    let era = a.div_euclid(400);
    let ae = a - era * 400;
    let mp = (m + 9) % 12;
    let dd = (153 * mp + 2) / 5 + d - 1;
    let de = ae * 365 + ae / 4 - ae / 100 + dd;
    era * 146_097 + de - 719_468
}

fn civil_desde_dias(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let de = z - era * 146_097;
    let ae = (de - de / 1460 + de / 36_524 - de / 146_096) / 365;
    let dd = de - (365 * ae + ae / 4 - ae / 100);
    let mp = (5 * dd + 2) / 153;
    let d = dd - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { ae + era * 400 + 1 } else { ae + era * 400 }, m, d)
}

/// `2026-11-16T12:00:00Z` → segundos desde 1970.
pub fn leer_fecha(t: &str) -> Option<u64> {
    let b = t.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' || b[19] != b'Z' {
        return None;
    }
    let n = |r: core::ops::Range<usize>| t.get(r).filter(|x| x.bytes().all(|c| c.is_ascii_digit())).and_then(|x| x.parse::<i64>().ok());
    let (a, m, d, h, mi, s) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 || a < 1970 {
        return None;
    }
    Some((dias_desde_civil(a, m, d) * 86_400 + h * 3600 + mi * 60 + s) as u64)
}

pub fn fecha(s: u64) -> String {
    let (a, m, d) = civil_desde_dias((s / 86_400) as i64);
    let r = s % 86_400;
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", a, m, d, r / 3600, r / 60 % 60, r % 60)
}

/// La fecha corta, para la gente: `16/11/2026 12:00` (en UTC).
pub fn fecha_corta(s: u64) -> String {
    let (a, m, d) = civil_desde_dias((s / 86_400) as i64);
    let r = s % 86_400;
    format!("{:02}/{:02}/{:04} {:02}:{:02}", d, m, a, r / 3600, r / 60 % 60)
}

/// Desde cuándo una hora del reloj es creíble: 2024 (la de `hora_poner`). Antes es que la máquina no la sabe.
pub const CREIBLE_DESDE: u64 = 1_704_067_200;

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn sumas() {
        let t = con_suma("PKG-DB 2\nName: x\n");
        assert_eq!(sin_suma(&t), Some("PKG-DB 2\nName: x\n"));
        assert_eq!(sin_suma(&t.replace("x", "y")), None);
        assert_eq!(sin_suma(&t[..t.len() - 3]), None);
        assert_eq!(sin_suma("PKG-DB 2\nName: x\n"), None);
        assert_eq!(sin_suma(""), None);
    }

    #[test]
    fn versiones_y_fechas() {
        for t in ["1.0-1", "2:1.0-3", "1.0-beta-2", "v1.2_3+b~1-10"] {
            assert_eq!(version(&leer_version(t).unwrap()), t);
        }
        for t in ["1.0", "x:1.0-1", "1.0-", "-1", "1.0-01", "1 0-1", "a-1"] {
            assert_eq!(leer_version(t), None, "{t}");
        }
        assert_eq!(bonita(&leer_version("1.0-1").unwrap()), "1.0");
        assert_eq!(bonita(&leer_version("2:1.0-3").unwrap()), "2:1.0-3");
        assert_eq!(hueco(&leer_version("1.0-1").unwrap()), "0-1.0-1");
        assert_eq!(leer_fecha("1970-01-01T00:00:00Z"), Some(0));
        let s = leer_fecha("2026-11-16T12:34:56Z").unwrap();
        assert_eq!(fecha(s), "2026-11-16T12:34:56Z");
        assert_eq!(fecha_corta(s), "16/11/2026 12:34");
        assert_eq!(leer_fecha("2024-02-29T00:00:00Z").map(fecha).as_deref(), Some("2024-02-29T00:00:00Z"));
        for mala in ["2026-13-01T00:00:00Z", "2026-11-16 12:00:00Z", "2026-11-16T12:00:00", "hoy"] {
            assert_eq!(leer_fecha(mala), None);
        }
    }
}
