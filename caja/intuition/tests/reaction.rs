//! ReAction sobre el BOOPSI nativo: window.class con un layout como los de Dash (`ra_window_create` y compañía), el
//! diseño automático (mínimos, pesos, columnas de etiquetas, grupos anidados), los clics que llegan como
//! `WMHI_GADGETUP`, las listas de nodos (chooser, listbrowser, radiobutton), las dos apariencias con los mismos
//! rectángulos y lo malo (hijos que no son objetos, miles de hijos, tamaños absurdos, anidados sin fin).

mod comun;

use caja_exec::Registros;
use caja_exec::estructuras::execbase;
use caja_intuition::boopsi::cte::*;
use caja_intuition::compositor::Tokens;
use caja_intuition::estructuras::{tag, wa, window as win};
use caja_intuition::{Evento, Hecho};
use comun::puente::*;
use comun::*;

const RD: u32 = 0x8500_0000;
const WINDOW_SIGMASK: u32 = RD + 0x25000 + 2;
const WINDOW_LAYOUT: u32 = RD + 0x25000 + 5;
const WINDOW_POSITION: u32 = RD + 0x25000 + 14;
const WINDOW_WINDOW: u32 = RD + 0x25000 + 1;
const WM_HANDLEINPUT: u32 = 0x570001;
const WM_OPEN: u32 = 0x570002;
const WM_CLOSE: u32 = 0x570003;
const WMHI_GADGETUP: u32 = 2 << 16;
const WMHI_CLOSEWINDOW: u32 = 1 << 16;
const WMHI_NEWSIZE: u32 = 5 << 16;
const L: u32 = RD + 0x7000;
const LAYOUT_ORIENTATION: u32 = L + 1;
const LAYOUT_BEVELSTYLE: u32 = L + 15;
const LAYOUT_ADDCHILD: u32 = L + 20;
const LAYOUT_ADDIMAGE: u32 = L + 21;
const LAYOUT_SPACEOUTER: u32 = L + 38;
const LAYOUT_SPACEINNER: u32 = L + 37;
const LAYOUT_LABEL: u32 = L + 16;
const CHILD_WEIGHTEDWIDTH: u32 = L + 0x100 + 5;
const CHILD_WEIGHTEDHEIGHT: u32 = L + 0x100 + 6;
const CHILD_MINWIDTH: u32 = L + 0x100 + 1;
const CHILD_LABEL: u32 = L + 0x100 + 12;
const SLIDER_MIN: u32 = RD + 0x28000 + 1;
const SLIDER_MAX: u32 = RD + 0x28000 + 2;
const SLIDER_LEVEL: u32 = RD + 0x28000 + 3;
const SLIDER_ORIENTATION: u32 = RD + 0x28000 + 4;
const CHOOSER_LABELS: u32 = RD + 0x1000 + 4;
const CHOOSER_SELECTED: u32 = RD + 0x1000 + 5;
const CNA_TEXT: u32 = 0x8500_1501;
const LISTBROWSER_LABELS: u32 = RD + 0x3000 + 3;
const LISTBROWSER_SELECTED: u32 = RD + 0x3000 + 4;
const LBNCA_TEXT: u32 = 0x8500_3505;
const LBNCA_COPYTEXT: u32 = 0x8500_3500 + 15;
const RADIOBUTTON_LABELS: u32 = RD + 0x14000 + 1;
const RADIOBUTTON_SELECTED: u32 = RD + 0x14000 + 4;
const RBNA_LABEL: u32 = 0x8002_0002;
const INTEGER_NUMBER: u32 = RD + 0x2000 + 1;
const INTEGER_MINIMUM: u32 = RD + 0x2000 + 3;
const INTEGER_MAXIMUM: u32 = RD + 0x2000 + 4;
const LABEL_TEXT: u32 = RD + 0x6000 + 1;
const BEVEL_STYLE: u32 = RD + 0x16000 + 1;
const BVS_GROUP: u32 = 2;
const BVS_SBAR_HORIZ: u32 = 6;

/// El número nativo de la función `k` (0: `GetClass`) de una biblioteca de clases.
fn funcion_lib(p: &mut Prueba, nombre: &str, k: u32) -> u16 {
    let lista = p.c.0.base() + execbase::LIBLIST;
    let base = p.c.0.find_name(lista, nombre.as_bytes()).unwrap();
    assert_ne!(base, 0, "{nombre}");
    l16(&p.c, base - 30 - 6 * k + 2)
}

