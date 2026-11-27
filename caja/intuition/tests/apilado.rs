//! El orden y el sitio de las pantallas (`apilado.rs`): las ventanas de una pantalla propia van dentro de su superficie
//! (hijas), `ScreenToFront`, `ScreenToBack` y `ScreenDepth` (en el compositor y en la lista de `IntuitionBase`), el
//! Workbench delante o detrás de las propias, `MoveScreen` y `ScreenPosition`, y lo que llega del compositor para la
//! superficie de una pantalla (el ratón, su barra para arrastrarla, el foco y el teclado de su ventana activa). Y lo
//! hostil.

mod comun;

use caja_exec::lvo as exec_lvo;
use caja_exec::{Config, Exec, MemoriaVec};
use caja_intuition::compositor::BTN_LEFT;
use caja_intuition::estructuras::{idcmp, intuimessage as im, intuitionbase as ib, screen as sc, tag, wa, wflg, window as win};
use caja_intuition::modos::id;
use caja_intuition::{CajaGrafica, CompositorDePrueba, Evento, Numeros};
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

fn pantalla(c: &mut Caja) -> u32 {
    let t = etiquetas(c, &[(0x8000_0032, id(1, 4))]);
    let s = i(c, "OpenScreenTagList", &[], &[0, t]);
    assert_ne!(s, 0, "{:?}", c.1.avisos());
    s
}

fn ventana(c: &mut Caja, s: u32, x: u32, y: u32) -> u32 {
    let mut t = vec![(wa::LEFT, x), (wa::TOP, y), (wa::WIDTH, 120), (wa::HEIGHT, 80)];
    t.push((wa::IDCMP, idcmp::RAWKEY | idcmp::ACTIVEWINDOW | idcmp::INACTIVEWINDOW));
    t.push((wa::FLAGS, wflg::DRAGBAR | wflg::DEPTHGADGET));
    if s != 0 {
        t.push((wa::CUSTOMSCREEN, s));
    }
    let t = etiquetas(c, &t);
    let w = i(c, "OpenWindowTagList", &[], &[0, t]);
    assert_ne!(w, 0, "{:?}", c.1.avisos());
    w
}

fn sup(c: &Caja, s: u32) -> u32 {
    c.1.superficie_de_pantalla(s).expect("su superficie")
}

fn lista(c: &Caja) -> Vec<u32> {
    let mut v = Vec::new();
    let mut s = l32(c, c.1.intuition_base() + ib::FIRSTSCREEN);
    while s != 0 && v.len() < 16 {
        v.push(s);
        s = l32(c, s + sc::NEXTSCREEN);
    }
    v
}

/// Las clases de los mensajes del `UserPort` (contestados).
fn clases(c: &mut Caja, w: u32) -> Vec<u32> {
    let puerto = l32(c, w + win::USERPORT);
    let mut v = Vec::new();
    loop {
        let m = c.0.get_msg(puerto).unwrap();
        if m == 0 {
            break;
        }
        v.push(l32(c, m + im::CLASS));
        let mut r = regs(&[], &[0, m]);
        c.0.llamar(exec_lvo::por_nombre("ReplyMsg").unwrap(), &mut r);
    }
    v
}

/// Las ventanas de una pantalla propia van dentro de su superficie, en sus coordenadas; las del Workbench, sueltas.
#[test]
fn ventanas_dentro_de_su_pantalla() {
    let mut c = caja_fhd();
    let s = pantalla(&mut c);
    let w = ventana(&mut c, s, 30, 40);
    let wb = ventana(&mut c, 0, 30, 40);
    let sw = c.1.superficie(w).unwrap();
    let pedida = &c.1.comp.ventana(sw).pedida;
    assert_eq!((pedida.padre, pedida.x, pedida.y), (Some(sup(&c, s)), 30, 40));
    assert_eq!(c.1.comp.ventana(c.1.superficie(wb).unwrap()).pedida.padre, None);
    // WindowToFront y WindowToBack la ordenan entre sus hermanas.
    let w2 = ventana(&mut c, s, 60, 60);
    let sw2 = c.1.superficie(w2).unwrap();
    assert_eq!(c.1.comp.hijas[&sup(&c, s)], vec![sw, sw2]);
    i(&mut c, "WindowToFront", &[], &[w]);
    assert_eq!(c.1.comp.hijas[&sup(&c, s)], vec![sw2, sw]);
    i(&mut c, "WindowToBack", &[], &[w]);
    assert_eq!(c.1.comp.hijas[&sup(&c, s)], vec![sw, sw2]);
    for x in [w, w2, wb] {
        i(&mut c, "CloseWindow", &[], &[x]);
    }
    assert_eq!(i(&mut c, "CloseScreen", &[], &[s]), 1);
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}

