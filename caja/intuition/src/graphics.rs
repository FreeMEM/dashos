//! graphics.library: RastPort, BitMap, dibujo, texto, fuentes y paletas (RKRM: Libraries, "Graphics Primitives",
//! "Graphics Library and Text"; autodocs de graphics.library).
//!
//! Un `BitMap` es nativo si está en la tabla de lienzos (los de las ventanas, la pantalla y `AllocBitMap` de más
//! de 8 planos o con amigo nativo); si no, es planar y se dibuja en sus planos, en la memoria del 68k. Las plumas
//! se traducen a ARGB con el `ColorMap` del lienzo al dibujar: en un mapa de 32 bits, cambiar la paleta después no
//! cambia lo ya pintado (como en una pantalla RTG de color verdadero).

use alloc::collections::BTreeSet;
use alloc::vec;
use alloc::vec::Vec;

use caja_exec::estructuras::{memf, node, nt};
use caja_exec::{Exec, Memoria};

use crate::acceso::{Acceso, puntero};
use crate::dibujo::{PLANO_UNOS, Pintor, Planar, Sup, Tinta, mas_cercano};
use crate::estructuras::{bitmap, colormap, drmd, gfxbase, rastport as rp_, textattr, textfont as tf, viewport};
use crate::lienzo::{self, Lienzo, Rect};
use crate::{CajaGrafica, Compositor, Fallo, Tabla, fuente, lvo};

/// Lo que graphics lleva fuera de la memoria del 68k.
#[derive(Default)]
pub(crate) struct Estado {
    pub base: u32,
    pub topaz: u32,
    /// `BitMap` nativo → sus píxeles.
    pub lienzos: Tabla<Lienzo>,
    /// `ColorMap` → sus colores ARGB.
    pub paletas: Tabla<Vec<u32>>,
    /// Plumas reservadas con `ObtainPen` en cada `ColorMap`.
    pub plumas: Tabla<BTreeSet<u8>>,
    /// El `ColorMap` del Workbench (el de los lienzos sin paleta).
    pub cm_wb: u32,
    /// Las capas de las ventanas: `Layer` → su recorte, en coordenadas de su mapa.
    pub capas: Tabla<Rect>,
    /// Las capas cuyo (0, 0) no es el del mapa: la interior de una ventana `WFLG_GIMMEZEROZERO`.
    pub origenes: Tabla<(i32, i32)>,
    /// Lo reservado por `AllocBitMap`: `BitMap` → bloques (dirección, largo), la estructura incluida.
    pub reservados: Tabla<Vec<(u32, u32)>>,
}

/// Lo que se lee de un `RastPort`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct InfoRp {
    pub capa: u32,
    pub bm: u32,
    pub fg: u8,
    pub bg: u8,
    pub modo: u8,
    pub mascara: u8,
    pub flags: u16,
    pub patron: u16,
    pub cnt: u8,
    pub area: u32,
    pub area_sz: u8,
    pub x: i32,
    pub y: i32,
    pub fuente: u32,
    pub espacio: i32,
}

/// Un mapa ya comprobado.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Mapa {
    Nativo,
    Planar(Planar),
}

/// Una fuente leída de su `struct TextFont`.
struct Fuente {
    alto: u32,
    ancho: u32,
    base: u32,
    lo: u32,
    hi: u32,
    datos: u32,
    modulo: u32,
    loc: u32,
    espacio: u32,
    kern: u32,
}

/// La paleta de arranque del Workbench: los cuatro colores de 3.1 (gris, negro, blanco, azul), cuatro más para
/// las plumas 4-7 y, detrás, un cubo de 6 x 6 x 6 y una rampa de grises. Provisional.
fn paleta_inicial() -> Vec<u32> {
    let mut p = vec![0u32; 256];
    let primeros = [0xAAAAAA, 0x000000, 0xFFFFFF, 0x6688BB, 0xEE4444, 0x55BB55, 0x3355DD, 0xEEAA22];
    p[..8].copy_from_slice(&primeros);
    let mut i = 8;
    for r in 0..6u32 {
        for g in 0..6u32 {
            for b in 0..6u32 {
                p[i] = ((r * 51) << 16) | ((g * 51) << 8) | (b * 51);
                i += 1;
            }
        }
    }
    let mut k = 0;
    while i < 256 {
        let v = k * 255 / 31;
        p[i] = v << 16 | v << 8 | v;
        i += 1;
        k += 1;
    }
    p
}

fn rgb4(r: u32, g: u32, b: u32) -> u32 {
    (((r & 15) * 0x11) << 16) | (((g & 15) * 0x11) << 8) | ((b & 15) * 0x11)
}

fn rgb32(r: u32, g: u32, b: u32) -> u32 {
    (r >> 24) << 16 | (g >> 24) << 8 | b >> 24
}

/// Aplica un minitérmino de blitter (con A siempre a 1, como en `BltBitMap`) bit a bit: B es el origen, C el
/// destino. 0xC0 copia, 0x30 copia invertido, 0x50 invierte el destino, 0x60 es O exclusivo, 0x80 Y, 0xE0 O.
pub(crate) fn minterm(m: u8, b: u32, c: u32) -> u32 {
    let t = |bit: u8| if m & (1 << bit) != 0 { u32::MAX } else { 0 };
    (t(7) & b & c) | (t(6) & b & !c) | (t(5) & !b & c) | (t(4) & !b & !c)
}

impl<C: Compositor> CajaGrafica<C> {
    // --- Instalación ---

    pub(crate) fn instalar_graphics<M: Memoria>(&mut self, ex: &mut Exec<M>) -> Result<(), Fallo> {
        let numeros: Vec<u16> = (0..lvo::GRAPHICS.len() as u16).map(|i| self.numeros.graphics + i).collect();
        let base = ex.crear_biblioteca_nativa("graphics.library", 40, 1, &numeros, gfxbase::TAMANO)?;
        self.gfx.base = base;
        let topaz = fuente::construir(ex)?;
        self.gfx.topaz = topaz;
        ex.new_list(base + gfxbase::TEXTFONTS, nt::FONT)?;
        ex.add_tail(base + gfxbase::TEXTFONTS, topaz)?;
        ex.e32(base + gfxbase::DEFAULTFONT, topaz)?;
        ex.e16(base + gfxbase::DISPLAYFLAGS, gfxbase::PAL)?;
        ex.e16(base + gfxbase::VBLANK, 50)?;
        ex.e16(base + gfxbase::NORMALDISPLAYROWS, 256)?;
        ex.e16(base + gfxbase::NORMALDISPLAYCOLUMNS, 640)?;
        ex.e8(
            base + gfxbase::CHIPREVBITS0,
            gfxbase::GFXF_HR_AGNUS | gfxbase::GFXF_HR_DENISE | gfxbase::GFXF_AA_ALICE | gfxbase::GFXF_AA_LISA,
        )?;
        ex.e8(base + gfxbase::MEMTYPE, 3)?;
        let cm = self.get_color_map(ex, 256)?;
        self.gfx.cm_wb = cm;
        for (i, c) in paleta_inicial().into_iter().enumerate() {
            self.poner_color(ex, cm, i as u32, c)?;
        }
        Ok(())
    }

    // --- Lo que comparten las funciones ---

    /// El color ARGB de una pluma en un `ColorMap` (0: el del Workbench). Negro si no existe.
    pub(crate) fn color(&self, cm: u32, pen: u8) -> u32 {
        let cm = if cm == 0 { self.gfx.cm_wb } else { cm };
        self.gfx.paletas.get(&cm).and_then(|p| p.get(pen as usize)).copied().unwrap_or(0)
    }

    fn paleta(&self, cm: u32) -> &[u32] {
        let cm = if cm == 0 { self.gfx.cm_wb } else { cm };
        self.gfx.paletas.get(&cm).map(|p| p.as_slice()).unwrap_or(&[])
    }

    pub(crate) fn leer_rp<M: Memoria>(&self, ex: &Exec<M>, rp: u32) -> Result<InfoRp, Fallo> {
        if rp == 0 {
            return Err(Fallo::RastPortMalo { dir: 0 });
        }
        ex.tramo(rp, rp_::TAMANO).map_err(|_| Fallo::RastPortMalo { dir: rp })?;
        Ok(InfoRp {
            capa: ex.l32(rp + rp_::LAYER)?,
            bm: ex.l32(rp + rp_::BITMAP)?,
            fg: ex.l8(rp + rp_::FGPEN)?,
            bg: ex.l8(rp + rp_::BGPEN)?,
            modo: ex.l8(rp + rp_::DRAWMODE)?,
            mascara: ex.l8(rp + rp_::MASK)?,
            flags: ex.l16(rp + rp_::FLAGS)?,
            patron: ex.l16(rp + rp_::LINEPTRN)?,
            cnt: ex.l8(rp + rp_::LINPATCNT)?,
            area: ex.l32(rp + rp_::AREAPTRN)?,
            area_sz: ex.l8(rp + rp_::AREAPTSZ)?,
            x: ex.l16(rp + rp_::CP_X)? as i16 as i32,
            y: ex.l16(rp + rp_::CP_Y)? as i16 as i32,
            fuente: ex.l32(rp + rp_::FONT)?,
            espacio: ex.l16(rp + rp_::TXSPACING)? as i16 as i32,
        })
    }