fn lib(p: &mut Prueba, nombre: &str, k: u32, d: &[u32], a: &[u32]) -> u32 {
    let n = funcion_lib(p, nombre, k);
    let mut r = regs(d, a);
    assert_eq!(caja_intuition::boopsi::llamar(p, n, &mut r), Hecho::Seguir);
    r.d[0]
}

fn clase(p: &mut Prueba, nombre: &str) -> u32 {
    let c = lib(p, nombre, 0, &[], &[]);
    assert_ne!(c, 0, "{nombre}");
    c
}

fn metodo(p: &mut Prueba, o: u32, palabras: &[u32]) -> u32 {
    let m = mensaje(p, palabras);
    do_method(p, o, m).unwrap()
}

fn caja_g(p: &Prueba, g: u32) -> (i32, i32, i32, i32) {
    (
        l16(&p.c, g + 4) as i16 as i32,
        l16(&p.c, g + 6) as i16 as i32,
        l16(&p.c, g + 8) as i16 as i32,
        l16(&p.c, g + 10) as i16 as i32,
    )
}

/// Todos los WMHI de la ventana, hasta WMHI_LASTMSG.
fn eventos(p: &mut Prueba, wo: u32) -> Vec<(u32, u16)> {
    let codigo = reservar(&mut p.c, 4);
    let mut v = Vec::new();
    for _ in 0..64 {
        let r = metodo(p, wo, &[WM_HANDLEINPUT, codigo]);
        if r == 0 {
            break;
        }
        v.push((r, l16(&p.c, codigo)));
    }
    v
}

/// Lo de `examples/test_doc_reaction.dash`: una ventana de ajustes con un campo, una casilla y dos botones en un grupo
/// horizontal con un separador, más un deslizador, una etiqueta y un entero.
struct Ventana {
    wo: u32,
    w: u32,
    raiz: u32,
    cadena: u32,
    casilla: u32,
    aceptar: u32,
    cancelar: u32,
    deslizador: u32,
    entero: u32,
}

