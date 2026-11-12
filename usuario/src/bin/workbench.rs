//! S10: el Workbench, primera versión (`docs/diseño/S10-escritorio.md`, "Workbench" y "Aspecto"). Es un cliente del
//! compositor, como la barra: si se cae, las ventanas de los demás siguen.
//!
//! - En el escritorio, los iconos de los volúmenes: una capa de abajo (wlr-layer-shell, capa 1), una columna a la
//!   derecha bajo la barra, transparente salvo los iconos. Un fondo de pantalla (capa 0) queda debajo y se ve. No pide el teclado: un
//!   clic en el escritorio no se lo quita a la ventana que lo tiene. Los iconos van arriba a la derecha.
//! - Doble clic en un volumen o en un cajón abre una ventana de cajón (xdg_toplevel con el kit): iconos en rejilla o
//!   lista con nombre, tamaño y fecha (un botón cambia). Doble clic en un cajón abre otra ventana; en un programa de
//!   `Programas`, lo lanza `sesiones` como la persona que está delante (como la barra).
//! - "Información" (el botón, o el botón derecho en un icono): comentario, bits de protección del Amiga, permisos,
//!   dueño, tamaño y fecha. Se cambian el comentario y los bits si se puede (`COMENTAR` y `PROTEGER`): solo su dueño, y
//!   no por una raíz de solo lectura.
//!
//! Raíces (S6b): la ficha es de Ana (1000) con el derecho de repartir, como el selector. Al empezar se cede a sí mismo
//! el disco solo para leer (raíz 1, "Sistema") y `C` para leer y escribir (raíz 2, "Home"), y solo va por ellas. `Home`
//! es `C` de forma provisional: el `Home:` de verdad es el volumen cifrado de Ana, y abrir un volumen por su nombre
//! pide el derecho de administrar; tendrá que cedérselo `sesiones` al entrar. El icono de `Home` sale solo mientras la
//! sesión de Ana está abierta (`ACTIVA` de `sesiones`).
//!
//! Los nombres que no se pueden enseñar sin dudas (vacíos, ".", "..", con '/', ':', '|' o de control) no salen ni se
//! abren, y todo se busca de un paso, por la asa del cajón y un solo nombre.
//!
//! Llaves: 1 el canal "ventanas" (con las capas, por su nombre), 2 el canal "archivos" y 3 el canal "sesiones".

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
#[allow(dead_code)]
mod ventana_cliente;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use dashos::println;
use dashos::protocolo::archivos as a;
use dashos::protocolo::sesiones as s;
use kit::controles::{Boton, Campo, Contexto, Hijo, Interfaz, Texto, DOBLE_MS};
use kit::grupos::Medida;
use kit::lienzo::{Color, Lienzo, Rect};
use kit::objetos::{atr, Arbol, Atr, Clase, Cuando, Obj, Que, Valor};
use kit::teclado::{Tecla, Teclado};
use kit::tema::{self, con_opacidad, Estado, Tema};
use kit::texto::Fuente;
use ventana_cliente::*;
use wayland::tablas::{
    dashos_tema_v1, wl_buffer, wl_compositor, wl_display, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool,
    wl_surface, xdg_surface, xdg_toplevel, xdg_wm_base, zwlr_layer_shell_v1, zwlr_layer_surface_v1,
};
use wayland::Escritor;

dashos::programa!(principal);
dashos::monton!();

const ARCHIVOS: u32 = 2;
const SESIONES: u32 = 3;
// Los objetos fijos (los de `ventana_cliente` van del 2 al 14); los de cada ventana, desde `PRIMER_ID`, sin repetir.
const CAPAS: u32 = 20;
const TEMA: u32 = 22;
const SUP_ESC: u32 = 30;
const CAPA_ESC: u32 = 31;
const PRIMER_ID: u32 = 100;
/// Dónde se ve la ventana del servicio de archivos, el escritorio (`VA`, de `ventana_cliente`) y las ventanas.
const VA_ARCHIVOS: usize = 0x50_0000_0000;
const VA_VENTANAS: usize = 0x60_0000_0000;
const HUECO_VENTANA: usize = 16 << 20;
const MAX_VENTANAS: usize = 8;
/// Las raíces que se cede (S6b, capa 2).
const RAIZ_SISTEMA: u32 = 1;
const RAIZ_HOME: u32 = 2;
const BOTON_IZQUIERDO: u32 = 0x110;
const BOTON_DERECHO: u32 = 0x111;
const KEY_ESC: u32 = 1;
const PLAZO_ARCHIVOS_US: u64 = 3_000_000;
const PLAZO_SESIONES_US: u64 = 5_000_000;
/// Cada cuánto se mira si la sesión de Ana sigue abierta (el icono de `Home`).
const CADA_ACTIVA_US: u64 = 1_500_000;
/// La ventana de cajón y la de Información (su alto, lo que pida el kit).
const ANCHO_CAJON: i32 = 440;
const ALTO_CAJON: i32 = 420;
const ANCHO_INFO: i32 = 460;
/// La rejilla de iconos de una ventana: celdas de 96 × 92; la lista: cabecera de 24 y filas de 32.
const CELDA_W: i32 = 96;
const CELDA_H: i32 = 92;
const CABECERA_LISTA: i32 = 24;
const FILA_LISTA: i32 = 32;
/// Los iconos del escritorio: celdas de 88 × 100 en una columna a la derecha.
const CELDA_ESC_W: i32 = 88;
const CELDA_ESC_H: i32 = 100;
/// El ancho de la capa del escritorio: la columna de los iconos.
const ANCHO_ESC: i32 = CELDA_ESC_W + 16;

// ---------------------------------------------------------------------------------------------------------------
// Esperar a otros servicios sin dejar de leer al compositor (que suelta a quien no lee).

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

/// Lo que contesta el servicio de archivos.
#[derive(Clone, Copy)]
struct Resp {
    e: i32,
    asa: u32,
    a: u64,
    b: u64,
    c: u64,
}

const FALLO: Resp = Resp { e: -1, asa: 0, a: 0, b: 0, c: 0 };

/// La sesión con el servicio de archivos.
struct Archivos {
    lectura: u32,
}

impl Archivos {
    fn abrir(c: &mut Cliente) -> Option<Archivos> {
        let (lectura, suyo) = dashos::canal_crear();
        let mut p = [0u8; a::CABECERA];
        p[0..4].copy_from_slice(&a::SESION.to_le_bytes());
        let mut r = [0u8; 32];
        let mut ll = [0u32; 1];
        if dashos::canal_escribir(ARCHIVOS, &p, &[suyo]) != 0 {
            return None;
        }
        // La respuesta de la sesión trae una llave: se lee aquí (`esperar` no las recoge).
        let hasta = dashos::ahora_us() + PLAZO_ARCHIVOS_US;
        loop {
            let (n, k, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut ll, 20_000);
            if n == 32 && k == 1 {
                break;
            }
            while c.recoger() {}
            if n != dashos::E_PLAZO || dashos::ahora_us() >= hasta {
                return None;
            }
        }
        if dashos::memoria_proyectar(ll[0], VA_ARCHIVOS, false) != 0 {
            return None;
        }
        dashos::cerrar(ll[0]);
        Some(Archivos { lectura })
    }

    /// Una orden; si iba por una de sus raíces y ya no vale, se las cede otra vez y lo intenta de nuevo: al volver a
    /// una instantánea del volumen (la prueba FS9 de `ficheros`), el servicio suelta las asas y las raíces de todos.
    fn pedir(&self, c: &mut Cliente, orden: u32, asa: u32, x: u64, y: u64, nombre: &[u8]) -> Resp {
        let r = self.pedir_una(c, orden, asa, x, y, nombre);
        if r.e == a::ASA_MALA && (asa == RAIZ_SISTEMA || asa == RAIZ_HOME) {
            let (s, h) = self.ceder_raices(c);
            println!("  [workbench] sus raíces ya no valían (se volvió a una instantánea): Sistema {}, Home {}", s, h);
            return self.pedir_una(c, orden, asa, x, y, nombre);
        }
        r
    }

    /// Se cede a sí mismo sus raíces (S6b, capa 2): el disco solo para leer, como "Sistema", y `C` para leer y escribir,
    /// como "Home". El estado de cada una.
    fn ceder_raices(&self, c: &mut Cliente) -> (i32, i32) {
        let mut ceder = |camino: &[u8], derechos: u64, hueco: u32| {
            let r = self.pedir_una(c, a::BUSCAR, 0, 0, 0, camino);
            if r.e != a::BIEN {
                return r.e;
            }
            let e = self.pedir_una(c, a::CEDER, r.asa, a::YO, derechos | (hueco as u64) << 8, b"").e;
            self.pedir_una(c, a::CERRAR, r.asa, 0, 0, b"");
            e
        };
        (ceder(b"", 4, RAIZ_SISTEMA), ceder(b"C", 4 | 2, RAIZ_HOME))
    }

    fn pedir_una(&self, c: &mut Cliente, orden: u32, asa: u32, x: u64, y: u64, nombre: &[u8]) -> Resp {
        if nombre.len() > 224 {
            return Resp { e: a::MAL_FORMADO, ..FALLO };
        }
        let mut p = [0u8; 256];
        p[0..4].copy_from_slice(&orden.to_le_bytes());
        p[4..8].copy_from_slice(&asa.to_le_bytes());
        p[8..16].copy_from_slice(&x.to_le_bytes());
        p[16..24].copy_from_slice(&y.to_le_bytes());
        p[24..28].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
        p[32..32 + nombre.len()].copy_from_slice(nombre);
        if dashos::canal_escribir(ARCHIVOS, &p[..32 + nombre.len()], &[]) != 0 {
            return FALLO;
        }
        let mut r = [0u8; 32];
        if esperar(c, self.lectura, &mut r, PLAZO_ARCHIVOS_US) != 32 {
            return FALLO;
        }
        let u64_en = |i: usize| u64::from_le_bytes(r[i..i + 8].try_into().unwrap());
        Resp { e: i32::from_le_bytes(r[0..4].try_into().unwrap()), asa: u32::from_le_bytes(r[4..8].try_into().unwrap()), a: u64_en(8), b: u64_en(16), c: u64_en(24) }
    }

    fn ventana(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(VA_ARCHIVOS as *const u8, a::VENTANA) }
    }

    fn cerrar(&self, c: &mut Cliente, asa: u32) {
        // Las raíces no se cierran.
        if asa >= a::RAICES {
            self.pedir(c, a::CERRAR, asa, 0, 0, b"");
        }
    }
}

/// Un nombre que se puede enseñar y buscar sin dudas: ni vacío, ni "." ni "..", sin '/', ':' (separan caminos y
/// volúmenes), sin '|' y sin letras de control.
fn nombre_valido(n: &str) -> bool {
    !n.is_empty() && n != "." && n != ".." && !n.contains(['/', ':', '|']) && !n.chars().any(char::is_control)
}

