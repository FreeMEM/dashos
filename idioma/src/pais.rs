//! El país: cómo se escriben las fechas, las horas, los números y el dinero. Es el `CountryPrefs` de AmigaOS (504
//! bytes), que va dentro de `locale.prefs` y, suelto, en cada `LOCALE:Countries/<país>.country` (IFF `PREF` con un
//! trozo `CTRY`).

use crate::iff;
use crate::letras::{de_campo, en_campo};
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

/// Lo que mide un `CountryPrefs`.
pub const LARGO: usize = 504;

/// Sistemas de medida.
pub const MEDIDAS_ISO: u8 = 0;
pub const MEDIDAS_EEUU: u8 = 1;
pub const MEDIDAS_IMPERIAL: u8 = 2;
pub const MEDIDAS_BRITANICO: u8 = 3;

/// Dónde va el signo de una cantidad de dinero.
pub const SIGNO_PARENTESIS: u8 = 0;
pub const SIGNO_DELANTE: u8 = 1;
pub const SIGNO_DETRAS: u8 = 2;
pub const SIGNO_ANTES_DE_MONEDA: u8 = 3;
pub const SIGNO_TRAS_MONEDA: u8 = 4;

/// Cómo se escribe una cantidad de dinero, positiva o negativa.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Signo {
    pub signo: String,
    /// Si hay un espacio entre el símbolo de la moneda y la cantidad.
    pub espacio: bool,
    /// `SIGNO_*`.
    pub donde: u8,
    /// Si el símbolo de la moneda va detrás de la cantidad.
    pub moneda_detras: bool,
}

/// Cómo se agrupan las cifras: el separador y el tamaño de cada grupo desde la coma (0, como el anterior; 255, no más
/// grupos).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cifras {
    pub decimal: String,
    pub grupo: String,
    pub grupo_frac: String,
    pub agrupar: [u8; 10],
    pub agrupar_frac: [u8; 10],
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pais {
    pub codigo: u32,
    pub telefono: u32,
    /// `MEDIDAS_*`.
    pub medidas: u8,
    pub fecha_hora: String,
    pub fecha: String,
    pub hora: String,
    pub fecha_hora_corta: String,
    pub fecha_corta: String,
    pub hora_corta: String,
    pub numeros: Cifras,
    pub dinero: Cifras,
    /// Cuántos decimales lleva el dinero (2 en el euro).
    pub decimales: u8,
    pub decimales_int: u8,
    /// El símbolo de la moneda (`€`), el de la fracción (`cts`) y el internacional (`EUR`).
    pub moneda: String,
    pub moneda_pequena: String,
    pub moneda_int: String,
    pub positivo: Signo,
    pub negativo: Signo,
    /// El primer día de la semana: 0, domingo; 1, lunes…
    pub calendario: u8,
}

/// Dónde está cada campo en los 504 bytes.
mod en {
    pub const CODIGO: usize = 16;
    pub const TELEFONO: usize = 20;
    pub const MEDIDAS: usize = 24;
    pub const FECHA_HORA: usize = 25;
    pub const FECHA: usize = 105;
    pub const HORA: usize = 145;
    pub const FECHA_HORA_CORTA: usize = 185;
    pub const FECHA_CORTA: usize = 265;
    pub const HORA_CORTA: usize = 305;
    pub const NUMEROS: usize = 345;
    pub const DINERO: usize = 395;
    pub const DECIMALES: usize = 445;
    pub const DECIMALES_INT: usize = 446;
    pub const MONEDA: usize = 447;
    pub const MONEDA_PEQUENA: usize = 457;
    pub const MONEDA_INT: usize = 467;
    pub const POSITIVO: usize = 477;
    pub const NEGATIVO: usize = 490;
    pub const CALENDARIO: usize = 503;
}

fn cifras(b: &[u8]) -> Cifras {
    Cifras {
        decimal: de_campo(&b[0..10]),
        grupo: de_campo(&b[10..20]),
        grupo_frac: de_campo(&b[20..30]),
        agrupar: b[30..40].try_into().unwrap(),
        agrupar_frac: b[40..50].try_into().unwrap(),
    }
}

fn signo(b: &[u8]) -> Signo {
    Signo { signo: de_campo(&b[0..10]), espacio: b[10] != 0, donde: b[11], moneda_detras: b[12] != 0 }
}

