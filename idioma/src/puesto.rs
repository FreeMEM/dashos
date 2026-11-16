//! El catálogo en uso de un programa (o del kit), a mano desde cualquier sitio: `static TEXTOS: Puesto`, y una función
//! `tr(número, "texto de siempre")` que lo consulta.

use crate::catalogo::Catalogo;
use crate::formato;
use alloc::boxed::Box;
use alloc::string::String;
use core::fmt::Display;
use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

/// El sitio del catálogo en uso. Vacío, todo sale con el texto de siempre.
pub struct Puesto(AtomicPtr<Catalogo>);

impl Default for Puesto {
    fn default() -> Self {
        Self::nuevo()
    }
}

impl Puesto {
    pub const fn nuevo() -> Self {
        Puesto(AtomicPtr::new(ptr::null_mut()))
    }

    /// Pone un catálogo (o ninguno: el idioma del programa). El anterior no se libera nunca: los textos que ya se
    /// dieron siguen valiendo. Cambiar de idioma cuesta, pues, lo que mide un catálogo; no es algo que se haga en bucle.
    pub fn poner(&self, catalogo: Option<Catalogo>) {
        let nuevo = match catalogo {
            Some(c) => Box::leak(Box::new(c)) as *mut Catalogo,
            None => ptr::null_mut(),
        };
        self.0.store(nuevo, Ordering::Release);
    }

    /// El catálogo puesto.
    pub fn catalogo(&self) -> Option<&'static Catalogo> {
        let p = self.0.load(Ordering::Acquire);
        // SEGURIDAD: `p` es nulo o viene de `Box::leak` en `poner`, y lo que se deja ahí no se libera ni se cambia
        // nunca: vale como referencia compartida para siempre.
        unsafe { p.as_ref() }
    }

    /// El texto con ese número, o el de siempre (como `GetCatalogStr`). El número 0 es "sin número": el de siempre.
    pub fn texto(&self, numero: u32, respaldo: &'static str) -> &'static str {
        match self.catalogo() {
            Some(c) if numero != 0 => c.texto(numero).unwrap_or(respaldo),
            _ => respaldo,
        }
    }

    /// Un texto con huecos (`{}`, `{0}`…) y sus valores. Si la traducción pide un valor que el original no da, el
    /// catálogo está mal hecho y sale el original.
    pub fn con(&self, numero: u32, respaldo: &'static str, valores: &[&dyn Display]) -> String {
        let t = self.texto(numero, respaldo);
        formato::poner(if formato::casa(respaldo, t) { t } else { respaldo }, valores)
    }
}