/// ScreenToFront, ScreenToBack y ScreenDepth: en el compositor (con sus ventanas, que van dentro) y en la lista de
/// IntuitionBase. El Workbench delante: las propias detrás de todo, en su orden; detrás: delante de todo.
#[test]
fn delante_y_detras() {
    let mut c = caja_fhd();
    let wb = c.1.workbench();
    let a = pantalla(&mut c);
    let b = pantalla(&mut c);
    let (sa, sb) = (sup(&c, a), sup(&c, b));
    let wa_ = ventana(&mut c, a, 10, 10);
    let swa = c.1.superficie(wa_).unwrap();
    let suelta = ventana(&mut c, 0, 10, 10);
    let ss = c.1.superficie(suelta).unwrap();
    assert_eq!(lista(&c), vec![b, a, wb]);
    assert_eq!(c.1.comp.apilado(), vec![sa, swa, sb, ss]);
    // A delante: con su ventana.
    i(&mut c, "ScreenToFront", &[], &[a]);
    assert_eq!(lista(&c), vec![a, b, wb]);
    assert_eq!(c.1.comp.apilado(), vec![sb, ss, sa, swa]);
    // A detrás (ScreenDepth con SDEPTH_TOBACK).
    i(&mut c, "ScreenDepth", &[1], &[a, 0]);
    assert_eq!(lista(&c), vec![b, wb, a]);
    assert_eq!(c.1.comp.apilado(), vec![sa, swa, sb, ss]);
    i(&mut c, "ScreenDepth", &[0], &[a, 0]);
    assert_eq!(lista(&c)[0], a);
    // El Workbench delante: las propias, detrás de todo y en su orden (A delante de B).
    i(&mut c, "ScreenToFront", &[], &[wb]);
    assert_eq!(lista(&c), vec![wb, a, b]);
    assert_eq!(c.1.comp.apilado(), vec![sb, sa, swa, ss]);
    // Detrás: delante de todo, en su orden.
    i(&mut c, "ScreenToBack", &[], &[wb]);
    assert_eq!(lista(&c), vec![a, b, wb]);
    assert_eq!(c.1.comp.apilado(), vec![ss, sb, sa, swa]);
    for x in [wa_, suelta] {
        i(&mut c, "CloseWindow", &[], &[x]);
    }
    for x in [a, b] {
        assert_eq!(i(&mut c, "CloseScreen", &[], &[x]), 1);
    }
    assert_eq!(lista(&c), vec![wb]);
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}

/// MoveScreen y ScreenPosition: su superficie va a su sitio (y sus ventanas con ella); el compositor contesta con la
/// posición. El Workbench es el escritorio: no se mueve.
#[test]
fn mover_pantallas() {
    let mut c = caja_fhd();
    let s = pantalla(&mut c);
    let w = ventana(&mut c, s, 30, 40);
    i(&mut c, "MoveScreen", &[0, 100], &[s]);
    c.1.atender(&mut c.0);
    assert_eq!((l16(&c, s + sc::LEFTEDGE), l16(&c, s + sc::TOPEDGE)), (0, 100));
    let v = c.1.comp.ventana(sup(&c, s));
    assert_eq!((v.x, v.y), (0, 100));
    // La ventana sigue en (30, 40) de su pantalla: va dentro.
    assert_eq!((l16(&c, w + win::LEFTEDGE), l16(&c, w + win::TOPEDGE)), (30, 40));
    // ScreenPosition: relativo, a un sitio, y SPOS_MAKEVISIBLE (ya se ve entera) no la mueve.
    i(&mut c, "ScreenPosition", &[0, 5, (-20i32) as u32, 0, 0], &[s]);
    assert_eq!((l16(&c, s + sc::LEFTEDGE), l16(&c, s + sc::TOPEDGE)), (5, 80));
    i(&mut c, "ScreenPosition", &[1, 0, 0, 0, 0], &[s]);
    assert_eq!((l16(&c, s + sc::LEFTEDGE), l16(&c, s + sc::TOPEDGE)), (0, 0));
    i(&mut c, "ScreenPosition", &[2, 300, 300, 400, 400], &[s]);
    assert_eq!((l16(&c, s + sc::LEFTEDGE), l16(&c, s + sc::TOPEDGE)), (0, 0));
    let wb = c.1.workbench();
    i(&mut c, "MoveScreen", &[0, 50], &[wb]);
    assert_eq!(l16(&c, wb + sc::TOPEDGE), 0);
    i(&mut c, "CloseWindow", &[], &[w]);
    assert_eq!(i(&mut c, "CloseScreen", &[], &[s]), 1);
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}

