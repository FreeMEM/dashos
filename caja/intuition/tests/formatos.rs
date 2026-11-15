//! Los formatos de píxel de los mapas de color verdadero (`chunky.rs`): cada mapa guarda sus píxeles en el suyo, en la
//! memoria del 68k; las funciones de cybergraphics y Picasso96 convierten desde y hacia el del programa. Ida y vuelta
//! por cada par de formatos sin perder lo que el formato guarda (contra un modelo de bits por canal escrito aquí, no
//! con las funciones de la biblioteca), la disposición de los bytes de cada uno, copias entre mapas de formatos
//! distintos y lo hostil: formatos que no existen, anchos impares en 24 bits y el relleno de las filas.

mod comun;

use caja_exec::Memoria;
use caja_intuition::cgx::constantes::*;
use comun::*;

fn x(c: &mut Caja, nombre: &str, d: &[u32], a: &[u32]) -> u32 {
    let n = c.1.numero_cgx(nombre).expect("función de cybergraphics");
    let mut r = regs(d, a);
    c.1.llamar(&mut c.0, n, &mut r);
    r.d[0]
}

fn p(c: &mut Caja, nombre: &str, d: &[u32], a: &[u32]) -> u32 {
    let n = c.1.numero_p96(nombre).expect("función de Picasso96API");
    let mut r = regs(d, a);
    c.1.llamar(&mut c.0, n, &mut r);
    r.d[0]
}

/// Lo que guarda cada `PIXFMT_*` de color verdadero: bits de rojo, verde y azul, y si lleva alfa.
fn bits_pixfmt(f: u32) -> (u32, u32, u32, bool) {
    match f {
        1..=4 => (5, 5, 5, false),
        5..=8 => (5, 6, 5, false),
        9 | 10 => (8, 8, 8, false),
        11..=13 => (8, 8, 8, true),
        _ => unreachable!(),
    }
}

/// Lo mismo de cada `RGBFB_*`.
fn bits_rgbfb(f: u32) -> (u32, u32, u32, bool) {
    match f {
        2 | 3 => (8, 8, 8, false),
        4 | 10 | 12 => (5, 6, 5, false),
        5 | 11 | 13 => (5, 5, 5, false),
        6..=9 => (8, 8, 8, true),
        _ => unreachable!(),
    }
}

/// Un canal de 8 bits que pasa por `n` bits (y se vuelve a ensanchar repitiendo los de arriba).
fn canal(v: u32, n: u32) -> u32 {
    if n == 8 {
        return v;
    }
    let q = v >> (8 - n);
    (q << (8 - n)) | (q >> (2 * n - 8))
}

fn modelo(c: u32, b: (u32, u32, u32, bool)) -> u32 {
    let (a, r, g, bl) = (c >> 24, (c >> 16) & 255, (c >> 8) & 255, c & 255);
    let a = if b.3 { a } else { 0 };
    (a << 24) | (canal(r, b.0) << 16) | (canal(g, b.1) << 8) | canal(bl, b.2)
}

fn junta(x: (u32, u32, u32, bool), y: (u32, u32, u32, bool)) -> (u32, u32, u32, bool) {
    (x.0.min(y.0), x.1.min(y.1), x.2.min(y.2), x.3 && y.3)
}

/// Colores de prueba (con alfa): esquinas, grises y al azar.
fn colores() -> Vec<u32> {
    let mut v = vec![0x0000_0000, 0xFFFF_FFFF, 0x80FF_0000, 0x4000_FF00, 0x2000_00FF, 0x7F12_3456, 0x01FE_DCBA];
    let mut s: u32 = 0x9E37_79B9;
    for _ in 0..25 {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        v.push(s);
    }
    v
}

/// Un mapa de `ancho` x 3 en el formato `fmt` y su `RastPort`.
fn mapa(c: &mut Caja, ancho: u32, fmt: u32) -> (u32, u32) {
    let bm = g(c, "AllocBitMap", &[ancho, 3, 32, BMF_SPECIALFMT | (fmt << 24)], &[0]);
    assert_ne!(bm, 0, "formato {}", fmt);
    assert_eq!(x(c, "GetCyberMapAttr", &[0x8000_0004], &[bm]), fmt);
    (bm, rastport(c, bm))
}

