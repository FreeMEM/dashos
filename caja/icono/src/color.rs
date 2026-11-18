//! Las imágenes de color de un icono: el `FORM ICON` de OS 3.5 (con paleta, y el trozo `ARGB` de OS4), las de
//! NewIcons (en los tooltypes) y los iconos PNG de OS4 y OS 3.2.
//!
//! Formatos, de la descripción pública de Dirk Stöcker (2002) y de lo que explica el lector de DashOS
//! (`iconos-cajas`, MPL, propio), del que salen las ideas; aquí se guarda además la paleta y los índices, que es lo
//! que icon.library da a los programas (`ICONCTRLA_GetImageData1`, `GetPalette1`...), y se escribe el `FORM ICON`.
//!
//! - `FACE` (6 bytes): ancho - 1, alto - 1, opciones (bit 0: sin marco), aspecto y bytes de paleta - 1 (u16).
//! - `IMAG` (uno por imagen): color transparente, colores - 1, opciones (bit 0: hay transparente; bit 1: trae
//!   paleta), formato de la imagen y de la paleta (0 tal cual, un byte por valor; 1 con repeticiones), bits por píxel,
//!   bytes de la imagen - 1 y de la paleta - 1 (u16) y los datos. Las repeticiones van sobre una tira de bits: un
//!   control de 8 bits n; de 0 a 127, n + 1 valores sueltos; 128, nada; de 129 a 255, el valor siguiente 257 - n veces.
//! - `ARGB` (OS4): 10 bytes de cabecera y zlib con ancho × alto píxeles A, R, G, B. Se lee para pintar y se vuelve a
//!   escribir tal cual (si no se cambió la imagen).
//! - NewIcons: líneas `IM1=` e `IM2=` en los tooltypes; cabecera (transparencia `B`/`C`, ancho y alto + 0x21, colores
//!   en dos letras), paleta y píxeles en una tira de bits de 7 por letra (0x20-0x6F y 0xA1-0xD0; 0xD1-0xFF son grupos
//!   de ceros). Cada línea acaba lo suyo.
//! - PNG: el fichero entero es un PNG (con un trozo `icOn` con los datos del icono) y, si hay imagen de seleccionado,
//!   un segundo PNG pegado detrás.

use alloc::vec;
use alloc::vec::Vec;

use crate::info::{Error, MAX_LADO, Planar};

/// Una imagen con paleta: un índice por píxel, la paleta (RGB) y el color transparente, si lo hay.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ImagenColor {
    pub indices: Vec<u8>,
    pub paleta: Vec<[u8; 3]>,
    pub transparente: Option<u8>,
}

impl ImagenColor {
    /// En ARGB (lo transparente con alfa 0).
    pub fn argb(&self) -> Vec<u32> {
        self.indices
            .iter()
            .map(|&i| match self.paleta.get(i as usize) {
                Some(c) if self.transparente != Some(i) => {
                    0xFF00_0000 | (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32
                }
                _ => 0,
            })
            .collect()
    }
}

/// Las imágenes de color de un icono.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Color {
    pub ancho: u16,
    pub alto: u16,
    pub sin_marco: bool,
    /// `PACK_ICON_ASPECT_RATIO`: numerador << 4 | denominador; 0, sin saber.
    pub aspecto: u8,
    /// Con paleta (`IMAG` o NewIcons): la normal y la de seleccionado.
    pub img: [Option<ImagenColor>; 2],
    /// En ARGB (PNG, o el trozo `ARGB` de OS4).
    pub argb: [Option<Vec<u32>>; 2],
    /// Trozos del `FORM ICON` que no se entienden o que se escriben tal cual (`ARGB`...), por orden.
    pub extra: Vec<([u8; 4], Vec<u8>)>,
}