/// Una entrada de un cajón.
#[derive(Clone)]
struct Entrada {
    nombre: String,
    cajon: bool,
    tam: u64,
    fecha: u64,
    modo: u16,
}

impl Entrada {
    fn programa(&self) -> bool {
        !self.cajon && self.modo & 0o111 != 0
    }
}

/// Lo que hay en el cajón de la asa: los cajones primero y luego los ficheros, por nombre. Sin los nombres raros (los
/// cuenta). El error del servicio si no se puede listar.
fn listar(ar: &Archivos, c: &mut Cliente, asa: u32) -> Result<(Vec<Entrada>, usize), i32> {
    let mut entradas = Vec::new();
    let mut raros = 0;
    let mut desde = 0u64;
    loop {
        let r = ar.pedir(c, a::LISTAR, asa, desde, 0, b"");
        if r.e != a::BIEN {
            return Err(r.e);
        }
        let (cuantos, siguiente, bytes) = (r.a, r.b, r.c);
        let v = &ar.ventana()[..(bytes as usize).min(a::VENTANA)];
        let mut off = 0;
        for _ in 0..cuantos {
            if off + 24 > v.len() {
                break;
            }
            let (tipo, largo, largo_com) = (v[off + 18], v[off + 19] as usize, v[off + 22] as usize);
            let total = 24 + largo + largo_com;
            if off + total > v.len() {
                break;
            }
            if tipo == a::FICHERO || tipo == a::CAJON {
                match core::str::from_utf8(&v[off + 24..off + 24 + largo]) {
                    Ok(n) if nombre_valido(n) => entradas.push(Entrada {
                        nombre: String::from(n),
                        cajon: tipo == a::CAJON,
                        tam: u64::from_le_bytes(v[off..off + 8].try_into().unwrap()),
                        fecha: u64::from_le_bytes(v[off + 8..off + 16].try_into().unwrap()),
                        modo: u16::from_le_bytes([v[off + 16], v[off + 17]]),
                    }),
                    _ => raros += 1,
                }
            }
            off += total.next_multiple_of(8);
        }
        if siguiente == u64::MAX || cuantos == 0 || entradas.len() > 1000 {
            break;
        }
        desde = siguiente;
    }
    entradas.sort_by(|x, y| y.cajon.cmp(&x.cajon).then_with(|| x.nombre.to_lowercase().cmp(&y.nombre.to_lowercase())));
    Ok((entradas, raros))
}

/// Una petición a `sesiones` con su llave de respuesta: los bytes de la respuesta, o un error.
fn a_sesiones(c: &mut Cliente, m: &[u8], r: &mut [u8]) -> i64 {
    let (lectura, suyo) = dashos::canal_crear();
    let n = if dashos::canal_escribir(SESIONES, m, &[suyo]) == 0 { esperar(c, lectura, r, PLAZO_SESIONES_US) } else { -1 };
    dashos::cerrar(lectura);
    n
}

/// Si la sesión de su usuario está abierta (delante o detrás).
fn sesion_abierta(c: &mut Cliente) -> bool {
    let mut r = [0u8; 4];
    a_sesiones(c, &s::ACTIVA.to_le_bytes(), &mut r) == 4 && i32::from_le_bytes(r) >= 0
}