fn ventana_ajustes(p: &mut Prueba) -> Ventana {
    let c_layout = clase(p, "layout.gadget");
    let c_ventana = clase(p, "window.class");
    let raiz = nuevo(p, c_layout, "", &[(LAYOUT_ORIENTATION, 1), (LAYOUT_SPACEOUTER, 1), (LAYOUT_BEVELSTYLE, BVS_GROUP)]);
    let c_label = clase(p, "label.image");
    let t = cadena(&mut p.c, "Bienvenido a ReAction");
    let etiqueta = nuevo(p, c_label, "", &[(LABEL_TEXT, t)]);
    set_g(p, raiz, 0, &[(LAYOUT_ADDIMAGE, etiqueta)]);
    let c_string = clase(p, "string.gadget");
    let amiga = cadena(&mut p.c, "Amiga");
    let cad = nuevo(p, c_string, "", &[(ga::ID, 0), (ga::RELVERIFY, 1), (stringa::TEXTVAL, amiga), (stringa::MAXCHARS, 40)]);
    let t = cadena(&mut p.c, "Nombre");
    let et_cad = nuevo(p, c_label, "", &[(LABEL_TEXT, t)]);
    set_g(p, raiz, 0, &[(LAYOUT_ADDCHILD, cad), (CHILD_LABEL, et_cad)]);
    let c_check = clase(p, "checkbox.gadget");
    let t = cadena(&mut p.c, "Sonido");
    let casilla = nuevo(p, c_check, "", &[(ga::ID, 1), (ga::TEXT, t), (ga::RELVERIFY, 1), (ga::SELECTED, 1)]);
    set_g(p, raiz, 0, &[(LAYOUT_ADDCHILD, casilla)]);
    let c_slider = clase(p, "slider.gadget");
    let desl = nuevo(p, c_slider, "", &[(ga::ID, 4), (ga::RELVERIFY, 1), (SLIDER_MIN, 0), (SLIDER_MAX, 100), (SLIDER_LEVEL, 50), (SLIDER_ORIENTATION, pga::FREEHORIZ)]);
    set_g(p, raiz, 0, &[(LAYOUT_ADDCHILD, desl)]);
    let c_int = clase(p, "integer.gadget");
    let entero = nuevo(p, c_int, "", &[(ga::ID, 5), (ga::RELVERIFY, 1), (INTEGER_NUMBER, 7), (INTEGER_MINIMUM, 0), (INTEGER_MAXIMUM, 99)]);
    set_g(p, raiz, 0, &[(LAYOUT_ADDCHILD, entero)]);
    // ra_layout_hgroup_begin / ra_add_button / ra_add_space / ra_layout_group_end.
    let hgrupo = nuevo(p, c_layout, "", &[(LAYOUT_ORIENTATION, 0), (LAYOUT_SPACEOUTER, 1), (LAYOUT_SPACEINNER, 1)]);
    let t = cadena(&mut p.c, "_Aceptar");
    let aceptar = nuevo(p, 0, "button.gadget", &[(ga::ID, 2), (ga::TEXT, t), (ga::RELVERIFY, 1)]);
    set_g(p, hgrupo, 0, &[(LAYOUT_ADDCHILD, aceptar)]);
    let c_bevel = clase(p, "bevel.image");
    let sep = nuevo(p, c_bevel, "", &[(BEVEL_STYLE, BVS_SBAR_HORIZ)]);
    set_g(p, hgrupo, 0, &[(LAYOUT_ADDIMAGE, sep)]);
    let t = cadena(&mut p.c, "_Cancelar");
    let cancelar = nuevo(p, 0, "button.gadget", &[(ga::ID, 3), (ga::TEXT, t), (ga::RELVERIFY, 1)]);
    set_g(p, hgrupo, 0, &[(LAYOUT_ADDCHILD, cancelar)]);
    set_g(p, raiz, 0, &[(LAYOUT_ADDCHILD, hgrupo)]);
    let titulo = cadena(&mut p.c, "Ajustes");
    let wo = nuevo(
        p,
        c_ventana,
        "",
        &[
            (wa::TITLE, titulo),
            (wa::WIDTH, 260),
            (wa::HEIGHT, 120),
            (wa::ACTIVATE, 1),
            (wa::DEPTHGADGET, 1),
            (wa::DRAGBAR, 1),
            (wa::CLOSEGADGET, 1),
            (wa::SIZEGADGET, 1),
            (WINDOW_POSITION, 1),
            (WINDOW_LAYOUT, raiz),
        ],
    );
    assert_ne!(wo, 0);
    let w = metodo(p, wo, &[WM_OPEN, 0]);
    assert_ne!(w, 0, "WM_OPEN");
    atender(p);
    Ventana { wo, w, raiz, cadena: cad, casilla, aceptar, cancelar, deslizador: desl, entero }
}

fn centro(p: &Prueba, g: u32) -> (i32, i32) {
    let (x, y, an, al) = caja_g(p, g);
    (x + an / 2, y + al / 2)
}

