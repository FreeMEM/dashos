//! El explorador de archivos de DashOS ("Archivos"), como Nautilus en lo que hace y en cómo se usa, con el aspecto del
//! escritorio (Material 3, los colores del fondo, claro y oscuro). Es un servicio del escritorio, como el Workbench: el
//! Workbench le pasa el doble clic en un volumen y él abre la ventana. Puede tener varias ventanas, y cada una sus
//! pestañas.
//!
//! - Cada ventana: barra lateral con los lugares (carpeta personal, papelera, Sistema y el cajón de la caja Amiga, con
//!   su marca 68K), barra de arriba con Atrás, Adelante, el camino con migas que se pulsan (Ctrl+L para escribirlo),
//!   Buscar, la vista (rejilla o lista con nombre, tamaño, tipo y fecha, ordenada por columna) y el menú; pestañas.
//! - Selección con clic, Ctrl, Mayúsculas, rectángulo y Ctrl+A; el teclado entero (flechas, Intro, Retroceso, F2,
//!   Supr, Ctrl+L, escribir para buscar).
//! - Carpeta nueva, cambiar el nombre, copiar, cortar y pegar, papelera (tirar, restaurar, vaciar), borrar de verdad
//!   con pregunta, arrastrar y soltar entre sus ventanas. Lo largo va a trozos (`explorador/ops.rs`), con progreso y
//!   Cancelar, sin colgar la ventana.
//! - Abrir con doble clic (los programas de `Programas`, por `sesiones`, como el Workbench), Abrir con, Propiedades
//!   (la rejilla de permisos de la Información del Workbench), búsqueda en las subcarpetas y miniaturas de los PNG.
//!
//! Seguridad (S6b): la ficha es de Ana (1000) con el derecho de repartir. Se cede a sí mismo sus raíces, en los huecos de
//! cada volumen del protocolo (`RAIZ_*`, paso 2 de las asignaciones): el disco solo para leer (1, `SYS:`), `C` para leer
//! y escribir (2, `Home:`, provisional como en el Workbench), `Work:` (4) y el cajón de la caja Amiga (5). `RAM:` (3)
//! es el volumen en memoria de la sesión: se lo pone `sesiones` al entrar su usuario. Los caminos se escriben como en el Amiga: `SYS:Programas`, `Home:Privado/nota.txt`. Solo va por ellas, por asas y un nombre cada vez; los nombres raros ni se enseñan ni se abren
//! (`explorador/logica.rs`). Copiar es leer y escribir con sus permisos: el servicio no hace nada que su ficha no deje.
//!
//! Llaves: 1 el canal "ventanas", 2 "archivos", 3 "sesiones" (lanzar y saber quién está) y 4 su canal "explorador",
//! donde el Workbench le pide abrir un lugar.

#![no_std]
#![no_main]
#![deny(unsafe_code)]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
#[allow(dead_code, unsafe_code)]
mod ventana_cliente;
#[path = "../permisos_kit.rs"]
mod permisos_kit;
#[path = "../explorador/logica.rs"]
#[allow(dead_code)]
mod logica;
#[path = "../explorador/ops.rs"]
#[allow(dead_code)]
mod ops;
#[path = "../explorador/pintar.rs"]
mod pintar;
#[path = "../explorador/iconos.rs"]
mod iconos;
#[path = "../explorador/iconos_amiga.rs"]
mod iconos_amiga;
#[path = "../idiomas.rs"]
mod idiomas;

/// Lo único con `unsafe`: ver como bytes o píxeles la memoria proyectada (la ventana del servicio y los búferes).
#[allow(unsafe_code)]
mod memoria {
    pub fn bytes(va: usize, tam: usize) -> &'static mut [u8] {
        // La memoria está proyectada en `va` con `tam` bytes, y solo se usa desde un hilo.
        unsafe { core::slice::from_raw_parts_mut(va as *mut u8, tam) }
    }

    pub fn pixeles(va: usize, n: usize) -> &'static mut [u32] {
        unsafe { core::slice::from_raw_parts_mut(va as *mut u32, n) }
    }
}

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use dashos::println;
pub use dashos::protocolo::archivos;
use dashos::protocolo::archivos as a;
use dashos::protocolo::sesiones as s;
use kit::controles::{Boton, Campo, Contexto, Hijo, Interfaz, Texto, DOBLE_MS};
use kit::lienzo::{Lienzo, Rect};
use kit::objetos::{atr, Arbol, Cuando, Obj, Que, Valor};
use kit::teclado::{Tecla, Teclado};
use kit::tema::Tema;
use kit::texto::Fuente;
use logica::*;
use ops::{Elemento, Fs, Resp, Tarea, Trabajo};
use ventana_cliente::*;
use wayland::tablas::{
    dashos_tema_v1, wl_buffer, wl_compositor, wl_display, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool,
    wl_surface, xdg_surface, xdg_toplevel, xdg_wm_base,
};
use wayland::Escritor;

dashos::programa!(principal);
dashos::monton!();

/// Idiomas (paso 4): el nombre de su catálogo y su versión (`tools/catalogo.py` lo lee de aquí), y el catálogo en uso.
const CATALOGO: (&str, u32) = ("explorador", 1);
static TEXTOS: idiomas::Puesto = idiomas::Puesto::nuevo();

/// Un texto, en el idioma del usuario (el castellano si no hay catálogo). Lo de la consola no pasa por aquí: va
/// siempre en castellano.
pub fn tr(numero: u32, de_siempre: &'static str) -> &'static str {
    TEXTOS.texto(numero, de_siempre)
}

/// Un texto con huecos y sus valores.
pub fn trf(numero: u32, de_siempre: &'static str, valores: &[&dyn core::fmt::Display]) -> String {
    TEXTOS.con(numero, de_siempre, valores)
}

const ARCHIVOS: u32 = 2;
const SESIONES: u32 = 3;
const CANAL: u32 = 4;
const TEMA: u32 = 22;
const PRIMER_ID: u32 = 100;
const VA_ARCHIVOS: usize = 0x50_0000_0000;
/// La ventana que se pide al servicio: más grande que la de siempre, para copiar a trozos más grandes.
const VENTANA_SERVICIO: usize = 256 * 1024;
const VA_VENTANAS: usize = 0x60_0000_0000;
/// Cada ventana, dos huecos (el búfer de ahora y el de un tamaño nuevo) de 32 MB.
const HUECO: usize = 32 << 20;
pub const MAX_VENTANAS: usize = 4;
const MAX_PESTANAS: usize = 8;
pub const RAIZ_SISTEMA: u32 = s::RAIZ_SYS as u32;
pub const RAIZ_HOME: u32 = s::RAIZ_HOME as u32;
pub const RAIZ_RAM: u32 = s::RAIZ_RAM as u32;
pub const RAIZ_WORK: u32 = s::RAIZ_WORK as u32;
/// El cajón de la caja Amiga, tras los de los volúmenes.
pub const RAIZ_AMIGA: u32 = 5;
const CAMINO_AMIGA: &[u8] = b"Estado/Servicios/caja-amiga";
const BOTON_IZQUIERDO: u32 = 0x110;
const BOTON_DERECHO: u32 = 0x111;
const BOTON_CENTRAL: u32 = 0x112;
const PLAZO_US: u64 = 3_000_000;
/// Lo más que mide una ventana (una pantalla de 1920 × 1200) y lo menos.
const MAX_ANCHO: i32 = 1920;
const MAX_ALTO: i32 = 1200;
const MIN_ANCHO: i32 = 480;
const MIN_ALTO: i32 = 360;
/// Lo que se lista de un cajón, como mucho.
const MAX_ENTRADAS: usize = 5000;
/// Los PNG de más de esto no tienen miniatura.
const MAX_MINIATURA: u64 = 2 << 20;
/// Lo que dura un aviso de abajo (con Deshacer).
const AVISO_US: u64 = 8_000_000;

// Las teclas de Linux.
const K_ESC: u32 = 1;
const K_1: u32 = 2;
const K_2: u32 = 3;
const K_RETROCESO: u32 = 14;
const K_TAB: u32 = 15;
const K_W: u32 = 17;
const K_R: u32 = 19;
const K_T: u32 = 20;
const K_I: u32 = 23;
const K_INTRO: u32 = 28;
const K_CTRL: u32 = 29;
const K_A: u32 = 30;
const K_F: u32 = 33;
const K_H: u32 = 35;
const K_L: u32 = 38;
const K_MAYUS: u32 = 42;
const K_Z: u32 = 44;
const K_X: u32 = 45;
const K_C: u32 = 46;
const K_V: u32 = 47;
const K_N: u32 = 49;
const K_MAYUS_D: u32 = 54;
const K_ALT: u32 = 56;
const K_ESPACIO: u32 = 57;
const K_F2: u32 = 60;
const K_F5: u32 = 63;
const K_CTRL_D: u32 = 97;
const K_INICIO: u32 = 102;
const K_ARRIBA: u32 = 103;
const K_REPAG: u32 = 104;
const K_IZQUIERDA: u32 = 105;
const K_DERECHA: u32 = 106;
const K_FIN: u32 = 107;
const K_ABAJO: u32 = 108;
const K_AVPAG: u32 = 109;
const K_SUPR: u32 = 111;

// ---------------------------------------------------------------------------------------------------------------
// El servicio de archivos.

pub struct Servicio {
    lectura: u32,
    tam: usize,
}

impl Servicio {
    fn abrir(c: &mut Cliente) -> Option<Servicio> {
        let (lectura, suyo) = dashos::canal_crear();
        let mut p = [0u8; a::CABECERA];
        p[0..4].copy_from_slice(&a::SESION.to_le_bytes());
        p[8..16].copy_from_slice(&(VENTANA_SERVICIO as u64).to_le_bytes());
        let mut r = [0u8; 32];
        let mut ll = [0u32; 1];
        if dashos::canal_escribir(ARCHIVOS, &p, &[suyo]) != 0 {
            return None;
        }
        let hasta = dashos::ahora_us() + PLAZO_US;
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
        let tam = (u64::from_le_bytes(r[8..16].try_into().unwrap()) as usize).clamp(4096, VENTANA_SERVICIO);
        if dashos::memoria_proyectar(ll[0], VA_ARCHIVOS, true) != 0 {
            return None;
        }
        dashos::cerrar(ll[0]);
        Some(Servicio { lectura, tam })
    }

    fn pedir_una(&self, c: &mut Cliente, orden: u32, asa: u32, x: u64, y: u64, nombre: &[u8]) -> Resp {
        let fallo = Resp { e: -1, ..Default::default() };
        if nombre.len() > 224 {
            return Resp { e: a::MAL_FORMADO, ..fallo };
        }
        let mut p = [0u8; 256];
        p[0..4].copy_from_slice(&orden.to_le_bytes());
        p[4..8].copy_from_slice(&asa.to_le_bytes());
        p[8..16].copy_from_slice(&x.to_le_bytes());
        p[16..24].copy_from_slice(&y.to_le_bytes());
        p[24..28].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
        p[32..32 + nombre.len()].copy_from_slice(nombre);
        if dashos::canal_escribir(ARCHIVOS, &p[..32 + nombre.len()], &[]) != 0 {
            return fallo;
        }
        let mut r = [0u8; 32];
        let hasta = dashos::ahora_us() + PLAZO_US;
        loop {
            let n = dashos::canal_leer_plazo(self.lectura, &mut r, &mut [], 20_000).0;
            if n == 32 {
                break;
            }
            if n != dashos::E_PLAZO || dashos::ahora_us() >= hasta {
                return fallo;
            }
            while c.recoger() {}
        }
        let u64_en = |i: usize| u64::from_le_bytes(r[i..i + 8].try_into().unwrap());
        Resp { e: i32::from_le_bytes(r[0..4].try_into().unwrap()), asa: u32::from_le_bytes(r[4..8].try_into().unwrap()), a: u64_en(8), b: u64_en(16), c: u64_en(24) }
    }

    /// Se cede a sí mismo sus raíces (S6b, capa 2): el estado de cada una. `RAM:` no: es el volumen en memoria de la
    /// sesión, y su raíz se la pone `sesiones` cuando entra su usuario.
    fn ceder_raices(&self, c: &mut Cliente) -> [i32; 4] {
        let mut ceder = |camino: &[u8], derechos: u64, hueco: u32| {
            let r = self.pedir_una(c, a::BUSCAR, 0, 0, 0, camino);
            if r.e != a::BIEN {
                return r.e;
            }
            let e = self.pedir_una(c, a::CEDER, r.asa, a::YO, derechos | (hueco as u64) << 8, b"").e;
            self.pedir_una(c, a::CERRAR, r.asa, 0, 0, b"");
            e
        };
        [
            ceder(b"", 4, RAIZ_SISTEMA),
            ceder(b"C", 4 | 2, RAIZ_HOME),
            ceder(s::CAMINO_WORK.as_bytes(), 4 | 2, RAIZ_WORK),
            ceder(CAMINO_AMIGA, 4 | 2, RAIZ_AMIGA),
        ]
    }
}

/// El servicio de archivos para `dashos::asignaciones`.
pub struct PorNombres<'a>(Real<'a>);

impl dashos::asignaciones::Fs for PorNombres<'_> {
    fn pedir(&mut self, orden: u32, asa: u32, x: u64, y: u64, nombre: &[u8]) -> dashos::asignaciones::Resp {
        let r = self.0.pedir(orden, asa, x, y, nombre);
        dashos::asignaciones::Resp { e: r.e, asa: r.asa, a: r.a, b: r.b, c: r.c }
    }

    fn ventana(&self) -> &[u8] {
        Fs::ventana(&self.0)
    }
}

/// El servicio con el cliente del compositor (para no dejar de leerle mientras se espera).
pub struct Real<'a> {
    s: &'a Servicio,
    c: &'a mut Cliente,
}

