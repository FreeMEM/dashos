//! layers.library y keymap.library nativas (`capas.rs`): lo que hacen, lo que aún no y lo malo (punteros nulos,
//! búferes cortos o fuera de la memoria), sin tumbar nada.

mod comun;

use caja_exec::Memoria;
use caja_intuition::Fallo;
use comun::*;

fn k(c: &mut Caja, nombre: &str, d: &[u32], a: &[u32]) -> u32 {
    let n = c.1.numero_capas(nombre).expect("función de layers o keymap");
    let mut r = regs(d, a);
    c.1.llamar(&mut c.0, n, &mut r);
    r.d[0]
}

/// Un `InputEvent` de tecla (o de otra clase).
fn evento(c: &mut Caja, clase: u8, code: u16, qual: u16) -> u32 {
    let ev = reservar(c, 22);
    c.0.mem.escribir8(ev + 4, clase).unwrap();
    e16(c, ev + 6, code);
    e16(c, ev + 8, qual);
    ev
}

fn bytes(c: &Caja, d: u32, n: u32) -> Vec<u8> {
    (0..n).map(|i| l8(c, d + i)).collect()
}

#[test]
fn estan_en_la_lista() {
    let c = caja();
    assert!(c.0.esta_en_memoria(b"layers.library"));
    assert!(c.0.esta_en_memoria(b"keymap.library"));
    let (l, km) = c.1.capas_bases();
    assert!(l != 0 && km != 0 && l != km);
}

#[test]
fn map_raw_key() {
    let mut c = caja();
    let buf = reservar(&mut c, 16);
    let casos: &[(u16, u16, &[u8])] = &[
        (0x20, 0, b"a"),
        (0x20, 0x0001, b"A"),
        (0x20, 0x0004, b"A"), // Bloq Mayús
        (0x01, 0, b"1"),
        (0x01, 0x0002, b"!"),
        (0x40, 0, b" "),
        (0x44, 0, b"\r"),
        (0x4C, 0, b"\x9bA"),
        (0x4F, 0x0001, b"\x9b A"),
        (0x50, 0, b"\x9b0~"),
        (0x59, 0x0001, b"\x9b19~"),
        (0x5F, 0, b"\x9b?~"),
        (0x20, 0x0008, b"\x01"), // Control
    ];
    for &(code, q, esperado) in casos {
        let ev = evento(&mut c, 1, code, q);
        let n = k(&mut c, "MapRawKey", &[0, 16], &[ev, buf, 0]);
        assert_eq!(n as usize, esperado.len(), "tecla {code:#x} con {q:#x}");
        assert_eq!(bytes(&c, buf, n), esperado, "tecla {code:#x} con {q:#x}");
    }
    // Soltar una tecla, otra clase de evento y una tecla que no escribe (Mayúsculas): nada.
    for (clase, code) in [(1u8, 0xA0u16), (2, 0x68), (1, 0x60)] {
        let ev = evento(&mut c, clase, code, 0);
        assert_eq!(k(&mut c, "MapRawKey", &[0, 16], &[ev, buf, 0]), 0);
    }
    // Un búfer corto: -1.
    let ev = evento(&mut c, 1, 0x4C, 0);
    assert_eq!(k(&mut c, "MapRawKey", &[0, 1], &[ev, buf, 0]), u32::MAX);
    assert!(c.1.tomar_avisos().is_empty());
}