impl Color {
    /// La imagen `i` (0 normal, 1 seleccionado) en ARGB, la mejor que haya.
    pub fn imagen_argb(&self, i: usize) -> Option<Vec<u32>> {
        if let Some(a) = &self.argb[i] {
            return Some(a.clone());
        }
        self.img[i].as_ref().map(|m| m.argb())
    }
    pub fn con_paleta(&self) -> bool {
        self.img[0].is_some()
    }
    fn pixeles(&self) -> usize {
        self.ancho as usize * self.alto as usize
    }
    /// Que lo de dentro cuadre con las medidas (tras un `IconControlA` que las cambie, por ejemplo).
    pub fn coherente(&self) -> bool {
        let n = self.pixeles();
        self.img.iter().flatten().all(|m| m.indices.len() == n && m.paleta.len() <= 256)
            && self.argb.iter().flatten().all(|a| a.len() == n)
    }
}

fn be16(d: &[u8], p: usize) -> u16 {
    u16::from_be_bytes([d[p], d[p + 1]])
}
fn be32(d: &[u8], p: usize) -> u32 {
    u32::from_be_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]])
}

/// Bits de un trozo de bytes, del más alto al más bajo.
struct Bits<'a> {
    d: &'a [u8],
    bit: usize,
}

impl Bits<'_> {
    fn leer(&mut self, n: u32) -> Option<u8> {
        if n == 0 || n > 8 || self.d.len() * 8 < self.bit + n as usize {
            return None;
        }
        let mut v = 0u32;
        for _ in 0..n {
            v = v << 1 | ((self.d[self.bit / 8] >> (7 - self.bit % 8)) & 1) as u32;
            self.bit += 1;
        }
        Some(v as u8)
    }
}

/// Escribe valores de `bits` bits seguidos, del más alto al más bajo.
struct Tira {
    v: Vec<u8>,
    bit: usize,
}

impl Tira {
    fn poner(&mut self, valor: u8, bits: u32) {
        for k in (0..bits).rev() {
            if self.bit.is_multiple_of(8) {
                self.v.push(0);
            }
            if (valor >> k) & 1 != 0 {
                let i = self.bit / 8;
                self.v[i] |= 0x80 >> (self.bit % 8);
            }
            self.bit += 1;
        }
    }
}

