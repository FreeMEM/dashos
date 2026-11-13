//! gadtools.library y los gadgets y menús de intuition: crear, dibujar, pulsar (con eventos del compositor de
//! prueba) y los mensajes que llegan; y lo malo: punteros malos, listas en círculo, soltar dos veces, la tarea que
//! muere con todo creado.

mod comun;

use caja_exec::Memoria;
use caja_intuition::compositor::{BTN_LEFT, BTN_RIGHT};
use caja_intuition::estructuras::{idcmp, intuimessage as im, tag, wa, wflg, window as win};
use caja_intuition::{Evento, Fallo};
use comun::*;

const B: u32 = 0x8008_0000;
const BUTTON: u32 = 1;
const CHECKBOX: u32 = 2;
const MX: u32 = 5;
const CYCLE: u32 = 7;
const SLIDER: u32 = 11;
const STRING: u32 = 12;
const LISTVIEW: u32 = 4;
const PALETTE: u32 = 8;

fn etiquetas(c: &mut Caja, t: &[(u32, u32)]) -> u32 {
    let d = reservar(c, (t.len() as u32 + 1) * 8);
    for (k, (a, b)) in t.iter().enumerate() {
        e32(c, d + 8 * k as u32, *a);
        e32(c, d + 8 * k as u32 + 4, *b);
    }
    e32(c, d + 8 * t.len() as u32, tag::TAG_DONE);
    d
}

#[allow(clippy::too_many_arguments)]
fn newgadget(c: &mut Caja, vi: u32, x: u16, y: u16, an: u16, al: u16, id: u16, texto: &str) -> u32 {
    let ng = reservar(c, 30);
    e16(c, ng, x);
    e16(c, ng + 2, y);
    e16(c, ng + 4, an);
    e16(c, ng + 6, al);
    let t = cadena(c, texto);
    e32(c, ng + 8, t);
    e16(c, ng + 16, id);
    e32(c, ng + 22, vi);
    ng
}

fn vector(c: &mut Caja, textos: &[&str]) -> u32 {
    let v = reservar(c, 4 * (textos.len() as u32 + 1));
    for (k, t) in textos.iter().enumerate() {
        let s = cadena(c, t);
        e32(c, v + 4 * k as u32, s);
    }
    v
}

fn mensajes(c: &mut Caja, w: u32) -> Vec<(u32, u16, u32)> {
    let puerto = l32(c, w + win::USERPORT);
    let mut v = Vec::new();
    loop {
        let m = gt(c, "GT_GetIMsg", &[], &[puerto]);
        if m == 0 {
            break;
        }
        v.push((l32(c, m + im::CLASS), l16(c, m + im::CODE), l32(c, m + im::IADDRESS)));
        gt(c, "GT_ReplyIMsg", &[], &[0, m]);
    }
    v
}

fn clic(c: &mut Caja, w: u32, x: i32, y: i32) {
    let s = c.1.superficie(w).unwrap();
    c.1.comp.meter(s, Evento::Raton { x, y });
    c.1.comp.meter(s, Evento::Boton { boton: BTN_LEFT, pulsado: true });
    c.1.comp.meter(s, Evento::Boton { boton: BTN_LEFT, pulsado: false });
    c.1.atender(&mut c.0);
}

