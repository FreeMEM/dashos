//! La lista de pantallas públicas (`publicas.rs`): `LockPubScreenList` y `UnlockPubScreenList` (una foto con sus
//! `PubScreenNode`), `NextPubScreen`, la pantalla pública de serie (`SetDefaultPubScreen`, `GetDefaultPubScreen`,
//! `LockPubScreen(NULL)`) y `SetPubScreenModes` (`SHANGHAI`, `POPPUBSCREEN`). Y lo hostil.

mod comun;

use caja_exec::estructuras::{list, node};
use caja_exec::{Config, Exec, Memoria, MemoriaVec};
use caja_intuition::estructuras::{intuitionbase as ib, screen as sc, tag, wa, window as win};
use caja_intuition::modos::id;
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
    for (k, (a, b)) in t.iter().enumerate() {
        e32(c, d + 8 * k as u32, *a);
        e32(c, d + 8 * k as u32 + 4, *b);
    }
    e32(c, d + 8 * t.len() as u32, tag::TAG_DONE);
    d
}

/// Una pantalla pública de nombre `nombre`, ya abierta (fuera del modo privado) si `abrir`.
fn publica(c: &mut Caja, nombre: &str, abrir: bool) -> u32 {
    let n = cadena(c, nombre);
    let t = etiquetas(c, &[(0x8000_0032, id(1, 4)), (0x8000_002F, n)]);
    let s = i(c, "OpenScreenTagList", &[], &[0, t]);
    assert_ne!(s, 0, "{:?}", c.1.avisos());
    if abrir {
        assert_eq!(i(c, "PubScreenStatus", &[0], &[s]) & 1, 1);
    }
    s
}

/// Los nodos de una `List` de `PubScreenNode`: (nombre, pantalla, flags, tamaño, visitantes).
fn nodos(c: &Caja, l: u32) -> Vec<(String, u32, u16, u16, u16)> {
    let mut v = Vec::new();
    let mut n = l32(c, l + list::LH_HEAD);
    while l32(c, n + node::LN_SUCC) != 0 && v.len() < 16 {
        let nombre = c.0.leer_cadena(l32(c, n + node::LN_NAME)).unwrap();
        v.push((nombre, l32(c, n + 14), l16(c, n + 18), l16(c, n + 20), l16(c, n + 22)));
        n = l32(c, n + node::LN_SUCC);
    }
    v
}

fn texto(c: &Caja, d: u32) -> String {
    c.0.leer_cadena(d).unwrap()
}

#[test]
fn lista_de_publicas() {
    let mut c = caja_fhd();
    let wb = c.1.workbench();
    let a = publica(&mut c, "Alfa", true);
    let b = publica(&mut c, "Beta", false);
    // Una visita a Alfa.
    let n = cadena(&mut c, "alfa");
    assert_eq!(i(&mut c, "LockPubScreen", &[], &[n]), a);
    let antes = c.0.bloques_de(0).len();
    let l = i(&mut c, "LockPubScreenList", &[], &[]);
    assert_ne!(l, 0);
    assert_eq!(
        nodos(&c, l),
        vec![
            ("Workbench".into(), wb, 0, 30, 0),
            ("Alfa".into(), a, 0, 30, 1),
            ("Beta".into(), b, 1, 30, 0),
        ]
    );
    // Otra vez (otra tarea, o la misma): la misma foto; se suelta con el último UnlockPubScreenList.
    assert_eq!(i(&mut c, "LockPubScreenList", &[], &[]), l);
    i(&mut c, "UnlockPubScreenList", &[], &[]);
    assert!(c.0.bloques_de(0).len() > antes);
    i(&mut c, "UnlockPubScreenList", &[], &[]);
    assert_eq!(c.0.bloques_de(0).len(), antes, "la foto, suelta");
    i(&mut c, "UnlockPubScreen", &[], &[0, a]);
    for s in [a, b] {
        assert_eq!(i(&mut c, "CloseScreen", &[], &[s]), 1);
    }
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}

