//! El idioma del usuario, leído: qué ficheros se miran y en qué orden. Los ficheros los da un `Lector` (en DashOS, el
//! servicio de archivos por las asignaciones del proceso; en las pruebas, uno de mentira).
//!
//! - El idioma y el país salen de `ENV:Sys/locale.prefs` (el fichero de AmigaOS); si no está, de
//!   `ENVARC:Sys/locale.prefs`; si tampoco, o está roto, castellano y España.
//! - El catálogo de un programa, de `PROGDIR:Catalogs/<idioma>/<programa>.catalog` o de
//!   `LOCALE:Catalogs/<idioma>/<programa>.catalog`, en el primer idioma preferido que lo tenga.
//! - Los textos del idioma (días, meses), de `LOCALE:Languages/<idioma>.language`; los datos del país, si el
//!   `locale.prefs` no los trae, de `LOCALE:Countries/<país>.country`.
//!
//! Un fichero que falta, que no se deja leer o que está roto no es un error: se sigue con lo de serie.

use crate::catalogo::{self, Catalogo, Pedido};
use crate::letras;
use crate::pais::Pais;
use crate::prefs::Prefs;
use crate::puesto::Puesto;
use crate::{Local, IDIOMA_DE_SERIE};
use alloc::string::String;
use alloc::vec::Vec;

/// Dónde están las preferencias en uso y las guardadas.
pub const PREFS: &str = "ENV:Sys/locale.prefs";
pub const PREFS_GUARDADAS: &str = "ENVARC:Sys/locale.prefs";
/// Lo más que mide un `locale.prefs` o un `.country` que se lee (los de verdad no llegan a 1 KB).
pub const MAX_PREFS: usize = 4096;

/// Por dónde se reconoce un fichero si cambia: fecha, tamaño y en qué capa de su asignación estaba.
pub type Firma = (u64, u64, usize);

/// Por qué no se leyó un fichero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Falta {
    /// No está (en ninguna capa).
    NoExiste,
    /// Está, pero no se deja leer, mide más del tope, o el servicio falló (su estado).
    Otra(i32),
}

/// Quien lee los ficheros.
pub trait Lector {
    /// El fichero `camino` (con nombre: `LOCALE:Catalogs/english/kit.catalog`), entero. Si mide más de `tope`, no se
    /// lee.
    fn leer(&mut self, camino: &str, tope: usize) -> Result<(Vec<u8>, Firma), Falta>;
    /// La firma de `camino` sin leerlo, o `None` si no está.
    fn firma(&mut self, camino: &str) -> Option<Firma>;
}

/// El idioma y el país del usuario, leídos, y de dónde salieron.
pub struct Idiomas {
    pub local: Local,
    /// De qué fichero salió (`PREFS`, `PREFS_GUARDADAS`), o `None` si no hay ninguno y es lo de serie.
    pub origen: Option<&'static str>,
    firma: Option<Firma>,
}

impl Idiomas {
    /// Lo de serie, sin leer nada: castellano y España.
    pub fn de_serie() -> Idiomas {
        Idiomas { local: Local::de_serie(), origen: None, firma: None }
    }

    /// Lee las preferencias de idioma del usuario y lo que cuelga de ellas. No falla: lo que no se pueda leer se
    /// queda como de serie.
    pub fn cargar<L: Lector>(l: &mut L) -> Idiomas {
        let mut yo = Idiomas::de_serie();
        for camino in [PREFS, PREFS_GUARDADAS] {
            match l.leer(camino, MAX_PREFS) {
                Ok((bytes, firma)) => {
                    // El fichero que hay manda aunque esté roto: uno roto en ENV: no deja paso al guardado.
                    yo.origen = Some(camino);
                    yo.firma = Some(firma);
                    if let Ok(p) = Prefs::leer(&bytes) {
                        let sin_datos = p.datos.is_none();
                        yo.local = Local::de_prefs(p);
                        if sin_datos {
                            yo.leer_pais(l);
                        }
                    }
                    break;
                }
                Err(Falta::NoExiste) => {}
                // No se deja leer, o es enorme: lo de serie, sin mirar el otro.
                Err(Falta::Otra(_)) => {
                    yo.origen = Some(camino);
                    break;
                }
            }
        }
        yo.leer_textos(l);
        yo
    }