/// Deshace las repeticiones hasta tener `n` valores de `bits` bits.
fn repeticiones(d: &[u8], bits: u32, n: usize) -> Result<Vec<u8>, Error> {
    let mut b = Bits { d, bit: 0 };
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

/// Pone las repeticiones: tramos de 3 o más iguales como repetición, lo demás suelto (de 128 en 128 como mucho).
fn comprimir(valores: &[u8], bits: u32) -> Vec<u8> {
    let mut t = Tira { v: Vec::new(), bit: 0 };
    let mut i = 0;
    let mut sueltos: Vec<u8> = Vec::new();
    let vaciar = |t: &mut Tira, s: &mut Vec<u8>| {
        for trozo in s.chunks(128) {
            t.poner((trozo.len() - 1) as u8, 8);
            for &v in trozo {
                t.poner(v, bits);
            }
        }
        s.clear();
    };
    while i < valores.len() {
        let mut j = i + 1;
        while j < valores.len() && valores[j] == valores[i] && j - i < 128 {
            j += 1;
        }
        if j - i >= 3 {
            vaciar(&mut t, &mut sueltos);
            t.poner((257 - (j - i)) as u8, 8);
            t.poner(valores[i], bits);
        } else {
            sueltos.extend_from_slice(&valores[i..j]);
        }
        i = j;
    }
    vaciar(&mut t, &mut sueltos);
    t.v
}

fn bits_para(colores: usize) -> u32 {
    let mut b = 1;
    while (1usize << b) < colores {
        b += 1;
    }
    b
}

/// Una imagen de un trozo `IMAG`. `paleta`: la de la primera, para una segunda que no trae.
fn imag(d: &[u8], n: usize, paleta: Option<&[[u8; 3]]>) -> Result<ImagenColor, Error> {
    if d.len() < 10 {
        return Err(Error::Corto);
    }
    let (transp, colores, opciones, f_img, f_pal, bits) = (d[0], d[1] as usize + 1, d[2], d[3], d[4], d[5]);
    let (t_img, t_pal) = (be16(d, 6) as usize + 1, be16(d, 8) as usize + 1);
    if !(1..=8).contains(&bits) || f_img > 1 || f_pal > 1 {
        return Err(Error::MalFormado);
    }
    let datos = &d[10..];
    let img = datos.get(..t_img).ok_or(Error::Corto)?;
    let indices =
        if f_img == 0 { img.get(..n).ok_or(Error::Corto)?.to_vec() } else { repeticiones(img, bits as u32, n)? };
    let paleta: Vec<[u8; 3]> = if opciones & 2 != 0 {
        let p = datos.get(t_img..t_img + t_pal).ok_or(Error::Corto)?;
        let rgb = if f_pal == 0 {
            p.get(..colores * 3).ok_or(Error::Corto)?.to_vec()
        } else {
            repeticiones(p, 8, colores * 3)?
        };
        rgb.chunks(3).map(|c| [c[0], c[1], c[2]]).collect()
    } else {
        paleta.ok_or(Error::MalFormado)?.to_vec()
    };
    Ok(ImagenColor { indices, paleta, transparente: (opciones & 1 != 0).then_some(transp) })
}

/// Una imagen de un trozo `ARGB` de OS4 (10 bytes de cabecera y zlib).
fn argb(d: &[u8], n: usize) -> Result<Vec<u32>, Error> {
    let z = d.get(10..).ok_or(Error::Corto)?;
    let crudo = kit::png::zlib(z, n * 4).map_err(|_| Error::Comprimido)?;
    if crudo.len() != n * 4 {
        return Err(Error::Corto);
    }
    Ok(crudo.chunks(4).map(|c| be32(c, 0)).collect())
}

/// Lee el `FORM ICON` del principio de `d`.
pub fn leer_glow(d: &[u8]) -> Result<Color, Error> {
    if d.len() < 12 || &d[..4] != b"FORM" || &d[8..12] != b"ICON" {
        return Err(Error::NoEsIcono);
    }
    let fin = (be32(d, 4) as usize).saturating_add(8).min(d.len());
    let mut c = Color::default();
    let mut cara = false;
    let (mut imags, mut argbs): (Vec<&[u8]>, Vec<&[u8]>) = (Vec::new(), Vec::new());
    let mut pos = 12usize;
    let mut trozos = 0;
    while pos + 8 <= fin {
        trozos += 1;
        if trozos > 256 {
            return Err(Error::MalFormado);
        }
        let largo = be32(d, pos + 4) as usize;
        let Some(datos) =
            pos.checked_add(8).and_then(|p| p.checked_add(largo)).filter(|&f| f <= fin).map(|f| &d[pos + 8..f])
        else {
            return Err(Error::Corto);
        };
        let id: [u8; 4] = [d[pos], d[pos + 1], d[pos + 2], d[pos + 3]];
        match &id {
            b"FACE" if datos.len() >= 6 => {
                c.ancho = datos[0] as u16 + 1;
                c.alto = datos[1] as u16 + 1;
                c.sin_marco = datos[2] & 1 != 0;
                c.aspecto = datos[3];
                cara = true;
            }
            b"IMAG" if imags.len() < 2 => imags.push(datos),
            b"ARGB" => {
                if argbs.len() < 2 {
                    argbs.push(datos);
                }
                c.extra.push((id, datos.to_vec()));
            }
            _ => c.extra.push((id, datos.to_vec())),
        }
        pos = pos.saturating_add(8 + largo + (largo & 1));
    }
    if !cara {
        return Err(Error::MalFormado);
    }
    let n = c.pixeles();
    if let Some(i) = imags.first() {
        let primera = imag(i, n, None)?;
        let segunda = imags.get(1).and_then(|s| imag(s, n, Some(&primera.paleta)).ok());
        c.img = [Some(primera), segunda];
    }
    for (k, a) in argbs.iter().enumerate() {
        c.argb[k] = argb(a, n).ok();
    }
    if c.img[0].is_none() && c.argb[0].is_none() {
        return Err(Error::MalFormado);
    }
    Ok(c)
}

fn trozo(v: &mut Vec<u8>, id: &[u8; 4], datos: &[u8]) {
    v.extend_from_slice(id);
    v.extend_from_slice(&(datos.len() as u32).to_be_bytes());
    v.extend_from_slice(datos);
    if datos.len() & 1 == 1 {
        v.push(0);
    }
}

fn imag_de(m: &ImagenColor, paleta_de_la_primera: Option<&[[u8; 3]]>) -> Vec<u8> {
    let colores = m.paleta.len().clamp(1, 256);
    let bits = bits_para(colores);
    let trae_paleta = paleta_de_la_primera != Some(&m.paleta[..]);
    let rle = comprimir(&m.indices, bits);
    let (f_img, img) =
        if rle.len() < m.indices.len() && rle.len() <= 65536 { (1u8, rle) } else { (0u8, m.indices.clone()) };
    let mut pal: Vec<u8> = m.paleta.iter().take(256).flat_map(|c| c.iter().copied()).collect();
    if pal.is_empty() {
        pal = vec![0, 0, 0];
    }
    let mut d = vec![
        m.transparente.unwrap_or(0),
        (colores - 1) as u8,
        m.transparente.is_some() as u8 | (trae_paleta as u8) << 1,
        f_img,
        0,
        bits as u8,
    ];
    d.extend_from_slice(&((img.len().max(1) - 1) as u16).to_be_bytes());
    d.extend_from_slice(&(((if trae_paleta { pal.len() } else { 1 }) - 1) as u16).to_be_bytes());
    d.extend_from_slice(&img);
    if trae_paleta {
        d.extend_from_slice(&pal);
    }
    d
}

/// El `FORM ICON` de `c`. Las imágenes ARGB sin paleta (las de un PNG) se pasan a paleta (`cuantizar`). `con_extra`:
/// con los trozos que se guardaron tal cual (no, si la imagen cambió: el `ARGB` de OS4 sería el de antes).
pub fn escribir_glow(c: &Color, con_extra: bool) -> Vec<u8> {
    let mut imgs: [Option<ImagenColor>; 2] = c.img.clone();
    for (img, argb) in imgs.iter_mut().zip(&c.argb) {
        if img.is_none()
            && let Some(a) = argb
        {
            *img = Some(cuantizar(a));
        }
    }
    let mut cuerpo = Vec::new();
    let max_pal = imgs.iter().flatten().map(|m| m.paleta.len().clamp(1, 256) * 3).max().unwrap_or(3);
    let mut face =
        vec![(c.ancho.clamp(1, 256) - 1) as u8, (c.alto.clamp(1, 256) - 1) as u8, c.sin_marco as u8, c.aspecto];
    face.extend_from_slice(&((max_pal - 1) as u16).to_be_bytes());
    trozo(&mut cuerpo, b"FACE", &face);
    if let Some(p) = &imgs[0] {
        trozo(&mut cuerpo, b"IMAG", &imag_de(p, None));
        if let Some(s) = &imgs[1] {
            trozo(&mut cuerpo, b"IMAG", &imag_de(s, Some(&p.paleta)));
        }
    }
    if con_extra {
        for (id, d) in &c.extra {
            trozo(&mut cuerpo, id, d);
        }
    }
    let mut v = Vec::with_capacity(cuerpo.len() + 12);
    v.extend_from_slice(b"FORM");
    v.extend_from_slice(&(cuerpo.len() as u32 + 4).to_be_bytes());
    v.extend_from_slice(b"ICON");
    v.extend_from_slice(&cuerpo);
    v
}

/// ARGB a una imagen con paleta: los colores tal cual si son 255 o menos; si no, un cubo de 6 × 6 × 6. Lo que tiene
/// menos de la mitad de alfa es el transparente (el 0).
pub fn cuantizar(px: &[u32]) -> ImagenColor {
    let transparente = px.iter().any(|&p| p >> 24 < 0x80);
    let mut paleta: Vec<[u8; 3]> = if transparente { vec![[0, 0, 0]] } else { Vec::new() };
    let rgb = |p: u32| [(p >> 16) as u8, (p >> 8) as u8, p as u8];
    let mut distintos: Vec<[u8; 3]> = px.iter().filter(|&&p| p >> 24 >= 0x80).map(|&p| rgb(p)).collect();
    distintos.sort_unstable();
    distintos.dedup();
    let exacto = distintos.len() + paleta.len() <= 256;
    if exacto {
        paleta.extend_from_slice(&distintos);
    } else {
        for r in 0..6u8 {
            for g in 0..6u8 {
                for b in 0..6u8 {
                    paleta.push([r * 51, g * 51, b * 51]);
                }
            }
        }
    }
    let base = transparente as usize;
    let indices = px
        .iter()
        .map(|&p| {
            if p >> 24 < 0x80 {
                return 0;
            }
            let c = rgb(p);
            if exacto {
                (base + distintos.binary_search(&c).unwrap_or(0)) as u8
            } else {
                let q = |v: u8| ((v as u16 + 25) / 51) as usize;
                (base + q(c[0]) * 36 + q(c[1]) * 6 + q(c[2])) as u8
            }
        })
        .collect();
    ImagenColor { indices, paleta, transparente: transparente.then_some(0) }
}

/// Los cuatro colores de serie del Workbench 2 y 3 (gris, negro, blanco y azul).
pub const COLORES_WB: [u32; 4] = [0xFFAA_AAAA, 0xFF00_0000, 0xFFFF_FFFF, 0xFF66_88BB];

/// Una imagen planar de 2 planos (los cuatro colores del Workbench, el más parecido) sacada de una de color: la que
/// lleva el `DiskObject` cuando el icono no trae otra (los PNG), para los programas que pintan `GadgetRender`.
pub fn planar_de(px: &[u32], ancho: u16, alto: u16) -> Planar {
    let (an, al) = (ancho as usize, alto as usize);
    let fila = an.div_ceil(16) * 2;
    let mut datos = vec![0u8; fila * al * 2];
    for y in 0..al {
        for x in 0..an {
            let p = px.get(y * an + x).copied().unwrap_or(0);
            let i = if p >> 24 < 0x80 {
                0
            } else {
                let dist = |q: u32| {
                    let d = |s: u32| ((p >> s) & 0xFF).abs_diff((q >> s) & 0xFF);
                    d(16) * d(16) + d(8) * d(8) + d(0) * d(0)
                };
                (0..4).min_by_key(|&k| dist(COLORES_WB[k])).unwrap_or(0)
            };
            for plano in 0..2 {
                if (i >> plano) & 1 != 0 {
                    datos[plano * fila * al + y * fila + x / 8] |= 0x80 >> (x % 8);
                }
            }
        }
    }
    Planar { izq: 0, arriba: 0, ancho, alto, prof: 2, pick: 3, onoff: 0, datos: Some(datos) }
}

// --- NewIcons ---

/// Si un tooltype es de los de NewIcons (las líneas de la imagen y su cabecera).
pub fn es_linea_newicons(t: &[u8]) -> bool {
    t.starts_with(b"IM1=") || t.starts_with(b"IM2=") || t == b"*** DON'T EDIT THE FOLLOWING LINES!! ***"
}

struct Linea<'a> {
    letras: &'a [u8],
    i: usize,
    grupo: u8,
    quedan: u32,
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
                (self.grupo, self.quedan) = (0, 7);
                continue;
            }
            let c = *self.letras.get(self.i)?;
            self.i += 1;
            match c {
                0x20..=0x6f => (self.grupo, self.quedan) = (c - 0x20, 7),
                0xa1..=0xd0 => (self.grupo, self.quedan) = (c - 0x51, 7),
                0xd1..=0xff => self.ceros = (c - 0xd0) as u32,
                _ => {
                    self.i = self.letras.len();
                    return None;
                }
            }
        }
    }
    fn leer(&mut self, n: u32) -> Option<u16> {
        let mut v = 0u16;
        for _ in 0..n {
            v = v << 1 | self.bit()?;
        }
        Some(v)
    }
}