impl Pais {
    /// De los 504 bytes de un `CountryPrefs`. `None` si no llegan.
    pub fn de_datos(b: &[u8]) -> Option<Pais> {
        let b = b.get(..LARGO)?;
        let u32_en = |i: usize| u32::from_be_bytes(b[i..i + 4].try_into().unwrap());
        Some(Pais {
            codigo: u32_en(en::CODIGO),
            telefono: u32_en(en::TELEFONO),
            medidas: b[en::MEDIDAS],
            fecha_hora: de_campo(&b[en::FECHA_HORA..en::FECHA]),
            fecha: de_campo(&b[en::FECHA..en::HORA]),
            hora: de_campo(&b[en::HORA..en::FECHA_HORA_CORTA]),
            fecha_hora_corta: de_campo(&b[en::FECHA_HORA_CORTA..en::FECHA_CORTA]),
            fecha_corta: de_campo(&b[en::FECHA_CORTA..en::HORA_CORTA]),
            hora_corta: de_campo(&b[en::HORA_CORTA..en::NUMEROS]),
            numeros: cifras(&b[en::NUMEROS..en::DINERO]),
            dinero: cifras(&b[en::DINERO..en::DECIMALES]),
            decimales: b[en::DECIMALES],
            decimales_int: b[en::DECIMALES_INT],
            moneda: de_campo(&b[en::MONEDA..en::MONEDA_PEQUENA]),
            moneda_pequena: de_campo(&b[en::MONEDA_PEQUENA..en::MONEDA_INT]),
            moneda_int: de_campo(&b[en::MONEDA_INT..en::POSITIVO]),
            positivo: signo(&b[en::POSITIVO..en::NEGATIVO]),
            negativo: signo(&b[en::NEGATIVO..en::CALENDARIO]),
            calendario: b[en::CALENDARIO],
        })
    }

    /// Los 504 bytes.
    pub fn a_datos(&self) -> [u8; LARGO] {
        let mut b = [0u8; LARGO];
        b[en::CODIGO..en::CODIGO + 4].copy_from_slice(&self.codigo.to_be_bytes());
        b[en::TELEFONO..en::TELEFONO + 4].copy_from_slice(&self.telefono.to_be_bytes());
        b[en::MEDIDAS] = self.medidas;
        en_campo(&self.fecha_hora, &mut b[en::FECHA_HORA..en::FECHA]);
        en_campo(&self.fecha, &mut b[en::FECHA..en::HORA]);
        en_campo(&self.hora, &mut b[en::HORA..en::FECHA_HORA_CORTA]);
        en_campo(&self.fecha_hora_corta, &mut b[en::FECHA_HORA_CORTA..en::FECHA_CORTA]);
        en_campo(&self.fecha_corta, &mut b[en::FECHA_CORTA..en::HORA_CORTA]);
        en_campo(&self.hora_corta, &mut b[en::HORA_CORTA..en::NUMEROS]);
        let cifras = |c: &Cifras, b: &mut [u8]| {
            en_campo(&c.decimal, &mut b[0..10]);
            en_campo(&c.grupo, &mut b[10..20]);
            en_campo(&c.grupo_frac, &mut b[20..30]);
            b[30..40].copy_from_slice(&c.agrupar);
            b[40..50].copy_from_slice(&c.agrupar_frac);
        };
        cifras(&self.numeros, &mut b[en::NUMEROS..en::DINERO]);
        cifras(&self.dinero, &mut b[en::DINERO..en::DECIMALES]);
        b[en::DECIMALES] = self.decimales;
        b[en::DECIMALES_INT] = self.decimales_int;
        en_campo(&self.moneda, &mut b[en::MONEDA..en::MONEDA_PEQUENA]);
        en_campo(&self.moneda_pequena, &mut b[en::MONEDA_PEQUENA..en::MONEDA_INT]);
        en_campo(&self.moneda_int, &mut b[en::MONEDA_INT..en::POSITIVO]);
        let signo = |s: &Signo, b: &mut [u8]| {
            en_campo(&s.signo, &mut b[0..10]);
            b[10] = s.espacio as u8;
            b[11] = s.donde;
            b[12] = s.moneda_detras as u8;
        };
        signo(&self.positivo, &mut b[en::POSITIVO..en::NEGATIVO]);
        signo(&self.negativo, &mut b[en::NEGATIVO..en::CALENDARIO]);
        b[en::CALENDARIO] = self.calendario;
        b
    }

    /// De un fichero `.country`: IFF `PREF` con un trozo `CTRY`.
    pub fn leer(bytes: &[u8]) -> Option<Pais> {
        for t in iff::forma(bytes, b"PREF").ok()? {
            let t = t.ok()?;
            if &t.id == b"CTRY" {
                return Pais::de_datos(t.datos).filter(Pais::completo);
            }
        }
        None
    }

    /// El fichero `.country`.
    pub fn escribir(&self) -> Vec<u8> {
        let mut cuerpo = Vec::new();
        iff::trozo(b"PRHD", &[0; 6], &mut cuerpo);
        iff::trozo(b"CTRY", &self.a_datos(), &mut cuerpo);
        iff::escribir_forma(b"PREF", &cuerpo)
    }

    /// Si trae lo mínimo para escribir fechas y números: plantillas de fecha y hora con algún código y una coma
    /// decimal. Unos datos a ceros (un `locale.prefs` hecho a mano) o de basura no valen.
    pub fn completo(&self) -> bool {
        self.fecha_corta.contains('%') && self.hora_corta.contains('%') && !self.numeros.decimal.is_empty()
    }