/// Lo que llega para la superficie de una pantalla: el ratón, su barra (arrastrarla), pulsar fuera de sus ventanas, el
/// foco y el teclado (para su ventana activa, que se activa al pulsarla: el compositor da el foco a la pantalla).
#[test]
fn eventos_de_la_pantalla() {
    let mut c = caja_fhd();
    let s = pantalla(&mut c);
    let ss = sup(&c, s);
    let w = ventana(&mut c, s, 30, 40);
    let sw = c.1.superficie(w).unwrap();
    clases(&mut c, w);
    // Pulsar la ventana la activa.
    c.1.comp.meter(sw, Evento::Raton { x: 50, y: 30 });
    c.1.comp.meter(sw, Evento::Boton { boton: BTN_LEFT, pulsado: true });
    c.1.comp.meter(sw, Evento::Boton { boton: BTN_LEFT, pulsado: false });
    c.1.atender(&mut c.0);
    assert_ne!(l32(&c, w + win::FLAGS) & wflg::WINDOWACTIVE, 0);
    assert!(clases(&mut c, w).contains(&idcmp::ACTIVEWINDOW));
    // El teclado de la pantalla va a su ventana activa.
    c.1.comp.meter(ss, Evento::Tecla { codigo: 30, pulsada: true, repeticion: false, caracter: Some('a') });
    c.1.atender(&mut c.0);
    assert_eq!(clases(&mut c, w), vec![idcmp::RAWKEY]);
    // Sin foco, inactiva; con él, otra vez activa.
    c.1.comp.meter(ss, Evento::Foco(false));
    c.1.atender(&mut c.0);
    assert_eq!(l32(&c, w + win::FLAGS) & wflg::WINDOWACTIVE, 0);
    c.1.comp.meter(ss, Evento::Foco(true));
    c.1.atender(&mut c.0);
    assert_ne!(l32(&c, w + win::FLAGS) & wflg::WINDOWACTIVE, 0);
    clases(&mut c, w);
    // El ratón en la pantalla: su MouseX/MouseY. Pulsar en su barra: arrastrarla; fuera de sus ventanas, ninguna activa.
    c.1.comp.meter(ss, Evento::Raton { x: 300, y: 5 });
    c.1.comp.meter(ss, Evento::Boton { boton: BTN_LEFT, pulsado: true });
    c.1.atender(&mut c.0);
    assert_eq!((l16(&c, s + sc::MOUSEX), l16(&c, s + sc::MOUSEY)), (300, 5));
    assert!(c.1.comp.registro.contains(&format!("arrastrar {ss}")));
    assert_eq!(l32(&c, w + win::FLAGS) & wflg::WINDOWACTIVE, 0);
    c.1.comp.registro.clear();
    c.1.comp.meter(ss, Evento::Boton { boton: BTN_LEFT, pulsado: false });
    c.1.comp.meter(ss, Evento::Raton { x: 300, y: 300 });
    c.1.comp.meter(ss, Evento::Boton { boton: BTN_LEFT, pulsado: true });
    c.1.atender(&mut c.0);
    assert!(!c.1.comp.registro.iter().any(|r| r.starts_with("arrastrar")));
    // El compositor la ha movido (arrastrada): su sitio.
    c.1.comp.meter(ss, Evento::Posicion { x: 0, y: 64 });
    c.1.atender(&mut c.0);
    assert_eq!(l16(&c, s + sc::TOPEDGE), 64);
    i(&mut c, "CloseWindow", &[], &[w]);
    // Cerrada su ventana activa, el teclado de la pantalla no va a ninguna parte.
    c.1.comp.meter(ss, Evento::Foco(true));
    c.1.comp.meter(ss, Evento::Tecla { codigo: 30, pulsada: true, repeticion: false, caracter: Some('a') });
    c.1.atender(&mut c.0);
    assert_eq!(i(&mut c, "CloseScreen", &[], &[s]), 1);
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}

#[test]
fn orden_hostil() {
    let mut c = caja_fhd();
    // Lo que no es una pantalla: aviso, nada cambia.
    for f in ["ScreenToFront", "ScreenToBack"] {
        i(&mut c, f, &[], &[0x4000]);
    }
    i(&mut c, "ScreenDepth", &[1], &[0, 0]);
    i(&mut c, "MoveScreen", &[1, 1], &[0x4000]);
    i(&mut c, "ScreenPosition", &[1, 1, 1, 0, 0], &[0x10]);
    assert_eq!(c.1.tomar_avisos().len(), 5);
    // Una lista de pantallas en círculo (estropeada por el programa) no cuelga.
    let a = pantalla(&mut c);
    let b = pantalla(&mut c);
    e32(&mut c, a + sc::NEXTSCREEN, b);
    i(&mut c, "ScreenToBack", &[], &[b]);
    let wb = c.1.workbench();
    assert_eq!(lista(&c), vec![a, wb, b], "rehecha, con el Workbench que se perdió");
    // Movida muy lejos: se queda en lo que cabe en un WORD.
    i(&mut c, "ScreenPosition", &[1, 0, 1_000_000, 0, 0], &[a]);
    assert_eq!(l16(&c, a + sc::TOPEDGE), i16::MAX as u16);
    for x in [a, b] {
        assert_eq!(i(&mut c, "CloseScreen", &[], &[x]), 1);
    }
    let av = c.1.tomar_avisos();
    assert!(av.is_empty(), "{:?}", av);
}
