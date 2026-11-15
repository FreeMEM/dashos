//! Los modos de pantalla de la caja: los de una tarjeta gráfica (RTG), no los del chipset. La caja no tiene chipset;
//! sus pantallas son de color verdadero (o de 256 colores de una paleta de 24 bits) en cualquier tamaño que quepa en el
//! escritorio de DashOS.
//!
//! Cada modo tiene su `ModeID` en un monitor propio (`MONITOR`, que no es ninguno de los del chipset de
//! `graphics/modeid.h`): el tamaño (`TAMANOS`, el 0 es el del escritorio) en los bits 4-11 y la profundidad
//! (`PROFUNDIDADES`) en los 0-3. Un tamaño que no cabe en el escritorio no está (como un modo que el monitor no da), ni
//! uno de la lista que es igual al del escritorio (está como el 0). La pantalla de Workbench va en el del escritorio
//! en 32 bits. Los modos del chipset siguen siendo del chipset: `IsCyberModeID` dice que no.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::chunky::Formato;

/// El monitor de los modos de la caja (`MONITOR_ID_MASK` = `0xFFFF1000`).
pub const MONITOR: u32 = 0x5DA0_1000;
pub const MASCARA_MONITOR: u32 = 0xFFFF_1000;
pub const INVALID_ID: u32 = 0xFFFF_FFFF;

/// Las profundidades que da la caja.
pub const PROFUNDIDADES: [u32; 5] = [8, 15, 16, 24, 32];
/// Los tamaños: el 0, el del escritorio; los demás, los de siempre de las tarjetas.
pub const TAMANOS: [(u32, u32); 8] =
    [(0, 0), (640, 480), (800, 600), (1024, 768), (1280, 720), (1280, 1024), (1600, 1200), (1920, 1080)];

/// Un modo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Modo {
    pub id: u32,
    pub ancho: u32,
    pub alto: u32,
    /// Bits por punto (8, 15, 16, 24, 32).
    pub prof: u32,
}

impl Modo {
    /// El formato de sus píxeles (`None`: 8 bits, plumas de una paleta).
    pub fn formato(&self) -> Option<Formato> {
        match self.prof {
            8 => None,
            15 => Some(Formato::Rgb15),
            16 => Some(Formato::Rgb16),
            24 => Some(Formato::Rgb24),
            _ => Some(Formato::Argb32),
        }
    }
    /// Su `PIXFMT_*` (`PIXFMT_LUT8`, 0, en 8 bits).
    pub fn pixfmt(&self) -> u32 {
        self.formato().map_or(0, Formato::pixfmt)
    }
    /// Su `RGBFB_*` (`RGBFB_CLUT`, 1, en 8 bits).
    pub fn rgbfb(&self) -> u32 {
        self.formato().map_or(1, |f| f.rgbfb().unwrap_or(0))
    }
    pub fn bpp(&self) -> u32 {
        self.formato().map_or(1, Formato::bpp)
    }
    /// Bits de rojo, verde y azul de un color (la paleta de los de 8 bits es de 24).
    pub fn bits(&self) -> (u8, u8, u8) {
        match self.prof {
            15 => (5, 5, 5),
            16 => (5, 6, 5),
            _ => (8, 8, 8),
        }
    }
    /// Su nombre (`DTAG_NAME`, `CyberModeNode`): "DashOS: 1920x1080 32 bits".
    pub fn nombre(&self) -> String {
        format!("DashOS: {}x{} {} bits", self.ancho, self.alto, self.prof)
    }
}

/// El `ModeID` del tamaño `t` y la profundidad `p` (índices).
pub fn id(t: usize, p: usize) -> u32 {
    MONITOR | ((t as u32) << 4) | p as u32
}

/// El modo de un `ModeID`, si es de la caja y está con este escritorio.
pub fn modo(id: u32, escritorio: (u32, u32)) -> Option<Modo> {
    if id & MASCARA_MONITOR != MONITOR || id & 0xF00 != 0 {
        return None;
    }
    let (t, p) = (((id >> 4) & 0xF) as usize, (id & 0xF) as usize);
    let prof = *PROFUNDIDADES.get(p)?;
    let (ancho, alto) = if t == 0 { escritorio } else { *TAMANOS.get(t)? };
    if t != 0 && ((ancho, alto) == escritorio || ancho > escritorio.0 || alto > escritorio.1) {
        return None;
    }
    Some(Modo { id, ancho, alto, prof })
}

/// Todos los modos que hay con este escritorio, en el orden de sus `ModeID`.
pub fn modos(escritorio: (u32, u32)) -> Vec<Modo> {
    let mut v = Vec::new();
    for t in 0..TAMANOS.len() {
        for p in 0..PROFUNDIDADES.len() {
            if let Some(m) = modo(id(t, p), escritorio) {
                v.push(m);
            }
        }
    }
    v
}

/// El de la pantalla de Workbench: el del escritorio, en 32 bits.
pub fn workbench() -> u32 {
    id(0, 4)
}

/// `BestModeID` de la caja: entre los modos que pasan `vale`, los de la profundidad más pequeña que llegue a `prof`;
/// entre ellos, el más pequeño que llegue al tamaño pedido (si no se pide, el del escritorio) y, si ninguno llega, el
/// más grande. `INVALID_ID` si no hay ninguno.
pub fn mejor(escritorio: (u32, u32), prof: u32, tam: (Option<u32>, Option<u32>), vale: impl Fn(&Modo) -> bool) -> u32 {
    let todos: Vec<Modo> = modos(escritorio).into_iter().filter(|m| m.prof >= prof && vale(m)).collect();
    let Some(p) = todos.iter().map(|m| m.prof).min() else { return INVALID_ID };
    let deesa: Vec<&Modo> = todos.iter().filter(|m| m.prof == p).collect();
    let (an, al) = (tam.0.unwrap_or(escritorio.0), tam.1.unwrap_or(escritorio.1));
    let area = |m: &&Modo| m.ancho as u64 * m.alto as u64;
    let llegan = deesa.iter().filter(|m| m.ancho >= an && m.alto >= al).min_by_key(area);
    llegan.or_else(|| deesa.iter().max_by_key(area)).map_or(INVALID_ID, |m| m.id)
}
