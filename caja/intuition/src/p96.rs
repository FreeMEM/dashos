//! Picasso96API.library nativa: una capa fina sobre cybergraphics (`cgx.rs`), con los mismos mapas (los nativos de la
//! caja, en `RGBFB_A8R8G8B8`), el mismo modo de pantalla y el mismo acceso directo.
//!
//! Escrita desde el autodoc público de Picasso96API y las constantes, estructuras y desplazamientos de su interfaz
//! (la traducción a Pascal de sus includes en las unidades de Free Pascal). No se ha leído código de Picasso96, AROS,
//! UAE ni Amiberry. Solo lo que piden los programas (Tachyon, por la biblioteca de Dash): mapas, sus atributos, leer y
//! escribir píxeles, el modo y su `p96LockBitMap`. Lo demás (pantallas propias, PIP, el pedidor y las listas de
//! modos, los datos de las placas) da un aviso de que falta y 0.

use alloc::vec::Vec;

use caja_exec::{Exec, Memoria, Registros};

use crate::acceso::{Acceso, palabra};
use crate::cgx::constantes::{BMF_SPECIALFMT, P96, RECTFMT_ARGB};
use crate::cgx::{INVALID_ID, MODO_CAJA, codificar_p96};
use crate::graphics::Mapa;
use crate::{CajaGrafica, Compositor, Fallo};

/// Las funciones de Picasso96API.library, desde -30. Las dos de -168 y -174 (los mensajes de PIP) no salen en la
/// interfaz que tenemos: van con su sitio.
pub const FUNCIONES: [&str; 28] = [
    "p96AllocBitMap",
    "p96FreeBitMap",
    "p96GetBitMapAttr",
    "p96LockBitMap",
    "p96UnlockBitMap",
    "p96BestModeIDTagList",
    "p96RequestModeIDTagList",
    "p96AllocModeListTagList",
    "p96FreeModeList",
    "p96GetModeIDAttr",
    "p96OpenScreenTagList",
    "p96CloseScreen",
    "p96WritePixelArray",
    "p96ReadPixelArray",
    "p96WritePixel",
    "p96ReadPixel",
    "p96RectFill",
    "p96WriteTrueColorData",
    "p96ReadTrueColorData",
    "p96PIP_OpenTagList",
    "p96PIP_Close",
    "p96PIP_SetTagList",
    "p96PIP_GetTagList",
    "p96PIP_GetIMsg",
    "p96PIP_ReplyIMsg",
    "p96GetRTGDataTagList",
    "p96GetBoardDataTagList",
    "p96EncodeColor",
];

pub const NOMBRE: &str = "Picasso96API.library";
pub const VERSION: u16 = 2;

/// Constantes de la interfaz de Picasso96API.
pub mod constantes {
    pub const RGBFB_NONE: u32 = 0;
    pub const RGBFB_CLUT: u32 = 1;
    pub const RGBFB_R8G8B8: u32 = 2;
    pub const RGBFB_A8R8G8B8: u32 = 6;
    pub const RGBFB_R5G6B5: u32 = 10;
    pub const RGBFB_B5G5R5PC: u32 = 13;
    pub const RGBFF_A8R8G8B8: u32 = 1 << RGBFB_A8R8G8B8;

    pub const P96BMA_WIDTH: u32 = 0;
    pub const P96BMA_HEIGHT: u32 = 1;
    pub const P96BMA_DEPTH: u32 = 2;
    pub const P96BMA_MEMORY: u32 = 3;
    pub const P96BMA_BYTESPERROW: u32 = 4;
    pub const P96BMA_BYTESPERPIXEL: u32 = 5;
    pub const P96BMA_BITSPERPIXEL: u32 = 6;
    pub const P96BMA_RGBFORMAT: u32 = 7;
    pub const P96BMA_ISP96: u32 = 8;

