//! Las pantallas propias: `OpenScreenTagList` con un modo de la caja (`modos.rs`), o `SA_LikeWorkbench`, da una
//! `struct Screen` nueva, del tamaño y la profundidad del modo (o los que pida, `SA_Width`/`SA_Height`), con:
//! - su `ColorMap`: la paleta del Workbench de partida, con los colores de `SA_Colors` (`ColorSpec`) y `SA_Colors32`
//!   (como `LoadRGB32`);
//! - su `DrawInfo`, con su profundidad y las plumas de `SA_Pens` (o las del Workbench);
//! - su `BitMap`: un lienzo en la memoria del 68k en el formato del modo (LUT8 en los de 8 bits, con su paleta), o el
//!   del programa (`SA_BitMap`, o `CUSTOMBITMAP` en la `NewScreen`);
//! - una superficie del compositor de su tamaño, sin borde: lo que se pinta en su mapa se ve, solo lo que ha cambiado
//!   (en el lienzo, lo apunta la vista; en el mapa del programa, `vigia.rs`).
//!
//! Sus ventanas son ventanas del compositor como las del Workbench, con la paleta y las plumas de su pantalla. Va
//! delante en la lista de `IntuitionBase`. Con `SA_PubName` es pública: `LockPubScreen` la encuentra por su nombre
//! (si no está en modo privado: `PubScreenStatus`), y al irse el último visitante se manda la señal de `SA_PubSig` a
//! `SA_PubTask`. `CloseScreen` la suelta si no le quedan ventanas ni visitantes (si no, FALSE, como en 3.x); si su
//! tarea acaba sin cerrarla, se cierra.
//!
//! Sin modo de la caja (uno del chipset o ninguno) y sin `SA_LikeWorkbench`, `OpenScreen` da la pantalla de Workbench,
//! como antes: la caja no tiene chipset. Un `ModeID` del monitor de la caja que no está (más grande que el escritorio,
//! una profundidad que no hay) falla con `OSERR_UNKNOWNMODE`.

use alloc::vec::Vec;

use caja_exec::estructuras::memf;
use caja_exec::{Exec, Memoria};

use crate::acceso::Acceso;
use crate::estructuras::{colormap, drawinfo as dri, intuitionbase as ib, rastport as rp_, screen as sc, viewport};
use crate::intuition::Pantalla;
use crate::{CajaGrafica, Compositor, Fallo, Superficie, lienzo, modos};

/// `struct NewScreen` (`intuition/screens.h`).
mod ns {
    pub const WIDTH: u32 = 4;
    pub const HEIGHT: u32 = 6;
    pub const TYPE: u32 = 14;
    pub const DEFAULTTITLE: u32 = 20;
    pub const CUSTOMBITMAP: u32 = 28;
    /// En `Type`: el mapa es el de `CustomBitMap`.
    pub const T_CUSTOMBITMAP: u16 = 0x0040;
    pub const EXTENSION: u32 = 32;
    pub const TAMANO: u32 = 32;
    pub const NS_EXTENDED: u16 = 0x1000;
}

/// Etiquetas de `OpenScreenTagList` (`SA_*`).
mod sa {
    pub const D: u32 = 0x8000_0020;
    pub const WIDTH: u32 = D + 0x03;
    pub const HEIGHT: u32 = D + 0x04;
    pub const TITLE: u32 = D + 0x08;
    pub const COLORS: u32 = D + 0x09;
    pub const ERRORCODE: u32 = D + 0x0A;
    pub const BITMAP: u32 = D + 0x0E;
    pub const PUBNAME: u32 = D + 0x0F;
    pub const PUBSIG: u32 = D + 0x10;
    pub const PUBTASK: u32 = D + 0x11;
    pub const DISPLAYID: u32 = D + 0x12;
    pub const PENS: u32 = D + 0x1A;
    pub const FULLPALETTE: u32 = D + 0x1B;
    pub const COLORS32: u32 = D + 0x23;
    pub const LIKEWORKBENCH: u32 = D + 0x27;
}

/// `PubScreenStatus`: el único estado que hay.
const PSNF_PRIVATE: u32 = 1;

/// Lo que pide `OpenScreenTagList`.
#[derive(Default)]
struct Peticion {
    ancho: i32,
    alto: i32,
    titulo: u32,
    id: Option<u32>,
    error: u32,
    colores: u32,
    colores32: u32,
    plumas: u32,
    mapa: u32,
    nombre: u32,
    senal: Option<u8>,
    tarea_aviso: u32,
    como_workbench: bool,
}

