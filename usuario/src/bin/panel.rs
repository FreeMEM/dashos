//! C2 de S10c: el panel rápido. Se abre y se cierra con Super+A (el compositor solo se lo dice a él, por el evento
//! `panel` de `dashos_tema_v1`) en una capa arriba a la derecha, y no se lleva el teclado de otra ventana sin un clic
//! (teclado "a demanda"). Sigue el tema del sistema, como lo demás.
//!
//! Los mosaicos (`panel_mosaicos`, compartido con el centro de control):
//! - Modo oscuro. No pone el tema él: se lo pide al centro ("modo oscuro" o "modo claro" por su canal, que solo se lo
//!   acepta a `panel`), que es quien lo guarda.
//! - Bloquear: como Super+L, por la petición `bloquear` de `dashos_tema_v1`, que el compositor solo le acepta a él.
//! - Volumen general: "−" y "+" de 10 en 10 %, por la orden `GENERAL` del servicio de sonido (solo se la acepta a él).
//!
//! El modo de edición (como el de DMS): con "Editar", cada mosaico se puede subir, bajar o quitar, y los quitados se
//! vuelven a poner (al final). Con "Hecho" (o al cerrar el panel) se guarda, si cambió: también por el centro
//! ("panel guardar ..."), que lo guarda en `C/Prefs/panel`. Al arrancar se lo pide ("panel leer"); si el centro no
//! contesta, los de serie, y lo vuelve a pedir al abrir.
//!
//! El panel mide lo que su contenido: al cambiar, pide otro tamaño a la capa y pinta en un búfer de esa altura (dos
//! búferes sobre la misma memoria, uno visto y el otro para el tamaño nuevo).
//!
//! S27, oleada 2 (`docs/diseño/S27-escritorio-dms.md`): el panel es el centro de control al estilo de DMS. Arriba, quién
//! está delante (lo dice `sesiones`, `DELANTE`) y cuánto lleva encendido, con Editar y Bloquear. Debajo, los
//! deslizadores del volumen (con el silencio y los botones de 10 en 10) y del brillo (apagado: aún no hay quien lo
//! lleve). Y una rejilla de dos columnas de mosaicos con su icono en un cuadrado redondeado: la red (lo que dice el
//! servicio de red, `ESTADO`), el modo oscuro y bloquear, en el orden guardado. El estado de la red y del volumen se lo
//! dice al compositor (petición `estado` de `dashos_tema_v1`), que se lo pasa a la barra para su píldora.
//!
//! El menú de apagado (decisión del usuario del 2026-11-13): el botón de la derecha de la cabecera cambia los controles
//! por cuatro filas: Apagar, Reiniciar y Cerrar sesión, que se piden a `sesiones` (`APAGAR`: solo se lo acepta al panel,
//! y con otras sesiones abiertas pregunta antes con `guardian`), y Bloquear, como el botón del candado.
//!
//! El estado de la red llega por "red-estado", que solo da eso (decisión del usuario del 2026-11-13): el panel no tiene
//! la llave de "red", que es el permiso de red. Al arrancar comprueba que con la suya no se abre una conexión.
//!
//! Llaves: 1 el canal "ventanas" (con el poder de escritorio, para la capa), 2 "sesiones", 3 "red-estado" (desde
//! trasfondo, la red se lanza antes que el fondo), 4 "centro" y 5 "sonido".

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
#[allow(dead_code)]
mod ventana_cliente;
#[path = "../panel_mosaicos.rs"]
#[allow(dead_code)]
mod panel_mosaicos;
#[path = "../volumen_osd.rs"]
mod volumen_osd;

use alloc::string::String;
use alloc::vec::Vec;
use dashos::println;
use kit::lienzo::{Color, Lienzo, Rect};
use kit::tema::{apagado, Tema};
use kit::texto::Fuente;
use panel_mosaicos::{Orden, MAX_TEXTO, MOSAICOS};
use volumen_osd::Volumen;
use ventana_cliente::*;
use wayland::tablas::{
    dashos_tema_v1, wl_buffer, wl_compositor, wl_display, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface, zwlr_layer_shell_v1,
    zwlr_layer_surface_v1,
};
use wayland::Escritor;

dashos::programa!(principal);
dashos::monton!();

const SESIONES: u32 = 2;
const RED: u32 = 3;
const CENTRO: u32 = 4;
const SONIDO: u32 = 5;
/// Cada cuánto se pregunta a la red, y cuánto se le espera.
const CADA_RED_US: u64 = 10_000_000;
const PLAZO_RED_US: u64 = 300_000;
const GENERAL: u32 = 13;
const CAPAS: u32 = 20;
const CAPA: u32 = 21;
const TEMA: u32 = 22;
/// El segundo búfer (el primero es `BUFER`), para cambiar de altura sin soltar el que se ve.
const BUFER_2: u32 = 23;
const ANCHO: i32 = 440;
/// Lo más alto que se pinta (la memoria es para esto).
const ALTO_MAX: i32 = 520;
/// La distancia a los bordes de arriba y de la derecha.
const MARGEN: i32 = 8;
/// Lo de arriba: quién está delante (o, editando, el título) y los botones Editar (o Hecho) y Bloquear.
const CABECERA: i32 = 76;
const BOTON_CABECERA: Rect = Rect { x: ANCHO - 16 - 40 - 8 - 40 - 8 - 88, y: 18, w: 88, h: 40 };
const BOTON_BLOQUEAR: Rect = Rect { x: ANCHO - 16 - 40 - 8 - 40, y: 18, w: 40, h: 40 };
/// El botón del menú de apagado, a la derecha del todo.
const BOTON_APAGADO: Rect = Rect { x: ANCHO - 16 - 40, y: 18, w: 40, h: 40 };
/// Las filas del menú de apagado: lo que pone y la orden de `sesiones` (`None`: bloquear, como el candado).
const ENERGIA: [(&str, Option<u32>); 4] = [
    ("Apagar", Some(dashos::protocolo::sesiones::MODO_APAGAR)),
    ("Reiniciar", Some(dashos::protocolo::sesiones::MODO_REINICIAR)),
    ("Cerrar sesión", Some(dashos::protocolo::sesiones::MODO_SALIR)),
    ("Bloquear", None),
];
/// Los deslizadores: filas de 56 a lo ancho.
const ALTO_DESLIZADOR: i32 = 56;
/// Los mosaicos de la rejilla: dos columnas, de 64 de alto, con 8 entre ellos.
const ALTO_CELDA: i32 = 64;
const ANCHO_CELDA: i32 = (ANCHO - 32 - 8) / 2;
/// Los mosaicos: a lo ancho, de 64 de alto, con 8 entre uno y otro.
const X: i32 = 16;
const W: i32 = ANCHO - 32;
/// En edición, cada fila de 56, con sus botones de icono de 40 a la derecha.
const ALTO_FILA: i32 = 56;
const ICONO: i32 = 40;
const BOTON_IZQUIERDO: u32 = 0x110;
/// Los mosaicos, por su sitio en `MOSAICOS`.
const OSCURO: usize = 0;
const BLOQUEAR: usize = 1;
const VOLUMEN: usize = 2;

