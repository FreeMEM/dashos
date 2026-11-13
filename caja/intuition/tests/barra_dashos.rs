//! La caja Amiga integrada en el escritorio (rama `amiga-integrada`): con "como DashOS" y un compositor que pone su
//! barra, la ventana lleva la barra de DashOS e intuition no pinta la suya (bordes de 0 y, arriba, el alto de la letra
//! más 1, con `WBorTop` 0 en la pantalla); cerrar y el borde de DashOS llegan como `IDCMP_CLOSEWINDOW` y `IDCMP_NEWSIZE`.
//! Y los textos de nuestras bibliotecas se apuntan para escribirlos con la letra de DashOS al ampliar. Con lo malo:
//! cambiar el tema con la ventana abierta, ventanas sin barra o sin borde, textos enormes, huecos de 0 y el programa
//! pintando encima.

mod comun;

use caja_exec::lvo as exec_lvo;
use caja_intuition::compositor::BTN_LEFT;
use caja_intuition::estructuras::{idcmp, intuimessage as im, layer, rastport, screen as sc, tag, wa, wflg, window as win};
use caja_intuition::nitido::{Alinea, Estilo};
use caja_intuition::{Evento, Tokens};
use comun::*;

const T: Tokens = Tokens::CLARO;
const BUTTON: u32 = 1;
const STRING: u32 = 12;

fn etiquetas(c: &mut Caja, t: &[(u32, u32)]) -> u32 {
    let d = reservar(c, (t.len() as u32 + 1) * 8);
    for (k, (a, b)) in t.iter().enumerate() {
        e32(c, d + 8 * k as u32, *a);
        e32(c, d + 8 * k as u32 + 4, *b);
    }
    e32(c, d + 8 * t.len() as u32, tag::TAG_DONE);
    d
}

/// Una caja con el tema de DashOS y un compositor que pone la barra.
fn caja_dashos() -> Caja {
    let mut c = caja();
    c.1.comp.tema = Some(T);
    c.1.comp.barra = true;
    c.1.comp.version_tema += 1;
    c.1.atender(&mut c.0);
    c
}

/// Una ventana de 300 x 160 con un botón "Aceptar" de GadTools en (10, 20) de 80 x 14 y un campo de texto en (10, 40),
/// y las banderas `flags`.
fn ventana(c: &mut Caja, flags: u32, mas: &[(u32, u32)]) -> u32 {
    let wb = c.1.workbench();
    let vi = gt(c, "GetVisualInfoA", &[], &[wb, 0]);
    let glist = reservar(c, 4);
    let g = gt(c, "CreateContext", &[], &[glist]);
    let ng = reservar(c, 30);
    for (o, v) in [(0, 10u16), (2, 20), (4, 80), (6, 14)] {
        e16(c, ng + o, v);
    }
    let texto = cadena(c, "Aceptar");
    e32(c, ng + 8, texto);
    e32(c, ng + 22, vi);
    e32(c, ng + 18, 0x10); // PLACETEXT_IN
    let t0 = etiquetas(c, &[]);
    let g = gt(c, "CreateGadgetA", &[BUTTON], &[g, ng, t0]);
    assert_ne!(g, 0);
    e16(c, ng + 2, 40);
    e16(c, ng, 60);
    let etiqueta = cadena(c, "Texto");
    e32(c, ng + 8, etiqueta);
    e32(c, ng + 18, 1); // PLACETEXT_LEFT
    let hola = cadena(c, "hola");
    let ts = etiquetas(c, &[(0x8008_002D, hola)]); // GTST_String
    assert_ne!(gt(c, "CreateGadgetA", &[STRING], &[g, ng, ts]), 0);
    let titulo = cadena(c, "Integrada");
    let mut t = vec![
        (wa::WIDTH, 300),
        (wa::HEIGHT, 160),
        (wa::TITLE, titulo),
        (wa::GADGETS, l32(c, glist)),
        (wa::IDCMP, idcmp::CLOSEWINDOW | idcmp::NEWSIZE | idcmp::MOUSEBUTTONS | idcmp::GADGETUP),
        (wa::FLAGS, flags),
    ];
    t.extend_from_slice(mas);
    let t = etiquetas(c, &t);
    let w = i(c, "OpenWindowTagList", &[], &[0, t]);
    assert_ne!(w, 0, "{:?}", c.1.avisos());
    c.1.atender(&mut c.0);
    w
}