/// Una imagen de NewIcons de sus líneas (sin el `IMn=`): sus medidas y la imagen.
fn newicon(lineas: &[&[u8]]) -> Result<(u16, u16, ImagenColor), Error> {
    let cab = lineas.first().ok_or(Error::Corto)?;
    if cab.len() < 5 {
        return Err(Error::Corto);
    }
    let menos = |c: u8| c.checked_sub(0x21).ok_or(Error::MalFormado);
    let transparente = cab[0] == b'B';
    let (ancho, alto) = (menos(cab[1])? as usize, menos(cab[2])? as usize);
    let colores = ((menos(cab[3])? as usize) << 6) + menos(cab[4])? as usize;
    if ancho == 0 || alto == 0 || colores == 0 || colores > 256 {
        return Err(Error::MalFormado);
    }
    let bits = bits_para(colores);
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
    let paleta: Vec<[u8; 3]> = rgb.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
    let total = ancho * alto;
    let mut indices = Vec::with_capacity(total);
    for l in lineas.iter().skip(n + 1) {
        let mut linea = Linea::new(l);
        while indices.len() < total {
            let Some(i) = linea.leer(bits) else { break };
            indices.push(if i as usize >= colores { 0 } else { i as u8 });
        }
        if indices.len() == total {
            let img = ImagenColor { indices, paleta, transparente: transparente.then_some(0) };
            return Ok((ancho as u16, alto as u16, img));
        }
    }
    Err(Error::Corto)
}

