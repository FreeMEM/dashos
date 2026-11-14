//! Ventanas: abrir y cerrar (con etiquetas y con `NewWindow`), lo que ve el compositor, los mensajes IDCMP en
//! orden con `ReplyMsg`, los gadgets del sistema, el cambio de tamaño, el teclado y las pantallas públicas.

mod comun;

use caja_exec::estructuras::{memf, msgport};
use caja_exec::{Memoria, Registros, lvo as exec_lvo};
use caja_intuition::compositor::{BTN_LEFT, BTN_MIDDLE, BTN_RIGHT};
use caja_intuition::estructuras::{drawinfo, idcmp, ie, intuimessage as im, intuitionbase as ib, layer};
use caja_intuition::estructuras::{newwindow as nw, rastport, screen as sc, tag, wa, wflg, window as win};
use caja_intuition::{Evento, Fallo};
use comun::*;

const TODO_IDCMP: u32 = idcmp::CLOSEWINDOW
    | idcmp::MOUSEBUTTONS
    | idcmp::MOUSEMOVE
    | idcmp::VANILLAKEY
    | idcmp::RAWKEY
    | idcmp::NEWSIZE
    | idcmp::REFRESHWINDOW
    | idcmp::ACTIVEWINDOW
    | idcmp::INACTIVEWINDOW
    | idcmp::CHANGEWINDOW;

const GADGETS: u32 = wflg::CLOSEGADGET | wflg::DRAGBAR | wflg::DEPTHGADGET | wflg::SIZEGADGET;

/// Una lista de etiquetas en la memoria de la caja.
fn etiquetas(c: &mut Caja, t: &[(u32, u32)]) -> u32 {
    let d = reservar(c, (t.len() as u32 + 1) * 8);
    for (k, (a, b)) in t.iter().enumerate() {
        e32(c, d + 8 * k as u32, *a);
        e32(c, d + 8 * k as u32 + 4, *b);
    }
    e32(c, d + 8 * t.len() as u32, tag::TAG_DONE);
    d
}

/// Abre una ventana de 200 x 100 en (50, 30), con título, todos los gadgets y todo el IDCMP.
fn abrir(c: &mut Caja, flags: u32) -> u32 {
    let titulo = cadena(c, "Hola DashOS");
    let t = etiquetas(
        c,
        &[
            (wa::LEFT, 50),
            (wa::TOP, 30),
            (wa::WIDTH, 200),
            (wa::HEIGHT, 100),
            (wa::TITLE, titulo),
            (wa::IDCMP, TODO_IDCMP),
            (wa::FLAGS, flags),
            (wa::MAXWIDTH, u32::MAX),
            (wa::MAXHEIGHT, u32::MAX),
        ],
    );
    let w = i(c, "OpenWindowTagList", &[], &[0, t]);
    assert_ne!(w, 0, "{:?}", c.1.avisos());
    w
}

/// Saca los mensajes del `UserPort`: (clase, código, calificador, x, y), contestándolos.
fn mensajes(c: &mut Caja, w: u32) -> Vec<(u32, u16, u16, i16, i16)> {
    let puerto = l32(c, w + win::USERPORT);
    let mut v = Vec::new();
    loop {
        let m = c.0.get_msg(puerto).unwrap();
        if m == 0 {
            break;
        }
        assert_eq!(l32(c, m + im::IDCMPWINDOW), w);
        v.push((
            l32(c, m + im::CLASS),
            l16(c, m + im::CODE),
            l16(c, m + im::QUALIFIER),
            l16(c, m + im::MOUSEX) as i16,
            l16(c, m + im::MOUSEY) as i16,
        ));
        let mut r = regs(&[], &[0, m]);
        c.0.llamar(exec_lvo::por_nombre("ReplyMsg").unwrap(), &mut r);
    }
    v
}

fn clases(v: &[(u32, u16, u16, i16, i16)]) -> Vec<u32> {
    v.iter().map(|m| m.0).collect()
}

