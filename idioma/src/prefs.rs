//! `ENV:Sys/locale.prefs`: el fichero de preferencias de idioma de AmigaOS (IFF `PREF` con un trozo `LCLE`, el
//! `LocalePrefs` de 860 bytes), para que la caja Amiga y lo nativo compartan fichero: el país, hasta diez idiomas por
//! orden de preferencia, la diferencia con Greenwich y los datos del país.

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

impl Prefs {
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