#[test]
fn map_ansi_y_su_keymap() {
    let mut c = caja();
    let s = cadena(&mut c, "Hi!");
    let buf = reservar(&mut c, 16);
    assert_eq!(k(&mut c, "MapANSI", &[3, 16], &[s, buf, 0]), 3);
    assert_eq!(bytes(&c, buf, 6), [0x25, 1, 0x17, 0, 0x01, 1]);
    // Sin sitio: -1. Un carácter que no está en el teclado: 0.
    assert_eq!(k(&mut c, "MapANSI", &[3, 3], &[s, buf, 0]), u32::MAX);
    let raro = cadena(&mut c, "\u{7}");
    assert_eq!(k(&mut c, "MapANSI", &[1, 16], &[raro, buf, 0]), 0);
    // La KeyMap de la caja: la "a" (0x20) es KCF_SHIFT con "A" y "a", y cambia con Bloq Mayús.
    let km = k(&mut c, "AskKeyMapDefault", &[], &[]);
    assert_ne!(km, 0);
    let (tipos, mapa, caps) = (l32(&c, km), l32(&c, km + 4), l32(&c, km + 8));
    assert_eq!(l8(&c, tipos + 0x20), 1);
    assert_eq!(l32(&c, mapa + 0x20 * 4), u32::from_be_bytes([0, 0, b'A', b'a']));
    assert_ne!(l8(&c, caps + 4) & 1, 0);
    // El espacio (0x40, de las altas) no cambia con Mayúsculas.
    let (tipos_hi, mapa_hi) = (l32(&c, km + 16), l32(&c, km + 20));
    assert_eq!(l8(&c, tipos_hi), 0);
    assert_eq!(l32(&c, mapa_hi) & 0xFF, b' ' as u32);
    // SetKeyMapDefault con otra: AskKeyMapDefault la devuelve; con 0, no cambia.
    let otra = reservar(&mut c, 32);
    k(&mut c, "SetKeyMapDefault", &[], &[otra]);
    assert_eq!(k(&mut c, "AskKeyMapDefault", &[], &[]), otra);
    k(&mut c, "SetKeyMapDefault", &[], &[0]);
    assert_eq!(k(&mut c, "AskKeyMapDefault", &[], &[]), otra);
}

#[test]
fn layers() {
    let mut c = caja();
    let capa = reservar(&mut c, 200);
    let region = reservar(&mut c, 16);
    assert_eq!(k(&mut c, "InstallClipRegion", &[], &[capa, region]), 0);
    assert_eq!(k(&mut c, "InstallClipRegion", &[], &[capa, 0]), region);
    let li = k(&mut c, "NewLayerInfo", &[], &[]);
    assert_ne!(li, 0);
    assert_eq!(k(&mut c, "FattenLayerInfo", &[], &[li]), 1);
    for f in ["LockLayerInfo", "UnlockLayerInfo", "LockLayers", "UnlockLayers", "ThinLayerInfo"] {
        assert_eq!(k(&mut c, f, &[], &[li]), 0, "{f}");
    }
    k(&mut c, "LockLayer", &[], &[0, capa]);
    k(&mut c, "UnlockLayer", &[], &[capa]);
    assert_eq!(k(&mut c, "LayerOccluded", &[], &[capa]), 0);
    assert_eq!(k(&mut c, "BeginUpdate", &[], &[capa]), 1);
    k(&mut c, "DisposeLayerInfo", &[], &[li]);
    k(&mut c, "DisposeLayerInfo", &[], &[0]);
    assert!(c.1.tomar_avisos().is_empty());
}

#[test]
fn hostil() {
    let mut c = caja();
    let buf = reservar(&mut c, 16);
    let fuera = 0xFFF0_0000;
    // Punteros nulos o fuera de la memoria: un aviso y 0, sin tumbar nada.
    assert_eq!(k(&mut c, "MapRawKey", &[0, 16], &[0, buf, 0]), 0);
    assert_eq!(k(&mut c, "MapRawKey", &[0, 16], &[fuera, buf, 0]), 0);
    let ev = evento(&mut c, 1, 0x20, 0);
    assert_eq!(k(&mut c, "MapRawKey", &[0, 16], &[ev, fuera, 0]), 0);
    // Un largo negativo: no cabe nada (-1), sin escribir.
    assert_eq!(k(&mut c, "MapRawKey", &[0, 0xFFFF_FFFF], &[ev, 0, 0]), u32::MAX);
    assert_eq!(k(&mut c, "MapANSI", &[u32::MAX, 1 << 30], &[fuera, buf, 0]), 0);
    assert_eq!(k(&mut c, "InstallClipRegion", &[], &[0, 0]), 0);
    assert_eq!(k(&mut c, "InstallClipRegion", &[], &[fuera, 0]), 0);
    // Lo que aún no está: NoImplementada, 0.
    assert_eq!(k(&mut c, "MoveLayer", &[1, 1], &[0, 0]), 0);
    assert_eq!(k(&mut c, "DoHookClipRects", &[], &[0, 0, 0]), 0);
    let avisos = c.1.tomar_avisos();
    assert_eq!(avisos.len(), 8, "{avisos:?}");
    assert!(matches!(avisos[6].fallo, Fallo::NoImplementada { .. }));
    assert_eq!(avisos[7].funcion, "DoHookClipRects");
}

