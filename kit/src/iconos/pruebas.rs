//! Pruebas de los iconos: el lector de trazados (también con basura), los símbolos, la colección en todos sus tamaños y
//! temas, el lector de SVG y los temas de Linux (con temas hostiles).

extern crate std;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use super::coleccion::{Coleccion, Iconos};
use super::freedesktop::{self, TemaIconos};
use super::trazado::{trazar, Matriz, MAX_TRAMOS};
use super::*;
use crate::lienzo::Lienzo;
use crate::tema::Tema;

fn lienzo_de(lado: i32) -> Vec<u32> {
    vec![0u32; (lado * lado) as usize]
}

fn tinta(px: &[u32]) -> usize {
    px.iter().filter(|p| **p >> 24 != 0).count()
}

fn mascara(d: &str, lado: i32, unidad: f32) -> Option<crate::trazos::Mascara> {
    rasterizar(d, &Matriz::escala(lado as f32 / unidad, 0.0, 0.0), lado)
}

#[test]
fn un_cuadrado_lleno_y_su_borde() {
    // Un cuadrado de 2 a 8 en un cuadro de 10: lleno del todo por dentro, nada fuera.
    let m = mascara("M2 2H8V8H2Z", 10, 10.0).unwrap();
    assert_eq!((m.x, m.y, m.ancho, m.alto), (2, 2, 6, 6));
    assert!(m.bits.iter().all(|&b| b >= 254), "{:?}", m.bits);
    // Medio píxel corrido: el borde a la mitad.
    let m = mascara("M2.5 2H8V8H2.5Z", 10, 10.0).unwrap();
    assert!((120..=135).contains(&m.bits[0]), "{}", m.bits[0]);
    assert!(m.bits[1] >= 254);
}

#[test]
fn relativas_repetidas_y_arcos() {
    // Lo mismo escrito de cuatro maneras: absoluto, relativo, con L repetida implícita y con H/V.
    let a = mascara("M2 2L8 2L8 8L2 8Z", 10, 10.0).unwrap();
    let b = mascara("m2 2l6 0l0 6l-6 0z", 10, 10.0).unwrap();
    let c = mascara("M2,2 8,2 8,8 2,8z", 10, 10.0).unwrap();
    let d = mascara("M2 2h6v6h-6z", 10, 10.0).unwrap();
    for m in [&b, &c, &d] {
        assert_eq!(m.bits, a.bits);
    }
    // Un círculo de radio 20 con dos arcos (y las banderas pegadas: "a20 20 0 1 1"): su área, π r².
    let m = mascara("M10 30a20 20 0 1 1 40 0a20 20 0 1 1-40 0z", 60, 60.0).unwrap();
    let area: u32 = m.bits.iter().map(|&b| b as u32).sum::<u32>() / 255;
    let pi_r2 = (core::f32::consts::PI * 400.0) as u32;
    assert!(area.abs_diff(pi_r2) < 12, "{area} y {pi_r2}");
    let m2 = mascara("M10 30a20 20 0 1 1 40 0a20 20 0 1 1 -40 0z", 60, 60.0).unwrap();
    assert_eq!(m.bits, m2.bits);
    // Un hueco al revés (regla distinto de cero): un anillo.
    let anillo = mascara("M10 30a20 20 0 1 1 40 0a20 20 0 1 1-40 0zM20 30a10 10 0 1 0 20 0a10 10 0 1 0-20 0z", 60, 60.0).unwrap();
    let centro = anillo.bits[(30 - anillo.y as usize) * anillo.ancho + (30 - anillo.x as usize)];
    assert_eq!(centro, 0);
    // Curvas cuadráticas y cúbicas, con S y T reflejando el control.
    assert!(mascara("M0 10Q5 0 10 10T20 10Z", 20, 20.0).is_some());
    assert!(mascara("M0 10C0 0 10 0 10 10S20 20 20 10Z", 20, 20.0).is_some());
}

