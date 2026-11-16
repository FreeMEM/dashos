//! Zonas horarias (S13, paso 4): de la hora en UTC a la hora local de una zona. Una base de datos mínima, hecha aquí:
//! UTC, las zonas de Europa que siguen la regla de la Unión Europea (la directiva 2000/84/CE: el horario de verano
//! empieza el último domingo de marzo a la 01:00 UTC y acaba el último domingo de octubre a la 01:00 UTC), entre ellas
//! Europe/Madrid y Atlantic/Canary, y unas cuantas sin cambio de hora.
//!
//! Lo que falta: las reglas de otros sitios con cambio de hora (Estados Unidos y Canadá, Chile, Australia...), la
//! historia (antes de 1996 Europa cambiaba en otras fechas; aquí toda fecha se calcula con la regla de hoy), los cambios
//! futuros de las reglas (habrá que poner al día esta tabla, o leer la base de datos de zonas de la IANA) y las zonas
//! escritas al estilo POSIX ("CET-1CEST,M3.5.0,M10.5.0/3").
//!
//! Sin `alloc` ni nada de DashOS: se prueba en el PC con `rustc --edition 2024 --test usuario/src/zona.rs`.

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Regla {
    /// Sin cambio de hora.
    Fija,
    /// La de la Unión Europea: una hora más desde el último domingo de marzo hasta el último de octubre, a la 01:00 UTC.
    Ue,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Zona {
    pub nombre: &'static str,
    /// Minutos al este de UTC en invierno.
    pub minutos: i32,
    pub regla: Regla,
}

const fn ue(nombre: &'static str, horas: i32) -> Zona {
    Zona { nombre, minutos: horas * 60, regla: Regla::Ue }
}

const fn fija(nombre: &'static str, minutos: i32) -> Zona {
    Zona { nombre, minutos, regla: Regla::Fija }
}

pub const UTC: Zona = fija("UTC", 0);

pub const ZONAS: &[Zona] = &[
    UTC,
    // Hora de Europa occidental, central y oriental, con la regla de la Unión Europea (la siguen también el Reino Unido,
    // Suiza, Noruega y Ucrania).
    ue("Atlantic/Canary", 0),
    ue("Europe/Lisbon", 0),
    ue("Europe/London", 0),
    ue("Europe/Dublin", 0),
    ue("Europe/Madrid", 1),
    ue("Africa/Ceuta", 1),
    ue("Europe/Andorra", 1),
    ue("Europe/Paris", 1),
    ue("Europe/Brussels", 1),
    ue("Europe/Amsterdam", 1),
    ue("Europe/Berlin", 1),
    ue("Europe/Zurich", 1),
    ue("Europe/Vienna", 1),
    ue("Europe/Rome", 1),
    ue("Europe/Prague", 1),
    ue("Europe/Warsaw", 1),
    ue("Europe/Budapest", 1),
    ue("Europe/Copenhagen", 1),
    ue("Europe/Oslo", 1),
    ue("Europe/Stockholm", 1),
    ue("Europe/Athens", 2),
    ue("Europe/Helsinki", 2),
    ue("Europe/Sofia", 2),
    ue("Europe/Bucharest", 2),
    ue("Europe/Kyiv", 2),
    // Sin cambio de hora (en 2026).
    fija("Atlantic/Reykjavik", 0),
    fija("Europe/Istanbul", 180),
    fija("Europe/Moscow", 180),
    fija("Asia/Dubai", 240),
    fija("Asia/Kolkata", 330),
    fija("Asia/Shanghai", 480),
    fija("Asia/Tokyo", 540),
    fija("America/Argentina/Buenos_Aires", -180),
    fija("America/Sao_Paulo", -180),
    fija("America/Caracas", -240),
    fija("America/Bogota", -300),
    fija("America/Lima", -300),
    fija("America/Mexico_City", -360),
];

/// La zona con ese nombre (como en la base de datos de la IANA; sin distinguir mayúsculas, sin espacios alrededor).
pub fn buscar(nombre: &str) -> Option<&'static Zona> {
    let nombre = nombre.trim();
    ZONAS.iter().find(|z| z.nombre.eq_ignore_ascii_case(nombre))
}

/// Días desde el 1 de enero de 1970 del día `d` del mes `m` del año `a` (días civiles, con eras de 400 años).
fn dias(a: i64, m: i64, d: i64) -> i64 {
    let a = if m <= 2 { a - 1 } else { a };
    let era = a.div_euclid(400);
    let del_era = a.rem_euclid(400);
    let del_anio = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    era * 146_097 + del_era * 365 + del_era / 4 - del_era / 100 + del_anio - 719_468
}

