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
    assert_eq!(k(&mut c, "CreateUpfrontLayer", &[0, 0, 10, 10, 0], &[0, 0, 0]), 0);
    assert_eq!(k(&mut c, "DoHookClipRects", &[], &[0, 0, 0]), 0);
    let avisos = c.1.tomar_avisos();
    assert_eq!(avisos.len(), 8, "{avisos:?}");
    assert!(matches!(avisos[6].fallo, Fallo::NoImplementada { .. }));
    assert_eq!(avisos[7].funcion, "DoHookClipRects");
}