/// Las imágenes de NewIcons de unos tooltypes, si las hay y se leen.
pub fn leer_newicons(tooltypes: &[Vec<u8>]) -> Option<Color> {
    let lineas = |p: &[u8]| -> Vec<&[u8]> { tooltypes.iter().filter_map(|t| t.strip_prefix(p)).collect() };
    let primera = lineas(b"IM1=");
    if primera.is_empty() {
        return None;
    }
    let (ancho, alto, normal) = newicon(&primera).ok()?;
    let segunda = lineas(b"IM2=");
    let sel = newicon(&segunda).ok().filter(|(a, h, _)| *a == ancho && *h == alto).map(|x| x.2);
    Some(Color { ancho, alto, img: [Some(normal), sel], ..Color::default() })
}

// --- PNG ---

const FIRMA_PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

pub fn es_png(d: &[u8]) -> bool {
    d.starts_with(&FIRMA_PNG)
}

/// Dónde acaba el PNG que empieza en `d` (tras su `IEND`).
fn fin_png(d: &[u8]) -> Option<usize> {
    let mut pos = 8usize;
    for _ in 0..100_000 {
        let cab = d.get(pos..pos.checked_add(8)?)?;
        let largo = be32(cab, 0) as usize;
        pos = pos.checked_add(12)?.checked_add(largo)?;
        if pos > d.len() {
            return None;
        }
        if &cab[4..8] == b"IEND" {
            return Some(pos);
        }
    }
    None
}

