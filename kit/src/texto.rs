//! El texto (K2): Roboto (la fuente de Material 3, OFL 1.1, en `fuentes/`), con una caché de glifos por letra y tamaño.
//! Mide y dibuja líneas de UTF-8 con el espaciado entre pares de la fuente (su tabla `kern`, si la tiene).
//!
//! Cada letra se rasteriza cuando hace falta por primera vez, con su contorno leído de la fuente (`ttf-parser`) en ese
//! momento (kit-memoria). Antes lo hacía `fontdue`, que al cargar una fuente convierte en líneas los contornos de todas
//! sus letras: 1 MB de montón por fuente con las 378 de Roboto recortada, y cada programa del kit lleva dos.
//!
//! El rasterizador (en `trazos`, compartido con los iconos) acumula el área con signo, como el de `fontdue`: cada tramo
//! del contorno deja en las celdas que cruza lo que sube o baja, repartido entre su celda y la de la derecha según dónde
//! cae su punto medio; al sumar cada fila de izquierda a derecha, lo acumulado es la parte de cada píxel que cae dentro
//! de la letra.

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use ttf_parser::{Face, GlyphId};

use crate::lienzo::{Color, Lienzo};
use crate::trazos::{acumular, Contorno};

static REGULAR: &[u8] = include_bytes!("../fuentes/Roboto-Regular.ttf");
static MEDIUM: &[u8] = include_bytes!("../fuentes/Roboto-Medium.ttf");
/// Roboto Mono (OFL 1.1, `fuentes/OFL-RobotoMono.txt`): la de ancho fijo, para una terminal (la caja Linux, H9). Solo
/// entra en los programas que la usan.
static MONO: &[u8] = include_bytes!("../fuentes/RobotoMono-Regular.ttf");

/// Lo que mide una línea: ancho, y la altura de encima y de debajo de la línea base (todo en píxeles).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Medidas {
    pub ancho: i32,
    pub sube: i32,
    pub baja: i32,
}

impl Medidas {
    pub fn alto(&self) -> i32 {
        self.sube + self.baja
    }
}

/// Un glifo rasterizado: su caja (la esquina de abajo a la izquierda respecto al origen de la pluma, con `y` hacia
/// arriba), cuánto avanza la pluma y la cobertura de cada píxel (0 a 255), por filas de arriba abajo.
struct Glifo {
    xmin: i32,
    ymin: i32,
    ancho: usize,
    alto: usize,
    avance: f32,
    bits: Vec<u8>,
}

pub struct Fuente {
    cara: Face<'static>,
    glifos: BTreeMap<(char, u32), Glifo>,
}

impl Fuente {
    fn de(bytes: &'static [u8]) -> Self {
        Self { cara: Face::parse(bytes, 0).expect("Roboto se lee"), glifos: BTreeMap::new() }
    }

    /// Roboto normal: el texto de siempre.
    pub fn regular() -> Self {
        Self::de(REGULAR)
    }

    /// Roboto Medium: las etiquetas de los botones y los títulos de Material.
    pub fn medium() -> Self {
        Self::de(MEDIUM)
    }

    /// Roboto Mono: todas las letras miden lo mismo (una terminal).
    pub fn mono() -> Self {
        Self::de(MONO)
    }

    /// Lo que avanza la pluma con `c` a `tam` píxeles (en una de ancho fijo, el ancho de cualquier letra).
    pub fn avance(&mut self, c: char, tam: u32) -> f32 {
        self.glifo(c, tam).avance
    }

    /// El glifo de `c`, el 0 (la caja vacía) si la fuente no la tiene.
    fn indice(&self, c: char) -> GlyphId {
        self.cara.glyph_index(c).unwrap_or(GlyphId(0))
    }

    fn glifo(&mut self, c: char, tam: u32) -> &Glifo {
        let cara = &self.cara;
        let id = cara.glyph_index(c).unwrap_or(GlyphId(0));
        self.glifos.entry((c, tam)).or_insert_with(|| rasterizar(cara, id, tam))
    }