#[test]
fn abrir_con_etiquetas() {
    let mut c = caja();
    let w = abrir(&mut c, GADGETS | wflg::ACTIVATE | wflg::REPORTMOUSE);
    assert_eq!((l16(&c, w + win::LEFTEDGE), l16(&c, w + win::TOPEDGE)), (50, 30));
    assert_eq!((l16(&c, w + win::WIDTH), l16(&c, w + win::HEIGHT)), (200, 100));
    // Bordes: barra de título de 11 (2 + 8 + 1), 4 a los lados y el gadget de tamaño a la derecha.
    let b: Vec<u8> = (0..4).map(|k| l8(&c, w + win::BORDERLEFT + k)).collect();
    assert_eq!(b, [4, 11, 18, 2]);
    assert_eq!((l16(&c, w + win::GZZWIDTH), l16(&c, w + win::GZZHEIGHT)), (178, 87));
    assert_eq!(l32(&c, w + win::WSCREEN), c.1.workbench());
    assert_eq!(l32(&c, w + win::IFONT), c.1.topaz());
    assert_eq!(c.0.leer_cadena(l32(&c, w + win::TITLE)).unwrap(), "Hola DashOS");
    assert_ne!(l32(&c, w + win::FLAGS) & wflg::WINDOWACTIVE, 0);
    // El RastPort dibuja en el mapa nativo de la ventana, con su capa.
    let rp = l32(&c, w + win::RPORT);
    let bm = l32(&c, rp + rastport::BITMAP);
    assert_eq!(c.1.lienzo(bm).map(|l| (l.ancho(), l.alto())), Some((200, 100)));
    let capa = l32(&c, rp + rastport::LAYER);
    assert_eq!(l32(&c, w + win::WLAYER), capa);
    assert_eq!(l32(&c, capa + layer::WINDOW), w);
    assert_eq!(l32(&c, rp + rastport::FONT), c.1.topaz());
    // El puerto es de la tarea que abrió.
    let p = l32(&c, w + win::USERPORT);
    assert_eq!(l32(&c, p + msgport::MP_SIGTASK), c.2);
    assert_ne!(l32(&c, w + win::WINDOWPORT), 0);
    // La pantalla y la IntuitionBase la conocen.
    assert_eq!(l32(&c, c.1.workbench() + sc::FIRSTWINDOW), w);
    assert_eq!(l32(&c, c.1.intuition_base() + ib::ACTIVEWINDOW), w);
    // El compositor tiene su ventana, con el título y el tamaño entero.
    let s = c.1.superficie(w).unwrap();
    let v = c.1.comp.ventana(s);
    assert_eq!((v.titulo.as_str(), v.x, v.y, v.ancho, v.alto), ("Hola DashOS", 50, 30, 200, 100));
    assert!(v.pedida.activa);
    // Ya ha recibido el primer búfer: el borde activo es del color de relleno (azul) y dentro, gris.
    assert_eq!(v.pixel(150, 5) & 0xFF_FFFF, 0x6688BB);
    assert_eq!(v.pixel(100, 50) & 0xFF_FFFF, 0xAAAAAA);
    assert_eq!(v.pixel(0, 0) & 0xFF_FFFF, 0xFFFFFF, "el relieve de fuera");
    // Al abrirse activa, llega ACTIVEWINDOW.
    assert_eq!(clases(&mensajes(&mut c, w)), [idcmp::ACTIVEWINDOW]);
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
}

#[test]
fn abrir_con_new_window_e_interior() {
    let mut c = caja();
    let n = reservar(&mut c, nw::TAMANO_EXT);
    e16(&mut c, n + nw::LEFTEDGE, 10);
    e16(&mut c, n + nw::TOPEDGE, 20);
    e16(&mut c, n + nw::WIDTH, 120);
    e16(&mut c, n + nw::HEIGHT, 60);
    c.0.mem.escribir8(n + nw::DETAILPEN, 0xFF).unwrap();
    c.0.mem.escribir8(n + nw::BLOCKPEN, 0xFF).unwrap();
    e32(&mut c, n + nw::IDCMPFLAGS, idcmp::CLOSEWINDOW);
    e32(&mut c, n + nw::FLAGS, wflg::CLOSEGADGET | wflg::NW_EXTENDED);
    e16(&mut c, n + nw::TYPE, 1);
    // ExtNewWindow: las etiquetas cambian el tamaño por dentro.
    let t = etiquetas(&mut c, &[(wa::INNERWIDTH, 100), (wa::INNERHEIGHT, 50)]);
    e32(&mut c, n + nw::EXTENSION, t);
    let w = i(&mut c, "OpenWindow", &[], &[n]);
    assert_ne!(w, 0);
    assert_eq!((l16(&c, w + win::WIDTH), l16(&c, w + win::HEIGHT)), (108, 63));
    assert_eq!((l16(&c, w + win::GZZWIDTH), l16(&c, w + win::GZZHEIGHT)), (100, 50));
    assert_eq!(l32(&c, w + win::FLAGS) & wflg::NW_EXTENDED, 0);
    assert_eq!(l8(&c, w + win::DETAILPEN), 0, "0xFF: las de la pantalla");
    assert_eq!(l8(&c, w + win::BLOCKPEN), 1);
    // Sin título ni ACTIVATE.
    assert_eq!(l32(&c, w + win::FLAGS) & wflg::WINDOWACTIVE, 0);
    // Una sin bordes, del tamaño de la pantalla (ancho y alto 0).
    let t = etiquetas(&mut c, &[(wa::BORDERLESS, 1)]);
    let w2 = i(&mut c, "OpenWindowTagList", &[], &[0, t]);
    assert_eq!((l16(&c, w2 + win::WIDTH), l16(&c, w2 + win::HEIGHT)), (640, 480));
    assert_eq!(l8(&c, w2 + win::BORDERTOP), 0);
    assert_eq!(l32(&c, w2 + win::USERPORT), 0, "sin IDCMP, sin puerto");
    assert!(c.1.comp.ventana(c.1.superficie(w2).unwrap()).pedida.sin_borde);
}

#[test]
fn dibujar_en_la_ventana_llega_al_compositor() {
    let mut c = caja();
    let w = abrir(&mut c, GADGETS);
    let rp = l32(&c, w + win::RPORT);
    let s = c.1.superficie(w).unwrap();
    let antes = c.1.comp.ventana(s).danos.len();
    pen(&mut c, rp, 2);
    rect(&mut c, rp, 20, 20, 39, 29);
    // Todavía no se ha mandado.
    assert_eq!(c.1.comp.ventana(s).pixel(30, 25) & 0xFF_FFFF, 0xAAAAAA);
    c.1.presentar();
    let v = c.1.comp.ventana(s);
    assert_eq!(v.pixel(30, 25) & 0xFF_FFFF, 0xFFFFFF);
    assert_eq!(v.danos.len(), antes + 1);
    assert_eq!(*v.danos.last().unwrap(), caja_intuition::Rect::nuevo(20, 20, 39, 29));
    // Las coordenadas del RastPort son las de la ventana (como en el Amiga): el (0, 0) es la esquina del borde.
    pen(&mut c, rp, 1);
    g(&mut c, "WritePixel", &[0, 0], &[0, rp]);
    c.1.presentar();
    assert_eq!(c.1.comp.ventana(s).pixel(0, 0) & 0xFF_FFFF, 0);
    // Fuera de la ventana no se dibuja.
    assert_eq!(g(&mut c, "WritePixel", &[200, 10], &[0, rp]) as i32, -1);
}

