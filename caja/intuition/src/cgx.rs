//! cybergraphics.library nativa (versión 41): el color verdadero de la caja para los programas que saben de RTG
//! (MUI, picture.datatype, Tachyon).
//!
//! Escrita desde el autodoc público (`cybergraphics.doc`), `cybergraphics.fd` y `cybergraphx/cybergraphics.h` del
//! SDK. No se ha leído código de CyberGraphX, Picasso96, AROS, UAE ni Amiberry.
//!
//! Cómo encaja (diseño en `docs/bitacoras/caja-amiga.md`, 2026-11-15 y 2026-11-15 (2)):
//! - **Los mapas de color verdadero de los programas son chunky** (`chunky.rs`): sus píxeles en la memoria del 68k, en
//!   el formato que pidieron (`BMF_SPECIALFMT`), el de su amigo o el de serie de su profundidad. `LockBitMapTagList` da
//!   su dirección de verdad, sin copiar. Los lienzos (ventanas y pantalla de Workbench: superficies del compositor, en
//!   ARGB fuera de la memoria del 68k) también son de cybergraphics: se ven en el formato del modo de su pantalla, y al
//!   cogerlos se copian a un búfer en ese formato y vuelven al soltarlos.
//! - Los planares (los del programa, `AllocBitMap` de hasta 8 planos) no son de cybergraphics (`CYBRMATTR_ISCYBERGFX`
//!   da 0), pero las funciones de píxeles también pintan en ellos: el color se lleva a la pluma más cercana de la paleta
//!   del Workbench.
//! - **Los modos** son los de la caja (`modos.rs`): varios tamaños y 8, 15, 16, 24 y 32 bits, en un monitor propio.
//! - Las funciones de píxeles recortan como el resto de graphics: al mapa, a la capa de la ventana y a su región de
//!   recorte. Los tamaños son UWORD (como dice el autodoc); las coordenadas de destino, WORD con signo. Convierten desde
//!   y hacia cualquier formato del programa (`RECTFMT_*`, `RGBFB_*` de Picasso96), con el alfa de los de 32 bits.

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

use caja_exec::estructuras::memf;
use caja_exec::{Exec, Memoria, Registros};

use crate::acceso::{Acceso, palabra};
use crate::chunky::Formato;
use crate::modos;
use crate::dibujo::{Planar, mas_cercano};
use crate::graphics::Mapa;
use crate::lienzo::Rect;
use crate::{CajaGrafica, Compositor, Fallo, Tabla};

/// Las funciones de cybergraphics.library 41, desde -30, en el orden de `cybergraphics.fd`; detrás, unas entradas
/// de reserva (de versiones que no tenemos) para que una llamada de más caiga en una función nativa que lo apunta
/// en vez de fuera de la tabla.
pub const FUNCIONES: [&str; 37] = [
    "cgfxPrivate1",
    "cgfxPrivate2",
    "cgfxPrivate3",
    "cgfxPrivate4",
    "IsCyberModeID",
    "BestCModeIDTagList",
    "CModeRequestTagList",
    "AllocCModeListTagList",
    "FreeCModeList",
    "cgfxPrivate5",
    "ScalePixelArray",
    "GetCyberMapAttr",
    "GetCyberIDAttr",
    "ReadRGBPixel",
    "WriteRGBPixel",
    "ReadPixelArray",
    "WritePixelArray",
    "MovePixelArray",
    "cgfxPrivate6",
    "InvertPixelArray",
    "FillPixelArray",
    "DoCDrawMethodTagList",
    "CVideoCtrlTagList",
    "LockBitMapTagList",
    "UnLockBitMap",
    "UnLockBitMapTagList",
    "ExtractColor",
    "cgfxPrivate7",
    "WriteLUTPixelArray",
    "cgfx (LVO -204)",
    "cgfx (LVO -210)",
    "cgfx (LVO -216)",
    "cgfx (LVO -222)",
    "cgfx (LVO -228)",
    "cgfx (LVO -234)",
    "cgfx (LVO -240)",
    "cgfx (LVO -246)",
];

pub const VERSION: u16 = 41;

/// El modo de la pantalla de Workbench (`modos::workbench`): el del escritorio en 32 bits.
pub const MODO_CAJA: u32 = modos::workbench();
/// `INVALID_ID` de `graphics/displayinfo.h`.
pub const INVALID_ID: u32 = 0xFFFF_FFFF;

/// Constantes de `cybergraphx/cybergraphics.h`.
pub mod constantes {
    pub const CYBRMATTR_XMOD: u32 = 0x8000_0001;
    pub const CYBRMATTR_BPPIX: u32 = 0x8000_0002;
    pub const CYBRMATTR_DISPADR: u32 = 0x8000_0003;
    pub const CYBRMATTR_PIXFMT: u32 = 0x8000_0004;
    pub const CYBRMATTR_WIDTH: u32 = 0x8000_0005;
    pub const CYBRMATTR_HEIGHT: u32 = 0x8000_0006;
    pub const CYBRMATTR_DEPTH: u32 = 0x8000_0007;
    pub const CYBRMATTR_ISCYBERGFX: u32 = 0x8000_0008;
    pub const CYBRMATTR_ISLINEARMEM: u32 = 0x8000_0009;

    pub const CYBRIDATTR_PIXFMT: u32 = 0x8000_0001;
    pub const CYBRIDATTR_WIDTH: u32 = 0x8000_0002;
    pub const CYBRIDATTR_HEIGHT: u32 = 0x8000_0003;
    pub const CYBRIDATTR_DEPTH: u32 = 0x8000_0004;
    pub const CYBRIDATTR_BPPIX: u32 = 0x8000_0005;

    pub const CYBRMREQ_TB: u32 = 0x8000_0000 + 0x40000;
    pub const CYBRMREQ_MIN_DEPTH: u32 = CYBRMREQ_TB;
    pub const CYBRMREQ_MAX_DEPTH: u32 = CYBRMREQ_TB + 1;
    pub const CYBRMREQ_MIN_WIDTH: u32 = CYBRMREQ_TB + 2;
    pub const CYBRMREQ_MAX_WIDTH: u32 = CYBRMREQ_TB + 3;
    pub const CYBRMREQ_MIN_HEIGHT: u32 = CYBRMREQ_TB + 4;
    pub const CYBRMREQ_MAX_HEIGHT: u32 = CYBRMREQ_TB + 5;
    pub const CYBRMREQ_CMODEL_ARRAY: u32 = CYBRMREQ_TB + 6;

    pub const CYBRBIDTG_TB: u32 = 0x8000_0000 + 0x50000;
    pub const CYBRBIDTG_DEPTH: u32 = CYBRBIDTG_TB;
    pub const CYBRBIDTG_NOMINAL_WIDTH: u32 = CYBRBIDTG_TB + 1;
    pub const CYBRBIDTG_NOMINAL_HEIGHT: u32 = CYBRBIDTG_TB + 2;