    pub const P96IDA_WIDTH: u32 = 0;
    pub const P96IDA_HEIGHT: u32 = 1;
    pub const P96IDA_DEPTH: u32 = 2;
    pub const P96IDA_BYTESPERPIXEL: u32 = 3;
    pub const P96IDA_BITSPERPIXEL: u32 = 4;
    pub const P96IDA_RGBFORMAT: u32 = 5;
    pub const P96IDA_ISP96: u32 = 6;
    pub const P96IDA_BOARDNUMBER: u32 = 7;
    pub const P96IDA_STDBYTESPERROW: u32 = 8;
    pub const P96IDA_BOARDNAME: u32 = 9;
    pub const P96IDA_COMPATIBLEFORMATS: u32 = 10;
    pub const P96IDA_VIDEOCOMPATIBLE: u32 = 11;
    pub const P96IDA_PABLOIVCOMPATIBLE: u32 = 12;
    pub const P96IDA_PALOMAIVCOMPATIBLE: u32 = 13;

    pub const P96BIDTAG_FORMATS_ALLOWED: u32 = 0x8000_0061;
    pub const P96BIDTAG_FORMATS_FORBIDDEN: u32 = 0x8000_0062;
    pub const P96BIDTAG_DEPTH: u32 = 0x8000_0065;

    /// `struct RenderInfo`: Memory, BytesPerRow (WORD), pad, RGBFormat.
    pub const RI_MEMORY: u32 = 0;
    pub const RI_BYTESPERROW: u32 = 4;
    pub const RI_RGBFORMAT: u32 = 8;
    pub const RI_TAMANO: u32 = 12;
}

use constantes::*;

#[derive(Default)]
pub(crate) struct Estado {
    pub base: u32,
    /// El nombre de la "placa" (`P96IDA_BOARDNAME`), en la memoria del sistema.
    pub placa: u32,
}

impl<C: Compositor> CajaGrafica<C> {
    pub(crate) fn instalar_p96<M: Memoria>(&mut self, ex: &mut Exec<M>) -> Result<(), Fallo> {
        let n0 = self.numeros.p96;
        let nums: Vec<u16> = (0..FUNCIONES.len() as u16).map(|i| n0 + i).collect();
        self.p96.base = ex.crear_biblioteca_nativa(NOMBRE, VERSION, 0, &nums, 34)?;
        self.p96.placa = ex.cadena_nueva(b"DashOS")?;
        Ok(())
    }

    /// La base de Picasso96API.library.
    pub fn p96_base(&self) -> u32 {
        self.p96.base
    }

    /// El número nativo de una función de Picasso96API, por su nombre.
    pub fn numero_p96(&self, nombre: &str) -> Option<u16> {
        FUNCIONES.iter().position(|n| *n == nombre).map(|i| self.numeros.p96 + i as u16)
    }

    pub(crate) fn es_p96(&self, n: u16) -> Option<usize> {
        let p0 = self.numeros.p96;
        (n >= p0 && ((n - p0) as usize) < FUNCIONES.len()).then(|| (n - p0) as usize)
    }

