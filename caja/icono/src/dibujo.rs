//! Pintar un icono en un `RastPort` (`DrawIconStateA`) y medir lo que ocupa (`GetIconRectangleA`).
//!
//! icon.library no pinta por su cuenta: llama a las funciones nativas de graphics, intuition y cybergraphics de la
//! caja, en el acto, por `Entorno::nativa`. Las busca en su tabla de saltos (en la lista de bibliotecas de exec, por
//! su nombre): si un programa la ha cambiado con `SetFunction`, esa entrada no se usa (sería 68k) y no se pinta.
//!
//! - La imagen clásica, con `DrawImage` de intuition (las plumas de la pantalla, como el Workbench); seleccionada sin
//!   imagen propia, en complemento (`SetDrMd(COMPLEMENT)` y `RectFill`).
//! - La de color (de OS 3.5, NewIcons o PNG), en ARGB: se lee el fondo con `ReadPixelArray` de cybergraphics, se
//!   mezcla con el alfa aquí y se escribe con `WritePixelArray`. Si no se puede, la clásica.
//! - El marco (si el icono no es sin marco), un bisel de un punto con las plumas de `DrawInfo` (o 2 y 1); el nombre,
//!   centrado debajo con `Move` y `Text`, en la pluma de texto.
//!
//! Lo que ocupa: el marco va de (0, 0) a lo que mida la imagen más los bordes de `ICONCTRLA_SetGlobalEmbossRect`; el
//! nombre, debajo, centrado (puede empezar a la izquierda de 0 si es más ancho que el icono).

use alloc::vec::Vec;

use caja_exec::estructuras::{execbase, memf};
use caja_exec::{Entrada, Exec, Fallo, Memoria, Registros};

use crate::diskobj::{dobj, gadget, image};
use crate::icon::{oscurecer, tags::*};
use crate::info::{GFLG_GADGHIMAGE, MAX_LADO};
use crate::mem::{cadena_hasta, comprobar, e16, etiquetas, l8, l16, lp, valor};
use crate::{Entorno, Iconos};

/// Estados de `intuition/imageclass.h`.
pub const IDS_NORMAL: u32 = 0;
pub const IDS_SELECTED: u32 = 1;
pub const IDS_DISABLED: u32 = 2;
pub const IDS_INACTIVESELECTED: u32 = 6;
pub const IDS_INACTIVEDISABLED: u32 = 7;
pub const IDS_SELECTEDDISABLED: u32 = 8;

/// Plumas de `DrawInfo` (`intuition/screens.h`).
const TEXTPEN: u32 = 2;
const SHINEPEN: u32 = 3;
const SHADOWPEN: u32 = 4;
const BACKGROUNDPEN: u32 = 7;
/// `DrawInfo`: `dri_NumPens` y `dri_Pens`.
const DRI_NUMPENS: u32 = 2;
const DRI_PENS: u32 = 4;
/// `RastPort`: la pluma A, el modo y las medidas de la letra.
const RP_FGPEN: u32 = 25;
const RP_DRAWMODE: u32 = 28;
const RP_TXHEIGHT: u32 = 58;
const RP_TXBASELINE: u32 = 62;
/// `sizeof(struct RastPort)`.
const TAMANO_RASTPORT: u32 = 100;
const JAM1: u32 = 0;
const COMPLEMENT: u32 = 2;
/// `RECTFMT_ARGB` de cybergraphics.
const RECTFMT_ARGB: u32 = 2;
/// Entre la imagen y su nombre.
const HUECO: i32 = 1;
const MAX_NOMBRE: u32 = 255;

/// LVO de lo que se usa.
const GFX_TEXTLENGTH: i32 = -54;
const GFX_TEXT: i32 = -60;
const GFX_MOVE: i32 = -240;
const GFX_RECTFILL: i32 = -306;
const GFX_SETAPEN: i32 = -342;
const GFX_SETDRMD: i32 = -354;
const INT_DRAWIMAGE: i32 = -114;
const CGX_READPIXELARRAY: i32 = -120;
const CGX_WRITEPIXELARRAY: i32 = -126;

