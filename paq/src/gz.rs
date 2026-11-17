//! gzip (RFC 1952), solo para leer: el índice (`index.gz`) y los paquetes (`.tar.gz`). Un solo miembro, con su CRC-32
//! y su tamaño comprobados, sin nada detrás, y con un tope de lo que sale (contra las bombas de compresión).

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use miniz_oxide::inflate::TINFLStatus;
use miniz_oxide::inflate::core::{DecompressorOxide, decompress, inflate_flags};

use crate::huella;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mal {
    /// No empieza como un gzip (o un método que no es deflate, o marcas reservadas).
    Cabecera,
    /// El deflate no se entiende o está cortado.
    Datos,
    /// Sale más de lo que se deja.
    Grande,
    /// El CRC-32 o el tamaño del final no cuadran.
    Final,
    /// Hay algo detrás del final (otro miembro o basura).
    Sobra,
}

const FTEXT: u8 = 1;
const FHCRC: u8 = 2;
const FEXTRA: u8 = 4;
const FNAME: u8 = 8;
const FCOMMENT: u8 = 16;

/// Lo que hay dentro de `datos`, si no pasa de `tope` bytes.
pub fn descomprimir(datos: &[u8], tope: usize) -> Result<Vec<u8>, Mal> {
    if datos.len() < 18 || datos[0] != 0x1f || datos[1] != 0x8b || datos[2] != 8 {
        return Err(Mal::Cabecera);
    }
    let marcas = datos[3];
    if marcas & !(FTEXT | FHCRC | FEXTRA | FNAME | FCOMMENT) != 0 {
        return Err(Mal::Cabecera);
    }
    let mut i = 10;
    if marcas & FEXTRA != 0 {
        let n = u16::from_le_bytes([*datos.get(i).ok_or(Mal::Cabecera)?, *datos.get(i + 1).ok_or(Mal::Cabecera)?]) as usize;
        i += 2 + n;
    }
    for m in [FNAME, FCOMMENT] {
        if marcas & m != 0 {
            let fin = datos.get(i..).ok_or(Mal::Cabecera)?.iter().position(|&c| c == 0).ok_or(Mal::Cabecera)?;
            i += fin + 1;
        }
    }
    if marcas & FHCRC != 0 {
        i += 2;
    }
    if i + 8 > datos.len() {
        return Err(Mal::Cabecera);
    }
    let mut entrada = &datos[i..];
    let flags = inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;
    let mut sal: Vec<u8> = vec![0; entrada.len().saturating_mul(3).max(1024).min(tope.max(1))];
    let mut d = Box::<DecompressorOxide>::default();
    let mut hecho = 0;
    let mut leido = 0;
    loop {
        let (estado, en, fuera) = decompress(&mut d, entrada, &mut sal, hecho, flags);
        hecho += fuera;
        leido += en;
        match estado {
            TINFLStatus::Done => break,
            TINFLStatus::HasMoreOutput => {
                if en > entrada.len() || sal.len() >= tope {
                    return Err(Mal::Grande);
                }
                entrada = &entrada[en..];
                let nuevo = sal.len().saturating_mul(2).min(tope);
                sal.resize(nuevo, 0);
            }
            _ => return Err(Mal::Datos),
        }
    }
    sal.truncate(hecho);
    let fin = i + leido;
    let cola = datos.get(fin..fin + 8).ok_or(Mal::Final)?;
    let crc = u32::from_le_bytes(cola[0..4].try_into().unwrap());
    let largo = u32::from_le_bytes(cola[4..8].try_into().unwrap());
    if crc != huella::crc32(&sal) || largo != sal.len() as u32 {
        return Err(Mal::Final);
    }
    if fin + 8 != datos.len() {
        return Err(Mal::Sobra);
    }
    Ok(sal)
}

/// Un gzip de `datos` sin comprimir (bloques "stored" de deflate): para las pruebas y para quien necesite escribir uno
/// sin compresor. Lo lee cualquier gzip.
pub fn sin_comprimir(datos: &[u8]) -> Vec<u8> {
    let mut v = Vec::from([0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255]);
    let mut trozos = datos.chunks(65535).peekable();
    if trozos.peek().is_none() {
        v.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    while let Some(t) = trozos.next() {
        v.push(if trozos.peek().is_none() { 1 } else { 0 });
        let n = t.len() as u16;
        v.extend_from_slice(&n.to_le_bytes());
        v.extend_from_slice(&(!n).to_le_bytes());
        v.extend_from_slice(t);
    }
    v.extend_from_slice(&huella::crc32(datos).to_le_bytes());
    v.extend_from_slice(&(datos.len() as u32).to_le_bytes());
    v
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn comprimido(datos: &[u8]) -> Vec<u8> {
        let mut v = Vec::from([0x1f, 0x8b, 8, FNAME, 0, 0, 0, 0, 0, 3]);
        v.extend_from_slice(b"nombre.txt\0");
        v.extend_from_slice(&miniz_oxide::deflate::compress_to_vec(datos, 6));
        v.extend_from_slice(&huella::crc32(datos).to_le_bytes());
        v.extend_from_slice(&(datos.len() as u32).to_le_bytes());
        v
    }

    #[test]
    fn ida_y_vuelta() {
        let texto: Vec<u8> = (0..100_000u32).flat_map(|i| (i % 251).to_le_bytes()).collect();
        assert_eq!(descomprimir(&comprimido(&texto), 1 << 20).unwrap(), texto);
        assert_eq!(descomprimir(&sin_comprimir(&texto), 1 << 20).unwrap(), texto);
        assert_eq!(descomprimir(&sin_comprimir(b""), 10).unwrap(), b"");
    }

    #[test]
    fn hostiles() {
        let bueno = comprimido(&[b'a'; 50_000]);
        // Una bomba: 50 KB de "a" con un tope de 10 KB.
        assert_eq!(descomprimir(&bueno, 10_000), Err(Mal::Grande));
        // Cortado en cada sitio: nunca da por bueno lo que no lo es.
        for n in 0..bueno.len() {
            assert!(descomprimir(&bueno[..n], 1 << 20).is_err(), "cortado en {n}");
        }
        // Un byte cambiado en los datos o en el final.
        for i in [25, bueno.len() / 2, bueno.len() - 5, bueno.len() - 1] {
            let mut m = bueno.clone();
            m[i] ^= 0x40;
            assert!(descomprimir(&m, 1 << 20).is_err(), "cambiado en {i}");
        }
        // Algo detrás, y otra cosa que no es gzip.
        let mut sobra = bueno.clone();
        sobra.push(0);
        assert_eq!(descomprimir(&sobra, 1 << 20), Err(Mal::Sobra));
        assert_eq!(descomprimir(b"PK\x03\x04 no soy un gzip de nada", 1 << 20), Err(Mal::Cabecera));
        // Basura al azar, que no tumbe nada.
        let mut x = 0x1234_5678u32;
        for _ in 0..2000 {
            let mut v = Vec::from([0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3]);
            for _ in 0..(x % 200) {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                v.push(x as u8);
            }
            let _ = descomprimir(&v, 1 << 16);
        }
    }
}
