//! cybergraphics.library nativa (versión 41): el color verdadero de la caja para los programas que saben de RTG
//! (MUI, picture.datatype, Tachyon).
//!
//! Escrita desde el autodoc público (`cybergraphics.doc`), `cybergraphics.fd` y `cybergraphx/cybergraphics.h` del
//! SDK. No se ha leído código de CyberGraphX, Picasso96, AROS, UAE ni Amiberry.
//!
//! Cómo encaja (diseño en `docs/bitacoras/caja-amiga.md`, 2026-11-15):
//! - **Los mapas de cybergraphics son los nativos de la caja** (`Lienzo`): ARGB de 32 bits, opacos, fuera de la
//!   memoria del 68k. Son los de las ventanas, la pantalla y `AllocBitMap` de más de 8 planos, con amigo nativo o
//!   con `BMF_SPECIALFMT` y un formato que no sea `PIXFMT_LUT8`. Para cybergraphics siempre son `PIXFMT_ARGB32`, 4
//!   bytes por píxel y 32 bits de profundidad, pida el programa el formato que pida (RGB16, RGB24...): el autodoc
//!   obliga a mirar el formato antes de tocar los píxeles. Los planares (los del programa, `AllocBitMap` de hasta 8
//!   planos) no son de cybergraphics (`CYBRMATTR_ISCYBERGFX` da 0), pero las funciones de píxeles también pintan en
//!   ellos: el color se lleva a la pluma más cercana de la paleta del Workbench.
//! - **Un solo modo de pantalla**: el de la caja (`MODO_CAJA`, el que da `GetVPModeID`), del tamaño del escritorio y
//!   en ARGB32. `IsCyberModeID` dice que sí a ese y a ningún otro; `BestCModeIDTagList` lo da siempre que la
//!   profundidad quepa (hasta 32).
//! - **`LockBitMapTagList`** da acceso directo a los píxeles: como no están en la memoria del 68k, se copian a un
//!   búfer suyo (big-endian, A R G B) y, al soltarlo (`UnLockBitMap`), se copian de vuelta al lienzo, que se manda
//!   al compositor como cualquier dibujo. Lo que el programa pinte con otras funciones mientras lo tiene cogido se
//!   pierde al soltarlo (el autodoc lo prohíbe). Un mapa lo coge una sola tarea; la misma puede cogerlo otra vez
//!   (cuenta), otra recibe 0. Lo que se queda cogido se suelta al acabar la tarea o al soltar el mapa.
//! - Las funciones de píxeles recortan como el resto de graphics: al mapa, a la capa de la ventana y a su región de
//!   recorte. Los tamaños son UWORD (como dice el autodoc); las coordenadas de destino, WORD con signo.

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

use caja_exec::estructuras::memf;
use caja_exec::{Exec, Memoria, Registros};

use crate::acceso::{Acceso, palabra};
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

/// El modo de pantalla de la caja: el que da `GetVPModeID` de cualquier pantalla.
pub const MODO_CAJA: u32 = 0x0002_9000;
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
    pub largo: u32,
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
    /// La paleta del mapa: 256 colores ARGB (la del lienzo, o la del Workbench en los planares). En los nativos, solo
    /// si se pidió (hacen falta para las plumas): una llamada de un punto no la arma.
    paleta: Vec<u32>,
}

