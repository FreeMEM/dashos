//! S10: la barra de la pantalla y el lanzador, primera versión (`docs/diseño/S10-escritorio.md`). Es un cliente del
//! compositor, como el panel rápido: si se cae, las ventanas siguen en su sitio.
//!
//! La barra es una capa de arriba (wlr-layer-shell) a todo lo ancho, con su zona exclusiva: las ventanas se colocan y
//! se maximizan debajo. Lleva:
//! - A la izquierda, el botón del lanzador.
//! - En medio, el título de la ventana con el teclado. Se lo dice el compositor por `dashos_tema_v1` (evento `titulo`,
//!   solo a la barra): es el de la ventana en la que escribe quien está delante, nunca el de otra, y vacío con la
//!   sesión bloqueada.
//! - A la derecha, la hora (`dashos::hora_ns`, UTC: aún no hay zona horaria) y el botón del panel rápido, que lo pide
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
//! Llaves: 1 el canal "ventanas" (con las capas, por su nombre), 2 el canal "archivos", 3 el canal "sesiones" y 4 el
//! canal "avisos".

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
#[allow(dead_code)]
mod ventana_cliente;
#[path = "../lanzables.rs"]
#[allow(dead_code)]
mod lanzables;

use alloc::string::String;
use alloc::vec::Vec;
use dashos::println;
use dashos::protocolo::archivos as a;
use dashos::protocolo::avisos as av;
use dashos::protocolo::sesiones as s;
use kit::lienzo::{Color, Lienzo, Rect};
use kit::teclado::{Tecla, Teclado};
use kit::tema::{con_opacidad, Tema};
use kit::texto::Fuente;
use ventana_cliente::*;
use wayland::tablas::{
    self, dashos_tema_v1, wl_buffer, wl_compositor, wl_display, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface,
    zwlr_layer_shell_v1, zwlr_layer_surface_v1,
};
use wayland::{leer_args, Escritor, Valor};

dashos::programa!(principal);
dashos::monton!();

// Las llaves de los canales van en el orden en que se nombra cada uno por primera vez en la lista de `inicio`, no en el
// de la lista de la barra: "archivos" se nombra antes que "sesiones".
const ARCHIVOS: u32 = 2;
const SESIONES: u32 = 3;
const AVISOS: u32 = 4;
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
/// Dónde se ve la ventana del servicio de archivos.
const VA_ARCHIVOS: usize = 0x50_0000_0000;

/// La barra: 40 de alto, con botones de icono de 32 a 8 de los lados.
const ALTO: i32 = 40;
const ANCHO_MAX: i32 = 3840;
const BOTON: i32 = 32;
const LADO: i32 = 8;
/// El lanzador: el buscador arriba y seis filas de 48 debajo.
const ANCHO_L: i32 = 360;
const FILAS: usize = 6;
const ALTO_FILA: i32 = 48;
const BUSCADOR: Rect = Rect { x: 12, y: 12, w: ANCHO_L - 24, h: 48 };
const Y_FILAS: i32 = 72;
/// Lo más alto que se hace (la memoria es para esto); al abrirlo, mide lo que piden sus programas (`alto_lanzador`).
const ALTO_L_MAX: i32 = Y_FILAS + FILAS as i32 * ALTO_FILA + 12;
/// Lo que se escribe en el buscador, como mucho (en letras).
const MAX_BUSCA: usize = 30;
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
/// La memoria: la barra (lo más ancha que se pinta) y, detrás, el lanzador, el toast y el historial.
const OFF_L: usize = (ANCHO_MAX * ALTO * 4) as usize;
const OFF_A: usize = OFF_L + (ANCHO_L * ALTO_L_MAX * 4) as usize;
const OFF_H: usize = OFF_A + (ANCHO_A * ALTO_A * 4) as usize;
const TAMANO: usize = OFF_H + (ANCHO_H * ALTO_H_MAX * 4) as usize;
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
}

fn zona_lanzador() -> Rect {
    Rect::new(LADO, (ALTO - BOTON) / 2, BOTON, BOTON)
}