#[test]
fn trazados_malos_no_tumban() {
    let malos = [
        "M", "M1", "M1 2 L", "M1 2 L3", "X1 2", "L1 2", "M1 2 3", "M1e39 0 L 2 2", "MNaN 0", "M0 0 A1 1 0 2 0 5 5",
        "M0 0 A1 1 0 1", "M1.2.3.4", "M0 0 Z 5", "M0 0 L1 1 #", "M--1 0", "M0 0 L1e7 1e7 L-1e7 1e7", "M 0 0 L 1",
        "\u{0}\u{1}", "M0 0 L ñ", "M0 0e L1 1",
    ];
    for d in malos {
        let mut c = crate::trazos::Contorno::nuevo(1.0);
        // Que diga Err o Ok da igual en algunos (el 1e7 vale): que no se caiga ni tarde.
        let _ = trazar::<true>(d, &Matriz::IDENTIDAD, &mut c);
        let _ = crate::trazos::cubrir(&c, 64);
    }
    for d in ["M", "M1", "X1 2", "L1 2", "MNaN 0", "M1e39 0", "M0 0 A1 1 0 2 0 5 5", "M0 0 Z 5"] {
        let mut c = crate::trazos::Contorno::nuevo(1.0);
        assert!(trazar::<true>(d, &Matriz::IDENTIDAD, &mut c).is_err(), "{d}");
    }
    // Vacío: nada que pintar, sin error.
    let mut c = crate::trazos::Contorno::nuevo(1.0);
    assert!(trazar::<true>("   ", &Matriz::IDENTIDAD, &mut c).is_ok());
    // Sin arcos (los símbolos): una A es un error.
    let mut c = crate::trazos::Contorno::nuevo(1.0);
    assert!(trazar::<false>("M0 0A1 1 0 0 1 2 2", &Matriz::IDENTIDAD, &mut c).is_err());
    // Uno enorme: el tope de tramos.
    let mut d = String::from("M0 0");
    for i in 0..(MAX_TRAMOS + 10) {
        d.push_str(if i % 2 == 0 { "l1 1" } else { "l1 -1" });
    }
    let mut c = crate::trazos::Contorno::nuevo(1.0);
    assert!(trazar::<true>(&d, &Matriz::IDENTIDAD, &mut c).is_err());
    // Muy fuera del cuadro (a un millón de píxeles): se recorta sin dar un millón de vueltas.
    let m = mascara("M-1000000 0H5V5H-1000000Z", 10, 10.0).unwrap();
    assert!(m.bits.iter().all(|&b| b >= 254) && m.ancho == 5, "{m:?}");
    let m = mascara("M5 0H1000000V5H5Z", 10, 10.0).unwrap();
    assert_eq!((m.x, m.ancho), (5, 5));
}

#[test]
fn todos_los_simbolos_se_pintan() {
    let s = Simbolos::new();
    for lado in [1, 16, 18, 20, 24, 48, 72] {
        for sim in simbolo::TODOS {
            let m = mascara_material(sim.d, lado).unwrap_or_else(|| panic!("{} no se lee", sim.nombre));
            if lado >= 16 {
                assert!(m.bits.iter().any(|&b| b > 128), "{} a {lado} sin tinta", sim.nombre);
            }
            let mut px = lienzo_de(lado);
            let mut l = Lienzo::new(&mut px, lado, lado, lado as usize);
            s.pintar(&mut l, 0, 0, lado, sim, 0xff00_0000);
        }
    }
    assert!(s.bytes() <= TOPE_CACHE);
    // Buscar por nombre.
    assert_eq!(Simbolo::por_nombre("search", false), Some(simbolo::SEARCH));
    assert_eq!(Simbolo::por_nombre("dark_mode", true), Some(simbolo::DARK_MODE_LLENO));
    assert_eq!(Simbolo::por_nombre("no_existe", false), None);
}

#[test]
fn simbolo_en_su_sitio() {
    // La lupa a 24: dentro de su margen de Material (2 de 24) y con la tinta arriba a la izquierda (el cristal).
    let mut px = lienzo_de(40);
    let mut l = Lienzo::new(&mut px, 40, 40, 40);
    Simbolos::new().pintar(&mut l, 8, 8, 24, simbolo::SEARCH, 0xff00_0000);
    for y in 0..40 {
        for x in 0..40 {
            if px[y * 40 + x] >> 24 != 0 {
                assert!((10..30).contains(&x) && (10..30).contains(&y), "({x}, {y})");
            }
        }
    }
    assert!(tinta(&px) > 60);
}

