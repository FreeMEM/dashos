//! Los modos de pantalla RTG de la caja (`modos.rs`) y las pantallas propias (`pantallas.rs`): la base de datos de
//! pantallas (`FindDisplayInfo`, `NextDisplayInfo`, `GetDisplayInfoData`, `ModeNotAvailable`), elegir modo
//! (`BestModeIDA`, `BestCModeIDTagList`, `p96BestModeIDTagList`), sus atributos, `OpenScreenTagList` con un `ModeID` de
//! la caja (su mapa en el formato del modo, sus ventanas, `CloseScreen`) y lo hostil. Con un escritorio de 1920 x 1080.

mod comun;

use caja_exec::{Config, Exec, MemoriaVec};
use caja_intuition::estructuras::{drawinfo as dri, intuitionbase as ib, screen, wa, window as win};
use caja_intuition::modos::{INVALID_ID, MONITOR, id, workbench};
use caja_intuition::{CajaGrafica, CompositorDePrueba, Numeros};
use comun::*;

fn caja_fhd() -> Caja {
    let mut e = Exec::arrancar(MemoriaVec::nueva(TAMANO as usize), Config::para(TAMANO)).expect("arranca");
    let t = e.crear_tarea("principal", 0, 4096, 0x10_0000).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(t));
    let g = CajaGrafica::instalar(&mut e, CompositorDePrueba::nuevo(1920, 1080), Numeros::default()).expect("instala");
    (e, g, t)
}

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

fn etiquetas(c: &mut Caja, t: &[(u32, u32)]) -> u32 {
    let d = reservar(c, 8 * t.len() as u32 + 8);
    for (i, (k, v)) in t.iter().enumerate() {
        e32(c, d + 8 * i as u32, *k);
        e32(c, d + 8 * i as u32 + 4, *v);
    }
    d
}

/// GetDisplayInfoData(handle, buf, size, tag, id)(a0/a1/d0/d1/d2) en un búfer nuevo.
fn info(c: &mut Caja, tag: u32, modo: u32) -> (u32, u32) {
    let buf = reservar(c, 128);
    let n = g(c, "GetDisplayInfoData", &[128, tag, modo], &[0, buf]);
    (buf, n)
}

#[test]
fn base_de_datos_de_pantallas() {
    let mut c = caja_fhd();
    // El Workbench, en el del escritorio a 32 bits; es de la caja (RTG), los del chipset no.
    let vp = c.1.workbench() + screen::VIEWPORT;
    assert_eq!(g(&mut c, "GetVPModeID", &[], &[vp]), workbench());
    assert_eq!(workbench() & 0xFFFF_1000, MONITOR);
    assert_eq!(x(&mut c, "IsCyberModeID", &[workbench()], &[]), 1);
    for chipset in [0x0000_0000, 0x0002_9000, 0x0001_9004, 0x0003_1000] {
        assert_eq!(x(&mut c, "IsCyberModeID", &[chipset], &[]), 0);
        assert_eq!(g(&mut c, "FindDisplayInfo", &[chipset], &[]), 0);
        assert_eq!(g(&mut c, "ModeNotAvailable", &[chipset], &[]), 2);
    }
    // NextDisplayInfo: todos los de la caja, en orden; los que no caben en 1920 x 1080 (1600 x 1200) no salen, ni el de
    // 1920 x 1080 de la lista (es el del escritorio).
    let mut v = Vec::new();
    let mut m = INVALID_ID;
    loop {
        m = g(&mut c, "NextDisplayInfo", &[m], &[]);
        if m == INVALID_ID {
            break;
        }
        v.push(m);
        assert!(v.len() <= 100);
    }
    assert_eq!(v.len(), 6 * 5);
    assert!(v.windows(2).all(|w| w[0] < w[1]));
    assert!(v.contains(&id(2, 2)) && !v.contains(&id(6, 0)) && !v.contains(&id(7, 0)));
    for &m in &v {
        assert_eq!(g(&mut c, "FindDisplayInfo", &[m], &[]), m);
        assert_eq!(g(&mut c, "ModeNotAvailable", &[m], &[]), 0);
        assert_eq!(x(&mut c, "IsCyberModeID", &[m], &[]), 1);
    }
    // GetDisplayInfoData de 800 x 600 a 16 bits: su profundidad, su tamaño, 5-6-5 bits por color, extranjero, su nombre.
    let m = id(2, 2);
    let (buf, n) = info(&mut c, 0x8000_1000, m);
    assert_eq!(n, 88);
    assert_eq!(l16(&c, buf + 16), 16, "MaxDepth");
    assert_eq!((l16(&c, buf + 26 + 4), l16(&c, buf + 26 + 6)), (799, 599), "Nominal");
    let (buf, _) = info(&mut c, 0x8000_0000, m);
    assert_ne!(l32(&c, buf + 18) & 0x8000_0000, 0, "DIPF_IS_FOREIGN");
    assert_eq!((l8(&c, buf + 40), l8(&c, buf + 41), l8(&c, buf + 42)), (5, 6, 5));
    let (buf, _) = info(&mut c, 0x8000_3000, m);
    let nombre: Vec<u8> = (0..23).map(|i| l8(&c, buf + 16 + i)).collect();
    assert_eq!(&nombre, b"DashOS: 800x600 16 bits");
    // Con el asa de FindDisplayInfo en vez del ID.
    let b2 = reservar(&mut c, 128);
    assert_eq!(g(&mut c, "GetDisplayInfoData", &[128, 0x8000_1000, 0], &[m, b2]), 88);
    assert_eq!(l16(&c, b2 + 16), 16);
    // Los atributos de cybergraphics y de Picasso96.
    assert_eq!(x(&mut c, "GetCyberIDAttr", &[0x8000_0001, id(0, 0)], &[]), 0, "PIXFMT_LUT8");
    assert_eq!(x(&mut c, "GetCyberIDAttr", &[0x8000_0005, id(0, 0)], &[]), 1);
    assert_eq!(x(&mut c, "GetCyberIDAttr", &[0x8000_0001, id(3, 3)], &[]), 9, "RGB24");
    assert_eq!(x(&mut c, "GetCyberIDAttr", &[0x8000_0002, id(3, 3)], &[]), 1024);
    assert_eq!(p(&mut c, "p96GetModeIDAttr", &[id(1, 1), 5], &[]), 11, "R5G5B5");
    assert_eq!(p(&mut c, "p96GetModeIDAttr", &[id(1, 4), 2], &[]), 32);
    assert_eq!(p(&mut c, "p96GetModeIDAttr", &[id(1, 4), 4], &[]), 24);
    assert_eq!(p(&mut c, "p96GetModeIDAttr", &[0x0002_9000, 6], &[]), 0, "ISP96 de uno del chipset");
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
}

