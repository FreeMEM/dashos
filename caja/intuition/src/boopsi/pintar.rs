//! Cómo se ven los gadgets y las imágenes nativas de BOOPSI y de ReAction: por el mismo rasgo `Tema` que GadTools
//! (`tema.rs`), en el lienzo de su ventana. Un botón de ReAction es un botón de GadTools pintado en su rectángulo:
//! las dos apariencias (clásica y "como DashOS") y la letra nítida salen de ahí sin nada propio.

use alloc::vec::Vec;

use caja_exec::{Exec, Memoria};

use crate::gadtools::Gt;
use crate::lienzo::Rect;
use crate::nitido::{Alinea, Estilo};
use crate::tema::{Elegido, Vista, bisel, llenar};
use crate::{CajaGrafica, Compositor};

/// Lo que se pinta como un gadget de GadTools.
pub(crate) struct Control<'a> {
    pub s: &'a Gt,
    pub desactivado: bool,
    pub elementos: &'a [Vec<u8>],
    pub cadena: &'a [u8],
    pub cursor: usize,
    pub activo_texto: bool,
}

impl<C: Compositor> CajaGrafica<C> {
    /// Pinta un control en el rectángulo `r` de la ventana `w` con su tema. Devuelve lo que tocó.
    pub(crate) fn pintar_control<M: Memoria>(&mut self, ex: &Exec<M>, w: u32, r: Rect, c: &Control) -> Rect {
        let Some(bm) = self.intui.ventanas.get(&w).map(|v| v.bm) else { return Rect::nuevo(0, 0, -1, -1) };
        let plumas = self.plumas(ex);
        let tema = self.tema_de(w);
        let glifos = &self.intui.glifos;
        let Some(l) = self.gfx.lienzos.get_mut(&bm) else { return Rect::nuevo(0, 0, -1, -1) };
        let vista = Vista {
            s: c.s,
            r,
            desactivado: c.desactivado,
            paleta: &[],
            elementos: c.elementos,
            cadena: c.cadena,
            cursor: c.cursor,
            activo_texto: c.activo_texto,
        };
        let dano = tema.como().gadget(l, glifos, &plumas, &vista);
        l.danar(dano.interseccion(&l.rect()));
        dano
    }

    /// Rellena `r` de la ventana con su color de dentro.
    pub(crate) fn fondo_en(&mut self, w: u32, r: Rect) {
        let Some(v) = self.intui.ventanas.get(&w) else { return };
        let (bm, f) = (v.bm, v.fondo);
        if let Some(l) = self.gfx.lienzos.get_mut(&bm) {
            let r = r.interseccion(&l.rect());
            if !r.vacio() {
                llenar(l, r, f);
                l.danar(r);
            }
        }
    }

    /// Un marco (de grupo, de bisel) en la ventana, con su tema: en relieve (hundido o en cresta) en el clásico, una
    /// tarjeta con contorno en el de DashOS.
    pub(crate) fn marco_en<M: Memoria>(&mut self, ex: &Exec<M>, w: u32, r: Rect, hundido: bool, cresta: bool) {
        let Some(bm) = self.intui.ventanas.get(&w).map(|v| v.bm) else { return };
        if r.x1 - r.x0 < 2 || r.y1 - r.y0 < 2 {
            return;
        }
        let p = self.plumas(ex);
        let tema = self.tema_de(w);
        let Some(l) = self.gfx.lienzos.get_mut(&bm) else { return };
        if !tema.como().bisel_caja(l, r, hundido, cresta) {
            bisel(l, r, p.claro, p.oscuro, hundido);
            if cresta {
                bisel(l, Rect::nuevo(r.x0 + 2, r.y0 + 1, r.x1 - 2, r.y1 - 1), p.claro, p.oscuro, !hundido);
            }
        }
        l.danar(r.interseccion(&l.rect()));
    }

    /// Una raya separadora (horizontal si es más ancha que alta).
    pub(crate) fn raya_en<M: Memoria>(&mut self, ex: &Exec<M>, w: u32, r: Rect) {
        let Some(bm) = self.intui.ventanas.get(&w).map(|v| v.bm) else { return };
        let p = self.plumas(ex);
        let material = matches!(self.tema_de(w), Elegido::Material(_));
        let linea = match self.tema_de(w) {
            Elegido::Material(m) => m.t.variante_contorno,
            _ => p.oscuro,
        };
        let Some(l) = self.gfx.lienzos.get_mut(&bm) else { return };
        let horizontal = r.x1 - r.x0 >= r.y1 - r.y0;
        if horizontal {
            let y = (r.y0 + r.y1) / 2;
            llenar(l, Rect::nuevo(r.x0, y, r.x1, y), linea);
            if !material {
                llenar(l, Rect::nuevo(r.x0, y + 1, r.x1, y + 1), p.claro);
            }
        } else {
            let x = (r.x0 + r.x1) / 2;
            llenar(l, Rect::nuevo(x, r.y0, x, r.y1), linea);
            if !material {
                llenar(l, Rect::nuevo(x + 1, r.y0, x + 1, r.y1), p.claro);
            }
        }
        l.danar(r.interseccion(&l.rect()));
    }

    /// Un texto en la ventana con la tinta del tema (nítido si va "como DashOS"), sin pasar de `hasta`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn texto_en<M: Memoria>(&mut self, ex: &Exec<M>, w: u32, x: i32, y: i32, s: &[u8], hasta: i32, alinea: Alinea, estilo: Estilo) {
        let Some(bm) = self.intui.ventanas.get(&w).map(|v| v.bm) else { return };
        let p = self.plumas(ex);
        let tinta = match self.tema_de(w) {
            Elegido::Material(m) => m.t.sobre_superficie,
            _ => p.texto,
        };
        let glifos = &self.intui.glifos;
        let Some(l) = self.gfx.lienzos.get_mut(&bm) else { return };
        let fin = crate::gadtools::texto_n(l, glifos, x, y, s, tinta, hasta, alinea, estilo);
        let r = Rect::nuevo(x, y, fin.max(x), y + crate::fuente::ALTO as i32);
        l.danar(r.interseccion(&l.rect()));
    }

    /// La ventana cuyo `RastPort` es `rp` (para pintar las imágenes en su lienzo con el tema).
    pub(crate) fn ventana_de_rp(&self, rp: u32) -> Option<u32> {
        self.intui.ventanas.iter().find(|(_, v)| v.rp == rp).map(|(w, _)| *w)
    }
}

/// Un texto sin el subrayado de las teclas ("_Aceptar" se ve "Aceptar").
pub(crate) fn sin_subrayado(t: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(t.len());
    let mut i = 0;
    while i < t.len() {
        if t[i] == b'_' && i + 1 < t.len() {
            i += 1;
        }
        v.push(t[i]);
        i += 1;
    }
    v
}