/// Lo que hace un clic en cada sitio.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Accion {
    Editar,
    Hecho,
    /// Un mosaico (por su sitio en `MOSAICOS`), fuera de edición.
    Mosaico(usize),
    VolumenMenos,
    VolumenMas,
    /// El altavoz del deslizador: pone o quita el silencio.
    Silencio,
    /// Un clic en la pista del volumen: el nivel de donde cae.
    Pista,
    /// En edición, por el puesto en el orden.
    Subir(usize),
    Bajar(usize),
    Quitar(usize),
    /// Volver a poner un quitado (por su sitio en `MOSAICOS`).
    Poner(usize),
    /// Abrir o cerrar el menú de apagado.
    MenuApagado,
    /// Una fila del menú de apagado (por su sitio en `ENERGIA`).
    Energia(usize),
}

/// Dónde va cada cosa, según el orden y si se edita. Lo usan el pintado y los clics: no se desalinean.
struct Disposicion {
    alto: i32,
    /// Los mosaicos o filas que se ven: (dónde, mosaico, quitado).
    filas: Vec<(Rect, usize, bool)>,
    /// Los sitios donde un clic hace algo.
    zonas: Vec<(Rect, Accion)>,
    /// En edición, dónde va el rótulo "Quitados" (su línea base).
    rotulo: i32,
    /// El deslizador del brillo (fuera de edición).
    brillo: Rect,
}

/// La red no es uno de los mosaicos guardados (`panel_mosaicos`): va siempre la primera de la rejilla.
const MOSAICO_RED: usize = usize::MAX;

/// La pista del deslizador del volumen dentro de su fila: tras el altavoz y antes de "−" y "+".
fn pista_volumen(r: Rect) -> Rect {
    Rect::new(r.x + 56, r.y + (r.h - 16) / 2, r.w - 56 - 2 * (ICONO + 4) - 16, 16)
}

fn disponer(orden: &Orden, editando: bool, menu: bool) -> Disposicion {
    let mut d = Disposicion { alto: 0, filas: Vec::new(), zonas: Vec::new(), rotulo: 0, brillo: Rect::new(0, 0, 0, 0) };
    d.zonas.push((BOTON_CABECERA, if editando { Accion::Hecho } else { Accion::Editar }));
    // Los botones de icono, desde la derecha, con 4 entre uno y otro.
    let icono = |r: Rect, k: i32| Rect::new(r.x + r.w - 8 - ICONO - k * (ICONO + 4), r.y + (r.h - ICONO) / 2, ICONO, ICONO);
    if !editando {
        d.zonas.push((BOTON_BLOQUEAR, Accion::Mosaico(BLOQUEAR)));
        d.zonas.push((BOTON_APAGADO, Accion::MenuApagado));
        let mut y = CABECERA;
        if menu {
            // El menú de apagado: una fila por cosa, en vez de los controles.
            for i in 0..ENERGIA.len() {
                let r = Rect::new(X, y, W, ALTO_FILA);
                d.zonas.push((r, Accion::Energia(i)));
                y += ALTO_FILA + 8;
            }
            d.alto = y + 8;
            return d;
        }
        // Los deslizadores: el volumen (si se ve su mosaico) y el brillo, siempre (apagado).
        if orden.vistos().any(|m| m == VOLUMEN) {
            let r = Rect::new(X, y, W, ALTO_DESLIZADOR);
            d.filas.push((r, VOLUMEN, false));
            d.zonas.push((Rect::new(r.x + 4, r.y + 8, ICONO, ICONO), Accion::Silencio));
            d.zonas.push((pista_volumen(r).dentro(-12), Accion::Pista));
            d.zonas.push((icono(r, 1), Accion::VolumenMenos));
            d.zonas.push((icono(r, 0), Accion::VolumenMas));
            y += ALTO_DESLIZADOR + 8;
        }
        d.brillo = Rect::new(X, y, W, ALTO_DESLIZADOR);
        y += ALTO_DESLIZADOR + 16;
        // La rejilla: la red primero (no se quita), y los demás mosaicos que se ven, menos el volumen.
        let mut k = 0;
        let mut celda = |d: &mut Disposicion, m: usize| {
            let r = Rect::new(X + (k % 2) * (ANCHO_CELDA + 8), y + (k / 2) * (ALTO_CELDA + 8), ANCHO_CELDA, ALTO_CELDA);
            d.filas.push((r, m, false));
            if m != MOSAICO_RED {
                d.zonas.push((r, Accion::Mosaico(m)));
            }
            k += 1;
        };
        celda(&mut d, MOSAICO_RED);
        for m in orden.vistos().filter(|&m| m != VOLUMEN) {
            celda(&mut d, m);
        }
        let filas = (k as i32 + 1) / 2;
        d.alto = y + filas * (ALTO_CELDA + 8) + 8;
        return d;
    }
    let mut y = CABECERA;
    for (p, m) in orden.vistos().enumerate() {
        let r = Rect::new(X, y, W, ALTO_FILA);
        d.filas.push((r, m, false));
        d.zonas.push((icono(r, 2), Accion::Subir(p)));
        d.zonas.push((icono(r, 1), Accion::Bajar(p)));
        d.zonas.push((icono(r, 0), Accion::Quitar(p)));
        y += ALTO_FILA + 8;
    }
    d.rotulo = y + 20;
    y += 32;
    let quitados: Vec<usize> = orden.quitados().collect();
    for &m in &quitados {
        let r = Rect::new(X, y, W, ALTO_FILA);
        d.filas.push((r, m, true));
        d.zonas.push((icono(r, 0), Accion::Poner(m)));
        y += ALTO_FILA + 8;
    }
    d.alto = if quitados.is_empty() { y + 28 } else { y + 8 };
    d
}

/// Un trazo de `a` a `b` (en píxeles) con las puntas redondas, de `grosor`, suavizado: 4 × 4 muestras por píxel. Para
/// los iconos (la fuente no trae flechas ni el signo menos).
fn trazo(l: &mut Lienzo, a: (i32, i32), b: (i32, i32), grosor: i32, color: Color) {
    // En octavos de píxel; las puntas en el centro de su píxel.
    let (ax, ay, bx, by) = (a.0 as i64 * 8 + 4, a.1 as i64 * 8 + 4, b.0 as i64 * 8 + 4, b.1 as i64 * 8 + 4);
    let (dx, dy) = (bx - ax, by - ay);
    let largo2 = dx * dx + dy * dy;
    let r = grosor as i64 * 4;
    let caja = Rect::new(a.0.min(b.0) - grosor, a.1.min(b.1) - grosor, (a.0 - b.0).abs() + 2 * grosor + 1, (a.1 - b.1).abs() + 2 * grosor + 1);
    for y in caja.y..caja.y + caja.h {
        for x in caja.x..caja.x + caja.w {
            let mut dentro = 0;
            for sy in 0..4 {
                for sx in 0..4 {
                    let (px, py) = (x as i64 * 8 + sx * 2 + 1 - ax, y as i64 * 8 + sy * 2 + 1 - ay);
                    let t = px * dx + py * dy;
                    let cerca = if largo2 == 0 || t <= 0 {
                        px * px + py * py <= r * r
                    } else if t >= largo2 {
                        let (qx, qy) = (px - dx, py - dy);
                        qx * qx + qy * qy <= r * r
                    } else {
                        let cruz = px * dy - py * dx;
                        cruz * cruz <= r * r * largo2
                    };
                    dentro += cerca as u32;
                }
            }
            l.punto(x, y, color, dentro * 255 / 16);
        }
    }
}

#[derive(Clone, Copy)]
enum Icono {
    Subir,
    Bajar,
    Quitar,
    Poner,
    Menos,
}

