//! Los catálogos: un fichero IFF `CTLG` como los de `locale.library` (`FVER`, `LANG`, `CSET`, `STRS`), con el texto en
//! UTF-8 (`CSET` 106) en los nativos y en Latin-1 en los del Amiga, que se convierten al leer.
//!
//! Un catálogo que no cuadra entero no vale: el programa sigue con sus textos de siempre. Un texto suelto que no es
//! UTF-8 válido se salta, y ese sale con el de siempre.

use crate::iff;
use crate::letras::{self, Juego};
use alloc::string::String;
use alloc::vec::Vec;

/// Lo más que mide un catálogo que se lee (1 MB): los del sistema no pasan de unas decenas de KB.
pub const MAX_BYTES: usize = 1 << 20;

/// Por qué no vale un catálogo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// No es un IFF `CTLG`, o un largo dice más de lo que hay.
    Formato(iff::Error),
    /// Pasa de `MAX_BYTES`.
    Grande,
    /// Un texto de `STRS` dice medir más de lo que queda.
    TextoCortado,
    /// Un juego de caracteres (`CSET`) que no sabemos leer.
    Juego(u32),
    /// Es el catálogo de otro programa (su `$VER:` trae otro nombre).
    DeOtro,
    /// No es de la versión que pide el programa.
    Version { pedida: u32, tiene: u32 },
}

/// Qué catálogo quiere el programa, como `OpenCatalog`: su nombre (sin `.catalog`) y su versión (0, cualquiera; si
/// no, tiene que ser esa: un catálogo de otra versión tiene otros números).
#[derive(Clone, Copy, Debug)]
pub struct Pedido<'a> {
    pub programa: &'a str,
    pub version: u32,
}

/// Un catálogo leído: los textos por su número, ya en UTF-8.
#[derive(Clone, Debug, Default)]
pub struct Catalogo {
    /// El nombre de su `$VER:` (`explorador.catalog`), o vacío si no trae.
    pub nombre: String,
    pub version: u32,
    pub revision: u32,
    /// El idioma que dice su `LANG`, o vacío.
    pub idioma: String,
    /// (número, dónde empieza, dónde acaba) en `texto`, por número.
    indice: Vec<(u32, u32, u32)>,
    texto: String,
    /// Cuántos textos se saltaron por no ser UTF-8 válido.
    pub malos: usize,
    /// Cuántos números salían más de una vez (vale el primero).
    pub repetidos: usize,
}

/// (nombre, versión, revisión) de un `$VER: nombre versión.revisión (fecha)`.
fn version(fver: &str) -> Option<(&str, u32, u32)> {
    let resto = &fver[fver.find("$VER:")? + 5..];
    let mut partes = resto.split_ascii_whitespace();
    let nombre = partes.next()?;
    let (v, r) = match partes.next() {
        Some(numeros) => {
            let (v, r) = numeros.split_once('.').unwrap_or((numeros, "0"));
            (v.parse().ok()?, r.parse().unwrap_or(0))
        }
        None => (0, 0),
    };
    Some((nombre, v, r))
}