/// El año del día `dias` (desde 1970).
fn anio(dias: i64) -> i64 {
    let z = dias + 719_468;
    let era = z.div_euclid(146_097);
    let de_la_era = z.rem_euclid(146_097);
    let anio_era = (de_la_era - de_la_era / 1460 + de_la_era / 36_524 - de_la_era / 146_096) / 365;
    let dia_del_anio = de_la_era - (365 * anio_era + anio_era / 4 - anio_era / 100);
    // El año civil empieza en enero; la cuenta, en marzo.
    anio_era + era * 400 + if dia_del_anio >= 306 { 1 } else { 0 }
}

/// El segundo (UTC, desde 1970) de la 01:00 UTC del último domingo del mes `m` (de 31 días) del año `a`.
fn ultimo_domingo(a: i64, m: i64) -> i64 {
    let ultimo = dias(a, m, 31);
    // El 1 de enero de 1970 fue jueves: con el domingo en 0, el día 0 es el 4.
    let semana = (ultimo + 4).rem_euclid(7);
    (ultimo - semana) * 86_400 + 3_600
}

/// Si en `utc_s` (segundos desde 1970, UTC) la zona está en horario de verano.
pub fn verano(z: &Zona, utc_s: u64) -> bool {
    match z.regla {
        Regla::Fija => false,
        Regla::Ue => {
            let s = utc_s.min(i64::MAX as u64 / 2) as i64;
            let a = anio(s.div_euclid(86_400));
            (ultimo_domingo(a, 3)..ultimo_domingo(a, 10)).contains(&s)
        }
    }
}

/// Los segundos que hay que sumar a `utc_s` para tener la hora local de la zona.
pub fn desfase_s(z: &Zona, utc_s: u64) -> i64 {
    (z.minutos as i64 + if verano(z, utc_s) { 60 } else { 0 }) * 60
}

/// La hora local de la zona, en nanosegundos "desde 1970" (para sacar la fecha con `reloj::fecha`).
pub fn local_ns(z: &Zona, utc_ns: u64) -> u64 {
    let d = desfase_s(z, utc_ns / 1_000_000_000) * 1_000_000_000;
    if d >= 0 { utc_ns.saturating_add(d as u64) } else { utc_ns.saturating_sub(d.unsigned_abs()) }
}

/// "UTC", "UTC+1", "UTC-3", "UTC+5:30": cómo va la zona respecto a UTC en `utc_s`. En `sal`; devuelve el largo.
pub fn etiqueta(z: &Zona, utc_s: u64, sal: &mut [u8; 10]) -> usize {
    let m = desfase_s(z, utc_s) / 60;
    sal[..3].copy_from_slice(b"UTC");
    if m == 0 {
        return 3;
    }
    sal[3] = if m < 0 { b'-' } else { b'+' };
    let (h, resto) = (m.unsigned_abs() / 60, m.unsigned_abs() % 60);
    let mut n = 4;
    if h >= 10 {
        sal[n] = b'0' + (h / 10) as u8;
        n += 1;
    }
    sal[n] = b'0' + (h % 10) as u8;
    n += 1;
    if resto != 0 {
        sal[n..n + 3].copy_from_slice(&[b':', b'0' + (resto / 10) as u8, b'0' + (resto % 10) as u8]);
        n += 3;
    }
    n
}

#[cfg(test)]
mod pruebas {
    use super::*;

    /// Segundos desde 1970 de una fecha y hora en UTC.
    fn utc(a: i64, m: i64, d: i64, h: i64, min: i64) -> u64 {
        (dias(a, m, d) * 86_400 + h * 3_600 + min * 60) as u64
    }

    fn eti(z: &Zona, s: u64) -> String {
        let mut b = [0u8; 10];
        let n = etiqueta(z, s, &mut b);
        String::from_utf8(b[..n].to_vec()).unwrap()
    }

    #[test]
    fn dias_y_anios() {
        assert_eq!((dias(1970, 1, 1), dias(2000, 3, 1), dias(2026, 11, 15)), (0, 11_017, 20_772));
        for (a, m, d) in [(1970, 1, 1), (1999, 12, 31), (2000, 1, 1), (2000, 2, 29), (2000, 3, 1), (2024, 2, 29), (2026, 12, 31), (2100, 3, 1), (2400, 2, 29)] {
            assert_eq!(anio(dias(a, m, d)), a, "{a}-{m}-{d}");
        }
    }

