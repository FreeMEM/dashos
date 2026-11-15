//! El kit de interfaz de DashOS con ABI de C (`kit-c`): las ventanas de los programas de Dash en DashOS.
//!
//! Con `./dash --dashos`, las funciones `mui_*` de Dash (las de MUI en el Amiga) llaman a estas `dash_ui_*`
//! (`include/kit_c.h`). Por dentro es el kit (`kit/`: objetos al modo de MUI y controles de Material 3) y un cliente de
//! Wayland del compositor, como los programas nativos (`usuario/src/bin/ajustes.rs`): el mismo programa de Dash sale
//! con MUI en el Amiga y con Material 3 en DashOS.
//!
//! - Cada cosa que se crea (aplicación, ventana, grupo, control, menú, opción de menú) es un número (> 0), como los
//!   objetos de MUI en Dash.
//! - Una ventana se construye entre `dash_ui_window_begin` y `dash_ui_window_end`, con grupos que se abren y se cierran
//!   (`dash_ui_group_begin`/`end`) y los controles dentro, en orden. Una etiqueta (`dash_ui_label`) seguida de un campo,
//!   un desplegable o una casilla en el mismo grupo pasa a ser la etiqueta de ese control (Material 3); si no, es un
//!   texto.
//! - Los menús de una ventana van en una fila arriba, cada uno un botón de texto que abre su menú (provisional: S10
//!   quiere los menús en la barra de la pantalla, y aún no hay protocolo para eso).
//! - Los eventos son números: `dash_ui_on(objeto, clase, número)` y `dash_ui_wait_event` espera al siguiente. Cerrar la
//!   primera ventana da -1 si no se dijo otro número.
//! - El montón es el del programa (`malloc`/`free` del runtime de Dash); los textos van en UTF-8 y los que se devuelven
//!   valen hasta unas cuantas llamadas después (el programa los copia).
//! - El compositor, por la llave que diga `dash_ui_usar_llave` (el runtime de Dash la sabe por cuántas trae el programa:
//!   `runtime/dashos/plat.c` de Dash). Sin ella, la 3 si la hay (lo que lanza `sesiones`: 1 su canal, 2 archivos, 3
//!   ventanas) o si no la 1 (lo que lanza `inicio` con los canales "ventanas" y "archivos").

#![no_std]

extern crate alloc;

#[allow(dead_code)]
#[path = "../../usuario/src/ventana_cliente.rs"]
mod ventana_cliente;

use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::{format, vec};
use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ffi::{CStr, c_char};

use dashos::println;
use kit::controles::{Boton, Campo, Casilla, Ciclo, Contexto, Hijo, Interfaz, Lista, Texto};
use kit::grupos::Medida;
use kit::lienzo::{Lienzo, Rect};
use kit::objetos::{Arbol, Atr, Clase, Cuando, Obj, Que, Valor, atr};
use kit::tema::{Estado as EstadoControl, Tema};
use ventana_cliente::{ASIENTO, COMPOSITOR, Cliente, Lamina, PUNTERO, REGISTRO, SHM, TECLADO, XDG};
use wayland::Escritor;
use wayland::tablas::{
    dashos_tema_v1, wl_buffer, wl_compositor, wl_display, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm_pool,
    wl_surface, xdg_surface, xdg_toplevel, xdg_wm_base,
};

// ---- el montón: el del programa de Dash ----

unsafe extern "C" {
    fn malloc(n: usize) -> *mut u8;
    fn free(p: *mut u8);
    fn exit(codigo: i32) -> !;
}

/// `malloc` del runtime de Dash da bloques alineados a 16: lo que pide más, con hueco delante y el puntero de verdad
/// justo antes.
struct Monton;

unsafe impl GlobalAlloc for Monton {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if l.align() <= 16 {
            return unsafe { malloc(l.size().max(1)) };
        }
        let p = unsafe { malloc(l.size() + l.align() + 16) };
        if p.is_null() {
            return p;
        }
        let a = (p as usize + 16 + l.align() - 1) & !(l.align() - 1);
        unsafe { (a as *mut *mut u8).sub(1).write(p) };
        a as *mut u8
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if l.align() <= 16 {
            unsafe { free(p) }
        } else {
            unsafe { free((p as *mut *mut u8).sub(1).read()) }
        }
    }
}

#[global_allocator]
static MONTON: Monton = Monton;

#[panic_handler]
fn fallo(i: &core::panic::PanicInfo) -> ! {
    println!("  [dash-ui] fallo: {}", i);
    unsafe { exit(20) }
}

// ---- el estado (un solo hilo: el del programa) ----

struct Global<T>(UnsafeCell<Option<T>>);
unsafe impl<T> Sync for Global<T> {}

static ESTADO: Global<Estado> = Global(UnsafeCell::new(None));
/// Los eventos que esperan a `dash_ui_wait_event`. Aparte del estado: los mete el kit (los buzones) mientras atiende al
/// ratón, con el estado prestado.
static COLA: Global<VecDeque<i32>> = Global(UnsafeCell::new(None));

fn est() -> &'static mut Estado {
    unsafe { (*ESTADO.0.get()).get_or_insert_with(Estado::nuevo) }
}

fn cola() -> &'static mut VecDeque<i32> {
    unsafe { (*COLA.0.get()).get_or_insert_with(VecDeque::new) }
}

/// Los eventos de los botones de `dash_ui_request` (por encima de lo que da un programa).
const PREGUNTA: i32 = 0x7fff_0000;

// ---- clases propias ----

/// Un buzón: no se ve; cada valor que le llega (por un aviso de otro objeto) es su evento en la cola.
struct Buzon(i32);

impl Clase for Buzon {
    fn nombre(&self) -> &'static str {
        "buzón"
    }

    fn ajustar(&self, _a: Atr, _v: Valor, _attrs: &BTreeMap<Atr, Valor>) -> Option<Valor> {
        cola().push_back(self.0);
        None
    }
}

/// Las opciones de un menú: "número/1" (activa) o "número/0", separadas por comas, en el orden de `OPCIONES`.
const ATR_IDS: Atr = 1000;

/// El título de un menú de la ventana: un botón de texto que abre su menú (el del kit, el de los desplegables). Elegir
/// una opción da su número; las desactivadas no hacen nada.
struct TituloMenu;

