//! Idiomas y catálogos para los programas nativos (S4b, paso 4 de `docs/REVISION_LOOP.md`; diseño en
//! `docs/bitacoras/idiomas.md`): el `Lector` que lee los ficheros de idioma del disco, por las asignaciones del
//! proceso (`ENV:`, `ENVARC:`, `LOCALE:`, `PROGDIR:`). Lo que entiende los ficheros y decide cuáles leer es el crate
//! `idioma` (`kit::idioma`), que no abre nada.
//!
//! Usa el montón, así que no va en la biblioteca `dashos` (que también enlazan programas sin montón): cada programa
//! lo incluye, como `ventana_cliente.rs`:
//!
//! ```ignore
//! #[path = "../idiomas.rs"]
//! mod idiomas;
//! /// El nombre de su catálogo y su versión (`tools/catalogo.py` lo lee de aquí).
//! const CATALOGO: (&str, u32) = ("explorador", 1);
//! static TEXTOS: idiomas::Puesto = idiomas::Puesto::nuevo();
//! /// Un texto, en el idioma del usuario. El número lo pone `tools/catalogo.py numerar` (se escribe 0).
//! fn tr(numero: u32, de_siempre: &'static str) -> &'static str { TEXTOS.texto(numero, de_siempre) }
//! /// Un texto con huecos (`{}`, `{0}`…) y sus valores.
//! fn trf(numero: u32, de_siempre: &'static str, valores: &[&dyn core::fmt::Display]) -> String { TEXTOS.con(numero, de_siempre, valores) }
//! // al arrancar, con su cliente del servicio de archivos y su tabla de nombres:
//! let idioma = idiomas::arrancar(&mut fs, &tabla, &TEXTOS, CATALOGO);
//! ```

use alloc::vec::Vec;
use dashos::asignaciones::{self, Fs, Sitios, Tabla};
use dashos::protocolo::archivos as a;
#[allow(unused_imports)]
pub use kit::idioma::{catalogo, fecha, formato, Catalogo, Falta, Firma, Idiomas, Lector, Local, Pais, Pedido, Prefs, Puesto, IDIOMA_DE_SERIE};

/// El servicio de archivos, por la tabla de nombres del proceso, como `Lector` de los ficheros de idioma.
pub struct PorAsignaciones<'a, F: Fs> {
    pub fs: &'a mut F,
    pub tabla: &'a Tabla,
}

impl<F: Fs> Lector for PorAsignaciones<'_, F> {
    /// Del primer sitio de las capas de `camino` donde esté, con `LEER_NOMBRE` (buscar, leer y cerrar en un viaje).
    fn leer(&mut self, camino: &str, tope: usize) -> Result<(Vec<u8>, Firma), Falta> {
        let mut sitios = Sitios::nuevos();
        if self.tabla.resolver(camino.as_bytes(), &mut sitios).is_err() {
            // Un nombre que no está en la tabla, o un camino que no vale: para quien lee, no existe.
            return Err(Falta::NoExiste);
        }
        let ventana = self.fs.ventana().len();
        if ventana == 0 {
            return Err(Falta::Otra(a::SIN_SITIO));
        }
        'sitios: for i in 0..sitios.len() {
            let Some((raiz, c)) = sitios.sitio(i) else { break };
            let mut bytes: Vec<u8> = Vec::new();
            loop {
                let r = self.fs.pedir(a::LEER_NOMBRE, raiz as u32, bytes.len() as u64, ventana as u64, c);
                if r.e == a::NO_EXISTE || r.e == a::ASA_MALA {
                    // En esta capa no está (o el proceso no tiene esa raíz): a la siguiente.
                    continue 'sitios;
                }
                if r.e != a::BIEN {
                    return Err(Falta::Otra(r.e));
                }
                let (leidos, fecha, total) = (r.a as usize, r.b, r.c);
                if total > tope as u64 {
                    return Err(Falta::Otra(a::LLENO));
                }
                // Nunca más de la ventana ni de lo que falta, diga lo que diga la respuesta.
                let falta = (total as usize).saturating_sub(bytes.len());
                let leidos = leidos.min(ventana).min(falta);
                bytes.extend_from_slice(&self.fs.ventana()[..leidos]);
                if leidos == falta {
                    return Ok((bytes, (fecha, total, i)));
                }
                if leidos == 0 {
                    // Se acortó mientras se leía: lo leído no es un fichero entero.
                    return Err(Falta::Otra(a::MAL_FORMADO));
                }
            }
        }
        Err(Falta::NoExiste)
    }

    fn firma(&mut self, camino: &str) -> Option<Firma> {
        asignaciones::mirar(self.fs, self.tabla, camino.as_bytes()).ok().map(|(r, capa)| (r.b, r.a, capa))
    }
}

/// Lo que hace un programa al arrancar: lee el idioma del usuario, pone el catálogo del kit y deja en `puesto` el del
/// programa. Lo que no se pueda leer se queda como de serie (castellano y España).
pub fn arrancar<F: Fs>(fs: &mut F, tabla: &Tabla, puesto: &Puesto, catalogo: (&str, u32)) -> Idiomas {
    kit::textos::arrancar(&mut PorAsignaciones { fs, tabla }, puesto, catalogo)
}
