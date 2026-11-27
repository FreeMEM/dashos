//! PNG (C3 de S10c): lo que lee el fondo de escritorio. Hecho aquí desde las especificaciones públicas: PNG (W3C,
//! segunda edición) y zlib y deflate (RFC 1950 y RFC 1951).
//!
//! Lee todos los tipos de color (gris, color, paleta, gris con alfa y color con alfa), con 1, 2, 4, 8 y 16 bits, y
//! entrelazados (Adam7). Comprueba el CRC de cada trozo y el Adler-32 de los datos: un fichero roto da un error, nunca
//! un pánico. Antes de pedir memoria mira lo que mide la imagen: una que pase de `max_px` píxeles es un error, por
//! mucho que el fichero sea pequeño, y lo descomprimido no puede pasar de lo que la cabecera dice que mide.
//!
//! Sale una `Imagen` en ARGB sin premultiplicar (`0xAARRGGBB`); de 16 bits se queda el byte alto. Los trozos
//! auxiliares (gamma, perfil de color, texto...) se saltan.

use crate::textos::tr;
use crate::imagen::Imagen;
use alloc::vec::Vec;

/// Lo que puede salir mal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// No empieza por la firma de PNG.
    NoEsPng,
    /// Se acaba antes de tiempo.
    Corto,
    /// El CRC de un trozo no cuadra.
    Crc,
    /// Una cabecera o un trozo que no siguen la especificación.
    MalFormado,
    /// Lo comprimido no es deflate válido, o su Adler-32 no cuadra.
    Comprimido,
    /// Más píxeles de los que se dejan (ancho, alto).
    Grande(u32, u32),
    /// No hay memoria para ella.
    SinMemoria,
    /// Se pidió con sus índices (`leer_indexada`) y no es de paleta.
    NoEsPaleta,
}

impl Error {
    /// El porqué, para decírselo al usuario.
    pub fn texto(&self) -> &'static str {
        match self {
            Error::NoEsPng => tr(3, "no es un PNG"),
            Error::Corto => tr(4, "se acaba antes de tiempo"),
            Error::Crc => tr(5, "un trozo está roto (su CRC no cuadra)"),
            Error::MalFormado => tr(6, "no sigue el formato"),
            Error::Comprimido => tr(7, "lo comprimido está roto"),
            Error::Grande(..) => tr(8, "demasiado grande"),
            Error::SinMemoria => tr(9, "no cabe en la memoria"),
            Error::NoEsPaleta => tr(27, "no es una imagen de paleta"),
        }
    }
}

const FIRMA: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// La tabla del CRC-32 de PNG (polinomio 0xEDB88320, el de ISO 3309), hecha al compilar.
const TABLA_CRC: [u32; 256] = {
    let mut t = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[n] = c;
        n += 1;
    }
    t
};