#[test]
fn ventana_como_la_de_dash() {
    let mut p = prueba();
    let v = ventana_ajustes(&mut p);
    let w = v.w;
    assert_eq!(get(&mut p, v.wo, WINDOW_WINDOW), w);
    assert_ne!(get(&mut p, v.wo, WINDOW_SIGMASK), 0);
    // La ventana cabe lo que pide el layout, y el layout ocupa lo de dentro.
    let (an, al) = (l16(&p.c, w + win::WIDTH) as i32, l16(&p.c, w + win::HEIGHT) as i32);
    let (lx, ly, la, lh) = caja_g(&p, v.raiz);
    assert!(lx >= 0 && ly >= 0 && lx + la <= an && ly + lh <= al, "layout {:?} en {an}x{al}", caja_g(&p, v.raiz));
    // Los hijos, dentro del layout y uno debajo de otro, sin pisarse.
    let hijos = [v.cadena, v.casilla, v.deslizador, v.entero, v.aceptar];
    let mut abajo = ly;
    for &g in &hijos {
        let (x, y, a, h) = caja_g(&p, g);
        assert!(x >= lx && x + a <= lx + la && y + h <= ly + lh, "{g:x}: {:?} fuera de {:?}", caja_g(&p, g), caja_g(&p, v.raiz));
        assert!(a > 0 && h > 0);
        assert!(y >= abajo, "{g:x} pisa al de arriba");
        abajo = y + h;
    }
    // La columna de etiquetas: el campo empieza a la derecha de "Nombre" (6 letras).
    assert!(caja_g(&p, v.cadena).0 >= lx + 6 * 8, "{:?}", caja_g(&p, v.cadena));
    // Los dos botones en la misma fila, el de cancelar a la derecha.
    let (ax, ay, aa, _) = caja_g(&p, v.aceptar);
    let (cx, cy, _, _) = caja_g(&p, v.cancelar);
    assert_eq!(ay, cy);
    assert!(cx > ax + aa, "{:?} {:?}", caja_g(&p, v.aceptar), caja_g(&p, v.cancelar));
    // Se ve algo: el texto de los botones y el marco del grupo.
    let bm = rl32(&p, rl32(&p, w + win::RPORT) + 4);
    let l = |x: i32, y: i32| p.c.1.pixel_de(&p.c.0, bm, x, y).unwrap_or(0);
    let fondo = l(an - 2, al / 2);
    let (bx, by) = centro(&p, v.aceptar);
    assert!((bx - 20..bx + 20).any(|x| l(x, by) != fondo), "el botón no se pintó");
    // La etiqueta "Nombre", en la columna de la izquierda del campo.
    let (cx0, cy0, _, ch) = caja_g(&p, v.cadena);
    assert!((lx..cx0).any(|x| (cy0..cy0 + ch).any(|y| l(x, y) != l(lx + 1, cy0 - 1))), "la etiqueta no se pintó");
    // Clic en "Aceptar": WMHI_GADGETUP con su GA_ID.
    clic(&mut p, w, bx, by);
    let e = eventos(&mut p, v.wo);
    assert!(e.iter().any(|x| x.0 == WMHI_GADGETUP | 2), "{e:x?}");
    // La casilla: de marcada a quitada, con su código.
    assert_eq!(get(&mut p, v.casilla, ga::SELECTED), 1);
    let (x, y, _, h) = caja_g(&p, v.casilla);
    clic(&mut p, w, x + 6, y + h / 2);
    assert_eq!(get(&mut p, v.casilla, ga::SELECTED), 0);
    let e = eventos(&mut p, v.wo);
    assert!(e.contains(&(WMHI_GADGETUP | 1, 0)), "{e:x?}");
    // El deslizador, al final.
    let (x, y, a, h) = caja_g(&p, v.deslizador);
    arrastrar(&mut p, w, (x + a / 2, y + h / 2), (x + a + 40, y + h / 2));
    assert_eq!(get(&mut p, v.deslizador, SLIDER_LEVEL), 100);
    let e = eventos(&mut p, v.wo);
    assert!(e.contains(&(WMHI_GADGETUP | 4, 100)), "{e:x?}");
    // El campo de texto: borrar y escribir.
    let (cx, cy) = centro(&p, v.cadena);
    clic(&mut p, w, cx + 60, cy);
    for _ in 0..5 {
        tecla(&mut p, w, 14, None);
    }
    for (k, ch) in [(0x20u32, 'D'), (0x18, 'a'), (0x1f, 's'), (0x23, 'h')] {
        tecla(&mut p, w, k, Some(ch));
    }
    tecla(&mut p, w, 28, Some('\r'));
    let buf = get(&mut p, v.cadena, stringa::TEXTVAL);
    assert_eq!(c_str(&p, buf), "Dash");
    let e = eventos(&mut p, v.wo);
    assert!(e.iter().any(|x| x.0 == WMHI_GADGETUP), "{e:x?}");
    // El entero, con su máximo.
    let (ex_, ey) = centro(&p, v.entero);
    clic(&mut p, w, ex_ + 40, ey);
    for (k, ch) in [(0x09u32, '8'), (0x0a, '9')] {
        tecla(&mut p, w, k, Some(ch));
    }
    tecla(&mut p, w, 28, Some('\r'));
    assert_eq!(get(&mut p, v.entero, INTEGER_NUMBER), 99, "7 y 89 → 789, al máximo");
    // Cerrar desde la barra: WMHI_CLOSEWINDOW.
    let s = p.c.1.superficie(w).unwrap();
    p.c.1.comp.meter(s, Evento::Cerrar);
    atender(&mut p);
    let e = eventos(&mut p, v.wo);
    assert!(e.iter().any(|x| x.0 == WMHI_CLOSEWINDOW), "{e:x?}");
    metodo(&mut p, v.wo, &[WM_CLOSE]);
    assert_eq!(get(&mut p, v.wo, WINDOW_WINDOW), 0);
    // DisposeObject de la ventana se lleva el layout y todos sus hijos (y las etiquetas).
    dispose(&mut p, v.wo);
    assert_eq!(p.c.1.objetos_boopsi(), 0);
    assert_eq!(avisos(&p), 0, "{:?}", p.c.1.avisos());
}

