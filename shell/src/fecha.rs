//! La fecha de unos segundos desde 1970, para `List`, `Date` y la hora del indicador. Lo que se le da ya va en la hora
//! local: quien la escribe suma antes el desfase de la zona del sistema (`con_desfase`; en DashOS, `Sistema::local_s`
//! del crate de las órdenes, con `SYS:Prefs/zona`, como la barra y el crate `idioma`).

use alloc::format;
use alloc::string::String;

/// Año, mes (1 a 12), día, hora, minuto y segundo.
pub fn partes(segundos: u64) -> (i64, u32, u32, u32, u32, u32) {
    let dias = (segundos / 86_400) as i64;
    let resto = segundos % 86_400;
    // Días civiles: desde el 1 de marzo del año 0, en eras de 400 años.
    let z = dias + 719_468;
    let era = z.div_euclid(146_097);
    let de = z.rem_euclid(146_097);
    let ae = (de - de / 1460 + de / 36_524 - de / 146_096) / 365;
    let da = de - (365 * ae + ae / 4 - ae / 100);
    let mp = (5 * da + 2) / 153;
    let dia = (da - (153 * mp + 2) / 5 + 1) as u32;
    let mes = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let anio = ae + era * 400 + if mes <= 2 { 1 } else { 0 };
    (anio, mes, dia, (resto / 3600) as u32, (resto / 60 % 60) as u32, (resto % 60) as u32)
}

/// Una hora del reloj (segundos desde 1970, en UTC) con un desfase de zona (segundos al este de UTC), sin pasar de los
/// extremos.
pub fn con_desfase(utc_s: u64, desfase_s: i64) -> u64 {
    if desfase_s >= 0 { utc_s.saturating_add(desfase_s as u64) } else { utc_s.saturating_sub(desfase_s.unsigned_abs()) }
}

/// `2026-11-15 16:30:05`.
pub fn texto(segundos: u64) -> String {
    let (a, m, d, h, mi, s) = partes(segundos);
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", a, m, d, h, mi, s)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn fechas() {
        assert_eq!(texto(0), "1970-01-01 00:00:00");
        assert_eq!(texto(951_782_400), "2000-02-29 00:00:00");
        assert_eq!(texto(1_794_760_205), "2026-11-15 16:30:05");
        assert_eq!(texto(4_102_444_799), "2099-12-31 23:59:59");
    }

    #[test]
    fn desfases() {
        // Madrid en invierno (+1 h) y en verano (+2 h), y una zona al oeste.
        assert_eq!(texto(con_desfase(1_794_760_205, 3600)), "2026-11-15 17:30:05");
        assert_eq!(texto(con_desfase(1_794_760_205, 7200)), "2026-11-15 18:30:05");
        assert_eq!(texto(con_desfase(1_794_760_205, -3 * 3600)), "2026-11-15 13:30:05");
        // Al cruzar la medianoche y el año.
        assert_eq!(texto(con_desfase(1_798_761_599, 3600)), "2027-01-01 00:59:59");
        // Hostil: los extremos no dan la vuelta.
        assert_eq!(con_desfase(10, -3600), 0);
        assert_eq!(con_desfase(u64::MAX - 5, 3600), u64::MAX);
        assert_eq!(con_desfase(0, i64::MIN), 0);
        assert_eq!(con_desfase(5, i64::MAX), i64::MAX as u64 + 5);
    }
}
