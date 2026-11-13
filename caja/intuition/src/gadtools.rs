//! gadtools.library nativa (V40) y los gadgets de intuition que necesita: `AddGList`, `RemoveGList`, `RefreshGList`,
//! `OnGadget`, `OffGadget`... y lo que pasa al pulsarlos (`IDCMP_GADGETDOWN`, `IDCMP_GADGETUP`, `IDCMP_MOUSEMOVE`).
//!
//! Escrito desde los autodocs de gadtools.library e intuition.library, `libraries/gadtools.h`, `intuition/intuition.h`
//! y `gadtools_lib.fd` del NDK y el RKRM (Libraries, "GadTools Library"). No se ha leído código de AmigaOS ni de AROS.
//!
//! Cada gadget de GadTools es una `struct Gadget` de verdad en la memoria del 68k (los programas la enlazan, la
//! comparan con `IAddress`, leen su `GadgetID`, su `UserData` y, en los de texto, el `Buffer` de su `StringInfo`),
//! creada con `AllocMem` de la tarea. Su estado (nivel, opción activa, marcado...) y su dibujo son nativos: una tabla
//! por dirección. Se dibujan en el lienzo de la ventana con el aspecto de GadTools de 3.x (bordes en relieve con las
//! plumas de la pantalla), en las coordenadas de la ventana (también en las `GIMMEZEROZERO`). Los gadgets de
//! intuition que no son de GadTools se reconocen (se pulsan y mandan sus mensajes) pero no se dibujan: su
//! `GadgetRender` (`Border`, `Image`) queda para otro paso.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use caja_exec::estructuras::{list, memf, node};
use caja_exec::{Exec, Memoria, Registros};

use crate::acceso::{Acceso, palabra};
use crate::estructuras::{drawinfo as dri, idcmp, ie, tag, window as win};
use crate::lienzo::{Lienzo, Rect};
use crate::lvo::{GADTOOLS as T, fila};
use crate::{CajaGrafica, Compositor, Fallo, fuente};

macro_rules! filas {
    ($tabla:ident; $($c:ident = $n:literal),* $(,)?) => { $(const $c: usize = fila(&$tabla, $n);)* };
}

filas!(T;
    CREATEGADGETA = "CreateGadgetA", FREEGADGETS = "FreeGadgets", GT_SETGADGETATTRSA = "GT_SetGadgetAttrsA",
    CREATEMENUSA = "CreateMenusA", FREEMENUS = "FreeMenus", LAYOUTMENUITEMSA = "LayoutMenuItemsA",
    LAYOUTMENUSA = "LayoutMenusA", GT_GETIMSG = "GT_GetIMsg", GT_REPLYIMSG = "GT_ReplyIMsg",
    GT_REFRESHWINDOW = "GT_RefreshWindow", GT_BEGINREFRESH = "GT_BeginRefresh", GT_ENDREFRESH = "GT_EndRefresh",
    GT_FILTERIMSG = "GT_FilterIMsg", GT_POSTFILTERIMSG = "GT_PostFilterIMsg", CREATECONTEXT = "CreateContext",
    DRAWBEVELBOXA = "DrawBevelBoxA", GETVISUALINFOA = "GetVisualInfoA", FREEVISUALINFO = "FreeVisualInfo",
    GT_GETGADGETATTRSA = "GT_GetGadgetAttrsA",
);

/// `struct Gadget` (`intuition/intuition.h`).
#[allow(dead_code)]
pub mod gadget {
    pub const NEXTGADGET: u32 = 0;
    pub const LEFTEDGE: u32 = 4;
    pub const TOPEDGE: u32 = 6;
    pub const WIDTH: u32 = 8;
    pub const HEIGHT: u32 = 10;
    pub const FLAGS: u32 = 12;
    pub const ACTIVATION: u32 = 14;
    pub const GADGETTYPE: u32 = 16;
    pub const GADGETRENDER: u32 = 18;
    pub const SELECTRENDER: u32 = 22;
    pub const GADGETTEXT: u32 = 26;
    pub const MUTUALEXCLUDE: u32 = 30;
    pub const SPECIALINFO: u32 = 34;
    pub const GADGETID: u32 = 38;
    pub const USERDATA: u32 = 40;
    pub const TAMANO: u32 = 44;

    pub const GFLG_GADGHNONE: u16 = 3;
    pub const GFLG_SELECTED: u16 = 0x80;
    pub const GFLG_DISABLED: u16 = 0x100;
    pub const GFLG_TABCYCLE: u16 = 0x200;
    pub const GACT_RELVERIFY: u16 = 1;
    pub const GACT_IMMEDIATE: u16 = 2;
    pub const GACT_FOLLOWMOUSE: u16 = 8;
    pub const GACT_TOGGLESELECT: u16 = 0x100;
    pub const GACT_LONGINT: u16 = 0x800;
    pub const GTYP_BOOLGADGET: u16 = 1;
    pub const GTYP_PROPGADGET: u16 = 3;
    pub const GTYP_STRGADGET: u16 = 4;
    pub const GTYP_GADTOOLS: u16 = 0x0100;
    pub const GTYP_GZZGADGET: u16 = 0x2000;
    pub const GTYP_SYSGADGET: u16 = 0x8000;
}

/// `struct StringInfo`.
#[allow(dead_code)]
pub mod stringinfo {
    pub const BUFFER: u32 = 0;
    pub const UNDOBUFFER: u32 = 4;
    pub const BUFFERPOS: u32 = 8;
    pub const MAXCHARS: u32 = 10;
    pub const DISPPOS: u32 = 12;
    pub const NUMCHARS: u32 = 16;
    pub const LONGINT: u32 = 28;
    pub const TAMANO: u32 = 36;
}

/// `struct NewGadget`.
pub mod newgadget {
    pub const LEFTEDGE: u32 = 0;
    pub const TOPEDGE: u32 = 2;
    pub const WIDTH: u32 = 4;
    pub const HEIGHT: u32 = 6;
    pub const GADGETTEXT: u32 = 8;
    pub const GADGETID: u32 = 16;
    pub const FLAGS: u32 = 18;
    pub const VISUALINFO: u32 = 22;
    pub const USERDATA: u32 = 26;
    pub const TAMANO: u32 = 30;
}

/// Los tipos de gadget de GadTools (`*_KIND`) y sus etiquetas (`GT_TagBase` + n).
#[allow(dead_code)]
pub mod kind {
    pub const GENERIC: u32 = 0;
    pub const BUTTON: u32 = 1;
    pub const CHECKBOX: u32 = 2;
    pub const INTEGER: u32 = 3;
    pub const LISTVIEW: u32 = 4;
    pub const MX: u32 = 5;
    pub const NUMBER: u32 = 6;
    pub const CYCLE: u32 = 7;
    pub const PALETTE: u32 = 8;
    pub const SCROLLER: u32 = 9;
    pub const SLIDER: u32 = 11;
    pub const STRING: u32 = 12;
    pub const TEXT: u32 = 13;
    /// El gadget de `CreateContext` (no es un tipo de GadTools: es nuestro).
    pub const CONTEXTO: u32 = 100;
}

#[allow(dead_code)]
pub mod gt {
    const B: u32 = 0x8008_0000;
    pub const CB_CHECKED: u32 = B + 4;
    pub const LV_TOP: u32 = B + 5;
    pub const LV_LABELS: u32 = B + 6;
    pub const LV_READONLY: u32 = B + 7;
    pub const MX_LABELS: u32 = B + 9;
    pub const MX_ACTIVE: u32 = B + 10;
    pub const TX_TEXT: u32 = B + 11;
    pub const NM_NUMBER: u32 = B + 13;
    pub const CY_LABELS: u32 = B + 14;
    pub const CY_ACTIVE: u32 = B + 15;
    pub const PA_DEPTH: u32 = B + 16;
    pub const PA_COLOR: u32 = B + 17;
    pub const PA_COLOROFFSET: u32 = B + 18;
    pub const PA_INDICATORWIDTH: u32 = B + 19;
    pub const SC_TOP: u32 = B + 21;
    pub const SC_TOTAL: u32 = B + 22;
    pub const SC_VISIBLE: u32 = B + 23;
    pub const SL_MIN: u32 = B + 38;
    pub const SL_MAX: u32 = B + 39;
    pub const SL_LEVEL: u32 = B + 40;
    pub const SL_MAXLEVELLEN: u32 = B + 41;
    pub const SL_LEVELFORMAT: u32 = B + 42;
    pub const SL_LEVELPLACE: u32 = B + 43;
    pub const ST_STRING: u32 = B + 45;
    pub const ST_MAXCHARS: u32 = B + 46;
    pub const IN_NUMBER: u32 = B + 47;
    pub const IN_MAXCHARS: u32 = B + 48;
    pub const BB_RECESSED: u32 = B + 51;
    pub const LV_SHOWSELECTED: u32 = B + 53;
    pub const LV_SELECTED: u32 = B + 54;
    pub const TX_BORDER: u32 = B + 57;
    pub const NM_BORDER: u32 = B + 58;
    pub const MX_SPACING: u32 = B + 61;
    pub const BB_FRAMETYPE: u32 = B + 77;
    pub const PA_NUMCOLORS: u32 = B + 70;
    /// `GA_Disabled` (`GA_Dummy + 14`).
    pub const GA_DISABLED: u32 = 0x8003_000E;
    pub const PLACETEXT_LEFT: u32 = 1;
    pub const PLACETEXT_RIGHT: u32 = 2;
    pub const PLACETEXT_ABOVE: u32 = 4;
    pub const PLACETEXT_BELOW: u32 = 8;
    pub const PLACETEXT_IN: u32 = 16;
    pub const BBFT_RIDGE: u32 = 2;
}

