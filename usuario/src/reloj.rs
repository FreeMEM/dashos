//! S27: la fecha y la hora de la barra, de los nanosegundos desde 1970 (UTC: aún no hay zona horaria). El día, con el
//! método de los días civiles (cuántos días van desde el 1 de marzo del año 0, en eras de 400 años), hecho aquí.
//!
//! Sin `alloc` ni nada de DashOS: se prueba en el PC con `rustc --edition 2024 --test usuario/src/reloj.rs`.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fecha {
    pub anio: i64,
    /// 1 a 12.
    pub mes: u32,
    /// 1 a 31.
    pub dia: u32,
    /// 0 lunes a 6 domingo.
    pub semana: u32,
    pub hora: u32,
    pub minuto: u32,
}

pub const DIAS: [&str; 7] = ["lun", "mar", "mié", "jue", "vie", "sáb", "dom"];
pub const MESES: [&str; 12] = ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic"];

/// La fecha de `ns` nanosegundos desde el 1 de enero de 1970 (UTC).
pub fn fecha(ns: u64) -> Fecha {
    let s = ns / 1_000_000_000;
    let dias = (s / 86_400) as i64;
    let en_el_dia = s % 86_400;
    // El 1 de enero de 1970 fue jueves (3, con el lunes en 0).
    let semana = ((dias + 3).rem_euclid(7)) as u32;
    // Días civiles: desde el 1 de marzo del año 0.
    let z = dias + 719_468;
    let era = z.div_euclid(146_097);
    let de_la_era = z.rem_euclid(146_097);
    let anio_era = (de_la_era - de_la_era / 1460 + de_la_era / 36_524 - de_la_era / 146_096) / 365;
    let dia_del_anio = de_la_era - (365 * anio_era + anio_era / 4 - anio_era / 100);
    let mp = (5 * dia_del_anio + 2) / 153;
    let dia = (dia_del_anio - (153 * mp + 2) / 5 + 1) as u32;
    let mes = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let anio = anio_era + era * 400 + if mes <= 2 { 1 } else { 0 };
    Fecha { anio, mes, dia, semana, hora: (en_el_dia / 3600) as u32, minuto: (en_el_dia / 60 % 60) as u32 }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn ns(s: u64) -> u64 {
        s * 1_000_000_000
    }

    #[test]
    fn fechas_conocidas() {
        let f = fecha(ns(0));
        assert_eq!((f.anio, f.mes, f.dia, DIAS[f.semana as usize], f.hora, f.minuto), (1970, 1, 1, "jue", 0, 0));
        // 2000-02-29 (bisiesto) 12:34, martes.
        let f = fecha(ns(951_827_640));
        assert_eq!((f.anio, f.mes, f.dia, DIAS[f.semana as usize], f.hora, f.minuto), (2000, 2, 29, "mar", 12, 34));
        // 2026-11-13 04:53, viernes.
        let f = fecha(ns(1_794_545_580));
        assert_eq!((f.anio, f.mes, f.dia, DIAS[f.semana as usize], MESES[f.mes as usize - 1], f.hora, f.minuto), (2026, 11, 13, "vie", "nov", 4, 53));
        // 2100-03-01 (2100 no es bisiesto), lunes.
        let f = fecha(ns(4_107_542_400));
        assert_eq!((f.anio, f.mes, f.dia, DIAS[f.semana as usize]), (2100, 3, 1, "lun"));
    }

    /// Hostil: cualquier valor da una fecha que vale (nada se desborda ni sale de los nombres).
    #[test]
    fn cualquier_valor() {
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        for v in [0, 1, u64::MAX, u64::MAX / 2].into_iter().chain((0..10_000).map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        })) {
            let f = fecha(v);
            assert!((1..=12).contains(&f.mes) && (1..=31).contains(&f.dia) && f.semana < 7 && f.hora < 24 && f.minuto < 60, "{v}: {f:?}");
        }
    }
}
