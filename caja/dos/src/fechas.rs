//! DateToStr: una `DateStamp` en texto (dos/datetime.h). Lo comprobado contra el oráculo (`pruebas68k/dosasl.c` en
//! FS-UAE con el Kickstart):
//!
//! - Los formatos: `FORMAT_DOS` (0) "dd-Mmm-yy", `FORMAT_INT` (1) "yy-mm-dd", `FORMAT_USA` (2) "mm-dd-yy" y
//!   `FORMAT_CDN` (3) "dd-mm-yy"; cualquier otro, como `FORMAT_DOS`. Los meses y los días, en inglés.
//! - La hora, "hh:mm:ss" (los tics, en segundos enteros).
//! - Con `DTF_SUBST`, la fecha es "Today", "Yesterday" o "Tomorrow" si toca; de 2 a 7 días atrás, el día de la
//!   semana; si no, la fecha.
//! - Devuelve DOSTRUE. Cada texto va solo si su puntero no es 0.
//!
//! StrToDate, al revés (`pruebas68k/dos2.c`):
//!
//! - Lee la fecha de `dat_StrDate` en el formato de `dat_Format` y la hora de `dat_StrTime` ("hh:mm" o "hh:mm:ss").
//!   Cada parte cambia solo si su puntero no es 0: la fecha, `ds_Days`; la hora, `ds_Minute` y `ds_Tick`.
//! - Los años de dos cifras: de 78 a 99 son 19xx; de 00 a 77, 20xx.
//! - Una fecha que no existe (30-Feb) o mal escrita devuelve 0 sin tocar nada. Una hora mala devuelve 0, pero la
//!   fecha ya leída se queda puesta.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::estructuras::*;
use crate::*;

/// `struct DateTime`.
mod datetime {
    pub const DAT_STAMP: u32 = 0;
    pub const DAT_FORMAT: u32 = 12;
    pub const DAT_FLAGS: u32 = 13;
    pub const DAT_STRDAY: u32 = 14;
    pub const DAT_STRDATE: u32 = 18;
    pub const DAT_STRTIME: u32 = 22;
    pub const TAMANO: u32 = 26;
}

const DTF_SUBST: u8 = 1;
const FORMAT_INT: u8 = 1;
const FORMAT_USA: u8 = 2;
const FORMAT_CDN: u8 = 3;

const DIAS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MESES: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// (año, mes 1-12, día 1-31) de `dias` desde el 1 de enero de 1978.
fn civil(dias: i64) -> (i64, u32, u32) {
    // Días desde el 1 de marzo del año 0 (los años empiezan en marzo: el día de más de febrero va al final).
    let z = dias + 2922 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

/// El texto de la fecha de `dias` en `formato`.
pub(crate) fn texto_fecha(dias: i64, formato: u8) -> String {
    let (y, m, d) = civil(dias);
    let yy = y.rem_euclid(100);
    match formato {
        FORMAT_INT => format!("{:02}-{:02}-{:02}", yy, m, d),
        FORMAT_USA => format!("{:02}-{:02}-{:02}", m, d, yy),
        FORMAT_CDN => format!("{:02}-{:02}-{:02}", d, m, yy),
        _ => format!("{:02}-{}-{:02}", d, MESES[(m - 1) as usize], yy),
    }
}

/// Días desde el 1 de enero de 1978 de (año, mes, día), sin comprobar.
fn dias_de(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468 - 2922
}

/// Un número de una o dos cifras.
fn cifras(t: &[u8]) -> Option<u32> {
    if t.is_empty() || t.len() > 2 || !t.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(t.iter().fold(0, |n, &b| n * 10 + (b - b'0') as u32))
}

/// Los días de una fecha escrita en `formato`, o None si está mal o no existe.
pub(crate) fn fecha_de_texto(t: &[u8], formato: u8) -> Option<i64> {
    let partes: Vec<&[u8]> = t.split(|&b| b == b'-').collect();
    let [a, b, c] = partes[..] else { return None };
    let (d, m, y) = match formato {
        FORMAT_INT => (cifras(c)?, cifras(b)?, cifras(a)?),
        FORMAT_USA => (cifras(b)?, cifras(a)?, cifras(c)?),
        FORMAT_CDN => (cifras(a)?, cifras(b)?, cifras(c)?),
        _ => {
            let m = MESES.iter().position(|n| n.as_bytes().eq_ignore_ascii_case(b))? as u32 + 1;
            (cifras(a)?, m, cifras(c)?)
        }
    };
    let y = if y < 78 { 2000 + y } else { 1900 + y } as i64;
    if !(1..=12).contains(&m) || d == 0 {
        return None;
    }
    let dias = dias_de(y, m, d);
    // El día existe si al volver da la misma fecha (30-Feb no).
    (civil(dias) == (y, m, d)).then_some(dias)
}

/// (minutos, tics) de una hora "hh:mm" o "hh:mm:ss", o None.
pub(crate) fn hora_de_texto(t: &[u8]) -> Option<(u32, u32)> {
    let partes: Vec<&[u8]> = t.split(|&b| b == b':').collect();
    let (h, m, s) = match partes[..] {
        [h, m] => (cifras(h)?, cifras(m)?, 0),
        [h, m, s] => (cifras(h)?, cifras(m)?, cifras(s)?),
        _ => return None,
    };
    (h < 24 && m < 60 && s < 60).then_some((h * 60 + m, s * TICKS_PER_SECOND))
}

fn dia_de_la_semana(dias: i64) -> &'static str {
    DIAS[dias.rem_euclid(7) as usize]
}

