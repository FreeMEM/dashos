//! Los ficheros de ajustes en texto (S36, regla 1; S27, apartado 7; diseño en `docs/bitacoras/ajustes-texto.md`):
//! leerlos y cambiarlos sin perder nada de lo que escribió una persona a mano.
//!
//! El formato, el de siempre de los ficheros de configuración:
//!
//! ```text
//! # Idioma y país. Este fichero manda; Sys/locale.prefs se genera de él.
//! idiomas = english, español
//! pais = great_britain
//! ```
//!
//! - Texto UTF-8, una cosa por línea. Una línea es un ajuste (`clave = valor`), un comentario (empieza por `#` o por
//!   `;`, como en los guiones del Amiga, con espacios delante o sin ellos) o está en blanco.
//! - La clave va antes del primer `=`, sin los espacios de los lados. Letras, cifras, `_` y `-`, hasta `MAX_CLAVE`
//!   bytes (sin punto: la orden `Ajuste` nombra un ajuste como `fichero.clave`). No distingue mayúsculas (como los nombres del Amiga), pero se guarda como se escribió.
//! - El valor es todo lo que va tras el `=`, sin los espacios de los lados. Puede estar vacío y puede llevar `#`
//!   (un color `#3d6f88`): no hay comentarios al final de una línea. Sin bytes de control (el tabulador sí), hasta
//!   `MAX_VALOR` bytes.
//! - Una clave repetida: vale la primera, como en los catálogos. Las de después se señalan como problema.
//! - Una línea que no es nada de eso (sin `=`, una clave con letras que no valen) no rompe el fichero: se guarda tal
//!   cual, no cuenta, y se señala como problema con su número de línea, para que la orden `Ajuste` lo diga.
//!
//! Cambiar un ajuste toca solo su línea: lo de antes del valor (la clave, los espacios, el `=`) se queda como estaba;
//! uno que no estaba va al final. Los comentarios, las líneas en blanco, las rotas y el orden no se tocan, y tampoco si
//! el fichero acababa en salto de línea o no, ni sus `\r\n`. Leer y escribir sin cambiar nada da los mismos bytes.
//!
//! No vale (y entonces quien lee se queda con lo de serie): lo que no es UTF-8, lo que pasa de `MAX_FICHERO` bytes o
//! de `MAX_LINEAS` líneas.
//!
//! Sin nada de DashOS ni de `dashos` (usa `alloc`, que la biblioteca no enlaza): lo incluye el crate `idioma` como
//! `idioma::texto` (`kit::idioma::texto` para los programas), y se prueba con su `cargo test`.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Lo más que mide un fichero de ajustes. Los de verdad son de unos cientos de bytes.
pub const MAX_FICHERO: usize = 64 * 1024;
/// Lo más de líneas.
pub const MAX_LINEAS: usize = 2048;
pub const MAX_CLAVE: usize = 64;
pub const MAX_VALOR: usize = 1024;

/// Por qué un fichero entero no vale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// No es UTF-8.
    NoEsTexto,
    /// Pasa de `MAX_FICHERO` bytes o de `MAX_LINEAS` líneas.
    Enorme,
}

/// Por qué no se puede poner un valor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MalValor {
    /// La clave está vacía, es larga o lleva algo que no es letra, cifra, `_` o `-`.
    Clave,
    /// El valor lleva un salto de línea u otro byte de control, o pasa de `MAX_VALOR`.
    Valor,
    /// El fichero pasaría de `MAX_FICHERO` o de `MAX_LINEAS`.
    Enorme,
}

/// Lo que se señala de una línea (su número empieza en 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problema {
    /// Ni ajuste, ni comentario, ni en blanco: no tiene `=`.
    SinIgual,
    /// La clave no vale (vacía, larga o con otras letras).
    ClaveMala,
    /// El valor lleva bytes de control, o es largo.
    ValorMalo,
    /// La clave ya salió antes: esta no cuenta.
    Repetida,
}

