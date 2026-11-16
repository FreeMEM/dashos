//! `ENV:Sys/locale.prefs`: el fichero de preferencias de idioma de AmigaOS (IFF `PREF` con un trozo `LCLE`, el
//! `LocalePrefs` de 860 bytes), para que la caja Amiga y lo nativo compartan fichero: el país, hasta diez idiomas por
//! orden de preferencia, la diferencia con Greenwich y los datos del país.
//!
//! Y su texto, que manda (S36, regla 1; `docs/bitacoras/ajustes-texto.md`): `ENV:Sys/locale`, en el formato de
//! `crate::texto`, con `idiomas` (por orden, separados por comas) y `pais`. Quien lo escribe (la orden `Ajuste`, el
//! centro de control) genera el IFF de él con `escribir`; editado a mano vale igual para lo nativo.

use crate::iff;
use crate::letras::{de_campo, en_campo, mismo_nombre, nombre_valido};
use crate::pais::{self, Pais};
use alloc::string::String;
use alloc::vec::Vec;

/// Lo que mide un `LocalePrefs`.
pub const LARGO: usize = 860;
/// Cuántos idiomas caben, y lo que mide el nombre de cada uno.
pub const MAX_IDIOMAS: usize = 10;
const LARGO_IDIOMA: usize = 30;
const EN_PAIS: usize = 16;
const EN_IDIOMAS: usize = 48;
const EN_GMT: usize = 348;
const EN_FLAGS: usize = 352;
const EN_DATOS: usize = 356;

/// Por qué no vale un `locale.prefs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Formato(iff::Error),
    /// No trae el trozo `LCLE`, o es más corto de lo que debe.
    SinDatos,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Prefs {
    /// El nombre del país (`España`), o vacío.
    pub pais: String,
    /// Los idiomas preferidos, por orden. Vacío: el de serie.
    pub idiomas: Vec<String>,
    /// Minutos al oeste de Greenwich (España en invierno, -60).
    pub gmt: i32,
    pub flags: u32,
    /// Los datos del país, si vienen completos.
    pub datos: Option<Pais>,
}

/// Las claves del texto.
pub const CLAVE_IDIOMAS: &str = "idiomas";
pub const CLAVE_PAIS: &str = "pais";
/// Lo que lleva encima un `Sys/locale` nuevo.
pub const CABECERA_TEXTO: &str = "Idioma y país (Ajustes > Idioma y región; la orden `Ajuste idioma`).\nEste fichero manda: Sys/locale.prefs, el que lee la caja Amiga, se genera de él.\nidiomas: por orden de preferencia, separados por comas (english, español). pais: el de LOCALE:Countries.";

/// Los idiomas de una lista separada por comas (`english, español`): sin vacíos ni repetidos, hasta `MAX_IDIOMAS`.
/// `Err` con el primero que no vale como nombre de fichero (con barras, dos puntos, de control, largo) o si son más de
/// `MAX_IDIOMAS`.
pub fn lista_de_idiomas(texto: &str) -> Result<Vec<String>, String> {
    let mut v: Vec<String> = Vec::new();
    for i in texto.split(',').map(str::trim).filter(|i| !i.is_empty()) {
        if !nombre_valido(i) {
            return Err(i.into());
        }
        if !v.iter().any(|x| mismo_nombre(x, i)) {
            if v.len() == MAX_IDIOMAS {
                return Err(i.into());
            }
            v.push(i.into());
        }
    }
    Ok(v)
}

impl Prefs {
    /// Del texto (`Sys/locale`). `Ok(None)` si no trae ni `idiomas` ni `pais` (entonces manda el IFF de la misma capa).
    /// Lo que no vale se salta: un idioma con un nombre malo no entra; un país malo se queda vacío (el de serie).
    pub fn de_texto(bytes: &[u8]) -> Result<Option<Prefs>, crate::texto::Error> {
        let a = crate::texto::Ajustes::leer(bytes)?;
        let (idiomas, pais) = (a.valor(CLAVE_IDIOMAS), a.valor(CLAVE_PAIS));
        if idiomas.is_none() && pais.is_none() {
            return Ok(None);
        }
        let mut p = Prefs::default();
        for i in idiomas.unwrap_or("").split(',').map(str::trim) {
            if nombre_valido(i) && !p.idiomas.iter().any(|x| mismo_nombre(x, i)) && p.idiomas.len() < MAX_IDIOMAS {
                p.idiomas.push(i.into());
            }
        }
        if let Some(pais) = pais.filter(|x| nombre_valido(x)) {
            p.pais = pais.into();
        }
        Ok(Some(p))
    }

