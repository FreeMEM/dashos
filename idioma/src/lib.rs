//! Idiomas y catálogos de DashOS, como en AmigaOS (S4b, paso 4 de `docs/REVISION_LOOP.md`; diseño en
//! `docs/bitacoras/idiomas.md`).
//!
//! - `prefs`: `ENV:Sys/locale.prefs`, el mismo fichero IFF que usa AmigaOS: idiomas preferidos y país.
//! - `catalogo`: `LOCALE:Catalogs/<idioma>/<programa>.catalog`, IFF `CTLG`, en UTF-8 (y los de Latin-1 del Amiga).
//! - `pais` y `fecha`: fechas, horas, números y dinero según el país.
//! - `puesto` y `formato`: el catálogo en uso de un programa y los textos con huecos.
//!
//! - `sesion`: qué ficheros se leen y en qué orden, sobre un `Lector` que los da.
//!
//! Aquí no se abre ningún fichero: todo trabaja con bytes ya leídos, y se prueba en el PC (`cargo test`). El `Lector`
//! que lee del disco por las asignaciones del proceso está en `usuario/src/idiomas.rs`.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod catalogo;
pub mod fecha;
pub mod formato;
pub mod iff;
pub mod letras;
pub mod pais;
pub mod prefs;
pub mod puesto;
pub mod sesion;
#[cfg(test)]
mod pruebas;

pub use catalogo::{Catalogo, Pedido};
pub use pais::Pais;
pub use prefs::Prefs;
pub use puesto::Puesto;
pub use sesion::{Falta, Firma, Idiomas, Lector};

use alloc::string::String;
use alloc::vec::Vec;

/// El idioma en que están escritos los programas del sistema: con él no se busca catálogo.
pub const IDIOMA_DE_SERIE: &str = "español";
/// El país de serie.
pub const PAIS_DE_SERIE: &str = "España";

/// El idioma y el país del usuario, con todo lo que hace falta para escribir a su manera.
#[derive(Clone, Debug)]
pub struct Local {
    /// Los idiomas preferidos, por orden. Vacío: el de serie.
    pub idiomas: Vec<String>,
    pub pais_nombre: String,
    pub pais: Pais,
    /// Minutos al oeste de Greenwich.
    pub gmt: i32,
    /// Los textos del idioma (días, meses…): un catálogo con los números de `fecha::cadenas`
    /// (`LOCALE:Languages/<idioma>.language`). Sin él, los del castellano.
    pub textos: Option<Catalogo>,
}

impl Default for Local {
    fn default() -> Self {
        Self::de_serie()
    }
}

impl Local {
    /// Castellano y España: lo que hay sin `locale.prefs`, o si está roto.
    pub fn de_serie() -> Local {
        Local { idiomas: Vec::new(), pais_nombre: String::from(PAIS_DE_SERIE), pais: Pais::espana(), gmt: -60, textos: None }
    }

    /// De un `locale.prefs` leído. Si no trae los datos del país, se quedan los de España hasta que quien lo carga
    /// lea el `.country` (`pais_nombre`).
    pub fn de_prefs(p: Prefs) -> Local {
        let serie = Local::de_serie();
        Local {
            idiomas: p.idiomas,
            pais_nombre: if p.pais.is_empty() { serie.pais_nombre } else { p.pais },
            pais: p.datos.unwrap_or(serie.pais),
            gmt: p.gmt,
            textos: None,
        }
    }

    /// El primer idioma preferido, o el de serie.
    pub fn idioma(&self) -> &str {
        self.idiomas.first().map_or(IDIOMA_DE_SERIE, |s| s.as_str())
    }

    /// Los idiomas en los que buscar el catálogo de un programa escrito en `de_serie`, por orden: los preferidos
    /// hasta llegar al del propio programa (con ese no hace falta catálogo, como `OC_BuiltInLanguage`).
    pub fn donde_buscar<'a>(&'a self, de_serie: &'a str) -> impl Iterator<Item = &'a str> {
        self.idiomas.iter().map(|s| s.as_str()).take_while(move |i| !letras::mismo_nombre(i, de_serie))
    }

    /// Si los textos del idioma (días, meses) hay que leerlos de un fichero, de qué idioma.
    pub fn idioma_de_textos(&self) -> Option<&str> {
        self.donde_buscar(IDIOMA_DE_SERIE).next()
    }

    /// Un texto del idioma por su número (`fecha::cadenas`): el del idioma del usuario o el del castellano.
    pub fn cadena(&self, numero: u32) -> &str {
        let serie = || fecha::CASTELLANO.get((numero as usize).wrapping_sub(1)).copied().unwrap_or("");
        match &self.textos {
            Some(c) => c.texto(numero).unwrap_or_else(serie),
            None => serie(),
        }
    }

    /// El momento local de una hora del reloj (segundos desde 1970, en UTC).
    pub fn momento(&self, unix: i64) -> fecha::Momento {
        fecha::Momento::de_unix(unix.saturating_sub(self.gmt as i64 * 60))
    }

    /// Una hora del reloj escrita con una plantilla de `FormatDate`.
    pub fn con_plantilla(&self, plantilla: &str, unix: i64) -> String {
        fecha::sin_huecos(&fecha::formatear(plantilla, &self.momento(unix), &|n| self.cadena(n)))
    }

    /// "lunes, 3 de noviembre de 2026" en España.
    pub fn fecha_larga(&self, unix: i64) -> String {
        self.con_plantilla(&self.pais.fecha, unix)
    }

    /// "03/11/2026" en España.
    pub fn fecha_corta(&self, unix: i64) -> String {
        self.con_plantilla(&self.pais.fecha_corta, unix)
    }

    /// "17:05:09" en España.
    pub fn hora_larga(&self, unix: i64) -> String {
        self.con_plantilla(&self.pais.hora, unix)
    }

    /// "17:05" en España.
    pub fn hora_corta(&self, unix: i64) -> String {
        self.con_plantilla(&self.pais.hora_corta, unix)
    }

    /// "03/11/2026 17:05" en España.
    pub fn fecha_hora_corta(&self, unix: i64) -> String {
        self.con_plantilla(&self.pais.fecha_hora_corta, unix)
    }

    pub fn numero(&self, n: i64) -> String {
        self.pais.numero(n)
    }

    pub fn decimal(&self, valor: i64, decimales: u32) -> String {
        self.pais.decimal(valor, decimales)
    }

    pub fn moneda(&self, centimos: i64) -> String {
        self.pais.moneda(centimos)
    }
}