/// Un icono de 24 (los de Material: chevrones, cerrar, añadir, quitar) centrado en `r`.
fn icono(l: &mut Lienzo, r: Rect, i: Icono, color: Color) {
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    match i {
        Icono::Subir => {
            trazo(l, (cx - 6, cy + 3), (cx, cy - 3), 2, color);
            trazo(l, (cx, cy - 3), (cx + 6, cy + 3), 2, color);
        }
        Icono::Bajar => {
            trazo(l, (cx - 6, cy - 3), (cx, cy + 3), 2, color);
            trazo(l, (cx, cy + 3), (cx + 6, cy - 3), 2, color);
        }
        Icono::Quitar => {
            trazo(l, (cx - 5, cy - 5), (cx + 5, cy + 5), 2, color);
            trazo(l, (cx + 5, cy - 5), (cx - 5, cy + 5), 2, color);
        }
        Icono::Poner => {
            trazo(l, (cx - 6, cy), (cx + 6, cy), 2, color);
            trazo(l, (cx, cy - 6), (cx, cy + 6), 2, color);
        }
        Icono::Menos => trazo(l, (cx - 6, cy), (cx + 6, cy), 2, color),
    }
}

/// Un texto centrado a lo ancho de `r`, con la línea base en `base`.
fn centrado(f: &mut Fuente, l: &mut Lienzo, r: Rect, base: i32, texto: &str, tam: u32, color: Color) {
    let ancho = f.medir(texto, tam).ancho;
    f.dibujar(l, r.x + (r.w - ancho) / 2, base, texto, tam, color);
}

struct Estado {
    tema: Tema,
    oscuro: bool,
    volumen: Volumen,
    orden: Orden,
    editando: bool,
    /// Lo que dijo la red: 0 sin servicio, 1 sin dirección, 2 conectada; y la dirección.
    red: u32,
    ip: [u8; 4],
    /// Quién está delante (de `sesiones`), si se sabe.
    persona: String,
    /// Se ve el menú de apagado en vez de los controles.
    menu: bool,
}

/// El de encendido (24): un arco abierto arriba y una raya que entra por el hueco. El hueco se borra con el color de
/// `fondo`.
fn icono_encendido(l: &mut Lienzo, r: Rect, color: Color, fondo: Color) {
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    l.borde(Rect::new(cx - 8, cy - 7, 17, 17), 9, 2, color);
    l.rellenar(Rect::new(cx - 4, cy - 8, 9, 6), fondo);
    trazo(l, (cx, cy - 9), (cx, cy - 1), 2, color);
}

/// El de reiniciar (24): un arco abierto arriba a la derecha con la punta de una flecha.
fn icono_reiniciar(l: &mut Lienzo, r: Rect, color: Color, fondo: Color) {
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    l.borde(Rect::new(cx - 8, cy - 8, 17, 17), 9, 2, color);
    l.rellenar(Rect::new(cx + 1, cy - 9, 9, 8), fondo);
    trazo(l, (cx + 1, cy - 8), (cx + 6, cy - 8), 2, color);
    trazo(l, (cx + 6, cy - 8), (cx + 6, cy - 3), 2, color);
}

/// El de cerrar sesión (24): media caja a la izquierda y una flecha que sale a la derecha.
fn icono_salir(l: &mut Lienzo, r: Rect, color: Color) {
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    trazo(l, (cx - 8, cy - 8), (cx - 1, cy - 8), 2, color);
    trazo(l, (cx - 8, cy - 8), (cx - 8, cy + 8), 2, color);
    trazo(l, (cx - 8, cy + 8), (cx - 1, cy + 8), 2, color);
    trazo(l, (cx - 3, cy), (cx + 8, cy), 2, color);
    trazo(l, (cx + 4, cy - 4), (cx + 8, cy), 2, color);
    trazo(l, (cx + 4, cy + 4), (cx + 8, cy), 2, color);
}

/// El candado (24): el cuerpo y el arco.
fn icono_candado(l: &mut Lienzo, r: Rect, color: Color) {
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    l.redondeado(Rect::new(cx - 7, cy - 2, 14, 10), 2, color);
    l.recortar(Rect::new(cx - 8, cy - 10, 16, 9));
    l.borde(Rect::new(cx - 5, cy - 9, 10, 14), 5, 2, color);
    l.soltar_recorte();
}

/// La luna del modo oscuro (24): un círculo al que le muerde otro del color del fondo.
fn icono_luna(l: &mut Lienzo, r: Rect, color: Color, fondo: Color) {
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    l.redondeado(Rect::new(cx - 8, cy - 8, 16, 16), 8, color);
    l.redondeado(Rect::new(cx - 3, cy - 11, 14, 14), 7, fondo);
}

/// La red (24): una caja arriba unida a dos abajo (el "lan" de Material); sin red, tachada.
fn icono_red(l: &mut Lienzo, r: Rect, color: Color, tachada: bool) {
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    l.redondeado(Rect::new(cx - 4, cy - 10, 8, 6), 1, color);
    l.rellenar(Rect::new(cx - 1, cy - 4, 2, 4), color);
    l.rellenar(Rect::new(cx - 8, cy, 16, 2), color);
    l.rellenar(Rect::new(cx - 8, cy + 2, 2, 2), color);
    l.rellenar(Rect::new(cx + 6, cy + 2, 2, 2), color);
    l.redondeado(Rect::new(cx - 10, cy + 4, 8, 6), 1, color);
    l.redondeado(Rect::new(cx + 2, cy + 4, 8, 6), 1, color);
    if tachada {
        trazo(l, (cx - 10, cy - 10), (cx + 10, cy + 10), 2, color);
    }
}

/// El sol del brillo (24): un círculo y ocho rayos.
fn icono_sol(l: &mut Lienzo, r: Rect, color: Color) {
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    l.redondeado(Rect::new(cx - 4, cy - 4, 9, 9), 4, color);
    for (dx, dy) in [(0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1)] {
        let (a, b) = if dx != 0 && dy != 0 { (5, 7) } else { (7, 9) };
        trazo(l, (cx + dx * a, cy + dy * a), (cx + dx * b, cy + dy * b), 2, color);
    }
}

/// Un deslizador de fila: el contenedor alto con esquinas de 28, el icono, la pista (activa en el primario y el resto en
/// el contenedor secundario, de 16 con los extremos redondos, como el deslizador del kit) y el agarre (una barra de 4).
fn deslizador(l: &mut Lienzo, r: Rect, pista: Rect, valor: u32, activo: bool, t: &Tema) {
    l.redondeado(r, kit::tema::RADIO_DIALOGO, t.contenedor_alto());
    let (activa, resto) = if activo { (t.primario, t.contenedor_secundario()) } else { (apagado(t.sobre_superficie), kit::tema::fondo_apagado(t.sobre_superficie)) };
    let lleno = (pista.w * valor.min(100) as i32 / 100).clamp(0, pista.w);
    // Hueco de 6 a cada lado del agarre.
    if lleno > 6 {
        l.redondeado_lados(Rect::new(pista.x, pista.y, lleno - 6, pista.h), 8, 2, activa);
    }
    if pista.w - lleno > 6 {
        l.redondeado_lados(Rect::new(pista.x + lleno + 6, pista.y, pista.w - lleno - 6, pista.h), 2, 8, resto);
    }
    l.redondeado(Rect::new(pista.x + lleno - 2, pista.y - 10, 4, pista.h + 20), 2, activa);
}

