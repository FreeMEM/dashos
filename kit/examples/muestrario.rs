//! Cada control del kit en cada estado (normal, encima, foco, pulsado, desactivado), en claro y en oscuro, en un PPM
//! para mirarlo en el PC: `cargo run --example muestrario -- salida.ppm`. Para comparar con Material 3. Debajo, el
//! menú de un desplegable abierto: con el ratón encima de una opción y con el teclado.

use std::collections::BTreeMap;

use kit::controles::{Boton, Campo, Casilla, Ciclo, Contexto, Deslizador, Hijo, Interfaz, Lista};
use kit::lienzo::{Lienzo, Rect};
use kit::objetos::{atr, Arbol, Atr, Clase, Valor};
use kit::tema::{Estado, Tema};

/// Una fila del muestrario: la clase, sus atributos y su tamaño.
type Fila = (Box<dyn Clase>, BTreeMap<Atr, Valor>, (i32, i32));

/// Lo que mide cada menú de muestra.
const MENU: (i32, i32) = (400, 250);

/// Una ventana con un desplegable y su menú abierto: con el ratón encima de la segunda opción, o (`teclado`) abierto con
/// Intro y una opción más abajo.
fn menu(tema: Tema, teclado: bool) -> Vec<u32> {
    let mut a = Arbol::new();
    let ciclo = a.crear(Ciclo, &[(atr::OPCIONES, Valor::Texto("Claro|Oscuro|Automático".into())), (atr::VALOR, Valor::Int(0))]);
    let boton = a.crear(Boton, &[(atr::TEXTO, Valor::Texto("Aceptar".into()))]);
    let mut i = Interfaz::new(a, Hijo::Columna(vec![Hijo::Fila(vec![Hijo::Obj(ciclo), Hijo::Obj(boton)])]), tema);
    i.esquinas = 16;
    i.colocar(Rect::new(0, 0, MENU.0, MENU.1));
    let r = i.sitios[&ciclo];
    if teclado {
        i.tecla(15, true);
        i.tecla(28, true);
        i.tecla(108, true);
        i.tecla(108, true);
    } else {
        i.raton(r.x + 20, r.y + 20, true);
        i.raton(r.x + 20, r.y + 20, false);
        i.raton(r.x + 40, r.y + r.h + 4 + 2 + 44 + 20, false);
    }
    let mut px = vec![0u32; (MENU.0 * MENU.1) as usize];
    let mut l = Lienzo::new(&mut px, MENU.0, MENU.1, MENU.0 as usize);
    i.dibujar(&mut l);
    px
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let estados = [
        Estado::default(),
        Estado { encima: true, ..Default::default() },
        Estado { foco: true, anillo: true, ..Default::default() },
        Estado { pulsado: true, encima: true, ..Default::default() },
        Estado { desactivado: true, ..Default::default() },
    ];
    let attrs = |v: &[(Atr, Valor)]| v.iter().cloned().collect::<BTreeMap<Atr, Valor>>();
    let texto = |t: &str| Valor::Texto(t.into());
    let filas: Vec<Fila> = vec![
        (Box::new(Boton), attrs(&[(atr::TEXTO, texto("Aceptar"))]), (100, 40)),
        (Box::new(Boton), attrs(&[(atr::TEXTO, texto("Cancelar")), (atr::ESTILO, Valor::Int(2))]), (100, 40)),
        (Box::new(Casilla), attrs(&[(atr::VALOR, Valor::Bool(false))]), (40, 40)),
        (Box::new(Casilla), attrs(&[(atr::VALOR, Valor::Bool(true))]), (40, 40)),
        (Box::new(Casilla), attrs(&[(atr::VALOR, Valor::Bool(false)), (atr::ETIQUETA, texto("Sonido"))]), (130, 40)),
        (Box::new(Deslizador), attrs(&[(atr::VALOR, Valor::Int(60))]), (130, 44)),
        (Box::new(Deslizador), attrs(&[(atr::VALOR, Valor::Int(0))]), (130, 44)),
        (Box::new(Ciclo), attrs(&[(atr::OPCIONES, texto("Claro|Oscuro"))]), (130, 56)),
        (Box::new(Ciclo), attrs(&[(atr::OPCIONES, texto("Claro|Oscuro")), (atr::ABIERTO, Valor::Bool(true))]), (130, 56)),
        (Box::new(Campo), attrs(&[(atr::TEXTO, texto("DashOS"))]), (130, 56)),
        (Box::new(Campo), attrs(&[(atr::ETIQUETA, texto("Nombre"))]), (130, 56)),
        (Box::new(Campo), attrs(&[(atr::ETIQUETA, texto("Nombre")), (atr::TEXTO, texto("Ana")), (atr::AYUDA, texto("Para la barra"))]), (130, 76)),
        (Box::new(Campo), attrs(&[(atr::ETIQUETA, texto("Nombre")), (atr::TEXTO, texto("Ana")), (atr::ERROR, texto("Ya existe"))]), (130, 76)),
        // La lista: con una elegida (con el foco del teclado, la actual es ella) y sin elegida (la actual, la primera).
        (Box::new(Lista), attrs(&[(atr::OPCIONES, texto("Uno|Dos")), (atr::FILAS, Valor::Int(2)), (atr::VALOR, Valor::Int(1))]), (140, 112)),
        (Box::new(Lista), attrs(&[(atr::OPCIONES, texto("Uno|Dos")), (atr::FILAS, Valor::Int(2)), (atr::VALOR, Valor::Int(-1))]), (140, 112)),
        // La de navegación (ESTILO 1), con la segunda elegida.
        (Box::new(Lista), attrs(&[(atr::OPCIONES, texto("Permisos|Discos")), (atr::FILAS, Valor::Int(2)), (atr::VALOR, Valor::Int(1)), (atr::ESTILO, Valor::Int(1))]), (140, 112)),
    ];
    let col = 160;
    let alto_fila = |ch: i32| ch + 32;
    let alto_filas: i32 = filas.iter().map(|f| alto_fila(f.2.1)).sum();
    let ancho_tema = col * estados.len() as i32;
    let (w, h) = (ancho_tema * 2, alto_filas + MENU.1 + 32);
    let mut px = vec![0u32; (w * h) as usize];
    let mut l = Lienzo::new(&mut px, w, h, w as usize);
    for (k, tema) in [Tema::CLARO, Tema::OSCURO].into_iter().enumerate() {
        let mut ctx = Contexto::new(tema);
        let x0 = k as i32 * ancho_tema;
        l.borrar(Rect::new(x0, 0, ancho_tema, h), tema.superficie);
        let mut y0 = 0;
        for (clase, a, (cw, ch)) in filas.iter() {
            for (i, &e) in estados.iter().enumerate() {
                let r = Rect::new(x0 + i as i32 * col + (col - cw) / 2, y0 + 16, *cw, *ch);
                l.recortar(r.dentro(-12));
                clase.dibujar(a, &mut l, r, &mut ctx, e);
                l.soltar_recorte();
            }
            y0 += alto_fila(*ch);
        }
        // Los dos menús, uno junto al otro, sobre el contenedor alto (para que se vean las esquinas de la ventana).
        l.borrar(Rect::new(x0, y0, ancho_tema, MENU.1 + 32), tema.contenedor_alto());
        for (j, teclado) in [false, true].into_iter().enumerate() {
            let m = menu(tema, teclado);
            let (mx, my) = (x0 + j as i32 * MENU.0, y0 + 16);
            for y in 0..MENU.1 {
                for x in 0..MENU.0 {
                    // Premultiplicado: encima de lo que hay, con su alfa.
                    let p = m[(y * MENU.0 + x) as usize];
                    let a = p >> 24;
                    let d = &mut l.px[((my + y) * w + mx + x) as usize];
                    let canal = |s: u32| (((p >> s) & 0xff) + (((*d >> s) & 0xff) * (255 - a) + 127) / 255).min(255);
                    *d = 0xff00_0000 | canal(16) << 16 | canal(8) << 8 | canal(0);
                }
            }
        }
    }
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    for p in &px {
        out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
    }
    std::fs::write(&args[1], out).unwrap();
}
