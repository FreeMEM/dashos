//! Las pantallas propias completas (`pantallas.rs`): su paleta (`SA_Colors`, `SA_Colors32`) y sus plumas (`SA_Pens`), que
//! toman sus ventanas; `SA_LikeWorkbench`; las públicas (`SA_PubName`, `LockPubScreen`, `UnlockPubScreen`,
//! `PubScreenStatus`, `SA_PubSig`, `WA_PubScreenName`); el mapa del programa (`SA_BitMap`, `CUSTOMBITMAP`); y que lo
//! que se pinta en el mapa de la pantalla se vea (su superficie del compositor). Y lo hostil.

mod comun;

use caja_exec::estructuras::task;
use caja_exec::{Config, Exec, Memoria, MemoriaVec};
use caja_intuition::estructuras::{drawinfo as dri, screen, wa, window as win};
use caja_intuition::modos::{id, workbench};
use caja_intuition::lienzo::Rect;
use caja_intuition::{CajaGrafica, CompositorDePrueba, Numeros};
use comun::*;

fn caja_fhd() -> Caja {
    let mut e = Exec::arrancar(MemoriaVec::nueva(TAMANO as usize), Config::para(TAMANO)).expect("arranca");
    let t = e.crear_tarea("principal", 0, 4096, 0x10_0000).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(t));
    let g = CajaGrafica::instalar(&mut e, CompositorDePrueba::nuevo(1920, 1080), Numeros::default()).expect("instala");
    (e, g, t)
}

fn etiquetas(c: &mut Caja, t: &[(u32, u32)]) -> u32 {
    let d = reservar(c, 8 * t.len() as u32 + 8);
    for (i, (k, v)) in t.iter().enumerate() {
        e32(c, d + 8 * i as u32, *k);
        e32(c, d + 8 * i as u32 + 4, *v);
    }
    d
}

fn datos16(c: &mut Caja, v: &[u16]) -> u32 {
    let d = reservar(c, 2 * v.len() as u32);
    for (i, x) in v.iter().enumerate() {
        e16(c, d + 2 * i as u32, *x);
    }
    d
}

fn abrir(c: &mut Caja, t: &[(u32, u32)]) -> u32 {
    let l = etiquetas(c, t);
    i(c, "OpenScreenTagList", &[], &[0, l])
}

/// El color de la pluma `p` de un `ColorMap` (con `GetRGB32`).
fn color(c: &mut Caja, cm: u32, p: u32) -> u32 {
    let t = reservar(c, 12);
    g(c, "GetRGB32", &[p, 1], &[cm, t]);
    (l32(c, t) >> 24) << 16 | (l32(c, t + 4) >> 24) << 8 | l32(c, t + 8) >> 24
}

#[test]
fn paleta_y_plumas_de_su_pantalla() {
    let mut c = caja_fhd();
    // SA_Colors: la pluma 1, roja (0xF00); SA_Colors32: la 2, 0x123456; SA_Pens: SHINEPEN, la 2.
    let cs = datos16(&mut c, &[1, 15, 0, 0, 0xFFFF]);
    let t32 = reservar(&mut c, 20);
    e32(&mut c, t32, 1 << 16 | 2);
    e32(&mut c, t32 + 4, 0x1200_0000);
    e32(&mut c, t32 + 8, 0x3400_0000);
    e32(&mut c, t32 + 12, 0x5600_0000);
    let mut pens = vec![0u16, 1, 1, 2, 1, 3, 1, 0, 2, 1, 2, 1, 0xFFFF];
    pens[dri::SHINEPEN] = 2;
    let pl = datos16(&mut c, &pens);
    let s = abrir(&mut c, &[(0x8000_0032, id(2, 4)), (0x8000_0029, cs), (0x8000_0043, t32), (0x8000_003A, pl)]);
    assert_ne!(s, 0, "{:?}", c.1.avisos());
    let cm = l32(&c, s + screen::VIEWPORT + 4);
    assert_eq!((color(&mut c, cm, 1), color(&mut c, cm, 2)), (0xFF0000, 0x123456));
    let di = i(&mut c, "GetScreenDrawInfo", &[], &[s]);
    let tabla = l32(&c, di + dri::DRI_PENS);
    assert_eq!(l16(&c, tabla + 2 * dri::SHINEPEN as u32), 2);
    // El Workbench no cambia.
    let cm_wb = l32(&c, c.1.workbench() + screen::VIEWPORT + 4);
    assert_ne!(color(&mut c, cm_wb, 2), 0x123456);
    // Una ventana en ella: su marco (clásico) con el SHINEPEN de su pantalla, el 0x123456; su lienzo, su paleta.
    let tw = etiquetas(&mut c, &[(wa::WIDTH, 120), (wa::HEIGHT, 60), (wa::CUSTOMSCREEN, s), (wa::FLAGS, 0x0F)]);
    let w = i(&mut c, "OpenWindowTagList", &[], &[0, tw]);
    assert_ne!(w, 0, "{:?}", c.1.avisos());
    let bm = l32(&c, l32(&c, w + win::RPORT) + 4);
    assert_eq!(c.1.pixel_de(&c.0, bm, 0, 0).unwrap() & 0x00FF_FFFF, 0x123456);
    let rp = l32(&c, w + win::RPORT);
    pen(&mut c, rp, 1);
    rect(&mut c, rp, 20, 20, 30, 30);
    assert_eq!(c.1.pixel_de(&c.0, bm, 25, 25).unwrap() & 0x00FF_FFFF, 0xFF0000);
    i(&mut c, "CloseWindow", &[], &[w]);
    assert_eq!(i(&mut c, "CloseScreen", &[], &[s]), 1);
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}