/// Fuera de edición: los deslizadores y la rejilla.
fn pintar_controles(l: &mut Lienzo, e: &Estado, d: &Disposicion, fuente: &mut Fuente, medio: &mut Fuente) {
    let t = &e.tema;
    for &(r, m, _) in &d.filas {
        if m == VOLUMEN {
            let v = e.volumen;
            deslizador(l, r, pista_volumen(r), v.efectivo(), !v.silencio, t);
            icono_altavoz(l, Rect::new(r.x + 12, r.y + 16, 24, 24), v.silencio, v.nivel, t.sobre_superficie);
            continue;
        }
        // Un mosaico: el icono en un cuadrado redondeado (en el primario si está encendido), el nombre y su estado.
        let encendido = (m == OSCURO && e.oscuro) || (m == MOSAICO_RED && e.red == 2);
        l.redondeado(r, 20, t.contenedor_alto());
        let cuadro = Rect::new(r.x + 12, r.y + 12, 40, 40);
        let (fondo, tinta) = if encendido { (t.primario, t.sobre_primario) } else { (t.contenedor_secundario(), t.sobre_contenedor_secundario()) };
        l.redondeado(cuadro, 12, fondo);
        let (titulo, abajo) = match m {
            MOSAICO_RED => {
                icono_red(l, cuadro, tinta, e.red != 2);
                let abajo = match e.red {
                    2 => alloc::format!("Conectada · {}.{}.{}.{}", e.ip[0], e.ip[1], e.ip[2], e.ip[3]),
                    1 => String::from("Sin dirección"),
                    _ => String::from("Sin red"),
                };
                ("Red", abajo)
            }
            OSCURO => {
                icono_luna(l, cuadro, tinta, fondo);
                (MOSAICOS[m].1, String::from(if e.oscuro { "Encendido" } else { "Apagado" }))
            }
            _ => {
                icono_candado(l, cuadro, tinta);
                (MOSAICOS[m].1, String::from("Super+L"))
            }
        };
        let ancho = r.w - 64 - 12;
        let titulo = que_quepa(medio, titulo, 14, ancho);
        medio.dibujar(l, r.x + 64, r.y + 29, &titulo, 14, t.sobre_superficie);
        let abajo = que_quepa(fuente, &abajo, 12, ancho);
        fuente.dibujar(l, r.x + 64, r.y + 47, &abajo, 12, t.sobre_variante_superficie);
    }
    // Los botones del volumen: de icono, sin fondo dentro de la fila; en el tope, apagados.
    for &(z, a) in &d.zonas {
        if matches!(a, Accion::VolumenMenos | Accion::VolumenMas) {
            let apagado_si = (a == Accion::VolumenMenos && e.volumen.efectivo() == 0) || (a == Accion::VolumenMas && e.volumen.efectivo() == 100);
            l.redondeado(z, ICONO / 2, if apagado_si { kit::tema::fondo_apagado(t.sobre_superficie) } else { t.contenedor_secundario() });
            let color = if apagado_si { apagado(t.sobre_superficie) } else { t.sobre_contenedor_secundario() };
            icono(l, z, if a == Accion::VolumenMenos { Icono::Menos } else { Icono::Poner }, color);
        }
    }
    // El brillo: apagado, con el sol y lo que pasa.
    if d.brillo.w > 0 {
        let r = d.brillo;
        let pista = Rect::new(r.x + 56, r.y + (r.h - 16) / 2, r.w - 56 - 140, 16);
        deslizador(l, r, pista, 100, false, t);
        icono_sol(l, Rect::new(r.x + 12, r.y + 16, 24, 24), apagado(t.sobre_superficie));
        fuente.dibujar(l, pista.x + pista.w + 16, r.y + 33, "Sin control aquí", 12, apagado(t.sobre_superficie));
    }
}

/// Un texto que cabe en `ancho`: si no, cortado con puntos suspensivos.
fn que_quepa(f: &mut Fuente, texto: &str, tam: u32, ancho: i32) -> String {
    if f.medir(texto, tam).ancho <= ancho {
        return String::from(texto);
    }
    let mut t = String::from(texto);
    while !t.is_empty() {
        t.pop();
        let prueba = alloc::format!("{}…", t.trim_end());
        if f.medir(&prueba, tam).ancho <= ancho {
            return prueba;
        }
    }
    String::new()
}

fn pintar(px: &mut [u32], alto: i32, e: &Estado, d: &Disposicion, fuente: &mut Fuente, medio: &mut Fuente) {
    // Material 3: el panel sobre el contenedor de superficie, con esquinas de 28 (fuera, transparente: el búfer es
    // ARGB); los mosaicos con esquinas de 20 (las filas de edición, de 16), el encendido en el primario y los demás en
    // el contenedor alto. En edición, los quitados con un borde y sin fondo.
    let t = &e.tema;
    px[..(ANCHO * alto) as usize].fill(0);
    let mut l = Lienzo::new(px, ANCHO, alto, ANCHO as usize);
    l.redondeado(Rect::new(0, 0, ANCHO, alto), 28, t.contenedor());
    if e.editando {
        fuente.dibujar(&mut l, 24, 46, "Editar panel", 22, t.sobre_superficie);
    } else {
        // Quién está delante: su inicial en un círculo del contenedor primario, el nombre y cuánto lleva encendido.
        let avatar = Rect::new(20, 18, 40, 40);
        l.redondeado(avatar, 20, t.contenedor_primario);
        let nombre = if e.persona.is_empty() { "DashOS" } else { e.persona.as_str() };
        let inicial: String = nombre.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
        centrado(medio, &mut l, avatar, avatar.y + 26, &inicial, 16, t.sobre_contenedor_primario);
        medio.dibujar(&mut l, 72, 35, nombre, 16, t.sobre_superficie);
        let min = dashos::ahora_us() / 60_000_000;
        let encendido = if min < 60 { alloc::format!("Encendido hace {} min", min) } else { alloc::format!("Encendido hace {} h {} min", min / 60, min % 60) };
        fuente.dibujar(&mut l, 72, 54, &encendido, 12, t.sobre_variante_superficie);
        // Bloquear: botón de icono tonal. El de apagado, igual; con el menú abierto, lleno.
        l.redondeado(BOTON_BLOQUEAR, ICONO / 2, t.contenedor_secundario());
        icono_candado(&mut l, BOTON_BLOQUEAR, t.sobre_contenedor_secundario());
        let (fondo, tinta) = if e.menu { (t.primario, t.sobre_primario) } else { (t.contenedor_secundario(), t.sobre_contenedor_secundario()) };
        l.redondeado(BOTON_APAGADO, ICONO / 2, fondo);
        icono_encendido(&mut l, BOTON_APAGADO, tinta, fondo);
    }
    // Editar: botón tonal; Hecho: lleno (lo que se espera que se pulse).
    let (fondo, texto) = if e.editando { (t.primario, t.sobre_primario) } else { (t.contenedor_secundario(), t.sobre_contenedor_secundario()) };
    l.redondeado(BOTON_CABECERA, BOTON_CABECERA.h / 2, fondo);
    centrado(medio, &mut l, BOTON_CABECERA, BOTON_CABECERA.y + 25, if e.editando { "Hecho" } else { "Editar" }, 14, texto);
    if !e.editando && e.menu {
        // El menú de apagado: filas como las de edición, con su icono en un cuadrado redondeado.
        for &(r, a) in &d.zonas {
            let Accion::Energia(i) = a else { continue };
            l.redondeado(r, 16, t.contenedor_alto());
            let cuadro = Rect::new(r.x + 8, r.y + 8, 40, 40);
            let (fondo, tinta) = (t.contenedor_secundario(), t.sobre_contenedor_secundario());
            l.redondeado(cuadro, 12, fondo);
            match i {
                0 => icono_encendido(&mut l, cuadro, tinta, fondo),
                1 => icono_reiniciar(&mut l, cuadro, tinta, fondo),
                2 => icono_salir(&mut l, cuadro, tinta),
                _ => icono_candado(&mut l, cuadro, tinta),
            }
            medio.dibujar(&mut l, r.x + 64, r.y + 34, ENERGIA[i].0, 16, t.sobre_superficie);
        }
        return;
    }
    if !e.editando {
        pintar_controles(&mut l, e, d, fuente, medio);
        return;
    }
    for &(r, m, quitado) in &d.filas {
        if quitado {
            l.borde(r, 16, 1, t.variante_contorno);
            fuente.dibujar(&mut l, r.x + 16, r.y + 34, MOSAICOS[m].1, 16, t.sobre_variante_superficie);
        } else {
            l.redondeado(r, 16, t.contenedor_alto());
            medio.dibujar(&mut l, r.x + 16, r.y + 34, MOSAICOS[m].1, 16, t.sobre_superficie);
        }
    }
    medio.dibujar(&mut l, 24, d.rotulo, "Quitados", 14, t.sobre_variante_superficie);
    if e.orden.quitados().next().is_none() {
        fuente.dibujar(&mut l, 24, d.rotulo + 26, "Ninguno", 14, t.sobre_variante_superficie);
    }
    let n = e.orden.cuantos();
    for &(z, a) in &d.zonas {
        let (i, desactivado) = match a {
            Accion::Subir(p) => (Icono::Subir, p == 0),
            Accion::Bajar(p) => (Icono::Bajar, p + 1 == n),
            Accion::Quitar(_) => (Icono::Quitar, false),
            Accion::Poner(_) => (Icono::Poner, false),
            _ => continue,
        };
        let color = if desactivado { apagado(t.sobre_variante_superficie) } else { t.sobre_variante_superficie };
        icono(&mut l, z, i, color);
    }
}