impl Fs for Real<'_> {
    fn pedir(&mut self, orden: u32, asa: u32, x: u64, y: u64, nombre: &[u8]) -> Resp {
        let r = self.s.pedir_una(self.c, orden, asa, x, y, nombre);
        // Al volver el disco a una instantánea, el servicio suelta las raíces de todos: se ceden otra vez (si esa raíz
        // de verdad ya no vale; una asa mala puede ser también la del destino de renombrar).
        if r.e == a::ASA_MALA && (RAIZ_SISTEMA..=RAIZ_AMIGA).contains(&asa) && self.s.pedir_una(self.c, a::DATOS, asa, 0, 0, b"").e == a::ASA_MALA {
            let e = self.s.ceder_raices(self.c);
            println!("  [explorador] sus raíces ya no valían: otra vez {:?}", e);
            return self.s.pedir_una(self.c, orden, asa, x, y, nombre);
        }
        r
    }

    fn ventana(&self) -> &[u8] {
        memoria::bytes(VA_ARCHIVOS, self.s.tam)
    }

    fn ventana_mut(&mut self) -> &mut [u8] {
        memoria::bytes(VA_ARCHIVOS, self.s.tam)
    }

    fn ahora(&self) -> u64 {
        dashos::hora_ns() / 1_000_000_000
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Sesiones: lanzar un programa de `Programas` como la persona de delante (como el Workbench).

fn a_sesiones(c: &mut Cliente, m: &[u8], r: &mut [u8]) -> i64 {
    let (lectura, suyo) = dashos::canal_crear();
    let n = if dashos::canal_escribir(SESIONES, m, &[suyo]) == 0 {
        let hasta = dashos::ahora_us() + 5_000_000;
        loop {
            let n = dashos::canal_leer_plazo(lectura, r, &mut [], 20_000).0;
            if n != dashos::E_PLAZO || dashos::ahora_us() >= hasta {
                break n;
            }
            while c.recoger() {}
        }
    } else {
        -1
    };
    dashos::cerrar(lectura);
    n
}

fn lanzar(c: &mut Cliente, nombre: &str) -> (i32, u64) {
    if !nombre_valido(nombre) {
        return (a::MAL_FORMADO, 0);
    }
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
// Los lugares de la barra lateral.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icono {
    Casa,
    Papelera,
    Disco,
    Amiga,
}

pub struct Sitio {
    pub nombre: &'static str,
    /// El nombre en castellano, para la consola.
    pub fijo: &'static str,
    pub lugar: Lugar,
    pub icono: Icono,
    /// La marca de la caja (68K), si es de una.
    pub marca: Option<&'static str>,
}

pub fn sitios() -> [Sitio; 6] {
    [
        Sitio { nombre: tr(1, "Carpeta personal"), fijo: "Carpeta personal", lugar: Lugar::raiz(RAIZ_HOME), icono: Icono::Casa, marca: None },
        Sitio { nombre: tr(2, "Papelera"), fijo: "Papelera", lugar: papelera_archivos(), icono: Icono::Papelera, marca: None },
        Sitio { nombre: "SYS:", fijo: "SYS:", lugar: Lugar::raiz(RAIZ_SISTEMA), icono: Icono::Disco, marca: None },
        Sitio { nombre: "RAM:", fijo: "RAM:", lugar: Lugar::raiz(RAIZ_RAM), icono: Icono::Disco, marca: None },
        Sitio { nombre: "Work:", fijo: "Work:", lugar: Lugar::raiz(RAIZ_WORK), icono: Icono::Disco, marca: None },
        Sitio { nombre: "Amiga", fijo: "Amiga", lugar: Lugar::raiz(RAIZ_AMIGA), icono: Icono::Amiga, marca: Some("68K") },
    ]
}

pub fn papelera() -> Lugar {
    Lugar::raiz(RAIZ_HOME).hijo(PAPELERA)
}

pub fn papelera_archivos() -> Lugar {
    papelera().hijo(PAPELERA_ARCHIVOS)
}

/// El nombre de una raíz (en el camino escrito y en las migas).
pub fn nombre_raiz(r: u32) -> &'static str {
    match r {
        RAIZ_SISTEMA => "SYS",
        RAIZ_HOME => "Home",
        RAIZ_RAM => "RAM",
        RAIZ_WORK => "Work",
        RAIZ_AMIGA => "Amiga",
        _ => "?",
    }
}

const RAICES_ESCRITAS: [(&str, u32); 8] = [
    ("Home", RAIZ_HOME),
    ("SYS", RAIZ_SISTEMA),
    ("RAM", RAIZ_RAM),
    ("Work", RAIZ_WORK),
    ("Amiga", RAIZ_AMIGA),
    ("Caja", RAIZ_AMIGA),
    ("Sistema", RAIZ_SISTEMA),
    ("Personal", RAIZ_HOME),
];

/// El camino para la consola y para la barra donde se escribe: "Home:Privado", "Papelera". Siempre en castellano.
pub fn texto_lugar(l: &Lugar) -> String {
    texto_lugar_con(l, "Papelera")
}

/// El camino para enseñarlo en la ventana: como `texto_lugar`, con la papelera en el idioma del usuario.
pub fn lugar_visible(l: &Lugar) -> String {
    texto_lugar_con(l, tr(2, "Papelera"))
}

fn texto_lugar_con(l: &Lugar, papelera: &str) -> String {
    if l.dentro_de(&papelera_archivos()) {
        return Lugar { raiz: 0, nombres: l.nombres[2..].to_vec() }.texto(papelera);
    }
    // Como en el Amiga (paso 2 de las asignaciones): el volumen con sus dos puntos y los cajones detrás.
    alloc::format!("{}:{}", nombre_raiz(l.raiz), l.nombres.join("/"))
}

/// Las migas del camino: (texto, lugar al que llevan). En el idioma del usuario.
pub fn migas(l: &Lugar) -> Vec<(String, Lugar)> {
    migas_con(l, tr(2, "Papelera"), tr(1, "Carpeta personal"))
}

/// Las migas, para la consola: siempre en castellano.
pub fn migas_fijas(l: &Lugar) -> Vec<(String, Lugar)> {
    migas_con(l, "Papelera", "Carpeta personal")
}

fn migas_con(l: &Lugar, papelera: &str, personal: &str) -> Vec<(String, Lugar)> {
    let mut v = Vec::new();
    let (desde, primera) = if l.dentro_de(&papelera_archivos()) { (2, String::from(papelera)) } else {
        (0, match l.raiz { RAIZ_HOME => String::from(personal), r => alloc::format!("{}:", nombre_raiz(r)) })
    };
    let mut x = Lugar { raiz: l.raiz, nombres: l.nombres[..desde].to_vec() };
    v.push((primera, x.clone()));
    for n in &l.nombres[desde..] {
        x = x.hijo(n);
        v.push((n.clone(), x.clone()));
    }
    v
}

// ---------------------------------------------------------------------------------------------------------------
// Una línea de texto que se edita (el camino, la búsqueda, los nombres).

#[derive(Clone, Default)]
pub struct Linea {
    pub texto: String,
    /// Donde está el cursor (en bytes) y, si hay, desde dónde está marcado.
    pub cursor: usize,
    pub marca: Option<usize>,
}

impl Linea {
    pub fn con(t: &str) -> Self {
        Linea { texto: String::from(t), cursor: t.len(), marca: Some(0) }
    }

    pub fn marcado(&self) -> (usize, usize) {
        match self.marca {
            Some(m) => (m.min(self.cursor), m.max(self.cursor)),
            None => (self.cursor, self.cursor),
        }
    }

    fn quitar_marcado(&mut self) -> bool {
        let (x, y) = self.marcado();
        self.marca = None;
        if x == y {
            return false;
        }
        self.texto.replace_range(x..y, "");
        self.cursor = x;
        true
    }

    fn anterior(&self, i: usize) -> usize {
        self.texto[..i].char_indices().last().map_or(0, |x| x.0)
    }

    fn siguiente(&self, i: usize) -> usize {
        self.texto[i..].chars().next().map_or(i, |c| i + c.len_utf8())
    }

    /// Una tecla; verdad si la usó.
    pub fn tecla(&mut self, t: Tecla, mayus: bool) -> bool {
        match t {
            Tecla::Letra(c) if !c.is_control() => {
                if self.texto.len() + c.len_utf8() > 200 {
                    return true;
                }
                self.quitar_marcado();
                self.texto.insert(self.cursor, c);
                self.cursor += c.len_utf8();
            }
            Tecla::Retroceso => {
                if !self.quitar_marcado() && self.cursor > 0 {
                    let a = self.anterior(self.cursor);
                    self.texto.replace_range(a..self.cursor, "");
                    self.cursor = a;
                }
            }
            Tecla::Suprimir => {
                if !self.quitar_marcado() && self.cursor < self.texto.len() {
                    let b = self.siguiente(self.cursor);
                    self.texto.replace_range(self.cursor..b, "");
                }
            }
            Tecla::Izquierda | Tecla::Derecha | Tecla::Inicio | Tecla::Fin => {
                if mayus {
                    self.marca.get_or_insert(self.cursor);
                } else if let (Some(_), Tecla::Izquierda | Tecla::Derecha) = (self.marca, t) {
                    let (x, y) = self.marcado();
                    self.cursor = if t == Tecla::Izquierda { x } else { y };
                    self.marca = None;
                    return true;
                } else {
                    self.marca = None;
                }
                self.cursor = match t {
                    Tecla::Izquierda => self.anterior(self.cursor),
                    Tecla::Derecha => self.siguiente(self.cursor),
                    Tecla::Inicio => 0,
                    _ => self.texto.len(),
                };
            }
            _ => return false,
        }
        true
    }

    pub fn todo(&mut self) {
        self.marca = Some(0);
        self.cursor = self.texto.len();
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Las ventanas y lo que tienen.

/// Una cosa que se ve en una pestaña: dónde está (su cajón) y qué es.
#[derive(Clone)]
pub struct Cosa {
    pub donde: Lugar,
    pub e: Entrada,
}

pub struct Pestana {
    pub lugar: Lugar,
    /// Todo lo del cajón (o lo encontrado), y lo que se ve, en orden (sus números en `cosas`).
    pub cosas: Vec<Cosa>,
    pub vista: Vec<usize>,
    pub raros: usize,
    pub sel: Seleccion,
    pub atras: Vec<Lugar>,
    pub adelante: Vec<Lugar>,
    /// Lo que se ha bajado la vista, en píxeles.
    pub desplaz: i32,
    pub lista: bool,
    pub orden: Orden,
    pub asc: bool,
    pub ocultos: bool,
    /// Lo que no se pudo listar.
    pub error: Option<String>,
    /// El mismo error, en castellano: para la consola.
    pub error_fijo: Option<String>,
    /// La búsqueda (en este cajón y los de dentro), si hay.
    pub busca: Option<ops::Busqueda>,
    /// Lo que se ha escrito para saltar a un nombre sin buscar (con la hora de la última letra).
    pub salto: (String, u64),
    /// El cursor vino del teclado: se dibuja su anillo.
    pub anillo: bool,
}

impl Pestana {
    fn nueva(lugar: Lugar, lista: bool, ocultos: bool) -> Self {
        Pestana {
            lugar,
            cosas: Vec::new(),
            vista: Vec::new(),
            raros: 0,
            sel: Seleccion::nueva(0),
            atras: Vec::new(),
            adelante: Vec::new(),
            desplaz: 0,
            lista,
            orden: Orden::Nombre,
            asc: true,
            ocultos,
            error: None,
            error_fijo: None,
            busca: None,
            salto: (String::new(), 0),
            anillo: false,
        }
    }

    pub fn titulo(&self) -> String {
        if let Some(b) = &self.busca {
            return trf(3, "Buscar \"{}\"", &[&b.busca]);
        }
        migas(&self.lugar).last().map_or_else(String::new, |m| m.0.clone())
    }

    pub fn en_papelera(&self) -> bool {
        self.lugar == papelera_archivos()
    }

    /// Lo que se ve, con lo elegido conservado por nombre.
    fn rehacer_vista(&mut self) {
        // La vista de antes puede ser de otras cosas (se acaban de cambiar): solo lo que siga existiendo.
        let cosas = &self.cosas;
        let elegidos: Vec<(Lugar, String)> = self.sel.elegidos().iter().filter_map(|&i| self.vista.get(i)).filter_map(|&k| cosas.get(k)).map(|c| (c.donde.clone(), c.e.nombre.clone())).collect();
        let cursor = self.sel.cursor.and_then(|i| self.vista.get(i)).and_then(|&k| cosas.get(k)).map(|c| c.e.nombre.clone());
        // En la caja Amiga, los `.info` de otra cosa se esconden como los ocultos (son su icono).
        let de_otro = if self.ocultos { Vec::new() } else { iconos_amiga::escondidas(&self.cosas) };
        let mut v: Vec<usize> = (0..self.cosas.len()).filter(|&i| self.ocultos || !(es_oculto(&self.cosas[i].e.nombre) || de_otro[i])).collect();
        if self.busca.is_none() {
            let entradas: Vec<Entrada> = self.cosas.iter().map(|c| c.e.clone()).collect();
            ordenar(&entradas, &mut v, self.orden, self.asc);
        }
        self.vista = v;
        self.sel = Seleccion::nueva(self.vista.len());
        for (i, &k) in self.vista.iter().enumerate() {
            if elegidos.iter().any(|(d, n)| *d == self.cosas[k].donde && *n == self.cosas[k].e.nombre) {
                self.sel.marcados[i] = true;
            }
            if cursor.as_deref() == Some(self.cosas[k].e.nombre.as_str()) {
                self.sel.cursor = Some(i);
                self.sel.ancla = Some(i);
            }
        }
    }

    pub fn cosa(&self, i: usize) -> Option<&Cosa> {
        self.vista.get(i).and_then(|&k| self.cosas.get(k))
    }

    fn elegidas(&self) -> Vec<Cosa> {
        self.sel.elegidos().iter().filter_map(|&i| self.cosa(i)).cloned().collect()
    }
}

/// Dónde se pulsa en una ventana: lo que dibuja `pintar` y mira el ratón.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zona {
    Nada,
    Barra,
    Atras,
    Adelante,
    Miga(usize),
    Camino,
    Buscar,
    Vista,
    Menu,
    Maximizar,
    Cerrar,
    Lugar(usize),
    Pestana(usize),
    CerrarPestana(usize),
    NuevaPestana,
    Columna(Orden),
    Cosa(usize),
    Fondo,
    Opcion(usize),
    BotonDialogo(usize),
    CampoDialogo,
    FilaDialogo(usize),
    CancelarTrabajo(usize),
    Deshacer,
    Vaciar,
    Borde(u32),
    Velo,
}

/// Lo que hace cada opción de un menú.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Accion {
    Abrir,
    AbrirPestana,
    AbrirVentana,
    AbrirCon,
    Cortar,
    Copiar,
    Pegar,
    Renombrar,
    Papelera,
    Borrar,
    Restaurar,
    Vaciar,
    NuevaCarpeta,
    TodoElegir,
    Ocultos,
    Propiedades,
    NuevaVentana,
    NuevaPestana,
    Recargar,
    Rejilla,
    Lista,
}

pub struct Menu {
    pub x: i32,
    pub y: i32,
    /// Cada opción: su texto en el idioma del usuario, lo que hace, si se puede, y su texto en castellano (para la
    /// consola).
    pub opciones: Vec<(&'static str, Accion, bool, &'static str)>,
    pub encima: Option<usize>,
}

