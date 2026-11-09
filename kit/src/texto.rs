//! El texto (K2): Roboto (la fuente de Material 3, OFL 1.1, en `fuentes/`) rasterizada con `fontdue`, con una caché
//! de glifos por letra y tamaño. Mide y dibuja líneas de UTF-8 con el espaciado entre pares de la fuente.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use fontdue::{Font, FontSettings, Metrics};

use crate::lienzo::{Color, Lienzo};

static REGULAR: &[u8] = include_bytes!("../fuentes/Roboto-Regular.ttf");
static MEDIUM: &[u8] = include_bytes!("../fuentes/Roboto-Medium.ttf");

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

pub struct Fuente {
    fuente: Font,
    glifos: BTreeMap<(char, u32), (Metrics, Vec<u8>)>,
}

impl Fuente {
    fn de(bytes: &'static [u8]) -> Self {
        let fuente = Font::from_bytes(bytes, FontSettings::default()).expect("Roboto se lee");
        Self { fuente, glifos: BTreeMap::new() }
    }

    /// Roboto normal: el texto de siempre.
    pub fn regular() -> Self {
        Self::de(REGULAR)
    }

    /// Roboto Medium: las etiquetas de los botones y los títulos de Material.
    pub fn medium() -> Self {
        Self::de(MEDIUM)
    }

    fn glifo(&mut self, c: char, tam: u32) -> &(Metrics, Vec<u8>) {
        let fuente = &self.fuente;
        self.glifos.entry((c, tam)).or_insert_with(|| fuente.rasterize(c, tam as f32))
    }

    /// Si la fuente tiene la letra (si no, sale su caja vacía).
    pub fn tiene(&self, c: char) -> bool {
        self.fuente.lookup_glyph_index(c) != 0
    }

    /// Lo que mide `texto` en una línea, a `tam` píxeles.
    pub fn medir(&mut self, texto: &str, tam: u32) -> Medidas {
        let linea = self.fuente.horizontal_line_metrics(tam as f32);
        let (sube, baja) = match linea {
            Some(l) => (libm::ceilf(l.ascent) as i32, libm::ceilf(-l.descent) as i32),
            None => (tam as i32, tam as i32 / 4),
        };
        let mut x = 0.0f32;
        let mut antes = None;
        for c in texto.chars() {
            if let Some(a) = antes {
                x += self.fuente.horizontal_kern(a, c, tam as f32).unwrap_or(0.0);
            }
            x += self.glifo(c, tam).0.advance_width;
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
                pluma += self.fuente.horizontal_kern(a, c, tam as f32).unwrap_or(0.0);
            }
            let (m, bits) = self.glifo(c, tam).clone();
            // La caja del glifo: `ymin` es lo que baja por debajo de la línea base.
            let x0 = libm::roundf(pluma) as i32 + m.xmin;
            let y0 = base - m.height as i32 - m.ymin;
            for gy in 0..m.height {
                for gx in 0..m.width {
                    let cob = bits[gy * m.width + gx] as u32;
                    if cob > 0 {
                        l.punto(x0 + gx as i32, y0 + gy as i32, color, cob);
                    }
                }
            }
            pluma += m.advance_width;
            antes = Some(c);
        }
    }
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