/// Un gadget de GadTools: lo que no está en la memoria del 68k.
#[derive(Clone, Default)]
pub(crate) struct Gt {
    pub tipo: u32,
    pub tarea: u32,
    /// Bloques que reservó CreateGadgetA (dirección, largo): el gadget y su `StringInfo` con sus búferes.
    pub bloques: Vec<(u32, u32)>,
    pub etiqueta: Vec<u8>,
    pub lugar: u32,
    /// Marcado, opción activa, nivel, color, elegido o número, según el tipo.
    pub valor: i32,
    pub min: i32,
    pub max: i32,
    pub textos: Vec<Vec<u8>>,
    pub texto: Vec<u8>,
    pub borde: bool,
    pub formato: Vec<u8>,
    pub lista: u32,
    pub arriba: i32,
    pub visibles: i32,
    pub solo_lectura: bool,
    pub colores: u32,
    pub desplaz: u32,
    pub indicador: i32,
    pub info: u32,
    pub espacio: i32,
    pub pulsado: bool,
}

/// Lo que se está haciendo con el ratón o el teclado sobre un gadget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Activo {
    /// Pulsado con el botón izquierdo (botones, casillas, ciclos, paletas, listas).
    Pulsado { w: u32, g: u32 },
    /// Arrastrando el mando de un deslizador; `dx` desde el borde del mando.
    Arrastre { w: u32, g: u32, dx: i32 },
    /// Un gadget de texto con el cursor dentro.
    Texto { w: u32, g: u32 },
    /// Un gadget de intuition que no es de GadTools, pulsado.
    Intuition { w: u32, g: u32 },
}

#[derive(Default)]
pub(crate) struct Estado {
    pub base: u32,
    pub gadgets: BTreeMap<u32, Gt>,
    /// `VisualInfo` repartidos: dirección → tarea.
    pub visuales: BTreeMap<u32, u32>,
    pub activo: Option<Activo>,
}

/// Tope de etiquetas de una lista (más es una lista en círculo).
const MAX_ETIQUETAS: u32 = 4096;
/// Tope de nodos de una lista de un listview.
const MAX_NODOS: u32 = 10_000;

/// Lee una lista de etiquetas (`TAG_MORE`, `TAG_SKIP`, `TAG_IGNORE`): sus pares (etiqueta, dato).
pub(crate) fn leer_etiquetas<M: Memoria>(ex: &Exec<M>, mut d: u32) -> Result<Vec<(u32, u32)>, Fallo> {
    let mut v = Vec::new();
    let mut vueltas = 0;
    while d != 0 {
        vueltas += 1;
        if vueltas > MAX_ETIQUETAS {
            return Err(Fallo::EtiquetasMalas { dir: d });
        }
        let (t, x) = (ex.l32(d.wrapping_add(tag::TI_TAG))?, ex.l32(d.wrapping_add(tag::TI_DATA))?);
        match t {
            tag::TAG_DONE => break,
            tag::TAG_IGNORE => {}
            tag::TAG_MORE => {
                d = x;
                continue;
            }
            tag::TAG_SKIP => d = d.wrapping_add(tag::TAMANO * x),
            _ => v.push((t, x)),
        }
        d = d.wrapping_add(tag::TAMANO);
    }
    Ok(v)
}

/// Un texto con la fuente topaz/8 en el lienzo: arriba a la izquierda en `(x, y)`, cortado en `hasta` (x exclusiva).
pub(crate) fn texto(l: &mut Lienzo, glifos: &[[u8; 8]], x: i32, y: i32, s: &[u8], c: u32, hasta: i32) -> i32 {
    let mut x = x;
    for &b in s {
        if x + fuente::ANCHO as i32 > hasta {
            break;
        }
        let gl = glifos.get(if b >= 32 { b as usize } else { 256 }).copied().unwrap_or([0; 8]);
        for (f, fila) in gl.iter().enumerate() {
            for k in 0..8 {
                if fila & (0x80 >> k) != 0 {
                    l.poner(x + k, y + f as i32, c);
                }
            }
        }
        x += fuente::ANCHO as i32;
    }
    x
}

pub(crate) fn llenar(l: &mut Lienzo, r: Rect, c: u32) {
    let r = r.interseccion(&l.rect());
    if r.vacio() {
        return;
    }
    for y in r.y0..=r.y1 {
        for x in r.x0..=r.x1 {
            l.poner(x, y, c);
        }
    }
}

/// Un marco de GadTools: claro arriba y a la izquierda, oscuro abajo y a la derecha (al revés si `hundido`). Como en
/// 3.x, las esquinas de abajo a la izquierda y de arriba a la derecha son del color de abajo y de arriba.
pub(crate) fn bisel(l: &mut Lienzo, r: Rect, claro: u32, oscuro: u32, hundido: bool) {
    let (a, b) = if hundido { (oscuro, claro) } else { (claro, oscuro) };
    llenar(l, Rect::nuevo(r.x0, r.y0, r.x1 - 1, r.y0), a);
    llenar(l, Rect::nuevo(r.x0, r.y0, r.x0, r.y1 - 1), a);
    llenar(l, Rect::nuevo(r.x0 + 1, r.y1, r.x1, r.y1), b);
    llenar(l, Rect::nuevo(r.x1, r.y0 + 1, r.x1, r.y1), b);
    // La segunda columna de los lados, como en GadTools (los bordes verticales son de dos píxeles).
    llenar(l, Rect::nuevo(r.x0 + 1, r.y0 + 1, r.x0 + 1, r.y1 - 1), a);
    llenar(l, Rect::nuevo(r.x1 - 1, r.y0 + 1, r.x1 - 1, r.y1 - 1), b);
}

/// Un texto del 68k: hasta `max` bytes o el 0.
fn cadena_corta<M: Memoria>(ex: &Exec<M>, d: u32, max: usize) -> Vec<u8> {
    if d == 0 {
        return Vec::new();
    }
    let mut v = ex.cadena(d).unwrap_or_default();
    v.truncate(max);
    v
}

/// Las opciones de un ciclo o de un mx: un vector de punteros acabado en NULL.
fn opciones<M: Memoria>(ex: &Exec<M>, d: u32) -> Vec<Vec<u8>> {
    let mut v = Vec::new();
    if d == 0 {
        return v;
    }
    for i in 0..256u32 {
        match ex.l32(d.wrapping_add(4 * i)) {
            Ok(0) | Err(_) => break,
            Ok(p) => v.push(cadena_corta(ex, p, 128)),
        }
    }
    v
}

/// `%ld`, `%lu`, `%ld%%`... con un número: lo que hace falta para `GTSL_LevelFormat` (RawDoFmt con un LONG).
fn formatear(f: &[u8], n: i32) -> Vec<u8> {
    let mut v = Vec::new();
    let mut i = 0;
    while i < f.len() {
        if f[i] == b'%' && i + 1 < f.len() {
            let mut j = i + 1;
            if f[j] == b'%' {
                v.push(b'%');
                i = j + 1;
                continue;
            }
            while j < f.len() && (f[j] == b'-' || f[j].is_ascii_digit()) {
                j += 1;
            }
            let ancho: usize = core::str::from_utf8(&f[i + 1..j]).ok().and_then(|s| s.trim_start_matches('-').parse().ok()).unwrap_or(0)
                .min(64);
            if j < f.len() && f[j] == b'l' {
                j += 1;
            }
            if j < f.len() && matches!(f[j], b'd' | b'u' | b'x' | b'X') {
                let s = match f[j] {
                    b'u' => alloc::format!("{}", n as u32),
                    b'x' => alloc::format!("{:x}", n),
                    b'X' => alloc::format!("{:X}", n),
                    _ => alloc::format!("{}", n),
                };
                v.extend(core::iter::repeat_n(b' ', ancho.saturating_sub(s.len())));
                v.extend_from_slice(s.as_bytes());
                i = j + 1;
                continue;
            }
        }
        v.push(f[i]);
        i += 1;
    }
    v
}

impl<C: Compositor> CajaGrafica<C> {
    pub(crate) fn instalar_gadtools<M: Memoria>(&mut self, ex: &mut Exec<M>) -> Result<(), Fallo> {
        let numeros: Vec<u16> = (0..T.len() as u16).map(|i| self.numeros.gadtools + i).collect();
        self.gt.base = ex.crear_biblioteca_nativa("gadtools.library", 40, 1, &numeros, 34)?;
        Ok(())
    }

    /// La base de gadtools.library.
    pub fn gadtools_base(&self) -> u32 {
        self.gt.base
    }

    pub(crate) fn es_gadtools(&self, n: u16) -> Option<usize> {
        let g0 = self.numeros.gadtools;
        (n >= g0 && ((n - g0) as usize) < T.len()).then(|| (n - g0) as usize)
    }