#[test]
fn elegir_modo() {
    let mut c = caja_fhd();
    let best = |c: &mut Caja, t: &[(u32, u32)]| {
        let l = etiquetas(c, t);
        g(c, "BestModeIDA", &[], &[l])
    };
    // La profundidad más pequeña que llega y el tamaño más pequeño que llega.
    assert_eq!(best(&mut c, &[(0x8000_0008, 16), (0x8000_0004, 800), (0x8000_0005, 600)]), id(2, 2));
    assert_eq!(best(&mut c, &[(0x8000_0008, 9), (0x8000_0006, 700), (0x8000_0007, 500)]), id(2, 1));
    assert_eq!(best(&mut c, &[(0x8000_0008, 24), (0x8000_0006, 1200), (0x8000_0007, 1000)]), id(5, 3));
    // Sin tamaño, el del escritorio; más grande que todo, el más grande.
    assert_eq!(best(&mut c, &[(0x8000_0008, 32)]), id(0, 4));
    assert_eq!(best(&mut c, &[(0x8000_0008, 8), (0x8000_0006, 4000), (0x8000_0007, 3000)]), id(0, 0));
    // Los bits por color y el monitor: 6 de verde descartan los de 15 bits; un monitor del chipset, ninguno.
    assert_eq!(best(&mut c, &[(0x8000_0008, 15), (0x8000_000D, 6)]), id(0, 2));
    assert_eq!(best(&mut c, &[(0x8000_0009, 0x0002_1000)]), INVALID_ID);
    assert_eq!(best(&mut c, &[(0x8000_0008, 33)]), INVALID_ID);
    // Como el de otro modo (BIDTAG_SourceID) o el de un ViewPort.
    assert_eq!(best(&mut c, &[(0x8000_000A, id(4, 4)), (0x8000_0008, 16)]), id(4, 2));
    let vp = c.1.workbench() + screen::VIEWPORT;
    assert_eq!(best(&mut c, &[(0x8000_0003, vp), (0x8000_0008, 8)]), id(0, 0));
    // Las DIPF_: las de la caja se tienen; otras (DIPF_IS_LACE), no.
    assert_eq!(best(&mut c, &[(0x8000_0001, 0x8000_0000), (0x8000_0008, 24)]), id(0, 3));
    assert_eq!(best(&mut c, &[(0x8000_0001, 0x0000_0001)]), INVALID_ID);
    assert_eq!(best(&mut c, &[(0x8000_0002, 0x8000_0000)]), INVALID_ID);
    // cybergraphics y Picasso96, lo mismo.
    let t = etiquetas(&mut c, &[(0x8005_0000, 15), (0x8005_0001, 1280), (0x8005_0002, 720)]);
    assert_eq!(x(&mut c, "BestCModeIDTagList", &[], &[t]), id(4, 1));
    let t = etiquetas(&mut c, &[(0x8000_0065, 16), (0x8000_0063, 640), (0x8000_0064, 480), (0x8000_0061, 1 << 6)]);
    assert_eq!(p(&mut c, "p96BestModeIDTagList", &[], &[t]), id(1, 4), "solo A8R8G8B8: el de 32 bits");
    let t = etiquetas(&mut c, &[(0x8000_0062, 0xFFFF)]);
    assert_eq!(p(&mut c, "p96BestModeIDTagList", &[], &[t]), INVALID_ID);
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
}