#[test]
fn cerrar_suelta_todo() {
    let mut c = caja();
    let libre = c.0.avail_mem(memf::ANY).unwrap();
    let w = abrir(&mut c, GADGETS | wflg::ACTIVATE);
    let s = c.1.superficie(w).unwrap();
    assert!(c.0.avail_mem(memf::ANY).unwrap() < libre);
    i(&mut c, "CloseWindow", &[], &[w]);
    // Lo que se reservó al abrir (incluido el ACTIVEWINDOW que estaba en el puerto) ha vuelto: solo quedan el
    // título y la lista de etiquetas, que son del programa.
    let del_programa = 16 + 80;
    assert_eq!(c.0.avail_mem(memf::ANY).unwrap() + del_programa, libre);
    assert!(c.1.comp.ventana(s).cerrada);
    assert!(c.1.ventanas().is_empty());
    assert_eq!(l32(&c, c.1.workbench() + sc::FIRSTWINDOW), 0);
    assert_eq!(l32(&c, c.1.intuition_base() + ib::ACTIVEWINDOW), 0);
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
    assert!(c.0.avisos().is_empty(), "{:?}", c.0.avisos());
}

#[test]
fn mensajes_en_orden_y_contestados() {
    let mut c = caja();
    c.1.poner_hora(1000, 250);
    let w = abrir(&mut c, GADGETS | wflg::ACTIVATE | wflg::REPORTMOUSE);
    let s = c.1.superficie(w).unwrap();
    mensajes(&mut c, w);
    for e in [
        Evento::Raton { x: 60, y: 40 },
        Evento::Boton { boton: BTN_LEFT, pulsado: true },
        Evento::Boton { boton: BTN_LEFT, pulsado: false },
        Evento::Tecla { codigo: 30, pulsada: true, repeticion: false, caracter: Some('a') },
        Evento::Tecla { codigo: 30, pulsada: false, repeticion: false, caracter: Some('a') },
        Evento::Tecla { codigo: 103, pulsada: true, repeticion: false, caracter: None },
        Evento::Cerrar,
    ] {
        c.1.comp.meter(s, e);
    }
    c.1.atender(&mut c.0);
    assert_eq!(c.1.mensajes_fuera(w).len(), 7);
    let m = mensajes(&mut c, w);
    assert_eq!(
        clases(&m),
        [
            idcmp::MOUSEMOVE,
            idcmp::MOUSEBUTTONS,
            idcmp::MOUSEBUTTONS,
            idcmp::VANILLAKEY,
            idcmp::RAWKEY,
            idcmp::RAWKEY,
            idcmp::CLOSEWINDOW
        ]
    );
    assert_eq!((m[0].1, m[0].3, m[0].4), (ie::IECODE_NOBUTTON, 60, 40));
    assert_eq!((m[1].1, m[1].2 & ie::LEFTBUTTON), (ie::SELECTDOWN, ie::LEFTBUTTON));
    assert_eq!((m[2].1, m[2].2 & ie::LEFTBUTTON), (ie::SELECTUP, 0));
    assert_eq!(m[3].1, b'a' as u16);
    assert_eq!(m[4].1, 0x20 | 0x80, "la A soltada");
    assert_eq!(m[5].1, 0x4C, "flecha arriba");
    // Contestados: vuelven al WindowPort y intuition los recoge al atender.
    assert_eq!(c.1.mensajes_fuera(w).len(), 7);
    c.1.atender(&mut c.0);
    assert!(c.1.mensajes_fuera(w).is_empty());
    // La hora y los datos de la ventana.
    let p = l32(&c, w + win::USERPORT);
    c.1.comp.meter(s, Evento::Cerrar);
    c.1.atender(&mut c.0);
    let msg = c.0.get_msg(p).unwrap();
    assert_eq!((l32(&c, msg + im::SECONDS), l32(&c, msg + im::MICROS)), (1000, 250));
    assert_eq!(l16(&c, msg + 18), im::TAMANO_EXT as u16, "mn_Length");
    assert_eq!((l16(&c, w + win::MOUSEX), l16(&c, w + win::GZZMOUSEX)), (60, 56));
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
}

#[test]
fn la_senal_despierta_a_la_tarea() {
    let mut c = caja();
    let w = abrir(&mut c, GADGETS);
    let s = c.1.superficie(w).unwrap();
    let p = l32(&c, w + win::USERPORT);
    let bit = 1u32 << l8(&c, p + msgport::MP_SIGBIT);
    // La tarea espera su señal (Wait bloquea); el CLOSEWINDOW la despierta.
    let mut r = regs(&[bit], &[]);
    let a = c.0.llamar(exec_lvo::por_nombre("Wait").unwrap(), &mut r);
    assert_eq!(a, caja_exec::Accion::Bloquear);
    assert_eq!(c.0.elegir_siguiente(), None);
    c.1.comp.meter(s, Evento::Cerrar);
    c.1.atender(&mut c.0);
    assert_eq!(c.0.elegir_siguiente(), Some(c.2));
    let mut r = regs(&[bit], &[]);
    c.0.llamar(exec_lvo::por_nombre("Wait").unwrap(), &mut r);
    assert_eq!(r.d[0], bit);
    assert_eq!(clases(&mensajes(&mut c, w)), [idcmp::CLOSEWINDOW]);
}

