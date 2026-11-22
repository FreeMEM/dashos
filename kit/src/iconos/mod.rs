//! Los iconos del kit (plan de iconos del 2026-11-13, `docs/diseño/S37-iconos.md`):
//!
//! - **Símbolos** (`Simbolos`): Material Symbols Rounded (Apache 2.0, `kit/iconos/NOTICE`), recortados a los que se
//!   usan (`kit/iconos/simbolos.lista` → `tools/iconos_material.py` → `simbolos_datos.rs`). Son de un color: los de la
//!   interfaz (la barra, el panel, los menús, Ajustes, el explorador).
//! - **La colección de DashOS** (`coleccion::Iconos`): carpetas, tipos de fichero, volúmenes y aparatos, dibujados aquí
//!   (`kit/iconos/dashos.iconos`), que toman los tonos del tema (Material You: del acento, que puede salir del fondo).
//! - **Temas de Linux** (`freedesktop`): los que instale el usuario (norma de temas de iconos de freedesktop), ninguno
//!   de serie. Su SVG lo lee `svg`, un lector mínimo; lo que no entiende da el icono de reserva.
//!
//! Todo se pinta con el rasterizador de las letras (`trazos`): un trazado de SVG es un contorno como el de una letra.
//! Cada máscara (un tamaño de un icono) se guarda la primera vez; la caché tiene un tope de memoria y se vacía al
//! llenarse. Nada de lo que llegue (un nombre que no existe, un tamaño de 0 o de cien mil, un trazado roto) tumba al
//! programa: sale el icono de reserva, dibujado con rectángulos.

use alloc::collections::BTreeMap;
use core::cell::RefCell;

use crate::lienzo::{Color, Lienzo, Rect};
use crate::trazos::{cubrir, Contorno, Mascara};

pub mod coleccion;
pub mod freedesktop;
pub mod svg;
pub mod trazado;

use trazado::Matriz;

/// Lo más grande que se pinta un icono (una máscara de 1024 × 1024 es 1 MB). Pedir más pinta este tamaño.
pub const MAX_LADO: i32 = 1024;
/// Lo que se deja ocupar a una caché de máscaras antes de vaciarla.
pub const TOPE_CACHE: usize = 512 * 1024;

/// Un Material Symbol de la lista (las constantes de `simbolo`), con su trazado: cada programa lleva solo los que usa.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Simbolo {
    pub(crate) nombre: &'static str,
    pub(crate) lleno: bool,
    pub(crate) d: &'static str,
}

/// Los símbolos que hay, por su nombre de Material (`simbolo::SEARCH`, `simbolo::DARK_MODE_LLENO`...).
pub mod simbolo {
    use super::Simbolo;
    include!("simbolos_datos.rs");
}

impl Simbolo {
    /// El nombre de Material y si es el lleno.
    pub fn nombre(&self) -> (&'static str, bool) {
        (self.nombre, self.lleno)
    }

    /// El símbolo `nombre` (lleno o de contorno), si está en la lista. Quien lo usa lleva todos los de la lista.
    pub fn por_nombre(nombre: &str, lleno: bool) -> Option<Simbolo> {
        simbolo::TODOS.binary_search_by(|s| (s.nombre, s.lleno).cmp(&(nombre, lleno))).ok().map(|i| simbolo::TODOS[i])
    }

    /// Un símbolo con un trazado cualquiera (en la caja de Material), para probar o para un símbolo propio.
    pub const fn propio(nombre: &'static str, d: &'static str) -> Simbolo {
        Simbolo { nombre, lleno: false, d }
    }

    /// Su trazado (en la caja de Material), para darlo en un atributo (`atr::ICONOS` de una lista).
    pub fn trazado(&self) -> &'static str {
        self.d
    }

    /// Para la caché: un resumen (FNV-1a) del trazado, que lo identifica venga de donde venga.
    pub(crate) fn llave(&self) -> usize {
        llave_de(self.d)
    }
}

pub(crate) fn llave_de(d: &str) -> usize {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in d.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    h as usize
}

/// El lado de verdad para pintar uno de `lado`: nada si es 0 o menos, y como mucho `MAX_LADO`.
pub(crate) fn lado_valido(lado: i32) -> Option<i32> {
    (lado > 0).then_some(lado.min(MAX_LADO))
}

/// La máscara de un trazado de la caja de Material (0 -960 960 960) a `lado` píxeles, o `None` si está roto.
pub(crate) fn mascara_material(d: &str, lado: i32) -> Option<Mascara> {
    let s = lado as f32 / 960.0;
    let mut c = Contorno::nuevo(1.0);
    trazado::trazar::<false>(d, &Matriz::escala(s, 0.0, 960.0 * s), &mut c).ok()?;
    Some(cubrir(&c, lado))
}

