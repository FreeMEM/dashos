//! S10: el Workbench, primera versión (`docs/diseño/S10-escritorio.md`, "Workbench" y "Aspecto"). Es un cliente del
//! compositor, como la barra: si se cae, las ventanas de los demás siguen.
//!
//! - En el escritorio, los discos (rama `dock-discos`, antes una columna de iconos): un panel flotante translúcido en
//!   una capa de abajo (wlr-layer-shell, capa 1), de serie arriba a la derecha bajo la barra, con una tarjeta por
//!   volumen (icono de la colección, nombre, lo libre y la barra del espacio) y sus animaciones (`discos_modelo.rs`).
//!   Un fondo de pantalla (capa 0) queda debajo y se ve. No pide el teclado: un clic en el escritorio no se lo quita a
//!   la ventana que lo tiene. Sus ajustes, en `ENV:Escritorio/discos` (`discos_ajustes.rs`), vigilados. Botón derecho:
//!   un menú con Abrir e Información.
//! - Doble clic en un volumen o en un cajón abre una ventana de cajón (xdg_toplevel con el kit): iconos en rejilla o
//!   lista con nombre, tamaño y fecha (un botón cambia). Doble clic en un cajón abre otra ventana; en un programa de
//!   `Programas`, lo lanza `sesiones` como la persona que está delante (como la barra).
//! - "Información" (el botón, o el botón derecho en un icono): comentario, bits de protección del Amiga, permisos,
//!   dueño, tamaño y fecha. Se cambian el comentario y los bits si se puede (`COMENTAR` y `PROTEGER`): solo su dueño, y
//!   no por una raíz de solo lectura.
//!
//! Raíces (S6b): la ficha es de Ana (1000) con el derecho de repartir, como el selector. Al empezar se cede a sí mismo
//! las raíces de sus volúmenes, en los mismos huecos que da `sesiones` (`RAIZ_*` del protocolo): el disco solo para leer
//! (`SYS:`), `C` para leer y escribir (`Home:`) y `Work:`, y `sesiones` le pone `RAM:` al entrar su usuario; y solo va por ellas. Los iconos del escritorio son
//! los volúmenes de su tabla de nombres (paso 2 de las asignaciones, `ASIGNACIONES` de `sesiones`), con su nombre. `Home`
//! es `C` de forma provisional: el `Home:` de verdad es el volumen cifrado de Ana, y abrir un volumen por su nombre
//! pide el derecho de administrar; tendrá que cedérselo `sesiones` al entrar. El icono de `Home` sale solo mientras la
//! sesión de Ana está abierta (`ACTIVA` de `sesiones`).
//!
//! Los nombres que no se pueden enseñar sin dudas (vacíos, ".", "..", con '/', ':', '|' o de control) no salen ni se
//! abren, y todo se busca de un paso, por la asa del cajón y un solo nombre.
//!
//! El doble clic en un volumen o en un cajón lo atiende el explorador de archivos (`explorador`, como Nautilus): se le
//! pide por su canal que abra ese lugar (la raíz y los nombres). Si no está, se abre aquí la ventana de cajón sencilla.
//!
//! Llaves: 1 el canal "ventanas" (con las capas, por su nombre), 2 el canal "archivos", 3 el canal "sesiones" y 4 el
//! canal "explorador".

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
#[allow(dead_code)]
mod ventana_cliente;
#[path = "../permisos_kit.rs"]
mod permisos_kit;
#[path = "../idiomas.rs"]
mod idiomas;
#[path = "../discos_ajustes.rs"]
#[allow(dead_code)]
mod ajustes;
#[path = "../muelle.rs"]
#[allow(dead_code)]
mod muelle;
#[path = "../discos_modelo.rs"]
#[allow(dead_code)]
mod discos_modelo;
#[path = "../cpu_propia.rs"]
mod cpu_propia;

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
use kit::tema::{con_opacidad, Tema};
use kit::texto::Fuente;
use discos_modelo::{Panel, Volumen};
use permisos_kit::Permisos;
use ventana_cliente::*;
use wayland::tablas::{
    dashos_tema_v1, wl_buffer, wl_compositor, wl_display, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool,
    wl_surface, xdg_surface, xdg_toplevel, xdg_wm_base, zwlr_layer_shell_v1, zwlr_layer_surface_v1,
};
use wayland::Escritor;

dashos::programa!(principal);
dashos::monton!();

/// Idiomas (paso 4): el nombre de su catálogo y su versión (`tools/catalogo.py` lo lee de aquí), y el catálogo en uso.
const CATALOGO: (&str, u32) = ("workbench", 1);
static TEXTOS: idiomas::Puesto = idiomas::Puesto::nuevo();

/// Un texto, en el idioma del usuario (el castellano si no hay catálogo).
fn tr(numero: u32, de_siempre: &'static str) -> &'static str {
    TEXTOS.texto(numero, de_siempre)
}

/// Un texto con huecos y sus valores.
fn trf(numero: u32, de_siempre: &'static str, valores: &[&dyn core::fmt::Display]) -> String {
    TEXTOS.con(numero, de_siempre, valores)
}

const ARCHIVOS: u32 = 2;
const SESIONES: u32 = 3;
const EXPLORADOR: u32 = 4;
// Los objetos fijos (los de `ventana_cliente` van del 2 al 14); los de cada ventana, desde `PRIMER_ID`, sin repetir.
const CAPAS: u32 = 20;
const TEMA: u32 = 22;
const SUP_ESC: u32 = 30;
const CAPA_ESC: u32 = 31;
const SUP_MENU: u32 = 32;
const CAPA_MENU: u32 = 33;
/// La marca de la puerta: los avisos de cambio de `ENV:` (los ajustes de los discos).
const MARCA_TIMBRE: u64 = 0x5742_454e_4348_0001;
/// Un cuadro de animación.
const CUADRO_US: u64 = 16_667;
/// Cada cuánto se mide el espacio de los volúmenes.
const CADA_MEDIR_US: u64 = 5_000_000;
/// Dónde se ve el búfer del menú de un disco (el de los discos va en `VA`).
const VA_MENU: usize = VA + (1 << 32);
const PRIMER_ID: u32 = 100;
/// Dónde se ve la ventana del servicio de archivos, el escritorio (`VA`, de `ventana_cliente`) y las ventanas.
const VA_ARCHIVOS: usize = 0x50_0000_0000;
const VA_VENTANAS: usize = 0x60_0000_0000;
const HUECO_VENTANA: usize = 16 << 20;
const MAX_VENTANAS: usize = 8;
/// Las raíces que se cede (S6b, capa 2), en los huecos de cada volumen del protocolo.
const RAIZ_SISTEMA: u32 = s::RAIZ_SYS as u32;
const RAIZ_HOME: u32 = s::RAIZ_HOME as u32;
const RAIZ_RAM: u32 = s::RAIZ_RAM as u32;
const RAIZ_WORK: u32 = s::RAIZ_WORK as u32;
/// Los volúmenes de siempre, si `sesiones` no da la tabla: nombre y raíz.
const VOLUMENES_DE_SIEMPRE: [(&str, u32); 4] = [("SYS", RAIZ_SISTEMA), ("Home", RAIZ_HOME), ("RAM", RAIZ_RAM), ("Work", RAIZ_WORK)];
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
        if r.e == a::ASA_MALA && (RAIZ_SISTEMA..=RAIZ_WORK).contains(&asa) {
            let [s, h, ..] = self.ceder_raices(c);
            println!("  [workbench] sus raíces ya no valían (se volvió a una instantánea): SYS: {}, Home: {}", s, h);
            return self.pedir_una(c, orden, asa, x, y, nombre);
        }
        r
    }

    /// Se cede a sí mismo sus raíces (S6b, capa 2): el disco solo para leer, como `SYS:`; `C` para leer y escribir, como
    /// `Home:`; y `Work:` (provisional, un cajón del disco). El estado de cada una. `RAM:` no se la cede: es el volumen
    /// en memoria de la sesión, y su raíz se la pone `sesiones` cuando entra su usuario (y se va cuando sale).
    fn ceder_raices(&self, c: &mut Cliente) -> [i32; 3] {
        let mut ceder = |camino: &[u8], derechos: u64, hueco: u32| {
            let r = self.pedir_una(c, a::BUSCAR, 0, 0, 0, camino);
            if r.e != a::BIEN {
                return r.e;
            }
            let e = self.pedir_una(c, a::CEDER, r.asa, a::YO, derechos | (hueco as u64) << 8, b"").e;
            self.pedir_una(c, a::CERRAR, r.asa, 0, 0, b"");
            e
        };
        [ceder(b"", 4, RAIZ_SISTEMA), ceder(b"C", 4 | 2, RAIZ_HOME), ceder(s::CAMINO_WORK.as_bytes(), 4 | 2, RAIZ_WORK)]
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

/// El servicio de archivos para `dashos::asignaciones` (con él lee `idiomas` los ficheros de idioma), con el cliente del
/// compositor para no dejar de leerle mientras se espera.
struct PorNombres<'a> {
    ar: &'a Archivos,
    c: &'a mut Cliente,
}