impl Catalogo {
    /// Lee un catálogo y mira que es el que se pide.
    pub fn leer(bytes: &[u8], pedido: Pedido) -> Result<Catalogo, Error> {
        if bytes.len() > MAX_BYTES {
            return Err(Error::Grande);
        }
        let mut fver: &[u8] = &[];
        let mut lang: &[u8] = &[];
        let mut cset = None;
        let mut strs: &[u8] = &[];
        for t in iff::forma(bytes, b"CTLG").map_err(Error::Formato)? {
            let t = t.map_err(Error::Formato)?;
            // De cada trozo vale el primero, como del resto.
            match &t.id {
                b"FVER" if fver.is_empty() => fver = t.datos,
                b"LANG" if lang.is_empty() => lang = t.datos,
                b"CSET" if cset.is_none() && t.datos.len() >= 4 => cset = Some(u32::from_be_bytes([t.datos[0], t.datos[1], t.datos[2], t.datos[3]])),
                b"STRS" if strs.is_empty() => strs = t.datos,
                _ => {}
            }
        }
        let juego = Juego::desde(cset.unwrap_or(0)).ok_or(Error::Juego(cset.unwrap_or(0)))?;
        let mut c = Catalogo::default();
        // La cabecera se lee como Latin-1 si el catálogo no es UTF-8 válido ahí: solo sirve para comparar.
        let mut cabecera = String::new();
        let sin_cero = |b: &'_ [u8]| -> usize { b.iter().position(|&x| x == 0).unwrap_or(b.len()) };
        if !letras::a_utf8(&fver[..sin_cero(fver)], juego, &mut cabecera) {
            letras::a_utf8(&fver[..sin_cero(fver)], Juego::Latin1, &mut cabecera);
        }
        if let Some((nombre, v, r)) = version(&cabecera) {
            c.nombre = String::from(nombre);
            c.version = v;
            c.revision = r;
        }
        if !letras::a_utf8(&lang[..sin_cero(lang)], juego, &mut c.idioma) {
            letras::a_utf8(&lang[..sin_cero(lang)], Juego::Latin1, &mut c.idioma);
        }
        if !c.nombre.is_empty() {
            let suyo = c.nombre.strip_suffix(".catalog").or_else(|| c.nombre.strip_suffix(".CATALOG")).unwrap_or(&c.nombre);
            if !letras::mismo_nombre(suyo, pedido.programa) {
                return Err(Error::DeOtro);
            }
        }
        if pedido.version != 0 && c.version != pedido.version {
            return Err(Error::Version { pedida: pedido.version, tiene: c.version });
        }
        // Los textos: número (4), largo (4), los bytes y relleno hasta un múltiplo de 4.
        let mut resto = strs;
        while resto.len() >= 8 {
            let numero = u32::from_be_bytes([resto[0], resto[1], resto[2], resto[3]]);
            let largo = u32::from_be_bytes([resto[4], resto[5], resto[6], resto[7]]) as usize;
            let cuerpo = &resto[8..];
            if largo > cuerpo.len() {
                return Err(Error::TextoCortado);
            }
            let datos = &cuerpo[..largo];
            let datos = &datos[..sin_cero(datos)];
            let desde = c.texto.len();
            if letras::a_utf8(datos, juego, &mut c.texto) {
                c.indice.push((numero, desde as u32, c.texto.len() as u32));
            } else {
                c.malos += 1;
            }
            let paso = largo.next_multiple_of(4).min(cuerpo.len());
            resto = &cuerpo[paso..];
        }
        if !resto.is_empty() && resto.iter().any(|&b| b != 0) {
            // Sobra media cabecera: el fichero se cortó.
            return Err(Error::TextoCortado);
        }
        // Por número; de los repetidos vale el primero que salía en el fichero (la ordenación es estable).
        c.indice.sort_by_key(|e| e.0);
        let antes = c.indice.len();
        c.indice.dedup_by_key(|e| e.0);
        c.repetidos = antes - c.indice.len();
        Ok(c)
    }

    /// El texto con ese número, si el catálogo lo trae.
    pub fn texto(&self, numero: u32) -> Option<&str> {
        let i = self.indice.binary_search_by_key(&numero, |e| e.0).ok()?;
        let (_, desde, hasta) = self.indice[i];
        self.texto.get(desde as usize..hasta as usize)
    }

    /// El texto con ese número, o el de siempre del programa (como `GetCatalogStr`).
    pub fn o<'a>(&'a self, numero: u32, respaldo: &'a str) -> &'a str {
        self.texto(numero).unwrap_or(respaldo)
    }

    /// Cuántos textos trae.
    pub fn cuantos(&self) -> usize {
        self.indice.len()
    }

    /// Los números que trae, en orden.
    pub fn numeros(&self) -> impl Iterator<Item = u32> + '_ {
        self.indice.iter().map(|e| e.0)
    }
}

/// Escribe un catálogo (para las pruebas y para quien guarde uno): `$VER:`, idioma, juego y los textos.
pub fn escribir(nombre: &str, version: u32, revision: u32, fecha: &str, idioma: &str, juego: Juego, textos: &[(u32, &str)]) -> Vec<u8> {
    let codificar = |t: &str| -> Vec<u8> {
        let mut b = if juego == Juego::Utf8 { t.as_bytes().to_vec() } else { letras::a_latin1(t) };
        b.push(0);
        b
    };
    let mut cuerpo = Vec::new();
    let ver = alloc::format!("$VER: {nombre} {version}.{revision} ({fecha})");
    iff::trozo(b"FVER", &codificar(&ver), &mut cuerpo);
    iff::trozo(b"LANG", &codificar(idioma), &mut cuerpo);
    let mut cset = [0u8; 32];
    cset[0..4].copy_from_slice(&juego.numero().to_be_bytes());
    iff::trozo(b"CSET", &cset, &mut cuerpo);
    let mut strs = Vec::new();
    for (numero, t) in textos {
        let b = codificar(t);
        strs.extend_from_slice(&numero.to_be_bytes());
        strs.extend_from_slice(&(b.len() as u32).to_be_bytes());
        strs.extend_from_slice(&b);
        strs.resize(strs.len().next_multiple_of(4), 0);
    }
    iff::trozo(b"STRS", &strs, &mut cuerpo);
    iff::escribir_forma(b"CTLG", &cuerpo)
}