/// Lanza `Programas/<nombre>` por `sesiones`, como la persona que está delante: (estado, proceso).
fn lanzar(c: &mut Cliente, nombre: &str) -> (i32, u64) {
    let camino = alloc::format!("Programas/{}", nombre);
    let mut p = Vec::with_capacity(12 + camino.len());
    p.extend_from_slice(&s::LANZAR.to_le_bytes());
    p.extend_from_slice(&(camino.len() as u32).to_le_bytes());
    p.extend_from_slice(&0u32.to_le_bytes());
    p.extend_from_slice(camino.as_bytes());
    let mut r = [0u8; 12];
    if a_sesiones(c, &p, &mut r) == 12 {
        (i32::from_le_bytes(r[0..4].try_into().unwrap()), u64::from_le_bytes(r[4..12].try_into().unwrap()))
    } else {
        (i32::MIN, 0)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Textos.

fn tamano(b: u64) -> String {
    match b {
        1 => String::from("1 byte"),
        0..1024 => alloc::format!("{} bytes", b),
        1024..1_048_576 => alloc::format!("{} KB", b.div_ceil(1024)),
        _ => alloc::format!("{},{} MB", b / 1_048_576, b % 1_048_576 * 10 / 1_048_576),
    }
}

/// Año, mes y día de un número de días desde el 1 de enero de 1970 (calendario gregoriano, por eras de 400 años).
fn dia_civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

/// La fecha en UTC (aún no hay zona horaria), como "12/11/2026 10:04"; sin fecha, una raya.
fn fecha(seg: u64) -> String {
    if seg == 0 {
        return String::from("—");
    }
    let (y, m, d) = dia_civil((seg / 86_400) as i64);
    let min = seg % 86_400 / 60;
    alloc::format!("{:02}/{:02}/{} {:02}:{:02}", d, m, y, min / 60, min % 60)
}

/// Un texto que cabe en `ancho`; si no, cortado con puntos suspensivos.
fn que_quepa(f: &mut Fuente, texto: &str, tam: u32, ancho: i32) -> String {
    if f.medir(texto, tam).ancho <= ancho {
        return String::from(texto);
    }
    let puntos = if f.tiene('…') { "…" } else { "..." };
    let mut t = String::from(texto);
    while !t.is_empty() {
        t.pop();
        let prueba = alloc::format!("{}{}", t.trim_end(), puntos);
        if f.medir(&prueba, tam).ancho <= ancho {
            return prueba;
        }
    }
    String::new()
}

fn centrado(f: &mut Fuente, l: &mut Lienzo, cx: i32, base: i32, texto: &str, tam: u32, color: Color) {
    let w = f.medir(texto, tam).ancho;
    f.dibujar(l, cx - w / 2, base, texto, tam, color);
}

// ---------------------------------------------------------------------------------------------------------------
// Los iconos, dibujados aquí: formas redondeadas con los tonos del tema (Material 3). Cada uno en una caja de `s`
// (pensados en 48 y escalados).

#[derive(Clone, Copy, PartialEq, Eq)]
enum Forma {
    Sistema,
    Home,
    Cajon,
    Fichero,
    Programa,
}

fn forma_de(e: &Entrada) -> Forma {
    if e.cajon {
        Forma::Cajon
    } else if e.programa() {
        Forma::Programa
    } else {
        Forma::Fichero
    }
}

fn icono(l: &mut Lienzo, x: i32, y: i32, s: i32, forma: Forma, t: &Tema) {
    let k = |v: i32| v * s / 48;
    let fino = (s / 24).max(1);
    match forma {
        // Un disco: cuerpo redondeado, una ranura y una luz.
        Forma::Sistema | Forma::Home => {
            let (fondo, tinta) = if forma == Forma::Sistema {
                (t.contenedor_primario, t.primario)
            } else {
                (t.contenedor_terciario, t.sobre_contenedor_terciario)
            };
            let cuerpo = Rect::new(x + k(3), y + k(11), k(42), k(28));
            l.redondeado(cuerpo, k(10), fondo);
            l.borde(cuerpo, k(10), fino, con_opacidad(tinta, 90));
            l.redondeado(Rect::new(x + k(10), y + k(27), k(18), k(4).max(2)), k(2), tinta);
            l.redondeado(Rect::new(x + k(33), y + k(26), k(6), k(6)), k(3), tinta);
            if forma == Forma::Home {
                // Una casita encima de la luz: el tejado en dos trazos.
                let (cx, cy) = (x + k(19), y + k(19));
                l.trazo(((cx - k(6)) * 8, cy * 8), (cx * 8, (cy - k(5)) * 8), 8 * fino, tinta);
                l.trazo((cx * 8, (cy - k(5)) * 8), ((cx + k(6)) * 8, cy * 8), 8 * fino, tinta);
            }
        }
        // Un cajón: la pestaña y el fondo en el primario, el frente en su contenedor.
        Forma::Cajon => {
            l.redondeado_esquinas(Rect::new(x + k(4), y + k(9), k(18), k(9)), [k(4), k(4), 0, 0], t.primario);
            l.redondeado(Rect::new(x + k(4), y + k(13), k(40), k(28)), k(5), t.primario);
            l.redondeado(Rect::new(x + k(4), y + k(18), k(40), k(23)), k(5), t.contenedor_primario);
        }
        // Una hoja con la esquina doblada y tres renglones.
        Forma::Fichero => {
            let hoja = Rect::new(x + k(10), y + k(4), k(28), k(40));
            l.redondeado(hoja, k(5), t.superficie);
            l.borde(hoja, k(5), fino, t.contorno);
            l.redondeado_esquinas(Rect::new(x + k(28), y + k(4), k(10), k(10)), [0, k(5), k(3), 0], t.variante_contorno);
            for i in 0..3 {
                l.redondeado(Rect::new(x + k(16), y + k(20) + i * k(7), k(16) - i * k(3), fino.max(k(2))), 1, t.variante_contorno);
            }
        }
        // Un programa: un cuadrado redondo en el contenedor terciario con un triángulo de "en marcha".
        Forma::Programa => {
            l.redondeado(Rect::new(x + k(6), y + k(6), k(36), k(36)), k(12), t.contenedor_terciario);
            triangulo(l, x + k(19), y + k(15), k(14), k(18), t.sobre_contenedor_terciario);
        }
    }
}

/// Un triángulo que apunta a la derecha, con la base vertical en `x`, de `w` de ancho y `h` de alto, con el borde
/// inclinado suavizado (en dieciseisavos de píxel).
fn triangulo(l: &mut Lienzo, x: i32, y: i32, w: i32, h: i32, color: Color) {
    let medio = h * 8;
    for fila in 0..h {
        // La distancia del centro de la fila al centro del triángulo, en dieciseisavos.
        let d = (fila * 16 + 8 - medio).abs();
        let ancho16 = (medio - d).max(0) * w * 2 / h;
        let llenos = ancho16 / 16;
        if llenos > 0 {
            l.rellenar(Rect::new(x, y + fila, llenos, 1), color);
        }
        let resto = (ancho16 % 16) as u32;
        if resto > 0 {
            l.punto(x + llenos, y + fila, color, resto * 255 / 16);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Las clases propias para el kit: el hueco del contenido de un cajón (lo pinta el programa) y la rejilla de permisos.

struct Hueco;

impl Clase for Hueco {
    fn nombre(&self) -> &'static str {
        "hueco"
    }

    fn medida(&self, _attrs: &BTreeMap<Atr, Valor>, _ctx: &mut Contexto) -> Medida {
        Medida::elastica(200, 120)
    }
}

/// Los permisos de Unix y los bits del Amiga juntos, en una rejilla de casillas (Material 3): filas Dueño, Grupo y Otros
/// con Leer, Escribir, Ejecutar y No borrar; y una fila más con Guion, Puro, Archivado y Residente. `VALOR` lleva los
/// bits del Amiga en los bits 16 y siguientes y el modo (12 bits) abajo. Al pulsar se apunta la casilla en `CURSOR` y
/// al soltar dentro se cambia.
struct Permisos;

const PERM_ETIQUETA: i32 = 76;
const PERM_COL: i32 = 80;
const PERM_CAB: i32 = 24;
const PERM_FILA: i32 = 40;
const COLUMNAS: [&str; 4] = ["Leer", "Escribir", "Ejecutar", "No borrar"];
const FILAS: [&str; 3] = ["Dueño", "Grupo", "Otros"];
const COLUMNAS_AMIGA: [&str; 4] = ["Guion", "Puro", "Archivado", "Residente"];

impl Permisos {
    /// El bit de la casilla (fila 0 a 3, columna 0 a 3) en el valor junto.
    fn bit(fila: usize, col: usize) -> i64 {
        match (fila, col) {
            (0..=2, 0..=2) => 0o400 >> (3 * fila + col),
            (0..=2, 3) => (16 << fila) << 16,
            (3, _) => ([4, 2, 1, 8][col]) << 16,
            _ => 0,
        }
    }

    /// El centro de la casilla, dentro de `r`.
    fn centro(r: Rect, fila: usize, col: usize) -> (i32, i32) {
        let y = if fila < 3 { PERM_CAB + fila as i32 * PERM_FILA } else { 2 * PERM_CAB + 3 * PERM_FILA };
        (r.x + PERM_ETIQUETA + col as i32 * PERM_COL + PERM_COL / 2, r.y + y + PERM_FILA / 2)
    }

    fn casilla_en(r: Rect, x: i32, y: i32) -> i64 {
        for fila in 0..4 {
            for col in 0..4 {
                let (cx, cy) = Self::centro(r, fila, col);
                if (x - cx).abs() <= 20 && (y - cy).abs() <= 20 {
                    return (fila * 4 + col) as i64;
                }
            }
        }
        -1
    }
}

impl Clase for Permisos {
    fn nombre(&self) -> &'static str {
        "permisos"
    }

    fn medida(&self, _attrs: &BTreeMap<Atr, Valor>, _ctx: &mut Contexto) -> Medida {
        Medida::fija(PERM_ETIQUETA + 4 * PERM_COL, 2 * PERM_CAB + 4 * PERM_FILA)
    }

    fn dibujar(&self, attrs: &BTreeMap<Atr, Valor>, l: &mut Lienzo, r: Rect, ctx: &mut Contexto, e: Estado) {
        let t = ctx.tema;
        let valor = attrs.get(&atr::VALOR).map_or(0, Valor::int);
        let pulsada = if e.pulsado { attrs.get(&atr::CURSOR).map_or(-1, Valor::int) } else { -1 };
        let suave = if e.desactivado { tema::apagado(t.sobre_superficie) } else { t.sobre_variante_superficie };
        for (cab, nombres) in [(0, COLUMNAS), (PERM_CAB + 3 * PERM_FILA, COLUMNAS_AMIGA)] {
            for (c, n) in nombres.iter().enumerate() {
                let cx = r.x + PERM_ETIQUETA + c as i32 * PERM_COL + PERM_COL / 2;
                centrado(&mut ctx.medium, l, cx, r.y + cab + 16, n, 12, suave);
            }
        }
        let tinta = if e.desactivado { tema::apagado(t.sobre_superficie) } else { t.sobre_superficie };
        for (fila, n) in FILAS.iter().chain(["Amiga"].iter()).enumerate() {
            let (_, cy) = Self::centro(r, fila, 0);
            ctx.regular.dibujar(l, r.x, cy + 5, n, 14, tinta);
            for col in 0..4 {
                let (cx, cy) = Self::centro(r, fila, col);
                let marcada = valor & Self::bit(fila, col) != 0;
                let caja = Rect::new(cx - 9, cy - 9, 18, 18);
                if pulsada == (fila * 4 + col) as i64 {
                    l.redondeado(Rect::new(cx - 20, cy - 20, 40, 40), 20, con_opacidad(if marcada { t.sobre_superficie } else { t.primario }, 26));
                }
                if marcada {
                    l.redondeado(caja, 2, if e.desactivado { tema::apagado(t.sobre_superficie) } else { t.primario });
                    let marca = if e.desactivado { t.superficie } else { t.sobre_primario };
                    let p = |x: i32, y: i32| (caja.x * 8 + x, caja.y * 8 + y);
                    l.trazo(p(30, 74), p(58, 102), 16, marca);
                    l.trazo(p(58, 102), p(115, 45), 16, marca);
                } else {
                    l.borde(caja, 2, 2, if e.desactivado { tema::apagado(t.sobre_superficie) } else { t.sobre_variante_superficie });
                }
            }
        }
    }

    fn pulsable(&self) -> bool {
        true
    }

    fn arrastrar(&self, _attrs: &BTreeMap<Atr, Valor>, r: Rect, x: i32, y: i32) -> Vec<(Atr, Valor)> {
        alloc::vec![(atr::CURSOR, Valor::Int(Self::casilla_en(r, x, y)))]
    }

    fn clic(&self, attrs: &BTreeMap<Atr, Valor>) -> Vec<(Atr, Valor)> {
        let k = attrs.get(&atr::CURSOR).map_or(-1, Valor::int);
        if !(0..16).contains(&k) {
            return Vec::new();
        }
        let v = attrs.get(&atr::VALOR).map_or(0, Valor::int);
        alloc::vec![(atr::VALOR, Valor::Int(v ^ Self::bit(k as usize / 4, k as usize % 4)))]
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Las ventanas.

/// Dónde está un cajón: por qué raíz y por qué nombres.
#[derive(Clone, PartialEq, Eq)]
struct Lugar {
    raiz: u32,
    nombres: Vec<String>,
}

impl Lugar {
    fn texto(&self) -> String {
        let mut t = String::from(if self.raiz == RAIZ_SISTEMA { "Sistema" } else { "Home" });
        for n in &self.nombres {
            t.push('/');
            t.push_str(n);
        }
        t
    }

    fn de_programas(&self) -> bool {
        self.raiz == RAIZ_SISTEMA && self.nombres.len() == 1 && self.nombres[0] == "Programas"
    }
}

struct Cajon {
    lugar: Lugar,
    asa: u32,
    entradas: Vec<Entrada>,
    raros: usize,
    lista: bool,
    elegido: Option<usize>,
    /// La primera entrada que se ve.
    primera: usize,
    /// El último clic en una entrada (cuál y cuándo): para el doble clic.
    ultimo: Option<(usize, u32)>,
    estado: Obj,
    vista: Obj,
    info: Obj,
    cerrar: Obj,
    hueco: Obj,
}

struct Info {
    ruta: String,
    asa: u32,
    /// Si la asa es suya (se cierra con la ventana); las raíces y la de un cajón abierto, no.
    cerrar_asa: bool,
    /// El cajón donde está (para poner al día sus ventanas tras guardar).
    padre: Option<Lugar>,
    comentario: String,
    valor: i64,
    puede: bool,
    campo: Obj,
    rejilla: Obj,
    aviso: Obj,
}

enum Que_ {
    Cajon(Cajon),
    Info(Info),
}

struct Ventana {
    /// El número de ventana desde que empezó (para las pruebas: la n-ésima que coloca el compositor).
    numero: u32,
    slot: usize,
    sup: u32,
    xsup: u32,
    top: u32,
    pool: u32,
    bufer: u32,
    mem: u32,
    w: i32,
    h: i32,
    configurada: bool,
    i: Interfaz,
    marca: Obj,
    que: Que_,
}

impl Ventana {
    fn va(&self) -> usize {
        VA_VENTANAS + self.slot * HUECO_VENTANA
    }
}

/// Las celdas que se ven de un cajón (índice de entrada y su rectángulo), en rejilla o en lista.
fn celdas(r: Rect, cj: &Cajon) -> Vec<(usize, Rect)> {
    let n = cj.entradas.len();
    let mut v = Vec::new();
    if cj.lista {
        let filas = ((r.h - CABECERA_LISTA) / FILA_LISTA).max(1) as usize;
        for (k, i) in (cj.primera..n).take(filas).enumerate() {
            v.push((i, Rect::new(r.x + 4, r.y + CABECERA_LISTA + k as i32 * FILA_LISTA, r.w - 8, FILA_LISTA)));
        }
    } else {
        let cols = (r.w / CELDA_W).max(1) as usize;
        let filas = (r.h / CELDA_H).max(1) as usize;
        let ox = r.x + (r.w - cols as i32 * CELDA_W) / 2;
        for (k, i) in (cj.primera..n).take(cols * filas).enumerate() {
            let (f, c) = (k / cols, k % cols);
            v.push((i, Rect::new(ox + c as i32 * CELDA_W, r.y + 4 + f as i32 * CELDA_H, CELDA_W, CELDA_H)));
        }
    }
    v
}

/// Cuántas entradas caben a la vez y de cuántas en cuántas se baja.
fn caben(r: Rect, cj: &Cajon) -> (usize, usize) {
    if cj.lista {
        (((r.h - CABECERA_LISTA) / FILA_LISTA).max(1) as usize, 1)
    } else {
        let cols = (r.w / CELDA_W).max(1) as usize;
        (cols * (r.h / CELDA_H).max(1) as usize, cols)
    }
}

fn pintar_contenido(l: &mut Lienzo, r: Rect, cj: &Cajon, t: &Tema, f: &mut Fuente, m: &mut Fuente) {
    l.redondeado(r, 12, t.contenedor_bajo());
    l.recortar(r);
    if cj.entradas.is_empty() {
        centrado(f, l, r.x + r.w / 2, r.y + r.h / 2, "Vacío", 14, t.sobre_variante_superficie);
    }
    let x_tam = r.x + r.w - 140;
    let x_fecha = r.x + r.w - 124;
    if cj.lista {
        m.dibujar(l, r.x + 48, r.y + 17, "Nombre", 12, t.sobre_variante_superficie);
        let w = m.medir("Tamaño", 12).ancho;
        m.dibujar(l, x_tam - w, r.y + 17, "Tamaño", 12, t.sobre_variante_superficie);
        m.dibujar(l, x_fecha, r.y + 17, "Fecha", 12, t.sobre_variante_superficie);
    }
    for (i, c) in celdas(r, cj) {
        let e = &cj.entradas[i];
        let elegida = cj.elegido == Some(i);
        if cj.lista {
            if elegida {
                l.redondeado(c, 8, t.contenedor_secundario());
            }
            icono(l, c.x + 10, c.y + 4, 24, forma_de(e), t);
            let tinta = if elegida { t.sobre_contenedor_secundario() } else { t.sobre_superficie };
            let nombre = que_quepa(f, &e.nombre, 14, x_tam - 80 - (c.x + 44));
            f.dibujar(l, c.x + 44, c.y + 21, &nombre, 14, tinta);
            if !e.cajon {
                let tt = tamano(e.tam);
                let w = f.medir(&tt, 13).ancho;
                f.dibujar(l, x_tam - w, c.y + 21, &tt, 13, t.sobre_variante_superficie);
            }
            f.dibujar(l, x_fecha, c.y + 21, &fecha(e.fecha), 13, t.sobre_variante_superficie);
        } else {
            let cx = c.x + c.w / 2;
            if elegida {
                l.redondeado(Rect::new(cx - 30, c.y + 2, 60, 58), 16, t.contenedor_secundario());
            }
            icono(l, cx - 24, c.y + 7, 48, forma_de(e), t);
            let nombre = que_quepa(f, &e.nombre, 13, c.w - 12);
            if elegida {
                let w = f.medir(&nombre, 13).ancho;
                l.redondeado(Rect::new(cx - w / 2 - 8, c.y + 63, w + 16, 22), 11, t.primario);
                centrado(f, l, cx, c.y + 78, &nombre, 13, t.sobre_primario);
            } else {
                centrado(f, l, cx, c.y + 78, &nombre, 13, t.sobre_superficie);
            }
        }
    }
    l.soltar_recorte();
}

// ---------------------------------------------------------------------------------------------------------------
// El escritorio.

struct Icono {
    nombre: &'static str,
    raiz: u32,
    r: Rect,
}

struct Escritorio {
    w: i32,
    h: i32,
    mem: u32,
    pool: u32,
    bufer: u32,
    iconos: Vec<Icono>,
    elegido: Option<usize>,
    encima: Option<usize>,
    ultimo: Option<(usize, u32)>,
    home: bool,
}

impl Escritorio {
    fn colocar(&mut self) {
        let mut v = alloc::vec![Icono { nombre: "Sistema", raiz: RAIZ_SISTEMA, r: Rect::default() }];
        if self.home {
            v.push(Icono { nombre: "Home", raiz: RAIZ_HOME, r: Rect::default() });
        }
        for (k, ic) in v.iter_mut().enumerate() {
            ic.r = Rect::new(self.w - 8 - CELDA_ESC_W, 12 + k as i32 * CELDA_ESC_H, CELDA_ESC_W, CELDA_ESC_H);
        }
        self.iconos = v;
        if self.elegido.is_some_and(|e| e >= self.iconos.len()) {
            self.elegido = None;
        }
        self.encima = None;
    }

    fn bajo(&self, x: i32, y: i32) -> Option<usize> {
        self.iconos.iter().position(|ic| ic.r.contiene(x, y))
    }

    fn pintar(&self, px: &mut [u32], t: &Tema, m: &mut Fuente) {
        px.fill(0);
        let mut l = Lienzo::new(px, self.w, self.h, self.w as usize);
        for (k, ic) in self.iconos.iter().enumerate() {
            let cx = ic.r.x + ic.r.w / 2;
            let elegido = self.elegido == Some(k);
            // Lo elegido, sobre el contenedor secundario; bajo el ratón, una capa de la superficie.
            if elegido {
                l.redondeado(Rect::new(cx - 36, ic.r.y + 2, 72, 68), 20, con_opacidad(t.contenedor_secundario(), 235));
            } else if self.encima == Some(k) {
                l.redondeado(Rect::new(cx - 36, ic.r.y + 2, 72, 68), 20, con_opacidad(t.superficie, 110));
            }
            icono(&mut l, cx - 28, ic.r.y + 7, 56, if ic.raiz == RAIZ_SISTEMA { Forma::Sistema } else { Forma::Home }, t);
            // El nombre sobre una pastilla, para que se lea sobre cualquier fondo.
            let w = m.medir(ic.nombre, 13).ancho;
            let pastilla = Rect::new(cx - w / 2 - 8, ic.r.y + 72, w + 16, 22);
            let (fondo, tinta) = if elegido { (t.primario, t.sobre_primario) } else { (con_opacidad(t.superficie, 220), t.sobre_superficie) };
            l.redondeado(pastilla, 11, fondo);
            m.dibujar(&mut l, cx - w / 2, ic.r.y + 87, ic.nombre, 13, tinta);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// El programa.

struct App {
    c: Cliente,
    ar: Archivos,
    tema: Tema,
    medio: Fuente,
    esc: Escritorio,
    esc_configurado: bool,
    ventanas: Vec<Ventana>,
    siguiente_id: u32,
    abiertas: u32,
    /// Sobre qué superficie está el puntero, dónde, y si el botón izquierdo está pulsado.
    sobre: u32,
    x: i32,
    y: i32,
    pulsado: bool,
    /// La superficie con el teclado.
    con_teclado: u32,
    teclado: Teclado,
    /// Los contextos del kit (con sus fuentes ya leídas) de las ventanas que se cerraron, para las siguientes.
    contextos: Vec<Contexto>,
}

fn texto_obj(arbol: &mut Arbol, t: &str, estilo: i64) -> Obj {
    arbol.crear(Texto, &[(atr::TEXTO, Valor::Texto(String::from(t))), (atr::ESTILO, Valor::Int(estilo))])
}

impl App {
    /// Una interfaz del kit, con el contexto de una ventana cerrada si hay (leer las fuentes otra vez pide unos megas).
    fn interfaz(&mut self, arbol: Arbol, raiz: Hijo) -> Interfaz {
        match self.contextos.pop() {
            Some(mut ctx) => {
                ctx.tema = self.tema;
                Interfaz::con_contexto(arbol, raiz, ctx)
            }
            None => Interfaz::new(arbol, raiz, self.tema),
        }
    }

    fn ids(&mut self, n: u32) -> u32 {
        let p = self.siguiente_id;
        self.siguiente_id += n;
        p
    }

    // --- El escritorio ---

    fn crear_escritorio(&mut self, gc: u32, gs: u32, gx: u32, ga: u32, gl: u32, gt: u32) {
        let mut e = Escritor::new();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gx).texto(Some("xdg_wm_base")).uint(6).uint(XDG).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gl).texto(Some("zwlr_layer_shell_v1")).uint(4).uint(CAPAS).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gt).texto(Some("dashos_tema_v1")).uint(1).uint(TEMA).fin();
        e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
        e.empezar(ASIENTO, wl_seat::req::GET_KEYBOARD).uint(TECLADO).fin();
        // La capa de abajo (1), una columna pegada a la derecha (arriba, abajo y derecha), sin zona: el compositor la
        // pone en el hueco que dejan las barras. Solo la columna de los iconos, no la pantalla entera: es ARGB y lo que
        // queda debajo de ella se mezcla en cada composición (con el PC cargado, la prueba de virt no llegaba al final).
        // Sin teclado.
        e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUP_ESC).fin();
        e.empezar(CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(CAPA_ESC).uint(SUP_ESC).uint(0).uint(1).texto(Some("workbench")).fin();
        e.empezar(CAPA_ESC, zwlr_layer_surface_v1::req::SET_SIZE).uint(ANCHO_ESC as u32).uint(0).fin();
        e.empezar(CAPA_ESC, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(1 | 2 | 8).fin();
        e.empezar(CAPA_ESC, zwlr_layer_surface_v1::req::SET_EXCLUSIVE_ZONE).int(0).fin();
        e.empezar(CAPA_ESC, zwlr_layer_surface_v1::req::SET_KEYBOARD_INTERACTIVITY).uint(0).fin();
        e.empezar(SUP_ESC, wl_surface::req::COMMIT).fin();
        self.c.mandar(&e);
    }

    /// El tamaño que da el compositor a la capa: memoria y búfer nuevos si cambia.
    fn escritorio_configurado(&mut self, serie: u32, w: i32, h: i32) {
        let mut e = Escritor::new();
        e.empezar(CAPA_ESC, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(serie).fin();
        let (w, h) = (w.clamp(1, 3840), h.clamp(1, 2400));
        if (w, h) != (self.esc.w, self.esc.h) {
            if self.esc.mem != 0 {
                e.empezar(self.esc.bufer, wl_buffer::req::DESTROY).fin();
                e.empezar(self.esc.pool, wl_shm_pool::req::DESTROY).fin();
                dashos::memoria_quitar(VA);
                dashos::cerrar(self.esc.mem);
                self.esc.mem = 0;
            }
            let tam = (w * h * 4) as usize;
            let m = dashos::memoria_crear(tam);
            if (m as i32) <= 0 || dashos::memoria_proyectar(m, VA, true) != 0 {
                println!("  [workbench] sin memoria para el escritorio ({}x{})", w, h);
                self.c.mandar(&e);
                return;
            }
            let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::TRANSFERIR, 0);
            let (pool, bufer) = (self.ids(1), self.ids(1));
            e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(pool).llave(copia as u32).int(tam as i32).fin();
            e.empezar(pool, wl_shm_pool::req::CREATE_BUFFER).uint(bufer).int(0).int(w).int(h).int(w * 4).uint(wayland::shm::ARGB8888).fin();
            self.esc.mem = m;
            self.esc.pool = pool;
            self.esc.bufer = bufer;
            self.esc.w = w;
            self.esc.h = h;
            self.esc.colocar();
        }
        self.c.mandar(&e);
        self.pintar_escritorio();
        if !self.esc_configurado {
            self.esc_configurado = true;
            let s = &self.esc.iconos[0].r;
            println!("  [workbench] en pantalla: {}x{}, icono de Sistema en ({}, {})", w, h, s.x + s.w / 2, s.y + 36);
            self.avisar_home();
        }
    }

    fn avisar_home(&self) {
        match self.esc.iconos.get(1) {
            Some(ic) => println!("  [workbench] icono de Home en ({}, {})", ic.r.x + ic.r.w / 2, ic.r.y + 36),
            None => println!("  [workbench] sin icono de Home: la sesión de Ana no está abierta"),
        }
    }

    fn pintar_escritorio(&mut self) {
        if self.esc.mem == 0 {
            return;
        }
        let px = unsafe { core::slice::from_raw_parts_mut(VA as *mut u32, (self.esc.w * self.esc.h) as usize) };
        self.esc.pintar(px, &self.tema, &mut self.medio);
        let mut e = Escritor::new();
        e.empezar(SUP_ESC, wl_surface::req::ATTACH).uint(self.esc.bufer).int(0).int(0).fin();
        e.empezar(SUP_ESC, wl_surface::req::DAMAGE).int(0).int(0).int(self.esc.w).int(self.esc.h).fin();
        e.empezar(SUP_ESC, wl_surface::req::COMMIT).fin();
        self.c.mandar(&e);
    }

    /// Mira si la sesión de Ana está abierta: con ella, el icono de `Home`.
    fn mirar_home(&mut self) {
        let abierta = sesion_abierta(&mut self.c);
        if abierta != self.esc.home {
            self.esc.home = abierta;
            self.esc.colocar();
            if self.esc_configurado {
                self.pintar_escritorio();
                self.avisar_home();
            }
        }
    }

    fn raton_escritorio(&mut self, boton: u32, pulsado: bool, ms: u32) {
        if !pulsado {
            return;
        }
        let bajo = self.esc.bajo(self.x, self.y);
        let antes = self.esc.elegido;
        self.esc.elegido = bajo;
        match (bajo, boton) {
            (Some(k), BOTON_IZQUIERDO) => {
                let doble = matches!(self.esc.ultimo, Some((u, t)) if u == k && ms.wrapping_sub(t) <= DOBLE_MS);
                self.esc.ultimo = if doble { None } else { Some((k, ms)) };
                if doble {
                    let raiz = self.esc.iconos[k].raiz;
                    self.pintar_escritorio();
                    self.abrir_volumen(raiz);
                    return;
                }
            }
            (Some(k), BOTON_DERECHO) => {
                let (raiz, nombre) = (self.esc.iconos[k].raiz, self.esc.iconos[k].nombre);
                self.pintar_escritorio();
                self.abrir_info(raiz, false, String::from(nombre), None, true);
                return;
            }
            _ => self.esc.ultimo = None,
        }
        if self.esc.elegido != antes {
            self.pintar_escritorio();
        }
    }

    // --- Las ventanas ---

    /// Crea una ventana de `w` × `h` con la interfaz `i`: memoria, búfer y xdg_toplevel. Se pinta al configurarse.
    fn nueva_ventana(&mut self, titulo: &str, w: i32, h: i32, i: Interfaz, marca: Obj, que: Que_) -> bool {
        let Some(slot) = (0..MAX_VENTANAS).find(|s| !self.ventanas.iter().any(|v| v.slot == *s)) else {
            println!("  [workbench] ya hay {} ventanas abiertas", MAX_VENTANAS);
            return false;
        };
        let tam = (w * h * 4) as usize;
        let m = dashos::memoria_crear(tam);
        let va = VA_VENTANAS + slot * HUECO_VENTANA;
        if (m as i32) <= 0 || dashos::memoria_proyectar(m, va, true) != 0 {
            println!("  [workbench] sin memoria para otra ventana");
            return false;
        }
        let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::TRANSFERIR, 0);
        let b = self.ids(5);
        let (sup, xsup, top, pool, bufer) = (b, b + 1, b + 2, b + 3, b + 4);
        let mut e = Escritor::new();
        e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(pool).llave(copia as u32).int(tam as i32).fin();
        e.empezar(pool, wl_shm_pool::req::CREATE_BUFFER).uint(bufer).int(0).int(w).int(h).int(w * 4).uint(wayland::shm::ARGB8888).fin();
        e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(sup).fin();
        e.empezar(XDG, xdg_wm_base::req::GET_XDG_SURFACE).uint(xsup).uint(sup).fin();
        e.empezar(xsup, xdg_surface::req::GET_TOPLEVEL).uint(top).fin();
        e.empezar(top, xdg_toplevel::req::SET_TITLE).texto(Some(titulo)).fin();
        e.empezar(sup, wl_surface::req::COMMIT).fin();
        self.c.mandar(&e);
        self.abiertas += 1;
        self.ventanas.push(Ventana { numero: self.abiertas, slot, sup, xsup, top, pool, bufer, mem: m, w, h, configurada: false, i, marca, que });
        true
    }

    fn presentar(&mut self, k: usize, adjuntar: bool) {
        let v = &mut self.ventanas[k];
        let px = unsafe { core::slice::from_raw_parts_mut(v.va() as *mut u32, (v.w * v.h) as usize) };
        let mut l = Lienzo::new(px, v.w, v.h, v.w as usize);
        v.i.dibujar(&mut l);
        if let Que_::Cajon(cj) = &v.que
            && let Some(&r) = v.i.sitios.get(&cj.hueco)
        {
            let t = v.i.ctx.tema;
            let ctx = &mut v.i.ctx;
            pintar_contenido(&mut l, r, cj, &t, &mut ctx.regular, &mut ctx.medium);
        }
        let mut e = Escritor::new();
        if adjuntar {
            e.empezar(v.sup, wl_surface::req::ATTACH).uint(v.bufer).int(0).int(0).fin();
        }
        e.empezar(v.sup, wl_surface::req::DAMAGE).int(0).int(0).int(v.w).int(v.h).fin();
        e.empezar(v.sup, wl_surface::req::COMMIT).fin();
        self.c.mandar(&e);
    }

    fn cerrar_ventana(&mut self, k: usize) {
        let v = self.ventanas.remove(k);
        let mut e = Escritor::new();
        e.empezar(v.top, xdg_toplevel::req::DESTROY).fin();
        e.empezar(v.xsup, xdg_surface::req::DESTROY).fin();
        e.empezar(v.sup, wl_surface::req::DESTROY).fin();
        e.empezar(v.bufer, wl_buffer::req::DESTROY).fin();
        e.empezar(v.pool, wl_shm_pool::req::DESTROY).fin();
        self.c.mandar(&e);
        dashos::memoria_quitar(v.va());
        dashos::cerrar(v.mem);
        let (asa, cerrar) = match &v.que {
            Que_::Cajon(cj) => (cj.asa, true),
            Que_::Info(inf) => (inf.asa, inf.cerrar_asa),
        };
        if cerrar {
            self.ar.cerrar(&mut self.c, asa);
        }
        if self.sobre == v.sup {
            self.sobre = 0;
        }
        println!("  [workbench] cierra la ventana {}", v.numero);
        if self.ventanas.is_empty() {
            println!("  [workbench] sin ventanas abiertas");
        }
        if self.contextos.len() < MAX_VENTANAS {
            self.contextos.push(v.i.ctx);
        }
    }

    /// Abre la ventana de un volumen (su raíz).
    fn abrir_volumen(&mut self, raiz: u32) {
        let lugar = Lugar { raiz, nombres: Vec::new() };
        match listar(&self.ar, &mut self.c, raiz) {
            Ok((ent, raros)) => {
                self.ventana_cajon(lugar, raiz, ent, raros);
            }
            Err(e) => println!("  [workbench] no abre {}: {}", lugar.texto(), e),
        }
    }

    fn ventana_cajon(&mut self, lugar: Lugar, asa: u32, entradas: Vec<Entrada>, raros: usize) {
        let mut arbol = Arbol::new();
        let texto = lugar.texto();
        // El título: el camino, con lo de delante cortado si es largo.
        let corto = if texto.chars().count() > 28 { alloc::format!("…{}", &texto[texto.len() - texto.chars().rev().take(27).map(char::len_utf8).sum::<usize>()..]) } else { texto.clone() };
        let titulo = texto_obj(&mut arbol, &corto, 1);
        let cerrar = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto("Cerrar".into())), (atr::ESTILO, Valor::Int(2))]);
        let estado = texto_obj(&mut arbol, "", 2);
        let vista = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto("Lista".into())), (atr::ESTILO, Valor::Int(1))]);
        let info = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto("Información".into())), (atr::ESTILO, Valor::Int(1))]);
        let hueco = arbol.crear(Hueco, &[]);
        let marca = texto_obj(&mut arbol, "", 0);
        for (o, que) in [(vista, "vista"), (info, "info"), (cerrar, "cerrar")] {
            arbol.notificar(o, atr::CLIC, Cuando::Igual(Valor::Bool(true)), marca, atr::TEXTO, Que::Fijo(Valor::Texto(que.into())));
        }
        let raiz = Hijo::Columna(alloc::vec![
            Hijo::Fila(alloc::vec![Hijo::Obj(titulo), Hijo::Obj(cerrar)]),
            Hijo::Fila(alloc::vec![Hijo::Obj(estado), Hijo::Obj(vista), Hijo::Obj(info)]),
            Hijo::Obj(hueco),
        ]);
        let mut i = self.interfaz(arbol, raiz);
        i.esquinas = 16;
        i.colocar(Rect::new(0, 0, ANCHO_CAJON, ALTO_CAJON));
        let cj = Cajon { lugar, asa, entradas, raros, lista: false, elegido: None, primera: 0, ultimo: None, estado, vista, info, cerrar, hueco };
        let mut cj = cj;
        Self::poner_estado(&mut i, &mut cj, None);
        if !self.nueva_ventana(&texto, ANCHO_CAJON, ALTO_CAJON, i, marca, Que_::Cajon(cj)) {
            self.ar.cerrar(&mut self.c, asa);
        }
    }

    /// La línea de estado de un cajón: cuántos hay, o un aviso.
    fn poner_estado(i: &mut Interfaz, cj: &mut Cajon, aviso: Option<&str>) {
        let t = match aviso {
            Some(a) => String::from(a),
            None if cj.entradas.len() == 1 => String::from("1 elemento"),
            None => alloc::format!("{} elementos", cj.entradas.len()),
        };
        i.arbol.poner(cj.estado, atr::TEXTO, Valor::Texto(t));
    }

    /// Dice dónde quedó cada cosa de una ventana de cajón (para las pruebas, que pulsan ahí).
    fn decir_sitios(&self, k: usize) {
        let v = &self.ventanas[k];
        let Que_::Cajon(cj) = &v.que else { return };
        let centro = |o: Obj| v.i.sitios.get(&o).map_or((0, 0), |r| (r.x + r.w / 2, r.y + r.h / 2));
        let (vx, vy) = centro(cj.vista);
        let (ix, iy) = centro(cj.info);
        let (cx, cy) = centro(cj.cerrar);
        println!("  [workbench] ventana {}: Lista o Iconos en ({}, {}), Información en ({}, {}), Cerrar en ({}, {})", v.numero, vx, vy, ix, iy, cx, cy);
        if let Some(&r) = v.i.sitios.get(&cj.hueco) {
            let sitios: Vec<String> = celdas(r, cj)
                .iter()
                .map(|(i, c)| {
                    let y = if cj.lista { c.y + c.h / 2 } else { c.y + 32 };
                    alloc::format!("{} en ({}, {})", cj.entradas[*i].nombre, c.x + c.w / 2, y)
                })
                .collect();
            println!("  [workbench] ventana {}, {}: {}", v.numero, if cj.lista { "lista" } else { "iconos" }, sitios.join("; "));
        }
    }

    /// Abre la entrada `i` del cajón de la ventana `k`: otro cajón (otra ventana), un programa de `Programas` (lo lanza
    /// `sesiones`) o nada.
    fn abrir_entrada(&mut self, k: usize, i: usize) {
        let (lugar, asa, e) = match &self.ventanas[k].que {
            Que_::Cajon(cj) => match cj.entradas.get(i) {
                Some(e) => (cj.lugar.clone(), cj.asa, e.clone()),
                None => return,
            },
            Que_::Info(_) => return,
        };
        let ruta = alloc::format!("{}/{}", lugar.texto(), e.nombre);
        if !nombre_valido(&e.nombre) {
            return;
        }
        if !e.cajon {
            let aviso = if e.programa() && lugar.de_programas() {
                let (estado, pid) = lanzar(&mut self.c, &e.nombre);
                println!("  [workbench] lanza Programas/{}: {} (proceso {})", e.nombre, estado, pid);
                if estado == 0 { alloc::format!("En marcha: {}", e.nombre) } else { alloc::format!("No se lanzó ({})", estado) }
            } else {
                println!("  [workbench] {} no se abre: solo se lanzan los programas de Sistema/Programas", ruta);
                String::from("Solo se lanzan los de Programas")
            };
            self.aviso_cajon(k, &aviso);
            return;
        }
        // Un paso, por la asa del cajón y el nombre; y se mira que siga siendo un cajón.
        let r = self.ar.pedir(&mut self.c, a::BUSCAR, asa, 0, 0, e.nombre.as_bytes());
        let tipo = (r.c >> 16) as u8;
        let res = if r.e != a::BIEN {
            Err(r.e)
        } else if tipo != a::CAJON {
            self.ar.cerrar(&mut self.c, r.asa);
            Err(a::NO_ES_CAJON)
        } else {
            match listar(&self.ar, &mut self.c, r.asa) {
                Ok(x) => Ok(x),
                Err(err) => {
                    self.ar.cerrar(&mut self.c, r.asa);
                    Err(err)
                }
            }
        };
        match res {
            Ok((ent, raros)) => {
                let mut nombres = lugar.nombres.clone();
                nombres.push(e.nombre.clone());
                self.ventana_cajon(Lugar { raiz: lugar.raiz, nombres }, r.asa, ent, raros);
            }
            Err(err) => {
                println!("  [workbench] no abre {}: {}", ruta, err);
                let aviso = if err == a::PROTEGIDO { alloc::format!("Sin permiso para abrir {}", e.nombre) } else { alloc::format!("No se abre {} ({})", e.nombre, err) };
                self.aviso_cajon(k, &aviso);
            }
        }
    }

    fn aviso_cajon(&mut self, k: usize, aviso: &str) {
        let v = &mut self.ventanas[k];
        if let Que_::Cajon(cj) = &mut v.que {
            Self::poner_estado(&mut v.i, cj, Some(aviso));
        }
        if v.configurada {
            self.presentar(k, false);
        }
    }

    fn cambiar_vista(&mut self, k: usize) {
        let v = &mut self.ventanas[k];
        let Que_::Cajon(cj) = &mut v.que else { return };
        cj.lista = !cj.lista;
        cj.primera = 0;
        cj.ultimo = None;
        v.i.arbol.poner(cj.vista, atr::TEXTO, Valor::Texto(String::from(if cj.lista { "Iconos" } else { "Lista" })));
        // El botón cambia de ancho: se vuelve a colocar.
        v.i.colocar(Rect::new(0, 0, v.w, v.h));
        let detalle: Vec<String> = cj.entradas.iter().map(|e| {
            if e.cajon { alloc::format!("{}/ {}", e.nombre, fecha(e.fecha)) } else { alloc::format!("{} {} {}", e.nombre, tamano(e.tam), fecha(e.fecha)) }
        }).collect();
        println!("  [workbench] {} como {}: {}", cj.lugar.texto(), if cj.lista { "lista" } else { "iconos" }, detalle.join("; "));
        if v.configurada {
            self.presentar(k, false);
        }
        self.decir_sitios(k);
    }

    /// Pone al día las ventanas de un cajón (tras guardar algo en él).
    fn releer(&mut self, lugar: &Lugar) {
        for k in 0..self.ventanas.len() {
            let asa = match &self.ventanas[k].que {
                Que_::Cajon(cj) if cj.lugar == *lugar => cj.asa,
                _ => continue,
            };
            if let Ok((ent, raros)) = listar(&self.ar, &mut self.c, asa) {
                let v = &mut self.ventanas[k];
                if let Que_::Cajon(cj) = &mut v.que {
                    let elegido = cj.elegido.and_then(|i| cj.entradas.get(i)).map(|e| e.nombre.clone());
                    cj.entradas = ent;
                    cj.raros = raros;
                    cj.elegido = elegido.and_then(|n| cj.entradas.iter().position(|e| e.nombre == n));
                    cj.primera = 0;
                    Self::poner_estado(&mut v.i, cj, None);
                }
                if self.ventanas[k].configurada {
                    self.presentar(k, false);
                }
            }
        }
    }

    // --- Información ---

    /// La Información de lo elegido en la ventana `k` (o del propio cajón, sin nada elegido).
    fn info_de(&mut self, k: usize) {
        let Que_::Cajon(cj) = &self.ventanas[k].que else { return };
        let (asa, lugar) = (cj.asa, cj.lugar.clone());
        match cj.elegido.and_then(|i| cj.entradas.get(i)).map(|e| e.nombre.clone()) {
            Some(nombre) => {
                if !nombre_valido(&nombre) {
                    return;
                }
                let r = self.ar.pedir(&mut self.c, a::BUSCAR, asa, 0, 0, nombre.as_bytes());
                if r.e != a::BIEN {
                    println!("  [workbench] sin Información de {}/{}: {}", lugar.texto(), nombre, r.e);
                    self.aviso_cajon(k, &alloc::format!("Sin acceso a {}", nombre));
                    return;
                }
                let ruta = alloc::format!("{}/{}", lugar.texto(), nombre);
                let raiz = lugar.raiz;
                self.abrir_info_asa(r.asa, true, ruta, Some(lugar), raiz);
            }
            None => {
                let raiz = lugar.raiz;
                self.abrir_info_asa(asa, false, lugar.texto(), None, raiz);
            }
        }
    }

    fn abrir_info(&mut self, asa: u32, cerrar_asa: bool, ruta: String, padre: Option<Lugar>, _volumen: bool) {
        let raiz = asa;
        self.abrir_info_asa(asa, cerrar_asa, ruta, padre, raiz);
    }

    /// La ventana de Información de lo que tiene la asa, a la que se llegó por la raíz `raiz`.
    fn abrir_info_asa(&mut self, asa: u32, cerrar_asa: bool, ruta: String, padre: Option<Lugar>, raiz: u32) {
        let d = self.ar.pedir(&mut self.c, a::DATOS, asa, 0, 0, b"");
        let du = self.ar.pedir(&mut self.c, a::DUENO, asa, 0, 0, b"");
        let co = self.ar.pedir(&mut self.c, a::COMENTARIO, asa, 0, 0, b"");
        if d.e != a::BIEN || du.e != a::BIEN {
            println!("  [workbench] sin Información de {}: datos {}, dueño {}", ruta, d.e, du.e);
            if cerrar_asa {
                self.ar.cerrar(&mut self.c, asa);
            }
            return;
        }
        let comentario = if co.e == a::BIEN {
            let n = (co.a as usize).min(255);
            String::from_utf8_lossy(&self.ar.ventana()[..n]).chars().filter(|c| !c.is_control()).collect()
        } else {
            String::new()
        };
        let modo = (d.c & 0o7777) as i64;
        let tipo = ((d.c >> 16) & 0xff) as u8;
        let bits = ((d.c >> 32) & 0xffff) as i64;
        let (dueno, grupo, yo) = (du.a, du.b, du.c);
        let puede = dueno == yo && raiz == RAIZ_HOME;
        let porque = if raiz != RAIZ_HOME {
            "Por Sistema solo se puede leer"
        } else if dueno != yo {
            "Solo lo puede cambiar su dueño"
        } else {
            ""
        };
        let nombre = ruta.rsplit('/').next().map_or_else(|| ruta.clone(), String::from);
        let que = match (tipo, padre.is_none()) {
            (a::CAJON, true) if !ruta.contains('/') => String::from("Volumen"),
            (a::CAJON, _) => String::from("Cajón"),
            _ if modo & 0o111 != 0 => alloc::format!("Programa · {}", tamano(d.a)),
            _ => alloc::format!("Fichero · {}", tamano(d.a)),
        };
        let mut arbol = Arbol::new();
        let titulo = texto_obj(&mut arbol, &nombre, 1);
        let linea1 = texto_obj(&mut arbol, &alloc::format!("{} · cambiado el {}", que, fecha(d.b)), 2);
        let tu = if dueno == yo { " (tú)" } else { "" };
        let linea2 = texto_obj(&mut arbol, &alloc::format!("Dueño: usuario {}{} · grupo {}", dueno, tu, grupo), 2);
        let campo = arbol.crear(
            Campo,
            &[(atr::TEXTO, Valor::Texto(comentario.clone())), (atr::ETIQUETA, Valor::Texto("Comentario".into())), (atr::DESACTIVADO, Valor::Bool(!puede))],
        );
        let valor = bits << 16 | modo;
        let rejilla = arbol.crear(Permisos, &[(atr::VALOR, Valor::Int(valor)), (atr::CURSOR, Valor::Int(-1)), (atr::DESACTIVADO, Valor::Bool(!puede))]);
        let aviso = texto_obj(&mut arbol, porque, 2);
        let hueco = texto_obj(&mut arbol, "", 0);
        let cancelar = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto(String::from(if puede { "Cancelar" } else { "Cerrar" }))), (atr::ESTILO, Valor::Int(2))]);
        let guardar = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto("Guardar".into()))]);
        let marca = texto_obj(&mut arbol, "", 0);
        arbol.notificar(cancelar, atr::CLIC, Cuando::Igual(Valor::Bool(true)), marca, atr::TEXTO, Que::Fijo(Valor::Texto("cancelar".into())));
        arbol.notificar(guardar, atr::CLIC, Cuando::Igual(Valor::Bool(true)), marca, atr::TEXTO, Que::Fijo(Valor::Texto("guardar".into())));
        let mut botones = alloc::vec![Hijo::Obj(hueco), Hijo::Obj(cancelar)];
        if puede {
            botones.push(Hijo::Obj(guardar));
        }
        let raiz_kit = Hijo::Columna(alloc::vec![
            Hijo::Obj(titulo),
            Hijo::Obj(linea1),
            Hijo::Obj(linea2),
            Hijo::Obj(campo),
            Hijo::Obj(rejilla),
            Hijo::Obj(aviso),
            Hijo::Fila(botones),
        ]);
        let mut i = self.interfaz(arbol, raiz_kit);
        i.esquinas = 16;
        let alto = i.medida().min.1.clamp(200, 560);
        i.colocar(Rect::new(0, 0, ANCHO_INFO, alto));
        if puede {
            i.foco = Some(campo);
        }
        let centro = |o: Obj| i.sitios.get(&o).map_or((0, 0), |r| (r.x + r.w / 2, r.y + r.h / 2));
        let (cx, cy) = centro(campo);
        let (gx, gy) = if puede { centro(guardar) } else { centro(cancelar) };
        let (ax, ay) = i.sitios.get(&rejilla).map_or((0, 0), |&r| Permisos::centro(r, 3, 2));
        let n = self.abiertas + 1;
        println!(
            "  [workbench] Información de {} (ventana {}): {}, dueño {}{}, grupo {}, modo {:o}, bits {:#x}, comentario \"{}\"; se puede cambiar: {}",
            ruta, n, que, dueno, tu, grupo, modo, bits, comentario, if puede { "sí" } else { "no" }
        );
        println!("  [workbench] ventana {}: campo en ({}, {}), Archivado en ({}, {}), {} en ({}, {})", n, cx, cy, ax, ay, if puede { "Guardar" } else { "Cerrar" }, gx, gy);
        let inf = Info { ruta, asa, cerrar_asa, padre, comentario, valor, puede, campo, rejilla, aviso };
        let titulo_v = alloc::format!("Información de {}", nombre);
        if !self.nueva_ventana(&titulo_v, ANCHO_INFO, alto, i, marca, Que_::Info(inf)) && cerrar_asa {
            self.ar.cerrar(&mut self.c, asa);
        }
    }

    /// Guarda lo cambiado en la Información `k`: el comentario (`COMENTAR`) y la protección (`PROTEGER`). Si todo va
    /// bien, cierra la ventana.
    fn guardar_info(&mut self, k: usize) {
        let v = &self.ventanas[k];
        let Que_::Info(inf) = &v.que else { return };
        if !inf.puede {
            return;
        }
        let texto = v.i.arbol.leer(inf.campo, atr::TEXTO).map(Valor::texto).unwrap_or_default();
        let valor = v.i.arbol.leer(inf.rejilla, atr::VALOR).map_or(inf.valor, Valor::int);
        let (asa, ruta, padre, viejo, valor_viejo, aviso) = (inf.asa, inf.ruta.clone(), inf.padre.clone(), inf.comentario.clone(), inf.valor, inf.aviso);
        let mut fallo = None;
        let mut partes = Vec::new();
        if texto != viejo {
            let e = if texto.len() > 200 { a::MAL_FORMADO } else { self.ar.pedir(&mut self.c, a::COMENTAR, asa, 0, 0, texto.as_bytes()).e };
            partes.push(alloc::format!("comentario \"{}\": {}", texto, e));
            if e != a::BIEN {
                fallo = Some(if e == a::MAL_FORMADO { "El comentario es demasiado largo" } else { "No se pudo cambiar el comentario" });
            }
        }
        if valor != valor_viejo {
            let (bits, modo) = ((valor >> 16) as u64, (valor & 0o7777) as u64);
            let e = self.ar.pedir(&mut self.c, a::PROTEGER, asa, bits, modo, b"").e;
            partes.push(alloc::format!("protección bits {:#x} modo {:o}: {}", bits, modo, e));
            if e != a::BIEN {
                fallo = Some("No se pudo cambiar la protección");
            }
        }
        if partes.is_empty() {
            partes.push(String::from("nada cambiado"));
        }
        println!("  [workbench] guarda {}: {}", ruta, partes.join(", "));
        match fallo {
            None => {
                self.cerrar_ventana(k);
                if let Some(p) = padre {
                    self.releer(&p);
                }
            }
            Some(porque) => {
                let v = &mut self.ventanas[k];
                v.i.arbol.poner(aviso, atr::TEXTO, Valor::Texto(String::from(porque)));
                self.presentar(k, false);
            }
        }
    }

    // --- Entrada ---

    fn ventana_de(&self, sup: u32) -> Option<usize> {
        self.ventanas.iter().position(|v| v.sup == sup)
    }

    /// Lo que pide la marca de la interfaz de la ventana `k` tras un evento.
    fn atender_marca(&mut self, k: usize) -> bool {
        let v = &mut self.ventanas[k];
        let que = v.i.arbol.leer(v.marca, atr::TEXTO).map(Valor::texto).unwrap_or_default();
        if que.is_empty() {
            return false;
        }
        v.i.arbol.poner(v.marca, atr::TEXTO, Valor::Texto(String::new()));
        match que.as_str() {
            "vista" => self.cambiar_vista(k),
            "info" => self.info_de(k),
            "cerrar" | "cancelar" => self.cerrar_ventana(k),
            "guardar" => self.guardar_info(k),
            _ => return false,
        }
        true
    }

    /// Un botón del ratón sobre la ventana `k`.
    fn raton_ventana(&mut self, k: usize, boton: u32, pulsado: bool, ms: u32) {
        let (x, y) = (self.x, self.y);
        // En el contenido de un cajón: elegir, doble clic y botón derecho.
        let mut abrir = None;
        let mut info = false;
        let mut repintar = false;
        {
            let v = &mut self.ventanas[k];
            if let Que_::Cajon(cj) = &mut v.que
                && let Some(&r) = v.i.sitios.get(&cj.hueco)
                && r.contiene(x, y)
            {
                if pulsado {
                    let bajo = celdas(r, cj).into_iter().find(|(_, c)| c.contiene(x, y)).map(|(i, _)| i);
                    if bajo != cj.elegido {
                        cj.elegido = bajo;
                        repintar = true;
                    }
                    match (bajo, boton) {
                        (Some(i), BOTON_IZQUIERDO) => {
                            let doble = matches!(cj.ultimo, Some((u, t)) if u == i && ms.wrapping_sub(t) <= DOBLE_MS);
                            cj.ultimo = if doble { None } else { Some((i, ms)) };
                            if doble {
                                abrir = Some(i);
                            }
                        }
                        (Some(_), BOTON_DERECHO) => info = true,
                        _ => cj.ultimo = None,
                    }
                }
            }
        }
        if boton == BOTON_IZQUIERDO {
            self.pulsado = pulsado;
            let v = &mut self.ventanas[k];
            repintar |= v.i.raton_hora(x, y, pulsado, ms);
        }
        if repintar && self.ventanas[k].configurada {
            self.presentar(k, false);
        }
        if let Some(i) = abrir {
            self.abrir_entrada(k, i);
        } else if info {
            self.info_de(k);
        } else {
            self.atender_marca(k);
        }
    }

    /// Una tecla en la ventana `k`: Esc la cierra; en un cajón, las flechas eligen e Intro abre; en la Información, al
    /// kit (el campo del comentario).
    fn tecla_ventana(&mut self, k: usize, codigo: u32, pulsada: bool) {
        if codigo == KEY_ESC {
            if pulsada {
                self.cerrar_ventana(k);
            }
            return;
        }
        let es_cajon = matches!(self.ventanas[k].que, Que_::Cajon(_));
        if !es_cajon {
            let v = &mut self.ventanas[k];
            if v.i.tecla(codigo, pulsada) && v.configurada {
                self.presentar(k, false);
            }
            self.atender_marca(k);
            return;
        }
        let Some(t) = self.teclado.tecla(codigo, pulsada) else { return };
        let mut abrir = None;
        {
            let v = &mut self.ventanas[k];
            let Que_::Cajon(cj) = &mut v.que else { return };
            let Some(&r) = v.i.sitios.get(&cj.hueco) else { return };
            let n = cj.entradas.len();
            if n == 0 {
                return;
            }
            let (caben_, paso) = caben(r, cj);
            let actual = cj.elegido.unwrap_or(0);
            let nuevo = match t {
                Tecla::Derecha => (actual + 1).min(n - 1),
                Tecla::Izquierda => actual.saturating_sub(1),
                Tecla::Abajo => (actual + paso).min(n - 1),
                Tecla::Arriba => actual.saturating_sub(paso),
                Tecla::Inicio => 0,
                Tecla::Fin => n - 1,
                Tecla::Intro => {
                    abrir = cj.elegido;
                    actual
                }
                _ => return,
            };
            if cj.elegido.is_none() && abrir.is_none() {
                cj.elegido = Some(0);
            } else {
                cj.elegido = Some(nuevo);
            }
            // Que se vea lo elegido.
            let e = cj.elegido.unwrap_or(0);
            if e < cj.primera {
                cj.primera = e / paso * paso;
            } else if e >= cj.primera + caben_ {
                cj.primera = (e + paso - caben_) / paso * paso;
            }
        }
        if self.ventanas[k].configurada {
            self.presentar(k, false);
        }
        if let Some(i) = abrir {
            self.abrir_entrada(k, i);
        }
    }

    /// El tema del sistema nuevo: a todo.
    fn poner_tema(&mut self, t: Tema) {
        self.tema = t;
        self.pintar_escritorio();
        for k in 0..self.ventanas.len() {
            self.ventanas[k].i.ctx.tema = t;
            if self.ventanas[k].configurada {
                self.presentar(k, false);
            }
        }
    }
}

