//! El aspecto de los programas de las cajas (S26, `docs/diseño/S26-aspecto-de-las-cajas.md`): si los programas de la
//! caja Amiga, de la caja Linux (y de las que vengan) se pintan con el tema de DashOS ("como DashOS") o con el suyo
//! ("original"), con excepciones por caja y por programa.
//!
//! El ajuste es un texto de líneas, el mismo en `Prefs/cajas` (lo guarda el centro de control) y en el protocolo del
//! tema (`dashos_tema_v1.cajas`, versión 3: lo pone el centro y el compositor lo reparte a quien escucha):
//!
//! ```text
//! general dashos
//! caja amiga original
//! programa linux original gimp
//! programa amiga dashos Directory Opus
//! ```
//!
//! - `general <aspecto>`: el de todas las cajas (una vez, la primera línea; si falta, `dashos`).
//! - `caja <caja> <aspecto>`: el de una caja, por encima del general (una vez por caja).
//! - `programa <caja> <aspecto> <nombre>`: el de un programa de esa caja, por encima de todo. El nombre es el resto de
//!   la línea (puede llevar espacios: los nombres del Amiga los llevan): el del fichero del programa, sin carpeta.
//!
//! Aspectos: `dashos` y `original`. Cajas: `amiga`, `linux` y `ppc` (S17). Lo que no sea esto, una línea repetida, un
//! nombre vacío, con `/`, `:` o caracteres de control, más de `MAX_PROGRAMAS` programas o más de `MAX_TEXTO` bytes es
//! un error: el texto entero no vale (quien lo lee se queda con lo que tenía).

use alloc::string::String;
use alloc::vec::Vec;

/// Lo más largo que puede ser el ajuste, en bytes.
pub const MAX_TEXTO: usize = 2048;
/// Excepciones por programa, como mucho.
pub const MAX_PROGRAMAS: usize = 32;
/// Lo más largo del nombre de un programa, en bytes.
pub const MAX_NOMBRE: usize = 64;

/// Cómo se pintan los programas de una caja.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aspecto {
    /// Con el tema de DashOS (Material 3, los colores, la letra y los radios del sistema).
    Dashos,
    /// Con el suyo (Intuition, GadTools, MUI, ReAction en el Amiga; Adwaita en Linux).
    Original,
}

impl Aspecto {
    pub fn nombre(self) -> &'static str {
        match self {
            Aspecto::Dashos => "dashos",
            Aspecto::Original => "original",
        }
    }

    fn de(t: &str) -> Option<Aspecto> {
        match t {
            "dashos" => Some(Aspecto::Dashos),
            "original" => Some(Aspecto::Original),
            _ => None,
        }
    }
}

/// Las cajas que conocen el ajuste.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Caja {
    Amiga,
    Linux,
    /// La caja PowerPC (S17), cuando exista.
    Ppc,
}

impl Caja {
    pub const TODAS: [Caja; 3] = [Caja::Amiga, Caja::Linux, Caja::Ppc];

    pub fn nombre(self) -> &'static str {
        match self {
            Caja::Amiga => "amiga",
            Caja::Linux => "linux",
            Caja::Ppc => "ppc",
        }
    }

    fn de(t: &str) -> Option<Caja> {
        Caja::TODAS.into_iter().find(|c| c.nombre() == t)
    }
}

/// El ajuste: el aspecto general, el de cada caja que lo cambia y las excepciones por programa.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ajuste {
    pub general: Aspecto,
    pub cajas: Vec<(Caja, Aspecto)>,
    pub programas: Vec<(Caja, String, Aspecto)>,
}

/// Por qué un texto no vale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Largo,
    NoUtf8,
    /// Una línea que no es ninguna de las tres, o con un aspecto o una caja que no existen.
    Linea,
    Repetida,
    Nombre,
    Demasiados,
}

impl Default for Ajuste {
    fn default() -> Self {
        Ajuste::DE_SERIE
    }
}

impl Ajuste {
    /// El de serie: todo como DashOS (S26: la petición del usuario es un entorno homogéneo).
    pub const DE_SERIE: Ajuste = Ajuste { general: Aspecto::Dashos, cajas: Vec::new(), programas: Vec::new() };

