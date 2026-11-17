//! S32: el disco visto por el instalador. Un disco es algo que lee y escribe sectores; lo demás (tablas, sistemas de
//! archivos) se lee encima, sin fiarse de nada.

use alloc::vec;
use alloc::vec::Vec;

/// El sector más grande que se admite.
pub const SECTOR_MAX: usize = 4096;

/// Un disco: sectores de `tam_sector()` bytes, de 0 a `sectores()`. Todo es sincrónico: cuando `escribir` y `vaciar`
/// vuelven con `Ok`, lo escrito está en el disco (en el orden en que se pidió, si entre medias se vació).
pub trait Disco {
    fn tam_sector(&self) -> usize;
    fn sectores(&self) -> u64;
    /// Llena `buf` (un múltiplo del sector) desde el sector `lba`.
    fn leer(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), ()>;
    /// Escribe `buf` (un múltiplo del sector) desde el sector `lba`.
    fn escribir(&mut self, lba: u64, buf: &[u8]) -> Result<(), ()>;
    /// Lo escrito, al disco de verdad (FLUSH del NVMe, del SATA).
    fn vaciar(&mut self) -> Result<(), ()>;
}

/// Lee `n` bytes desde el byte `desde` del disco (no hace falta que estén alineados). `None` si se sale o no se lee.
pub fn leer_bytes(d: &mut dyn Disco, desde: u64, n: usize) -> Option<Vec<u8>> {
    let ts = d.tam_sector() as u64;
    let fin = desde.checked_add(n as u64)?;
    if fin > d.sectores().checked_mul(ts)? {
        return None;
    }
    let (s0, s1) = (desde / ts, fin.div_ceil(ts));
    let mut buf = vec![0u8; ((s1 - s0) * ts) as usize];
    // De a trozos de 1 MiB como mucho: los servicios de disco tienen su tope por petición.
    let trozo = (1 << 20) / ts as usize;
    for (i, c) in buf.chunks_mut(trozo * ts as usize).enumerate() {
        d.leer(s0 + (i * trozo) as u64, c).ok()?;
    }
    let o = (desde - s0 * ts) as usize;
    Some(buf[o..o + n].to_vec())
}

/// Escribe `datos` desde el byte `desde`, leyendo antes lo que haya en los sectores de los bordes.
pub fn escribir_bytes(d: &mut dyn Disco, desde: u64, datos: &[u8]) -> Result<(), ()> {
    let ts = d.tam_sector() as u64;
    let fin = desde.checked_add(datos.len() as u64).ok_or(())?;
    if fin > d.sectores().checked_mul(ts).ok_or(())? {
        return Err(());
    }
    let (s0, s1) = (desde / ts, fin.div_ceil(ts));
    let o = (desde - s0 * ts) as usize;
    let mut buf = if o == 0 && datos.len() as u64 % ts == 0 {
        datos.to_vec()
    } else {
        let mut b = leer_bytes(d, s0 * ts, ((s1 - s0) * ts) as usize).ok_or(())?;
        b[o..o + datos.len()].copy_from_slice(datos);
        b
    };
    let trozo = ((1 << 20) / ts) as usize;
    for (i, c) in buf.chunks_mut(trozo * ts as usize).enumerate() {
        d.escribir(s0 + (i * trozo) as u64, c)?;
    }
    Ok(())
}

