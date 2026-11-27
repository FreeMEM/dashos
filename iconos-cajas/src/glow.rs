//! OS 3.5 (GlowIcons): el `FORM ICON` de IFF que va detrás de lo clásico (descripción pública de Dirk Stöcker, 2002).
//!
//! Trozos que se leen (los demás se saltan, y un trozo de largo impar lleva un byte de relleno):
//! - `FACE` (6 bytes): ancho - 1, alto - 1, opciones (bit 0: sin marco), aspecto y bytes de paleta - 1.
//! - `IMAG` (uno por imagen, el segundo es la de seleccionado): color transparente, colores - 1, opciones (bit 0: hay
//!   transparente; bit 1: trae paleta, si no usa la de la primera), formato de la imagen y de la paleta (0 tal cual, 1
//!   con repeticiones), bits por píxel, bytes de la imagen - 1 y de la paleta - 1 (u16), y luego los datos. Las
//!   repeticiones son las de ILBM sobre una tira de bits: un byte de control n; de 0 a 127, copiar los n + 1 valores
//!   siguientes; 128, nada; de 129 a 255, repetir el valor siguiente 257 - n veces. Cada valor tiene los bits por
//!   píxel (o 8 en la paleta). Sin comprimir, un byte por píxel.
//! - `ARGB` (OS4, uno por imagen): 10 bytes de cabecera y un flujo zlib con ancho × alto píxeles en A, R, G, B. Sin
//!   documentación pública; deducido de los bytes de iconos de verdad (el tamaño sale de `FACE`).

use crate::amiga::Error;
use crate::lector::{be16, be32, Bits};
use alloc::vec::Vec;
use kit::imagen::Imagen;

pub struct Glow {
    pub normal: Imagen,
    pub seleccionado: Option<Imagen>,
    pub sin_marco: bool,
    /// Si salió de los trozos ARGB.
    pub argb: bool,
}

/// Descomprime las repeticiones hasta tener `n` valores de `bits` bits.
fn repeticiones(d: &[u8], bits: u32, n: usize) -> Result<Vec<u8>, Error> {
    let mut b = Bits::new(d);
    let mut sal = Vec::with_capacity(n);
    while sal.len() < n {
        // Cada vuelta gasta al menos los 8 bits del control: con lo que hay, se acaba.
        let c = b.leer(8).ok_or(Error::Corto)?;
        if c < 128 {
            for _ in 0..=c {
                let v = b.leer(bits).ok_or(Error::Corto)?;
                if sal.len() < n {
                    sal.push(v);
                }
            }
        } else if c > 128 {
            let v = b.leer(bits).ok_or(Error::Corto)?;
            let veces = (257 - c as usize).min(n - sal.len());
            sal.extend(core::iter::repeat_n(v, veces));
        }
    }
    Ok(sal)
}

/// Una imagen de un trozo `IMAG`. `paleta`: la de la primera, para la segunda que no trae.
fn imag(d: &[u8], ancho: usize, alto: usize, paleta: Option<&[u32]>) -> Result<(Imagen, Vec<u32>), Error> {
    if d.len() < 10 {
        return Err(Error::Corto);
    }
    let (transparente, colores, opciones, f_img, f_pal, bits) = (d[0], d[1] as usize + 1, d[2], d[3], d[4], d[5]);
    let (t_img, t_pal) = (be16(d, 6) as usize + 1, be16(d, 8) as usize + 1);
    if !(1..=8).contains(&bits) || f_img > 1 || f_pal > 1 {
        return Err(Error::MalFormado);
    }
    let datos = &d[10..];
    let img = datos.get(..t_img).ok_or(Error::Corto)?;
    let n = ancho * alto;
    let indices = if f_img == 0 { img.get(..n).ok_or(Error::Corto)?.to_vec() } else { repeticiones(img, bits as u32, n)? };
    let paleta: Vec<u32> = if opciones & 2 != 0 {
        let p = datos.get(t_img..t_img + t_pal).ok_or(Error::Corto)?;
        let rgb = if f_pal == 0 { p.get(..colores * 3).ok_or(Error::Corto)?.to_vec() } else { repeticiones(p, 8, colores * 3)? };
        rgb.chunks(3).map(|c| 0xff00_0000 | (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32).collect()
    } else {
        paleta.ok_or(Error::MalFormado)?.to_vec()
    };
    let px = indices
        .iter()
        .map(|&i| if (opciones & 1 != 0 && i == transparente) || i as usize >= paleta.len() { 0 } else { paleta[i as usize] })
        .collect();
    Ok((Imagen { ancho, alto, px }, paleta))
}

/// Una imagen de un trozo `ARGB`.
fn argb(d: &[u8], ancho: usize, alto: usize) -> Result<Imagen, Error> {
    let n = ancho * alto * 4;
    let z = d.get(10..).ok_or(Error::Corto)?;
    let crudo = kit::png::zlib(z, n).map_err(|_| Error::Comprimido)?;
    if crudo.len() != n {
        return Err(Error::Corto);
    }
    Ok(Imagen { ancho, alto, px: crudo.chunks(4).map(|c| be32(c, 0)).collect() })
}

/// Lee el `FORM ICON` del principio de `d`.
pub fn leer(d: &[u8]) -> Result<Glow, Error> {
    if d.len() < 12 || &d[..4] != b"FORM" || &d[8..12] != b"ICON" {
        return Err(Error::NoEsIcono);
    }
    let fin = (be32(d, 4) as usize).saturating_add(8).min(d.len());
    let mut cara = None;
    let (mut imags, mut argbs): (Vec<&[u8]>, Vec<&[u8]>) = (Vec::new(), Vec::new());
    let mut pos = 12usize;
    while pos + 8 <= fin {
        let largo = be32(d, pos + 4) as usize;
        let Some(datos) = pos.checked_add(8 + largo).filter(|&f| f <= fin).map(|f| &d[pos + 8..f]) else { break };
        match &d[pos..pos + 4] {
            b"FACE" if datos.len() >= 6 => cara = Some((datos[0] as usize + 1, datos[1] as usize + 1, datos[2] & 1 != 0)),
            b"IMAG" if imags.len() < 2 => imags.push(datos),
            b"ARGB" if argbs.len() < 2 => argbs.push(datos),
            _ => {}
        }
        pos += 8 + largo + (largo & 1);
    }
    let (ancho, alto, sin_marco) = cara.ok_or(Error::MalFormado)?;
    if let Some(a) = argbs.first()
        && let Ok(normal) = argb(a, ancho, alto)
    {
        let seleccionado = argbs.get(1).and_then(|s| argb(s, ancho, alto).ok());
        return Ok(Glow { normal, seleccionado, sin_marco, argb: true });
    }
    let (normal, paleta) = imag(imags.first().ok_or(Error::MalFormado)?, ancho, alto, None)?;
    let seleccionado = imags.get(1).and_then(|s| imag(s, ancho, alto, Some(&paleta)).ok()).map(|i| i.0);
    Ok(Glow { normal, seleccionado, sin_marco, argb: false })
}