impl Clase for TituloMenu {
    fn nombre(&self) -> &'static str {
        "menú"
    }

    fn ajustar(&self, a: Atr, v: Valor, attrs: &BTreeMap<Atr, Valor>) -> Option<Valor> {
        if a != atr::VALOR {
            return Some(v);
        }
        let ids = attrs.get(&ATR_IDS).map(Valor::texto).unwrap_or_default();
        if let Some(p) = ids.split(',').nth(v.int().max(0) as usize) {
            let mut partes = p.split('/');
            let id: i32 = partes.next().unwrap_or("0").parse().unwrap_or(0);
            if partes.next() == Some("1") {
                cola().push_back(id);
            }
        }
        None
    }

    fn medida(&self, attrs: &BTreeMap<Atr, Valor>, ctx: &mut Contexto) -> Medida {
        Boton.medida(attrs, ctx)
    }

    fn dibujar(&self, attrs: &BTreeMap<Atr, Valor>, l: &mut Lienzo, r: Rect, ctx: &mut Contexto, e: EstadoControl) {
        Boton.dibujar(attrs, l, r, ctx, e)
    }

    fn pulsable(&self) -> bool {
        true
    }

    fn clic(&self, attrs: &BTreeMap<Atr, Valor>) -> Vec<(Atr, Valor)> {
        if attrs.get(&atr::OPCIONES).is_some_and(|o| !o.texto().is_empty()) {
            vec![(atr::ABIERTO, Valor::Bool(true))]
        } else {
            Vec::new()
        }
    }
}

// ---- lo que es cada número ----

#[derive(Clone, Copy, PartialEq)]
enum Cosa {
    App,
    Ventana(usize),
    Grupo(usize),
    /// Un control (o un texto) de la ventana.
    Obj(usize, Obj),
    /// Una etiqueta: su texto, y si pasó a ser la etiqueta de un control, cuál.
    Etiqueta(usize, Obj),
    Menu(usize, usize),
    Opcion(usize, usize, usize),
}

struct OpcionMenu {
    titulo: String,
    id: i32,
    activa: bool,
}

struct MenuVentana {
    obj: Obj,
    opciones: Vec<OpcionMenu>,
}

/// Un grupo abierto mientras se construye una ventana.
struct Marco {
    horizontal: bool,
    hijos: Vec<Hijo>,
    /// La última etiqueta, si es el último hijo: el control que venga detrás la toma.
    etiqueta: Option<(u32, Obj)>,
}

/// Lo que se ve de una ventana abierta: su superficie, su búfer y el tamaño.
struct Superficie {
    sup: u32,
    xsup: u32,
    top: u32,
    bufer: u32,
    lamina: Lamina,
    /// Cuál de sus dos sitios de memoria usa (cambia al cambiar de tamaño).
    alterna: bool,
    medida: (i32, i32),
    pedida: (i32, i32),
    configurada: bool,
}

struct Ventana {
    titulo: String,
    /// Mientras se construye: el árbol y los grupos abiertos.
    arbol: Option<Arbol>,
    pila: Vec<Marco>,
    menus: Vec<MenuVentana>,
    interfaz: Option<Interfaz>,
    sup: Option<Superficie>,
    /// El número que da su botón de cerrar (`dash_ui_on(v, 2, n)`).
    al_cerrar: Option<i32>,
    /// Las listas y sus líneas.
    listas: BTreeMap<Obj, Vec<String>>,
    /// Las etiquetas que pasaron a un control: su texto.
    etiquetas: BTreeMap<Obj, (String, Option<Obj>)>,
    /// Hay que volver a colocar (algo cambió de texto) y dibujar.
    recolocar: bool,
    dicha: bool,
}

/// Dónde se proyectan los píxeles de las ventanas: dos sitios de 256 MB por ventana desde 320 GB (lejos del montón de
/// Dash, desde 4 GB, y de los PIE, de 128 a 256 GB).
const VA_VENTANAS: usize = 0x50_0000_0000;
const SITIO: usize = 0x1000_0000;
const MAX_ANCHO: i32 = 1920;
const MAX_ALTO: i32 = 1200;
/// Lo más pequeño de una ventana (Material 3: un diálogo mide al menos 280).
const MIN_ANCHO: i32 = 280;
/// El objeto del tema del sistema (`dashos_tema_v1`), como en Ajustes.
const TEMA: u32 = 43;

struct Estado {
    cosas: Vec<Cosa>,
    ventanas: Vec<Ventana>,
    construyendo: Option<usize>,
    /// El menú que se está llenando (ventana, menú).
    menu: Option<(usize, usize)>,
    cliente: Option<Cliente>,
    llave: u32,
    siguiente_id: u32,
    tema: Tema,
    tema_actual: (u32, bool),
    puntero: Option<usize>,
    teclado: Option<usize>,
    xy: (i32, i32),
    pulsado: bool,
    /// Los textos devueltos, que valen unas cuantas llamadas.
    textos: [Vec<u8>; 8],
    texto_i: usize,
    /// La pregunta abierta (`dash_ui_request`): solo ella atiende.
    pregunta: Option<usize>,
}

impl Estado {
    fn nuevo() -> Estado {
        Estado {
            cosas: Vec::new(),
            ventanas: Vec::new(),
            construyendo: None,
            menu: None,
            cliente: None,
            llave: 0,
            siguiente_id: 1000,
            tema: Tema::CLARO,
            tema_actual: (0xff67_50a4, false),
            puntero: None,
            teclado: None,
            xy: (0, 0),
            pulsado: false,
            textos: Default::default(),
            texto_i: 0,
            pregunta: None,
        }
    }

    fn cosa(&mut self, c: Cosa) -> i32 {
        self.cosas.push(c);
        self.cosas.len() as i32
    }

    fn de(&self, h: i32) -> Option<Cosa> {
        if h <= 0 { None } else { self.cosas.get(h as usize - 1).copied() }
    }

    fn id(&mut self) -> u32 {
        self.siguiente_id += 1;
        self.siguiente_id
    }

    fn devolver(&mut self, t: &str) -> *const c_char {
        let i = self.texto_i;
        self.texto_i = (i + 1) % self.textos.len();
        let b = &mut self.textos[i];
        b.clear();
        b.extend_from_slice(t.as_bytes());
        b.push(0);
        b.as_ptr() as *const c_char
    }