    pub(crate) fn llamar_p96<M: Memoria>(&mut self, ex: &mut Exec<M>, f: usize, r: &mut Registros) -> Result<u32, Fallo> {
        let (d, a) = (r.d, r.a);
        Ok(match FUNCIONES[f] {
            "p96AllocBitMap" => {
                // Un formato de color verdadero pide un mapa en ese formato, como `BMF_SPECIALFMT` en cybergraphics
                // (A8B8G8R8, que no tiene `PIXFMT_*`, va en A8R8G8B8). CLUT o ninguno: lo de siempre (por la
                // profundidad o el amigo).
                let rgbf = d[7];
                let fmt = if rgbf == 7 { Some(crate::chunky::Formato::Argb32) } else { crate::chunky::Formato::de_rgbfb(rgbf) };
                let flags = match fmt {
                    Some(f) => (d[3] & 0x00FF_FFFF) | BMF_SPECIALFMT | (f.pixfmt() << 24),
                    None => d[3] & 0x00FF_FF7F,
                };
                self.alloc_bit_map(ex, d[0], d[1], d[2], flags, a[0])?
            }
            "p96FreeBitMap" => {
                self.free_bit_map(ex, a[0])?;
                0
            }
            "p96GetBitMapAttr" => self.p96_get_bit_map_attr(ex, a[0], d[0])?,
            "p96LockBitMap" => self.p96_lock_bit_map(ex, a[0], a[1], d[0])?,
            "p96UnlockBitMap" => {
                if let Some(c) = self.cgx.cerrojos.get(&d[0])
                    && c.bm != a[0]
                {
                    return Err(Fallo::Sospechoso { dir: a[0] });
                }
                self.unlock_bit_map(ex, d[0], 0)?;
                0
            }
            "p96BestModeIDTagList" => {
                let (mut prof, mut si, mut no) = (8, u32::MAX, 0);
                for (t, v) in crate::gadtools::leer_etiquetas(ex, a[0])? {
                    match t {
                        P96BIDTAG_DEPTH => prof = v,
                        P96BIDTAG_FORMATS_ALLOWED => si = v,
                        P96BIDTAG_FORMATS_FORBIDDEN => no = v,
                        _ => {}
                    }
                }
                if prof <= 32 && si & RGBFF_A8R8G8B8 != 0 && no & RGBFF_A8R8G8B8 == 0 { MODO_CAJA } else { INVALID_ID }
            }
            "p96GetModeIDAttr" => self.p96_get_mode_id_attr(d[0], d[1]),
            "p96WritePixelArray" | "p96ReadPixelArray" => {
                let ri = a[0];
                if ri == 0 {
                    return Err(caja_exec::Fallo::PunteroNulo.into());
                }
                ex.tramo(ri, RI_TAMANO)?;
                let mem = ex.l32(ri + RI_MEMORY)?;
                let bpr = ex.l16(ri + RI_BYTESPERROW)? as u32 as i32;
                let rgbf = ex.l32(ri + RI_RGBFORMAT)?;
                if rgbf > 0xFF {
                    return Err(Fallo::Sospechoso { dir: rgbf });
                }
                let w = |i: usize| d[i] as u16 as i32;
                if FUNCIONES[f] == "p96WritePixelArray" {
                    self.write_pixel_array(ex, mem, w(0), w(1), bpr, a[1], palabra(d[2]), palabra(d[3]), w(4), w(5), P96 + rgbf, None)?;
                } else {
                    self.read_pixel_array(ex, mem, w(0), w(1), bpr, a[1], palabra(d[2]), palabra(d[3]), w(4), w(5), P96 + rgbf)?;
                }
                0
            }
            "p96WritePixel" => {
                self.write_rgb_pixel(ex, a[1], palabra(d[0]), palabra(d[1]), d[2])?;
                0
            }
            "p96ReadPixel" => self.read_rgb_pixel(ex, a[1], palabra(d[0]), palabra(d[1]))?,
            "p96RectFill" => {
                let (x0, y0, x1, y1) = (palabra(d[0]), palabra(d[1]), palabra(d[2]), palabra(d[3]));
                if x1 >= x0 && y1 >= y0 {
                    // El color se escribe tal cual en los nativos; en los planares, la pluma más cercana.
                    let k = 4 * ((x1 - x0 + 1) as u32);
                    let fila: Vec<u8> = (0..(x1 - x0 + 1)).flat_map(|_| d[4].to_be_bytes()).collect();
                    let tmp = ex.reservar(k, 0)?;
                    let res = ex
                        .escribir_bytes(tmp, &fila)
                        .and_then(|_| self.write_pixel_array(ex, tmp, 0, 0, 0, a[1], x0, y0, x1 - x0 + 1, y1 - y0 + 1, RECTFMT_ARGB, None));
                    ex.soltar(tmp, k)?;
                    res?;
                }
                0
            }
            "p96EncodeColor" => {
                if crate::cgx::bytes_por_pixel(P96 + d[0]).is_none() || d[0] == RGBFB_CLUT {
                    return Err(Fallo::Sospechoso { dir: d[0] });
                }
                codificar_p96(d[0], d[1])
            }
            // p96RequestModeIDTagList (como si se cancelara), las listas de modos, las pantallas propias, PIP, los
            // datos de color verdadero por canales y los de las placas.
            _ => return Err(Fallo::NoImplementada { n: self.numeros.p96 + f as u16 }),
        })
    }

