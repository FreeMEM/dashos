//! Una imagen en memoria (C3 de S10c: el fondo de escritorio) y cómo se escala a la pantalla.
//!
//! Los píxeles van en ARGB sin premultiplicar (`0xAARRGGBB`), como los da `png::leer`. Escalar la deja opaca: lo que
//! tiene transparencia se pone encima de un color de fondo. Todo con enteros (DashOS no tiene FPU).

use crate::lienzo::Color;
use alloc::vec::Vec;

pub struct Imagen {
    pub ancho: usize,
    pub alto: usize,
    pub px: Vec<u32>,
}

/// `c` (sin premultiplicar) encima de `fondo` (opaco).
pub fn sobre(c: u32, fondo: Color) -> u32 {
    let a = c >> 24;
    if a == 255 {
        return c;
    }
    let canal = |s: u32| ((((c >> s) & 0xff) * a + ((fondo >> s) & 0xff) * (255 - a) + 127) / 255) << s;
    0xff00_0000 | canal(16) | canal(8) | canal(0)
}

/// Lo que se ve de la imagen al cubrir `ancho` × `alto` (se recorta lo que sobra, lo mismo a cada lado), en
/// coordenadas de la imagen con 16 bits de fracción: origen x, origen y, paso x y paso y por píxel de destino.
pub fn encuadre(ancho_img: usize, alto_img: usize, ancho: usize, alto: usize) -> (i64, i64, i64, i64) {
    let (sw, sh, w, h) = (ancho_img as i64, alto_img as i64, ancho as i64, alto as i64);
    let (vw, vh) = if w * sh >= h * sw { (sw << 16, (sw << 16) * h / w) } else { ((sh << 16) * w / h, sh << 16) };
    (((sw << 16) - vw) / 2, ((sh << 16) - vh) / 2, vw / w, vh / h)
}