pub fn u16_en(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

pub fn u32_en(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

pub fn u64_en(b: &[u8], i: usize) -> u64 {
    u32_en(b, i) as u64 | (u32_en(b, i + 4) as u64) << 32
}

pub fn poner_u16(b: &mut [u8], i: usize, v: u16) {
    b[i..i + 2].copy_from_slice(&v.to_le_bytes());
}

pub fn poner_u32(b: &mut [u8], i: usize, v: u32) {
    b[i..i + 4].copy_from_slice(&v.to_le_bytes());
}

pub fn poner_u64(b: &mut [u8], i: usize, v: u64) {
    b[i..i + 8].copy_from_slice(&v.to_le_bytes());
}

/// CRC-32 (el de la GPT y el de zip), poco a poco: `crc` empieza en 0.
pub fn crc32(crc: u32, datos: &[u8]) -> u32 {
    let mut c = !crc;
    for &b in datos {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
        }
    }
    !c
}

/// Un GUID como se escribe (8-4-4-4-12), desde su forma en el disco (los tres primeros campos al revés).
pub fn guid_texto(g: &[u8; 16], out: &mut alloc::string::String) {
    use core::fmt::Write;
    let orden = [3, 2, 1, 0, 5, 4, 7, 6, 8, 9, 10, 11, 12, 13, 14, 15];
    for (k, &i) in orden.iter().enumerate() {
        if matches!(k, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        let _ = write!(out, "{:02X}", g[i]);
    }
}

/// Al revés que `guid_texto`. `None` si no tiene la forma.
pub fn guid_de_texto(t: &str) -> Option<[u8; 16]> {
    let t = t.as_bytes();
    if t.len() != 36 || t[8] != b'-' || t[13] != b'-' || t[18] != b'-' || t[23] != b'-' {
        return None;
    }
    let mut cifras = [0u8; 16];
    let mut k = 0;
    let mut i = 0;
    while i < 36 {
        if t[i] == b'-' {
            i += 1;
            continue;
        }
        let h = |c: u8| (c as char).to_digit(16);
        cifras[k] = (h(t[i])? * 16 + h(t[i + 1])?) as u8;
        k += 1;
        i += 2;
    }
    let orden = [3, 2, 1, 0, 5, 4, 7, 6, 8, 9, 10, 11, 12, 13, 14, 15];
    let mut g = [0u8; 16];
    for (k, &i) in orden.iter().enumerate() {
        g[i] = cifras[k];
    }
    Some(g)
}

/// Un disco en memoria, para las pruebas y para lo que se arma antes de escribirlo.
pub struct EnMemoria {
    pub datos: Vec<u8>,
    pub sector: usize,
    /// Tras cuántas escrituras más deja de escribir (para simular un corte de luz): `None`, nunca.
    pub corte: Option<usize>,
    /// Lo que se escribió, por orden: (sector, cuántos).
    pub escrituras: Vec<(u64, u64)>,
}

impl EnMemoria {
    pub fn nuevo(bytes: usize, sector: usize) -> Self {
        EnMemoria { datos: vec![0; bytes], sector, corte: None, escrituras: Vec::new() }
    }
}

impl Disco for EnMemoria {
    fn tam_sector(&self) -> usize {
        self.sector
    }
    fn sectores(&self) -> u64 {
        (self.datos.len() / self.sector) as u64
    }
    fn leer(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), ()> {
        let o = (lba as usize).checked_mul(self.sector).ok_or(())?;
        if buf.len() % self.sector != 0 || o.checked_add(buf.len()).is_none_or(|f| f > self.datos.len()) {
            return Err(());
        }
        buf.copy_from_slice(&self.datos[o..o + buf.len()]);
        Ok(())
    }
    fn escribir(&mut self, lba: u64, buf: &[u8]) -> Result<(), ()> {
        let o = (lba as usize).checked_mul(self.sector).ok_or(())?;
        if buf.len() % self.sector != 0 || o.checked_add(buf.len()).is_none_or(|f| f > self.datos.len()) {
            return Err(());
        }
        if let Some(c) = self.corte.as_mut() {
            if *c == 0 {
                return Err(());
            }
            *c -= 1;
        }
        self.datos[o..o + buf.len()].copy_from_slice(buf);
        self.escrituras.push((lba, (buf.len() / self.sector) as u64));
        Ok(())
    }
    fn vaciar(&mut self) -> Result<(), ()> {
        Ok(())
    }
}