#[test]
fn como_el_workbench() {
    let mut c = caja();
    let s = abrir(&mut c, &[(0x8000_0047, 1)]);
    assert_ne!(s, 0);
    assert_ne!(s, c.1.workbench());
    assert_eq!(g(&mut c, "GetVPModeID", &[], &[s + screen::VIEWPORT]), workbench());
    assert_eq!((l16(&c, s + screen::WIDTH), l16(&c, s + screen::HEIGHT)), (640, 480));
    assert_eq!(i(&mut c, "CloseScreen", &[], &[s]), 1);
}

#[test]
fn pantallas_publicas() {
    let mut c = caja_fhd();
    let nombre = cadena(&mut c, "MiPantalla");
    let otro = cadena(&mut c, "mipantalla");
    let s = abrir(&mut c, &[(0x8000_0032, id(1, 4)), (0x8000_002F, nombre), (0x8000_0030, 9)]);
    assert_ne!(s, 0);
    assert_ne!(l16(&c, s + screen::FLAGS) & 0x0002, 0, "PUBLICSCREEN");
    // Nace en modo privado: no se encuentra; abierta, sí (sin distinguir mayúsculas).
    assert_eq!(i(&mut c, "LockPubScreen", &[], &[otro]), 0);
    assert_eq!(i(&mut c, "PubScreenStatus", &[0], &[s]), 1);
    assert_eq!(i(&mut c, "LockPubScreen", &[], &[otro]), s);
    // Con un visitante no se cierra ni se pone en privado.
    assert_eq!(i(&mut c, "CloseScreen", &[], &[s]), 0);
    assert_eq!(i(&mut c, "PubScreenStatus", &[1], &[s]), 0);
    // Una ventana por WA_PubScreenName va a ella.
    let tw = etiquetas(&mut c, &[(wa::WIDTH, 80), (wa::HEIGHT, 40), (wa::PUBSCREENNAME, nombre)]);
    let w = i(&mut c, "OpenWindowTagList", &[], &[0, tw]);
    assert_eq!(l32(&c, w + win::WSCREEN), s);
    i(&mut c, "CloseWindow", &[], &[w]);
    // El último visitante se va: la señal 9 a la tarea que la abrió.
    let t = c.2;
    e32(&mut c, t + task::TC_SIGRECVD, 0);
    i(&mut c, "UnlockPubScreen", &[], &[0, s]);
    assert_ne!(l32(&c, t + task::TC_SIGRECVD) & (1 << 9), 0);
    // Otra con el mismo nombre: OSERR_PUBNOTUNIQUE.
    let error = reservar(&mut c, 4);
    assert_eq!(abrir(&mut c, &[(0x8000_0032, id(1, 4)), (0x8000_002F, otro), (0x8000_002A, error)]), 0);
    assert_eq!(l32(&c, error), 5);
    // En privado otra vez, y cerrada; el nombre queda libre.
    assert_eq!(i(&mut c, "PubScreenStatus", &[1], &[s]), 1);
    assert_eq!(i(&mut c, "LockPubScreen", &[], &[nombre]), 0);
    assert_eq!(i(&mut c, "CloseScreen", &[], &[s]), 1);
    let s2 = abrir(&mut c, &[(0x8000_0032, id(1, 4)), (0x8000_002F, otro)]);
    assert_ne!(s2, 0);
    // El Workbench no se toca con PubScreenStatus; una que no es pública, 0.
    let wb = c.1.workbench();
    assert_eq!(i(&mut c, "PubScreenStatus", &[1], &[wb]), 0);
    let s3 = abrir(&mut c, &[(0x8000_0032, id(1, 4))]);
    assert_eq!(i(&mut c, "PubScreenStatus", &[0], &[s3]), 0);
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}