impl Imagen {
    /// Reducida `k` veces por cada lado: cada píxel es la media de un cuadro de k × k (los del borde, de lo que haya),
    /// ya encima de `fondo`.
    pub fn reducir(&self, k: usize, fondo: Color) -> Imagen {
        let k = k.max(1);
        let (w, h) = (self.ancho.div_ceil(k), self.alto.div_ceil(k));
        let mut px = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
                for yy in y * k..((y + 1) * k).min(self.alto) {
                    for xx in x * k..((x + 1) * k).min(self.ancho) {
                        let c = sobre(self.px[yy * self.ancho + xx], fondo);
                        r += (c >> 16) & 0xff;
                        g += (c >> 8) & 0xff;
                        b += c & 0xff;
                        n += 1;
                    }
                }
                px.push(0xff00_0000 | ((r + n / 2) / n) << 16 | ((g + n / 2) / n) << 8 | (b + n / 2) / n);
            }
        }
        Imagen { ancho: w, alto: h, px }
    }

    /// Escalada para cubrir `ancho` × `alto` (como un fondo de escritorio: sin bandas, recortando lo que sobra), en
    /// `destino` (ancho × alto píxeles, opacos). Bilineal; si encoge a menos de la mitad, antes se reduce por cuadros,
    /// para que no salga granulada.
    pub fn cubrir(&self, ancho: usize, alto: usize, fondo: Color, destino: &mut [u32]) {
        if ancho == 0 || alto == 0 || self.ancho == 0 || self.alto == 0 {
            return;
        }
        let (_, _, px, py) = encuadre(self.ancho, self.alto, ancho, alto);
        let k = (px.min(py) >> 16) as usize;
        if k >= 2 {
            return self.reducir(k, fondo).cubrir(ancho, alto, fondo, destino);
        }
        let (ox, oy, px, py) = encuadre(self.ancho, self.alto, ancho, alto);
        let (sw, sh) = (self.ancho as i64, self.alto as i64);
        // Para cada columna, sus dos de la imagen y cuánto de la segunda (de 256).
        let columnas: Vec<(usize, usize, u32)> = (0..ancho as i64)
            .map(|x| {
                let s = (ox + x * px + px / 2 - 32768).clamp(0, (sw - 1) << 16);
                let x0 = (s >> 16) as usize;
                (x0, (x0 + 1).min(sw as usize - 1), ((s >> 8) & 0xff) as u32)
            })
            .collect();
        let en = |x: usize, y: usize| sobre(self.px[y * self.ancho + x], fondo);
        for y in 0..alto {
            let s = (oy + y as i64 * py + py / 2 - 32768).clamp(0, (sh - 1) << 16);
            let y0 = (s >> 16) as usize;
            let y1 = (y0 + 1).min(sh as usize - 1);
            let fy = ((s >> 8) & 0xff) as u32;
            let fila = &mut destino[y * ancho..(y + 1) * ancho];
            for (d, &(x0, x1, fx)) in fila.iter_mut().zip(&columnas) {
                let (a, b, c, e) = (en(x0, y0), en(x1, y0), en(x0, y1), en(x1, y1));
                let canal = |s: u32| {
                    let v = |p: u32| (p >> s) & 0xff;
                    let arriba = v(a) * (256 - fx) + v(b) * fx;
                    let abajo = v(c) * (256 - fx) + v(e) * fx;
                    ((arriba * (256 - fy) + abajo * fy + 32768) >> 16) << s
                };
                *d = 0xff00_0000 | canal(16) | canal(8) | canal(0);
            }
        }
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn lisa(w: usize, h: usize, c: u32) -> Imagen {
        Imagen { ancho: w, alto: h, px: alloc::vec![c; w * h] }
    }

    #[test]
    fn cubre_sin_bandas() {
        // Más ancha que la pantalla: se recorta a los lados; más alta: arriba y abajo. Lisa, sale igual.
        for (w, h) in [(960, 600), (300, 900), (13, 7), (1, 1), (4000, 3000)] {
            let img = lisa(w, h, 0xff12_3456);
            let mut d = alloc::vec![0u32; 80 * 60];
            img.cubrir(80, 60, 0xff00_0000, &mut d);
            assert!(d.iter().all(|&p| p == 0xff12_3456), "{w}x{h}");
        }
    }

    #[test]
    fn recorta_por_igual_y_es_suave() {
        // Un degradado horizontal de 0 a 255 en 256 columnas (64 de alto), en una pantalla de 64 × 128: se escala al
        // doble para cubrir el alto, y del ancho se ven las 32 columnas del centro (de la 112 a la 143), sin saltos.
        let img = Imagen { ancho: 256, alto: 64, px: (0..256 * 64).map(|i| 0xff00_0000 | (i % 256) as u32).collect() };
        let mut d = alloc::vec![0u32; 64 * 128];
        img.cubrir(64, 128, 0, &mut d);
        let azul: Vec<u32> = (0..64).map(|x| d[x] & 0xff).collect();
        assert_eq!((azul[0], azul[63]), (112, 143), "{azul:?}");
        assert!(azul.windows(2).all(|p| p[1] >= p[0] && p[1] - p[0] <= 1), "{azul:?}");
    }

    #[test]
    fn transparente_sobre_el_fondo() {
        let img = lisa(4, 4, 0x80ff_0000);
        let mut d = alloc::vec![0u32; 4];
        img.cubrir(2, 2, 0xff00_00ff, &mut d);
        assert_eq!(d[0], 0xff80_007f);
        // Reducir mucho: la media de los cuadros.
        let mut tablero = lisa(64, 64, 0);
        for (i, p) in tablero.px.iter_mut().enumerate() {
            *p = if (i % 64 + i / 64) % 2 == 0 { 0xffff_ffff } else { 0xff00_0000 };
        }
        let mut d = alloc::vec![0u32; 8 * 8];
        tablero.cubrir(8, 8, 0, &mut d);
        assert!(d.iter().all(|&p| (0xff7f_7f7f..=0xff80_8080).contains(&p)), "{:08x}", d[0]);
    }
}