fn zona_panel(ancho: i32) -> Rect {
    Rect::new(ancho - LADO - BOTON, (ALTO - BOTON) / 2, BOTON, BOTON)
}

/// La campana de los avisos, a la izquierda del botón del panel.
fn zona_campana(ancho: i32) -> Rect {
    Rect::new(ancho - LADO - 2 * BOTON - 4, (ALTO - BOTON) / 2, BOTON, BOTON)
}

struct Barra {
    tema: Tema,
    ancho: i32,
    titulo: String,
    /// La hora que se enseña (horas y minutos), si se sabe.
    hora: Option<(u64, u64)>,
    encima: Option<Boton>,
    lanzador_abierto: bool,
    /// Los avisos sin leer (la campana lleva un punto) y si el historial está abierto.
    sin_leer: u32,
    historial_abierto: bool,
}

struct Lanzador {
    programas: Vec<String>,
    busca: String,
    /// Los que casan con la búsqueda (por su sitio en `programas`), los que empiezan así primero.
    vistos: Vec<usize>,
    elegido: usize,
    /// La primera fila que se ve (con más de `FILAS`, se desplaza con lo elegido).
    primero: usize,
    encima: Option<usize>,
    /// Si el listado de `Programas/` falló: el porqué.
    fallo: Option<&'static str>,
    /// Lo que mide de alto (lo fija al abrirse).
    alto: i32,
}

impl Lanzador {
    fn filtrar(&mut self) {
        let b = self.busca.to_lowercase();
        let mut empiezan = Vec::new();
        let mut contienen = Vec::new();
        for (i, p) in self.programas.iter().enumerate() {
            let n = p.to_lowercase();
            if n.starts_with(&b) {
                empiezan.push(i);
            } else if n.contains(&b) {
                contienen.push(i);
            }
        }
        empiezan.extend(contienen);
        self.vistos = empiezan;
        self.elegido = 0;
        self.primero = 0;
        self.encima = None;
    }

    /// Cuántas filas caben.
    fn filas(&self) -> usize {
        ((self.alto - Y_FILAS - 12) / ALTO_FILA).max(1) as usize
    }

    fn mover(&mut self, abajo: bool) {
        if self.vistos.is_empty() {
            return;
        }
        self.elegido = if abajo { (self.elegido + 1).min(self.vistos.len() - 1) } else { self.elegido.saturating_sub(1) };
        if self.elegido < self.primero {
            self.primero = self.elegido;
        } else if self.elegido >= self.primero + self.filas() {
            self.primero = self.elegido + 1 - self.filas();
        }
    }

    fn nombres(&self) -> String {
        let v: Vec<&str> = self.vistos.iter().map(|&i| self.programas[i].as_str()).collect();
        if v.is_empty() { String::from("ninguno") } else { v.join(" ") }
    }
}

/// La fila `k` de las que se ven del lanzador.
/// La altura del lanzador con `n` programas: hasta `FILAS` filas, y al menos una (para decir que no hay). Se fija al
/// abrirlo: al buscar no cambia.
fn alto_lanzador(n: usize) -> i32 {
    Y_FILAS + n.clamp(1, FILAS) as i32 * ALTO_FILA + 12
}

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

/// El icono de aplicaciones de Material (nueve puntos en rejilla), de 18, centrado en `r`.
fn icono_apps(l: &mut Lienzo, r: Rect, color: Color) {
    let (cx2, cy2) = (2 * r.x + r.w, 2 * r.y + r.h);
    for fy in -1..=1 {
        for fx in -1..=1 {
            punto(l, cx2 + fx * 14, cy2 + fy * 14, 4, color);
        }
    }
}

/// El icono de ajustes rápidos (tres deslizadores), de 18, centrado en `r`: líneas finas y un mando en cada una.
fn icono_ajustes(l: &mut Lienzo, r: Rect, color: Color) {
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    for (k, mando) in [(-1i32, 3i32), (0, -4), (1, 1)] {
        let y = cy + 6 * k;
        // En octavos de píxel, por el centro de la fila de píxeles.
        l.trazo(((cx - 8) * 8, y * 8 + 4), ((cx + 8) * 8, y * 8 + 4), 14, color);
        punto(l, 2 * (cx + mando) + 1, 2 * y + 1, 6, color);
    }
}