/// Lo que se pinta en el mapa de la pantalla se ve: su superficie del compositor.
#[test]
fn se_ve_lo_de_su_mapa() {
    let mut c = caja_fhd();
    let s = abrir(&mut c, &[(0x8000_0032, id(1, 3))]);
    let sup = c.1.superficie_de_pantalla(s).expect("su superficie");
    assert_eq!((c.1.comp.ventana(sup).pedida.ancho, c.1.comp.ventana(sup).pedida.alto), (640, 480));
    let rp = s + screen::RASTPORT;
    let cm = l32(&c, s + screen::VIEWPORT + 4);
    pen(&mut c, rp, 3);
    rect(&mut c, rp, 100, 100, 200, 150);
    c.1.presentar(&c.0);
    let azul = color(&mut c, cm, 3);
    assert_eq!(c.1.comp.ventana(sup).pixel(150, 120) & 0x00FF_FFFF, azul);
    // Al cerrarla, su superficie también.
    assert_eq!(i(&mut c, "CloseScreen", &[], &[s]), 1);
    assert!(c.1.comp.ventana(sup).cerrada);
}

/// SA_BitMap (y CUSTOMBITMAP en la NewScreen): el mapa es el del programa, y también se ve.
#[test]
fn mapa_del_programa() {
    let mut c = caja_fhd();
    let (bm, rpm) = planar(&mut c, 4, 320, 200);
    let s = abrir(&mut c, &[(0x8000_0032, id(1, 0)), (0x8000_002E, bm), (0x8000_0023, 320), (0x8000_0024, 200)]);
    assert_ne!(s, 0, "{:?}", c.1.avisos());
    assert_eq!(l32(&c, s + screen::RASTPORT + 4), bm);
    assert_eq!(l32(&c, s + screen::BITMAP + 8), l32(&c, bm + 8), "una copia de su BitMap");
    pen(&mut c, rpm, 5);
    rect(&mut c, rpm, 10, 10, 20, 20);
    c.1.presentar(&c.0);
    let sup = c.1.superficie_de_pantalla(s).unwrap();
    let cm = l32(&c, s + screen::VIEWPORT + 4);
    let esperado = color(&mut c, cm, 5);
    assert_eq!(c.1.comp.ventana(sup).pixel(15, 15) & 0x00FF_FFFF, esperado);
    assert_eq!(i(&mut c, "CloseScreen", &[], &[s]), 1);
    // CUSTOMBITMAP en la NewScreen, con su extensión de etiquetas.
    let ext = etiquetas(&mut c, &[(0x8000_0032, id(1, 0))]);
    let nueva = reservar(&mut c, 40);
    e16(&mut c, nueva + 4, 320);
    e16(&mut c, nueva + 6, 200);
    e16(&mut c, nueva + 14, 0x000F | 0x0040 | 0x1000);
    e32(&mut c, nueva + 28, bm);
    e32(&mut c, nueva + 32, ext);
    let s2 = i(&mut c, "OpenScreen", &[], &[nueva]);
    assert_eq!(l32(&c, s2 + screen::RASTPORT + 4), bm);
    assert_eq!(i(&mut c, "CloseScreen", &[], &[s2]), 1);
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}

fn ultimo_dano(c: &Caja, sup: u32) -> (usize, Rect) {
    let d = &c.1.comp.ventana(sup).danos;
    (d.len(), *d.last().unwrap_or(&Rect::nuevo(0, 0, -1, -1)))
}

fn cgx(c: &mut Caja, nombre: &str, d: &[u32], a: &[u32]) -> u32 {
    let n = c.1.numero_cgx(nombre).expect("función de cybergraphics");
    let mut r = regs(d, a);
    c.1.llamar(&mut c.0, n, &mut r);
    r.d[0]
}

