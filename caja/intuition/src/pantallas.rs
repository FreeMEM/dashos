//! Las pantallas propias: `OpenScreenTagList` con un modo de la caja (`modos.rs`) da una `struct Screen` nueva, del
//! tamaño y la profundidad del modo (o los que pida, `SA_Width`/`SA_Height`), con su `ColorMap` (la paleta del
//! Workbench de partida), su `DrawInfo` y su `BitMap`: chunky en la memoria del 68k en el formato del modo (15, 16, 24 y
//! 32 bits, y LUT8 en los de 8: una pluma por byte con su paleta, como en una tarjeta; `LockBitMap` da su dirección de
//! verdad y los mapas amigos salen en ese formato). Sus ventanas son ventanas del compositor como las del Workbench (su lienzo lleva el formato del modo). Se
//! pone delante en la lista de `IntuitionBase`. `CloseScreen` la suelta si no le quedan ventanas (si no, FALSE, como
//! en 3.x); si su tarea acaba sin cerrarla, se cierra.
//!
//! Sin `SA_DisplayID` de la caja (un modo del chipset o ninguno), `OpenScreen` da la pantalla de Workbench, como antes:
//! la caja no tiene chipset. Un `ModeID` del monitor de la caja que no está (más grande que el escritorio, una
//! profundidad que no hay) falla con `OSERR_UNKNOWNMODE`.

use alloc::vec;
use alloc::vec::Vec;

use caja_exec::estructuras::memf;
use caja_exec::{Exec, Memoria};

use crate::acceso::Acceso;
use crate::estructuras::{colormap, drawinfo as dri, intuitionbase as ib, rastport as rp_, screen as sc, viewport};
use crate::intuition::Pantalla;
use crate::{CajaGrafica, Compositor, Fallo, lienzo, modos};

/// `struct NewScreen` (`intuition/screens.h`).
mod ns {
    pub const WIDTH: u32 = 4;
    pub const HEIGHT: u32 = 6;
    pub const DEPTH: u32 = 8;
    pub const TYPE: u32 = 14;
    pub const DEFAULTTITLE: u32 = 20;
    pub const EXTENSION: u32 = 32;
    pub const TAMANO: u32 = 32;
    pub const NS_EXTENDED: u16 = 0x1000;
}

/// Etiquetas de `OpenScreenTagList` (`SA_*`).
mod sa {
    pub const D: u32 = 0x8000_0020;
    pub const WIDTH: u32 = D + 0x03;
    pub const HEIGHT: u32 = D + 0x04;
    pub const DEPTH: u32 = D + 0x05;
    pub const TITLE: u32 = D + 0x08;
    pub const ERRORCODE: u32 = D + 0x0A;
    pub const DISPLAYID: u32 = D + 0x12;
}

const OSERR_NOMEM: u32 = 3;
const OSERR_UNKNOWNMODE: u32 = 6;

impl<C: Compositor> CajaGrafica<C> {
    /// La pantalla de Workbench o una propia, por su dirección.
    pub(crate) fn pantalla(&self, s: u32) -> Option<&Pantalla> {
        if s != 0 && s == self.intui.wb.dir { Some(&self.intui.wb) } else { self.intui.pantallas.get(&s) }
    }

    /// El `ModeID` del `ViewPort` de una pantalla (`GetVPModeID`); el del Workbench si no es de ninguna.
    pub(crate) fn modo_de_vp(&self, vp: u32) -> u32 {
        self.pantalla(vp.wrapping_sub(sc::VIEWPORT)).map_or(self.intui.wb.modo, |p| p.modo)
    }

    /// Cuántas pantallas propias hay abiertas (para las pruebas).
    pub fn pantallas_propias(&self) -> usize {
        self.intui.pantallas.len()
    }

