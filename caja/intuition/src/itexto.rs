//! intuition.library: DrawImage, DrawBorder y PrintIText, las imágenes, bordes y textos "a mano" de intuition
//! (`struct Image`, `struct Border`, `struct IntuiText`; RKRM: Libraries, "Intuition Images, Line Drawing and Text").
//! Lo comprobado contra el oráculo (`pruebas68k/imagen.c` en FS-UAE con el Kickstart):
//!
//! - DrawImage copia: cada punto es la pluma que sale de sus planos, sin mirar el modo ni las plumas del RastPort.
//!   El plano `p` del RastPort sale del plano siguiente de `ImageData` si `PlanePick` tiene el bit `p`; si no, del bit
//!   `p` de `PlaneOnOff`. Sin datos (`PlanePick` 0), un rectángulo de una pluma. Sigue `NextImage`.
//! - DrawBorder: cada borde, una línea por sus `Count` puntos con su pluma y su modo. El RastPort queda como estaba,
//!   también su posición.
//! - PrintIText: cada texto con sus plumas, su modo y su fuente (o la del RastPort), con la línea base en
//!   `TopEdge` más la de la fuente. Las plumas, el modo y la fuente vuelven a ser los de antes; la posición queda donde
//!   acabó el último texto.

use alloc::vec;
use alloc::vec::Vec;

use caja_exec::{Exec, Memoria};

use crate::acceso::Acceso;
use crate::dibujo::Tinta;
use crate::estructuras::{rastport as rp_, textfont as tf};
use crate::{CajaGrafica, Compositor, Fallo};

/// Cuántos elementos de una cadena (`NextImage`...) se siguen como mucho: una cadena en bucle no cuelga la caja.
const MAX_CADENA: usize = 4096;

/// `struct Image`.
mod image {
    pub const LEFTEDGE: u32 = 0;
    pub const TOPEDGE: u32 = 2;
    pub const WIDTH: u32 = 4;
    pub const HEIGHT: u32 = 6;
    pub const DEPTH: u32 = 8;
    pub const IMAGEDATA: u32 = 10;
    pub const PLANEPICK: u32 = 14;
    pub const PLANEONOFF: u32 = 15;
    pub const NEXTIMAGE: u32 = 16;
    pub const TAMANO: u32 = 20;
}

/// `struct Border`.
mod border {
    pub const LEFTEDGE: u32 = 0;
    pub const TOPEDGE: u32 = 2;
    pub const FRONTPEN: u32 = 4;
    pub const BACKPEN: u32 = 5;
    pub const DRAWMODE: u32 = 6;
    pub const COUNT: u32 = 7;
    pub const XY: u32 = 8;
    pub const NEXTBORDER: u32 = 12;
    pub const TAMANO: u32 = 16;
}

/// `struct IntuiText`.
mod intuitext {
    pub const FRONTPEN: u32 = 0;
    pub const BACKPEN: u32 = 1;
    pub const DRAWMODE: u32 = 2;
    pub const LEFTEDGE: u32 = 4;
    pub const TOPEDGE: u32 = 6;
    pub const ITEXTFONT: u32 = 8;
    pub const ITEXT: u32 = 12;
    pub const NEXTTEXT: u32 = 16;
    pub const TAMANO: u32 = 20;
}

/// Lo del RastPort que DrawBorder y PrintIText cambian y luego dejan como estaba.
struct Guardado {
    fg: u8,
    bg: u8,
    modo: u8,
    x: u16,
    y: u16,
    flags: u16,
    cnt: u8,
}

impl<C: Compositor> CajaGrafica<C> {
    fn guardar_rp<M: Memoria>(&self, ex: &Exec<M>, rp: u32) -> Result<Guardado, Fallo> {
        let i = self.leer_rp(ex, rp)?;
        Ok(Guardado { fg: i.fg, bg: i.bg, modo: i.modo, x: i.x as u16, y: i.y as u16, flags: i.flags, cnt: i.cnt })
    }

    fn poner_plumas<M: Memoria>(ex: &mut Exec<M>, rp: u32, fg: u8, bg: u8, modo: u8) -> Result<(), Fallo> {
        ex.e8(rp + rp_::FGPEN, fg)?;
        ex.e8(rp + rp_::BGPEN, bg)?;
        ex.e8(rp + rp_::DRAWMODE, modo)
    }

