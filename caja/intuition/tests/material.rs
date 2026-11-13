//! S26: el aspecto de DashOS en la caja Amiga. Con el tema de DashOS (`CompositorDePrueba::tema`), las ventanas se
//! abren con la superficie de Material, sus bordes y sus gadgets de GadTools salen con los papeles del tema y
//! `DrawBevelBoxA` es una tarjeta; al cambiar el tema (o el ajuste, o la excepción de un programa) se repintan. Y lo
//! malo: las plumas de la `DrawInfo` fuera de la paleta no cambian nada con el tema de DashOS.

mod comun;

use caja_intuition::Tokens;
use caja_intuition::estructuras::{tag, wa, wflg, window as win};
use comun::*;

const T: Tokens = Tokens::CLARO;
const BUTTON: u32 = 1;

fn etiquetas(c: &mut Caja, t: &[(u32, u32)]) -> u32 {
    let d = reservar(c, (t.len() as u32 + 1) * 8);
    for (k, (a, b)) in t.iter().enumerate() {
        e32(c, d + 8 * k as u32, *a);
        e32(c, d + 8 * k as u32 + 4, *b);
    }
    e32(c, d + 8 * t.len() as u32, tag::TAG_DONE);
    d
}

/// Una ventana de 300 x 160 con un botón de GadTools en (10, 20) de 60 x 14.
fn ventana(c: &mut Caja) -> u32 {
    let wb = c.1.workbench();
    let vi = gt(c, "GetVisualInfoA", &[], &[wb, 0]);
    let glist = reservar(c, 4);
    let g = gt(c, "CreateContext", &[], &[glist]);
    let ng = reservar(c, 30);
    for (o, v) in [(0, 10u16), (2, 20), (4, 60), (6, 14)] {
        e16(c, ng + o, v);
    }
    e32(c, ng + 22, vi);
    let t0 = etiquetas(c, &[]);
    assert_ne!(gt(c, "CreateGadgetA", &[BUTTON], &[g, ng, t0]), 0);
    let t = etiquetas(
        c,
        &[
            (wa::WIDTH, 300),
            (wa::HEIGHT, 160),
            (wa::GADGETS, l32(c, glist)),
            (wa::FLAGS, wflg::ACTIVATE | wflg::CLOSEGADGET | wflg::DEPTHGADGET | wflg::DRAGBAR | wflg::SIZEGADGET),
        ],
    );
    let w = i(c, "OpenWindowTagList", &[], &[0, t]);
    assert_ne!(w, 0);
    c.1.atender(&mut c.0);
    w
}

/// (lo de dentro, el botón, la barra de título) en la ventana que ve el compositor.
fn muestras(c: &Caja, w: u32) -> (u32, u32, u32) {
    let s = c.1.superficie(w).unwrap();
    let v = c.1.comp.ventana(s);
    // (Los gadgets de GadTools se colocan desde la esquina de la ventana, bordes incluidos.)
    (v.pixel(250, 120), v.pixel(10 + 15, 20 + 7), v.pixel(300 - 23 - 2, 1))
}

fn cambiar(c: &mut Caja, tema: Option<Tokens>, originales: &[&str]) {
    c.1.comp.tema = tema;
    c.1.comp.originales = originales.iter().map(|s| s.to_string()).collect();
    c.1.comp.version_tema += 1;
    c.1.atender(&mut c.0);
}

#[test]
fn ventana_con_el_tema_de_dashos_y_al_cambiarlo() {
    let mut c = caja();
    // Con el aspecto original (sin tema): lo de 3.x.
    let w = ventana(&mut c);
    let clasico = muestras(&c, w);
    assert_ne!(clasico.0, T.superficie);
    // Llega el tema de DashOS con la ventana abierta: se repinta.
    cambiar(&mut c, Some(T), &[]);
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
    assert_eq!(muestras(&c, w), (T.superficie, T.contenedor_secundario, T.contenedor_alto));
    // Una ventana nueva sale ya con él (también lo que asoma al agrandarla, de la superficie).
    let w2 = ventana(&mut c);
    assert_eq!(muestras(&c, w2).0, T.superficie);
    // La excepción del programa (la tarea de las pruebas se llama "principal"): vuelve el original.
    // (La barra, la de una ventana inactiva: la activa es ya la nueva.)
    cambiar(&mut c, Some(T), &["principal"]);
    assert_eq!((muestras(&c, w).0, muestras(&c, w).1), (clasico.0, clasico.1));
    assert_eq!(muestras(&c, w2).0, clasico.0);
    // Y sin la excepción, otra vez el de DashOS; con el ajuste "original" (sin tema), el clásico.
    cambiar(&mut c, Some(T), &[]);
    assert_eq!(muestras(&c, w), (T.superficie, T.contenedor_secundario, T.contenedor));
    cambiar(&mut c, None, &[]);
    assert_eq!(muestras(&c, w).0, clasico.0);
    assert_eq!(muestras(&c, w).1, clasico.1);
}

/// La `DrawInfo` con plumas fuera de la paleta (y una tabla que apunta a cualquier sitio): con el tema de DashOS, los
/// colores siguen saliendo de los tokens; con el clásico, nada se cae.
#[test]
fn plumas_fuera_de_rango() {
    for tema in [Some(T), None] {
        let mut c = caja();
        c.1.comp.tema = tema;
        let wb = c.1.workbench();
        let dri = i(&mut c, "GetScreenDrawInfo", &[], &[wb]);
        assert_ne!(dri, 0);
        let tabla = l32(&c, dri + 4);
        let n = l16(&c, dri + 2) as u32;
        for k in 0..n {
            e16(&mut c, tabla + 2 * k, [0xFFFF, 200, 255, 0x8000][k as usize % 4]);
        }
        let w = ventana(&mut c);
        if tema.is_some() {
            assert_eq!(muestras(&c, w), (T.superficie, T.contenedor_secundario, T.contenedor_alto));
        }
        // La tabla de plumas apunta fuera de la memoria.
        e32(&mut c, dri + 4, 0xFFFF_FFF0);
        let w2 = ventana(&mut c);
        if tema.is_some() {
            assert_eq!(muestras(&c, w2).1, T.contenedor_secundario);
        }
    }
}

/// `DrawBevelBoxA` con el tema de DashOS: una tarjeta con contorno; lo de dentro no se toca.
#[test]
fn bevel_como_tarjeta() {
    let mut c = caja();
    c.1.comp.tema = Some(T);
    let w = ventana(&mut c);
    let rp = l32(&c, w + win::RPORT);
    let t = etiquetas(&mut c, &[]);
    gt(&mut c, "DrawBevelBoxA", &[100, 60, 120, 50], &[rp, t]);
    // Con un tamaño absurdo, nada se cae.
    gt(&mut c, "DrawBevelBoxA", &[0xFFFF_8000, 0xFFFF_8000, 0x7FFF, 0x7FFF], &[rp, t]);
    c.1.atender(&mut c.0);
    let s = c.1.superficie(w).unwrap();
    let v = c.1.comp.ventana(s);
    assert_eq!(v.pixel(160, 60), T.variante_contorno);
    assert_eq!(v.pixel(100, 85), T.variante_contorno);
    assert_eq!(v.pixel(160, 85), T.superficie);
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
}
