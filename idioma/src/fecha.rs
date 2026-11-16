//! Fechas y horas con las plantillas de `FormatDate` de `locale.library` (`%d/%m/%Y`, `%H:%M`…), que son las que
//! traen `locale.prefs` y los `.country`.

use alloc::string::String;
use core::fmt::Write;

/// Los números de los textos de un idioma (los de `GetLocaleStr`): días, meses y poco más.
pub mod cadenas {
    /// El domingo; los demás días, seguidos hasta el sábado (7).
    pub const DIA_1: u32 = 1;
    /// El domingo abreviado; hasta el sábado (14).
    pub const DIA_CORTO_1: u32 = 8;
    /// Enero; hasta diciembre (26).
    pub const MES_1: u32 = 15;
    /// Enero abreviado; hasta diciembre (38).
    pub const MES_CORTO_1: u32 = 27;
    pub const SI: u32 = 39;
    pub const NO: u32 = 40;
    pub const AM: u32 = 41;
    pub const PM: u32 = 42;
    pub const GUION_BLANDO: u32 = 43;
    pub const GUION_DURO: u32 = 44;
    pub const COMILLA_ABRE: u32 = 45;
    pub const COMILLA_CIERRA: u32 = 46;
    pub const AYER: u32 = 47;
    pub const HOY: u32 = 48;
    pub const MANANA: u32 = 49;
    pub const FUTURO: u32 = 50;
    pub const ULTIMA: u32 = 50;
}

/// Los textos del castellano, el idioma de serie: del número 1 al 50.
pub const CASTELLANO: [&str; 50] = [
    "domingo", "lunes", "martes", "miércoles", "jueves", "viernes", "sábado",
    "dom", "lun", "mar", "mié", "jue", "vie", "sáb",
    "enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre",
    "ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic",
    "Sí", "No", "a. m.", "p. m.", "-", "-", "«", "»", "Ayer", "Hoy", "Mañana", "Futuro",
];

/// Un momento del calendario gregoriano.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Momento {
    pub ano: i32,
    /// De 1 a 12.
    pub mes: u8,
    pub dia: u8,
    pub hora: u8,
    pub minuto: u8,
    pub segundo: u8,
    /// 0, domingo.
    pub dia_semana: u8,
    /// Del 1 al 366.
    pub dia_ano: u16,
}

fn bisiesto(ano: i32) -> bool {
    ano % 4 == 0 && (ano % 100 != 0 || ano % 400 == 0)
}

impl Momento {
    /// El momento de `segundos` desde 1970 (ya en la hora local). Fuera de los años 1 a 9999 se queda en el borde.
    pub fn de_unix(segundos: i64) -> Momento {
        // Del 1-1-0001 al 31-12-9999.
        let segundos = segundos.clamp(-62_135_596_800, 253_402_300_799);
        let dias = segundos.div_euclid(86_400);
        let resto = segundos.rem_euclid(86_400);
        // Días desde 1970 a año, mes y día: por eras de 400 años (146 097 días) que empiezan un 1 de marzo.
        let z = dias + 719_468;
        let era = z.div_euclid(146_097);
        let de_era = z.rem_euclid(146_097);
        let ano_de_era = (de_era - de_era / 1_460 + de_era / 36_524 - de_era / 146_096) / 365;
        let dia_de_ano = de_era - (365 * ano_de_era + ano_de_era / 4 - ano_de_era / 100);
        let mp = (5 * dia_de_ano + 2) / 153;
        let dia = (dia_de_ano - (153 * mp + 2) / 5 + 1) as u8;
        let mes = (if mp < 10 { mp + 3 } else { mp - 9 }) as u8;
        let ano = (ano_de_era + era * 400 + if mes <= 2 { 1 } else { 0 }) as i32;
        const ANTES: [u16; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
        let dia_ano = ANTES[mes as usize - 1] + dia as u16 + if mes > 2 && bisiesto(ano) { 1 } else { 0 };
        Momento {
            ano,
            mes,
            dia,
            hora: (resto / 3_600) as u8,
            minuto: (resto / 60 % 60) as u8,
            segundo: (resto % 60) as u8,
            // El 1-1-1970 fue jueves.
            dia_semana: (dias + 4).rem_euclid(7) as u8,
            dia_ano,
        }
    }
}

/// Lo más que mide una fecha escrita: una plantilla hostil no hace crecer el texto sin fin.
pub const MAX_SALIDA: usize = 512;

/// Escribe `m` según `plantilla`. `cadena` da los textos del idioma por su número (`cadenas`). Un código que no existe
/// sale tal cual.
pub fn formatear<'a>(plantilla: &str, m: &Momento, cadena: &dyn Fn(u32) -> &'a str) -> String {
    let mut s = String::new();
    escribir(plantilla, m, cadena, &mut s, 0);
    s
}