/// Ida y vuelta por WritePixelArray/ReadPixelArray, en los formatos de cybergraphics, en mapas de los 13 formatos.
#[test]
fn ida_y_vuelta_cybergraphics() {
    let mut c = caja();
    let col = colores();
    let n = col.len() as u32;
    for fmt in 1..=13u32 {
        let (_, rp) = mapa(&mut c, n, fmt);
        for (rect, bpp, bits) in [
            (RECTFMT_ARGB, 4, (8, 8, 8, true)),
            (RECTFMT_RGBA, 4, (8, 8, 8, true)),
            (RECTFMT_RGB, 3, (8, 8, 8, false)),
        ] {
            let mut b = Vec::new();
            for &k in &col {
                let [a, r, g_, bl] = k.to_be_bytes();
                match rect {
                    RECTFMT_ARGB => b.extend_from_slice(&[a, r, g_, bl]),
                    RECTFMT_RGBA => b.extend_from_slice(&[r, g_, bl, a]),
                    _ => b.extend_from_slice(&[r, g_, bl]),
                }
            }
            let src = reservar(&mut c, b.len() as u32);
            c.0.mem.leer_mut(src, b.len() as u32).unwrap().copy_from_slice(&b);
            assert_eq!(x(&mut c, "WritePixelArray", &[0, 0, n * bpp, 0, 1, n, 1, rect], &[src, rp]), n);
            let dst = reservar(&mut c, n * bpp);
            assert_eq!(x(&mut c, "ReadPixelArray", &[0, 0, n * bpp, 0, 1, n, 1, rect], &[dst, rp]), n);
            let leido = c.0.mem.leer(dst, n * bpp).unwrap().to_vec();
            for (i, &k) in col.iter().enumerate() {
                let q = &leido[i * bpp as usize..][..bpp as usize];
                let v = match rect {
                    RECTFMT_ARGB => u32::from_be_bytes([q[0], q[1], q[2], q[3]]),
                    RECTFMT_RGBA => u32::from_be_bytes([q[3], q[0], q[1], q[2]]),
                    _ => u32::from_be_bytes([0, q[0], q[1], q[2]]),
                };
                let esperado = modelo(k, junta(bits_pixfmt(fmt), bits));
                assert_eq!(v, esperado, "mapa {} programa {} color {:08x}", fmt, rect, k);
            }
        }
    }
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}

/// Ida y vuelta por p96WritePixelArray/p96ReadPixelArray (RenderInfo) en cada `RGBFB_*` de color verdadero, en mapas
/// de los 13 formatos.
#[test]
fn ida_y_vuelta_picasso96() {
    let mut c = caja();
    let col = colores();
    let n = col.len() as u32;
    let ri = reservar(&mut c, 12);
    for fmt in 1..=13u32 {
        let (_, rp) = mapa(&mut c, n, fmt);
        for rgbf in 2..=13u32 {
            let bpp = match rgbf {
                2 | 3 => 3,
                6..=9 => 4,
                _ => 2,
            };
            // Lo que se escribe: cada color ya en ese formato (pasado por su modelo), en los bytes de ese formato.
            let mut b = Vec::new();
            for &k in &col {
                let m = modelo(k, bits_rgbfb(rgbf));
                let [a, r, g_, bl] = m.to_be_bytes();
                let (r5, g5, g6, b5) = ((r >> 3) as u16, (g_ >> 3) as u16, (g_ >> 2) as u16, (bl >> 3) as u16);
                match rgbf {
                    2 => b.extend_from_slice(&[r, g_, bl]),
                    3 => b.extend_from_slice(&[bl, g_, r]),
                    6 => b.extend_from_slice(&[a, r, g_, bl]),
                    7 => b.extend_from_slice(&[a, bl, g_, r]),
                    8 => b.extend_from_slice(&[r, g_, bl, a]),
                    9 => b.extend_from_slice(&[bl, g_, r, a]),
                    10 => b.extend_from_slice(&(r5 << 11 | g6 << 5 | b5).to_be_bytes()),
                    4 => b.extend_from_slice(&(r5 << 11 | g6 << 5 | b5).to_le_bytes()),
                    11 => b.extend_from_slice(&(r5 << 10 | g5 << 5 | b5).to_be_bytes()),
                    5 => b.extend_from_slice(&(r5 << 10 | g5 << 5 | b5).to_le_bytes()),
                    12 => b.extend_from_slice(&(b5 << 11 | g6 << 5 | r5).to_le_bytes()),
                    13 => b.extend_from_slice(&(b5 << 10 | g5 << 5 | r5).to_le_bytes()),
                    _ => unreachable!(),
                }
            }
            let src = reservar(&mut c, b.len() as u32);
            c.0.mem.leer_mut(src, b.len() as u32).unwrap().copy_from_slice(&b);
            e32(&mut c, ri, src);
            e16(&mut c, ri + 4, (n * bpp) as u16);
            e32(&mut c, ri + 8, rgbf);
            p(&mut c, "p96WritePixelArray", &[0, 0, 0, 2, n, 1], &[ri, rp]);
            let dst = reservar(&mut c, n * bpp);
            e32(&mut c, ri, dst);
            p(&mut c, "p96ReadPixelArray", &[0, 0, 0, 2, n, 1], &[ri, rp]);
            let leido = c.0.mem.leer(dst, n * bpp).unwrap().to_vec();
            let original = c.0.mem.leer(src, n * bpp).unwrap().to_vec();
            if bits_pixfmt(fmt).0 >= bits_rgbfb(rgbf).0 && bits_pixfmt(fmt).1 >= bits_rgbfb(rgbf).1
                && (bits_pixfmt(fmt).3 || !bits_rgbfb(rgbf).3)
            {
                // El mapa guarda todo lo que lleva el formato del programa: vuelve igual, byte a byte.
                assert_eq!(leido, original, "mapa {} programa RGBFB {}", fmt, rgbf);
            } else {
                // Si no, lo que guarda el más pequeño: se compara por color, leído por cybergraphics en ARGB.
                let argb = reservar(&mut c, 4 * n);
                x(&mut c, "ReadPixelArray", &[0, 0, 4 * n, 0, 2, n, 1, RECTFMT_ARGB], &[argb, rp]);
                for (i, &k) in col.iter().enumerate() {
                    let v = l32(&c, argb + 4 * i as u32);
                    let esperado = modelo(modelo(k, bits_rgbfb(rgbf)), bits_pixfmt(fmt));
                    assert_eq!(v, esperado, "mapa {} programa RGBFB {} color {:08x}", fmt, rgbf, k);
                }
            }
        }
    }
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}

