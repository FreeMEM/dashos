//! Los textos del kit, por catálogo (`LOCALE:Catalogs/<idioma>/kit.catalog`): el kit está escrito en castellano y, con
//! un catálogo puesto, dice lo suyo en el idioma del usuario. El kit no abre ficheros: los lee el `Lector` del programa
//! (`arrancar`, al empezar).
//!
//! Cada texto va como `tr(número, "texto de siempre")`. `tools/catalogo.py` los saca de ahí a `locale/kit.cd`.

use idioma::{Catalogo, Idiomas, Lector, Puesto};

/// El nombre de su catálogo y su versión, que sube cuando cambia lo que significa un número. `tools/catalogo.py` lo
/// lee de aquí.
pub const CATALOGO: (&str, u32) = ("kit", 1);
/// El idioma en que está escrito.
pub const IDIOMA: &str = idioma::IDIOMA_DE_SERIE;

static TEXTOS: Puesto = Puesto::nuevo();

/// Pone el catálogo del kit (o lo quita: castellano).
pub fn poner_catalogo(catalogo: Option<Catalogo>) {
    TEXTOS.poner(catalogo);
}

/// El texto número `numero` del kit en el idioma del usuario, o el de siempre.
pub fn tr(numero: u32, respaldo: &'static str) -> &'static str {
    TEXTOS.texto(numero, respaldo)
}

/// Lo que hace un programa al arrancar: lee el idioma del usuario, pone el catálogo del kit y deja en `puesto` el del
/// programa (su nombre y su versión, como `CATALOGO`; escrito en castellano). Lo que no se pueda leer se queda como de
/// serie.
pub fn arrancar<L: Lector>(lector: &mut L, puesto: &Puesto, (programa, version): (&str, u32)) -> Idiomas {
    let idiomas = Idiomas::cargar(lector);
    poner_catalogo(idiomas.catalogo(lector, CATALOGO.0, CATALOGO.1, IDIOMA));
    idiomas.poner(lector, puesto, programa, version);
    idiomas
}