    /// Un nombre de programa que vale: no vacío, como mucho `MAX_NOMBRE` bytes, sin `/`, `:`, controles ni espacios a
    /// los lados.
    pub fn nombre_vale(n: &str) -> bool {
        !n.is_empty()
            && n.len() <= MAX_NOMBRE
            && n.trim() == n
            && !n.chars().any(|c| c == '/' || c == ':' || c.is_control())
    }

    /// Lee el texto (de `Prefs/cajas` o del protocolo). Un 0 o un salto de línea al final no cuentan.
    pub fn entender(bytes: &[u8]) -> Result<Ajuste, Error> {
        if bytes.len() > MAX_TEXTO {
            return Err(Error::Largo);
        }
        let texto = core::str::from_utf8(bytes).map_err(|_| Error::NoUtf8)?;
        let texto = texto.trim_end_matches('\0');
        let mut a = Ajuste::DE_SERIE;
        let mut con_general = false;
        for (k, linea) in texto.split('\n').enumerate() {
            if linea.is_empty() {
                continue;
            }
            let mut partes = linea.splitn(4, ' ');
            match (partes.next(), partes.next(), partes.next(), partes.next()) {
                (Some("general"), Some(asp), None, None) => {
                    if con_general || k != 0 {
                        return Err(Error::Repetida);
                    }
                    a.general = Aspecto::de(asp).ok_or(Error::Linea)?;
                    con_general = true;
                }
                (Some("caja"), Some(caja), Some(asp), None) => {
                    let (caja, asp) = (Caja::de(caja).ok_or(Error::Linea)?, Aspecto::de(asp).ok_or(Error::Linea)?);
                    if a.cajas.iter().any(|c| c.0 == caja) {
                        return Err(Error::Repetida);
                    }
                    a.cajas.push((caja, asp));
                }
                (Some("programa"), Some(caja), Some(asp), Some(nombre)) => {
                    let (caja, asp) = (Caja::de(caja).ok_or(Error::Linea)?, Aspecto::de(asp).ok_or(Error::Linea)?);
                    if !Ajuste::nombre_vale(nombre) {
                        return Err(Error::Nombre);
                    }
                    if a.programas.iter().any(|p| p.0 == caja && p.1 == nombre) {
                        return Err(Error::Repetida);
                    }
                    if a.programas.len() == MAX_PROGRAMAS {
                        return Err(Error::Demasiados);
                    }
                    a.programas.push((caja, String::from(nombre), asp));
                }
                _ => return Err(Error::Linea),
            }
        }
        Ok(a)
    }

    /// El texto, una línea por regla y con el salto de la última: lo que lee `entender`.
    pub fn escribir(&self) -> String {
        let mut t = alloc::format!("general {}\n", self.general.nombre());
        for (c, a) in &self.cajas {
            t.push_str(&alloc::format!("caja {} {}\n", c.nombre(), a.nombre()));
        }
        for (c, n, a) in &self.programas {
            t.push_str(&alloc::format!("programa {} {} {}\n", c.nombre(), a.nombre(), n));
        }
        t
    }

    /// El aspecto de una caja (sin mirar programas).
    pub fn de_caja(&self, caja: Caja) -> Aspecto {
        self.cajas.iter().find(|c| c.0 == caja).map_or(self.general, |c| c.1)
    }

    /// El aspecto de un programa de una caja: su excepción, o el de su caja, o el general.
    pub fn para(&self, caja: Caja, programa: &str) -> Aspecto {
        self.programas.iter().find(|p| p.0 == caja && p.1 == programa).map_or_else(|| self.de_caja(caja), |p| p.2)
    }