/// Lo que ocupa un icono: la imagen, sus bordes y su nombre.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Medidas {
    pub ancho: i32,
    pub alto: i32,
    /// Bordes: izquierda, arriba, derecha, abajo.
    pub borde: [i32; 4],
    pub marco: bool,
    /// El nombre: dónde empieza (x, relativa al icono), su ancho, alto y línea base.
    pub nombre: Option<(i32, i32, i32, i32)>,
}

impl Medidas {
    pub fn total(&self) -> (i32, i32) {
        (self.borde[0] + self.ancho + self.borde[2], self.borde[1] + self.alto + self.borde[3])
    }
    /// El rectángulo (MinX, MinY, MaxX, MaxY) relativo al icono.
    pub fn rectangulo(&self) -> [i32; 4] {
        let (w, h) = self.total();
        let mut r = [0, 0, w - 1, h - 1];
        if let Some((x, tw, th, _)) = self.nombre {
            r[0] = r[0].min(x);
            r[2] = r[2].max(x + tw - 1);
            r[3] = h - 1 + HUECO + th;
        }
        r
    }
}

fn mezclar(fondo: u32, c: u32, alfa: u32) -> u32 {
    let canal = |s: u32| {
        let (f, v) = ((fondo >> s) & 0xFF, (c >> s) & 0xFF);
        ((v * alfa + f * (255 - alfa)) / 255) << s
    };
    0xFF00_0000 | canal(16) | canal(8) | canal(0)
}

impl Iconos {
    /// La función `lvo` de la biblioteca `nombre`, ejecutada en el acto con `r` (A6 la pone esto).
    fn biblio<M: Memoria, E: Entorno<M>>(
        &self,
        ex: &mut Exec<M>,
        en: &mut E,
        nombre: &[u8],
        lvo: i32,
        mut r: Registros,
    ) -> Result<Registros, Fallo> {
        let base = ex.find_name(ex.base() + execbase::LIBLIST, nombre)?;
        if base == 0 {
            return Err(Fallo::BibliotecaDesconocida { dir: 0 });
        }
        let n = match ex.entrada(base.wrapping_add_signed(lvo))? {
            Entrada::Nativa(n) => n,
            // Cambiada con SetFunction: sería 68k, que aquí no se puede ejecutar.
            _ => return Err(Fallo::Sospechoso { dir: base.wrapping_add_signed(lvo) }),
        };
        r.a[6] = base;
        if !en.nativa(ex, n, &mut r) {
            return Err(Fallo::NoImplementada { n });
        }
        Ok(r)
    }

    fn gfx<M: Memoria, E: Entorno<M>>(
        &self,
        ex: &mut Exec<M>,
        en: &mut E,
        lvo: i32,
        rp: u32,
        d: &[u32],
        a0: u32,
    ) -> Result<u32, Fallo> {
        let mut r = Registros::default();
        r.a[1] = rp;
        r.a[0] = a0;
        r.d[..d.len()].copy_from_slice(d);
        Ok(self.biblio(ex, en, b"graphics.library", lvo, r)?.d[0])
    }

    #[allow(clippy::too_many_arguments)]
    fn rect<M: Memoria, E: Entorno<M>>(
        &self,
        ex: &mut Exec<M>,
        en: &mut E,
        rp: u32,
        pluma: u32,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
    ) -> Result<(), Fallo> {
        if x1 < x0 || y1 < y0 {
            return Ok(());
        }
        self.gfx(ex, en, GFX_SETAPEN, rp, &[pluma], 0)?;
        self.gfx(ex, en, GFX_RECTFILL, rp, &[x0 as u32, y0 as u32, x1 as u32, y1 as u32], 0)?;
        Ok(())
    }

