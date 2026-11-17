//! S10: la barra de la pantalla y el lanzador, primera versión (`docs/diseño/S10-escritorio.md`). Es un cliente del
//! compositor, como el panel rápido: si se cae, las ventanas siguen en su sitio.
//!
//! La barra es una capa de arriba (wlr-layer-shell) a todo lo ancho, con su zona exclusiva: las ventanas se colocan y
//! se maximizan debajo. Lleva:
//! - A la izquierda, el botón del lanzador.
//! - En medio, el título de la ventana con el teclado. Se lo dice el compositor por `dashos_tema_v1` (evento `titulo`,
//!   solo a la barra): es el de la ventana en la que escribe quien está delante, nunca el de otra, y vacío con la
//!   sesión bloqueada.
//! - A la derecha, la hora (`dashos::hora_ns`, en la zona horaria de `Prefs/zona` del disco del sistema, que lee al
//!   arrancar por su raíz 1; sin ella, UTC; `dashos::zona`) y el botón del panel rápido, que lo pide
//!   como Super+A (petición `panel`, que el compositor solo le acepta a la barra).
//!
//! El lanzador es otra capa, arriba a la izquierda bajo su botón, con el teclado mientras se ve. Se abre con el botón
//! o con Super sola (el compositor se lo dice con el evento `lanzador`). Enseña los programas de `Programas/` (la barra
//! solo ve ese cajón: su raíz en el servicio de archivos) y busca al escribir. Intro lanza el elegido por `sesiones`,
//! que lo lanza como la persona que está delante; la barra no lanza nada por sí misma. Esc, Super o el botón lo cierran.
//!
//! Material 3, con el tema del sistema, como el panel rápido: la barra sobre el contenedor de superficie, botones de
//! icono tonales (el del lanzador, lleno mientras está abierto), y el lanzador como el panel, con esquinas de 28, el
//! buscador en el contenedor alto y lo elegido en el contenedor secundario.
//!
//! Los avisos (las notificaciones, S10): la barra se suscribe al servicio `avisos` con una "puerta" (una copia marcada
//! del extremo por el que le escribe el compositor: un proceso solo espera en un canal), por la que el servicio toca el
//! timbre cuando hay uno nuevo. La barra lo pide y lo enseña como un toast: una capa arriba a la derecha, bajo la barra,
//! unos segundos, que se cierra sola o con un clic. La campana (a la izquierda de la hora) lleva un punto si hay alguno
//! sin leer; al pulsarla, el historial, en una capa como el panel rápido, con "Borrar todo". Lo que llega retenido (de
//! una sesión que estaba detrás, U6) sale en el historial, no como toast.
//!
//! Trasfondo (como en DMS, solo la idea): a la izquierda de la campana, la CPU y la memoria en una píldora (lo mide la
//! barra con la llamada 29 cada 2 s). Al pulsarla, un desplegable con los procesos (`MONITOR` de `sesiones`: los de la
//! persona de delante con detalle, las cajas con su pastilla, y el sistema y otros usuarios sumados).
//!
//! Llaves: 1 el canal "ventanas" (con las capas, por su nombre), 2 el canal "archivos", 3 el canal "sesiones", 4 el
//! canal "centro" (trasfondo: abrir el monitor del sistema) y 5 el canal "avisos".

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
#[allow(dead_code)]
mod ventana_cliente;
#[path = "../lanzables.rs"]
#[allow(dead_code)]
mod lanzables;
#[path = "../lanzador_lista.rs"]
mod lanzador_lista;
#[path = "../reloj.rs"]
mod reloj;
use dashos::zona;
#[path = "../monitor.rs"]
#[allow(dead_code)]
mod monitor;
// S10: los menús del programa activo en la barra (la opción clásica del Amiga).
#[path = "../barra_menus.rs"]
mod barra_menus;

use alloc::string::String;
use alloc::vec::Vec;
use dashos::println;
use dashos::protocolo::archivos as a;
use dashos::protocolo::avisos as av;
use dashos::protocolo::sesiones as s;
use kit::iconos::{simbolo, Simbolos};
use kit::lienzo::{Color, Lienzo, Rect};
use lanzador_lista::{Lista, MAX_PROGRAMAS};
use kit::teclado::{Tecla, Teclado};
use kit::tema::{con_opacidad, Tema};
use kit::texto::Fuente;
use ventana_cliente::*;
use wayland::tablas::{
    self, dashos_tema_v1, wl_buffer, wl_compositor, wl_display, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm_pool, wl_surface,
    zwlr_layer_shell_v1, zwlr_layer_surface_v1,
};
use wayland::{leer_args, Escritor, Valor};

dashos::programa!(principal);
dashos::monton!();

// Las llaves de los canales van en el orden en que se nombra cada uno por primera vez en la lista de `inicio`, no en el
// de la lista de la barra: "archivos" se nombra antes que "sesiones".
const ARCHIVOS: u32 = 2;
const SESIONES: u32 = 3;
/// Trasfondo: el centro de control, para abrir el monitor del sistema ("abrir monitor").
const CENTRO: u32 = 4;
const AVISOS: u32 = 5;
/// La marca de la puerta por la que el servicio de avisos toca el timbre.
const MARCA_TIMBRE: u64 = 0x4156_4953_0000_0001;
// Los objetos propios (los de `ventana_cliente` van del 2 al 14).
const CAPAS: u32 = 20;
const CAPA_BARRA: u32 = 21;
const TEMA: u32 = 22;
const SUP_LANZADOR: u32 = 23;
const CAPA_LANZADOR: u32 = 24;
const BUFER_LANZADOR: u32 = 25;
const SUP_AVISO: u32 = 26;
const CAPA_AVISO: u32 = 27;
const BUFER_AVISO: u32 = 28;
const SUP_HIST: u32 = 29;
const CAPA_HIST: u32 = 30;
const BUFER_HIST: u32 = 31;
// Los pools del lanzador, del aviso y del historial: cada uno con su memoria, solo mientras se ve (kit-memoria).
const POOL_L: u32 = 32;
const POOL_A: u32 = 33;
const POOL_H: u32 = 34;
/// Trasfondo: el desplegable de los procesos.
const SUP_PROC: u32 = 35;
const CAPA_PROC: u32 = 36;
const BUFER_PROC: u32 = 37;
const POOL_P: u32 = 38;
const ANCHO_P: i32 = 400;
const FILAS_P: usize = 10;
const ALTO_FILA_P: i32 = 36;
const Y_FILAS_P: i32 = 76;
const PIE_P: i32 = 52;
const ALTO_P_MAX: i32 = Y_FILAS_P + FILAS_P as i32 * ALTO_FILA_P + 12 + PIE_P;
/// Cada cuánto se mide la CPU y la memoria de la píldora (y el desplegable, si está abierto).
const CADA_MEDIDA_US: u64 = 2_000_000;
/// Dónde se ve la ventana del servicio de archivos.
const VA_ARCHIVOS: usize = 0x50_0000_0000;

/// La barra (S27, al estilo de DMS): una capa de 40 de alto a todo lo ancho, con su zona exclusiva. Dentro, una tira
/// flotante de 32 (a 6 de los lados y 4 de arriba), translúcida, y en ella píldoras de 24 de alto.
const ALTO: i32 = 40;
const ANCHO_MAX: i32 = 3840;
const TIRA_X: i32 = 6;
const TIRA_Y: i32 = 4;
const TIRA_ALTO: i32 = 32;
const PILDORA_Y: i32 = 8;
const PILDORA_ALTO: i32 = 24;
const HUECO: i32 = 6;
/// La primera píldora, a 4 del borde de la tira.
const LADO: i32 = TIRA_X + 4;
/// Lo opaca que es la tira (de 255): se ve algo del fondo detrás.
const OPACIDAD_TIRA: u32 = 224;
/// Los espacios de trabajo que se enseñan como poco (como el mínimo de DMS), aunque estén vacíos.
const MIN_ESPACIOS: u32 = 3;
/// El lanzador (S27, como el de DMS y el "spotlight"): en el centro de la pantalla, el buscador arriba y hasta
/// `FILAS` filas de 56 debajo, con un pie de ayuda.
const ANCHO_L: i32 = 600;
const FILAS: usize = 7;
const ALTO_FILA: i32 = 56;
const BUSCADOR: Rect = Rect { x: 16, y: 16, w: ANCHO_L - 32, h: 56 };
const Y_FILAS: i32 = 88;
const PIE_L: i32 = 52;
/// El toast: una tarjeta de 344 × 80 con su sombra, a 8 de la barra, en un búfer con margen; dura `DURA_AVISO_US`.
const ANCHO_A: i32 = 360;
const ALTO_A: i32 = 100;
const DURA_AVISO_US: u64 = 5_000_000;
/// El historial: la cabecera, hasta `FILAS_H` avisos y el pie.
const ANCHO_H: i32 = 380;
const FILAS_H: usize = 5;
const ALTO_FILA_H: i32 = 76;
const Y_FILAS_H: i32 = 64;
const PIE_H: i32 = 40;
const ALTO_H_MAX: i32 = Y_FILAS_H + FILAS_H as i32 * ALTO_FILA_H + PIE_H;
/// Dónde se ven las memorias de los búferes: la barra en `VA`, del ancho que tiene; el lanzador, el toast y el historial,
/// cada uno en la suya, que se pide al abrirlo y se suelta al cerrarlo (kit-memoria: antes, 2,1 MB siempre, con la barra
/// a 3840 de ancho).
const VA_L: usize = VA + (1 << 32);
const VA_A: usize = VA + (2 << 32);
const VA_H: usize = VA + (3 << 32);
const VA_P: usize = VA + (4 << 32);
const BOTON_IZQUIERDO: u32 = 0x110;
const KEY_ESC: u32 = 1;
/// Lo que se espera a `sesiones` y a archivos, como mucho.
const PLAZO_LANZAR_US: u64 = 5_000_000;
const PLAZO_ARCHIVOS_US: u64 = 1_000_000;
const PLAZO_AVISOS_US: u64 = 1_000_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Boton {
    Lanzador,
    Panel,
    Campana,
    Monitor,
    Espacio(u32),
}

struct Barra {
    tema: Tema,
    ancho: i32,
    titulo: String,
    /// La fecha y la hora que se enseñan (hasta el minuto), si se sabe la hora.
    hora: Option<reloj::Fecha>,
    encima: Option<Boton>,
    lanzador_abierto: bool,
    /// Los avisos sin leer (la campana lleva un punto) y si el historial está abierto.
    sin_leer: u32,
    historial_abierto: bool,
    /// S27: el espacio de trabajo que se ve y los que tienen ventanas (bit n-1 por el espacio n), de `espacios`.
    espacio: u32,
    ocupados: u32,
    /// S27, oleada 2: el estado que da el panel por el compositor: red (0 sin servicio, 1 sin dirección, 2 conectada),
    /// nivel del volumen y silencio. `None` hasta que llega.
    estado: Option<(u32, u32, bool)>,
    /// Trasfondo: la CPU y la memoria en uso (décimas), si ya se midieron, y si el desplegable está abierto.
    uso: (Option<u32>, Option<u32>),
    procesos_abierto: bool,
    /// Los Material Symbols de la barra (kit::iconos), con su caché.
    simbolos: Simbolos,
    /// S10: los menús del programa activo, si van en la barra.
    mb: barra_menus::MenusBarra,
}

/// Dónde va cada cosa de la barra (se calcula al pintar y al mirar dónde cae el ratón).
struct Disposicion {
    lanzador: Rect,
    espacios: Rect,
    /// Cada espacio: su zona de clic, su marca y su número.
    celdas: Vec<(Rect, Rect, u32)>,
    titulo: Option<(Rect, String)>,
    reloj: Rect,
    hora: String,
    fecha: String,
    estado: Rect,
    campana: Rect,
    monitor: Rect,
    textos_monitor: (String, String),
    /// S10: dónde van los títulos de los menús del programa activo (tras su título), si van en la barra.
    menus: Option<Rect>,
}

/// Cuántos espacios se enseñan: hasta el más alto con ventanas o el que se ve, y al menos `MIN_ESPACIOS`.
fn cuantos_espacios(espacio: u32, ocupados: u32) -> u32 {
    let alto = 32 - (ocupados & 0x1ff).leading_zeros();
    alto.max(espacio).max(MIN_ESPACIOS).min(9)
}