/// La lupa de buscar, de 20, centrada en `r`.
fn icono_lupa(l: &mut Lienzo, r: Rect, color: Color) {
    let (cx, cy) = (r.x + r.w / 2 - 2, r.y + r.h / 2 - 2);
    l.borde(Rect::new(cx - 6, cy - 6, 13, 13), 7, 2, color);
    l.trazo(((cx + 4) * 8 + 4, (cy + 4) * 8 + 4), ((cx + 9) * 8, (cy + 9) * 8), 18, color);
}

fn pintar_barra(px: &mut [u32], b: &Barra, fuente: &mut Fuente, medio: &mut Fuente) {
    let t = &b.tema;
    let w = b.ancho;
    let mut l = Lienzo::new(px, w, ALTO, w as usize);
    l.borrar(Rect::new(0, 0, w, ALTO), t.contenedor());
    // El lanzador: tonal; abierto, lleno (como un botón de icono que se queda pulsado). Encima, la capa de estado.
    let r = zona_lanzador();
    let (fondo, tinta) = if b.lanzador_abierto { (t.primario, t.sobre_primario) } else { (t.contenedor_secundario(), t.sobre_contenedor_secundario()) };
    l.redondeado(r, BOTON / 2, fondo);
    if b.encima == Some(Boton::Lanzador) {
        l.redondeado(r, BOTON / 2, con_opacidad(tinta, 20));
    }
    icono_apps(&mut l, r, tinta);
    // El panel rápido: tonal.
    let r = zona_panel(w);
    l.redondeado(r, BOTON / 2, t.contenedor_secundario());
    if b.encima == Some(Boton::Panel) {
        l.redondeado(r, BOTON / 2, con_opacidad(t.sobre_contenedor_secundario(), 20));
    }
    icono_ajustes(&mut l, r, t.sobre_contenedor_secundario());
    // La campana: tonal; con el historial abierto, llena. Con avisos sin leer, un punto del color de error arriba a la
    // derecha (la insignia pequeña de Material 3).
    let r = zona_campana(w);
    let (fondo, tinta) = if b.historial_abierto { (t.primario, t.sobre_primario) } else { (t.contenedor_secundario(), t.sobre_contenedor_secundario()) };
    l.redondeado(r, BOTON / 2, fondo);
    if b.encima == Some(Boton::Campana) {
        l.redondeado(r, BOTON / 2, con_opacidad(tinta, 20));
    }
    icono_campana(&mut l, r, tinta);
    if b.sin_leer > 0 {
        punto(&mut l, 2 * (r.x + r.w - 7), 2 * (r.y + 7), 8, t.error);
    }
    // La hora, a la izquierda de la campana.
    let hora = match b.hora {
        Some((h, m)) => alloc::format!("{:02}:{:02}", h, m),
        None => String::from("--:--"),
    };
    let ancho_hora = medio.medir(&hora, 14).ancho;
    let x_hora = r.x - 12 - ancho_hora;
    medio.dibujar(&mut l, x_hora, 25, &hora, 14, t.sobre_superficie);
    // El título, en medio de la pantalla, sin pisar los lados; sin ventana con el teclado, el nombre del sistema.
    let izquierda = LADO + BOTON + 16;
    let derecha = x_hora - 16;
    let hueco = (derecha - izquierda).max(0);
    let (texto, color, f) = if b.titulo.is_empty() {
        (String::from("DashOS"), t.sobre_variante_superficie, &mut *fuente)
    } else {
        (que_quepa(medio, &b.titulo, 14, hueco), t.sobre_superficie, &mut *medio)
    };
    let ancho = f.medir(&texto, 14).ancho;
    let x = ((w - ancho) / 2).clamp(izquierda, (derecha - ancho).max(izquierda));
    f.dibujar(&mut l, x, 25, &texto, 14, color);
}