/// Capas sueltas (Tachyon pinta así fuera de pantalla): `CreateUpfrontLayer` en un mapa, su `RastPort` con el (0, 0) en
/// la esquina de la capa y recortado a ella, borrada con la pluma 0 al crearla (no con `LAYERS_NOBACKFILL`),
/// `DeleteLayer`; `WhichLayer`. Lo hostil: mapa nulo o que no es un mapa, rectángulos al revés o negativos, borrar dos
/// veces o lo que no es una capa, y la tarea que acaba sin borrarlas.
#[test]
fn capas_sueltas() {
    use caja_intuition::estructuras::{cliprect, layer, rastport};
    let mut c = caja();
    let antes = c.0.bloques_de(0).len();
    let li = k(&mut c, "NewLayerInfo", &[], &[]);
    let (bm, rpm) = planar(&mut c, 2, 32, 16);
    pen(&mut c, rpm, 3);
    rect(&mut c, rpm, 0, 0, 31, 15);
    // Una capa de (8, 4) a (23, 11): se borra con la pluma 0.
    let capa = k(&mut c, "CreateUpfrontLayer", &[8, 4, 23, 11, 1], &[li, bm, 0]);
    assert_ne!(capa, 0, "{:?}", c.1.avisos());
    let rp = l32(&c, capa + layer::RP);
    assert_eq!(l32(&c, rp + rastport::LAYER), capa);
    assert_eq!(l32(&c, rp + rastport::BITMAP), bm);
    assert_eq!(l32(&c, capa + layer::LAYERINFO), li);
    assert_eq!((l16(&c, capa + layer::WIDTH), l16(&c, capa + layer::HEIGHT)), (16, 8));
    let cr = l32(&c, capa + layer::CLIPRECT);
    assert_eq!((l16(&c, cr + cliprect::BOUNDS), l16(&c, cr + cliprect::BOUNDS + 6)), (8, 11));
    assert_ne!(l32(&c, capa + layer::DAMAGELIST), 0);
    assert_eq!(pluma(&mut c, rpm, 8, 4), 0);
    assert_eq!(pluma(&mut c, rpm, 7, 4), 3);
    // Su (0, 0) es la esquina de la capa, y no se sale: un rectángulo enorme queda dentro.
    pen(&mut c, rp, 1);
    rect(&mut c, rp, 0, 0, 1000, 1000);
    assert_eq!(pluma(&mut c, rpm, 8, 4), 1);
    assert_eq!(pluma(&mut c, rpm, 23, 11), 1);
    assert_eq!(pluma(&mut c, rpm, 24, 11), 3);
    assert_eq!(pluma(&mut c, rpm, 8, 12), 3);
    assert_eq!(pluma(&mut c, rp, 0, 0), 1);
    // Con LAYERS_NOBACKFILL (gancho 1) no se borra.
    let otra = k(&mut c, "CreateBehindHookLayer", &[0, 0, 3, 3, 1], &[li, bm, 0, 1]);
    assert_ne!(otra, 0);
    assert_eq!(pluma(&mut c, rpm, 0, 0), 3);
    assert_eq!(k(&mut c, "DeleteLayer", &[], &[0, otra]), 1);
    assert_eq!(k(&mut c, "DeleteLayer", &[], &[0, capa]), 1);
    assert!(c.1.tomar_avisos().is_empty());
    // WhichLayer sin ventana activa: ninguna.
    assert_eq!(k(&mut c, "WhichLayer", &[10, 10], &[li]), 0);
    // Hostil: borrar dos veces o lo que no es una capa; mapa nulo, que no es un mapa, rectángulos al revés o negativos.
    assert_eq!(k(&mut c, "DeleteLayer", &[], &[0, capa]), 0);
    assert_eq!(k(&mut c, "DeleteLayer", &[], &[0, li]), 0);
    assert_eq!(k(&mut c, "CreateUpfrontLayer", &[0, 0, 10, 10, 1], &[li, 0, 0]), 0);
    assert_eq!(k(&mut c, "CreateUpfrontLayer", &[0, 0, 10, 10, 1], &[li, 0xFFFF_FF00, 0]), 0);
    assert_eq!(k(&mut c, "CreateUpfrontLayer", &[10, 10, 5, 5, 1], &[li, bm, 0]), 0);
    assert_eq!(k(&mut c, "CreateUpfrontLayer", &[(-4i32) as u32, 0, 5, 5, 1], &[li, bm, 0]), 0);
    assert_eq!(c.1.tomar_avisos().len(), 6);
    // Una capa más allá del mapa se crea, pero no pinta fuera de él.
    let lejos = k(&mut c, "CreateUpfrontLayer", &[30, 14, 0x7FFF, 0x7FFF, 1], &[li, bm, 0]);
    assert_ne!(lejos, 0);
    let rpl = l32(&c, lejos + layer::RP);
    pen(&mut c, rpl, 2);
    rect(&mut c, rpl, 0, 0, 0x7FFF, 0x7FFF);
    assert_eq!(pluma(&mut c, rpm, 31, 15), 2);
    assert_eq!(pluma(&mut c, rpm, 29, 15), 3);
    // La tarea acaba sin borrarla: la suelta intuition.
    let t = c.2;
    c.1.tarea_muerta(&mut c.0, t);
    assert!(c.1.tomar_avisos().is_empty());
    k(&mut c, "DisposeLayerInfo", &[], &[li]);
    assert_eq!(c.0.bloques_de(0).len(), antes);
}