fn disponer(b: &Barra, fuente: &mut Fuente, medio: &mut Fuente) -> Disposicion {
    let w = b.ancho;
    let pildora = |x: i32, ancho: i32| Rect::new(x, PILDORA_Y, ancho, PILDORA_ALTO);
    let lanzador = pildora(LADO, 40);
    // Los espacios: el que se ve, una píldora alargada (24 × 8); los demás, puntos de 8. Cada uno con su zona de clic.
    let n = cuantos_espacios(b.espacio, b.ocupados);
    let mut x = lanzador.x + lanzador.w + HUECO + 10;
    let mut celdas = Vec::new();
    for k in 1..=n {
        let ancho = if k == b.espacio { 24 } else { 8 };
        let marca = Rect::new(x, PILDORA_Y + (PILDORA_ALTO - 8) / 2, ancho, 8);
        celdas.push((Rect::new(x - 3, PILDORA_Y, ancho + 6, PILDORA_ALTO), marca, k));
        x += ancho + 6;
    }
    let espacios = pildora(lanzador.x + lanzador.w + HUECO, x - 6 + 10 - (lanzador.x + lanzador.w + HUECO));
    // A la derecha: el estado (red y sonido, abre el panel rápido) y, a su izquierda, la campana.
    let estado = pildora(w - LADO - 60, 60);
    let campana = pildora(estado.x - HUECO - 36, 36);
    // Trasfondo: la CPU y la memoria, a la izquierda de la campana.
    let txt = |d: Option<u32>| d.map_or(String::from("–"), |d| alloc::format!("{} %", (d + 5) / 10));
    let textos_monitor = (txt(b.uso.0), txt(b.uso.1));
    let ancho_monitor = 12 + 16 + 4 + fuente.medir("100 %", 13).ancho + 10 + 16 + 4 + fuente.medir("100 %", 13).ancho + 12;
    let monitor = pildora(campana.x - HUECO - ancho_monitor, ancho_monitor);
    // En medio, el reloj: "hh:mm · vie 13 nov".
    let (hora, fecha) = match b.hora {
        Some(f) => (alloc::format!("{:02}:{:02}", f.hora, f.minuto), alloc::format!("{} {} {}", reloj::DIAS[f.semana as usize], f.dia, reloj::MESES[f.mes as usize - 1])),
        None => (String::from("--:--"), String::new()),
    };
    let ancho_reloj = medio.medir(&hora, 14).ancho + if fecha.is_empty() { 0 } else { 20 + fuente.medir(&fecha, 14).ancho } + 28;
    let reloj = pildora((w - ancho_reloj) / 2, ancho_reloj);
    // El título de la ventana con el teclado, tras los espacios, sin llegar al reloj (ni a la campana, si el reloj no
    // cabe).
    let x_titulo = espacios.x + espacios.w + HUECO;
    let tope = if reloj.x > x_titulo + 80 { reloj.x - HUECO } else { monitor.x - HUECO };
    // S10: con los menús en la barra, el título más corto, y los menús detrás.
    let con_menus = b.mb.en_barra();
    let titulo = (!b.titulo.is_empty() && tope - x_titulo > 48).then(|| {
        let texto = que_quepa(fuente, &b.titulo, 14, (tope - x_titulo - 28).min(if con_menus { 160 } else { 420 }));
        let ancho = fuente.medir(&texto, 14).ancho + 28;
        (pildora(x_titulo, ancho), texto)
    });
    let x_menus = titulo.as_ref().map_or(x_titulo, |t| t.0.x + t.0.w + HUECO);
    // Si los títulos no caben antes del reloj, el reloj va detrás de ellos; si tampoco cabe ahí, no se ve.
    let mut reloj = reloj;
    let menus = if con_menus {
        let fin = x_menus + b.mb.ancho_fila(medio);
        if fin + HUECO > reloj.x {
            if fin + HUECO + reloj.w <= monitor.x - HUECO {
                reloj.x = fin + HUECO;
            } else {
                reloj.w = 0;
            }
        }
        let tope = if reloj.w > 0 { reloj.x - HUECO } else { monitor.x - HUECO };
        (tope > x_menus).then(|| Rect::new(x_menus, TIRA_Y, tope - x_menus, TIRA_ALTO))
    } else {
        None
    };
    Disposicion { lanzador, espacios, celdas, titulo, reloj, hora, fecha, estado, campana, monitor, textos_monitor, menus }
}

impl Disposicion {
    /// El botón que queda en (x, y), si hay alguno.
    fn boton_en(&self, x: i32, y: i32) -> Option<Boton> {
        if self.lanzador.contiene(x, y) {
            return Some(Boton::Lanzador);
        }
        if self.estado.contiene(x, y) {
            return Some(Boton::Panel);
        }
        if self.campana.contiene(x, y) {
            return Some(Boton::Campana);
        }
        if self.monitor.contiene(x, y) {
            return Some(Boton::Monitor);
        }
        self.celdas.iter().find(|c| c.0.contiene(x, y)).map(|c| Boton::Espacio(c.2))
    }
}

/// El lanzador abierto: la lista (sin dibujo, `lanzador_lista`) y lo del dibujo.
struct Lanzador {
    lista: Lista,
    encima: Option<usize>,
    /// Si el listado de `Programas/` falló: el porqué.
    fallo: Option<&'static str>,
    /// Lo que mide de alto (lo fija al abrirse).
    alto: i32,
    /// La memoria de su búfer, mientras está abierto.
    hoja: Option<Lamina>,
    simbolos: Simbolos,
}

/// La altura del lanzador con `n` programas: hasta `FILAS` filas, y al menos una (para decir que no hay). Se fija al
/// abrirlo: al buscar no cambia.
fn alto_lanzador(n: usize) -> i32 {
    Y_FILAS + n.clamp(1, FILAS) as i32 * ALTO_FILA + 8 + PIE_L
}

/// La fila `k` de las que se ven del lanzador.
fn fila(k: usize) -> Rect {
    Rect::new(8, Y_FILAS + k as i32 * ALTO_FILA, ANCHO_L - 16, ALTO_FILA)
}

/// Un texto que cabe en `ancho`: si no, cortado con puntos suspensivos.
fn que_quepa(f: &mut Fuente, texto: &str, tam: u32, ancho: i32) -> String {
    if f.medir(texto, tam).ancho <= ancho {
        return String::from(texto);
    }
    let puntos = if f.tiene('…') { "…" } else { "..." };
    let mut t: String = String::from(texto);
    while !t.is_empty() {
        t.pop();
        let prueba = alloc::format!("{}{}", t.trim_end(), puntos);
        if f.medir(&prueba, tam).ancho <= ancho {
            return prueba;
        }
    }
    String::new()
}

fn centrado(f: &mut Fuente, l: &mut Lienzo, r: Rect, base: i32, texto: &str, tam: u32, color: Color) {
    let ancho = f.medir(texto, tam).ancho;
    f.dibujar(l, r.x + (r.w - ancho) / 2, base, texto, tam, color);
}

/// Un círculo lleno de diámetro `d` con el centro en (cx, cy) (en píxeles; los de diámetro par, entre dos píxeles).
fn punto(l: &mut Lienzo, cx2: i32, cy2: i32, d: i32, color: Color) {
    // cx2 y cy2 van en medios píxeles: así un punto de 4 queda centrado en una rejilla par.
    l.redondeado(Rect::new((cx2 - d) / 2, (cy2 - d) / 2, d, d), d / 2, color);
}

/// El símbolo del altavoz para el nivel (Material: "volume_up" por encima del 50 %, "volume_down" hasta él,
/// "volume_mute" en 0) y el silencio ("volume_off").
fn simbolo_sonido(nivel: u32, silencio: bool) -> kit::iconos::Simbolo {
    match (silencio, nivel) {
        (true, _) => simbolo::VOLUME_OFF,
        (false, 0) => simbolo::VOLUME_MUTE,
        (false, 1..=50) => simbolo::VOLUME_DOWN,
        _ => simbolo::VOLUME_UP,
    }
}

/// La barra, al estilo de DMS con Material 3: la tira en el contenedor de superficie, algo translúcida, y en ella las
/// píldoras en el contenedor alto. El lanzador y el estado llevan el icono en el primario (el acento); abiertos, la
/// píldora es del primario. Encima del ratón, la capa de estado (8 %).
fn pintar_barra(px: &mut [u32], b: &Barra, fuente: &mut Fuente, medio: &mut Fuente) {
    let t = &b.tema;
    let w = b.ancho;
    let d = disponer(b, fuente, medio);
    let mut l = Lienzo::new(px, w, ALTO, w as usize);
    l.borrar(Rect::new(0, 0, w, ALTO), 0);
    l.redondeado(Rect::new(TIRA_X, TIRA_Y, w - 2 * TIRA_X, TIRA_ALTO), TIRA_ALTO / 2, con_opacidad(t.contenedor(), OPACIDAD_TIRA));
    let pildora = |l: &mut Lienzo, r: Rect, abierto: bool, boton: Option<Boton>| -> Color {
        let (fondo, tinta) = if abierto { (t.primario, t.sobre_primario) } else { (t.contenedor_alto(), t.sobre_superficie) };
        l.redondeado(r, r.h / 2, fondo);
        if boton.is_some() && b.encima == boton {
            l.redondeado(r, r.h / 2, con_opacidad(tinta, 20));
        }
        tinta
    };
    // El lanzador.
    let tinta = pildora(&mut l, d.lanzador, b.lanzador_abierto, Some(Boton::Lanzador));
    b.simbolos.centrado(&mut l, d.lanzador, 20, simbolo::APPS, if b.lanzador_abierto { tinta } else { t.primario });
    // Los espacios: el que se ve en el primario; los que tienen ventanas, en el color del texto; los vacíos, en el del
    // contorno. Encima de uno, su capa de estado.
    pildora(&mut l, d.espacios, false, None);
    for &(zona, marca, k) in &d.celdas {
        if b.encima == Some(Boton::Espacio(k)) {
            l.redondeado(zona.dentro(1), (zona.h - 2) / 2, con_opacidad(t.sobre_superficie, 20));
        }
        let color = if k == b.espacio {
            t.primario
        } else if b.ocupados & (1 << (k - 1)) != 0 {
            t.sobre_superficie
        } else {
            t.contorno
        };
        l.redondeado(marca, 4, color);
    }
    // El título.
    if let Some((r, texto)) = &d.titulo {
        pildora(&mut l, *r, false, None);
        fuente.dibujar(&mut l, r.x + 14, r.y + 17, texto, 14, t.sobre_superficie);
    }
    // S10: los menús del programa activo, si van en la barra.
    if let Some(fila) = d.menus {
        b.mb.pintar_fila(&mut l, t, fuente, medio, fila, w);
    }
    // El reloj: la hora en medium y la fecha, tras un punto, en el color suave. (Sin sitio, con menús en la barra, no.)
    if d.reloj.w > 0 {
        pildora(&mut l, d.reloj, false, None);
        let x = d.reloj.x + 14;
        medio.dibujar(&mut l, x, d.reloj.y + 17, &d.hora, 14, t.sobre_superficie);
        if !d.fecha.is_empty() {
            let x = x + medio.medir(&d.hora, 14).ancho + 8;
            punto(&mut l, 2 * x + 4, 2 * (d.reloj.y + d.reloj.h / 2), 4, t.sobre_variante_superficie);
            fuente.dibujar(&mut l, x + 12, d.reloj.y + 17, &d.fecha, 14, t.sobre_variante_superficie);
        }
    }
    // La campana, con el punto de error si hay avisos sin leer.
    let tinta = pildora(&mut l, d.campana, b.historial_abierto, Some(Boton::Campana));
    b.simbolos.centrado(&mut l, d.campana, 20, simbolo::NOTIFICATIONS, tinta);
    if b.sin_leer > 0 {
        punto(&mut l, 2 * (d.campana.x + d.campana.w - 11), 2 * (d.campana.y + 6), 6, t.error);
    }
    // Trasfondo: la CPU (un chip) y la memoria (una tarjeta), cada una con su uso; abre el desplegable de los procesos.
    let tinta = pildora(&mut l, d.monitor, b.procesos_abierto, Some(Boton::Monitor));
    let color_icono = if b.procesos_abierto { tinta } else { t.primario };
    let mut x = d.monitor.x + 12;
    let cy = d.monitor.y + d.monitor.h / 2;
    b.simbolos.pintar(&mut l, x - 1, cy - 9, 18, simbolo::MEMORY, color_icono);
    x += 20;
    fuente.dibujar(&mut l, x, d.monitor.y + 17, &d.textos_monitor.0, 13, tinta);
    x += fuente.medir("100 %", 13).ancho + 10;
    b.simbolos.pintar(&mut l, x - 1, cy - 9, 18, simbolo::MEMORY_ALT, color_icono);
    fuente.dibujar(&mut l, x + 20, d.monitor.y + 17, &d.textos_monitor.1, 13, tinta);
    // El estado: red y sonido, en el primario (como DMS); abre el panel rápido.
    pildora(&mut l, d.estado, false, Some(Boton::Panel));
    // Con el estado del panel (oleada 2): sin red, el icono en el color suave y tachado; con el silencio, la cruz.
    let (red, nivel, silencio) = b.estado.unwrap_or((2, 100, false));
    // De 18: los de Material dejan 2 de margen de cada 24 (a 16 se quedaban pequeños al lado del texto).
    let r_red = Rect::new(d.estado.x + 9, d.estado.y + 3, 18, 18);
    if red == 2 {
        b.simbolos.pintar(&mut l, r_red.x, r_red.y, 18, simbolo::LAN, t.primario);
    } else {
        b.simbolos.pintar(&mut l, r_red.x, r_red.y, 18, simbolo::LAN, t.sobre_variante_superficie);
        l.trazo(((r_red.x + 2) * 8, (r_red.y + 2) * 8), ((r_red.x + 16) * 8, (r_red.y + 16) * 8), 12, t.sobre_variante_superficie);
    }
    let color = if silencio { t.sobre_variante_superficie } else { t.primario };
    b.simbolos.pintar(&mut l, d.estado.x + 33, d.estado.y + 3, 18, simbolo_sonido(nivel, silencio), color);
}