    /// Las medidas de la imagen que se pinta: la de color si la hay (y se usa), o la clásica, o el gadget.
    fn medidas_imagen<M: Memoria>(&self, ex: &Exec<M>, icono: u32) -> Result<(i32, i32), Fallo> {
        comprobar(icono, dobj::TAMANO, false)?;
        if let Some(c) = self.iconos.get(&icono).and_then(|n| n.color.as_ref())
            && self.global.color
            && c.ancho > 0
            && c.coherente()
            && (c.img[0].is_some() || c.argb[0].is_some())
        {
            return Ok((c.ancho as i32, c.alto as i32));
        }
        let img = lp(ex, icono + dobj::DO_GADGET + gadget::GADGETRENDER)?;
        if img != 0 {
            return Ok((l16(ex, img + image::WIDTH)? as i32, l16(ex, img + image::HEIGHT)? as i32));
        }
        let g = icono + dobj::DO_GADGET;
        Ok((l16(ex, g + gadget::WIDTH)? as i32, l16(ex, g + gadget::HEIGHT)? as i32))
    }

    /// Lo que ocupa el icono con estas etiquetas y este nombre (que se mide con la letra de `rp`).
    pub fn medir<M: Memoria, E: Entorno<M>>(
        &self,
        ex: &mut Exec<M>,
        en: &mut E,
        rp: u32,
        icono: u32,
        nombre: u32,
        lista: u32,
    ) -> Result<Medidas, Fallo> {
        if icono == 0 {
            return Err(Fallo::PunteroNulo);
        }
        let et = etiquetas(ex, lista)?;
        let (ancho, alto) = self.medidas_imagen(ex, icono)?;
        if ancho as usize > MAX_LADO || alto as usize > MAX_LADO {
            return Err(Fallo::Sospechoso { dir: icono });
        }
        let sin_borde = valor(&et, ICONDRAWA_BORDERLESS, 0) != 0;
        let sin_marco_icono = self.iconos.get(&icono).is_some_and(|n| n.sin_marco) || self.global.sin_marco;
        let sin_marco = valor(&et, ICONDRAWA_FRAMELESS, sin_marco_icono as u32) != 0;
        let m = self.global.marco;
        let borde = if sin_borde { [0; 4] } else { [-m[0] as i32, -m[1] as i32, m[2] as i32, m[3] as i32] };
        let mut md = Medidas { ancho, alto, borde, marco: !sin_borde && !sin_marco, nombre: None };
        if nombre != 0 {
            if rp == 0 {
                return Err(Fallo::PunteroNulo);
            }
            comprobar(rp, TAMANO_RASTPORT, false)?;
            let t = cadena_hasta(ex, nombre, 4096)?;
            let largo = (t.len() as u32).min(MAX_NOMBRE);
            let tw = self.gfx(ex, en, GFX_TEXTLENGTH, rp, &[largo], nombre)? as u16 as i32;
            let th = l16(ex, rp + RP_TXHEIGHT)? as i32;
            let base = l16(ex, rp + RP_TXBASELINE)? as i32;
            let (w, _) = md.total();
            md.nombre = Some(((w - tw) / 2, tw, th, base));
        }
        Ok(md)
    }

