//! cybergraphics.library nativa (`cgx.rs`): los atributos de mapas y modos, las funciones de píxeles en mapas nativos
//! y planares (con su recorte), el acceso directo (`LockBitMapTagList`) y lo hostil: rectángulos fuera, negativos y
//! enormes, formatos raros, punteros nulos, cerrojos sin soltar y mapas planares en funciones de color verdadero.

mod comun;

use caja_exec::Memoria;
use caja_intuition::Fallo;
use caja_intuition::cgx::constantes::*;
use caja_intuition::cgx::{INVALID_ID, MODO_CAJA};
use comun::*;

fn x(c: &mut Caja, nombre: &str, d: &[u32], a: &[u32]) -> u32 {
    let n = c.1.numero_cgx(nombre).expect("función de cybergraphics");
    let mut r = regs(d, a);
    c.1.llamar(&mut c.0, n, &mut r);
    r.d[0]
}

/// Una lista de etiquetas en memoria.
fn etiquetas(c: &mut Caja, t: &[(u32, u32)]) -> u32 {
    let d = reservar(c, 8 * t.len() as u32 + 8);
    for (i, (k, v)) in t.iter().enumerate() {
        e32(c, d + 8 * i as u32, *k);
        e32(c, d + 8 * i as u32 + 4, *v);
    }
    d
}

/// Un rectángulo de píxeles en memoria con sus bytes.
fn datos(c: &mut Caja, b: &[u8]) -> u32 {
    let d = reservar(c, b.len() as u32);
    c.0.mem.leer_mut(d, b.len() as u32).unwrap().copy_from_slice(b);
    d
}

/// El color de un punto, sin el alfa (que guardan los mapas chunky de 32 bits).
fn rgb(c: &mut Caja, rp: u32, px: i32, py: i32) -> u32 {
    x(c, "ReadRGBPixel", &[px as u32, py as u32], &[0, rp]) & 0x00FF_FFFF
}

#[test]
fn biblioteca_y_modos() {
    let mut c = caja();
    assert_ne!(c.1.cgx_base(), 0);
    assert_eq!(l16(&c, c.1.cgx_base() + 20), 41);
    assert_eq!(g(&mut c, "GetVPModeID", &[], &[0]), MODO_CAJA);
    assert_eq!(x(&mut c, "IsCyberModeID", &[MODO_CAJA], &[]), 1);
    assert_eq!(x(&mut c, "IsCyberModeID", &[0], &[]), 0);
    assert_eq!(x(&mut c, "IsCyberModeID", &[INVALID_ID], &[]), 0);
    assert_eq!(x(&mut c, "GetCyberIDAttr", &[0x8000_0001, MODO_CAJA], &[]), PIXFMT_ARGB32);
    assert_eq!(x(&mut c, "GetCyberIDAttr", &[0x8000_0002, MODO_CAJA], &[]), 640);
    assert_eq!(x(&mut c, "GetCyberIDAttr", &[0x8000_0003, MODO_CAJA], &[]), 480);
    assert_eq!(x(&mut c, "GetCyberIDAttr", &[0x8000_0004, MODO_CAJA], &[]), 32);
    assert_eq!(x(&mut c, "GetCyberIDAttr", &[0x8000_0005, MODO_CAJA], &[]), 4);
    assert_eq!(x(&mut c, "GetCyberIDAttr", &[0x8000_0099, MODO_CAJA], &[]), u32::MAX);
    assert_eq!(x(&mut c, "GetCyberIDAttr", &[0x8000_0001, 0x1234], &[]), 0);
    // BestCModeIDTagList: la profundidad pedida (24) y el más pequeño que llegue al tamaño (con un escritorio de 640 x
    // 480, solo hay ese); 8 bits de serie; más de 32, ninguno.
    let id = |t: usize, p: usize| caja_intuition::modos::id(t, p);
    let t = etiquetas(&mut c, &[(0x8005_0000, 24), (0x8005_0001, 800), (0x8005_0002, 600)]);
    assert_eq!(x(&mut c, "BestCModeIDTagList", &[], &[t]), id(0, 3));
    assert_eq!(x(&mut c, "BestCModeIDTagList", &[], &[0]), id(0, 0));
    let t = etiquetas(&mut c, &[(0x8005_0000, 64)]);
    assert_eq!(x(&mut c, "BestCModeIDTagList", &[], &[t]), INVALID_ID);
    // La lista de modos: los cinco del escritorio (8, 15, 16, 24 y 32 bits), en orden.
    let antes = c.0.bloques_de(c.2).len();
    let l = x(&mut c, "AllocCModeListTagList", &[], &[0, 0]);
    assert_ne!(l, 0);
    let mut n = l32(&c, l);
    let mut vistos = Vec::new();
    while l32(&c, n) != 0 {
        assert_eq!((l16(&c, n + CMN_WIDTH), l16(&c, n + CMN_HEIGHT)), (640, 480));
        assert_eq!(l8(&c, n + CMN_MODETEXT), b'D');
        vistos.push((l32(&c, n + CMN_DISPLAYID), l16(&c, n + CMN_DEPTH)));
        n = l32(&c, n);
    }
    assert_eq!(n, l + 4, "acaba en lh_Tail");
    assert_eq!(vistos, [(id(0, 0), 8), (id(0, 1), 15), (id(0, 2), 16), (id(0, 3), 24), (id(0, 4), 32)]);
    x(&mut c, "FreeCModeList", &[], &[l]);
    assert_eq!(c.0.bloques_de(c.2).len(), antes);
    // Filtros que los dejan fuera: profundidad, tamaño y modelos de color que no da ninguno.
    let t = etiquetas(&mut c, &[(0x8004_0000, 33)]);
    assert_eq!(x(&mut c, "AllocCModeListTagList", &[], &[0, t]), 0);
    let t = etiquetas(&mut c, &[(0x8004_0003, 320)]);
    assert_eq!(x(&mut c, "AllocCModeListTagList", &[], &[0, t]), 0);
    let modelos = datos(&mut c, &[0, 2, 0, 12, 0xFF, 0xFF]);
    let t = etiquetas(&mut c, &[(0x8004_0006, modelos)]);
    assert_eq!(x(&mut c, "AllocCModeListTagList", &[], &[0, t]), 0);
    let modelos = datos(&mut c, &[0, 5, 0, 11, 0xFF, 0xFF]);
    let t = etiquetas(&mut c, &[(0x8004_0006, modelos)]);
    let l = x(&mut c, "AllocCModeListTagList", &[], &[0, t]);
    assert_ne!(l, 0);
    assert_eq!(l32(&c, l32(&c, l) + CMN_DISPLAYID), id(0, 2), "RGB16 y ARGB32: el primero, el de 16");
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
    // Hostil: soltar dos veces o lo que no es una lista; sin pedidor de modos (falla limpia, apuntado).
    x(&mut c, "FreeCModeList", &[], &[l]);
    x(&mut c, "FreeCModeList", &[], &[l]);
    x(&mut c, "FreeCModeList", &[], &[0x1234]);
    x(&mut c, "FreeCModeList", &[], &[0]);
    assert_eq!(x(&mut c, "CModeRequestTagList", &[], &[0, 0]), 0);
    let av = c.1.tomar_avisos();
    assert_eq!(av.len(), 3, "{:?}", av);
    assert!(matches!(av[2].fallo, Fallo::NoImplementada { .. }));
    // Una lista sin soltar se olvida al acabar la tarea (su memoria la suelta exec).
    x(&mut c, "AllocCModeListTagList", &[], &[0, 0]);
    let t = c.2;
    c.1.tarea_muerta(&mut c.0, t);
}