/// La campana de los avisos (18), centrada en `r`: la cúpula, el borde y el badajo.
fn icono_campana(l: &mut Lienzo, r: Rect, color: Color) {
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    l.redondeado_esquinas(Rect::new(cx - 6, cy - 7, 12, 11), [6, 6, 1, 1], color);
    l.redondeado(Rect::new(cx - 8, cy + 3, 16, 2), 1, color);
    punto(l, 2 * cx, 2 * cy + 13, 4, color);
}

/// La hora (UTC) de `ns`, como "hh:mm"; sin hora, vacío.
fn hora_de(ns: u64) -> String {
    if ns == 0 {
        return String::new();
    }
    let min = ns / 60_000_000_000 % 1440;
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

fn pintar_lanzador(px: &mut [u32], la: &Lanzador, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente) {
    px.fill(0);
    let alto = la.alto;
    let mut l = Lienzo::new(px, ANCHO_L, alto, ANCHO_L as usize);
    l.redondeado(Rect::new(0, 0, ANCHO_L, alto), 28, t.contenedor());
    // El buscador: una barra de búsqueda de Material, redonda, con la lupa y el cursor.
    l.redondeado(BUSCADOR, BUSCADOR.h / 2, t.contenedor_alto());
    icono_lupa(&mut l, Rect::new(BUSCADOR.x + 8, BUSCADOR.y + 12, 24, 24), t.sobre_variante_superficie);
    let x = BUSCADOR.x + 44;
    let base = BUSCADOR.y + 30;
    let cursor = if la.busca.is_empty() {
        fuente.dibujar(&mut l, x + 4, base, "Buscar programas", 16, t.sobre_variante_superficie);
        x
    } else {
        fuente.dibujar(&mut l, x, base, &la.busca, 16, t.sobre_superficie);
        x + fuente.medir(&la.busca, 16).ancho + 1
    };
    l.rellenar(Rect::new(cursor, BUSCADOR.y + 13, 2, 22), t.primario);
    // Las filas: un círculo con la inicial (contenedor primario) y el nombre. Lo elegido, en el contenedor secundario,
    // con "Intro" a la derecha; bajo el ratón, la capa de estado.
    for (k, &i) in la.vistos.iter().enumerate().skip(la.primero).take(la.filas()) {
        let r = fila(k - la.primero);
        let elegido = k == la.elegido;
        if elegido {
            l.redondeado(r, r.h / 2, t.contenedor_secundario());
        } else if la.encima == Some(k) {
            l.redondeado(r, r.h / 2, con_opacidad(t.sobre_superficie, 20));
        }
        let nombre = &la.programas[i];
        let avatar = Rect::new(r.x + 8, r.y + 8, 32, 32);
        l.redondeado(avatar, 16, t.contenedor_primario);
        let inicial: String = nombre.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
        centrado(medio, &mut l, avatar, avatar.y + 21, &inicial, 14, t.sobre_contenedor_primario);
        let ancho_nombre = r.w - 56 - if elegido { 64 } else { 16 };
        let visto = que_quepa(fuente, nombre, 16, ancho_nombre);
        fuente.dibujar(&mut l, r.x + 52, r.y + 30, &visto, 16, t.sobre_superficie);
        if elegido {
            let ancho = fuente.medir("Intro", 12).ancho;
            fuente.dibujar(&mut l, r.x + r.w - 16 - ancho, r.y + 29, "Intro", 12, t.sobre_variante_superficie);
        }
    }
    if la.vistos.is_empty() {
        let texto = match (la.fallo, la.programas.is_empty()) {
            (Some(_), _) => "No se puede leer Programas",
            (None, true) => "No hay programas",
            (None, false) => "Ningún programa se llama así",
        };
        centrado(fuente, &mut l, Rect::new(0, Y_FILAS, ANCHO_L, ALTO_FILA), Y_FILAS + 30, texto, 14, t.sobre_variante_superficie);
    }
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
        if siguiente == u64::MAX || cuantos == 0 || programas.len() > 200 {
            break;
        }
        desde = siguiente;
    }
    dashos::memoria_quitar(VA_ARCHIVOS);
    dashos::cerrar(lectura);
    programas.sort_by_key(|p| p.to_lowercase());
    resultado.map(|_| programas)
}