    /// Se conecta al compositor y pide lo de siempre: compositor, memoria compartida, xdg, el asiento (ratón y
    /// teclado) y el tema del sistema.
    fn conectar(&mut self) -> bool {
        if self.cliente.is_some() {
            return true;
        }
        // Sin decir cuál: la 3 si existe (lo que lanza `sesiones`); si no, la 1 (lo que lanza `inicio`).
        let llave = if self.llave != 0 { self.llave } else if dashos::memoria_tamano(3) != -2 { 3 } else { 1 };
        ventana_cliente::usar_llave(llave);
        let Some(mut c) = Cliente::conectar() else { return false };
        let Some(globales) = c.globales() else { return false };
        let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| (g.0, g.2));
        let (Some(gc), Some(gs), Some(gx), Some(ga)) =
            (global("wl_compositor"), global("wl_shm"), global("xdg_wm_base"), global("wl_seat"))
        else {
            println!("  [dash-ui] al compositor le faltan globales");
            return false;
        };
        let mut e = Escritor::new();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc.0).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs.0).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gx.0).texto(Some("xdg_wm_base")).uint(6).uint(XDG).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga.0).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
        e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
        e.empezar(ASIENTO, wl_seat::req::GET_KEYBOARD).uint(TECLADO).fin();
        if let Some((gt, v)) = global("dashos_tema_v1") {
            e.empezar(REGISTRO, wl_registry::req::BIND).uint(gt).texto(Some("dashos_tema_v1")).uint(v.min(2)).uint(TEMA).fin();
        }
        c.mandar(&e);
        self.cliente = Some(c);
        true
    }

    fn mandar(&mut self, e: &Escritor) {
        if let Some(c) = self.cliente.as_mut() {
            c.mandar(e);
        }
    }

    fn arbol(&mut self, w: usize) -> Option<&mut Arbol> {
        let v = self.ventanas.get_mut(w)?;
        match v.interfaz.as_mut() {
            Some(i) => Some(&mut i.arbol),
            None => v.arbol.as_mut(),
        }
    }

    /// Un control nuevo en el grupo abierto de la ventana que se construye.
    fn control(&mut self, clase: impl Clase + 'static, attrs: &[(Atr, Valor)]) -> i32 {
        let Some(w) = self.construyendo else { return 0 };
        let tomar_etiqueta = matches!(clase.nombre(), "campo" | "ciclo" | "casilla");
        let v = &mut self.ventanas[w];
        let Some(arbol) = v.arbol.as_mut() else { return 0 };
        let o = arbol.crear(clase, attrs);
        let Some(marco) = v.pila.last_mut() else { return 0 };
        // Una etiqueta justo delante: pasa a ser la de este control (Material 3), si no tiene.
        if let Some((_, et)) = marco.etiqueta.take() {
            if tomar_etiqueta && matches!(marco.hijos.last(), Some(Hijo::Obj(x)) if *x == et) {
                let texto = v.etiquetas.get(&et).map(|e| e.0.clone()).unwrap_or_default();
                marco.hijos.pop();
                arbol.poner(o, atr::ETIQUETA, Valor::Texto(texto));
                if let Some(e) = v.etiquetas.get_mut(&et) {
                    e.1 = Some(o);
                }
            }
        }
        marco.hijos.push(Hijo::Obj(o));
        self.cosa(Cosa::Obj(w, o))
    }

    /// Dónde está una cosa: su ventana y su objeto (el control de una etiqueta que pasó a él).
    fn objeto(&self, h: i32) -> Option<(usize, Obj)> {
        match self.de(h)? {
            Cosa::Obj(w, o) => Some((w, o)),
            Cosa::Etiqueta(w, o) => Some((w, o)),
            Cosa::Menu(w, m) => Some((w, self.ventanas[w].menus.get(m)?.obj)),
            _ => None,
        }
    }

    fn clase(&mut self, w: usize, o: Obj) -> &'static str {
        self.arbol(w).and_then(|a| a.objeto(o)).map_or("", |x| x.clase.nombre())
    }

    /// Las opciones de un menú, otra vez en su título.
    fn rehacer_menu(&mut self, w: usize, m: usize) {
        let Some(menu) = self.ventanas.get(w).and_then(|v| v.menus.get(m)) else { return };
        let obj = menu.obj;
        let titulos: Vec<String> = menu.opciones.iter().map(|o| o.titulo.replace('|', "¦")).collect();
        let ids: Vec<String> = menu.opciones.iter().map(|o| format!("{}/{}", o.id, o.activa as u8)).collect();
        if let Some(a) = self.arbol(w) {
            a.poner(obj, atr::OPCIONES, Valor::Texto(titulos.join("|")));
            a.poner(obj, ATR_IDS, Valor::Texto(ids.join(",")));
        }
        self.ventanas[w].recolocar = true;
    }

    /// Las líneas de una lista, otra vez en ella; la elegida sigue dentro.
    fn rehacer_lista(&mut self, w: usize, o: Obj) {
        let Some(lineas) = self.ventanas.get(w).and_then(|v| v.listas.get(&o)) else { return };
        let n = lineas.len() as i64;
        let texto = lineas.iter().map(|l| l.replace('|', "¦")).collect::<Vec<_>>().join("|");
        if let Some(a) = self.arbol(w) {
            a.poner(o, atr::OPCIONES, Valor::Texto(texto));
            let v = a.leer(o, atr::VALOR).map_or(-1, Valor::int);
            if v >= n {
                a.poner(o, atr::VALOR, Valor::Int(if n > 0 { n - 1 } else { -1 }));
            }
            let p = a.leer(o, atr::PRIMERA).map_or(0, Valor::int);
            if p > 0 && p >= n {
                a.poner(o, atr::PRIMERA, Valor::Int((n - 1).max(0)));
            }
        }
        self.ventanas[w].recolocar = true;
    }

    /// Abre la ventana `w`: su superficie, del tamaño que pide la interfaz.
    fn abrir(&mut self, w: usize) -> bool {
        if self.ventanas.get(w).is_none_or(|v| v.interfaz.is_none()) {
            return false;
        }
        if self.ventanas[w].sup.is_some() {
            return true;
        }
        if !self.conectar() {
            return false;
        }
        let tema = self.tema;
        let (sup, xsup, top, pool, bufer) = (self.id(), self.id(), self.id(), self.id(), self.id());
        let v = &mut self.ventanas[w];
        let i = v.interfaz.as_mut().unwrap();
        i.ctx.tema = tema;
        let m = i.medida();
        let medida = (m.min.0.clamp(MIN_ANCHO, MAX_ANCHO), m.min.1.clamp(64, MAX_ALTO));
        i.colocar(Rect::new(0, 0, medida.0, medida.1));
        let mut e = Escritor::new();
        let Some(lamina) = Lamina::nueva(&mut e, (medida.0 * medida.1 * 4) as usize, VA_VENTANAS + w * 2 * SITIO, pool) else {
            println!("  [dash-ui] sin memoria para una ventana de {}x{}", medida.0, medida.1);
            return false;
        };
        e.empezar(pool, wl_shm_pool::req::CREATE_BUFFER).uint(bufer).int(0).int(medida.0).int(medida.1).int(medida.0 * 4).uint(wayland::shm::ARGB8888).fin();
        e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(sup).fin();
        e.empezar(XDG, xdg_wm_base::req::GET_XDG_SURFACE).uint(xsup).uint(sup).fin();
        e.empezar(xsup, xdg_surface::req::GET_TOPLEVEL).uint(top).fin();
        e.empezar(top, xdg_toplevel::req::SET_TITLE).texto(Some(&v.titulo)).fin();
        e.empezar(sup, wl_surface::req::COMMIT).fin();
        v.sup = Some(Superficie { sup, xsup, top, bufer, lamina, alterna: false, medida, pedida: (0, 0), configurada: false });
        v.recolocar = false;
        self.mandar(&e);
        true
    }

    fn cerrar(&mut self, w: usize) {
        let Some(s) = self.ventanas.get_mut(w).and_then(|v| v.sup.take()) else { return };
        let mut e = Escritor::new();
        e.empezar(s.top, xdg_toplevel::req::DESTROY).fin();
        e.empezar(s.xsup, xdg_surface::req::DESTROY).fin();
        e.empezar(s.sup, wl_surface::req::DESTROY).fin();
        e.empezar(s.bufer, wl_buffer::req::DESTROY).fin();
        s.lamina.soltar(&mut e);
        self.mandar(&e);
        if self.puntero == Some(w) {
            self.puntero = None;
        }
        if self.teclado == Some(w) {
            self.teclado = None;
        }
    }

    fn abiertas(&self) -> usize {
        self.ventanas.iter().filter(|v| v.sup.is_some()).count()
    }

    /// Dibuja la ventana `w` y lo enseña (con `adjuntar`, su búfer nuevo).
    fn pintar(&mut self, w: usize, adjuntar: bool) {
        let v = &mut self.ventanas[w];
        let (Some(i), Some(s)) = (v.interfaz.as_mut(), v.sup.as_mut()) else { return };
        if !s.configurada {
            return;
        }
        if v.recolocar {
            i.colocar(Rect::new(0, 0, s.medida.0, s.medida.1));
            v.recolocar = false;
        }
        let (ancho, alto) = s.medida;
        {
            let px = s.lamina.px((ancho * alto) as usize);
            let mut l = Lienzo::new(px, ancho, alto, ancho as usize);
            i.dibujar(&mut l);
        }
        let mut e = Escritor::new();
        if adjuntar {
            e.empezar(s.sup, wl_surface::req::ATTACH).uint(s.bufer).int(0).int(0).fin();
        }
        e.empezar(s.sup, wl_surface::req::DAMAGE).int(0).int(0).int(ancho).int(alto).fin();
        e.empezar(s.sup, wl_surface::req::COMMIT).fin();
        self.mandar(&e);
    }

    /// Las ventanas con algo cambiado, dibujadas otra vez.
    fn repintar(&mut self) {
        for w in 0..self.ventanas.len() {
            let v = &mut self.ventanas[w];
            let sucia = v.recolocar || v.interfaz.as_ref().is_some_and(|i| !i.arbol.sucios_sin_limpiar().is_empty());
            if sucia && v.sup.as_ref().is_some_and(|s| s.configurada) {
                self.pintar(w, false);
            }
        }
    }

    /// La primera vez que se ve: una línea con su tamaño y dónde están sus botones (para las pruebas y para depurar).
    fn decir(&mut self, w: usize) {
        let v = &mut self.ventanas[w];
        let (Some(i), Some(s)) = (v.interfaz.as_ref(), v.sup.as_ref()) else { return };
        if v.dicha {
            return;
        }
        v.dicha = true;
        let mut botones = String::new();
        for (o, r) in &i.sitios {
            let Some(x) = i.arbol.objeto(*o) else { continue };
            if x.clase.nombre() == "botón" || x.clase.nombre() == "menú" {
                let t = x.attrs.get(&atr::TEXTO).map(Valor::texto).unwrap_or_default();
                botones.push_str(&format!("; \"{}\" en ({}, {})", t, r.x + r.w / 2, r.y + r.h / 2));
            }
        }
        println!("  [dash-ui] ventana \"{}\" en pantalla ({}x{}){}", v.titulo, s.medida.0, s.medida.1, botones);
    }

    fn ventana_de(&self, objeto: u32) -> Option<usize> {
        self.ventanas.iter().position(|v| v.sup.as_ref().is_some_and(|s| s.sup == objeto || s.xsup == objeto || s.top == objeto))
    }

    /// Si la ventana `w` atiende al ratón y al teclado (con una pregunta abierta, solo ella).
    fn atiende(&self, w: usize) -> bool {
        self.pregunta.is_none_or(|p| p == w)
    }

    /// Un evento del compositor. Devuelve falso si se cortó la conexión.
    fn atender(&mut self, o: u32, orden: u16, cuerpo: &[u8]) -> bool {
        let palabra = |k: usize| cuerpo.get(4 * k..4 * k + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
        match (o, orden) {
            (XDG, xdg_wm_base::ev::PING) => {
                let mut e = Escritor::new();
                e.empezar(XDG, xdg_wm_base::req::PONG).uint(palabra(0)).fin();
                self.mandar(&e);
            }
            (TEMA, dashos_tema_v1::ev::ESQUEMA) => {
                kit::tema::recordar_evento_esquema(cuerpo);
            }
            (TEMA, dashos_tema_v1::ev::TEMA) => {
                let (color, oscuro) = (palabra(0) | 0xff00_0000, palabra(1) != 0);
                if (color, oscuro) != self.tema_actual {
                    self.tema_actual = (color, oscuro);
                    self.tema = Tema::del_sistema(color, oscuro);
                    for w in 0..self.ventanas.len() {
                        if let Some(i) = self.ventanas[w].interfaz.as_mut() {
                            i.ctx.tema = self.tema;
                        }
                        if self.ventanas[w].sup.as_ref().is_some_and(|s| s.configurada) {
                            self.pintar(w, false);
                        }
                    }
                }
            }
            (TECLADO, wl_keyboard::ev::KEYMAP) => {
                if let Some(l) = self.cliente.as_mut().and_then(|c| c.juntador.tomar_llave()) {
                    dashos::cerrar(l);
                }
            }
            (TECLADO, wl_keyboard::ev::ENTER) => self.teclado = self.ventana_de(palabra(1)),
            (TECLADO, wl_keyboard::ev::LEAVE) => self.teclado = None,
            (TECLADO, wl_keyboard::ev::KEY) => {
                if let Some(w) = self.teclado.filter(|&w| self.atiende(w)) {
                    if let Some(i) = self.ventanas[w].interfaz.as_mut() {
                        if i.tecla(palabra(2), palabra(3) == 1) {
                            self.pintar(w, false);
                        }
                    }
                }
            }
            (PUNTERO, wl_pointer::ev::ENTER) => {
                self.puntero = self.ventana_de(palabra(1));
                self.xy = ((palabra(2) as i32) >> 8, (palabra(3) as i32) >> 8);
                self.raton(None);
            }
            (PUNTERO, wl_pointer::ev::LEAVE) => {
                self.pulsado = false;
                self.xy = (-1, -1);
                self.raton(None);
                self.puntero = None;
            }
            (PUNTERO, wl_pointer::ev::MOTION) => {
                self.xy = ((palabra(1) as i32) >> 8, (palabra(2) as i32) >> 8);
                self.raton(None);
            }
            (PUNTERO, wl_pointer::ev::BUTTON) if palabra(2) == 0x110 => {
                self.pulsado = palabra(3) == 1;
                self.raton(Some(palabra(1)));
            }
            (PUNTERO, wl_pointer::ev::AXIS) if palabra(1) == 0 => {
                if let Some(w) = self.puntero.filter(|&w| self.atiende(w)) {
                    let (x, y) = self.xy;
                    if let Some(i) = self.ventanas[w].interfaz.as_mut() {
                        if i.rueda(x, y, kit::controles::pasos_rueda(palabra(2))) {
                            self.pintar(w, false);
                        }
                    }
                }
            }
            (1, wl_display::ev::ERROR) => {
                println!("  [dash-ui] error de protocolo del compositor ({} {})", palabra(0), palabra(1));
                return false;
            }
            _ => {
                if let Some(w) = self.ventana_de(o) {
                    self.de_ventana(w, o, orden, cuerpo);
                }
            }
        }
        true
    }

    fn raton(&mut self, ms: Option<u32>) {
        let Some(w) = self.puntero.filter(|&w| self.atiende(w)) else { return };
        let ((x, y), pulsado) = (self.xy, self.pulsado);
        let Some(i) = self.ventanas[w].interfaz.as_mut() else { return };
        let cambio = match ms {
            Some(ms) => i.raton_hora(x, y, pulsado, ms),
            None => i.raton(x, y, pulsado),
        };
        if cambio {
            self.pintar(w, false);
        }
    }

    /// Lo de la superficie y la ventana de `w`: configurar (el tamaño) y cerrar.
    fn de_ventana(&mut self, w: usize, o: u32, orden: u16, cuerpo: &[u8]) {
        let palabra = |k: usize| cuerpo.get(4 * k..4 * k + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
        let Some(s) = self.ventanas[w].sup.as_mut() else { return };
        if o == s.top && orden == xdg_toplevel::ev::CONFIGURE {
            s.pedida = (palabra(0) as i32, palabra(1) as i32);
        } else if o == s.top && orden == xdg_toplevel::ev::CLOSE {
            if self.pregunta == Some(w) {
                cola().push_back(PREGUNTA);
            } else if let Some(id) = self.ventanas[w].al_cerrar {
                cola().push_back(id);
            } else if w == self.primera() {
                cola().push_back(-1);
            } else {
                self.cerrar(w);
            }
        } else if o == s.xsup && orden == xdg_surface::ev::CONFIGURE {
            let mut e = Escritor::new();
            e.empezar(s.xsup, xdg_surface::req::ACK_CONFIGURE).uint(palabra(0)).fin();
            if !s.configurada {
                s.configurada = true;
                self.mandar(&e);
                self.pintar(w, true);
                self.decir(w);
                return;
            }
            // Un tamaño nuevo (0 × 0: el propio): otra memoria, otro pool y otro búfer; los viejos se sueltan.
            let propio = {
                let i = self.ventanas[w].interfaz.as_mut().unwrap();
                let m = i.medida();
                (m.min.0.clamp(MIN_ANCHO, MAX_ANCHO), m.min.1.clamp(64, MAX_ALTO))
            };
            let s = self.ventanas[w].sup.as_mut().unwrap();
            let nueva = if s.pedida.0 > 0 && s.pedida.1 > 0 { (s.pedida.0.min(MAX_ANCHO), s.pedida.1.min(MAX_ALTO)) } else { propio };
            if nueva == s.medida {
                self.mandar(&e);
                return;
            }
            let (pool, bufer) = (self.siguiente_id + 1, self.siguiente_id + 2);
            self.siguiente_id += 2;
            let s = self.ventanas[w].sup.as_mut().unwrap();
            let va = VA_VENTANAS + w * 2 * SITIO + if s.alterna { 0 } else { SITIO };
            let Some(lamina) = Lamina::nueva(&mut e, (nueva.0 * nueva.1 * 4) as usize, va, pool) else {
                self.mandar(&e);
                return;
            };
            e.empezar(pool, wl_shm_pool::req::CREATE_BUFFER).uint(bufer).int(0).int(nueva.0).int(nueva.1).int(nueva.0 * 4).uint(wayland::shm::ARGB8888).fin();
            e.empezar(s.bufer, wl_buffer::req::DESTROY).fin();
            let vieja = core::mem::replace(&mut s.lamina, lamina);
            vieja.soltar(&mut e);
            s.bufer = bufer;
            s.alterna = !s.alterna;
            s.medida = nueva;
            let maximizada = s.pedida.0 > 0;
            self.mandar(&e);
            let v = &mut self.ventanas[w];
            if let Some(i) = v.interfaz.as_mut() {
                i.esquinas = if maximizada { 0 } else { kit::tema::RADIO_VENTANA };
            }
            v.recolocar = true;
            self.pintar(w, true);
        }
    }

    /// La primera ventana que se hizo (cerrarla acaba, como en MUI), sin contar las preguntas.
    fn primera(&self) -> usize {
        0
    }

    /// Espera eventos del compositor hasta que haya uno en la cola que cumpla `vale`; lo saca y lo devuelve. -1 si se
    /// corta la conexión o no queda ninguna ventana abierta.
    fn esperar(&mut self, vale: impl Fn(i32) -> bool) -> i32 {
        loop {
            if let Some(k) = cola().iter().position(|&id| vale(id)) {
                return cola().remove(k).unwrap();
            }
            self.repintar();
            if self.abiertas() == 0 {
                return -1;
            }
            let Some((o, orden, cuerpo)) = self.cliente.as_mut().and_then(|c| c.evento()) else { return -1 };
            if !self.atender(o, orden, &cuerpo) {
                return -1;
            }
        }
    }
}

// ---- textos ----

unsafe fn texto(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    String::from_utf8_lossy(unsafe { CStr::from_ptr(p) }.to_bytes()).into_owned()
}

/// Sin los códigos de MUI (ESC y una letra: "\33c", "\33b"...; "\33P[...]" e "\33I[...]" con su corchete).
fn sin_codigos(t: &str) -> String {
    let mut out = String::new();
    let mut cs = t.chars().peekable();
    while let Some(c) = cs.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        let Some(_) = cs.next() else { break };
        if cs.peek() == Some(&'[') {
            for c in cs.by_ref() {
                if c == ']' {
                    break;
                }
            }
        }
    }
    out
}