/// La hora de `ns` en la zona horaria (S13), como "hh:mm"; sin hora, vacío.
fn hora_de(ns: u64) -> String {
    if ns == 0 {
        return String::new();
    }
    let min = zona::local_ns(zona_de_ahora(), ns) / 60_000_000_000 % 1440;
    alloc::format!("{:02}:{:02}", min / 60, min % 60)
}

/// Un aviso en una fila o en el toast: el círculo con la inicial de quien avisa (urgente: en el color de error), "de ·
/// hora" en pequeño, el título y el texto, cada uno en una línea.
fn pintar_aviso(l: &mut Lienzo, r: Rect, a: &av::Aviso, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente) {
    let avatar = Rect::new(r.x + 16, r.y + (r.h - 40) / 2, 40, 40);
    let (fondo, tinta) = if a.urgencia == av::URGENTE { (t.error, 0xffff_ffff) } else { (t.contenedor_primario, t.sobre_contenedor_primario) };
    l.redondeado(avatar, 20, fondo);
    let inicial: String = a.de().chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    centrado(medio, l, avatar, avatar.y + 26, &inicial, 16, tinta);
    let x = avatar.x + avatar.w + 16;
    let ancho = r.x + r.w - 16 - x;
    let hora = hora_de(a.hora);
    // Lo lanzado del disco lleva su nombre de fichero; lo del sistema, el que le dio `inicio`.
    let de = if a.sistema { String::from(a.de()) } else { alloc::format!("{} (programa)", a.de()) };
    let cabeza = if hora.is_empty() { de } else { alloc::format!("{} · {}", de, hora) };
    let y = r.y + (r.h - 60) / 2;
    let cabeza = que_quepa(fuente, &cabeza, 12, ancho);
    fuente.dibujar(l, x, y + 13, &cabeza, 12, t.sobre_variante_superficie);
    let titulo = que_quepa(medio, a.titulo(), 14, ancho);
    medio.dibujar(l, x, y + 34, &titulo, 14, t.sobre_superficie);
    if a.lx > 0 {
        let texto = que_quepa(fuente, a.texto(), 14, ancho);
        fuente.dibujar(l, x, y + 54, &texto, 14, t.sobre_variante_superficie);
    }
}

/// El toast: una tarjeta en el contenedor alto, con su sombra (elevación 3), dentro de un búfer con margen.
fn pintar_toast(px: &mut [u32], a: &av::Aviso, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente) {
    px.fill(0);
    let mut l = Lienzo::new(px, ANCHO_A, ALTO_A, ANCHO_A as usize);
    let tarjeta = tarjeta_toast();
    l.sombra(tarjeta, 16, 3, 0, 8, 60);
    l.redondeado(tarjeta, 16, t.contenedor_alto());
    pintar_aviso(&mut l, tarjeta, a, t, fuente, medio);
}

fn tarjeta_toast() -> Rect {
    Rect::new(8, 8, ANCHO_A - 16, ALTO_A - 20)
}

/// Lo que mide el historial con `n` avisos.
fn alto_historial(n: usize) -> i32 {
    Y_FILAS_H + n.clamp(1, FILAS_H) as i32 * ALTO_FILA_H + PIE_H
}

fn fila_h(k: usize) -> Rect {
    Rect::new(8, Y_FILAS_H + k as i32 * ALTO_FILA_H, ANCHO_H - 16, ALTO_FILA_H)
}

/// El botón de texto "Borrar todo", arriba a la derecha del historial.
fn zona_borrar(medio: &mut Fuente) -> Rect {
    let w = medio.medir("Borrar todo", 14).ancho + 24;
    Rect::new(ANCHO_H - 16 - w, 12, w, 40)
}

/// El historial, como el panel rápido: esquinas de 28 en el contenedor, "Avisos" y "Borrar todo" arriba y los avisos del
/// más nuevo al más viejo. Los que no se habían leído al abrirlo llevan un punto del primario.
fn pintar_historial(px: &mut [u32], h: &Historial, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente) {
    px.fill(0);
    let mut l = Lienzo::new(px, ANCHO_H, h.alto, ANCHO_H as usize);
    l.redondeado(Rect::new(0, 0, ANCHO_H, h.alto), 28, t.contenedor());
    fuente.dibujar(&mut l, 24, 40, "Avisos", 22, t.sobre_superficie);
    let rb = zona_borrar(medio);
    let hay = !h.avisos.is_empty();
    if hay && h.encima_borrar {
        l.redondeado(rb, rb.h / 2, con_opacidad(t.primario, 20));
    }
    let color = if hay { t.primario } else { con_opacidad(t.sobre_superficie, 97) };
    centrado(medio, &mut l, rb, rb.y + 25, "Borrar todo", 14, color);
    for (k, a) in h.avisos.iter().enumerate().skip(h.primero).take(FILAS_H) {
        // Cada fila, con 4 de separación (las sin leer llevan fondo, y no deben tocarse).
        let r = fila_h(k - h.primero).dentro(2);
        if !a.leido {
            l.redondeado(r, 16, t.contenedor_alto());
            punto(&mut l, 2 * (r.x + r.w - 14), 2 * (r.y + r.h / 2), 8, t.primario);
        }
        pintar_aviso(&mut l, Rect::new(r.x, r.y, r.w - 16, r.h), a, t, fuente, medio);
    }
    if !hay {
        centrado(fuente, &mut l, Rect::new(0, Y_FILAS_H, ANCHO_H, ALTO_FILA_H), Y_FILAS_H + 42, "No hay avisos", 14, t.sobre_variante_superficie);
    } else if h.avisos.len() > FILAS_H {
        let raya = if fuente.tiene('–') { "–" } else { "-" };
        let pie = alloc::format!("{}{}{} de {} · la rueda mueve la lista", h.primero + 1, raya, (h.primero + FILAS_H).min(h.avisos.len()), h.avisos.len());
        centrado(fuente, &mut l, Rect::new(0, h.alto - PIE_H, ANCHO_H, PIE_H), h.alto - PIE_H + 26, &pie, 12, t.sobre_variante_superficie);
    }
}

/// El historial abierto: los avisos que se ven (del más nuevo al más viejo) y lo que mide.
struct Historial {
    avisos: Vec<av::Aviso>,
    primero: usize,
    alto: i32,
    encima_borrar: bool,
}

/// Una petición al servicio de avisos (con `extra`, llaves que van tras la de la respuesta): estado, sin leer, visibles
/// y el aviso, si trae. `None` si no contesta.
fn pedir_avisos(c: &mut Cliente, m: &[u8], extra: &[u32]) -> Option<(i32, u32, u32, Option<av::Aviso>)> {
    let (lectura, suyo) = dashos::canal_crear();
    let mut llaves = Vec::with_capacity(1 + extra.len());
    llaves.push(suyo);
    llaves.extend_from_slice(extra);
    let mut r = [0u8; 256];
    let n = if dashos::canal_escribir(AVISOS, m, &llaves) == 0 {
        esperar(c, lectura, &mut r, PLAZO_AVISOS_US)
    } else {
        llaves.iter().for_each(|&l| { dashos::cerrar(l); });
        -1
    };
    dashos::cerrar(lectura);
    if n < av::CABECERA_LEER as i64 {
        return None;
    }
    let u = |i: usize| u32::from_le_bytes(r[i..i + 4].try_into().unwrap());
    let aviso = if n as usize > av::CABECERA_LEER { av::Aviso::leer(&r[av::CABECERA_LEER..n as usize]) } else { None };
    Some((u(0) as i32, u(4), u(8), aviso))
}

fn orden_avisos(orden: u32, arg: u32) -> [u8; 8] {
    let mut m = [0u8; 8];
    m[0..4].copy_from_slice(&orden.to_le_bytes());
    m[4..8].copy_from_slice(&arg.to_le_bytes());
    m
}

/// Todos los avisos que se ven, del más nuevo al más viejo, y cuántos sin leer.
fn leer_historial(c: &mut Cliente) -> (Vec<av::Aviso>, u32) {
    let mut v = Vec::new();
    let mut antes = u32::MAX;
    let mut sin = 0;
    while v.len() < av::MAX_HISTORIAL {
        match pedir_avisos(c, &orden_avisos(av::LEER, antes), &[]) {
            Some((av::BIEN, s, _, Some(a))) if a.numero < antes => {
                sin = s;
                antes = a.numero;
                v.push(a);
            }
            _ => break,
        }
    }
    (v, sin)
}

/// El lanzador, como el de DMS con Material 3: una hoja en el contenedor de superficie con esquinas de 28; arriba la
/// barra de búsqueda (redonda, en el contenedor alto); cada programa en una fila con su icono (la inicial en un
/// cuadrado redondeado del contenedor primario), su nombre y de dónde sale; lo elegido en el contenedor secundario; y
/// abajo, las teclas.
fn pintar_lanzador(px: &mut [u32], la: &Lanzador, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente) {
    px.fill(0);
    let alto = la.alto;
    let lista = &la.lista;
    let mut l = Lienzo::new(px, ANCHO_L, alto, ANCHO_L as usize);
    l.redondeado(Rect::new(0, 0, ANCHO_L, alto), 28, t.contenedor());
    l.redondeado(BUSCADOR, BUSCADOR.h / 2, t.contenedor_alto());
    la.simbolos.pintar(&mut l, BUSCADOR.x + 12, BUSCADOR.y + 16, 24, simbolo::SEARCH, t.sobre_variante_superficie);
    let x = BUSCADOR.x + 52;
    let base = BUSCADOR.y + 34;
    let cursor = if lista.busca.is_empty() {
        fuente.dibujar(&mut l, x + 4, base, "Buscar programas", 16, t.sobre_variante_superficie);
        x
    } else {
        let visto = que_quepa(fuente, &lista.busca, 16, BUSCADOR.w - 72);
        fuente.dibujar(&mut l, x, base, &visto, 16, t.sobre_superficie);
        x + fuente.medir(&visto, 16).ancho + 1
    };
    l.rellenar(Rect::new(cursor, BUSCADOR.y + 16, 2, 24), t.primario);
    for k in 0..lista.filas {
        let Some(v) = lista.en_fila(k) else { break };
        let r = fila(k);
        let elegido = v == lista.elegido;
        if elegido {
            l.redondeado(r, 16, t.contenedor_secundario());
        } else if la.encima == Some(v) {
            l.redondeado(r, 16, con_opacidad(t.sobre_superficie, 20));
        }
        let nombre = &lista.programas[lista.vistos[v]];
        let icono = Rect::new(r.x + 12, r.y + 8, 40, 40);
        l.redondeado(icono, 12, t.contenedor_primario);
        let inicial: String = nombre.chars().find(|c| !c.is_control()).map(|c| c.to_uppercase().collect()).unwrap_or_default();
        centrado(medio, &mut l, icono, icono.y + 27, &inicial, 18, t.sobre_contenedor_primario);
        let ancho = r.w - 68 - if elegido { 80 } else { 16 };
        let visto = que_quepa(medio, nombre, 16, ancho);
        medio.dibujar(&mut l, r.x + 68, r.y + 25, &visto, 16, t.sobre_superficie);
        let de = alloc::format!("Programas/{}", nombre);
        let de = que_quepa(fuente, &de, 12, ancho);
        fuente.dibujar(&mut l, r.x + 68, r.y + 44, &de, 12, t.sobre_variante_superficie);
        if elegido {
            // "Intro" en una etiqueta con contorno.
            let w = medio.medir("Intro", 12).ancho + 16;
            let e = Rect::new(r.x + r.w - 16 - w, r.y + (r.h - 24) / 2, w, 24);
            l.borde(e, 8, 1, t.contorno);
            centrado(medio, &mut l, e, e.y + 16, "Intro", 12, t.sobre_variante_superficie);
        }
    }
    if lista.vistos.is_empty() {
        let texto = match (la.fallo, lista.programas.is_empty()) {
            (Some(_), _) => "No se puede leer Programas",
            (None, true) => "No hay programas",
            (None, false) => "Ningún programa se llama así",
        };
        centrado(fuente, &mut l, Rect::new(0, Y_FILAS, ANCHO_L, ALTO_FILA), Y_FILAS + 33, texto, 14, t.sobre_variante_superficie);
    }
    // El pie: una línea fina y las teclas, y cuántos casan.
    let y = alto - PIE_L;
    l.rellenar(Rect::new(24, y, ANCHO_L - 48, 1), t.variante_contorno);
    fuente.dibujar(&mut l, 24, y + 31, "Flechas: elegir · Intro: abrir · Esc: cerrar", 12, t.sobre_variante_superficie);
    let cuenta = match lista.vistos.len() {
        1 => String::from("1 programa"),
        n => alloc::format!("{} programas", n),
    };
    let w = fuente.medir(&cuenta, 12).ancho;
    fuente.dibujar(&mut l, ANCHO_L - 24 - w, y + 31, &cuenta, 12, t.sobre_variante_superficie);
}

