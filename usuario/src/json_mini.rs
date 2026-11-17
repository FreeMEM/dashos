//! Un lector de JSON pequeño, con topes, para el importador de los ajustes de DMS (`barra_config::importar_dms`).
//! RFC 8259 entero (objetos, listas, textos con sus escapes y `\u` con pares sustitutos, números, `true`, `false`,
//! `null`), sin nada de DashOS: se prueba con `rustc --edition 2024 --test usuario/src/json_mini.rs`.
//!
//! Hostil: lo que no es JSON da `Err` con la posición, nunca un pánico; hondura, tamaño y número de valores con tope
//! (un `[[[[...` de un millón no desborda la pila, un fichero de 100 MB no se lee).

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Lo más que se lee.
pub const MAX_BYTES: usize = 4 << 20;
/// Lo más hondo (listas y objetos uno dentro de otro).
pub const MAX_HONDURA: usize = 64;
/// Lo más de valores en todo el fichero.
pub const MAX_VALORES: usize = 200_000;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Nulo,
    Bool(bool),
    Num(f64),
    Texto(String),
    Lista(Vec<Json>),
    /// Las claves en su orden; una repetida vale la última (como en JavaScript, que es lo que lee DMS).
    Objeto(Vec<(String, Json)>),
}

impl Json {
    /// El valor de `clave` de un objeto (la última, si se repite).
    pub fn campo(&self, clave: &str) -> Option<&Json> {
        match self {
            Json::Objeto(v) => v.iter().rev().find(|(k, _)| k == clave).map(|(_, x)| x),
            _ => None,
        }
    }

    pub fn texto(&self) -> Option<&str> {
        match self {
            Json::Texto(t) => Some(t),
            _ => None,
        }
    }

    pub fn num(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn lista(&self) -> Option<&[Json]> {
        match self {
            Json::Lista(v) => Some(v),
            _ => None,
        }
    }
}

/// Por qué no se lee.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// El byte donde se vio el fallo.
    pub en: usize,
    pub porque: &'static str,
}

struct Lector<'a> {
    b: &'a [u8],
    i: usize,
    valores: usize,
}

/// Lee un documento JSON entero (con espacios delante y detrás, nada más).
pub fn leer(bytes: &[u8]) -> Result<Json, Error> {
    if bytes.len() > MAX_BYTES {
        return Err(Error { en: 0, porque: "demasiado grande" });
    }
    // Un BOM de UTF-8 delante se salta.
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let mut l = Lector { b: bytes, i: 0, valores: 0 };
    let v = l.valor(0)?;
    l.blancos();
    if l.i != l.b.len() {
        return Err(l.mal("sobra algo tras el documento"));
    }
    Ok(v)
}