    /// `OpenScreen` (`etiquetas` 0) y `OpenScreenTagList`.
    pub fn open_screen<M: Memoria>(&mut self, ex: &mut Exec<M>, nueva: u32, etiquetas: u32) -> Result<u32, Fallo> {
        let (mut ancho, mut alto, mut _prof, mut titulo, mut id, mut error) = (0i32, 0i32, 0i32, 0u32, None, 0u32);
        let mut listas = Vec::new();
        if nueva != 0 {
            ex.tramo(nueva, ns::TAMANO)?;
            ancho = ex.l16(nueva + ns::WIDTH)? as i16 as i32;
            alto = ex.l16(nueva + ns::HEIGHT)? as i16 as i32;
            _prof = ex.l16(nueva + ns::DEPTH)? as i16 as i32;
            titulo = ex.l32(nueva + ns::DEFAULTTITLE)?;
            if ex.l16(nueva + ns::TYPE)? & ns::NS_EXTENDED != 0 {
                listas.push(ex.l32(nueva + ns::EXTENSION)?);
            }
        }
        listas.push(etiquetas);
        for l in listas {
            for (t, v) in crate::gadtools::leer_etiquetas(ex, l)? {
                match t {
                    sa::WIDTH => ancho = v as i32,
                    sa::HEIGHT => alto = v as i32,
                    sa::DEPTH => _prof = v as i32,
                    sa::TITLE => titulo = v,
                    sa::ERRORCODE => error = v,
                    sa::DISPLAYID => id = Some(v),
                    _ => {}
                }
            }
        }
        let esc = self.comp.tamano_escritorio();
        let Some(m) = id.and_then(|i| modos::modo(i, esc)) else {
            if id.is_some_and(|i| i & modos::MASCARA_MONITOR == modos::MONITOR) {
                if error != 0 {
                    ex.e32(error, OSERR_UNKNOWNMODE)?;
                }
                return Ok(0);
            }
            // Un modo del chipset, o ninguno: la de Workbench (la caja no tiene chipset).
            return self.lock_pub_screen(ex, 0);
        };
        let an = if ancho > 0 { ancho as u32 } else { m.ancho }.min(lienzo::MAX_LADO);
        let al = if alto > 0 { alto as u32 } else { m.alto }.min(lienzo::MAX_LADO);
        match self.crear_pantalla(ex, m, an, al, titulo) {
            Ok(Some(s)) => Ok(s),
            Ok(None) => {
                if error != 0 {
                    ex.e32(error, OSERR_NOMEM)?;
                }
                Ok(0)
            }
            Err(e) => Err(e),
        }
    }