    /// España, el país de serie.
    pub fn espana() -> Pais {
        let cifras = Cifras { decimal: ",".into(), grupo: ".".into(), grupo_frac: String::new(), agrupar: [3, 0, 0, 0, 0, 0, 0, 0, 0, 0], agrupar_frac: [255, 0, 0, 0, 0, 0, 0, 0, 0, 0] };
        Pais {
            codigo: u32::from_be_bytes(*b"ES\0\0"),
            telefono: 34,
            medidas: MEDIDAS_ISO,
            fecha_hora: "%A, %e de %B de %Y, %H:%M:%S".into(),
            fecha: "%A, %e de %B de %Y".into(),
            hora: "%H:%M:%S".into(),
            fecha_hora_corta: "%d/%m/%Y %H:%M".into(),
            fecha_corta: "%d/%m/%Y".into(),
            hora_corta: "%H:%M".into(),
            numeros: cifras.clone(),
            dinero: cifras,
            decimales: 2,
            decimales_int: 2,
            moneda: "€".into(),
            moneda_pequena: "cts".into(),
            moneda_int: "EUR".into(),
            positivo: Signo { signo: String::new(), espacio: true, donde: SIGNO_DELANTE, moneda_detras: true },
            negativo: Signo { signo: "-".into(), espacio: true, donde: SIGNO_DELANTE, moneda_detras: true },
            calendario: 1,
        }
    }

    /// Un entero con sus separadores de miles: 1234567 es `1.234.567` en España.
    pub fn numero(&self, n: i64) -> String {
        let mut s = String::new();
        if n < 0 {
            s.push('-');
        }
        agrupar(n.unsigned_abs(), &self.numeros, &mut s);
        s
    }

    /// Un número con decimales, dado sin coma: `decimal(123456, 2)` es `1.234,56` en España.
    pub fn decimal(&self, valor: i64, decimales: u32) -> String {
        let mut s = String::new();
        if valor < 0 {
            s.push('-');
        }
        con_decimales(valor.unsigned_abs(), decimales, &self.numeros, &mut s);
        s
    }

    /// Una cantidad de dinero, dada en la unidad pequeña (céntimos): `moneda(123456)` es `1.234,56 €` en España.
    pub fn moneda(&self, centimos: i64) -> String {
        let como = if centimos < 0 { &self.negativo } else { &self.positivo };
        let mut cantidad = String::new();
        con_decimales(centimos.unsigned_abs(), self.decimales.min(18) as u32, &self.dinero, &mut cantidad);
        let hueco = if como.espacio { " " } else { "" };
        let (antes_moneda, tras_moneda) = match como.donde {
            SIGNO_ANTES_DE_MONEDA => (como.signo.as_str(), ""),
            SIGNO_TRAS_MONEDA => ("", como.signo.as_str()),
            _ => ("", ""),
        };
        let mut base = String::new();
        if como.moneda_detras {
            let _ = write!(base, "{cantidad}{hueco}{antes_moneda}{}{tras_moneda}", self.moneda);
        } else {
            let _ = write!(base, "{antes_moneda}{}{tras_moneda}{hueco}{cantidad}", self.moneda);
        }
        match como.donde {
            SIGNO_PARENTESIS if centimos < 0 => alloc::format!("({base})"),
            SIGNO_DELANTE => alloc::format!("{}{base}", como.signo),
            SIGNO_DETRAS => alloc::format!("{base}{}", como.signo),
            _ => base,
        }
    }
}

/// Las cifras de `n` con el separador de grupos de `c`.
fn agrupar(n: u64, c: &Cifras, s: &mut String) {
    let mut cifras = [0u8; 20];
    let mut cuantas = 0;
    let mut resto = n;
    loop {
        cifras[cuantas] = b'0' + (resto % 10) as u8;
        cuantas += 1;
        resto /= 10;
        if resto == 0 {
            break;
        }
    }
    // Dónde va un separador, contando las cifras desde la derecha.
    let mut cortes = [false; 20];
    let (mut i, mut tamano, mut pos) = (0, 0usize, 0usize);
    if !c.grupo.is_empty() {
        loop {
            match c.agrupar.get(i) {
                Some(255) => break,
                Some(&g) if g != 0 => tamano = g as usize,
                // 0, o se acabó la lista: como el anterior.
                _ => {}
            }
            if tamano == 0 {
                break;
            }
            i += 1;
            pos += tamano;
            if pos >= cuantas {
                break;
            }
            cortes[pos] = true;
        }
    }
    for k in (0..cuantas).rev() {
        s.push(cifras[k] as char);
        if k > 0 && cortes[k] {
            s.push_str(&c.grupo);
        }
    }
}

fn con_decimales(valor: u64, decimales: u32, c: &Cifras, s: &mut String) {
    let decimales = decimales.min(18);
    let unidad = 10u64.pow(decimales);
    agrupar(valor / unidad, c, s);
    if decimales > 0 {
        s.push_str(&c.decimal);
        let _ = write!(s, "{:0ancho$}", valor % unidad, ancho = decimales as usize);
    }
}