    /// Los datos del país desde su `.country`, por su nombre tal cual y en minúsculas (los ficheros del Amiga van en
    /// minúsculas y el `locale.prefs` dice `España`).
    fn leer_pais<L: Lector>(&mut self, l: &mut L) {
        let nombre = self.local.pais_nombre.clone();
        if !letras::nombre_valido(&nombre) {
            return;
        }
        for n in [nombre.clone(), nombre.to_lowercase()] {
            let camino = alloc::format!("LOCALE:Countries/{n}.country");
            if let Some(p) = l.leer(&camino, MAX_PREFS).ok().and_then(|(b, _)| Pais::leer(&b)) {
                self.local.pais = p;
                return;
            }
        }
    }

    /// Los textos del idioma (días, meses): los del primer idioma preferido que tenga su `.language`.
    fn leer_textos<L: Lector>(&mut self, l: &mut L) {
        let idiomas: Vec<String> = self.local.donde_buscar(IDIOMA_DE_SERIE).map(String::from).collect();
        for i in idiomas {
            let nombre = alloc::format!("{i}.language");
            let camino = alloc::format!("LOCALE:Languages/{nombre}");
            let leido = l.leer(&camino, catalogo::MAX_BYTES).ok();
            if let Some(c) = leido.and_then(|(b, _)| Catalogo::leer(&b, Pedido { programa: &nombre, version: 0 }).ok()) {
                self.local.textos = Some(c);
                return;
            }
        }
    }

    /// El catálogo de `programa` (escrito en `de_serie`) en el idioma del usuario, como `OpenCatalog`: `None` si su
    /// idioma es el del programa o no hay catálogo que valga (entonces, los textos de siempre).
    pub fn catalogo<L: Lector>(&self, l: &mut L, programa: &str, version: u32, de_serie: &str) -> Option<Catalogo> {
        if !letras::nombre_valido(programa) {
            return None;
        }
        for i in self.local.donde_buscar(de_serie) {
            for cajon in ["PROGDIR:Catalogs", "LOCALE:Catalogs"] {
                let camino = alloc::format!("{cajon}/{i}/{programa}.catalog");
                let Ok((bytes, _)) = l.leer(&camino, catalogo::MAX_BYTES) else { continue };
                if let Ok(c) = Catalogo::leer(&bytes, Pedido { programa, version }) {
                    return Some(c);
                }
            }
        }
        None
    }

    /// Deja en `puesto` el catálogo de `programa` (escrito en castellano), o ninguno. Devuelve si hay catálogo.
    pub fn poner<L: Lector>(&self, l: &mut L, puesto: &Puesto, programa: &str, version: u32) -> bool {
        let c = self.catalogo(l, programa, version, IDIOMA_DE_SERIE);
        let hay = c.is_some();
        puesto.poner(c);
        hay
    }

    /// Si las preferencias de idioma ya no son las que se leyeron (otra fecha, otro tamaño, o un fichero que antes no
    /// estaba): hay que `cargar` otra vez y abrir los catálogos de nuevo. Cuesta una o dos preguntas al servicio de
    /// archivos. Hasta que `archivos` avise de los cambios, es la forma de enterarse.
    pub fn ha_cambiado<L: Lector>(&self, l: &mut L) -> bool {
        let ahora = match l.firma(PREFS) {
            Some(f) => Some((PREFS, f)),
            None => l.firma(PREFS_GUARDADAS).map(|f| (PREFS_GUARDADAS, f)),
        };
        match (ahora, self.origen, self.firma) {
            (None, None, _) => false,
            (Some((camino, f)), Some(origen), Some(antes)) => camino != origen || f != antes,
            // Estaba y no se dejó leer: no se sabe más que si es el mismo fichero.
            (Some((camino, _)), Some(origen), None) => camino != origen,
            _ => true,
        }
    }
}