pub enum Dialogo {
    Nombre { titulo: &'static str, boton: &'static str, linea: Linea, nuevo: bool, viejo: String, error: Option<String> },
    Confirmar { titulo: String, texto: String, boton: &'static str, accion: Accion, cosas: Vec<Cosa> },
    AbrirCon { fichero: String, programas: Vec<String>, elegido: Option<usize> },
    Propiedades { i: Interfaz, rect: Rect, asa: u32, comentario: String, valor: i64, puede: bool, campo: Obj, rejilla: Obj, aviso: Obj, marca: Obj, ruta: String, donde: Lugar },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Foco {
    Contenido,
    Camino,
    Buscar,
}

pub struct Aviso {
    pub texto: String,
    /// Lo que se deshace: (trabajo de vuelta).
    pub deshacer: Option<Deshacer>,
    pub hasta: u64,
}

#[derive(Clone)]
pub enum Deshacer {
    /// Sacar de la papelera estos nombres.
    Restaurar(Vec<String>),
    /// Volver a llamar `nuevo` como `viejo` en `donde`.
    Renombrar { donde: Lugar, viejo: String, nuevo: String },
    /// Volver a mover estos de `donde` a su sitio.
    Mover { donde: Lugar, nombres: Vec<String>, origen: Lugar },
}

pub struct Ventana {
    pub numero: u32,
    slot: usize,
    sup: u32,
    xsup: u32,
    top: u32,
    pool: u32,
    bufer: u32,
    mem: u32,
    mitad: usize,
    pub w: i32,
    pub h: i32,
    configurada: bool,
    pub maximizada: bool,
    pedida: (i32, i32),
    limites: (i32, i32),
    pub pestanas: Vec<Pestana>,
    pub activa: usize,
    pub foco: Foco,
    pub linea: Linea,
    pub menu: Option<Menu>,
    pub dialogo: Option<Dialogo>,
    pub encima: Zona,
    pub zonas: Vec<(Rect, Zona)>,
    /// El rectángulo de elegir: desde dónde, hasta dónde y lo que estaba elegido antes.
    pub rect: Option<(i32, i32, i32, i32, Vec<bool>)>,
    pub aviso: Option<Aviso>,
    /// Lo pulsado y dónde (para soltar encima, arrastrar o el rectángulo), y el último clic (doble clic).
    pulsado: Option<(Zona, i32, i32, bool)>,
    ultimo: Option<(Zona, u32)>,
    /// Dónde está el contenido (para el rectángulo y la rueda).
    pub contenido: Rect,
    /// Cuántas columnas y filas a la vista tiene la rejilla.
    pub columnas: usize,
    pub filas: usize,
}

impl Ventana {
    pub fn p(&self) -> &Pestana {
        &self.pestanas[self.activa]
    }

    pub fn pm(&mut self) -> &mut Pestana {
        &mut self.pestanas[self.activa]
    }

    fn va(&self, mitad: usize) -> usize {
        VA_VENTANAS + (self.slot * 2 + mitad) * HUECO
    }

    pub fn zona(&self, x: i32, y: i32) -> Zona {
        self.zonas.iter().rev().find(|(r, _)| r.contiene(x, y)).map_or(Zona::Nada, |z| z.1)
    }
}

/// Lo que se arrastra: de qué ventana, qué cosas y dónde está el puntero (en qué ventana).
pub struct Arrastre {
    pub desde: u32,
    pub cosas: Vec<Cosa>,
    pub x: i32,
    pub y: i32,
    pub sobre: u32,
    pub destino: Option<Lugar>,
}

/// Un trabajo en marcha, con lo que hay que decir de él.
pub struct EnMarcha {
    pub t: Trabajo,
    pub titulo: String,
    /// El título en castellano, para la consola.
    pub titulo_fijo: String,
    pub ventana: u32,
    pub id: u32,
}

pub struct App {
    c: Cliente,
    sv: Servicio,
    /// Su tabla de nombres (paso 2 de las asignaciones, `ASIGNACIONES` de `sesiones`): los caminos escritos con un
    /// nombre (`LIBS:`, `ENVARC:Sys`, `T:`) se resuelven por ella.
    nombres: dashos::asignaciones::Tabla,
    pub tema: Tema,
    pub regular: Fuente,
    pub medio: Fuente,
    ventanas: Vec<Ventana>,
    siguiente_id: u32,
    abiertas: u32,
    /// Sobre qué superficie está el puntero, dónde, los botones y las teclas que modifican.
    sobre: u32,
    x: i32,
    y: i32,
    boton: bool,
    serie: u32,
    ctrl: bool,
    mayus: bool,
    alt: bool,
    con_teclado: u32,
    teclado: Teclado,
    /// Lo copiado o cortado.
    portapapeles: Option<(Vec<Cosa>, bool)>,
    pub trabajos: Vec<EnMarcha>,
    siguiente_trabajo: u32,
    pub arrastre: Option<Arrastre>,
    /// Las miniaturas de los PNG (por lugar y nombre): `None` si no se pudo.
    pub miniaturas: BTreeMap<(Lugar, String), Option<kit::imagen::Imagen>>,
    /// Los iconos `.info` de la caja Amiga (`explorador/iconos_amiga.rs`).
    pub iconos_amiga: iconos_amiga::IconosAmiga,
    /// Cuánto ocupa el disco: (tamaño, libre).
    pub disco: (u64, u64),
    contexto: Option<Contexto>,
    /// Quién es cada marca del canal "explorador" (lo dice `inicio`).
    quienes: BTreeMap<u64, String>,
}

fn texto_obj(arbol: &mut Arbol, t: &str, estilo: i64) -> Obj {
    arbol.crear(Texto, &[(atr::TEXTO, Valor::Texto(String::from(t))), (atr::ESTILO, Valor::Int(estilo))])
}

impl App {
    fn fs(&mut self) -> Real<'_> {
        Real { s: &self.sv, c: &mut self.c }
    }

    fn ids(&mut self, n: u32) -> u32 {
        let p = self.siguiente_id;
        self.siguiente_id += n;
        p
    }

    fn ventana_de(&self, sup: u32) -> Option<usize> {
        self.ventanas.iter().position(|v| v.sup == sup)
    }

    fn por_numero(&self, n: u32) -> Option<usize> {
        self.ventanas.iter().position(|v| v.numero == n)
    }

    pub fn ahora(&self) -> u64 {
        dashos::hora_ns() / 1_000_000_000
    }

    // --- Ventanas ---

    /// Una ventana nueva en `lugar`. Se pinta al configurarse.
    fn nueva_ventana(&mut self, lugar: Lugar) -> bool {
        let Some(slot) = (0..MAX_VENTANAS).find(|s| !self.ventanas.iter().any(|v| v.slot == *s)) else {
            println!("  [explorador] ya hay {} ventanas abiertas", MAX_VENTANAS);
            return false;
        };
        let b = self.ids(3);
        let (sup, xsup, top) = (b, b + 1, b + 2);
        let mut e = Escritor::new();
        e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(sup).fin();
        e.empezar(XDG, xdg_wm_base::req::GET_XDG_SURFACE).uint(xsup).uint(sup).fin();
        e.empezar(xsup, xdg_surface::req::GET_TOPLEVEL).uint(top).fin();
        e.empezar(top, xdg_toplevel::req::SET_TITLE).texto(Some(tr(4, "Archivos"))).fin();
        e.empezar(top, xdg_toplevel::req::SET_APP_ID).texto(Some("explorador")).fin();
        e.empezar(top, xdg_toplevel::req::SET_MIN_SIZE).int(MIN_ANCHO).int(MIN_ALTO).fin();
        e.empezar(sup, wl_surface::req::COMMIT).fin();
        self.c.mandar(&e);
        self.abiertas += 1;
        let mut v = Ventana {
            numero: self.abiertas,
            slot,
            sup,
            xsup,
            top,
            pool: 0,
            bufer: 0,
            mem: 0,
            mitad: 0,
            w: 0,
            h: 0,
            configurada: false,
            maximizada: false,
            pedida: (0, 0),
            limites: (0, 0),
            pestanas: Vec::new(),
            activa: 0,
            foco: Foco::Contenido,
            linea: Linea::default(),
            menu: None,
            dialogo: None,
            encima: Zona::Nada,
            zonas: Vec::new(),
            rect: None,
            aviso: None,
            pulsado: None,
            ultimo: None,
            contenido: Rect::default(),
            columnas: 1,
            filas: 1,
        };
        v.pestanas.push(Pestana::nueva(lugar, false, false));
        self.ventanas.push(v);
        let k = self.ventanas.len() - 1;
        self.cargar(k, 0);
        true
    }

    /// Un tamaño nuevo (o el primero): memoria, pool y búfer nuevos, y se pinta.
    fn medir(&mut self, k: usize, w: i32, h: i32) -> bool {
        let (w, h) = (w.clamp(MIN_ANCHO, MAX_ANCHO), h.clamp(MIN_ALTO, MAX_ALTO));
        let v = &self.ventanas[k];
        if v.mem != 0 && (w, h) == (v.w, v.h) {
            return true;
        }
        let tam = (w * h * 4) as usize;
        let m = dashos::memoria_crear(tam);
        let otra = if v.mem == 0 { 0 } else { 1 - v.mitad };
        let va = v.va(otra);
        if (m as i32) <= 0 || dashos::memoria_proyectar(m, va, true) != 0 {
            if (m as i32) > 0 {
                dashos::cerrar(m);
            }
            println!("  [explorador] sin memoria para una ventana de {}x{}", w, h);
            return false;
        }
        let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::TRANSFERIR, 0);
        let b = self.ids(2);
        let (pool, bufer) = (b, b + 1);
        let mut e = Escritor::new();
        let v = &mut self.ventanas[k];
        if v.mem != 0 {
            e.empezar(v.bufer, wl_buffer::req::DESTROY).fin();
            e.empezar(v.pool, wl_shm_pool::req::DESTROY).fin();
        }
        e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(pool).llave(copia as u32).int(tam as i32).fin();
        e.empezar(pool, wl_shm_pool::req::CREATE_BUFFER).uint(bufer).int(0).int(w).int(h).int(w * 4).uint(wayland::shm::ARGB8888).fin();
        self.c.mandar(&e);
        let v = &mut self.ventanas[k];
        if v.mem != 0 {
            dashos::memoria_quitar(v.va(v.mitad));
            dashos::cerrar(v.mem);
        }
        (v.mem, v.mitad, v.pool, v.bufer, v.w, v.h) = (m, otra, pool, bufer, w, h);
        true
    }

    pub fn presentar(&mut self, k: usize) {
        if !self.ventanas[k].configurada || self.ventanas[k].mem == 0 {
            return;
        }
        let ahora_us = dashos::ahora_us();
        let ahora = self.ahora();
        let (va, w, h) = {
            let v = &self.ventanas[k];
            (v.va(v.mitad), v.w, v.h)
        };
        let px = memoria::pixeles(va, (w * h) as usize);
        let mut l = Lienzo::new(px, w, h, w as usize);
        {
            let mut v = core::mem::replace(&mut self.ventanas[k], ventana_vacia());
            pintar::pintar(&mut l, &mut v, self, ahora, ahora_us);
            self.ventanas[k] = v;
        }
        let v = &self.ventanas[k];
        let mut e = Escritor::new();
        e.empezar(v.sup, wl_surface::req::ATTACH).uint(v.bufer).int(0).int(0).fin();
        e.empezar(v.sup, wl_surface::req::DAMAGE).int(0).int(0).int(v.w).int(v.h).fin();
        e.empezar(v.sup, wl_surface::req::COMMIT).fin();
        self.c.mandar(&e);
    }

    fn presentar_todas(&mut self) {
        for k in 0..self.ventanas.len() {
            self.presentar(k);
        }
    }

    fn cerrar_ventana(&mut self, k: usize) {
        let mut v = self.ventanas.remove(k);
        let mut e = Escritor::new();
        e.empezar(v.top, xdg_toplevel::req::DESTROY).fin();
        e.empezar(v.xsup, xdg_surface::req::DESTROY).fin();
        e.empezar(v.sup, wl_surface::req::DESTROY).fin();
        if v.mem != 0 {
            e.empezar(v.bufer, wl_buffer::req::DESTROY).fin();
            e.empezar(v.pool, wl_shm_pool::req::DESTROY).fin();
        }
        self.c.mandar(&e);
        if v.mem != 0 {
            dashos::memoria_quitar(v.va(v.mitad));
            dashos::cerrar(v.mem);
        }
        if let Some(Dialogo::Propiedades { asa, i, .. }) = v.dialogo.take() {
            let mut fs = self.fs();
            ops::cerrar(&mut fs, asa);
            self.contexto = Some(i.ctx);
        }
        for p in &mut v.pestanas {
            if let Some(mut b) = p.busca.take() {
                let mut fs = Real { s: &self.sv, c: &mut self.c };
                b.parar(&mut fs);
            }
        }
        if self.sobre == v.sup {
            self.sobre = 0;
        }
        println!("  [explorador] cierra la ventana {}", v.numero);
        if self.ventanas.is_empty() {
            println!("  [explorador] sin ventanas abiertas");
        }
    }

    // --- Lugares ---

    /// Lee el cajón de la pestaña `p` de la ventana `k` (o vuelve a leerlo).
    fn cargar(&mut self, k: usize, p: usize) {
        let lugar = self.ventanas[k].pestanas[p].lugar.clone();
        let r = {
            let mut fs = self.fs();
            ops::abrir(&mut fs, &lugar).and_then(|asa| {
                let r = ops::listar(&mut fs, asa, MAX_ENTRADAS);
                ops::cerrar(&mut fs, asa);
                r
            })
        };
        let pe = &mut self.ventanas[k].pestanas[p];
        match r {
            Ok((v, raros)) => {
                pe.cosas = v.into_iter().map(|e| Cosa { donde: lugar.clone(), e }).collect();
                pe.raros = raros;
                pe.error = None;
                pe.error_fijo = None;
            }
            Err(e) => {
                pe.cosas.clear();
                pe.raros = 0;
                pe.error = Some(String::from(if e == a::PROTEGIDO { tr(5, "No tienes permiso para ver esta carpeta") } else { ops::texto_error(e) }));
                pe.error_fijo = Some(String::from(if e == a::PROTEGIDO { "No tienes permiso para ver esta carpeta" } else { ops::texto_error_fijo(e) }));
                println!("  [explorador] no abre {}: {}", texto_lugar(&lugar), e);
            }
        }
        pe.rehacer_vista();
    }