    pub const PIXFMT_LUT8: u32 = 0;
    pub const PIXFMT_RGB16: u32 = 5;
    pub const PIXFMT_RGB24: u32 = 9;
    pub const PIXFMT_ARGB32: u32 = 11;
    pub const PIXFMT_RGBA32: u32 = 13;

    pub const RECTFMT_RGB: u32 = 0;
    pub const RECTFMT_RGBA: u32 = 1;
    pub const RECTFMT_ARGB: u32 = 2;
    pub const RECTFMT_LUT8: u32 = 3;
    pub const RECTFMT_GREY8: u32 = 4;
    /// El formato del propio mapa (V43; Dash lo usa): ARGB en los nativos, una pluma por byte en los planares.
    pub const RECTFMT_RAW: u32 = 5;
    /// Los formatos de Picasso96 (`RGBFB_*`) van detrás, para compartir la conversión: `P96 + RGBFB_*`.
    pub const P96: u32 = 0x100;

    pub const CTABFMT_XRGB8: u32 = 0;

    pub const LBMI_WIDTH: u32 = 0x8400_1001;
    pub const LBMI_HEIGHT: u32 = 0x8400_1002;
    pub const LBMI_DEPTH: u32 = 0x8400_1003;
    pub const LBMI_PIXFMT: u32 = 0x8400_1004;
    pub const LBMI_BYTESPERPIX: u32 = 0x8400_1005;
    pub const LBMI_BYTESPERROW: u32 = 0x8400_1006;
    pub const LBMI_BASEADDRESS: u32 = 0x8400_1007;

    pub const UBMI_UPDATERECTS: u32 = 0x8500_1001;
    pub const UBMI_REALLYUNLOCK: u32 = 0x8500_1002;

    /// `AllocBitMap`: el formato va en los 8 bits de arriba de las opciones.
    pub const BMF_SPECIALFMT: u32 = 1 << 7;

    /// `struct CyberModeNode`: Node (14), ModeText[32], DisplayID, Width, Height, Depth, DisplayTagList.
    pub const CMN_MODETEXT: u32 = 14;
    pub const CMN_DISPLAYID: u32 = 46;
    pub const CMN_WIDTH: u32 = 50;
    pub const CMN_HEIGHT: u32 = 52;
    pub const CMN_DEPTH: u32 = 54;
    pub const CMN_DISPLAYTAGLIST: u32 = 56;
    pub const CMN_TAMANO: u32 = 60;
}

use constantes::*;

/// Tope de rectángulos de `UBMI_UPDATERECTS` (más es una lista rota o en círculo).
const MAX_RECTS: u32 = 16384;

/// Un mapa cogido con `LockBitMapTagList`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Cerrojo {
    pub bm: u32,
    pub bpr: u32,
    pub fmt: Formato,
    pub ancho: u32,
    pub alto: u32,
    pub tarea: u32,
    pub veces: u32,
}

#[derive(Default)]
pub(crate) struct Estado {
    pub base: u32,
    /// Búfer del 68k (el asa que se devuelve) → su cerrojo.
    pub cerrojos: Tabla<Cerrojo>,
    /// Listas de `AllocCModeListTagList`: dirección → (largo, tarea).
    pub listas: Tabla<(u32, u32)>,
}

/// Dónde pinta un `RastPort`, ya comprobado: su mapa, el recorte (mapa, capa y región, en coordenadas del
/// `RastPort`) y dónde cae su (0, 0) en el mapa.
struct Destino {
    bm: u32,
    mapa: Mapa,
    /// Los límites del mapa en coordenadas del `RastPort` (para leer).
    lim: Rect,
    clip: Rect,
    region: Option<Vec<Rect>>,
    dx: i32,
    dy: i32,
    /// El lienzo (ventana, pantalla) que es el mapa, si lo es: se trata como un chunky (sus píxeles también están en la
    /// memoria del 68k) y lo escrito se apunta como cambiado para el compositor.
    lienzo: Option<u32>,
    /// La paleta del mapa: 256 colores ARGB (la del lienzo, o la del Workbench en los planares). En los nativos, solo
    /// si se pidió (hacen falta para las plumas): una llamada de un punto no la arma.
    paleta: Vec<u32>,
}

impl Destino {
    /// Si sus valores son colores (un lienzo o un chunky) y no plumas.
    fn nativo(&self) -> bool {
        self.mapa.de_color()
    }

    /// Los trozos de la fila `y` entre `xa` y `xb` (incluidos) que se pueden pintar.
    fn trozos(&self, y: i32, xa: i32, xb: i32) -> Vec<(i32, i32)> {
        let (xa, xb) = (xa.max(self.clip.x0), xb.min(self.clip.x1));
        if y < self.clip.y0 || y > self.clip.y1 || xb < xa {
            return Vec::new();
        }
        match &self.region {
            None => vec![(xa, xb)],
            Some(r) => r
                .iter()
                .filter(|q| q.y0 <= y && y <= q.y1)
                .filter_map(|q| {
                    let (a, b) = (xa.max(q.x0), xb.min(q.x1));
                    (a <= b).then_some((a, b))
                })
                .collect(),
        }
    }
}

/// Lo que se escribe: colores ARGB o plumas (para el destino se traducen).
enum Valores {
    Argb(Vec<u32>),
    Plumas(Vec<u8>),
}

/// Los formatos de los mapas chunky (`chunky::Formato`) van detrás de todo, para `RECTFMT_RAW`: `MAPA + PIXFMT_*`.
pub(crate) const MAPA: u32 = 0x200;

/// `RGBFB_A8B8G8R8`: de Picasso96, sin `PIXFMT_*` (no se guarda ningún mapa así; se lee y se escribe).
const RGBFB_A8B8G8R8: u32 = 7;

/// El formato de mapa que hay detrás de un código de Picasso96 (`P96 + RGBFB_*`) o de mapa (`MAPA + PIXFMT_*`).
fn formato_de_codigo(fmt: u32) -> Option<Formato> {
    if fmt >= MAPA {
        Formato::de_pixfmt(fmt - MAPA)
    } else if fmt >= P96 {
        Formato::de_rgbfb(fmt - P96)
    } else {
        None
    }
}

/// Los bytes por píxel de un formato (los de cybergraphics, `RECTFMT_*`; los de Picasso96, `P96 + RGBFB_*`; los de
/// los mapas, `MAPA + PIXFMT_*`).
pub(crate) fn bytes_por_pixel(fmt: u32) -> Option<u32> {
    match fmt {
        RECTFMT_RGB => Some(3),
        RECTFMT_RGBA | RECTFMT_ARGB => Some(4),
        RECTFMT_LUT8 | RECTFMT_GREY8 => Some(1),
        _ if fmt == P96 + 1 => Some(1),
        _ if fmt == P96 + RGBFB_A8B8G8R8 => Some(4),
        _ => formato_de_codigo(fmt).map(Formato::bpp),
    }
}