/// NextPubScreen: la siguiente pública abierta, dando la vuelta (las privadas no cuentan).
#[test]
fn siguiente_publica() {
    let mut c = caja_fhd();
    let wb = c.1.workbench();
    let buf = reservar(&mut c, 140);
    assert_eq!(i(&mut c, "NextPubScreen", &[], &[wb, buf]), buf);
    assert_eq!(texto(&c, buf), "Workbench", "sola, ella misma");
    let a = publica(&mut c, "Alfa", true);
    let b = publica(&mut c, "Beta", false);
    let g = publica(&mut c, "Gamma", true);
    assert_eq!(i(&mut c, "NextPubScreen", &[], &[wb, buf]), buf);
    assert_eq!(texto(&c, buf), "Alfa");
    i(&mut c, "NextPubScreen", &[], &[a, buf]);
    assert_eq!(texto(&c, buf), "Gamma", "Beta es privada");
    i(&mut c, "NextPubScreen", &[], &[g, buf]);
    assert_eq!(texto(&c, buf), "Workbench");
    // Desde una privada: la primera.
    i(&mut c, "NextPubScreen", &[], &[b, buf]);
    assert_eq!(texto(&c, buf), "Workbench");
    for s in [a, b, g] {
        assert_eq!(i(&mut c, "CloseScreen", &[], &[s]), 1);
    }
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}

/// La de serie: SetDefaultPubScreen la cambia (sin distinguir mayúsculas), GetDefaultPubScreen la dice, LockPubScreen(NULL)
/// la da; cerrada, vuelve a ser el Workbench. Un nombre que no es de ninguna (o de una privada) no cambia nada.
#[test]
fn la_de_serie() {
    let mut c = caja_fhd();
    let wb = c.1.workbench();
    let a = publica(&mut c, "Alfa", true);
    let b = publica(&mut c, "Beta", false);
    let buf = reservar(&mut c, 140);
    i(&mut c, "GetDefaultPubScreen", &[], &[buf]);
    assert_eq!(texto(&c, buf), "Workbench");
    for (nombre, esperada) in [("ALFA", a), ("nadie", a), ("Beta", a)] {
        let n = cadena(&mut c, nombre);
        i(&mut c, "SetDefaultPubScreen", &[], &[n]);
        i(&mut c, "GetDefaultPubScreen", &[], &[buf]);
        assert_eq!(texto(&c, buf), "Alfa", "{nombre}");
        assert_eq!(i(&mut c, "LockPubScreen", &[], &[0]), esperada);
        i(&mut c, "UnlockPubScreen", &[], &[0, esperada]);
    }
    // En modo privado no es la de serie (la da el Workbench), y al volver a abrirla, sí.
    assert_eq!(i(&mut c, "PubScreenStatus", &[1], &[a]) & 1, 1);
    assert_eq!(i(&mut c, "LockPubScreen", &[], &[0]), wb);
    i(&mut c, "UnlockPubScreen", &[], &[0, wb]);
    i(&mut c, "PubScreenStatus", &[0], &[a]);
    // NULL: el Workbench.
    i(&mut c, "SetDefaultPubScreen", &[], &[0]);
    assert_eq!(i(&mut c, "LockPubScreen", &[], &[0]), wb);
    i(&mut c, "UnlockPubScreen", &[], &[0, wb]);
    // Cerrada la de serie, el Workbench.
    let n = cadena(&mut c, "Alfa");
    i(&mut c, "SetDefaultPubScreen", &[], &[n]);
    assert_eq!(i(&mut c, "CloseScreen", &[], &[a]), 1);
    i(&mut c, "GetDefaultPubScreen", &[], &[buf]);
    assert_eq!(texto(&c, buf), "Workbench");
    assert_eq!(i(&mut c, "CloseScreen", &[], &[b]), 1);
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}