/// Lo que no debe ver ni abrir (S6b), con sus raíces ya puestas: el cajón de `sesiones` y su registro (por Sistema), el
/// cajón de otra persona (`DeCarla`) y su fichero, crear por la raíz de solo lectura y subir por encima de `Home` con
/// "..". Y los nombres raros: los que no enseña al listar y los que no busca.
fn comprobar_lo_ajeno(ar: &Archivos, c: &mut Cliente) {
    let listar_en = |c: &mut Cliente, raiz: u32, camino: &str| {
        let r = ar.pedir(c, a::BUSCAR, raiz, 0, 0, camino.as_bytes());
        if r.e != a::BIEN {
            return r.e;
        }
        let l = ar.pedir(c, a::LISTAR, r.asa, 0, 0, b"").e;
        ar.cerrar(c, r.asa);
        l
    };
    let abrir_en = |c: &mut Cliente, raiz: u32, camino: &str| {
        let r = ar.pedir(c, a::BUSCAR, raiz, 0, 0, camino.as_bytes());
        if r.e == a::BIEN {
            ar.cerrar(c, r.asa);
        }
        r.e
    };
    let sesiones_l = listar_en(c, RAIZ_SISTEMA, "Estado/Servicios/sesiones");
    let registro = abrir_en(c, RAIZ_SISTEMA, "Estado/Servicios/sesiones/registro");
    let carla_l = listar_en(c, RAIZ_SISTEMA, "DeCarla");
    let secreto = abrir_en(c, RAIZ_SISTEMA, "DeCarla/carta.txt");
    let crear = ar.pedir(c, a::CREAR, RAIZ_SISTEMA, a::FICHERO as u64, 0, b"workbench.txt");
    if crear.e == a::BIEN {
        ar.cerrar(c, crear.asa);
    }
    let subir = abrir_en(c, RAIZ_HOME, "..");
    let subir_txt = if subir == a::BIEN { "se abre" } else { "no se abre" };
    let bien = sesiones_l == a::PROTEGIDO && registro == a::PROTEGIDO && carla_l == a::PROTEGIDO && secreto == a::PROTEGIDO
        && crear.e == a::PROTEGIDO && subir != a::BIEN;
    println!(
        "  [workbench] lo ajeno: listar el cajón de sesiones {}, abrir su registro {}; listar DeCarla {}, abrir su fichero {}; crear por Sistema {}; \"..\" desde Home {}: {}",
        sesiones_l, registro, carla_l, secreto, crear.e, subir_txt, if bien { "bien" } else { "MAL" }
    );
    // Los nombres raros: '|' en Home y ':' en Sistema/Imagenes no se enseñan; "..", "a/b", "x:y" y "" no se buscan.
    let raros_home = listar(ar, c, RAIZ_HOME).map_or(0, |x| x.1);
    let r = ar.pedir(c, a::BUSCAR, RAIZ_SISTEMA, 0, 0, b"Imagenes");
    let raros_img = if r.e == a::BIEN {
        let n = listar(ar, c, r.asa).map_or(0, |x| x.1);
        ar.cerrar(c, r.asa);
        n
    } else {
        0
    };
    let no_busca = ["..", ".", "a/b", "x:y", "", "uno|dos"].iter().all(|n| !nombre_valido(n));
    let bien = raros_home >= 1 && raros_img >= 1 && no_busca;
    println!(
        "  [workbench] nombres raros: {} sin enseñar en Home, {} en Sistema/Imagenes; \"..\", \"a/b\", \"x:y\" no se buscan: {}",
        raros_home, raros_img, if bien { "bien" } else { "MAL" }
    );
}