/// El CRC-32 de `datos` (el de PNG y zlib).
pub fn crc32(datos: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in datos {
        c = TABLA_CRC[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    c ^ 0xffff_ffff
}

fn adler32(datos: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    // De 5552 en 5552 bytes no se desborda antes de reducir (lo de RFC 1950).
    for trozo in datos.chunks(5552) {
        for &x in trozo {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    b << 16 | a
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// Un `Vec` de `n` ceros, o `SinMemoria` si no hay (sin abortar el programa).
fn ceros<T: Clone + Default>(n: usize) -> Result<Vec<T>, Error> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| Error::SinMemoria)?;
    v.resize(n, T::default());
    Ok(v)
}

// ---------------------------------------------------------------------------------------------------------------------
// Deflate (RFC 1951).

/// Lee bits de menos a más significativo, como pide deflate.
struct Bits<'a> {
    datos: &'a [u8],
    pos: usize,
    buf: u64,
    hay: u32,
}

impl<'a> Bits<'a> {
    fn new(datos: &'a [u8]) -> Self {
        Bits { datos, pos: 0, buf: 0, hay: 0 }
    }

    /// Llena el búfer hasta tener `n` bits, si quedan bytes.
    fn llenar(&mut self, n: u32) {
        while self.hay < n && self.hay <= 56 {
            let b = match self.datos.get(self.pos) {
                Some(&b) => b,
                None => return,
            };
            self.pos += 1;
            self.buf |= (b as u64) << self.hay;
            self.hay += 8;
        }
    }

    fn bits(&mut self, n: u32) -> Result<u32, Error> {
        if n == 0 {
            return Ok(0);
        }
        self.llenar(n);
        if self.hay < n {
            return Err(Error::Corto);
        }
        let v = (self.buf & ((1u64 << n) - 1)) as u32;
        self.buf >>= n;
        self.hay -= n;
        Ok(v)
    }

    /// Lo que queda, desde el siguiente byte entero.
    fn al_byte(&mut self) {
        let sobra = self.hay % 8;
        self.buf >>= sobra;
        self.hay -= sobra;
    }
}

/// Bits de la tabla rápida de un código de Huffman: los códigos de hasta 9 bits salen de una vez.
const RAPIDA: u32 = 9;

/// Un código de Huffman canónico: cuántos códigos hay de cada largo, los símbolos ordenados por largo y una tabla para
/// los cortos (símbolo | largo << 9; 0 si no está).
struct Huffman {
    cuenta: [u16; 16],
    simbolos: [u16; 288],
    rapida: [u16; 1 << RAPIDA],
}

impl Huffman {
    /// De los largos de cada símbolo (0: no está). Un código con más códigos de los que caben es un error; uno con
    /// menos se acepta (deflate lo permite, por ejemplo, con una sola distancia).
    fn new(largos: &[u8]) -> Result<Huffman, Error> {
        let mut h = Huffman { cuenta: [0; 16], simbolos: [0; 288], rapida: [0; 1 << RAPIDA] };
        for &l in largos {
            h.cuenta[l as usize] += 1;
        }
        h.cuenta[0] = 0;
        let mut quedan = 1i32;
        for l in 1..16 {
            quedan = quedan * 2 - h.cuenta[l] as i32;
            if quedan < 0 {
                return Err(Error::Comprimido);
            }
        }
        // Dónde empieza cada largo en `simbolos`, y el primer código de cada largo.
        let mut desde = [0u16; 16];
        for l in 1..15 {
            desde[l + 1] = desde[l] + h.cuenta[l];
        }
        let mut codigo = [0u32; 16];
        let mut c = 0u32;
        for l in 1..16 {
            codigo[l] = c;
            c = (c + h.cuenta[l] as u32) << 1;
        }
        for (s, &l) in largos.iter().enumerate() {
            if l == 0 {
                continue;
            }
            let l = l as usize;
            h.simbolos[desde[l] as usize] = s as u16;
            desde[l] += 1;
            let cod = codigo[l];
            codigo[l] += 1;
            if l as u32 <= RAPIDA {
                // El código va del bit más alto al más bajo; en el flujo, el primero que llega es el más alto.
                let mut rev = 0u32;
                for k in 0..l {
                    rev |= ((cod >> k) & 1) << (l - 1 - k);
                }
                let mut i = rev;
                while i < 1 << RAPIDA {
                    h.rapida[i as usize] = s as u16 | (l as u16) << 9;
                    i += 1 << l;
                }
            }
        }
        Ok(h)
    }

    fn simbolo(&self, b: &mut Bits) -> Result<u16, Error> {
        b.llenar(RAPIDA);
        let e = self.rapida[(b.buf & ((1 << RAPIDA) - 1)) as usize];
        if e != 0 && (e >> 9) as u32 <= b.hay {
            let l = (e >> 9) as u32;
            b.buf >>= l;
            b.hay -= l;
            return Ok(e & 0x1ff);
        }
        // Largo: bit a bit, con el orden canónico (el primer código de cada largo y cuántos hay).
        let (mut codigo, mut primero, mut indice) = (0i32, 0i32, 0i32);
        for l in 1..16 {
            codigo |= b.bits(1)? as i32;
            let n = self.cuenta[l] as i32;
            if codigo - n < primero {
                return Ok(self.simbolos[(indice + codigo - primero) as usize]);
            }
            indice += n;
            primero = (primero + n) << 1;
            codigo <<= 1;
        }
        Err(Error::Comprimido)
    }
}

const BASE_LARGO: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const EXTRA_LARGO: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const BASE_DIST: [u16; 30] =
    [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const EXTRA_DIST: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
/// El orden de los largos del código de los largos (RFC 1951, 3.2.7).
const ORDEN: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

/// Descomprime un flujo deflate en `salida`, que no pasa de `max` bytes (más es un error: lo que dice la cabecera de la
/// imagen manda).
fn inflar(b: &mut Bits, salida: &mut Vec<u8>, max: usize) -> Result<(), Error> {
    loop {
        let ultimo = b.bits(1)? == 1;
        match b.bits(2)? {
            0 => {
                b.al_byte();
                let largo = b.bits(16)?;
                let contra = b.bits(16)?;
                if largo != !contra & 0xffff {
                    return Err(Error::Comprimido);
                }
                if salida.len() + largo as usize > max {
                    return Err(Error::Comprimido);
                }
                for _ in 0..largo {
                    salida.push(b.bits(8)? as u8);
                }
            }
            1 => {
                let mut l = [0u8; 288];
                l[..144].fill(8);
                l[144..256].fill(9);
                l[256..280].fill(7);
                l[280..].fill(8);
                let lit = Huffman::new(&l)?;
                let dist = Huffman::new(&[5u8; 30])?;
                bloque(b, salida, max, &lit, &dist)?;
            }
            2 => {
                let hlit = b.bits(5)? as usize + 257;
                let hdist = b.bits(5)? as usize + 1;
                let hclen = b.bits(4)? as usize + 4;
                if hlit > 286 || hdist > 30 {
                    return Err(Error::Comprimido);
                }
                let mut l = [0u8; 19];
                for &k in &ORDEN[..hclen] {
                    l[k] = b.bits(3)? as u8;
                }
                let de_largos = Huffman::new(&l)?;
                let mut largos = [0u8; 286 + 30];
                let mut i = 0;
                while i < hlit + hdist {
                    let s = de_largos.simbolo(b)?;
                    let (valor, veces) = match s {
                        0..=15 => (s as u8, 1),
                        16 => {
                            if i == 0 {
                                return Err(Error::Comprimido);
                            }
                            (largos[i - 1], 3 + b.bits(2)? as usize)
                        }
                        17 => (0, 3 + b.bits(3)? as usize),
                        18 => (0, 11 + b.bits(7)? as usize),
                        _ => return Err(Error::Comprimido),
                    };
                    if i + veces > hlit + hdist {
                        return Err(Error::Comprimido);
                    }
                    largos[i..i + veces].fill(valor);
                    i += veces;
                }
                if largos[256] == 0 {
                    return Err(Error::Comprimido);
                }
                let lit = Huffman::new(&largos[..hlit])?;
                let dist = Huffman::new(&largos[hlit..hlit + hdist])?;
                bloque(b, salida, max, &lit, &dist)?;
            }
            _ => return Err(Error::Comprimido),
        }
        if ultimo {
            return Ok(());
        }
    }
}

/// Un bloque con códigos de Huffman, hasta el símbolo 256.
fn bloque(b: &mut Bits, salida: &mut Vec<u8>, max: usize, lit: &Huffman, dist: &Huffman) -> Result<(), Error> {
    loop {
        let s = lit.simbolo(b)? as usize;
        if s < 256 {
            if salida.len() >= max {
                return Err(Error::Comprimido);
            }
            salida.push(s as u8);
            continue;
        }
        if s == 256 {
            return Ok(());
        }
        let k = s - 257;
        if k >= 29 {
            return Err(Error::Comprimido);
        }
        let largo = BASE_LARGO[k] as usize + b.bits(EXTRA_LARGO[k] as u32)? as usize;
        let d = dist.simbolo(b)? as usize;
        if d >= 30 {
            return Err(Error::Comprimido);
        }
        let distancia = BASE_DIST[d] as usize + b.bits(EXTRA_DIST[d] as u32)? as usize;
        if distancia > salida.len() || salida.len() + largo > max {
            return Err(Error::Comprimido);
        }
        let desde = salida.len() - distancia;
        for i in 0..largo {
            let c = salida[desde + i];
            salida.push(c);
        }
    }
}

/// Descomprime un flujo zlib (RFC 1950) de hasta `max` bytes, con su Adler-32 comprobado.
pub fn zlib(datos: &[u8], max: usize) -> Result<Vec<u8>, Error> {
    if datos.len() < 6 {
        return Err(Error::Corto);
    }
    let (cmf, flg) = (datos[0], datos[1]);
    // Método 8 (deflate), ventana de 32 KB como mucho, sin diccionario, y la comprobación de la cabecera.
    if cmf & 0x0f != 8 || cmf >> 4 > 7 || flg & 0x20 != 0 || (cmf as u32 * 256 + flg as u32) % 31 != 0 {
        return Err(Error::Comprimido);
    }
    let mut salida = Vec::new();
    salida.try_reserve_exact(max).map_err(|_| Error::SinMemoria)?;
    let mut b = Bits::new(&datos[2..]);
    inflar(&mut b, &mut salida, max)?;
    b.al_byte();
    // Lo que el lector cogió de más vuelve atrás: el Adler-32 va en los 4 bytes siguientes.
    let usado = 2 + b.pos - (b.hay / 8) as usize;
    let adler = datos.get(usado..usado + 4).ok_or(Error::Corto)?;
    if be32(adler) != adler32(&salida) {
        return Err(Error::Comprimido);
    }
    Ok(salida)
}

// ---------------------------------------------------------------------------------------------------------------------
// PNG.

/// La cabecera (IHDR).
#[derive(Clone, Copy, Debug)]
struct Cabecera {
    ancho: u32,
    alto: u32,
    bits: u8,
    tipo: u8,
    entrelazada: bool,
}

impl Cabecera {
    /// Canales por píxel.
    fn canales(&self) -> usize {
        match self.tipo {
            0 | 3 => 1,
            4 => 2,
            2 => 3,
            _ => 4,
        }
    }

    /// Bytes de una línea de `ancho` píxeles, sin el byte del filtro.
    fn linea(&self, ancho: usize) -> Option<usize> {
        ancho.checked_mul(self.canales() * self.bits as usize).map(|b| b.div_ceil(8))
    }

    /// Bytes por píxel para los filtros (como poco 1).
    fn bpp(&self) -> usize {
        (self.canales() * self.bits as usize).div_ceil(8)
    }
}

/// Las siete pasadas de Adam7: columna y fila de inicio, y paso en cada una.
const ADAM7: [(usize, usize, usize, usize); 7] = [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)];

/// Los subimágenes que hay en los datos: (columna, fila, paso x, paso y, ancho, alto); sin entrelazar, una.
fn pasadas(c: &Cabecera) -> Vec<(usize, usize, usize, usize, usize, usize)> {
    let (w, h) = (c.ancho as usize, c.alto as usize);
    if !c.entrelazada {
        return alloc::vec![(0, 0, 1, 1, w, h)];
    }
    ADAM7
        .iter()
        .map(|&(x0, y0, dx, dy)| (x0, y0, dx, dy, (w + dx - 1 - x0) / dx, (h + dy - 1 - y0) / dy))
        .filter(|p| p.4 > 0 && p.5 > 0)
        .collect()
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let (pa, pb, pc) = ((p - a as i16).abs(), (p - b as i16).abs(), (p - c as i16).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Deshace los filtros de una subimagen de `filas` líneas de `largo` bytes (cada una con su byte de filtro delante),
/// en su sitio.
fn sin_filtros(datos: &mut [u8], largo: usize, filas: usize, bpp: usize) -> Result<(), Error> {
    for f in 0..filas {
        let (antes, ahora) = datos.split_at_mut(f * (largo + 1));
        let arriba = if f > 0 { Some(&antes[antes.len() - largo..]) } else { None };
        let filtro = ahora[0];
        let fila = &mut ahora[1..=largo];
        let alto = |i: usize| arriba.map_or(0, |a| a[i]);
        match filtro {
            0 => {}
            1 => {
                for i in bpp..largo {
                    fila[i] = fila[i].wrapping_add(fila[i - bpp]);
                }
            }
            2 => {
                for i in 0..largo {
                    fila[i] = fila[i].wrapping_add(alto(i));
                }
            }
            3 => {
                for i in 0..largo {
                    let a = if i >= bpp { fila[i - bpp] } else { 0 };
                    fila[i] = fila[i].wrapping_add(((a as u16 + alto(i) as u16) / 2) as u8);
                }
            }
            4 => {
                for i in 0..largo {
                    let (a, c) = if i >= bpp { (fila[i - bpp], alto(i - bpp)) } else { (0, 0) };
                    fila[i] = fila[i].wrapping_add(paeth(a, alto(i), c));
                }
            }
            _ => return Err(Error::MalFormado),
        }
    }
    Ok(())
}

/// Lo que se sabe de los colores: la paleta (ARGB) y el color transparente de gris o color (tRNS), en la escala de
/// los bits de la imagen.
struct Colores {
    paleta: Vec<u32>,
    transparente: Option<[u16; 3]>,
}

/// El valor de la muestra `k` (de `bits` bits) de una línea.
fn muestra(fila: &[u8], k: usize, bits: u8) -> u16 {
    match bits {
        8 => fila[k] as u16,
        16 => u16::from_be_bytes([fila[2 * k], fila[2 * k + 1]]),
        _ => {
            let b = bits as usize;
            let bit = k * b;
            ((fila[bit / 8] >> (8 - b - bit % 8)) & ((1 << b) - 1) as u8) as u16
        }
    }
}

/// Pasa una muestra de `bits` bits a 8.
fn a8(v: u16, bits: u8) -> u32 {
    match bits {
        1 => v as u32 * 255,
        2 => v as u32 * 85,
        4 => v as u32 * 17,
        8 => v as u32,
        _ => (v >> 8) as u32,
    }
}

/// El píxel `x` de una línea ya sin filtros, en ARGB.
fn pixel(c: &Cabecera, col: &Colores, fila: &[u8], x: usize) -> u32 {
    let n = c.canales();
    let m = |i: usize| muestra(fila, x * n + i, c.bits);
    match c.tipo {
        0 => {
            let g = m(0);
            let a = if col.transparente.is_some_and(|t| t[0] == g) { 0 } else { 255 };
            let g = a8(g, c.bits);
            a << 24 | g << 16 | g << 8 | g
        }
        2 => {
            let (r, g, b) = (m(0), m(1), m(2));
            let a = if col.transparente == Some([r, g, b]) { 0 } else { 255 };
            a << 24 | a8(r, c.bits) << 16 | a8(g, c.bits) << 8 | a8(b, c.bits)
        }
        // Un índice fuera de la paleta: negro opaco (la especificación lo deja a quien lee; no es un fallo).
        3 => col.paleta.get(m(0) as usize).copied().unwrap_or(0xff00_0000),
        4 => {
            let g = a8(m(0), c.bits);
            a8(m(1), c.bits) << 24 | g << 16 | g << 8 | g
        }
        _ => a8(m(3), c.bits) << 24 | a8(m(0), c.bits) << 16 | a8(m(1), c.bits) << 8 | a8(m(2), c.bits),
    }
}

/// Lee un PNG entero. `max_px`: los píxeles que se dejan como mucho (ancho × alto); más es `Grande`, sin pedir memoria.
pub fn leer(bytes: &[u8], max_px: u64) -> Result<Imagen, Error> {
    leer_todo(bytes, max_px, false).map(|t| t.0)
}

/// Una imagen de paleta (tipo de color 3) con sus índices, para animar la paleta ("color cycling", rama `fondos`).
pub struct Indexada {
    pub ancho: usize,
    pub alto: usize,
    /// Un índice por píxel (de 1, 2, 4 u 8 bits en el fichero; aquí, un byte cada uno).
    pub indices: Vec<u8>,
    /// La paleta (ARGB), tal como viene; un índice fuera de ella se ve negro.
    pub paleta: Vec<u32>,
    /// Los trozos de texto `tEXt` (clave y texto, en Latin-1 pasado a UTF-8), como mucho `MAX_TEXTOS`.
    pub textos: Vec<(alloc::string::String, alloc::string::String)>,
}

/// Lo más que se guarda de los trozos de texto (cuántos y lo que mide cada uno).
pub const MAX_TEXTOS: usize = 16;
pub const MAX_TEXTO: usize = 4096;

/// Lee una imagen de paleta con sus índices. `Err(NoEsPaleta)` si es de otro tipo de color.
pub fn leer_indexada(bytes: &[u8], max_px: u64) -> Result<Indexada, Error> {
    let (img, ind, textos) = leer_todo(bytes, max_px, true)?;
    let (indices, paleta) = ind.ok_or(Error::NoEsPaleta)?;
    Ok(Indexada { ancho: img.ancho, alto: img.alto, indices, paleta, textos })
}

type Todo = (Imagen, Option<(Vec<u8>, Vec<u32>)>, Vec<(alloc::string::String, alloc::string::String)>);

fn leer_todo(bytes: &[u8], max_px: u64, indexada: bool) -> Result<Todo, Error> {
    let mut textos = Vec::new();
    if bytes.len() < 8 || bytes[..8] != FIRMA {
        return Err(Error::NoEsPng);
    }
    let mut pos = 8;
    let mut cab: Option<Cabecera> = None;
    let mut colores = Colores { paleta: Vec::new(), transparente: None };
    let mut idat: Vec<u8> = Vec::new();
    let mut fin = false;
    while !fin {
        let cabeza = bytes.get(pos..pos + 8).ok_or(Error::Corto)?;
        let largo = be32(cabeza) as usize;
        if largo > 0x7fff_ffff {
            return Err(Error::MalFormado);
        }
        let tipo: [u8; 4] = cabeza[4..8].try_into().unwrap();
        let datos = bytes.get(pos + 8..pos + 8 + largo).ok_or(Error::Corto)?;
        let crc = bytes.get(pos + 8 + largo..pos + 12 + largo).ok_or(Error::Corto)?;
        if crc32(&bytes[pos + 4..pos + 8 + largo]) != be32(crc) {
            return Err(Error::Crc);
        }
        pos += 12 + largo;
        match &tipo {
            b"IHDR" => {
                if cab.is_some() || largo != 13 {
                    return Err(Error::MalFormado);
                }
                let c = Cabecera { ancho: be32(datos), alto: be32(&datos[4..]), bits: datos[8], tipo: datos[9], entrelazada: datos[12] == 1 };
                let bits_valen = match c.tipo {
                    0 => matches!(c.bits, 1 | 2 | 4 | 8 | 16),
                    3 => matches!(c.bits, 1 | 2 | 4 | 8),
                    2 | 4 | 6 => matches!(c.bits, 8 | 16),
                    _ => false,
                };
                if c.ancho == 0 || c.alto == 0 || c.ancho > 0x7fff_ffff || c.alto > 0x7fff_ffff || !bits_valen || datos[10] != 0 || datos[11] != 0 || datos[12] > 1 {
                    return Err(Error::MalFormado);
                }
                if c.ancho as u64 * c.alto as u64 > max_px {
                    return Err(Error::Grande(c.ancho, c.alto));
                }
                cab = Some(c);
            }
            _ if cab.is_none() => return Err(Error::MalFormado),
            b"PLTE" => {
                if largo % 3 != 0 || largo == 0 || largo > 3 * 256 {
                    return Err(Error::MalFormado);
                }
                colores.paleta = datos.chunks(3).map(|p| 0xff00_0000 | (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32).collect();
            }
            b"tRNS" => match cab.map(|c| c.tipo) {
                Some(3) => {
                    for (i, &a) in datos.iter().enumerate().take(colores.paleta.len()) {
                        colores.paleta[i] = (a as u32) << 24 | (colores.paleta[i] & 0x00ff_ffff);
                    }
                }
                Some(0) if largo == 2 => colores.transparente = Some([u16::from_be_bytes([datos[0], datos[1]]), 0, 0]),
                Some(2) if largo == 6 => {
                    let v = |i: usize| u16::from_be_bytes([datos[i], datos[i + 1]]);
                    colores.transparente = Some([v(0), v(2), v(4)]);
                }
                _ => {}
            },
            b"IDAT" => {
                idat.try_reserve(largo).map_err(|_| Error::SinMemoria)?;
                idat.extend_from_slice(datos);
            }
            b"tEXt" if indexada && textos.len() < MAX_TEXTOS && largo <= MAX_TEXTO => {
                if let Some(cero) = datos.iter().position(|&b| b == 0) {
                    let latin = |b: &[u8]| b.iter().map(|&c| c as char).collect::<alloc::string::String>();
                    textos.push((latin(&datos[..cero]), latin(&datos[cero + 1..])));
                }
            }
            b"IEND" => fin = true,
            // Un trozo crítico que no se conoce (la primera letra en mayúscula): no se puede leer bien.
            _ if tipo[0] & 0x20 == 0 => return Err(Error::MalFormado),
            _ => {}
        }
    }
    let c = cab.ok_or(Error::MalFormado)?;
    if c.tipo == 3 && colores.paleta.is_empty() {
        return Err(Error::MalFormado);
    }
    // Lo que miden los datos sin comprimir: cada línea de cada pasada con su byte de filtro.
    let partes = pasadas(&c);
    let mut total = 0usize;
    for p in &partes {
        let l = c.linea(p.4).ok_or(Error::Grande(c.ancho, c.alto))?;
        total = (l + 1).checked_mul(p.5).and_then(|t| t.checked_add(total)).ok_or(Error::Grande(c.ancho, c.alto))?;
    }
    let mut datos = zlib(&idat, total)?;
    drop(idat);
    if datos.len() != total {
        return Err(Error::Corto);
    }
    let (w, h) = (c.ancho as usize, c.alto as usize);
    if indexada && c.tipo != 3 {
        return Err(Error::NoEsPaleta);
    }
    let mut px: Vec<u32> = ceros(w * h)?;
    let mut indices: Vec<u8> = if indexada { ceros(w * h)? } else { Vec::new() };
    let mut desde = 0;
    for &(x0, y0, dx, dy, pw, ph) in &partes {
        let largo = c.linea(pw).unwrap();
        let trozo = &mut datos[desde..desde + (largo + 1) * ph];
        sin_filtros(trozo, largo, ph, c.bpp())?;
        for f in 0..ph {
            let fila = &trozo[f * (largo + 1) + 1..(f + 1) * (largo + 1)];
            let y = y0 + f * dy;
            for k in 0..pw {
                px[y * w + x0 + k * dx] = pixel(&c, &colores, fila, k);
                if indexada {
                    indices[y * w + x0 + k * dx] = muestra(fila, k, c.bits) as u8;
                }
            }
        }
        desde += (largo + 1) * ph;
    }
    let ind = indexada.then(|| (indices, colores.paleta));
    Ok((Imagen { ancho: w, alto: h, px }, ind, textos))
}

/// Solo la cabecera: (ancho, alto). Para decir lo que mide sin leerla entera.
pub fn medidas(bytes: &[u8]) -> Result<(u32, u32), Error> {
    if bytes.len() < 33 || bytes[..8] != FIRMA {
        return Err(Error::NoEsPng);
    }
    if &bytes[12..16] != b"IHDR" {
        return Err(Error::MalFormado);
    }
    Ok((be32(&bytes[16..]), be32(&bytes[20..])))
}

#[cfg(test)]
mod pruebas {
    use super::*;

    /// zlib de Python (`zlib.compress(b"hola " * 40 + b"ñandú", 9)`): Huffman fijo y referencias hacia atrás.
    const HOLA: &[u8] = include_bytes!("pruebas/hola.zlib");

    #[test]
    fn zlib_de_python() {
        let mut esperado = Vec::new();
        for _ in 0..40 {
            esperado.extend_from_slice(b"hola ");
        }
        esperado.extend_from_slice("ñandú".as_bytes());
        assert_eq!(zlib(HOLA, 1000).unwrap(), esperado);
        // Con menos sitio del que hace falta, un error (no se escribe de más).
        assert_eq!(zlib(HOLA, 100), Err(Error::Comprimido));
        // Cortado, o con el Adler-32 tocado.
        assert!(zlib(&HOLA[..HOLA.len() - 6], 1000).is_err());
        let mut malo = HOLA.to_vec();
        let n = malo.len();
        malo[n - 1] ^= 1;
        assert_eq!(zlib(&malo, 1000), Err(Error::Comprimido));
    }

    #[test]
    fn deflate_guardado_y_roto() {
        // Un bloque guardado (sin comprimir): cabecera zlib, BFINAL=1 y tipo 0, largo y su complemento, datos, Adler.
        let datos = b"DashOS";
        let mut z = alloc::vec![0x78, 0x01, 0x01, 6, 0, !6u8, 0xff];
        z.extend_from_slice(datos);
        z.extend_from_slice(&adler32(datos).to_be_bytes());
        assert_eq!(zlib(&z, 6).unwrap(), datos);
        // El tipo de bloque 3 no existe.
        let mut z3 = alloc::vec![0x78, 0x01, 0x07];
        z3.extend_from_slice(&[0; 8]);
        assert_eq!(zlib(&z3, 100), Err(Error::Comprimido));
        // Una distancia que va antes del principio: Huffman fijo, longitud 3 (257) a distancia 1 sin nada antes.
        // 257 son 7 bits 0000001; la distancia 0, 5 bits 00000. Bits de menos a más: 1 (final), 10 (tipo 1)...
        let mut b = 0u32;
        let mut n = 0;
        let poner = |v: u32, k: u32, b: &mut u32, n: &mut u32| {
            *b |= v << *n;
            *n += k;
        };
        poner(1, 1, &mut b, &mut n);
        poner(1, 2, &mut b, &mut n);
        // El código 0000001 va del bit alto al bajo.
        for bit in [0, 0, 0, 0, 0, 0, 1] {
            poner(bit, 1, &mut b, &mut n);
        }
        poner(0, 5, &mut b, &mut n);
        let mut z = alloc::vec![0x78, 0x01];
        z.extend_from_slice(&b.to_le_bytes());
        z.extend_from_slice(&[0; 4]);
        assert_eq!(zlib(&z, 100), Err(Error::Comprimido));
        // Basura: no hay pánico.
        let mut x = 0x1234_5678u32;
        for _ in 0..2000 {
            let mut v = alloc::vec![0x78, 0x9c];
            for _ in 0..64 {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                v.push(x as u8);
            }
            let _ = zlib(&v, 4096);
        }
    }

    /// El color que pone `tools/fondos_prueba.py` en el píxel (x, y) de sus imágenes de prueba de 37 × 23.
    fn formula(x: usize, y: usize) -> (u32, u32, u32, u32) {
        ((x * 7) as u32 & 0xff, (y * 11) as u32 & 0xff, ((x * y) as u32 * 3) & 0xff, (255 - x * 3) as u32 & 0xff)
    }

    fn comprobar(nombre: &str, bytes: &[u8], como: impl Fn(usize, usize) -> u32) {
        let img = leer(bytes, 1 << 20).unwrap_or_else(|e| panic!("{nombre}: {e:?}"));
        assert_eq!((img.ancho, img.alto), (37, 23), "{nombre}");
        for y in 0..23 {
            for x in 0..37 {
                assert_eq!(img.px[y * 37 + x], como(x, y), "{nombre} en ({x}, {y})");
            }
        }
    }

    /// Los PNG de prueba de `tools/fondos_prueba.py --pruebas`: uno por tipo de color y profundidad, con los cinco
    /// filtros (uno por línea, por turnos), comprimidos por zlib de Python (Huffman dinámico).
    #[test]
    fn tipos_de_png() {
        let argb = |a: u32, r: u32, g: u32, b: u32| a << 24 | r << 16 | g << 8 | b;
        comprobar("color", include_bytes!("pruebas/color.png"), |x, y| {
            let (r, g, b, _) = formula(x, y);
            argb(255, r, g, b)
        });
        comprobar("color con alfa", include_bytes!("pruebas/alfa.png"), |x, y| {
            let (r, g, b, a) = formula(x, y);
            argb(a, r, g, b)
        });
        comprobar("gris", include_bytes!("pruebas/gris.png"), |x, y| {
            let g = formula(x, y).0;
            argb(255, g, g, g)
        });
        comprobar("gris con alfa", include_bytes!("pruebas/gris_alfa.png"), |x, y| {
            let (g, _, _, a) = formula(x, y);
            argb(a, g, g, g)
        });
        // 16 bits: el byte alto es el de la fórmula; el bajo, otro.
        comprobar("color de 16 bits", include_bytes!("pruebas/color16.png"), |x, y| {
            let (r, g, b, _) = formula(x, y);
            argb(255, r, g, b)
        });
        // Paleta de 4 bits (16 colores) con transparencia en el primero.
        comprobar("paleta", include_bytes!("pruebas/paleta.png"), |x, y| {
            let i = ((x + y) % 16) as u32;
            argb(if i == 0 { 0 } else { 255 }, i * 16, 255 - i * 16, i * 8)
        });
        // Gris de 1 bit: un tablero.
        comprobar("gris de 1 bit", include_bytes!("pruebas/bits1.png"), |x, y| if (x + y) % 2 == 1 { 0xffff_ffff } else { 0xff00_0000 });
        // Entrelazada (Adam7), color.
        comprobar("entrelazada", include_bytes!("pruebas/adam7.png"), |x, y| {
            let (r, g, b, _) = formula(x, y);
            argb(255, r, g, b)
        });
    }

    #[test]
    fn rotas_y_enormes() {
        let bueno = include_bytes!("pruebas/color.png");
        assert!(leer(bueno, 1 << 20).is_ok());
        // Un byte cambiado en medio: el CRC lo ve.
        let mut malo = bueno.to_vec();
        malo[60] ^= 0x40;
        assert_eq!(leer(&malo, 1 << 20).err(), Some(Error::Crc));
        // Cortado.
        for n in [0, 7, 8, 20, 40, bueno.len() / 2, bueno.len() - 1] {
            assert!(leer(&bueno[..n], 1 << 20).is_err(), "cortado en {n}");
        }
        // Más píxeles de los que se dejan: error antes de leer los datos.
        assert_eq!(leer(bueno, 100).err(), Some(Error::Grande(37, 23)));
        let enorme = include_bytes!("pruebas/enorme.png");
        assert_eq!(medidas(enorme), Ok((40000, 40000)));
        assert_eq!(leer(enorme, 16 << 20).err(), Some(Error::Grande(40000, 40000)));
        // Si se dejara, los datos no dan para tanto: corto, sin pedir 6 GB.
        assert!(leer(enorme, u64::MAX).is_err());
        assert_eq!(leer(b"GIF89a.....", 100).err(), Some(Error::NoEsPng));
        // Basura con la firma delante: errores, nunca un pánico.
        let mut x = 0x9e37_79b9u32;
        for _ in 0..500 {
            let mut v = bueno.to_vec();
            for _ in 0..4 {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                let i = 8 + (x as usize % (v.len() - 8));
                v[i] = (x >> 8) as u8;
            }
            let _ = leer(&v, 1 << 20);
        }
    }

    #[test]
    fn crc_de_referencia() {
        // El CRC-32 de "123456789" es 0xCBF43926 (el valor de comprobación de ISO 3309).
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
    }
}