/// Un píxel de un formato de color verdadero a ARGB: el alfa, el que lleve (0 si no lleva).
fn a_argb(fmt: u32, p: &[u8]) -> u32 {
    let c = |a: u8, r: u8, g: u8, b: u8| u32::from_be_bytes([a, r, g, b]);
    match fmt {
        RECTFMT_RGB => c(0, p[0], p[1], p[2]),
        RECTFMT_RGBA => c(p[3], p[0], p[1], p[2]),
        RECTFMT_ARGB => c(p[0], p[1], p[2], p[3]),
        RECTFMT_GREY8 => c(0, p[0], p[0], p[0]),
        _ if fmt == P96 + RGBFB_A8B8G8R8 => c(p[0], p[3], p[2], p[1]),
        _ => formato_de_codigo(fmt).map_or(0, |f| f.leer(p)),
    }
}

/// Un color ARGB a un píxel de un formato de color verdadero (con su alfa, si lo lleva).
fn de_argb(fmt: u32, argb: u32, o: &mut Vec<u8>) {
    let [a, r, g, b] = argb.to_be_bytes();
    match fmt {
        RECTFMT_RGB => o.extend_from_slice(&[r, g, b]),
        RECTFMT_RGBA => o.extend_from_slice(&[r, g, b, a]),
        RECTFMT_ARGB => o.extend_from_slice(&[a, r, g, b]),
        RECTFMT_GREY8 => o.push(((r as u32 * 77 + g as u32 * 150 + b as u32 * 29) >> 8) as u8),
        _ if fmt == P96 + RGBFB_A8B8G8R8 => o.extend_from_slice(&[a, b, g, r]),
        _ => {
            if let Some(f) = formato_de_codigo(fmt) {
                let mut t = [0u8; 4];
                f.escribir(argb, &mut t);
                o.extend_from_slice(&t[..f.bpp() as usize]);
            }
        }
    }
}

/// `p96EncodeColor`: un color ARGB en un formato de Picasso96 (los bytes del píxel, alineados a la derecha).
pub(crate) fn codificar_p96(rgbf: u32, argb: u32) -> u32 {
    let mut o = Vec::new();
    de_argb(P96 + rgbf, argb, &mut o);
    o.iter().fold(0, |v, &b| (v << 8) | b as u32)
}

/// Una pluma por byte (`RECTFMT_LUT8`, `RGBFB_CLUT`).
fn es_de_plumas(fmt: u32) -> bool {
    fmt == RECTFMT_LUT8 || fmt == P96 + 1
}

/// Convierte los bytes de un trozo de fila de origen.
fn convertir(fmt: u32, b: &[u8]) -> Valores {
    if es_de_plumas(fmt) {
        return Valores::Plumas(b.to_vec());
    }
    let bpp = bytes_por_pixel(fmt).unwrap_or(1) as usize;
    Valores::Argb(b.chunks_exact(bpp).map(|p| a_argb(fmt, p)).collect())
}

/// Una dirección `base + fila * modulo + col * bpp` del 68k, o un fallo si se sale de 32 bits.
fn direccion(base: u32, fila: u32, modulo: u32, col: u32, bpp: u32) -> Result<u32, Fallo> {
    let d = base as u64 + fila as u64 * modulo as u64 + col as u64 * bpp as u64;
    u32::try_from(d).map_err(|_| caja_exec::Fallo::FueraDeMemoria { dir: base, largo: 0 }.into())
}

fn u16_de(v: u32) -> i32 {
    (v & 0xFFFF) as i32
}

impl<C: Compositor> CajaGrafica<C> {
    pub(crate) fn instalar_cgx<M: Memoria>(&mut self, ex: &mut Exec<M>) -> Result<(), Fallo> {
        let n0 = self.numeros.cybergraphics;
        let nums: Vec<u16> = (0..FUNCIONES.len() as u16).map(|i| n0 + i).collect();
        self.cgx.base = ex.crear_biblioteca_nativa("cybergraphics.library", VERSION, 1, &nums, 34)?;
        Ok(())
    }

    /// La base de cybergraphics.library.
    pub fn cgx_base(&self) -> u32 {
        self.cgx.base
    }

    /// El número nativo de una función de cybergraphics, por su nombre.
    pub fn numero_cgx(&self, nombre: &str) -> Option<u16> {
        FUNCIONES.iter().position(|n| *n == nombre).map(|i| self.numeros.cybergraphics + i as u16)
    }

    pub(crate) fn es_cgx(&self, n: u16) -> Option<usize> {
        let c0 = self.numeros.cybergraphics;
        (n >= c0 && ((n - c0) as usize) < FUNCIONES.len()).then(|| (n - c0) as usize)
    }

    /// Cuántos mapas siguen cogidos con `LockBitMapTagList` (para las pruebas).
    pub fn cerrojos_cgx(&self) -> usize {
        self.cgx.cerrojos.len()
    }