#[test]
fn gadgets_del_sistema() {
    let mut c = caja();
    let w = abrir(&mut c, GADGETS);
    let s = c.1.superficie(w).unwrap();
    let clic = |c: &mut Caja, x: i32, y: i32, x2: i32, y2: i32| {
        c.1.comp.meter(s, Evento::Raton { x, y });
        c.1.comp.meter(s, Evento::Boton { boton: BTN_LEFT, pulsado: true });
        c.1.comp.meter(s, Evento::Raton { x: x2, y: y2 });
        c.1.comp.meter(s, Evento::Boton { boton: BTN_LEFT, pulsado: false });
        c.1.atender(&mut c.0);
    };
    // Cerrar: pulsar y soltar dentro del gadget.
    clic(&mut c, 5, 5, 6, 6);
    assert_eq!(clases(&mensajes(&mut c, w)), [idcmp::CLOSEWINDOW]);
    // Pulsar en cerrar y soltar fuera: nada.
    clic(&mut c, 5, 5, 100, 50);
    assert!(mensajes(&mut c, w).is_empty());
    // Barra de arrastre, profundidad y tamaño: se le piden al compositor, sin mensajes.
    clic(&mut c, 100, 5, 100, 5);
    clic(&mut c, 195, 5, 195, 5);
    clic(&mut c, 195, 95, 195, 95);
    let reg: Vec<&str> = c.1.comp.registro.iter().map(|s| s.as_str()).collect();
    assert!(reg.contains(&"arrastrar 1") && reg.contains(&"profundidad 1") && reg.contains(&"redimensionar 1"));
    assert!(mensajes(&mut c, w).is_empty());
    // Dentro: MOUSEBUTTONS.
    clic(&mut c, 100, 50, 100, 50);
    let m = mensajes(&mut c, w);
    assert_eq!(m.iter().map(|m| m.1).collect::<Vec<_>>(), [ie::SELECTDOWN, ie::SELECTUP]);
    // El botón derecho solo con RMBTRAP; el del medio, siempre.
    for (b, p) in [(BTN_RIGHT, true), (BTN_RIGHT, false), (BTN_MIDDLE, true)] {
        c.1.comp.meter(s, Evento::Boton { boton: b, pulsado: p });
    }
    c.1.atender(&mut c.0);
    assert_eq!(mensajes(&mut c, w).iter().map(|m| m.1).collect::<Vec<_>>(), [ie::MIDDLEDOWN]);
}

#[test]
fn cambiar_de_tamano() {
    let mut c = caja();
    let w = abrir(&mut c, GADGETS);
    let s = c.1.superficie(w).unwrap();
    let rp = l32(&c, w + win::RPORT);
    c.1.comp.meter(s, Evento::Tamano { ancho: 300, alto: 150 });
    c.1.atender(&mut c.0);
    assert_eq!((l16(&c, w + win::WIDTH), l16(&c, w + win::HEIGHT)), (300, 150));
    assert_eq!(l16(&c, w + win::GZZWIDTH), 278);
    assert_eq!(clases(&mensajes(&mut c, w)), [idcmp::NEWSIZE, idcmp::REFRESHWINDOW, idcmp::CHANGEWINDOW]);
    let v = c.1.comp.ventana(s);
    assert_eq!((v.ancho, v.alto), (300, 150));
    // Lo que era el borde derecho queda dentro, borrado.
    assert_eq!(v.pixel(190, 50) & 0xFF_FFFF, 0xAAAAAA);
    // Y la capa recorta al tamaño nuevo.
    pen(&mut c, rp, 1);
    assert_eq!(g(&mut c, "WritePixel", &[250, 120], &[0, rp]), 0);
    assert_eq!(g(&mut c, "WritePixel", &[300, 120], &[0, rp]) as i32, -1);
    // SizeWindow: el compositor contesta y llega NEWSIZE. Con límites.
    assert_eq!(i(&mut c, "WindowLimits", &[100, 60, 320, 200], &[w]), 1);
    i(&mut c, "SizeWindow", &[100, 100], &[w]);
    c.1.atender(&mut c.0);
    assert_eq!((l16(&c, w + win::WIDTH), l16(&c, w + win::HEIGHT)), (320, 200));
    mensajes(&mut c, w);
    // Unos límites que no dejan el tamaño de ahora: FALSE.
    assert_eq!(i(&mut c, "WindowLimits", &[400, 0, 0, 0], &[w]), 0);
    // MoveWindow: CHANGEWINDOW y la posición nueva.
    i(&mut c, "MoveWindow", &[10, (-5i32) as u32], &[w]);
    c.1.atender(&mut c.0);
    assert_eq!((l16(&c, w + win::LEFTEDGE), l16(&c, w + win::TOPEDGE)), (60, 25));
    assert_eq!(clases(&mensajes(&mut c, w)), [idcmp::CHANGEWINDOW]);
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
}

