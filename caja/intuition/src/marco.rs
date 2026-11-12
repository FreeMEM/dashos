//! Los adornos de Intuition dentro del búfer de la ventana: bordes, barra de título y los gadgets de cerrar,
//! profundidad y tamaño (provisional: dibujo propio con las plumas de la pantalla, al estilo de 3.1 pero no
//! copiado de nada). Y dónde cae cada gadget, para los clics.

use alloc::vec::Vec;

use crate::estructuras::{drawinfo as dri, wflg};
use crate::lienzo::{Lienzo, Rect};
use crate::{CajaGrafica, Compositor, fuente};

/// Anchos de los gadgets de la barra de título y tamaño del de tamaño (provisional, a comprobar con el oráculo).
pub(crate) const ANCHO_CERRAR: i32 = 20;
pub(crate) const ANCHO_PROF: i32 = 23;
pub(crate) const GADGET_TAMANO: (i32, i32) = (18, 10);

/// Los gadgets del sistema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Gadget {
    Cerrar,
    Profundidad,
    Tamano,
    Arrastre,
}

/// Lo que hace falta para pintar y para los clics.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Geometria {
    pub ancho: i32,
    pub alto: i32,
    pub flags: u32,
    /// Izquierda, arriba, derecha, abajo.
    pub bordes: [i32; 4],
}

impl Geometria {
    fn barra(&self) -> bool {
        self.bordes[1] > 2
    }

    /// El gadget del sistema que hay en `(x, y)`.
    pub fn zona(&self, x: i32, y: i32) -> Option<Gadget> {
        if x < 0 || y < 0 || x >= self.ancho || y >= self.alto || self.flags & wflg::BORDERLESS != 0 {
            return None;
        }
        if self.flags & wflg::SIZEGADGET != 0 && x >= self.ancho - GADGET_TAMANO.0 && y >= self.alto - GADGET_TAMANO.1 {
            return Some(Gadget::Tamano);
        }
        if !self.barra() || y >= self.bordes[1] {
            return None;
        }
        if self.flags & wflg::CLOSEGADGET != 0 && x < ANCHO_CERRAR {
            return Some(Gadget::Cerrar);
        }
        if self.flags & wflg::DEPTHGADGET != 0 && x >= self.ancho - ANCHO_PROF {
            return Some(Gadget::Profundidad);
        }
        if self.flags & wflg::DRAGBAR != 0 {
            return Some(Gadget::Arrastre);
        }
        None
    }
}

fn llenar(l: &mut Lienzo, r: Rect, c: u32) {
    let r = r.interseccion(&l.rect());
    for y in r.y0..=r.y1 {
        for x in r.x0..=r.x1 {
            l.poner(x, y, c);
        }
    }
}

/// Un marco en relieve: claro arriba y a la izquierda, oscuro abajo y a la derecha (al revés si `hundido`).
fn relieve(l: &mut Lienzo, r: Rect, claro: u32, oscuro: u32, hundido: bool) {
    let (a, b) = if hundido { (oscuro, claro) } else { (claro, oscuro) };
    llenar(l, Rect::nuevo(r.x0, r.y0, r.x1, r.y0), a);
    llenar(l, Rect::nuevo(r.x0, r.y0, r.x0, r.y1), a);
    llenar(l, Rect::nuevo(r.x0, r.y1, r.x1, r.y1), b);
    llenar(l, Rect::nuevo(r.x1, r.y0, r.x1, r.y1), b);
}

fn contorno(l: &mut Lienzo, r: Rect, c: u32) {
    relieve(l, r, c, c, false);
}