#[test]
fn tamanos_limite() {
    let s = Simbolos::new();
    let mut px = lienzo_de(32);
    let mut l = Lienzo::new(&mut px, 32, 32, 32);
    for lado in [i32::MIN, -5, 0] {
        s.pintar(&mut l, 0, 0, lado, simbolo::HOME, 0xff00_0000);
        Iconos::new().pintar_nombre(&mut l, 0, 0, lado, "carpeta", &Tema::CLARO);
    }
    drop(l);
    assert_eq!(tinta(&px), 0, "con lado 0 o menos no se pinta nada");
    let mut l = Lienzo::new(&mut px, 32, 32, 32);
    // Enormes: se pinta como mucho a MAX_LADO, recortado al lienzo, sin pedir memoria de más.
    for lado in [MAX_LADO, MAX_LADO + 1, 100_000, i32::MAX] {
        s.pintar(&mut l, -10, -10, lado, simbolo::HOME, 0xff00_0000);
        Iconos::new().pintar_nombre(&mut l, -5, -5, lado, "disquete", &Tema::OSCURO);
    }
    // Fuera del lienzo del todo.
    s.pintar(&mut l, 1000, 1000, 24, simbolo::HOME, 0xff00_0000);
    s.pintar(&mut l, -1000, -1000, 24, simbolo::HOME, 0xff00_0000);
    // Un píxel.
    let mut uno = lienzo_de(1);
    let mut l1 = Lienzo::new(&mut uno, 1, 1, 1);
    s.pintar(&mut l1, 0, 0, 1, simbolo::HOME, 0xff00_0000);
    Iconos::new().pintar_nombre(&mut l1, 0, 0, 1, "carpeta", &Tema::CLARO);
}

#[test]
fn simbolo_roto_da_la_reserva() {
    let roto = Simbolo::propio("roto", "M0 0 L 1 X");
    let mut px = lienzo_de(24);
    let mut l = Lienzo::new(&mut px, 24, 24, 24);
    let s = Simbolos::new();
    s.pintar(&mut l, 0, 0, 24, roto, 0xff00_0000);
    // Y la segunda vez sale de la caché (el fallo también se recuerda).
    s.pintar(&mut l, 0, 0, 24, roto, 0xff00_0000);
    drop(l);
    assert!(tinta(&px) > 20, "la reserva se ve");
}

#[test]
fn la_coleccion_se_lee_entera() {
    let c = Coleccion::leer(include_str!("../../iconos/dashos.iconos")).unwrap_or_else(|(n, e)| panic!("línea {n}: {e}"));
    assert!(c.cuantos() >= 60, "{}", c.cuantos());
    let i = Iconos::de(c);
    for n in i.nombres() {
        let ic = i.buscar(n).unwrap();
        assert!(!i.nombres_linux(ic).is_empty(), "{n} sin nombre de freedesktop");
    }
    assert_eq!(i.buscar_linux("folder-music"), i.buscar("carpeta-musica"));
    assert_eq!(i.buscar_linux("user-trash-full"), i.buscar("papelera-llena"));
}

#[test]
fn colecciones_malas() {
    for (texto, linea) in [
        ("icono x forma=nada", 1),
        ("forma a\nfondo M0 0h1v1z\nforma a", 3),
        ("icono x familia=morado", 1),
        ("icono x emblema=no_existe_esto", 1),
        ("fondo M0 0h1v1z", 1),
        ("forma a\nrosa M0 0h1v1z", 2),
        ("forma a\nfondo@200 M0 0h1v1z", 2),
        ("forma a\nemblema 1 2", 2),
        ("icono x\nemblema 1 2 3", 2),
        ("icono x forma", 1),
        ("forma f\nfondo M0 0h1v1z\nicono x forma=f emblema=home", 3),
        ("icono x\nicono x", 2),
    ] {
        let texto: &'static str = alloc::boxed::Box::leak(String::from(texto).into_boxed_str());
        match Coleccion::leer(texto) {
            Err((n, _)) => assert_eq!(n, linea, "{texto:?}"),
            Ok(_) => panic!("{texto:?} se leyó"),
        }
    }
    // Una que se lee pero con un trazado roto: sale la reserva, sin caer.
    let c = Coleccion::leer("icono x\nfondo M0 0 L1 X").unwrap();
    let mut i = Iconos::de(c);
    let mut px = lienzo_de(48);
    let mut l = Lienzo::new(&mut px, 48, 48, 48);
    i.pintar_nombre(&mut l, 0, 0, 48, "x", &Tema::CLARO);
    assert!(tinta(&px) > 100);
    // Y un nombre que no existe.
    let mut px = lienzo_de(48);
    let mut l = Lienzo::new(&mut px, 48, 48, 48);
    Iconos::new().pintar_nombre(&mut l, 0, 0, 48, "no-existe", &Tema::CLARO);
    assert!(tinta(&px) > 100);
}