/// `InstallClipRegion` recorta lo que se pinta en la capa (en sus coordenadas): rellenos, líneas, copias. Sin región,
/// todo otra vez. Lo hostil: una región en círculo (no se pinta nada, aviso), una larguísima (rápido, sin colgarse).
#[test]
fn region_de_recorte() {
    use caja_intuition::estructuras::{layer, region, region_rectangle as rr};
    let mut c = caja();
    let li = k(&mut c, "NewLayerInfo", &[], &[]);
    let (bm, rpm) = planar(&mut c, 2, 32, 16);
    // Una capa en (4, 2): su (0, 0) es el (4, 2) del mapa.
    let capa = k(&mut c, "CreateUpfrontLayer", &[4, 2, 27, 13, 1], &[li, bm, 0]);
    let rp = l32(&c, capa + layer::RP);
    let reg = g(&mut c, "NewRegion", &[], &[]);
    let rectangulo = |c: &mut Caja, x0: u16, y0: u16, x1: u16, y1: u16| {
        let d = reservar(c, 8);
        for (i, v) in [x0, y0, x1, y1].into_iter().enumerate() {
            e16(c, d + 2 * i as u32, v);
        }
        d
    };
    let (a, b) = (rectangulo(&mut c, 0, 0, 3, 1), rectangulo(&mut c, 6, 3, 7, 4));
    g(&mut c, "OrRectRegion", &[], &[reg, a]);
    g(&mut c, "OrRectRegion", &[], &[reg, b]);
    assert_eq!(k(&mut c, "InstallClipRegion", &[], &[capa, reg]), 0);
    pen(&mut c, rp, 1);
    rect_fill(&mut c, rp, 0, 0, 100, 100);
    let v = ver(&mut c, rpm, 14, 8);
    assert_eq!(v, ["..............", "..............", "....####......", "....####......", "..............", "..........##..", "..........##..", ".............."]);
    // Una línea que cruza: solo sus puntos dentro.
    pen(&mut c, rp, 2);
    mover(&mut c, rp, 0, 4);
    draw(&mut c, rp, 20, 4);
    assert_eq!((pluma(&mut c, rpm, 10, 6), pluma(&mut c, rpm, 11, 6), pluma(&mut c, rpm, 12, 6)), (2, 2, 0));
    // Una copia a la capa (ClipBlit desde otro mapa): recortada igual.
    let (_, rpo) = planar(&mut c, 2, 32, 16);
    pen(&mut c, rpo, 3);
    rect(&mut c, rpo, 0, 0, 31, 15);
    g(&mut c, "ClipBlit", &[0, 0, 0, 0, 24, 12, 0xC0], &[rpo, rp]);
    assert_eq!((pluma(&mut c, rpm, 4, 2), pluma(&mut c, rpm, 8, 2), pluma(&mut c, rpm, 10, 5)), (3, 0, 3));
    // Sin región: todo.
    assert_eq!(k(&mut c, "InstallClipRegion", &[], &[capa, 0]), reg);
    rect_fill(&mut c, rp, 0, 0, 100, 100);
    assert_eq!(pluma(&mut c, rpm, 8, 2), 2);
    assert!(c.1.tomar_avisos().is_empty(), "{:?}", c.1.avisos());
    // Hostil: una región en círculo: no se pinta nada y se avisa.
    let circ = reservar(&mut c, 12);
    let n = reservar(&mut c, 16);
    e32(&mut c, circ + region::REGIONRECTANGLE, n);
    e32(&mut c, n + rr::NEXT, n);
    k(&mut c, "InstallClipRegion", &[], &[capa, circ]);
    pen(&mut c, rp, 0);
    rect_fill(&mut c, rp, 0, 0, 100, 100);
    assert_eq!(pluma(&mut c, rpm, 8, 2), 2);
    assert_eq!(c.1.tomar_avisos().len(), 1);
    // Una región de 16000 rectángulos de un punto (en bandas): se pinta en poco tiempo.
    let larga = g(&mut c, "NewRegion", &[], &[]);
    let nodos = reservar(&mut c, 16 * 16000);
    e32(&mut c, larga + region::REGIONRECTANGLE, nodos);
    for i in 0..16000u32 {
        let d = nodos + 16 * i;
        e32(&mut c, d + rr::NEXT, if i + 1 < 16000 { d + 16 } else { 0 });
        let (x, y) = ((i % 160) as u16 * 2, (i / 160) as u16 * 2);
        for (j, v) in [x, y, x, y].into_iter().enumerate() {
            e16(&mut c, d + rr::BOUNDS + 2 * j as u32, v);
        }
    }
    k(&mut c, "InstallClipRegion", &[], &[capa, larga]);
    let t = std::time::Instant::now();
    for _ in 0..10 {
        rect_fill(&mut c, rp, 0, 0, 100, 100);
    }
    assert!(t.elapsed().as_secs() < 20, "{:?}", t.elapsed());
    assert_eq!((pluma(&mut c, rpm, 4, 2), pluma(&mut c, rpm, 5, 2)), (0, 2));
    assert!(c.1.tomar_avisos().is_empty(), "{:?}", c.1.avisos());
    k(&mut c, "InstallClipRegion", &[], &[capa, 0]);
    k(&mut c, "DeleteLayer", &[], &[0, capa]);
}

fn rect_fill(c: &mut Caja, rp: u32, x0: i32, y0: i32, x1: i32, y1: i32) {
    rect(c, rp, x0, y0, x1, y1);
}