#[test]
fn teclado() {
    let mut c = caja();
    let t = etiquetas(&mut c, &[(wa::IDCMP, idcmp::RAWKEY), (wa::WIDTH, 100), (wa::HEIGHT, 50)]);
    let w = i(&mut c, "OpenWindowTagList", &[], &[0, t]);
    let s = c.1.superficie(w).unwrap();
    for (codigo, pulsada) in [(42, true), (59, true), (59, false), (42, false), (79, true), (125, true)] {
        c.1.comp.meter(s, Evento::Tecla { codigo, pulsada, repeticion: false, caracter: None });
    }
    c.1.atender(&mut c.0);
    let m = mensajes(&mut c, w);
    let v: Vec<(u16, u16)> = m.iter().map(|m| (m.1, m.2 & 0x1FF)).collect();
    assert_eq!(
        v,
        [
            (0x60, ie::LSHIFT),
            (0x50, ie::LSHIFT),
            (0xD0, ie::LSHIFT),
            (0xE0, 0),
            (0x1D, ie::NUMERICPAD),
            (0x66, ie::LCOMMAND)
        ]
    );
    // El IAddress de RAWKEY apunta a algo legible (las teclas anteriores, a cero).
    c.1.comp.meter(s, Evento::Tecla { codigo: 30, pulsada: true, repeticion: false, caracter: None });
    c.1.atender(&mut c.0);
    let p = l32(&c, w + win::USERPORT);
    let msg = c.0.get_msg(p).unwrap();
    let ia = l32(&c, msg + im::IADDRESS);
    assert!(ia > 0x1000);
    assert_eq!(l32(&c, ia), 0);
    let mut r = regs(&[], &[0, msg]);
    c.0.llamar(exec_lvo::por_nombre("ReplyMsg").unwrap(), &mut r);
    // Repeticiones sin contestar: como mucho 3 (WA_RptQueue).
    for _ in 0..10 {
        c.1.comp.meter(s, Evento::Tecla { codigo: 30, pulsada: true, repeticion: true, caracter: None });
    }
    c.1.atender(&mut c.0);
    let m = mensajes(&mut c, w);
    assert_eq!(m.len(), 3);
    assert!(m.iter().all(|m| m.2 & ie::REPEAT != 0));
}

#[test]
fn vanillakey_con_y_sin_caracter() {
    let mut c = caja();
    let t = etiquetas(&mut c, &[(wa::IDCMP, idcmp::VANILLAKEY), (wa::WIDTH, 100), (wa::HEIGHT, 50)]);
    let w = i(&mut c, "OpenWindowTagList", &[], &[0, t]);
    let s = c.1.superficie(w).unwrap();
    let teclas = [
        (42, true, None),      // Mayúsculas
        (30, true, None),      // A (distribución por defecto: 'A')
        (42, false, None),     //
        (39, true, Some('ñ')), // lo que diga el compositor (xkb, distribución española)
        (28, true, None),      // Intro
        (103, true, None),     // Flecha: sin carácter y sin RAWKEY pedido, nada
        (30, false, None),     // Soltar: nada con VANILLAKEY
        (29, true, None),      // Control
        (46, true, None),      // C → 3
    ];
    for (codigo, pulsada, caracter) in teclas {
        c.1.comp.meter(s, Evento::Tecla { codigo, pulsada, repeticion: false, caracter });
    }
    c.1.atender(&mut c.0);
    let v: Vec<u16> = mensajes(&mut c, w).iter().map(|m| m.1).collect();
    assert_eq!(v, [b'A' as u16, 0xF1, 13, 3]);
}

#[test]
fn cola_del_raton() {
    let mut c = caja();
    let w = abrir(&mut c, GADGETS | wflg::REPORTMOUSE);
    let s = c.1.superficie(w).unwrap();
    for k in 0..10 {
        c.1.comp.meter(s, Evento::Raton { x: 20 + k, y: 20 });
    }
    c.1.atender(&mut c.0);
    assert_eq!(mensajes(&mut c, w).len(), 5, "WA_MouseQueue por defecto");
    // Sin REPORTMOUSE no llegan.
    let w2 = abrir(&mut c, GADGETS);
    let s2 = c.1.superficie(w2).unwrap();
    c.1.comp.meter(s2, Evento::Raton { x: 20, y: 20 });
    c.1.atender(&mut c.0);
    assert!(mensajes(&mut c, w2).is_empty());
}

#[test]
fn modify_idcmp_y_puerto_compartido() {
    let mut c = caja();
    let w1 = abrir(&mut c, GADGETS);
    let p1 = l32(&c, w1 + win::USERPORT);
    // ModifyIDCMP(0): el puerto se va; con algo, vuelve otro.
    assert_eq!(i(&mut c, "ModifyIDCMP", &[0], &[w1]), 1);
    assert_eq!(l32(&c, w1 + win::USERPORT), 0);
    assert_eq!(i(&mut c, "ModifyIDCMP", &[idcmp::CLOSEWINDOW], &[w1]), 1);
    let p1b = l32(&c, w1 + win::USERPORT);
    assert_ne!(p1b, 0);
    let _ = p1;
    // Una segunda ventana sin IDCMP que usa el puerto de la primera (RKRM: puerto compartido).
    let t = etiquetas(&mut c, &[(wa::WIDTH, 80), (wa::HEIGHT, 40), (wa::CLOSEGADGET, 1)]);
    let w2 = i(&mut c, "OpenWindowTagList", &[], &[0, t]);
    assert_eq!(l32(&c, w2 + win::USERPORT), 0);
    e32(&mut c, w2 + win::USERPORT, p1b);
    i(&mut c, "ModifyIDCMP", &[idcmp::CLOSEWINDOW], &[w2]);
    let (s1, s2) = (c.1.superficie(w1).unwrap(), c.1.superficie(w2).unwrap());
    c.1.comp.meter(s2, Evento::Cerrar);
    c.1.comp.meter(s1, Evento::Cerrar);
    c.1.comp.meter(s2, Evento::Cerrar);
    c.1.atender(&mut c.0);
    assert_eq!(c.0.nodos(p1b + msgport::MP_MSGLIST).unwrap().len(), 3);
    // Cerrar la segunda quita sus mensajes del puerto compartido, pero no el puerto.
    i(&mut c, "CloseWindow", &[], &[w2]);
    let quedan = c.0.nodos(p1b + msgport::MP_MSGLIST).unwrap();
    assert_eq!(quedan.len(), 1);
    assert_eq!(l32(&c, quedan[0] + im::IDCMPWINDOW), w1);
    assert_eq!(c.0.bloque(p1b).map(|b| b.largo), Some(40), "el puerto sigue vivo");
    i(&mut c, "CloseWindow", &[], &[w1]);
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
    assert!(c.0.avisos().is_empty(), "{:?}", c.0.avisos());
}