    /// `GetIconRectangleA`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn get_icon_rectangle<M: Memoria, E: Entorno<M>>(
        &mut self,
        ex: &mut Exec<M>,
        en: &mut E,
        rp: u32,
        icono: u32,
        nombre: u32,
        rect: u32,
        lista: u32,
    ) -> Result<bool, Fallo> {
        if rect == 0 || icono == 0 {
            return Ok(false);
        }
        comprobar(rect, 8, true)?;
        let md = self.medir(ex, en, rp, icono, nombre, lista)?;
        let r = md.rectangulo();
        for (k, v) in r.iter().enumerate() {
            if *v < i16::MIN as i32 || *v > i16::MAX as i32 {
                return Ok(false);
            }
            e16(ex, rect + 2 * k as u32, *v as i16 as u16)?;
        }
        Ok(true)
    }

    fn pluma<M: Memoria>(ex: &Exec<M>, dri: u32, i: u32, defecto: u32) -> u32 {
        if dri == 0 {
            return defecto;
        }
        let n = l16(ex, dri.wrapping_add(DRI_NUMPENS)).unwrap_or(0) as u32;
        let p = lp(ex, dri.wrapping_add(DRI_PENS)).unwrap_or(0);
        if p == 0 || i >= n {
            return defecto;
        }
        l16(ex, p.wrapping_add(2 * i)).map_or(defecto, |v| v as u32)
    }

    /// `DrawIconStateA`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_icon_state<M: Memoria, E: Entorno<M>>(
        &mut self,
        ex: &mut Exec<M>,
        en: &mut E,
        rp: u32,
        icono: u32,
        nombre: u32,
        x: i32,
        y: i32,
        estado: u32,
        lista: u32,
    ) -> Result<(), Fallo> {
        if rp == 0 {
            return Err(Fallo::PunteroNulo);
        }
        comprobar(rp, TAMANO_RASTPORT, true)?;
        let md = self.medir(ex, en, rp, icono, nombre, lista)?;
        let et = etiquetas(ex, lista)?;
        let dri = valor(&et, ICONDRAWA_DRAWINFO, 0);
        let borrar = valor(&et, ICONDRAWA_ERASEBACKGROUND, 1) != 0;
        let enlace = valor(&et, ICONDRAWA_ISLINK, 0) != 0;
        let seleccionado = matches!(estado, IDS_SELECTED | IDS_INACTIVESELECTED | IDS_SELECTEDDISABLED);
        let apagado = matches!(estado, IDS_DISABLED | IDS_INACTIVEDISABLED | IDS_SELECTEDDISABLED);
        let (pluma_antes, modo_antes) = (l8(ex, rp + RP_FGPEN)? as u32, l8(ex, rp + RP_DRAWMODE)? as u32);
        self.gfx(ex, en, GFX_SETDRMD, rp, &[JAM1], 0)?;
        let (w, h) = md.total();
        let fondo = Self::pluma(ex, dri, BACKGROUNDPEN, 0);
        if md.marco || borrar {
            self.rect(ex, en, rp, fondo, x, y, x + w - 1, y + h - 1)?;
        }
        if md.marco {
            let (claro, oscuro) = (Self::pluma(ex, dri, SHINEPEN, 2), Self::pluma(ex, dri, SHADOWPEN, 1));
            let (arriba, abajo) = if seleccionado { (oscuro, claro) } else { (claro, oscuro) };
            self.rect(ex, en, rp, arriba, x, y, x + w - 1, y)?;
            self.rect(ex, en, rp, arriba, x, y, x, y + h - 1)?;
            self.rect(ex, en, rp, abajo, x, y + h - 1, x + w - 1, y + h - 1)?;
            self.rect(ex, en, rp, abajo, x + w - 1, y, x + w - 1, y + h - 1)?;
        }
        let (ix, iy) = (x + md.borde[0], y + md.borde[1]);
        let pintada = self.pintar_color(ex, en, rp, icono, ix, iy, seleccionado, apagado);
        if !matches!(pintada, Ok(true)) {
            if let Err(f) = pintada {
                // Sin cybergraphics (o cambiada): la imagen clásica.
                self.avisar(ex, "DrawIconStateA", f);
            }
            self.pintar_clasica(ex, en, rp, icono, ix, iy, md.ancho, md.alto, seleccionado)?;
        }
        if let Some((nx, tw, _, base)) = md.nombre {
            let t = cadena_hasta(ex, nombre, 4096)?;
            let largo = (t.len() as u32).min(MAX_NOMBRE);
            let (lx, ly) = (x + nx, y + h + HUECO);
            self.gfx(ex, en, GFX_SETAPEN, rp, &[Self::pluma(ex, dri, TEXTPEN, 1)], 0)?;
            self.gfx(ex, en, GFX_MOVE, rp, &[lx as u32, (ly + base) as u32], 0)?;
            self.gfx(ex, en, GFX_TEXT, rp, &[largo], nombre)?;
            if enlace {
                let p = Self::pluma(ex, dri, TEXTPEN, 1);
                self.rect(ex, en, rp, p, lx, ly + base + 1, lx + tw - 1, ly + base + 1)?;
            }
        }
        self.gfx(ex, en, GFX_SETAPEN, rp, &[pluma_antes], 0)?;
        self.gfx(ex, en, GFX_SETDRMD, rp, &[modo_antes], 0)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn pintar_clasica<M: Memoria, E: Entorno<M>>(
        &self,
        ex: &mut Exec<M>,
        en: &mut E,
        rp: u32,
        icono: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        seleccionado: bool,
    ) -> Result<(), Fallo> {
        let g = icono + dobj::DO_GADGET;
        let (normal, sel) = (lp(ex, g + gadget::GADGETRENDER)?, lp(ex, g + gadget::SELECTRENDER)?);
        let flags = l16(ex, g + gadget::FLAGS)?;
        let con_segunda = seleccionado && sel != 0 && flags & 3 == GFLG_GADGHIMAGE;
        let img = if con_segunda { sel } else { normal };
        if img == 0 {
            return Ok(());
        }
        let mut r = Registros::default();
        r.a[0] = rp;
        r.a[1] = img;
        r.d[0] = x as u32;
        r.d[1] = y as u32;
        self.biblio(ex, en, b"intuition.library", INT_DRAWIMAGE, r)?;
        if seleccionado && !con_segunda {
            self.gfx(ex, en, GFX_SETDRMD, rp, &[COMPLEMENT], 0)?;
            self.gfx(ex, en, GFX_RECTFILL, rp, &[x as u32, y as u32, (x + w - 1) as u32, (y + h - 1) as u32], 0)?;
            self.gfx(ex, en, GFX_SETDRMD, rp, &[JAM1], 0)?;
        }
        Ok(())
    }

    /// La imagen de color, mezclada con el fondo. `Ok(false)` si el icono no la tiene.
    #[allow(clippy::too_many_arguments)]
    fn pintar_color<M: Memoria, E: Entorno<M>>(
        &self,
        ex: &mut Exec<M>,
        en: &mut E,
        rp: u32,
        icono: u32,
        x: i32,
        y: i32,
        seleccionado: bool,
        apagado: bool,
    ) -> Result<bool, Fallo> {
        let Some(c) = self.iconos.get(&icono).and_then(|n| n.color.as_ref()) else { return Ok(false) };
        if !self.global.color || !c.coherente() || c.ancho == 0 || c.alto == 0 {
            return Ok(false);
        }
        let real = if seleccionado { c.imagen_argb(1) } else { None };
        let Some(mut px) = real.clone().or_else(|| c.imagen_argb(0)) else { return Ok(false) };
        if seleccionado && real.is_none() {
            // Sin imagen de seleccionado: la normal, más oscura (ChangeToSelectedIconColor).
            for p in &mut px {
                let o = |s: u32| (oscurecer(((*p >> s) & 0xFF) as u8) as u32) << s;
                *p = (*p & 0xFF00_0000) | o(16) | o(8) | o(0);
            }
        }
        let (an, al) = (c.ancho as u32, c.alto as u32);
        let largo = an * al * 4;
        let buf = ex.alloc_mem(largo, memf::PUBLIC)?;
        if buf == 0 {
            return Err(Fallo::SinSitio);
        }
        let r = (|| -> Result<(), Fallo> {
            let mut r = Registros::default();
            r.a[0] = buf;
            r.d[2] = an * 4;
            r.a[1] = rp;
            r.d[3] = x as u32;
            r.d[4] = y as u32;
            r.d[5] = an;
            r.d[6] = al;
            r.d[7] = RECTFMT_ARGB;
            self.biblio(ex, en, b"cybergraphics.library", CGX_READPIXELARRAY, r)?;
            let fondo = crate::mem::bytes(ex, buf, largo)?;
            let mut v = Vec::with_capacity(largo as usize);
            for (k, p) in px.iter().enumerate() {
                let f = u32::from_be_bytes([fondo[4 * k], fondo[4 * k + 1], fondo[4 * k + 2], fondo[4 * k + 3]]);
                let mut a = p >> 24;
                if apagado {
                    a /= 2;
                }
                v.extend_from_slice(&mezclar(f, *p, a).to_be_bytes());
            }
            crate::mem::escribir(ex, buf, &v)?;
            self.biblio(ex, en, b"cybergraphics.library", CGX_WRITEPIXELARRAY, r)?;
            Ok(())
        })();
        let _ = ex.free_mem(buf, largo);
        r.map(|_| true)
    }
}
