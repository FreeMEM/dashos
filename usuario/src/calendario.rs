//! S27, oleada 3: el calendario del mes del "Dash" (el que abre el reloj de la barra), como `DMonthGrid` de DMS
//! (DankMaterialShell v2.1.2, `DCommon/Widgets/DMonthGrid.qml` y `Modules/DDash/Overview/CalendarOverviewCard.qml`; MIT,
//! copyright (c) 2025 Avenge Media LLC; aviso entero en `docs/licencias/DankMaterialShell-MIT.txt`): siempre 6 semanas de
//! 7 días (42 casillas), empezando por el lunes (el primer día de la semana de es_ES, el de shiva), con los días del mes
//! de antes y del de después; el número de semana ISO; y moverse por días, semanas y meses.
//!
//! Las fechas, en días civiles (como `reloj.rs`). Sin `alloc` ni nada de DashOS: se prueba en el PC con
//! `rustc --edition 2024 --test usuario/src/calendario.rs`.

/// Un día: año, mes (1 a 12) y día (1 a 31).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Dia {
    pub anio: i64,
    pub mes: u32,
    pub dia: u32,
}

pub const MESES: [&str; 12] = ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"];
/// Las cabeceras de las columnas (`dayName(..., ShortFormat)` en es_ES), del lunes al domingo.
pub const CORTOS: [&str; 7] = ["lun", "mar", "mié", "jue", "vie", "sáb", "dom"];

pub fn bisiesto(a: i64) -> bool {
    (a % 4 == 0 && a % 100 != 0) || a % 400 == 0
}

pub fn dias_del_mes(a: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => if bisiesto(a) { 29 } else { 28 },
    }
}

impl Dia {
    pub fn new(anio: i64, mes: u32, dia: u32) -> Dia {
        let mes = mes.clamp(1, 12);
        Dia { anio, mes, dia: dia.clamp(1, dias_del_mes(anio, mes)) }
    }

    /// Los días desde el 1 de enero de 1970 (días civiles: desde el 1 de marzo del año 0, en eras de 400 años).
    pub fn numero(&self) -> i64 {
        let (a, m) = if self.mes <= 2 { (self.anio - 1, self.mes + 9) } else { (self.anio, self.mes - 3) };
        let era = a.div_euclid(400);
        let de_la_era = a.rem_euclid(400);
        let dia_del_anio = (153 * m as i64 + 2) / 5 + self.dia as i64 - 1;
        let dia_de_la_era = de_la_era * 365 + de_la_era / 4 - de_la_era / 100 + dia_del_anio;
        era * 146_097 + dia_de_la_era - 719_468
    }

    /// El día de un número de días desde 1970.
    pub fn de_numero(n: i64) -> Dia {
        let z = n + 719_468;
        let era = z.div_euclid(146_097);
        let de_la_era = z.rem_euclid(146_097);
        let anio_era = (de_la_era - de_la_era / 1460 + de_la_era / 36_524 - de_la_era / 146_096) / 365;
        let dia_del_anio = de_la_era - (365 * anio_era + anio_era / 4 - anio_era / 100);
        let mp = (5 * dia_del_anio + 2) / 153;
        let dia = (dia_del_anio - (153 * mp + 2) / 5 + 1) as u32;
        let mes = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        Dia { anio: anio_era + era * 400 + if mes <= 2 { 1 } else { 0 }, mes, dia }
    }

    /// El día de la semana: 0 lunes a 6 domingo (el 1 de enero de 1970 fue jueves).
    pub fn semana(&self) -> u32 {
        (self.numero() + 3).rem_euclid(7) as u32
    }

    /// `n` días después (o antes).
    pub fn mas(&self, n: i64) -> Dia {
        Dia::de_numero(self.numero().saturating_add(n))
    }

    /// `n` meses después (o antes), con el día recortado al último del mes si no lo tiene.
    pub fn mas_meses(&self, n: i64) -> Dia {
        let total = self.anio * 12 + self.mes as i64 - 1 + n;
        Dia::new(total.div_euclid(12), (total.rem_euclid(12) + 1) as u32, self.dia)
    }