/// Una ventana con un botón, una casilla, un ciclo, un mx, un deslizador, una cadena, una lista y una paleta.
fn ventana(c: &mut Caja) -> (u32, u32, u32, Vec<u32>) {
    let wb = c.1.workbench();
    let vi = gt(c, "GetVisualInfoA", &[], &[wb, 0]);
    assert_ne!(vi, 0);
    let glist = reservar(c, 4);
    let mut g = gt(c, "CreateContext", &[], &[glist]);
    assert_eq!(l32(c, glist), g);
    let mut gads = Vec::new();
    let opciones = vector(c, &["Uno", "Dos", "Tres"]);
    let lista = reservar(c, 14);
    // Una lista de exec con tres nodos.
    let nodos: Vec<u32> = (0..3).map(|_| reservar(c, 14)).collect();
    e32(c, lista, nodos[0]);
    e32(c, lista + 4, 0);
    e32(c, lista + 8, nodos[2]);
    for (k, &n) in nodos.iter().enumerate() {
        e32(c, n, if k < 2 { nodos[k + 1] } else { lista + 4 });
        e32(c, n + 4, if k > 0 { nodos[k - 1] } else { lista });
        let s = cadena(c, &format!("Nodo {k}"));
        e32(c, n + 10, s);
    }
    let sl = cadena(c, "%ld");
    let hola = cadena(c, "hola");
    for (k, (tipo, x, y, an, al, tags)) in [
        (BUTTON, 10, 20, 60, 14, vec![]),
        (CHECKBOX, 10, 40, 26, 11, vec![(B + 4, 1)]),
        (CYCLE, 10, 60, 80, 14, vec![(B + 14, opciones)]),
        (MX, 120, 20, 17, 9, vec![(B + 9, opciones), (B + 61, 1)]),
        (SLIDER, 40, 80, 100, 11, vec![(B + 38, 0), (B + 39, 10), (B + 40, 3), (B + 42, sl)]),
        (STRING, 10, 100, 100, 14, vec![(B + 45, hola), (B + 46, 20)]),
        (LISTVIEW, 160, 60, 100, 50, vec![(B + 6, lista)]),
        (PALETTE, 160, 120, 60, 20, vec![(B + 16, 2), (B + 17, 1)]),
    ]
    .into_iter()
    .enumerate()
    {
        let ng = newgadget(c, vi, x, y, an, al, k as u16, "G");
        let t = etiquetas(c, &tags);
        g = gt(c, "CreateGadgetA", &[tipo], &[g, ng, t]);
        assert_ne!(g, 0, "gadget {k}");
        gads.push(g);
    }
    let t = etiquetas(
        c,
        &[
            (wa::WIDTH, 300),
            (wa::HEIGHT, 160),
            (wa::GADGETS, l32(c, glist)),
            (wa::FLAGS, wflg::ACTIVATE | wflg::CLOSEGADGET | wflg::DRAGBAR),
            (wa::IDCMP, idcmp::GADGETUP | idcmp::GADGETDOWN | idcmp::MOUSEMOVE | idcmp::MENUPICK | idcmp::CLOSEWINDOW),
        ],
    );
    let w = i(c, "OpenWindowTagList", &[], &[0, t]);
    assert_ne!(w, 0);
    (w, vi, l32(c, glist), gads)
}

#[test]
fn gadgets_y_eventos() {
    let mut c = caja();
    let (w, vi, glist, g) = ventana(&mut c);
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
    assert_eq!(l32(&c, w + win::FIRSTGADGET), glist);
    // Botón: GADGETUP con código 0.
    clic(&mut c, w, 30, 25);
    assert_eq!(mensajes(&mut c, w), [(idcmp::GADGETUP, 0, g[0])]);
    // Casilla: estaba marcada; ahora no.
    clic(&mut c, w, 15, 45);
    assert_eq!(mensajes(&mut c, w), [(idcmp::GADGETUP, 0, g[1])]);
    // Ciclo: a la siguiente opción.
    clic(&mut c, w, 40, 65);
    clic(&mut c, w, 40, 65);
    assert_eq!(mensajes(&mut c, w), [(idcmp::GADGETUP, 1, g[2]), (idcmp::GADGETUP, 2, g[2])]);
    // Mx: GADGETDOWN con la opción (9 de alto más 1 de separación).
    clic(&mut c, w, 125, 20 + 2 * 10 + 3);
    assert_eq!(mensajes(&mut c, w), [(idcmp::GADGETDOWN, 2, g[3])]);
    // Lista: el segundo nodo.
    clic(&mut c, w, 190, 60 + 2 + 9 + 4);
    assert_eq!(mensajes(&mut c, w), [(idcmp::GADGETUP, 1, g[6])]);
    // GT_GetGadgetAttrs de todo eso.
    let p = reservar(&mut c, 16);
    let t = etiquetas(&mut c, &[(B + 4, p), (B + 15, p + 4), (B + 10, p + 8), (B + 54, p + 12)]);
    gt(&mut c, "GT_GetGadgetAttrsA", &[], &[g[1], 0, 0, t]);
    assert_eq!(l32(&c, p), 0);
    // Cadena: se activa con un clic, se escribe y Return da GADGETUP; el texto, en su StringInfo.
    clic(&mut c, w, 100, 105);
    let s = c.1.superficie(w).unwrap();
    for (cod, ch) in [(30, 'x'), (28, '\r')] {
        c.1.comp.meter(s, Evento::Tecla { codigo: cod, pulsada: true, repeticion: false, caracter: Some(ch) });
    }
    c.1.atender(&mut c.0);
    assert_eq!(mensajes(&mut c, w), [(idcmp::GADGETUP, 0, g[5])]);
    let si = l32(&c, g[5] + 34);
    let buf = l32(&c, si);
    assert_eq!(c.0.mem.leer(buf, 6).unwrap(), b"holax\0");
    // Deslizador por GT_SetGadgetAttrs y GT_GetGadgetAttrs.
    let t = etiquetas(&mut c, &[(B + 40, 7)]);
    gt(&mut c, "GT_SetGadgetAttrsA", &[], &[g[4], w, 0, t]);
    let t = etiquetas(&mut c, &[(B + 40, p)]);
    assert_eq!(gt(&mut c, "GT_GetGadgetAttrsA", &[], &[g[4], w, 0, t]), 1);
    assert_eq!(l32(&c, p), 7);
    // Desactivado: no responde.
    i(&mut c, "OffGadget", &[], &[g[0], w, 0]);
    clic(&mut c, w, 30, 25);
    assert!(mensajes(&mut c, w).is_empty());
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
    // Todo fuera: sin memoria perdida.
    i(&mut c, "RemoveGList", &[u32::MAX], &[w, glist]);
    i(&mut c, "CloseWindow", &[], &[w]);
    gt(&mut c, "FreeGadgets", &[], &[glist]);
    gt(&mut c, "FreeVisualInfo", &[], &[vi]);
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
}

