//! El cargador de ejecutables hunk de `LoadSeg` (y de las bibliotecas que carga ramlib), en Rust: lo mismo que
//! `caja/hunk.c` (el que usa la caja para sus programas de prueba), sobre la memoria de la caja.
//!
//! Escrito de la documentación pública del formato (The AmigaDOS Manual, los ficheros de carga, y `dos/doshunks.h` del
//! NDK): `HUNK_HEADER`, `HUNK_CODE`, `HUNK_DATA`, `HUNK_BSS`, `HUNK_RELOC32`, `HUNK_RELOC32SHORT`, `HUNK_DREL32` (el
//! mismo, en ejecutables viejos), `HUNK_SYMBOL`, `HUNK_DEBUG`, `HUNK_NAME` y `HUNK_END`. Cada hunk va en un bloque con 8
//! bytes delante: su tamaño y el BPTR al siguiente (la lista de segmentos de AmigaDOS). Todo lo que se lee del fichero
//! se comprueba antes: un fichero roto u hostil da un error y suelta lo que reservó; nunca escribe fuera de sus hunks.

use alloc::vec::Vec;

use crate::Caja;
use crate::errores::{ERROR_BAD_HUNK, ERROR_NO_FREE_STORE};

const HUNK_NAME: u32 = 0x3E8;
const HUNK_CODE: u32 = 0x3E9;
const HUNK_DATA: u32 = 0x3EA;
const HUNK_BSS: u32 = 0x3EB;
const HUNK_RELOC32: u32 = 0x3EC;
const HUNK_SYMBOL: u32 = 0x3F0;
const HUNK_DEBUG: u32 = 0x3F1;
const HUNK_END: u32 = 0x3F2;
const HUNK_HEADER: u32 = 0x3F3;
const HUNK_DREL32: u32 = 0x3F7;
const HUNK_RELOC32SHORT: u32 = 0x3FC;

/// Tope de hunks de un fichero y de tamaño de uno (64 MB).
const MAX_HUNKS: u32 = 1024;
const MAX_BYTES: u32 = 64 << 20;

struct Lector<'a> {
    d: &'a [u8],
    pos: usize,
}

impl Lector<'_> {
    fn l32(&mut self) -> Result<u32, u32> {
        let b = self.d.get(self.pos..self.pos + 4).ok_or(ERROR_BAD_HUNK)?;
        self.pos += 4;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn l16(&mut self) -> Result<u32, u32> {
        let b = self.d.get(self.pos..self.pos + 2).ok_or(ERROR_BAD_HUNK)?;
        self.pos += 2;
        Ok(u16::from_be_bytes([b[0], b[1]]) as u32)
    }
    fn bytes(&mut self, n: u32) -> Result<&[u8], u32> {
        let n = n as usize;
        let b = self.d.get(self.pos..self.pos.checked_add(n).ok_or(ERROR_BAD_HUNK)?).ok_or(ERROR_BAD_HUNK)?;
        self.pos += n;
        Ok(b)
    }
    fn saltar_largos(&mut self, n: u32) -> Result<(), u32> {
        self.bytes(n.checked_mul(4).ok_or(ERROR_BAD_HUNK)?).map(|_| ())
    }
    fn fin(&self) -> bool {
        self.pos + 4 > self.d.len()
    }
}

/// Un hunk reservado: el bloque (con sus 8 bytes delante), su contenido y su tamaño.
struct Seg {
    bloque: u32,
    contenido: u32,
    bytes: u32,
}

/// Carga `datos` y devuelve la lista de segmentos (un BPTR), o un error de dos (`ERROR_BAD_HUNK`,
/// `ERROR_NO_FREE_STORE`). Si falla, suelta lo que reservó.
pub fn cargar<C: Caja + ?Sized>(c: &mut C, datos: &[u8]) -> Result<u32, u32> {
    let mut segs: Vec<Seg> = Vec::new();
    match cargar_en(c, datos, &mut segs) {
        Ok(()) => Ok(segs.first().map_or(0, |s| (s.bloque + 4) >> 2)),
        Err(e) => {
            for s in segs {
                c.liberar(s.bloque, s.bytes + 8);
            }
            Err(e)
        }
    }
}