/// SetPubScreenModes: SHANGHAI (las que iban al Workbench sin pedirlo, a la de serie) y POPPUBSCREEN (la pública pasa
/// delante al abrirse en ella una ventana). Devuelve los de antes.
#[test]
fn modos_de_las_publicas() {
    let mut c = caja_fhd();
    let wb = c.1.workbench();
    let a = publica(&mut c, "Alfa", true);
    let otra = publica(&mut c, "Otra", true);
    let n = cadena(&mut c, "Alfa");
    i(&mut c, "SetDefaultPubScreen", &[], &[n]);
    assert_eq!(i(&mut c, "SetPubScreenModes", &[1], &[]), 0);
    let t = etiquetas(&mut c, &[(wa::WIDTH, 100), (wa::HEIGHT, 50)]);
    let w = i(&mut c, "OpenWindowTagList", &[], &[0, t]);
    assert_eq!(l32(&c, w + win::WSCREEN), a, "SHANGHAI: a la de serie");
    // Pedida en el Workbench (por su nombre), en el Workbench.
    let nwb = cadena(&mut c, "Workbench");
    let t = etiquetas(&mut c, &[(wa::WIDTH, 100), (wa::HEIGHT, 50), (wa::PUBSCREENNAME, nwb)]);
    let w2 = i(&mut c, "OpenWindowTagList", &[], &[0, t]);
    assert_eq!(l32(&c, w2 + win::WSCREEN), wb);
    // POPPUBSCREEN: Otra está delante; una ventana en Alfa la pone delante.
    assert_eq!(i(&mut c, "SetPubScreenModes", &[2 | 0xFF00], &[]), 1, "los de antes");
    assert_eq!(l32(&c, c.1.intuition_base() + ib::FIRSTSCREEN), otra);
    let t = etiquetas(&mut c, &[(wa::WIDTH, 100), (wa::HEIGHT, 50), (wa::CUSTOMSCREEN, a)]);
    let w3 = i(&mut c, "OpenWindowTagList", &[], &[0, t]);
    assert_eq!(l32(&c, c.1.intuition_base() + ib::FIRSTSCREEN), a);
    assert_eq!(i(&mut c, "SetPubScreenModes", &[0], &[]), 2, "solo los que hay");
    for x in [w, w2, w3] {
        i(&mut c, "CloseWindow", &[], &[x]);
    }
    for s in [a, otra] {
        assert_eq!(i(&mut c, "CloseScreen", &[], &[s]), 1);
    }
    assert_eq!(l16(&c, wb + sc::FLAGS) & 1, 1, "el Workbench sigue siendo el Workbench");
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}

#[test]
fn publicas_hostiles() {
    let mut c = caja_fhd();
    let wb = c.1.workbench();
    // UnlockPubScreenList sin LockPubScreenList; NextPubScreen de lo que no es una pantalla, o con el búfer fuera de la
    // memoria (o nulo); SetDefaultPubScreen con un nombre que se sale de la memoria: aviso, nada cambia.
    i(&mut c, "UnlockPubScreenList", &[], &[]);
    let buf = reservar(&mut c, 140);
    assert_eq!(i(&mut c, "NextPubScreen", &[], &[0x4000, buf]), 0);
    assert_eq!(i(&mut c, "NextPubScreen", &[], &[wb, TAMANO - 2]), 0);
    assert_eq!(i(&mut c, "NextPubScreen", &[], &[wb, 0]), 0);
    let fin = TAMANO - 4;
    c.0.mem.leer_mut(fin, 4).unwrap().copy_from_slice(b"AAAA");
    i(&mut c, "SetDefaultPubScreen", &[], &[fin]);
    assert_eq!(c.1.tomar_avisos().len(), 5);
    assert_eq!(i(&mut c, "LockPubScreen", &[], &[0]), wb);
    i(&mut c, "UnlockPubScreen", &[], &[0, wb]);
    // Un nombre larguísimo: la lista y NextPubScreen lo cortan en MAXPUBSCREENNAME (139).
    let largo = "x".repeat(400);
    let s = publica(&mut c, &largo, true);
    i(&mut c, "NextPubScreen", &[], &[wb, buf]);
    assert_eq!(texto(&c, buf).len(), 139);
    let l = i(&mut c, "LockPubScreenList", &[], &[]);
    assert_eq!(nodos(&c, l)[1].0.len(), 139);
    // La tarea que tenía la lista acaba sin soltarla: se suelta.
    let antes = c.0.bloques_de(0).len();
    c.1.tarea_muerta(&mut c.0, c.2);
    assert!(c.0.bloques_de(0).len() < antes, "la foto, suelta al acabar la tarea");
    i(&mut c, "UnlockPubScreenList", &[], &[]);
    assert_eq!(c.1.tomar_avisos().len(), 1, "ya no la tenía nadie");
    let _ = s;
}