/// Espera la respuesta de otro servicio por `lectura` sin dejar de recoger lo que mande el compositor (que suelta a
/// quien no lee): los eventos quedan guardados para después. Los bytes leídos, o un error.
fn esperar(c: &mut Cliente, lectura: u32, buf: &mut [u8], plazo_us: u64) -> i64 {
    let hasta = dashos::ahora_us() + plazo_us;
    loop {
        let n = dashos::canal_leer_plazo(lectura, buf, &mut [], 20_000).0;
        if n != dashos::E_PLAZO {
            return n;
        }
        while c.recoger() {}
        if dashos::ahora_us() >= hasta {
            return dashos::E_PLAZO;
        }
    }
}

/// Los programas de `Programas/` (la raíz de la barra en el servicio de archivos): los ficheros que se pueden ejecutar
/// y que `sesiones` le dejaría lanzar, por nombre.
fn listar(c: &mut Cliente) -> Result<Vec<String>, &'static str> {
    let (lectura, suyo) = dashos::canal_crear();
    let mut cab = [0u8; a::CABECERA];
    cab[0..4].copy_from_slice(&a::SESION.to_le_bytes());
    let mut r = [0u8; 32];
    let mut ll = [0u32; 1];
    let (n, k, _) = if dashos::canal_escribir(ARCHIVOS, &cab, &[suyo]) == 0 {
        dashos::canal_leer_plazo(lectura, &mut r, &mut ll, PLAZO_ARCHIVOS_US)
    } else {
        (-1, 0, 0)
    };
    if n != 32 || k != 1 || dashos::memoria_proyectar(ll[0], VA_ARCHIVOS, false) != 0 {
        if k == 1 {
            dashos::cerrar(ll[0]);
        }
        dashos::cerrar(lectura);
        return Err("sin servicio de archivos");
    }
    dashos::cerrar(ll[0]);
    let mut programas = Vec::new();
    let mut desde = 0u64;
    let mut resultado = Ok(());
    loop {
        let mut p = [0u8; a::CABECERA];
        p[0..4].copy_from_slice(&a::LISTAR.to_le_bytes());
        p[8..16].copy_from_slice(&desde.to_le_bytes());
        let mut r = [0u8; 32];
        if dashos::canal_escribir(ARCHIVOS, &p, &[]) != 0 || esperar(c, lectura, &mut r, PLAZO_ARCHIVOS_US) != 32 {
            resultado = Err("archivos no contesta");
            break;
        }
        let estado = i32::from_le_bytes(r[0..4].try_into().unwrap());
        let u64_en = |i: usize| u64::from_le_bytes(r[i..i + 8].try_into().unwrap());
        let (cuantos, siguiente, bytes) = (u64_en(8), u64_en(16), u64_en(24));
        if estado != a::BIEN {
            resultado = Err("no se puede listar");
            break;
        }
        let v = unsafe { core::slice::from_raw_parts(VA_ARCHIVOS as *const u8, (bytes as usize).min(a::VENTANA)) };
        let mut off = 0;
        for _ in 0..cuantos {
            if off + 24 > v.len() {
                break;
            }
            let modo = u16::from_le_bytes([v[off + 16], v[off + 17]]);
            let (tipo, largo, largo_com) = (v[off + 18], v[off + 19] as usize, v[off + 22] as usize);
            let total = 24 + largo + largo_com;
            if off + total > v.len() {
                break;
            }
            let nombre = &v[off + 24..off + 24 + largo];
            if tipo == a::FICHERO && modo & 0o111 != 0 && lanzables::nombre_valido(nombre)
                && let Ok(n) = core::str::from_utf8(nombre)
            {
                programas.push(String::from(n));
            }
            off += total.next_multiple_of(8);
        }
        if siguiente == u64::MAX || cuantos == 0 || programas.len() >= MAX_PROGRAMAS {
            break;
        }
        desde = siguiente;
    }
    dashos::memoria_quitar(VA_ARCHIVOS);
    dashos::cerrar(lectura);
    // El Shell nativo no está en `Programas/` sino en `SYS:System` (S4b): va en la lista con su nombre.
    if !programas.iter().any(|p| p == lanzables::NOMBRE_SHELL) {
        programas.push(String::from(lanzables::NOMBRE_SHELL));
    }
    programas.sort_by_key(|p| p.to_lowercase());
    resultado.map(|_| programas)
}

/// Pide a `sesiones` que lance `Programas/<nombre>` (como quien está delante): su estado y el número del proceso.
fn lanzar(c: &mut Cliente, nombre: &str) -> (i32, u64) {
    let camino = if nombre == lanzables::NOMBRE_SHELL {
        String::from(core::str::from_utf8(lanzables::SHELL).unwrap_or("System/Shell"))
    } else {
        alloc::format!("{}{}", core::str::from_utf8(lanzables::CAJON).unwrap_or("Programas/"), nombre)
    };
    let mut p = Vec::with_capacity(12 + camino.len());
    p.extend_from_slice(&s::LANZAR.to_le_bytes());
    p.extend_from_slice(&(camino.len() as u32).to_le_bytes());
    p.extend_from_slice(&0u32.to_le_bytes());
    p.extend_from_slice(camino.as_bytes());
    let (lectura, suyo) = dashos::canal_crear();
    let mut r = [0u8; 12];
    let n = if dashos::canal_escribir(SESIONES, &p, &[suyo]) == 0 { esperar(c, lectura, &mut r, PLAZO_LANZAR_US) } else { -1 };
    dashos::cerrar(lectura);
    if n == 12 { (i32::from_le_bytes(r[0..4].try_into().unwrap()), u64::from_le_bytes(r[4..12].try_into().unwrap())) } else { (i32::MIN, 0) }
}

/// Los avisos en la barra: el toast que se ve (y hasta cuándo), el historial abierto y el último aviso que ya se enseñó o
/// se sabía (los de número mayor son nuevos).
struct Avisos {
    activo: bool,
    toast: Option<(av::Aviso, u64)>,
    toast_listo: bool,
    hist: Option<Historial>,
    hist_listo: bool,
    visto_hasta: u32,
    /// Las memorias del toast y del historial, mientras se ven.
    hoja_a: Option<Lamina>,
    hoja_h: Option<Lamina>,
}

fn pantalla_toast(c: &mut Cliente, hoja: &mut Option<Lamina>, a: &av::Aviso, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente) {
    let Some(hoja) = hoja else { return };
    pintar_toast(hoja.px((ANCHO_A * ALTO_A) as usize), a, t, fuente, medio);
    let mut e = Escritor::new();
    e.empezar(SUP_AVISO, wl_surface::req::ATTACH).uint(BUFER_AVISO).int(0).int(0).fin();
    e.empezar(SUP_AVISO, wl_surface::req::DAMAGE).int(0).int(0).int(ANCHO_A).int(ALTO_A).fin();
    e.empezar(SUP_AVISO, wl_surface::req::COMMIT).fin();
    c.mandar(&e);
}

fn pantalla_historial(c: &mut Cliente, hoja: &mut Option<Lamina>, h: &Historial, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente) {
    let Some(hoja) = hoja else { return };
    pintar_historial(hoja.px((ANCHO_H * h.alto) as usize), h, t, fuente, medio);
    let mut e = Escritor::new();
    e.empezar(SUP_HIST, wl_surface::req::ATTACH).uint(BUFER_HIST).int(0).int(0).fin();
    e.empezar(SUP_HIST, wl_surface::req::DAMAGE).int(0).int(0).int(ANCHO_H).int(h.alto).fin();
    e.empezar(SUP_HIST, wl_surface::req::COMMIT).fin();
    c.mandar(&e);
}