impl dashos::asignaciones::Fs for PorNombres<'_> {
    fn pedir(&mut self, orden: u32, asa: u32, x: u64, y: u64, nombre: &[u8]) -> dashos::asignaciones::Resp {
        // Sin ceder las raíces otra vez si una no vale: leer el idioma no arregla nada, y lo que falte no está.
        let r = self.ar.pedir_una(self.c, orden, asa, x, y, nombre);
        dashos::asignaciones::Resp { e: r.e, asa: r.asa, a: r.a, b: r.b, c: r.c }
    }

    fn ventana(&self) -> &[u8] {
        self.ar.ventana()
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

/// El tamaño para la consola: siempre en castellano (lo comprueban las pruebas).
fn tamano(b: u64) -> String {
    match b {
        1 => String::from("1 byte"),
        0..1024 => alloc::format!("{} bytes", b),
        1024..1_048_576 => alloc::format!("{} KB", b.div_ceil(1024)),
        _ => alloc::format!("{},{} MB", b / 1_048_576, b % 1_048_576 * 10 / 1_048_576),
    }
}

/// El tamaño para las ventanas, en el idioma del usuario.
fn tamano_visible(b: u64) -> String {
    match b {
        1 => String::from(tr(1, "1 byte")),
        0..1024 => trf(2, "{} bytes", &[&b]),
        1024..1_048_576 => trf(3, "{} KB", &[&b.div_ceil(1024)]),
        _ => trf(4, "{},{} MB", &[&(b / 1_048_576), &(b % 1_048_576 * 10 / 1_048_576)]),
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

// ---------------------------------------------------------------------------------------------------------------
// Las ventanas.

/// Dónde está un cajón: por qué raíz y por qué nombres.
#[derive(Clone, PartialEq, Eq)]
struct Lugar {
    raiz: u32,
    nombres: Vec<String>,
}

impl Lugar {
    /// Como en el Amiga: "SYS:", "SYS:Programas", "Home:Privado/nota.txt".
    fn texto(&self) -> String {
        let mut t = String::from(nombre_volumen(self.raiz));
        t.push(':');
        t.push_str(&self.nombres.join("/"));
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
        centrado(f, l, r.x + r.w / 2, r.y + r.h / 2, tr(5, "Vacío"), 14, t.sobre_variante_superficie);
    }
    let x_tam = r.x + r.w - 140;
    let x_fecha = r.x + r.w - 124;
    if cj.lista {
        m.dibujar(l, r.x + 48, r.y + 17, tr(6, "Nombre"), 12, t.sobre_variante_superficie);
        let w = m.medir(tr(7, "Tamaño"), 12).ancho;
        m.dibujar(l, x_tam - w, r.y + 17, tr(7, "Tamaño"), 12, t.sobre_variante_superficie);
        m.dibujar(l, x_fecha, r.y + 17, tr(8, "Fecha"), 12, t.sobre_variante_superficie);
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
                let tt = tamano_visible(e.tam);
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
// Los discos del escritorio (rama `dock-discos`). Antes, una columna de iconos de 104 de ancho pegada a la derecha;
// ahora, el panel flotante de `discos_modelo.rs` (como los widgets de escritorio de DMS), pintado aquí con el kit.

/// El nombre del volumen de una raíz (para los títulos y los caminos).
fn nombre_volumen(raiz: u32) -> &'static str {
    VOLUMENES_DE_SIEMPRE.iter().find(|v| v.1 == raiz).map_or("?", |v| v.0)
}

/// El icono de la colección (`kit/iconos/dashos.iconos`) de un volumen.
fn icono_volumen(raiz: u32) -> &'static str {
    match raiz {
        RAIZ_SISTEMA => "disco-sistema",
        RAIZ_HOME => "carpeta-casa",
        RAIZ_RAM => "disco-ram",
        _ => "disco-duro",
    }
}

/// Los volúmenes de su tabla de nombres (`ASIGNACIONES` de `sesiones`), en su orden: los que conoce, con la raíz que dice
/// la tabla. Sin tabla, los de siempre. Los nombres de los volúmenes no se traducen.
fn volumenes_de_la_tabla(tabla: Option<&dashos::asignaciones::Tabla>) -> Vec<(&'static str, u32)> {
    let Some(t) = tabla else {
        println!("  [workbench] sin tabla de nombres: los volúmenes de siempre");
        return VOLUMENES_DE_SIEMPRE.to_vec();
    };
    let mut v = Vec::new();
    for l in t.volumenes() {
        let raiz = l.destinos.strip_prefix(b"@").and_then(|r| core::str::from_utf8(r).ok()).and_then(|r| r.parse::<u32>().ok());
        let conocido = VOLUMENES_DE_SIEMPRE.iter().find(|x| dashos::asignaciones::mismo(x.0.as_bytes(), l.nombre));
        if let (Some(raiz), Some(&(nombre, de_siempre))) = (raiz, conocido) {
            // Solo las raíces que se cedió (las de siempre de cada volumen).
            if raiz == de_siempre {
                v.push((nombre, raiz));
            }
        }
    }
    let nombres: Vec<&str> = v.iter().map(|x| x.0).collect();
    println!("  [workbench] volúmenes de su tabla de nombres: {}", nombres.join(": ") + ":");
    v
}

/// Lo que se puede hacer con un disco desde su menú (botón derecho).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Opcion {
    Abrir,
    Informacion,
}

fn texto_opcion(o: Opcion) -> &'static str {
    match o {
        Opcion::Abrir => tr(36, "Abrir"),
        Opcion::Informacion => tr(37, "Información"),
    }
}

/// Filas del menú: 40 de alto (las del menú de Material del kit son de 44; en un menú corto de escritorio, algo menos),
/// 8 de relleno arriba y abajo.
const MENU_FILA: i32 = 40;
const MENU_RELLENO: i32 = 6;
const MENU_ANCHO: i32 = 188;

/// El menú de un disco: una capa de arriba (3), con el teclado (Esc lo cierra), junto a la tarjeta.
struct MenuDiscos {
    nombre: String,
    raiz: u32,
    opciones: Vec<Opcion>,
    tam: (i32, i32),
    encima: Option<usize>,
    hoja: Option<Lamina>,
    bufer: u32,
    bufer_tam: (i32, i32),
    listo: bool,
    /// La entrada (de 0 a 255 de opacidad), 75 ms con OutQuart como el menú del dock de DMS.
    aparece: muelle::Tramo,
    /// Desde cuándo está el ratón fuera de los discos y del menú.
    fuera_desde: Option<u64>,
}

/// Lo que espera el menú con el ratón fuera de los discos y de él antes de cerrarse (de él no llega un clic en otra
/// ventana).
const MENU_FUERA_US: u64 = 1_500_000;

struct Escritorio {
    aj: ajustes::Discos,
    panel: Panel,
    /// La capa: creada; si ya sabe el alto de la pantalla (el primer configure, de arriba abajo); el tamaño y las anclas
    /// que pidió; y lo que configuró el compositor.
    capa: bool,
    medida: bool,
    pedido: (i32, i32, u32),
    w: i32,
    h: i32,
    hoja: Option<Lamina>,
    bufer: u32,
    bufer_tam: (i32, i32),
    home: bool,
    /// Los volúmenes de su tabla de nombres: nombre y raíz; y lo que mide y tiene libre cada raíz.
    volumenes: Vec<(&'static str, u32)>,
    medidas: BTreeMap<u32, (u64, u64)>,
    ultimo: Option<(usize, u32)>,
    menu: Option<MenuDiscos>,
    /// Los sitios dichos la última vez (para no repetirlos).
    dicho: Vec<(String, i32, i32)>,
}

/// Escala la opacidad de lo pintado en `r` por `a` (de 0 a 255): el búfer va premultiplicado, así que se escalan los
/// cuatro canales. Para las tarjetas que entran o salen y el panel que aparece.
fn atenuar(px: &mut [u32], ancho: i32, alto: i32, r: Rect, a: u32) {
    if a >= 255 {
        return;
    }
    let (x0, y0) = (r.x.max(0), r.y.max(0));
    let (x1, y1) = ((r.x + r.w).min(ancho), (r.y + r.h).min(alto));
    for y in y0..y1 {
        let fila = &mut px[(y * ancho) as usize..(y * ancho + ancho) as usize];
        for p in &mut fila[x0 as usize..x1.max(x0) as usize] {
            let c = *p;
            if c == 0 {
                continue;
            }
            let e = |s: u32| (((c >> s) & 0xff) * a / 255) << s;
            *p = e(24) | e(16) | e(8) | e(0);
        }
    }
}

fn rect_de(c: discos_modelo::Caja) -> Rect {
    Rect::new(c.x, c.y, c.w, c.h)
}

/// El color de la barra del espacio: el del acento; con el disco casi lleno, hacia el error (DMS: aviso pasado el 75 %
/// y error pasado el 90 %; el tema no tiene papel de aviso: la mitad entre los dos).
fn color_uso(t: &Tema, milesimas: i32) -> Color {
    if milesimas > 900 {
        t.error
    } else if milesimas > 750 {
        kit::tema::mezcla(t.primario, t.error, 128)
    } else {
        t.primario
    }
}

/// Pinta el panel de los discos en el búfer de `w` × `h`.
#[allow(clippy::too_many_arguments)]
fn pintar_panel(px: &mut [u32], w: i32, h: i32, p: &Panel, t: &Tema, medio: &mut Fuente, regular: &mut Fuente, iconos: &mut kit::iconos::coleccion::Iconos, simbolos: &kit::iconos::Simbolos, ahora: u64) {
    px.fill(0);
    let mut l = Lienzo::new(px, w, h, w as usize);
    let m = p.m();
    let caja = rect_de(p.caja_panel());
    let opacidad = p.aj.opacidad * 255 / 100;
    // El fondo: el contenedor del tema con la transparencia de los ajustes, y un filo para que se vea sobre cualquier fondo.
    l.redondeado(caja, m.radio, con_opacidad(t.contenedor(), opacidad));
    l.borde(caja, m.radio, 1, con_opacidad(t.variante_contorno, 70));
    if p.aj.titulo {
        let cy = caja.y + m.relleno + m.cabecera / 2;
        simbolos.pintar(&mut l, caja.x + 14, cy - 9, 18, kit::iconos::simbolo::HARD_DRIVE, t.sobre_variante_superficie);
        let titulo = tr(38, "Discos");
        let mm = medio.medir(titulo, 13);
        medio.dibujar(&mut l, caja.x + 40, cy + (mm.sube - mm.baja) / 2, titulo, 13, t.sobre_variante_superficie);
    }
    let fondo_tarjeta = con_opacidad(t.contenedor_alto(), (opacidad + 40).min(255));
    for (k, c) in p.cajas() {
        let tj = &p.tarjetas[k];
        let r = rect_de(c);
        let entera = Rect::new(c.x, c.y, c.w, m.tarjeta);
        l.recortar(r);
        let elegido = p.elegido.as_deref() == Some(tj.vol.nombre.as_str());
        l.redondeado(entera, m.radio_tarjeta, if elegido { con_opacidad(t.contenedor_secundario(), 240) } else { fondo_tarjeta });
        // La capa de estado del ratón encima (8 %, Material) y la onda del clic.
        let enc = tj.encima.entero().clamp(0, 1000) as u32;
        if enc > 0 {
            l.redondeado(entera, m.radio_tarjeta, con_opacidad(t.sobre_superficie, 31 * enc / 1000));
            l.borde(entera, m.radio_tarjeta, 1, con_opacidad(t.primario, 120 * enc / 1000));
        }
        if let Some((ox, oy, radio, a)) = p.onda(k, ahora) {
            l.redondeado(Rect::new(c.x + ox - radio, c.y + oy - radio, 2 * radio, 2 * radio), radio, con_opacidad(t.sobre_superficie, a));
        }
        // El icono, que crece un 10 % con el ratón encima.
        let lado = m.icono + m.icono * enc as i32 / 10_000;
        let ix = c.x + 10 + (m.icono - lado) / 2;
        let iy = c.y + (m.tarjeta - lado) / 2;
        iconos.pintar_nombre(&mut l, ix, iy, lado, tj.vol.icono, t);
        // El nombre, lo libre y la barra del espacio.
        let tx = c.x + 10 + m.icono + 12;
        let derecha = c.x + c.w - 12;
        let tinta = if elegido { t.sobre_contenedor_secundario() } else { t.sobre_superficie };
        let suave = if elegido { t.sobre_contenedor_secundario() } else { t.sobre_variante_superficie };
        let nombre = alloc::format!("{}:", tj.vol.nombre);
        let usado = discos_modelo::usado_milesimas(tj.vol.mide, tj.vol.libre);
        let medido = tj.vol.mide > 0;
        let libre = if medido {
            trf(39, "{} libres de {}", &[&discos_modelo::tamano_corto(tj.vol.libre.min(tj.vol.mide)), &discos_modelo::tamano_corto(tj.vol.mide)])
        } else {
            String::from(tr(40, "sin medir"))
        };
        let (b_nombre, b_libre) = if p.aj.espacio { (c.y + m.tarjeta * 21 / 60, c.y + m.tarjeta * 38 / 60) } else { (c.y + m.tarjeta * 22 / 48, c.y + m.tarjeta * 38 / 48) };
        let porcentaje = alloc::format!("{} %", (usado + 5) / 10);
        let ancho_pc = if medido && p.aj.espacio { medio.medir(&porcentaje, 12).ancho + 8 } else { 0 };
        let n = que_quepa(medio, &nombre, 14, derecha - tx - ancho_pc);
        medio.dibujar(&mut l, tx, b_nombre, &n, 14, tinta);
        if ancho_pc > 0 {
            medio.dibujar(&mut l, derecha - ancho_pc + 8, b_nombre, &porcentaje, 12, suave);
        }
        let lb = que_quepa(regular, &libre, 12, derecha - tx);
        regular.dibujar(&mut l, tx, b_libre, &lb, 12, suave);
        if p.aj.espacio {
            let by = c.y + m.tarjeta * 46 / 60;
            let pista = Rect::new(tx, by, derecha - tx, 4);
            l.redondeado(pista, 2, con_opacidad(t.contorno, 51));
            let llena = pista.w * tj.uso.entero().clamp(0, 1000) / 1000;
            if llena > 0 {
                l.redondeado(Rect::new(tx, by, llena.max(4), 4), 2, color_uso(t, usado));
            }
        }
        l.soltar_recorte();
        let pres = tj.presente() as u32;
        if pres < 1000 {
            drop(l);
            atenuar(px, w, h, r, pres * 255 / 1000);
            l = Lienzo::new(px, w, h, w as usize);
        }
    }
    drop(l);
    let a = p.opacidad_panel();
    if a < 255 {
        atenuar(px, w, h, Rect::new(0, 0, w, h), a);
    }
}

/// El menú de un disco: el contenedor del menú de Material (esquinas de 12), filas con su símbolo y la de encima con la
/// capa de estado.
fn pintar_menu(px: &mut [u32], m: &MenuDiscos, t: &Tema, medio: &mut Fuente, simbolos: &kit::iconos::Simbolos, ahora: u64) {
    px.fill(0);
    let (w, h) = m.tam;
    let mut l = Lienzo::new(px, w, h, w as usize);
    l.redondeado(Rect::new(0, 0, w, h), 12, t.contenedor());
    l.borde(Rect::new(0, 0, w, h), 12, 1, con_opacidad(t.variante_contorno, 90));
    for (k, o) in m.opciones.iter().enumerate() {
        let f = Rect::new(4, MENU_RELLENO + k as i32 * MENU_FILA, w - 8, MENU_FILA);
        if m.encima == Some(k) {
            l.redondeado(f, 8, con_opacidad(t.sobre_superficie, 20));
        }
        let s = match o {
            Opcion::Abrir => kit::iconos::simbolo::ARROW_FORWARD,
            Opcion::Informacion => kit::iconos::simbolo::INFO,
        };
        simbolos.pintar(&mut l, f.x + 10, f.y + (f.h - 20) / 2, 20, s, t.sobre_variante_superficie);
        let texto = texto_opcion(*o);
        let mm = medio.medir(texto, 14);
        medio.dibujar(&mut l, f.x + 42, f.y + (f.h + mm.sube - mm.baja) / 2, texto, 14, t.sobre_superficie);
    }
    drop(l);
    let a = m.aparece.valor(ahora).clamp(0, 255) as u32;
    atenuar(px, w, h, Rect::new(0, 0, w, h), a);
}

/// `fichero` de `ENV:` o, si no está, de `ENVARC:` (por su tabla de nombres; sin ella, el de fábrica de
/// `SYS:Prefs/Env-Archive`). `Ok(None)` si no está en ninguno.
fn leer_ajuste(ar: &Archivos, c: &mut Cliente, fichero: &str) -> Result<Option<Vec<u8>>, String> {
    let max = kit::idioma::texto::MAX_FICHERO;
    let leer = |c: &mut Cliente, asa: u32| -> Result<Vec<u8>, i32> {
        let r = ar.pedir(c, a::LEER, asa, 0, max as u64 + 1, b"");
        if r.e != a::BIEN {
            return Err(r.e);
        }
        if r.a as usize > max {
            return Err(a::MAL_FORMADO);
        }
        Ok(ar.ventana()[..r.a as usize].to_vec())
    };
    let Some(tabla) = dashos::asignaciones::pedir_tabla(SESIONES) else {
        let r = ar.pedir(c, a::BUSCAR, RAIZ_SISTEMA, 0, 0, alloc::format!("Prefs/Env-Archive/{}", fichero).as_bytes());
        if r.e == a::NO_EXISTE {
            return Ok(None);
        }
        if r.e != a::BIEN {
            return Err(alloc::format!("el de fábrica no se deja leer ({})", r.e));
        }
        let b = leer(c, r.asa);
        ar.cerrar(c, r.asa);
        return b.map(Some).map_err(|e| alloc::format!("el de fábrica no se deja leer ({})", e));
    };
    for asignacion in ["ENV", "ENVARC"] {
        let camino = alloc::format!("{}:{}", asignacion, fichero);
        match dashos::asignaciones::abrir(&mut PorNombres { ar, c }, &tabla, camino.as_bytes()) {
            Ok((r, _)) => {
                let b = leer(c, r.asa);
                ar.cerrar(c, r.asa);
                return b.map(Some).map_err(|e| alloc::format!("{} no se deja leer ({})", camino, e));
            }
            Err(a::NO_EXISTE) => {}
            Err(e) => return Err(alloc::format!("{} no se deja leer ({})", camino, e)),
        }
    }
    Ok(None)
}

/// Los ajustes de los discos en uso, y lo que se dice de ellos.
fn leer_ajustes(ar: &Archivos, c: &mut Cliente) -> (ajustes::Discos, Vec<String>) {
    let mut dicho = Vec::new();
    let pares = |b: &[u8]| -> Result<Vec<(String, String)>, String> {
        let x = kit::idioma::texto::Ajustes::leer(b).map_err(|_| String::from("no es texto, o es enorme"))?;
        Ok(x.todos().map(|(c, v)| (String::from(c), String::from(v))).collect())
    };
    let aj = match leer_ajuste(ar, c, ajustes::FICHERO).and_then(|b| b.map(|b| pares(&b)).transpose()) {
        Ok(Some(p)) => {
            let (d, problemas) = ajustes::Discos::de_pares(p.iter().map(|(c, v)| (c.as_str(), v.as_str())));
            for x in problemas.iter().take(8) {
                dicho.push(alloc::format!("{}: {}", ajustes::FICHERO, x.texto()));
            }
            d
        }
        Ok(None) => ajustes::Discos::de_serie(),
        Err(porque) => {
            dicho.push(alloc::format!("{}: {}; lo de serie", ajustes::FICHERO, porque));
            ajustes::Discos::de_serie()
        }
    };
    (aj, dicho)
}

/// La vigilancia de `ENV:` y `ENV:Escritorio` (avisos de cambio por la puerta), como la del dock: así un cambio en
/// Ajustes, con la orden `Ajuste` o en un editor se ve en marcha.
struct Vigila {
    ids: [u32; 2],
}

const VIGILA: [&str; 2] = ["ENV:", "ENV:Escritorio"];

impl Vigila {
    /// Arma las que falten (sin sesión no hay `ENV:`: se vuelve a intentar al cambiar la sesión).
    fn armar(&mut self, ar: &Archivos, c: &mut Cliente, puerta: u32) {
        if puerta == 0 || !self.ids.contains(&0) {
            return;
        }
        let Some(tabla) = dashos::asignaciones::pedir_tabla(SESIONES) else { return };
        for (k, camino) in VIGILA.iter().enumerate() {
            if self.ids[k] != 0 {
                continue;
            }
            let Ok((r, _)) = dashos::asignaciones::abrir(&mut PorNombres { ar, c }, &tabla, camino.as_bytes()) else { continue };
            let copia = dashos::llave_duplicar(puerta, dashos::ESCRIBIR | dashos::TRANSFERIR | dashos::UNA_VEZ, 0);
            if copia > 0 {
                let (e, id, _, _) = dashos::vigilar::vigilar(ARCHIVOS, ar.lectura, r.asa, copia as u32, 2_000_000);
                if e == a::BIEN {
                    self.ids[k] = id;
                    println!("  [workbench] vigila {} (los ajustes de los discos)", camino);
                }
            }
            ar.cerrar(c, r.asa);
        }
    }

    /// Pide los avisos (los pone a cero) y suelta las vigilancias que acabaron. Si hubo alguno.
    fn avisos(&mut self, ar: &Archivos, c: &mut Cliente) -> bool {
        let r = ar.pedir(c, a::AVISOS, 0, 0, 0, b"");
        if r.e != a::BIEN {
            return false;
        }
        for i in 0..r.a as usize {
            if let Some((id, marcas, _)) = a::leer_aviso(ar.ventana(), i)
                && marcas & (a::AVISO_BORRADO | a::AVISO_FUERA) != 0
            {
                for x in self.ids.iter_mut().filter(|x| **x == id) {
                    *x = 0;
                }
            }
        }
        r.a > 0
    }
}

// ---------------------------------------------------------------------------------------------------------------
// El programa.

struct App {
    c: Cliente,
    ar: Archivos,
    tema: Tema,
    medio: Fuente,
    regular: Fuente,
    /// La colección de iconos (los de los volúmenes) y los símbolos de Material.
    iconos: kit::iconos::coleccion::Iconos,
    simbolos: kit::iconos::Simbolos,
    esc: Escritorio,
    /// La puerta de los avisos de cambio y la vigilancia de `ENV:`.
    puerta: u32,
    vigila: Vigila,
    ultimo_cuadro: u64,
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

    // --- Los discos del escritorio ---

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
        self.c.mandar(&e);
        if self.esc.aj.mostrar {
            self.crear_capa();
        } else {
            println!("  [workbench] discos del escritorio: escondidos (mostrar = no)");
        }
    }

    /// La capa de los discos: la de abajo (1), sin teclado ni zona propia (el compositor la pone en el hueco que dejan
    /// las barras). Primero, de arriba abajo en su lado y sin búfer, para saber el alto que hay; luego, del tamaño del
    /// panel (`pedir_tamano`). Solo el panel, no la pantalla: lo transparente de una capa se mezcla en cada composición.
    fn crear_capa(&mut self) {
        let m = self.esc.panel.m();
        let lado = if self.esc.aj.lado == ajustes::Lado::Derecha { 8 } else { 4 };
        let mut e = Escritor::new();
        e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUP_ESC).fin();
        e.empezar(CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(CAPA_ESC).uint(SUP_ESC).uint(0).uint(1).texto(Some("workbench")).fin();
        e.empezar(CAPA_ESC, zwlr_layer_surface_v1::req::SET_SIZE).uint(m.ancho as u32).uint(0).fin();
        e.empezar(CAPA_ESC, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(1 | 2 | lado).fin();
        e.empezar(CAPA_ESC, zwlr_layer_surface_v1::req::SET_EXCLUSIVE_ZONE).int(0).fin();
        e.empezar(CAPA_ESC, zwlr_layer_surface_v1::req::SET_KEYBOARD_INTERACTIVITY).uint(0).fin();
        e.empezar(SUP_ESC, wl_surface::req::COMMIT).fin();
        self.c.mandar(&e);
        self.esc.capa = true;
        self.esc.medida = false;
        self.esc.pedido = (m.ancho, 0, 1 | 2 | lado);
        self.esc.w = 0;
        self.esc.h = 0;
    }

    /// Quita la capa (al esconder los discos en los ajustes, o para hacerla de nuevo en el otro lado).
    fn quitar_capa(&mut self, esconder: bool) {
        self.cerrar_menu();
        if !self.esc.capa {
            return;
        }
        let mut e = Escritor::new();
        e.empezar(CAPA_ESC, zwlr_layer_surface_v1::req::DESTROY).fin();
        e.empezar(SUP_ESC, wl_surface::req::DESTROY).fin();
        if let Some(h) = self.esc.hoja.take() {
            if self.esc.bufer_tam.0 > 0 {
                e.empezar(self.esc.bufer, wl_buffer::req::DESTROY).fin();
            }
            h.soltar(&mut e);
        }
        self.c.mandar(&e);
        self.esc.capa = false;
        self.esc.bufer_tam = (0, 0);
        self.esc.w = 0;
        self.esc.h = 0;
        self.esc.dicho.clear();
        if esconder {
            println!("  [workbench] discos del escritorio: escondidos");
        }
    }

    /// El tamaño y las anclas que pide el panel ahora; se piden al compositor si cambiaron.
    fn pedir_tamano(&mut self) {
        if !self.esc.capa || !self.esc.medida {
            return;
        }
        let m = self.esc.panel.m();
        let (w, h) = (self.esc.panel.ancho_superficie(), self.esc.panel.alto_superficie());
        let lado = if self.esc.aj.lado == ajustes::Lado::Derecha { 8 } else { 4 };
        let alto = match self.esc.aj.altura {
            ajustes::Altura::Arriba => 1,
            ajustes::Altura::Centro => 0,
            ajustes::Altura::Abajo => 2,
        };
        if self.esc.pedido == (w, h, lado | alto) {
            return;
        }
        let mg = |b: u32| if (lado | alto) & b != 0 { m.margen } else { 0 };
        let mut e = Escritor::new();
        e.empezar(CAPA_ESC, zwlr_layer_surface_v1::req::SET_SIZE).uint(w as u32).uint(h as u32).fin();
        e.empezar(CAPA_ESC, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(lado | alto).fin();
        e.empezar(CAPA_ESC, zwlr_layer_surface_v1::req::SET_MARGIN).int(mg(1)).int(mg(8)).int(mg(2)).int(mg(4)).fin();
        e.empezar(SUP_ESC, wl_surface::req::COMMIT).fin();
        self.c.mandar(&e);
        self.esc.pedido = (w, h, lado | alto);
    }

    /// Los volúmenes de ahora (con la sesión, `Home:` y `RAM:`) y lo que mide cada uno, al panel. Dice lo que se montó y
    /// lo que se desmontó, y pide otro tamaño si hace falta.
    fn poner_volumenes(&mut self) {
        // Hasta el primer configure no se sabe cuántas caben: se ponen entonces.
        if !self.esc.medida {
            return;
        }
        let home = self.esc.home;
        let vols: Vec<Volumen> = self
            .esc
            .volumenes
            .iter()
            .filter(|&&(_, r)| (r != RAIZ_HOME && r != RAIZ_RAM) || home)
            .map(|&(nombre, raiz)| {
                let (mide, libre) = self.esc.medidas.get(&raiz).copied().unwrap_or((0, 0));
                Volumen { nombre: String::from(nombre), raiz, icono: icono_volumen(raiz), mide, libre }
            })
            .collect();
        let cambios = self.esc.panel.poner(&vols);
        if !cambios.montados.is_empty() {
            println!("  [workbench] discos: entra {}", cambios.montados.iter().map(|n| alloc::format!("{}:", n)).collect::<Vec<_>>().join(" "));
        }
        if !cambios.desmontados.is_empty() {
            println!("  [workbench] discos: sale {}", cambios.desmontados.iter().map(|n| alloc::format!("{}:", n)).collect::<Vec<_>>().join(" "));
        }
        if cambios.fuera > 0 {
            println!("  [workbench] discos: {} no caben en la pantalla", cambios.fuera);
        }
        self.pedir_tamano();
    }

    /// Lo que mide y tiene libre cada volumen (`INFO` por su raíz). Si cambió algo, al panel.
    fn medir_volumenes(&mut self) {
        let mut cambia = false;
        let raices: Vec<u32> = self.esc.volumenes.iter().map(|v| v.1).filter(|&r| (r != RAIZ_HOME && r != RAIZ_RAM) || self.esc.home).collect();
        for raiz in raices {
            let r = self.ar.pedir(&mut self.c, a::INFO, raiz, 0, 0, b"");
            let m = if r.e == a::BIEN { (r.a, r.b) } else { (0, 0) };
            if self.esc.medidas.insert(raiz, m) != Some(m) {
                cambia = true;
            }
        }
        if cambia {
            self.poner_volumenes();
        }
    }

    /// El configure de la capa: la primera vez es para medir (de arriba abajo); luego, el tamaño del panel.
    fn escritorio_configurado(&mut self, serie: u32, w: i32, h: i32) {
        let mut e = Escritor::new();
        e.empezar(CAPA_ESC, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(serie).fin();
        self.c.mandar(&e);
        if !self.esc.medida {
            // El alto que dejan las barras: cuántas tarjetas caben. Y ya, el tamaño de verdad.
            self.esc.medida = true;
            self.esc.panel.alto_pantalla = h.clamp(1, 4320);
            self.poner_volumenes();
            self.pedir_tamano();
            return;
        }
        let (w, h) = (w.clamp(1, 3840), h.clamp(1, 4320));
        let otro_tamano = (w, h) != (self.esc.w, self.esc.h);
        self.esc.w = w;
        self.esc.h = h;
        self.pintar_escritorio();
        if otro_tamano {
            self.decir_discos(w, h);
        }
    }

    /// Dónde queda cada disco (en la capa; el compositor dice dónde está la capa): las pruebas pulsan ahí.
    fn decir_discos(&mut self, w: i32, h: i32) {
        let mut sitios = Vec::new();
        for k in 0..self.esc.panel.tarjetas.len() {
            if let Some(c) = self.esc.panel.caja_final(k) {
                sitios.push((self.esc.panel.tarjetas[k].vol.nombre.clone(), c.x + c.w / 2, c.y + c.h / 2));
            }
        }
        if sitios.is_empty() || sitios == self.esc.dicho {
            return;
        }
        let primera = self.esc.dicho.is_empty();
        if primera {
            let (n, x, y) = &sitios[0];
            println!("  [workbench] en pantalla: {}x{}, icono de {} en ({}, {})", w, h, n, x, y);
        }
        for (n, x, y) in sitios.iter().skip(if primera { 1 } else { 0 }) {
            println!("  [workbench] icono de {} en ({}, {})", n, x, y);
        }
        if !sitios.iter().any(|s| s.0 == "Home") && !self.esc.home {
            println!("  [workbench] sin icono de Home ni de RAM: la sesión de Ana no está abierta");
        }
        self.esc.dicho = sitios;
    }

    fn pintar_escritorio(&mut self) {
        if !self.esc.capa || self.esc.w == 0 {
            return;
        }
        let (w, h) = (self.esc.w, self.esc.h);
        let mut e = Escritor::new();
        if self.esc.bufer_tam != (w, h) || self.esc.hoja.is_none() {
            if self.esc.bufer_tam.0 > 0 {
                e.empezar(self.esc.bufer, wl_buffer::req::DESTROY).fin();
                self.esc.bufer_tam = (0, 0);
            }
            let bytes = (w * h * 4) as usize;
            if self.esc.hoja.as_ref().is_none_or(|x| x.tamano < bytes) {
                if let Some(x) = self.esc.hoja.take() {
                    x.soltar(&mut e);
                }
                let pool = self.ids(1);
                match Lamina::nueva(&mut e, bytes.next_multiple_of(4096), VA, pool) {
                    Some(x) => self.esc.hoja = Some(x),
                    None => {
                        println!("  [workbench] sin memoria para los discos ({}x{})", w, h);
                        self.c.mandar(&e);
                        return;
                    }
                }
            }
            let bufer = self.ids(1);
            let pool = self.esc.hoja.as_ref().unwrap().pool;
            e.empezar(pool, wl_shm_pool::req::CREATE_BUFFER).uint(bufer).int(0).int(w).int(h).int(w * 4).uint(wayland::shm::ARGB8888).fin();
            self.esc.bufer = bufer;
            self.esc.bufer_tam = (w, h);
        }
        let ahora = dashos::ahora_us();
        let px = self.esc.hoja.as_mut().unwrap().px((w * h) as usize);
        pintar_panel(px, w, h, &self.esc.panel, &self.tema, &mut self.medio, &mut self.regular, &mut self.iconos, &self.simbolos, ahora);
        e.empezar(SUP_ESC, wl_surface::req::ATTACH).uint(self.esc.bufer).int(0).int(0).fin();
        e.empezar(SUP_ESC, wl_surface::req::DAMAGE).int(0).int(0).int(w).int(h).fin();
        e.empezar(SUP_ESC, wl_surface::req::COMMIT).fin();
        self.c.mandar(&e);
    }

    /// Mira si la sesión de Ana está abierta: con ella, `Home:` y `RAM:` entran en el panel (y salen sin ella).
    fn mirar_home(&mut self) {
        let abierta = sesion_abierta(&mut self.c);
        if abierta != self.esc.home {
            self.esc.home = abierta;
            self.medir_volumenes();
            self.poner_volumenes();
            // Con la sesión hay `ENV:`: sus ajustes (y vigilarlos).
            self.releer_ajustes();
            let p = self.puerta;
            self.vigila.armar(&self.ar, &mut self.c, p);
        }
    }

    /// Lee los ajustes de los discos y los pone en marcha si cambiaron.
    fn releer_ajustes(&mut self) {
        let (aj, dicho) = leer_ajustes(&self.ar, &mut self.c);
        for d in &dicho {
            println!("  [workbench] {}", d);
        }
        if aj == self.esc.aj {
            return;
        }
        println!(
            "  [workbench] discos: ajustes en marcha: {}, a la {}, {}, {}, opacidad {} %",
            if aj.mostrar { "a la vista" } else { "escondidos" },
            if aj.lado == ajustes::Lado::Derecha { "derecha" } else { "izquierda" },
            match aj.altura {
                ajustes::Altura::Arriba => "arriba",
                ajustes::Altura::Centro => "en el centro",
                ajustes::Altura::Abajo => "abajo",
            },
            aj.tamano.nombre(),
            aj.opacidad
        );
        let antes = self.esc.aj.clone();
        self.esc.aj = aj.clone();
        self.esc.panel.poner_ajustes(aj.clone());
        if !aj.mostrar {
            self.quitar_capa(true);
            return;
        }
        if !self.esc.capa || antes.lado != aj.lado {
            // Otro lado: la capa se hace de nuevo (y vuelve a entrar).
            self.quitar_capa(false);
            let vols = self.esc.panel.tarjetas.iter().map(|t| t.vol.clone()).collect::<Vec<_>>();
            self.esc.panel = Panel::nuevo(aj, self.esc.panel.alto_pantalla);
            self.esc.panel.poner(&vols);
            self.crear_capa();
            return;
        }
        self.esc.dicho.clear();
        self.pedir_tamano();
        self.pintar_escritorio();
    }

    fn raton_escritorio(&mut self, boton: u32, pulsado: bool, ms: u32) {
        if !pulsado {
            return;
        }
        let bajo = self.esc.panel.bajo(self.x, self.y);
        match (bajo, boton) {
            (Some(k), BOTON_IZQUIERDO) => {
                self.esc.panel.pulsar(k, self.x, self.y, dashos::ahora_us());
                let doble = matches!(self.esc.ultimo, Some((u, t)) if u == k && ms.wrapping_sub(t) <= DOBLE_MS);
                self.esc.ultimo = if doble { None } else { Some((k, ms)) };
                self.pintar_escritorio();
                if doble {
                    let raiz = self.esc.panel.tarjetas[k].vol.raiz;
                    self.abrir_volumen(raiz);
                }
            }
            (Some(k), BOTON_DERECHO) => {
                self.esc.panel.pulsar(k, self.x, self.y, dashos::ahora_us());
                self.esc.ultimo = None;
                self.pintar_escritorio();
                self.abrir_menu(k);
            }
            _ => {
                self.esc.ultimo = None;
                if self.esc.panel.elegido.take().is_some() {
                    self.pintar_escritorio();
                }
            }
        }
    }

    /// El menú de la tarjeta `k`, al lado del panel que da a la pantalla, a la altura de la tarjeta.
    fn abrir_menu(&mut self, k: usize) {
        self.cerrar_menu();
        let Some(c) = self.esc.panel.caja_final(k) else { return };
        let t = &self.esc.panel.tarjetas[k];
        let opciones = alloc::vec![Opcion::Abrir, Opcion::Informacion];
        let tam = (MENU_ANCHO, 2 * MENU_RELLENO + opciones.len() as i32 * MENU_FILA);
        // En la capa del menú, anclada como la de los discos, apartada hacia dentro de la pantalla.
        let m = self.esc.panel.m();
        let lado = if self.esc.aj.lado == ajustes::Lado::Derecha { 8 } else { 4 };
        let (_, _, anclas) = self.esc.pedido;
        let arriba_capa = match self.esc.aj.altura {
            ajustes::Altura::Arriba => m.margen,
            _ => 0,
        };
        let mut e = Escritor::new();
        e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUP_MENU).fin();
        e.empezar(CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(CAPA_MENU).uint(SUP_MENU).uint(0).uint(3).texto(Some("workbench-menu")).fin();
        e.empezar(CAPA_MENU, zwlr_layer_surface_v1::req::SET_SIZE).uint(tam.0 as u32).uint(tam.1 as u32).fin();
        if anclas & 1 != 0 {
            // Arriba: a la altura de la tarjeta.
            e.empezar(CAPA_MENU, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(1 | lado).fin();
            let y = arriba_capa + c.y;
            let x = m.margen + m.ancho + 4;
            e.empezar(CAPA_MENU, zwlr_layer_surface_v1::req::SET_MARGIN).int(y).int(if lado == 8 { x } else { 0 }).int(0).int(if lado == 4 { x } else { 0 }).fin();
        } else {
            // En el centro o abajo: al lado del panel, centrado o pegado abajo como él.
            e.empezar(CAPA_MENU, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(anclas & 2 | lado).fin();
            let x = m.margen + m.ancho + 4;
            e.empezar(CAPA_MENU, zwlr_layer_surface_v1::req::SET_MARGIN).int(0).int(if lado == 8 { x } else { 0 }).int(if anclas & 2 != 0 { m.margen } else { 0 }).int(if lado == 4 { x } else { 0 }).fin();
        }
        // Sin zona (0), como la de los discos: las dos en el hueco que dejan las barras, y los márgenes cuadran.
        e.empezar(CAPA_MENU, zwlr_layer_surface_v1::req::SET_EXCLUSIVE_ZONE).int(0).fin();
        // El teclado, a petición (Esc lo cierra si se pulsó en él): uno exclusivo no dejaría escribir en otra ventana
        // mientras el menú esté abierto.
        e.empezar(CAPA_MENU, zwlr_layer_surface_v1::req::SET_KEYBOARD_INTERACTIVITY).uint(2).fin();
        e.empezar(SUP_MENU, wl_surface::req::COMMIT).fin();
        self.c.mandar(&e);
        let nombres: Vec<&str> = opciones.iter().map(|o| texto_opcion(*o)).collect();
        println!("  [workbench] menú de {}: {}", t.vol.nombre, nombres.join(" · "));
        let ahora = dashos::ahora_us();
        let aparece = if self.esc.aj.animaciones { muelle::Tramo::quieto(0).hacia(ahora, 255, 75_000, muelle::Curva::SalidaCuartica) } else { muelle::Tramo::quieto(255) };
        self.esc.menu = Some(MenuDiscos { nombre: t.vol.nombre.clone(), raiz: t.vol.raiz, opciones, tam, encima: None, hoja: None, bufer: 0, bufer_tam: (0, 0), listo: false, aparece, fuera_desde: None });
    }

    fn repintar_menu(&mut self) {
        let Some(m) = self.esc.menu.as_mut() else { return };
        if !m.listo {
            return;
        }
        let (w, h) = m.tam;
        let mut e = Escritor::new();
        if m.hoja.is_none() {
            let pool = self.siguiente_id;
            self.siguiente_id += 2;
            let Some(x) = Lamina::nueva(&mut e, ((w * h * 4) as usize).next_multiple_of(4096), VA_MENU, pool) else {
                self.c.mandar(&e);
                return;
            };
            e.empezar(pool, wl_shm_pool::req::CREATE_BUFFER).uint(pool + 1).int(0).int(w).int(h).int(w * 4).uint(wayland::shm::ARGB8888).fin();
            m.hoja = Some(x);
            m.bufer = pool + 1;
            m.bufer_tam = (w, h);
        }
        let ahora = dashos::ahora_us();
        let mut hoja = m.hoja.take().unwrap();
        pintar_menu(hoja.px((w * h) as usize), m, &self.tema, &mut self.medio, &self.simbolos, ahora);
        m.hoja = Some(hoja);
        e.empezar(SUP_MENU, wl_surface::req::ATTACH).uint(m.bufer).int(0).int(0).fin();
        e.empezar(SUP_MENU, wl_surface::req::DAMAGE).int(0).int(0).int(w).int(h).fin();
        e.empezar(SUP_MENU, wl_surface::req::COMMIT).fin();
        self.c.mandar(&e);
    }

    fn cerrar_menu(&mut self) {
        let Some(mut m) = self.esc.menu.take() else { return };
        let mut e = Escritor::new();
        e.empezar(CAPA_MENU, zwlr_layer_surface_v1::req::DESTROY).fin();
        e.empezar(SUP_MENU, wl_surface::req::DESTROY).fin();
        if let Some(h) = m.hoja.take() {
            e.empezar(m.bufer, wl_buffer::req::DESTROY).fin();
            h.soltar(&mut e);
        }
        self.c.mandar(&e);
        println!("  [workbench] menú cerrado");
    }

    /// La fila del menú bajo (`x`, `y`) de su capa.
    fn fila_menu(&self, x: i32, y: i32) -> Option<usize> {
        let m = self.esc.menu.as_ref()?;
        if x < 4 || x >= m.tam.0 - 4 || y < MENU_RELLENO {
            return None;
        }
        let k = ((y - MENU_RELLENO) / MENU_FILA) as usize;
        (k < m.opciones.len()).then_some(k)
    }

    fn elegir_menu(&mut self, k: usize) {
        let Some(m) = self.esc.menu.as_ref() else { return };
        let (o, raiz, nombre) = (m.opciones[k], m.raiz, m.nombre.clone());
        println!("  [workbench] menú de {}: {}", nombre, texto_opcion(o));
        self.cerrar_menu();
        match o {
            Opcion::Abrir => self.abrir_volumen(raiz),
            Opcion::Informacion => self.abrir_info(raiz, false, alloc::format!("{}:", nombre), None, true),
        }
    }

    /// Un cuadro: avanza lo que se mueve y pinta si algo cambió. Si sigue moviéndose.
    fn cuadro(&mut self) -> bool {
        let ahora = dashos::ahora_us();
        // Como mucho un cuadro cada 16,7 ms: cada commit trae un aviso del compositor (el búfer suelto) que despierta
        // el bucle, y sin esto se pintaba en cuanto llegaba, más deprisa que la pantalla.
        if ahora.saturating_sub(self.ultimo_cuadro) + 1_000 < CUADRO_US && self.esc.panel.animando() {
            return true;
        }
        let paso = ahora.saturating_sub(self.ultimo_cuadro).min(100_000);
        self.ultimo_cuadro = ahora;
        // Hasta que la capa está en pantalla (dos idas y vueltas con el compositor), nada se mueve: si no, con el
        // sistema ocupado la entrada acababa antes del primer cuadro y no se veía.
        let en_pantalla = self.esc.capa && self.esc.w > 0 && self.esc.bufer_tam.0 > 0;
        if en_pantalla && self.esc.panel.avanzar(ahora, paso) {
            self.pintar_escritorio();
        }
        if let Some(m) = self.esc.menu.as_ref()
            && m.listo
            && !m.aparece.acabado(ahora.saturating_sub(paso))
        {
            self.repintar_menu();
        }
        let tras = self.esc.panel.alto_superficie();
        if tras != self.esc.pedido.1 {
            self.pedir_tamano();
        }
        // El ratón se fue de los discos y del menú (a otra ventana, de la que no llega nada): el menú se cierra.
        if self.esc.menu.as_ref().and_then(|m| m.fuera_desde).is_some_and(|d| ahora.saturating_sub(d) >= MENU_FUERA_US) {
            self.cerrar_menu();
        }
        self.esc.panel.animando() || self.esc.menu.as_ref().is_some_and(|m| !m.aparece.acabado(ahora))
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

    /// Pide al explorador que abra un lugar: "abrir", la raíz y los nombres separados por un 0. Verdad si se lo pidió.
    fn al_explorador(&self, lugar: &Lugar) -> bool {
        let mut m = Vec::from(&b"abrir"[..]);
        m.push(lugar.raiz as u8);
        for (i, n) in lugar.nombres.iter().enumerate() {
            if i > 0 {
                m.push(0);
            }
            m.extend_from_slice(n.as_bytes());
        }
        if m.len() > 240 {
            return false;
        }
        let e = dashos::canal_escribir(EXPLORADOR, &m, &[]);
        println!("  [workbench] pide al explorador que abra {} ({})", lugar.texto(), e);
        e == 0
    }

    /// Abre la ventana de un volumen (su raíz): en el explorador, o aquí si no está.
    fn abrir_volumen(&mut self, raiz: u32) {
        let lugar = Lugar { raiz, nombres: Vec::new() };
        if self.al_explorador(&lugar) {
            return;
        }
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
        let cerrar = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto(tr(9, "Cerrar").into())), (atr::ESTILO, Valor::Int(2))]);
        let estado = texto_obj(&mut arbol, "", 2);
        let vista = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto(tr(10, "Lista").into())), (atr::ESTILO, Valor::Int(1))]);
        let info = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto(tr(11, "Información").into())), (atr::ESTILO, Valor::Int(1))]);
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
        i.esquinas = kit::tema::RADIO_VENTANA;
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
            None if cj.entradas.len() == 1 => String::from(tr(12, "1 elemento")),
            None => trf(13, "{} elementos", &[&cj.entradas.len()]),
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
                if estado == 0 { trf(14, "En marcha: {}", &[&e.nombre]) } else { trf(15, "No se lanzó ({})", &[&estado]) }
            } else {
                println!("  [workbench] {} no se abre: solo se lanzan los programas de SYS:Programas", ruta);
                String::from(tr(16, "Solo se lanzan los de Programas"))
            };
            self.aviso_cajon(k, &aviso);
            return;
        }
        let mut nombres = lugar.nombres.clone();
        nombres.push(e.nombre.clone());
        if self.al_explorador(&Lugar { raiz: lugar.raiz, nombres }) {
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
                let aviso = if err == a::PROTEGIDO { trf(17, "Sin permiso para abrir {}", &[&e.nombre]) } else { trf(18, "No se abre {} ({})", &[&e.nombre, &err]) };
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
        v.i.arbol.poner(cj.vista, atr::TEXTO, Valor::Texto(String::from(if cj.lista { tr(19, "Iconos") } else { tr(10, "Lista") })));
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
                    self.aviso_cajon(k, &trf(20, "Sin acceso a {}", &[&nombre]));
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
        // Por SYS: solo se lee; por Home:, Work: y RAM: (raíces de leer y escribir), su dueño lo cambia. Antes solo por
        // Home: (la Información de Work: y RAM: desde los discos decía "Por SYS").
        let puede = dueno == yo && raiz != RAIZ_SISTEMA;
        let porque = if raiz == RAIZ_SISTEMA {
            tr(21, "Por SYS: solo se puede leer")
        } else if dueno != yo {
            tr(22, "Solo lo puede cambiar su dueño")
        } else {
            ""
        };
        let nombre = ruta.rsplit('/').next().map_or_else(|| ruta.clone(), String::from);
        // Lo que es, para la consola (en castellano); la línea de la ventana, entera y por catálogo.
        let que = match (tipo, padre.is_none()) {
            (a::CAJON, true) if !ruta.contains('/') => String::from("Volumen"),
            (a::CAJON, _) => String::from("Cajón"),
            _ if modo & 0o111 != 0 => alloc::format!("Programa · {}", tamano(d.a)),
            _ => alloc::format!("Fichero · {}", tamano(d.a)),
        };
        let mut arbol = Arbol::new();
        let titulo = texto_obj(&mut arbol, &nombre, 1);
        let cambiado = fecha(d.b);
        let que_es = match (tipo, padre.is_none()) {
            (a::CAJON, true) if !ruta.contains('/') => trf(23, "Volumen · cambiado el {}", &[&cambiado]),
            (a::CAJON, _) => trf(24, "Cajón · cambiado el {}", &[&cambiado]),
            _ if modo & 0o111 != 0 => trf(25, "Programa · {} · cambiado el {}", &[&tamano_visible(d.a), &cambiado]),
            _ => trf(26, "Fichero · {} · cambiado el {}", &[&tamano_visible(d.a), &cambiado]),
        };
        let linea1 = texto_obj(&mut arbol, &que_es, 2);
        let tu = if dueno == yo { " (tú)" } else { "" };
        let de_quien = if dueno == yo {
            trf(27, "Dueño: usuario {} (tú) · grupo {}", &[&dueno, &grupo])
        } else {
            trf(28, "Dueño: usuario {} · grupo {}", &[&dueno, &grupo])
        };
        let linea2 = texto_obj(&mut arbol, &de_quien, 2);
        let campo = arbol.crear(
            Campo,
            &[(atr::TEXTO, Valor::Texto(comentario.clone())), (atr::ETIQUETA, Valor::Texto(tr(29, "Comentario").into())), (atr::DESACTIVADO, Valor::Bool(!puede))],
        );
        let valor = bits << 16 | modo;
        let rejilla = arbol.crear(Permisos, &[(atr::VALOR, Valor::Int(valor)), (atr::CURSOR, Valor::Int(-1)), (atr::DESACTIVADO, Valor::Bool(!puede))]);
        let aviso = texto_obj(&mut arbol, porque, 2);
        let hueco = texto_obj(&mut arbol, "", 0);
        let cancelar = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto(String::from(if puede { tr(30, "Cancelar") } else { tr(9, "Cerrar") }))), (atr::ESTILO, Valor::Int(2))]);
        let guardar = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto(tr(31, "Guardar").into()))]);
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
        i.esquinas = kit::tema::RADIO_VENTANA;
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
        let titulo_v = trf(32, "Información de {}", &[&nombre]);
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
                fallo = Some(if e == a::MAL_FORMADO { tr(33, "El comentario es demasiado largo") } else { tr(34, "No se pudo cambiar el comentario") });
            }
        }
        if valor != valor_viejo {
            let (bits, modo) = ((valor >> 16) as u64, (valor & 0o7777) as u64);
            let e = self.ar.pedir(&mut self.c, a::PROTEGER, asa, bits, modo, b"").e;
            partes.push(alloc::format!("protección bits {:#x} modo {:o}: {}", bits, modo, e));
            if e != a::BIEN {
                fallo = Some(tr(35, "No se pudo cambiar la protección"));
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
        "  [workbench] lo ajeno: listar el cajón de sesiones {}, abrir su registro {}; listar DeCarla {}, abrir su fichero {}; crear por SYS: {}; \"..\" desde Home: {}: {}",
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
        "  [workbench] nombres raros: {} sin enseñar en Home:, {} en SYS:Imagenes; \"..\", \"a/b\", \"x:y\" no se buscan: {}",
        raros_home, raros_img, if bien { "bien" } else { "MAL" }
    );
}

