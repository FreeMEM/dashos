//! Leer bytes en big-endian (el orden del 68k) y bits, sin salirse nunca de lo que hay.

use crate::amiga::Error;

/// Un cursor sobre los bytes del fichero.
pub struct Lector<'a> {
    d: &'a [u8],
    pub pos: usize,
}

impl<'a> Lector<'a> {
    pub fn new(d: &'a [u8]) -> Self {
        Lector { d, pos: 0 }
    }

    /// `n` bytes desde donde va, y avanza.
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let fin = self.pos.checked_add(n).ok_or(Error::Corto)?;
        let b = self.d.get(self.pos..fin).ok_or(Error::Corto)?;
        self.pos = fin;
        Ok(b)
    }

    pub fn u32(&mut self) -> Result<u32, Error> {
        let b = self.bytes(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Lo que queda por leer.
    pub fn resto(&self) -> &'a [u8] {
        self.d.get(self.pos..).unwrap_or(&[])
    }
}

/// Un u16 o u32 en big-endian en `pos` de `d` (para las cabeceras de tamaño fijo, ya medidas).
pub fn be16(d: &[u8], pos: usize) -> u16 {
    u16::from_be_bytes([d[pos], d[pos + 1]])
}

pub fn be32(d: &[u8], pos: usize) -> u32 {
    u32::from_be_bytes([d[pos], d[pos + 1], d[pos + 2], d[pos + 3]])
}

/// Bits de un trozo de bytes, del más alto al más bajo (como los escriben GlowIcons y NewIcons).
pub struct Bits<'a> {
    d: &'a [u8],
    /// El bit siguiente, contando desde el principio.
    bit: usize,
}

impl<'a> Bits<'a> {
    pub fn new(d: &'a [u8]) -> Self {
        Bits { d, bit: 0 }
    }

    /// Los bits que quedan.
    pub fn quedan(&self) -> usize {
        (self.d.len() * 8).saturating_sub(self.bit)
    }

    /// Los `n` bits siguientes (de 1 a 8) como número, o `None` si no quedan tantos.
    pub fn leer(&mut self, n: u32) -> Option<u8> {
        if n == 0 || n > 8 || self.quedan() < n as usize {
            return None;
        }
        let mut v = 0u32;
        for _ in 0..n {
            let b = (self.d[self.bit / 8] >> (7 - self.bit % 8)) & 1;
            v = v << 1 | b as u32;
            self.bit += 1;
        }
        Some(v as u8)
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn no_se_sale() {
        let mut l = Lector::new(&[1, 2, 3, 4, 5]);
        assert_eq!(l.u32(), Ok(0x0102_0304));
        assert_eq!(l.u32(), Err(Error::Corto));
        assert_eq!(l.bytes(1), Ok(&[5u8][..]));
        assert_eq!(l.bytes(1), Err(Error::Corto));
        assert_eq!(l.bytes(usize::MAX), Err(Error::Corto));
        let mut b = Bits::new(&[0b1010_0000]);
        assert_eq!(b.leer(3), Some(0b101));
        assert_eq!(b.leer(5), Some(0));
        assert_eq!(b.leer(1), None);
        assert_eq!(Bits::new(&[0xff]).leer(9), None);
    }
}