#[test]
fn menus_con_el_boton_derecho() {
    let mut c = caja();
    let (w, vi, _, _) = ventana(&mut c);
    // Proyecto: Abrir, ---, Salir (con tecla); Editar: Marcado (CHECKIT|MENUTOGGLE).
    let nm = reservar(&mut c, 20 * 7);
    let textos = ["Proyecto", "Abrir", "", "Salir", "Editar", "Marcado"];
    let tipos = [1u8, 2, 2, 2, 1, 2];
    for k in 0..6u32 {
        let e = nm + 20 * k;
        c.0.mem.escribir8(e, tipos[k as usize]).unwrap();
        let t = if k == 2 { u32::MAX } else { cadena(&mut c, textos[k as usize]) };
        e32(&mut c, e + 2, t);
        if k == 3 {
            let q = cadena(&mut c, "Q");
            e32(&mut c, e + 6, q);
        }
        if k == 5 {
            e16(&mut c, e + 10, 1 | 8);
        }
        e32(&mut c, e + 16, 100 + k);
    }
    let m = gt(&mut c, "CreateMenusA", &[], &[nm, 0]);
    assert_ne!(m, 0);
    assert_eq!(gt(&mut c, "LayoutMenusA", &[], &[m, vi, 0]), 1);
    i(&mut c, "SetMenuStrip", &[], &[w, m]);
    // ItemAddress de Proyecto > Salir (menú 0, elemento 2): su UserData detrás.
    let salir = i(&mut c, "ItemAddress", &[(2 << 5) | (0x1F << 11)], &[m]);
    assert_eq!(l32(&c, salir + 34), 103);
    // Con el botón derecho: sobre "Proyecto", abajo hasta "Salir", soltar.
    let s = c.1.superficie(w).unwrap();
    let iy = 11 + l16(&c, salir + 6) as i32 + 3;
    for e in [
        Evento::Raton { x: 20, y: 5 },
        Evento::Boton { boton: BTN_RIGHT, pulsado: true },
        Evento::Raton { x: 20, y: 6 },
        Evento::Raton { x: 20, y: iy },
        Evento::Boton { boton: BTN_RIGHT, pulsado: false },
    ] {
        c.1.comp.meter(s, e);
    }
    c.1.atender(&mut c.0);
    let codigo = (2 << 5) | (0x1F << 11);
    assert_eq!(mensajes(&mut c, w), [(idcmp::MENUPICK, codigo, 0)]);
    // Soltar fuera de todo: MENUNULL.
    for e in [Evento::Boton { boton: BTN_RIGHT, pulsado: true }, Evento::Raton { x: 250, y: 140 }, Evento::Boton { boton: BTN_RIGHT, pulsado: false }] {
        c.1.comp.meter(s, e);
    }
    c.1.atender(&mut c.0);
    assert_eq!(mensajes(&mut c, w), [(idcmp::MENUPICK, 0xFFFF, 0)]);
    i(&mut c, "ClearMenuStrip", &[], &[w]);
    gt(&mut c, "FreeMenus", &[], &[m]);
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
}