    /// DrawImage(rp, image, left, top).
    pub(crate) fn draw_image<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, im: u32, x: i32, y: i32) -> Result<(), Fallo> {
        let i = self.leer_rp(ex, rp)?;
        let cm = self.cm_de(i.bm);
        let mut im = im;
        for _ in 0..MAX_CADENA {
            if im == 0 {
                return Ok(());
            }
            ex.tramo(im, image::TAMANO)?;
            let x0 = x + ex.l16(im + image::LEFTEDGE)? as i16 as i32;
            let y0 = y + ex.l16(im + image::TOPEDGE)? as i16 as i32;
            let ancho = (ex.l16(im + image::WIDTH)? as i16).max(0) as u32;
            let alto = (ex.l16(im + image::HEIGHT)? as i16).max(0) as u32;
            let hondo = (ex.l16(im + image::DEPTH)? as i16).max(0) as u32;
            let datos = ex.l32(im + image::IMAGEDATA)?;
            let pick = ex.l8(im + image::PLANEPICK)?;
            let onoff = ex.l8(im + image::PLANEONOFF)?;
            let siguiente = ex.l32(im + image::NEXTIMAGE)?;
            // Los planos que de verdad se leen: los de PlanePick, hasta Depth.
            let usados = (pick.count_ones()).min(hondo);
            let palabras = ancho.div_ceil(16);
            let plano = palabras * 2 * alto;
            let bits = if usados == 0 || datos == 0 || plano == 0 { Vec::new() } else { ex.bytes(datos, plano * usados)? };
            let mut plumas = vec![0u8; (ancho * alto) as usize];
            for fy in 0..alto {
                for fx in 0..ancho {
                    let mut pen = 0u8;
                    let mut k = 0;
                    for p in 0..8 {
                        let b = if pick & (1 << p) != 0 && k < usados && !bits.is_empty() {
                            let o = (k * plano + fy * palabras * 2 + fx / 8) as usize;
                            k += 1;
                            bits[o] >> (7 - fx % 8) & 1
                        } else {
                            onoff >> p & 1
                        };
                        pen |= b << p;
                    }
                    plumas[(fy * ancho + fx) as usize] = pen;
                }
            }
            let mut colores = [None; 256];
            for &p in &plumas {
                if colores[p as usize].is_none() {
                    colores[p as usize] = Some(self.color(cm, p));
                }
            }
            {
                let mut pi = self.pintor(ex, &i)?;
                for fy in 0..alto {
                    for fx in 0..ancho {
                        let pen = plumas[(fy * ancho + fx) as usize];
                        let argb = colores[pen as usize].unwrap_or(0);
                        pi.tinta(x0 + fx as i32, y0 + fy as i32, Tinta { pen, argb });
                    }
                }
            }
            im = siguiente;
        }
        Ok(())
    }

    /// DrawBorder(rp, border, left, top).
    pub(crate) fn draw_border<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, b: u32, x: i32, y: i32) -> Result<(), Fallo> {
        let g = self.guardar_rp(ex, rp)?;
        let r = self.bordes(ex, rp, b, x, y);
        Self::poner_plumas(ex, rp, g.fg, g.bg, g.modo)?;
        ex.e16(rp + rp_::CP_X, g.x)?;
        ex.e16(rp + rp_::CP_Y, g.y)?;
        ex.e16(rp + rp_::FLAGS, g.flags)?;
        ex.e8(rp + rp_::LINPATCNT, g.cnt)?;
        r
    }

    fn bordes<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, b: u32, x: i32, y: i32) -> Result<(), Fallo> {
        let mut b = b;
        for _ in 0..MAX_CADENA {
            if b == 0 {
                return Ok(());
            }
            ex.tramo(b, border::TAMANO)?;
            let x0 = x + ex.l16(b + border::LEFTEDGE)? as i16 as i32;
            let y0 = y + ex.l16(b + border::TOPEDGE)? as i16 as i32;
            let n = (ex.l8(b + border::COUNT)? as i8).max(0) as u32;
            let xy = ex.l32(b + border::XY)?;
            Self::poner_plumas(
                ex,
                rp,
                ex.l8(b + border::FRONTPEN)?,
                ex.l8(b + border::BACKPEN)?,
                ex.l8(b + border::DRAWMODE)?,
            )?;
            if n > 0 {
                let puntos = ex.bytes(xy, n * 4)?;
                for (k, c) in puntos.chunks(4).enumerate() {
                    let px = x0 + i16::from_be_bytes([c[0], c[1]]) as i32;
                    let py = y0 + i16::from_be_bytes([c[2], c[3]]) as i32;
                    if k == 0 {
                        self.mover(ex, rp, px, py)?;
                    } else {
                        self.draw(ex, rp, px, py)?;
                    }
                }
            }
            b = ex.l32(b + border::NEXTBORDER)?;
        }
        Ok(())
    }

    /// PrintIText(rp, itext, left, top).
    pub(crate) fn print_i_text<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, t: u32, x: i32, y: i32) -> Result<(), Fallo> {
        let g = self.guardar_rp(ex, rp)?;
        let fuente = self.leer_rp(ex, rp)?.fuente;
        let r = self.textos(ex, rp, t, x, y, fuente);
        Self::poner_plumas(ex, rp, g.fg, g.bg, g.modo)?;
        r
    }

    fn textos<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, t: u32, x: i32, y: i32, fuente: u32) -> Result<(), Fallo> {
        let mut t = t;
        for _ in 0..MAX_CADENA {
            if t == 0 {
                return Ok(());
            }
            ex.tramo(t, intuitext::TAMANO)?;
            let ta = ex.l32(t + intuitext::ITEXTFONT)?;
            // Su fuente, si la lleva y se puede abrir; si no, la del RastPort.
            let propia = if ta != 0 { self.open_font(ex, ta).unwrap_or(0) } else { 0 };
            if propia != 0 {
                self.set_font(ex, rp, propia)?;
            }
            let f = if propia != 0 { propia } else { fuente };
            let base = if f != 0 { ex.l16(f + tf::TF_BASELINE)? as i16 as i32 } else { 6 };
            Self::poner_plumas(
                ex,
                rp,
                ex.l8(t + intuitext::FRONTPEN)?,
                ex.l8(t + intuitext::BACKPEN)?,
                ex.l8(t + intuitext::DRAWMODE)?,
            )?;
            let px = x + ex.l16(t + intuitext::LEFTEDGE)? as i16 as i32;
            let py = y + ex.l16(t + intuitext::TOPEDGE)? as i16 as i32 + base;
            self.mover(ex, rp, px, py)?;
            let s = ex.l32(t + intuitext::ITEXT)?;
            let r = if s != 0 {
                let n = ex.cadena(s)?.len() as u32;
                self.text(ex, rp, s, n)
            } else {
                Ok(())
            };
            if propia != 0 {
                if fuente != 0 {
                    self.set_font(ex, rp, fuente)?;
                }
                self.close_font(ex, propia)?;
            }
            r?;
            t = ex.l32(t + intuitext::NEXTTEXT)?;
        }
        Ok(())
    }
}