    /// Si la fuente tiene la letra (si no, sale su caja vacía).
    pub fn tiene(&self, c: char) -> bool {
        self.cara.glyph_index(c).is_some()
    }

    fn escala(&self, tam: u32) -> f32 {
        tam as f32 / self.cara.units_per_em() as f32
    }

    /// El espaciado entre el par `a`, `b` a `tam` píxeles, de la tabla `kern` (Roboto recortada no la trae).
    fn par(&self, a: char, b: char, tam: u32) -> f32 {
        let Some(kern) = self.cara.tables().kern else { return 0.0 };
        let (ia, ib) = (self.indice(a), self.indice(b));
        kern.subtables
            .into_iter()
            .find(|t| t.horizontal && !t.variable && !t.has_cross_stream)
            .and_then(|t| t.glyphs_kerning(ia, ib))
            .map_or(0.0, |v| v as f32 * self.escala(tam))
    }

    /// Lo que mide `texto` en una línea, a `tam` píxeles.
    pub fn medir(&mut self, texto: &str, tam: u32) -> Medidas {
        let e = self.escala(tam);
        let (sube, baja) = (libm::ceilf(self.cara.ascender() as f32 * e) as i32, libm::ceilf(-(self.cara.descender() as f32) * e) as i32);
        let mut x = 0.0f32;
        let mut antes = None;
        for c in texto.chars() {
            if let Some(a) = antes {
                x += self.par(a, c, tam);
            }
            x += self.glifo(c, tam).avance;
            antes = Some(c);
        }
        Medidas { ancho: libm::ceilf(x) as i32, sube, baja }
    }

    /// Dibuja `texto` con la línea base en `base`, desde `x`.
    pub fn dibujar(&mut self, l: &mut Lienzo, x: i32, base: i32, texto: &str, tam: u32, color: Color) {
        let mut pluma = x as f32;
        let mut antes = None;
        for c in texto.chars() {
            if let Some(a) = antes {
                pluma += self.par(a, c, tam);
            }
            let g = self.glifo(c, tam);
            // La caja del glifo: `ymin` es lo que baja por debajo de la línea base.
            let x0 = libm::roundf(pluma) as i32 + g.xmin;
            let y0 = base - g.alto as i32 - g.ymin;
            for gy in 0..g.alto {
                for gx in 0..g.ancho {
                    let cob = g.bits[gy * g.ancho + gx] as u32;
                    if cob > 0 {
                        l.punto(x0 + gx as i32, y0 + gy as i32, color, cob);
                    }
                }
            }
            pluma += g.avance;
            antes = Some(c);
        }
    }
}

/// Rasteriza el glifo `id` a `tam` píxeles.
fn rasterizar(cara: &Face<'static>, id: GlyphId, tam: u32) -> Glifo {
    let escala = tam as f32 / cara.units_per_em() as f32;
    let avance = cara.glyph_hor_advance(id).unwrap_or(0) as f32 * escala;
    let mut c = Contorno::nuevo(escala);
    if cara.outline_glyph(id, &mut c).is_none() || c.tramos.is_empty() {
        return Glifo { xmin: 0, ymin: 0, ancho: 0, alto: 0, avance, bits: Vec::new() };
    }
    let (xmin, ymin) = (libm::floorf(c.caja[0]), libm::floorf(c.caja[1]));
    let ancho = (libm::ceilf(c.caja[2]) - xmin).max(1.0) as usize;
    let alto = (libm::ceilf(c.caja[3]) - ymin).max(1.0) as usize;
    let arriba = ymin + alto as f32;
    // Dos celdas más por fila: lo que cae a la derecha de la última columna.
    let paso = ancho + 2;
    let mut a = vec![0.0f32; paso * alto];
    for t in &c.tramos {
        acumular(&mut a, paso, ancho, alto, t[0] - xmin, arriba - t[1], t[2] - xmin, arriba - t[3]);
    }
    let mut bits = vec![0u8; ancho * alto];
    for y in 0..alto {
        let mut suma = 0.0f32;
        for x in 0..ancho {
            suma += a[y * paso + x];
            bits[y * ancho + x] = (libm::fabsf(suma) * 255.9).clamp(0.0, 255.0) as u8;
        }
    }
    Glifo { xmin: xmin as i32, ymin: ymin as i32, ancho, alto, avance, bits }
}