/// El mapa del programa se presenta solo donde cambia: lo que pinta graphics, lo que borra `BltClear` en sus planos y,
/// lo que el programa escribe a mano (sin llamadas), en cuanto se ve en un sondeo; desde entonces, en cada presentación,
/// hasta que deja de hacerlo.
#[test]
fn mapa_del_programa_solo_lo_que_cambia() {
    let mut c = caja_fhd();
    let (bm, rpm) = planar(&mut c, 4, 320, 200);
    let s = abrir(&mut c, &[(0x8000_0032, id(1, 0)), (0x8000_002E, bm), (0x8000_0023, 320), (0x8000_0024, 200)]);
    let sup = c.1.superficie_de_pantalla(s).unwrap();
    let cm = l32(&c, s + screen::VIEWPORT + 4);
    // La primera vez, entero; sin cambios, nada.
    c.1.presentar(&c.0);
    let (n, d) = ultimo_dano(&c, sup);
    assert_eq!(d, Rect::nuevo(0, 0, 319, 199));
    c.1.presentar(&c.0);
    assert_eq!(ultimo_dano(&c, sup).0, n, "nada que presentar");
    // graphics: solo lo pintado.
    pen(&mut c, rpm, 5);
    rect(&mut c, rpm, 10, 10, 20, 20);
    c.1.presentar(&c.0);
    assert_eq!(ultimo_dano(&c, sup), (n + 1, Rect::nuevo(10, 10, 20, 20)));
    assert_eq!(c.1.comp.ventana(sup).pixel(15, 15) & 0x00FF_FFFF, color(&mut c, cm, 5));
    // BltClear en sus planos: esas filas.
    let bpr = l16(&c, bm) as u32;
    let plano0 = l32(&c, bm + 8);
    g(&mut c, "BltClear", &[2 * bpr, 0], &[0, plano0 + 12 * bpr]);
    c.1.presentar(&c.0);
    assert_eq!(ultimo_dano(&c, sup), (n + 2, Rect::nuevo(0, 12, 319, 13)));
    assert_eq!(c.1.comp.ventana(sup).pixel(15, 12) & 0x00FF_FFFF, color(&mut c, cm, 4), "sin el plano 0, la 4");
    // A mano, sin llamadas: no se ve hasta el sondeo...
    c.0.mem.leer_mut(plano0 + 100 * bpr, 1).unwrap()[0] = 0xFF;
    c.1.presentar(&c.0);
    assert_eq!(ultimo_dano(&c, sup).0, n + 2);
    assert_eq!(c.1.escrituras_directas(s), Some((0, false)));
    c.1.poner_hora(1, 0);
    c.1.presentar(&c.0);
    assert_eq!(ultimo_dano(&c, sup), (n + 3, Rect::nuevo(0, 100, 319, 100)));
    assert_eq!(c.1.comp.ventana(sup).pixel(3, 100) & 0x00FF_FFFF, color(&mut c, cm, 1));
    assert_eq!(c.1.escrituras_directas(s), Some((1, true)));
    // ... y desde entonces, en cada presentación (solo sus filas).
    c.0.mem.leer_mut(plano0 + 150 * bpr + 2, 1).unwrap()[0] = 0x80;
    c.1.presentar(&c.0);
    assert_eq!(ultimo_dano(&c, sup), (n + 4, Rect::nuevo(0, 150, 319, 150)));
    assert_eq!(c.1.comp.ventana(sup).pixel(16, 150) & 0x00FF_FFFF, color(&mut c, cm, 1));
    // Sin más, deja de mirar en cada presentación (y no presenta nada).
    for _ in 0..20 {
        c.1.presentar(&c.0);
    }
    assert_eq!(c.1.escrituras_directas(s), Some((2, false)));
    assert_eq!(ultimo_dano(&c, sup).0, n + 4);
    assert_eq!(i(&mut c, "CloseScreen", &[], &[s]), 1);
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}