    pub(crate) fn llamar_cgx<M: Memoria>(&mut self, ex: &mut Exec<M>, f: usize, r: &mut Registros) -> Result<u32, Fallo> {
        let (d, a) = (r.d, r.a);
        let w = |i: usize| u16_de(d[i]);
        Ok(match FUNCIONES[f] {
            "IsCyberModeID" => modos::modo(d[0], self.comp.tamano_escritorio()).is_some() as u32,
            "BestCModeIDTagList" => self.best_cmode_id(ex, a[0])?,
            "AllocCModeListTagList" => self.alloc_cmode_list(ex, a[1])?,
            "FreeCModeList" => {
                self.free_cmode_list(ex, a[0])?;
                0
            }
            "GetCyberMapAttr" => self.get_cyber_map_attr(ex, a[0], d[0])?,
            "GetCyberIDAttr" => self.get_cyber_id_attr(d[0], d[1]),
            "ReadRGBPixel" => self.read_rgb_pixel(ex, a[1], palabra(d[0]), palabra(d[1]))?,
            "WriteRGBPixel" => self.write_rgb_pixel(ex, a[1], palabra(d[0]), palabra(d[1]), d[2])?,
            "WritePixelArray" => self.write_pixel_array(
                ex,
                a[0],
                w(0),
                w(1),
                w(2),
                a[1],
                palabra(d[3]),
                palabra(d[4]),
                w(5),
                w(6),
                d[7] & 0xFF,
                None,
            )?,
            "WriteLUTPixelArray" => {
                if d[7] & 0xFF != CTABFMT_XRGB8 {
                    return Err(Fallo::Sospechoso { dir: d[7] });
                }
                if a[2] == 0 {
                    return Err(caja_exec::Fallo::PunteroNulo.into());
                }
                let tabla = ex.bytes(a[2], 1024)?;
                let colores: Vec<u32> =
                    tabla.chunks_exact(4).map(|c| u32::from_be_bytes([0, c[1], c[2], c[3]])).collect();
                self.write_pixel_array(
                    ex,
                    a[0],
                    w(0),
                    w(1),
                    w(2),
                    a[1],
                    palabra(d[3]),
                    palabra(d[4]),
                    w(5),
                    w(6),
                    RECTFMT_LUT8,
                    Some(&colores),
                )?
            }
            "ReadPixelArray" => self.read_pixel_array(
                ex,
                a[0],
                w(0),
                w(1),
                w(2),
                a[1],
                palabra(d[3]),
                palabra(d[4]),
                w(5),
                w(6),
                d[7] & 0xFF,
            )?,
            "ScalePixelArray" => self.scale_pixel_array(
                ex,
                a[0],
                w(0),
                w(1),
                w(2),
                a[1],
                palabra(d[3]),
                palabra(d[4]),
                w(5),
                w(6),
                d[7] & 0xFF,
            )?,
            "FillPixelArray" => self.fill_pixel_array(ex, a[1], palabra(d[0]), palabra(d[1]), w(2), w(3), d[4])?,
            "InvertPixelArray" => self.invert_pixel_array(ex, a[1], palabra(d[0]), palabra(d[1]), w(2), w(3))?,
            "MovePixelArray" => {
                let (an, al) = (w(4), w(5));
                if an == 0 || al == 0 {
                    return Ok(0);
                }
                self.clip_blit(ex, a[1], true, palabra(d[0]), palabra(d[1]), a[1], palabra(d[2]), palabra(d[3]), an, al, 0xC0)?;
                (an * al) as u32
            }
            "ExtractColor" => self.extract_color(ex, a[0], a[1], d[0], palabra(d[1]), palabra(d[2]), w(3), w(4))?,
            "LockBitMapTagList" => self.lock_bit_map(ex, a[0], a[1])?,
            "UnLockBitMap" => {
                self.unlock_bit_map(ex, a[0], 0)?;
                0
            }
            "UnLockBitMapTagList" => {
                self.unlock_bit_map(ex, a[0], a[1])?;
                0
            }
            // Sin nada que controlar: la salida de vídeo es la de DashOS.
            "CVideoCtrlTagList" => 0,
            // CModeRequestTagList (sin pedidor de modos: como si se cancelara), DoCDrawMethodTagList y las privadas.
            _ => return Err(Fallo::NoImplementada { n: self.numeros.cybergraphics + f as u16 }),
        })
    }

    // --- Modos de pantalla ---

    /// `BestCModeIDTagList`: entre los modos de la caja (`modos::mejor`), el de la profundidad más pequeña que llegue a
    /// `CYBRBIDTG_Depth` (8 de serie) y el más pequeño que llegue al tamaño pedido. `CYBRBIDTG_MonitorID` y
    /// `CYBRBIDTG_BoardName` (una placa concreta) no cuentan: la caja es una sola.
    fn best_cmode_id<M: Memoria>(&mut self, ex: &mut Exec<M>, tags: u32) -> Result<u32, Fallo> {
        let (mut prof, mut an, mut al) = (8, None, None);
        for (t, v) in crate::gadtools::leer_etiquetas(ex, tags)? {
            match t {
                CYBRBIDTG_DEPTH => prof = v,
                CYBRBIDTG_NOMINAL_WIDTH => an = Some(v & 0xFFFF),
                CYBRBIDTG_NOMINAL_HEIGHT => al = Some(v & 0xFFFF),
                _ => {}
            }
        }
        Ok(modos::mejor(self.comp.tamano_escritorio(), prof, (an, al), |_| true))
    }

    fn get_cyber_id_attr(&self, attr: u32, id: u32) -> u32 {
        let Some(m) = modos::modo(id, self.comp.tamano_escritorio()) else { return 0 };
        match attr {
            CYBRIDATTR_PIXFMT => m.pixfmt(),
            CYBRIDATTR_WIDTH => m.ancho,
            CYBRIDATTR_HEIGHT => m.alto,
            CYBRIDATTR_DEPTH => m.prof,
            CYBRIDATTR_BPPIX => m.bpp(),
            _ => u32::MAX,
        }
    }

    /// `AllocCModeListTagList`: una `List` de `CyberModeNode`, uno por cada modo de la caja que pase los filtros (los de
    /// serie del autodoc: de 320 x 240 a 1600 x 1200, de 8 a 32 bits, todos los modelos de color).
    fn alloc_cmode_list<M: Memoria>(&mut self, ex: &mut Exec<M>, tags: u32) -> Result<u32, Fallo> {
        let (mut pmin, mut pmax, mut amin, mut amax, mut hmin, mut hmax) = (8u32, 32u32, 320u32, 1600u32, 240u32, 1200u32);
        let mut modelos: Option<Vec<u32>> = None;
        for (t, v) in crate::gadtools::leer_etiquetas(ex, tags)? {
            match t {
                CYBRMREQ_MIN_DEPTH => pmin = v & 0xFFFF,
                CYBRMREQ_MAX_DEPTH => pmax = v & 0xFFFF,
                CYBRMREQ_MIN_WIDTH => amin = v,
                CYBRMREQ_MAX_WIDTH => amax = v,
                CYBRMREQ_MIN_HEIGHT => hmin = v,
                CYBRMREQ_MAX_HEIGHT => hmax = v,
                CYBRMREQ_CMODEL_ARRAY if v != 0 => {
                    // UWORD con los modelos; acaba en uno que no existe (como -1). Como mucho 16.
                    let mut l = Vec::new();
                    for i in 0..16 {
                        let m = ex.l16(v.wrapping_add(2 * i))? as u32;
                        if m > PIXFMT_RGBA32 {
                            break;
                        }
                        l.push(m);
                    }
                    modelos = Some(l);
                }
                _ => {}
            }
        }
        let lista: Vec<modos::Modo> = modos::modos(self.comp.tamano_escritorio())
            .into_iter()
            .filter(|m| (pmin..=pmax).contains(&m.prof) && (amin..=amax).contains(&m.ancho) && (hmin..=hmax).contains(&m.alto))
            .filter(|m| modelos.as_ref().is_none_or(|l| l.contains(&m.pixfmt())))
            .collect();
        if lista.is_empty() {
            return Ok(0);
        }
        // La lista (14 bytes, en 16) y sus nodos, en un bloque de la tarea.
        let largo = 16 + CMN_TAMANO * lista.len() as u32;
        let l = ex.alloc_mem(largo, memf::PUBLIC | memf::CLEAR)?;
        if l == 0 {
            return Ok(0);
        }
        let n = |i: usize| l + 16 + CMN_TAMANO * i as u32;
        let ultimo = n(lista.len() - 1);
        // lh_Head = el primero, lh_Tail = 0, lh_TailPred = el último; cada nodo, a sus vecinos (o a la cabeza).
        ex.e32(l, n(0))?;
        ex.e32(l + 4, 0)?;
        ex.e32(l + 8, ultimo)?;
        for (i, m) in lista.iter().enumerate() {
            let d = n(i);
            ex.e32(d, if i + 1 < lista.len() { n(i + 1) } else { l + 4 })?;
            ex.e32(d + 4, if i == 0 { l } else { n(i - 1) })?;
            ex.e32(d + 10, d + CMN_MODETEXT)?;
            let nombre = m.nombre();
            ex.escribir_bytes(d + CMN_MODETEXT, &nombre.as_bytes()[..nombre.len().min(31)])?;
            ex.e32(d + CMN_DISPLAYID, m.id)?;
            ex.e16(d + CMN_WIDTH, m.ancho.min(0xFFFF) as u16)?;
            ex.e16(d + CMN_HEIGHT, m.alto.min(0xFFFF) as u16)?;
            ex.e16(d + CMN_DEPTH, m.prof as u16)?;
        }
        self.cgx.listas.insert(l, (largo, ex.tarea_actual()));
        Ok(l)
    }