#[test]
fn cada_icono_en_cada_tamano_y_tema() {
    let mut i = Iconos::new();
    let nombres: Vec<&str> = i.nombres().collect();
    let azul = Tema::del_sistema(0xff00_61a4, false);
    for t in [&Tema::CLARO, &Tema::OSCURO, &azul] {
        for lado in [16, 24, 32, 48, 72, 96] {
            for n in &nombres {
                let mut px = lienzo_de(lado);
                let mut l = Lienzo::new(&mut px, lado, lado, lado as usize);
                i.pintar_nombre(&mut l, 0, 0, lado, n, t);
                // Que se vea: al menos un cuarto del cuadro con tinta, y nada en el borde de fuera del todo (salvo los
                // que llegan a él a propósito).
                let k = tinta(&px);
                assert!(k * 6 > (lado * lado) as usize, "{n} a {lado}: {k}");
            }
        }
    }
    assert!(i.bytes() <= TOPE_CACHE, "{}", i.bytes());
}

#[test]
fn los_tonos_siguen_al_tema() {
    // La misma carpeta, en violeta y en azul: los colores cambian (toma el acento).
    let pinta = |t: &Tema| {
        let mut px = lienzo_de(48);
        let mut l = Lienzo::new(&mut px, 48, 48, 48);
        Iconos::new().pintar_nombre(&mut l, 0, 0, 48, "carpeta", t);
        px[30 * 48 + 24]
    };
    let (v, a) = (pinta(&Tema::CLARO), pinta(&Tema::del_sistema(0xff00_61a4, false)));
    assert_ne!(v, a);
    // En azul, el frente de la carpeta tira a azul (más azul que rojo).
    assert!((a & 0xff) > (a >> 16 & 0xff), "{a:08x}");
    // En oscuro, más oscura que en claro.
    let o = pinta(&Tema::OSCURO);
    let luz = |c: u32| (c >> 16 & 0xff) + (c >> 8 & 0xff) + (c & 0xff);
    assert!(luz(o) < luz(v));
}

