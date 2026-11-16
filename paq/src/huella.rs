//! Huellas: CRC-32 (la de los ficheros de la base y la del gzip) y SHA-256 (paquetes, índices y la `Sum:` de los
//! ficheros de texto), y el hexadecimal.

use alloc::string::String;
use sha2::{Digest, Sha256};

/// La tabla del CRC-32 de IEEE (el de gzip y zlib), con el polinomio al revés.
const fn tabla_crc() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}
static CRC: [u32; 256] = tabla_crc();

/// El CRC-32 de `datos`.
pub fn crc32(datos: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in datos {
        c = CRC[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    !c
}

/// El SHA-256 de `datos`.
pub fn sha256(datos: &[u8]) -> [u8; 32] {
    Sha256::digest(datos).into()
}

/// `datos` en hexadecimal, en minúsculas.
pub fn hex(datos: &[u8]) -> String {
    const C: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(datos.len() * 2);
    for &b in datos {
        s.push(C[(b >> 4) as usize] as char);
        s.push(C[(b & 15) as usize] as char);
    }
    s
}

/// De hexadecimal (mayúsculas o minúsculas) a los `N` bytes justos; si no, `None`.
pub fn de_hex<const N: usize>(t: &str) -> Option<[u8; N]> {
    let t = t.as_bytes();
    if t.len() != 2 * N {
        return None;
    }
    let v = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    let mut s = [0u8; N];
    for i in 0..N {
        s[i] = (v(t[2 * i])? << 4) | v(t[2 * i + 1])?;
    }
    Some(s)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn vectores_conocidos() {
        // Los de siempre: "123456789" (CRC-32 de IEEE) y "abc" (FIPS 180-2).
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(de_hex::<2>("aBff"), Some([0xab, 0xff]));
        assert_eq!(de_hex::<2>("abf"), None);
        assert_eq!(de_hex::<1>("zz"), None);
    }
}