    /// El texto, sobre el que había (`antes`, si se entiende: sus comentarios y lo demás se quedan); uno nuevo lleva
    /// `CABECERA_TEXTO`. Solo los idiomas y el país: la diferencia horaria es de la zona del sistema, y los datos del
    /// país, de su `.country`.
    pub fn a_texto(&self, antes: Option<&[u8]>) -> Vec<u8> {
        let mut a = antes.and_then(|b| crate::texto::Ajustes::leer(b).ok()).unwrap_or_else(|| {
            let mut a = crate::texto::Ajustes::nuevo();
            a.comentar(CABECERA_TEXTO);
            a
        });
        // Valen siempre: nombres que ya pasaron por `nombre_valido`.
        let _ = a.poner(CLAVE_IDIOMAS, &self.idiomas.join(", "));
        let _ = a.poner(CLAVE_PAIS, &self.pais);
        a.texto().into_bytes()
    }

    pub fn leer(bytes: &[u8]) -> Result<Prefs, Error> {
        let mut datos = None;
        for t in iff::forma(bytes, b"PREF").map_err(Error::Formato)? {
            let t = t.map_err(Error::Formato)?;
            if &t.id == b"LCLE" && datos.is_none() {
                datos = Some(t.datos);
            }
        }
        let b = datos.and_then(|d| d.get(..LARGO)).ok_or(Error::SinDatos)?;
        let mut p = Prefs { pais: de_campo(&b[EN_PAIS..EN_IDIOMAS]), ..Prefs::default() };
        // Un nombre que no vale (con barras, de control…) no llega a formar parte de un camino.
        if !nombre_valido(&p.pais) {
            p.pais.clear();
        }
        for i in 0..MAX_IDIOMAS {
            let campo = &b[EN_IDIOMAS + i * LARGO_IDIOMA..EN_IDIOMAS + (i + 1) * LARGO_IDIOMA];
            let nombre = de_campo(campo);
            if nombre.is_empty() {
                // Como en AmigaOS: la lista acaba en el primer hueco (detrás puede quedar basura de otra vez).
                break;
            }
            if nombre_valido(&nombre) && !p.idiomas.iter().any(|x| mismo_nombre(x, &nombre)) {
                p.idiomas.push(nombre);
            }
        }
        let gmt = i32::from_be_bytes(b[EN_GMT..EN_GMT + 4].try_into().unwrap());
        // De -14 a +12 horas; otra cosa es basura.
        p.gmt = if (-14 * 60..=12 * 60).contains(&gmt) { gmt } else { 0 };
        p.flags = u32::from_be_bytes(b[EN_FLAGS..EN_FLAGS + 4].try_into().unwrap());
        p.datos = Pais::de_datos(&b[EN_DATOS..]).filter(Pais::completo);
        Ok(p)
    }

    /// El fichero. Sin datos de país, van a ceros (quien lo lea irá al `.country`).
    pub fn escribir(&self) -> Vec<u8> {
        let mut b = [0u8; LARGO];
        en_campo(&self.pais, &mut b[EN_PAIS..EN_IDIOMAS]);
        for (i, idioma) in self.idiomas.iter().take(MAX_IDIOMAS).enumerate() {
            en_campo(idioma, &mut b[EN_IDIOMAS + i * LARGO_IDIOMA..EN_IDIOMAS + (i + 1) * LARGO_IDIOMA]);
        }
        b[EN_GMT..EN_GMT + 4].copy_from_slice(&self.gmt.to_be_bytes());
        b[EN_FLAGS..EN_FLAGS + 4].copy_from_slice(&self.flags.to_be_bytes());
        if let Some(d) = &self.datos {
            b[EN_DATOS..EN_DATOS + pais::LARGO].copy_from_slice(&d.a_datos());
        }
        let mut cuerpo = Vec::new();
        iff::trozo(b"PRHD", &[0; 6], &mut cuerpo);
        iff::trozo(b"LCLE", &b, &mut cuerpo);
        iff::escribir_forma(b"PREF", &cuerpo)
    }
}