/// Cómo quedan los bytes de un rojo (con alfa 0x80) en la memoria de cada formato, leídos por `LockBitMap`: lo que dice
/// `cybergraphx/cybergraphics.h` (los PC, con los dos bytes al revés).
#[test]
fn bytes_de_cada_formato() {
    let mut c = caja();
    let esperado: [(u32, &[u8]); 13] = [
        (1, &[0x7C, 0x00]),
        (2, &[0x00, 0x1F]),
        (3, &[0x00, 0x7C]),
        (4, &[0x1F, 0x00]),
        (5, &[0xF8, 0x00]),
        (6, &[0x00, 0x1F]),
        (7, &[0x00, 0xF8]),
        (8, &[0x1F, 0x00]),
        (9, &[0xFF, 0x00, 0x00]),
        (10, &[0x00, 0x00, 0xFF]),
        (11, &[0x80, 0xFF, 0x00, 0x00]),
        (12, &[0x00, 0x00, 0xFF, 0x80]),
        (13, &[0xFF, 0x00, 0x00, 0x80]),
    ];
    let campos = reservar(&mut c, 8);
    let t = reservar(&mut c, 24);
    e32(&mut c, t, LBMI_BASEADDRESS);
    e32(&mut c, t + 4, campos);
    e32(&mut c, t + 8, LBMI_BYTESPERPIX);
    e32(&mut c, t + 12, campos + 4);
    for (fmt, bytes) in esperado {
        let (bm, rp) = mapa(&mut c, 3, fmt);
        let s = reservar(&mut c, 4);
        e32(&mut c, s, 0x80FF_0000);
        x(&mut c, "WritePixelArray", &[0, 0, 4, 1, 0, 1, 1, RECTFMT_ARGB], &[s, rp]);
        let h = x(&mut c, "LockBitMapTagList", &[], &[bm, t]);
        assert_ne!(h, 0);
        let (base, bpp) = (l32(&c, campos), l32(&c, campos + 4));
        assert_eq!(bpp as usize, bytes.len());
        let v = c.0.mem.leer(base + bpp, bpp).unwrap().to_vec();
        assert_eq!(v, bytes, "formato {}", fmt);
        // RECTFMT_RAW: los bytes tal cual, en el formato del mapa.
        let raw = reservar(&mut c, 4);
        c.0.mem.leer_mut(raw, bpp).unwrap().copy_from_slice(bytes);
        x(&mut c, "WritePixelArray", &[0, 0, 4, 2, 0, 1, 1, RECTFMT_RAW], &[raw, rp]);
        assert_eq!(c.0.mem.leer(base + 2 * bpp, bpp).unwrap(), bytes, "RAW en el formato {}", fmt);
        x(&mut c, "UnLockBitMap", &[], &[h]);
    }
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}