#[test]
fn atributos_de_mapas() {
    let mut c = caja();
    let (bm, _) = nativo(&mut c, 100, 50);
    let a = |c: &mut Caja, bm: u32, attr: u32| x(c, "GetCyberMapAttr", &[attr], &[bm]);
    assert_eq!(a(&mut c, bm, 0x8000_0008), u32::MAX, "ISCYBERGFX");
    assert_eq!(a(&mut c, bm, 0x8000_0009), u32::MAX, "ISLINEARMEM");
    assert_eq!(a(&mut c, bm, 0x8000_0004), PIXFMT_ARGB32);
    assert_eq!(a(&mut c, bm, 0x8000_0005), 100);
    assert_eq!(a(&mut c, bm, 0x8000_0006), 50);
    assert_eq!(a(&mut c, bm, 0x8000_0007), 32);
    assert_eq!(a(&mut c, bm, 0x8000_0002), 4);
    assert_eq!(a(&mut c, bm, 0x8000_0001), 400);
    assert_eq!(a(&mut c, bm, 0x8000_00FF), u32::MAX);
    let (pl, _) = planar(&mut c, 3, 40, 10);
    assert_eq!(a(&mut c, pl, 0x8000_0008), 0);
    assert_eq!(a(&mut c, pl, 0x8000_0005), 48, "el ancho de un planar va en palabras");
    assert_eq!(a(&mut c, pl, 0x8000_0006), 10);
    assert_eq!(a(&mut c, pl, 0x8000_0007), 3);
    assert!(c.1.tomar_avisos().is_empty());
    assert_eq!(a(&mut c, 0, 0x8000_0008), 0);
    assert_eq!(a(&mut c, 0xFFFF_FF00, 0x8000_0008), 0);
    assert_eq!(c.1.tomar_avisos().len(), 2);
    // AllocBitMap: más de 8 planos, o un formato de color verdadero con BMF_SPECIALFMT, da un mapa de cybergraphics.
    let especial = |fmt: u32| BMF_SPECIALFMT | (fmt << 24);
    // Cada uno en su formato: el pedido, o el de serie de su profundidad.
    for (prof, flags, fmt, bits) in [
        (15, 0, 1, 15),
        (16, 0, PIXFMT_RGB16, 16),
        (24, 2, PIXFMT_RGB24, 24),
        (32, 2 | 16, PIXFMT_ARGB32, 32),
        (12, 0, 1, 15),
        (8, especial(PIXFMT_RGB16), PIXFMT_RGB16, 16),
        (32, especial(10), 10, 24),
        (24, especial(PIXFMT_RGBA32), PIXFMT_RGBA32, 32),
        (8, especial(PIXFMT_LUT8), 0, 8),
        (8, 2, 0, 8),
        (5, 0, 0, 5),
    ] {
        let m = g(&mut c, "AllocBitMap", &[20, 10, prof, flags], &[0]);
        assert_ne!(m, 0, "{} {:x}", prof, flags);
        let de_cgx = fmt != 0;
        assert_eq!(a(&mut c, m, 0x8000_0008) != 0, de_cgx, "{} {:x}", prof, flags);
        assert_eq!(a(&mut c, m, 0x8000_0004), fmt, "{} {:x}", prof, flags);
        assert_eq!(g(&mut c, "GetBitMapAttr", &[0, 4], &[m]), bits, "{} {:x}", prof, flags);
        assert_eq!(a(&mut c, m, 0x8000_0007), if de_cgx { bits } else { prof });
        g(&mut c, "FreeBitMap", &[], &[m]);
    }
    // Un amigo de cybergraphics da otro de cybergraphics aunque pida 8 planos.
    let m = g(&mut c, "AllocBitMap", &[20, 10, 8, 16], &[bm]);
    assert_eq!(a(&mut c, m, 0x8000_0008), u32::MAX);
    assert!(c.1.tomar_avisos().is_empty());
}