/// OpenScreenTagList con un modo de la caja: una pantalla de verdad, con su mapa en el formato del modo y sus ventanas.
#[test]
fn pantalla_propia() {
    let mut c = caja_fhd();
    let antes = c.0.bloques_de(0).len();
    let wb = c.1.workbench();
    let titulo = cadena(&mut c, "Mi pantalla");
    let t = etiquetas(&mut c, &[(0x8000_0032, id(2, 2)), (0x8000_0028, titulo)]);
    let s = i(&mut c, "OpenScreenTagList", &[], &[0, t]);
    assert_ne!(s, 0, "{:?}", c.1.avisos());
    assert_ne!(s, wb);
    assert_eq!(c.1.pantallas_propias(), 1);
    assert_eq!((l16(&c, s + screen::WIDTH), l16(&c, s + screen::HEIGHT)), (800, 600));
    assert_eq!(l32(&c, c.1.intuition_base() + ib::FIRSTSCREEN), s, "delante");
    assert_eq!(l32(&c, s + screen::NEXTSCREEN), wb);
    assert_eq!(g(&mut c, "GetVPModeID", &[], &[s + screen::VIEWPORT]), id(2, 2));
    let di = i(&mut c, "GetScreenDrawInfo", &[], &[s]);
    assert_eq!(l16(&c, di + dri::DRI_DEPTH), 16);
    // Su mapa: chunky RGB16 en la memoria del 68k, sin copia al cogerlo; los amigos, en su formato.
    let bm = s + screen::BITMAP;
    assert_eq!(l32(&c, s + screen::RASTPORT + 4), bm);
    assert_eq!(x(&mut c, "GetCyberMapAttr", &[0x8000_0004], &[bm]), 5);
    assert_eq!(g(&mut c, "GetBitMapAttr", &[0, 4], &[bm]), 16);
    let h = x(&mut c, "LockBitMapTagList", &[], &[bm, 0]);
    assert_eq!(h, x(&mut c, "GetCyberMapAttr", &[0x8000_0003], &[bm]));
    x(&mut c, "UnLockBitMap", &[], &[h]);
    let amigo = g(&mut c, "AllocBitMap", &[10, 10, 8, 0], &[bm]);
    assert_eq!(x(&mut c, "GetCyberMapAttr", &[0x8000_0004], &[amigo]), 5);
    g(&mut c, "FreeBitMap", &[], &[amigo]);
    // Una ventana en ella: su pantalla, su ViewPort, su lienzo visto en RGB16.
    let tw = etiquetas(&mut c, &[(wa::WIDTH, 200), (wa::HEIGHT, 100), (wa::CUSTOMSCREEN, s)]);
    let w = i(&mut c, "OpenWindowTagList", &[], &[0, tw]);
    assert_ne!(w, 0, "{:?}", c.1.avisos());
    assert_eq!(l32(&c, w + win::WSCREEN), s);
    assert_eq!(l32(&c, s + screen::FIRSTWINDOW), w);
    assert_eq!(i(&mut c, "ViewPortAddress", &[], &[w]), s + screen::VIEWPORT);
    let rpw = l32(&c, w + win::RPORT);
    let bmw = l32(&c, rpw + 4);
    assert_eq!(x(&mut c, "GetCyberMapAttr", &[0x8000_0004], &[bmw]), 5);
    // Con la ventana abierta, CloseScreen dice que no; cerrada, la suelta y vuelve el Workbench delante.
    assert_eq!(i(&mut c, "CloseScreen", &[], &[s]), 0);
    i(&mut c, "CloseWindow", &[], &[w]);
    assert_eq!(l32(&c, s + screen::FIRSTWINDOW), 0);
    assert_eq!(i(&mut c, "CloseScreen", &[], &[s]), 1);
    assert_eq!(c.1.pantallas_propias(), 0);
    assert_eq!(l32(&c, c.1.intuition_base() + ib::FIRSTSCREEN), wb);
    assert_eq!(c.0.bloques_de(0).len(), antes, "todo suelto");
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
    // Una de 8 bits: chunky LUT8 (de cybergraphics, una pluma por byte), y su modo también.
    let t = etiquetas(&mut c, &[(0x8000_0032, id(1, 0))]);
    let s8 = i(&mut c, "OpenScreenTagList", &[], &[0, t]);
    assert_ne!(s8, 0);
    assert_eq!(g(&mut c, "GetBitMapAttr", &[0, 4], &[s8 + screen::BITMAP]), 8);
    assert_eq!(x(&mut c, "GetCyberMapAttr", &[0x8000_0008], &[s8 + screen::BITMAP]), u32::MAX);
    assert_eq!(x(&mut c, "GetCyberMapAttr", &[0x8000_0004], &[s8 + screen::BITMAP]), 0, "PIXFMT_LUT8");
    assert_eq!(x(&mut c, "GetCyberMapAttr", &[0x8000_0001], &[s8 + screen::BITMAP]), 640, "un byte por punto");
    let m8 = g(&mut c, "GetVPModeID", &[], &[s8 + screen::VIEWPORT]);
    assert_eq!(x(&mut c, "IsCyberModeID", &[m8], &[]), 1);
    // Una con SA_Width/SA_Height propios, y otra con NewScreen y su extensión de etiquetas.
    let t = etiquetas(&mut c, &[(0x8000_0032, id(0, 3)), (0x8000_0023, 320), (0x8000_0024, 200)]);
    let s2 = i(&mut c, "OpenScreenTagList", &[], &[0, t]);
    assert_eq!((l16(&c, s2 + screen::WIDTH), l16(&c, s2 + screen::HEIGHT)), (320, 200));
    let ext = etiquetas(&mut c, &[(0x8000_0032, id(3, 4))]);
    let nueva = reservar(&mut c, 40);
    e16(&mut c, nueva + 14, 0x000F | 0x1000);
    e32(&mut c, nueva + 32, ext);
    let s3 = i(&mut c, "OpenScreen", &[], &[nueva]);
    assert_eq!(g(&mut c, "GetVPModeID", &[], &[s3 + screen::VIEWPORT]), id(3, 4));
    assert_eq!(c.1.pantallas_propias(), 3);
    // La tarea acaba sin cerrarlas: se cierran.
    let tarea = c.2;
    c.1.tarea_muerta(&mut c.0, tarea);
    assert_eq!(c.1.pantallas_propias(), 0);
    assert_eq!(l32(&c, c.1.intuition_base() + ib::FIRSTSCREEN), wb);
    assert_eq!(c.0.bloques_de(0).len(), antes);
}

