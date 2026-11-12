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
//! Llaves: 1 el canal "ventanas" (con las capas, por su nombre), 2 el canal "archivos" y 3 el canal "sesiones".

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
// Los objetos propios (los de `ventana_cliente` van del 2 al 14).
const CAPAS: u32 = 20;
const CAPA_BARRA: u32 = 21;
const TEMA: u32 = 22;
const SUP_LANZADOR: u32 = 23;
const CAPA_LANZADOR: u32 = 24;
const BUFER_LANZADOR: u32 = 25;
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
/// La memoria: la barra (lo más ancha que se pinta) y, detrás, el lanzador.
const OFF_L: usize = (ANCHO_MAX * ALTO * 4) as usize;
const TAMANO: usize = OFF_L + (ANCHO_L * ALTO_L_MAX * 4) as usize;
const BOTON_IZQUIERDO: u32 = 0x110;
const KEY_ESC: u32 = 1;
/// Lo que se espera a `sesiones` y a archivos, como mucho.
const PLAZO_LANZAR_US: u64 = 5_000_000;
const PLAZO_ARCHIVOS_US: u64 = 1_000_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Boton {
    Lanzador,
    Panel,
}

fn zona_lanzador() -> Rect {
    Rect::new(LADO, (ALTO - BOTON) / 2, BOTON, BOTON)
}

fn zona_panel(ancho: i32) -> Rect {
    Rect::new(ancho - LADO - BOTON, (ALTO - BOTON) / 2, BOTON, BOTON)
}

struct Barra {
    tema: Tema,
    ancho: i32,
    titulo: String,
    /// La hora que se enseña (horas y minutos), si se sabe.
    hora: Option<(u64, u64)>,
    encima: Option<Boton>,
    lanzador_abierto: bool,
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
    // La hora, a la izquierda del botón del panel.
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
    let (mut fuente, mut medio) = (Fuente::regular(), Fuente::medium());
    let Some(mut c) = Cliente::conectar() else {
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

    let mut b = Barra { tema: Tema::CLARO, ancho: 0, titulo: String::new(), hora: hora_ahora(), encima: None, lanzador_abierto: false };
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
        let ev = match c.evento_plazo(hasta_el_minuto()) {
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
            (1, wl_display::ev::ERROR) => {
                println!("  [barra] error de protocolo en el objeto {}", palabra(0));
                return 6;
            }
            _ => {}
        }
    }
}