fn principal() -> u64 {
    dashos::monton::con_nombre(&"workbench");
    // Arranca de los últimos, con el compositor atendiendo a muchos: si su cola está llena, se reintenta un rato.
    let mut cliente = None;
    for _ in 0..100 {
        cliente = Cliente::conectar();
        if cliente.is_some() {
            break;
        }
        dashos::dormir_us(50_000);
    }
    let Some(mut c) = cliente else {
        let (x, y) = dashos::canal_crear();
        let e = dashos::canal_escribir(VENTANAS, &[wayland::canal::TROZO_CONECTAR], &[y]);
        dashos::cerrar(x);
        println!("  [workbench] sin compositor ({})", e);
        return 1;
    };
    let Some(globales) = c.globales() else { return 2 };
    let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
    let (Some(gc), Some(gs), Some(gx), Some(ga), Some(gl), Some(gt)) =
        (global("wl_compositor"), global("wl_shm"), global("xdg_wm_base"), global("wl_seat"), global("zwlr_layer_shell_v1"), global("dashos_tema_v1"))
    else {
        println!("  [workbench] el compositor no le da capas o tema");
        return 3;
    };
    let Some(ar) = Archivos::abrir(&mut c) else {
        println!("  [workbench] sin servicio de archivos");
        return 4;
    };
    let (sistema, home) = ar.ceder_raices(&mut c);
    println!("  [workbench] raíces: Sistema solo para leer {}, Home para leer y escribir {}", sistema, home);
    if sistema != a::BIEN || home != a::BIEN {
        return 5;
    }
    comprobar_lo_ajeno(&ar, &mut c);
    let mut app = App {
        c,
        ar,
        tema: Tema::CLARO,
        medio: Fuente::medium(),
        esc: Escritorio { w: 0, h: 0, mem: 0, pool: 0, bufer: 0, iconos: Vec::new(), elegido: None, encima: None, ultimo: None, home: false },
        esc_configurado: false,
        ventanas: Vec::new(),
        siguiente_id: PRIMER_ID,
        abiertas: 0,
        sobre: 0,
        x: 0,
        y: 0,
        pulsado: false,
        con_teclado: 0,
        teclado: Teclado::new(),
        contextos: Vec::new(),
    };
    app.esc.home = sesion_abierta(&mut app.c);
    app.crear_escritorio(gc, gs, gx, ga, gl, gt);
    let mut mirar = dashos::ahora_us() + CADA_ACTIVA_US;
    loop {
        let ev = match app.c.evento_plazo(500_000) {
            Err(()) => {
                println!("  [workbench] se cortó la conexión con el compositor");
                return 6;
            }
            Ok(ev) => ev,
        };
        if dashos::ahora_us() >= mirar {
            app.mirar_home();
            mirar = dashos::ahora_us() + CADA_ACTIVA_US;
        }
        let Some((o, orden, cuerpo)) = ev else { continue };
        let palabra = |i: usize| cuerpo.get(4 * i..4 * i + 4).map_or(0, |p| u32::from_le_bytes(p.try_into().unwrap()));
        match (o, orden) {
            (XDG, xdg_wm_base::ev::PING) => {
                let mut e = Escritor::new();
                e.empezar(XDG, xdg_wm_base::req::PONG).uint(palabra(0)).fin();
                app.c.mandar(&e);
            }
            (TEMA, dashos_tema_v1::ev::TEMA) => {
                let t = Tema::del_sistema(palabra(0) | 0xff00_0000, palabra(1) != 0);
                if t != app.tema {
                    app.poner_tema(t);
                }
            }
            (CAPA_ESC, zwlr_layer_surface_v1::ev::CONFIGURE) => {
                app.escritorio_configurado(palabra(0), palabra(1) as i32, palabra(2) as i32);
            }
            (CAPA_ESC, zwlr_layer_surface_v1::ev::CLOSED) => {
                println!("  [workbench] el compositor cerró el escritorio");
                return 0;
            }
            (TECLADO, wl_keyboard::ev::KEYMAP) => {
                if let Some(l) = app.c.juntador.tomar_llave() {
                    dashos::cerrar(l);
                }
            }
            (TECLADO, wl_keyboard::ev::ENTER) => app.con_teclado = palabra(1),
            (TECLADO, wl_keyboard::ev::LEAVE) => app.con_teclado = 0,
            (TECLADO, wl_keyboard::ev::KEY) => {
                if let Some(k) = app.ventana_de(app.con_teclado) {
                    app.tecla_ventana(k, palabra(2), palabra(3) == 1);
                }
            }
            (PUNTERO, wl_pointer::ev::ENTER | wl_pointer::ev::MOTION | wl_pointer::ev::LEAVE) => {
                let ms = palabra(0);
                match orden {
                    wl_pointer::ev::ENTER => (app.sobre, app.x, app.y) = (palabra(1), palabra(2) as i32 >> 8, palabra(3) as i32 >> 8),
                    wl_pointer::ev::LEAVE => {
                        if let Some(k) = app.ventana_de(app.sobre)
                            && app.ventanas[k].i.raton(-1, -1, false)
                            && app.ventanas[k].configurada
                        {
                            app.presentar(k, false);
                        }
                        app.sobre = 0;
                        app.pulsado = false;
                    }
                    _ => (app.x, app.y) = (palabra(1) as i32 >> 8, palabra(2) as i32 >> 8),
                }
                // Bajo el ratón, en el escritorio: la capa de estado del icono.
                let encima = if app.sobre == SUP_ESC { app.esc.bajo(app.x, app.y) } else { None };
                if encima != app.esc.encima {
                    app.esc.encima = encima;
                    app.pintar_escritorio();
                }
                if orden != wl_pointer::ev::LEAVE
                    && let Some(k) = app.ventana_de(app.sobre)
                {
                    let (x, y, p) = (app.x, app.y, app.pulsado);
                    let v = &mut app.ventanas[k];
                    let cambia = if orden == wl_pointer::ev::MOTION { v.i.raton_hora(x, y, p, ms) } else { v.i.raton(x, y, p) };
                    if cambia && v.configurada {
                        app.presentar(k, false);
                    }
                }
            }
            // La rueda (eje vertical, 0) sobre una ventana: desplaza su lista.
            (PUNTERO, wl_pointer::ev::AXIS) if palabra(1) == 0 => {
                let (x, y) = (app.x, app.y);
                if let Some(k) = app.ventana_de(app.sobre)
                    && app.ventanas[k].i.rueda(x, y, kit::controles::pasos_rueda(palabra(2)))
                    && app.ventanas[k].configurada
                {
                    app.presentar(k, false);
                }
            }
            (PUNTERO, wl_pointer::ev::BUTTON) => {
                let (ms, boton, pulsado) = (palabra(1), palabra(2), palabra(3) == 1);
                if app.sobre == SUP_ESC {
                    app.raton_escritorio(boton, pulsado, ms);
                } else if let Some(k) = app.ventana_de(app.sobre) {
                    app.raton_ventana(k, boton, pulsado, ms);
                }
            }
            (1, wl_display::ev::ERROR) => {
                println!("  [workbench] error de protocolo en el objeto {}: {}", palabra(0), palabra(1));
                return 7;
            }
            _ => {
                // Las de las ventanas: su configure (la primera vez, se pinta y dice dónde quedó cada cosa).
                if orden == xdg_surface::ev::CONFIGURE
                    && let Some(k) = app.ventanas.iter().position(|v| v.xsup == o)
                {
                    let mut e = Escritor::new();
                    e.empezar(o, xdg_surface::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                    app.c.mandar(&e);
                    if !app.ventanas[k].configurada {
                        app.ventanas[k].configurada = true;
                        app.presentar(k, true);
                        let v = &app.ventanas[k];
                        match &v.que {
                            Que_::Cajon(cj) => {
                                let nombres: Vec<&str> = cj.entradas.iter().map(|e| e.nombre.as_str()).collect();
                                println!(
                                    "  [workbench] ventana {} de {} en pantalla: {} {} ({} nombres raros sin enseñar): {}",
                                    v.numero, cj.lugar.texto(), cj.entradas.len(), if cj.entradas.len() == 1 { "elemento" } else { "elementos" }, cj.raros, nombres.join(" ")
                                );
                                app.decir_sitios(k);
                            }
                            Que_::Info(inf) => println!("  [workbench] ventana {} de Información de {} en pantalla", v.numero, inf.ruta),
                        }
                    }
                } else if orden == xdg_toplevel::ev::CLOSE
                    && let Some(k) = app.ventanas.iter().position(|v| v.top == o)
                {
                    app.cerrar_ventana(k);
                }
            }
        }
    }
}