    /// Lleva la pestaña activa de la ventana `k` a `lugar` (con historial).
    fn ir(&mut self, k: usize, lugar: Lugar, historial: bool) {
        if !lugar.valido() {
            return;
        }
        let v = &mut self.ventanas[k];
        v.foco = Foco::Contenido;
        let p = v.pm();
        if let Some(mut b) = p.busca.take() {
            let mut fs = Real { s: &self.sv, c: &mut self.c };
            b.parar(&mut fs);
        }
        let p = self.ventanas[k].pm();
        if historial && p.lugar != lugar {
            let viejo = core::mem::replace(&mut p.lugar, lugar);
            p.atras.push(viejo);
            if p.atras.len() > 50 {
                p.atras.remove(0);
            }
            p.adelante.clear();
        } else {
            p.lugar = lugar;
        }
        p.desplaz = 0;
        p.sel = Seleccion::nueva(0);
        let activa = self.ventanas[k].activa;
        self.cargar(k, activa);
        // Lo de dentro, sin elegir; el cursor en lo primero (para el teclado).
        let p = self.ventanas[k].pm();
        if !p.vista.is_empty() {
            p.sel.cursor = Some(0);
        }
        self.decir_ventana(k);
    }

    fn atras(&mut self, k: usize) {
        let p = self.ventanas[k].pm();
        if let Some(l) = p.atras.pop() {
            let actual = core::mem::replace(&mut p.lugar, l.clone());
            p.adelante.push(actual);
            self.ir(k, l, false);
        }
    }

    fn adelante(&mut self, k: usize) {
        let p = self.ventanas[k].pm();
        if let Some(l) = p.adelante.pop() {
            let actual = core::mem::replace(&mut p.lugar, l.clone());
            p.atras.push(actual);
            self.ir(k, l, false);
        }
    }

    fn arriba(&mut self, k: usize) {
        let l = self.ventanas[k].p().lugar.clone();
        // En la papelera, arriba es la carpeta personal.
        let padre = if l == papelera_archivos() { Some(Lugar::raiz(RAIZ_HOME)) } else { l.padre() };
        if let Some(padre) = padre {
            let nombre = l.nombres.last().cloned();
            self.ir(k, padre, true);
            // El cajón del que se viene, elegido.
            if let Some(n) = nombre {
                let p = self.ventanas[k].pm();
                if let Some(i) = (0..p.vista.len()).find(|&i| p.cosa(i).is_some_and(|c| c.e.nombre == n)) {
                    p.sel.clic(i, false, false);
                }
            }
        }
    }

    /// Dice dónde quedó cada cosa de la ventana (para las pruebas, que pulsan ahí).
    fn decir_ventana(&mut self, k: usize) {
        self.presentar(k);
        let v = &self.ventanas[k];
        if !v.configurada {
            return;
        }
        let p = v.p();
        let nombres: Vec<&str> = p.vista.iter().map(|&i| p.cosas[i].e.nombre.as_str()).collect();
        println!(
            "  [explorador] ventana {} en {}, {} {} ({} nombres raros sin enseñar){}: {}",
            v.numero,
            texto_lugar(&p.lugar),
            p.vista.len(),
            if p.vista.len() == 1 { "elemento" } else { "elementos" },
            p.raros,
            p.error_fijo.as_ref().map_or_else(String::new, |e| alloc::format!(", {}", e)),
            nombres.join(" | ")
        );
        self.decir_sitios(k);
    }

    fn decir_sitios(&self, k: usize) {
        let v = &self.ventanas[k];
        let p = v.p();
        let centro = |r: &Rect| (r.x + r.w / 2, r.y + r.h / 2);
        let mut t: Vec<String> = Vec::new();
        for (r, z) in &v.zonas {
            let n = match z {
                Zona::Cosa(i) => match p.cosa(*i) {
                    Some(c) => c.e.nombre.clone(),
                    None => continue,
                },
                Zona::Lugar(i) => alloc::format!("lugar {}", sitios()[*i].fijo),
                Zona::Atras => String::from("Atrás"),
                Zona::Adelante => String::from("Adelante"),
                Zona::Buscar => String::from("Buscar"),
                Zona::Vista => String::from("Vista"),
                Zona::Menu => String::from("Menú"),
                Zona::Cerrar => String::from("Cerrar"),
                Zona::Camino => String::from("camino"),
                Zona::Fondo => String::from("fondo"),
                Zona::Vaciar => String::from("Vaciar"),
                Zona::Miga(i) => alloc::format!("miga {}", i),
                Zona::Columna(o) => alloc::format!("columna {:?}", o),
                _ => continue,
            };
            let (x, y) = centro(r);
            // Del fondo, un sitio de abajo a la derecha donde no hay nada; de una fila de la lista, su nombre.
            let (x, y) = match z {
                Zona::Fondo => (r.x + r.w - 24, r.y + r.h - 24),
                Zona::Cosa(_) if p.lista => (r.x + 60, y),
                _ => (x, y),
            };
            t.push(alloc::format!("{} en ({}, {})", n, x, y));
        }
        println!("  [explorador] ventana {}, {}: {}", v.numero, if p.lista { "lista" } else { "rejilla" }, t.join("; "));
    }

    // --- Abrir ---

    /// Abre lo elegido (o la cosa `i`): una carpeta, ahí (o en otra pestaña); un programa de `Programas`, lo lanza.
    fn abrir_cosa(&mut self, k: usize, i: usize, donde: Accion) {
        let Some(c) = self.ventanas[k].p().cosa(i).cloned() else { return };
        if c.e.cajon {
            let l = c.donde.hijo(&c.e.nombre);
            match donde {
                Accion::AbrirPestana => self.nueva_pestana(k, l),
                Accion::AbrirVentana => {
                    self.nueva_ventana(l);
                }
                _ => self.ir(k, l, true),
            }
            return;
        }
        let es_programa = c.e.programa() && c.donde == Lugar::raiz(RAIZ_SISTEMA).hijo("Programas");
        if es_programa {
            let (estado, pid) = lanzar(&mut self.c, &c.e.nombre);
            println!("  [explorador] lanza Programas/{}: {} (proceso {})", c.e.nombre, estado, pid);
            let t = if estado == 0 { trf(6, "\"{}\" está en marcha", &[&c.e.nombre]) } else { trf(7, "\"{}\" no se pudo lanzar ({})", &[&c.e.nombre, &estado]) };
            self.avisar(k, t, None);
        } else {
            println!("  [explorador] {} no se abre: no hay programa para {}", texto_lugar(&c.donde.hijo(&c.e.nombre)), tipo_fijo(&c.e));
            self.avisar(k, trf(8, "No hay ningún programa para abrir \"{}\" ({})", &[&c.e.nombre, &tipo_texto(&c.e)]), None);
        }
    }

    fn nueva_pestana(&mut self, k: usize, l: Lugar) {
        let v = &mut self.ventanas[k];
        if v.pestanas.len() >= MAX_PESTANAS {
            return;
        }
        let (lista, ocultos) = (v.p().lista, v.p().ocultos);
        v.pestanas.insert(v.activa + 1, Pestana::nueva(l, lista, ocultos));
        v.activa += 1;
        let p = v.activa;
        self.cargar(k, p);
        println!("  [explorador] ventana {}: pestaña nueva ({} pestañas)", self.ventanas[k].numero, self.ventanas[k].pestanas.len());
        self.decir_ventana(k);
    }

    fn cerrar_pestana(&mut self, k: usize, p: usize) {
        if self.ventanas[k].pestanas.len() <= 1 {
            self.cerrar_ventana(k);
            return;
        }
        let v = &mut self.ventanas[k];
        let mut pe = v.pestanas.remove(p);
        if let Some(mut b) = pe.busca.take() {
            let mut fs = Real { s: &self.sv, c: &mut self.c };
            b.parar(&mut fs);
        }
        let v = &mut self.ventanas[k];
        if v.activa >= v.pestanas.len() || v.activa > p {
            v.activa = v.activa.saturating_sub(1);
        }
        println!("  [explorador] ventana {}: cierra una pestaña ({} quedan)", v.numero, v.pestanas.len());
        self.decir_ventana(k);
    }

    // --- Avisos ---

    fn avisar(&mut self, k: usize, texto: String, deshacer: Option<Deshacer>) {
        let hasta = dashos::ahora_us() + AVISO_US;
        if let Some(v) = self.ventanas.get_mut(k) {
            v.aviso = Some(Aviso { texto, deshacer, hasta });
        }
    }

    // --- Trabajos ---

    /// Empieza un trabajo. Su título va dos veces: en castellano (para la consola) y en el idioma del usuario (para la
    /// ventana).
    fn empezar(&mut self, k: usize, t: Trabajo, (titulo_fijo, titulo): (String, String)) {
        let numero = self.ventanas.get(k).map_or(0, |v| v.numero);
        self.siguiente_trabajo += 1;
        println!("  [explorador] empieza: {}", titulo_fijo);
        self.trabajos.push(EnMarcha { t, titulo, titulo_fijo, ventana: numero, id: self.siguiente_trabajo });
    }

    /// El título de tirar `cosas` a la papelera: (en castellano, en el idioma del usuario).
    fn titulo_papelera(cosas: &[Cosa]) -> (String, String) {
        if cosas.len() == 1 {
            (alloc::format!("Mover \"{}\" a la papelera", cosas[0].e.nombre), trf(9, "Mover \"{}\" a la papelera", &[&cosas[0].e.nombre]))
        } else {
            (alloc::format!("Mover {} elementos a la papelera", cosas.len()), trf(10, "Mover {} elementos a la papelera", &[&cosas.len()]))
        }
    }

    /// Avanza los trabajos y búsquedas un poco. Verdad si queda algo.
    fn trabajar(&mut self) -> bool {
        let mut cambio = false;
        if let Some(m) = self.trabajos.first_mut() {
            let mut fs = Real { s: &self.sv, c: &mut self.c };
            let acabado = m.t.paso(&mut fs, 12);
            cambio = true;
            if acabado {
                let m = self.trabajos.remove(0);
                self.acabado(m);
            }
        }
        // Las búsquedas.
        for k in 0..self.ventanas.len() {
            for p in 0..self.ventanas[k].pestanas.len() {
                let pe = &mut self.ventanas[k].pestanas[p];
                let Some(b) = pe.busca.as_mut() else { continue };
                if b.acabada {
                    continue;
                }
                let mut fs = Real { s: &self.sv, c: &mut self.c };
                let nuevo = b.paso(&mut fs, 6);
                let acabada = b.acabada;
                if nuevo || acabada {
                    let pe = &mut self.ventanas[k].pestanas[p];
                    let b = pe.busca.as_ref().unwrap();
                    let busca = b.busca.clone();
                    pe.cosas = b.encontrado.iter().map(|(l, e)| Cosa { donde: l.clone(), e: e.clone() }).collect();
                    pe.rehacer_vista();
                    if pe.sel.cuantos() == 0 && !pe.vista.is_empty() {
                        pe.sel.clic(0, false, false);
                    }
                    if acabada {
                        let n = pe.vista.len();
                        let nombres: Vec<String> = pe.vista.iter().map(|&i| texto_lugar(&pe.cosas[i].donde.hijo(&pe.cosas[i].e.nombre))).collect();
                        println!("  [explorador] búsqueda de \"{}\" acabada: {} resultados: {}", busca, n, nombres.join(" | "));
                    }
                    cambio = true;
                }
            }
        }
        if cambio {
            self.presentar_todas();
        }
        !self.trabajos.is_empty() || self.ventanas.iter().any(|v| v.pestanas.iter().any(|p| p.busca.as_ref().is_some_and(|b| !b.acabada)))
    }

    fn acabado(&mut self, m: EnMarcha) {
        let t = &m.t;
        let bien = t.errores.is_empty();
        println!(
            "  [explorador] acabado: {}: {} de {} elementos, {} bytes{}{}",
            m.titulo_fijo,
            t.resultado.len(),
            t.elementos.len(),
            t.bytes,
            if t.raros > 0 { alloc::format!(", {} nombres raros sin tocar", t.raros) } else { String::new() },
            if bien { String::new() } else { alloc::format!("; errores: {}", t.errores.join("; ")) }
        );
        let k = self.por_numero(m.ventana).or(if self.ventanas.is_empty() { None } else { Some(0) });
        // El aviso, con lo que se puede deshacer.
        if let Some(k) = k {
            let n = t.resultado.len();
            let (texto, deshacer) = match t.tarea {
                _ if !bien => (alloc::format!("{}: {}", m.titulo, t.errores_visibles.first().map_or("", |e| e.as_str())), None),
                Tarea::Papelera if n == 1 => (trf(11, "\"{}\" se ha movido a la papelera", &[&t.elementos[0].nombre]), Some(Deshacer::Restaurar(t.resultado.iter().map(|r| r.1.clone()).collect()))),
                Tarea::Papelera => (trf(12, "{} elementos movidos a la papelera", &[&n]), Some(Deshacer::Restaurar(t.resultado.iter().map(|r| r.1.clone()).collect()))),
                Tarea::Restaurar => (if n == 1 { String::from(tr(13, "Un elemento restaurado")) } else { trf(14, "{} elementos restaurados", &[&n]) }, None),
                Tarea::Borrar => (if n == 1 { String::from(tr(15, "Un elemento borrado para siempre")) } else { trf(16, "{} elementos borrados para siempre", &[&n]) }, None),
                Tarea::Mover => (
                    if n == 1 { String::from(tr(17, "Un elemento movido")) } else { trf(18, "{} elementos movidos", &[&n]) },
                    t.destino.clone().zip(t.elementos.first().map(|e| e.desde.clone())).map(|(donde, origen)| Deshacer::Mover { donde, nombres: t.resultado.iter().map(|r| r.1.clone()).collect(), origen }),
                ),
                Tarea::Copiar => (if n == 1 { String::from(tr(19, "Un elemento copiado")) } else { trf(20, "{} elementos copiados", &[&n]) }, None),
            };
            self.avisar(k, texto, deshacer);
        }
        // Todo lo que se ve se vuelve a leer (lo de las pestañas tocadas).
        let tocados: Vec<Lugar> = t.elementos.iter().map(|e| e.desde.clone()).chain(t.destino.clone()).chain(t.resultado.iter().map(|r| r.0.clone())).chain(t.papelera.iter().map(|_| papelera_archivos())).collect();
        let creados: Vec<(Lugar, String)> = t.resultado.clone();
        for k in 0..self.ventanas.len() {
            for p in 0..self.ventanas[k].pestanas.len() {
                let pe = &self.ventanas[k].pestanas[p];
                if pe.busca.is_none() && tocados.contains(&pe.lugar) {
                    self.cargar(k, p);
                    // Lo nuevo, elegido (como Nautilus tras pegar).
                    let pe = &mut self.ventanas[k].pestanas[p];
                    if creados.iter().any(|c| c.0 == pe.lugar) && matches!(m.t.tarea, Tarea::Copiar | Tarea::Mover | Tarea::Restaurar) {
                        pe.sel.nada();
                        for i in 0..pe.vista.len() {
                            if creados.iter().any(|c| c.0 == pe.lugar && pe.cosas[pe.vista[i]].e.nombre == c.1) {
                                pe.sel.marcados[i] = true;
                                pe.sel.cursor = Some(i);
                            }
                        }
                    }
                }
            }
            if self.ventanas[k].configurada {
                self.decir_ventana(k);
            }
        }
        self.disco_libre();
    }