/// La máscara de un trazado por la matriz `m`, en un cuadro de `lado`, o `None` si está roto.
pub(crate) fn rasterizar(d: &str, m: &Matriz, lado: i32) -> Option<Mascara> {
    let mut c = Contorno::nuevo(1.0);
    trazado::trazar::<true>(d, m, &mut c).ok()?;
    Some(cubrir(&c, lado))
}

/// Una caché de máscaras con tope de memoria.
pub(crate) struct Cache<K: Ord + Copy> {
    mapa: BTreeMap<K, Option<Mascara>>,
    bytes: usize,
}

impl<K: Ord + Copy> Cache<K> {
    pub(crate) const fn new() -> Self {
        Cache { mapa: BTreeMap::new(), bytes: 0 }
    }

    /// La de `k`, hecha con `hacer` la primera vez (`None` si no se pudo, y también se recuerda).
    pub(crate) fn dar(&mut self, k: K, hacer: impl FnOnce() -> Option<Mascara>) -> Option<&Mascara> {
        if !self.mapa.contains_key(&k) {
            let m = hacer();
            let b = m.as_ref().map_or(16, Mascara::bytes);
            if self.bytes + b > TOPE_CACHE {
                self.mapa.clear();
                self.bytes = 0;
            }
            self.bytes += b;
            self.mapa.insert(k, m);
        }
        self.mapa.get(&k).and_then(Option::as_ref)
    }

    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }
}

/// Los símbolos de la interfaz de un programa, con su caché (como `Fuente` con las letras). Se pintan con `&self` (la
/// caché va en un `RefCell`), para que quepan en las estructuras que se pintan sin `mut` (la barra, los menús).
pub struct Simbolos {
    cache: RefCell<Cache<(usize, i32)>>,
}

impl Default for Simbolos {
    fn default() -> Self {
        Self::new()
    }
}

impl Simbolos {
    pub const fn new() -> Self {
        Simbolos { cache: RefCell::new(Cache::new()) }
    }

    /// Pinta `s` en el cuadro de `lado` con la esquina en (`x`, `y`), en `color`. El de Material deja su margen
    /// (2 de 24): el dibujo ocupa los 20/24 del centro.
    pub fn pintar(&self, l: &mut Lienzo, x: i32, y: i32, lado: i32, s: Simbolo, color: Color) {
        self.pintar_trazado(l, x, y, lado, s.d, color);
    }

    /// Pinta el trazado `d` (de la caja de Material, como el de un símbolo) en el cuadro de `lado`.
    pub fn pintar_trazado(&self, l: &mut Lienzo, x: i32, y: i32, lado: i32, d: &str, color: Color) {
        let Some(lado) = lado_valido(lado) else { return };
        let hecho = self.cache.try_borrow_mut().ok().map(|mut c| c.dar((llave_de(d), lado), || mascara_material(d, lado)).map(|m| m.pintar(l, x, y, color)).is_some());
        match hecho {
            Some(true) => {}
            Some(false) => reserva_simbolo(l, x, y, lado, color),
            None => match mascara_material(d, lado) {
                Some(m) => m.pintar(l, x, y, color),
                None => reserva_simbolo(l, x, y, lado, color),
            },
        }
    }

    /// Pinta `s` de `lado` centrado en `r`.
    pub fn centrado(&self, l: &mut Lienzo, r: Rect, lado: i32, s: Simbolo, color: Color) {
        self.pintar(l, r.x + (r.w - lado) / 2, r.y + (r.h - lado) / 2, lado, s, color);
    }

    /// Lo que ocupa la caché.
    pub fn bytes(&self) -> usize {
        self.cache.try_borrow().map_or(0, |c| c.bytes())
    }
}

/// El símbolo de reserva (un trazado que no se pudo leer): un cuadrado redondeado hueco, hecho con rectángulos.
pub fn reserva_simbolo(l: &mut Lienzo, x: i32, y: i32, lado: i32, color: Color) {
    let Some(lado) = lado_valido(lado) else { return };
    let m = (lado / 6).max(0);
    let r = Rect::new(x + m, y + m, lado - 2 * m, lado - 2 * m);
    if r.w > 2 {
        l.borde(r, (lado / 8).max(1), (lado / 12).max(1), color);
    } else {
        l.rellenar(r, color);
    }
}

#[cfg(test)]
mod pruebas;