#[test]
fn cambiar_de_tamano() {
    let mut p = prueba();
    let v = ventana_ajustes(&mut p);
    let antes = caja_g(&p, v.aceptar);
    let s = p.c.1.superficie(v.w).unwrap();
    p.c.1.comp.meter(s, Evento::Tamano { ancho: 500, alto: 300 });
    atender(&mut p);
    let e = eventos(&mut p, v.wo);
    assert!(e.iter().any(|x| x.0 == WMHI_NEWSIZE), "{e:x?}");
    atender(&mut p);
    let (_, _, la, lh) = caja_g(&p, v.raiz);
    assert!(la > 400 && lh > 200, "{:?}", caja_g(&p, v.raiz));
    let despues = caja_g(&p, v.aceptar);
    assert!(despues.2 > antes.2, "los botones se estiran: {antes:?} → {despues:?}");
    metodo(&mut p, v.wo, &[WM_CLOSE]);
    dispose(&mut p, v.wo);
    assert_eq!(p.c.1.objetos_boopsi(), 0);
}

/// Una lista de exec con los nodos de `alloc` (la función 1 de la biblioteca de clases).
fn lista_nodos(p: &mut Prueba, biblioteca: &str, tag_texto: u32, columnas: bool, textos: &[&str]) -> u32 {
    let lista = reservar(&mut p.c, 14);
    p.c.0.new_list(lista, 0).unwrap();
    for t in textos {
        let s = cadena(&mut p.c, t);
        let e = etiquetas(p, &[(tag_texto, s), (LBNCA_COPYTEXT, 1)]);
        let n = if columnas { lib(p, biblioteca, 1, &[1], &[e]) } else { lib(p, biblioteca, 1, &[], &[e]) };
        assert_ne!(n, 0);
        p.c.0.add_tail(lista, n).unwrap();
    }
    lista
}

#[test]
fn listas_de_nodos() {
    let mut p = prueba();
    let c_layout = clase(&mut p, "layout.gadget");
    let raiz = nuevo(&mut p, c_layout, "", &[(LAYOUT_ORIENTATION, 1), (LAYOUT_SPACEOUTER, 1)]);
    let colores = lista_nodos(&mut p, "chooser.gadget", CNA_TEXT, false, &["Rojo", "Verde", "Azul"]);
    let ch = nuevo(&mut p, 0, "chooser.gadget", &[(ga::ID, 1), (ga::RELVERIFY, 1), (CHOOSER_LABELS, colores), (CHOOSER_SELECTED, 0)]);
    let filas = lista_nodos(&mut p, "listbrowser.gadget", LBNCA_TEXT, true, &["uno", "dos", "tres", "cuatro"]);
    let lb = nuevo(&mut p, 0, "listbrowser.gadget", &[(ga::ID, 2), (ga::RELVERIFY, 1), (LISTBROWSER_LABELS, filas)]);
    let opciones = lista_nodos(&mut p, "radiobutton.gadget", RBNA_LABEL, true, &["A", "B", "C"]);
    let rb = nuevo(&mut p, 0, "radiobutton.gadget", &[(ga::ID, 3), (ga::RELVERIFY, 1), (RADIOBUTTON_LABELS, opciones)]);
    for g in [ch, lb, rb] {
        set_g(&mut p, raiz, 0, &[(LAYOUT_ADDCHILD, g), (CHILD_WEIGHTEDHEIGHT, if g == lb { 100 } else { 0 })]);
    }
    let wo = nuevo(&mut p, 0, "window.class", &[(wa::WIDTH, 200), (wa::HEIGHT, 200), (WINDOW_LAYOUT, raiz)]);
    let w = metodo(&mut p, wo, &[WM_OPEN, 0]);
    assert_ne!(w, 0);
    atender(&mut p);
    // El chooser: un clic, la siguiente opción.
    let (x, y) = centro(&p, ch);
    clic(&mut p, w, x, y);
    assert_eq!(get(&mut p, ch, CHOOSER_SELECTED), 1);
    let e = eventos(&mut p, wo);
    assert!(e.contains(&(WMHI_GADGETUP | 1, 1)), "{e:x?}");
    // La lista: la tercera fila.
    let (lx, ly, _, _) = caja_g(&p, lb);
    clic(&mut p, w, lx + 10, ly + 2 + 2 * 9 + 4);
    assert_eq!(get(&mut p, lb, LISTBROWSER_SELECTED), 2);
    let e = eventos(&mut p, wo);
    assert!(e.contains(&(WMHI_GADGETUP | 2, 2)), "{e:x?}");
    // Los botones de radio: el tercero.
    let (rx, ry, _, _) = caja_g(&p, rb);
    clic(&mut p, w, rx + 4, ry + 2 * 11 + 3);
    assert_eq!(get(&mut p, rb, RADIOBUTTON_SELECTED), 2);
    // GetChooserNodeAttrs: el texto copiado.
    let primero = rl32(&p, colores);
    let st = reservar(&mut p.c, 4);
    let e = etiquetas(&mut p, &[(CNA_TEXT, st)]);
    lib(&mut p, "chooser.gadget", 4, &[], &[primero, e]);
    assert_eq!(c_str(&p, rl32(&p, st)), "Rojo");
    metodo(&mut p, wo, &[WM_CLOSE]);
    dispose(&mut p, wo);
    // Los nodos los suelta el programa: FreeListBrowserList y FreeChooserNode uno a uno.
    lib(&mut p, "listbrowser.gadget", 10, &[], &[filas]);
    for l in [colores, opciones] {
        loop {
            let n = p.c.0.rem_head(l).unwrap();
            if n == 0 {
                break;
            }
            let b = if l == colores { "chooser.gadget" } else { "radiobutton.gadget" };
            lib(&mut p, b, 2, &[], &[n]);
        }
    }
    assert_eq!(p.c.1.objetos_boopsi(), 0);
    assert_eq!(avisos(&p), 0, "{:?}", p.c.1.avisos());
}