#[test]
fn gadtools_hostil() {
    let mut c = caja();
    let wb = c.1.workbench();
    let vi = gt(&mut c, "GetVisualInfoA", &[], &[wb, 0]);
    let glist = reservar(&mut c, 4);
    let ctx = gt(&mut c, "CreateContext", &[], &[glist]);
    let ng = newgadget(&mut c, vi, 1, 1, 10, 10, 0, "x");
    // Previous a 0: 0 sin aviso (así se encadenan los fallos).
    assert_eq!(gt(&mut c, "CreateGadgetA", &[BUTTON], &[0, ng, 0]), 0);
    assert!(c.1.avisos().is_empty());
    // Punteros malos, tipos que no existen, un VisualInfo inventado, etiquetas en círculo.
    for (tipo, prev, ngm) in [(BUTTON, ctx, 0u32), (BUTTON, ctx, 4), (BUTTON, 0xFFFF_FFF0, ng), (99, ctx, ng), (10, ctx, ng)] {
        assert_eq!(gt(&mut c, "CreateGadgetA", &[tipo], &[prev, ngm, 0]), 0);
    }
    let ng2 = newgadget(&mut c, 0x1234, 1, 1, 10, 10, 0, "x");
    assert_eq!(gt(&mut c, "CreateGadgetA", &[BUTTON], &[ctx, ng2, 0]), 0);
    let circ = reservar(&mut c, 16);
    e32(&mut c, circ, tag::TAG_MORE);
    e32(&mut c, circ + 4, circ);
    assert_eq!(gt(&mut c, "CreateGadgetA", &[BUTTON], &[ctx, ng, circ]), 0);
    assert!(matches!(c.1.avisos().last().unwrap().fallo, Fallo::EtiquetasMalas { .. }));
    // Un ciclo con opciones que apuntan fuera: se crea sin ellas.
    let malas = reservar(&mut c, 12);
    e32(&mut c, malas, 0xFFFF_FFF0);
    let t = etiquetas(&mut c, &[(B + 14, malas)]);
    assert_ne!(gt(&mut c, "CreateGadgetA", &[CYCLE], &[ctx, ng, t]), 0);
    // GT_SetGadgetAttrs y GT_GetGadgetAttrs de lo que no es un gadget; FreeGadgets dos veces; FreeVisualInfo de
    // lo que no es.
    gt(&mut c, "GT_SetGadgetAttrsA", &[], &[0x1000, 0, 0, 0]);
    gt(&mut c, "GT_GetGadgetAttrsA", &[], &[0xFFFF_FFFF, 0, 0, 0]);
    gt(&mut c, "FreeGadgets", &[], &[ctx]);
    gt(&mut c, "FreeGadgets", &[], &[ctx]);
    gt(&mut c, "FreeVisualInfo", &[], &[vi + 2]);
    // Menús: tipos que no existen, un elemento sin título, ItemAddress y LayoutMenus con punteros malos.
    let nm = reservar(&mut c, 40);
    c.0.mem.escribir8(nm, 2).unwrap();
    assert_eq!(gt(&mut c, "CreateMenusA", &[], &[nm, 0]), 0);
    c.0.mem.escribir8(nm, 77).unwrap();
    assert_eq!(gt(&mut c, "CreateMenusA", &[], &[nm, 0]), 0);
    assert_eq!(gt(&mut c, "CreateMenusA", &[], &[0xFFFF_FFF8, 0]), 0);
    for n in [0u32, 0xFFFF, 0x1234, 0xFFFF_FFFF] {
        i(&mut c, "ItemAddress", &[n], &[0xFFFF_FFF0]);
    }
    gt(&mut c, "LayoutMenusA", &[], &[0xFFFF_FFF0, vi, 0]);
    // Una lista de menús en círculo: LayoutMenus acaba.
    let m = reservar(&mut c, 34);
    e32(&mut c, m, m);
    gt(&mut c, "LayoutMenusA", &[], &[m, vi, 0]);
    gt(&mut c, "FreeMenus", &[], &[m]);
    // DrawBevelBox con un RastPort malo y tamaños imposibles.
    gt(&mut c, "DrawBevelBoxA", &[0, 0, 100, 100], &[0x10, 0]);
    gt(&mut c, "DrawBevelBoxA", &[0, 0, 0xFFFF_8000, 0xFFFF_8000], &[0x10, 0]);
    assert!(!c.1.avisos().is_empty());
    // La tarea muere con todo creado: no queda nada suyo en las tablas.
    let (w, _, _, _) = ventana(&mut c);
    let t = c.2;
    c.1.tarea_muerta(&mut c.0, t);
    assert!(c.1.ventanas().is_empty());
    let _ = w;
}