impl Lector<'_> {
    fn mal(&self, porque: &'static str) -> Error {
        Error { en: self.i, porque }
    }

    fn blancos(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn mira(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn palabra(&mut self, p: &[u8], v: Json) -> Result<Json, Error> {
        if self.b[self.i..].starts_with(p) {
            self.i += p.len();
            Ok(v)
        } else {
            Err(self.mal("palabra que no es true, false ni null"))
        }
    }

    fn valor(&mut self, hondura: usize) -> Result<Json, Error> {
        if hondura > MAX_HONDURA {
            return Err(self.mal("demasiado hondo"));
        }
        self.valores += 1;
        if self.valores > MAX_VALORES {
            return Err(self.mal("demasiados valores"));
        }
        self.blancos();
        match self.mira() {
            None => Err(self.mal("se acaba antes de tiempo")),
            Some(b'{') => {
                self.i += 1;
                let mut v = Vec::new();
                self.blancos();
                if self.mira() == Some(b'}') {
                    self.i += 1;
                    return Ok(Json::Objeto(v));
                }
                loop {
                    self.blancos();
                    if self.mira() != Some(b'"') {
                        return Err(self.mal("una clave tiene que ser un texto"));
                    }
                    let k = self.texto()?;
                    self.blancos();
                    if self.mira() != Some(b':') {
                        return Err(self.mal("falta el : tras la clave"));
                    }
                    self.i += 1;
                    let x = self.valor(hondura + 1)?;
                    v.push((k, x));
                    self.blancos();
                    match self.mira() {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Json::Objeto(v));
                        }
                        _ => return Err(self.mal("falta , o } en un objeto")),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut v = Vec::new();
                self.blancos();
                if self.mira() == Some(b']') {
                    self.i += 1;
                    return Ok(Json::Lista(v));
                }
                loop {
                    v.push(self.valor(hondura + 1)?);
                    self.blancos();
                    match self.mira() {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Json::Lista(v));
                        }
                        _ => return Err(self.mal("falta , o ] en una lista")),
                    }
                }
            }
            Some(b'"') => self.texto().map(Json::Texto),
            Some(b't') => self.palabra(b"true", Json::Bool(true)),
            Some(b'f') => self.palabra(b"false", Json::Bool(false)),
            Some(b'n') => self.palabra(b"null", Json::Nulo),
            Some(b'-' | b'0'..=b'9') => self.numero(),
            Some(_) => Err(self.mal("no es un valor de JSON")),
        }
    }

    fn numero(&mut self) -> Result<Json, Error> {
        let desde = self.i;
        if self.mira() == Some(b'-') {
            self.i += 1;
        }
        let cifras = |l: &mut Self| {
            let d = l.i;
            while l.mira().is_some_and(|c| c.is_ascii_digit()) {
                l.i += 1;
            }
            l.i - d
        };
        match self.mira() {
            Some(b'0') => self.i += 1,
            Some(b'1'..=b'9') => {
                cifras(self);
            }
            _ => return Err(self.mal("número sin cifras")),
        }
        if self.mira() == Some(b'.') {
            self.i += 1;
            if cifras(self) == 0 {
                return Err(self.mal("número sin cifras tras el punto"));
            }
        }
        if matches!(self.mira(), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.mira(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if cifras(self) == 0 {
                return Err(self.mal("número sin cifras en el exponente"));
            }
        }
        let t = core::str::from_utf8(&self.b[desde..self.i]).map_err(|_| self.mal("número que no se lee"))?;
        t.parse::<f64>().map(Json::Num).map_err(|_| self.mal("número que no se lee"))
    }

    fn hex4(&mut self) -> Result<u32, Error> {
        let Some(h) = self.b.get(self.i..self.i + 4) else { return Err(self.mal("\\u cortado")) };
        let mut v = 0u32;
        for &c in h {
            let d = (c as char).to_digit(16).ok_or_else(|| self.mal("\\u con algo que no es hexadecimal"))?;
            v = v * 16 + d;
        }
        self.i += 4;
        Ok(v)
    }

    fn texto(&mut self) -> Result<String, Error> {
        self.i += 1;
        let mut s = String::new();
        loop {
            let desde = self.i;
            // Lo que va de corrido, de una vez.
            while self.mira().is_some_and(|c| c != b'"' && c != b'\\' && c >= 0x20) {
                self.i += 1;
            }
            s.push_str(core::str::from_utf8(&self.b[desde..self.i]).map_err(|_| Error { en: desde, porque: "texto que no es UTF-8" })?);
            match self.mira() {
                None => return Err(self.mal("texto sin cerrar")),
                Some(b'"') => {
                    self.i += 1;
                    return Ok(s);
                }
                Some(b'\\') => {
                    self.i += 1;
                    let Some(e) = self.mira() else { return Err(self.mal("escape cortado")) };
                    self.i += 1;
                    match e {
                        b'"' => s.push('"'),
                        b'\\' => s.push('\\'),
                        b'/' => s.push('/'),
                        b'b' => s.push('\u{8}'),
                        b'f' => s.push('\u{c}'),
                        b'n' => s.push('\n'),
                        b'r' => s.push('\r'),
                        b't' => s.push('\t'),
                        b'u' => {
                            let a = self.hex4()?;
                            let c = if (0xd800..0xdc00).contains(&a) {
                                // Par sustituto: tiene que venir el segundo.
                                if !self.b[self.i..].starts_with(b"\\u") {
                                    return Err(self.mal("sustituto alto sin el bajo"));
                                }
                                self.i += 2;
                                let b = self.hex4()?;
                                if !(0xdc00..0xe000).contains(&b) {
                                    return Err(self.mal("sustituto alto sin el bajo"));
                                }
                                0x10000 + ((a - 0xd800) << 10) + (b - 0xdc00)
                            } else {
                                a
                            };
                            s.push(char::from_u32(c).ok_or_else(|| self.mal("\\u que no es un carácter"))?);
                        }
                        _ => return Err(self.mal("escape que no existe")),
                    }
                }
                Some(_) => return Err(self.mal("byte de control dentro de un texto")),
            }
        }
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn lo_de_siempre() {
        let v = leer(br#" {"a": [1, -2.5e3, true, false, null], "b": "x\"\u00f1\ud83d\ude00", "c": {}} "#).unwrap();
        assert_eq!(v.campo("a").unwrap().lista().unwrap().len(), 5);
        assert_eq!(v.campo("a").unwrap().lista().unwrap()[1], Json::Num(-2500.0));
        assert_eq!(v.campo("b").unwrap().texto(), Some("x\"ñ😀"));
        assert_eq!(v.campo("c"), Some(&Json::Objeto(Vec::new())));
        // Clave repetida: la última.
        assert_eq!(leer(br#"{"a":1,"a":2}"#).unwrap().campo("a"), Some(&Json::Num(2.0)));
        assert_eq!(leer(b"\xef\xbb\xbf[]").unwrap(), Json::Lista(Vec::new()));
    }

    #[test]
    fn hostil() {
        for malo in [
            &b""[..], b"{", b"[1,]", b"{\"a\"}", b"{\"a\":}", b"[01]", b"[1.]", b"[1e]", b"\"\\x\"", b"\"\\ud800\"", b"\"\\udc00\\u0041\"",
            b"\"\x01\"", b"\"\xff\"", b"tru", b"nul", b"{} {}", b"[\"a\" \"b\"]", b"{1:2}", b"-", b"\"\\u12\"",
        ] {
            assert!(leer(malo).is_err(), "{:?}", core::str::from_utf8(malo));
        }
        // Muy hondo: error, sin desbordar la pila.
        let mut hondo = alloc::vec![b'['; 2_500_000];
        hondo.extend(alloc::vec![b']'; 2_500_000]);
        assert_eq!(leer(&hondo).unwrap_err().porque, "demasiado grande");
        let mut hondo = alloc::vec![b'['; 100_000];
        hondo.extend(alloc::vec![b']'; 100_000]);
        assert_eq!(leer(&hondo).unwrap_err().porque, "demasiado hondo");
        // Muchos valores: tope.
        let mut muchos = String::from("[");
        for _ in 0..300_000 {
            muchos.push_str("0,");
        }
        muchos.push('0');
        muchos.push(']');
        assert_eq!(leer(muchos.as_bytes()).unwrap_err().porque, "demasiados valores");
        // Bytes al azar: nunca un pánico.
        let mut x = 0x1234_5678u32;
        for _ in 0..20_000 {
            let n = (x % 40) as usize;
            let mut v = Vec::with_capacity(n);
            for _ in 0..n {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                let alfabeto = b"{}[]\":,0123456789.eE-+tfnrul\\ ab\x00\xc3\xb1";
                v.push(alfabeto[(x as usize) % alfabeto.len()]);
            }
            let _ = leer(&v);
        }
    }
}