impl<C: Compositor> CajaGrafica<C> {
    /// Dibuja los adornos de una ventana en su lienzo.
    pub(crate) fn dibujar_marco(&mut self, w: u32) {
        let Some(v) = self.intui.ventanas.get(&w) else { return };
        let g = v.geometria;
        if g.flags & wflg::BORDERLESS != 0 && !g.barra() {
            return;
        }
        let activa = v.activa;
        let titulo: Vec<u8> = v.titulo.clone();
        let bm = v.bm;
        let pen = |i: usize| self.intui.wb.plumas[i] as u8;
        let cm = self.intui.wb.cm;
        let col = |i: usize| self.color(cm, pen(i));
        let (claro, oscuro) = (col(dri::SHINEPEN), col(dri::SHADOWPEN));
        let relleno = if activa { col(dri::FILLPEN) } else { col(dri::BACKGROUNDPEN) };
        let tinta = if activa { col(dri::FILLTEXTPEN) } else { col(dri::TEXTPEN) };
        let glifos = &self.intui.glifos;
        let Some(l) = self.gfx.lienzos.get_mut(&bm) else { return };
        let (an, al) = (g.ancho, g.alto);
        let [bi, ba, bd, bb] = g.bordes;
        // Las cuatro franjas del borde.
        llenar(l, Rect::nuevo(0, 0, an - 1, ba - 1), relleno);
        llenar(l, Rect::nuevo(0, al - bb, an - 1, al - 1), relleno);
        llenar(l, Rect::nuevo(0, 0, bi - 1, al - 1), relleno);
        llenar(l, Rect::nuevo(an - bd, 0, an - 1, al - 1), relleno);
        relieve(l, Rect::nuevo(0, 0, an - 1, al - 1), claro, oscuro, false);
        if bi > 0 && ba > 0 && bd > 0 && bb > 0 {
            relieve(l, Rect::nuevo(bi - 1, ba - 1, an - bd, al - bb), claro, oscuro, true);
        }
        if g.barra() {
            let mut x0 = 4;
            let mut x1 = an - 4;
            if g.flags & wflg::CLOSEGADGET != 0 {
                let r = Rect::nuevo(0, 0, ANCHO_CERRAR - 1, ba - 1);
                relieve(l, r, claro, oscuro, false);
                let (cx, cy) = (ANCHO_CERRAR / 2, ba / 2);
                llenar(l, Rect::nuevo(cx - 2, cy - 2, cx + 2, cy + 2), claro);
                contorno(l, Rect::nuevo(cx - 3, cy - 3, cx + 3, cy + 3), oscuro);
                x0 = ANCHO_CERRAR + 4;
            }
            if g.flags & wflg::DEPTHGADGET != 0 {
                let gx = an - ANCHO_PROF;
                relieve(l, Rect::nuevo(gx, 0, an - 1, ba - 1), claro, oscuro, false);
                contorno(l, Rect::nuevo(gx + 4, 2, gx + 13, ba - 5), oscuro);
                llenar(l, Rect::nuevo(gx + 9, 4, gx + 18, ba - 3), claro);
                contorno(l, Rect::nuevo(gx + 9, 4, gx + 18, ba - 3), oscuro);
                x1 = gx - 4;
            }
            // El título, con la fuente de la pantalla, cortado antes del gadget de profundidad.
            let arriba = ((ba - fuente::ALTO as i32) / 2).max(1);
            let mut x = x0;
            for &c in &titulo {
                if x + fuente::ANCHO as i32 > x1 {
                    break;
                }
                let gl = glifos.get(if (32..=255).contains(&c) { c as usize } else { 256 }).copied().unwrap_or([0; 8]);
                for (f, fila) in gl.iter().enumerate() {
                    for b in 0..8 {
                        if fila & (0x80 >> b) != 0 {
                            l.poner(x + b, arriba + f as i32, tinta);
                        }
                    }
                }
                x += fuente::ANCHO as i32;
            }
        }
        if g.flags & wflg::SIZEGADGET != 0 {
            let r = Rect::nuevo(an - GADGET_TAMANO.0, al - GADGET_TAMANO.1, an - 1, al - 1);
            llenar(l, r, relleno);
            relieve(l, r, claro, oscuro, false);
            // Una esquina: dos rectángulos, el pequeño dentro del grande.
            contorno(l, Rect::nuevo(r.x0 + 4, r.y0 + 2, r.x1 - 3, r.y1 - 2), oscuro);
            llenar(l, Rect::nuevo(r.x0 + 5, r.y0 + 3, r.x0 + 8, r.y0 + 4), claro);
        }
    }

    /// Los glifos de topaz por carácter (256 es la caja), para el título.
    pub(crate) fn tabla_glifos() -> Vec<[u8; 8]> {
        let todos = fuente::glifos();
        let caja = todos.iter().find(|(c, _)| *c == fuente::NO_HAY).map(|(_, g)| *g).unwrap_or([0; 8]);
        let mut v = alloc::vec![caja; 257];
        for (c, g) in todos {
            if (c as usize) < 256 {
                v[c as usize] = g;
            }
        }
        v
    }
}
