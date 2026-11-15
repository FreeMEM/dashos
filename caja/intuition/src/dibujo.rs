//! Dibujar en un mapa: nativo (ARGB, en un `Lienzo`) o planar (los planos de bits de un `struct BitMap` en la
//! memoria del 68k, como los ve el chipset). El `Pintor` junta el mapa, el recorte y las plumas ya resueltas, y
//! sabe poner un punto según el modo de dibujo (`JAM1`, `JAM2`, `COMPLEMENT`, `INVERSVID`). Encima van las
//! líneas (Bresenham), los rectángulos y las plantillas de un bit (texto y `BltTemplate`).

use alloc::vec::Vec;
use caja_exec::Memoria;

use crate::estructuras::drmd;
use crate::lienzo::{Lienzo, Rect};

/// Un mapa planar: hasta 8 planos de `bpr` bytes por fila. Un plano 0 se lee como ceros y uno `0xFFFFFFFF` como
/// unos; en ninguno de los dos se escribe (es lo que hace el blitter con ellos).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Planar {
    pub planos: [u32; 8],
    pub prof: u8,
    pub bpr: u32,
    pub filas: u32,
    pub ancho: u32,
}

pub(crate) const PLANO_UNOS: u32 = 0xFFFF_FFFF;

impl Planar {
    pub fn rect(&self) -> Rect {
        Rect::de_tamano(self.ancho, self.filas)
    }

    fn sitio(&self, i: usize, x: i32, y: i32) -> Option<(u32, u8)> {
        let p = self.planos[i];
        if p == 0 || p == PLANO_UNOS {
            return None;
        }
        Some((p + y as u32 * self.bpr + (x as u32 >> 3), 0x80u8 >> (x as u32 & 7)))
    }

    /// La pluma de `(x, y)` (sin comprobar los límites).
    pub fn leer<M: Memoria>(&self, mem: &M, x: i32, y: i32) -> u8 {
        let mut v = 0u8;
        for i in 0..self.prof as usize {
            let bit = match self.planos[i] {
                0 => false,
                PLANO_UNOS => true,
                _ => match self.sitio(i, x, y) {
                    Some((d, m)) => mem.leer8(d).map(|b| b & m != 0).unwrap_or(false),
                    None => false,
                },
            };
            if bit {
                v |= 1 << i;
            }
        }
        v
    }

    /// Escribe la pluma `v` en los planos de `mascara` (sin comprobar los límites).
    pub fn escribir<M: Memoria>(&self, mem: &mut M, x: i32, y: i32, v: u8, mascara: u8) {
        for i in 0..self.prof as usize {
            if mascara & (1 << i) == 0 {
                continue;
            }
            if let Some((d, m)) = self.sitio(i, x, y)
                && let Ok(b) = mem.leer_mut(d, 1)
            {
                if v & (1 << i) != 0 {
                    b[0] |= m;
                } else {
                    b[0] &= !m;
                }
            }
        }
    }
}

/// Dónde se dibuja.
pub(crate) enum Sup<'a, M: Memoria> {
    Nativa(&'a mut Lienzo),
    Planar(&'a mut M, Planar),
}

/// Una pluma y su color en el mapa (el color solo cuenta en los nativos).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Tinta {
    pub pen: u8,
    pub argb: u32,
}

pub(crate) struct Pintor<'a, M: Memoria> {
    pub sup: Sup<'a, M>,
    /// Lo que se puede tocar: el mapa y la capa de la ventana.
    pub clip: Rect,
    /// La región de recorte de la capa (`InstallClipRegion`), en bandas y en las coordenadas del pintor: solo se toca
    /// lo que cae dentro de alguno de sus rectángulos (y de `clip`, que ya es su caja).
    pub region: Option<Vec<Rect>>,
    pub a: Tinta,
    pub b: Tinta,
    pub modo: u8,
    /// `rp_Mask`: los planos que se escriben (solo en los planares).
    pub mascara: u8,
    /// Dónde cae el (0, 0) del `RastPort` en su mapa: el de la capa interior de una ventana `WFLG_GIMMEZEROZERO`
    /// es la esquina de dentro de los bordes. Las coordenadas del pintor (y `clip`) son las del `RastPort`.
    pub dx: i32,
    pub dy: i32,
}