    /// El número de semana ISO 8601 (la semana empieza el lunes; la 1 es la del primer jueves del año).
    pub fn semana_iso(&self) -> u32 {
        let jueves = self.mas(3 - self.semana() as i64);
        let primero = Dia::new(jueves.anio, 1, 1);
        ((jueves.numero() - primero.numero()) / 7 + 1) as u32
    }
}

/// Las 42 casillas del mes de `d` (de lunes a domingo, 6 semanas): el día y si es del mes.
pub fn rejilla(d: Dia) -> [(Dia, bool); 42] {
    let primero = Dia::new(d.anio, d.mes, 1);
    let inicio = primero.mas(-(primero.semana() as i64));
    core::array::from_fn(|i| {
        let x = inicio.mas(i as i64);
        (x, x.mes == d.mes && x.anio == d.anio)
    })
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn dias_conocidos() {
        assert_eq!(Dia::new(1970, 1, 1).numero(), 0);
        assert_eq!(Dia::new(1970, 1, 1).semana(), 3);
        // 2026-11-16 fue lunes; 2026-11-17, martes.
        assert_eq!(Dia::new(2026, 11, 16).semana(), 0);
        assert_eq!(Dia::new(2026, 11, 17).semana(), 1);
        assert_eq!(Dia::new(2000, 2, 29).semana(), 1);
        // Ida y vuelta, en un buen trozo de días (también antes de 1970).
        for n in (-800_000..800_000).step_by(997) {
            assert_eq!(Dia::de_numero(n).numero(), n);
        }
        assert_eq!((dias_del_mes(2024, 2), dias_del_mes(2100, 2), dias_del_mes(2000, 2), dias_del_mes(2026, 11)), (29, 28, 29, 30));
    }

    #[test]
    fn la_rejilla_de_noviembre() {
        let r = rejilla(Dia::new(2026, 11, 17));
        // Noviembre de 2026 empieza en domingo: la primera semana trae del 26 al 31 de octubre.
        assert_eq!(r[0].0, Dia::new(2026, 10, 26));
        assert!(!r[0].1 && r[6].1 && r[6].0.dia == 1);
        assert_eq!(r[41].0, Dia::new(2026, 12, 6));
        assert_eq!(r.iter().filter(|x| x.1).count(), 30);
        // Todas empiezan en lunes y son seguidas.
        for i in 0..42 {
            assert_eq!(r[i].0.semana(), i as u32 % 7);
            if i > 0 {
                assert_eq!(r[i].0.numero(), r[i - 1].0.numero() + 1);
            }
        }
        // Un febrero que empieza en lunes y tiene 28 días (2027): la primera casilla es el 1.
        let f = rejilla(Dia::new(2027, 2, 10));
        assert_eq!(f[0].0, Dia::new(2027, 2, 1));
    }

    #[test]
    fn moverse() {
        let d = Dia::new(2026, 1, 31);
        assert_eq!(d.mas_meses(1), Dia::new(2026, 2, 28));
        assert_eq!(d.mas_meses(-1), Dia::new(2025, 12, 31));
        assert_eq!(d.mas_meses(13), Dia::new(2027, 2, 28));
        assert_eq!(Dia::new(2024, 3, 31).mas_meses(-1), Dia::new(2024, 2, 29));
        assert_eq!(Dia::new(2026, 12, 31).mas(1), Dia::new(2027, 1, 1));
        assert_eq!(Dia::new(2026, 3, 1).mas(-1), Dia::new(2026, 2, 28));
        // Valores enormes no rompen.
        let _ = d.mas(i64::MAX / 4);
        let _ = d.mas_meses(1_000_000);
        assert_eq!(Dia::new(2026, 13, 40), Dia::new(2026, 12, 31));
    }

    #[test]
    fn semanas_iso() {
        assert_eq!(Dia::new(2026, 11, 17).semana_iso(), 47);
        assert_eq!(Dia::new(2026, 1, 1).semana_iso(), 1);
        // 2027-01-01 (viernes) es de la semana 53 de 2026.
        assert_eq!(Dia::new(2027, 1, 1).semana_iso(), 53);
        assert_eq!(Dia::new(2024, 12, 30).semana_iso(), 1);
    }
}