/// Pide a `sesiones` que lance `Programas/<nombre>` (como quien está delante): su estado y el número del proceso.
fn lanzar(c: &mut Cliente, nombre: &str) -> (i32, u64) {
    let camino = alloc::format!("{}{}", core::str::from_utf8(lanzables::CAJON).unwrap_or("Programas/"), nombre);
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
}

fn pantalla_toast(c: &mut Cliente, a: &av::Aviso, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente) {
    let px = unsafe { core::slice::from_raw_parts_mut((VA + OFF_A) as *mut u32, (ANCHO_A * ALTO_A) as usize) };
    pintar_toast(px, a, t, fuente, medio);
    let mut e = Escritor::new();
    e.empezar(SUP_AVISO, wl_surface::req::ATTACH).uint(BUFER_AVISO).int(0).int(0).fin();
    e.empezar(SUP_AVISO, wl_surface::req::DAMAGE).int(0).int(0).int(ANCHO_A).int(ALTO_A).fin();
    e.empezar(SUP_AVISO, wl_surface::req::COMMIT).fin();
    c.mandar(&e);
}

fn pantalla_historial(c: &mut Cliente, h: &Historial, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente) {
    let px = unsafe { core::slice::from_raw_parts_mut((VA + OFF_H) as *mut u32, (ANCHO_H * h.alto) as usize) };
    pintar_historial(px, h, t, fuente, medio);
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
            pantalla_toast(c, &a, t, fuente, medio);
        }
        return;
    }
    avs.toast = Some((a, hasta));
    avs.toast_listo = false;
    let mut e = Escritor::new();
    e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER_AVISO).int(OFF_A as i32).int(ANCHO_A).int(ALTO_A).int(ANCHO_A * 4).uint(0).fin();
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
    avs.hist = Some(Historial { avisos, primero: 0, alto, encima_borrar: false });
    avs.hist_listo = false;
    b.historial_abierto = true;
    // Arriba a la derecha, bajo su botón (sin zona propia: el compositor la pone bajo la barra), sin teclado.
    let mut e = Escritor::new();
    e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER_HIST).int(OFF_H as i32).int(ANCHO_H).int(alto).int(ANCHO_H * 4).uint(0).fin();
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
            pantalla_historial(c, h, &b.tema, fuente, medio);
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