    fn disco_libre(&mut self) {
        let r = self.fs().pedir(a::INFO, 0, 0, 0, b"");
        if r.e == a::BIEN {
            if self.disco.0 == 0 {
                println!("  [explorador] disco: {} libres de {}", tamano_fijo(r.b), tamano_fijo(r.a));
            }
            self.disco = (r.a, r.b);
        }
    }

    fn elementos(cosas: &[Cosa]) -> Vec<Elemento> {
        cosas.iter().map(|c| Elemento { desde: c.donde.clone(), nombre: c.e.nombre.clone(), cajon: c.e.cajon }).collect()
    }

    fn raices() -> Vec<u32> {
        alloc::vec![RAIZ_SISTEMA, RAIZ_HOME, RAIZ_RAM, RAIZ_WORK, RAIZ_AMIGA]
    }

    /// Tirar a la papelera lo elegido (o borrar de verdad, preguntando, si no puede ir: Sistema, o ya está en ella).
    fn a_la_papelera(&mut self, k: usize, para_siempre: bool) {
        let cosas = self.ventanas[k].p().elegidas();
        if cosas.is_empty() {
            return;
        }
        let en_papelera = self.ventanas[k].p().en_papelera();
        let sin_papelera = cosas.iter().any(|c| c.donde.raiz != RAIZ_HOME || c.donde.dentro_de(&papelera()));
        if para_siempre || en_papelera || sin_papelera {
            let titulo = if cosas.len() == 1 { trf(21, "¿Borrar \"{}\" para siempre?", &[&cosas[0].e.nombre]) } else { trf(22, "¿Borrar {} elementos para siempre?", &[&cosas.len()]) };
            let texto = String::from(if sin_papelera && !en_papelera && !para_siempre { tr(23, "Aquí no hay papelera: no se podrá recuperar.") } else { tr(24, "No se podrá recuperar.") });
            self.ventanas[k].dialogo = Some(Dialogo::Confirmar { titulo, texto, boton: tr(25, "Borrar"), accion: Accion::Borrar, cosas });
            println!("  [explorador] ventana {}: pregunta antes de borrar", self.ventanas[k].numero);
            self.decir_dialogo(k);
            return;
        }
        let titulo = Self::titulo_papelera(&cosas);
        let t = Trabajo::nuevo(Tarea::Papelera, Self::elementos(&cosas), None, Some(papelera()), Self::raices());
        self.empezar(k, t, titulo);
    }

    fn borrar_ya(&mut self, k: usize, cosas: Vec<Cosa>) {
        let titulo = if cosas.len() == 1 {
            (alloc::format!("Borrar \"{}\"", cosas[0].e.nombre), trf(26, "Borrar \"{}\"", &[&cosas[0].e.nombre]))
        } else {
            (alloc::format!("Borrar {} elementos", cosas.len()), trf(27, "Borrar {} elementos", &[&cosas.len()]))
        };
        let pap = if cosas.iter().all(|c| c.donde == papelera_archivos()) { Some(papelera()) } else { None };
        let t = Trabajo::nuevo(Tarea::Borrar, Self::elementos(&cosas), None, pap, Self::raices());
        self.empezar(k, t, titulo);
    }

    fn restaurar(&mut self, k: usize, nombres: Vec<String>) {
        let cosas: Vec<Elemento> = nombres.into_iter().map(|n| Elemento { desde: papelera_archivos(), cajon: false, nombre: n }).collect();
        if cosas.is_empty() {
            return;
        }
        let titulo = if cosas.len() == 1 {
            (alloc::format!("Restaurar \"{}\"", cosas[0].nombre), trf(28, "Restaurar \"{}\"", &[&cosas[0].nombre]))
        } else {
            (alloc::format!("Restaurar {} elementos", cosas.len()), trf(29, "Restaurar {} elementos", &[&cosas.len()]))
        };
        let t = Trabajo::nuevo(Tarea::Restaurar, cosas, None, Some(papelera()), Self::raices());
        self.empezar(k, t, titulo);
    }

    fn vaciar_papelera(&mut self, k: usize) {
        let r = {
            let mut fs = self.fs();
            ops::abrir(&mut fs, &papelera_archivos()).and_then(|asa| {
                let r = ops::listar(&mut fs, asa, MAX_ENTRADAS);
                ops::cerrar(&mut fs, asa);
                r
            })
        };
        let Ok((v, _)) = r else { return };
        if v.is_empty() {
            self.avisar(k, String::from(tr(30, "La papelera ya está vacía")), None);
            return;
        }
        let cosas: Vec<Cosa> = v.into_iter().map(|e| Cosa { donde: papelera_archivos(), e }).collect();
        self.ventanas[k].dialogo = Some(Dialogo::Confirmar {
            titulo: String::from(tr(31, "¿Vaciar la papelera?")),
            texto: if cosas.len() == 1 { trf(32, "Se borrará para siempre {} elemento.", &[&cosas.len()]) } else { trf(33, "Se borrarán para siempre {} elementos.", &[&cosas.len()]) },
            boton: tr(34, "Vaciar"),
            accion: Accion::Vaciar,
            cosas,
        });
        println!("  [explorador] ventana {}: pregunta antes de vaciar la papelera", self.ventanas[k].numero);
        self.decir_dialogo(k);
    }

    fn copiar_o_cortar(&mut self, k: usize, cortar: bool) {
        let cosas = self.ventanas[k].p().elegidas();
        if cosas.is_empty() {
            return;
        }
        let n = cosas.len();
        println!("  [explorador] {} {} elemento{}: {}", if cortar { "corta" } else { "copia" }, n, if n == 1 { "" } else { "s" }, cosas.iter().map(|c| c.e.nombre.as_str()).collect::<Vec<_>>().join(", "));
        self.portapapeles = Some((cosas, cortar));
        let uno = &self.portapapeles.as_ref().unwrap().0[0].e.nombre;
        let t = match (n == 1, cortar) {
            (true, true) => trf(35, "\"{}\" listo para mover", &[uno]),
            (true, false) => trf(36, "\"{}\" listo para copiar", &[uno]),
            (false, true) => trf(37, "{} elementos listos para mover", &[&n]),
            (false, false) => trf(38, "{} elementos listos para copiar", &[&n]),
        };
        self.avisar(k, t, None);
    }

    fn pegar(&mut self, k: usize, destino: Option<Lugar>) {
        let Some((cosas, cortar)) = self.portapapeles.clone() else { return };
        let destino = destino.unwrap_or_else(|| self.ventanas[k].p().lugar.clone());
        if self.ventanas[k].p().busca.is_some() && destino == self.ventanas[k].p().lugar {
            return;
        }
        self.mover_o_copiar(k, cosas, destino, cortar);
        if cortar {
            self.portapapeles = None;
        }
    }

    fn mover_o_copiar(&mut self, k: usize, cosas: Vec<Cosa>, destino: Lugar, mover: bool) {
        let n = cosas.len();
        let que = if n == 1 { alloc::format!("\"{}\"", cosas[0].e.nombre) } else { alloc::format!("{} elementos", n) };
        let fijo = alloc::format!("{} {} a {}", if mover { "Mover" } else { "Copiar" }, que, migas_fijas(&destino).last().map_or(String::new(), |m| m.0.clone()));
        let a_donde = migas(&destino).last().map_or(String::new(), |m| m.0.clone());
        let visible = match (mover, n == 1) {
            (true, true) => trf(39, "Mover \"{}\" a {}", &[&cosas[0].e.nombre, &a_donde]),
            (true, false) => trf(40, "Mover {} elementos a {}", &[&n, &a_donde]),
            (false, true) => trf(41, "Copiar \"{}\" a {}", &[&cosas[0].e.nombre, &a_donde]),
            (false, false) => trf(42, "Copiar {} elementos a {}", &[&n, &a_donde]),
        };
        let titulo = (fijo, visible);
        let t = Trabajo::nuevo(if mover { Tarea::Mover } else { Tarea::Copiar }, Self::elementos(&cosas), Some(destino), None, Self::raices());
        self.empezar(k, t, titulo);
    }

    fn deshacer(&mut self, k: usize) {
        let Some(d) = self.ventanas[k].aviso.take().and_then(|a| a.deshacer) else { return };
        println!("  [explorador] deshace");
        match d {
            Deshacer::Restaurar(n) => self.restaurar(k, n),
            Deshacer::Renombrar { donde, viejo, nuevo } => {
                self.renombrar_en(k, &donde, &nuevo, &viejo, false);
            }
            Deshacer::Mover { donde, nombres, origen } => {
                let cosas: Vec<Cosa> = nombres.into_iter().map(|n| Cosa { donde: donde.clone(), e: Entrada { nombre: n, cajon: false, tam: 0, fecha: 0, modo: 0, bits: 0 } }).collect();
                self.mover_o_copiar(k, cosas, origen, true);
            }
        }
    }

    /// Cambia el nombre de `viejo` a `nuevo` en `donde`. El estado.
    fn renombrar_en(&mut self, k: usize, donde: &Lugar, viejo: &str, nuevo: &str, con_deshacer: bool) -> i32 {
        let e = {
            let mut fs = self.fs();
            match ops::abrir(&mut fs, donde) {
                Ok(dir) => {
                    let e = ops::renombrar(&mut fs, dir, viejo, dir, nuevo);
                    ops::cerrar(&mut fs, dir);
                    e
                }
                Err(e) => e,
            }
        };
        println!("  [explorador] cambia el nombre de {} a \"{}\": {}", texto_lugar(&donde.hijo(viejo)), nuevo, e);
        if e == a::BIEN {
            self.recargar_lugar(donde);
            for v in &mut self.ventanas {
                for p in &mut v.pestanas {
                    if p.lugar == *donde {
                        if let Some(i) = (0..p.vista.len()).find(|&i| p.cosa(i).is_some_and(|c| c.e.nombre == nuevo)) {
                            p.sel.clic(i, false, false);
                        }
                    }
                }
            }
            if con_deshacer {
                self.avisar(k, trf(43, "\"{}\" ahora se llama \"{}\"", &[&viejo, &nuevo]), Some(Deshacer::Renombrar { donde: donde.clone(), viejo: String::from(viejo), nuevo: String::from(nuevo) }));
            }
            for k in 0..self.ventanas.len() {
                self.decir_ventana(k);
            }
        }
        e
    }

    fn recargar_lugar(&mut self, l: &Lugar) {
        for k in 0..self.ventanas.len() {
            for p in 0..self.ventanas[k].pestanas.len() {
                if self.ventanas[k].pestanas[p].lugar == *l && self.ventanas[k].pestanas[p].busca.is_none() {
                    self.cargar(k, p);
                }
            }
        }
    }

    // --- Diálogos ---

    fn decir_dialogo(&mut self, k: usize) {
        self.presentar(k);
        let v = &self.ventanas[k];
        let t: Vec<String> = v
            .zonas
            .iter()
            .filter_map(|(r, z)| match z {
                Zona::BotonDialogo(i) => Some(alloc::format!("botón {} en ({}, {})", i, r.x + r.w / 2, r.y + r.h / 2)),
                Zona::CampoDialogo => Some(alloc::format!("campo en ({}, {})", r.x + r.w / 2, r.y + r.h / 2)),
                Zona::FilaDialogo(i) => Some(alloc::format!("fila {} en ({}, {})", i, r.x + r.w / 2, r.y + r.h / 2)),
                _ => None,
            })
            .collect();
        println!("  [explorador] ventana {}, diálogo: {}", v.numero, t.join("; "));
    }

    fn dialogo_nombre(&mut self, k: usize, nuevo: bool) {
        // El título, también en castellano: para la consola.
        let (titulo, titulo_fijo, boton, linea, viejo) = if nuevo {
            (tr(44, "Carpeta nueva"), "Carpeta nueva", tr(45, "Crear"), Linea::con(tr(46, "Carpeta sin nombre")), String::new())
        } else {
            let p = self.ventanas[k].p();
            let el = p.sel.elegidos();
            if el.len() != 1 || p.en_papelera() {
                return;
            }
            let Some(c) = p.cosa(el[0]) else { return };
            let mut l = Linea::con(&c.e.nombre);
            // Como Nautilus: marcado el nombre sin la extensión.
            let (base, _) = base_y_extension(&c.e.nombre, c.e.cajon);
            l.marca = Some(0);
            l.cursor = base.len();
            (tr(47, "Cambiar el nombre"), "Cambiar el nombre", tr(48, "Cambiar"), l, c.e.nombre.clone())
        };
        self.ventanas[k].dialogo = Some(Dialogo::Nombre { titulo, boton, linea, nuevo, viejo, error: None });
        println!("  [explorador] ventana {}: {}", self.ventanas[k].numero, titulo_fijo);
        self.decir_dialogo(k);
    }