    fn p96_get_bit_map_attr<M: Memoria>(&mut self, ex: &mut Exec<M>, bm: u32, attr: u32) -> Result<u32, Fallo> {
        Ok(match self.mapa(ex, bm)? {
            Mapa::Nativo => {
                let l = &self.gfx.lienzos[&bm];
                let f = l.formato;
                match attr {
                    P96BMA_WIDTH => l.ancho(),
                    P96BMA_HEIGHT => l.alto(),
                    P96BMA_DEPTH => f.bpp() * 8,
                    P96BMA_BITSPERPIXEL => f.profundidad().min(24),
                    P96BMA_BYTESPERROW => crate::chunky::Chunky::bpr_de(l.ancho(), f).unwrap_or(0),
                    P96BMA_BYTESPERPIXEL => f.bpp(),
                    P96BMA_RGBFORMAT => f.rgbfb().unwrap_or(RGBFB_NONE),
                    P96BMA_ISP96 => 1,
                    // La memoria (solo con el mapa cogido, y nunca la del lienzo: no está en la del 68k), la placa...
                    _ => 0,
                }
            }
            Mapa::Chunky(c) => match attr {
                P96BMA_WIDTH => c.ancho,
                P96BMA_HEIGHT => c.alto,
                P96BMA_DEPTH => c.fmt.bpp() * 8,
                P96BMA_BITSPERPIXEL => c.fmt.profundidad().min(24),
                P96BMA_MEMORY => c.base,
                P96BMA_BYTESPERROW => c.bpr,
                P96BMA_BYTESPERPIXEL => c.fmt.bpp(),
                P96BMA_RGBFORMAT => c.fmt.rgbfb().unwrap_or(RGBFB_NONE),
                P96BMA_ISP96 => 1,
                _ => 0,
            },
            Mapa::Planar(p) => match attr {
                P96BMA_WIDTH => p.ancho,
                P96BMA_HEIGHT => ex.l16(bm + crate::estructuras::bitmap::ROWS)? as u32,
                P96BMA_DEPTH | P96BMA_BITSPERPIXEL => ex.l8(bm + crate::estructuras::bitmap::DEPTH)? as u32,
                P96BMA_BYTESPERROW => p.bpr,
                P96BMA_RGBFORMAT => RGBFB_NONE,
                _ => 0,
            },
        })
    }

    /// `p96LockBitMap`: el cerrojo de cybergraphics (el búfer en la memoria del 68k), con su `RenderInfo` en `buf` si
    /// cabe. Su asa es el cerrojo; 0 si no se puede (planar, de otra tarea, sin memoria, filas de más de 32 KB).
    fn p96_lock_bit_map<M: Memoria>(&mut self, ex: &mut Exec<M>, bm: u32, buf: u32, largo: u32) -> Result<u32, Fallo> {
        if buf != 0 && largo >= RI_TAMANO {
            ex.tramo(buf, RI_TAMANO)?;
        }
        let asa = self.lock_bit_map(ex, bm, 0)?;
        if asa == 0 {
            return Ok(0);
        }
        let c = self.cgx.cerrojos[&asa];
        if c.bpr > 0x7FFF {
            // `BytesPerRow` del `RenderInfo` es un WORD.
            self.unlock_bit_map(ex, asa, 0)?;
            return Ok(0);
        }
        if buf != 0 && largo >= RI_TAMANO {
            ex.e32(buf + RI_MEMORY, asa)?;
            ex.e16(buf + RI_BYTESPERROW, c.bpr as u16)?;
            ex.e16(buf + RI_BYTESPERROW + 2, 0)?;
            ex.e32(buf + RI_RGBFORMAT, c.fmt.rgbfb().unwrap_or(RGBFB_NONE))?;
        }
        Ok(asa)
    }

    fn p96_get_mode_id_attr(&self, id: u32, attr: u32) -> u32 {
        if attr > P96IDA_PALOMAIVCOMPATIBLE {
            return u32::MAX;
        }
        if id != MODO_CAJA {
            return 0;
        }
        let (an, al) = self.comp.tamano_escritorio();
        match attr {
            P96IDA_WIDTH => an,
            P96IDA_HEIGHT => al,
            P96IDA_DEPTH | P96IDA_BITSPERPIXEL => 32,
            P96IDA_BYTESPERPIXEL => 4,
            P96IDA_RGBFORMAT => RGBFB_A8R8G8B8,
            P96IDA_ISP96 => 1,
            P96IDA_STDBYTESPERROW => an * 4,
            P96IDA_BOARDNAME => self.p96.placa,
            P96IDA_COMPATIBLEFORMATS => RGBFF_A8R8G8B8,
            // P96IDA_BOARDNUMBER (la 0) y las de vídeo (no).
            _ => 0,
        }
    }
}