/// El altavoz de Material (24), centrado en `r`: la caja, la bocina y dos ondas; con el silencio, una cruz en vez de
/// las ondas. Las ondas, como trozos de un borde redondo recortado a la derecha.
fn icono_altavoz(l: &mut Lienzo, r: Rect, silencio: bool, nivel: u32, color: Color) {
    let (cx, cy) = (r.x + r.w / 2 - 4, r.y + r.h / 2);
    l.redondeado(Rect::new(cx - 7, cy - 3, 5, 7), 1, color);
    // La bocina: un trapecio, columna a columna, con los bordes suavizados por dos trazos.
    for i in 0..5 {
        l.rellenar(Rect::new(cx - 2 + i, cy - 3 - i, 1, 7 + 2 * i), color);
    }
    trazo(l, (cx - 2, cy - 3), (cx + 2, cy - 7), 1, color);
    trazo(l, (cx - 2, cy + 3), (cx + 2, cy + 7), 1, color);
    if silencio {
        trazo(l, (cx + 6, cy - 3), (cx + 11, cy + 2), 2, color);
        trazo(l, (cx + 11, cy - 3), (cx + 6, cy + 2), 2, color);
        return;
    }
    l.recortar(Rect::new(cx + 4, r.y, r.w, r.h));
    if nivel > 0 {
        l.borde(Rect::new(cx - 1, cy - 4, 9, 9), 5, 2, color);
    }
    if nivel > 50 {
        l.borde(Rect::new(cx - 4, cy - 8, 17, 17), 9, 2, color);
    }
    l.soltar_recorte();
}

/// S27, oleada 2: el estado de la red (0 sin servicio, 1 sin dirección, 2 conectada) y la dirección, con poco plazo
/// (el compositor suelta a quien no lee).
fn estado_red() -> (u32, [u8; 4]) {
    use dashos::protocolo::red as r;
    match dashos::red_cliente::estado(RED, PLAZO_RED_US) {
        Ok(e) if e[r::E_CONFIGURADA] != 0 => (2, e[r::E_IP].to_le_bytes()),
        Ok(_) => (1, [0; 4]),
        Err(_) => (0, [0; 4]),
    }
}

/// Apagar, reiniciar o cerrar la sesión (`MODO_*`), por `sesiones`: lo que conteste (0 si lo hace o lo pregunta).
fn pedir_apagado(modo: u32) -> i32 {
    use dashos::protocolo::sesiones as s;
    let (lectura, escritura) = dashos::canal_crear();
    let mut p = [0u8; 8];
    p[0..4].copy_from_slice(&s::APAGAR.to_le_bytes());
    p[4..8].copy_from_slice(&modo.to_le_bytes());
    let mut r = [0u8; 4];
    let n = if dashos::canal_escribir(SESIONES, &p, &[escritura]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], 2_000_000).0 } else { -1 };
    dashos::cerrar(lectura);
    if n == 4 { i32::from_le_bytes(r) } else { i32::MIN }
}

/// Prueba hostil: con la llave de "red-estado", pedir una conexión (al eco del PC en la prueba) no da nada: ni
/// `R_CONECTAR` ni memoria. Verdadero si no llegó nada en un segundo (o se cerró la respuesta).
fn conectar_con_el_estado() -> bool {
    use dashos::protocolo::red as r;
    let (lectura, escritura) = dashos::canal_crear();
    let pedido = [r::CONECTAR, r::IPV4, 7, r::ANILLO_MIN as u32, u32::from_le_bytes([10, 0, 2, 100]), 0, 0, 0];
    let b: Vec<u8> = pedido.iter().flat_map(|w| w.to_le_bytes()).collect();
    let mut resp = [0u8; 16];
    let mut ll = [0u32; 4];
    let (n, k) = if dashos::canal_escribir(RED, &b, &[escritura]) == 0 {
        let (n, k, _) = dashos::canal_leer_plazo(lectura, &mut resp, &mut ll, 1_000_000);
        (n, k)
    } else {
        (-1, 0)
    };
    // Con error, `k` no vale (la llamada no lo toca): solo se miran las llaves si llegó algo.
    if n >= 0 {
        ll[..k.min(ll.len())].iter().for_each(|&l| {
            dashos::cerrar(l);
        });
    }
    dashos::cerrar(lectura);
    n < 0
}

/// S27, oleada 2: quién está delante, de `sesiones` (vacío si no hay nadie o no contesta).
fn quien_delante() -> String {
    use dashos::protocolo::sesiones as s;
    let (lectura, escritura) = dashos::canal_crear();
    let mut r = [0u8; s::LARGO_DELANTE];
    let n = if dashos::canal_escribir(SESIONES, &s::DELANTE.to_le_bytes(), &[escritura]) == 0 {
        dashos::canal_leer_plazo(lectura, &mut r, &mut [], 300_000).0
    } else {
        -1
    };
    dashos::cerrar(lectura);
    if n != s::LARGO_DELANTE as i64 || r[..4] != [0; 4] {
        return String::new();
    }
    let fin = r[4..].iter().position(|&b| b == 0).map_or(16, |p| p);
    core::str::from_utf8(&r[4..4 + fin]).map(|t| t.chars().filter(|c| !c.is_control()).collect()).unwrap_or_default()
}

/// S27, oleada 2: si cambió el estado de la red o del volumen, se dice al compositor, que se lo pasa a la barra.
fn decir_estado(c: &mut Cliente, e: &Estado, dicho: &mut Option<(u32, u32, u32)>) {
    let ahora = (e.red, e.volumen.nivel, e.volumen.silencio as u32);
    if *dicho == Some(ahora) {
        return;
    }
    *dicho = Some(ahora);
    let mut w = Escritor::new();
    w.empezar(TEMA, dashos_tema_v1::req::ESTADO).uint(ahora.0).uint(ahora.1).uint(ahora.2).fin();
    c.mandar(&w);
}