impl<F: SistemaArchivos> Dos<F> {
    /// DateToStr(datetime).
    pub(crate) fn date_to_str<C: Caja>(&mut self, c: &mut C, dt: u32) -> R<u32> {
        Self::tramo(c, dt, datetime::TAMANO)?;
        let ds = dt + datetime::DAT_STAMP;
        let dias = Self::l32(c, ds + datestamp::DS_DAYS)? as i32 as i64;
        let minutos = Self::l32(c, ds + datestamp::DS_MINUTE)?;
        let tics = Self::l32(c, ds + datestamp::DS_TICK)?;
        let formato = Self::l8(c, dt + datetime::DAT_FORMAT)?;
        let banderas = Self::l8(c, dt + datetime::DAT_FLAGS)?;
        let mut fecha = texto_fecha(dias, formato);
        if banderas & DTF_SUBST != 0 {
            let (s, _) = c.hora();
            let hoy = (s / 86400) as i64 - DIAS_1970_A_1978 as i64;
            match dias - hoy {
                0 => fecha = String::from("Today"),
                -1 => fecha = String::from("Yesterday"),
                1 => fecha = String::from("Tomorrow"),
                -7..=-2 => fecha = String::from(dia_de_la_semana(dias)),
                _ => {}
            }
        }
        let segundos = tics / TICKS_PER_SECOND;
        let hora = format!("{:02}:{:02}:{:02}", minutos / 60, minutos % 60, segundos);
        for (campo, texto) in [
            (datetime::DAT_STRDAY, dia_de_la_semana(dias)),
            (datetime::DAT_STRDATE, fecha.as_str()),
            (datetime::DAT_STRTIME, hora.as_str()),
        ] {
            let p = Self::l32(c, dt + campo)?;
            if p != 0 {
                let mut b: Vec<u8> = texto.bytes().collect();
                b.push(0);
                Self::poner(c, p, &b)?;
            }
        }
        Ok(DOSTRUE)
    }

    /// StrToDate(datetime).
    pub(crate) fn str_to_date<C: Caja>(&mut self, c: &mut C, dt: u32) -> R<u32> {
        Self::tramo(c, dt, datetime::TAMANO)?;
        let ds = dt + datetime::DAT_STAMP;
        let formato = Self::l8(c, dt + datetime::DAT_FORMAT)?;
        let pf = Self::l32(c, dt + datetime::DAT_STRDATE)?;
        if pf != 0 {
            match fecha_de_texto(&Self::cadena(c, pf)?, formato) {
                Some(d) => Self::e32(c, ds + datestamp::DS_DAYS, d as u32)?,
                None => return Ok(DOSFALSE),
            }
        }
        let ph = Self::l32(c, dt + datetime::DAT_STRTIME)?;
        if ph != 0 {
            match hora_de_texto(&Self::cadena(c, ph)?) {
                Some((m, t)) => {
                    Self::e32(c, ds + datestamp::DS_MINUTE, m)?;
                    Self::e32(c, ds + datestamp::DS_TICK, t)?;
                }
                None => return Ok(DOSFALSE),
            }
        }
        Ok(DOSTRUE)
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    /// Lo que da el oráculo (pruebas68k/dosasl.c).
    #[test]
    fn como_el_oraculo() {
        assert_eq!(texto_fecha(7000, 0), "02-Mar-97");
        assert_eq!(texto_fecha(7000, 1), "97-03-02");
        assert_eq!(texto_fecha(7000, 2), "03-02-97");
        assert_eq!(texto_fecha(7000, 3), "02-03-97");
        assert_eq!(texto_fecha(7000, 4), "02-Mar-97");
        assert_eq!(texto_fecha(7000, 9), "02-Mar-97");
        assert_eq!(texto_fecha(0, 0), "01-Jan-78");
        assert_eq!(texto_fecha(13148, 1), "13-12-31");
        assert_eq!(texto_fecha(1461, 2), "01-01-82");
        assert_eq!(dia_de_la_semana(7000), "Sunday");
        assert_eq!(dia_de_la_semana(0), "Sunday");
        assert_eq!(dia_de_la_semana(13148), "Tuesday");
        assert_eq!(dia_de_la_semana(1461), "Friday");
        // Un 29 de febrero (2000) y el día siguiente.
        assert_eq!(texto_fecha(8094, 1), "00-02-29");
        assert_eq!(texto_fecha(8095, 1), "00-03-01");
    }

    /// StrToDate, lo que da el oráculo (pruebas68k/dos2.c).
    #[test]
    fn al_reves() {
        assert_eq!(fecha_de_texto(b"02-Mar-97", 0), Some(7000));
        assert_eq!(fecha_de_texto(b"97-03-02", 1), Some(7000));
        assert_eq!(fecha_de_texto(b"03-02-97", 2), Some(7000));
        assert_eq!(fecha_de_texto(b"02-03-97", 3), Some(7000));
        assert_eq!(fecha_de_texto(b"01-Jan-78", 0), Some(0));
        assert_eq!(fecha_de_texto(b"31-Dec-13", 0), Some(13148));
        assert_eq!(fecha_de_texto(b"29-Feb-00", 0), Some(8094));
        assert_eq!(fecha_de_texto(b"30-Feb-00", 0), None);
        assert_eq!(fecha_de_texto(b"mal", 0), None);
        assert_eq!(hora_de_texto(b"12:30:24"), Some((750, 1200)));
        assert_eq!(hora_de_texto(b"01:02"), Some((62, 0)));
        assert_eq!(hora_de_texto(b"23:59:59"), Some((1439, 2950)));
        assert_eq!(hora_de_texto(b"25:00"), None);
    }
}