/// Las posiciones de los hijos de la ventana de ajustes con un tema.
type Cajas = Vec<(i32, i32, i32, i32)>;

fn cajas_con_tema(tema: Option<Tokens>) -> (Cajas, u32, u32) {
    let mut p = prueba();
    p.c.1.comp.tema = tema;
    let v = ventana_ajustes(&mut p);
    let cajas = [v.cadena, v.casilla, v.deslizador, v.entero, v.aceptar, v.cancelar].iter().map(|&g| caja_g(&p, g)).collect();
    let bm = rl32(&p, rl32(&p, v.w + win::RPORT) + 4);
    let l = |x: i32, y: i32| p.c.1.pixel_de(&p.c.0, bm, x, y).unwrap_or(0);
    let (x, y) = centro(&p, v.aceptar);
    let fondo = l(2, l16(&p.c, v.w + win::HEIGHT) as i32 - 3);
    let boton = l(x - 20, y);
    // Para mirarla: DUMP_RA=carpeta escribe la ventana en PNG.
    if let Ok(d) = std::env::var("DUMP_RA") {
        let (an, al) = p.c.1.lienzo(bm).map_or((0, 0), |k| (k.ancho(), k.alto()));
        let pix: Vec<u32> = (0..al as i32).flat_map(|y| (0..an as i32).map(move |x| (x, y))).map(|(x, y)| l(x, y)).collect();
        let nombre = if tema.is_some() { "ra-dashos.png" } else { "ra-clasico.png" };
        std::fs::write(format!("{d}/{nombre}"), png(an, al, &pix)).unwrap();
    }
    metodo(&mut p, v.wo, &[WM_CLOSE]);
    dispose(&mut p, v.wo);
    (cajas, fondo, boton)
}

#[test]
fn dos_apariencias_mismos_rectangulos() {
    let (clasico, fondo_c, boton_c) = cajas_con_tema(None);
    let (material, fondo_m, boton_m) = cajas_con_tema(Some(Tokens::CLARO));
    // La barra de DashOS cambia los bordes: los hijos se mueven juntos, del mismo tamaño.
    let d = (material[0].0 - clasico[0].0, material[0].1 - clasico[0].1);
    for (a, b) in clasico.iter().zip(&material) {
        assert_eq!((a.2, a.3), (b.2, b.3), "mismo tamaño");
        assert_eq!((b.0 - a.0, b.1 - a.1), d, "mismo sitio relativo");
    }
    assert_eq!(fondo_m | 0xFF00_0000, Tokens::CLARO.superficie | 0xFF00_0000, "el fondo de DashOS");
    assert_ne!(fondo_c | 0xFF00_0000, fondo_m | 0xFF00_0000);
    assert_ne!(boton_c, boton_m);
}