    fn free_cmode_list<M: Memoria>(&mut self, ex: &mut Exec<M>, l: u32) -> Result<(), Fallo> {
        if l == 0 {
            return Ok(());
        }
        let Some((largo, _)) = self.cgx.listas.remove(&l) else { return Err(Fallo::NoReservado { dir: l }) };
        Ok(ex.free_mem(l, largo)?)
    }

    // --- Mapas ---

    fn get_cyber_map_attr<M: Memoria>(&mut self, ex: &mut Exec<M>, bm: u32, attr: u32) -> Result<u32, Fallo> {
        let mapa = self.mapa(ex, bm)?;
        Ok(match mapa {
            Mapa::Nativo => {
                let l = &self.gfx.lienzos[&bm];
                let f = l.formato();
                match attr {
                    CYBRMATTR_XMOD => crate::chunky::Chunky::bpr_de(l.ancho(), f).unwrap_or(0),
                    CYBRMATTR_BPPIX => f.bpp(),
                    CYBRMATTR_PIXFMT => f.pixfmt(),
                    CYBRMATTR_WIDTH => l.ancho(),
                    CYBRMATTR_HEIGHT => l.alto(),
                    CYBRMATTR_DEPTH => f.profundidad(),
                    CYBRMATTR_ISCYBERGFX | CYBRMATTR_ISLINEARMEM => u32::MAX,
                    CYBRMATTR_DISPADR => l.mapa.base,
                    _ => u32::MAX,
                }
            }
            Mapa::Planar(p) => match attr {
                CYBRMATTR_XMOD => p.bpr,
                CYBRMATTR_WIDTH => p.ancho,
                CYBRMATTR_HEIGHT => ex.l16(bm + crate::estructuras::bitmap::ROWS)? as u32,
                CYBRMATTR_DEPTH => ex.l8(bm + crate::estructuras::bitmap::DEPTH)? as u32,
                CYBRMATTR_BPPIX | CYBRMATTR_PIXFMT | CYBRMATTR_ISCYBERGFX | CYBRMATTR_ISLINEARMEM | CYBRMATTR_DISPADR => 0,
                _ => u32::MAX,
            },
            Mapa::Chunky(c) => match attr {
                CYBRMATTR_XMOD => c.bpr,
                CYBRMATTR_BPPIX => c.fmt.bpp(),
                CYBRMATTR_PIXFMT => c.fmt.pixfmt(),
                CYBRMATTR_WIDTH => c.ancho,
                CYBRMATTR_HEIGHT => c.alto,
                CYBRMATTR_DEPTH => c.fmt.profundidad(),
                CYBRMATTR_ISCYBERGFX | CYBRMATTR_ISLINEARMEM => u32::MAX,
                CYBRMATTR_DISPADR => c.base,
                _ => u32::MAX,
            },
        })
    }