    pub(crate) fn llamar_gadtools<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        f: usize,
        r: &mut Registros,
    ) -> Result<u32, Fallo> {
        let (d, a) = (r.d, r.a);
        match f {
            CREATECONTEXT => self.create_context(ex, a[0]),
            CREATEGADGETA => self.create_gadget(ex, d[0], a[0], a[1], a[2]),
            FREEGADGETS => self.free_gadgets(ex, a[0]).map(|_| 0),
            GT_SETGADGETATTRSA => self.gt_set_attrs(ex, a[0], a[1], a[3]).map(|_| 0),
            GT_GETGADGETATTRSA => self.gt_get_attrs(ex, a[0], a[3]),
            GETVISUALINFOA => {
                let s = a[0];
                if s != self.intui.wb.dir {
                    return Err(Fallo::PantallaDesconocida { dir: s });
                }
                let vi = ex.alloc_mem(8, memf::PUBLIC | memf::CLEAR)?;
                if vi == 0 {
                    return Err(Fallo::SinMemoria);
                }
                ex.e32(vi, s)?;
                self.gt.visuales.insert(vi, ex.tarea_actual());
                Ok(vi)
            }
            FREEVISUALINFO => {
                if a[0] == 0 {
                    return Ok(0);
                }
                if self.gt.visuales.remove(&a[0]).is_none() {
                    return Err(Fallo::Sospechoso { dir: a[0] });
                }
                ex.free_mem(a[0], 8)?;
                Ok(0)
            }
            GT_GETIMSG => Ok(ex.get_msg(a[0])?),
            GT_REPLYIMSG => {
                if a[1] != 0 {
                    ex.reply_msg(a[1])?;
                }
                Ok(0)
            }
            GT_FILTERIMSG | GT_POSTFILTERIMSG => Ok(a[1]),
            GT_REFRESHWINDOW => {
                self.ventana(a[0])?;
                self.dibujar_lista(ex, a[0], ex.l32(a[0] + win::FIRSTGADGET)?, -1, true);
                Ok(0)
            }
            GT_BEGINREFRESH => self.ventana(a[0]).map(|_| 0),
            GT_ENDREFRESH => self.end_refresh(ex, a[0], d[0] != 0).map(|_| 0),
            DRAWBEVELBOXA => self.draw_bevel_box(ex, a[0], palabra(d[0]), palabra(d[1]), palabra(d[2]), palabra(d[3]), a[1]).map(|_| 0),
            CREATEMENUSA => self.create_menus(ex, a[0], a[1]),
            FREEMENUS => self.free_menus(ex, a[0]).map(|_| 0),
            LAYOUTMENUSA => self.layout_menus(ex, a[0], false),
            LAYOUTMENUITEMSA => self.layout_menus(ex, a[0], true),
            _ => Err(Fallo::NoImplementada { n: self.numeros.gadtools + f as u16 }),
        }
    }

    // --- Crear y soltar ---

    fn nuevo_bloque<M: Memoria>(&mut self, ex: &mut Exec<M>, largo: u32, bloques: &mut Vec<(u32, u32)>) -> Result<u32, Fallo> {
        let d = ex.alloc_mem(largo, memf::PUBLIC | memf::CLEAR)?;
        if d == 0 {
            return Err(Fallo::SinMemoria);
        }
        bloques.push((d, largo));
        Ok(d)
    }

    /// `CreateContext(&glist)`: el primer gadget de la lista (invisible) en `*glist`.
    fn create_context<M: Memoria>(&mut self, ex: &mut Exec<M>, glist: u32) -> Result<u32, Fallo> {
        ex.tramo(glist, 4)?;
        let mut bloques = Vec::new();
        let g = self.nuevo_bloque(ex, gadget::TAMANO, &mut bloques)?;
        ex.e16(g.wrapping_add(gadget::FLAGS), gadget::GFLG_GADGHNONE)?;
        ex.e16(g.wrapping_add(gadget::GADGETTYPE), gadget::GTYP_GADTOOLS | gadget::GTYP_BOOLGADGET)?;
        ex.e32(glist, g)?;
        self.gt.gadgets.insert(g, Gt { tipo: kind::CONTEXTO, tarea: ex.tarea_actual(), bloques, ..Default::default() });
        Ok(g)
    }

    /// `CreateGadgetA(kind, previous, newgadget, tags)`: el gadget nuevo, enlazado detrás de `previous`. Con
    /// `previous` a 0 (un `CreateGadget` anterior que falló), 0 sin aviso: así se encadenan.
    fn create_gadget<M: Memoria>(&mut self, ex: &mut Exec<M>, tipo: u32, prev: u32, ng: u32, tags: u32) -> Result<u32, Fallo> {
        if prev == 0 {
            return Ok(0);
        }
        if !matches!(tipo, 1..=9 | 11..=13) {
            return Err(Fallo::Sospechoso { dir: tipo });
        }
        ex.tramo(ng, newgadget::TAMANO)?;
        ex.tramo(prev, gadget::TAMANO)?;
        let vi = ex.l32(ng.wrapping_add(newgadget::VISUALINFO))?;
        if !self.gt.visuales.contains_key(&vi) {
            return Err(Fallo::Sospechoso { dir: vi });
        }
        let etiquetas = leer_etiquetas(ex, tags)?;
        let et = |t: u32, defecto: u32| etiquetas.iter().rev().find(|(x, _)| *x == t).map_or(defecto, |(_, d)| *d);
        let mut s = Gt {
            tipo,
            tarea: ex.tarea_actual(),
            etiqueta: cadena_corta(ex, ex.l32(ng.wrapping_add(newgadget::GADGETTEXT))?, 128),
            lugar: ex.l32(ng.wrapping_add(newgadget::FLAGS))? & 0x1F,
            ..Default::default()
        };
        if s.lugar == 0 {
            s.lugar = match tipo {
                kind::BUTTON => gt::PLACETEXT_IN,
                kind::MX | kind::CHECKBOX => gt::PLACETEXT_LEFT,
                kind::LISTVIEW | kind::PALETTE => gt::PLACETEXT_ABOVE,
                _ => gt::PLACETEXT_LEFT,
            };
        }
        let (x, y) = (ex.l16(ng.wrapping_add(newgadget::LEFTEDGE))?, ex.l16(ng.wrapping_add(newgadget::TOPEDGE))?);
        let (mut an, mut al) = (ex.l16(ng.wrapping_add(newgadget::WIDTH))?, ex.l16(ng.wrapping_add(newgadget::HEIGHT))?);
        let mut bloques = Vec::new();
        let g = self.nuevo_bloque(ex, gadget::TAMANO, &mut bloques)?;
        let mut activacion = gadget::GACT_RELVERIFY;
        let mut tipo_i = gadget::GTYP_GADTOOLS | gadget::GTYP_BOOLGADGET;
        match tipo {
            kind::CHECKBOX => {
                s.valor = (et(gt::CB_CHECKED, 0) != 0) as i32;
                (an, al) = (26, 11);
            }
            kind::MX => {
                s.textos = opciones(ex, et(gt::MX_LABELS, 0));
                s.valor = et(gt::MX_ACTIVE, 0) as i32;
                s.espacio = et(gt::MX_SPACING, 1) as i32;
                (an, al) = (17, 9);
                activacion = gadget::GACT_IMMEDIATE;
            }
            kind::CYCLE => {
                s.textos = opciones(ex, et(gt::CY_LABELS, 0));
                s.valor = et(gt::CY_ACTIVE, 0) as i32;
            }
            kind::TEXT => {
                s.texto = cadena_corta(ex, et(gt::TX_TEXT, 0), 256);
                s.borde = et(gt::TX_BORDER, 0) != 0;
            }
            kind::NUMBER => {
                s.valor = et(gt::NM_NUMBER, 0) as i32;
                s.borde = et(gt::NM_BORDER, 0) != 0;
            }
            kind::SLIDER | kind::SCROLLER => {
                if tipo == kind::SLIDER {
                    s.min = et(gt::SL_MIN, 0) as i32;
                    s.max = et(gt::SL_MAX, 15) as i32;
                    s.valor = et(gt::SL_LEVEL, 0) as i32;
                    s.formato = cadena_corta(ex, et(gt::SL_LEVELFORMAT, 0), 32);
                    s.visibles = et(gt::SL_MAXLEVELLEN, 2) as i32;
                } else {
                    s.max = et(gt::SC_TOTAL, 0) as i32;
                    s.visibles = et(gt::SC_VISIBLE, 2) as i32;
                    s.valor = et(gt::SC_TOP, 0) as i32;
                }
                activacion = gadget::GACT_RELVERIFY | gadget::GACT_IMMEDIATE | gadget::GACT_FOLLOWMOUSE;
                tipo_i = gadget::GTYP_GADTOOLS | gadget::GTYP_PROPGADGET;
            }
            kind::PALETTE => {
                let prof = et(gt::PA_DEPTH, 1).clamp(1, 8);
                s.colores = et(gt::PA_NUMCOLORS, 1 << prof).clamp(2, 256);
                s.desplaz = et(gt::PA_COLOROFFSET, 0);
                s.valor = et(gt::PA_COLOR, 1) as i32;
                s.indicador = et(gt::PA_INDICATORWIDTH, 0) as i32;
            }
            kind::LISTVIEW => {
                s.lista = et(gt::LV_LABELS, 0);
                s.arriba = et(gt::LV_TOP, 0) as i32;
                s.valor = et(gt::LV_SELECTED, u32::MAX) as i32;
                s.solo_lectura = et(gt::LV_READONLY, 0) != 0;
                activacion = gadget::GACT_RELVERIFY | gadget::GACT_IMMEDIATE;
            }
            kind::STRING | kind::INTEGER => {
                let entero = tipo == kind::INTEGER;
                let max = if entero { et(gt::IN_MAXCHARS, 10) } else { et(gt::ST_MAXCHARS, 64) }.clamp(1, 4096) + 1;
                let si = self.nuevo_bloque(ex, stringinfo::TAMANO + 2 * max, &mut bloques)?;
                let (buf, undo) = (si.wrapping_add(stringinfo::TAMANO), si.wrapping_add(stringinfo::TAMANO) + max);
                let inicial = if entero {
                    alloc::format!("{}", et(gt::IN_NUMBER, 0) as i32).into_bytes()
                } else {
                    cadena_corta(ex, et(gt::ST_STRING, 0), max as usize - 1)
                };
                ex.escribir_bytes(buf, &inicial)?;
                ex.e32(si.wrapping_add(stringinfo::BUFFER), buf)?;
                ex.e32(si.wrapping_add(stringinfo::UNDOBUFFER), undo)?;
                ex.e16(si.wrapping_add(stringinfo::MAXCHARS), max as u16)?;
                ex.e16(si.wrapping_add(stringinfo::NUMCHARS), inicial.len() as u16)?;
                ex.e16(si.wrapping_add(stringinfo::BUFFERPOS), inicial.len() as u16)?;
                if entero {
                    ex.e32(si.wrapping_add(stringinfo::LONGINT), et(gt::IN_NUMBER, 0))?;
                    activacion |= gadget::GACT_LONGINT;
                }
                ex.e32(g.wrapping_add(gadget::SPECIALINFO), si)?;
                s.info = si;
                tipo_i = gadget::GTYP_GADTOOLS | gadget::GTYP_STRGADGET;
            }
            _ => {}
        }
        let mut flags = gadget::GFLG_GADGHNONE;
        if et(gt::GA_DISABLED, 0) != 0 {
            flags |= gadget::GFLG_DISABLED;
        }
        if tipo == kind::STRING || tipo == kind::INTEGER {
            flags |= gadget::GFLG_TABCYCLE;
        }
        if tipo == kind::MX {
            // El gadget cubre todas las opciones.
            let n = s.textos.len().max(1) as u16;
            al = n.saturating_mul(al).saturating_add((n - 1).saturating_mul(s.espacio.clamp(0, 255) as u16));
        }
        ex.e16(g.wrapping_add(gadget::LEFTEDGE), x)?;
        ex.e16(g.wrapping_add(gadget::TOPEDGE), y)?;
        ex.e16(g.wrapping_add(gadget::WIDTH), an)?;
        ex.e16(g.wrapping_add(gadget::HEIGHT), al)?;
        ex.e16(g.wrapping_add(gadget::FLAGS), flags)?;
        ex.e16(g.wrapping_add(gadget::ACTIVATION), activacion)?;
        ex.e16(g.wrapping_add(gadget::GADGETTYPE), tipo_i)?;
        ex.e16(g.wrapping_add(gadget::GADGETID), ex.l16(ng.wrapping_add(newgadget::GADGETID))?)?;
        ex.e32(g.wrapping_add(gadget::USERDATA), ex.l32(ng.wrapping_add(newgadget::USERDATA))?)?;
        s.bloques = bloques;
        self.gt.gadgets.insert(g, s);
        // Al final de la lista de `previous`.
        let mut ultimo = prev;
        for _ in 0..10_000 {
            let n = ex.l32(ultimo.wrapping_add(gadget::NEXTGADGET))?;
            if n == 0 {
                break;
            }
            ultimo = n;
        }
        ex.e32(ultimo.wrapping_add(gadget::NEXTGADGET), g)?;
        Ok(g)
    }

    /// `FreeGadgets(glist)`: suelta los gadgets de GadTools de la lista (los demás se quedan).
    fn free_gadgets<M: Memoria>(&mut self, ex: &mut Exec<M>, glist: u32) -> Result<(), Fallo> {
        let mut g = glist;
        let mut vueltas = 0;
        while g != 0 && vueltas < 10_000 {
            vueltas += 1;
            let siguiente = ex.l32(g.wrapping_add(gadget::NEXTGADGET))?;
            match self.gt.gadgets.remove(&g) {
                Some(s) => {
                    if matches!(self.gt.activo, Some(Activo::Texto { g: x, .. }) | Some(Activo::Pulsado { g: x, .. }) | Some(Activo::Arrastre { g: x, .. }) if x == g)
                    {
                        self.gt.activo = None;
                    }
                    for (d, n) in s.bloques {
                        ex.free_mem(d, n)?;
                    }
                }
                None => {
                    if g == glist {
                        return Err(Fallo::Sospechoso { dir: g });
                    }
                }
            }
            g = siguiente;
        }
        Ok(())
    }

    /// La tarea `t` ha muerto: sus gadgets y sus `VisualInfo` (la memoria la suelta exec).
    pub(crate) fn gadtools_tarea_muerta(&mut self, t: u32) {
        self.gt.gadgets.retain(|_, g| g.tarea != t);
        self.gt.visuales.retain(|_, x| *x != t);
        self.gt.activo = None;
        self.menus_tarea_muerta(t);
    }

    // --- Atributos ---

    fn gt_set_attrs<M: Memoria>(&mut self, ex: &mut Exec<M>, g: u32, w: u32, tags: u32) -> Result<(), Fallo> {
        if !self.gt.gadgets.contains_key(&g) {
            return Err(Fallo::Sospechoso { dir: g });
        }
        let etiquetas = leer_etiquetas(ex, tags)?;
        let mut s = self.gt.gadgets[&g].clone();
        for (t, x) in etiquetas {
            match (s.tipo, t) {
                (_, gt::GA_DISABLED) => {
                    let f = ex.l16(g.wrapping_add(gadget::FLAGS))?;
                    let f = if x != 0 { f | gadget::GFLG_DISABLED } else { f & !gadget::GFLG_DISABLED };
                    ex.e16(g.wrapping_add(gadget::FLAGS), f)?;
                }
                (kind::CHECKBOX, gt::CB_CHECKED) => s.valor = (x != 0) as i32,
                (kind::MX, gt::MX_ACTIVE) | (kind::CYCLE, gt::CY_ACTIVE) => s.valor = x as i32,
                (kind::CYCLE, gt::CY_LABELS) => s.textos = opciones(ex, x),
                (kind::TEXT, gt::TX_TEXT) => s.texto = cadena_corta(ex, x, 256),
                (kind::NUMBER, gt::NM_NUMBER) => s.valor = x as i32,
                (kind::SLIDER, gt::SL_MIN) => s.min = x as i32,
                (kind::SLIDER, gt::SL_MAX) => s.max = x as i32,
                (kind::SLIDER, gt::SL_LEVEL) | (kind::SCROLLER, gt::SC_TOP) => s.valor = x as i32,
                (kind::SCROLLER, gt::SC_TOTAL) => s.max = x as i32,
                (kind::SCROLLER, gt::SC_VISIBLE) => s.visibles = x as i32,
                (kind::PALETTE, gt::PA_COLOR) => s.valor = x as i32,
                (kind::LISTVIEW, gt::LV_LABELS) => s.lista = x,
                (kind::LISTVIEW, gt::LV_TOP) => s.arriba = x as i32,
                (kind::LISTVIEW, gt::LV_SELECTED) => s.valor = x as i32,
                (kind::STRING, gt::ST_STRING) | (kind::INTEGER, gt::IN_NUMBER) => {
                    let max = ex.l16(s.info.wrapping_add(stringinfo::MAXCHARS))? as usize;
                    let nuevo = if s.tipo == kind::INTEGER {
                        ex.e32(s.info.wrapping_add(stringinfo::LONGINT), x)?;
                        alloc::format!("{}", x as i32).into_bytes()
                    } else {
                        cadena_corta(ex, x, max.saturating_sub(1))
                    };
                    let buf = ex.l32(s.info.wrapping_add(stringinfo::BUFFER))?;
                    ex.escribir_bytes(buf, &nuevo)?;
                    ex.e8(buf + nuevo.len() as u32, 0)?;
                    ex.e16(s.info.wrapping_add(stringinfo::NUMCHARS), nuevo.len() as u16)?;
                    ex.e16(s.info.wrapping_add(stringinfo::BUFFERPOS), nuevo.len() as u16)?;
                }
                _ => {}
            }
        }
        if s.tipo == kind::SLIDER {
            s.valor = s.valor.clamp(s.min.min(s.max), s.max.max(s.min));
        }
        self.gt.gadgets.insert(g, s);
        if w != 0 && self.intui.ventanas.contains_key(&w) {
            self.dibujar_gt(ex, w, g);
        }
        Ok(())
    }

    /// `GT_GetGadgetAttrsA`: escribe en cada `ti_Data` (un puntero a LONG) el valor de su atributo. Devuelve cuántos.
    fn gt_get_attrs<M: Memoria>(&mut self, ex: &mut Exec<M>, g: u32, tags: u32) -> Result<u32, Fallo> {
        let Some(s) = self.gt.gadgets.get(&g).cloned() else { return Err(Fallo::Sospechoso { dir: g }) };
        let mut n = 0;
        for (t, p) in leer_etiquetas(ex, tags)? {
            let v = match (s.tipo, t) {
                (_, gt::GA_DISABLED) => (ex.l16(g.wrapping_add(gadget::FLAGS))? & gadget::GFLG_DISABLED != 0) as u32,
                (kind::CHECKBOX, gt::CB_CHECKED)
                | (kind::MX, gt::MX_ACTIVE)
                | (kind::CYCLE, gt::CY_ACTIVE)
                | (kind::NUMBER, gt::NM_NUMBER)
                | (kind::SLIDER, gt::SL_LEVEL)
                | (kind::SCROLLER, gt::SC_TOP)
                | (kind::PALETTE, gt::PA_COLOR)
                | (kind::LISTVIEW, gt::LV_SELECTED) => s.valor as u32,
                (kind::SLIDER, gt::SL_MIN) => s.min as u32,
                (kind::SLIDER, gt::SL_MAX) => s.max as u32,
                (kind::LISTVIEW, gt::LV_TOP) => s.arriba as u32,
                (kind::LISTVIEW, gt::LV_LABELS) => s.lista,
                (kind::STRING, gt::ST_STRING) => ex.l32(s.info.wrapping_add(stringinfo::BUFFER))?,
                (kind::INTEGER, gt::IN_NUMBER) => ex.l32(s.info.wrapping_add(stringinfo::LONGINT))?,
                _ => continue,
            };
            ex.e32(p, v)?;
            n += 1;
        }
        Ok(n)
    }

    // --- Listas de gadgets de intuition ---

    /// `AddGList(window, gadget, position, numgad, requester)`: mete `numgad` gadgets (-1, todos) de la lista de
    /// `gadget` en la de la ventana, en `position` (-1 o más allá: al final). Devuelve la posición.
    pub(crate) fn add_g_list<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32, g: u32, pos: i32, n: i32) -> Result<u32, Fallo> {
        self.ventana(w)?;
        if g == 0 {
            return Ok(u32::MAX);
        }
        // El último de los que entran.
        let mut ultimo = g;
        let mut k = 1;
        while n < 0 || k < n {
            let s = ex.l32(ultimo.wrapping_add(gadget::NEXTGADGET))?;
            if s == 0 || k > 10_000 {
                break;
            }
            ultimo = s;
            k += 1;
        }
        let mut anterior = w.wrapping_add(win::FIRSTGADGET) - gadget::NEXTGADGET;
        let mut p = 0;
        loop {
            let s = ex.l32(anterior.wrapping_add(gadget::NEXTGADGET))?;
            if s == 0 || (pos >= 0 && p >= pos) || p > 10_000 {
                break;
            }
            anterior = s;
            p += 1;
        }
        let resto = ex.l32(anterior.wrapping_add(gadget::NEXTGADGET))?;
        ex.e32(ultimo.wrapping_add(gadget::NEXTGADGET), resto)?;
        ex.e32(anterior.wrapping_add(gadget::NEXTGADGET), g)?;
        Ok(p as u32)
    }

    /// `RemoveGList(window, gadget, numgad)`: saca `numgad` gadgets (-1, hasta el final) desde `gadget`. Devuelve la
    /// posición que tenía, o -1 si no estaba.
    pub(crate) fn remove_g_list<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32, g: u32, n: i32) -> Result<u32, Fallo> {
        self.ventana(w)?;
        let mut anterior = w.wrapping_add(win::FIRSTGADGET) - gadget::NEXTGADGET;
        let mut p = 0;
        loop {
            let s = ex.l32(anterior.wrapping_add(gadget::NEXTGADGET))?;
            if s == 0 || p > 10_000 {
                return Ok(u32::MAX);
            }
            if s == g {
                break;
            }
            anterior = s;
            p += 1;
        }
        let mut ultimo = g;
        let mut k = 1;
        while n < 0 || k < n {
            let s = ex.l32(ultimo.wrapping_add(gadget::NEXTGADGET))?;
            if s == 0 || k > 10_000 {
                break;
            }
            ultimo = s;
            k += 1;
        }
        let resto = ex.l32(ultimo.wrapping_add(gadget::NEXTGADGET))?;
        ex.e32(anterior.wrapping_add(gadget::NEXTGADGET), resto)?;
        ex.e32(ultimo.wrapping_add(gadget::NEXTGADGET), 0)?;
        if matches!(self.gt.activo, Some(Activo::Texto { w: x, .. }) if x == w) {
            self.gt.activo = None;
        }
        Ok(p as u32)
    }

    /// `OnGadget`/`OffGadget`: cambia `GFLG_DISABLED` y lo vuelve a dibujar.
    pub(crate) fn on_off_gadget<M: Memoria>(&mut self, ex: &mut Exec<M>, g: u32, w: u32, si: bool) -> Result<(), Fallo> {
        let f = ex.l16(g.wrapping_add(gadget::FLAGS))?;
        ex.e16(g.wrapping_add(gadget::FLAGS), if si { f & !gadget::GFLG_DISABLED } else { f | gadget::GFLG_DISABLED })?;
        if self.intui.ventanas.contains_key(&w) {
            self.dibujar_gt(ex, w, g);
        }
        Ok(())
    }

    /// Dibuja `n` gadgets (-1, todos) de la lista desde `g` en la ventana `w`. Con `solo_gt`, solo los de GadTools.
    pub(crate) fn dibujar_lista<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32, g: u32, n: i32, _solo_gt: bool) {
        let mut g = g;
        let mut k = 0;
        while g != 0 && (n < 0 || k < n) && k < 10_000 {
            self.dibujar_gt(ex, w, g);
            g = ex.l32(g.wrapping_add(gadget::NEXTGADGET)).unwrap_or(0);
            k += 1;
        }
    }

    // --- Dibujo ---

    fn pluma(&self, i: usize) -> u32 {
        self.color(self.intui.wb.cm, self.intui.wb.plumas[i] as u8)
    }

    /// El rectángulo de un gadget en la ventana.
    fn caja_de<M: Memoria>(ex: &Exec<M>, g: u32) -> Option<Rect> {
        let x = ex.l16(g.wrapping_add(gadget::LEFTEDGE)).ok()? as i16 as i32;
        let y = ex.l16(g.wrapping_add(gadget::TOPEDGE)).ok()? as i16 as i32;
        let an = ex.l16(g.wrapping_add(gadget::WIDTH)).ok()? as i16 as i32;
        let al = ex.l16(g.wrapping_add(gadget::HEIGHT)).ok()? as i16 as i32;
        (an > 0 && al > 0).then(|| Rect::nuevo(x, y, x + an - 1, y + al - 1))
    }

    /// El rectángulo de un gadget en las coordenadas de la ventana `w`: en una ventana `GIMMEZEROZERO`, los que no
    /// llevan `GTYP_GZZGADGET` van en la capa de dentro (RKRM: Libraries, "Intuition Gadgets"), desde la esquina de
    /// dentro de los bordes.
    fn caja_en<M: Memoria>(ex: &Exec<M>, w: u32, g: u32) -> Option<Rect> {
        let r = Self::caja_de(ex, g)?;
        let gzz = ex.l32(w.wrapping_add(win::FLAGS)).unwrap_or(0) & crate::estructuras::wflg::GIMMEZEROZERO != 0;
        if gzz && ex.l16(g.wrapping_add(gadget::GADGETTYPE)).unwrap_or(0) & gadget::GTYP_GZZGADGET == 0 {
            let dx = ex.l8(w.wrapping_add(win::BORDERLEFT)).unwrap_or(0) as i32;
            let dy = ex.l8(w.wrapping_add(win::BORDERTOP)).unwrap_or(0) as i32;
            return Some(Rect::nuevo(r.x0 + dx, r.y0 + dy, r.x1 + dx, r.y1 + dy));
        }
        Some(r)
    }

    /// Los elementos de la lista de un listview (sus `ln_Name`).
    fn elementos<M: Memoria>(ex: &Exec<M>, lista: u32) -> Vec<Vec<u8>> {
        let mut v = Vec::new();
        if lista == 0 || lista == u32::MAX {
            return v;
        }
        let mut n = ex.l32(lista.wrapping_add(list::LH_HEAD)).unwrap_or(0);
        while n != 0 && (v.len() as u32) < MAX_NODOS {
            let s = ex.l32(n.wrapping_add(node::LN_SUCC)).unwrap_or(0);
            if s == 0 {
                break;
            }
            v.push(cadena_corta(ex, ex.l32(n.wrapping_add(node::LN_NAME)).unwrap_or(0), 128));
            n = s;
        }
        v
    }

    /// Dibuja un gadget de GadTools (los demás, nada todavía) en el lienzo de su ventana.
    pub(crate) fn dibujar_gt<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32, g: u32) {
        let Some(s) = self.gt.gadgets.get(&g).cloned() else { return };
        if s.tipo == kind::CONTEXTO {
            return;
        }
        let Some(r) = Self::caja_en(ex, w, g) else { return };
        let Some(bm) = self.intui.ventanas.get(&w).map(|v| v.bm) else { return };
        let desactivado = ex.l16(g.wrapping_add(gadget::FLAGS)).unwrap_or(0) & gadget::GFLG_DISABLED != 0;
        let (claro, oscuro, tinta, fondo, relleno, tinta_rel, bloque) = (
            self.pluma(dri::SHINEPEN),
            self.pluma(dri::SHADOWPEN),
            self.pluma(dri::TEXTPEN),
            self.pluma(dri::BACKGROUNDPEN),
            self.pluma(dri::FILLPEN),
            self.pluma(dri::FILLTEXTPEN),
            self.pluma(dri::BLOCKPEN),
        );
        let paleta: Vec<u32> = (0..s.colores.min(256)).map(|i| self.color(self.intui.wb.cm, (i + s.desplaz) as u8)).collect();
        let elementos = if s.tipo == kind::LISTVIEW { Self::elementos(ex, s.lista) } else { Vec::new() };
        let (cadena, cursor) = if s.info != 0 {
            let buf = ex.l32(s.info.wrapping_add(stringinfo::BUFFER)).unwrap_or(0);
            let max = ex.l16(s.info.wrapping_add(stringinfo::MAXCHARS)).unwrap_or(1) as usize;
            (cadena_corta(ex, buf, max), ex.l16(s.info.wrapping_add(stringinfo::BUFFERPOS)).unwrap_or(0) as usize)
        } else {
            (Vec::new(), 0)
        };
        let activo_texto = matches!(self.gt.activo, Some(Activo::Texto { g: x, .. }) if x == g);
        let glifos = &self.intui.glifos;
        let Some(l) = self.gfx.lienzos.get_mut(&bm) else { return };
        let fa = fuente::ALTO as i32;
        let fw = fuente::ANCHO as i32;
        let centro_y = |r: &Rect| r.y0 + (r.y1 - r.y0 + 1 - fa) / 2;
        let mut dano = r;
        // El fondo de la caja (las etiquetas de fuera se pintan sobre lo que haya).
        llenar(l, r, fondo);
        match s.tipo {
            kind::BUTTON => {
                bisel(l, r, claro, oscuro, s.pulsado);
                if s.pulsado {
                    llenar(l, Rect::nuevo(r.x0 + 2, r.y0 + 1, r.x1 - 2, r.y1 - 1), relleno);
                }
            }
            kind::CHECKBOX => {
                bisel(l, r, claro, oscuro, false);
                if s.valor != 0 {
                    // Una marca: dos trazos.
                    let (cx, cy) = (r.x0 + 6, r.y0 + 5);
                    for i in 0..3 {
                        llenar(l, Rect::nuevo(cx + i * 2, cy + i - 1, cx + i * 2 + 1, cy + i), tinta);
                    }
                    for i in 0..5 {
                        llenar(l, Rect::nuevo(cx + 6 + i * 2, cy + 1 - i, cx + 7 + i * 2, cy + 2 - i), tinta);
                    }
                }
            }
            kind::MX => {
                let alto1 = 9;
                for (i, t) in s.textos.iter().enumerate() {
                    let y = r.y0 + i as i32 * (alto1 + s.espacio);
                    let b = Rect::nuevo(r.x0, y, r.x0 + 16, y + alto1 - 1);
                    let elegido = i as i32 == s.valor;
                    bisel(l, b, claro, oscuro, elegido);
                    if elegido {
                        llenar(l, Rect::nuevo(b.x0 + 4, b.y0 + 2, b.x1 - 4, b.y1 - 2), relleno);
                    }
                    let tx = b.x1 + 1 + 6;
                    texto(l, glifos, tx, y + (alto1 - fa) / 2 + 1, t, tinta, i32::MAX);
                    dano = dano.union(&Rect::nuevo(tx, y, tx + fw * t.len() as i32, y + alto1));
                }
            }
            kind::CYCLE => {
                bisel(l, r, claro, oscuro, s.pulsado);
                if s.pulsado {
                    llenar(l, Rect::nuevo(r.x0 + 2, r.y0 + 1, r.x1 - 2, r.y1 - 1), relleno);
                }
                // El dibujo del ciclo (una flecha en círculo) y la separación.
                let (gx, gy) = (r.x0 + 6, centro_y(&r));
                let c = if s.pulsado { tinta_rel } else { tinta };
                llenar(l, Rect::nuevo(gx + 1, gy, gx + 5, gy), c);
                llenar(l, Rect::nuevo(gx, gy + 1, gx, gy + 6), c);
                llenar(l, Rect::nuevo(gx + 6, gy + 1, gx + 6, gy + 2), c);
                llenar(l, Rect::nuevo(gx + 1, gy + 7, gx + 5, gy + 7), c);
                llenar(l, Rect::nuevo(gx + 4, gy + 2, gx + 8, gy + 2), c);
                llenar(l, Rect::nuevo(gx + 5, gy + 3, gx + 7, gy + 3), c);
                llenar(l, Rect::nuevo(gx + 6, gy + 4, gx + 6, gy + 4), c);
                llenar(l, Rect::nuevo(r.x0 + 18, r.y0 + 2, r.x0 + 18, r.y1 - 2), oscuro);
                llenar(l, Rect::nuevo(r.x0 + 19, r.y0 + 2, r.x0 + 19, r.y1 - 2), claro);
                let t = s.textos.get(s.valor.max(0) as usize).cloned().unwrap_or_default();
                let zona = Rect::nuevo(r.x0 + 20, r.y0, r.x1, r.y1);
                let tx = zona.x0 + ((zona.x1 - zona.x0 + 1) - fw * t.len() as i32).max(0) / 2;
                texto(l, glifos, tx, centro_y(&r) + 1, &t, c, r.x1 - 2);
            }
            kind::STRING | kind::INTEGER => {
                bisel(l, r, claro, oscuro, false);
                bisel(l, Rect::nuevo(r.x0 + 2, r.y0 + 1, r.x1 - 2, r.y1 - 1), claro, oscuro, true);
                let tx = r.x0 + 6;
                let ty = centro_y(&r) + 1;
                let fin = texto(l, glifos, tx, ty, &cadena, tinta, r.x1 - 4);
                if activo_texto {
                    let cx = tx + fw * cursor as i32;
                    if cx + fw <= r.x1 - 3 {
                        llenar(l, Rect::nuevo(cx, ty, cx + fw - 1, ty + fa - 1), relleno);
                        if let Some(&ch) = cadena.get(cursor) {
                            texto(l, glifos, cx, ty, &[ch], tinta_rel, r.x1 - 3);
                        }
                    }
                }
                let _ = fin;
            }
            kind::TEXT | kind::NUMBER => {
                if s.borde {
                    bisel(l, r, claro, oscuro, true);
                }
                let t = if s.tipo == kind::NUMBER { alloc::format!("{}", s.valor).into_bytes() } else { s.texto.clone() };
                let tx = if s.borde { r.x0 + 6 } else { r.x0 };
                texto(l, glifos, tx, centro_y(&r) + 1, &t, tinta, r.x1 - 2);
            }
            kind::SLIDER | kind::SCROLLER => {
                bisel(l, r, claro, oscuro, false);
                let dentro = Rect::nuevo(r.x0 + 4, r.y0 + 2, r.x1 - 4, r.y1 - 2);
                let mando = Self::mando(&s, dentro);
                bisel(l, mando, claro, oscuro, false);
                llenar(l, Rect::nuevo(mando.x0 + 2, mando.y0 + 1, mando.x1 - 2, mando.y1 - 1), relleno);
                if s.tipo == kind::SLIDER && !s.formato.is_empty() {
                    let t = formatear(&s.formato, s.valor);
                    let ancho = fw * s.visibles.max(t.len() as i32);
                    let tx = r.x0 - 4 - ancho;
                    let caja = Rect::nuevo(tx, r.y0, r.x0 - 4, r.y1);
                    llenar(l, caja, fondo);
                    texto(l, glifos, tx + ancho - fw * t.len() as i32, centro_y(&r) + 1, &t, tinta, r.x0 - 3);
                    dano = dano.union(&caja);
                }
            }
            kind::PALETTE => {
                bisel(l, r, claro, oscuro, false);
                let ind = if s.indicador > 0 { s.indicador } else { 0 };
                if ind > 0 {
                    let ri = Rect::nuevo(r.x0 + 4, r.y0 + 2, r.x0 + ind - 2, r.y1 - 2);
                    bisel(l, ri, claro, oscuro, true);
                    let c = paleta.get((s.valor - s.desplaz as i32).max(0) as usize).copied().unwrap_or(fondo);
                    llenar(l, Rect::nuevo(ri.x0 + 2, ri.y0 + 1, ri.x1 - 2, ri.y1 - 1), c);
                }
                for (i, cel) in Self::celdas(&s, r).into_iter().enumerate() {
                    let c = paleta.get(i).copied().unwrap_or(fondo);
                    if i as i32 + s.desplaz as i32 == s.valor {
                        llenar(l, cel, tinta);
                        llenar(l, Rect::nuevo(cel.x0 + 2, cel.y0 + 2, cel.x1 - 2, cel.y1 - 2), c);
                    } else {
                        llenar(l, Rect::nuevo(cel.x0 + 1, cel.y0 + 1, cel.x1 - 1, cel.y1 - 1), c);
                    }
                }
            }
            kind::LISTVIEW => {
                let lista = Rect::nuevo(r.x0, r.y0, r.x1 - 16, r.y1);
                bisel(l, lista, claro, oscuro, true);
                let alto_el = fa + 1;
                let filas = ((lista.y1 - lista.y0 - 3) / alto_el).max(0);
                for f in 0..filas {
                    let i = s.arriba + f;
                    let Some(t) = elementos.get(i.max(0) as usize) else { break };
                    let y = lista.y0 + 2 + f * alto_el;
                    let elegido = i == s.valor;
                    if elegido {
                        llenar(l, Rect::nuevo(lista.x0 + 2, y, lista.x1 - 2, y + alto_el - 1), relleno);
                    }
                    texto(l, glifos, lista.x0 + 4, y + 1, t, if elegido { tinta_rel } else { tinta }, lista.x1 - 2);
                }
                // La barra: un deslizador y dos flechas.
                let barra = Rect::nuevo(r.x1 - 15, r.y0, r.x1, r.y1 - 16);
                bisel(l, barra, claro, oscuro, false);
                let total = elementos.len() as i32;
                if total > filas && filas > 0 {
                    let alto = (barra.y1 - barra.y0 - 3).max(4);
                    let km = (alto * filas / total).max(4);
                    let ky = barra.y0 + 2 + (alto - km) * s.arriba.clamp(0, total - filas) / (total - filas).max(1);
                    bisel(l, Rect::nuevo(barra.x0 + 4, ky, barra.x1 - 4, ky + km), claro, oscuro, false);
                } else {
                    bisel(l, Rect::nuevo(barra.x0 + 4, barra.y0 + 2, barra.x1 - 4, barra.y1 - 2), claro, oscuro, false);
                }
                for (k, y0) in [(0, r.y1 - 15), (1, r.y1 - 7)] {
                    let b = Rect::nuevo(r.x1 - 15, y0, r.x1, y0 + 7);
                    bisel(l, b, claro, oscuro, false);
                    let cx = b.x0 + 8;
                    for i in 0..3 {
                        let fy = if k == 0 { b.y0 + 2 + i } else { b.y1 - 2 - i };
                        llenar(l, Rect::nuevo(cx - i, fy, cx + i, fy), tinta);
                    }
                }
            }
            _ => {}
        }
        // La etiqueta.
        if !s.etiqueta.is_empty() && s.tipo != kind::MX || (s.tipo == kind::MX && !s.etiqueta.is_empty()) {
            let t = &s.etiqueta;
            let ancho = fw * t.len() as i32;
            let c = if s.pulsado && s.lugar == gt::PLACETEXT_IN { tinta_rel } else { tinta };
            let mut izq = r.x0;
            if s.tipo == kind::SLIDER && !s.formato.is_empty() {
                izq -= 4 + fw * s.visibles.max(1);
            }
            let (tx, ty) = match s.lugar {
                gt::PLACETEXT_IN => (r.x0 + ((r.x1 - r.x0 + 1) - ancho) / 2, centro_y(&r) + 1),
                gt::PLACETEXT_RIGHT => (r.x1 + 1 + 6, centro_y(&r) + 1),
                gt::PLACETEXT_ABOVE => (r.x0 + ((r.x1 - r.x0 + 1) - ancho) / 2, r.y0 - fa - 2),
                gt::PLACETEXT_BELOW => (r.x0 + ((r.x1 - r.x0 + 1) - ancho) / 2, r.y1 + 3),
                _ => (izq - 6 - ancho, if s.tipo == kind::MX { r.y0 + 1 } else { centro_y(&r) + 1 }),
            };
            texto(l, glifos, tx, ty, t, c, i32::MAX);
            dano = dano.union(&Rect::nuevo(tx, ty, tx + ancho, ty + fa));
        }
        if desactivado {
            // Fantasma: la trama de puntos de BLOCKPEN, como intuition.
            for y in r.y0..=r.y1 {
                for x in r.x0..=r.x1 {
                    if (x + y) % 2 == 0 && (y % 2 == 0) {
                        l.poner(x, y, bloque);
                    }
                }
            }
        }
        l.danar(dano.interseccion(&l.rect()));
    }

    /// El mando de un deslizador (horizontal si es más ancho que alto) dentro de `dentro`.
    fn mando(s: &Gt, dentro: Rect) -> Rect {
        let (rango, pos, vis) = if s.tipo == kind::SLIDER {
            ((s.max - s.min).max(0) + 1, s.valor - s.min, 1)
        } else {
            (s.max.max(1), s.valor, s.visibles.max(1))
        };
        let horizontal = dentro.x1 - dentro.x0 >= dentro.y1 - dentro.y0;
        let largo = if horizontal { dentro.x1 - dentro.x0 + 1 } else { dentro.y1 - dentro.y0 + 1 };
        let mando = (largo * vis.min(rango) / rango.max(1)).max(6).min(largo);
        let libre = largo - mando;
        let pasos = (rango - vis).max(1);
        let p = libre * pos.clamp(0, pasos) / pasos;
        if horizontal {
            Rect::nuevo(dentro.x0 + p, dentro.y0, dentro.x0 + p + mando - 1, dentro.y1)
        } else {
            Rect::nuevo(dentro.x0, dentro.y0 + p, dentro.x1, dentro.y0 + p + mando - 1)
        }
    }

    /// Las celdas de los colores de una paleta, en orden (filas y columnas que más se acerquen a cuadradas).
    fn celdas(s: &Gt, r: Rect) -> Vec<Rect> {
        let n = s.colores.max(1) as i32;
        let x0 = r.x0 + if s.indicador > 0 { s.indicador + 2 } else { 4 };
        let zona = Rect::nuevo(x0, r.y0 + 2, r.x1 - 4, r.y1 - 2);
        let (an, al) = ((zona.x1 - zona.x0 + 1).max(1), (zona.y1 - zona.y0 + 1).max(1));
        let mut mejor = (1, n);
        for filas in 1..=n {
            if n % filas != 0 {
                continue;
            }
            let cols = n / filas;
            let (cw, ch) = (an / cols, al / filas);
            if cw > 0 && ch > 0 && (cw - ch).abs() < (an / mejor.1 - al / mejor.0).abs() {
                mejor = (filas, cols);
            }
        }
        let (filas, cols) = mejor;
        let mut v = Vec::new();
        for i in 0..n {
            let (f, c) = (i / cols, i % cols);
            v.push(Rect::nuevo(
                zona.x0 + c * an / cols,
                zona.y0 + f * al / filas,
                zona.x0 + (c + 1) * an / cols - 1,
                zona.y0 + (f + 1) * al / filas - 1,
            ));
        }
        v
    }

    /// `DrawBevelBoxA`: un marco en relieve en un `RastPort` (con `GTBB_Recessed`, hundido; con
    /// `GTBB_FrameType` = `BBFT_RIDGE`, doble).
    #[allow(clippy::too_many_arguments)]
    fn draw_bevel_box<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, x: i32, y: i32, an: i32, al: i32, tags: u32) -> Result<(), Fallo> {
        let e = leer_etiquetas(ex, tags)?;
        let hundido = e.iter().any(|(t, d)| *t == gt::BB_RECESSED && *d != 0);
        let cresta = e.iter().any(|(t, d)| *t == gt::BB_FRAMETYPE && *d == gt::BBFT_RIDGE);
        if an < 2 || al < 2 {
            return Ok(());
        }
        let antes = self.leer_rp(ex, rp)?.fg as u32;
        let (claro, oscuro) = (self.intui.wb.plumas[dri::SHINEPEN] as u32, self.intui.wb.plumas[dri::SHADOWPEN] as u32);
        let caja = |s: &mut Self, ex: &mut Exec<M>, x0: i32, y0: i32, x1: i32, y1: i32, h: bool| -> Result<(), Fallo> {
            let (a, b) = if h { (oscuro, claro) } else { (claro, oscuro) };
            s.set_a_pen(ex, rp, a)?;
            s.rect_fill(ex, rp, x0, y0, x1 - 1, y0)?;
            s.rect_fill(ex, rp, x0, y0, x0 + 1, y1 - 1)?;
            s.set_a_pen(ex, rp, b)?;
            s.rect_fill(ex, rp, x0 + 1, y1, x1, y1)?;
            s.rect_fill(ex, rp, x1 - 1, y0 + 1, x1, y1)
        };
        caja(self, ex, x, y, x + an - 1, y + al - 1, hundido)?;
        if cresta && an > 4 && al > 2 {
            caja(self, ex, x + 2, y + 1, x + an - 3, y + al - 2, !hundido)?;
        }
        self.set_a_pen(ex, rp, antes)
    }

    // --- El ratón y el teclado ---

    /// El gadget (de la lista de la ventana) que hay en `(x, y)`, sin los desactivados.
    fn gadget_en<M: Memoria>(&self, ex: &Exec<M>, w: u32, x: i32, y: i32) -> Option<u32> {
        let mut g = ex.l32(w.wrapping_add(win::FIRSTGADGET)).ok()?;
        let mut k = 0;
        while g != 0 && k < 10_000 {
            k += 1;
            let tipo = ex.l16(g.wrapping_add(gadget::GADGETTYPE)).unwrap_or(0);
            let flags = ex.l16(g.wrapping_add(gadget::FLAGS)).unwrap_or(0);
            if tipo & gadget::GTYP_SYSGADGET == 0
                && flags & gadget::GFLG_DISABLED == 0
                && Self::caja_en(ex, w, g).is_some_and(|r| r.contiene(x, y))
            {
                let gtk = self.gt.gadgets.get(&g).map(|s| s.tipo);
                if !matches!(gtk, Some(kind::CONTEXTO) | Some(kind::TEXT) | Some(kind::NUMBER)) {
                    return Some(g);
                }
            }
            g = ex.l32(g.wrapping_add(gadget::NEXTGADGET)).unwrap_or(0);
        }
        None
    }

    /// El botón izquierdo bajó en `(x, y)` de la ventana: si es un gadget, lo suyo. `None` si no hay gadget ahí.
    pub(crate) fn gadget_pulsar<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32, x: i32, y: i32) -> Option<Result<bool, Fallo>> {
        // Un gadget de texto activo pierde el cursor.
        if let Some(Activo::Texto { w: tw, g: tg }) = self.gt.activo {
            self.gt.activo = None;
            if self.intui.ventanas.contains_key(&tw) {
                self.dibujar_gt(ex, tw, tg);
            }
        }
        let g = self.gadget_en(ex, w, x, y)?;
        Some(self.pulsar(ex, w, g, x, y))
    }

    fn pulsar<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32, g: u32, x: i32, y: i32) -> Result<bool, Fallo> {
        let Some(mut s) = self.gt.gadgets.get(&g).cloned() else {
            // Un gadget de intuition: GADGETDOWN si es GACT_IMMEDIATE; el UP, al soltar.
            self.gt.activo = Some(Activo::Intuition { w, g });
            let act = ex.l16(g.wrapping_add(gadget::ACTIVATION))?;
            if act & gadget::GACT_TOGGLESELECT != 0 {
                let f = ex.l16(g.wrapping_add(gadget::FLAGS))?;
                ex.e16(g.wrapping_add(gadget::FLAGS), f ^ gadget::GFLG_SELECTED)?;
            }
            return if act & gadget::GACT_IMMEDIATE != 0 { self.enviar(ex, w, idcmp::GADGETDOWN, 0, g) } else { Ok(false) };
        };
        let r = Self::caja_en(ex, w, g).unwrap_or(Rect::nuevo(0, 0, 0, 0));
        match s.tipo {
            kind::MX => {
                let alto1 = 9 + s.espacio;
                let i = ((y - r.y0) / alto1.max(1)).clamp(0, s.textos.len() as i32 - 1);
                s.valor = i;
                self.gt.gadgets.insert(g, s);
                self.dibujar_gt(ex, w, g);
                self.enviar(ex, w, idcmp::GADGETDOWN, i as u16, g)
            }
            kind::STRING | kind::INTEGER => {
                let buf = ex.l32(s.info.wrapping_add(stringinfo::BUFFER))?;
                let n = ex.cadena(buf).map(|v| v.len()).unwrap_or(0) as i32;
                let pos = ((x - r.x0 - 6) / fuente::ANCHO as i32).clamp(0, n);
                ex.e16(s.info.wrapping_add(stringinfo::BUFFERPOS), pos as u16)?;
                self.gt.activo = Some(Activo::Texto { w, g });
                self.dibujar_gt(ex, w, g);
                Ok(false)
            }
            kind::SLIDER | kind::SCROLLER => {
                let dentro = Rect::nuevo(r.x0 + 4, r.y0 + 2, r.x1 - 4, r.y1 - 2);
                let m = Self::mando(&s, dentro);
                if m.contiene(x, y) {
                    self.gt.activo = Some(Activo::Arrastre { w, g, dx: x - m.x0 });
                    return self.enviar(ex, w, idcmp::GADGETDOWN, s.valor as u16, g);
                }
                // En el carril: un paso hacia el ratón.
                let paso = if s.tipo == kind::SLIDER { 1 } else { s.visibles.max(1) };
                s.valor += if x < m.x0 || y < m.y0 { -paso } else { paso };
                let (lo, hi) = if s.tipo == kind::SLIDER { (s.min, s.max) } else { (0, (s.max - s.visibles).max(0)) };
                s.valor = s.valor.clamp(lo, hi);
                let v = s.valor;
                self.gt.gadgets.insert(g, s);
                self.dibujar_gt(ex, w, g);
                self.enviar(ex, w, idcmp::GADGETUP, v as u16, g)
            }
            kind::LISTVIEW => {
                let lista = Rect::nuevo(r.x0, r.y0, r.x1 - 16, r.y1);
                let total = Self::elementos(ex, s.lista).len() as i32;
                let filas = ((lista.y1 - lista.y0 - 3) / (fuente::ALTO as i32 + 1)).max(1);
                if x > lista.x1 {
                    // La barra: las flechas mueven una fila; el carril, una página.
                    let d = if y >= r.y1 - 15 { if y >= r.y1 - 7 { 1 } else { -1 } } else if y < r.y0 + (r.y1 - r.y0) / 2 { -filas } else { filas };
                    s.arriba = (s.arriba + d).clamp(0, (total - filas).max(0));
                    self.gt.gadgets.insert(g, s);
                    self.dibujar_gt(ex, w, g);
                    return Ok(false);
                }
                let i = s.arriba + (y - lista.y0 - 2) / (fuente::ALTO as i32 + 1);
                if s.solo_lectura || i < 0 || i >= total {
                    return Ok(false);
                }
                s.valor = i;
                self.gt.gadgets.insert(g, s);
                self.dibujar_gt(ex, w, g);
                self.enviar(ex, w, idcmp::GADGETUP, i as u16, g)
            }
            _ => {
                s.pulsado = matches!(s.tipo, kind::BUTTON | kind::CYCLE);
                self.gt.gadgets.insert(g, s);
                self.gt.activo = Some(Activo::Pulsado { w, g });
                self.dibujar_gt(ex, w, g);
                Ok(false)
            }
        }
    }

    /// El ratón se movió con un gadget pulsado: `true` si era cosa de un gadget (y si hay que replanificar).
    pub(crate) fn gadget_mover<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32, x: i32, y: i32) -> Option<Result<bool, Fallo>> {
        match self.gt.activo? {
            Activo::Pulsado { w: aw, g } if aw == w => {
                let dentro = Self::caja_en(ex, w, g).is_some_and(|r| r.contiene(x, y));
                let s = self.gt.gadgets.get_mut(&g)?;
                let antes = s.pulsado;
                s.pulsado = dentro && matches!(s.tipo, kind::BUTTON | kind::CYCLE);
                if antes != s.pulsado {
                    self.dibujar_gt(ex, w, g);
                }
                Some(Ok(false))
            }
            Activo::Arrastre { w: aw, g, dx } if aw == w => {
                let r = Self::caja_en(ex, w, g)?;
                let mut s = self.gt.gadgets.get(&g)?.clone();
                let dentro = Rect::nuevo(r.x0 + 4, r.y0 + 2, r.x1 - 4, r.y1 - 2);
                let m = Self::mando(&s, dentro);
                let horizontal = dentro.x1 - dentro.x0 >= dentro.y1 - dentro.y0;
                let (largo, mlargo, p) = if horizontal {
                    (dentro.x1 - dentro.x0 + 1, m.x1 - m.x0 + 1, x - dx - dentro.x0)
                } else {
                    (dentro.y1 - dentro.y0 + 1, m.y1 - m.y0 + 1, y - dx - dentro.y0)
                };
                let libre = (largo - mlargo).max(1);
                let (lo, hi) = if s.tipo == kind::SLIDER { (s.min, s.max) } else { (0, (s.max - s.visibles).max(0)) };
                let v = (lo + (p.clamp(0, libre) * (hi - lo) + libre / 2) / libre).clamp(lo, hi);
                if v == s.valor {
                    return Some(Ok(false));
                }
                s.valor = v;
                self.gt.gadgets.insert(g, s);
                self.dibujar_gt(ex, w, g);
                Some(self.enviar(ex, w, idcmp::MOUSEMOVE, v as u16, g))
            }
            _ => None,
        }
    }

    /// El botón izquierdo subió: lo que toque al gadget pulsado. `None` si no había ninguno.
    pub(crate) fn gadget_soltar<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32, x: i32, y: i32) -> Option<Result<bool, Fallo>> {
        match self.gt.activo? {
            Activo::Texto { .. } => None,
            Activo::Intuition { w: aw, g } => {
                self.gt.activo = None;
                let dentro = Self::caja_en(ex, w, g).is_some_and(|r| r.contiene(x, y));
                let act = ex.l16(g.wrapping_add(gadget::ACTIVATION)).unwrap_or(0);
                if aw == w && dentro && act & gadget::GACT_RELVERIFY != 0 {
                    return Some(self.enviar(ex, w, idcmp::GADGETUP, 0, g));
                }
                Some(Ok(false))
            }
            Activo::Arrastre { w: aw, g, .. } => {
                self.gt.activo = None;
                let v = self.gt.gadgets.get(&g).map_or(0, |s| s.valor);
                Some(if aw == w { self.enviar(ex, w, idcmp::GADGETUP, v as u16, g) } else { Ok(false) })
            }
            Activo::Pulsado { w: aw, g } => {
                self.gt.activo = None;
                let dentro = aw == w && Self::caja_en(ex, w, g).is_some_and(|r| r.contiene(x, y));
                let mut s = self.gt.gadgets.get(&g)?.clone();
                s.pulsado = false;
                let mut codigo = None;
                if dentro {
                    match s.tipo {
                        kind::BUTTON => codigo = Some(0),
                        kind::CHECKBOX => {
                            s.valor = (s.valor == 0) as i32;
                            codigo = Some(s.valor);
                        }
                        kind::CYCLE => {
                            let n = s.textos.len().max(1) as i32;
                            let atras = self.intui.calif.bits & (ie::LSHIFT | ie::RSHIFT) != 0;
                            s.valor = (s.valor + if atras { n - 1 } else { 1 }) % n;
                            codigo = Some(s.valor);
                        }
                        kind::PALETTE => {
                            let r = Self::caja_en(ex, w, g)?;
                            if let Some(i) = Self::celdas(&s, r).iter().position(|c| c.contiene(x, y)) {
                                s.valor = i as i32 + s.desplaz as i32;
                                codigo = Some(s.valor);
                            }
                        }
                        _ => {}
                    }
                }
                self.gt.gadgets.insert(g, s);
                if self.intui.ventanas.contains_key(&aw) {
                    self.dibujar_gt(ex, aw, g);
                }
                Some(match codigo {
                    Some(c) => self.enviar(ex, aw, idcmp::GADGETUP, c as u16, g),
                    None => Ok(false),
                })
            }
        }
    }

    /// Una tecla con un gadget de texto activo en la ventana `w`: la edita. `None` si no hay ninguno (la tecla va al
    /// programa como siempre).
    pub(crate) fn gadget_tecla<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32, crudo: Option<u8>, c: Option<u8>) -> Option<Result<bool, Fallo>> {
        let Some(Activo::Texto { w: aw, g }) = self.gt.activo else { return None };
        if aw != w {
            return None;
        }
        Some(self.editar(ex, w, g, crudo, c))
    }

    fn editar<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32, g: u32, crudo: Option<u8>, c: Option<u8>) -> Result<bool, Fallo> {
        let Some(s) = self.gt.gadgets.get(&g).cloned() else { return Ok(false) };
        let si = s.info;
        let buf = ex.l32(si.wrapping_add(stringinfo::BUFFER))?;
        let max = ex.l16(si.wrapping_add(stringinfo::MAXCHARS))? as usize;
        let mut t = cadena_corta(ex, buf, max.saturating_sub(1));
        let mut pos = (ex.l16(si.wrapping_add(stringinfo::BUFFERPOS))? as usize).min(t.len());
        let mut fin = None;
        match (crudo, c) {
            (Some(0x44), _) | (Some(0x43), _) => fin = Some(0u16),
            (Some(0x42), _) => fin = Some(9),
            (Some(0x41), _) if pos > 0 => {
                t.remove(pos - 1);
                pos -= 1;
            }
            (Some(0x46), _) if pos < t.len() => {
                t.remove(pos);
            }
            (Some(0x4F), _) => pos = pos.saturating_sub(1),
            (Some(0x4E), _) => pos = (pos + 1).min(t.len()),
            (_, Some(ch)) if ch >= 32 && ch != 127 => {
                let vale = s.tipo != kind::INTEGER || ch.is_ascii_digit() || (ch == b'-' && pos == 0);
                if vale && t.len() + 1 < max {
                    t.insert(pos, ch);
                    pos += 1;
                }
            }
            _ => {}
        }
        ex.escribir_bytes(buf, &t)?;
        ex.e8(buf + t.len() as u32, 0)?;
        ex.e16(si.wrapping_add(stringinfo::NUMCHARS), t.len() as u16)?;
        ex.e16(si.wrapping_add(stringinfo::BUFFERPOS), pos as u16)?;
        if s.tipo == kind::INTEGER {
            let n: i32 = core::str::from_utf8(&t).ok().and_then(|x| x.parse().ok()).unwrap_or(0);
            ex.e32(si.wrapping_add(stringinfo::LONGINT), n as u32)?;
        }
        if let Some(codigo) = fin {
            self.gt.activo = None;
            self.dibujar_gt(ex, w, g);
            return self.enviar(ex, w, idcmp::GADGETUP, codigo, g);
        }
        self.dibujar_gt(ex, w, g);
        Ok(false)
    }

    /// La ventana `w` se cierra: lo activo en ella se olvida.
    pub(crate) fn gadtools_ventana_cerrada(&mut self, w: u32) {
        let de_w = match self.gt.activo {
            Some(Activo::Pulsado { w: x, .. })
            | Some(Activo::Arrastre { w: x, .. })
            | Some(Activo::Texto { w: x, .. })
            | Some(Activo::Intuition { w: x, .. }) => x == w,
            None => false,
        };
        if de_w {
            self.gt.activo = None;
        }
        self.menu_ventana_cerrada(w);
    }
}