#[test]
fn layouts_hostiles() {
    let mut p = prueba();
    let c_layout = clase(&mut p, "layout.gadget");
    let raiz = nuevo(&mut p, c_layout, "", &[(LAYOUT_ORIENTATION, 1)]);
    // Hijos que no son objetos, a sí mismo, el mismo dos veces, una etiqueta que no es un objeto.
    let basura = reservar(&mut p.c, 64);
    set_g(&mut p, raiz, 0, &[(LAYOUT_ADDCHILD, basura + 12), (LAYOUT_ADDCHILD, raiz), (LAYOUT_ADDCHILD, 0xFFFF_FFF0)]);
    let b = nuevo(&mut p, 0, "button.gadget", &[(ga::ID, 1)]);
    set_g(&mut p, raiz, 0, &[(LAYOUT_ADDCHILD, b), (CHILD_LABEL, basura), (LAYOUT_ADDCHILD, b), (CHILD_MINWIDTH, 0xFFFF_FFFF)]);
    // Miles de hijos.
    let grande = nuevo(&mut p, c_layout, "", &[(LAYOUT_ORIENTATION, 0)]);
    for k in 0..2000 {
        let g = nuevo(&mut p, 0, "button.gadget", &[(ga::ID, k), (CHILD_WEIGHTEDWIDTH, k)]);
        set_g(&mut p, grande, 0, &[(LAYOUT_ADDCHILD, g), (CHILD_WEIGHTEDWIDTH, k * 1000)]);
    }
    set_g(&mut p, raiz, 0, &[(LAYOUT_ADDCHILD, grande)]);
    // Anidados muy hondo: el tope de niveles corta el cálculo, sin colgarse.
    let mut dentro = nuevo(&mut p, c_layout, "", &[]);
    for _ in 0..100 {
        let fuera = nuevo(&mut p, c_layout, "", &[(LAYOUT_ADDCHILD, dentro)]);
        dentro = fuera;
    }
    set_g(&mut p, raiz, 0, &[(LAYOUT_ADDCHILD, dentro)]);
    let label = cadena(&mut p.c, "Grupo con un título larguísimo que no cabe en ningún sitio razonable");
    set_g(&mut p, raiz, 0, &[(LAYOUT_LABEL, label), (LAYOUT_BEVELSTYLE, BVS_GROUP)]);
    // Una ventana absurda.
    let wo = nuevo(&mut p, 0, "window.class", &[(wa::WIDTH, 100_000), (wa::HEIGHT, 0xFFFF_FFFF), (WINDOW_LAYOUT, raiz)]);
    let w = metodo(&mut p, wo, &[WM_OPEN, 0]);
    assert_ne!(w, 0);
    atender(&mut p);
    clic(&mut p, w, 10, 30);
    eventos(&mut p, wo);
    // RethinkLayout y LayoutLimits.
    let lim = reservar(&mut p.c, 8);
    lib(&mut p, "layout.gadget", 4, &[], &[raiz, lim, 0, 0]);
    lib(&mut p, "layout.gadget", 3, &[1], &[raiz, w, 0]);
    atender(&mut p);
    metodo(&mut p, wo, &[WM_CLOSE]);
    dispose(&mut p, wo);
    assert_eq!(p.c.1.objetos_boopsi(), 0, "todo fuera (el repetido, una vez)");
    // Una ventana sin layout, y WM_HANDLEINPUT sin ventana.
    let wo = nuevo(&mut p, 0, "window.class", &[(wa::WIDTH, 100), (wa::HEIGHT, 50)]);
    assert_eq!(eventos(&mut p, wo), vec![]);
    let w = metodo(&mut p, wo, &[WM_OPEN, 0]);
    assert_ne!(w, 0);
    assert_eq!(metodo(&mut p, wo, &[WM_OPEN, 0]), w, "abrirla dos veces da la misma");
    dispose(&mut p, wo);
    assert!(p.c.1.ventanas().is_empty(), "DisposeObject la cierra");
    let _ = (Registros::default(), tag::TAG_DONE);
}
