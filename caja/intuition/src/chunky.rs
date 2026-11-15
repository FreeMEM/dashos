//! Los mapas de color verdadero de la caja que no son superficies del compositor (los de `AllocBitMap` de color
//! verdadero, `p96AllocBitMap`): sus píxeles viven en la memoria del 68k, en el formato que pidió el programa, como en
//! una tarjeta gráfica. `LockBitMapTagList` da su dirección de verdad y el programa escribe ahí directo; graphics,
//! cybergraphics y Picasso96 leen y escriben ahí, convirtiendo. Lo que se ve (una ventana) se lo copian con
//! `BltBitMap` y compañía, que pasan al ARGB del compositor.
//!
//! Los formatos son los de `cybergraphx/cybergraphics.h` (`PIXFMT_*`), con su equivalente de Picasso96 (`RGBFB_*`)
//! cuando lo hay. Los de 15 y 16 bits "PC" llevan los dos bytes al revés (little-endian); los de 32, un byte de alfa
//! que se guarda tal cual.

use caja_exec::Memoria;

use crate::lienzo::Rect;

/// Un formato de píxel de un mapa de color verdadero, en el orden de `PIXFMT_*` (desde `PIXFMT_RGB15`, el 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Formato {
    Rgb15,
    Bgr15,
    Rgb15Pc,
    Bgr15Pc,
    Rgb16,
    Bgr16,
    Rgb16Pc,
    Bgr16Pc,
    Rgb24,
    Bgr24,
    Argb32,
    Bgra32,
    Rgba32,
}

/// Todos, en orden.
pub const FORMATOS: [Formato; 13] = [
    Formato::Rgb15,
    Formato::Bgr15,
    Formato::Rgb15Pc,
    Formato::Bgr15Pc,
    Formato::Rgb16,
    Formato::Bgr16,
    Formato::Rgb16Pc,
    Formato::Bgr16Pc,
    Formato::Rgb24,
    Formato::Bgr24,
    Formato::Argb32,
    Formato::Bgra32,
    Formato::Rgba32,
];

fn de5(v: u32) -> u32 {
    (v << 3) | (v >> 2)
}
fn de6(v: u32) -> u32 {
    (v << 2) | (v >> 4)
}

impl Formato {
    /// Su `PIXFMT_*`.
    pub fn pixfmt(self) -> u32 {
        self as u32 + 1
    }
    pub fn de_pixfmt(p: u32) -> Option<Formato> {
        FORMATOS.get((p as usize).wrapping_sub(1)).copied()
    }
    /// Su `RGBFB_*` de Picasso96, si lo tiene (los BGR de 15 y 16 bits big-endian no).
    pub fn rgbfb(self) -> Option<u32> {
        use Formato::*;
        Some(match self {
            Rgb15 => 11,
            Rgb15Pc => 5,
            Bgr15Pc => 13,
            Rgb16 => 10,
            Rgb16Pc => 4,
            Bgr16Pc => 12,
            Rgb24 => 2,
            Bgr24 => 3,
            Argb32 => 6,
            Bgra32 => 9,
            Rgba32 => 8,
            Bgr15 | Bgr16 => return None,
        })
    }
    /// El formato de un `RGBFB_*` (A8B8G8R8, CLUT y los YUV no tienen: `None`).
    pub fn de_rgbfb(f: u32) -> Option<Formato> {
        FORMATOS.iter().copied().find(|x| x.rgbfb() == Some(f))
    }
    /// Bytes por píxel.
    pub fn bpp(self) -> u32 {
        match self as u32 {
            0..=7 => 2,
            8 | 9 => 3,
            _ => 4,
        }
    }
    /// Bits que guarda (15, 16, 24 o 32).
    pub fn profundidad(self) -> u32 {
        match self as u32 {
            0..=3 => 15,
            4..=7 => 16,
            8 | 9 => 24,
            _ => 32,
        }
    }
    /// El formato de serie de una profundidad (sin que el programa diga cuál): de 9 a 15 bits, RGB15; 16, RGB16; de 17 a
    /// 24, RGB24; más, ARGB32.
    pub fn de_profundidad(p: u32) -> Formato {
        match p {
            0..=15 => Formato::Rgb15,
            16 => Formato::Rgb16,
            17..=24 => Formato::Rgb24,
            _ => Formato::Argb32,
        }
    }
    /// Un píxel (sus `bpp` bytes) a ARGB (el alfa, el guardado en los de 32 bits; 0 en los demás).
    pub fn leer(self, p: &[u8]) -> u32 {
        use Formato::*;
        let rgb = |r: u32, g: u32, b: u32| (r << 16) | (g << 8) | b;
        let c555 = |v: u32| rgb(de5((v >> 10) & 31), de5((v >> 5) & 31), de5(v & 31));
        let c565 = |v: u32| rgb(de5((v >> 11) & 31), de6((v >> 5) & 63), de5(v & 31));
        let i555 = |v: u32| rgb(de5(v & 31), de5((v >> 5) & 31), de5((v >> 10) & 31));
        let i565 = |v: u32| rgb(de5(v & 31), de6((v >> 5) & 63), de5((v >> 11) & 31));
        let be = || u16::from_be_bytes([p[0], p[1]]) as u32;
        let le = || u16::from_le_bytes([p[0], p[1]]) as u32;
        match self {
            Rgb15 => c555(be()),
            Bgr15 => i555(be()),
            Rgb15Pc => c555(le()),
            Bgr15Pc => i555(le()),
            Rgb16 => c565(be()),
            Bgr16 => i565(be()),
            Rgb16Pc => c565(le()),
            Bgr16Pc => i565(le()),
            Rgb24 => rgb(p[0] as u32, p[1] as u32, p[2] as u32),
            Bgr24 => rgb(p[2] as u32, p[1] as u32, p[0] as u32),
            Argb32 => u32::from_be_bytes([p[0], p[1], p[2], p[3]]),
            Bgra32 => u32::from_be_bytes([p[3], p[2], p[1], p[0]]),
            Rgba32 => u32::from_be_bytes([p[3], p[0], p[1], p[2]]),
        }
    }
    /// Un color ARGB a un píxel (los `bpp` primeros bytes de `o`).
    pub fn escribir(self, argb: u32, o: &mut [u8]) {
        use Formato::*;
        let [a, r, g, b] = argb.to_be_bytes();
        let (r5, g5, g6, b5) = ((r >> 3) as u16, (g >> 3) as u16, (g >> 2) as u16, (b >> 3) as u16);
        let mut dos = |v: u16, big: bool| o[..2].copy_from_slice(&if big { v.to_be_bytes() } else { v.to_le_bytes() });
        match self {
            Rgb15 => dos(r5 << 10 | g5 << 5 | b5, true),
            Bgr15 => dos(b5 << 10 | g5 << 5 | r5, true),
            Rgb15Pc => dos(r5 << 10 | g5 << 5 | b5, false),
            Bgr15Pc => dos(b5 << 10 | g5 << 5 | r5, false),
            Rgb16 => dos(r5 << 11 | g6 << 5 | b5, true),
            Bgr16 => dos(b5 << 11 | g6 << 5 | r5, true),
            Rgb16Pc => dos(r5 << 11 | g6 << 5 | b5, false),
            Bgr16Pc => dos(b5 << 11 | g6 << 5 | r5, false),
            Rgb24 => o[..3].copy_from_slice(&[r, g, b]),
            Bgr24 => o[..3].copy_from_slice(&[b, g, r]),
            Argb32 => o[..4].copy_from_slice(&[a, r, g, b]),
            Bgra32 => o[..4].copy_from_slice(&[b, g, r, a]),
            Rgba32 => o[..4].copy_from_slice(&[r, g, b, a]),
        }
    }
}