/// Sin el subrayado que marca la tecla de MUI ("_Guardar").
fn sin_tecla(t: &str) -> String {
    match t.find('_') {
        Some(i) => {
            let mut s = String::from(&t[..i]);
            s.push_str(&t[i + 1..]);
            s
        }
        None => t.to_string(),
    }
}

/// Una etiqueta de formulario como etiqueta de Material: sin códigos, sin la tecla y sin los dos puntos.
fn etiqueta_material(t: &str) -> String {
    sin_tecla(&sin_codigos(t)).trim().trim_end_matches(':').trim().to_string()
}

// ---- la ABI de C (include/kit_c.h) ----

/// La llave del canal "ventanas" (de serie, la 3 si existe y si no la 1).
#[unsafe(no_mangle)]
pub extern "C" fn dash_ui_usar_llave(llave: i32) {
    if llave > 0 {
        est().llave = llave as u32;
    }
}

/// La aplicación: se conecta al compositor. 0 si no hay.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dash_ui_app_begin(_titulo: *const c_char, _base: *const c_char) -> i32 {
    let e = est();
    if let Some(k) = e.cosas.iter().position(|c| *c == Cosa::App) {
        return k as i32 + 1;
    }
    if !e.conectar() {
        return 0;
    }
    e.cosa(Cosa::App)
}

