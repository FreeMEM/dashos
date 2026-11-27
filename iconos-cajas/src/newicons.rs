//! NewIcons: la imagen escrita como texto en los ToolTypes (descripción pública de Dirk Stöcker, 2002).
//!
//! Tras las líneas `" "` y `"*** DON'T EDIT THE FOLLOWING LINES!! ***"` van las de la primera imagen (`IM1=`) y, si la
//! hay, las de la segunda (`IM2=`). La primera línea de cada una empieza con la cabecera: transparencia (`B` sí, `C` no),
//! ancho + 0x21, alto + 0x21 y el número de colores en dos letras (`((a - 0x21) << 6) + (b - 0x21)`). Detrás, la
//! paleta (rojo, verde y azul, 8 bits cada uno) y, en las líneas siguientes, los píxeles (con los bits justos para los
//! colores que hay), todo como una tira de bits: cada letra de 0x20 a 0x6F vale 7 bits (0x00 a 0x4F), de 0xA1 a 0xD0
//! también (0x50 a 0x7F), y de 0xD1 a 0xFF son tantos grupos de 7 ceros como la letra menos 0xD0. Cada línea se
//! acaba con lo suyo: los bits que sobran al final se tiran, y la imagen empieza en la línea siguiente a la de la
//! paleta. (Comprobado con NewIcons de verdad: así salen exactamente las filas.)

use crate::amiga::Error;
use alloc::vec::Vec;
use kit::imagen::Imagen;

/// Lo más que mide un NewIcon por lado: la letra más alta (0xFF) menos 0x21.
const MAX_LADO: usize = 0xff - 0x21;
/// Colores de la paleta, como mucho (se han visto iconos con 257 guardados).
const MAX_COLORES: usize = 512;

/// Los bits de una línea.
struct Linea<'a> {
    letras: &'a [u8],
    i: usize,
    grupo: u8,
    /// Bits que quedan del grupo de ahora.
    quedan: u32,
    /// Grupos de ceros que quedan de una letra de repetición.
    ceros: u32,
}

impl<'a> Linea<'a> {
    fn new(letras: &'a [u8]) -> Self {
        Linea { letras, i: 0, grupo: 0, quedan: 0, ceros: 0 }
    }

    fn bit(&mut self) -> Option<u16> {
        loop {
            if self.quedan > 0 {
                self.quedan -= 1;
                return Some(((self.grupo >> self.quedan) & 1) as u16);
            }
            if self.ceros > 0 {
                self.ceros -= 1;
                self.grupo = 0;
                self.quedan = 7;
                continue;
            }
            let c = *self.letras.get(self.i)?;
            self.i += 1;
            match c {
                0x20..=0x6f => (self.grupo, self.quedan) = (c - 0x20, 7),
                0xa1..=0xd0 => (self.grupo, self.quedan) = (c - 0x51, 7),
                0xd1..=0xff => self.ceros = (c - 0xd0) as u32,
                // Una letra que no es del código: la línea se acaba ahí.
                _ => {
                    self.i = self.letras.len();
                    return None;
                }
            }
        }
    }

    /// Los `n` bits siguientes, o `None` si la línea se acaba antes (lo que sobra se tira).
    fn leer(&mut self, n: u32) -> Option<u16> {
        let mut v = 0u16;
        for _ in 0..n {
            v = v << 1 | self.bit()?;
        }
        Some(v)
    }
}

/// Lee una imagen de sus líneas (sin el `IMn=`).
fn imagen(lineas: &[&[u8]]) -> Result<Imagen, Error> {
    let cab = lineas.first().ok_or(Error::Corto)?;
    if cab.len() < 5 {
        return Err(Error::Corto);
    }
    let menos = |c: u8| c.checked_sub(0x21).ok_or(Error::MalFormado);
    let transparente = cab[0] == b'B';
    let (ancho, alto) = (menos(cab[1])? as usize, menos(cab[2])? as usize);
    let colores = ((menos(cab[3])? as usize) << 6) + menos(cab[4])? as usize;
    if ancho == 0 || alto == 0 || ancho > MAX_LADO || alto > MAX_LADO || colores == 0 || colores > MAX_COLORES {
        return Err(Error::MalFormado);
    }
    let mut bits = 1;
    while (1usize << bits) < colores {
        bits += 1;
    }
    // La paleta, desde detrás de la cabecera, en las líneas que haga falta.
    let mut n = 0;
    let mut linea = Linea::new(&cab[5..]);
    let mut rgb = Vec::with_capacity(colores * 3);
    while rgb.len() < colores * 3 {
        match linea.leer(8) {
            Some(v) => rgb.push(v as u8),
            None => {
                n += 1;
                linea = Linea::new(lineas.get(n).ok_or(Error::Corto)?);
            }
        }
    }
    let paleta: Vec<u32> = rgb.chunks(3).map(|c| 0xff00_0000 | (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32).collect();
    // Los píxeles, desde la línea siguiente.
    let total = ancho * alto;
    let mut px = Vec::with_capacity(total);
    for l in lineas.iter().skip(n + 1) {
        let mut linea = Linea::new(l);
        while px.len() < total {
            let Some(i) = linea.leer(bits) else { break };
            let i = i as usize;
            px.push(if (transparente && i == 0) || i >= paleta.len() { 0 } else { paleta[i] });
        }
        if px.len() == total {
            return Ok(Imagen { ancho, alto, px });
        }
    }
    Err(Error::Corto)
}

/// Las líneas de la imagen `prefijo` (`IM1=` o `IM2=`), sin él.
fn lineas<'a>(tooltypes: &[&'a [u8]], prefijo: &[u8]) -> Vec<&'a [u8]> {
    tooltypes.iter().filter_map(|t| t.strip_prefix(prefijo)).collect()
}

/// La primera imagen y la segunda (si la hay y se lee bien), si los ToolTypes traen un NewIcon que se pueda leer.
pub fn leer(tooltypes: &[&[u8]]) -> Option<(Imagen, Option<Imagen>)> {
    let primera = lineas(tooltypes, b"IM1=");
    if primera.is_empty() {
        return None;
    }
    let normal = imagen(&primera).ok()?;
    let segunda = lineas(tooltypes, b"IM2=");
    let sel = if segunda.is_empty() { None } else { imagen(&segunda).ok() };
    Some((normal, sel))
}