    #[test]
    fn madrid() {
        let z = buscar("Europe/Madrid").unwrap();
        // Invierno: UTC+1. 2026-11-15 12:00 UTC son las 13:00.
        let s = utc(2026, 11, 15, 12, 0);
        assert_eq!((desfase_s(z, s), verano(z, s), eti(z, s)), (3_600, false, "UTC+1".into()));
        assert_eq!(local_ns(z, s * 1_000_000_000) / 1_000_000_000, utc(2026, 11, 15, 13, 0));
        // Verano: UTC+2.
        let s = utc(2026, 7, 1, 12, 0);
        assert_eq!((desfase_s(z, s), eti(z, s)), (7_200, "UTC+2".into()));
        // El cambio de 2026: el 29 de marzo a la 01:00 UTC (las 2 pasan a ser las 3) y el 25 de octubre a la 01:00 UTC
        // (las 3 pasan a ser las 2).
        assert!(!verano(z, utc(2026, 3, 29, 0, 59)) && verano(z, utc(2026, 3, 29, 1, 0)));
        assert!(verano(z, utc(2026, 10, 25, 0, 59)) && !verano(z, utc(2026, 10, 25, 1, 0)));
        // Otros años: 2024 (31 de marzo y 27 de octubre), 2027 (28 de marzo y 31 de octubre), 2030 (31 y 27).
        for (a, marzo, octubre) in [(2024, 31, 27), (2025, 30, 26), (2027, 28, 31), (2028, 26, 29), (2030, 31, 27)] {
            assert!(!verano(z, utc(a, 3, marzo, 0, 59)) && verano(z, utc(a, 3, marzo, 1, 0)), "marzo de {a}");
            assert!(verano(z, utc(a, 10, octubre, 0, 59)) && !verano(z, utc(a, 10, octubre, 1, 0)), "octubre de {a}");
            assert!(!verano(z, utc(a, 1, 1, 0, 0)) && !verano(z, utc(a, 12, 31, 23, 59)) && verano(z, utc(a, 6, 15, 0, 0)));
        }
    }

    #[test]
    fn otras() {
        let (ver, inv) = (utc(2026, 7, 1, 12, 0), utc(2026, 1, 1, 12, 0));
        assert_eq!((eti(&UTC, ver), eti(&UTC, inv)), ("UTC".into(), "UTC".into()));
        let c = buscar("atlantic/canary ").unwrap();
        assert_eq!((eti(c, inv), eti(c, ver)), ("UTC".into(), "UTC+1".into()));
        let h = buscar("Europe/Helsinki").unwrap();
        assert_eq!((eti(h, inv), eti(h, ver)), ("UTC+2".into(), "UTC+3".into()));
        assert_eq!(eti(buscar("Asia/Kolkata").unwrap(), ver), "UTC+5:30");
        assert_eq!(eti(buscar("America/Bogota").unwrap(), ver), "UTC-5");
        let b = buscar("America/Bogota").unwrap();
        assert_eq!(local_ns(b, inv * 1_000_000_000) / 1_000_000_000, inv - 5 * 3_600);
        assert_eq!(buscar("Marte/Olympus"), None);
        assert_eq!(buscar(""), None);
        // Sin nombres repetidos.
        for (i, z) in ZONAS.iter().enumerate() {
            assert!(ZONAS[..i].iter().all(|o| !o.nombre.eq_ignore_ascii_case(z.nombre)), "{}", z.nombre);
        }
    }

    /// Hostil: cualquier instante y cualquier zona dan un desfase de la zona (con o sin su hora de verano), sin reventar.
    #[test]
    fn cualquier_valor() {
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        for v in [0, 1, u64::MAX, u64::MAX / 2, u64::MAX / 1_000_000_000].into_iter().chain((0..20_000).map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x >> (x % 40)
        })) {
            for z in ZONAS {
                let d = desfase_s(z, v);
                assert!(d == z.minutos as i64 * 60 || (z.regla == Regla::Ue && d == z.minutos as i64 * 60 + 3_600));
                let _ = local_ns(z, v);
                let mut b = [0u8; 10];
                assert!(etiqueta(z, v, &mut b) <= 10);
            }
        }
        // Con una zona al oeste y la hora a 0, no baja de 0.
        assert_eq!(local_ns(buscar("America/Lima").unwrap(), 5), 0);
    }
}