    /// El botón de un diálogo (0, cancelar; 1, el de hacer).
    fn boton_dialogo(&mut self, k: usize, b: usize) {
        let Some(d) = self.ventanas[k].dialogo.take() else { return };
        if b == 0 {
            if let Dialogo::Propiedades { asa, i, .. } = d {
                let mut fs = self.fs();
                ops::cerrar(&mut fs, asa);
                self.contexto = Some(i.ctx);
            }
            println!("  [explorador] ventana {}: diálogo cancelado", self.ventanas[k].numero);
            self.presentar(k);
            return;
        }
        match d {
            Dialogo::Nombre { titulo, boton, linea, nuevo, viejo, .. } => {
                let n = linea.texto.trim();
                let n = String::from(n);
                // El error: (en castellano, para la consola; en el idioma del usuario, para el diálogo).
                let error: Option<(&str, &str)> = if !nombre_valido(&n) {
                    Some(if n.contains('/') || n.contains(':') {
                        ("El nombre no puede llevar \"/\" ni \":\"", tr(49, "El nombre no puede llevar \"/\" ni \":\""))
                    } else if n == "." || n == ".." {
                        ("\".\" y \"..\" no valen como nombre", tr(50, "\".\" y \"..\" no valen como nombre"))
                    } else if n.is_empty() {
                        ("Escribe un nombre", tr(51, "Escribe un nombre"))
                    } else {
                        ("Ese nombre no vale", tr(52, "Ese nombre no vale"))
                    })
                } else if !nuevo && n == viejo {
                    None
                } else if self.ventanas[k].p().cosas.iter().any(|c| plano(&c.e.nombre) == plano(&n) && plano(&c.e.nombre) != plano(&viejo)) {
                    Some(if nuevo {
                        ("Ya hay una carpeta o un fichero con ese nombre", tr(53, "Ya hay una carpeta o un fichero con ese nombre"))
                    } else {
                        ("Ya hay otro con ese nombre", tr(54, "Ya hay otro con ese nombre"))
                    })
                } else {
                    None
                };
                if let Some((fijo, e)) = error {
                    println!("  [explorador] ventana {}: el nombre \"{}\" no vale: {}", self.ventanas[k].numero, n, fijo);
                    self.ventanas[k].dialogo = Some(Dialogo::Nombre { titulo, boton, linea, nuevo, viejo, error: Some(String::from(e)) });
                    self.presentar(k);
                    return;
                }
                let donde = self.ventanas[k].p().lugar.clone();
                if nuevo {
                    let r = {
                        let mut fs = self.fs();
                        match ops::abrir(&mut fs, &donde) {
                            Ok(dir) => {
                                let r = fs.pedir(a::CREAR, dir, a::CAJON as u64, 0, n.as_bytes());
                                if r.e == a::BIEN {
                                    ops::cerrar(&mut fs, r.asa);
                                }
                                ops::cerrar(&mut fs, dir);
                                r.e
                            }
                            Err(e) => e,
                        }
                    };
                    println!("  [explorador] crea la carpeta {}: {}", texto_lugar(&donde.hijo(&n)), r);
                    if r == a::BIEN {
                        self.recargar_lugar(&donde);
                        let p = self.ventanas[k].pm();
                        if let Some(i) = (0..p.vista.len()).find(|&i| p.cosa(i).is_some_and(|c| c.e.nombre == n)) {
                            p.sel.clic(i, false, false);
                        }
                        self.decir_ventana(k);
                    } else {
                        self.avisar(k, trf(55, "No se pudo crear \"{}\": {}", &[&n, &ops::texto_error(r)]), None);
                        self.presentar(k);
                    }
                } else if n != viejo {
                    let e = self.renombrar_en(k, &donde, &viejo, &n, true);
                    if e != a::BIEN {
                        self.avisar(k, trf(56, "No se pudo cambiar el nombre: {}", &[&ops::texto_error(e)]), None);
                        self.presentar(k);
                    }
                } else {
                    self.presentar(k);
                }
            }
            Dialogo::Confirmar { accion, cosas, .. } => {
                println!("  [explorador] ventana {}: confirma {:?}", self.ventanas[k].numero, accion);
                self.borrar_ya(k, cosas);
                self.presentar(k);
            }
            Dialogo::AbrirCon { fichero, programas, elegido } => {
                if let Some(p) = elegido.and_then(|i| programas.get(i)) {
                    let (estado, pid) = lanzar(&mut self.c, p);
                    println!("  [explorador] abre {} con Programas/{}: {} (proceso {})", fichero, p, estado, pid);
                    let t = if estado == 0 { trf(57, "\"{}\" está en marcha (aún no recibe el fichero)", &[p]) } else { trf(7, "\"{}\" no se pudo lanzar ({})", &[p, &estado]) };
                    self.avisar(k, t, None);
                }
                self.presentar(k);
            }
            d @ Dialogo::Propiedades { .. } => {
                self.ventanas[k].dialogo = Some(d);
                self.guardar_propiedades(k);
            }
        }
    }

    fn abrir_con(&mut self, k: usize) {
        let p = self.ventanas[k].p();
        let el = p.sel.elegidos();
        let Some(c) = el.first().and_then(|&i| p.cosa(i)).cloned() else { return };
        if c.e.cajon {
            return;
        }
        let r = {
            let mut fs = self.fs();
            ops::abrir(&mut fs, &Lugar::raiz(RAIZ_SISTEMA).hijo("Programas")).and_then(|asa| {
                let r = ops::listar(&mut fs, asa, 1000);
                ops::cerrar(&mut fs, asa);
                r
            })
        };
        let mut programas: Vec<String> = r.map(|x| x.0).unwrap_or_default().into_iter().filter(|e| e.programa()).map(|e| e.nombre).collect();
        programas.sort_by(|x, y| comparar_nombres(x, y));
        self.ventanas[k].dialogo = Some(Dialogo::AbrirCon { fichero: texto_lugar(&c.donde.hijo(&c.e.nombre)), programas, elegido: None });
        self.decir_dialogo(k);
    }