const OSERR_NOMEM: u32 = 3;
const OSERR_PUBNOTUNIQUE: u32 = 5;
const OSERR_UNKNOWNMODE: u32 = 6;

impl<C: Compositor> CajaGrafica<C> {
    /// La pantalla de Workbench o una propia, por su dirección.
    pub(crate) fn pantalla(&self, s: u32) -> Option<&Pantalla> {
        if s != 0 && s == self.intui.wb.dir { Some(&self.intui.wb) } else { self.intui.pantallas.get(&s) }
    }

    /// La pantalla de una ventana (la de Workbench si no es de ninguna).
    pub(crate) fn pantalla_de(&self, w: u32) -> &Pantalla {
        self.intui.ventanas.get(&w).and_then(|v| self.pantalla(v.pantalla)).unwrap_or(&self.intui.wb)
    }

    /// El `ModeID` del `ViewPort` de una pantalla (`GetVPModeID`); el del Workbench si no es de ninguna.
    pub(crate) fn modo_de_vp(&self, vp: u32) -> u32 {
        self.pantalla(vp.wrapping_sub(sc::VIEWPORT)).map_or(self.intui.wb.modo, |p| p.modo)
    }

    /// La superficie del compositor de una pantalla propia (para las pruebas).
    pub fn superficie_de_pantalla(&self, s: u32) -> Option<Superficie> {
        self.intui.pantallas.get(&s).map(|p| p.superficie).filter(|&x| x != 0)
    }

    /// Cuántas pantallas propias hay abiertas (para las pruebas).
    pub fn pantallas_propias(&self) -> usize {
        self.intui.pantallas.len()
    }

    /// `OpenScreen` (`etiquetas` 0) y `OpenScreenTagList`.
    pub fn open_screen<M: Memoria>(&mut self, ex: &mut Exec<M>, nueva: u32, etiquetas: u32) -> Result<u32, Fallo> {
        let mut q = Peticion::default();
        let mut listas = Vec::new();
        if nueva != 0 {
            ex.tramo(nueva, ns::TAMANO)?;
            q.ancho = ex.l16(nueva + ns::WIDTH)? as i16 as i32;
            q.alto = ex.l16(nueva + ns::HEIGHT)? as i16 as i32;
            q.titulo = ex.l32(nueva + ns::DEFAULTTITLE)?;
            let tipo = ex.l16(nueva + ns::TYPE)?;
            if tipo & ns::T_CUSTOMBITMAP != 0 {
                q.mapa = ex.l32(nueva + ns::CUSTOMBITMAP)?;
            }
            if tipo & ns::NS_EXTENDED != 0 {
                listas.push(ex.l32(nueva + ns::EXTENSION)?);
            }
        }
        listas.push(etiquetas);
        for l in listas {
            for (t, v) in crate::gadtools::leer_etiquetas(ex, l)? {
                match t {
                    sa::WIDTH => q.ancho = v as i32,
                    sa::HEIGHT => q.alto = v as i32,
                    sa::TITLE => q.titulo = v,
                    sa::ERRORCODE => q.error = v,
                    sa::DISPLAYID => q.id = Some(v),
                    sa::COLORS => q.colores = v,
                    sa::COLORS32 => q.colores32 = v,
                    sa::PENS => q.plumas = v,
                    sa::BITMAP => q.mapa = v,
                    sa::PUBNAME => q.nombre = v,
                    sa::PUBSIG => q.senal = (v < 32).then_some(v as u8),
                    sa::PUBTASK => q.tarea_aviso = v,
                    sa::LIKEWORKBENCH => q.como_workbench = v != 0,
                    // La paleta de partida ya es entera (la del Workbench).
                    sa::FULLPALETTE => {}
                    _ => {}
                }
            }
        }
        let esc = self.comp.tamano_escritorio();
        if q.id.is_none() && q.como_workbench {
            q.id = Some(self.intui.wb.modo);
        }
        let Some(m) = q.id.and_then(|i| modos::modo(i, esc)) else {
            if q.id.is_some_and(|i| i & modos::MASCARA_MONITOR == modos::MONITOR) {
                if q.error != 0 {
                    ex.e32(q.error, OSERR_UNKNOWNMODE)?;
                }
                return Ok(0);
            }
            // Un modo del chipset, o ninguno: la de Workbench (la caja no tiene chipset).
            return self.lock_pub_screen(ex, 0);
        };
        // Un nombre público que ya tiene otra: OSERR_PUBNOTUNIQUE.
        if q.nombre != 0 && self.nombre_libre(ex, q.nombre)?.is_none() {
            if q.error != 0 {
                ex.e32(q.error, OSERR_PUBNOTUNIQUE)?;
            }
            return Ok(0);
        }
        let an = if q.ancho > 0 { q.ancho as u32 } else { m.ancho }.min(lienzo::MAX_LADO);
        let al = if q.alto > 0 { q.alto as u32 } else { m.alto }.min(lienzo::MAX_LADO);
        match self.crear_pantalla(ex, m, an, al, &q) {
            Ok(Some(s)) => Ok(s),
            Ok(None) => {
                if q.error != 0 {
                    ex.e32(q.error, OSERR_NOMEM)?;
                }
                Ok(0)
            }
            Err(e) => Err(e),
        }
    }