/// Enseña `a` como toast: crea su capa (arriba a la derecha, bajo la barra, sin teclado) o, si ya hay uno, lo cambia.
fn abrir_toast(c: &mut Cliente, avs: &mut Avisos, a: av::Aviso, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente) {
    let hasta = dashos::ahora_us() + DURA_AVISO_US;
    if avs.toast.is_some() {
        avs.toast = Some((a, hasta));
        if avs.toast_listo {
            pantalla_toast(c, &mut avs.hoja_a, &a, t, fuente, medio);
        }
        return;
    }
    let mut e = Escritor::new();
    let Some(hoja) = Lamina::nueva(&mut e, (ANCHO_A * ALTO_A * 4) as usize, VA_A, POOL_A) else {
        println!("  [barra] sin memoria para el aviso");
        return;
    };
    avs.hoja_a = Some(hoja);
    avs.toast = Some((a, hasta));
    avs.toast_listo = false;
    e.empezar(POOL_A, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER_AVISO).int(0).int(ANCHO_A).int(ALTO_A).int(ANCHO_A * 4).uint(0).fin();
    e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUP_AVISO).fin();
    e.empezar(CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(CAPA_AVISO).uint(SUP_AVISO).uint(0).uint(2).texto(Some("aviso")).fin();
    e.empezar(CAPA_AVISO, zwlr_layer_surface_v1::req::SET_SIZE).uint(ANCHO_A as u32).uint(ALTO_A as u32).fin();
    e.empezar(CAPA_AVISO, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(1 | 8).fin();
    e.empezar(CAPA_AVISO, zwlr_layer_surface_v1::req::SET_KEYBOARD_INTERACTIVITY).uint(0).fin();
    e.empezar(SUP_AVISO, wl_surface::req::COMMIT).fin();
    c.mandar(&e);
}

fn cerrar_toast(c: &mut Cliente, avs: &mut Avisos, porque: &str) {
    if avs.toast.take().is_none() {
        return;
    }
    let mut e = Escritor::new();
    e.empezar(CAPA_AVISO, zwlr_layer_surface_v1::req::DESTROY).fin();
    e.empezar(SUP_AVISO, wl_surface::req::DESTROY).fin();
    e.empezar(BUFER_AVISO, wl_buffer::req::DESTROY).fin();
    if let Some(h) = avs.hoja_a.take() {
        h.soltar(&mut e);
    }
    c.mandar(&e);
    avs.toast_listo = false;
    println!("  [barra] aviso cerrado: {}", porque);
}

/// Abre el historial: lee los avisos que se ven y los marca como leídos en el servicio (en la lista, el punto de los que
/// no se habían leído sigue hasta cerrarla).
fn abrir_historial(c: &mut Cliente, b: &mut Barra, avs: &mut Avisos, medio: &mut Fuente) {
    let (avisos, _) = leer_historial(c);
    let alto = alto_historial(avisos.len());
    if let Some((_, sin, _, _)) = pedir_avisos(c, &orden_avisos(av::LEIDOS, 0), &[]) {
        b.sin_leer = sin;
    }
    if let Some(a) = avisos.first() {
        avs.visto_hasta = avs.visto_hasta.max(a.numero);
    }
    let sin = avisos.iter().filter(|a| !a.leido).count();
    let mut quienes: Vec<&str> = Vec::new();
    for a in &avisos {
        if !quienes.contains(&a.de()) {
            quienes.push(a.de());
        }
    }
    let rb = zona_borrar(medio);
    println!("  [barra] historial abierto: {} avisos, {} sin leer; de {}; Borrar todo en ({}, {})", avisos.len(), sin,
        if quienes.is_empty() { String::from("nadie") } else { quienes.join(", ") }, rb.x + rb.w / 2, rb.y + rb.h / 2);
    // Su memoria, para lo más alto que se hace: al borrar todo, se queda más bajo en la misma.
    let mut e = Escritor::new();
    let Some(hoja) = Lamina::nueva(&mut e, (ANCHO_H * ALTO_H_MAX * 4) as usize, VA_H, POOL_H) else {
        println!("  [barra] sin memoria para el historial");
        return;
    };
    avs.hoja_h = Some(hoja);
    avs.hist = Some(Historial { avisos, primero: 0, alto, encima_borrar: false });
    avs.hist_listo = false;
    b.historial_abierto = true;
    // Arriba a la derecha, bajo su botón (sin zona propia: el compositor la pone bajo la barra), sin teclado.
    e.empezar(POOL_H, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER_HIST).int(0).int(ANCHO_H).int(alto).int(ANCHO_H * 4).uint(0).fin();
    e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUP_HIST).fin();
    e.empezar(CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(CAPA_HIST).uint(SUP_HIST).uint(0).uint(2).texto(Some("avisos")).fin();
    e.empezar(CAPA_HIST, zwlr_layer_surface_v1::req::SET_SIZE).uint(ANCHO_H as u32).uint(alto as u32).fin();
    e.empezar(CAPA_HIST, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(1 | 8).fin();
    e.empezar(CAPA_HIST, zwlr_layer_surface_v1::req::SET_MARGIN).int(LADO).int(LADO).int(0).int(0).fin();
    e.empezar(CAPA_HIST, zwlr_layer_surface_v1::req::SET_KEYBOARD_INTERACTIVITY).uint(0).fin();
    e.empezar(SUP_HIST, wl_surface::req::COMMIT).fin();
    c.mandar(&e);
}

fn cerrar_historial(c: &mut Cliente, b: &mut Barra, avs: &mut Avisos) {
    if avs.hist.take().is_none() {
        return;
    }
    let mut e = Escritor::new();
    e.empezar(CAPA_HIST, zwlr_layer_surface_v1::req::DESTROY).fin();
    e.empezar(SUP_HIST, wl_surface::req::DESTROY).fin();
    e.empezar(BUFER_HIST, wl_buffer::req::DESTROY).fin();
    if let Some(h) = avs.hoja_h.take() {
        h.soltar(&mut e);
    }
    c.mandar(&e);
    avs.hist_listo = false;
    b.historial_abierto = false;
    println!("  [barra] historial cerrado");
}

/// El servicio tocó el timbre: lo nuevo. Con el historial abierto, se relee; si no, el más nuevo sale como toast si no se
/// había visto, no se ha leído y no llega retenido (de una sesión que estaba detrás).
fn al_timbre(c: &mut Cliente, b: &mut Barra, avs: &mut Avisos, fuente: &mut Fuente, medio: &mut Fuente) {
    let Some((_, sin, _, nuevo)) = pedir_avisos(c, &orden_avisos(av::LEER, u32::MAX), &[]) else { return };
    if (sin > 0) != (b.sin_leer > 0) {
        println!("  [barra] campana: {}", if sin > 0 { alloc::format!("con punto ({} sin leer)", sin) } else { String::from("sin punto") });
    }
    b.sin_leer = sin;
    if avs.hist.is_some() {
        let (avisos, _) = leer_historial(c);
        if let Some(h) = avs.hist.as_mut() {
            h.primero = h.primero.min(avisos.len().saturating_sub(1));
            h.avisos = avisos;
        }
        if let Some((_, sin, _, _)) = pedir_avisos(c, &orden_avisos(av::LEIDOS, 0), &[]) {
            b.sin_leer = sin;
        }
        if avs.hist_listo && let Some(h) = &avs.hist {
            pantalla_historial(c, &mut avs.hoja_h, h, &b.tema, fuente, medio);
        }
    } else if let Some(a) = nuevo.filter(|a| a.numero > avs.visto_hasta && !a.leido && !a.retenido) {
        println!("  [barra] aviso de {}: \"{}\" (urgencia {}); campana {}", a.de(), a.titulo(), a.urgencia,
            if sin > 0 { alloc::format!("con punto ({} sin leer)", sin) } else { String::from("sin punto") });
        abrir_toast(c, avs, a, &b.tema, fuente, medio);
    }
    if let Some(a) = nuevo {
        avs.visto_hasta = avs.visto_hasta.max(a.numero);
    }
}

/// Trasfondo: el desplegable de los procesos, mientras se ve.
struct Procesos {
    hoja: Option<Lamina>,
    listo: bool,
    alto: i32,
    sin_datos: bool,
    encima_abrir: bool,
}

/// La CPU de la máquina y la memoria en uso, en décimas (llamada 29: nada de otros procesos).
fn medir_uso(m: &mut monitor::Medidor) -> (Option<u32>, Option<u32>) {
    let presentes = dashos::medir(14, 0).max(0) as u64;
    let ociosos: [Option<u64>; monitor::MAX_NUCLEOS] = core::array::from_fn(|n| {
        let v = dashos::medir(3, n as u64);
        (presentes & (1 << n) != 0 && v >= 0).then_some(v as u64)
    });
    m.nucleos(presentes, &ociosos, dashos::ahora_us());
    m.memoria(dashos::medir(5, 0).max(0) as u64, dashos::medir(6, 0).max(0) as u64);
    (m.total.ultimo(), m.memoria_hist.ultimo())
}

/// Los procesos que deja ver `sesiones` (`MONITOR`, todas las páginas). `false` si no contesta.
fn medir_procesos(m: &mut monitor::Medidor) -> bool {
    let mut entradas = Vec::new();
    let mut total = usize::MAX;
    while entradas.len() < total && entradas.len() < 256 {
        let mut p = [0u8; 8];
        p[0..4].copy_from_slice(&s::MONITOR.to_le_bytes());
        p[4..8].copy_from_slice(&(entradas.len() as u32).to_le_bytes());
        let (lectura, suyo) = dashos::canal_crear();
        let mut r = [0u8; 256];
        let n = if dashos::canal_escribir(SESIONES, &p, &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], 1_000_000).0 } else { -1 };
        dashos::cerrar(lectura);
        match (n > 0).then(|| monitor::leer_pagina(&r[..n as usize])).flatten() {
            Some((t, v)) if !v.is_empty() => {
                total = t;
                entradas.extend(v);
            }
            Some(_) => break,
            None => return false,
        }
    }
    m.procesos(entradas, dashos::ahora_us());
    true
}

fn alto_procesos(n: usize) -> i32 {
    Y_FILAS_P + n.clamp(1, FILAS_P) as i32 * ALTO_FILA_P + 12 + PIE_P
}

/// El botón del pie del desplegable: abre el monitor del sistema en Ajustes.
fn boton_abrir(alto: i32) -> Rect {
    Rect::new(ANCHO_P - 16 - 236, alto - PIE_P + 6, 236, 40)
}
const ABRIR_MONITOR: &str = "Abrir el monitor del sistema";

/// La pastilla de una caja, como la de la barra de sus ventanas: "68K" o "LINUX" en su color. Devuelve su ancho.
fn pastilla_caja(l: &mut Lienzo, f: &mut Fuente, x: i32, cy: i32, clase: u32) -> i32 {
    use wayland::escena::marca as mc;
    let (texto, fondo, tinta) = match clase {
        monitor::M_CAJA_AMIGA => (mc::TEXTO_AMIGA, mc::COLOR_AMIGA, mc::TINTA_AMIGA),
        monitor::M_CAJA_LINUX => (mc::TEXTO_LINUX, mc::COLOR_LINUX, mc::TINTA),
        _ => return 0,
    };
    let w = f.medir(texto, 10).ancho + 12;
    l.redondeado(Rect::new(x, cy - 8, w, 16), 8, fondo);
    f.dibujar(l, x + 6, cy + 4, texto, 10, tinta);
    w
}

fn pintar_procesos(px: &mut [u32], p: &Procesos, m: &monitor::Medidor, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente) {
    let mut l = Lienzo::new(px, ANCHO_P, p.alto, ANCHO_P as usize);
    l.borrar(Rect::new(0, 0, ANCHO_P, p.alto), 0);
    l.redondeado(Rect::new(0, 0, ANCHO_P, p.alto), kit::tema::RADIO_DIALOGO, t.contenedor());
    medio.dibujar(&mut l, 20, 30, "Procesos", 16, t.sobre_superficie);
    let total = m.total.ultimo().map_or(String::new(), |d| alloc::format!("CPU {}", monitor::por_ciento(d)));
    let w = fuente.medir(&total, 13).ancho;
    fuente.dibujar(&mut l, ANCHO_P - 20 - w, 30, &total, 13, t.sobre_variante_superficie);
    fuente.dibujar(&mut l, 20, 50, "Los tuyos; el sistema y otros usuarios, sumados", 12, t.sobre_variante_superficie);
    let (col_cpu, col_mem) = (ANCHO_P - 20 - 84, ANCHO_P - 20);
    let derecha = |f: &mut Fuente, l: &mut Lienzo, x: i32, y: i32, s: &str, tam: u32, c: Color| {
        let w = f.medir(s, tam).ancho;
        f.dibujar(l, x - w, y, s, tam, c);
    };
    derecha(medio, &mut l, col_cpu, 70, "CPU", 11, t.sobre_variante_superficie);
    derecha(medio, &mut l, col_mem, 70, "Memoria", 11, t.sobre_variante_superficie);
    let filas = m.los_que_mas(FILAS_P);
    if filas.is_empty() {
        fuente.dibujar(&mut l, 20, Y_FILAS_P + 24, if p.sin_datos { "sesiones no contesta" } else { "Nada que enseñar" }, 13, t.sobre_variante_superficie);
    }
    for (k, v) in filas.iter().enumerate() {
        let y = Y_FILAS_P + k as i32 * ALTO_FILA_P;
        if k % 2 == 1 {
            l.redondeado(Rect::new(8, y, ANCHO_P - 16, ALTO_FILA_P), 8, t.contenedor_bajo());
        }
        let cy = y + ALTO_FILA_P / 2;
        let mut x = 20;
        let pw = pastilla_caja(&mut l, medio, x, cy, v.e.clase);
        if pw > 0 {
            x += pw + 8;
        }
        let suma = matches!(v.e.clase, monitor::M_SISTEMA | monitor::M_OTROS);
        let nombre = que_quepa(fuente, &v.e.titulo(), 14, col_cpu - 70 - x);
        fuente.dibujar(&mut l, x, cy + 5, &nombre, 14, if suma { t.sobre_variante_superficie } else { t.sobre_superficie });
        derecha(fuente, &mut l, col_cpu, cy + 5, &monitor::por_ciento(v.cpu), 13, t.sobre_superficie);
        derecha(fuente, &mut l, col_mem, cy + 5, &monitor::bytes(v.e.memoria), 13, t.sobre_variante_superficie);
    }
    // El pie: un botón de texto de Material 3 (el primario, sin contenedor) que abre el monitor del sistema.
    let b = boton_abrir(p.alto);
    l.rellenar(Rect::new(16, b.y - 6, ANCHO_P - 32, 1), t.variante_contorno);
    if p.encima_abrir {
        l.redondeado(b, b.h / 2, con_opacidad(t.primario, 20));
    }
    let w = medio.medir(ABRIR_MONITOR, 14).ancho;
    medio.dibujar(&mut l, b.x + (b.w - w) / 2, b.y + 25, ABRIR_MONITOR, 14, t.primario);
}

fn pantalla_procesos(c: &mut Cliente, p: &mut Procesos, m: &monitor::Medidor, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente) {
    let alto = p.alto;
    let Some(hoja) = p.hoja.as_mut() else { return };
    let px = hoja.px((ANCHO_P * alto) as usize);
    let p2 = Procesos { hoja: None, listo: p.listo, alto, sin_datos: p.sin_datos, encima_abrir: p.encima_abrir };
    pintar_procesos(px, &p2, m, t, fuente, medio);
    let mut e = Escritor::new();
    e.empezar(SUP_PROC, wl_surface::req::ATTACH).uint(BUFER_PROC).int(0).int(0).fin();
    e.empezar(SUP_PROC, wl_surface::req::DAMAGE).int(0).int(0).int(ANCHO_P).int(alto).fin();
    e.empezar(SUP_PROC, wl_surface::req::COMMIT).fin();
    c.mandar(&e);
}