#[test]
fn pantallas_publicas() {
    let mut c = caja();
    let wb = c.1.workbench();
    assert_eq!(i(&mut c, "LockPubScreen", &[], &[0]), wb);
    let n = cadena(&mut c, "Workbench");
    assert_eq!(i(&mut c, "LockPubScreen", &[], &[n]), wb);
    let otra = cadena(&mut c, "DOPUS.1");
    assert_eq!(i(&mut c, "LockPubScreen", &[], &[otra]), 0);
    assert_eq!(c.1.bloqueos_workbench(), 2);
    i(&mut c, "UnlockPubScreen", &[], &[0, wb]);
    i(&mut c, "UnlockPubScreen", &[], &[n, 0]);
    assert_eq!(c.1.bloqueos_workbench(), 0);
    i(&mut c, "UnlockPubScreen", &[], &[0, wb]);
    assert!(matches!(c.1.avisos()[0].fallo, Fallo::DesbloqueoDeMas { .. }));
    // Abrir en una pantalla pública por nombre; con otro nombre, falla salvo con WA_PubScreenFallBack.
    let t = etiquetas(&mut c, &[(wa::PUBSCREEN, wb), (wa::WIDTH, 50), (wa::HEIGHT, 50)]);
    assert_ne!(i(&mut c, "OpenWindowTagList", &[], &[0, t]), 0);
    let t = etiquetas(&mut c, &[(wa::PUBSCREENNAME, otra)]);
    assert_eq!(i(&mut c, "OpenWindowTagList", &[], &[0, t]), 0);
    let t = etiquetas(&mut c, &[(wa::PUBSCREENNAME, otra), (wa::PUBSCREENFALLBACK, 1)]);
    assert_ne!(i(&mut c, "OpenWindowTagList", &[], &[0, t]), 0);
    // GetScreenData y GetScreenDrawInfo.
    let buf = reservar(&mut c, sc::TAMANO);
    assert_eq!(i(&mut c, "GetScreenData", &[sc::TAMANO, 1], &[buf, 0]), 1);
    assert_eq!(l16(&c, buf + sc::WIDTH), 640);
    let di = i(&mut c, "GetScreenDrawInfo", &[], &[wb]);
    let plumas = l32(&c, di + drawinfo::DRI_PENS);
    assert_eq!(l16(&c, plumas + 2 * drawinfo::FILLPEN as u32), 3);
    assert_eq!(l16(&c, plumas + 2 * drawinfo::NUMDRIPENS as u32), 0xFFFF);
    let w0 = c.1.ventanas()[0];
    assert_eq!(i(&mut c, "ViewPortAddress", &[], &[w0]), wb + sc::VIEWPORT);
}

#[test]
fn titulos_foco_y_peticiones() {
    let mut c = caja();
    let w = abrir(&mut c, GADGETS);
    let s = c.1.superficie(w).unwrap();
    let t = cadena(&mut c, "Otro título");
    i(&mut c, "SetWindowTitles", &[], &[w, t, u32::MAX]);
    assert_eq!(c.1.comp.ventana(s).titulo, "Otro t\u{c3}\u{ad}tulo");
    // El foco: ACTIVEWINDOW e INACTIVEWINDOW, y el color del borde.
    c.1.comp.meter(s, Evento::Foco(true));
    c.1.atender(&mut c.0);
    assert_eq!(c.1.comp.ventana(s).pixel(150, 5) & 0xFF_FFFF, 0x6688BB);
    c.1.comp.meter(s, Evento::Foco(false));
    c.1.atender(&mut c.0);
    assert_eq!(c.1.comp.ventana(s).pixel(150, 5) & 0xFF_FFFF, 0xAAAAAA);
    assert_eq!(clases(&mensajes(&mut c, w)), [idcmp::ACTIVEWINDOW, idcmp::INACTIVEWINDOW]);
    for f in ["ActivateWindow", "WindowToFront", "WindowToBack"] {
        i(&mut c, f, &[], &[w]);
    }
    i(&mut c, "DisplayBeep", &[], &[0]);
    let reg = &c.1.comp.registro;
    assert!(reg.ends_with(&["activar 1".into(), "frente 1".into(), "fondo 1".into()]));
    assert_eq!(c.1.comp.pitidos, 1);
    // CurrentTime.
    c.1.poner_hora(77, 88);
    let d = reservar(&mut c, 8);
    i(&mut c, "CurrentTime", &[], &[d, d + 4]);
    assert_eq!((l32(&c, d), l32(&c, d + 4)), (77, 88));
}