    /// Monta una pantalla propia del modo `m`, de `an` x `al`. `None` sin memoria (soltado lo que se reservó).
    fn crear_pantalla<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        m: modos::Modo,
        an: u32,
        al: u32,
        titulo: u32,
    ) -> Result<Option<u32>, Fallo> {
        let wb = self.intui.wb.dir;
        let cm = self.get_color_map(ex, 256)?;
        if cm == 0 {
            return Ok(None);
        }
        if let Some(p) = self.gfx.paletas.get(&self.gfx.cm_wb).cloned() {
            self.gfx.paletas.insert(cm, p);
        }
        let mut bloques: Vec<(u32, u32)> = Vec::new();
        let s = match ex.reservar(sc::TAMANO, memf::CLEAR) {
            Ok(s) => s,
            Err(_) => {
                self.free_color_map(ex, cm)?;
                return Ok(None);
            }
        };
        bloques.push((s, sc::TAMANO));
        // Los píxeles de su BitMap: chunky en el formato del modo (LUT8 en los de 8 bits, como en una tarjeta).
        let bm = s + sc::BITMAP;
        let fmt = m.formato().unwrap_or(crate::chunky::Formato::Argb32);
        let pixeles = self.chunky_en(ex, bm, an, al, fmt, cm)?.map(|b| vec![b]);
        let Some(pixeles) = pixeles else {
            ex.soltar(s, sc::TAMANO)?;
            self.free_color_map(ex, cm)?;
            return Ok(None);
        };
        bloques.extend(pixeles);
        // Lo mismo que el Workbench: su fuente, sus bordes, su barra.
        for campo in [sc::BARHEIGHT, sc::BARVBORDER, sc::BARHBORDER, sc::MENUVBORDER, sc::MENUHBORDER, sc::WBORTOP, sc::WBORLEFT, sc::WBORRIGHT, sc::WBORBOTTOM] {
            ex.e8(s + campo, ex.l8(wb + campo)?)?;
        }
        ex.e32(s + sc::FONT, ex.l32(wb + sc::FONT)?)?;
        ex.e16(s + sc::WIDTH, an as u16)?;
        ex.e16(s + sc::HEIGHT, al as u16)?;
        ex.e16(s + sc::FLAGS, sc::CUSTOMSCREEN | sc::SHOWTITLE)?;
        ex.e32(s + sc::TITLE, titulo)?;
        ex.e32(s + sc::DEFAULTTITLE, titulo)?;
        let plumas = self.intui.wb.plumas;
        ex.e8(s + sc::DETAILPEN, plumas[dri::DETAILPEN] as u8)?;
        ex.e8(s + sc::BLOCKPEN, plumas[dri::BLOCKPEN] as u8)?;
        let vp = s + sc::VIEWPORT;
        ex.e32(vp + viewport::COLORMAP, cm)?;
        ex.e16(vp + viewport::DWIDTH, an as u16)?;
        ex.e16(vp + viewport::DHEIGHT, al as u16)?;
        ex.e16(vp + viewport::MODES, viewport::HIRES)?;
        ex.e32(cm + colormap::CM_VP, vp)?;
        self.init_rast_port(ex, s + sc::RASTPORT)?;
        ex.e32(s + sc::RASTPORT + rp_::BITMAP, bm)?;
        // Su DrawInfo, con la profundidad del modo.
        let di = ex.reservar(dri::TAMANO, memf::CLEAR)?;
        let largo_plumas = (dri::NUMDRIPENS as u32 + 1) * 2;
        let tabla = ex.reservar(largo_plumas, memf::CLEAR)?;
        bloques.push((di, dri::TAMANO));
        bloques.push((tabla, largo_plumas));
        for (i, p) in plumas.iter().enumerate() {
            ex.e16(tabla + 2 * i as u32, *p)?;
        }
        ex.e16(tabla + 2 * dri::NUMDRIPENS as u32, 0xFFFF)?;
        ex.e16(di + dri::DRI_VERSION, dri::VERSION)?;
        ex.e16(di + dri::DRI_NUMPENS, dri::NUMDRIPENS as u16)?;
        ex.e32(di + dri::DRI_PENS, tabla)?;
        ex.e32(di + dri::DRI_FONT, self.gfx.topaz)?;
        ex.e16(di + dri::DRI_DEPTH, m.prof as u16)?;
        ex.e16(di + dri::DRI_RESOLUTION, 44)?;
        ex.e16(di + dri::DRI_RESOLUTION + 2, 44)?;
        ex.e32(di + dri::DRI_FLAGS, dri::DRIF_NEWLOOK)?;
        // Delante en la lista de pantallas.
        let base = self.intui.base;
        ex.e32(s + sc::NEXTSCREEN, ex.l32(base + ib::FIRSTSCREEN)?)?;
        ex.e32(base + ib::FIRSTSCREEN, s)?;
        ex.e32(base + ib::ACTIVESCREEN, s)?;
        let tarea = ex.tarea_actual();
        self.intui.pantallas.insert(
            s,
            Pantalla { dir: s, ancho: an, alto: al, cm, drawinfo: di, bloqueos: 0, plumas, modo: m.id, tarea, bloques },
        );
        Ok(Some(s))
    }

    /// `CloseScreen`: la de Workbench, como `UnlockPubScreen`; una propia, se suelta si no le quedan ventanas (TRUE) y
    /// si le quedan, FALSE.
    pub fn close_screen<M: Memoria>(&mut self, ex: &mut Exec<M>, s: u32) -> Result<u32, Fallo> {
        if s != 0 && s == self.intui.wb.dir {
            return self.unlock_pub_screen(ex, 0, s).map(|_| 1);
        }
        if !self.intui.pantallas.contains_key(&s) {
            return Err(Fallo::PantallaDesconocida { dir: s });
        }
        if self.intui.ventanas.values().any(|v| v.pantalla == s) {
            return Ok(0);
        }
        let p = self.intui.pantallas.remove(&s).unwrap();
        // Fuera de la lista de pantallas.
        let base = self.intui.base;
        let mut ant = base + ib::FIRSTSCREEN;
        for _ in 0..4096 {
            let x = ex.l32(ant)?;
            if x == 0 {
                break;
            }
            if x == s {
                ex.e32(ant, ex.l32(s + sc::NEXTSCREEN)?)?;
                break;
            }
            ant = x + sc::NEXTSCREEN;
        }
        if ex.l32(base + ib::ACTIVESCREEN)? == s {
            ex.e32(base + ib::ACTIVESCREEN, ex.l32(base + ib::FIRSTSCREEN)?)?;
        }
        let bm = s + sc::BITMAP;
        self.gfx.chunkies.remove(&bm);
        self.cgx_mapa_soltado(ex, bm);
        self.free_color_map(ex, p.cm)?;
        for (d, l) in p.bloques {
            ex.soltar(d, l)?;
        }
        Ok(1)
    }

    /// Las pantallas propias de una tarea que acaba (sus ventanas ya se cerraron): se cierran.
    pub(crate) fn pantallas_tarea_muerta<M: Memoria>(&mut self, ex: &mut Exec<M>, t: u32) {
        let suyas: Vec<u32> = self.intui.pantallas.iter().filter(|(_, p)| p.tarea == t).map(|(s, _)| *s).collect();
        for s in suyas {
            if let Err(f) = self.close_screen(ex, s) {
                self.avisar(ex, "CloseScreen", f);
            }
        }
    }
}