    /// Propiedades: la Información del Workbench (comentario, permisos y bits del Amiga, dueño), en un diálogo.
    fn propiedades(&mut self, k: usize) {
        let p = self.ventanas[k].p();
        let el = p.sel.elegidos();
        let (donde, nombre) = match el.first().and_then(|&i| p.cosa(i)) {
            Some(c) => (c.donde.clone(), Some(c.e.nombre.clone())),
            None => (p.lugar.clone(), None),
        };
        let r = {
            let mut fs = self.fs();
            match ops::abrir(&mut fs, &donde) {
                Ok(dir) => match &nombre {
                    Some(n) => {
                        let r = fs.pedir(a::BUSCAR, dir, 0, 0, n.as_bytes());
                        ops::cerrar(&mut fs, dir);
                        if r.e == a::BIEN { Ok(r.asa) } else { Err(r.e) }
                    }
                    None => Ok(dir),
                },
                Err(e) => Err(e),
            }
        };
        let Ok(asa) = r else { return };
        let ruta = match &nombre {
            Some(n) => texto_lugar(&donde.hijo(n)),
            None => texto_lugar(&donde),
        };
        let (d, du, co) = {
            let mut fs = self.fs();
            (fs.pedir(a::DATOS, asa, 0, 0, b""), fs.pedir(a::DUENO, asa, 0, 0, b""), fs.pedir(a::COMENTARIO, asa, 0, 0, b""))
        };
        if d.e != a::BIEN || du.e != a::BIEN {
            let mut fs = self.fs();
            ops::cerrar(&mut fs, asa);
            return;
        }
        let comentario: String = if co.e == a::BIEN {
            let fs = self.fs();
            let n = (co.a as usize).min(255);
            String::from_utf8_lossy(&fs.ventana()[..n]).chars().filter(|c| !c.is_control()).collect()
        } else {
            String::new()
        };
        let modo = (d.c & 0o7777) as i64;
        let tipo = ((d.c >> 16) & 0xff) as u8;
        let bits = ((d.c >> 32) & 0xffff) as i64;
        let (dueno, grupo, yo) = (du.a, du.b, du.c);
        let puede = dueno == yo && donde.raiz != RAIZ_SISTEMA;
        let porque = if donde.raiz == RAIZ_SISTEMA { tr(58, "Por SYS: solo se puede leer") } else if dueno != yo { tr(59, "Solo lo puede cambiar su dueño") } else { "" };
        let titulo_t = nombre.clone().unwrap_or_else(|| migas(&donde).last().map_or(String::new(), |m| m.0.clone()));
        // Qué es: en castellano para la consola (`que`) y en el idioma del usuario para el diálogo (`que_visible`).
        let como_fichero = Entrada { nombre: titulo_t.clone(), cajon: false, tam: d.a, fecha: d.b, modo: modo as u16, bits: 0 };
        let que = if tipo == a::CAJON { String::from("Carpeta") } else { alloc::format!("{} · {}", tipo_fijo(&como_fichero), tamano_fijo(d.a)) };
        let que_visible = if tipo == a::CAJON { String::from(tr(60, "Carpeta")) } else { alloc::format!("{} · {}", tipo_texto(&como_fichero), tamano(d.a)) };
        let mut arbol = Arbol::new();
        let titulo = texto_obj(&mut arbol, &titulo_t, 1);
        let linea1 = texto_obj(&mut arbol, &trf(61, "{} · cambiado: {}", &[&que_visible, &fecha(d.b, self.ahora())]), 2);
        let tu = if dueno == yo { " (tú)" } else { "" };
        let en = lugar_visible(&donde);
        let linea2 = if dueno == yo {
            trf(62, "Dueño: usuario {} (tú) · grupo {} · en {}", &[&dueno, &grupo, &en])
        } else {
            trf(63, "Dueño: usuario {} · grupo {} · en {}", &[&dueno, &grupo, &en])
        };
        let linea2 = texto_obj(&mut arbol, &linea2, 2);
        let campo = arbol.crear(Campo, &[(atr::TEXTO, Valor::Texto(comentario.clone())), (atr::ETIQUETA, Valor::Texto(tr(64, "Comentario").into())), (atr::DESACTIVADO, Valor::Bool(!puede))]);
        let valor = bits << 16 | modo;
        let rejilla = arbol.crear(permisos_kit::Permisos, &[(atr::VALOR, Valor::Int(valor)), (atr::CURSOR, Valor::Int(-1)), (atr::DESACTIVADO, Valor::Bool(!puede))]);
        let aviso = texto_obj(&mut arbol, porque, 2);
        let hueco = texto_obj(&mut arbol, "", 0);
        let cancelar = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto(String::from(if puede { tr(65, "Cancelar") } else { tr(66, "Cerrar") }))), (atr::ESTILO, Valor::Int(2))]);
        let guardar = arbol.crear(Boton, &[(atr::TEXTO, Valor::Texto(tr(67, "Guardar").into()))]);
        let marca = texto_obj(&mut arbol, "", 0);
        arbol.notificar(cancelar, atr::CLIC, Cuando::Igual(Valor::Bool(true)), marca, atr::TEXTO, Que::Fijo(Valor::Texto("cancelar".into())));
        arbol.notificar(guardar, atr::CLIC, Cuando::Igual(Valor::Bool(true)), marca, atr::TEXTO, Que::Fijo(Valor::Texto("guardar".into())));
        let mut botones = alloc::vec![Hijo::Obj(hueco), Hijo::Obj(cancelar)];
        if puede {
            botones.push(Hijo::Obj(guardar));
        }
        let raiz = Hijo::Columna(alloc::vec![Hijo::Obj(titulo), Hijo::Obj(linea1), Hijo::Obj(linea2), Hijo::Obj(campo), Hijo::Obj(rejilla), Hijo::Obj(aviso), Hijo::Fila(botones)]);
        let mut i = match self.contexto.take() {
            Some(mut ctx) => {
                ctx.tema = self.tema;
                Interfaz::con_contexto(arbol, raiz, ctx)
            }
            None => Interfaz::new(arbol, raiz, self.tema),
        };
        i.esquinas = kit::tema::RADIO_DIALOGO;
        i.margen = 24;
        let alto = i.medida().min.1.clamp(200, 600);
        println!("  [explorador] Propiedades: {} de alto, la ventana {}", alto, self.ventanas[k].h);
        let ancho = 460;
        let v = &self.ventanas[k];
        let rect = Rect::new((v.w - ancho) / 2, ((v.h - alto) / 2).max(4), ancho, alto.min(v.h - 8));
        i.colocar(Rect::new(0, 0, rect.w, rect.h));
        if puede {
            i.foco = Some(campo);
        }
        let centro = |o: Obj| i.sitios.get(&o).map_or((0, 0), |r| (rect.x + r.x + r.w / 2, rect.y + r.y + r.h / 2));
        let (cx, cy) = centro(campo);
        let (gx, gy) = if puede { centro(guardar) } else { centro(cancelar) };
        let (ax, ay) = i.sitios.get(&rejilla).map_or((0, 0), |&r| {
            let (x, y) = permisos_kit::Permisos::centro(r, 3, 2);
            (rect.x + x, rect.y + y)
        });
        println!(
            "  [explorador] Propiedades de {} (ventana {}): {}, dueño {}{}, grupo {}, modo {:o}, bits {:#x}, comentario \"{}\"; se puede cambiar: {}",
            ruta, v.numero, que, dueno, tu, grupo, modo, bits, comentario, if puede { "sí" } else { "no" }
        );
        println!("  [explorador] ventana {}, propiedades: campo en ({}, {}); Archivado en ({}, {}); {} en ({}, {})", v.numero, cx, cy, ax, ay, if puede { "Guardar" } else { "Cerrar" }, gx, gy);
        self.ventanas[k].dialogo = Some(Dialogo::Propiedades { i, rect, asa, comentario, valor, puede, campo, rejilla, aviso, marca, ruta, donde });
        self.presentar(k);
    }

    fn guardar_propiedades(&mut self, k: usize) {
        let Some(Dialogo::Propiedades { i, asa, comentario, valor, puede, campo, rejilla, aviso, ruta, donde, .. }) = &self.ventanas[k].dialogo else { return };
        if !*puede {
            return;
        }
        let texto = i.arbol.leer(*campo, atr::TEXTO).map(Valor::texto).unwrap_or_default();
        let nuevo = i.arbol.leer(*rejilla, atr::VALOR).map_or(*valor, Valor::int);
        let (asa, ruta, viejo, valor_viejo, aviso, donde) = (*asa, ruta.clone(), comentario.clone(), *valor, *aviso, donde.clone());
        let mut fallo = None;
        let mut partes = Vec::new();
        {
            let mut fs = self.fs();
            if texto != viejo {
                let e = if texto.len() > 200 { a::MAL_FORMADO } else { fs.pedir(a::COMENTAR, asa, 0, 0, texto.as_bytes()).e };
                partes.push(alloc::format!("comentario \"{}\": {}", texto, e));
                if e != a::BIEN {
                    fallo = Some(if e == a::MAL_FORMADO { tr(68, "El comentario es demasiado largo") } else { tr(69, "No se pudo cambiar el comentario") });
                }
            }
            if nuevo != valor_viejo {
                let (bits, modo) = ((nuevo >> 16) as u64, (nuevo & 0o7777) as u64);
                let e = fs.pedir(a::PROTEGER, asa, bits, modo, b"").e;
                partes.push(alloc::format!("protección bits {:#x} modo {:o}: {}", bits, modo, e));
                if e != a::BIEN {
                    fallo = Some(tr(70, "No se pudo cambiar la protección"));
                }
            }
        }
        if partes.is_empty() {
            partes.push(String::from("nada cambiado"));
        }
        println!("  [explorador] guarda {}: {}", ruta, partes.join(", "));
        match fallo {
            None => {
                if let Some(Dialogo::Propiedades { asa, i, .. }) = self.ventanas[k].dialogo.take() {
                    let mut fs = self.fs();
                    ops::cerrar(&mut fs, asa);
                    self.contexto = Some(i.ctx);
                }
                self.recargar_lugar(&donde);
                self.presentar(k);
            }
            Some(porque) => {
                if let Some(Dialogo::Propiedades { i, .. }) = &mut self.ventanas[k].dialogo {
                    i.arbol.poner(aviso, atr::TEXTO, Valor::Texto(String::from(porque)));
                }
                self.presentar(k);
            }
        }
    }

    // --- Menús ---

    fn menu_de(&mut self, k: usize, x: i32, y: i32, sobre_cosa: bool) {
        let p = self.ventanas[k].p();
        let papelera = p.en_papelera();
        let n = p.sel.cuantos();
        let solo_leer = p.lugar.raiz == RAIZ_SISTEMA;
        let hay_pegar = self.portapapeles.is_some() && !solo_leer && !papelera;
        let un_fichero = n == 1 && p.sel.elegidos().first().and_then(|&i| p.cosa(i)).is_some_and(|c| !c.e.cajon);
        let ocultos = if p.ocultos { (tr(71, "Esconder los ocultos"), "Esconder los ocultos") } else { (tr(72, "Mostrar los ocultos"), "Mostrar los ocultos") };
        let opciones: Vec<(&'static str, Accion, bool, &'static str)> = if sobre_cosa && papelera {
            alloc::vec![
                (tr(73, "Restaurar"), Accion::Restaurar, true, "Restaurar"),
                (tr(74, "Borrar para siempre"), Accion::Borrar, true, "Borrar para siempre"),
                (tr(75, "Propiedades"), Accion::Propiedades, n == 1, "Propiedades"),
            ]
        } else if sobre_cosa {
            alloc::vec![
                (tr(76, "Abrir"), Accion::Abrir, n == 1, "Abrir"),
                (tr(77, "Abrir en una pestaña nueva"), Accion::AbrirPestana, n == 1 && !un_fichero, "Abrir en una pestaña nueva"),
                (tr(78, "Abrir con…"), Accion::AbrirCon, un_fichero, "Abrir con…"),
                (tr(79, "Cortar"), Accion::Cortar, !solo_leer, "Cortar"),
                (tr(80, "Copiar"), Accion::Copiar, true, "Copiar"),
                (tr(81, "Pegar dentro"), Accion::Pegar, hay_pegar && n == 1 && !un_fichero, "Pegar dentro"),
                (tr(82, "Cambiar el nombre…"), Accion::Renombrar, n == 1 && !solo_leer, "Cambiar el nombre…"),
                (tr(83, "Mover a la papelera"), Accion::Papelera, !solo_leer && p.lugar.raiz == RAIZ_HOME, "Mover a la papelera"),
                (tr(84, "Borrar para siempre…"), Accion::Borrar, !solo_leer, "Borrar para siempre…"),
                (tr(75, "Propiedades"), Accion::Propiedades, n == 1, "Propiedades"),
            ]
        } else if papelera {
            alloc::vec![
                (tr(85, "Vaciar la papelera"), Accion::Vaciar, !p.vista.is_empty(), "Vaciar la papelera"),
                (tr(86, "Elegir todo"), Accion::TodoElegir, true, "Elegir todo"),
                (tr(72, "Mostrar los ocultos"), Accion::Ocultos, true, "Mostrar los ocultos"),
            ]
        } else {
            alloc::vec![
                (tr(87, "Carpeta nueva…"), Accion::NuevaCarpeta, !solo_leer && p.busca.is_none(), "Carpeta nueva…"),
                (tr(88, "Pegar"), Accion::Pegar, hay_pegar && p.busca.is_none(), "Pegar"),
                (tr(86, "Elegir todo"), Accion::TodoElegir, true, "Elegir todo"),
                (ocultos.0, Accion::Ocultos, true, ocultos.1),
                (tr(75, "Propiedades"), Accion::Propiedades, p.busca.is_none(), "Propiedades"),
            ]
        };
        self.ventanas[k].menu = Some(Menu { x, y, opciones, encima: None });
        self.presentar(k);
        let v = &self.ventanas[k];
        let t: Vec<String> = v.zonas.iter().filter_map(|(r, z)| match z {
            Zona::Opcion(i) => v.menu.as_ref().map(|m| alloc::format!("{} en ({}, {})", m.opciones[*i].3, r.x + r.w / 2, r.y + r.h / 2)),
            _ => None,
        }).collect();
        println!("  [explorador] ventana {}, menú: {}", v.numero, t.join("; "));
    }

    fn menu_principal(&mut self, k: usize) {
        let v = &self.ventanas[k];
        let (x, y) = v.zonas.iter().find(|z| z.1 == Zona::Menu).map_or((v.w - 240, 48), |(r, _)| (r.x + r.w - 232, r.y + r.h + 4));
        let p = v.p();
        let ocultos = if p.ocultos { (tr(71, "Esconder los ocultos"), "Esconder los ocultos") } else { (tr(72, "Mostrar los ocultos"), "Mostrar los ocultos") };
        let opciones = alloc::vec![
            (tr(89, "Ventana nueva"), Accion::NuevaVentana, true, "Ventana nueva"),
            (tr(90, "Pestaña nueva"), Accion::NuevaPestana, true, "Pestaña nueva"),
            (tr(87, "Carpeta nueva…"), Accion::NuevaCarpeta, p.lugar.raiz != RAIZ_SISTEMA && !p.en_papelera() && p.busca.is_none(), "Carpeta nueva…"),
            (tr(91, "Iconos"), Accion::Rejilla, p.lista, "Iconos"),
            (tr(92, "Lista"), Accion::Lista, !p.lista, "Lista"),
            (ocultos.0, Accion::Ocultos, true, ocultos.1),
            (tr(93, "Volver a leer"), Accion::Recargar, true, "Volver a leer"),
            (tr(85, "Vaciar la papelera"), Accion::Vaciar, true, "Vaciar la papelera"),
        ];
        self.ventanas[k].menu = Some(Menu { x: x.max(4), y, opciones, encima: None });
        self.presentar(k);
    }

    fn hacer(&mut self, k: usize, accion: Accion) {
        self.ventanas[k].menu = None;
        let cursor = self.ventanas[k].p().sel.cursor;
        let primera = self.ventanas[k].p().sel.elegidos().first().copied().or(cursor);
        match accion {
            Accion::Abrir | Accion::AbrirPestana | Accion::AbrirVentana => {
                if let Some(i) = primera {
                    self.abrir_cosa(k, i, accion);
                }
            }
            Accion::AbrirCon => self.abrir_con(k),
            Accion::Cortar => self.copiar_o_cortar(k, true),
            Accion::Copiar => self.copiar_o_cortar(k, false),
            Accion::Pegar => {
                // Sobre una carpeta elegida, dentro de ella.
                let p = self.ventanas[k].p();
                let dentro = (p.sel.cuantos() == 1).then(|| primera.and_then(|i| p.cosa(i)).filter(|c| c.e.cajon).map(|c| c.donde.hijo(&c.e.nombre))).flatten();
                self.pegar(k, dentro);
            }
            Accion::Renombrar => self.dialogo_nombre(k, false),
            Accion::Papelera => self.a_la_papelera(k, false),
            Accion::Borrar => self.a_la_papelera(k, true),
            Accion::Restaurar => {
                let n: Vec<String> = self.ventanas[k].p().elegidas().into_iter().map(|c| c.e.nombre).collect();
                self.restaurar(k, n);
            }
            Accion::Vaciar => self.vaciar_papelera(k),
            Accion::NuevaCarpeta => self.dialogo_nombre(k, true),
            Accion::TodoElegir => {
                self.ventanas[k].pm().sel.todo();
                println!("  [explorador] ventana {}: elige todo ({})", self.ventanas[k].numero, self.ventanas[k].p().sel.cuantos());
            }
            Accion::Ocultos => {
                let v = &mut self.ventanas[k];
                let o = !v.p().ocultos;
                for p in &mut v.pestanas {
                    p.ocultos = o;
                    p.rehacer_vista();
                }
                println!("  [explorador] ventana {}: {} los ocultos", v.numero, if o { "enseña" } else { "esconde" });
                self.decir_ventana(k);
            }
            Accion::Propiedades => self.propiedades(k),
            Accion::NuevaVentana => {
                let l = self.ventanas[k].p().lugar.clone();
                self.nueva_ventana(l);
            }
            Accion::NuevaPestana => {
                let l = self.ventanas[k].p().lugar.clone();
                self.nueva_pestana(k, l);
            }
            Accion::Recargar => {
                let p = self.ventanas[k].activa;
                self.cargar(k, p);
                self.decir_ventana(k);
            }
            Accion::Rejilla | Accion::Lista => self.poner_vista(k, accion == Accion::Lista),
        }
        self.presentar(k);
    }

    fn poner_vista(&mut self, k: usize, lista: bool) {
        let p = self.ventanas[k].pm();
        p.lista = lista;
        p.desplaz = 0;
        let detalle: Vec<String> = p.vista.iter().map(|&i| {
            let e = &p.cosas[i].e;
            if e.cajon { alloc::format!("{}/", e.nombre) } else { alloc::format!("{} {} {}", e.nombre, tamano_fijo(e.tam), tipo_fijo(e)) }
        }).collect();
        println!("  [explorador] ventana {} como {}: {}", self.ventanas[k].numero, if lista { "lista" } else { "rejilla" }, detalle.join(" | "));
        self.decir_ventana(k);
    }

    /// Ordena por la columna (otra vez la misma: al revés).
    fn ordenar_por(&mut self, k: usize, o: Orden) {
        let numero = self.ventanas[k].numero;
        let p = self.ventanas[k].pm();
        if p.orden == o {
            p.asc = !p.asc;
        } else {
            p.orden = o;
            p.asc = true;
        }
        p.rehacer_vista();
        let nombres: Vec<&str> = p.vista.iter().map(|&i| p.cosas[i].e.nombre.as_str()).collect();
        println!("  [explorador] ventana {}: por {:?}, {}: {}", numero, o, if p.asc { "de menos a más" } else { "de más a menos" }, nombres.join(" | "));
        self.decir_sitios(k);
    }

    // --- La búsqueda y el camino ---

    fn empezar_busqueda(&mut self, k: usize, texto: &str) {
        let v = &mut self.ventanas[k];
        v.foco = Foco::Buscar;
        v.linea = Linea::default();
        v.linea.texto = String::from(texto);
        v.linea.cursor = texto.len();
        self.buscar(k);
    }

    /// Busca lo de la línea en el cajón de la pestaña y en los de dentro (vacía: vuelve al cajón).
    fn buscar(&mut self, k: usize) {
        let texto = String::from(self.ventanas[k].linea.texto.trim());
        let p = self.ventanas[k].pm();
        if let Some(mut b) = p.busca.take() {
            let mut fs = Real { s: &self.sv, c: &mut self.c };
            b.parar(&mut fs);
        }
        let p = self.ventanas[k].pm();
        if texto.is_empty() {
            let a = self.ventanas[k].activa;
            self.cargar(k, a);
            self.decir_ventana(k);
        } else {
            p.busca = Some(ops::Busqueda::nueva(&texto, p.lugar.clone(), p.ocultos));
            p.cosas.clear();
            p.rehacer_vista();
            p.desplaz = 0;
            println!("  [explorador] ventana {}: busca \"{}\" en {} y lo de dentro", self.ventanas[k].numero, texto, texto_lugar(&self.ventanas[k].p().lugar));
        }
        self.presentar(k);
    }

    fn cerrar_busqueda(&mut self, k: usize) {
        self.ventanas[k].foco = Foco::Contenido;
        self.ventanas[k].linea = Linea::default();
        if self.ventanas[k].p().busca.is_some() {
            self.buscar(k);
        }
    }

    /// El lugar de un camino con nombre ("LIBS:", "ENVARC:Sys", "Home:Viaje"), por su tabla de nombres: el de la primera
    /// capa donde exista, como en el Amiga. `None` si el nombre no está en la tabla (entonces vale lo de siempre:
    /// "Home/Privado", "Amiga:"). Un camino que intenta salir del nombre, o que lleva a una raíz que no tiene, no vale.
    fn lugar_por_nombre(&mut self, texto: &str) -> Option<Result<Lugar, Porque>> {
        use dashos::asignaciones as asig;
        let (nombre, _) = texto.split_once(':')?;
        self.nombres.buscar(nombre.as_bytes())?;
        let tabla = self.nombres.clone();
        let mut fs = PorNombres(self.fs());
        let capa = match asig::mirar(&mut fs, &tabla, texto.as_bytes()) {
            Ok((_, capa)) => capa,
            Err(e) if e == a::MAL_FORMADO => return Some(Err(("ese camino no vale", tr(94, "ese camino no vale")))),
            Err(asig::BUCLE) => return Some(Err(("esa asignación lleva a sí misma", tr(95, "esa asignación lleva a sí misma")))),
            Err(asig::NO_CABE) => return Some(Err(("ese camino es demasiado largo", tr(96, "ese camino es demasiado largo")))),
            Err(e) if e == a::PROTEGIDO => return Some(Err(("sin permiso", tr(97, "sin permiso")))),
            Err(_) => return Some(Err(("no existe en ninguna de sus capas", tr(98, "no existe en ninguna de sus capas")))),
        };
        let mut s = asig::Sitios::nuevos();
        tabla.resolver(texto.as_bytes(), &mut s).ok()?;
        let (raiz, camino) = s.sitio(capa)?;
        let nombres = core::str::from_utf8(camino).ok()?.split('/').filter(|p| !p.is_empty()).map(String::from).collect();
        let l = Lugar { raiz: raiz as u32, nombres };
        Some(if App::raices().contains(&l.raiz) && l.valido() { Ok(l) } else { Err(("lleva a donde no llega", tr(99, "lleva a donde no llega"))) })
    }

    fn ir_al_camino(&mut self, k: usize) {
        let texto = self.ventanas[k].linea.texto.clone();
        let actual = self.ventanas[k].p().lugar.clone();
        let destino = self.lugar_por_nombre(texto.trim()).unwrap_or_else(|| interpretar_camino(&texto, &RAICES_ESCRITAS, &actual));
        match destino {
            Ok(l) => {
                // Se abre paso a paso: si algo no es una carpeta, no se va.
                let r = {
                    let mut fs = self.fs();
                    ops::abrir(&mut fs, &l).map(|asa| ops::cerrar(&mut fs, asa))
                };
                match r {
                    Ok(()) => {
                        println!("  [explorador] ventana {}: camino escrito \"{}\": va a {}", self.ventanas[k].numero, texto, texto_lugar(&l));
                        self.ventanas[k].foco = Foco::Contenido;
                        self.ir(k, l, true);
                    }
                    Err(e) => {
                        println!("  [explorador] ventana {}: camino escrito \"{}\": no se abre ({})", self.ventanas[k].numero, texto, e);
                        self.avisar(k, alloc::format!("\"{}\": {}", texto, ops::texto_error(e)), None);
                        self.presentar(k);
                    }
                }
            }
            Err((porque_fijo, porque)) => {
                println!("  [explorador] ventana {}: camino escrito \"{}\": no vale ({})", self.ventanas[k].numero, texto, porque_fijo);
                self.avisar(k, String::from(porque), None);
                self.presentar(k);
            }
        }
    }

    // --- Miniaturas ---

    /// La miniatura siguiente que falte de lo que se ve (una por vuelta). Verdad si hizo una.
    fn una_miniatura(&mut self) -> bool {
        let mut falta = None;
        'fuera: for v in &self.ventanas {
            let p = v.p();
            if p.lista {
                continue;
            }
            for &i in &p.vista {
                let c = &p.cosas[i];
                if c.e.es_imagen() && c.e.tam <= MAX_MINIATURA && !self.miniaturas.contains_key(&(c.donde.clone(), c.e.nombre.clone())) {
                    falta = Some((c.donde.clone(), c.e.nombre.clone()));
                    break 'fuera;
                }
            }
        }
        let Some((donde, nombre)) = falta else { return false };
        let datos = {
            let mut fs = self.fs();
            ops::abrir(&mut fs, &donde).and_then(|dir| {
                let r = ops::leer_entero(&mut fs, dir, &nombre, MAX_MINIATURA);
                ops::cerrar(&mut fs, dir);
                r
            })
        };
        // Se decodifica aquí, en el proceso del explorador (S6b, regla 5), con un tope de píxeles.
        let img = datos.ok().and_then(|d| kit::png::leer(&d, 1536 * 1024).ok()).map(|img| {
            let k = (img.ancho.max(img.alto) / 128).max(1);
            if k > 1 { img.reducir(k, self.tema.superficie) } else { img }
        });
        println!("  [explorador] miniatura de {}: {}", texto_lugar(&donde.hijo(&nombre)), img.as_ref().map_or_else(|| String::from("no se pudo"), |i| alloc::format!("{}x{}", i.ancho, i.alto)));
        self.miniaturas.insert((donde, nombre), img);
        if self.miniaturas.len() > 64 {
            let primera = self.miniaturas.keys().next().cloned().unwrap();
            self.miniaturas.remove(&primera);
        }
        self.presentar_todas();
        true
    }
}