impl Problema {
    pub fn texto(self) -> &'static str {
        match self {
            Problema::SinIgual => "no es un ajuste (le falta el =) ni un comentario",
            Problema::ClaveMala => "la clave no vale (letras, cifras, _ y -)",
            Problema::ValorMalo => "el valor lleva bytes de control o es demasiado largo",
            Problema::Repetida => "la clave ya está antes: vale la primera",
        }
    }
}

#[derive(Clone, Debug)]
enum Linea {
    /// Un ajuste: lo de antes del valor (clave, espacios, `=` y espacios), la clave, el valor, lo de después (espacios)
    /// y el final de línea (`""`, `"\n"` o `"\r\n"`).
    Ajuste { cabeza: String, clave: String, valor: String, cola: String, fin: &'static str },
    /// Un comentario, una línea en blanco o una que no vale: tal cual, con su final.
    Otra { texto: String, fin: &'static str, problema: Option<Problema> },
}

/// Un fichero de ajustes leído.
#[derive(Clone, Debug, Default)]
pub struct Ajustes {
    lineas: Vec<Linea>,
}

/// Si `c` puede ir en una clave.
fn letra_de_clave(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-')
}

/// Si `clave` vale como clave.
pub fn clave_valida(clave: &str) -> bool {
    !clave.is_empty() && clave.len() <= MAX_CLAVE && clave.chars().all(letra_de_clave)
}

/// Si `valor` (ya sin los espacios de los lados) vale como valor.
pub fn valor_valido(valor: &str) -> bool {
    valor.len() <= MAX_VALOR && valor.chars().all(|c| c == '\t' || !c.is_control())
}

fn igual(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.chars().zip(b.chars()).all(|(x, y)| x.to_lowercase().eq(y.to_lowercase()))
}

/// Parte `texto` en sus líneas, cada una con su final.
fn partir(texto: &str) -> impl Iterator<Item = (&str, &'static str)> {
    let mut resto = texto;
    core::iter::from_fn(move || {
        if resto.is_empty() {
            return None;
        }
        Some(match resto.find('\n') {
            Some(i) => {
                let (linea, fin) = if i > 0 && resto.as_bytes()[i - 1] == b'\r' { (&resto[..i - 1], "\r\n") } else { (&resto[..i], "\n") };
                resto = &resto[i + 1..];
                (linea, fin)
            }
            None => {
                let l = resto;
                resto = "";
                (l, "")
            }
        })
    })
}

fn una_linea(linea: &str, fin: &'static str) -> Linea {
    let otra = |problema| Linea::Otra { texto: String::from(linea), fin, problema };
    let limpia = linea.trim_start();
    if limpia.is_empty() || limpia.starts_with('#') || limpia.starts_with(';') {
        return otra(None);
    }
    let Some(i) = linea.find('=') else { return otra(Some(Problema::SinIgual)) };
    let clave = linea[..i].trim();
    if !clave_valida(clave) {
        return otra(Some(Problema::ClaveMala));
    }
    let tras = &linea[i + 1..];
    let valor = tras.trim();
    if !valor_valido(valor) {
        return otra(Some(Problema::ValorMalo));
    }
    // Lo de antes del valor y lo de después, tal cual.
    let empieza = i + 1 + (tras.len() - tras.trim_start().len());
    let (cabeza, cola) = if valor.is_empty() { (linea, "") } else { (&linea[..empieza], &linea[empieza + valor.len()..]) };
    Linea::Ajuste { cabeza: String::from(cabeza), clave: String::from(clave), valor: String::from(valor), cola: String::from(cola), fin }
}

impl Ajustes {
    /// Un fichero vacío.
    pub fn nuevo() -> Ajustes {
        Ajustes::default()
    }

    /// Lee un fichero. Solo falla si no es texto o es enorme; las líneas que no valen se guardan y se señalan.
    pub fn leer(bytes: &[u8]) -> Result<Ajustes, Error> {
        if bytes.len() > MAX_FICHERO {
            return Err(Error::Enorme);
        }
        let texto = core::str::from_utf8(bytes).map_err(|_| Error::NoEsTexto)?;
        let mut lineas = Vec::new();
        for (linea, fin) in partir(texto) {
            if lineas.len() >= MAX_LINEAS {
                return Err(Error::Enorme);
            }
            lineas.push(una_linea(linea, fin));
        }
        let mut yo = Ajustes { lineas };
        yo.marcar_repetidas();
        Ok(yo)
    }

    /// Las claves que ya salieron antes no cuentan: pasan a ser "otras" con su problema.
    fn marcar_repetidas(&mut self) {
        let mut vistas: Vec<String> = Vec::new();
        for l in self.lineas.iter_mut() {
            if let Linea::Ajuste { cabeza, valor, cola, fin, clave } = l {
                if vistas.iter().any(|v| igual(v, clave)) {
                    let texto = alloc::format!("{}{}{}", cabeza, valor, cola);
                    *l = Linea::Otra { texto, fin, problema: Some(Problema::Repetida) };
                } else {
                    vistas.push(clave.clone());
                }
            }
        }
    }

    /// El valor de `clave` (sin distinguir mayúsculas), si está.
    pub fn valor(&self, clave: &str) -> Option<&str> {
        self.lineas.iter().find_map(|l| match l {
            Linea::Ajuste { clave: c, valor, .. } if igual(c, clave) => Some(valor.as_str()),
            _ => None,
        })
    }

    /// Las claves y sus valores, en el orden del fichero.
    pub fn todos(&self) -> impl Iterator<Item = (&str, &str)> {
        self.lineas.iter().filter_map(|l| match l {
            Linea::Ajuste { clave, valor, .. } => Some((clave.as_str(), valor.as_str())),
            _ => None,
        })
    }

    /// Las líneas que no cuentan, con su número (desde 1) y por qué.
    pub fn problemas(&self) -> Vec<(usize, Problema)> {
        self.lineas
            .iter()
            .enumerate()
            .filter_map(|(i, l)| match l {
                Linea::Otra { problema: Some(p), .. } => Some((i + 1, *p)),
                _ => None,
            })
            .collect()
    }

    /// Pone `clave = valor` (el valor, sin los espacios de los lados). Si la clave está, cambia solo su valor; si no,
    /// la añade al final. No cambia nada si no vale.
    pub fn poner(&mut self, clave: &str, valor: &str) -> Result<(), MalValor> {
        if !clave_valida(clave) {
            return Err(MalValor::Clave);
        }
        let valor = valor.trim();
        if !valor_valido(valor) {
            return Err(MalValor::Valor);
        }
        let antes = self.medida();
        for l in self.lineas.iter_mut() {
            if let Linea::Ajuste { clave: c, valor: v, cabeza, .. } = l
                && igual(c, clave)
            {
                if antes - v.len() + valor.len() > MAX_FICHERO {
                    return Err(MalValor::Enorme);
                }
                // Una línea "clave =" sin valor: su cabeza no lleva el espacio de después del "=".
                if v.is_empty() && !valor.is_empty() && !cabeza.ends_with(' ') && !cabeza.ends_with('\t') {
                    cabeza.push(' ');
                }
                *v = String::from(valor);
                return Ok(());
            }
        }
        let nueva = alloc::format!("{} = {}", clave, valor);
        if self.lineas.len() >= MAX_LINEAS || antes + nueva.len() + 2 > MAX_FICHERO {
            return Err(MalValor::Enorme);
        }
        // Si la última línea no acababa, se acaba ahora (con el final que use el fichero).
        let fin_del_fichero = self.final_de_linea();
        if let Some(Linea::Ajuste { fin, .. } | Linea::Otra { fin, .. }) = self.lineas.last_mut()
            && fin.is_empty()
        {
            *fin = fin_del_fichero;
        }
        let con = if valor.is_empty() { alloc::format!("{} =", clave) } else { alloc::format!("{} = ", clave) };
        self.lineas.push(Linea::Ajuste { cabeza: con, clave: String::from(clave), valor: String::from(valor), cola: String::new(), fin: fin_del_fichero });
        Ok(())
    }

    /// Quita la línea de `clave`. Si había, `true`.
    pub fn quitar(&mut self, clave: &str) -> bool {
        match self.lineas.iter().position(|l| matches!(l, Linea::Ajuste { clave: c, .. } if igual(c, clave))) {
            Some(i) => {
                self.lineas.remove(i);
                true
            }
            None => false,
        }
    }

    /// Añade un comentario al final (lo que vale para un fichero nuevo: qué es y quién lo escribe).
    pub fn comentar(&mut self, texto: &str) {
        let fin = self.final_de_linea();
        if let Some(Linea::Ajuste { fin: f, .. } | Linea::Otra { fin: f, .. }) = self.lineas.last_mut()
            && f.is_empty()
        {
            *f = fin;
        }
        for l in texto.lines() {
            self.lineas.push(Linea::Otra { texto: alloc::format!("# {}", l).trim_end().into(), fin, problema: None });
        }
    }

    /// El final de línea del fichero: el de su primera línea con final, o `\n`.
    fn final_de_linea(&self) -> &'static str {
        self.lineas
            .iter()
            .map(|l| match l {
                Linea::Ajuste { fin, .. } | Linea::Otra { fin, .. } => *fin,
            })
            .find(|f| !f.is_empty())
            .unwrap_or("\n")
    }

    fn medida(&self) -> usize {
        self.lineas
            .iter()
            .map(|l| match l {
                Linea::Ajuste { cabeza, valor, cola, fin, .. } => cabeza.len() + valor.len() + cola.len() + fin.len(),
                Linea::Otra { texto, fin, .. } => texto.len() + fin.len(),
            })
            .sum()
    }

    /// El fichero, en texto.
    pub fn texto(&self) -> String {
        let mut s = String::with_capacity(self.medida());
        for l in &self.lineas {
            match l {
                Linea::Ajuste { cabeza, valor, cola, fin, .. } => {
                    s.push_str(cabeza);
                    s.push_str(valor);
                    s.push_str(cola);
                    s.push_str(fin);
                }
                Linea::Otra { texto, fin, .. } => {
                    s.push_str(texto);
                    s.push_str(fin);
                }
            }
        }
        s
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    const EJEMPLO: &str = "# Idioma y país.\n; también así\n\nidiomas = english, español\n  pais=great_britain  \nesto no vale\n";

    #[test]
    fn leer_y_escribir_igual() {
        for t in [EJEMPLO, "", "a = 1", "a = 1\r\nb = 2\r\n", "\n\n\n", "# solo\n", "clave =\n", "x=#3d6f88\n", "  # sangrado\n"] {
            let a = Ajustes::leer(t.as_bytes()).unwrap();
            assert_eq!(a.texto(), t, "{t:?}");
        }
        let a = Ajustes::leer(EJEMPLO.as_bytes()).unwrap();
        assert_eq!(a.valor("idiomas"), Some("english, español"));
        assert_eq!(a.valor("PAIS"), Some("great_britain"));
        assert_eq!(a.valor("esto"), None);
        assert_eq!(a.problemas(), [(6, Problema::SinIgual)]);
        assert_eq!(Ajustes::leer(b"x=#3d6f88\n").unwrap().valor("x"), Some("#3d6f88"));
        assert_eq!(Ajustes::leer(b"clave =\n").unwrap().valor("clave"), Some(""));
    }

    #[test]
    fn poner_no_pierde_nada() {
        let mut a = Ajustes::leer(EJEMPLO.as_bytes()).unwrap();
        a.poner("pais", "españa").unwrap();
        a.poner("Idiomas", "  español ").unwrap();
        a.poner("nuevo", "sí").unwrap();
        assert_eq!(a.texto(), "# Idioma y país.\n; también así\n\nidiomas = español\n  pais=españa  \nesto no vale\nnuevo = sí\n");
        // Sin salto al final, y con \r\n: se acaba la última y la nueva lleva el mismo final.
        let mut b = Ajustes::leer(b"a = 1\r\nb = 2").unwrap();
        b.poner("c", "3").unwrap();
        assert_eq!(b.texto(), "a = 1\r\nb = 2\r\nc = 3\r\n");
        // Un valor vacío que se llena, y uno que se vacía.
        let mut c = Ajustes::leer(b"a =\nb = 2\n").unwrap();
        c.poner("a", "1").unwrap();
        c.poner("b", "").unwrap();
        assert_eq!(c.texto(), "a = 1\nb = \n");
        assert_eq!(Ajustes::leer(c.texto().as_bytes()).unwrap().valor("b"), Some(""));
        // Quitar y comentar.
        let mut d = Ajustes::nuevo();
        d.comentar("Apariencia.\nEste fichero manda.");
        d.poner("modo", "oscuro").unwrap();
        assert!(d.quitar("MODO"));
        assert!(!d.quitar("modo"));
        assert_eq!(d.texto(), "# Apariencia.\n# Este fichero manda.\n");
    }

    #[test]
    fn repetidas_vale_la_primera() {
        let mut a = Ajustes::leer(b"a = 1\nA = 2\na = 3\n").unwrap();
        assert_eq!(a.valor("a"), Some("1"));
        assert_eq!(a.problemas(), [(2, Problema::Repetida), (3, Problema::Repetida)]);
        a.poner("a", "9").unwrap();
        assert_eq!(a.texto(), "a = 9\nA = 2\na = 3\n");
        assert_eq!(a.todos().count(), 1);
    }

    /// Hostil: lo que no vale no rompe el fichero ni se cuela como ajuste.
    #[test]
    fn hostil() {
        assert_eq!(Ajustes::leer(b"\xff\xfeoscuro magenta\x00").err(), Some(Error::NoEsTexto));
        assert_eq!(Ajustes::leer(&alloc::vec![b'#'; MAX_FICHERO + 1]).err(), Some(Error::Enorme));
        assert_eq!(Ajustes::leer("a = 1\n".repeat(MAX_LINEAS + 1).as_bytes()).err(), Some(Error::Enorme));
        let raro = "= sin clave\nmal clave = x\nbien = con\u{7}campana\nlarga = ".to_string() + &"x".repeat(MAX_VALOR + 1) + "\nok = 1\n";
        let a = Ajustes::leer(raro.as_bytes()).unwrap();
        assert_eq!(a.problemas(), [(1, Problema::ClaveMala), (2, Problema::ClaveMala), (3, Problema::ValorMalo), (4, Problema::ValorMalo)]);
        assert_eq!(a.todos().collect::<Vec<_>>(), [("ok", "1")]);
        assert_eq!(a.texto(), raro);
        let mut b = Ajustes::nuevo();
        assert_eq!(b.poner("", "x"), Err(MalValor::Clave));
        assert_eq!(b.poner("a b", "x"), Err(MalValor::Clave));
        assert_eq!(b.poner("a=b", "x"), Err(MalValor::Clave));
        assert_eq!(b.poner("a.b", "x"), Err(MalValor::Clave));
        assert_eq!(b.poner(&"k".repeat(MAX_CLAVE + 1), "x"), Err(MalValor::Clave));
        assert_eq!(b.poner("a", "uno\ndos = 2"), Err(MalValor::Valor));
        assert_eq!(b.poner("a", "uno\rdos"), Err(MalValor::Valor));
        assert_eq!(b.poner("a", &"v".repeat(MAX_VALOR + 1)), Err(MalValor::Valor));
        assert_eq!(b.texto(), "");
        // Llenar hasta el tope: el que no cabe no entra, y lo de antes sigue.
        let mut c = Ajustes::nuevo();
        let mut puestos = 0;
        for i in 0.. {
            match c.poner(&alloc::format!("k{i}"), &"v".repeat(1000)) {
                Ok(()) => puestos += 1,
                Err(e) => {
                    assert_eq!(e, MalValor::Enorme);
                    break;
                }
            }
        }
        assert!(c.texto().len() <= MAX_FICHERO && puestos > 50);
        assert_eq!(Ajustes::leer(c.texto().as_bytes()).unwrap().todos().count(), puestos);
        // Bytes al azar: nunca se cae, y lo que lee lo vuelve a escribir igual.
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        for _ in 0..2000 {
            let mut v = Vec::new();
            for _ in 0..(x % 200) {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                v.push(b"a= #;\n\r\t\x01xyz\xc3\xb1"[(x % 14) as usize]);
            }
            x ^= x << 13;
            if let Ok(a) = Ajustes::leer(&v) {
                assert_eq!(a.texto().as_bytes(), &v[..]);
            }
        }
    }
}