/// BltBitMap entre mapas de cualquier par de formatos: queda lo que guarda el más pequeño de los dos.
#[test]
fn copias_entre_formatos() {
    let mut c = caja();
    let col = colores();
    let n = col.len() as u32;
    let src = reservar(&mut c, 4 * n);
    for (i, &k) in col.iter().enumerate() {
        e32(&mut c, src + 4 * i as u32, k);
    }
    for fo in 1..=13u32 {
        let (bo, rpo) = mapa(&mut c, n, fo);
        x(&mut c, "WritePixelArray", &[0, 0, 4 * n, 0, 0, n, 1, RECTFMT_ARGB], &[src, rpo]);
        for fd in 1..=13u32 {
            let (bd, rpd) = mapa(&mut c, n, fd);
            g(&mut c, "BltBitMap", &[0, 0, 0, 0, n, 1, 0xC0, 0xFF], &[bo, bd]);
            for (i, &k) in col.iter().enumerate() {
                let v = x(&mut c, "ReadRGBPixel", &[i as u32, 0], &[0, rpd]);
                let esperado = modelo(modelo(k, bits_pixfmt(fo)), bits_pixfmt(fd));
                assert_eq!(v, esperado, "de {} a {}: {:08x}", fo, fd, k);
            }
            g(&mut c, "FreeBitMap", &[], &[bd]);
        }
        g(&mut c, "FreeBitMap", &[], &[bo]);
    }
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}

/// Lo hostil: formatos que no existen, anchos impares en 24 bits (el relleno de las filas no se toca), filas que no
/// se salen a la siguiente.
#[test]
fn formatos_hostiles() {
    let mut c = caja();
    // BMF_SPECIALFMT con un formato que no existe: 0 y un aviso.
    for f in [14u32, 99, 255] {
        assert_eq!(g(&mut c, "AllocBitMap", &[8, 8, 32, BMF_SPECIALFMT | (f << 24)], &[0]), 0);
    }
    assert_eq!(c.1.tomar_avisos().len(), 3);
    // Picasso96 con un RGBFormat que no existe: el de serie de su profundidad (sin formato no es un fallo).
    let m = p(&mut c, "p96AllocBitMap", &[8, 8, 16, 0, 0, 0, 0, 99], &[0]);
    assert_eq!(p(&mut c, "p96GetBitMapAttr", &[7], &[m]), 10, "R5G6B5");
    // Un RenderInfo con un formato YUV o que no existe: aviso y nada escrito.
    let rp = rastport(&mut c, m);
    let ri = reservar(&mut c, 12);
    let s = reservar(&mut c, 64);
    e32(&mut c, ri, s);
    e16(&mut c, ri + 4, 16);
    for f in [0u32, 14, 15, 16, 200] {
        e32(&mut c, ri + 8, f);
        p(&mut c, "p96WritePixelArray", &[0, 0, 0, 0, 2, 2], &[ri, rp]);
    }
    assert_eq!(c.1.tomar_avisos().len(), 5);
    // 24 bits de ancho impar (7): filas de 24 bytes con 3 de relleno, que nadie toca.
    let (bm, rp) = mapa(&mut c, 7, PIXFMT_RGB24);
    assert_eq!(x(&mut c, "GetCyberMapAttr", &[0x8000_0001], &[bm]), 24);
    let base = x(&mut c, "GetCyberMapAttr", &[0x8000_0003], &[bm]);
    for f in 0..3 {
        c.0.mem.leer_mut(base + f * 24 + 21, 3).unwrap().copy_from_slice(&[0xEE, 0xEE, 0xEE]);
    }
    assert_eq!(x(&mut c, "FillPixelArray", &[0, 0, 100, 100, 0x0012_3456], &[0, rp]), 21);
    let src = reservar(&mut c, 7 * 3 * 3);
    c.0.mem.leer_mut(src, 63).unwrap().fill(0x77);
    x(&mut c, "WritePixelArray", &[0, 0, 21, 0, 0, 7, 3, RECTFMT_RGB], &[src, rp]);
    for f in 0..3 {
        assert_eq!(c.0.mem.leer(base + f * 24 + 21, 3).unwrap(), &[0xEE, 0xEE, 0xEE], "relleno de la fila {}", f);
        assert_eq!(c.0.mem.leer(base + f * 24 + 18, 3).unwrap(), &[0x77, 0x77, 0x77]);
    }
    // El último punto de una fila no cae en la siguiente.
    x(&mut c, "WriteRGBPixel", &[6, 0, 0x00AB_CDEF], &[0, rp]);
    assert_eq!(x(&mut c, "ReadRGBPixel", &[0, 1], &[0, rp]), 0x0077_7777);
    // Leer a un formato de mapa por ReadPixelArray (RECTFMT_RAW no vale para leer) y un formato fuera de rango.
    let dst = reservar(&mut c, 64);
    assert_eq!(x(&mut c, "ReadPixelArray", &[0, 0, 16, 0, 0, 2, 1, RECTFMT_RAW], &[dst, rp]), 0);
    assert_eq!(x(&mut c, "ReadPixelArray", &[0, 0, 16, 0, 0, 2, 1, 0x80], &[dst, rp]), 0);
    assert_eq!(c.1.tomar_avisos().len(), 2);
}