#[test]
fn escribir_y_leer_pixeles() {
    let mut c = caja();
    let (_, rp) = nativo(&mut c, 64, 32);
    // RGB: 2 x 2 en (10, 5), con su módulo (3 píxeles por fila, el primero se salta con SrcX = 1).
    let src = datos(&mut c, &[0, 0, 0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0, 0, 0, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC]);
    assert_eq!(x(&mut c, "WritePixelArray", &[1, 0, 9, 10, 5, 2, 2, RECTFMT_RGB], &[src, rp]), 4);
    assert_eq!(rgb(&mut c, rp, 10, 5), 0x112233);
    assert_eq!(rgb(&mut c, rp, 11, 5), 0x445566);
    assert_eq!(rgb(&mut c, rp, 10, 6), 0x778899);
    assert_eq!(rgb(&mut c, rp, 11, 6), 0xAABBCC);
    // RGBA, ARGB y GREY8.
    let s = datos(&mut c, &[1, 2, 3, 99]);
    x(&mut c, "WritePixelArray", &[0, 0, 4, 0, 0, 1, 1, RECTFMT_RGBA], &[s, rp]);
    assert_eq!(rgb(&mut c, rp, 0, 0), 0x010203);
    let s = datos(&mut c, &[99, 4, 5, 6]);
    x(&mut c, "WritePixelArray", &[0, 0, 4, 1, 0, 1, 1, RECTFMT_ARGB], &[s, rp]);
    assert_eq!(rgb(&mut c, rp, 1, 0), 0x040506);
    let s = datos(&mut c, &[0x80]);
    x(&mut c, "WritePixelArray", &[0, 0, 1, 2, 0, 1, 1, RECTFMT_GREY8], &[s, rp]);
    assert_eq!(rgb(&mut c, rp, 2, 0), 0x808080);
    // LUT8: por la paleta del mapa (la del Workbench: la pluma 2 es blanca).
    let s = datos(&mut c, &[2]);
    x(&mut c, "WritePixelArray", &[0, 0, 1, 3, 0, 1, 1, RECTFMT_LUT8], &[s, rp]);
    assert_eq!(rgb(&mut c, rp, 3, 0), 0xFFFFFF);
    // WriteLUTPixelArray con su tabla.
    let mut t = vec![0u8; 1024];
    t[4 * 7..4 * 7 + 4].copy_from_slice(&[0xEE, 0x12, 0x34, 0x56]);
    let tabla = datos(&mut c, &t);
    let s = datos(&mut c, &[7, 7]);
    assert_eq!(x(&mut c, "WriteLUTPixelArray", &[0, 0, 2, 20, 20, 2, 1, CTABFMT_XRGB8], &[s, rp, tabla]), 2);
    assert_eq!(rgb(&mut c, rp, 21, 20), 0x123456);
    // ReadPixelArray: de vuelta, en los tres formatos (el alfa, 0).
    let dst = reservar(&mut c, 64);
    assert_eq!(x(&mut c, "ReadPixelArray", &[0, 0, 6, 10, 5, 2, 1, RECTFMT_RGB], &[dst, rp]), 2);
    assert_eq!((0..6).map(|i| l8(&c, dst + i)).collect::<Vec<_>>(), [0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
    x(&mut c, "ReadPixelArray", &[0, 0, 8, 10, 6, 1, 1, RECTFMT_ARGB], &[dst, rp]);
    assert_eq!(l32(&c, dst), 0x0077_8899);
    x(&mut c, "ReadPixelArray", &[0, 0, 8, 10, 6, 1, 1, RECTFMT_RGBA], &[dst, rp]);
    assert_eq!(l32(&c, dst), 0x7788_9900);
    // WriteRGBPixel / ReadRGBPixel.
    assert_eq!(x(&mut c, "WriteRGBPixel", &[63, 31, 0xFF00_FF00], &[0, rp]), 0);
    assert_eq!(rgb(&mut c, rp, 63, 31), 0x00FF00);
    assert_eq!(x(&mut c, "WriteRGBPixel", &[64, 0, 0], &[0, rp]), u32::MAX);
    assert_eq!(x(&mut c, "WriteRGBPixel", &[(-1i32) as u32, 0, 0], &[0, rp]), u32::MAX);
    assert_eq!(rgb(&mut c, rp, -1, 0), 0);
    // FillPixelArray, InvertPixelArray (dos veces deja lo que había) y MovePixelArray (solapado).
    assert_eq!(x(&mut c, "FillPixelArray", &[30, 10, 4, 4, 0x00AB_CDEF], &[0, rp]), 16);
    assert_eq!(rgb(&mut c, rp, 33, 13), 0xABCDEF);
    x(&mut c, "InvertPixelArray", &[30, 10, 2, 1], &[0, rp]);
    assert_eq!(rgb(&mut c, rp, 30, 10), 0x543210);
    x(&mut c, "InvertPixelArray", &[30, 10, 2, 1], &[0, rp]);
    assert_eq!(rgb(&mut c, rp, 30, 10), 0xABCDEF);
    assert_eq!(x(&mut c, "MovePixelArray", &[30, 10, 32, 11, 4, 4], &[0, rp]), 16);
    assert_eq!(rgb(&mut c, rp, 35, 14), 0xABCDEF);
    assert_eq!(rgb(&mut c, rp, 32, 11), 0xABCDEF);
    // ScalePixelArray: 2 x 1 a 4 x 2, al más cercano.
    let s = datos(&mut c, &[0xFF, 0, 0, 0, 0, 0xFF]);
    assert_eq!(x(&mut c, "ScalePixelArray", &[2, 1, 6, 40, 0, 4, 2, RECTFMT_RGB], &[s, rp]), 8);
    let fila: Vec<u32> = (40..44).map(|px| rgb(&mut c, rp, px, 1)).collect();
    assert_eq!(fila, [0xFF0000, 0xFF0000, 0x0000FF, 0x0000FF]);
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}

#[test]
fn en_mapas_planares() {
    let mut c = caja();
    let (_, rp) = planar(&mut c, 3, 32, 8);
    // El color verdadero, a la pluma más cercana del Workbench; LUT8 tal cual; FillPixelArray, el azul.
    let s = datos(&mut c, &[0, 0, 0]);
    assert_eq!(x(&mut c, "WritePixelArray", &[0, 0, 3, 2, 2, 1, 1, RECTFMT_RGB], &[s, rp]), 1);
    let p = pluma(&mut c, rp, 2, 2);
    assert_eq!(rgb(&mut c, rp, 2, 2), 0, "la pluma {} es negra", p);
    let s = datos(&mut c, &[5, 6]);
    x(&mut c, "WritePixelArray", &[0, 0, 2, 4, 2, 2, 1, RECTFMT_LUT8], &[s, rp]);
    assert_eq!((pluma(&mut c, rp, 4, 2), pluma(&mut c, rp, 5, 2)), (5, 6));
    x(&mut c, "FillPixelArray", &[0, 5, 3, 2, 0x00FF_FF03], &[0, rp]);
    assert_eq!(pluma(&mut c, rp, 2, 6), 3);
    x(&mut c, "InvertPixelArray", &[0, 5, 1, 1], &[0, rp]);
    assert_eq!(pluma(&mut c, rp, 0, 5), 4, "todos los planos (3)");
    // ExtractColor: la pluma 3 a un mapa de un plano.
    let (una, rp1) = planar(&mut c, 1, 16, 4);
    assert_eq!(x(&mut c, "ExtractColor", &[3, 0, 5, 4, 2], &[rp, una]), 1);
    assert_eq!((pluma(&mut c, rp1, 0, 0), pluma(&mut c, rp1, 1, 0), pluma(&mut c, rp1, 0, 1)), (0, 1, 1));
    // Un mapa de destino demasiado pequeño, o nativo: FALSE.
    assert_eq!(x(&mut c, "ExtractColor", &[3, 0, 0, 17, 1], &[rp, una]), 0);
    let (nat, _) = nativo(&mut c, 8, 8);
    assert_eq!(x(&mut c, "ExtractColor", &[3, 0, 0, 1, 1], &[rp, nat]), 0);
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}

/// Lo hostil de las funciones de píxeles: nada se cae, lo de fuera no se pinta y lo malo se apunta.
#[test]
fn pixeles_hostiles() {
    let mut c = caja();
    let (bm, rp) = nativo(&mut c, 16, 16);
    let src = datos(&mut c, &[0x10; 16 * 16 * 4]);
    // Parte fuera por la izquierda y arriba (negativos): solo lo de dentro, del origen desplazado.
    let s2 = datos(&mut c, &[1, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4, 4]);
    assert_eq!(x(&mut c, "WritePixelArray", &[0, 0, 6, (-1i32) as u32, (-1i32) as u32, 2, 2, RECTFMT_RGB], &[s2, rp]), 1);
    assert_eq!(rgb(&mut c, rp, 0, 0), 0x040404);
    // Todo fuera: 0 y sin leer el origen (aunque el origen sea nulo... no: nulo es un fallo).
    assert_eq!(x(&mut c, "WritePixelArray", &[0, 0, 64, 100, 100, 16, 16, RECTFMT_ARGB], &[src, rp]), 0);
    // Enorme (65535 x 65535): solo se lee lo que cae dentro del mapa.
    assert_eq!(x(&mut c, "WritePixelArray", &[0, 0, 64, 0, 0, 0xFFFF, 0xFFFF, RECTFMT_ARGB], &[src, rp]), 256);
    assert_eq!(x(&mut c, "FillPixelArray", &[0, 0, 0xFFFF, 0xFFFF, 0x123456], &[0, rp]), 256);
    assert_eq!(x(&mut c, "InvertPixelArray", &[(-32768i32) as u32, (-32768i32) as u32, 0xFFFF, 0xFFFF], &[0, rp]), 256);
    // Un tamaño de 0: nada.
    assert_eq!(x(&mut c, "WritePixelArray", &[0, 0, 64, 0, 0, 0, 5, RECTFMT_ARGB], &[src, rp]), 0);
    assert_eq!(x(&mut c, "ScalePixelArray", &[0, 4, 64, 0, 0, 5, 5, RECTFMT_ARGB], &[src, rp]), 0);
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
    // Formatos raros, punteros nulos, un origen que se sale de la memoria, un RastPort que no lo es.
    assert_eq!(x(&mut c, "WritePixelArray", &[0, 0, 64, 0, 0, 4, 4, 7], &[src, rp]), 0);
    assert_eq!(x(&mut c, "WritePixelArray", &[0, 0, 64, 0, 0, 4, 4, RECTFMT_RGB], &[0, rp]), 0);
    assert_eq!(x(&mut c, "WritePixelArray", &[0, 0, 64, 0, 0, 4, 4, RECTFMT_RGB], &[src, 0]), 0);
    assert_eq!(x(&mut c, "WritePixelArray", &[0, 0, 0xFFFF, 0, 0, 16, 16, RECTFMT_ARGB], &[TAMANO - 64, rp]), 0);
    assert_eq!(x(&mut c, "ReadPixelArray", &[0, 0, 64, 0, 0, 4, 4, RECTFMT_LUT8], &[src, rp]), 0);
    assert_eq!(x(&mut c, "ReadPixelArray", &[0, 0, 64, 0, 0, 4, 4, RECTFMT_RGB], &[0, rp]), 0);
    assert_eq!(x(&mut c, "WriteLUTPixelArray", &[0, 0, 4, 0, 0, 4, 4, 1], &[src, rp, src]), 0);
    assert_eq!(x(&mut c, "WriteLUTPixelArray", &[0, 0, 4, 0, 0, 4, 4, 0], &[src, rp, 0]), 0);
    assert_eq!(x(&mut c, "WriteRGBPixel", &[0, 0, 0], &[0, 0]), u32::MAX);
    assert_eq!(x(&mut c, "ScalePixelArray", &[4, 4, 64, 0, 0, 8, 8, 9], &[src, rp]), 0);
    assert_eq!(x(&mut c, "FillPixelArray", &[0, 0, 4, 4, 0], &[0, 0xFFFF_FF00]), 0);
    assert_eq!(c.1.tomar_avisos().len(), 11);
    // ReadPixelArray en parte fuera: lo de fuera no se toca.
    let dst = datos(&mut c, &[0xEE; 12]);
    x(&mut c, "FillPixelArray", &[0, 0, 16, 16, 0x0A0B0C], &[0, rp]);
    assert_eq!(x(&mut c, "ReadPixelArray", &[0, 0, 12, 15, 0, 3, 1, RECTFMT_ARGB], &[dst, rp]), 1);
    assert_eq!((l32(&c, dst), l32(&c, dst + 4)), (0x000A_0B0C, 0xEEEE_EEEE));
    // Al azar: tamaños, posiciones, módulos y formatos cualesquiera; nada se cae.
    let mut s: u32 = 0x1234_5678;
    let mut azar = || {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        s
    };
    for _ in 0..2000 {
        let f = ["WritePixelArray", "ReadPixelArray", "ScalePixelArray", "WriteLUTPixelArray"][(azar() % 4) as usize];
        let d: Vec<u32> = (0..8).map(|i| if i == 7 { azar() % 6 } else { azar() & 0x1F }).collect();
        x(&mut c, f, &d, &[src, rp, src]);
    }
    c.1.tomar_avisos();
    assert_eq!(x(&mut c, "GetCyberMapAttr", &[0x8000_0005], &[bm]), 16);
}

/// El recorte: a la capa (sus coordenadas) y a su región, como el resto de graphics.
#[test]
fn recorte_de_capa() {
    let mut c = caja();
    let n = c.1.numero_capas("NewLayerInfo").unwrap();
    let mut r = regs(&[], &[]);
    c.1.llamar(&mut c.0, n, &mut r);
    let li = r.d[0];
    let (bm, rpm) = nativo(&mut c, 32, 16);
    let n = c.1.numero_capas("CreateUpfrontLayer").unwrap();
    let mut r = regs(&[4, 2, 19, 9, 1], &[li, bm, 0]);
    c.1.llamar(&mut c.0, n, &mut r);
    let capa = r.d[0];
    assert_ne!(capa, 0);
    let rp = l32(&c, capa + caja_intuition::estructuras::layer::RP);
    // Un relleno enorme en la capa: solo dentro de ella (16 x 8), con su (0, 0) en el (4, 2) del mapa.
    assert_eq!(x(&mut c, "FillPixelArray", &[0, 0, 1000, 1000, 0xFF0000], &[0, rp]), 128);
    assert_eq!(rgb(&mut c, rpm, 4, 2), 0xFF0000);
    assert_eq!(rgb(&mut c, rpm, 19, 9), 0xFF0000);
    assert_ne!(rgb(&mut c, rpm, 20, 9), 0xFF0000);
    assert_ne!(rgb(&mut c, rpm, 3, 2), 0xFF0000);
    // Con una región: solo un rectángulo de (1, 1) a (2, 2) de la capa.
    let reg = g(&mut c, "NewRegion", &[], &[]);
    let q = datos(&mut c, &[0, 1, 0, 1, 0, 2, 0, 2]);
    g(&mut c, "OrRectRegion", &[], &[reg, q]);
    let n = c.1.numero_capas("InstallClipRegion").unwrap();
    let mut r = regs(&[], &[capa, reg]);
    c.1.llamar(&mut c.0, n, &mut r);
    assert_eq!(x(&mut c, "FillPixelArray", &[0, 0, 16, 8, 0x00FF00], &[0, rp]), 4);
    assert_eq!(rgb(&mut c, rpm, 5, 3), 0x00FF00);
    assert_eq!(rgb(&mut c, rpm, 6, 4), 0x00FF00);
    assert_eq!(rgb(&mut c, rpm, 7, 4), 0xFF0000);
    let s = datos(&mut c, &[9; 16 * 8 * 3]);
    assert_eq!(x(&mut c, "WritePixelArray", &[0, 0, 48, 0, 0, 16, 8, RECTFMT_RGB], &[s, rp]), 4);
    assert_eq!(rgb(&mut c, rpm, 5, 3), 0x090909);
    assert_eq!(x(&mut c, "WriteRGBPixel", &[0, 0, 0x777777], &[0, rp]), 0, "dentro del mapa aunque recortado");
    assert_eq!(rgb(&mut c, rpm, 4, 2), 0xFF0000);
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}

#[test]
fn acceso_directo() {
    let mut c = caja();
    let (bm, rp) = nativo(&mut c, 8, 4);
    x(&mut c, "FillPixelArray", &[0, 0, 8, 4, 0x112233], &[0, rp]);
    let antes = c.0.bloques_de(0).len();
    let campos = reservar(&mut c, 28);
    let t = etiquetas(&mut c, &[
        (LBMI_BASEADDRESS, campos),
        (LBMI_WIDTH, campos + 4),
        (LBMI_HEIGHT, campos + 8),
        (LBMI_DEPTH, campos + 12),
        (LBMI_PIXFMT, campos + 16),
        (LBMI_BYTESPERPIX, campos + 20),
        (LBMI_BYTESPERROW, campos + 24),
    ]);
    let asa = x(&mut c, "LockBitMapTagList", &[], &[bm, t]);
    assert_ne!(asa, 0);
    let base = l32(&c, campos);
    assert_eq!(base, asa);
    // La dirección es la de sus píxeles de verdad (la misma que da CYBRMATTR_DISPADR): no hay copia.
    assert_eq!(x(&mut c, "GetCyberMapAttr", &[0x8000_0003], &[bm]), base);
    assert_eq!(c.0.bloques_de(0).len(), antes, "coger no reserva nada");
    assert_eq!((l32(&c, campos + 4), l32(&c, campos + 8), l32(&c, campos + 12)), (8, 4, 32));
    assert_eq!((l32(&c, campos + 16), l32(&c, campos + 20), l32(&c, campos + 24)), (PIXFMT_ARGB32, 4, 32));
    assert_eq!(l32(&c, base), 0x0011_2233);
    assert_eq!(l32(&c, base + 32 * 3 + 28), 0x0011_2233);
    // Lo escrito ahí está en el mapa en seguida (aún cogido) y sigue al soltarlo.
    e32(&mut c, base + 32 + 4, 0x00AB_CDEF);
    assert_eq!(rgb(&mut c, rp, 1, 1), 0xABCDEF);
    x(&mut c, "UnLockBitMap", &[], &[asa]);
    assert_eq!(rgb(&mut c, rp, 1, 1), 0xABCDEF);
    assert_eq!(c.1.cerrojos_cgx(), 0);
    // Otra vez por la misma tarea: la misma asa, y hay que soltarlo dos veces; UBMI_REALLYUNLOCK a FALSE no lo suelta.
    let a1 = x(&mut c, "LockBitMapTagList", &[], &[bm, t]);
    let a2 = x(&mut c, "LockBitMapTagList", &[], &[bm, t]);
    assert_eq!((a1, a2), (base, base));
    x(&mut c, "UnLockBitMap", &[], &[a1]);
    assert_eq!(c.1.cerrojos_cgx(), 1);
    let tu = etiquetas(&mut c, &[(UBMI_REALLYUNLOCK, 0)]);
    x(&mut c, "UnLockBitMapTagList", &[], &[a1, tu]);
    assert_eq!(c.1.cerrojos_cgx(), 1);
    x(&mut c, "UnLockBitMapTagList", &[], &[a1, 0]);
    assert_eq!(c.1.cerrojos_cgx(), 0);
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
    // Un mapa planar no es de cybergraphics: 0, sin aviso, y los campos sin tocar.
    let (pl, _) = planar(&mut c, 2, 16, 4);
    e32(&mut c, campos, 0xDEAD);
    assert_eq!(x(&mut c, "LockBitMapTagList", &[], &[pl, t]), 0);
    assert_eq!(l32(&c, campos), 0xDEAD);
    assert!(c.1.tomar_avisos().is_empty());
    // Hostil: mapa nulo, una etiqueta que apunta fuera, soltar lo que no está cogido o dos veces, una RectList en
    // círculo.
    assert_eq!(x(&mut c, "LockBitMapTagList", &[], &[0, t]), 0);
    let mala = etiquetas(&mut c, &[(LBMI_BASEADDRESS, 2)]);
    assert_eq!(x(&mut c, "LockBitMapTagList", &[], &[bm, mala]), 0);
    assert_eq!(c.1.cerrojos_cgx(), 0);
    x(&mut c, "UnLockBitMap", &[], &[0x4000]);
    x(&mut c, "UnLockBitMap", &[], &[0]);
    let a = x(&mut c, "LockBitMapTagList", &[], &[bm, 0]);
    assert_ne!(a, 0, "sin etiquetas también");
    let rl = reservar(&mut c, 16);
    e32(&mut c, rl, 1);
    e32(&mut c, rl + 4, rl);
    let circ = etiquetas(&mut c, &[(UBMI_UPDATERECTS, rl), (UBMI_REALLYUNLOCK, 0)]);
    x(&mut c, "UnLockBitMapTagList", &[], &[a, circ]);
    x(&mut c, "UnLockBitMap", &[], &[a]);
    x(&mut c, "UnLockBitMap", &[], &[a]);
    assert_eq!(c.1.tomar_avisos().len(), 6, "{:?}", c.1.avisos());
    // Cogido y sin soltar: lo suelta FreeBitMap (y la memoria de sus píxeles vuelve)...
    let (bm2, _) = nativo(&mut c, 4, 4);
    assert_ne!(x(&mut c, "LockBitMapTagList", &[], &[bm2, 0]), 0);
    g(&mut c, "FreeBitMap", &[], &[bm2]);
    assert_eq!(c.1.cerrojos_cgx(), 0);
    // ... o el final de la tarea.
    let a = x(&mut c, "LockBitMapTagList", &[], &[bm, 0]);
    e32(&mut c, a, 0x0055_5555);
    let tarea = c.2;
    c.1.tarea_muerta(&mut c.0, tarea);
    assert_eq!(c.1.cerrojos_cgx(), 0);
    assert_eq!(rgb(&mut c, rp, 0, 0), 0x555555);
    assert!(c.0.bloques_de(0).len() <= antes + 2, "solo los mapas de la prueba");
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}

/// `LockBitMapTagList` de un lienzo (la pantalla, una ventana: superficies del compositor) también da la dirección de
/// verdad de sus píxeles en la memoria del 68k, sin copia; al soltarlo, lo que dice `UBMI_UPDATERECTS` (o todo) queda
/// como cambiado para el compositor.
#[test]
fn acceso_directo_a_un_lienzo() {
    use caja_intuition::estructuras::screen;
    let mut c = caja();
    let bm = c.1.workbench() + screen::BITMAP;
    let rp = c.1.workbench() + screen::RASTPORT;
    x(&mut c, "FillPixelArray", &[0, 0, 4, 1, 0x112233], &[0, rp]);
    let antes = c.0.bloques_de(0).len();
    let campos = reservar(&mut c, 8);
    let t = etiquetas(&mut c, &[(LBMI_BASEADDRESS, campos), (LBMI_BYTESPERROW, campos + 4)]);
    let a1 = x(&mut c, "LockBitMapTagList", &[], &[bm, t]);
    assert_ne!(a1, 0);
    assert_eq!((l32(&c, campos), l32(&c, campos + 4)), (a1, 640 * 4));
    assert_eq!(a1, x(&mut c, "GetCyberMapAttr", &[0x8000_0003], &[bm]), "la de sus píxeles");
    assert_eq!(c.0.bloques_de(0).len(), antes, "sin búfer de copia");
    assert_eq!(l32(&c, a1) & 0x00FF_FFFF, 0x112233);
    // Lo escrito se ve en seguida; al soltar con un rectángulo, ese queda por presentar.
    e32(&mut c, a1 + 8, 0x0000_0002);
    assert_eq!(rgb(&mut c, rp, 2, 0), 2);
    let rl = reservar(&mut c, 16);
    e32(&mut c, rl, 1);
    e16(&mut c, rl + 8 + 4, 2);
    let tu = etiquetas(&mut c, &[(UBMI_UPDATERECTS, rl), (UBMI_REALLYUNLOCK, 0)]);
    x(&mut c, "UnLockBitMapTagList", &[], &[a1, tu]);
    assert_eq!(c.1.cerrojos_cgx(), 1);
    assert!(c.1.lienzo(bm).unwrap().dano().is_some_and(|d| d.contiene(2, 0)));
    x(&mut c, "UnLockBitMap", &[], &[a1]);
    assert_eq!(c.1.cerrojos_cgx(), 0);
    assert_eq!(c.0.bloques_de(0).len(), antes);
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}

fn p(c: &mut Caja, nombre: &str, d: &[u32], a: &[u32]) -> u32 {
    let n = c.1.numero_p96(nombre).expect("función de Picasso96API");
    let mut r = regs(d, a);
    c.1.llamar(&mut c.0, n, &mut r);
    r.d[0]
}

/// Picasso96API: la capa fina sobre lo mismo (mapas, atributos, píxeles con `RenderInfo`, cerrojo y modo).
#[test]
fn picasso96() {
    use caja_intuition::p96::constantes::*;
    let mut c = caja();
    assert_ne!(c.1.p96_base(), 0);
    // Un mapa de color verdadero (por el formato o por la profundidad) y uno CLUT de 8 planos.
    let bm = p(&mut c, "p96AllocBitMap", &[16, 8, 32, 0, 0, 0, 0, RGBFB_A8R8G8B8], &[0]);
    assert_ne!(bm, 0);
    // Cada uno en el suyo: R8G8B8 (3 bytes, filas de 48), R5G6B5 (2), y por la profundidad sin formato (R8G8B8).
    for (rgbf, prof, bpp, otro) in [(RGBFB_R8G8B8, 24, 3, RGBFB_R8G8B8), (RGBFB_R5G6B5, 16, 2, RGBFB_R5G6B5), (0, 24, 3, RGBFB_R8G8B8)] {
        let m = p(&mut c, "p96AllocBitMap", &[16, 8, prof, 0, 0, 0, 0, rgbf], &[0]);
        assert_eq!(p(&mut c, "p96GetBitMapAttr", &[P96BMA_RGBFORMAT], &[m]), otro);
        assert_eq!(p(&mut c, "p96GetBitMapAttr", &[P96BMA_BYTESPERPIXEL], &[m]), bpp);
        assert_eq!(p(&mut c, "p96GetBitMapAttr", &[P96BMA_BYTESPERROW], &[m]), 16 * bpp);
        p(&mut c, "p96FreeBitMap", &[], &[m]);
    }
    let pl = p(&mut c, "p96AllocBitMap", &[16, 8, 8, 0, 0, 0, 0, RGBFB_CLUT], &[0]);
    let a = |c: &mut Caja, bm: u32, attr: u32| p(c, "p96GetBitMapAttr", &[attr], &[bm]);
    assert_eq!((a(&mut c, bm, P96BMA_WIDTH), a(&mut c, bm, P96BMA_HEIGHT)), (16, 8));
    assert_eq!((a(&mut c, bm, P96BMA_RGBFORMAT), a(&mut c, bm, P96BMA_ISP96)), (RGBFB_A8R8G8B8, 1));
    assert_eq!((a(&mut c, bm, P96BMA_BYTESPERPIXEL), a(&mut c, bm, P96BMA_BYTESPERROW)), (4, 64));
    assert_eq!((a(&mut c, pl, P96BMA_RGBFORMAT), a(&mut c, pl, P96BMA_ISP96), a(&mut c, pl, P96BMA_DEPTH)), (RGBFB_NONE, 0, 8));
    let rp = rastport(&mut c, bm);
    // p96WritePixelArray con RenderInfo en R8G8B8 (como Dash) y en R5G6B5; p96ReadPixelArray de vuelta en A8R8G8B8.
    let px = datos(&mut c, &[0x10, 0x20, 0x30, 0x40, 0x50, 0x60]);
    let ri = reservar(&mut c, 12);
    e32(&mut c, ri, px);
    e16(&mut c, ri + 4, 6);
    e32(&mut c, ri + 8, RGBFB_R8G8B8);
    p(&mut c, "p96WritePixelArray", &[0, 0, 3, 2, 2, 1], &[ri, rp]);
    assert_eq!((rgb(&mut c, rp, 3, 2), rgb(&mut c, rp, 4, 2)), (0x102030, 0x405060));
    let px16 = datos(&mut c, &[0xF8, 0x00]);
    e32(&mut c, ri, px16);
    e32(&mut c, ri + 8, RGBFB_R5G6B5);
    p(&mut c, "p96WritePixelArray", &[0, 0, 5, 5, 1, 1], &[ri, rp]);
    assert_eq!(rgb(&mut c, rp, 5, 5), 0xFF0000);
    let dst = reservar(&mut c, 8);
    e32(&mut c, ri, dst);
    e32(&mut c, ri + 8, RGBFB_A8R8G8B8);
    p(&mut c, "p96ReadPixelArray", &[0, 0, 3, 2, 2, 1], &[ri, rp]);
    assert_eq!((l32(&c, dst), l32(&c, dst + 4)), (0x0010_2030, 0x0040_5060));
    e32(&mut c, ri + 8, RGBFB_B5G5R5PC);
    p(&mut c, "p96ReadPixelArray", &[0, 0, 5, 5, 1, 1], &[ri, rp]);
    assert_eq!(l16(&c, dst), 0x1F00, "B5G5R5PC: rojo en los bits bajos, en little-endian");
    // p96WritePixel / p96ReadPixel / p96RectFill / p96EncodeColor.
    assert_eq!(p(&mut c, "p96WritePixel", &[0, 0, 0x00AA_BBCC], &[0, rp]), 0);
    assert_eq!(p(&mut c, "p96ReadPixel", &[0, 0], &[0, rp]), 0xAABBCC);
    p(&mut c, "p96RectFill", &[8, 0, 15, 7, 0x0000_FF00], &[0, rp]);
    assert_eq!((rgb(&mut c, rp, 8, 0), rgb(&mut c, rp, 15, 7)), (0xFF00, 0xFF00));
    assert_ne!(rgb(&mut c, rp, 7, 0), 0xFF00);
    assert_eq!(p(&mut c, "p96EncodeColor", &[RGBFB_R5G6B5, 0x00FF_0000], &[]), 0xF800);
    assert_eq!(p(&mut c, "p96EncodeColor", &[RGBFB_A8R8G8B8, 0x0012_3456], &[]), 0x0012_3456);
    // El modo de la caja.
    assert_eq!(p(&mut c, "p96BestModeIDTagList", &[], &[0]), caja_intuition::modos::id(0, 0), "8 bits de serie");
    let t = etiquetas(&mut c, &[(P96BIDTAG_FORMATS_ALLOWED, 1 << RGBFB_R5G6B5)]);
    assert_eq!(p(&mut c, "p96BestModeIDTagList", &[], &[t]), caja_intuition::modos::id(0, 2));
    let t = etiquetas(&mut c, &[(P96BIDTAG_FORMATS_ALLOWED, 1 << 3)]);
    assert_eq!(p(&mut c, "p96BestModeIDTagList", &[], &[t]), INVALID_ID, "B8G8R8: ninguno");
    let m = |c: &mut Caja, attr: u32| p(c, "p96GetModeIDAttr", &[MODO_CAJA, attr], &[]);
    assert_eq!((m(&mut c, P96IDA_WIDTH), m(&mut c, P96IDA_HEIGHT), m(&mut c, P96IDA_RGBFORMAT)), (640, 480, RGBFB_A8R8G8B8));
    assert_eq!((m(&mut c, P96IDA_ISP96), m(&mut c, 99)), (1, u32::MAX));
    let placa = m(&mut c, P96IDA_BOARDNAME);
    assert_eq!(l8(&c, placa), b'D');
    // p96LockBitMap: su RenderInfo y el búfer; al soltarlo llega al mapa.
    let buf = reservar(&mut c, 12);
    let asa = p(&mut c, "p96LockBitMap", &[12], &[bm, buf]);
    assert_ne!(asa, 0);
    let mem = l32(&c, buf);
    assert_eq!((l16(&c, buf + 4), l32(&c, buf + 8)), (64, RGBFB_A8R8G8B8));
    assert_eq!(l32(&c, mem), 0x00AA_BBCC);
    e32(&mut c, mem + 4, 0x0001_0203);
    p(&mut c, "p96UnlockBitMap", &[asa], &[bm]);
    assert_eq!(rgb(&mut c, rp, 1, 0), 0x010203);
    assert_eq!(c.1.cerrojos_cgx(), 0);
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
    // Hostil: un planar no se coge; soltar con otro mapa, RenderInfo nulo o con un formato que no existe, formatos
    // raros en EncodeColor, lo que no está hecho.
    assert_eq!(p(&mut c, "p96LockBitMap", &[12], &[pl, buf]), 0);
    let asa = p(&mut c, "p96LockBitMap", &[0], &[bm, 0]);
    assert_ne!(asa, 0, "sin búfer también");
    p(&mut c, "p96UnlockBitMap", &[asa], &[pl]);
    assert_eq!(c.1.cerrojos_cgx(), 1);
    p(&mut c, "p96UnlockBitMap", &[asa], &[bm]);
    p(&mut c, "p96WritePixelArray", &[0, 0, 0, 0, 1, 1], &[0, rp]);
    e32(&mut c, ri + 8, 14);
    p(&mut c, "p96WritePixelArray", &[0, 0, 0, 0, 1, 1], &[ri, rp]);
    e32(&mut c, ri + 8, 0x1234_5678);
    p(&mut c, "p96ReadPixelArray", &[0, 0, 0, 0, 1, 1], &[ri, rp]);
    assert_eq!(p(&mut c, "p96EncodeColor", &[RGBFB_CLUT, 0], &[]), 0);
    assert_eq!(p(&mut c, "p96EncodeColor", &[99, 0], &[]), 0);
    assert_eq!(p(&mut c, "p96OpenScreenTagList", &[], &[0]), 0);
    let av = c.1.tomar_avisos();
    assert_eq!(av.len(), 7, "{:?}", av);
    assert!(matches!(av[6].fallo, Fallo::NoImplementada { .. }));
    // Los mapas se sueltan con p96FreeBitMap.
    let antes = c.0.bloques_de(0).len();
    p(&mut c, "p96FreeBitMap", &[], &[bm]);
    p(&mut c, "p96FreeBitMap", &[], &[pl]);
    p(&mut c, "p96FreeBitMap", &[], &[0]);
    assert!(c.0.bloques_de(0).len() < antes);
    assert!(c.1.tomar_avisos().is_empty());
}

/// graphics: `WritePixelArray8`/`ReadPixelArray8` y las de una línea (las pide picture.datatype): una pluma por byte,
/// filas de 16 en 16; de un mapa nativo se lee la pluma del color más parecido.
#[test]
fn pixeles_de_8_bits() {
    let mut c = caja();
    let (_, rp) = planar(&mut c, 3, 32, 8);
    let tmp = reservar(&mut c, 64);
    // 3 x 2 en (4, 1): filas de 16 bytes.
    let mut t = vec![0u8; 32];
    t[..3].copy_from_slice(&[1, 2, 3]);
    t[16..19].copy_from_slice(&[4, 5, 6]);
    let src = datos(&mut c, &t);
    assert_eq!(g(&mut c, "WritePixelArray8", &[4, 1, 6, 2], &[rp, tmp, src]), 6);
    assert_eq!((pluma(&mut c, rp, 4, 1), pluma(&mut c, rp, 6, 2)), (1, 6));
    let dst = reservar(&mut c, 32);
    assert_eq!(g(&mut c, "ReadPixelArray8", &[4, 1, 6, 2], &[rp, tmp, dst]), 6);
    assert_eq!((l8(&c, dst), l8(&c, dst + 2), l8(&c, dst + 16), l8(&c, dst + 18)), (1, 3, 4, 6));
    let lin = datos(&mut c, &[7, 7, 7, 7]);
    assert_eq!(g(&mut c, "WritePixelLine8", &[0, 7, 4], &[rp, tmp, lin]), 4);
    assert_eq!(g(&mut c, "ReadPixelLine8", &[2, 7, 2], &[rp, tmp, dst]), 2);
    assert_eq!(l8(&c, dst + 1), 7);
    // De un mapa nativo: la pluma 2 (blanco) se lee como 2.
    let (_, rpn) = nativo(&mut c, 8, 2);
    let blanco = datos(&mut c, &[2]);
    g(&mut c, "WritePixelLine8", &[0, 0, 1], &[rpn, tmp, blanco]);
    assert_eq!(rgb(&mut c, rpn, 0, 0), 0xFFFFFF);
    assert_eq!(g(&mut c, "ReadPixelLine8", &[0, 0, 1], &[rpn, tmp, dst]), 1);
    assert_eq!(l8(&c, dst), 2);
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
    // Al revés o fuera: nada; un RastPort nulo o una tabla fuera de la memoria, aviso.
    assert_eq!(g(&mut c, "WritePixelArray8", &[6, 2, 4, 1], &[rp, tmp, src]), 0);
    assert_eq!(g(&mut c, "WritePixelLine8", &[100, 100, 4], &[rp, tmp, lin]), 0);
    assert_eq!(g(&mut c, "ReadPixelLine8", &[0, 0, 4], &[0, tmp, dst]), 0);
    assert_eq!(g(&mut c, "WritePixelLine8", &[0, 0, 4], &[rp, tmp, TAMANO - 2]), 0);
    assert_eq!(c.1.tomar_avisos().len(), 2);
}
