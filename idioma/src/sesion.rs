//! El idioma del usuario, leído: qué ficheros se miran y en qué orden. Los ficheros los da un `Lector` (en DashOS, el
//! servicio de archivos por las asignaciones del proceso; en las pruebas, uno de mentira).
//!
//! - El idioma y el país salen de `ENV:Sys/locale`, su texto, que manda (S36, `prefs::Prefs::de_texto`); si no está o
//!   no trae ni idiomas ni país, de `ENV:Sys/locale.prefs` (el fichero de AmigaOS, que se genera del texto, o el que
//!   deja la caja); si no, lo mismo en `ENVARC:`; si tampoco, o el que hay está roto, castellano y España.
//! - El catálogo de un programa, de `PROGDIR:Catalogs/<idioma>/<programa>.catalog` o de
//!   `LOCALE:Catalogs/<idioma>/<programa>.catalog`, en el primer idioma preferido que lo tenga.
//! - Los textos del idioma (días, meses), de `LOCALE:Languages/<idioma>.language`; los datos del país, si el
//!   `locale.prefs` no los trae, de `LOCALE:Countries/<país>.country`.
//! - La zona horaria, de `SYS:Prefs/zona`: es del sistema, la misma que enseña la barra. Si no está, no se entiende o
//!   no está en la tabla, valen los minutos de `locale.prefs`.
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

/// Dónde están las preferencias en uso y las guardadas: el texto, que manda, y el IFF del Amiga.
pub const TEXTO: &str = "ENV:Sys/locale";
pub const PREFS: &str = "ENV:Sys/locale.prefs";
pub const TEXTO_GUARDADO: &str = "ENVARC:Sys/locale";
pub const PREFS_GUARDADAS: &str = "ENVARC:Sys/locale.prefs";
/// Por dónde se busca, en orden.
const CAMINOS: [(&str, bool); 4] = [(TEXTO, true), (PREFS, false), (TEXTO_GUARDADO, true), (PREFS_GUARDADAS, false)];
/// Lo más que mide un `locale.prefs` o un `.country` que se lee (los de verdad no llegan a 1 KB).
pub const MAX_PREFS: usize = 4096;
/// La zona horaria del sistema (la de la barra) y lo más que mide su fichero (una línea).
pub const ZONA: &str = "SYS:Prefs/zona";
pub const MAX_ZONA: usize = 256;

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
    /// De qué fichero salió (`TEXTO`, `PREFS`, `TEXTO_GUARDADO`, `PREFS_GUARDADAS`), o `None` si no hay ninguno y es lo
    /// de serie.
    pub origen: Option<&'static str>,
    /// Los ficheros que se miraron hasta dar con el que manda, con su firma de entonces (`None`: no estaba).
    vistos: Vec<(&'static str, Option<Firma>)>,
    /// La firma de `ZONA` cuando se leyó (`None`: no estaba).
    zona_firma: Option<Firma>,
}

impl Idiomas {
    /// Lo de serie, sin leer nada: castellano y España.
    pub fn de_serie() -> Idiomas {
        Idiomas { local: Local::de_serie(), origen: None, vistos: Vec::new(), zona_firma: None }
    }

    /// Lee las preferencias de idioma del usuario y lo que cuelga de ellas. No falla: lo que no se pueda leer se
    /// queda como de serie.
    pub fn cargar<L: Lector>(l: &mut L) -> Idiomas {
        let mut yo = Idiomas::de_serie();
        for (camino, es_texto) in CAMINOS {
            match l.leer(camino, MAX_PREFS) {
                Ok((bytes, firma)) => {
                    yo.vistos.push((camino, Some(firma)));
                    // El fichero que hay manda aunque esté roto: uno roto en ENV: no deja paso al guardado. Un texto
                    // sin idiomas ni país (solo comentarios) deja paso al IFF de su misma capa.
                    let leido = if es_texto {
                        match Prefs::de_texto(&bytes) {
                            Ok(None) => continue,
                            Ok(Some(p)) => Some(p),
                            Err(_) => None,
                        }
                    } else {
                        Prefs::leer(&bytes).ok()
                    };
                    yo.origen = Some(camino);
                    if let Some(p) = leido {
                        let sin_datos = p.datos.is_none();
                        yo.local = Local::de_prefs(p);
                        if sin_datos {
                            yo.leer_pais(l);
                        }
                    }
                    break;
                }
                Err(Falta::NoExiste) => yo.vistos.push((camino, None)),
                // No se deja leer, o es enorme: lo de serie, sin mirar los demás.
                Err(Falta::Otra(_)) => {
                    yo.vistos.push((camino, l.firma(camino)));
                    yo.origen = Some(camino);
                    break;
                }
            }
        }
        yo.leer_textos(l);
        yo.leer_zona(l);
        yo
    }

    /// La zona horaria del sistema. Si no está, no se deja leer, no es texto o no está en la tabla, sin zona: los
    /// minutos de `locale.prefs`.
    fn leer_zona<L: Lector>(&mut self, l: &mut L) {
        match l.leer(ZONA, MAX_ZONA) {
            Ok((bytes, firma)) => {
                self.zona_firma = Some(firma);
                self.local.zona = crate::zona::de_fichero(&bytes);
            }
            Err(Falta::NoExiste) => {}
            // Está pero no se lee (o es enorme): su firma, para que `ha_cambiado` no lo tome por un cambio.
            Err(Falta::Otra(_)) => self.zona_firma = l.firma(ZONA),
        }
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
    /// estaba delante del que manda), o la zona horaria del sistema cambió: hay que `cargar` otra vez y abrir los
    /// catálogos de nuevo. Cuesta una pregunta al servicio de archivos por fichero mirado. Para quien no vigila: quien
    /// recibe los avisos de cambio de `ENV:Sys` (`archivos`, `VIGILAR`) carga otra vez sin preguntar (la fecha va en
    /// segundos: dos cambios del mismo tamaño en el mismo segundo no se distinguen por la firma).
    pub fn ha_cambiado<L: Lector>(&self, l: &mut L) -> bool {
        // La zona horaria: otra fecha o tamaño, o aparece o desaparece.
        if l.firma(ZONA) != self.zona_firma {
            return true;
        }
        self.vistos.iter().any(|&(camino, antes)| l.firma(camino) != antes)
    }
}