    /// Comprueba un `BitMap`: nativo, o planar con sus planos en memoria.
    pub(crate) fn mapa<M: Memoria>(&self, ex: &Exec<M>, bm: u32) -> Result<Mapa, Fallo> {
        if bm == 0 {
            return Err(Fallo::BitMapMalo { dir: 0 });
        }
        if self.gfx.lienzos.contains_key(&bm) {
            return Ok(Mapa::Nativo);
        }
        ex.tramo(bm, bitmap::TAMANO).map_err(|_| Fallo::BitMapMalo { dir: bm })?;
        let bpr = ex.l16(bm + bitmap::BYTESPERROW)? as u32;
        let filas = ex.l16(bm + bitmap::ROWS)? as u32;
        let prof = ex.l8(bm + bitmap::DEPTH)?;
        if prof as u32 > bitmap::MAX_PLANOS {
            return Err(Fallo::BitMapMalo { dir: bm });
        }
        let mut planos = [0u32; 8];
        for (i, p) in planos.iter_mut().enumerate() {
            *p = ex.l32(bm + bitmap::PLANES + 4 * i as u32)?;
        }
        // Entrelazado: los planos de una fila van seguidos, y `BytesPerRow` es la fila de todos.
        let entrelazado = prof > 1
            && bpr.is_multiple_of(prof as u32)
            && (1..prof as usize).all(|i| planos[i] == planos[0].wrapping_add(i as u32 * (bpr / prof as u32)));
        let fila = if entrelazado { bpr / prof as u32 } else { bpr };
        // Sin ningún plano de verdad (todos 0 o todo unos) no hay dónde escribir: se trata como vacío, para no
        // recorrer un mapa enorme que no existe.
        let reales = planos.iter().take(prof as usize).any(|&p| p != 0 && p != PLANO_UNOS);
        if fila == 0 || filas == 0 || !reales {
            return Ok(Mapa::Planar(Planar { planos, prof: 0, bpr, filas: 0, ancho: 0 }));
        }
        let largo = (filas as u64 - 1) * bpr as u64 + fila as u64;
        for &p in planos.iter().take(prof as usize) {
            if p == 0 || p == PLANO_UNOS {
                continue;
            }
            let largo = u32::try_from(largo).map_err(|_| Fallo::BitMapMalo { dir: bm })?;
            ex.tramo(p, largo).map_err(|_| Fallo::BitMapMalo { dir: bm })?;
        }
        Ok(Mapa::Planar(Planar { planos, prof, bpr, filas, ancho: fila * 8 }))
    }

    /// El recorte de un `RastPort`: su capa, si es de una ventana.
    pub(crate) fn recorte_capa(&self, capa: u32) -> Option<Rect> {
        if capa == 0 { None } else { self.gfx.capas.get(&capa).copied() }
    }

    /// La región de recorte (`InstallClipRegion`) de una capa de graphics (la de una ventana o una suelta), en bandas y
    /// en las coordenadas de la capa (las del `RastPort`). `None` si no tiene; una región que no se puede leer es un
    /// fallo (no se pinta nada).
    pub(crate) fn region_de_capa<M: Memoria>(&self, ex: &Exec<M>, capa: u32) -> Result<Option<Vec<Rect>>, Fallo> {
        if capa == 0 || !self.gfx.capas.contains_key(&capa) {
            return Ok(None);
        }
        let reg = ex.l32(capa + crate::estructuras::layer::CLIPREGION)?;
        if reg == 0 {
            return Ok(None);
        }
        let (rs, _) = self.leer_region(ex, reg)?;
        Ok(Some(crate::regiones::operar(&rs, &[], crate::regiones::Op::O).ok_or(Fallo::Sospechoso { dir: reg })?))
    }

    /// La región de recorte de una capa, en las coordenadas de su mapa (la de la capa movida a su origen `(dx, dy)`).
    fn region_en_mapa<M: Memoria>(&self, ex: &Exec<M>, capa: u32, dx: i32, dy: i32) -> Result<Option<Vec<Rect>>, Fallo> {
        Ok(self.region_de_capa(ex, capa)?.map(|v| {
            v.into_iter().map(|q| Rect::nuevo(q.x0 + dx, q.y0 + dy, q.x1 + dx, q.y1 + dy)).collect()
        }))
    }

    /// Dónde cae el (0, 0) de un `RastPort` en su mapa (el de su capa; (0, 0) casi siempre).
    pub(crate) fn origen_capa(&self, capa: u32) -> (i32, i32) {
        if capa == 0 { (0, 0) } else { self.gfx.origenes.get(&capa).copied().unwrap_or((0, 0)) }
    }