/// La hora y los minutos de ahora (UTC), si se sabe la hora.
fn hora_ahora() -> Option<(u64, u64)> {
    let ns = dashos::hora_ns();
    (ns > 0).then(|| {
        let min = ns / 60_000_000_000 % 1440;
        (min / 60, min % 60)
    })
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
    let m = dashos::memoria_crear(TAMANO);
    if (m as i32) <= 0 || dashos::memoria_proyectar(m as u32, VA, true) != 0 {
        println!("  [barra] sin memoria");
        return 4;
    }
    let copia = dashos::llave_duplicar(m as u32, dashos::LEER | dashos::TRANSFERIR, 0);
    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gl).texto(Some("zwlr_layer_shell_v1")).uint(4).uint(CAPAS).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gt).texto(Some("dashos_tema_v1")).uint(1).uint(TEMA).fin();
    e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
    e.empezar(ASIENTO, wl_seat::req::GET_KEYBOARD).uint(TECLADO).fin();
    e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(POOL).llave(copia as u32).int(TAMANO as i32).fin();
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

    let mut b = Barra { tema: Tema::CLARO, ancho: 0, titulo: String::new(), hora: hora_ahora(), encima: None, lanzador_abierto: false, sin_leer: 0, historial_abierto: false };
    // Los avisos: se suscribe con la puerta. Lo que ya hubiera no sale como toast, solo cuenta para la campana.
    let mut avs = Avisos { activo: false, toast: None, toast_listo: false, hist: None, hist_listo: false, visto_hasta: 0 };
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
    let mut la = Lanzador { programas: Vec::new(), busca: String::new(), vistos: Vec::new(), elegido: 0, primero: 0, encima: None, fallo: None, alto: 0 };
    // El búfer de la barra: su ancho (0 si aún no está hecho).
    let mut bufer_ancho = 0i32;
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
    let pantalla_lanzador = |c: &mut Cliente, la: &Lanzador, t: &Tema, fuente: &mut Fuente, medio: &mut Fuente| {
        let px = unsafe { core::slice::from_raw_parts_mut((VA + OFF_L) as *mut u32, (ANCHO_L * la.alto) as usize) };
        pintar_lanzador(px, la, t, fuente, medio);
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
            if let (Some((h0, m0)), Some((h, m))) = (b.hora, ahora) {
                println!("  [barra] la hora se mueve: {:02}:{:02} → {:02}:{:02}", h0, m0, h, m);
            }
            b.hora = ahora;
            pantalla_barra(&mut c, &b, &mut fuente, &mut medio);
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
                c.mandar(&e);
                b.lanzador_abierto = false;
                *configurado = false;
                println!("  [barra] lanzador cerrado");
            } else {
                // Los programas, cada vez: pueden haber llegado otros.
                match listar(c) {
                    Ok(p) => (la.programas, la.fallo) = (p, None),
                    Err(porque) => {
                        println!("  [barra] no puede listar Programas: {}", porque);
                        (la.programas, la.fallo) = (Vec::new(), Some(porque));
                    }
                }
                la.busca.clear();
                la.filtrar();
                // Su búfer, del alto que piden sus programas (el de la vez anterior, fuera).
                if la.alto != 0 {
                    e.empezar(BUFER_LANZADOR, wl_buffer::req::DESTROY).fin();
                }
                la.alto = alto_lanzador(la.programas.len());
                e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER_LANZADOR).int(OFF_L as i32).int(ANCHO_L).int(la.alto).int(ANCHO_L * 4).uint(0).fin();
                // Arriba a la izquierda, bajo su botón (sin zona propia: el compositor la pone bajo la barra), en la capa
                // de arriba, con el teclado mientras se vea (exclusivo, 1).
                e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUP_LANZADOR).fin();
                e.empezar(CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(CAPA_LANZADOR).uint(SUP_LANZADOR).uint(0).uint(2).texto(Some("lanzador")).fin();
                e.empezar(CAPA_LANZADOR, zwlr_layer_surface_v1::req::SET_SIZE).uint(ANCHO_L as u32).uint(la.alto as u32).fin();
                e.empezar(CAPA_LANZADOR, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(1 | 4).fin();
                e.empezar(CAPA_LANZADOR, zwlr_layer_surface_v1::req::SET_MARGIN).int(LADO).int(0).int(0).int(LADO).fin();
                e.empezar(CAPA_LANZADOR, zwlr_layer_surface_v1::req::SET_KEYBOARD_INTERACTIVITY).uint(1).fin();
                e.empezar(SUP_LANZADOR, wl_surface::req::COMMIT).fin();
                c.mandar(&e);
                b.lanzador_abierto = true;
                println!("  [barra] lanzador abierto: {} programas", la.programas.len());
            }
            pantalla_barra(c, b, fuente, medio);
        };
        // Lanza lo elegido y cierra el lanzador.
        let lanzar_elegido = |c: &mut Cliente, b: &mut Barra, la: &mut Lanzador, configurado: &mut bool, fuente: &mut Fuente, medio: &mut Fuente| {
            let Some(&i) = la.vistos.get(la.elegido) else { return };
            let nombre = la.programas[i].clone();
            alternar(c, b, la, configurado, fuente, medio);
            let (estado, pid) = lanzar(c, &nombre);
            println!("  [barra] lanza Programas/{}: {} (proceso {})", nombre, estado, pid);
        };
        match (o, orden) {
            (TEMA, dashos_tema_v1::ev::TEMA) => {
                b.tema = Tema::del_sistema(palabra(0) | 0xff00_0000, palabra(1) != 0);
                pantalla_barra(&mut c, &b, &mut fuente, &mut medio);
                if b.lanzador_abierto && lanzador_configurado {
                    pantalla_lanzador(&mut c, &la, &b.tema, &mut fuente, &mut medio);
                }
                if avs.toast_listo && let Some((a, _)) = avs.toast {
                    pantalla_toast(&mut c, &a, &b.tema, &mut fuente, &mut medio);
                }
                if avs.hist_listo && let Some(h) = &avs.hist {
                    pantalla_historial(&mut c, h, &b.tema, &mut fuente, &mut medio);
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
                    e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER).int(0).int(ancho).int(ALTO).int(ancho * 4).uint(0).fin();
                    bufer_ancho = ancho;
                }
                c.mandar(&e);
                b.ancho = ancho;
                pantalla_barra(&mut c, &b, &mut fuente, &mut medio);
                if !avisado {
                    avisado = true;
                    println!("  [barra] en pantalla: {}x{}, zona exclusiva {}", ancho, ALTO, ALTO);
                    let (l, p) = (zona_lanzador(), zona_panel(ancho));
                    println!("  [barra] botones: lanzador en ({}, {}), panel en ({}, {})", l.x + l.w / 2, l.y + l.h / 2, p.x + p.w / 2, p.y + p.h / 2);
                    let ca = zona_campana(ancho);
                    println!("  [barra] campana en ({}, {})", ca.x + ca.w / 2, ca.y + ca.h / 2);
                    match b.hora {
                        Some((h, m)) => println!("  [barra] hora: {:02}:{:02} (UTC)", h, m),
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
                pantalla_lanzador(&mut c, &la, &b.tema, &mut fuente, &mut medio);
            }
            (CAPA_AVISO, zwlr_layer_surface_v1::ev::CONFIGURE) if avs.toast.is_some() => {
                let mut e = Escritor::new();
                e.empezar(CAPA_AVISO, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                c.mandar(&e);
                avs.toast_listo = true;
                if let Some((a, _)) = avs.toast {
                    pantalla_toast(&mut c, &a, &b.tema, &mut fuente, &mut medio);
                }
            }
            (CAPA_AVISO, zwlr_layer_surface_v1::ev::CLOSED) => cerrar_toast(&mut c, &mut avs, "lo cerró el compositor"),
            (CAPA_HIST, zwlr_layer_surface_v1::ev::CONFIGURE) if avs.hist.is_some() => {
                let mut e = Escritor::new();
                e.empezar(CAPA_HIST, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                c.mandar(&e);
                avs.hist_listo = true;
                if let Some(h) = &avs.hist {
                    pantalla_historial(&mut c, h, &b.tema, &mut fuente, &mut medio);
                }
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
            (TECLADO, wl_keyboard::ev::KEY) if b.lanzador_abierto => {
                let (codigo, pulsada) = (palabra(2), palabra(3) == 1);
                if codigo == KEY_ESC {
                    if pulsada {
                        alternar(&mut c, &mut b, &mut la, &mut lanzador_configurado, &mut fuente, &mut medio);
                    }
                    continue;
                }
                let antes = la.busca.clone();
                match teclado.tecla(codigo, pulsada) {
                    Some(Tecla::Letra(l)) if la.busca.chars().count() < MAX_BUSCA && !l.is_control() => la.busca.push(l),
                    Some(Tecla::Retroceso) => {
                        la.busca.pop();
                    }
                    Some(Tecla::Abajo | Tecla::Tabulador) => la.mover(true),
                    Some(Tecla::Arriba | Tecla::TabuladorAtras) => la.mover(false),
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
                if la.busca != antes {
                    la.filtrar();
                    println!("  [barra] busca \"{}\": {}", la.busca, la.nombres());
                }
                if lanzador_configurado {
                    pantalla_lanzador(&mut c, &la, &b.tema, &mut fuente, &mut medio);
                }
            }
            (PUNTERO, wl_pointer::ev::ENTER | wl_pointer::ev::LEAVE | wl_pointer::ev::MOTION) => {
                match orden {
                    wl_pointer::ev::ENTER => (sobre, x, y) = (palabra(1), palabra(2) as i32 >> 8, palabra(3) as i32 >> 8),
                    wl_pointer::ev::LEAVE => sobre = 0,
                    _ => (x, y) = (palabra(1) as i32 >> 8, palabra(2) as i32 >> 8),
                }
                // La capa de estado de lo que queda bajo el puntero: se repinta solo si cambia.
                let encima = match sobre {
                    SUPERFICIE if b.ancho > 0 && zona_lanzador().contiene(x, y) => Some(Boton::Lanzador),
                    SUPERFICIE if b.ancho > 0 && zona_panel(b.ancho).contiene(x, y) => Some(Boton::Panel),
                    SUPERFICIE if b.ancho > 0 && zona_campana(b.ancho).contiene(x, y) => Some(Boton::Campana),
                    _ => None,
                };
                if encima != b.encima {
                    b.encima = encima;
                    pantalla_barra(&mut c, &b, &mut fuente, &mut medio);
                }
                let fila_encima = if sobre == SUP_LANZADOR {
                    (0..la.filas()).find(|&k| fila(k).contiene(x, y)).map(|k| k + la.primero).filter(|&k| k < la.vistos.len())
                } else {
                    None
                };
                let borrar = sobre == SUP_HIST && zona_borrar(&mut medio).contiene(x, y);
                if let Some(h) = avs.hist.as_mut().filter(|h| h.encima_borrar != borrar) {
                    h.encima_borrar = borrar;
                    if avs.hist_listo {
                        pantalla_historial(&mut c, h, &b.tema, &mut fuente, &mut medio);
                    }
                }
                if fila_encima != la.encima {
                    la.encima = fila_encima;
                    if b.lanzador_abierto && lanzador_configurado {
                        pantalla_lanzador(&mut c, &la, &b.tema, &mut fuente, &mut medio);
                    }
                }
            }
            (PUNTERO, wl_pointer::ev::BUTTON) if palabra(2) == BOTON_IZQUIERDO && palabra(3) == 1 => {
                if sobre == SUPERFICIE && b.ancho > 0 {
                    if zona_lanzador().contiene(x, y) {
                        alternar(&mut c, &mut b, &mut la, &mut lanzador_configurado, &mut fuente, &mut medio);
                    } else if zona_panel(b.ancho).contiene(x, y) {
                        let mut e = Escritor::new();
                        e.empezar(TEMA, dashos_tema_v1::req::PANEL).fin();
                        c.mandar(&e);
                        println!("  [barra] pide el panel rápido");
                    } else if zona_campana(b.ancho).contiene(x, y) && avs.activo {
                        if avs.hist.is_some() {
                            cerrar_historial(&mut c, &mut b, &mut avs);
                        } else {
                            cerrar_toast(&mut c, &mut avs, "se abre el historial");
                            abrir_historial(&mut c, &mut b, &mut avs, &mut medio);
                        }
                        pantalla_barra(&mut c, &b, &mut fuente, &mut medio);
                    }
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
                            e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER_HIST).int(OFF_H as i32).int(ANCHO_H).int(h.alto).int(ANCHO_H * 4).uint(0).fin();
                            e.empezar(CAPA_HIST, zwlr_layer_surface_v1::req::SET_SIZE).uint(ANCHO_H as u32).uint(h.alto as u32).fin();
                            c.mandar(&e);
                        }
                        if avs.hist_listo && let Some(h) = &avs.hist {
                            pantalla_historial(&mut c, h, &b.tema, &mut fuente, &mut medio);
                        }
                    }
                } else if sobre == SUP_LANZADOR && b.lanzador_abierto {
                    let k = (0..la.filas()).find(|&k| fila(k).contiene(x, y)).map(|k| k + la.primero);
                    if let Some(k) = k.filter(|&k| k < la.vistos.len()) {
                        la.elegido = k;
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
                        if avs.hist_listo {
                            pantalla_historial(&mut c, h, &b.tema, &mut fuente, &mut medio);
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