/// Sin modo de la caja (uno del chipset o ninguno), la de Workbench, como antes; lo hostil.
#[test]
fn pantallas_hostiles() {
    let mut c = caja_fhd();
    let wb = c.1.workbench();
    let t = etiquetas(&mut c, &[(0x8000_0032, 0x0002_9000)]);
    assert_eq!(i(&mut c, "OpenScreenTagList", &[], &[0, t]), wb);
    assert_eq!(i(&mut c, "OpenScreenTagList", &[], &[0, 0]), wb);
    assert_eq!(c.1.bloqueos_workbench(), 2);
    assert_eq!(i(&mut c, "CloseScreen", &[], &[wb]), 1);
    assert_eq!(i(&mut c, "CloseScreen", &[], &[wb]), 1);
    assert_eq!(c.1.bloqueos_workbench(), 0);
    // Un ModeID del monitor de la caja que no está (más grande que el escritorio, una profundidad que no hay, bits
    // de más): 0 y OSERR_UNKNOWNMODE.
    let error = reservar(&mut c, 4);
    for m in [id(6, 4), id(0, 5), id(0, 0) | 0x100, id(15, 0)] {
        e32(&mut c, error, 0);
        let t = etiquetas(&mut c, &[(0x8000_0032, m), (0x8000_002A, error)]);
        assert_eq!(i(&mut c, "OpenScreenTagList", &[], &[0, t]), 0, "{:x}", m);
        assert_eq!(l32(&c, error), 6);
    }
    assert_eq!(c.1.pantallas_propias(), 0);
    assert!(c.1.tomar_avisos().is_empty());
    // NewScreen fuera de la memoria, una ventana en una pantalla que no existe, cerrar lo que no es una pantalla.
    assert_eq!(i(&mut c, "OpenScreen", &[], &[TAMANO - 8]), 0);
    let tw = etiquetas(&mut c, &[(wa::WIDTH, 50), (wa::HEIGHT, 50), (wa::CUSTOMSCREEN, 0x4000)]);
    assert_eq!(i(&mut c, "OpenWindowTagList", &[], &[0, tw]), 0);
    assert_eq!(i(&mut c, "CloseScreen", &[], &[0x4000]), 0);
    assert_eq!(c.1.tomar_avisos().len(), 3);
}