    /// El destino de un `RastPort`, con su recorte, como lo arma `pintor`.
    fn destino<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, con_paleta: bool) -> Result<Destino, Fallo> {
        let i = self.leer_rp(ex, rp)?;
        let mut mapa = self.mapa(ex, i.bm)?;
        // Un lienzo es un chunky más (con sus píxeles reservados ya, si aún no los tenía).
        let mut lienzo = None;
        if let Mapa::Nativo = mapa {
            let _ = crate::graphics::vista(&mut self.gfx.lienzos, ex, i.bm);
            let l = &self.gfx.lienzos[&i.bm];
            if l.con_pixeles() {
                mapa = Mapa::Chunky(l.mapa);
                lienzo = Some(i.bm);
            }
        }
        let r_mapa = match mapa {
            Mapa::Nativo => self.gfx.lienzos[&i.bm].rect(),
            Mapa::Planar(p) => p.rect(),
            Mapa::Chunky(c) => c.rect(),
        };
        let mut clip = r_mapa;
        if let Some(c) = self.recorte_capa(i.capa) {
            clip = clip.interseccion(&c);
        }
        let (dx, dy) = self.origen_capa(i.capa);
        let mover = |r: Rect| Rect::nuevo(r.x0 - dx, r.y0 - dy, r.x1 - dx, r.y1 - dy);
        let mut clip = mover(clip);
        let region = self.region_de_capa(ex, i.capa)?;
        if let Some(r) = &region {
            let caja = r.iter().fold(Rect::nuevo(0, 0, -1, -1), |u, q| u.union(q));
            clip = clip.interseccion(&caja);
        }
        let cm = self.cm_de(i.bm);
        let paleta = if con_paleta || !mapa.de_color() {
            (0..=255u8).map(|p| self.color(cm, p)).collect()
        } else {
            Vec::new()
        };
        Ok(Destino { bm: i.bm, mapa, lim: mover(r_mapa), clip, region, dx, dy, lienzo, paleta })
    }

    /// Escribe un trozo de fila (`x` en coordenadas del `RastPort`) ya recortado. Devuelve los puntos puestos.
    fn poner_trozo<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        d: &Destino,
        cache: &mut BTreeMap<u32, u8>,
        x: i32,
        y: i32,
        v: Valores,
    ) -> u32 {
        let (mx, my) = (x + d.dx, y + d.dy);
        match d.mapa {
            // Un lienzo sin píxeles (sin memoria para ellos): nada que pintar. Los que los tienen van como chunky.
            Mapa::Nativo => 0,
            Mapa::Chunky(c) => {
                // Colores en los de color verdadero; plumas en los de 8 bits (el color, a la más cercana).
                let fila: Vec<u32> = match (v, c.fmt.es_lut()) {
                    (Valores::Argb(c), false) => c,
                    (Valores::Plumas(p), false) => p.iter().map(|&q| d.paleta[q as usize]).collect(),
                    (Valores::Plumas(p), true) => p.iter().map(|&q| q as u32).collect(),
                    (Valores::Argb(c), true) => c
                        .iter()
                        .map(|&argb| *cache.entry(argb & 0x00FF_FFFF).or_insert_with(|| mas_cercano(&d.paleta, argb)) as u32)
                        .collect(),
                };
                c.escribir_fila(&mut ex.mem, mx, my, &fila);
                if let Some(l) = d.lienzo.and_then(|b| self.gfx.lienzos.get_mut(&b)) {
                    l.danar(Rect::nuevo(mx, my, mx + fila.len() as i32 - 1, my));
                }
                fila.len() as u32
            }
            Mapa::Planar(p) => {
                let plumas: Vec<u8> = match v {
                    Valores::Plumas(q) => q,
                    Valores::Argb(c) => c
                        .iter()
                        .map(|&argb| *cache.entry(argb & 0x00FF_FFFF).or_insert_with(|| mas_cercano(&d.paleta, argb)))
                        .collect(),
                };
                for (k, &q) in plumas.iter().enumerate() {
                    p.escribir(&mut ex.mem, mx + k as i32, my, q, 0xFF);
                }
                plumas.len() as u32
            }
        }
    }

    /// `WritePixelArray` (y `WriteLUTPixelArray`, con `lut`): de un rectángulo de la memoria del 68k al `RastPort`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn write_pixel_array<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        src: u32,
        sx: i32,
        sy: i32,
        modulo: i32,
        rp: u32,
        x: i32,
        y: i32,
        an: i32,
        al: i32,
        fmt: u32,
        lut: Option<&[u32]>,
    ) -> Result<u32, Fallo> {
        let d = self.destino(ex, rp, es_de_plumas(fmt) && lut.is_none())?;
        // RECTFMT_RAW: el formato del propio mapa.
        let fmt = match (fmt, d.mapa) {
            (RECTFMT_RAW, Mapa::Chunky(c)) if c.fmt.es_lut() => RECTFMT_LUT8,
            (RECTFMT_RAW, Mapa::Chunky(c)) => MAPA + c.fmt.pixfmt(),
            (RECTFMT_RAW, Mapa::Nativo) => RECTFMT_ARGB,
            (RECTFMT_RAW, Mapa::Planar(_)) => RECTFMT_LUT8,
            _ => fmt,
        };
        let bpp = bytes_por_pixel(fmt).ok_or(Fallo::Sospechoso { dir: fmt })?;
        if an == 0 || al == 0 {
            return Ok(0);
        }
        if src == 0 {
            return Err(caja_exec::Fallo::PunteroNulo.into());
        }
        let mut cache = BTreeMap::new();
        let mut n = 0;
        let (y0, y1) = (y.max(d.clip.y0), (y + al - 1).min(d.clip.y1));
        for py in y0..=y1 {
            for (xa, xb) in d.trozos(py, x, x + an - 1) {
                let col = (sx + xa - x) as u32;
                let dir = direccion(src, (sy + py - y) as u32, modulo as u32, col, bpp)?;
                let bytes = ex.bytes(dir, (xb - xa + 1) as u32 * bpp)?;
                let v = match (convertir(fmt, &bytes), lut) {
                    (Valores::Plumas(p), Some(t)) => Valores::Argb(p.iter().map(|&q| t[q as usize]).collect()),
                    (v, _) => v,
                };
                n += self.poner_trozo(ex, &d, &mut cache, xa, py, v);
            }
        }
        Ok(n)
    }

    /// `ReadPixelLine8`, `ReadPixelArray8`, `WritePixelLine8` y `WritePixelArray8` de graphics: una pluma por byte,
    /// filas de `an` redondeado a 16 bytes. Al leer de un mapa nativo, la pluma del color más parecido. Devuelve los
    /// puntos leídos o escritos (0 con un tamaño negativo o nulo).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn pixeles8<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        rp: u32,
        x: i32,
        y: i32,
        an: i32,
        al: i32,
        tabla: u32,
        leer: bool,
    ) -> Result<u32, Fallo> {
        if an <= 0 || al <= 0 {
            return Ok(0);
        }
        let modulo = (an + 15) & !15;
        if modulo > 0xFFFF {
            return Err(Fallo::TamanoMalo { ancho: an, alto: al });
        }
        if leer {
            self.read_pixel_array(ex, tabla, 0, 0, modulo, rp, x, y, an, al, P96 + 1)
        } else {
            self.write_pixel_array(ex, tabla, 0, 0, modulo, rp, x, y, an, al, RECTFMT_LUT8, None)
        }
    }

    /// `ScalePixelArray`: escala al punto más cercano (el autodoc avisa de que no es preciso).
    #[allow(clippy::too_many_arguments)]
    fn scale_pixel_array<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        src: u32,
        san: i32,
        sal: i32,
        modulo: i32,
        rp: u32,
        x: i32,
        y: i32,
        an: i32,
        al: i32,
        fmt: u32,
    ) -> Result<u32, Fallo> {
        let bpp = bytes_por_pixel(fmt).ok_or(Fallo::Sospechoso { dir: fmt })?;
        let d = self.destino(ex, rp, es_de_plumas(fmt))?;
        if an == 0 || al == 0 || san == 0 || sal == 0 {
            return Ok(0);
        }
        if src == 0 {
            return Err(caja_exec::Fallo::PunteroNulo.into());
        }
        let mut cache = BTreeMap::new();
        let mut n = 0;
        let (y0, y1) = (y.max(d.clip.y0), (y + al - 1).min(d.clip.y1));
        for py in y0..=y1 {
            let fy = ((py - y) as i64 * sal as i64 / al as i64) as u32;
            let fila = ex.bytes(direccion(src, fy, modulo as u32, 0, bpp)?, san as u32 * bpp)?;
            for (xa, xb) in d.trozos(py, x, x + an - 1) {
                let mut b = Vec::with_capacity(((xb - xa + 1) as u32 * bpp) as usize);
                for px in xa..=xb {
                    let fx = ((px - x) as i64 * san as i64 / an as i64) as usize * bpp as usize;
                    b.extend_from_slice(&fila[fx..fx + bpp as usize]);
                }
                n += self.poner_trozo(ex, &d, &mut cache, xa, py, convertir(fmt, &b));
            }
        }
        Ok(n)
    }

    /// `FillPixelArray`: en los planares, el byte de abajo (el azul) es la pluma.
    #[allow(clippy::too_many_arguments)]
    fn fill_pixel_array<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        rp: u32,
        x: i32,
        y: i32,
        an: i32,
        al: i32,
        argb: u32,
    ) -> Result<u32, Fallo> {
        let d = self.destino(ex, rp, false)?;
        let mut cache = BTreeMap::new();
        let mut n = 0;
        if an == 0 || al == 0 {
            return Ok(0);
        }
        let (y0, y1) = (y.max(d.clip.y0), (y + al - 1).min(d.clip.y1));
        for py in y0..=y1 {
            for (xa, xb) in d.trozos(py, x, x + an - 1) {
                let k = (xb - xa + 1) as usize;
                let v = if d.nativo() { Valores::Argb(vec![argb; k]) } else { Valores::Plumas(vec![argb as u8; k]) };
                n += self.poner_trozo(ex, &d, &mut cache, xa, py, v);
            }
        }
        Ok(n)
    }

    /// `InvertPixelArray`: el color sin el alfa (o todos los planos de la pluma).
    fn invert_pixel_array<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        rp: u32,
        x: i32,
        y: i32,
        an: i32,
        al: i32,
    ) -> Result<u32, Fallo> {
        let d = self.destino(ex, rp, false)?;
        let mut cache = BTreeMap::new();
        let mut n = 0;
        if an == 0 || al == 0 {
            return Ok(0);
        }
        let (y0, y1) = (y.max(d.clip.y0), (y + al - 1).min(d.clip.y1));
        for py in y0..=y1 {
            for (xa, xb) in d.trozos(py, x, x + an - 1) {
                let crudos: Vec<u32> = (xa..=xb).map(|px| self.crudo(ex, &d, px, py)).collect();
                let v = if d.nativo() {
                    Valores::Argb(crudos.iter().map(|c| c ^ 0x00FF_FFFF).collect())
                } else {
                    Valores::Plumas(crudos.iter().map(|&c| !(c as u8)).collect())
                };
                n += self.poner_trozo(ex, &d, &mut cache, xa, py, v);
            }
        }
        Ok(n)
    }

    /// El valor crudo de un punto (en coordenadas del `RastPort`): ARGB en los nativos, la pluma en los planares; 0
    /// fuera del mapa.
    fn crudo<M: Memoria>(&self, ex: &Exec<M>, d: &Destino, x: i32, y: i32) -> u32 {
        if !d.lim.contiene(x, y) {
            return 0;
        }
        let (mx, my) = (x + d.dx, y + d.dy);
        match d.mapa {
            Mapa::Nativo => self.gfx.lienzos.get(&d.bm).map_or(0, |l| l.pixel_en(&ex.mem, mx, my)),
            Mapa::Planar(p) => p.leer(&ex.mem, mx, my) as u32,
            Mapa::Chunky(c) => c.leer(&ex.mem, mx, my),
        }
    }

    /// El color de un punto, o `None` fuera del mapa. El alfa, el que guarde un chunky de 32 bits; 0 en los demás (los
    /// lienzos no llevan alfa).
    fn color_de<M: Memoria>(&self, ex: &Exec<M>, d: &Destino, x: i32, y: i32) -> Option<u32> {
        if !d.lim.contiene(x, y) {
            return None;
        }
        let c = self.crudo(ex, d, x, y);
        Some(match d.mapa {
            Mapa::Chunky(k) if k.fmt.es_lut() => d.paleta[c as usize & 0xFF] & 0x00FF_FFFF,
            Mapa::Chunky(_) if d.lienzo.is_some() => c & 0x00FF_FFFF,
            Mapa::Chunky(_) => c,
            Mapa::Nativo => c & 0x00FF_FFFF,
            Mapa::Planar(_) => d.paleta[c as usize & 0xFF] & 0x00FF_FFFF,
        })
    }

    pub(crate) fn read_rgb_pixel<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, x: i32, y: i32) -> Result<u32, Fallo> {
        let d = self.destino(ex, rp, false)?;
        Ok(self.color_de(ex, &d, x, y).unwrap_or(0))
    }

    /// `WriteRGBPixel`: 0, o -1 si el punto cae fuera del mapa.
    pub(crate) fn write_rgb_pixel<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, x: i32, y: i32, argb: u32) -> Result<u32, Fallo> {
        let d = self.destino(ex, rp, false)?;
        if !d.lim.contiene(x, y) {
            return Ok(u32::MAX);
        }
        if !d.trozos(y, x, x).is_empty() {
            let mut cache = BTreeMap::new();
            self.poner_trozo(ex, &d, &mut cache, x, y, Valores::Argb(vec![argb]));
        }
        Ok(0)
    }

    /// `ReadPixelArray`: del `RastPort` a un rectángulo de la memoria del 68k (RGB, RGBA o ARGB; el alfa, el que guarde
    /// el mapa). Lo que cae fuera del mapa no se toca.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn read_pixel_array<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        dst: u32,
        dxs: i32,
        dys: i32,
        modulo: i32,
        rp: u32,
        x: i32,
        y: i32,
        an: i32,
        al: i32,
        fmt: u32,
    ) -> Result<u32, Fallo> {
        // Hacia la memoria: RGB, RGBA, ARGB (alfa 0) y los de Picasso96 (en CLUT, la pluma, o la más cercana).
        let bpp = match fmt {
            RECTFMT_RGB | RECTFMT_RGBA | RECTFMT_ARGB => bytes_por_pixel(fmt).unwrap_or(0),
            _ if fmt >= P96 => bytes_por_pixel(fmt).ok_or(Fallo::Sospechoso { dir: fmt })?,
            _ => return Err(Fallo::Sospechoso { dir: fmt }),
        };
        let d = self.destino(ex, rp, es_de_plumas(fmt))?;
        if an == 0 || al == 0 {
            return Ok(0);
        }
        if dst == 0 {
            return Err(caja_exec::Fallo::PunteroNulo.into());
        }
        let r = Rect::nuevo(x, y, x + an - 1, y + al - 1).interseccion(&d.lim);
        if r.vacio() {
            return Ok(0);
        }
        let mut n = 0;
        for py in r.y0..=r.y1 {
            let mut b = Vec::with_capacity((r.ancho() * bpp) as usize);
            for px in r.x0..=r.x1 {
                if es_de_plumas(fmt) {
                    let c = self.crudo(ex, &d, px, py);
                    b.push(if d.nativo() { mas_cercano(&d.paleta, c) } else { c as u8 });
                } else {
                    de_argb(fmt, self.color_de(ex, &d, px, py).unwrap_or(0), &mut b);
                }
            }
            let dir = direccion(dst, (dys + py - y) as u32, modulo as u32, (dxs + r.x0 - x) as u32, bpp)?;
            ex.escribir_bytes(dir, &b)?;
            n += r.ancho();
        }
        Ok(n)
    }

    /// `ExtractColor`: en el plano 0 de `mapa` (planar, desde su (0, 0)), un 1 donde el punto del `RastPort` es del
    /// color dado (RGB en los nativos, la pluma en los planares) y un 0 donde no.
    #[allow(clippy::too_many_arguments)]
    fn extract_color<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        rp: u32,
        mapa: u32,
        color: u32,
        x: i32,
        y: i32,
        an: i32,
        al: i32,
    ) -> Result<u32, Fallo> {
        let d = self.destino(ex, rp, false)?;
        let Mapa::Planar(p) = self.mapa(ex, mapa)? else { return Ok(0) };
        if p.prof == 0 || (p.ancho as i32) < an || (p.filas as i32) < al {
            return Ok(0);
        }
        let solo: Planar = Planar { prof: 1, ..p };
        for j in 0..al {
            for i in 0..an {
                let c = self.crudo(ex, &d, x + i, y + j);
                let es = if d.nativo() { c & 0x00FF_FFFF == color & 0x00FF_FFFF } else { c == color & 0xFF };
                solo.escribir(&mut ex.mem, i, j, es as u8, 1);
            }
        }
        Ok(1)
    }

    // --- Acceso directo ---

    /// `LockBitMapTagList`: la dirección de verdad de los píxeles del mapa en la memoria del 68k, sin copiar (el asa es
    /// esa dirección): un chunky, o un lienzo (una ventana, la pantalla; si aún no tenía píxeles, se le reservan ahora).
    /// 0 si el mapa es planar, si lo tiene otra tarea o si no hay memoria.
    pub(crate) fn lock_bit_map<M: Memoria>(&mut self, ex: &mut Exec<M>, bm: u32, tags: u32) -> Result<u32, Fallo> {
        if bm == 0 {
            return Err(Fallo::BitMapMalo { dir: 0 });
        }
        if !self.gfx.lienzos.contains_key(&bm) && !self.gfx.chunkies.contains_key(&bm) {
            return Ok(0);
        }
        let lista = crate::gadtools::leer_etiquetas(ex, tags)?;
        for &(t, p) in &lista {
            if (LBMI_WIDTH..=LBMI_BASEADDRESS).contains(&t) {
                ex.tramo(p, 4)?;
            }
        }
        let k = match self.gfx.chunkies.get(&bm) {
            Some(k) => *k,
            None => {
                let _ = crate::graphics::vista(&mut self.gfx.lienzos, ex, bm);
                let l = &self.gfx.lienzos[&bm];
                if !l.con_pixeles() {
                    return Ok(0);
                }
                l.mapa
            }
        };
        let tarea = ex.tarea_actual();
        let ya = self.cgx.cerrojos.iter().find(|(_, c)| c.bm == bm).map(|(h, c)| (*h, *c));
        let (asa, c) = match ya {
            Some((h, c)) if c.tarea == tarea => {
                self.cgx.cerrojos.get_mut(&h).unwrap().veces += 1;
                (h, c)
            }
            Some(_) => return Ok(0),
            None => {
                let c = Cerrojo { bm, ancho: k.ancho, alto: k.alto, bpr: k.bpr, fmt: k.fmt, tarea, veces: 1 };
                self.cgx.cerrojos.insert(k.base, c);
                (k.base, c)
            }
        };
        for (t, p) in lista {
            let v = match t {
                LBMI_WIDTH => c.ancho,
                LBMI_HEIGHT => c.alto,
                LBMI_DEPTH => c.fmt.profundidad(),
                LBMI_PIXFMT => c.fmt.pixfmt(),
                LBMI_BYTESPERPIX => c.fmt.bpp(),
                LBMI_BYTESPERROW => c.bpr,
                LBMI_BASEADDRESS => asa,
                _ => continue,
            };
            ex.e32(p, v)?;
        }
        Ok(asa)
    }

    /// Lo que el programa pudo escribir directamente en un lienzo cogido (`r`, en coordenadas del mapa): cambiado, para
    /// que el compositor lo presente. En un chunky no hay nada que hacer.
    fn marcar_lienzo(&mut self, c: &Cerrojo, r: Rect) {
        if let Some(l) = self.gfx.lienzos.get_mut(&c.bm) {
            l.danar(r);
        }
    }

    /// `UnLockBitMap` y `UnLockBitMapTagList` (con `tags`): lo suelta. En un lienzo, lo que dice `UBMI_UPDATERECTS` (o, si
    /// no lo dice, el mapa entero) se apunta como cambiado para el compositor. Con `UBMI_REALLYUNLOCK` a FALSE solo se
    /// apunta y sigue cogido.
    pub(crate) fn unlock_bit_map<M: Memoria>(&mut self, ex: &mut Exec<M>, asa: u32, tags: u32) -> Result<(), Fallo> {
        if asa == 0 {
            return Err(caja_exec::Fallo::PunteroNulo.into());
        }
        let Some(c) = self.cgx.cerrojos.get(&asa).copied() else { return Err(Fallo::NoReservado { dir: asa }) };
        let (mut soltar, mut rects) = (true, None);
        if tags != 0 {
            for (t, v) in crate::gadtools::leer_etiquetas(ex, tags)? {
                match t {
                    UBMI_REALLYUNLOCK => soltar = v != 0,
                    UBMI_UPDATERECTS if v != 0 => rects = Some(leer_rectlist(ex, v)?),
                    _ => {}
                }
            }
        }
        match rects {
            Some(rs) => {
                for r in rs {
                    self.marcar_lienzo(&c, r);
                }
            }
            None => self.marcar_lienzo(&c, Rect::de_tamano(c.ancho, c.alto)),
        }
        if !soltar {
            return Ok(());
        }
        if c.veces > 1 {
            self.cgx.cerrojos.get_mut(&asa).unwrap().veces -= 1;
            return Ok(());
        }
        self.cgx.cerrojos.remove(&asa);
        Ok(())
    }

    /// Lo cogido de un mapa que se suelta (`FreeBitMap`, el cierre de una ventana): se olvida.
    pub(crate) fn cgx_mapa_soltado<M: Memoria>(&mut self, _ex: &mut Exec<M>, bm: u32) {
        self.cgx.cerrojos.retain(|_, c| c.bm != bm);
    }

    /// Lo cogido por una tarea que acaba: se suelta (lo de un lienzo, apuntado como cambiado). Sus listas de modos las
    /// suelta exec con su memoria.
    pub(crate) fn cgx_tarea_muerta<M: Memoria>(&mut self, _ex: &mut Exec<M>, t: u32) {
        let suyos: Vec<Cerrojo> = self.cgx.cerrojos.values().filter(|c| c.tarea == t).copied().collect();
        for c in suyos {
            self.marcar_lienzo(&c, Rect::de_tamano(c.ancho, c.alto));
        }
        self.cgx.cerrojos.retain(|_, c| c.tarea != t);
        self.cgx.listas.retain(|_, (_, tarea)| *tarea != t);
    }
}

/// Lee una `struct RectList` (rl_num, rl_next, y rl_num `struct Rectangle`), siguiendo `rl_next`. Más de
/// `MAX_RECTS` rectángulos o listas es una lista rota (o en círculo).
fn leer_rectlist<M: Memoria>(ex: &Exec<M>, mut d: u32) -> Result<Vec<Rect>, Fallo> {
    let mut v = Vec::new();
    let mut listas = 0;
    while d != 0 {
        listas += 1;
        let n = ex.l32(d)?;
        if listas > MAX_RECTS || n > MAX_RECTS - v.len() as u32 {
            return Err(Fallo::Sospechoso { dir: d });
        }
        for i in 0..n {
            let q = d.wrapping_add(8 + 8 * i);
            let w = |o: u32| -> Result<i32, Fallo> { Ok(ex.l16(q.wrapping_add(o))? as i16 as i32) };
            v.push(Rect::nuevo(w(0)?, w(2)?, w(4)?, w(6)?));
        }
        d = ex.l32(d.wrapping_add(4))?;
    }
    Ok(v)
}