/// Cierra las ventanas y suelta el compositor.
#[unsafe(no_mangle)]
pub extern "C" fn dash_ui_app_end() {
    let e = est();
    for w in 0..e.ventanas.len() {
        e.cerrar(w);
    }
    e.cliente = None;
    e.cosas.clear();
    e.ventanas.clear();
    e.construyendo = None;
    e.menu = None;
    cola().clear();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dash_ui_window_begin(titulo: *const c_char, _id: i32) -> i32 {
    let e = est();
    let titulo = sin_codigos(&unsafe { texto(titulo) });
    e.ventanas.push(Ventana {
        titulo,
        arbol: Some(Arbol::new()),
        pila: vec![Marco { horizontal: false, hijos: Vec::new(), etiqueta: None }],
        menus: Vec::new(),
        interfaz: None,
        sup: None,
        al_cerrar: None,
        listas: BTreeMap::new(),
        etiquetas: BTreeMap::new(),
        recolocar: false,
        dicha: false,
    });
    let w = e.ventanas.len() - 1;
    e.construyendo = Some(w);
    e.cosa(Cosa::Ventana(w))
}

/// Acaba la ventana: su interfaz, con la fila de menús arriba si tiene.
#[unsafe(no_mangle)]
pub extern "C" fn dash_ui_window_end() {
    let e = est();
    let Some(w) = e.construyendo.take() else { return };
    let tema = e.tema;
    let v = &mut e.ventanas[w];
    while v.pila.len() > 1 {
        let m = v.pila.pop().unwrap();
        let g = if m.horizontal { Hijo::Fila(m.hijos) } else { Hijo::Columna(m.hijos) };
        v.pila.last_mut().unwrap().hijos.push(g);
    }
    let mut hijos = v.pila.pop().map(|m| m.hijos).unwrap_or_default();
    let mut arbol = v.arbol.take().unwrap_or_default();
    if !v.menus.is_empty() {
        // A la izquierda, como una barra de menús: un hueco que crece detrás.
        let mut fila: Vec<Hijo> = v.menus.iter().map(|m| Hijo::Obj(m.obj)).collect();
        fila.push(Hijo::Obj(arbol.crear(Texto, &[(atr::TEXTO, Valor::Texto(String::new()))])));
        hijos.insert(0, Hijo::Fila(fila));
    }
    let mut i = Interfaz::new(arbol, Hijo::Columna(hijos), tema);
    // Material 3: como un diálogo, 24 de margen y esquinas redondas (el búfer es ARGB).
    i.margen = 24;
    i.esquinas = kit::tema::RADIO_VENTANA;
    v.interfaz = Some(i);
}

#[unsafe(no_mangle)]
pub extern "C" fn dash_ui_window_open(win: i32) -> i32 {
    let e = est();
    match e.de(win) {
        Some(Cosa::Ventana(w)) => e.abrir(w) as i32,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn dash_ui_window_close(win: i32) {
    let e = est();
    if let Some(Cosa::Ventana(w)) = e.de(win) {
        e.cerrar(w);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn dash_ui_group_begin(horizontal: i32) -> i32 {
    let e = est();
    let Some(w) = e.construyendo else { return 0 };
    if let Some(m) = e.ventanas[w].pila.last_mut() {
        m.etiqueta = None;
    }
    e.ventanas[w].pila.push(Marco { horizontal: horizontal != 0, hijos: Vec::new(), etiqueta: None });
    e.cosa(Cosa::Grupo(w))
}

#[unsafe(no_mangle)]
pub extern "C" fn dash_ui_group_end() {
    let e = est();
    let Some(w) = e.construyendo else { return };
    let v = &mut e.ventanas[w];
    if v.pila.len() < 2 {
        return;
    }
    let m = v.pila.pop().unwrap();
    let g = if m.horizontal { Hijo::Fila(m.hijos) } else { Hijo::Columna(m.hijos) };
    v.pila.last_mut().unwrap().hijos.push(g);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dash_ui_text(t: *const c_char) -> i32 {
    let t = sin_codigos(&unsafe { texto(t) });
    est().control(Texto, &[(atr::TEXTO, Valor::Texto(t))])
}

/// Una etiqueta: un texto que, si le sigue un campo, un desplegable o una casilla, pasa a ser la etiqueta de ese
/// control. Detrás de una casilla sin etiqueta, pasa a ella.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dash_ui_label(t: *const c_char) -> i32 {
    let original = unsafe { texto(t) };
    let e = est();
    let Some(w) = e.construyendo else { return 0 };
    let limpia = etiqueta_material(&original);
    // Detrás de una casilla sin etiqueta: la suya.
    {
        let v = &mut e.ventanas[w];
        let arbol = v.arbol.as_mut().unwrap();
        let marco = v.pila.last_mut().unwrap();
        if let Some(Hijo::Obj(o)) = marco.hijos.last() {
            let o = *o;
            let casilla = arbol.objeto(o).is_some_and(|x| x.clase.nombre() == "casilla" && !x.attrs.contains_key(&atr::ETIQUETA));
            if casilla {
                arbol.poner(o, atr::ETIQUETA, Valor::Texto(limpia.clone()));
                let et = arbol.crear(Texto, &[]);
                v.etiquetas.insert(et, (limpia, Some(o)));
                return e.cosa(Cosa::Etiqueta(w, et));
            }
        }
    }
    let h = e.control(Texto, &[(atr::TEXTO, Valor::Texto(sin_tecla(&sin_codigos(&original))))]);
    let Some(Cosa::Obj(_, o)) = e.de(h) else { return h };
    let v = &mut e.ventanas[w];
    v.etiquetas.insert(o, (limpia, None));
    v.pila.last_mut().unwrap().etiqueta = Some((h as u32, o));
    e.cosas[h as usize - 1] = Cosa::Etiqueta(w, o);
    h
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dash_ui_button(t: *const c_char) -> i32 {
    let t = sin_tecla(&sin_codigos(&unsafe { texto(t) }));
    est().control(Boton, &[(atr::TEXTO, Valor::Texto(t))])
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dash_ui_string(inicial: *const c_char, _maximo: i32) -> i32 {
    let t = unsafe { texto(inicial) };
    est().control(Campo, &[(atr::TEXTO, Valor::Texto(t))])
}

#[unsafe(no_mangle)]
pub extern "C" fn dash_ui_checkbox(marcada: i32) -> i32 {
    est().control(Casilla, &[(atr::VALOR, Valor::Bool(marcada != 0))])
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dash_ui_cycle(opciones: *const c_char) -> i32 {
    let t = unsafe { texto(opciones) };
    let t: Vec<String> = t.split('|').map(|o| sin_tecla(&sin_codigos(o))).collect();
    est().control(Ciclo, &[(atr::OPCIONES, Valor::Texto(t.join("|"))), (atr::VALOR, Valor::Int(0))])
}

#[unsafe(no_mangle)]
pub extern "C" fn dash_ui_list() -> i32 {
    let e = est();
    let h = e.control(Lista, &[(atr::OPCIONES, Valor::Texto(String::new())), (atr::VALOR, Valor::Int(-1))]);
    if let Some(Cosa::Obj(w, o)) = e.de(h) {
        e.ventanas[w].listas.insert(o, Vec::new());
    }
    h
}

/// Una línea en `pos` (0 la primera; negativo o pasado el final, al final).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dash_ui_list_insert(lista: i32, pos: i32, t: *const c_char) {
    let t = unsafe { texto(t) };
    let e = est();
    let Some((w, o)) = e.objeto(lista) else { return };
    let Some(l) = e.ventanas[w].listas.get_mut(&o) else { return };
    let p = if pos < 0 || pos as usize > l.len() { l.len() } else { pos as usize };
    l.insert(p, t);
    e.rehacer_lista(w, o);
}

#[unsafe(no_mangle)]
pub extern "C" fn dash_ui_list_remove(lista: i32, pos: i32) {
    let e = est();
    let Some((w, o)) = e.objeto(lista) else { return };
    let Some(l) = e.ventanas[w].listas.get_mut(&o) else { return };
    if pos >= 0 && (pos as usize) < l.len() {
        l.remove(pos as usize);
        e.rehacer_lista(w, o);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn dash_ui_list_clear(lista: i32) {
    let e = est();
    let Some((w, o)) = e.objeto(lista) else { return };
    if let Some(l) = e.ventanas[w].listas.get_mut(&o) {
        l.clear();
        if let Some(a) = e.arbol(w) {
            a.poner(o, atr::VALOR, Valor::Int(-1));
            a.poner(o, atr::PRIMERA, Valor::Int(0));
        }
        e.rehacer_lista(w, o);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn dash_ui_list_count(lista: i32) -> i32 {
    let e = est();
    e.objeto(lista).and_then(|(w, o)| e.ventanas[w].listas.get(&o)).map_or(0, |l| l.len() as i32)
}

#[unsafe(no_mangle)]
pub extern "C" fn dash_ui_list_get(lista: i32, pos: i32) -> *const c_char {
    let e = est();
    let t = e
        .objeto(lista)
        .and_then(|(w, o)| e.ventanas[w].listas.get(&o))
        .and_then(|l| if pos >= 0 { l.get(pos as usize).cloned() } else { None })
        .unwrap_or_default();
    e.devolver(&t)
}

/// Un menú de la ventana que se construye: un botón de texto en la fila de arriba.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dash_ui_menu_begin(titulo: *const c_char) -> i32 {
    let t = sin_tecla(&sin_codigos(&unsafe { texto(titulo) }));
    let e = est();
    let Some(w) = e.construyendo.or(e.ventanas.len().checked_sub(1)) else { return 0 };
    let Some(a) = e.arbol(w) else { return 0 };
    let obj = a.crear(TituloMenu, &[(atr::TEXTO, Valor::Texto(t)), (atr::ESTILO, Valor::Int(2)), (atr::OPCIONES, Valor::Texto(String::new()))]);
    let v = &mut e.ventanas[w];
    v.menus.push(MenuVentana { obj, opciones: Vec::new() });
    let m = v.menus.len() - 1;
    e.menu = Some((w, m));
    e.cosa(Cosa::Menu(w, m))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dash_ui_menu_item(titulo: *const c_char, _tecla: *const c_char, id: i32) -> i32 {
    let t = sin_tecla(&sin_codigos(&unsafe { texto(titulo) }));
    let e = est();
    let Some((w, m)) = e.menu else { return 0 };
    let ops = &mut e.ventanas[w].menus[m].opciones;
    ops.push(OpcionMenu { titulo: t, id, activa: true });
    let k = ops.len() - 1;
    e.rehacer_menu(w, m);
    e.cosa(Cosa::Opcion(w, m, k))
}

/// Las rayas de MUI entre opciones: el menú de Material del kit no las tiene; no se ven.
#[unsafe(no_mangle)]
pub extern "C" fn dash_ui_menu_separator() {}

#[unsafe(no_mangle)]
pub extern "C" fn dash_ui_menu_end() {
    est().menu = None;
}

/// Que `obj` dé el evento `id`: `clase` 0 al pulsarlo, 1 al cambiar su valor o su texto, 2 al cerrar la ventana, 3 con
/// un doble clic en una lista.
#[unsafe(no_mangle)]
pub extern "C" fn dash_ui_on(obj: i32, clase: i32, id: i32) {
    let e = est();
    if clase == 2 {
        if let Some(Cosa::Ventana(w)) = e.de(obj) {
            e.ventanas[w].al_cerrar = Some(id);
        }
        return;
    }
    let Some((w, o)) = e.objeto(obj) else { return };
    let nombre = e.clase(w, o);
    let (a, cuando) = match (clase, nombre) {
        (0, _) => (atr::CLIC, Cuando::Igual(Valor::Bool(true))),
        (1, "campo") => (atr::TEXTO, Cuando::Siempre),
        (1, _) => (atr::VALOR, Cuando::Siempre),
        (3, _) => (atr::DOBLE, Cuando::Igual(Valor::Bool(true))),
        _ => return,
    };
    let Some(arbol) = e.arbol(w) else { return };
    let buzon = arbol.crear(Buzon(id), &[]);
    arbol.notificar(o, a, cuando, buzon, atr::VALOR, Que::Disparo);
}

#[unsafe(no_mangle)]
pub extern "C" fn dash_ui_get_text(obj: i32) -> *const c_char {
    let e = est();
    let t = match e.de(obj) {
        Some(Cosa::Ventana(w)) => e.ventanas[w].titulo.clone(),
        Some(Cosa::Etiqueta(w, o)) => e.ventanas[w].etiquetas.get(&o).map(|x| x.0.clone()).unwrap_or_default(),
        Some(Cosa::Opcion(w, m, k)) => e.ventanas[w].menus[m].opciones[k].titulo.clone(),
        _ => match e.objeto(obj) {
            Some((w, o)) => e.arbol(w).and_then(|a| a.leer(o, atr::TEXTO)).map(Valor::texto).unwrap_or_default(),
            None => String::new(),
        },
    };
    e.devolver(&t)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dash_ui_set_text(obj: i32, t: *const c_char) {
    let t = unsafe { texto(t) };
    let e = est();
    match e.de(obj) {
        Some(Cosa::Ventana(w)) => {
            let t = sin_codigos(&t);
            if let Some(top) = e.ventanas[w].sup.as_ref().map(|s| s.top) {
                let mut es = Escritor::new();
                es.empezar(top, xdg_toplevel::req::SET_TITLE).texto(Some(&t)).fin();
                e.mandar(&es);
            }
            e.ventanas[w].titulo = t;
        }
        Some(Cosa::Etiqueta(w, o)) => {
            let limpia = etiqueta_material(&t);
            let control = e.ventanas[w].etiquetas.get(&o).and_then(|x| x.1);
            if let Some(x) = e.ventanas[w].etiquetas.get_mut(&o) {
                x.0 = limpia.clone();
            }
            if let Some(a) = e.arbol(w) {
                match control {
                    Some(c) => a.poner(c, atr::ETIQUETA, Valor::Texto(limpia)),
                    None => a.poner(o, atr::TEXTO, Valor::Texto(sin_tecla(&sin_codigos(&t)))),
                }
            }
            e.ventanas[w].recolocar = true;
        }
        Some(Cosa::Opcion(w, m, k)) => {
            e.ventanas[w].menus[m].opciones[k].titulo = sin_tecla(&sin_codigos(&t));
            e.rehacer_menu(w, m);
        }
        _ => {
            let Some((w, o)) = e.objeto(obj) else { return };
            let t = match e.clase(w, o) {
                "campo" => t,
                "botón" | "menú" => sin_tecla(&sin_codigos(&t)),
                _ => sin_codigos(&t),
            };
            if let Some(a) = e.arbol(w) {
                a.poner(o, atr::TEXTO, Valor::Texto(t));
            }
            e.ventanas[w].recolocar = true;
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn dash_ui_get_value(obj: i32) -> i32 {
    let e = est();
    let Some((w, o)) = e.objeto(obj) else { return 0 };
    match e.arbol(w).and_then(|a| a.leer(o, atr::VALOR)) {
        Some(Valor::Bool(b)) => *b as i32,
        Some(v) => v.int() as i32,
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn dash_ui_set_value(obj: i32, valor: i32) {
    let e = est();
    let Some((w, o)) = e.objeto(obj) else { return };
    let v = match e.clase(w, o) {
        "casilla" => Valor::Bool(valor != 0),
        "lista" => {
            let n = e.ventanas[w].listas.get(&o).map_or(0, |l| l.len() as i32);
            Valor::Int(if (0..n).contains(&valor) { valor as i64 } else { -1 })
        }
        _ => Valor::Int(valor as i64),
    };
    if let Some(a) = e.arbol(w) {
        a.poner(o, atr::VALOR, v);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn dash_ui_set_enabled(obj: i32, activo: i32) {
    let e = est();
    if let Some(Cosa::Opcion(w, m, k)) = e.de(obj) {
        e.ventanas[w].menus[m].opciones[k].activa = activo != 0;
        e.rehacer_menu(w, m);
        return;
    }
    let Some((w, o)) = e.objeto(obj) else { return };
    let o = match e.de(obj) {
        Some(Cosa::Etiqueta(w2, et)) => e.ventanas[w2].etiquetas.get(&et).and_then(|x| x.1).unwrap_or(o),
        _ => o,
    };
    if let Some(a) = e.arbol(w) {
        a.poner(o, atr::DESACTIVADO, Valor::Bool(activo == 0));
    }
}

/// Pregunta con un diálogo de Material 3 sobre la ventana `win` (0: sobre la aplicación): un título, un texto y los
/// botones de `botones` separados por "|". Las demás ventanas esperan. Devuelve el botón: 1, 2... desde la izquierda,
/// 0 el último (también al cerrar el diálogo).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dash_ui_request(win: i32, titulo: *const c_char, t: *const c_char, botones: *const c_char) -> i32 {
    let (titulo, t, botones) = unsafe { (texto(titulo), texto(t), texto(botones)) };
    let e = est();
    if !e.conectar() {
        return 0;
    }
    let botones: Vec<String> = if botones.is_empty() { vec!["Ok".into()] } else { botones.split('|').map(|b| sin_tecla(&sin_codigos(b))).collect() };
    let n = botones.len();
    let mut a = Arbol::new();
    let mut hijos = Vec::new();
    let titulo = sin_codigos(&titulo);
    if !titulo.is_empty() {
        hijos.push(Hijo::Obj(a.crear(Texto, &[(atr::TEXTO, Valor::Texto(titulo.clone())), (atr::ESTILO, Valor::Int(1))])));
        hijos.push(Hijo::Espacio(0));
    }
    for linea in sin_codigos(&t).split('\n') {
        hijos.push(Hijo::Obj(a.crear(Texto, &[(atr::TEXTO, Valor::Texto(linea.to_string())), (atr::ESTILO, Valor::Int(2))])));
    }
    hijos.push(Hijo::Espacio(8));
    // Material 3: los botones al final de la fila; el primero, relleno (la acción principal), los demás de texto.
    let mut fila = vec![Hijo::Obj(a.crear(Texto, &[(atr::TEXTO, Valor::Texto(String::new()))]))];
    for (k, b) in botones.iter().enumerate() {
        let o = a.crear(Boton, &[(atr::TEXTO, Valor::Texto(b.clone())), (atr::ESTILO, Valor::Int(if k == 0 { 0 } else { 2 }))]);
        let numero = if k + 1 == n { 0 } else { k as i32 + 1 };
        let buzon = a.crear(Buzon(PREGUNTA + 1 + numero), &[]);
        a.notificar(o, atr::CLIC, Cuando::Igual(Valor::Bool(true)), buzon, atr::VALOR, Que::Disparo);
        fila.push(Hijo::Obj(o));
    }
    hijos.push(Hijo::Fila(fila));
    let mut i = Interfaz::new(a, Hijo::Columna(hijos), e.tema);
    i.margen = 24;
    i.esquinas = kit::tema::RADIO_DIALOGO;
    e.ventanas.push(Ventana {
        titulo: if titulo.is_empty() { "?".into() } else { titulo },
        arbol: None,
        pila: Vec::new(),
        menus: Vec::new(),
        interfaz: Some(i),
        sup: None,
        al_cerrar: None,
        listas: BTreeMap::new(),
        etiquetas: BTreeMap::new(),
        recolocar: false,
        dicha: false,
    });
    let w = e.ventanas.len() - 1;
    if !e.abrir(w) {
        e.ventanas.pop();
        return 0;
    }
    // Sobre su ventana (el compositor la pone delante de ella).
    let padre = match e.de(win) {
        Some(Cosa::Ventana(p)) => e.ventanas[p].sup.as_ref().map(|s| s.top),
        _ => e.ventanas.iter().find_map(|v| v.sup.as_ref().map(|s| s.top)),
    };
    if let (Some(p), Some(top)) = (padre, e.ventanas[w].sup.as_ref().map(|s| s.top)) {
        if p != top {
            let mut es = Escritor::new();
            es.empezar(top, xdg_toplevel::req::SET_PARENT).uint(p).fin();
            e.mandar(&es);
        }
    }
    let antes = e.pregunta.replace(w);
    let r = e.esperar(|id| id >= PREGUNTA && id <= PREGUNTA + n as i32);
    e.pregunta = antes;
    e.cerrar(w);
    // La ventana se queda en la lista (sin superficie): los números de las demás no cambian.
    if r <= PREGUNTA { 0 } else { r - PREGUNTA - 1 }
}

/// El siguiente evento; -1 para acabar (la primera ventana cerrada sin `dash_ui_on`, sin compositor o sin ventanas
/// abiertas).
#[unsafe(no_mangle)]
pub extern "C" fn dash_ui_wait_event() -> i32 {
    let e = est();
    e.esperar(|id| !(PREGUNTA..=PREGUNTA + 64).contains(&id))
}