/// Abre el desplegable bajo la píldora (`r`): mide dos veces (un cuarto de segundo entre una y otra) para tener ya el uso
/// de cada uno.
fn abrir_procesos(c: &mut Cliente, b: &mut Barra, p: &mut Procesos, m: &mut monitor::Medidor, r: Rect) {
    let ok = medir_procesos(m);
    dashos::dormir_us(250_000);
    b.uso = medir_uso(m);
    let ok = ok && medir_procesos(m);
    p.sin_datos = !ok;
    p.alto = alto_procesos(m.vistos.len());
    let mut e = Escritor::new();
    let Some(hoja) = Lamina::nueva(&mut e, (ANCHO_P * ALTO_P_MAX * 4) as usize, VA_P, POOL_P) else {
        println!("  [barra] sin memoria para los procesos");
        return;
    };
    p.hoja = Some(hoja);
    p.listo = false;
    b.procesos_abierto = true;
    let derecha = (b.ancho - (r.x + r.w)).max(LADO);
    e.empezar(POOL_P, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER_PROC).int(0).int(ANCHO_P).int(p.alto).int(ANCHO_P * 4).uint(0).fin();
    e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUP_PROC).fin();
    e.empezar(CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(CAPA_PROC).uint(SUP_PROC).uint(0).uint(2).texto(Some("procesos")).fin();
    e.empezar(CAPA_PROC, zwlr_layer_surface_v1::req::SET_SIZE).uint(ANCHO_P as u32).uint(p.alto as u32).fin();
    e.empezar(CAPA_PROC, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(1 | 8).fin();
    e.empezar(CAPA_PROC, zwlr_layer_surface_v1::req::SET_MARGIN).int(LADO).int(derecha).int(0).int(0).fin();
    e.empezar(CAPA_PROC, zwlr_layer_surface_v1::req::SET_KEYBOARD_INTERACTIVITY).uint(0).fin();
    e.empezar(SUP_PROC, wl_surface::req::COMMIT).fin();
    c.mandar(&e);
    let nombres: Vec<String> = m.los_que_mas(FILAS_P).iter().map(|v| match v.e.pastilla() {
        Some(x) => alloc::format!("{} [{}]", v.e.titulo(), x),
        None => v.e.titulo(),
    }).collect();
    let b = boton_abrir(p.alto);
    println!("  [barra] procesos abiertos: {} filas{}: {}; abrir el monitor en ({}, {})", m.vistos.len(), if ok { "" } else { " (sesiones no contesta)" }, nombres.join(", "), b.x + b.w / 2, b.y + b.h / 2);
}

fn cerrar_procesos(c: &mut Cliente, b: &mut Barra, p: &mut Procesos) {
    if !b.procesos_abierto {
        return;
    }
    let mut e = Escritor::new();
    e.empezar(CAPA_PROC, zwlr_layer_surface_v1::req::DESTROY).fin();
    e.empezar(SUP_PROC, wl_surface::req::DESTROY).fin();
    e.empezar(BUFER_PROC, wl_buffer::req::DESTROY).fin();
    if let Some(h) = p.hoja.take() {
        h.soltar(&mut e);
    }
    c.mandar(&e);
    p.listo = false;
    b.procesos_abierto = false;
    println!("  [barra] procesos cerrados");
}

/// S10: dónde cae el ratón para los menús: la barra, lo desplegado u otra cosa.
fn donde_menus(sobre: u32) -> barra_menus::Donde {
    match sobre {
        SUPERFICIE => barra_menus::Donde::Barra,
        s if barra_menus::es_suya(s) => barra_menus::Donde::Menu(s),
        _ => barra_menus::Donde::Otro,
    }
}

/// S13: la zona horaria (su puesto en `zona::ZONAS`; 0 es UTC). La pone `leer_zona` al arrancar.
static ZONA: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

fn zona_de_ahora() -> &'static zona::Zona {
    zona::ZONAS.get(ZONA.load(core::sync::atomic::Ordering::Relaxed)).unwrap_or(&zona::UTC)
}

/// S13: lee la zona horaria de `Prefs/zona` (una línea con su nombre, como "Europe/Madrid"), por la raíz 1 de la barra
/// en el servicio de archivos (los ajustes del sistema, solo para leer). Si no hay fichero, no se entiende o la zona no
/// está en la tabla, se queda UTC. Devuelve el porqué si no se pudo.
fn leer_zona() -> Result<(), &'static str> {
    let (lectura, suyo) = dashos::canal_crear();
    let mut cab = [0u8; a::CABECERA];
    cab[0..4].copy_from_slice(&a::SESION.to_le_bytes());
    let mut r = [0u8; 32];
    let mut ll = [0u32; 1];
    let (n, k, _) = if dashos::canal_escribir(ARCHIVOS, &cab, &[suyo]) == 0 {
        dashos::canal_leer_plazo(lectura, &mut r, &mut ll, PLAZO_ARCHIVOS_US)
    } else {
        (-1, 0, 0)
    };
    if n != 32 || k != 1 || dashos::memoria_proyectar(ll[0], VA_ARCHIVOS, false) != 0 {
        if n >= 0 && k == 1 {
            dashos::cerrar(ll[0]);
        }
        dashos::cerrar(lectura);
        return Err("sin servicio de archivos");
    }
    dashos::cerrar(ll[0]);
    // Una orden: estado, asa y el primer número de la respuesta.
    let pedir = |orden: u32, asa: u32, y: u64, nombre: &str| -> (i32, u32, u64) {
        let mut p = [0u8; a::CABECERA + 16];
        p[0..4].copy_from_slice(&orden.to_le_bytes());
        p[4..8].copy_from_slice(&asa.to_le_bytes());
        p[16..24].copy_from_slice(&y.to_le_bytes());
        p[24..28].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
        p[32..32 + nombre.len()].copy_from_slice(nombre.as_bytes());
        let mut r = [0u8; 32];
        if dashos::canal_escribir(ARCHIVOS, &p[..32 + nombre.len()], &[]) != 0 || dashos::canal_leer_plazo(lectura, &mut r, &mut [], PLAZO_ARCHIVOS_US).0 != 32 {
            return (-1, 0, 0);
        }
        (i32::from_le_bytes(r[0..4].try_into().unwrap()), u32::from_le_bytes(r[4..8].try_into().unwrap()), u64::from_le_bytes(r[8..16].try_into().unwrap()))
    };
    let mut resultado = Err("no hay Prefs/zona");
    // La raíz 1: `Prefs` del disco del sistema.
    let (e, asa, tam) = pedir(a::BUSCAR, 1, 0, "zona");
    if e == a::BIEN {
        let (e, _, leidos) = pedir(a::LEER, asa, tam.min(64), "");
        resultado = Err("Prefs/zona no se lee");
        if e == a::BIEN {
            let v = unsafe { core::slice::from_raw_parts(VA_ARCHIVOS as *const u8, (leidos as usize).min(64)) };
            let nombre = core::str::from_utf8(v).ok().and_then(|t| t.lines().next()).unwrap_or("");
            resultado = match zona::ZONAS.iter().position(|z| z.nombre.eq_ignore_ascii_case(nombre.trim())) {
                Some(i) => {
                    ZONA.store(i, core::sync::atomic::Ordering::Relaxed);
                    Ok(())
                }
                None => Err("la zona de Prefs/zona no está en la tabla"),
            };
        }
        pedir(a::CERRAR, asa, 0, "");
    }
    dashos::memoria_quitar(VA_ARCHIVOS);
    dashos::cerrar(lectura);
    resultado
}

/// La fecha y la hora de ahora en la zona horaria (`ZONA`), si se sabe la hora.
fn hora_ahora() -> Option<reloj::Fecha> {
    let ns = dashos::hora_ns();
    (ns > 0).then(|| reloj::fecha(zona::local_ns(zona_de_ahora(), ns)))
}

/// Cuánto falta para el próximo minuto (o 10 s si no se sabe la hora), en microsegundos.
fn hasta_el_minuto() -> u64 {
    let ns = dashos::hora_ns();
    if ns == 0 { 10_000_000 } else { 60_000_000 - (ns / 1000) % 60_000_000 + 50_000 }
}