fn cargar_en<C: Caja + ?Sized>(c: &mut C, datos: &[u8], segs: &mut Vec<Seg>) -> Result<(), u32> {
    let mut l = Lector { d: datos, pos: 0 };
    if l.l32()? != HUNK_HEADER {
        return Err(ERROR_BAD_HUNK);
    }
    // Los nombres de bibliotecas residentes: no se admiten (ningún ejecutable de verdad los lleva).
    if l.l32()? != 0 {
        return Err(ERROR_BAD_HUNK);
    }
    let tabla = l.l32()?;
    let (primero, ultimo) = (l.l32()?, l.l32()?);
    if primero > ultimo || ultimo >= tabla || tabla > MAX_HUNKS || primero != 0 {
        return Err(ERROR_BAD_HUNK);
    }
    let cuantos = ultimo - primero + 1;
    for _ in 0..cuantos {
        let t = l.l32()?;
        if t >> 30 == 3 {
            l.l32()?; // los atributos MEMF_ (se ignoran: la caja no tiene chip aparte).
        }
        let bytes = (t & 0x3FFF_FFFF).checked_mul(4).filter(|&b| b <= MAX_BYTES).ok_or(ERROR_BAD_HUNK)?;
        let bloque = c.reservar(bytes + 8);
        if bloque == 0 {
            return Err(ERROR_NO_FREE_STORE);
        }
        c.escribir32(bloque, bytes + 8).ok_or(ERROR_NO_FREE_STORE)?;
        segs.push(Seg { bloque, contenido: bloque + 8, bytes });
    }
    // La lista: cada uno apunta (BPTR) a la palabra "siguiente" del otro.
    for i in 0..segs.len() {
        let sig = segs.get(i + 1).map_or(0, |s| (s.bloque + 4) >> 2);
        c.escribir32(segs[i].bloque + 4, sig).ok_or(ERROR_BAD_HUNK)?;
    }
    let mut actual: Option<usize> = None;
    let mut siguiente = 0usize;
    while !l.fin() {
        let tipo = l.l32()? & 0x3FFF_FFFF;
        match tipo {
            HUNK_NAME => {
                let n = l.l32()?;
                l.saltar_largos(n)?;
            }
            HUNK_CODE | HUNK_DATA | HUNK_BSS => {
                if actual.is_some() || siguiente >= segs.len() {
                    return Err(ERROR_BAD_HUNK);
                }
                let n = l.l32()? & 0x3FFF_FFFF;
                let bytes = n.checked_mul(4).ok_or(ERROR_BAD_HUNK)?;
                let s = &segs[siguiente];
                if bytes > s.bytes {
                    return Err(ERROR_BAD_HUNK);
                }
                if tipo != HUNK_BSS {
                    let b = l.bytes(bytes)?;
                    let dst = c.trozo_mut(s.contenido, bytes).ok_or(ERROR_BAD_HUNK)?;
                    dst.copy_from_slice(b);
                }
                actual = Some(siguiente);
                siguiente += 1;
            }
            HUNK_RELOC32 | HUNK_RELOC32SHORT | HUNK_DREL32 => {
                let i = actual.ok_or(ERROR_BAD_HUNK)?;
                let corto = tipo != HUNK_RELOC32;
                loop {
                    let n = if corto { l.l16()? } else { l.l32()? };
                    if n == 0 {
                        break;
                    }
                    let h = if corto { l.l16()? } else { l.l32()? } as usize;
                    let destino = segs.get(h).ok_or(ERROR_BAD_HUNK)?.contenido;
                    for _ in 0..n {
                        let off = if corto { l.l16()? } else { l.l32()? };
                        let s = &segs[i];
                        if off.checked_add(4).is_none_or(|f| f > s.bytes) {
                            return Err(ERROR_BAD_HUNK);
                        }
                        let d = s.contenido + off;
                        let v = c.leer32(d).ok_or(ERROR_BAD_HUNK)?;
                        c.escribir32(d, v.wrapping_add(destino)).ok_or(ERROR_BAD_HUNK)?;
                    }
                }
                // Las cortas acaban en un múltiplo de 4.
                if corto && !l.pos.is_multiple_of(4) {
                    l.l16()?;
                }
            }
            HUNK_SYMBOL => loop {
                let n = l.l32()?;
                if n == 0 {
                    break;
                }
                l.saltar_largos(n & 0x00FF_FFFF)?;
                l.l32()?;
            },
            HUNK_DEBUG => {
                let n = l.l32()?;
                l.saltar_largos(n)?;
            }
            // (Tras uno, un HUNK_DEBUG y su HUNK_SYMBOL pueden llevar otro HUNK_END: de nadie.)
            HUNK_END => actual = None,
            _ => return Err(ERROR_BAD_HUNK),
        }
    }
    if siguiente != segs.len() {
        return Err(ERROR_BAD_HUNK);
    }
    Ok(())
}

/// Suelta una lista de segmentos de `cargar` (con un tope: una lista en círculo no da vueltas sin fin).
pub fn descargar<C: Caja + ?Sized>(c: &mut C, seglist: u32) {
    let mut b = seglist;
    for _ in 0..MAX_HUNKS {
        if b == 0 || b >= 1 << 30 {
            return;
        }
        let p = b << 2;
        let Some(bloque) = p.checked_sub(4) else { return };
        let (Some(bytes), Some(sig)) = (c.leer32(bloque), c.leer32(p)) else { return };
        c.liberar(bloque, bytes);
        b = sig;
    }
}

/// Las partes de una lista de segmentos: (contenido, bytes) de cada uno, con tope.
pub fn segmentos<C: Caja + ?Sized>(c: &C, seglist: u32) -> Vec<(u32, u32)> {
    let mut v = Vec::new();
    let mut b = seglist;
    while b != 0 && b < 1 << 30 && v.len() < MAX_HUNKS as usize {
        let p = b << 2;
        let (Some(bytes), Some(sig)) = (c.leer32(p.wrapping_sub(4)), c.leer32(p)) else { break };
        v.push((p + 4, bytes.saturating_sub(8)));
        b = sig;
    }
    v
}