/// Un mapa de color verdadero del programa: lo que escribe cybergraphics, y lo que escribe el programa tras
/// `LockBitMap` (al soltarlo: lo que dice `UBMI_UPDATERECTS`).
#[test]
fn mapa_del_programa_de_color_verdadero() {
    let mut c = caja_fhd();
    let (bm, rpm) = nativo(&mut c, 160, 100);
    let s = abrir(&mut c, &[(0x8000_0032, id(1, 0)), (0x8000_002E, bm), (0x8000_0023, 160), (0x8000_0024, 100)]);
    assert_ne!(s, 0, "{:?}", c.1.avisos());
    let sup = c.1.superficie_de_pantalla(s).unwrap();
    c.1.presentar(&c.0);
    let n = ultimo_dano(&c, sup).0;
    // FillPixelArray(rp, x, y, w, h, argb)(a1/d0/d1/d2/d3/d4).
    cgx(&mut c, "FillPixelArray", &[30, 40, 5, 6, 0x00_12_34_56], &[0, rpm]);
    c.1.presentar(&c.0);
    assert_eq!(ultimo_dano(&c, sup), (n + 1, Rect::nuevo(30, 40, 34, 45)));
    assert_eq!(c.1.comp.ventana(sup).pixel(32, 42) & 0x00FF_FFFF, 0x123456);
    // LockBitMap, a mano, y UnLockBitMapTagList con UBMI_UPDATERECTS.
    let base = cgx(&mut c, "LockBitMapTagList", &[], &[bm, 0]);
    assert_ne!(base, 0);
    let bpr = cgx(&mut c, "GetCyberMapAttr", &[0x8000_0001], &[bm]);
    e32(&mut c, base + 70 * bpr + 4 * 80, 0x00AB_CDEF);
    let rl = reservar(&mut c, 16);
    e32(&mut c, rl, 1);
    e32(&mut c, rl + 4, 0);
    for (k, v) in [80u16, 70, 80, 70].iter().enumerate() {
        e16(&mut c, rl + 8 + 2 * k as u32, *v);
    }
    let t = etiquetas(&mut c, &[(0x8500_1001, rl)]);
    cgx(&mut c, "UnLockBitMapTagList", &[], &[base, t]);
    c.1.presentar(&c.0);
    assert_eq!(ultimo_dano(&c, sup), (n + 2, Rect::nuevo(80, 70, 80, 70)));
    assert_eq!(c.1.comp.ventana(sup).pixel(80, 70) & 0x00FF_FFFF, 0xABCDEF);
    assert_eq!(c.1.escrituras_directas(s), Some((0, false)));
    assert_eq!(i(&mut c, "CloseScreen", &[], &[s]), 1);
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}

#[test]
fn pantallas_propias_hostiles() {
    let mut c = caja_fhd();
    // Un SA_Colors que se sale de la memoria, un SA_BitMap que no es un mapa: 0, aviso (y nada reservado). Un nombre
    // público vacío: 0 (no vale), sin aviso.
    let antes = c.0.bloques_de(0).len();
    assert_eq!(abrir(&mut c, &[(0x8000_0032, id(1, 4)), (0x8000_0029, TAMANO - 4)]), 0);
    assert_eq!(abrir(&mut c, &[(0x8000_0032, id(1, 4)), (0x8000_002E, TAMANO - 8)]), 0);
    assert_eq!(abrir(&mut c, &[(0x8000_0032, id(1, 4)), (0x8000_002F, TAMANO - 1)]), 0);
    assert_eq!(c.1.tomar_avisos().len(), 2);
    assert_eq!(c.0.bloques_de(0).len(), antes);
    assert_eq!(c.1.pantallas_propias(), 0);
    // Una tabla SA_Colors32 sin fin no cuelga (64 tramos como mucho); SA_Pens sin ~0, solo las que caben.
    let t = reservar(&mut c, 4096);
    for k in 0..256 {
        e32(&mut c, t + 16 * k, 1 << 16);
    }
    let pens = datos16(&mut c, &[3; 40]);
    let s = abrir(&mut c, &[(0x8000_0032, id(1, 4)), (0x8000_0043, t), (0x8000_003A, pens)]);
    assert_ne!(s, 0);
    assert_eq!(i(&mut c, "CloseScreen", &[], &[s]), 1);
    // UnlockPubScreen de más, PubScreenStatus de lo que no es una pantalla.
    let wb = c.1.workbench();
    i(&mut c, "UnlockPubScreen", &[], &[0, wb]);
    i(&mut c, "PubScreenStatus", &[0], &[0x4000]);
    assert_eq!(c.1.tomar_avisos().len(), 2);
}