fn un_png(d: &[u8]) -> Result<(u16, u16, Vec<u32>), Error> {
    let (an, al) = kit::png::medidas(d).map_err(|_| Error::Comprimido)?;
    if an as usize > MAX_LADO || al as usize > MAX_LADO || an == 0 || al == 0 {
        return Err(Error::Grande);
    }
    let img = kit::png::leer(d, (MAX_LADO * MAX_LADO) as u64).map_err(|e| match e {
        kit::png::Error::Grande(..) => Error::Grande,
        kit::png::Error::Corto => Error::Corto,
        _ => Error::Comprimido,
    })?;
    Ok((img.ancho as u16, img.alto as u16, img.px))
}

/// Un icono PNG: sus imágenes (la de seleccionado, si va pegada detrás y mide lo mismo).
pub fn leer_png(d: &[u8]) -> Result<Color, Error> {
    let (ancho, alto, normal) = un_png(d)?;
    let sel = fin_png(d)
        .and_then(|f| d.get(f..))
        .filter(|r| es_png(r))
        .and_then(|r| un_png(r).ok())
        .filter(|(a, h, _)| *a == ancho && *h == alto)
        .map(|x| x.2);
    Ok(Color { ancho, alto, sin_marco: true, argb: [Some(normal), sel], ..Color::default() })
}