impl<M: Memoria> Pintor<'_, M> {
    pub fn es_nativa(&self) -> bool {
        matches!(self.sup, Sup::Nativa(_))
    }

    /// Los límites del mapa (sin el recorte de la capa), en coordenadas del `RastPort`.
    pub fn limites(&self) -> Rect {
        let r = match &self.sup {
            Sup::Nativa(l) => l.rect(),
            Sup::Planar(_, p) => p.rect(),
        };
        Rect::nuevo(r.x0 - self.dx, r.y0 - self.dy, r.x1 - self.dx, r.y1 - self.dy)
    }

    /// Pone el (0, 0) del `RastPort` en `(dx, dy)` del mapa (el recorte pasa a coordenadas del `RastPort`).
    pub fn trasladar(&mut self, dx: i32, dy: i32) {
        let c = self.clip;
        self.clip = Rect::nuevo(c.x0 - dx, c.y0 - dy, c.x1 - dx, c.y1 - dy);
        if let Some(r) = &mut self.region {
            for q in r.iter_mut() {
                *q = Rect::nuevo(q.x0 - dx, q.y0 - dy, q.x1 - dx, q.y1 - dy);
            }
        }
        self.dx = dx;
        self.dy = dy;
    }

    /// El valor crudo de un punto: el color ARGB en los nativos, la pluma en los planares. 0 fuera del mapa.
    pub fn leer(&self, x: i32, y: i32) -> u32 {
        if !self.limites().contiene(x, y) {
            return 0;
        }
        let (x, y) = (x + self.dx, y + self.dy);
        match &self.sup {
            Sup::Nativa(l) => l.pixel(x, y),
            Sup::Planar(mem, p) => p.leer(&**mem, x, y) as u32,
        }
    }

    /// Escribe un valor crudo (ARGB o pluma), sin modo de dibujo, si cae dentro del recorte.
    /// Pone una región de recorte (rectángulos en bandas, en las coordenadas del pintor): `clip` pasa a ser también su
    /// caja.
    pub fn recortar_con(&mut self, region: Vec<Rect>) {
        let caja = region.iter().fold(Rect::nuevo(0, 0, -1, -1), |u, r| u.union(r));
        self.clip = self.clip.interseccion(&caja);
        self.region = Some(region);
    }

    /// Si `(x, y)` cae en la región de recorte (o no hay): la banda por búsqueda binaria y, dentro, el rectángulo.
    fn en_region(&self, x: i32, y: i32) -> bool {
        let Some(r) = &self.region else { return true };
        // La primera banda que acaba en `y` o más abajo.
        let i = r.partition_point(|q| q.y1 < y);
        let Some(b) = r.get(i) else { return false };
        if b.y0 > y {
            return false;
        }
        let fin = i + r[i..].partition_point(|q| q.y0 == b.y0);
        let banda = &r[i..fin];
        let k = banda.partition_point(|q| q.x1 < x);
        banda.get(k).is_some_and(|q| q.x0 <= x)
    }

    pub fn crudo(&mut self, x: i32, y: i32, v: u32) {
        if !self.clip.contiene(x, y) || !self.en_region(x, y) {
            return;
        }
        let (x, y) = (x + self.dx, y + self.dy);
        let mascara = self.mascara;
        match &mut self.sup {
            Sup::Nativa(l) => l.poner(x, y, v),
            Sup::Planar(mem, p) => p.escribir(&mut **mem, x, y, v as u8, mascara),
        }
    }

    pub fn tinta(&mut self, x: i32, y: i32, t: Tinta) {
        let v = if self.es_nativa() { t.argb } else { t.pen as u32 };
        self.crudo(x, y, v);
    }

    /// Invierte un punto: en los planares, los bits de los planos de la máscara; en los nativos, el color (sin
    /// tocar el alfa). Dos veces deja lo que había.
    pub fn complementar(&mut self, x: i32, y: i32) {
        if !self.clip.contiene(x, y) || !self.en_region(x, y) {
            return;
        }
        let (x, y) = (x + self.dx, y + self.dy);
        let mascara = self.mascara;
        match &mut self.sup {
            Sup::Nativa(l) => {
                let v = l.pixel(x, y);
                l.poner(x, y, v ^ 0x00FF_FFFF);
            }
            Sup::Planar(mem, p) => {
                let v = p.leer(&**mem, x, y);
                p.escribir(&mut **mem, x, y, v ^ mascara, mascara);
            }
        }
    }

    /// Un punto de una figura: `delante` si es del dibujo (un 1 de la plantilla o del patrón), detrás si no.
    pub fn punto(&mut self, x: i32, y: i32, delante: bool) {
        let delante = if self.modo & drmd::INVERSVID != 0 { !delante } else { delante };
        if self.modo & drmd::COMPLEMENT != 0 {
            if delante {
                self.complementar(x, y);
            }
            return;
        }
        if delante {
            let a = self.a;
            self.tinta(x, y, a);
        } else if self.modo & drmd::JAM2 != 0 {
            let b = self.b;
            self.tinta(x, y, b);
        }
    }

    /// Una línea de `(x0, y0)` a `(x1, y1)`, los dos extremos incluidos (Bresenham, en los ocho octantes). Cada
    /// punto toma un bit de `patron`, del 15 hacia abajo, empezando en `*cnt`. Con `sin_primero`, el primer punto
    /// no se pinta (pero gasta su bit).
    #[allow(clippy::too_many_arguments)]
    pub fn linea(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, patron: u16, cnt: &mut u8, sin_primero: bool) {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let (mut x, mut y, mut err) = (x0, y0, dx + dy);
        let mut primero = true;
        loop {
            let bit = (patron >> (*cnt & 15)) & 1 != 0;
            *cnt = (*cnt + 15) & 15;
            if !(primero && sin_primero) {
                self.punto(x, y, bit);
            }
            primero = false;
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Un rectángulo lleno (extremos incluidos), con el patrón de área si lo hay: una fila de 16 bits por línea,
    /// repetidas cada `patron.len()` líneas, ancladas al origen del mapa.
    pub fn rectangulo(&mut self, r: Rect, patron: &[u16]) {
        let r = r.interseccion(&self.clip);
        if r.vacio() {
            return;
        }
        for y in r.y0..=r.y1 {
            let fila = if patron.is_empty() { 0xFFFF } else { patron[y as usize % patron.len()] };
            for x in r.x0..=r.x1 {
                self.punto(x, y, fila & (0x8000 >> (x & 15)) != 0);
            }
        }
    }

    /// Una plantilla de un bit de `ancho` x `alto` en `(x, y)`: `bit(col, fila)` dice si el punto es del dibujo.
    pub fn plantilla(&mut self, x: i32, y: i32, ancho: u32, alto: u32, bit: impl Fn(u32, u32) -> bool) {
        let r = Rect::nuevo(x, y, x + ancho as i32 - 1, y + alto as i32 - 1).interseccion(&self.clip);
        if r.vacio() {
            return;
        }
        for py in r.y0..=r.y1 {
            for px in r.x0..=r.x1 {
                self.punto(px, py, bit((px - x) as u32, (py - y) as u32));
            }
        }
    }
}

/// La pluma de `paleta` más parecida a un color (distancia en RGB).
pub(crate) fn mas_cercano(paleta: &[u32], argb: u32) -> u8 {
    let (r, g, b) = ((argb >> 16) & 0xFF, (argb >> 8) & 0xFF, argb & 0xFF);
    let mut mejor = (u32::MAX, 0u8);
    for (i, &c) in paleta.iter().enumerate().take(256) {
        let (cr, cg, cb) = ((c >> 16) & 0xFF, (c >> 8) & 0xFF, c & 0xFF);
        let d = r.abs_diff(cr).pow(2) + g.abs_diff(cg).pow(2) + b.abs_diff(cb).pow(2);
        if d < mejor.0 {
            mejor = (d, i as u8);
            if d == 0 {
                break;
            }
        }
    }
    mejor.1
}