fn bordes(c: &Caja, w: u32) -> [u8; 4] {
    [0, 1, 2, 3].map(|k| l8(c, w + win::BORDERLEFT + k))
}

fn wbor(c: &Caja) -> [u8; 4] {
    let s = c.1.workbench();
    [l8(c, s + sc::WBORTOP), l8(c, s + sc::WBORLEFT), l8(c, s + sc::WBORRIGHT), l8(c, s + sc::WBORBOTTOM)]
}

fn mensajes(c: &mut Caja, w: u32) -> Vec<u32> {
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

const GADGETS: u32 = wflg::CLOSEGADGET | wflg::DRAGBAR | wflg::DEPTHGADGET | wflg::SIZEGADGET | wflg::ACTIVATE;

#[test]
fn barra_de_dashos_y_bordes() {
    let mut c = caja_dashos();
    // La pantalla dice que las ventanas no llevan bordes de intuition.
    assert_eq!(wbor(&c), [0, 0, 0, 0]);
    let w = ventana(&mut c, GADGETS, &[(wa::MAXWIDTH, 500), (wa::MAXHEIGHT, 400), (wa::MINWIDTH, 100), (wa::MINHEIGHT, 50)]);
    // Arriba, WBorTop (0) + la letra (8) + 1; nada más: coherente con `WBorTop + ta_YSize + 1`.
    assert_eq!(bordes(&c, w), [0, 9, 0, 0]);
    assert_eq!((l16(&c, w + win::GZZWIDTH), l16(&c, w + win::GZZHEIGHT)), (300, 151));
    let s = c.1.superficie(w).unwrap();
    let v = c.1.comp.ventana(s);
    assert!(v.pedida.barra_dashos);
    assert_eq!((v.ancho, v.alto, v.pix.len()), (300, 160, 300 * 160), "{:?}", v.pedida);
    // Sin barra de intuition: arriba del todo, la superficie (en las dos esquinas y donde estaría el título).
    for (x, y) in [(1, 1), (10, 5), (150, 4), (298, 1), (299, 159), (0, 159)] {
        assert_eq!(v.pixel(x, y), T.superficie, "({x}, {y})");
    }
    mensajes(&mut c, w);
    // Donde estaría el gadget de cerrar: un clic normal (MOUSEBUTTONS), no CLOSEWINDOW.
    for (x, y) in [(5, 5), (295, 5), (295, 155)] {
        c.1.comp.meter(s, Evento::Raton { x, y });
        c.1.comp.meter(s, Evento::Boton { boton: BTN_LEFT, pulsado: true });
        c.1.comp.meter(s, Evento::Boton { boton: BTN_LEFT, pulsado: false });
    }
    c.1.atender(&mut c.0);
    let m = mensajes(&mut c, w);
    assert!(!m.contains(&idcmp::CLOSEWINDOW) && m.contains(&idcmp::MOUSEBUTTONS), "{m:x?}");
    assert!(!c.1.comp.registro.iter().any(|r| r.starts_with("arrastrar") || r.starts_with("redimensionar") || r.starts_with("profundidad")));
    // El botón de cerrar de la barra de DashOS: CLOSEWINDOW.
    c.1.comp.meter(s, Evento::Cerrar);
    c.1.atender(&mut c.0);
    assert_eq!(mensajes(&mut c, w), [idcmp::CLOSEWINDOW]);
    // El borde de DashOS: NEWSIZE, dentro de WindowLimits (absurdos, al límite).
    // (0 x 0 no llega desde la caja, que no manda tamaños de 0; aquí, lo más pequeño que deja.)
    for (pide, queda) in [((400, 300), (400, 300)), ((10, 10), (100, 50)), ((0, 0), (100, 50)), ((9000, 9000), (500, 400))] {
        c.1.comp.meter(s, Evento::Tamano { ancho: pide.0, alto: pide.1 });
        c.1.atender(&mut c.0);
        assert_eq!((l16(&c, w + win::WIDTH), l16(&c, w + win::HEIGHT)), queda, "pide {pide:?}");
    }
    assert!(mensajes(&mut c, w).contains(&idcmp::NEWSIZE));
    // Tras agrandar, lo de abajo a la derecha (antes fuera) es superficie y los bordes siguen igual.
    let v = c.1.comp.ventana(s);
    assert_eq!(v.pixel(499, 399), T.superficie);
    assert_eq!(bordes(&c, w), [0, 9, 0, 0]);
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
}

#[test]
fn gimmezerozero_y_ventanas_sin_barra() {
    let mut c = caja_dashos();
    // GimmeZeroZero: la capa empieza bajo el borde de arriba (9) y ocupa todo lo demás.
    let w = ventana(&mut c, GADGETS | wflg::GIMMEZEROZERO, &[]);
    let capa = l32(&c, l32(&c, w + win::RPORT) + rastport::LAYER);
    let b = capa + layer::BOUNDS;
    assert_eq!([l16(&c, b), l16(&c, b + 2), l16(&c, b + 4), l16(&c, b + 6)], [0, 9, 299, 159]);
    // Sin título ni gadgets: sin barra (ni de DashOS ni de intuition) y sin bordes.
    let w2 = ventana(&mut c, wflg::ACTIVATE, &[(wa::TITLE, 0)]);
    assert_eq!(bordes(&c, w2), [0, 0, 0, 0]);
    assert!(!c.1.comp.ventana(c.1.superficie(w2).unwrap()).pedida.barra_dashos);
    // Sin borde (la que se pinta su propia barra): nada de DashOS, bordes de 0.
    let w3 = ventana(&mut c, wflg::BORDERLESS | wflg::DRAGBAR, &[]);
    assert_eq!(bordes(&c, w3), [0, 0, 0, 0]);
    assert!(!c.1.comp.ventana(c.1.superficie(w3).unwrap()).pedida.barra_dashos);
    // De fondo: los bordes de DashOS pero sin su barra (no se mueve).
    let w4 = ventana(&mut c, wflg::BACKDROP | wflg::DRAGBAR, &[]);
    assert!(!c.1.comp.ventana(c.1.superficie(w4).unwrap()).pedida.barra_dashos);
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
}

#[test]
fn original_y_compositor_sin_barra() {
    // El programa con el aspecto original: los bordes de 3.x y su barra de intuition.
    let mut c = caja_dashos();
    c.1.comp.originales = vec!["principal".into()];
    c.1.comp.version_tema += 1;
    c.1.atender(&mut c.0);
    let w = ventana(&mut c, GADGETS, &[]);
    assert_eq!(bordes(&c, w), [4, 11, 18, 2]);
    assert!(!c.1.comp.ventana(c.1.superficie(w).unwrap()).pedida.barra_dashos);
    // (La pantalla es de la caja entera: va "como DashOS".)
    assert_eq!(wbor(&c), [0, 0, 0, 0]);
    // Un compositor que no pone barra: como antes (la barra de intuition pintada como Material).
    let mut c = caja();
    c.1.comp.tema = Some(T);
    c.1.comp.version_tema += 1;
    c.1.atender(&mut c.0);
    assert_eq!(wbor(&c), [2, 4, 4, 2]);
    let w = ventana(&mut c, GADGETS, &[]);
    assert_eq!(bordes(&c, w), [4, 11, 18, 2]);
}

#[test]
fn cambiar_tema_con_la_ventana_abierta() {
    let mut c = caja_dashos();
    let w = ventana(&mut c, GADGETS, &[]);
    let s = c.1.superficie(w).unwrap();
    // A "original": la ventana sigue con sus bordes (el programa ya colocó sus cosas) y su barra de DashOS; dentro,
    // lo clásico. La pantalla vuelve a los bordes de 3.x para las ventanas nuevas.
    c.1.comp.tema = None;
    c.1.comp.version_tema += 1;
    c.1.atender(&mut c.0);
    assert_eq!(bordes(&c, w), [0, 9, 0, 0]);
    assert_eq!(wbor(&c), [2, 4, 4, 2]);
    let v = c.1.comp.ventana(s);
    assert_ne!(v.pixel(150, 4), T.superficie);
    assert!(v.textos.is_empty(), "con el clásico no hay letra de DashOS");
    let w2 = ventana(&mut c, GADGETS, &[]);
    assert_eq!(bordes(&c, w2), [4, 11, 18, 2]);
    // Y de vuelta: la de antes, otra vez con la superficie; la nueva conserva su barra de intuition (Material).
    c.1.comp.tema = Some(T);
    c.1.comp.version_tema += 1;
    c.1.atender(&mut c.0);
    assert_eq!(c.1.comp.ventana(s).pixel(150, 4), T.superficie);
    assert_eq!(bordes(&c, w2), [4, 11, 18, 2]);
    assert!(!c.1.comp.ventana(s).textos.is_empty());
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
}

#[test]
fn textos_nitidos() {
    let mut c = caja_dashos();
    let w = ventana(&mut c, GADGETS, &[]);
    let s = c.1.superficie(w).unwrap();
    let v = c.1.comp.ventana(s);
    // La etiqueta del botón, centrada y gruesa; el texto del campo, en celdas. Todos valen.
    let boton = v.textos.iter().find(|t| t.texto == b"Aceptar").expect("la etiqueta del botón");
    assert_eq!((boton.alinea, boton.estilo), (Alinea::Centro, Estilo::Grueso));
    assert!(boton.r.x0 >= 10 && boton.r.x1 < 90 && boton.r.ancho() == 7 * 8 && boton.r.alto() == 8);
    let campo = v.textos.iter().find(|t| t.texto == b"hola").expect("el texto del campo");
    assert_eq!(campo.estilo, Estilo::Celdas);
    for t in &v.textos {
        assert!(t.vale(&v.pix, v.ancho, v.alto), "{:?}", t.texto);
    }
    // La etiqueta de fuera del campo se repinta sin borrar: lo de debajo sigue siendo la superficie, sin la topaz.
    for _ in 0..3 {
        gt(&mut c, "GT_RefreshWindow", &[], &[w, 0]);
        let primero = l32(&c, w + win::FIRSTGADGET);
        i(&mut c, "RefreshGList", &[u32::MAX], &[primero, w, 0]);
    }
    c.1.atender(&mut c.0);
    let v = c.1.comp.ventana(s);
    let fuera = v.textos.iter().find(|t| t.texto == b"Texto").expect("la etiqueta de fuera");
    assert_eq!(fuera.alinea, Alinea::Derecha);
    assert!(fuera.vale(&v.pix, v.ancho, v.alto));
    assert!(fuera.antes.iter().all(|&p| p | 0xFF00_0000 == T.superficie), "la topaz en el fondo de la etiqueta");
    // El programa pinta encima de la etiqueta: deja de valer (sale tal cual, con la topaz).
    let rp = l32(&c, w + win::RPORT);
    pen(&mut c, rp, 1);
    rect(&mut c, rp, 30, 22, 40, 30);
    c.1.atender(&mut c.0);
    let v = c.1.comp.ventana(s);
    let boton = v.textos.iter().find(|t| t.texto == b"Aceptar").unwrap();
    assert!(!boton.vale(&v.pix, v.ancho, v.alto));
    // Pulsar el botón lo repinta: vuelve a valer (otro color, el del botón pulsado).
    c.1.comp.meter(s, Evento::Raton { x: 50, y: 27 });
    c.1.comp.meter(s, Evento::Boton { boton: BTN_LEFT, pulsado: true });
    c.1.atender(&mut c.0);
    let v = c.1.comp.ventana(s);
    let boton = v.textos.iter().find(|t| t.texto == b"Aceptar").unwrap();
    assert!(boton.vale(&v.pix, v.ancho, v.alto));
    assert_eq!(boton.color, T.sobre_primario);
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
}

#[test]
fn textos_nitidos_hostiles() {
    let mut c = caja_dashos();
    // Una etiqueta enorme (más que la ventana) y un botón pegado al borde: el texto que no cabe en la ventana no se
    // apunta; nada se cae.
    let wb = c.1.workbench();
    let vi = gt(&mut c, "GetVisualInfoA", &[], &[wb, 0]);
    let glist = reservar(&mut c, 4);
    let mut g = gt(&mut c, "CreateContext", &[], &[glist]);
    let largo: String = "M".repeat(3000);
    let largo = cadena(&mut c, &largo);
    let ng = reservar(&mut c, 30);
    for (x, y, an, texto) in [(10u16, 20u16, 80u16, largo), (295, 20, 4, largo), (0, 150, 0, largo), (100, 60, 60, 0)] {
        for (o, v) in [(0, x), (2, y), (4, an), (6, 14)] {
            e16(&mut c, ng + o, v);
        }
        e32(&mut c, ng + 8, texto);
        e32(&mut c, ng + 22, vi);
        e32(&mut c, ng + 18, 0x10);
        let t0 = etiquetas(&mut c, &[]);
        g = gt(&mut c, "CreateGadgetA", &[BUTTON], &[g, ng, t0]);
        assert_ne!(g, 0);
    }
    let primero = l32(&c, glist);
    let t = etiquetas(&mut c, &[(wa::WIDTH, 300), (wa::HEIGHT, 160), (wa::GADGETS, primero), (wa::FLAGS, GADGETS)]);
    let w = i(&mut c, "OpenWindowTagList", &[], &[0, t]);
    assert_ne!(w, 0);
    c.1.atender(&mut c.0);
    let s = c.1.superficie(w).unwrap();
    let v = c.1.comp.ventana(s);
    for t in &v.textos {
        assert!(t.r.x0 >= 0 && t.r.y0 >= 0 && t.r.x1 < 300 && t.r.y1 < 160, "{:?}", t.r);
        assert!(t.texto.len() <= caja_intuition::nitido::MAX_LETRAS);
        assert!(!t.texto.is_empty() && !t.r.vacio());
    }
    // Muchos repintados: los textos guardados tienen tope.
    for k in 0..300 {
        c.1.comp.meter(s, Evento::Raton { x: 50, y: 27 });
        c.1.comp.meter(s, Evento::Boton { boton: BTN_LEFT, pulsado: k % 2 == 0 });
        c.1.comp.meter(s, Evento::Tamano { ancho: 200 + (k % 50), alto: 100 + (k % 30) });
        c.1.atender(&mut c.0);
    }
    assert!(c.1.comp.ventana(s).textos.len() <= caja_intuition::nitido::MAX_TEXTOS);
}

#[test]
fn requester_con_la_barra_de_dashos() {
    let mut c = caja_dashos();
    let es = reservar(&mut c, 20);
    e32(&mut c, es, 20);
    let (t, f, b) = (cadena(&mut c, "Dash"), cadena(&mut c, "Error"), cadena(&mut c, "Vale|Salir"));
    e32(&mut c, es + 8, t);
    e32(&mut c, es + 12, f);
    e32(&mut c, es + 16, b);
    let n = c.1.numero_intuition("EasyRequestArgs").unwrap();
    let mut r = regs(&[], &[0, es, 0, 0]);
    assert_eq!(c.1.llamar(&mut c.0, n, &mut r), caja_exec::Accion::Bloquear);
    let w = *c.1.ventanas().last().unwrap();
    assert_eq!(bordes(&c, w), [0, 9, 0, 0]);
    let s = c.1.superficie(w).unwrap();
    assert!(c.1.comp.ventana(s).pedida.barra_dashos);
    // El texto y los botones, con la letra de DashOS.
    c.1.atender(&mut c.0);
    assert!(c.1.comp.ventana(s).textos.iter().any(|t| t.texto == b"Salir"));
    // Cerrar desde la barra: como "Salir" (0).
    c.1.comp.meter(s, Evento::Cerrar);
    c.1.comp.meter(s, Evento::Cerrar);
    c.1.atender(&mut c.0);
    let mut r = regs(&[], &[0, es, 0, 0]);
    assert_eq!(c.1.llamar(&mut c.0, n, &mut r), caja_exec::Accion::Seguir);
    assert_eq!(r.d[0], 0);
    assert!(c.1.ventanas().is_empty());
}

/// BorderTop en los dos aspectos, frente al oráculo (FS-UAE, Kickstart y Workbench 3.1): allí la pantalla da `WBorTop`
/// 2, `WBorLeft` 4, `WBorRight` 4 y `WBorBottom` 2, y una ventana con barra, `BorderTop` = `WBorTop` + alto de la letra
/// de la pantalla + 1 (12 con topaz/9; aquí la letra es topaz/8: 11), 4 a la izquierda, 18 a la derecha con el gadget de
/// tamaño y 2 abajo; los gadgets de la barra, del alto de `BorderTop`. "Como DashOS" (con la barra de DashOS), la misma
/// cuenta con `WBorTop` 0: 9.
#[test]
fn border_top_en_los_dos_aspectos() {
    // Original (con un compositor que pone barras: no la usa).
    let mut c = caja();
    c.1.comp.barra = true;
    c.1.comp.version_tema += 1;
    c.1.atender(&mut c.0);
    let alto_letra = caja_intuition::FUENTE_ALTO as u8;
    assert_eq!(wbor(&c), [2, 4, 4, 2]);
    let w = ventana(&mut c, GADGETS, &[]);
    let b = bordes(&c, w);
    assert_eq!(b, [4, wbor(&c)[0] + alto_letra + 1, 18, 2]);
    assert!(!c.1.comp.ventana(c.1.superficie(w).unwrap()).pedida.barra_dashos);
    // La barra de intuition, de todo su alto: la fila de abajo de la barra es del borde (sombra del relieve de dentro), y
    // el título cabe entero entre la primera y la última fila.
    let s = c.1.superficie(w).unwrap();
    let v = c.1.comp.ventana(s);
    let ba = b[1] as u32;
    let fondo_barra = v.pixel(150, 1);
    let filas_con_titulo: Vec<u32> = (0..ba).filter(|&y| (24..80).any(|x| v.pixel(x, y) != fondo_barra && v.pixel(x, y) != v.pixel(23, y))).collect();
    assert!(!filas_con_titulo.is_empty(), "el título");
    assert!(filas_con_titulo[0] >= 1 && *filas_con_titulo.last().unwrap() <= ba - 2, "el título cabe en la barra: {filas_con_titulo:?} en {ba}");
    // El gadget de profundidad, del alto de la barra: su relieve llega a la fila ba - 1.
    let an = v.ancho;
    assert_ne!(v.pixel(an - 10, ba - 1), v.pixel(an - 10, ba / 2), "el gadget de profundidad acaba en la última fila de la barra");
    // Como DashOS.
    let mut c = caja_dashos();
    assert_eq!(wbor(&c), [0, 0, 0, 0]);
    let w = ventana(&mut c, GADGETS, &[]);
    assert_eq!(bordes(&c, w), [0, wbor(&c)[0] + alto_letra + 1, 0, 0]);
}