fn ventana_vacia() -> Ventana {
    Ventana {
        numero: 0,
        slot: 0,
        sup: 0,
        xsup: 0,
        top: 0,
        pool: 0,
        bufer: 0,
        mem: 0,
        mitad: 0,
        w: 0,
        h: 0,
        configurada: false,
        maximizada: false,
        pedida: (0, 0),
        limites: (0, 0),
        pestanas: Vec::new(),
        activa: 0,
        foco: Foco::Contenido,
        linea: Linea::default(),
        menu: None,
        dialogo: None,
        encima: Zona::Nada,
        zonas: Vec::new(),
        rect: None,
        aviso: None,
        pulsado: None,
        ultimo: None,
        contenido: Rect::default(),
        columnas: 1,
        filas: 1,
    }
}

#[path = "../explorador/entrada.rs"]
mod entrada;

/// Lo que no debe ver ni hacer (S6b), con sus raíces ya puestas: lo de Carla, subir por encima de Home, crear por la
/// raíz de solo lectura y nombres raros al crear, renombrar y buscar.
pub fn preparar(app: &mut App) {
    comprobar_lo_ajeno(app);
    comprobar_nombres(app);
    preparar_papelera(app);
    app.disco_libre();
    println!("  [explorador] listo: espera a que el Workbench le pida abrir algo");
}

/// Paso 2 de las asignaciones: pide su tabla de nombres a `sesiones` y mira que los caminos con nombre llevan donde
/// deben, por sus capas: `C:` a `SYS:C` (no hay `Home:C`), `LIBS:` y `ENVARC:` a `SYS:`, `Programas:` a `SYS:Programas`;
/// que `T:` (`RAM:T`) no existe todavía, porque `RAM:` es de la sesión y aún no ha entrado nadie; que un camino que sube no vale; que por un nombre no se abre lo de Carla; y que un nombre que no
/// está deja paso a lo de siempre.
fn comprobar_nombres(app: &mut App) {
    match dashos::asignaciones::pedir_tabla(SESIONES) {
        Some(t) => {
            // Con la tabla, el idioma del usuario: antes de que se abra ninguna ventana (sin tabla, en castellano).
            let mut fs = PorNombres(app.fs());
            idiomas::arrancar(&mut fs, &t, &TEXTOS, CATALOGO);
            app.nombres = t;
        }
        None => println!("  [explorador] sin tabla de nombres"),
    }
    let mut ver = |app: &mut App, c: &str| match app.lugar_por_nombre(c) {
        Some(Ok(l)) => texto_lugar(&l),
        Some(Err(e)) => String::from(e.0),
        None => String::from("sin ese nombre"),
    };
    let casos = ["C:grande.txt", "LIBS:", "ENVARC:Sys", "T:", "Programas:hola", "Home:../DeCarla", "SYS:/Libs", "Nada:x", "Amiga:", "SYS:DeCarla"];
    let quiere = [
        "SYS:C/grande.txt", "SYS:Libs", "SYS:Prefs/Env-Archive/Sys", "no existe en ninguna de sus capas", "SYS:Programas/hola", "ese camino no vale", "ese camino no vale", "sin ese nombre",
        "sin ese nombre", "SYS:DeCarla",
    ];
    let dan: Vec<String> = casos.iter().map(|c| ver(app, c)).collect();
    // Lo de Carla: el nombre lleva a SYS:DeCarla (un nombre solo traduce), pero el servicio no deja listarlo.
    let carla = match app.lugar_por_nombre("SYS:DeCarla") {
        Some(Ok(l)) => {
            let mut fs = app.fs();
            ops::abrir(&mut fs, &l).and_then(|asa| {
                let r = ops::listar(&mut fs, asa, 10).map(|_| ());
                ops::cerrar(&mut fs, asa);
                r
            })
        }
        _ => Ok(()),
    };
    let bien = dan.iter().zip(quiere.iter()).all(|(d, q)| d == q) && carla == Err(a::PROTEGIDO);
    let mut t = String::new();
    for (c, d) in casos.iter().zip(dan.iter()) {
        t.push_str(&alloc::format!("{} = {}; ", c, d));
    }
    println!("  [explorador] nombres: {}listar SYS:DeCarla {:?}: {}", t, carla.err(), if bien { "bien" } else { "MAL" });
}

fn comprobar_lo_ajeno(app: &mut App) {
    let mut fs = app.fs();
    let carla = ops::abrir(&mut fs, &Lugar::raiz(RAIZ_SISTEMA).hijo("DeCarla")).and_then(|asa| {
        let r = ops::listar(&mut fs, asa, 10).map(|_| ());
        ops::cerrar(&mut fs, asa);
        r
    });
    let carta = fs.pedir(a::BUSCAR, RAIZ_SISTEMA, 0, 0, b"DeCarla/carta.txt").e;
    let subir = ops::abrir(&mut fs, &Lugar { raiz: RAIZ_HOME, nombres: alloc::vec![String::from("..")] });
    let crear = fs.pedir(a::CREAR, RAIZ_SISTEMA, a::CAJON as u64, 0, b"explorador").e;
    // Copiar algo de Carla a Home: el servicio no deja leerlo.
    let mut t = Trabajo::nuevo(Tarea::Copiar, alloc::vec![Elemento { desde: Lugar::raiz(RAIZ_SISTEMA).hijo("DeCarla"), nombre: String::from("carta.txt"), cajon: false }], Some(Lugar::raiz(RAIZ_HOME)), None, App::raices());
    while !t.paso(&mut fs, 50) {}
    let copia = fs.pedir(a::BUSCAR, RAIZ_HOME, 0, 0, b"carta.txt").e;
    // Nombres raros, también los que el servicio dejaría: por el explorador, ni crear ni renombrar.
    let raros = ["..", ".", "a/b", "x:y", "", "uno|dos", "tab\tdos"].iter().all(|n| !nombre_valido(n));
    let ren = ops::renombrar(&mut fs, RAIZ_HOME, "Privado", RAIZ_HOME, "..");
    // Y el servicio, si alguien le manda esos nombres sin pasar por el explorador.
    let ren_servicio = fs.pedir(a::RENOMBRAR, RAIZ_HOME, 0, RAIZ_HOME as u64, b"Privado\0..").e;
    let crear_punto = [&b".."[..], b"."].iter().map(|n| fs.pedir(a::CREAR, RAIZ_HOME, a::CAJON as u64, 0, n).e).max().unwrap_or(0);
    let mover_dentro = {
        let mut t = Trabajo::nuevo(Tarea::Mover, alloc::vec![Elemento { desde: Lugar::raiz(RAIZ_HOME), nombre: String::from("Privado"), cajon: true }], Some(Lugar::raiz(RAIZ_HOME).hijo("Privado")), None, App::raices());
        while !t.paso(&mut fs, 50) {}
        t.errores.len() == 1 && t.resultado.is_empty()
    };
    // Y el servicio, por su cuenta, tampoco mete un cajón en sí mismo (por asas, sin pasar por el explorador).
    let por_asas = match ops::abrir(&mut fs, &Lugar::raiz(RAIZ_HOME).hijo("Privado")) {
        Ok(asa) => {
            let e = ops::renombrar(&mut fs, RAIZ_HOME, "Privado", asa, "dentro");
            ops::cerrar(&mut fs, asa);
            e
        }
        Err(e) => e,
    };
    let bien = carla == Err(a::PROTEGIDO) && carta == a::PROTEGIDO && subir.is_err() && crear == a::PROTEGIDO && copia == a::NO_EXISTE && raros && ren == a::MAL_FORMADO && ren_servicio != a::BIEN && crear_punto < 0 && mover_dentro && por_asas != a::BIEN;
    println!(
        "  [explorador] lo ajeno: listar DeCarla {:?}, abrir su carta {}, copiarla {}; \"..\" desde Home: {}; crear por SYS: {}; renombrar a \"..\" {} (servicio {}), crear \"..\" {}; Privado dentro de sí {} (servicio {}): {}",
        carla.err(), carta, if copia == a::NO_EXISTE { "no" } else { "SÍ" }, if subir.is_err() { "no" } else { "SÍ" }, crear, ren, ren_servicio, crear_punto, if mover_dentro { "no" } else { "SÍ" }, por_asas, if bien { "bien" } else { "MAL" }
    );
}

/// La papelera de Home: se crea si no está (".Papelera" con "archivos" e "info", solo de Ana).
fn preparar_papelera(app: &mut App) {
    let mut fs = app.fs();
    let mut crear = |dir: u32, n: &str| -> Result<u32, i32> {
        match ops::abrir_cajon(&mut fs, dir, n) {
            Ok(x) => Ok(x),
            Err(a::NO_EXISTE) => {
                let r = fs.pedir(a::CREAR, dir, a::CAJON as u64, 0, n.as_bytes());
                if r.e == a::BIEN {
                    fs.pedir(a::PROTEGER, r.asa, 0, 0o700, b"");
                    Ok(r.asa)
                } else {
                    Err(r.e)
                }
            }
            Err(e) => Err(e),
        }
    };
    let r = crear(RAIZ_HOME, PAPELERA).and_then(|p| {
        let x = crear(p, PAPELERA_ARCHIVOS);
        let y = crear(p, PAPELERA_INFO);
        let e = x.and_then(|x| y.map(|y| (x, y)));
        e.map(|(x, y)| [x, y, p])
    });
    match r {
        Ok(asas) => {
            for asa in asas {
                ops::cerrar(&mut fs, asa);
            }
            println!("  [explorador] papelera en Home:{}: lista", PAPELERA);
        }
        Err(e) => println!("  [explorador] sin papelera: {}", e),
    }
}

fn principal() -> u64 {
    dashos::monton::con_nombre(&"explorador");
    let mut cliente = None;
    for _ in 0..100 {
        cliente = Cliente::conectar();
        if cliente.is_some() {
            break;
        }
        dashos::dormir_us(50_000);
    }
    let Some(mut c) = cliente else {
        println!("  [explorador] sin compositor");
        return 1;
    };
    let Some(globales) = c.globales() else { return 2 };
    let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
    let (Some(gc), Some(gs), Some(gx), Some(ga)) = (global("wl_compositor"), global("wl_shm"), global("xdg_wm_base"), global("wl_seat")) else {
        println!("  [explorador] al compositor le falta algo");
        return 3;
    };
    let gt = global("dashos_tema_v1");
    let Some(sv) = Servicio::abrir(&mut c) else {
        println!("  [explorador] sin servicio de archivos");
        return 4;
    };
    let raices = sv.ceder_raices(&mut c);
    println!(
        "  [explorador] raíces: SYS: solo para leer {}, Home: {}, Work: {}, Amiga {}; RAM: la pone sesiones al entrar; ventana del servicio de {} KB",
        raices[0], raices[1], raices[2], raices[3], sv.tam / 1024
    );
    if raices[0] != a::BIEN || raices[1] != a::BIEN {
        return 5;
    }
    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gx).texto(Some("xdg_wm_base")).uint(6).uint(XDG).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
    if let Some(gt) = gt {
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gt).texto(Some("dashos_tema_v1")).uint(1).uint(TEMA).fin();
    }
    e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
    e.empezar(ASIENTO, wl_seat::req::GET_KEYBOARD).uint(TECLADO).fin();
    c.mandar(&e);
    let mut app = App {
        c,
        sv,
        nombres: dashos::asignaciones::Tabla::vacia(),
        tema: Tema::CLARO,
        regular: Fuente::regular(),
        medio: Fuente::medium(),
        ventanas: Vec::new(),
        siguiente_id: PRIMER_ID,
        abiertas: 0,
        sobre: 0,
        x: 0,
        y: 0,
        boton: false,
        serie: 0,
        ctrl: false,
        mayus: false,
        alt: false,
        con_teclado: 0,
        teclado: Teclado::new(),
        portapapeles: None,
        trabajos: Vec::new(),
        siguiente_trabajo: 0,
        arrastre: None,
        miniaturas: BTreeMap::new(),
        iconos_amiga: Default::default(),
        disco: (0, 0),
        contexto: None,
        quienes: BTreeMap::new(),
    };
    // Las comprobaciones y la papelera, más tarde (`entrada::bucle`, en `preparar`): al arrancar, el servicio de
    // archivos y la consola están muy ocupados con los demás, y adelantarse cambiaba el orden en que salen las ventanas.
    entrada::bucle(&mut app)
}