/// Una ventana dibujada por el programa, para mirarla: si `CAPTURA_PNG` dice un fichero, se guarda ahí.
#[test]
fn captura() {
    let mut c = caja();
    let w = abrir(&mut c, GADGETS | wflg::ACTIVATE);
    let rp = l32(&c, w + win::RPORT);
    pen(&mut c, rp, 1);
    let texto = "DashOS: ¡hola, Amiga!";
    let latin1: Vec<u8> = texto.chars().map(|ch| ch as u32 as u8).collect();
    let s = reservar(&mut c, latin1.len() as u32 + 1);
    c.0.mem.leer_mut(s, latin1.len() as u32).unwrap().copy_from_slice(&latin1);
    g(&mut c, "SetDrMd", &[1], &[0, rp]);
    g(&mut c, "SetBPen", &[0], &[0, rp]);
    mover(&mut c, rp, 10, 24);
    g(&mut c, "Text", &[latin1.len() as u32], &[s, rp]);
    // Unas líneas en abanico y rectángulos de colores.
    for k in 0..12 {
        pen(&mut c, rp, 4 + k % 4);
        mover(&mut c, rp, 20, 90);
        draw(&mut c, rp, 20 + k as i32 * 6, 40);
    }
    for (k, p) in [3u32, 4, 5, 6, 7, 2].iter().enumerate() {
        pen(&mut c, rp, *p);
        rect(&mut c, rp, 100 + 13 * k as i32, 40, 110 + 13 * k as i32, 85);
    }
    g(&mut c, "SetDrMd", &[2], &[0, rp]);
    rect(&mut c, rp, 95, 60, 175, 70);
    c.1.presentar();
    let v = c.1.comp.ventana(c.1.superficie(w).unwrap()).clone();
    // Algo de cada: el borde azul, el texto negro, un rectángulo de la pluma 3.
    assert_eq!(v.pixel(150, 3) & 0xFF_FFFF, 0x6688BB);
    assert!((10..250).any(|x| (18..26).any(|y| v.pixel(x, y) & 0xFF_FFFF == 0)));
    assert_eq!(v.pixel(105, 45) & 0xFF_FFFF, 0x6688BB);
    if let Ok(ruta) = std::env::var("CAPTURA_PNG") {
        std::fs::write(ruta, png(v.ancho, v.alto, &v.pix)).unwrap();
    }
    let _ = Registros::default();
}

#[test]
fn gimme_zero_zero_dibuja_dentro_de_los_bordes() {
    // WFLG_GIMMEZEROZERO (el `Window` de Dash): el (0, 0) del `RPort` es la esquina de dentro de los bordes, y lo
    // que se pinta no sale de ahí (los bordes los dibuja intuition).
    let mut c = caja();
    let w = abrir(&mut c, wflg::GIMMEZEROZERO | wflg::CLOSEGADGET | wflg::DRAGBAR);
    let rp = l32(&c, w + win::RPORT);
    let bm = l32(&c, rp + rastport::BITMAP);
    let (bi, ba) = (l8(&c, w + win::BORDERLEFT) as i32, l8(&c, w + win::BORDERTOP) as i32);
    let (bd, bb) = (l8(&c, w + win::BORDERRIGHT) as i32, l8(&c, w + win::BORDERBOTTOM) as i32);
    assert!(bi > 0 && ba > 0);
    assert_eq!(l16(&c, w + win::GZZWIDTH) as i32, 200 - bi - bd);
    let capa = l32(&c, w + win::WLAYER);
    assert_eq!(l16(&c, capa + layer::BOUNDS) as i32, bi);
    let marco: Vec<u32> = {
        let l = c.1.lienzo(bm).unwrap();
        (0..200).map(|x| l.pixel(x, 0)).chain((0..100).map(|y| l.pixel(0, y))).collect()
    };
    pen(&mut c, rp, 2);
    rect(&mut c, rp, 0, 0, 9, 4);
    // Fuera por arriba y por la izquierda: nada (no se pisa el marco).
    rect(&mut c, rp, -20, -20, -1, 50);
    mover(&mut c, rp, -5, -5);
    draw(&mut c, rp, 300, -5);
    let l = c.1.lienzo(bm).unwrap();
    let blanco = l.pixel(bi, ba);
    assert_eq!(l.pixel(bi + 9, ba + 4), blanco);
    assert_ne!(l.pixel(bi + 10, ba + 5), blanco);
    let marco2: Vec<u32> = (0..200).map(|x| l.pixel(x, 0)).chain((0..100).map(|y| l.pixel(0, y))).collect();
    assert_eq!(marco, marco2, "el marco sigue igual");
    // ReadPixel, en las mismas coordenadas.
    assert_eq!(pluma(&mut c, rp, 0, 0), 2);
    assert_eq!(pluma(&mut c, rp, -1, 0), -1);
    assert_eq!(pluma(&mut c, rp, 200 - bi - bd, 0), -1, "a la derecha, fuera");
    // Al cambiar de tamaño, la capa interior sigue a los bordes.
    let s = c.1.superficie(w).unwrap();
    c.1.comp.meter(s, Evento::Tamano { ancho: 300, alto: 150 });
    c.1.atender(&mut c.0);
    assert_eq!(l16(&c, w + win::GZZWIDTH) as i32, 300 - bi - bd);
    assert_eq!(pluma(&mut c, rp, 300 - bi - bd - 1, 150 - ba - bb - 1), 0);
    assert_eq!(pluma(&mut c, rp, 300 - bi - bd, 0), -1);
    mensajes(&mut c, w);
    i(&mut c, "CloseWindow", &[], &[w]);
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
}

