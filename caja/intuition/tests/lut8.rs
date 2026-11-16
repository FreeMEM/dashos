//! Los mapas chunky de 8 bits (`PIXFMT_LUT8`, `RGBFB_CLUT`): una pluma por byte con una paleta, como en una tarjeta.
//! graphics pinta plumas (exactas, también las repetidas), cybergraphics convierte (el color, a la pluma más cercana; la
//! pluma, a su color), las copias pasan por la paleta, y una ventana en una pantalla RTG de 8 bits se presenta con la
//! paleta de su pantalla: cambiar un color cambia lo que se ve sin repintar. Y lo hostil.

mod comun;

use caja_exec::{Config, Exec, Memoria, MemoriaVec};
use caja_intuition::cgx::constantes::*;
use caja_intuition::estructuras::{screen, wa, window as win};
use caja_intuition::modos::id;
use caja_intuition::{CajaGrafica, CompositorDePrueba, Numeros};
use comun::*;

fn x(c: &mut Caja, nombre: &str, d: &[u32], a: &[u32]) -> u32 {
    let n = c.1.numero_cgx(nombre).expect("función de cybergraphics");
    let mut r = regs(d, a);
    c.1.llamar(&mut c.0, n, &mut r);
    r.d[0]
}

fn etiquetas(c: &mut Caja, t: &[(u32, u32)]) -> u32 {
    let d = reservar(c, 8 * t.len() as u32 + 8);
    for (i, (k, v)) in t.iter().enumerate() {
        e32(c, d + 8 * i as u32, *k);
        e32(c, d + 8 * i as u32 + 4, *v);
    }
    d
}

/// Un mapa LUT8 de `ancho` x 4 y su `RastPort`.
fn lut8(c: &mut Caja, ancho: u32) -> (u32, u32) {
    let bm = g(c, "AllocBitMap", &[ancho, 4, 8, BMF_SPECIALFMT | (PIXFMT_LUT8 << 24)], &[0]);
    assert_ne!(bm, 0);
    (bm, rastport(c, bm))
}

#[test]
fn plumas_en_un_mapa_de_8_bits() {
    let mut c = caja();
    let (bm, rp) = lut8(&mut c, 16);
    assert_eq!(x(&mut c, "GetCyberMapAttr", &[0x8000_0004], &[bm]), PIXFMT_LUT8);
    assert_eq!(x(&mut c, "GetCyberMapAttr", &[0x8000_0002], &[bm]), 1);
    assert_eq!(x(&mut c, "GetCyberMapAttr", &[0x8000_0008], &[bm]), u32::MAX);
    assert_eq!(g(&mut c, "GetBitMapAttr", &[0, 4], &[bm]), 8);
    // Nace de la pluma 0; RectFill y WritePixel escriben plumas tal cual, y ReadPixel las lee.
    assert_eq!(pluma(&mut c, rp, 3, 3), 0);
    pen(&mut c, rp, 200);
    rect(&mut c, rp, 2, 1, 5, 2);
    assert_eq!((pluma(&mut c, rp, 2, 1), pluma(&mut c, rp, 5, 2), pluma(&mut c, rp, 6, 2)), (200, 200, 0));
    // En la memoria, un byte por punto: lo que da LockBitMap.
    let base = x(&mut c, "LockBitMapTagList", &[], &[bm, 0]);
    let bpr = x(&mut c, "GetCyberMapAttr", &[0x8000_0001], &[bm]);
    assert_eq!(bpr, 16);
    assert_eq!(l8(&c, base + bpr + 2), 200);
    x(&mut c, "UnLockBitMap", &[], &[base]);
    // COMPLEMENT: los bits de la máscara, como en un planar.
    g(&mut c, "SetDrMd", &[2], &[0, rp]);
    g(&mut c, "WritePixel", &[2, 1], &[0, rp]);
    assert_eq!(pluma(&mut c, rp, 2, 1), 200 ^ 0xFF);
    // cybergraphics: un color, a la pluma más cercana; ReadRGBPixel, el color de la pluma.
    let s = reservar(&mut c, 4);
    e32(&mut c, s, 0x0000_0000);
    x(&mut c, "WritePixelArray", &[0, 0, 4, 8, 0, 1, 1, RECTFMT_ARGB], &[s, rp]);
    let negro = pluma(&mut c, rp, 8, 0) as u32;
    assert_eq!(x(&mut c, "ReadRGBPixel", &[8, 0], &[0, rp]) & 0x00FF_FFFF, 0);
    assert_ne!(negro, 200);
    // RAW: plumas tal cual; LUT8: tal cual.
    let p = reservar(&mut c, 2);
    c.0.mem.leer_mut(p, 2).unwrap().copy_from_slice(&[7, 9]);
    x(&mut c, "WritePixelArray", &[0, 0, 2, 9, 3, 2, 1, RECTFMT_RAW], &[p, rp]);
    assert_eq!((pluma(&mut c, rp, 9, 3), pluma(&mut c, rp, 10, 3)), (7, 9));
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}