    /// Pone el aspecto de una caja (si es el general, quita la línea de la caja).
    pub fn poner_caja(&mut self, caja: Caja, aspecto: Aspecto) {
        self.cajas.retain(|c| c.0 != caja);
        if aspecto != self.general {
            self.cajas.push((caja, aspecto));
        }
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn ida_y_vuelta() {
        let t = "general dashos\ncaja amiga original\nprograma linux original gimp\nprograma amiga dashos Directory Opus\n";
        let a = Ajuste::entender(t.as_bytes()).unwrap();
        assert_eq!(a.escribir(), t);
        assert_eq!(a.para(Caja::Linux, "gimp"), Aspecto::Original);
        assert_eq!(a.para(Caja::Linux, "foot"), Aspecto::Dashos);
        assert_eq!(a.para(Caja::Amiga, "Directory Opus"), Aspecto::Dashos);
        assert_eq!(a.para(Caja::Amiga, "MultiView"), Aspecto::Original);
        assert_eq!(a.para(Caja::Ppc, "x"), Aspecto::Dashos);
        // Con un 0 al final (como viene un texto de Wayland), y vacío: el de serie.
        assert_eq!(Ajuste::entender(b"general original\n\0").unwrap().general, Aspecto::Original);
        assert_eq!(Ajuste::entender(b"").unwrap(), Ajuste::DE_SERIE);
    }

    #[test]
    fn poner_caja() {
        let mut a = Ajuste::DE_SERIE;
        a.poner_caja(Caja::Linux, Aspecto::Original);
        assert_eq!(a.escribir(), "general dashos\ncaja linux original\n");
        a.poner_caja(Caja::Linux, Aspecto::Dashos);
        assert_eq!(a.escribir(), "general dashos\n");
    }

    #[test]
    fn hostiles() {
        let malos: &[(&[u8], Error)] = &[
            (b"general rosa\n", Error::Linea),
            (b"general dashos\ngeneral original\n", Error::Repetida),
            (b"caja amiga original\ngeneral dashos\n", Error::Repetida),
            (b"caja amiga original\ncaja amiga dashos\n", Error::Repetida),
            (b"caja atari dashos\n", Error::Linea),
            (b"programa linux original \n", Error::Nombre),
            (b"programa linux original ../../bin/sh\n", Error::Nombre),
            (b"programa linux original a\tb\n", Error::Nombre),
            (b"programa amiga original DH0:C/Dir\n", Error::Nombre),
            (b"programa linux original  sh\n", Error::Nombre),
            (b"programa linux original sh\nprograma linux dashos sh\n", Error::Repetida),
            (b"caja linux\n", Error::Linea),
            (b"general dashos extra\n", Error::Linea),
            (b"\xff\xfe\n", Error::NoUtf8),
            (b"general\0dashos\n", Error::Linea),
        ];
        for (t, e) in malos {
            assert_eq!(Ajuste::entender(t), Err(*e), "{:?}", core::str::from_utf8(t));
        }
        let largo = alloc::format!("programa linux original {}\n", "x".repeat(MAX_NOMBRE + 1));
        assert_eq!(Ajuste::entender(largo.as_bytes()), Err(Error::Nombre));
        let enorme = "a".repeat(MAX_TEXTO + 1);
        assert_eq!(Ajuste::entender(enorme.as_bytes()), Err(Error::Largo));
        let mut muchos = String::new();
        for k in 0..=MAX_PROGRAMAS {
            muchos.push_str(&alloc::format!("programa amiga original p{}\n", k));
        }
        assert_eq!(Ajuste::entender(muchos.as_bytes()), Err(Error::Demasiados));
        // Basura al azar: nunca un pánico, y lo que vale se escribe y se vuelve a leer igual.
        let mut x = 0x2545_f491_4f6c_dd1du64;
        for _ in 0..20_000 {
            let n = (x % 64) as usize;
            let mut b = Vec::new();
            for _ in 0..n {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                let palabras: &[&[u8]] = &[b"general ", b"caja ", b"programa ", b"amiga ", b"linux ", b"dashos", b"original", b"\n", b" ", b"x", b"\xc3\xb1", b"/", b"\0"];
                b.extend_from_slice(palabras[(x % palabras.len() as u64) as usize]);
            }
            if let Ok(a) = Ajuste::entender(&b) {
                assert_eq!(Ajuste::entender(a.escribir().as_bytes()), Ok(a));
            }
        }
    }
}