fn principal() -> u64 {
    dashos::monton::con_nombre(&"barra");
    let (mut fuente, mut medio) = (Fuente::regular(), Fuente::medium());
    let Some((mut c, puerta)) = Cliente::conectar_con_puerta(MARCA_TIMBRE) else {
        println!("  [barra] sin compositor");
        return 1;
    };
    let Some(globales) = c.globales() else { return 2 };
    let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
    let (Some(gc), Some(gs), Some(ga), Some(gl), Some(gt)) =
        (global("wl_compositor"), global("wl_shm"), global("wl_seat"), global("zwlr_layer_shell_v1"), global("dashos_tema_v1"))
    else {
        println!("  [barra] el compositor no le da capas o tema");
        return 3;
    };
    let global_menus = global("dashos_menus_v1");
    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gl).texto(Some("zwlr_layer_shell_v1")).uint(4).uint(CAPAS).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gt).texto(Some("dashos_tema_v1")).uint(5).uint(TEMA).fin();
    e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
    e.empezar(ASIENTO, wl_seat::req::GET_KEYBOARD).uint(TECLADO).fin();
    // S10: los menús del programa activo (si el compositor los tiene).
    let mut mb = barra_menus::MenusBarra::default();
    if let Some(gm) = global_menus {
        mb.enlazar(&mut e, gm, globales.iter().find(|g| g.1 == "wl_output").map(|g| g.0));
    }
    // La barra: arriba (anclas 1 arriba, 4 izquierda, 8 derecha), a todo lo ancho, en la capa de arriba (2), con su
    // zona exclusiva y sin teclado.
    e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUPERFICIE).fin();
    e.empezar(CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(CAPA_BARRA).uint(SUPERFICIE).uint(0).uint(2).texto(Some("barra")).fin();
    e.empezar(CAPA_BARRA, zwlr_layer_surface_v1::req::SET_SIZE).uint(0).uint(ALTO as u32).fin();
    e.empezar(CAPA_BARRA, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(1 | 4 | 8).fin();
    e.empezar(CAPA_BARRA, zwlr_layer_surface_v1::req::SET_EXCLUSIVE_ZONE).int(ALTO).fin();
    e.empezar(CAPA_BARRA, zwlr_layer_surface_v1::req::SET_KEYBOARD_INTERACTIVITY).uint(0).fin();
    e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
    c.mandar(&e);

    // S13: la zona horaria, antes de la primera hora.
    if let Err(porque) = leer_zona() {
        println!("  [barra] zona horaria: {}; se queda UTC", porque);
    }
    let mut b = Barra { tema: Tema::CLARO, ancho: 0, titulo: String::new(), hora: hora_ahora(), encima: None, lanzador_abierto: false, sin_leer: 0, historial_abierto: false, espacio: 1, ocupados: 0, estado: None, uso: (None, None), procesos_abierto: false, mb, simbolos: Simbolos::new() };
    // Trasfondo: lo que se mide (la píldora y el desplegable) y el desplegable.
    let mut med = monitor::Medidor::default();
    b.uso = medir_uso(&mut med);
    let mut proxima_medida = dashos::ahora_us() + CADA_MEDIDA_US;
    let mut procs = Procesos { hoja: None, listo: false, alto: 0, sin_datos: false, encima_abrir: false };
    let mut uso_dicho = false;
    // Los avisos: se suscribe con la puerta. Lo que ya hubiera no sale como toast, solo cuenta para la campana.
    let mut avs = Avisos { activo: false, toast: None, toast_listo: false, hist: None, hist_listo: false, visto_hasta: 0, hoja_a: None, hoja_h: None };
    if puerta != 0 {
        match pedir_avisos(&mut c, &orden_avisos(av::SUSCRIBIR, 0), &[puerta]) {
            Some((av::BIEN, sin, total, _)) => {
                avs.activo = true;
                b.sin_leer = sin;
                if let Some((_, _, _, Some(a))) = pedir_avisos(&mut c, &orden_avisos(av::LEER, u32::MAX), &[]) {
                    avs.visto_hasta = a.numero;
                }
                println!("  [barra] avisos: suscrita; {} avisos, {} sin leer", total, sin);
            }
            r => println!("  [barra] sin servicio de avisos ({})", r.map_or(i32::MIN, |x| x.0)),
        }
    }
    let mut la = Lanzador { lista: Lista::default(), encima: None, fallo: None, alto: 0, hoja: None, simbolos: Simbolos::new() };
    // El búfer de la barra: su ancho (0 si aún no está hecho) y su memoria, de ese ancho.
    let mut bufer_ancho = 0i32;
    let mut hoja_barra: Option<Lamina> = None;
    let mut lanzador_configurado = false;
    let mut teclado = Teclado::new();
    // Dónde está el puntero: en qué superficie y en qué punto.
    let (mut sobre, mut x, mut y) = (0u32, 0i32, 0i32);
    let mut avisado = false;
    let pantalla_barra = |c: &mut Cliente, b: &Barra, fuente: &mut Fuente, medio: &mut Fuente| {
        if b.ancho == 0 {
            return;
        }
        let px = unsafe { core::slice::from_raw_parts_mut(VA as *mut u32, (b.ancho * ALTO) as usize) };
        pintar_barra(px, b, fuente, medio);
        let mut e = Escritor::new();
        e.empezar(SUPERFICIE, wl_surface::req::ATTACH).uint(BUFER).int(0).int(0).fin();
        e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(b.ancho).int(ALTO).fin();
        e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
        c.mandar(&e);
    };
    let pantalla_lanzador = |c: &mut Cliente, la: &mut Lanzador, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente| {
        let Some(mut hoja) = la.hoja.take() else { return };
        pintar_lanzador(hoja.px((ANCHO_L * la.alto) as usize), la, t, fuente, medio);
        la.hoja = Some(hoja);
        let mut e = Escritor::new();
        e.empezar(SUP_LANZADOR, wl_surface::req::ATTACH).uint(BUFER_LANZADOR).int(0).int(0).fin();
        e.empezar(SUP_LANZADOR, wl_surface::req::DAMAGE).int(0).int(0).int(ANCHO_L).int(la.alto).fin();
        e.empezar(SUP_LANZADOR, wl_surface::req::COMMIT).fin();
        c.mandar(&e);
    };
    loop {
        let plazo = match avs.toast {
            Some((_, hasta)) => hasta_el_minuto().min(hasta.saturating_sub(dashos::ahora_us()).max(1)),
            None => hasta_el_minuto(),
        };
        let plazo = plazo.min(proxima_medida.saturating_sub(dashos::ahora_us()).max(1));
        let ev = match c.evento_plazo(plazo) {
            Err(()) => {
                println!("  [barra] se cortó la conexión con el compositor");
                return 5;
            }
            Ok(ev) => ev,
        };
        // La hora, en cada vuelta: si cambió el minuto, se repinta.
        let ahora = hora_ahora();
        if ahora != b.hora {
            if let (Some(f0), Some(f)) = (b.hora, ahora) {
                println!("  [barra] la hora se mueve: {:02}:{:02} → {:02}:{:02}", f0.hora, f0.minuto, f.hora, f.minuto);
            }
            b.hora = ahora;
            pantalla_barra(&mut c, &b, &mut fuente, &mut medio);
        }
        // Trasfondo: la CPU y la memoria de la píldora (se repinta si cambia lo que se ve) y, abierto, el desplegable.
        if dashos::ahora_us() >= proxima_medida {
            proxima_medida = dashos::ahora_us() + CADA_MEDIDA_US;
            let antes = (b.uso.0.map(|d| (d + 5) / 10), b.uso.1.map(|d| (d + 5) / 10));
            b.uso = medir_uso(&mut med);
            if (b.uso.0.map(|d| (d + 5) / 10), b.uso.1.map(|d| (d + 5) / 10)) != antes {
                pantalla_barra(&mut c, &b, &mut fuente, &mut medio);
            }
            if !uso_dicho && b.uso.0.is_some() && b.ancho > 0 {
                uso_dicho = true;
                let d = disponer(&b, &mut fuente, &mut medio);
                println!("  [barra] CPU y memoria: {} y {}; en ({}, {})", d.textos_monitor.0, d.textos_monitor.1, d.monitor.x + d.monitor.w / 2, d.monitor.y + d.monitor.h / 2);
            }
            if b.procesos_abierto {
                procs.sin_datos = !medir_procesos(&mut med);
                if procs.listo {
                    pantalla_procesos(&mut c, &mut procs, &med, &b.tema, &mut fuente, &mut medio);
                }
            }
        }
        // El toast, fuera al acabar su tiempo; y el timbre de los avisos.
        if avs.toast.is_some_and(|(_, hasta)| dashos::ahora_us() >= hasta) {
            cerrar_toast(&mut c, &mut avs, "se acabó su tiempo");
        }
        if c.timbres > 0 {
            c.timbres = 0;
            if avs.activo {
                let antes = b.sin_leer;
                al_timbre(&mut c, &mut b, &mut avs, &mut fuente, &mut medio);
                if b.sin_leer != antes {
                    pantalla_barra(&mut c, &b, &mut fuente, &mut medio);
                }
            }
        }
        let Some((o, orden, cuerpo)) = ev else { continue };
        let palabra = |i: usize| cuerpo.get(4 * i..4 * i + 4).map_or(0, |p| u32::from_le_bytes(p.try_into().unwrap()));
        // Abrir o cerrar el lanzador.
        let alternar = |c: &mut Cliente, b: &mut Barra, la: &mut Lanzador, configurado: &mut bool, fuente: &mut Fuente, medio: &mut Fuente| {
            let mut e = Escritor::new();
            if b.lanzador_abierto {
                e.empezar(CAPA_LANZADOR, zwlr_layer_surface_v1::req::DESTROY).fin();
                e.empezar(SUP_LANZADOR, wl_surface::req::DESTROY).fin();
                e.empezar(BUFER_LANZADOR, wl_buffer::req::DESTROY).fin();
                if let Some(h) = la.hoja.take() {
                    h.soltar(&mut e);
                }
                c.mandar(&e);
                b.lanzador_abierto = false;
                *configurado = false;
                println!("  [barra] lanzador cerrado");
            } else {
                // Los programas, cada vez: pueden haber llegado otros.
                let programas = match listar(c) {
                    Ok(p) => {
                        la.fallo = None;
                        p
                    }
                    Err(porque) => {
                        println!("  [barra] no puede listar Programas: {}", porque);
                        la.fallo = Some(porque);
                        Vec::new()
                    }
                };
                let n = programas.len();
                la.lista.poner(programas, n.clamp(1, FILAS));
                la.encima = None;
                // Su búfer, del alto que piden sus programas, en una memoria suya mientras se ve.
                la.alto = alto_lanzador(n);
                let Some(hoja) = Lamina::nueva(&mut e, (ANCHO_L * la.alto * 4) as usize, VA_L, POOL_L) else {
                    println!("  [barra] sin memoria para el lanzador");
                    return;
                };
                la.hoja = Some(hoja);
                e.empezar(POOL_L, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER_LANZADOR).int(0).int(ANCHO_L).int(la.alto).int(ANCHO_L * 4).uint(0).fin();
                // En el centro de lo que deja la barra (sin anclas ni zona propia), en la capa de arriba, con el teclado
                // mientras se vea (exclusivo, 1).
                e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUP_LANZADOR).fin();
                e.empezar(CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(CAPA_LANZADOR).uint(SUP_LANZADOR).uint(0).uint(2).texto(Some("lanzador")).fin();
                e.empezar(CAPA_LANZADOR, zwlr_layer_surface_v1::req::SET_SIZE).uint(ANCHO_L as u32).uint(la.alto as u32).fin();
                e.empezar(CAPA_LANZADOR, zwlr_layer_surface_v1::req::SET_KEYBOARD_INTERACTIVITY).uint(1).fin();
                e.empezar(SUP_LANZADOR, wl_surface::req::COMMIT).fin();
                c.mandar(&e);
                b.lanzador_abierto = true;
                println!("  [barra] lanzador abierto: {} programas", la.lista.programas.len());
            }
            pantalla_barra(c, b, fuente, medio);
        };
        // Lanza lo elegido y cierra el lanzador.
        let lanzar_elegido = |c: &mut Cliente, b: &mut Barra, la: &mut Lanzador, configurado: &mut bool, fuente: &mut Fuente, medio: &mut Fuente| {
            let Some(nombre) = la.lista.elegido_nombre().map(String::from) else { return };
            alternar(c, b, la, configurado, fuente, medio);
            let (estado, pid) = lanzar(c, &nombre);
            println!("  [barra] lanza Programas/{}: {} (proceso {})", nombre, estado, pid);
        };
        // S10: lo de los menús del programa activo.
        if let Some(repintar) = b.mb.evento(&mut c, o, orden, &cuerpo, &b.tema.clone(), &mut fuente, &mut medio) {
            if repintar {
                pantalla_barra(&mut c, &b, &mut fuente, &mut medio);
            }
            continue;
        }
        match (o, orden) {
            (TEMA, dashos_tema_v1::ev::TEMA) => {
                b.tema = Tema::del_sistema(palabra(0) | 0xff00_0000, palabra(1) != 0);
                pantalla_barra(&mut c, &b, &mut fuente, &mut medio);
                // Ajustes en texto: el tema cambia en marcha (lo pone el centro, de `ENV:Escritorio/apariencia`).
                println!("  [barra] tema del sistema: {}, color {:06x}", if palabra(1) != 0 { "oscuro" } else { "claro" }, palabra(0) & 0xff_ffff);
                if b.lanzador_abierto && lanzador_configurado {
                    pantalla_lanzador(&mut c, &mut la, &b.tema, &mut fuente, &mut medio);
                }
                if avs.toast_listo && let Some((a, _)) = avs.toast {
                    pantalla_toast(&mut c, &mut avs.hoja_a, &a, &b.tema, &mut fuente, &mut medio);
                }
                if avs.hist_listo && let Some(h) = &avs.hist {
                    pantalla_historial(&mut c, &mut avs.hoja_h, h, &b.tema, &mut fuente, &mut medio);
                }
                if procs.listo {
                    pantalla_procesos(&mut c, &mut procs, &med, &b.tema, &mut fuente, &mut medio);
                }
            }
            (TEMA, dashos_tema_v1::ev::TITULO) => {
                let m = &tablas::DASHOS_TEMA_V1.events[dashos_tema_v1::ev::TITULO as usize];
                let titulo = match leer_args(m, &cuerpo, &mut core::iter::empty()).ok().as_deref() {
                    Some([Valor::Str(Some(t))]) => t.chars().filter(|c| !c.is_control()).collect(),
                    _ => String::new(),
                };
                if titulo != b.titulo {
                    println!("  [barra] título: {}", if titulo.is_empty() { "(ninguno)" } else { &titulo });
                    b.titulo = titulo;
                    pantalla_barra(&mut c, &b, &mut fuente, &mut medio);
                }
            }
            // S27: los espacios de trabajo, del compositor.
            // S27, oleada 2: el estado de la red y del volumen, que da el panel.
            (TEMA, dashos_tema_v1::ev::ESTADO) => {
                let nuevo = Some((palabra(0).min(2), palabra(1).min(100), palabra(2) != 0));
                if nuevo != b.estado {
                    b.estado = nuevo;
                    let (red, nivel, silencio) = nuevo.unwrap();
                    println!("  [barra] estado: red {}, volumen {}{}", ["sin servicio", "sin dirección", "conectada"][red as usize], nivel, if silencio { " (silencio)" } else { "" });
                    pantalla_barra(&mut c, &b, &mut fuente, &mut medio);
                }
            }
            (TEMA, dashos_tema_v1::ev::ESPACIOS) => {
                let (actual, ocupados) = (palabra(0).clamp(1, 9), palabra(1) & 0x1ff);
                if (actual, ocupados) != (b.espacio, b.ocupados) {
                    (b.espacio, b.ocupados) = (actual, ocupados);
                    // Dónde queda el 1 (para las pruebas: un clic ahí lo pide).
                    let d = disponer(&b, &mut fuente, &mut medio);
                    let uno = d.celdas[0].0;
                    println!("  [barra] espacios: se ve el {} de {}; el 1 en ({}, {})", actual, d.celdas.len(), uno.x + uno.w / 2, uno.y + uno.h / 2);
                    pantalla_barra(&mut c, &b, &mut fuente, &mut medio);
                }
            }
            (TEMA, dashos_tema_v1::ev::LANZADOR) => {
                alternar(&mut c, &mut b, &mut la, &mut lanzador_configurado, &mut fuente, &mut medio);
            }
            (CAPA_BARRA, zwlr_layer_surface_v1::ev::CONFIGURE) => {
                let ancho = (palabra(1) as i32).clamp(1, ANCHO_MAX);
                let mut e = Escritor::new();
                e.empezar(CAPA_BARRA, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                if ancho != bufer_ancho {
                    if bufer_ancho != 0 {
                        e.empezar(BUFER, wl_buffer::req::DESTROY).fin();
                    }
                    if let Some(h) = hoja_barra.take() {
                        h.soltar(&mut e);
                    }
                    let Some(hoja) = Lamina::nueva(&mut e, (ancho * ALTO * 4) as usize, VA, POOL) else {
                        println!("  [barra] sin memoria");
                        return 4;
                    };
                    hoja_barra = Some(hoja);
                    e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER).int(0).int(ancho).int(ALTO).int(ancho * 4).uint(0).fin();
                    bufer_ancho = ancho;
                }
                c.mandar(&e);
                b.ancho = ancho;
                pantalla_barra(&mut c, &b, &mut fuente, &mut medio);
                if !avisado {
                    avisado = true;
                    println!("  [barra] en pantalla: {}x{}, zona exclusiva {}", ancho, ALTO, ALTO);
                    let d = disponer(&b, &mut fuente, &mut medio);
                    let (l, p) = (d.lanzador, d.estado);
                    println!("  [barra] botones: lanzador en ({}, {}), panel en ({}, {})", l.x + l.w / 2, l.y + l.h / 2, p.x + p.w / 2, p.y + p.h / 2);
                    let ca = d.campana;
                    println!("  [barra] campana en ({}, {})", ca.x + ca.w / 2, ca.y + ca.h / 2);
                    match b.hora {
                        Some(f) => {
                            // S13: la hora local de la zona, cómo va respecto a UTC ahora, y la hora en UTC.
                            let ns = dashos::hora_ns();
                            let z = zona_de_ahora();
                            let mut eti = [0u8; 10];
                            let n = zona::etiqueta(z, ns / 1_000_000_000, &mut eti);
                            let utc = reloj::fecha(ns);
                            println!(
                                "  [barra] hora: {:02}:{:02} ({}, {}; en UTC, las {:02}:{:02}), {} {}",
                                f.hora, f.minuto, z.nombre, core::str::from_utf8(&eti[..n]).unwrap_or("?"), utc.hora, utc.minuto, d.hora, d.fecha
                            );
                        }
                        None => println!("  [barra] hora: no se sabe"),
                    }
                }
            }
            (CAPA_BARRA, zwlr_layer_surface_v1::ev::CLOSED) => {
                println!("  [barra] el compositor cerró la barra");
                return 0;
            }
            (CAPA_LANZADOR, zwlr_layer_surface_v1::ev::CONFIGURE) if b.lanzador_abierto => {
                let mut e = Escritor::new();
                e.empezar(CAPA_LANZADOR, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                c.mandar(&e);
                lanzador_configurado = true;
                pantalla_lanzador(&mut c, &mut la, &b.tema, &mut fuente, &mut medio);
            }
            (CAPA_AVISO, zwlr_layer_surface_v1::ev::CONFIGURE) if avs.toast.is_some() => {
                let mut e = Escritor::new();
                e.empezar(CAPA_AVISO, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                c.mandar(&e);
                avs.toast_listo = true;
                if let Some((a, _)) = avs.toast {
                    pantalla_toast(&mut c, &mut avs.hoja_a, &a, &b.tema, &mut fuente, &mut medio);
                }
            }
            (CAPA_AVISO, zwlr_layer_surface_v1::ev::CLOSED) => cerrar_toast(&mut c, &mut avs, "lo cerró el compositor"),
            (CAPA_HIST, zwlr_layer_surface_v1::ev::CONFIGURE) if avs.hist.is_some() => {
                let mut e = Escritor::new();
                e.empezar(CAPA_HIST, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                c.mandar(&e);
                avs.hist_listo = true;
                if let Some(h) = &avs.hist {
                    pantalla_historial(&mut c, &mut avs.hoja_h, h, &b.tema, &mut fuente, &mut medio);
                }
            }
            (CAPA_PROC, zwlr_layer_surface_v1::ev::CONFIGURE) if b.procesos_abierto => {
                let mut e = Escritor::new();
                e.empezar(CAPA_PROC, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                c.mandar(&e);
                procs.listo = true;
                pantalla_procesos(&mut c, &mut procs, &med, &b.tema, &mut fuente, &mut medio);
            }
            (CAPA_PROC, zwlr_layer_surface_v1::ev::CLOSED) => {
                cerrar_procesos(&mut c, &mut b, &mut procs);
                pantalla_barra(&mut c, &b, &mut fuente, &mut medio);
            }
            (CAPA_HIST, zwlr_layer_surface_v1::ev::CLOSED) => {
                cerrar_historial(&mut c, &mut b, &mut avs);
                pantalla_barra(&mut c, &b, &mut fuente, &mut medio);
            }
            (CAPA_LANZADOR, zwlr_layer_surface_v1::ev::CLOSED) => {
                b.lanzador_abierto = false;
                lanzador_configurado = false;
                pantalla_barra(&mut c, &b, &mut fuente, &mut medio);
            }
            (TECLADO, wl_keyboard::ev::KEYMAP) => {
                if let Some(l) = c.juntador.tomar_llave() {
                    dashos::cerrar(l);
                }
            }
            // S10: con un menú abierto (su capa tiene el teclado), las teclas son suyas; y si el teclado se va, se cierra.
            (TECLADO, wl_keyboard::ev::KEY) if b.mb.tecla(&mut c, palabra(2), palabra(3) == 1, &b.tema.clone(), &mut fuente, &mut medio) => {
                pantalla_barra(&mut c, &b, &mut fuente, &mut medio);
            }
            (TECLADO, wl_keyboard::ev::LEAVE) => {
                if b.mb.teclado_sale(&mut c, palabra(1), &b.tema.clone(), &mut fuente, &mut medio) {
                    pantalla_barra(&mut c, &b, &mut fuente, &mut medio);
                }
            }
            (TECLADO, wl_keyboard::ev::KEY) if b.lanzador_abierto => {
                let (codigo, pulsada) = (palabra(2), palabra(3) == 1);
                if codigo == KEY_ESC {
                    if pulsada {
                        alternar(&mut c, &mut b, &mut la, &mut lanzador_configurado, &mut fuente, &mut medio);
                    }
                    continue;
                }
                let mut cambia = false;
                match teclado.tecla(codigo, pulsada) {
                    Some(Tecla::Letra(l)) => cambia = la.lista.escribir(l),
                    Some(Tecla::Retroceso) => cambia = la.lista.borrar(),
                    Some(Tecla::Abajo | Tecla::Tabulador) => la.lista.mover(true),
                    Some(Tecla::Arriba | Tecla::TabuladorAtras) => la.lista.mover(false),
                    Some(Tecla::Intro) => {
                        lanzar_elegido(&mut c, &mut b, &mut la, &mut lanzador_configurado, &mut fuente, &mut medio);
                        continue;
                    }
                    _ => {
                        if !pulsada {
                            continue;
                        }
                    }
                }
                if cambia {
                    println!("  [barra] busca \"{}\": {}", la.lista.busca, la.lista.nombres());
                }
                if lanzador_configurado {
                    pantalla_lanzador(&mut c, &mut la, &b.tema, &mut fuente, &mut medio);
                }
            }
            (PUNTERO, wl_pointer::ev::ENTER | wl_pointer::ev::LEAVE | wl_pointer::ev::MOTION) => {
                match orden {
                    wl_pointer::ev::ENTER => (sobre, x, y) = (palabra(1), palabra(2) as i32 >> 8, palabra(3) as i32 >> 8),
                    wl_pointer::ev::LEAVE => sobre = 0,
                    _ => (x, y) = (palabra(1) as i32 >> 8, palabra(2) as i32 >> 8),
                }
                // S10: los menús (pasar por los títulos, por las entradas).
                let donde = donde_menus(sobre);
                if b.mb.raton(&mut c, donde, x, y, None, &b.tema.clone(), &mut fuente, &mut medio) == Some(true) {
                    pantalla_barra(&mut c, &b, &mut fuente, &mut medio);
                }
                // La capa de estado de lo que queda bajo el puntero: se repinta solo si cambia.
                let encima = if sobre == SUPERFICIE && b.ancho > 0 { disponer(&b, &mut fuente, &mut medio).boton_en(x, y) } else { None };
                if encima != b.encima {
                    b.encima = encima;
                    pantalla_barra(&mut c, &b, &mut fuente, &mut medio);
                }
                let fila_encima = if sobre == SUP_LANZADOR {
                    (0..la.lista.filas).find(|&k| fila(k).contiene(x, y)).and_then(|k| la.lista.en_fila(k))
                } else {
                    None
                };
                let abrir = sobre == SUP_PROC && boton_abrir(procs.alto).contiene(x, y);
                if b.procesos_abierto && abrir != procs.encima_abrir {
                    procs.encima_abrir = abrir;
                    if procs.listo {
                        pantalla_procesos(&mut c, &mut procs, &med, &b.tema, &mut fuente, &mut medio);
                    }
                }
                let borrar = sobre == SUP_HIST && zona_borrar(&mut medio).contiene(x, y);
                if let Some(h) = avs.hist.as_mut().filter(|h| h.encima_borrar != borrar) {
                    h.encima_borrar = borrar;
                    if avs.hist_listo {
                        pantalla_historial(&mut c, &mut avs.hoja_h, h, &b.tema, &mut fuente, &mut medio);
                    }
                }
                if fila_encima != la.encima {
                    la.encima = fila_encima;
                    if b.lanzador_abierto && lanzador_configurado {
                        pantalla_lanzador(&mut c, &mut la, &b.tema, &mut fuente, &mut medio);
                    }
                }
            }
            // S10: un botón sobre los menús (los títulos en la barra, lo desplegado): suyo.
            (PUNTERO, wl_pointer::ev::BUTTON) if b.mb.raton(&mut c, donde_menus(sobre), x, y, Some((palabra(2), palabra(3) == 1)), &b.tema.clone(), &mut fuente, &mut medio).is_some() => {
                pantalla_barra(&mut c, &b, &mut fuente, &mut medio);
            }
            (PUNTERO, wl_pointer::ev::AXIS) if barra_menus::es_suya(sobre) && palabra(1) == 0 => {
                b.mb.rueda(&mut c, sobre, x, y, kit::controles::pasos_rueda(palabra(2)), &b.tema.clone(), &mut fuente, &mut medio);
            }
            (PUNTERO, wl_pointer::ev::BUTTON) if palabra(2) == BOTON_IZQUIERDO && palabra(3) == 1 => {
                let en_barra = if sobre == SUPERFICIE && b.ancho > 0 { disponer(&b, &mut fuente, &mut medio).boton_en(x, y) } else { None };
                if sobre == SUPERFICIE && b.ancho > 0 {
                    if en_barra == Some(Boton::Lanzador) {
                        alternar(&mut c, &mut b, &mut la, &mut lanzador_configurado, &mut fuente, &mut medio);
                    } else if let Some(Boton::Espacio(k)) = en_barra {
                        // S27: ver ese espacio; lo hace el compositor, que contesta con `espacios`.
                        let mut e = Escritor::new();
                        e.empezar(TEMA, dashos_tema_v1::req::ESPACIO).uint(k).fin();
                        c.mandar(&e);
                        println!("  [barra] pide el espacio {}", k);
                    } else if en_barra == Some(Boton::Panel) {
                        let mut e = Escritor::new();
                        e.empezar(TEMA, dashos_tema_v1::req::PANEL).fin();
                        c.mandar(&e);
                        println!("  [barra] pide el panel rápido");
                    } else if en_barra == Some(Boton::Monitor) {
                        if b.procesos_abierto {
                            cerrar_procesos(&mut c, &mut b, &mut procs);
                        } else {
                            let r = disponer(&b, &mut fuente, &mut medio).monitor;
                            abrir_procesos(&mut c, &mut b, &mut procs, &mut med, r);
                        }
                        pantalla_barra(&mut c, &b, &mut fuente, &mut medio);
                    } else if en_barra == Some(Boton::Campana) && avs.activo {
                        if avs.hist.is_some() {
                            cerrar_historial(&mut c, &mut b, &mut avs);
                        } else {
                            cerrar_toast(&mut c, &mut avs, "se abre el historial");
                            abrir_historial(&mut c, &mut b, &mut avs, &mut medio);
                        }
                        pantalla_barra(&mut c, &b, &mut fuente, &mut medio);
                    }
                } else if sobre == SUP_PROC && b.procesos_abierto && boton_abrir(procs.alto).contiene(x, y) {
                    let r = dashos::canal_escribir(CENTRO, b"abrir monitor", &[]);
                    println!("  [barra] abre el monitor del sistema: {}", r);
                    cerrar_procesos(&mut c, &mut b, &mut procs);
                    pantalla_barra(&mut c, &b, &mut fuente, &mut medio);
                } else if sobre == SUP_AVISO {
                    cerrar_toast(&mut c, &mut avs, "con un clic");
                } else if sobre == SUP_HIST && zona_borrar(&mut medio).contiene(x, y) {
                    if avs.hist.as_ref().is_some_and(|h| !h.avisos.is_empty()) {
                        let r = pedir_avisos(&mut c, &orden_avisos(av::BORRAR_TODO, 0), &[]);
                        println!("  [barra] borrar todo: {}, quedan {}", r.map_or(i32::MIN, |x| x.0), r.map_or(0, |x| x.2));
                        if let Some(h) = avs.hist.as_mut() {
                            h.avisos.clear();
                            h.primero = 0;
                            // Más bajo: solo cabe "No hay avisos". Un búfer nuevo, del tamaño nuevo.
                            h.alto = alto_historial(0);
                            let mut e = Escritor::new();
                            e.empezar(BUFER_HIST, wl_buffer::req::DESTROY).fin();
                            e.empezar(POOL_H, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER_HIST).int(0).int(ANCHO_H).int(h.alto).int(ANCHO_H * 4).uint(0).fin();
                            e.empezar(CAPA_HIST, zwlr_layer_surface_v1::req::SET_SIZE).uint(ANCHO_H as u32).uint(h.alto as u32).fin();
                            c.mandar(&e);
                        }
                        if avs.hist_listo && let Some(h) = &avs.hist {
                            pantalla_historial(&mut c, &mut avs.hoja_h, h, &b.tema, &mut fuente, &mut medio);
                        }
                    }
                } else if sobre == SUP_LANZADOR && b.lanzador_abierto {
                    if let Some(k) = (0..la.lista.filas).find(|&k| fila(k).contiene(x, y)).and_then(|k| la.lista.en_fila(k)) {
                        la.lista.elegido = k;
                        lanzar_elegido(&mut c, &mut b, &mut la, &mut lanzador_configurado, &mut fuente, &mut medio);
                    }
                }
                continue;
            }
            // La rueda sobre el historial: una fila por paso.
            (PUNTERO, wl_pointer::ev::AXIS) if sobre == SUP_HIST && palabra(1) == 0 => {
                if let Some(h) = avs.hist.as_mut() {
                    let paso = (palabra(2) as i32).signum();
                    let max = h.avisos.len().saturating_sub(FILAS_H);
                    let nuevo = (h.primero as i32 + paso).clamp(0, max as i32) as usize;
                    if nuevo != h.primero {
                        h.primero = nuevo;
                        println!("  [barra] historial: la rueda lo lleva a la fila {} de {}", nuevo + 1, h.avisos.len());
                        if avs.hist_listo {
                            pantalla_historial(&mut c, &mut avs.hoja_h, h, &b.tema, &mut fuente, &mut medio);
                        }
                    }
                }
            }
            (1, wl_display::ev::ERROR) => {
                println!("  [barra] error de protocolo en el objeto {}", palabra(0));
                return 6;
            }
            _ => {}
        }
    }
}