    /// Un pintor sobre el mapa de un `RastPort`, con sus plumas, su modo y su recorte.
    pub(crate) fn pintor<'a, M: Memoria>(
        &'a mut self,
        ex: &'a mut Exec<M>,
        i: &InfoRp,
    ) -> Result<Pintor<'a, M>, Fallo> {
        let mapa = self.mapa(ex, i.bm)?;
        let capa = self.recorte_capa(i.capa);
        let (dx, dy) = self.origen_capa(i.capa);
        let region = self.region_de_capa(ex, i.capa)?;
        let mut p = self.pintor_mapa(ex, i.bm, mapa, capa, i.fg, i.bg, i.modo, i.mascara)?;
        if (dx, dy) != (0, 0) {
            p.trasladar(dx, dy);
        }
        if let Some(r) = region {
            p.recortar_con(r);
        }
        Ok(p)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn pintor_mapa<'a, M: Memoria>(
        &'a mut self,
        ex: &'a mut Exec<M>,
        bm: u32,
        mapa: Mapa,
        capa: Option<Rect>,
        fg: u8,
        bg: u8,
        modo: u8,
        mascara: u8,
    ) -> Result<Pintor<'a, M>, Fallo> {
        match mapa {
            Mapa::Nativo => {
                let cm = self.gfx.lienzos.get(&bm).map(|l| l.paleta).unwrap_or(0);
                let a = Tinta { pen: fg, argb: self.color(cm, fg) };
                let b = Tinta { pen: bg, argb: self.color(cm, bg) };
                let l = self.gfx.lienzos.get_mut(&bm).ok_or(Fallo::BitMapMalo { dir: bm })?;
                let mut clip = l.rect();
                if let Some(c) = capa {
                    clip = clip.interseccion(&c);
                }
                Ok(Pintor { sup: Sup::Nativa(l), clip, region: None, a, b, modo, mascara, dx: 0, dy: 0 })
            }
            Mapa::Planar(p) => {
                let mut clip = p.rect();
                if let Some(c) = capa {
                    clip = clip.interseccion(&c);
                }
                let a = Tinta { pen: fg, argb: 0 };
                let b = Tinta { pen: bg, argb: 0 };
                Ok(Pintor { sup: Sup::Planar(&mut ex.mem, p), clip, region: None, a, b, modo, mascara, dx: 0, dy: 0 })
            }
        }
    }

    // --- RastPort ---

    /// `InitRastPort`: todo a cero salvo `Mask` = 0xFF, `FgPen` = `AOlPen` = 0xFF, `LinePtrn` = 0xFFFF,
    /// `DrawMode` = JAM2 y la fuente por defecto.
    pub fn init_rast_port<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32) -> Result<(), Fallo> {
        if rp == 0 {
            return Err(Fallo::RastPortMalo { dir: 0 });
        }
        ex.cero(rp, rp_::TAMANO).map_err(|_| Fallo::RastPortMalo { dir: rp })?;
        ex.e8(rp + rp_::MASK, 0xFF)?;
        ex.e8(rp + rp_::FGPEN, 0xFF)?;
        ex.e8(rp + rp_::AOLPEN, 0xFF)?;
        ex.e8(rp + rp_::DRAWMODE, drmd::JAM2)?;
        ex.e16(rp + rp_::LINEPTRN, 0xFFFF)?;
        ex.e8(rp + rp_::LINPATCNT, 15)?;
        ex.e16(rp + rp_::FLAGS, rp_::FRST_DOT)?;
        ex.e16(rp + rp_::PENWIDTH, 1)?;
        ex.e16(rp + rp_::PENHEIGHT, 1)?;
        let f = self.gfx.topaz;
        self.set_font(ex, rp, f)
    }

    pub fn set_a_pen<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, pen: u32) -> Result<(), Fallo> {
        self.leer_rp(ex, rp)?;
        ex.e8(rp + rp_::FGPEN, pen as u8)
    }

    pub fn set_b_pen<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, pen: u32) -> Result<(), Fallo> {
        self.leer_rp(ex, rp)?;
        ex.e8(rp + rp_::BGPEN, pen as u8)
    }

    pub fn set_dr_md<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, modo: u32) -> Result<(), Fallo> {
        self.leer_rp(ex, rp)?;
        ex.e8(rp + rp_::DRAWMODE, modo as u8)
    }

    /// `SetOutlinePen`: devuelve la de antes.
    pub fn set_outline_pen<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, pen: u32) -> Result<u32, Fallo> {
        self.leer_rp(ex, rp)?;
        let antes = ex.l8(rp + rp_::AOLPEN)? as u32;
        ex.e8(rp + rp_::AOLPEN, pen as u8)?;
        let f = ex.l16(rp + rp_::FLAGS)?;
        ex.e16(rp + rp_::FLAGS, f | rp_::AREAOUTLINE)?;
        Ok(antes)
    }

    /// `SetWriteMask`: siempre se puede (devuelve TRUE).
    pub fn set_write_mask<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, mascara: u32) -> Result<u32, Fallo> {
        self.leer_rp(ex, rp)?;
        ex.e8(rp + rp_::MASK, mascara as u8)?;
        Ok(1)
    }

    /// `Move`: la posición actual, y el primer punto de la próxima línea se dibuja.
    pub fn mover<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, x: i32, y: i32) -> Result<(), Fallo> {
        let i = self.leer_rp(ex, rp)?;
        ex.e16(rp + rp_::CP_X, x as u16)?;
        ex.e16(rp + rp_::CP_Y, y as u16)?;
        ex.e16(rp + rp_::FLAGS, i.flags | rp_::FRST_DOT)?;
        ex.e8(rp + rp_::LINPATCNT, 15)
    }

    /// `Draw`: una línea desde la posición actual hasta `(x, y)`, con el patrón de línea, y la posición pasa a
    /// `(x, y)`. En COMPLEMENT, el primer punto de una línea encadenada (sin `Move` antes) no se dibuja, para no
    /// invertir dos veces la esquina (provisional: a comprobar con el oráculo).
    pub fn draw<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, x: i32, y: i32) -> Result<(), Fallo> {
        let i = self.leer_rp(ex, rp)?;
        let sin_primero = i.modo & drmd::COMPLEMENT != 0 && i.flags & rp_::FRST_DOT == 0;
        let mut cnt = i.cnt & 15;
        {
            let mut p = self.pintor(ex, &i)?;
            p.linea(i.x, i.y, x, y, i.patron, &mut cnt, sin_primero);
        }
        ex.e16(rp + rp_::CP_X, x as u16)?;
        ex.e16(rp + rp_::CP_Y, y as u16)?;
        ex.e8(rp + rp_::LINPATCNT, cnt)?;
        ex.e16(rp + rp_::FLAGS, i.flags & !rp_::FRST_DOT)
    }

    /// `PolyDraw`: `Draw` a cada punto de una tabla de pares de WORD.
    pub fn poly_draw<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, n: u32, tabla: u32) -> Result<(), Fallo> {
        let n = n as u16 as i16;
        if n <= 0 {
            return Ok(());
        }
        let puntos = ex.bytes(tabla, n as u32 * 4)?;
        for c in puntos.chunks(4) {
            let x = i16::from_be_bytes([c[0], c[1]]) as i32;
            let y = i16::from_be_bytes([c[2], c[3]]) as i32;
            self.draw(ex, rp, x, y)?;
        }
        Ok(())
    }

    /// El patrón de área de un `RastPort` (`AreaPtrn`, `2^AreaPtSz` filas), o vacío.
    pub(crate) fn patron_area<M: Memoria>(&self, ex: &Exec<M>, i: &InfoRp) -> Result<Vec<u16>, Fallo> {
        if i.area == 0 || i.area_sz > 8 {
            return Ok(Vec::new());
        }
        let filas = 1u32 << i.area_sz;
        let b = ex.bytes(i.area, filas * 2)?;
        Ok(b.chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect())
    }

    /// `RectFill`: con la pluma A (y el patrón de área, si lo hay). Un rectángulo al revés no dibuja nada.
    pub fn rect_fill<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        rp: u32,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
    ) -> Result<(), Fallo> {
        let i = self.leer_rp(ex, rp)?;
        if x1 < x0 || y1 < y0 {
            return Ok(());
        }
        let patron = self.patron_area(ex, &i)?;
        let mut p = self.pintor(ex, &i)?;
        p.rectangulo(Rect::nuevo(x0, y0, x1, y1), &patron);
        Ok(())
    }

    /// `WritePixel`: 0 si lo ha puesto, -1 si cae fuera.
    pub fn write_pixel<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, x: i32, y: i32) -> Result<i32, Fallo> {
        let i = self.leer_rp(ex, rp)?;
        let mut p = self.pintor(ex, &i)?;
        if !p.clip.contiene(x, y) {
            return Ok(-1);
        }
        p.punto(x, y, true);
        Ok(0)
    }

    /// `ReadPixel`: la pluma del punto (en un mapa nativo, la del color más parecido de su paleta), o -1 fuera.
    pub fn read_pixel<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, x: i32, y: i32) -> Result<i32, Fallo> {
        let i = self.leer_rp(ex, rp)?;
        let mapa = self.mapa(ex, i.bm)?;
        let mut clip = match mapa {
            Mapa::Nativo => self.gfx.lienzos[&i.bm].rect(),
            Mapa::Planar(p) => p.rect(),
        };
        if let Some(c) = self.recorte_capa(i.capa) {
            clip = clip.interseccion(&c);
        }
        let (dx, dy) = self.origen_capa(i.capa);
        let (x, y) = (x.saturating_add(dx), y.saturating_add(dy));
        if !clip.contiene(x, y) {
            return Ok(-1);
        }
        Ok(match mapa {
            Mapa::Nativo => {
                let l = &self.gfx.lienzos[&i.bm];
                mas_cercano(self.paleta(l.paleta), l.pixel(x, y)) as i32
            }
            Mapa::Planar(p) => p.leer(&ex.mem, x, y) as i32,
        })
    }

    /// `SetRast`: todo el mapa (o la capa) de una pluma, sin modo de dibujo.
    pub fn set_rast<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, pen: u32) -> Result<(), Fallo> {
        let i = self.leer_rp(ex, rp)?;
        let cm = self.gfx.lienzos.get(&i.bm).map(|l| l.paleta).unwrap_or(0);
        let t = Tinta { pen: pen as u8, argb: self.color(cm, pen as u8) };
        let mut p = self.pintor(ex, &i)?;
        let r = p.clip;
        for y in r.y0..=r.y1 {
            for x in r.x0..=r.x1 {
                p.tinta(x, y, t);
            }
        }
        Ok(())
    }

    /// `ScrollRaster`: mueve el contenido de `(x0, y0)-(x1, y1)` `dx` a la izquierda y `dy` hacia arriba, y llena
    /// lo que queda al descubierto con la pluma B.
    #[allow(clippy::too_many_arguments)]
    pub fn scroll_raster<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        rp: u32,
        dx: i32,
        dy: i32,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
    ) -> Result<(), Fallo> {
        let i = self.leer_rp(ex, rp)?;
        let mut p = self.pintor(ex, &i)?;
        let r = Rect::nuevo(x0, y0, x1, y1).interseccion(&p.clip);
        if r.vacio() {
            return Ok(());
        }
        let (an, al) = (r.ancho() as usize, r.alto() as usize);
        let mut copia = Vec::with_capacity(an * al);
        for y in r.y0..=r.y1 {
            for x in r.x0..=r.x1 {
                copia.push(p.leer(x, y));
            }
        }
        let b = p.b;
        for y in r.y0..=r.y1 {
            for x in r.x0..=r.x1 {
                let (sx, sy) = (x + dx, y + dy);
                if r.contiene(sx, sy) {
                    let v = copia[(sy - r.y0) as usize * an + (sx - r.x0) as usize];
                    p.crudo(x, y, v);
                } else {
                    p.tinta(x, y, b);
                }
            }
        }
        Ok(())
    }

    /// `ClearEOL`: de la posición actual al borde derecho, la altura de la línea de texto, con la pluma B.
    pub fn clear_eol<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32) -> Result<(), Fallo> {
        let i = self.leer_rp(ex, rp)?;
        let alto = ex.l16(rp + rp_::TXHEIGHT)? as i32;
        let base = ex.l16(rp + rp_::TXBASELINE)? as i32;
        let mut p = self.pintor(ex, &i)?;
        let r = Rect::nuevo(i.x, i.y - base, p.clip.x1, i.y - base + alto - 1).interseccion(&p.clip);
        let b = p.b;
        for y in r.y0..=r.y1 {
            for x in r.x0..=r.x1 {
                p.tinta(x, y, b);
            }
        }
        Ok(())
    }

    /// `ClearScreen`: `ClearEOL` y todo lo que hay debajo de la línea.
    pub fn clear_screen<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32) -> Result<(), Fallo> {
        self.clear_eol(ex, rp)?;
        let i = self.leer_rp(ex, rp)?;
        let alto = ex.l16(rp + rp_::TXHEIGHT)? as i32;
        let base = ex.l16(rp + rp_::TXBASELINE)? as i32;
        let mut p = self.pintor(ex, &i)?;
        let r = Rect::nuevo(p.clip.x0, i.y - base + alto, p.clip.x1, p.clip.y1).interseccion(&p.clip);
        let b = p.b;
        for y in r.y0..=r.y1 {
            for x in r.x0..=r.x1 {
                p.tinta(x, y, b);
            }
        }
        Ok(())
    }

    // --- Copias de bloques ---

    /// Lee un rectángulo de un mapa como valores crudos (ARGB en los nativos, plumas en los planares).
    fn leer_rect<M: Memoria>(&self, ex: &Exec<M>, bm: u32, mapa: Mapa, r: Rect) -> Vec<u32> {
        let mut v = Vec::with_capacity(r.ancho() as usize * r.alto() as usize);
        for y in r.y0..=r.y1 {
            for x in r.x0..=r.x1 {
                v.push(match mapa {
                    Mapa::Nativo => self.gfx.lienzos.get(&bm).map(|l| l.pixel(x, y)).unwrap_or(0),
                    Mapa::Planar(p) => p.leer(&ex.mem, x, y) as u32,
                });
            }
        }
        v
    }

    fn limites(&self, bm: u32, mapa: Mapa) -> Rect {
        match mapa {
            Mapa::Nativo => self.gfx.lienzos.get(&bm).map(|l| l.rect()).unwrap_or(Rect::nuevo(0, 0, -1, -1)),
            Mapa::Planar(p) => p.rect(),
        }
    }

    /// El corazón de `BltBitMap`, `ClipBlit`, `BltBitMapRastPort` y `BltMaskBitMapRastPort`: copia `ancho` x `alto` de
    /// un mapa a otro con un minitérmino, recortando a los dos (y a `capa` en el destino). Entre un mapa planar y uno
    /// nativo, las plumas pasan por la paleta del nativo. Con `plantilla` (un plano de un bit del ancho y el alto del
    /// origen, con filas de `RASSIZE`), solo se tocan los puntos con su bit a 1. Devuelve los planos copiados.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn copiar<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        origen: u32,
        xs: i32,
        ys: i32,
        destino: u32,
        xd: i32,
        yd: i32,
        ancho: i32,
        alto: i32,
        m: u8,
        mascara: u8,
        capa: Option<Rect>,
        plantilla: Option<u32>,
        region: Option<Vec<Rect>>,
    ) -> Result<u32, Fallo> {
        let mo = self.mapa(ex, origen)?;
        let md = self.mapa(ex, destino)?;
        if ancho <= 0 || alto <= 0 {
            return Ok(0);
        }
        let mut lim_d = self.limites(destino, md);
        if let Some(c) = capa {
            lim_d = lim_d.interseccion(&c);
        }
        let lim_o = self.limites(origen, mo);
        // Recorte: el rectángulo de destino, movido al origen, dentro de los dos.
        let (dx, dy) = (xd - xs, yd - ys);
        let ro = Rect::nuevo(xs, ys, xs + ancho - 1, ys + alto - 1).interseccion(&lim_o).interseccion(&Rect::nuevo(
            lim_d.x0 - dx,
            lim_d.y0 - dy,
            lim_d.x1 - dx,
            lim_d.y1 - dy,
        ));
        if ro.vacio() {
            return Ok(0);
        }
        let datos = self.leer_rect(ex, origen, mo, ro);
        // Las filas de la plantilla que caen en lo copiado (en coordenadas del origen), leídas antes de pintar.
        let filas_plantilla = match plantilla {
            Some(pl) => {
                let por_fila = (lim_o.ancho()).div_ceil(16) * 2;
                let mut v = Vec::with_capacity(ro.alto() as usize);
                for y in ro.y0..=ro.y1 {
                    let d = (pl as u64) + (y - lim_o.y0) as u64 * por_fila as u64 + (ro.x0 - lim_o.x0) as u64 / 8;
                    let d = u32::try_from(d).map_err(|_| caja_exec::Fallo::FueraDeMemoria { dir: pl, largo: 0 })?;
                    let ultimo = (ro.x1 - lim_o.x0) as u32 / 8 - (ro.x0 - lim_o.x0) as u32 / 8;
                    v.push(ex.bytes(d, ultimo + 1)?);
                }
                Some(v)
            }
            None => None,
        };
        let cm_o = self.gfx.lienzos.get(&origen).map(|l| l.paleta).unwrap_or(0);
        let cm_d = self.gfx.lienzos.get(&destino).map(|l| l.paleta).unwrap_or(0);
        // Traducir al tipo del destino.
        let datos: Vec<u32> = match (mo, md) {
            (Mapa::Planar(_), Mapa::Nativo) => datos.iter().map(|&v| self.color(cm_d, v as u8)).collect(),
            (Mapa::Nativo, Mapa::Planar(_)) => {
                let pal = self.paleta(cm_o);
                datos.iter().map(|&v| mas_cercano(pal, v) as u32).collect()
            }
            _ => datos,
        };
        let planos = match md {
            Mapa::Nativo => 8,
            Mapa::Planar(p) => p.prof as u32,
        };
        let mut p = self.pintor_mapa(ex, destino, md, capa, 0, 0, drmd::JAM2, mascara)?;
        if let Some(r) = region {
            p.recortar_con(r);
        }
        let an = ro.ancho() as usize;
        let nativo = matches!(md, Mapa::Nativo);
        for y in ro.y0..=ro.y1 {
            for x in ro.x0..=ro.x1 {
                let b = datos[(y - ro.y0) as usize * an + (x - ro.x0) as usize];
                let (tx, ty) = (x + dx, y + dy);
                if let Some(f) = &filas_plantilla {
                    // El bit de la plantilla; con ANBC, el origen invertido (en un nativo, el color sin el alfa).
                    let k = (x - lim_o.x0) as u32 - ((ro.x0 - lim_o.x0) as u32 & !7);
                    if f[(y - ro.y0) as usize][(k / 8) as usize] & (0x80 >> (k % 8)) == 0 {
                        continue;
                    }
                    let v = if m & 0xC0 == 0 {
                        if nativo { b ^ 0x00FF_FFFF } else { !b & 0xFF }
                    } else {
                        b
                    };
                    p.crudo(tx, ty, v);
                    continue;
                }
                let v = if m == 0xC0 {
                    b
                } else {
                    let c = p.leer(tx, ty);
                    minterm(m, b, c)
                };
                p.crudo(tx, ty, v);
            }
        }
        Ok(planos)
    }

    /// `BltBitMap`: devuelve los planos copiados.
    #[allow(clippy::too_many_arguments)]
    pub fn blt_bit_map<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        origen: u32,
        xs: i32,
        ys: i32,
        destino: u32,
        xd: i32,
        yd: i32,
        ancho: i32,
        alto: i32,
        m: u8,
        mascara: u8,
    ) -> Result<u32, Fallo> {
        self.copiar(ex, origen, xs, ys, destino, xd, yd, ancho, alto, m, mascara, None, None, None)
    }

    /// `ClipBlit` y `BltBitMapRastPort` (con `origen_rp` = 0 y `origen` el mapa).
    #[allow(clippy::too_many_arguments)]
    pub fn clip_blit<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        origen: u32,
        origen_es_rp: bool,
        xs: i32,
        ys: i32,
        destino_rp: u32,
        xd: i32,
        yd: i32,
        ancho: i32,
        alto: i32,
        m: u8,
    ) -> Result<(), Fallo> {
        let (bm_o, (ox, oy)) = if origen_es_rp {
            let o = self.leer_rp(ex, origen)?;
            (o.bm, self.origen_capa(o.capa))
        } else {
            (origen, (0, 0))
        };
        let d = self.leer_rp(ex, destino_rp)?;
        let capa = self.recorte_capa(d.capa);
        let (dx, dy) = self.origen_capa(d.capa);
        let region = self.region_en_mapa(ex, d.capa, dx, dy)?;
        let (xs, ys) = (xs.saturating_add(ox), ys.saturating_add(oy));
        let (xd, yd) = (xd.saturating_add(dx), yd.saturating_add(dy));
        self.copiar(ex, bm_o, xs, ys, d.bm, xd, yd, ancho, alto, m, d.mascara, capa, None, region)?;
        Ok(())
    }

    /// `BltMaskBitMapRastPort`: como `BltBitMapRastPort`, pero solo donde la plantilla (`bltmask`) tiene su bit a 1;
    /// con (ABC|ABNC|ANBC) el origen tal cual, con ANBC invertido.
    #[allow(clippy::too_many_arguments)]
    pub fn blt_mask_bit_map_rast_port<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        origen: u32,
        xs: i32,
        ys: i32,
        destino_rp: u32,
        xd: i32,
        yd: i32,
        ancho: i32,
        alto: i32,
        m: u8,
        plantilla: u32,
    ) -> Result<(), Fallo> {
        if plantilla == 0 {
            return Err(caja_exec::Fallo::PunteroNulo.into());
        }
        let d = self.leer_rp(ex, destino_rp)?;
        let capa = self.recorte_capa(d.capa);
        let (dx, dy) = self.origen_capa(d.capa);
        let region = self.region_en_mapa(ex, d.capa, dx, dy)?;
        let (xd, yd) = (xd.saturating_add(dx), yd.saturating_add(dy));
        self.copiar(ex, origen, xs, ys, d.bm, xd, yd, ancho, alto, m, d.mascara, capa, Some(plantilla), region)?;
        Ok(())
    }

    /// `BltTemplate`: una plantilla de un bit (`srcMod` bytes por fila, empezando en el bit `xSrc`) con las plumas
    /// y el modo del `RastPort`.
    #[allow(clippy::too_many_arguments)]
    pub fn blt_template<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        fuente: u32,
        x_src: u32,
        modulo: i32,
        rp: u32,
        x: i32,
        y: i32,
        ancho: i32,
        alto: i32,
    ) -> Result<(), Fallo> {
        let i = self.leer_rp(ex, rp)?;
        if ancho <= 0 || alto <= 0 {
            return Ok(());
        }
        let x_src = x_src & 0xFFFF;
        let por_fila = (x_src + ancho as u32).div_ceil(8);
        let mut filas = Vec::with_capacity(alto as usize);
        for f in 0..alto {
            let d = (fuente as i64) + f as i64 * modulo as i64;
            let d = u32::try_from(d).map_err(|_| caja_exec::Fallo::FueraDeMemoria { dir: fuente, largo: 0 })?;
            filas.push(ex.bytes(d, por_fila)?);
        }
        let mut p = self.pintor(ex, &i)?;
        p.plantilla(x, y, ancho as u32, alto as u32, |c, f| {
            let b = x_src + c;
            filas[f as usize][(b >> 3) as usize] & (0x80 >> (b & 7)) != 0
        });
        Ok(())
    }

    /// `BltClear`: pone a cero `n` bytes (o `filas << 16 | bytes por fila` si `flags` lleva el bit 1).
    pub fn blt_clear<M: Memoria>(&mut self, ex: &mut Exec<M>, d: u32, n: u32, flags: u32) -> Result<(), Fallo> {
        let largo = if flags & 2 != 0 { (n >> 16) * (n & 0xFFFF) } else { n };
        if largo == 0 {
            return Ok(());
        }
        ex.cero(d, largo)
    }

    /// `WriteChunkyPixels`: una pluma por byte, fila a fila, sin modo de dibujo.
    #[allow(clippy::too_many_arguments)]
    pub fn write_chunky_pixels<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        rp: u32,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        tabla: u32,
        bpr: i32,
    ) -> Result<(), Fallo> {
        let i = self.leer_rp(ex, rp)?;
        if x1 < x0 || y1 < y0 {
            return Ok(());
        }
        let an = (x1 - x0 + 1) as u32;
        let mut filas = Vec::new();
        for f in 0..=(y1 - y0) {
            let d = tabla as i64 + f as i64 * bpr as i64;
            let d = u32::try_from(d).map_err(|_| caja_exec::Fallo::FueraDeMemoria { dir: tabla, largo: 0 })?;
            filas.push(ex.bytes(d, an)?);
        }
        let cm = self.gfx.lienzos.get(&i.bm).map(|l| l.paleta).unwrap_or(0);
        let colores: Vec<u32> = (0..=255u8).map(|p| self.color(cm, p)).collect();
        let mut p = self.pintor(ex, &i)?;
        p.mascara = 0xFF;
        for (f, fila) in filas.iter().enumerate() {
            for (c, &pen) in fila.iter().enumerate() {
                p.tinta(x0 + c as i32, y0 + f as i32, Tinta { pen, argb: colores[pen as usize] });
            }
        }
        Ok(())
    }

    // --- Texto y fuentes ---

    fn leer_fuente<M: Memoria>(&self, ex: &Exec<M>, f: u32) -> Result<Fuente, Fallo> {
        if f == 0 {
            return Err(Fallo::FuenteMala { dir: 0 });
        }
        ex.tramo(f, tf::TAMANO).map_err(|_| Fallo::FuenteMala { dir: f })?;
        let lo = ex.l8(f + tf::TF_LOCHAR)? as u32;
        let hi = ex.l8(f + tf::TF_HICHAR)? as u32;
        let flags = ex.l8(f + tf::TF_FLAGS)?;
        let fu = Fuente {
            alto: ex.l16(f + tf::TF_YSIZE)? as u32,
            ancho: ex.l16(f + tf::TF_XSIZE)? as u32,
            base: ex.l16(f + tf::TF_BASELINE)? as u32,
            lo,
            hi,
            datos: ex.l32(f + tf::TF_CHARDATA)?,
            modulo: ex.l16(f + tf::TF_MODULO)? as u32,
            loc: ex.l32(f + tf::TF_CHARLOC)?,
            espacio: if flags & tf::FPF_PROPORTIONAL != 0 { ex.l32(f + tf::TF_CHARSPACE)? } else { 0 },
            kern: ex.l32(f + tf::TF_CHARKERN)?,
        };
        if hi < lo || fu.alto == 0 || fu.alto > 256 {
            return Err(Fallo::FuenteMala { dir: f });
        }
        let n = hi - lo + 2;
        ex.tramo(fu.loc, n * 4).map_err(|_| Fallo::FuenteMala { dir: f })?;
        if fu.modulo > 0 {
            ex.tramo(fu.datos, fu.modulo * fu.alto).map_err(|_| Fallo::FuenteMala { dir: f })?;
        }
        if fu.espacio != 0 {
            ex.tramo(fu.espacio, n * 2).map_err(|_| Fallo::FuenteMala { dir: f })?;
        }
        if fu.kern != 0 {
            ex.tramo(fu.kern, n * 2).map_err(|_| Fallo::FuenteMala { dir: f })?;
        }
        Ok(fu)
    }

    /// El glifo de un carácter: (desplazamiento en bits, ancho, avance, interletraje).
    fn glifo<M: Memoria>(&self, ex: &Exec<M>, fu: &Fuente, c: u8) -> Result<(u32, u32, i32, i32), Fallo> {
        let c = c as u32;
        let i = if c >= fu.lo && c <= fu.hi { c - fu.lo } else { fu.hi - fu.lo + 1 };
        let loc = ex.l32(fu.loc + i * 4)?;
        let avance = if fu.espacio != 0 { ex.l16(fu.espacio + i * 2)? as i16 as i32 } else { fu.ancho as i32 };
        let kern = if fu.kern != 0 { ex.l16(fu.kern + i * 2)? as i16 as i32 } else { 0 };
        Ok((loc >> 16, loc & 0xFFFF, avance, kern))
    }

    /// La fuente de un `RastPort`; sin fuente, topaz (y un aviso).
    fn fuente_de<M: Memoria>(&mut self, ex: &Exec<M>, i: &InfoRp, funcion: &'static str) -> Result<Fuente, Fallo> {
        if i.fuente == 0 {
            self.avisar(ex, funcion, Fallo::FuenteMala { dir: 0 });
            return self.leer_fuente(ex, self.gfx.topaz);
        }
        self.leer_fuente(ex, i.fuente)
    }

    /// `TextLength`: el avance de `n` caracteres (con el interletraje y `TxSpacing`).
    pub fn text_length<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, s: u32, n: u32) -> Result<u32, Fallo> {
        let i = self.leer_rp(ex, rp)?;
        let n = n & 0xFFFF;
        if n == 0 {
            return Ok(0);
        }
        let fu = self.fuente_de(ex, &i, "TextLength")?;
        let texto = ex.bytes(s, n)?;
        let mut largo = 0i32;
        for c in texto {
            let (_, _, avance, kern) = self.glifo(ex, &fu, c)?;
            largo += avance + kern + i.espacio;
        }
        Ok(largo.clamp(0, 0x7FFF) as u32)
    }

    /// `GetDisplayInfoData` (`graphics/displayinfo.h`): lo que se sabe del modo de la pantalla de la caja (la del
    /// Workbench, del tamaño del escritorio, 8 bits por color): `DTAG_DISP`, `DTAG_DIMS`, `DTAG_MNTR` y `DTAG_NAME`, cada
    /// uno con su `QueryHeader`. Copia como mucho `largo` bytes en `buf` y devuelve cuántos; otra etiqueta, 0.
    pub fn get_display_info_data<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        buf: u32,
        largo: u32,
        tag: u32,
        id: u32,
    ) -> Result<u32, Fallo> {
        let (an, al) = self.comp.tamano_escritorio();
        let (an, al) = (an.min(0x7FFF) as u16, al.min(0x7FFF) as u16);
        let mut v: alloc::vec::Vec<u8> = match tag {
            0x8000_0000 => alloc::vec![0; 56],
            0x8000_1000 => alloc::vec![0; 88],
            0x8000_2000 => alloc::vec![0; 96],
            0x8000_3000 => alloc::vec![0; 56],
            _ => return Ok(0),
        };
        let pon16 = |v: &mut [u8], o: usize, x: u16| v[o..o + 2].copy_from_slice(&x.to_be_bytes());
        let pon32 = |v: &mut [u8], o: usize, x: u32| v[o..o + 4].copy_from_slice(&x.to_be_bytes());
        let n = v.len();
        pon32(&mut v, 0, tag);
        pon32(&mut v, 4, id);
        pon32(&mut v, 8, 3); // TAG_SKIP
        pon32(&mut v, 12, (n as u32 - 16) / 8);
        let rect = |v: &mut [u8], o: usize| {
            pon16(v, o + 4, an - 1);
            pon16(v, o + 6, al - 1);
        };
        match tag {
            0x8000_0000 => {
                pon32(&mut v, 18, 0x0000_0100 | 0x0000_0200); // DIPF_IS_WB | DIPF_IS_DRAGGABLE
                pon16(&mut v, 22, 22);
                pon16(&mut v, 24, 22);
                pon16(&mut v, 26, 35);
                pon16(&mut v, 28, 8);
                pon16(&mut v, 30, 4096);
                pon16(&mut v, 32, 22);
                pon16(&mut v, 34, 22);
                v[40] = 8;
                v[41] = 8;
                v[42] = 8;
            }
            0x8000_1000 => {
                pon16(&mut v, 16, 8);
                pon16(&mut v, 18, 16);
                pon16(&mut v, 20, 16);
                pon16(&mut v, 22, 16368);
                pon16(&mut v, 24, 16384);
                for o in [26, 34, 42, 50, 58] {
                    rect(&mut v, o);
                }
            }
            0x8000_2000 => {
                pon16(&mut v, 24, 22);
                pon16(&mut v, 26, 22);
                pon16(&mut v, 36, al);
                pon16(&mut v, 38, an / 2);
                pon16(&mut v, 76, 1);
                pon16(&mut v, 78, 1);
                pon32(&mut v, 84, id);
            }
            _ => {
                let nombre = b"DashOS";
                v[16..16 + nombre.len()].copy_from_slice(nombre);
            }
        }
        let k = (largo as usize).min(n);
        if k == 0 {
            return Ok(0);
        }
        ex.tramo(buf, k as u32)?;
        for (i, b) in v[..k].iter().enumerate() {
            ex.e8(buf + i as u32, *b)?;
        }
        Ok(k as u32)
    }

    /// `WeighTAMatch(reqTextAttr, targetTextAttr, targetTags)`: de 0 (nada que ver) a 32767 (`MAXFONTMATCHWEIGHT`, la
    /// misma): cada punto de alto de diferencia, y cada estilo distinto, restan. Desde V39 es privada (sin autodoc).
    pub fn weigh_ta_match<M: Memoria>(&mut self, ex: &mut Exec<M>, pedida: u32, otra: u32) -> Result<u32, Fallo> {
        use crate::estructuras::textattr as ta;
        // El nombre no cuenta (lo compara quien llama): diskfont de 3.2 pasa como `TextAttr` la `TextFont` desplazada
        // 16 bytes, para que `tf_YSize`, `tf_Style` y `tf_Flags` caigan en su sitio; su `ta_Name` es basura.
        ex.tramo(pedida, 8)?;
        ex.tramo(otra, 8)?;
        let dy = (ex.l16(pedida + ta::TA_YSIZE)? as i32 - ex.l16(otra + ta::TA_YSIZE)? as i32).unsigned_abs();
        let estilos = (ex.l8(pedida + 6)? ^ ex.l8(otra + 6)?).count_ones();
        Ok(32767u32.saturating_sub(dy * 1024 + estilos * 256))
    }

    /// `TextExtent(rp, string, count, textExtent)`: el ancho (como `TextLength`), el alto de la fuente y el rectángulo
    /// que ocupa respecto a la posición actual (la línea base): de 0 a ancho - 1 y de -base a alto - base - 1.
    pub fn text_extent<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, s: u32, n: u32, te: u32) -> Result<(), Fallo> {
        ex.tramo(te, 12)?;
        let ancho = self.text_length(ex, rp, s, n)?;
        let i = self.leer_rp(ex, rp)?;
        let fu = self.fuente_de(ex, &i, "TextExtent")?;
        let (alto, base) = (fu.alto.min(0x7FFF) as i32, fu.base.min(0x7FFF) as i32);
        ex.e16(te, ancho as u16)?;
        ex.e16(te + 2, alto as u16)?;
        ex.e16(te + 4, 0)?;
        ex.e16(te + 6, (-base) as i16 as u16)?;
        ex.e16(te + 8, (ancho as i32 - 1).max(0) as i16 as u16)?;
        ex.e16(te + 10, (alto - base - 1) as i16 as u16)?;
        Ok(())
    }

    /// `Text`: `n` caracteres con la fuente del `RastPort` en la posición actual (la línea base en `cp_y`), con
    /// las plumas y el modo; la posición avanza. En JAM2, el fondo de cada glifo va con la pluma B.
    pub fn text<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, s: u32, n: u32) -> Result<(), Fallo> {
        let i = self.leer_rp(ex, rp)?;
        let n = n & 0xFFFF;
        if n == 0 {
            return Ok(());
        }
        let fu = self.fuente_de(ex, &i, "Text")?;
        let texto = ex.bytes(s, n)?;
        // Leer antes los glifos: el pintor tiene prestada la memoria si el mapa es planar.
        let mut glifos = Vec::with_capacity(texto.len());
        for c in texto {
            let (bit, ancho, avance, kern) = self.glifo(ex, &fu, c)?;
            let mut filas = Vec::with_capacity(fu.alto as usize);
            let por_fila = (bit + ancho).div_ceil(8) - bit / 8;
            for f in 0..fu.alto {
                if ancho == 0 || fu.modulo == 0 {
                    filas.push(Vec::new());
                    continue;
                }
                let d = fu.datos + f * fu.modulo + bit / 8;
                filas.push(ex.bytes(d, por_fila)?);
            }
            glifos.push((bit & 7, ancho, avance, kern, filas));
        }
        let arriba = i.y - fu.base as i32;
        let mut x = i.x;
        {
            let mut p = self.pintor(ex, &i)?;
            for (bit, ancho, avance, kern, filas) in &glifos {
                if *ancho > 0 {
                    p.plantilla(x + kern, arriba, *ancho, fu.alto, |c, f| {
                        let fila = &filas[f as usize];
                        let b = bit + c;
                        fila.get((b >> 3) as usize).is_some_and(|v| v & (0x80 >> (b & 7)) != 0)
                    });
                }
                x += avance + kern + i.espacio;
            }
        }
        ex.e16(rp + rp_::CP_X, x as u16)
    }

    /// `SetFont`: la fuente y sus medidas en el `RastPort`.
    pub fn set_font<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, f: u32) -> Result<(), Fallo> {
        self.leer_rp(ex, rp)?;
        let fu = self.leer_fuente(ex, f)?;
        ex.e32(rp + rp_::FONT, f)?;
        ex.e16(rp + rp_::TXHEIGHT, fu.alto as u16)?;
        ex.e16(rp + rp_::TXWIDTH, fu.ancho as u16)?;
        ex.e16(rp + rp_::TXBASELINE, fu.base as u16)?;
        ex.e8(rp + rp_::ALGOSTYLE, 0)
    }

    /// `AskFont`: rellena una `TextAttr` con la fuente del `RastPort`.
    pub fn ask_font<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, ta: u32) -> Result<(), Fallo> {
        let i = self.leer_rp(ex, rp)?;
        ex.tramo(ta, textattr::TAMANO)?;
        let (nombre, flags, estilo) = if i.fuente != 0 {
            let f = puntero(i.fuente)?;
            (ex.l32(f + node::LN_NAME)?, ex.l8(f + tf::TF_FLAGS)?, ex.l8(f + tf::TF_STYLE)?)
        } else {
            (0, 0, 0)
        };
        ex.e32(ta + textattr::TA_NAME, nombre)?;
        ex.e16(ta + textattr::TA_YSIZE, ex.l16(rp + rp_::TXHEIGHT)?)?;
        ex.e8(ta + textattr::TA_STYLE, estilo)?;
        ex.e8(ta + textattr::TA_FLAGS, flags)
    }

    /// `OpenFont`: busca en `GfxBase->TextFonts` una fuente con ese nombre (sin mirar mayúsculas), del tamaño
    /// pedido o del más cercano. 0 si no hay ninguna con ese nombre (diskfont buscaría en disco).
    pub fn open_font<M: Memoria>(&mut self, ex: &mut Exec<M>, ta: u32) -> Result<u32, Fallo> {
        if ta == 0 {
            return Err(caja_exec::Fallo::PunteroNulo.into());
        }
        ex.tramo(ta, textattr::TAMANO)?;
        let nombre = ex.cadena(ex.l32(ta + textattr::TA_NAME)?)?;
        let alto = ex.l16(ta + textattr::TA_YSIZE)? as i32;
        let mut mejor: Option<(i32, u32)> = None;
        for f in ex.nodos(self.gfx.base + gfxbase::TEXTFONTS)? {
            let n = ex.cadena(ex.l32(f + node::LN_NAME)?)?;
            if !n.eq_ignore_ascii_case(&nombre) {
                continue;
            }
            let d = (ex.l16(f + tf::TF_YSIZE)? as i32 - alto).abs();
            if mejor.is_none_or(|(m, _)| d < m) {
                mejor = Some((d, f));
            }
        }
        let Some((_, f)) = mejor else { return Ok(0) };
        let a = ex.l16(f + tf::TF_ACCESSORS)?;
        ex.e16(f + tf::TF_ACCESSORS, a.saturating_add(1))?;
        Ok(f)
    }

    /// `CloseFont`.
    pub fn close_font<M: Memoria>(&mut self, ex: &mut Exec<M>, f: u32) -> Result<(), Fallo> {
        if f == 0 {
            return Ok(());
        }
        ex.tramo(f, tf::TAMANO).map_err(|_| Fallo::FuenteMala { dir: f })?;
        let a = ex.l16(f + tf::TF_ACCESSORS)?;
        if a == 0 {
            return Err(Fallo::FuenteMala { dir: f });
        }
        ex.e16(f + tf::TF_ACCESSORS, a - 1)
    }

    // --- BitMap ---

    /// `InitBitMap`: profundidad, filas y bytes por fila (el ancho redondeado a 16 bits). No toca `Planes[]`.
    pub fn init_bit_map<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        bm: u32,
        prof: u32,
        ancho: u32,
        alto: u32,
    ) -> Result<(), Fallo> {
        if bm == 0 {
            return Err(Fallo::BitMapMalo { dir: 0 });
        }
        ex.tramo(bm, bitmap::PLANES).map_err(|_| Fallo::BitMapMalo { dir: bm })?;
        let ancho = ancho & 0xFFFF;
        ex.e16(bm + bitmap::BYTESPERROW, (ancho.div_ceil(16) * 2) as u16)?;
        ex.e16(bm + bitmap::ROWS, alto as u16)?;
        ex.e8(bm + bitmap::FLAGS, 0)?;
        ex.e8(bm + bitmap::DEPTH, prof as u8)?;
        ex.e16(bm + bitmap::PAD, 0)
    }

    /// `AllocRaster`: un plano de memoria chip (sin borrar).
    pub fn alloc_raster<M: Memoria>(&mut self, ex: &mut Exec<M>, ancho: u32, alto: u32) -> Result<u32, Fallo> {
        let largo = (ancho & 0xFFFF).div_ceil(16) * 2 * (alto & 0xFFFF);
        if largo == 0 {
            return Ok(0);
        }
        Ok(ex.alloc_mem(largo, memf::CHIP)?)
    }

    /// `FreeRaster`.
    pub fn free_raster<M: Memoria>(&mut self, ex: &mut Exec<M>, p: u32, ancho: u32, alto: u32) -> Result<(), Fallo> {
        if p == 0 {
            return Ok(());
        }
        let largo = (ancho & 0xFFFF).div_ceil(16) * 2 * (alto & 0xFFFF);
        Ok(ex.free_mem(p, largo)?)
    }

    /// Registra un mapa nativo nuevo con su `struct BitMap` en `bm`. La estructura dice 8 planos con `Planes[]` a
    /// cero y `BytesPerRow` como si fuera planar (provisional: un programa que escriba en los planos da en la
    /// página 0, y el Enforcer lo apunta).
    pub(crate) fn nuevo_nativo<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        bm: u32,
        ancho: u32,
        alto: u32,
        cm: u32,
    ) -> Result<(), Fallo> {
        ex.cero(bm, bitmap::TAMANO)?;
        ex.e16(bm + bitmap::BYTESPERROW, (ancho.div_ceil(16) * 2).min(0xFFFF) as u16)?;
        ex.e16(bm + bitmap::ROWS, alto as u16)?;
        ex.e8(bm + bitmap::DEPTH, 8)?;
        let fondo = self.color(cm, 0);
        self.gfx.lienzos.insert(bm, Lienzo::nuevo(ancho, alto, fondo, cm));
        Ok(())
    }

    /// `AllocBitMap`: nativo de 32 bits si pide más de 8 planos, su amigo es nativo o pide un formato de color verdadero
    /// de cybergraphics (`BMF_SPECIALFMT`); si no, planar en memoria
    /// chip (entrelazado con `BMF_INTERLEAVED`), como el del chipset.
    pub fn alloc_bit_map<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        ancho: u32,
        alto: u32,
        prof: u32,
        flags: u32,
        amigo: u32,
    ) -> Result<u32, Fallo> {
        let (an, al) = (ancho as i32, alto as i32);
        // cybergraphics: con `BMF_SPECIALFMT` y un formato de color verdadero (los 8 bits de arriba), también nativo.
        let especial = flags & crate::cgx::constantes::BMF_SPECIALFMT != 0 && flags >> 24 != crate::cgx::constantes::PIXFMT_LUT8;
        let nativo = prof > 8 || especial || (amigo != 0 && self.gfx.lienzos.contains_key(&amigo));
        if !lienzo::tamano_valido(an as i64, al as i64) || prof == 0 || prof > 32 || an > 0xFFFF {
            return Err(Fallo::TamanoMalo { ancho: an, alto: al });
        }
        let bm = ex.reservar(bitmap::TAMANO, memf::CLEAR)?;
        if nativo {
            let cm = self.gfx.lienzos.get(&amigo).map(|l| l.paleta).unwrap_or(0);
            self.nuevo_nativo(ex, bm, ancho, alto, cm)?;
            // Sus píxeles, ya: si el montón de la caja no da para ellos, AllocBitMap da 0 como en el Amiga sin memoria
            // (sin aviso: no es un error del programa), en vez de un mapa en el que no se puede pintar.
            if !self.gfx.lienzos.get_mut(&bm).is_some_and(|l| l.con_pixeles()) {
                self.gfx.lienzos.remove(&bm);
                ex.soltar(bm, bitmap::TAMANO)?;
                return Ok(0);
            }
            self.gfx.reservados.insert(bm, vec![(bm, bitmap::TAMANO)]);
            return Ok(bm);
        }
        let fila = ancho.div_ceil(16) * 2;
        let plano = fila as u64 * alto as u64;
        let mut bloques = vec![(bm, bitmap::TAMANO)];
        let total = plano * prof as u64;
        if total > u32::MAX as u64 / 2 {
            ex.soltar(bm, bitmap::TAMANO)?;
            return Err(Fallo::TamanoMalo { ancho: an, alto: al });
        }
        let req = memf::CHIP | memf::CLEAR;
        let entrelazado = flags & bitmap::BMF_INTERLEAVED != 0 && prof > 1;
        let mut planos = [0u32; 8];
        let mut falta = false;
        if entrelazado {
            let d = ex.alloc_mem(total as u32, req)?;
            if d == 0 {
                falta = true;
            } else {
                ex.cambiar_dueno(d, 0);
                bloques.push((d, total as u32));
                for (i, p) in planos.iter_mut().enumerate().take(prof as usize) {
                    *p = d + i as u32 * fila;
                }
            }
        } else {
            for p in planos.iter_mut().take(prof as usize) {
                let d = ex.alloc_mem(plano as u32, req)?;
                if d == 0 {
                    falta = true;
                    break;
                }
                ex.cambiar_dueno(d, 0);
                bloques.push((d, plano as u32));
                *p = d;
            }
        }
        if falta {
            for (d, l) in bloques {
                ex.soltar(d, l)?;
            }
            return Err(Fallo::SinMemoria);
        }
        let bpr = if entrelazado { fila * prof } else { fila };
        ex.e16(bm + bitmap::BYTESPERROW, bpr as u16)?;
        ex.e16(bm + bitmap::ROWS, alto as u16)?;
        ex.e8(bm + bitmap::DEPTH, prof as u8)?;
        for (i, p) in planos.iter().enumerate() {
            ex.e32(bm + bitmap::PLANES + 4 * i as u32, *p)?;
        }
        self.gfx.reservados.insert(bm, bloques);
        Ok(bm)
    }

    /// `FreeBitMap`. Con 0 no hace nada; uno que no salió de `AllocBitMap` da un aviso.
    pub fn free_bit_map<M: Memoria>(&mut self, ex: &mut Exec<M>, bm: u32) -> Result<(), Fallo> {
        if bm == 0 {
            return Ok(());
        }
        let Some(bloques) = self.gfx.reservados.remove(&bm) else { return Err(Fallo::NoReservado { dir: bm }) };
        self.gfx.lienzos.remove(&bm);
        self.cgx_mapa_soltado(ex, bm);
        for (d, l) in bloques {
            ex.soltar(d, l)?;
        }
        Ok(())
    }

    /// `GetBitMapAttr`.
    pub fn get_bit_map_attr<M: Memoria>(&mut self, ex: &mut Exec<M>, bm: u32, attr: u32) -> Result<u32, Fallo> {
        let mapa = self.mapa(ex, bm)?;
        Ok(match (mapa, attr) {
            (Mapa::Nativo, bitmap::BMA_WIDTH) => self.gfx.lienzos[&bm].ancho(),
            (Mapa::Nativo, bitmap::BMA_HEIGHT) => self.gfx.lienzos[&bm].alto(),
            (Mapa::Nativo, bitmap::BMA_DEPTH) => 32,
            (Mapa::Nativo, bitmap::BMA_FLAGS) => bitmap::BMF_DISPLAYABLE,
            (Mapa::Planar(p), bitmap::BMA_WIDTH) => p.ancho,
            (Mapa::Planar(_), bitmap::BMA_HEIGHT) => ex.l16(bm + bitmap::ROWS)? as u32,
            (Mapa::Planar(_), bitmap::BMA_DEPTH) => ex.l8(bm + bitmap::DEPTH)? as u32,
            (Mapa::Planar(p), bitmap::BMA_FLAGS) => {
                let entrelazado = p.prof > 1 && p.ancho / 8 != p.bpr;
                bitmap::BMF_STANDARD | if entrelazado { bitmap::BMF_INTERLEAVED } else { 0 }
            }
            _ => 0,
        })
    }

    // --- Paletas ---

    /// `GetColorMap`: un `ColorMap` de `n` colores (hasta 256), con su tabla nativa.
    pub fn get_color_map<M: Memoria>(&mut self, ex: &mut Exec<M>, n: u32) -> Result<u32, Fallo> {
        let n = n.min(256);
        if n == 0 {
            return Ok(0);
        }
        let cm = ex.reservar(colormap::TAMANO, memf::CLEAR)?;
        let tabla = ex.reservar(n * 2, memf::CLEAR)?;
        let bajos = ex.reservar(n * 2, memf::CLEAR)?;
        ex.e8(cm + colormap::TYPE, colormap::COLORMAP_TYPE_V39)?;
        ex.e16(cm + colormap::COUNT, n as u16)?;
        ex.e32(cm + colormap::COLORTABLE, tabla)?;
        ex.e32(cm + colormap::LOWCOLORBITS, bajos)?;
        self.gfx.paletas.insert(cm, vec![0; n as usize]);
        Ok(cm)
    }

    /// `FreeColorMap`.
    pub fn free_color_map<M: Memoria>(&mut self, ex: &mut Exec<M>, cm: u32) -> Result<(), Fallo> {
        if cm == 0 {
            return Ok(());
        }
        if cm == self.gfx.cm_wb {
            return Err(Fallo::Sospechoso { dir: cm });
        }
        let Some(p) = self.gfx.paletas.remove(&cm) else { return Err(Fallo::ColorMapMalo { dir: cm }) };
        self.gfx.plumas.remove(&cm);
        let n = p.len() as u32;
        let tabla = ex.l32(cm + colormap::COLORTABLE)?;
        let bajos = ex.l32(cm + colormap::LOWCOLORBITS)?;
        ex.soltar(tabla, n * 2)?;
        ex.soltar(bajos, n * 2)?;
        ex.soltar(cm, colormap::TAMANO)
    }

    /// El `ColorMap` de un `ViewPort`, si graphics lo conoce.
    fn cm_de_vp<M: Memoria>(&self, ex: &Exec<M>, vp: u32) -> Result<u32, Fallo> {
        if vp == 0 {
            return Err(caja_exec::Fallo::PunteroNulo.into());
        }
        let cm = ex.l32(puntero(vp)? + viewport::COLORMAP)?;
        if !self.gfx.paletas.contains_key(&cm) {
            return Err(Fallo::ColorMapMalo { dir: cm });
        }
        Ok(cm)
    }

    /// Pone un color (ARGB) en la tabla nativa y en la del 68k (`ColorTable` con 4 bits por componente y
    /// `LowColorBits` con los 4 de abajo). Fuera de la tabla no hace nada.
    pub(crate) fn poner_color<M: Memoria>(&mut self, ex: &mut Exec<M>, cm: u32, n: u32, c: u32) -> Result<(), Fallo> {
        let Some(p) = self.gfx.paletas.get_mut(&cm) else { return Err(Fallo::ColorMapMalo { dir: cm }) };
        if n as usize >= p.len() {
            return Ok(());
        }
        p[n as usize] = c & 0xFF_FFFF;
        let (r, g, b) = ((c >> 16) & 0xFF, (c >> 8) & 0xFF, c & 0xFF);
        let alto = (r >> 4) << 8 | (g >> 4) << 4 | b >> 4;
        let bajo = (r & 15) << 8 | (g & 15) << 4 | (b & 15);
        let tabla = puntero(ex.l32(cm + colormap::COLORTABLE)?)?;
        let bajos = puntero(ex.l32(cm + colormap::LOWCOLORBITS)?)?;
        ex.e16(tabla + n * 2, alto as u16)?;
        ex.e16(bajos + n * 2, bajo as u16)
    }

    pub fn set_rgb4<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        vp: u32,
        n: u32,
        r: u32,
        g: u32,
        b: u32,
    ) -> Result<(), Fallo> {
        let cm = self.cm_de_vp(ex, vp)?;
        self.poner_color(ex, cm, n, rgb4(r, g, b))
    }

    pub fn set_rgb4_cm<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        cm: u32,
        n: u32,
        r: u32,
        g: u32,
        b: u32,
    ) -> Result<(), Fallo> {
        self.poner_color(ex, cm, n, rgb4(r, g, b))
    }

    pub fn set_rgb32<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        vp: u32,
        n: u32,
        r: u32,
        g: u32,
        b: u32,
    ) -> Result<(), Fallo> {
        let cm = self.cm_de_vp(ex, vp)?;
        self.poner_color(ex, cm, n, rgb32(r, g, b))
    }

    pub fn set_rgb32_cm<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        cm: u32,
        n: u32,
        r: u32,
        g: u32,
        b: u32,
    ) -> Result<(), Fallo> {
        self.poner_color(ex, cm, n, rgb32(r, g, b))
    }

    /// `LoadRGB4`: `n` colores de 12 bits (UWORD `0x0RGB`) desde la pluma 0.
    pub fn load_rgb4<M: Memoria>(&mut self, ex: &mut Exec<M>, vp: u32, colores: u32, n: u32) -> Result<(), Fallo> {
        let cm = self.cm_de_vp(ex, vp)?;
        let n = (n & 0xFFFF).min(256);
        let t = ex.bytes(colores, n * 2)?;
        for (i, c) in t.chunks(2).enumerate() {
            let v = u16::from_be_bytes([c[0], c[1]]) as u32;
            self.poner_color(ex, cm, i as u32, rgb4(v >> 8, v >> 4, v))?;
        }
        Ok(())
    }

    /// `LoadRGB32`: tramos de `cuántos << 16 | primero` y tres LONG por color, hasta un tramo de 0 colores.
    pub fn load_rgb32<M: Memoria>(&mut self, ex: &mut Exec<M>, vp: u32, tabla: u32) -> Result<(), Fallo> {
        let cm = self.cm_de_vp(ex, vp)?;
        if tabla == 0 {
            return Ok(());
        }
        let mut d = tabla;
        for _ in 0..1024 {
            let cab = ex.l32(d)?;
            let (n, primero) = (cab >> 16, cab & 0xFFFF);
            if n == 0 {
                return Ok(());
            }
            let n = n.min(256);
            let v = ex.bytes(d + 4, n * 12)?;
            for (i, c) in v.chunks(12).enumerate() {
                let r = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
                let g = u32::from_be_bytes([c[4], c[5], c[6], c[7]]);
                let b = u32::from_be_bytes([c[8], c[9], c[10], c[11]]);
                self.poner_color(ex, cm, primero + i as u32, rgb32(r, g, b))?;
            }
            d += 4 + n * 12;
        }
        Err(Fallo::Sospechoso { dir: tabla })
    }

    /// `GetRGB4`: `0x0RGB`, o -1 fuera de la tabla.
    pub fn get_rgb4<M: Memoria>(&mut self, _ex: &mut Exec<M>, cm: u32, n: u32) -> Result<i32, Fallo> {
        let p = self.gfx.paletas.get(&cm).ok_or(Fallo::ColorMapMalo { dir: cm })?;
        let Some(&c) = p.get(n as usize) else { return Ok(-1) };
        Ok((((c >> 20) & 15) << 8 | ((c >> 12) & 15) << 4 | ((c >> 4) & 15)) as i32)
    }

    /// `GetRGB32`: tres LONG por color, con los 8 bits repetidos a lo ancho.
    pub fn get_rgb32<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        cm: u32,
        primero: u32,
        n: u32,
        tabla: u32,
    ) -> Result<(), Fallo> {
        let p = self.gfx.paletas.get(&cm).ok_or(Fallo::ColorMapMalo { dir: cm })?.clone();
        let n = n.min(256);
        ex.tramo(tabla, n * 12)?;
        for i in 0..n {
            let c = p.get((primero + i) as usize).copied().unwrap_or(0);
            for (k, desp) in [16u32, 8, 0].iter().enumerate() {
                ex.e32(tabla + i * 12 + k as u32 * 4, ((c >> desp) & 0xFF) * 0x0101_0101)?;
            }
        }
        Ok(())
    }

    /// `FindColor` y `ObtainBestPenA`: la pluma del color más parecido (hasta `max`).
    pub fn find_color(&mut self, cm: u32, r: u32, g: u32, b: u32, max: i32) -> Result<u32, Fallo> {
        let p = self.gfx.paletas.get(&cm).ok_or(Fallo::ColorMapMalo { dir: cm })?;
        let tope = if max < 0 { p.len() } else { (max as usize + 1).min(p.len()) };
        Ok(mas_cercano(&p[..tope], rgb32(r, g, b)) as u32)
    }

    /// `ObtainPen`: con `n` = -1, una pluma libre (de la 4 a la 251, provisional) con ese color; con un número,
    /// esa si está libre. -1 si no hay.
    #[allow(clippy::too_many_arguments)]
    pub fn obtain_pen<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        cm: u32,
        n: i32,
        r: u32,
        g: u32,
        b: u32,
    ) -> Result<i32, Fallo> {
        let len = self.gfx.paletas.get(&cm).ok_or(Fallo::ColorMapMalo { dir: cm })?.len() as i32;
        let usadas = self.gfx.plumas.entry(cm).or_default();
        let pen = if n < 0 {
            match (4..len.min(252)).find(|p| !usadas.contains(&(*p as u8))) {
                Some(p) => p,
                None => return Ok(-1),
            }
        } else if n < len && !usadas.contains(&(n as u8)) {
            n
        } else {
            return Ok(-1);
        };
        usadas.insert(pen as u8);
        self.poner_color(ex, cm, pen as u32, rgb32(r, g, b))?;
        Ok(pen)
    }

    /// `ReleasePen`.
    pub fn release_pen(&mut self, cm: u32, n: u32) {
        if let Some(u) = self.gfx.plumas.get_mut(&cm) {
            u.remove(&(n as u8));
        }
    }
}