/// Copias entre un LUT8, un ARGB32 y un planar: las plumas pasan por la paleta, los colores van a la más cercana, y
/// entre LUT8 y planar, las plumas tal cual.
#[test]
fn copias_con_mapas_de_8_bits() {
    let mut c = caja();
    let (b8, rp8) = lut8(&mut c, 8);
    pen(&mut c, rp8, 2);
    rect(&mut c, rp8, 0, 0, 3, 3);
    pen(&mut c, rp8, 123);
    rect(&mut c, rp8, 4, 0, 7, 3);
    let (b32, rp32) = nativo(&mut c, 8, 4);
    g(&mut c, "BltBitMap", &[0, 0, 0, 0, 8, 4, 0xC0, 0xFF], &[b8, b32]);
    assert_eq!(x(&mut c, "ReadRGBPixel", &[1, 1], &[0, rp32]) & 0x00FF_FFFF, 0xFFFFFF, "la pluma 2, blanca");
    let (bp, rpp) = planar(&mut c, 8, 16, 4);
    g(&mut c, "BltBitMap", &[0, 0, 0, 0, 8, 4, 0xC0, 0xFF], &[b8, bp]);
    assert_eq!((pluma(&mut c, rpp, 1, 1), pluma(&mut c, rpp, 5, 1)), (2, 123));
    let (b8b, rp8b) = lut8(&mut c, 8);
    g(&mut c, "BltBitMap", &[0, 0, 0, 0, 8, 4, 0xC0, 0xFF], &[b32, b8b]);
    assert_eq!(pluma(&mut c, rp8b, 1, 1), 2, "el blanco, a la pluma 2");
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}

/// Una ventana en una pantalla RTG de 8 bits: su mapa es de plumas, y se presenta con la paleta de su pantalla.
#[test]
fn ventana_en_una_pantalla_de_8_bits() {
    let mut e = Exec::arrancar(MemoriaVec::nueva(TAMANO as usize), Config::para(TAMANO)).expect("arranca");
    let t = e.crear_tarea("principal", 0, 4096, 0x10_0000).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(t));
    let gr = CajaGrafica::instalar(&mut e, CompositorDePrueba::nuevo(1920, 1080), Numeros::default()).expect("instala");
    let mut c: Caja = (e, gr, t);
    let ts = etiquetas(&mut c, &[(0x8000_0032, id(1, 0))]);
    let s = i(&mut c, "OpenScreenTagList", &[], &[0, ts]);
    assert_ne!(s, 0);
    let tw = etiquetas(&mut c, &[(wa::WIDTH, 100), (wa::HEIGHT, 60), (wa::CUSTOMSCREEN, s), (wa::FLAGS, 0x0800)]);
    let w = i(&mut c, "OpenWindowTagList", &[], &[0, tw]);
    assert_ne!(w, 0, "{:?}", c.1.avisos());
    let rp = l32(&c, w + win::RPORT);
    let bm = l32(&c, rp + 4);
    assert_eq!(x(&mut c, "GetCyberMapAttr", &[0x8000_0004], &[bm]), PIXFMT_LUT8);
    pen(&mut c, rp, 5);
    rect(&mut c, rp, 10, 10, 20, 20);
    assert_eq!(pluma(&mut c, rp, 15, 15), 5);
    c.1.presentar(&c.0);
    let sup = c.1.superficie(w).unwrap();
    let antes = c.1.comp.ventana(sup).pixel(15, 15);
    assert_eq!(antes & 0x00FF_FFFF, x(&mut c, "ReadRGBPixel", &[15, 15], &[0, rp]) & 0x00FF_FFFF, "el color de la pluma 5");
    // Otro color para la pluma 5: lo que se ve cambia sin repintar la ventana.
    g(&mut c, "SetRGB32", &[5, 0x1200_0000, 0x3400_0000, 0x5600_0000], &[s + screen::VIEWPORT]);
    c.1.presentar(&c.0);
    assert_eq!(c.1.comp.ventana(sup).pixel(15, 15) & 0x00FF_FFFF, 0x123456);
    assert_eq!(pluma(&mut c, rp, 15, 15), 5, "la pluma sigue siendo la 5");
    i(&mut c, "CloseWindow", &[], &[w]);
    assert_eq!(i(&mut c, "CloseScreen", &[], &[s]), 1);
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}