/// El volumen general al servicio de sonido (sin respuesta).
fn pedir_volumen(por_ciento: u32) {
    let p: [u32; 2] = [GENERAL, (por_ciento as f32 / 100.0).to_bits()];
    let b: Vec<u8> = p.iter().flat_map(|w| w.to_le_bytes()).collect();
    dashos::canal_escribir(SONIDO, &b, &[]);
}

/// Un pedido al centro de control con su llave de respuesta: los bytes de la respuesta (o nada, si no contesta en
/// `plazo_us`).
fn al_centro(pedido: &[u8], respuesta: &mut [u8], plazo_us: u64) -> Option<usize> {
    let (lectura, escritura) = dashos::canal_crear();
    let n = if dashos::canal_escribir(CENTRO, pedido, &[escritura]) == 0 {
        dashos::canal_leer_plazo(lectura, respuesta, &mut [], plazo_us).0
    } else {
        -1
    };
    dashos::cerrar(lectura);
    usize::try_from(n).ok()
}

/// Pide el modo al centro de control; su estado (0 si lo puso).
fn pedir_modo(oscuro: bool) -> i32 {
    let mut r = [0u8; 4];
    match al_centro(if oscuro { b"modo oscuro" } else { b"modo claro" }, &mut r, 5_000_000) {
        Some(4) => i32::from_le_bytes(r),
        _ => i32::MIN,
    }
}

/// Los mosaicos guardados, por el centro: (su estado, el orden). Nada si no contesta o contesta algo que no vale.
fn leer_mosaicos(plazo_us: u64) -> Option<(i32, Orden)> {
    let mut r = [0u8; 4 + MAX_TEXTO];
    let n = al_centro(b"panel leer", &mut r, plazo_us)?;
    if n < 4 {
        return None;
    }
    let estado = i32::from_le_bytes(r[..4].try_into().unwrap());
    Orden::entender(&r[4..n]).ok().map(|o| (estado, o))
}

/// Que el centro guarde los mosaicos; su estado (0 si se guardaron).
fn guardar_mosaicos(orden: &Orden) -> i32 {
    let mut linea = [0u8; MAX_TEXTO];
    let n = orden.escribir(&mut linea);
    let mut p = Vec::from(&b"panel guardar "[..]);
    p.extend_from_slice(&linea[..n - 1]);
    let mut r = [0u8; 4];
    match al_centro(&p, &mut r, 5_000_000) {
        Some(4) => i32::from_le_bytes(r),
        _ => i32::MIN,
    }
}

fn nombres(orden: &Orden) -> String {
    let v: Vec<&str> = orden.vistos().map(|m| MOSAICOS[m].0).collect();
    if v.is_empty() { String::from("ninguno") } else { v.join(" ") }
}

/// Un punto del panel visto desde la esquina de arriba a la derecha de la pantalla (para las pruebas).
fn desde_la_esquina(r: Rect) -> (i32, i32) {
    (MARGEN + ANCHO - r.x - r.w / 2, MARGEN + r.y + r.h / 2)
}

/// Para las pruebas: dónde queda cada botón, desde la esquina de arriba a la derecha de la pantalla.
fn decir_sitios(e: &Estado, d: &Disposicion, abierto_ahora: bool) {
    let zona = |a: Accion| d.zonas.iter().find(|z| z.1 == a).map(|z| desde_la_esquina(z.0));
    if !e.editando && e.menu {
        let filas: Vec<String> = (0..ENERGIA.len())
            .filter_map(|i| zona(Accion::Energia(i)).map(|(x, y)| alloc::format!("{} ({}, {})", ENERGIA[i].0.to_lowercase(), x, y)))
            .collect();
        println!("  [panel] menú de apagado: {}", filas.join(" "));
        return;
    }
    if !e.editando {
        let (ex, ey) = desde_la_esquina(BOTON_CABECERA);
        println!("  [panel] se ven: {}; editar a ({}, {})", nombres(&e.orden), ex, ey);
        if abierto_ahora {
            let (ax, ay) = desde_la_esquina(BOTON_APAGADO);
            println!("  [panel] botón de apagado a ({}, {})", ax, ay);
            if let Some((x, y)) = zona(Accion::Mosaico(OSCURO)) {
                println!("  [panel] abierto; mosaico a ({}, {}) de la esquina de arriba a la derecha", x, y);
            }
            if let (Some(menos), Some(mas)) = (zona(Accion::VolumenMenos), zona(Accion::VolumenMas)) {
                println!("  [panel] volumen: menos a ({}, {}), más a ({}, {})", menos.0, menos.1, mas.0, mas.1);
            }
        }
        return;
    }
    // Una línea por fila (una larga se podría mezclar con lo de otro programa), con el número de esta vez.
    static VECES: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    let n = VECES.fetch_add(1, core::sync::atomic::Ordering::Relaxed) + 1;
    let (hx, hy) = desde_la_esquina(BOTON_CABECERA);
    println!("  [panel] edición {}: hecho ({}, {})", n, hx, hy);
    for (p, m) in e.orden.vistos().enumerate() {
        let (s, b, q) = (zona(Accion::Subir(p)).unwrap(), zona(Accion::Bajar(p)).unwrap(), zona(Accion::Quitar(p)).unwrap());
        println!("  [panel] edición {}: {}: subir ({}, {}) bajar ({}, {}) quitar ({}, {})", n, MOSAICOS[m].0, s.0, s.1, b.0, b.1, q.0, q.1);
    }
    for m in e.orden.quitados() {
        let (x, y) = zona(Accion::Poner(m)).unwrap();
        println!("  [panel] edición {}: {}: poner ({}, {})", n, MOSAICOS[m].0, x, y);
    }
}