/// Un mapa de color verdadero en la memoria del 68k: `alto` filas de `bpr` bytes desde `base`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chunky {
    pub base: u32,
    pub bpr: u32,
    pub ancho: u32,
    pub alto: u32,
    pub fmt: Formato,
    /// El `ColorMap` de sus plumas (el de su amigo; 0, el del Workbench).
    pub cm: u32,
}

impl Chunky {
    /// Bytes por fila de un mapa de `ancho` píxeles: alineados a 4 (los de 24 bits de ancho impar llevan relleno).
    pub fn bpr_de(ancho: u32, fmt: Formato) -> Option<u32> {
        ancho.checked_mul(fmt.bpp())?.checked_add(3).map(|v| v & !3)
    }
    pub fn largo(&self) -> u32 {
        self.bpr * self.alto
    }
    pub fn rect(&self) -> Rect {
        Rect::de_tamano(self.ancho, self.alto)
    }
    fn dir(&self, x: i32, y: i32) -> u32 {
        self.base + y as u32 * self.bpr + x as u32 * self.fmt.bpp()
    }
    /// El color ARGB de `(x, y)` (sin comprobar los límites; 0 si la memoria no está).
    pub fn leer<M: Memoria>(&self, mem: &M, x: i32, y: i32) -> u32 {
        mem.leer(self.dir(x, y), self.fmt.bpp()).map(|p| self.fmt.leer(p)).unwrap_or(0)
    }
    /// Escribe un color ARGB en `(x, y)` (sin comprobar los límites).
    pub fn escribir<M: Memoria>(&self, mem: &mut M, x: i32, y: i32, argb: u32) {
        let fmt = self.fmt;
        if let Ok(p) = mem.leer_mut(self.dir(x, y), fmt.bpp()) {
            fmt.escribir(argb, p);
        }
    }
    /// Lee `n` puntos seguidos de la fila `y` desde `x` (ya recortados), en ARGB.
    pub fn leer_fila<M: Memoria>(&self, mem: &M, x: i32, y: i32, n: u32, o: &mut alloc::vec::Vec<u32>) {
        let bpp = self.fmt.bpp();
        o.clear();
        match mem.leer(self.dir(x, y), n * bpp) {
            Ok(b) => o.extend(b.chunks_exact(bpp as usize).map(|p| self.fmt.leer(p))),
            Err(_) => o.resize(n as usize, 0),
        }
    }
    /// Escribe puntos ARGB seguidos en la fila `y` desde `x` (ya recortados).
    pub fn escribir_fila<M: Memoria>(&self, mem: &mut M, x: i32, y: i32, v: &[u32]) {
        let (fmt, bpp) = (self.fmt, self.fmt.bpp());
        if let Ok(b) = mem.leer_mut(self.dir(x, y), v.len() as u32 * bpp) {
            // Los de 32 y 24 bits, sin pasar por `escribir` en cada punto (es lo que más se pinta).
            match fmt {
                Formato::Argb32 => {
                    for (o, &c) in b.chunks_exact_mut(4).zip(v) {
                        o.copy_from_slice(&c.to_be_bytes());
                    }
                }
                Formato::Bgra32 => {
                    for (o, &c) in b.chunks_exact_mut(4).zip(v) {
                        o.copy_from_slice(&c.to_le_bytes());
                    }
                }
                Formato::Rgb24 => {
                    for (o, &c) in b.chunks_exact_mut(3).zip(v) {
                        o.copy_from_slice(&c.to_be_bytes()[1..]);
                    }
                }
                _ => {
                    for (o, &c) in b.chunks_exact_mut(bpp as usize).zip(v) {
                        fmt.escribir(c, o);
                    }
                }
            }
        }
    }
}