impl Destino {
    fn nativo(&self) -> bool {
        matches!(self.mapa, Mapa::Nativo)
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

/// Los bytes por píxel de un formato (los de cybergraphics, `RECTFMT_*`, o los de Picasso96, `P96 + RGBFB_*`).
pub(crate) fn bytes_por_pixel(fmt: u32) -> Option<u32> {
    match fmt {
        RECTFMT_RGB => Some(3),
        RECTFMT_RGBA | RECTFMT_ARGB => Some(4),
        RECTFMT_LUT8 | RECTFMT_GREY8 => Some(1),
        _ if fmt >= P96 => match fmt - P96 {
            1 => Some(1),
            2 | 3 => Some(3),
            6..=9 => Some(4),
            4 | 5 | 10..=13 => Some(2),
            _ => None,
        },
        _ => None,
    }
}

fn de5(v: u32) -> u32 {
    (v << 3) | (v >> 2)
}
fn de6(v: u32) -> u32 {
    (v << 2) | (v >> 4)
}

/// Un píxel de un formato de color verdadero a RGB (alfa 0).
fn a_rgb(fmt: u32, p: &[u8]) -> u32 {
    let rgb = |r: u8, g: u8, b: u8| u32::from_be_bytes([0, r, g, b]);
    let be = || u16::from_be_bytes([p[0], p[1]]) as u32;
    let le = || u16::from_le_bytes([p[0], p[1]]) as u32;
    let r565 = |v: u32| (de5(v >> 11) << 16) | (de6((v >> 5) & 63) << 8) | de5(v & 31);
    let r555 = |v: u32| (de5((v >> 10) & 31) << 16) | (de5((v >> 5) & 31) << 8) | de5(v & 31);
    let b565 = |v: u32| (de5(v & 31) << 16) | (de6((v >> 5) & 63) << 8) | de5(v >> 11);
    let b555 = |v: u32| (de5(v & 31) << 16) | (de5((v >> 5) & 31) << 8) | de5((v >> 10) & 31);
    match fmt {
        RECTFMT_RGB | RECTFMT_RGBA => rgb(p[0], p[1], p[2]),
        RECTFMT_ARGB => rgb(p[1], p[2], p[3]),
        RECTFMT_GREY8 => rgb(p[0], p[0], p[0]),
        _ => match fmt.wrapping_sub(P96) {
            2 | 8 => rgb(p[0], p[1], p[2]),
            3 | 9 => rgb(p[2], p[1], p[0]),
            6 => rgb(p[1], p[2], p[3]),
            7 => rgb(p[3], p[2], p[1]),
            10 => r565(be()),
            4 => r565(le()),
            11 => r555(be()),
            5 => r555(le()),
            12 => b565(le()),
            13 => b555(le()),
            _ => 0,
        },
    }
}

/// Un color RGB a un píxel de un formato de color verdadero (el alfa, 0).
fn de_rgb(fmt: u32, c: u32, o: &mut Vec<u8>) {
    let [_, r, g, b] = c.to_be_bytes();
    let (r5, g5, g6, b5) = ((r >> 3) as u16, (g >> 3) as u16, (g >> 2) as u16, (b >> 3) as u16);
    match fmt {
        RECTFMT_RGB => o.extend_from_slice(&[r, g, b]),
        RECTFMT_RGBA => o.extend_from_slice(&[r, g, b, 0]),
        RECTFMT_ARGB => o.extend_from_slice(&[0, r, g, b]),
        RECTFMT_GREY8 => o.push(((r as u32 * 77 + g as u32 * 150 + b as u32 * 29) >> 8) as u8),
        _ => match fmt.wrapping_sub(P96) {
            2 => o.extend_from_slice(&[r, g, b]),
            3 => o.extend_from_slice(&[b, g, r]),
            6 => o.extend_from_slice(&[0, r, g, b]),
            7 => o.extend_from_slice(&[0, b, g, r]),
            8 => o.extend_from_slice(&[r, g, b, 0]),
            9 => o.extend_from_slice(&[b, g, r, 0]),
            10 => o.extend_from_slice(&(r5 << 11 | g6 << 5 | b5).to_be_bytes()),
            4 => o.extend_from_slice(&(r5 << 11 | g6 << 5 | b5).to_le_bytes()),
            11 => o.extend_from_slice(&(r5 << 10 | g5 << 5 | b5).to_be_bytes()),
            5 => o.extend_from_slice(&(r5 << 10 | g5 << 5 | b5).to_le_bytes()),
            12 => o.extend_from_slice(&(b5 << 11 | g6 << 5 | r5).to_le_bytes()),
            13 => o.extend_from_slice(&(b5 << 10 | g5 << 5 | r5).to_le_bytes()),
            _ => {}
        },
    }
}

/// `p96EncodeColor`: un color ARGB en un formato de Picasso96 (los bytes del píxel, alineados a la derecha).
pub(crate) fn codificar_p96(rgbf: u32, argb: u32) -> u32 {
    let mut o = Vec::new();
    de_rgb(P96 + rgbf, argb & 0x00FF_FFFF, &mut o);
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
    Valores::Argb(b.chunks_exact(bpp).map(|p| a_rgb(fmt, p)).collect())
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
            "IsCyberModeID" => (d[0] == MODO_CAJA) as u32,
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

    fn best_cmode_id<M: Memoria>(&mut self, ex: &mut Exec<M>, tags: u32) -> Result<u32, Fallo> {
        let mut prof = 8;
        for (t, v) in crate::gadtools::leer_etiquetas(ex, tags)? {
            if t == CYBRBIDTG_DEPTH {
                prof = v;
            }
        }
        Ok(if prof <= 32 { MODO_CAJA } else { INVALID_ID })
    }

    fn get_cyber_id_attr(&self, attr: u32, id: u32) -> u32 {
        if id != MODO_CAJA {
            return 0;
        }
        let (an, al) = self.comp.tamano_escritorio();
        match attr {
            CYBRIDATTR_PIXFMT => PIXFMT_ARGB32,
            CYBRIDATTR_WIDTH => an,
            CYBRIDATTR_HEIGHT => al,
            CYBRIDATTR_DEPTH => 32,
            CYBRIDATTR_BPPIX => 4,
            _ => u32::MAX,
        }
    }

    /// `AllocCModeListTagList`: una `List` con un `CyberModeNode` (el modo de la caja) si pasa los filtros. Los de
    /// tamaño solo cuentan si se dan (el modo es del tamaño del escritorio, que puede pasar del máximo de serie).
    fn alloc_cmode_list<M: Memoria>(&mut self, ex: &mut Exec<M>, tags: u32) -> Result<u32, Fallo> {
        let (an, al) = self.comp.tamano_escritorio();
        let (mut pmin, mut pmax) = (8u32, 32u32);
        let mut vale = true;
        for (t, v) in crate::gadtools::leer_etiquetas(ex, tags)? {
            match t {
                CYBRMREQ_MIN_DEPTH => pmin = v & 0xFFFF,
                CYBRMREQ_MAX_DEPTH => pmax = v & 0xFFFF,
                CYBRMREQ_MIN_WIDTH => vale &= an >= v,
                CYBRMREQ_MAX_WIDTH => vale &= an <= v,
                CYBRMREQ_MIN_HEIGHT => vale &= al >= v,
                CYBRMREQ_MAX_HEIGHT => vale &= al <= v,
                CYBRMREQ_CMODEL_ARRAY if v != 0 => {
                    // UWORD con los modelos; acaba en uno que no existe (como -1). Como mucho 16.
                    let mut esta = false;
                    for i in 0..16 {
                        let m = ex.l16(v.wrapping_add(2 * i))? as u32;
                        if m > PIXFMT_RGBA32 {
                            break;
                        }
                        esta |= m == PIXFMT_ARGB32;
                    }
                    vale &= esta;
                }
                _ => {}
            }
        }
        if !vale || pmin > 32 || pmax < 32 {
            return Ok(0);
        }
        // La lista (14 bytes, en 16) y su nodo, en un bloque de la tarea.
        let largo = 16 + CMN_TAMANO;
        let l = ex.alloc_mem(largo, memf::PUBLIC | memf::CLEAR)?;
        if l == 0 {
            return Ok(0);
        }
        let n = l + 16;
        // lh_Head = nodo, lh_Tail = 0, lh_TailPred = nodo; el nodo apunta a lh_Tail y a lh_Head.
        ex.e32(l, n)?;
        ex.e32(l + 4, 0)?;
        ex.e32(l + 8, n)?;
        ex.e32(n, l + 4)?;
        ex.e32(n + 4, l)?;
        ex.e32(n + 10, n + CMN_MODETEXT)?;
        ex.escribir_bytes(n + CMN_MODETEXT, b"DashOS")?;
        ex.e32(n + CMN_DISPLAYID, MODO_CAJA)?;
        ex.e16(n + CMN_WIDTH, an.min(0xFFFF) as u16)?;
        ex.e16(n + CMN_HEIGHT, al.min(0xFFFF) as u16)?;
        ex.e16(n + CMN_DEPTH, 32)?;
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
                match attr {
                    CYBRMATTR_XMOD => l.ancho() * 4,
                    CYBRMATTR_BPPIX => 4,
                    CYBRMATTR_PIXFMT => PIXFMT_ARGB32,
                    CYBRMATTR_WIDTH => l.ancho(),
                    CYBRMATTR_HEIGHT => l.alto(),
                    CYBRMATTR_DEPTH => 32,
                    CYBRMATTR_ISCYBERGFX | CYBRMATTR_ISLINEARMEM => u32::MAX,
                    // Privada: los píxeles no están en la memoria del 68k.
                    CYBRMATTR_DISPADR => 0,
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
        })
    }

    /// El destino de un `RastPort`, con su recorte, como lo arma `pintor`.
    fn destino<M: Memoria>(&self, ex: &Exec<M>, rp: u32, con_paleta: bool) -> Result<Destino, Fallo> {
        let i = self.leer_rp(ex, rp)?;
        let mapa = self.mapa(ex, i.bm)?;
        let r_mapa = match mapa {
            Mapa::Nativo => self.gfx.lienzos[&i.bm].rect(),
            Mapa::Planar(p) => p.rect(),
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
        let cm = match mapa {
            Mapa::Nativo => self.gfx.lienzos[&i.bm].paleta,
            Mapa::Planar(_) => 0,
        };
        let paleta = if con_paleta || !matches!(mapa, Mapa::Nativo) {
            (0..=255u8).map(|p| self.color(cm, p)).collect()
        } else {
            Vec::new()
        };
        Ok(Destino { bm: i.bm, mapa, lim: mover(r_mapa), clip, region, dx, dy, paleta })
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
            Mapa::Nativo => {
                let fila: Vec<u32> = match v {
                    Valores::Argb(c) => c,
                    Valores::Plumas(p) => p.iter().map(|&q| d.paleta[q as usize]).collect(),
                };
                self.gfx.lienzos.get_mut(&d.bm).map_or(0, |l| l.escribir_fila(mx, my, &fila))
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
        let fmt = if fmt == RECTFMT_RAW { if d.nativo() { RECTFMT_ARGB } else { RECTFMT_LUT8 } } else { fmt };
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
            Mapa::Nativo => self.gfx.lienzos.get(&d.bm).map_or(0, |l| l.pixel(mx, my)),
            Mapa::Planar(p) => p.leer(&ex.mem, mx, my) as u32,
        }
    }

    /// El color de un punto (RGB, alfa 0: la caja no lleva alfa), o `None` fuera del mapa.
    fn color_de<M: Memoria>(&self, ex: &Exec<M>, d: &Destino, x: i32, y: i32) -> Option<u32> {
        if !d.lim.contiene(x, y) {
            return None;
        }
        let c = self.crudo(ex, d, x, y);
        Some(if d.nativo() { c } else { d.paleta[c as usize & 0xFF] } & 0x00FF_FFFF)
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

    /// `ReadPixelArray`: del `RastPort` a un rectángulo de la memoria del 68k (RGB, RGBA o ARGB; el alfa, 0). Lo que
    /// cae fuera del mapa no se toca.
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
                    de_rgb(fmt, self.color_de(ex, &d, px, py).unwrap_or(0), &mut b);
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

    /// `LockBitMapTagList`: copia los píxeles a un búfer de la memoria del 68k (A R G B) y da su asa (la dirección
    /// del búfer). 0 si el mapa no es de cybergraphics, si lo tiene otra tarea o si no hay memoria.
    pub(crate) fn lock_bit_map<M: Memoria>(&mut self, ex: &mut Exec<M>, bm: u32, tags: u32) -> Result<u32, Fallo> {
        if bm == 0 {
            return Err(Fallo::BitMapMalo { dir: 0 });
        }
        if !self.gfx.lienzos.contains_key(&bm) {
            return Ok(0);
        }
        let lista = crate::gadtools::leer_etiquetas(ex, tags)?;
        for &(t, p) in &lista {
            if (LBMI_WIDTH..=LBMI_BASEADDRESS).contains(&t) {
                ex.tramo(p, 4)?;
            }
        }
        let tarea = ex.tarea_actual();
        let ya = self.cgx.cerrojos.iter().find(|(_, c)| c.bm == bm).map(|(h, c)| (*h, *c));
        let (asa, c) = match ya {
            Some((h, c)) if c.tarea == tarea => {
                self.cgx.cerrojos.get_mut(&h).unwrap().veces += 1;
                (h, c)
            }
            Some(_) => return Ok(0),
            None => {
                let l = self.gfx.lienzos.get_mut(&bm).unwrap();
                let (an, al) = (l.ancho(), l.alto());
                let Some(largo) = an.checked_mul(al).and_then(|n| n.checked_mul(4)) else { return Ok(0) };
                if !l.con_pixeles() {
                    return Ok(0);
                }
                let buf = match ex.reservar(largo, 0) {
                    Ok(b) => b,
                    Err(_) => return Ok(0),
                };
                let pix = self.gfx.lienzos.get_mut(&bm).unwrap().pixeles();
                match ex.mem.leer_mut(buf, largo) {
                    Ok(dst) => {
                        for (o, p) in dst.chunks_exact_mut(4).zip(pix) {
                            o.copy_from_slice(&p.to_be_bytes());
                        }
                    }
                    Err(e) => {
                        let _ = ex.soltar(buf, largo);
                        return Err(e.into());
                    }
                }
                let c = Cerrojo { bm, largo, ancho: an, alto: al, tarea, veces: 1 };
                self.cgx.cerrojos.insert(buf, c);
                (buf, c)
            }
        };
        for (t, p) in lista {
            let v = match t {
                LBMI_WIDTH => c.ancho,
                LBMI_HEIGHT => c.alto,
                LBMI_DEPTH => 32,
                LBMI_PIXFMT => PIXFMT_ARGB32,
                LBMI_BYTESPERPIX => 4,
                LBMI_BYTESPERROW => c.ancho * 4,
                LBMI_BASEADDRESS => asa,
                _ => continue,
            };
            ex.e32(p, v)?;
        }
        Ok(asa)
    }

    /// Copia al lienzo lo del búfer de un cerrojo, en `r` (coordenadas del mapa).
    fn volver_al_lienzo<M: Memoria>(&mut self, ex: &Exec<M>, c: &Cerrojo, asa: u32, r: Rect) {
        let r = r.interseccion(&Rect::de_tamano(c.ancho, c.alto));
        if r.vacio() {
            return;
        }
        let Some(l) = self.gfx.lienzos.get_mut(&c.bm) else { return };
        if l.ancho() != c.ancho || l.alto() != c.alto {
            // El mapa cambió de tamaño (una ventana): lo cogido ya no vale.
            return;
        }
        let mut fila = Vec::with_capacity(r.ancho() as usize);
        for y in r.y0..=r.y1 {
            let d = asa + (y as u32 * c.ancho + r.x0 as u32) * 4;
            let Ok(b) = ex.mem.leer(d, r.ancho() * 4) else { return };
            fila.clear();
            fila.extend(b.chunks_exact(4).map(|p| u32::from_be_bytes([p[0], p[1], p[2], p[3]])));
            l.escribir_fila(r.x0, y, &fila);
        }
    }

    /// `UnLockBitMap` y `UnLockBitMapTagList` (con `tags`): copia lo del búfer al lienzo y lo suelta. Con
    /// `UBMI_REALLYUNLOCK` a FALSE solo copia (los rectángulos de `UBMI_UPDATERECTS`, o todo) y sigue cogido.
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
        if !soltar {
            match rects {
                Some(rs) => {
                    for r in rs {
                        self.volver_al_lienzo(ex, &c, asa, r);
                    }
                }
                None => self.volver_al_lienzo(ex, &c, asa, Rect::de_tamano(c.ancho, c.alto)),
            }
            return Ok(());
        }
        if c.veces > 1 {
            self.cgx.cerrojos.get_mut(&asa).unwrap().veces -= 1;
            return Ok(());
        }
        // Al soltarlo de verdad se copia todo: la memoria es lineal (`CYBRMATTR_ISLINEARMEM`), así que el programa
        // puede haber escrito fuera de los rectángulos que dice.
        self.volver_al_lienzo(ex, &c, asa, Rect::de_tamano(c.ancho, c.alto));
        self.cgx.cerrojos.remove(&asa);
        ex.soltar(asa, c.largo)
    }

    /// Lo cogido de un mapa que se suelta (`FreeBitMap`): sin copiar nada.
    pub(crate) fn cgx_mapa_soltado<M: Memoria>(&mut self, ex: &mut Exec<M>, bm: u32) {
        let asas: Vec<(u32, u32)> =
            self.cgx.cerrojos.iter().filter(|(_, c)| c.bm == bm).map(|(a, c)| (*a, c.largo)).collect();
        for (a, l) in asas {
            self.cgx.cerrojos.remove(&a);
            let _ = ex.soltar(a, l);
        }
    }

    /// Lo cogido por una tarea que acaba: se copia y se suelta. Sus listas de modos las suelta exec con su memoria.
    pub(crate) fn cgx_tarea_muerta<M: Memoria>(&mut self, ex: &mut Exec<M>, t: u32) {
        let asas: Vec<(u32, Cerrojo)> =
            self.cgx.cerrojos.iter().filter(|(_, c)| c.tarea == t).map(|(a, c)| (*a, *c)).collect();
        for (a, c) in asas {
            self.volver_al_lienzo(ex, &c, a, Rect::de_tamano(c.ancho, c.alto));
            self.cgx.cerrojos.remove(&a);
            let _ = ex.soltar(a, c.largo);
        }
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