    /// El nombre de una pantalla pública nueva, si nadie lo tiene (sin distinguir mayúsculas).
    fn nombre_libre<M: Memoria>(&self, ex: &Exec<M>, nombre: u32) -> Result<Option<Vec<u8>>, Fallo> {
        let n = ex.cadena(nombre)?;
        let ocupado = core::iter::once(&self.intui.wb)
            .chain(self.intui.pantallas.values())
            .any(|p| p.nombre.as_ref().is_some_and(|x| x.eq_ignore_ascii_case(&n)));
        Ok((!ocupado && !n.is_empty()).then_some(n))
    }

    /// Monta una pantalla propia del modo `m`, de `an` x `al`. `None` sin memoria (soltado lo que se reservó).
    fn crear_pantalla<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        m: modos::Modo,
        an: u32,
        al: u32,
        q: &Peticion,
    ) -> Result<Option<u32>, Fallo> {
        let wb = self.intui.wb.dir;
        // Primero se lee todo lo que da el programa (lo que no se puede leer es un fallo, antes de reservar nada).
        let colores = self.colores_pedidos(ex, q)?;
        let mut plumas = self.intui.wb.plumas;
        if q.plumas != 0 {
            // SA_Pens: las de la DrawInfo, acabadas en ~0.
            for (i, p) in plumas.iter_mut().enumerate() {
                let v = ex.l16(q.plumas + 2 * i as u32)?;
                if v == 0xFFFF {
                    break;
                }
                *p = v;
            }
        }
        let copia_mapa = if q.mapa != 0 { Some(ex.bytes(q.mapa, crate::estructuras::bitmap::TAMANO)?) } else { None };
        let cm = self.get_color_map(ex, 256)?;
        if cm == 0 {
            return Ok(None);
        }
        if let Some(p) = self.gfx.paletas.get(&self.gfx.cm_wb).cloned() {
            self.gfx.paletas.insert(cm, p);
        }
        for (i, rgb) in colores {
            self.poner_color(ex, cm, i, rgb)?;
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
        // Su mapa: el del programa (copiado en el de la pantalla, como hace intuition), o un lienzo en el formato del
        // modo, con sus píxeles ya en la memoria del 68k.
        let bm = s + sc::BITMAP;
        let mapa_rp = if let Some(b) = copia_mapa {
            ex.escribir_bytes(bm, &b)?;
            q.mapa
        } else {
            let fmt = m.formato().unwrap_or(crate::chunky::Formato::Argb32);
            let colores = self.gfx.paletas.get(&cm).cloned().unwrap_or_default();
            self.nuevo_nativo(ex, bm, an, al, cm)?;
            if let Some(l) = self.gfx.lienzos.get_mut(&bm) {
                l.poner_formato(fmt);
                l.poner_lut(&colores);
            }
            let con = crate::graphics::vista(&mut self.gfx.lienzos, ex, bm).is_some_and(|v| v.l.con_pixeles());
            if !con {
                self.gfx.lienzos.remove(&bm);
                ex.soltar(s, sc::TAMANO)?;
                self.free_color_map(ex, cm)?;
                return Ok(None);
            }
            bm
        };
        // Lo mismo que el Workbench: su fuente, sus bordes, su barra.
        for campo in [sc::BARHEIGHT, sc::BARVBORDER, sc::BARHBORDER, sc::MENUVBORDER, sc::MENUHBORDER, sc::WBORTOP, sc::WBORLEFT, sc::WBORRIGHT, sc::WBORBOTTOM] {
            ex.e8(s + campo, ex.l8(wb + campo)?)?;
        }
        ex.e32(s + sc::FONT, ex.l32(wb + sc::FONT)?)?;
        ex.e16(s + sc::WIDTH, an as u16)?;
        ex.e16(s + sc::HEIGHT, al as u16)?;
        let publica = if q.nombre != 0 { sc::PUBLICSCREEN } else { 0 };
        ex.e16(s + sc::FLAGS, sc::CUSTOMSCREEN | sc::SHOWTITLE | publica)?;
        ex.e32(s + sc::TITLE, q.titulo)?;
        ex.e32(s + sc::DEFAULTTITLE, q.titulo)?;
        ex.e8(s + sc::DETAILPEN, plumas[dri::DETAILPEN] as u8)?;
        ex.e8(s + sc::BLOCKPEN, plumas[dri::BLOCKPEN] as u8)?;
        let vp = s + sc::VIEWPORT;
        ex.e32(vp + viewport::COLORMAP, cm)?;
        ex.e16(vp + viewport::DWIDTH, an as u16)?;
        ex.e16(vp + viewport::DHEIGHT, al as u16)?;
        ex.e16(vp + viewport::MODES, viewport::HIRES)?;
        ex.e32(cm + colormap::CM_VP, vp)?;
        self.init_rast_port(ex, s + sc::RASTPORT)?;
        ex.e32(s + sc::RASTPORT + rp_::BITMAP, mapa_rp)?;
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
        // Su superficie del compositor, sin borde y de su tamaño: se ve lo que se pinta en su mapa.
        let titulo: alloc::string::String =
            if q.titulo != 0 { ex.cadena(q.titulo).unwrap_or_default().iter().map(|&c| c as char).collect() } else { alloc::string::String::new() };
        let pedida = crate::compositor::NuevaVentana {
            titulo,
            x: 0,
            y: 0,
            ancho: an,
            alto: al,
            activa: false,
            sin_borde: true,
            minimo: (an, al),
            maximo: (an, al),
            programa: Self::nombre_programa(ex, tarea),
            barra_dashos: false,
            hueco_marca: None,
            padre: None,
        };
        let superficie = self.comp.crear_ventana(&pedida).unwrap_or(0);
        let nombre = if q.nombre != 0 { self.nombre_libre(ex, q.nombre)? } else { None };
        let aviso = q.senal.map(|b| (if q.tarea_aviso != 0 { q.tarea_aviso } else { tarea }, b));
        self.intui.pantallas.insert(
            s,
            Pantalla {
                dir: s,
                ancho: an,
                alto: al,
                cm,
                drawinfo: di,
                bloqueos: 0,
                plumas,
                modo: m.id,
                tarea,
                bloques,
                // Una pantalla pública nace en modo privado (como en 3.x): `PubScreenStatus(s, 0)` la abre.
                privada: nombre.is_some(),
                nombre,
                aviso,
                superficie,
                mapa_programa: q.mapa,
                activa: 0,
            },
        );
        if q.mapa != 0 {
            self.gfx.vigilados.entry(q.mapa).or_insert_with(crate::vigia::Vigilado::nuevo);
        }
        Ok(Some(s))
    }

    /// Los colores que pide una pantalla nueva: los de `SA_Colors` (`ColorSpec`: índice, R, G y B de 4 bits; acaba en
    /// -1) y los de `SA_Colors32` (como `LoadRGB32`: número << 16 | primero, y de cada color R, G y B de 32 bits; acaba
    /// en 0; 64 tramos como mucho). Pares (pluma, RGB).
    fn colores_pedidos<M: Memoria>(&self, ex: &Exec<M>, q: &Peticion) -> Result<Vec<(u32, u32)>, Fallo> {
        let mut v = Vec::new();
        if q.colores != 0 {
            for k in 0..256u32 {
                let d = q.colores + 8 * k;
                let i = ex.l16(d)? as i16;
                if i < 0 {
                    break;
                }
                let c = |o: u32| -> Result<u32, Fallo> { Ok((ex.l16(d + o)? as u32 & 15) * 0x11) };
                v.push((i as u32, (c(2)? << 16) | (c(4)? << 8) | c(6)?));
            }
        }
        if q.colores32 != 0 {
            let mut d = q.colores32;
            for _ in 0..64 {
                let cab = ex.l32(d)?;
                let (n, primero) = (cab >> 16, cab & 0xFFFF);
                if n == 0 {
                    break;
                }
                for k in 0..n.min(256) {
                    let c = |o: u32| -> Result<u32, Fallo> { Ok(ex.l32(d + 4 + 12 * k + o)? >> 24) };
                    v.push((primero + k, (c(0)? << 16) | (c(4)? << 8) | c(8)?));
                }
                d += 4 + 12 * n;
            }
        }
        Ok(v)
    }

    /// `CloseScreen`: la de Workbench, como `UnlockPubScreen`; una propia, se suelta si no le quedan ventanas ni
    /// visitantes (TRUE) y si le quedan, FALSE.
    pub fn close_screen<M: Memoria>(&mut self, ex: &mut Exec<M>, s: u32) -> Result<u32, Fallo> {
        if s != 0 && s == self.intui.wb.dir {
            return self.unlock_pub_screen(ex, 0, s).map(|_| 1);
        }
        let Some(p) = self.intui.pantallas.get(&s) else { return Err(Fallo::PantallaDesconocida { dir: s }) };
        if p.bloqueos > 0 || self.intui.ventanas.values().any(|v| v.pantalla == s) {
            return Ok(0);
        }
        let p = self.intui.pantallas.remove(&s).unwrap();
        if p.superficie != 0 {
            self.comp.cerrar_ventana(p.superficie);
        }
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
        if p.mapa_programa != 0 && !self.intui.pantallas.values().any(|o| o.mapa_programa == p.mapa_programa) {
            self.gfx.vigilados.remove(&p.mapa_programa);
        }
        let bm = s + sc::BITMAP;
        self.soltar_lienzo(ex, bm);
        self.free_color_map(ex, p.cm)?;
        for (d, l) in p.bloques {
            ex.soltar(d, l)?;
        }
        Ok(1)
    }

    /// `PubScreenStatus`: `PSNF_PRIVATE` la pone en modo privado (no si tiene visitantes) y 0 la abre. El bit de abajo
    /// del resultado es 0 si no es pública o no se pudo poner en privado.
    pub fn pub_screen_status(&mut self, s: u32, flags: u32) -> Result<u32, Fallo> {
        let dueno = self.pantalla(s).map_or(0, |p| p.tarea);
        // Visitantes: las ventanas de otros programas en ella.
        let visitantes = self.intui.ventanas.values().filter(|v| v.pantalla == s && v.tarea != dueno).count();
        let Some(p) = self.pantalla_mut(s) else { return Err(Fallo::PantallaDesconocida { dir: s }) };
        if p.nombre.is_none() || p.tarea == 0 {
            return Ok(0);
        }
        if flags & PSNF_PRIVATE != 0 {
            if p.bloqueos > 0 || visitantes > 0 {
                return Ok(0);
            }
            p.privada = true;
        } else {
            p.privada = false;
        }
        Ok(1)
    }

    pub(crate) fn pantalla_mut(&mut self, s: u32) -> Option<&mut Pantalla> {
        if s != 0 && s == self.intui.wb.dir { Some(&mut self.intui.wb) } else { self.intui.pantallas.get_mut(&s) }
    }

    /// Presenta al compositor lo que ha cambiado en el mapa de cada pantalla propia (el lienzo, con su daño; el mapa
    /// del programa, con lo apuntado y lo que se vea en sus filas: `vigia.rs`).
    pub(crate) fn presentar_pantallas<M: Memoria>(&mut self, ex: &Exec<M>) {
        let todas: Vec<(u32, Superficie, u32, u32, u32, u32)> = self
            .intui
            .pantallas
            .values()
            .filter(|p| p.superficie != 0)
            .map(|p| (p.dir, p.superficie, p.mapa_programa, p.ancho, p.alto, p.cm))
            .collect();
        for (s, sup, programa, an, al, cm) in todas {
            if programa != 0 {
                self.presentar_mapa_programa(ex, programa, sup, an, al, cm);
                continue;
            }
            let Some(l) = self.gfx.lienzos.get_mut(&(s + sc::BITMAP)) else { continue };
            let (lan, lal) = (l.ancho(), l.alto());
            if let Some((d, pix, textos)) = l.presentar(&ex.mem) {
                self.comp.presentar(sup, pix, lan, lal, d, textos);
            }
        }
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