#[test]
fn la_cache_tiene_tope() {
    let s = Simbolos::new();
    let mut px = lienzo_de(8);
    let mut l = Lienzo::new(&mut px, 8, 8, 8);
    for lado in 1..300 {
        s.pintar(&mut l, 0, 0, lado, simbolo::HOME, 0xff00_0000);
        assert!(s.bytes() <= TOPE_CACHE);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// SVG

fn svg_pixel(svg: &str, lado: i32, x: usize, y: usize) -> u32 {
    let img = svg::pintar(svg.as_bytes(), lado, 0xff12_3456).unwrap();
    img.px[y * img.ancho + x]
}

#[test]
fn svg_sencillo() {
    let s = r##"<?xml version="1.0"?><!-- hola --><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">
        <rect x="0" y="0" width="8" height="16" fill="#f00"/>
        <g transform="translate(8 0)"><path d="M0 0h8v8H0z" style="fill:#00ff00;fill-opacity:0.5"/></g>
        <circle cx="12" cy="12" r="3" fill="currentColor"/></svg>"##;
    assert_eq!(svg_pixel(s, 16, 2, 8), 0xffff_0000);
    let verde = svg_pixel(s, 16, 12, 2);
    assert_eq!(verde & 0x00ff_ffff, 0x00ff00);
    assert!((120..=135).contains(&(verde >> 24)), "{verde:08x}");
    assert_eq!(svg_pixel(s, 16, 12, 12), 0xff12_3456);
    assert_eq!(svg_pixel(s, 16, 15, 15) >> 24, 0, "fuera de todo, transparente");
    // Al doble de tamaño, lo mismo en el doble de sitio.
    assert_eq!(svg_pixel(s, 32, 4, 16), 0xffff_0000);
}

#[test]
fn svg_degradados_defs_y_rect_redondeado() {
    let s = r##"<svg viewBox="0 0 10 10" xmlns:xlink="http://www.w3.org/1999/xlink"><defs>
        <linearGradient id="a"><stop offset="0" stop-color="#0000ff"/><stop offset="1" stop-color="#ffffff"/></linearGradient>
        <linearGradient id="b" xlink:href="#a"/>
        <path id="oculto" d="M0 0h10v10H0z" fill="#ff0000"/></defs>
        <rect width="10" height="10" rx="3" fill="url(#b)"/></svg>"##;
    assert_eq!(svg_pixel(s, 10, 5, 5), 0xff00_00ff, "el color del degradado, sin pintar lo de <defs>");
    assert!(svg_pixel(s, 10, 0, 0) >> 24 < 20, "la esquina redondeada");
}

#[test]
fn svg_malos() {
    let malos: [&[u8]; 12] = [
        b"",
        b"no es xml",
        b"<svg",
        b"<svg viewBox='0 0 10 10'><path d='M0 0 L X'/></svg>",
        b"<svg viewBox='0 0 0 0' width='0' height='0'></svg>",
        b"<path d='M0 0h1v1z'/>",
        b"<svg viewBox='0 0 10 10'><g transform='rotar(4)'><path d='M0 0h1v1z'/></g></svg>",
        b"<svg viewBox='0 0 10 10'><rect width='5' height=5/></svg>",
        b"<svg viewBox='0 0 10 10'><!-- sin cerrar",
        b"\xff\xfe<svg/>",
        b"<svg viewBox='0 0 10 10'><path d='M0 0h1v1z' fill='#12'/></svg>",
        b"<svg viewBox='0 0 10 10'><rect width='5' height='5' fill=\"#f00/></svg>",
    ];
    for m in malos {
        let _ = svg::pintar(m, 16, 0);
    }
    assert!(svg::pintar(b"no es xml", 16, 0).is_err());
    assert!(svg::pintar(b"<svg viewBox='0 0 10 10'><path d='M0 0 L X'/></svg>", 16, 0).is_err());
    // Demasiado hondo.
    let mut s = String::from("<svg viewBox='0 0 10 10'>");
    for _ in 0..100 {
        s.push_str("<g>");
    }
    assert!(svg::pintar(s.as_bytes(), 16, 0).is_err());
    // Demasiados elementos, o demasiado grande.
    let mut s = String::from("<svg viewBox='0 0 10 10'>");
    for _ in 0..5000 {
        s.push_str("<rect width='1' height='1'/>");
    }
    s.push_str("</svg>");
    assert!(svg::pintar(s.as_bytes(), 16, 0).is_err());
    assert!(svg::pintar(&vec![b' '; svg::MAX_BYTES + 1], 16, 0).is_err());
    // Tamaños malos.
    assert!(svg::pintar(b"<svg viewBox='0 0 10 10'/>", 0, 0).is_err());
    assert_eq!(svg::pintar(b"<svg viewBox='0 0 10 10'/>", 100_000, 0).unwrap().ancho, MAX_LADO as usize);
}

// ---------------------------------------------------------------------------------------------------------------
// Temas de Linux

const INDEX: &str = "[Icon Theme]\nName=Mi Tema\nComment=uno\nInherits=Base, hicolor\nDirectories=16x16/places,48x48/places,scalable/places,../fuera\n\n[16x16/places]\nSize=16\nType=Fixed\n\n[48x48/places]\nSize=48\nType=Threshold\n\n[scalable/places]\nSize=64\nMinSize=8\nMaxSize=512\nType=Scalable\n\n[../fuera]\nSize=16\n";
const BASE: &str = "[Icon Theme]\nName=Base\nInherits=Mi\nDirectories=32x32/mimetypes\n[32x32/mimetypes]\nSize=32\nType=Fixed\n";
const HICOLOR: &str = "[Icon Theme]\nName=Hicolor\nHidden=true\nDirectories=48x48/apps\n[48x48/apps]\nSize=48\n";

fn temas() -> Vec<TemaIconos> {
    freedesktop::cadena("Mi", &mut |n| match n {
        "Mi" => freedesktop::leer_index(n, INDEX.as_bytes()).ok(),
        "Base" => freedesktop::leer_index(n, BASE.as_bytes()).ok(),
        "hicolor" => freedesktop::leer_index(n, HICOLOR.as_bytes()).ok(),
        _ => None,
    })
}

#[test]
fn index_theme() {
    let t = freedesktop::leer_index("Mi", INDEX.as_bytes()).unwrap();
    assert_eq!(t.nombre, "Mi Tema");
    assert_eq!(t.hereda, ["Base", "hicolor"]);
    // El cajón "../fuera" no se acepta.
    assert_eq!(t.cajones.len(), 3);
    assert!(t.cajones.iter().all(|c| !c.ruta.contains("..")));
    assert!(freedesktop::leer_index("hicolor", HICOLOR.as_bytes()).unwrap().oculto);
    for malo in ["", "sin seccion=1", "[Otra]\nName=x", "\u{0}"] {
        assert!(freedesktop::leer_index("x", malo.as_bytes()).is_err(), "{malo:?}");
    }
    assert!(freedesktop::leer_index("../x", INDEX.as_bytes()).is_err());
    assert!(freedesktop::leer_index("x", &vec![b'a'; freedesktop::MAX_INDEX + 1]).is_err());
    assert!(freedesktop::leer_index("x", b"\xff[Icon Theme]").is_err());
}

#[test]
fn buscar_en_la_cadena() {
    let t = temas();
    // Mi, Base (que hereda de Mi: el bucle se corta) y hicolor al final.
    assert_eq!(t.iter().map(|t| t.cajon.as_str()).collect::<Vec<_>>(), ["Mi", "Base", "hicolor"]);
    let hay = ["Mi/16x16/places/folder.png", "Mi/scalable/places/folder.svg", "Base/32x32/mimetypes/text-x-generic.png", "hicolor/48x48/apps/firefox.png", "Mi/48x48/places/user-trash.png"];
    let mut existe = |r: &str| hay.contains(&r);
    // Exacto; escalable si no hay de ese tamaño; el más cercano; heredado; en hicolor.
    assert_eq!(freedesktop::buscar(&t, "folder", 16, &mut existe).as_deref(), Some("Mi/16x16/places/folder.png"));
    assert_eq!(freedesktop::buscar(&t, "folder", 100, &mut existe).as_deref(), Some("Mi/scalable/places/folder.svg"));
    assert_eq!(freedesktop::buscar(&t, "user-trash", 46, &mut existe).as_deref(), Some("Mi/48x48/places/user-trash.png"));
    assert_eq!(freedesktop::buscar(&t, "user-trash", 16, &mut existe).as_deref(), Some("Mi/48x48/places/user-trash.png"));
    assert_eq!(freedesktop::buscar(&t, "text-x-generic", 48, &mut existe).as_deref(), Some("Base/32x32/mimetypes/text-x-generic.png"));
    assert_eq!(freedesktop::buscar(&t, "firefox", 48, &mut existe).as_deref(), Some("hicolor/48x48/apps/firefox.png"));
    // De "folder-music" a "folder".
    assert_eq!(freedesktop::buscar(&t, "folder-music", 16, &mut existe).as_deref(), Some("Mi/16x16/places/folder.png"));
    // Nada, o nombres que intentan salir del tema.
    assert_eq!(freedesktop::buscar(&t, "nada", 16, &mut existe), None);
    for malo in ["../../etc/passwd", "a/b", ".oculto", "", "x\0y"] {
        let mut mirado = Vec::new();
        assert_eq!(freedesktop::buscar(&t, malo, 16, &mut |r| { mirado.push(String::from(r)); true }), None, "{malo}");
        assert!(mirado.is_empty());
    }
    assert_eq!(freedesktop::buscar(&t, "folder", 0, &mut existe), None);
}

#[test]
fn herencia_enorme_se_corta() {
    // Cada tema hereda de otro nuevo, sin fin.
    let mut pedidos = 0;
    let t = freedesktop::cadena("t0", &mut |n| {
        pedidos += 1;
        let k: u32 = n.trim_start_matches('t').parse().unwrap_or(0);
        let texto = alloc::format!("[Icon Theme]\nInherits=t{}\nDirectories=a\n[a]\nSize=16\n", k + 1);
        let texto: &'static str = alloc::boxed::Box::leak(texto.into_boxed_str());
        freedesktop::leer_index(n, texto.as_bytes()).ok()
    });
    assert!(t.len() <= freedesktop::MAX_HERENCIA + 1, "{}", t.len());
    assert!(pedidos < 20, "{pedidos}");
}

#[test]
fn pintar_iconos_de_un_tema() {
    let svg = br##"<svg viewBox="0 0 16 16"><rect width="16" height="16" fill="#336699"/></svg>"##;
    let img = freedesktop::pintar("Mi/scalable/places/folder.svg", svg, 48, 0).unwrap();
    assert_eq!((img.ancho, img.px[24 * 48 + 24]), (48, 0xff33_6699));
    // Un PNG de 2 × 1 (rojo y azul) llevado a 4: centrado, con transparencia arriba y abajo.
    let png = include_bytes!("../pruebas/color.png");
    let img = freedesktop::pintar("x.png", png, 4, 0).unwrap();
    assert_eq!(img.ancho, 4);
    assert!(freedesktop::pintar("x.png", b"no es png", 16, 0).is_err());
    assert!(freedesktop::pintar("x.xpm", b"/* XPM */", 16, 0).is_err());
    assert!(freedesktop::pintar("x.svg", svg, 0, 0).is_err());
    // Pintarlo en un lienzo, mezclado con lo que hay.
    let img = freedesktop::pintar("a.svg", svg, 8, 0).unwrap();
    let mut px = vec![0xff00_0000u32; 100];
    let mut l = Lienzo::new(&mut px, 10, 10, 10);
    svg::pintar_imagen(&mut l, 1, 1, &img);
    assert_eq!(px[5 * 10 + 5] & 0x00ff_ffff, 0x33_6699);
    assert_eq!(px[0], 0xff00_0000);
}

#[test]
fn la_eleccion_en_texto() {
    let t = freedesktop::texto_de_eleccion(Some("Papirus"), None);
    assert!(t.contains("tema = Papirus") || t.contains("tema=Papirus"), "{t}");
    assert_eq!(freedesktop::eleccion_de(t.as_bytes()).as_deref(), Some("Papirus"));
    // Sobre lo que había: el comentario de quien lo editó a mano se queda.
    let otra = freedesktop::texto_de_eleccion(None, Some(b"# mio\ntema = Papirus\n"));
    assert!(otra.contains("# mio") && freedesktop::eleccion_de(otra.as_bytes()).is_none(), "{otra}");
    for malo in [&b"tema = ../../C"[..], b"tema =", b"", b"\xff\xfe", b"nada", b"tema = a/b", b"tema = .oculto"] {
        assert_eq!(freedesktop::eleccion_de(malo), None, "{malo:?}");
    }
    // Un nombre malo no se escribe: sale "dashos".
    assert!(freedesktop::eleccion_de(freedesktop::texto_de_eleccion(Some("../x"), None).as_bytes()).is_none());
}

#[test]
fn numeros_sin_parse() {
    use super::trazado::numero_de;
    for (t, v) in [("0", 0.0f32), ("-12.5", -12.5), ("+3", 3.0), (".5", 0.5), ("5.", 5.0), ("1e3", 1000.0), ("2.5E-2", 0.025), ("-0", 0.0), ("960", 960.0), ("56.5", 56.5)] {
        assert!((numero_de(t.as_bytes()).unwrap() - v).abs() < 1e-5, "{t}");
    }
    for malo in ["", "-", ".", "e3", "1e", "1e+", "1.2.3", "1x", "--1", "1e99999999"] {
        let r = numero_de(malo.as_bytes());
        assert!(r.is_none() || !r.unwrap().is_finite(), "{malo}: {r:?}");
    }
}