/// Una ventana de refresco simple: su capa con su `Layer_Info`, una `ClipRect` que la cubre y una `DamageList` con todo
/// por pintar al abrirla (MUI la mira para saber qué repintar); `EndRefresh(TRUE)` la vacía, un cambio de tamaño la
/// vuelve a llenar y cerrar no deja nada. Lo hostil: el programa estropea los punteros de la capa.
#[test]
fn capa_con_dano() {
    use caja_intuition::estructuras::{cliprect, region, region_rectangle as rr};
    let mut c = caja();
    let antes = c.0.bloques_de(0).len();
    let w = abrir(&mut c, GADGETS | wflg::SIMPLE_REFRESH);
    let capa = l32(&c, w + win::WLAYER);
    assert_eq!(l32(&c, capa + layer::LAYERINFO), c.1.workbench() + sc::LAYERINFO);
    let cr = l32(&c, capa + layer::CLIPRECT);
    assert_ne!(cr, 0);
    assert_eq!(l32(&c, cr + cliprect::NEXT), 0);
    assert_eq!(l32(&c, cr + cliprect::LOBS), 0);
    assert_eq!((l16(&c, cr + cliprect::BOUNDS + 4), l16(&c, cr + cliprect::BOUNDS + 6)), (199, 99));
    let dano = l32(&c, capa + layer::DAMAGELIST);
    assert_ne!(dano, 0);
    let rect = |c: &Caja, reg: u32| {
        let n = l32(c, reg + region::REGIONRECTANGLE);
        if n == 0 {
            return None;
        }
        assert_eq!(l32(c, n + rr::NEXT), 0);
        let b = |o: u32| l16(c, reg + o) as i16 + l16(c, n + rr::BOUNDS + o % 4) as i16;
        Some((b(0), b(2), b(4), b(6)))
    };
    assert_eq!(rect(&c, dano), Some((0, 0, 199, 99)));
    assert_ne!(l16(&c, capa + layer::FLAGS) & layer::LAYERREFRESH, 0);
    assert_eq!(clases(&mensajes(&mut c, w)), [idcmp::REFRESHWINDOW]);
    i(&mut c, "BeginRefresh", &[], &[w]);
    i(&mut c, "EndRefresh", &[1], &[w]);
    assert_eq!(rect(&c, dano), None);
    assert_eq!(l16(&c, capa + layer::FLAGS) & layer::LAYERREFRESH, 0);
    // Cambiar de tamaño: todo por pintar otra vez, y la ClipRect con el tamaño nuevo.
    let s = c.1.superficie(w).unwrap();
    c.1.comp.meter(s, Evento::Tamano { ancho: 300, alto: 150 });
    c.1.atender(&mut c.0);
    assert_eq!(rect(&c, dano), Some((0, 0, 299, 149)));
    assert_eq!(l16(&c, cr + cliprect::BOUNDS + 4), 299);
    // EndRefresh(FALSE) no la vacía.
    i(&mut c, "EndRefresh", &[0], &[w]);
    assert_eq!(rect(&c, dano), Some((0, 0, 299, 149)));
    mensajes(&mut c, w);
    // Hostil: el programa estropea los punteros de la capa; intuition usa los suyos y cierra sin perder nada.
    e32(&mut c, capa + layer::DAMAGELIST, 0xFFFF_FFF0);
    e32(&mut c, capa + layer::CLIPRECT, 0);
    i(&mut c, "EndRefresh", &[1], &[w]);
    assert_eq!(rect(&c, dano), None);
    i(&mut c, "CloseWindow", &[], &[w]);
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
    assert_eq!(c.0.bloques_de(0).len(), antes);
    // Una ventana inteligente también tiene su DamageList, vacía, y no recibe REFRESHWINDOW al abrirse.
    let w = abrir(&mut c, GADGETS);
    let capa = l32(&c, w + win::WLAYER);
    assert_eq!(rect(&c, l32(&c, capa + layer::DAMAGELIST)), None);
    assert!(clases(&mensajes(&mut c, w)).is_empty());
}

/// `DoubleClick`: dentro de 500 ms (el doble clic de DashOS), sí; más, al revés o con microsegundos imposibles, no.
#[test]
fn doble_clic() {
    let mut c = caja();
    assert_eq!(i(&mut c, "DoubleClick", &[10, 900_000, 11, 300_000], &[]), 1);
    assert_eq!(i(&mut c, "DoubleClick", &[10, 0, 10, 500_000], &[]), 1);
    assert_eq!(i(&mut c, "DoubleClick", &[10, 0, 10, 500_001], &[]), 0);
    assert_eq!(i(&mut c, "DoubleClick", &[11, 0, 10, 900_000], &[]), 0);
    assert_eq!(i(&mut c, "DoubleClick", &[10, 2_000_000, 10, 2_100_000], &[]), 0);
    assert_eq!(i(&mut c, "DoubleClick", &[u32::MAX, 999_999, 0, 0], &[]), 0);
    assert_eq!(i(&mut c, "DoubleClick", &[0, 0, u32::MAX, 0], &[]), 0);
    assert!(c.1.avisos().is_empty());
}