fn principal() -> u64 {
    dashos::monton::con_nombre(&"panel");
    let (mut fuente, mut medio) = (Fuente::regular(), Fuente::medium());
    // Los mosaicos guardados, antes de nada (al arrancar no hay prisa: nadie ha pulsado aún Super+A).
    let mut orden = Orden::de_serie();
    let mut cargado = false;
    match leer_mosaicos(5_000_000) {
        Some((estado, o)) => {
            println!("  [panel] mosaicos: {} (el centro: {})", nombres(&o), estado);
            orden = o;
            // Sin servicio de archivos (-2), se vuelve a pedir al abrir.
            cargado = estado >= 0;
        }
        None => println!("  [panel] el centro no dice los mosaicos: los de serie"),
    }
    println!("  [panel] con la llave del estado de la red, abrir una conexión: {}", if conectar_con_el_estado() { "no se puede (bien)" } else { "MAL" });
    let Some(mut c) = Cliente::conectar() else {
        println!("  [panel] sin compositor");
        return 1;
    };
    let Some(globales) = c.globales() else { return 2 };
    let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
    let (Some(gc), Some(gs), Some(ga), Some(gl), Some(gt)) =
        (global("wl_compositor"), global("wl_shm"), global("wl_seat"), global("zwlr_layer_shell_v1"), global("dashos_tema_v1"))
    else {
        println!("  [panel] el compositor no le da capas o tema");
        return 3;
    };
    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gl).texto(Some("zwlr_layer_shell_v1")).uint(4).uint(CAPAS).fin();
    // La 3: las teclas del volumen (S27).
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gt).texto(Some("dashos_tema_v1")).uint(5).uint(TEMA).fin();
    e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
    c.mandar(&e);
    // La memoria del dibujo, una vez; los búferes, según la altura.
    let tamano = (ANCHO * ALTO_MAX * 4) as usize;
    let m = dashos::memoria_crear(tamano);
    if (m as i32) <= 0 || dashos::memoria_proyectar(m as u32, VA, true) != 0 {
        println!("  [panel] sin memoria");
        return 4;
    }
    let copia = dashos::llave_duplicar(m as u32, dashos::LEER | dashos::TRANSFERIR, 0);
    let mut e = Escritor::new();
    e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(POOL).llave(copia as u32).int(tamano as i32).fin();
    c.mandar(&e);
    println!("  [panel] en marcha: Super+A lo abre");
    let mut est = Estado { tema: Tema::CLARO, oscuro: false, volumen: Volumen::new(), orden, editando: false, red: 0, ip: [0; 4], persona: String::new(), menu: false };
    // S27, oleada 2: lo último que se dijo al compositor del estado (para la barra) y cuándo toca preguntar a la red.
    let mut estado_dicho: Option<(u32, u32, u32)> = None;
    let mut proxima_red = 0u64;
    let mut abierto = false;
    // Cuántas veces se abrió (con Super+A o con el botón de la barra): para las pruebas.
    let mut veces_abierto = 0u32;
    // Lo configurado por el compositor (la altura de la capa) y el pedido; 0 si aún nada.
    let (mut configurado, mut pedido) = (0i32, 0i32);
    // Los dos búferes: su altura (0 si no está hecho) y cuál se ve.
    let mut buferes = [(BUFER, 0i32), (BUFER_2, 0i32)];
    let mut visto = 0usize;
    // Lo que había al entrar en edición, para guardar solo si cambió.
    let mut antes = est.orden;
    let (mut x, mut y, mut dentro) = (0i32, 0i32, false);
    // Pinta con la altura configurada (con el búfer de esa altura) o, si el contenido pide otra, pide a la capa ese
    // tamaño y pinta al llegar su configure.
    let pintar_o_pedir = |c: &mut Cliente, est: &Estado, configurado: i32, pedido: &mut i32, buferes: &mut [(u32, i32); 2], visto: &mut usize,
                          fuente: &mut Fuente, medio: &mut Fuente, decir: bool, abierto_ahora: bool| {
        let d = disponer(&est.orden, est.editando, est.menu);
        let alto = d.alto.min(ALTO_MAX);
        if alto != configurado {
            if *pedido != alto {
                *pedido = alto;
                let mut e = Escritor::new();
                e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_SIZE).uint(ANCHO as u32).uint(alto as u32).fin();
                e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
                c.mandar(&e);
            }
            return;
        }
        let mut e = Escritor::new();
        if buferes[*visto].1 != alto {
            // El otro búfer, rehecho con esta altura (sobre la misma memoria); el que se veía sigue hasta el commit.
            let otro = 1 - *visto;
            if buferes[otro].1 != 0 {
                e.empezar(buferes[otro].0, wl_buffer::req::DESTROY).fin();
            }
            e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER).uint(buferes[otro].0).int(0).int(ANCHO).int(alto).int(ANCHO * 4).uint(0).fin();
            buferes[otro].1 = alto;
            *visto = otro;
        }
        let px = unsafe { core::slice::from_raw_parts_mut(VA as *mut u32, tamano / 4) };
        pintar(px, alto, est, &d, fuente, medio);
        e.empezar(SUPERFICIE, wl_surface::req::ATTACH).uint(buferes[*visto].0).int(0).int(0).fin();
        e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(ANCHO).int(alto).fin();
        e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
        c.mandar(&e);
        if decir {
            decir_sitios(est, &d, abierto_ahora);
        }
    };
    // Lo que hay que decir tras el próximo pintado (para las pruebas): (decir, recién abierto).
    let mut por_decir = (false, false);
    loop {
        // S27, oleada 2: cada `CADA_RED_US`, el estado de la red; si cambia algo, se dice (a la barra, por el compositor).
        if dashos::ahora_us() >= proxima_red {
            proxima_red = dashos::ahora_us() + CADA_RED_US;
            let (red, ip) = estado_red();
            if (red, ip) != (est.red, est.ip) {
                println!("  [panel] red: {}", match red {
                    2 => alloc::format!("conectada ({}.{}.{}.{})", ip[0], ip[1], ip[2], ip[3]),
                    1 => String::from("sin dirección"),
                    _ => String::from("sin servicio"),
                });
                (est.red, est.ip) = (red, ip);
                if abierto && configurado > 0 && !est.editando {
                    pintar_o_pedir(&mut c, &est, configurado, &mut pedido, &mut buferes, &mut visto, &mut fuente, &mut medio, false, false);
                }
            }
        }
        decir_estado(&mut c, &est, &mut estado_dicho);
        // Se espera como mucho hasta que toque preguntar otra vez a la red. (S27: el OSD del volumen lo enseña el dock,
        // con el aviso del compositor y el nivel que se le da aquí con `estado`.)
        let hasta = proxima_red;
        let ev = c.evento_plazo(hasta.saturating_sub(dashos::ahora_us()).max(1));
        let ev = match ev {
            Err(()) => {
                println!("  [panel] se cortó la conexión con el compositor");
                return 5;
            }
            Ok(ev) => ev,
        };
        let Some((o, orden_ev, cuerpo)) = ev else { continue };
        let palabra = |i: usize| u32::from_le_bytes(cuerpo[4 * i..4 * i + 4].try_into().unwrap());
        // Cerrar el panel: si se estaba editando, se acaba la edición y se guarda.
        let cerrar = |c: &mut Cliente, est: &mut Estado, antes: &Orden, bloquear: bool| {
            let mut e = Escritor::new();
            e.empezar(CAPA, zwlr_layer_surface_v1::req::DESTROY).fin();
            e.empezar(SUPERFICIE, wl_surface::req::DESTROY).fin();
            if bloquear {
                e.empezar(TEMA, dashos_tema_v1::req::BLOQUEAR).fin();
            }
            c.mandar(&e);
            if est.editando {
                est.editando = false;
                if est.orden != *antes {
                    println!("  [panel] mosaicos guardados al cerrar: {}", guardar_mosaicos(&est.orden));
                }
            }
        };
        match (o, orden_ev) {
            (TEMA, dashos_tema_v1::ev::TEMA) => {
                est.oscuro = palabra(1) != 0;
                est.tema = Tema::del_sistema(palabra(0) | 0xff00_0000, est.oscuro);
                if abierto && configurado > 0 {
                    pintar_o_pedir(&mut c, &est, configurado, &mut pedido, &mut buferes, &mut visto, &mut fuente, &mut medio, false, false);
                }
            }
            (TEMA, dashos_tema_v1::ev::PANEL) if abierto => {
                abierto = false;
                cerrar(&mut c, &mut est, &antes, false);
                println!("  [panel] cerrado");
            }
            (TEMA, dashos_tema_v1::ev::PANEL) => {
                // Quién está delante, cada vez (puede haber cambiado de persona).
                let persona = quien_delante();
                if persona != est.persona {
                    println!("  [panel] delante: {}", if persona.is_empty() { "nadie" } else { &persona });
                    est.persona = persona;
                }
                // Si al arrancar el centro no los dijo, otra vez (con poco plazo: el compositor suelta a quien no lee).
                if !cargado && let Some((estado, o)) = leer_mosaicos(1_000_000) {
                    println!("  [panel] mosaicos: {} (el centro: {})", nombres(&o), estado);
                    est.orden = o;
                    cargado = estado >= 0;
                }
                // Arriba a la derecha (anclas 1 arriba y 8 derecha), en la capa de encima (2), con el teclado a demanda (2).
                // Sin zona propia: el compositor lo pone bajo la barra, si la hay (S10).
                abierto = true;
                veces_abierto += 1;
                println!("  [panel] abierto por {}.ª vez", veces_abierto);
                est.menu = false;
                (configurado, pedido) = (0, disponer(&est.orden, false, false).alto.min(ALTO_MAX));
                por_decir = (true, true);
                let mut e = Escritor::new();
                e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUPERFICIE).fin();
                e.empezar(CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(CAPA).uint(SUPERFICIE).uint(0).uint(2).texto(Some("panel")).fin();
                e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_SIZE).uint(ANCHO as u32).uint(pedido as u32).fin();
                e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(1 | 8).fin();
                e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_MARGIN).int(MARGEN).int(MARGEN).int(0).int(0).fin();
                e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_KEYBOARD_INTERACTIVITY).uint(2).fin();
                e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
                c.mandar(&e);
            }
            (CAPA, zwlr_layer_surface_v1::ev::CONFIGURE) => {
                let mut e = Escritor::new();
                e.empezar(CAPA, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                c.mandar(&e);
                configurado = palabra(2) as i32;
                let (decir, ahora) = por_decir;
                por_decir = (false, false);
                pintar_o_pedir(&mut c, &est, configurado, &mut pedido, &mut buferes, &mut visto, &mut fuente, &mut medio, decir, ahora);
            }
            (CAPA, zwlr_layer_surface_v1::ev::CLOSED) => abierto = false,
            // S27: una tecla del volumen. Se cambia y se manda al servicio de sonido; el OSD lo enseña el dock.
            (TEMA, dashos_tema_v1::ev::VOLUMEN) => {
                let paso = palabra(0) as i32;
                if est.volumen.tecla(paso) {
                    pedir_volumen(est.volumen.efectivo());
                }
                if abierto && configurado > 0 && !est.editando {
                    pintar_o_pedir(&mut c, &est, configurado, &mut pedido, &mut buferes, &mut visto, &mut fuente, &mut medio, false, false);
                }
            }
            (PUNTERO, wl_pointer::ev::ENTER) => {
                dentro = true;
                (x, y) = (palabra(2) as i32 >> 8, palabra(3) as i32 >> 8);
            }
            (PUNTERO, wl_pointer::ev::LEAVE) => dentro = false,
            (PUNTERO, wl_pointer::ev::MOTION) => (x, y) = (palabra(1) as i32 >> 8, palabra(2) as i32 >> 8),
            (PUNTERO, wl_pointer::ev::BUTTON) if palabra(2) == BOTON_IZQUIERDO && palabra(3) == 1 && dentro && abierto && configurado > 0 => {
                let d = disponer(&est.orden, est.editando, est.menu);
                let Some(&(_, accion)) = d.zonas.iter().find(|z| z.0.contiene(x, y)) else { continue };
                // Si cambia lo que se ve, se pinta y se dicen los sitios nuevos.
                let mut cambia = true;
                match accion {
                    Accion::Mosaico(OSCURO) => {
                        let estado = pedir_modo(!est.oscuro);
                        println!("  [panel] modo oscuro {}: {}", if est.oscuro { "apagado" } else { "encendido" }, estado);
                        cambia = false;
                    }
                    Accion::Mosaico(BLOQUEAR) => {
                        // Se cierra el panel y se pide el bloqueo: lo hace quien bloquea (`guardian`), como con Super+L.
                        abierto = false;
                        cerrar(&mut c, &mut est, &antes, true);
                        println!("  [panel] bloquear");
                        continue;
                    }
                    Accion::Mosaico(_) => cambia = false,
                    Accion::MenuApagado => {
                        est.menu = !est.menu;
                        println!("  [panel] menú de apagado {}", if est.menu { "abierto" } else { "cerrado" });
                    }
                    Accion::Energia(i) => {
                        // Se cierra el panel y se pide (bloquear, como el candado).
                        abierto = false;
                        est.menu = false;
                        let Some(modo) = ENERGIA[i].1 else {
                            cerrar(&mut c, &mut est, &antes, true);
                            println!("  [panel] bloquear");
                            continue;
                        };
                        cerrar(&mut c, &mut est, &antes, false);
                        println!("  [panel] {}: {}", ENERGIA[i].0.to_lowercase(), pedir_apagado(modo));
                        continue;
                    }
                    Accion::VolumenMenos | Accion::VolumenMas | Accion::Silencio | Accion::Pista => {
                        match accion {
                            Accion::Silencio => {
                                est.volumen.tecla(0);
                            }
                            Accion::Pista => {
                                let r = d.filas.iter().find(|f| f.1 == VOLUMEN).map(|f| pista_volumen(f.0)).unwrap_or(Rect::new(0, 0, 1, 1));
                                est.volumen.poner(((x - r.x) * 100 / r.w.max(1)).clamp(0, 100) as u32);
                            }
                            _ => est.volumen.boton(accion == Accion::VolumenMas),
                        }
                        pedir_volumen(est.volumen.efectivo());
                        println!("  [panel] volumen {}{}", est.volumen.nivel, if est.volumen.silencio { " (silencio)" } else { "" });
                        pintar_o_pedir(&mut c, &est, configurado, &mut pedido, &mut buferes, &mut visto, &mut fuente, &mut medio, false, false);
                        cambia = false;
                    }
                    Accion::Editar => {
                        est.editando = true;
                        antes = est.orden;
                        println!("  [panel] a editar");
                    }
                    Accion::Hecho => {
                        est.editando = false;
                        if est.orden != antes {
                            let guardado = guardar_mosaicos(&est.orden);
                            cargado = true;
                            println!("  [panel] hecho: {}; guardado: {}", nombres(&est.orden), guardado);
                        } else {
                            println!("  [panel] hecho, sin cambios");
                        }
                    }
                    Accion::Subir(p) | Accion::Bajar(p) | Accion::Quitar(p) => {
                        let m = est.orden.en(p).unwrap_or(0);
                        let (hecho, que) = match accion {
                            Accion::Subir(_) => (est.orden.subir(p), "sube"),
                            Accion::Bajar(_) => (est.orden.bajar(p), "baja"),
                            _ => (est.orden.quitar(p), "quita"),
                        };
                        cambia = hecho;
                        if hecho {
                            println!("  [panel] {} {}", que, MOSAICOS[m].0);
                        }
                    }
                    Accion::Poner(m) => {
                        cambia = est.orden.poner(m);
                        if cambia {
                            println!("  [panel] pone {}", MOSAICOS[m].0);
                        }
                    }
                }
                if cambia {
                    let alto = disponer(&est.orden, est.editando, est.menu).alto.min(ALTO_MAX);
                    if alto == configurado {
                        pintar_o_pedir(&mut c, &est, configurado, &mut pedido, &mut buferes, &mut visto, &mut fuente, &mut medio, true, false);
                    } else {
                        // Se pinta y se dice con el configure del tamaño nuevo.
                        por_decir = (true, false);
                        pintar_o_pedir(&mut c, &est, configurado, &mut pedido, &mut buferes, &mut visto, &mut fuente, &mut medio, false, false);
                    }
                }
            }
            (1, wl_display::ev::ERROR) => {
                println!("  [panel] error de protocolo en el objeto {}", palabra(0));
                return 6;
            }
            _ => {}
        }
    }
}