fn principal() -> u64 {
    dashos::monton::con_nombre(&"workbench");
    // Arranca de los últimos, con el compositor atendiendo a muchos: si su cola está llena, se reintenta un rato.
    // Con una puerta: por ella llegan los avisos de cambio de `ENV:` (los ajustes de los discos).
    let mut cliente = None;
    for _ in 0..100 {
        cliente = Cliente::conectar_con_puerta(MARCA_TIMBRE);
        if cliente.is_some() {
            break;
        }
        dashos::dormir_us(50_000);
    }
    let Some((mut c, puerta)) = cliente else {
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
    let [sistema, home, work] = ar.ceder_raices(&mut c);
    println!("  [workbench] raíces: SYS: solo para leer {}, Home: {}, Work: {}; RAM: la pone sesiones al entrar", sistema, home, work);
    if sistema != a::BIEN || home != a::BIEN {
        return 5;
    }
    comprobar_lo_ajeno(&ar, &mut c);
    // Idiomas (paso 4): con sus raíces ya cedidas y antes de hacer ninguna interfaz, el idioma del usuario y su
    // catálogo. La tabla de nombres es la misma que da luego los volúmenes; si no llega, se sigue en castellano.
    let tabla = dashos::asignaciones::pedir_tabla(SESIONES);
    if let Some(t) = &tabla {
        let _ = idiomas::arrancar(&mut PorNombres { ar: &ar, c: &mut c }, t, &TEXTOS, CATALOGO);
    }
    // Los discos del escritorio: sus ajustes (sin sesión, los guardados de fábrica o los de serie).
    let (aj, dicho) = leer_ajustes(&ar, &mut c);
    for d in &dicho {
        println!("  [workbench] {}", d);
    }
    let volumenes = volumenes_de_la_tabla(tabla.as_ref());
    let mut app = App {
        c,
        ar,
        tema: Tema::CLARO,
        medio: Fuente::medium(),
        regular: Fuente::regular(),
        iconos: kit::iconos::coleccion::Iconos::new(),
        simbolos: kit::iconos::Simbolos::new(),
        esc: Escritorio {
            panel: Panel::nuevo(aj.clone(), 0),
            aj,
            capa: false,
            medida: false,
            pedido: (0, 0, 0),
            w: 0,
            h: 0,
            hoja: None,
            bufer: 0,
            bufer_tam: (0, 0),
            home: false,
            volumenes,
            medidas: BTreeMap::new(),
            ultimo: None,
            menu: None,
            dicho: Vec::new(),
        },
        puerta,
        vigila: Vigila { ids: [0; 2] },
        ultimo_cuadro: dashos::ahora_us(),
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
    app.medir_volumenes();
    app.crear_escritorio(gc, gs, gx, ga, gl, gt);
    app.vigila.armar(&app.ar, &mut app.c, puerta);
    let mut mirar = dashos::ahora_us() + CADA_ACTIVA_US;
    let mut medir = dashos::ahora_us() + CADA_MEDIR_US;
    let mut animando = true;
    let mut cpu = cpu_propia::CpuPropia::new("workbench");
    loop {
        // El plazo: un cuadro si algo se mueve; si no, lo siguiente que toque (sin despertar en balde).
        let ahora = dashos::ahora_us();
        let mut plazo = mirar.saturating_sub(ahora).clamp(1, 500_000);
        if animando || app.esc.menu.as_ref().is_some_and(|m| m.fuera_desde.is_some()) {
            plazo = plazo.min(CUADRO_US);
        }
        let ev = match app.c.evento_plazo(plazo) {
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
        if dashos::ahora_us() >= medir {
            app.medir_volumenes();
            medir = dashos::ahora_us() + CADA_MEDIR_US;
        }
        // Los avisos de cambio de `ENV:`: los ajustes de los discos otra vez.
        if app.c.timbres > 0 {
            app.c.timbres = 0;
            if app.vigila.avisos(&app.ar, &mut app.c) {
                app.releer_ajustes();
            }
            app.vigila.armar(&app.ar, &mut app.c, puerta);
        }
        animando = app.cuadro();
        cpu.vuelta(animando);
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
                println!("  [workbench] el compositor cerró los discos");
                app.quitar_capa(false);
            }
            (CAPA_MENU, zwlr_layer_surface_v1::ev::CONFIGURE) => {
                let mut e = Escritor::new();
                e.empezar(CAPA_MENU, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                app.c.mandar(&e);
                if let Some(m) = app.esc.menu.as_mut() {
                    if !m.listo {
                        m.aparece = if app.esc.aj.animaciones { muelle::Tramo::quieto(0).hacia(dashos::ahora_us(), 255, 75_000, muelle::Curva::SalidaCuartica) } else { muelle::Tramo::quieto(255) };
                    }
                    m.listo = true;
                }
                app.repintar_menu();
                animando = true;
            }
            (CAPA_MENU, zwlr_layer_surface_v1::ev::CLOSED) => app.cerrar_menu(),
            (TECLADO, wl_keyboard::ev::KEY) if app.esc.menu.is_some() && app.con_teclado == SUP_MENU => {
                if palabra(3) == 1 && palabra(2) == KEY_ESC {
                    app.cerrar_menu();
                }
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
                // Bajo el ratón, en los discos: la capa de estado de la tarjeta (se anima en los cuadros siguientes).
                let encima = if app.sobre == SUP_ESC { app.esc.panel.bajo(app.x, app.y) } else { None };
                if app.esc.panel.poner_encima(encima) {
                    animando = true;
                }
                // En el menú: la fila de encima. Fuera de los discos y del menú, el menú se cierra en un momento.
                let fila = if app.sobre == SUP_MENU { app.fila_menu(app.x, app.y) } else { None };
                let sobre = app.sobre;
                if let Some(m) = app.esc.menu.as_mut() {
                    m.fuera_desde = if sobre == SUP_MENU || sobre == SUP_ESC { None } else { m.fuera_desde.or(Some(dashos::ahora_us())) };
                    if m.encima != fila {
                        m.encima = fila;
                        app.repintar_menu();
                    }
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
                if app.sobre == SUP_MENU {
                    if pulsado && let Some(k) = app.fila_menu(app.x, app.y) {
                        app.elegir_menu(k);
                    }
                } else if app.sobre == SUP_ESC {
                    if app.esc.menu.is_some() && pulsado {
                        app.cerrar_menu();
                    }
                    app.raton_escritorio(boton, pulsado, ms);
                    animando = true;
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