#[cfg(test)]
mod pruebas {
    extern crate std;
    use super::*;
    use crate::lienzo::Rect;
    use alloc::vec;

    #[test]
    fn medir() {
        let mut f = Fuente::regular();
        let hola = f.medir("Hola", 16);
        assert!(hola.ancho > 20 && hola.ancho < 50, "{hola:?}");
        assert!(hola.sube >= 14 && hola.baja >= 3, "{hola:?}");
        // Más texto, más ancho; el doble de tamaño, el doble de ancho (con redondeo).
        assert!(f.medir("Hola, DashOS", 16).ancho > hola.ancho);
        let doble = f.medir("Hola", 32).ancho;
        assert!((doble - 2 * hola.ancho).abs() <= 2, "{doble} y {}", hola.ancho);
        assert_eq!(f.medir("", 16).ancho, 0);
        // La Medium es algo más ancha que la normal.
        assert!(Fuente::medium().medir("Aceptar", 14).ancho >= f.medir("Aceptar", 14).ancho);
    }

    #[test]
    fn rasterizar_llena_por_dentro() {
        // La I de 40: un trazo recto, lleno del todo por dentro y con su caja ajustada a la tinta.
        let f = Fuente::regular();
        let g = rasterizar(&f.cara, f.indice('I'), 40);
        assert!(g.ancho >= 3 && g.ancho <= 8 && g.alto >= 27 && g.alto <= 30, "{}x{}", g.ancho, g.alto);
        let llenos = (0..g.alto).filter(|&y| (0..g.ancho).any(|x| g.bits[y * g.ancho + x] >= 250)).count();
        assert!(llenos + 2 >= g.alto, "{llenos} de {}", g.alto);
        // Ninguna fila ni columna del borde de la caja está vacía.
        assert!((0..g.ancho).any(|x| g.bits[x] > 0) && (0..g.ancho).any(|x| g.bits[(g.alto - 1) * g.ancho + x] > 0));
        // El espacio no tiene tinta, pero avanza.
        let e = rasterizar(&f.cara, f.indice(' '), 40);
        assert!(e.bits.is_empty() && e.avance > 5.0);
    }

    #[test]
    fn letras_del_castellano() {
        let f = Fuente::regular();
        for c in "ñÑáéíóúüÁÉÍÓÚÜ¿¡€ç".chars() {
            assert!(f.tiene(c), "{c}");
        }
    }

    #[test]
    fn dibujar_dentro_de_su_caja() {
        let (w, h) = (120i32, 40i32);
        let mut px = vec![0u32; (w * h) as usize];
        let mut f = Fuente::regular();
        let m = f.medir("Ñandú", 20);
        let mut l = Lienzo::new(&mut px, w, h, w as usize);
        f.dibujar(&mut l, 10, 30, "Ñandú", 20, 0xff00_0000);
        // Hay tinta, y toda cae en la caja de la línea (con un píxel de margen por el suavizado).
        let caja = Rect::new(9, 30 - m.sube - 1, m.ancho + 2, m.alto() + 2);
        let mut tinta = 0;
        for y in 0..h {
            for x in 0..w {
                if px[(y * w + x) as usize] >> 24 > 0 {
                    tinta += 1;
                    assert!(caja.contiene(x, y), "({x}, {y}) fuera de {caja:?}");
                }
            }
        }
        assert!(tinta > 100, "{tinta}");
        // La tilde de la Ñ sube por encima de la n: hay tinta por encima de la altura de la x.
        let alto_x = 30 - 11;
        assert!((10..20).any(|x| (30 - m.sube..alto_x - 4).any(|y| px[(y * w + x) as usize] >> 24 > 0)));
    }
}