fn escribir<'a>(plantilla: &str, m: &Momento, cadena: &dyn Fn(u32) -> &'a str, s: &mut String, nivel: u8) {
    let hora12 = if m.hora % 12 == 0 { 12 } else { m.hora % 12 };
    let mut letras = plantilla.chars();
    while let Some(c) = letras.next() {
        if s.len() >= MAX_SALIDA {
            return;
        }
        if c != '%' {
            s.push(c);
            continue;
        }
        let Some(codigo) = letras.next() else {
            s.push('%');
            return;
        };
        // Los códigos compuestos no se meten unos dentro de otros más de una vez.
        let compuesto = |p: &str, s: &mut String| {
            if nivel == 0 {
                escribir(p, m, cadena, s, 1);
            }
        };
        let _ = match codigo {
            'a' => Ok(s.push_str(cadena(cadenas::DIA_CORTO_1 + m.dia_semana as u32))),
            'A' => Ok(s.push_str(cadena(cadenas::DIA_1 + m.dia_semana as u32))),
            'b' | 'h' => Ok(s.push_str(cadena(cadenas::MES_CORTO_1 + m.mes as u32 - 1))),
            'B' => Ok(s.push_str(cadena(cadenas::MES_1 + m.mes as u32 - 1))),
            'c' => Ok(compuesto("%a %b %d %H:%M:%S %Y", s)),
            'C' => Ok(compuesto("%a %b %e %T %Y", s)),
            'd' => write!(s, "{:02}", m.dia),
            'D' | 'x' => Ok(compuesto("%m/%d/%y", s)),
            'e' => write!(s, "{:2}", m.dia),
            'H' => write!(s, "{:02}", m.hora),
            'I' => write!(s, "{:02}", hora12),
            'j' => write!(s, "{:03}", m.dia_ano),
            'm' => write!(s, "{:02}", m.mes),
            'M' => write!(s, "{:02}", m.minuto),
            'n' => Ok(s.push('\n')),
            'p' => Ok(s.push_str(cadena(if m.hora < 12 { cadenas::AM } else { cadenas::PM }))),
            'q' => write!(s, "{}", m.hora),
            'Q' => write!(s, "{}", hora12),
            'r' => Ok(compuesto("%I:%M:%S %p", s)),
            'R' => Ok(compuesto("%H:%M", s)),
            'S' => write!(s, "{:02}", m.segundo),
            't' => Ok(s.push('\t')),
            'T' | 'X' => Ok(compuesto("%H:%M:%S", s)),
            // La semana del año, con el domingo como primer día.
            'U' => write!(s, "{:02}", (m.dia_ano as u32 + 6 - m.dia_semana as u32) / 7),
            'w' => write!(s, "{}", m.dia_semana),
            'y' => write!(s, "{:02}", m.ano.rem_euclid(100)),
            'Y' => write!(s, "{}", m.ano),
            '%' => Ok(s.push('%')),
            otro => {
                s.push('%');
                Ok(s.push(otro))
            }
        };
    }
}

/// Quita el espacio de delante y los dobles de en medio: `%e` rellena con un espacio los días de una cifra, y en una
/// frase ("lunes,  3 de noviembre") sobra.
pub fn sin_huecos(texto: &str) -> String {
    let mut s = String::with_capacity(texto.len());
    let mut antes = true;
    for c in texto.chars() {
        let espacio = c == ' ';
        if !(espacio && antes) {
            s.push(c);
        }
        antes = espacio;
    }
    s
}
