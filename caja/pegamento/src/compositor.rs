//! El rasgo `Compositor` de intuition (`caja/intuition/src/compositor.rs`) sobre Wayland: la caja es un cliente del
//! compositor de DashOS por el canal "ventanas", como `ventana-a` o Ajustes (`usuario/src/ventana_cliente.rs`).
//!
//! Cada ventana de Intuition es un `xdg_toplevel` con un búfer `wl_shm` XRGB8888 del tamaño de la ventana entera (los
//! bordes de Intuition van dentro). El búfer es memoria de la caja que el compositor lee con una copia de solo lectura
//! de la llave. Wayland va en little-endian aunque la caja sea big-endian: los mensajes los hace `wayland::Escritor`,
//! que ya escribe en little-endian, y los puntos se escriben con sus bytes dados la vuelta.
//!
//! Nada aquí espera al compositor salvo al conectar y en `sincronizar`: `sacar_eventos` lee lo que haya sin esperar.
//!
//! **Escala** (`caja_intuition::escala`): en una pantalla grande las ventanas se ven a 2x o 3x (`escala` del ajuste de
//! S26, `auto` de serie). El programa no se entera: intuition pinta a 1x; aquí cada píxel va al búfer como un cuadrado
//! de `f` x `f` y las coordenadas del ratón se dividen entre `f`. El compositor ve una ventana normal, más grande. La
//! pantalla de Workbench mide lo de la pantalla real entre el factor de la caja.
//!
//! **La barra de DashOS** (rama `amiga-integrada`): una ventana "como DashOS" con barra pide la del compositor
//! (`xdg-decoration`, modo del servidor), como una de Linux: título, cerrar, maximizar, las cuatro esquinas redondeadas y
//! la sombra (todo del compositor). Cerrar llega como `Evento::Cerrar`
//! (`IDCMP_CLOSEWINDOW`); el borde de DashOS y maximizar, como un `xdg_toplevel.configure` con tamaño, que aquí pasa a
//! `Evento::Tamano` (intuition lo ajusta a `WindowLimits` y manda `IDCMP_NEWSIZE`).
//!
//! **Las pantallas propias**: cada una es un `xdg_toplevel` y sus ventanas van dentro, como `wl_subsurface` en sus
//! coordenadas (se mueven con ella al arrastrarla y van delante de ella). Ponerlas delante o detrás entre sus hermanas es
//! `place_above`/`place_below`; arrastrarlas o cambiarles el tamaño lo hace la caja con el ratón (las subsuperficies no
//! tienen `xdg_toplevel.move`). Una pantalla delante o detrás de las demás ventanas: `dashos_tema_v1.apilar` (versión 7).
//! El teclado llega a la pantalla (el compositor da el foco a las de primer nivel); intuition lo pasa a su ventana activa.
//!
//! **La letra de DashOS** (`caja_intuition::nitido`): ampliada, lo que escribieron nuestras bibliotecas se vuelve a
//! escribir con Roboto del kit al tamaño de la pantalla, en el hueco de la topaz.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::slice;

use caja_intuition::compositor::{Compositor, Evento, NuevaVentana, Superficie, Tokens};
use caja_intuition::escala;
use caja_intuition::lienzo::Rect;
use caja_intuition::nitido::{self, Estilo, Letras, TextoNitido};
use crate::plataforma;
use crate::transporte::{Recogido, Transporte};
use crate::tema::TemaCaja;
use wayland::tablas::{
    dashos_tema_v1, wl_buffer, wl_callback, wl_compositor, wl_display, wl_keyboard, wl_output, wl_pointer, wl_registry, wl_seat,
    wl_shm, wl_shm_pool, wl_subcompositor, wl_subsurface, wl_surface, xdg_surface, xdg_toplevel, xdg_wm_base,
    zxdg_decoration_manager_v1, zxdg_toplevel_decoration_v1,
};
use wayland::aspecto::{Caja, MAX_ESCALA};
use wayland::Escritor;

/// Dónde van los búferes de las ventanas: una franja de 64 MB por superficie, por encima de 4 GB.
const VA_BUFERES: usize = 0x0000_0005_0000_0000;
const FRANJA: usize = 64 << 20;
/// Lo más grande que se deja abrir: 4096 de lado y lo que quepa en una franja.
const MAX_LADO: u32 = 4096;
const MAX_SUPERFICIES: u32 = 256;
/// `wl_shm.format`: XRGB8888 (el alfa de los mapas nativos es siempre 0xFF).
const XRGB8888: u32 = 1;
/// `zxdg_toplevel_decoration_v1.mode`: la barra la pone el compositor.
const MODO_SERVIDOR: u32 = 2;
/// `xdg_toplevel.resize_edge`: abajo a la derecha.
const BORDE_ABAJO_DERECHA: u32 = 10;

struct Ventana {
    sup: u32,
    xdg: u32,
    top: u32,
    pool: u32,
    buf: u32,
    memoria: u32,
    va: usize,
    /// El tamaño de la ventana para el programa (1x) y el factor con que se ve: el búfer mide `ancho·f` x `alto·f`.
    ancho: u32,
    alto: u32,
    f: u32,
    configurada: bool,
    titulo: String,
    /// El programa que la abrió (para su escala).
    programa: String,
    /// Su decoración (`zxdg_toplevel_decoration_v1`, 0 si no lleva la barra de DashOS).
    deco: u32,
    /// El tamaño del último `xdg_toplevel.configure` (en la superficie ampliada; 0 si lo elige ella), hasta su
    /// `xdg_surface.configure`.
    pedido: (i32, i32),
    /// El hueco de la marca de la caja en su barra de intuition (1x), para decírselo al compositor con su factor.
    hueco_marca: Option<(i32, i32)>,
    /// Si va dentro de una pantalla propia: la superficie de la pantalla, su `wl_subsurface` y dónde está (1x, en la
    /// pantalla). Sin `xdg_surface` ni `xdg_toplevel` (`xdg` y `top` a 0).
    padre: Option<Superficie>,
    sub: u32,
    sitio: (i32, i32),
}

/// Lo que se hace con el ratón dentro de una pantalla propia (una subsuperficie no se puede arrastrar desde el
/// compositor): mover una ventana o cambiarle el tamaño, desde donde se pulsó (`ancla`, 1x en la ventana).
#[derive(Clone, Copy, Debug)]
struct Arrastre {
    s: Superficie,
    tamano: bool,
    ancla: (i32, i32),
    /// El tamaño al empezar y el que lleva (al cambiarlo).
    inicio: (u32, u32),
    nuevo: (u32, u32),
}

/// La letra de DashOS (el kit) para los textos nítidos: Roboto, Roboto Medium y Roboto Mono.
struct LetrasKit {
    normal: kit::texto::Fuente,
    grueso: kit::texto::Fuente,
    mono: kit::texto::Fuente,
}

impl LetrasKit {
    fn nuevas() -> LetrasKit {
        LetrasKit { normal: kit::texto::Fuente::regular(), grueso: kit::texto::Fuente::medium(), mono: kit::texto::Fuente::mono() }
    }
    fn de(&mut self, e: Estilo) -> &mut kit::texto::Fuente {
        match e {
            Estilo::Normal => &mut self.normal,
            Estilo::Grueso => &mut self.grueso,
            Estilo::Celdas => &mut self.mono,
        }
    }
}

impl Letras for LetrasKit {
    fn ancho(&mut self, e: Estilo, texto: &str, tam: u32) -> i32 {
        self.de(e).medir(texto, tam).ancho
    }
    fn alturas(&mut self, _e: Estilo, tam: u32) -> (i32, i32) {
        // Roboto: mayúsculas de 1456/2048 del cuerpo y lo que baja la g, unos 430/2048.
        (((tam * 1456 + 1024) / 2048) as i32, ((tam * 430 + 1024) / 2048) as i32)
    }
    fn escribir(&mut self, e: Estilo, px: &mut [u32], ancho: i32, alto: i32, x: i32, base: i32, texto: &str, tam: u32, color: u32) {
        if ancho <= 0 || alto <= 0 || px.len() < ancho as usize * alto as usize {
            return;
        }
        let mut l = kit::lienzo::Lienzo::new(px, ancho, alto, ancho as usize);
        self.de(e).dibujar(&mut l, x, base, texto, tam, color);
    }
    fn tiene(&mut self, e: Estilo, c: char) -> bool {
        self.de(e).tiene(c)
    }
}

pub struct CompositorWayland {
    /// El canal de DashOS o el socket de Linux (`transporte.rs`).
    t: Transporte,
    pub conectado: bool,
    siguiente_id: u32,
    compositor: u32,
    shm: u32,
    xdg: u32,
    asiento: u32,
    /// Los objetos del ratón, del teclado y de la pantalla (0 si no hay).
    raton: u32,
    teclado: u32,
    salida: u32,
    /// El tamaño de la pantalla de DashOS (su modo actual).
    pantalla: (u32, u32),
    ventanas: BTreeMap<Superficie, Ventana>,
    siguiente_sup: Superficie,
    eventos: Vec<(Superficie, Evento)>,
    puntero: Option<Superficie>,
    foco: Option<Superficie>,
    serie_boton: u32,
    sync_hecho: Option<u32>,
    /// Lo que la caja tiene que decir por la consola (ventanas en pantalla, errores del compositor).
    pub dichos: Vec<String>,
    /// S26: el objeto del tema del sistema (`dashos_tema_v1`, 0 si no hay) y lo que dice: las plumas de DashOS para
    /// quien pinta los controles (`tema.rs`).
    tema_obj: u32,
    /// La versión de `dashos_tema_v1` enlazada (la 6 tiene `marca`).
    tema_version: u32,
    pub tema: TemaCaja,
    /// `zxdg_decoration_manager_v1` (0 si el compositor no la tiene): la barra de DashOS.
    deco_mgr: u32,
    /// La letra de DashOS, cuando hace falta la primera vez.
    letras: Option<LetrasKit>,
    /// `wl_subcompositor` (0 si no hay: las ventanas de una pantalla propia van sueltas).
    subcompositor: u32,
    /// Las ventanas de cada pantalla propia, de atrás adelante.
    hijas: BTreeMap<Superficie, Vec<Superficie>>,
    /// Lo último que se supo del ratón (1x, en la superficie que lo tiene) y el arrastre de una ventana de pantalla.
    raton_en: (i32, i32),
    arrastre: Option<Arrastre>,
}

fn palabra(c: &[u8], i: usize) -> u32 {
    c.get(4 * i..4 * i + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

impl CompositorWayland {
    /// Sin compositor: no se abre ninguna ventana (`crear_ventana` da `None`) y el escritorio mide 640 x 480.
    pub fn sin_conectar() -> CompositorWayland {
        CompositorWayland {
            t: Transporte::nada(),
            conectado: false,
            siguiente_id: 2,
            compositor: 0,
            shm: 0,
            xdg: 0,
            asiento: 0,
            raton: 0,
            teclado: 0,
            salida: 0,
            pantalla: (640, 480),
            ventanas: BTreeMap::new(),
            siguiente_sup: 1,
            eventos: Vec::new(),
            puntero: None,
            foco: None,
            serie_boton: 0,
            sync_hecho: None,
            dichos: Vec::new(),
            tema_obj: 0,
            tema_version: 0,
            tema: TemaCaja::default(),
            deco_mgr: 0,
            letras: None,
            subcompositor: 0,
            hijas: BTreeMap::new(),
            raton_en: (0, 0),
            arrastre: None,
        }
    }

    /// Se conecta al compositor por la llave `llave` (el canal "ventanas" que da `inicio`), pide los globales y los
    /// ata. Si no puede, se queda sin conectar.
    pub fn conectar(llave: u32) -> CompositorWayland {
        let mut c = CompositorWayland::sin_conectar();
        let Some(t) = Transporte::conectar(llave) else { return c };
        c.t = t;
        c.conectado = true;
        // Los globales, con un sync.
        let registro = c.nuevo_id();
        let sync = c.nuevo_id();
        let mut e = Escritor::new();
        e.empezar(1, wl_display::req::GET_REGISTRY).uint(registro).fin();
        e.empezar(1, wl_display::req::SYNC).uint(sync).fin();
        c.mandar(&e);
        let mut globales: Vec<(u32, String, u32)> = Vec::new();
        let hasta = plataforma::ahora_us() + 5_000_000;
        loop {
            let Some((o, orden, cuerpo)) = c.siguiente(hasta) else {
                c.conectado = false;
                return c;
            };
            if o == registro && orden == wl_registry::ev::GLOBAL {
                let largo = palabra(&cuerpo, 1) as usize;
                let nombre = cuerpo.get(8..8 + largo.saturating_sub(1)).unwrap_or(&[]);
                let version = palabra(&cuerpo, 2 + largo.div_ceil(4));
                globales.push((palabra(&cuerpo, 0), String::from_utf8_lossy(nombre).into_owned(), version));
            } else if o == sync && orden == wl_callback::ev::DONE {
                break;
            } else {
                c.atender(o, orden, &cuerpo);
            }
        }
        let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| (g.0, g.2));
        let (Some(gc), Some(gs), Some(gx)) = (global("wl_compositor"), global("wl_shm"), global("xdg_wm_base")) else {
            c.dichos.push(String::from("el compositor no tiene wl_compositor, wl_shm o xdg_wm_base"));
            c.conectado = false;
            return c;
        };
        let mut e = Escritor::new();
        c.compositor = c.nuevo_id();
        e.empezar(registro, wl_registry::req::BIND).uint(gc.0).texto(Some("wl_compositor")).uint(gc.1.min(4)).uint(c.compositor).fin();
        c.shm = c.nuevo_id();
        e.empezar(registro, wl_registry::req::BIND).uint(gs.0).texto(Some("wl_shm")).uint(1).uint(c.shm).fin();
        c.xdg = c.nuevo_id();
        e.empezar(registro, wl_registry::req::BIND).uint(gx.0).texto(Some("xdg_wm_base")).uint(gx.1.min(2)).uint(c.xdg).fin();
        if let Some(ga) = global("wl_seat") {
            c.asiento = c.nuevo_id();
            e.empezar(registro, wl_registry::req::BIND).uint(ga.0).texto(Some("wl_seat")).uint(ga.1.min(5)).uint(c.asiento).fin();
            c.raton = c.nuevo_id();
            c.teclado = c.nuevo_id();
            e.empezar(c.asiento, wl_seat::req::GET_POINTER).uint(c.raton).fin();
            e.empezar(c.asiento, wl_seat::req::GET_KEYBOARD).uint(c.teclado).fin();
        }
        // El tamaño del escritorio es el de la primera pantalla (su modo actual): el de la pantalla de Workbench.
        if let Some(go) = globales.iter().find(|g| g.1 == "wl_output") {
            c.salida = c.nuevo_id();
            e.empezar(registro, wl_registry::req::BIND).uint(go.0).texto(Some("wl_output")).uint(go.2.min(2)).uint(c.salida).fin();
        }
        // S26: el tema de DashOS (color, modo y el aspecto de los programas de las cajas). Llega al enlazar y cada vez
        // que cambia.
        if let Some(gt) = global("dashos_tema_v1") {
            c.tema_obj = c.nuevo_id();
            e.empezar(registro, wl_registry::req::BIND).uint(gt.0).texto(Some("dashos_tema_v1")).uint(gt.1.min(7)).uint(c.tema_obj).fin();
            c.tema_version = gt.1.min(7);
        }
        // Las ventanas de las pantallas propias van dentro de ellas.
        if let Some(gs) = global("wl_subcompositor") {
            c.subcompositor = c.nuevo_id();
            e.empezar(registro, wl_registry::req::BIND).uint(gs.0).texto(Some("wl_subcompositor")).uint(1).uint(c.subcompositor).fin();
        }
        // La barra de DashOS para las ventanas "como DashOS" (como las de Linux que la piden).
        if let Some(gd) = global("zxdg_decoration_manager_v1") {
            c.deco_mgr = c.nuevo_id();
            e.empezar(registro, wl_registry::req::BIND).uint(gd.0).texto(Some("zxdg_decoration_manager_v1")).uint(1).uint(c.deco_mgr).fin();
        }
        c.mandar(&e);
        c.sincronizar(2_000_000);
        c
    }

    fn nuevo_id(&mut self) -> u32 {
        let id = self.siguiente_id;
        self.siguiente_id += 1;
        id
    }

    fn mandar(&mut self, e: &Escritor) {
        if !self.conectado {
            return;
        }
        if !self.t.mandar(e) {
            self.dichos.push(String::from("el compositor no admite lo que se le manda: se deja"));
            self.conectado = false;
        }
    }

    /// Lee un trozo del compositor, esperando como mucho `us`. `false` si no llegó nada.
    fn recoger(&mut self, us: u64) -> bool {
        if !self.conectado {
            return false;
        }
        match self.t.recoger(us) {
            Recogido::Nada => false,
            Recogido::Algo => true,
            Recogido::Cerrado(n) => {
                self.dichos.push(alloc::format!("el compositor ha cerrado la conexión ({})", n));
                self.conectado = false;
                false
            }
        }
    }

    /// El siguiente mensaje, esperando hasta el instante `hasta` (µs).
    fn siguiente(&mut self, hasta: u64) -> Option<(u32, u16, Vec<u8>)> {
        loop {
            if let Some(m) = self.t.siguiente() {
                return Some(m);
            }
            let ahora = plataforma::ahora_us();
            if ahora >= hasta || !self.conectado {
                return None;
            }
            self.recoger(hasta - ahora);
        }
    }

    /// Atiende lo que haya llegado, sin esperar.
    fn bombear(&mut self) {
        while self.recoger(1) {}
        while let Some((o, orden, cuerpo)) = self.t.siguiente() {
            self.atender(o, orden, &cuerpo);
        }
    }

    /// Un `wl_display.sync` y su respuesta, atendiendo lo demás mientras. `true` si el compositor contestó en `us`.
    pub fn sincronizar(&mut self, us: u64) -> bool {
        if !self.conectado {
            return false;
        }
        let id = self.nuevo_id();
        let mut e = Escritor::new();
        e.empezar(1, wl_display::req::SYNC).uint(id).fin();
        self.mandar(&e);
        let hasta = plataforma::ahora_us() + us;
        while let Some((o, orden, cuerpo)) = self.siguiente(hasta) {
            self.atender(o, orden, &cuerpo);
            if self.sync_hecho == Some(id) {
                return true;
            }
        }
        false
    }

    fn por_objeto(&self, f: impl Fn(&Ventana) -> u32, o: u32) -> Option<Superficie> {
        self.ventanas.iter().find(|(_, v)| f(v) == o).map(|(s, _)| *s)
    }

    /// Un evento del compositor.
    fn atender(&mut self, o: u32, orden: u16, c: &[u8]) {
        if o == 1 {
            if orden == wl_display::ev::ERROR {
                let largo = palabra(c, 2) as usize;
                let texto = c.get(12..12 + largo.saturating_sub(1)).unwrap_or(&[]);
                self.dichos.push(alloc::format!(
                    "error de protocolo del compositor (objeto {}, código {}): {}",
                    palabra(c, 0),
                    palabra(c, 1),
                    String::from_utf8_lossy(texto)
                ));
            }
            return;
        }
        if o == self.xdg && orden == xdg_wm_base::ev::PING {
            let mut e = Escritor::new();
            e.empezar(self.xdg, xdg_wm_base::req::PONG).uint(palabra(c, 0)).fin();
            self.mandar(&e);
            return;
        }
        if let Some(s) = self.por_objeto(|v| v.xdg, o) {
            if orden == xdg_surface::ev::CONFIGURE {
                self.configurar(s, palabra(c, 0));
            }
            return;
        }
        if let Some(s) = self.por_objeto(|v| v.top, o) {
            match orden {
                xdg_toplevel::ev::CLOSE => self.eventos.push((s, Evento::Cerrar)),
                // configure: ancho, alto, estados. Vale con el `xdg_surface.configure` que lo sigue.
                xdg_toplevel::ev::CONFIGURE => {
                    if let Some(v) = self.ventanas.get_mut(&s) {
                        v.pedido = (palabra(c, 0) as i32, palabra(c, 1) as i32);
                    }
                }
                _ => {}
            }
            return;
        }
        // zxdg_toplevel_decoration_v1.configure: el modo (se pidió el del servidor; si da otro, la ventana sigue igual).
        if o != 0 && self.por_objeto(|v| v.deco, o).is_some() {
            return;
        }
        // wl_buffer.release y wl_surface.enter/leave: nada que hacer.
        if self.por_objeto(|v| v.buf, o).is_some() || self.por_objeto(|v| v.sup, o).is_some() {
            return;
        }
        if o == self.tema_obj && o != 0 {
            // (De la versión 6, la de `marca`: los eventos que no son de la caja se ignoran en `tema_ev`.)
            self.tema_ev(orden, c);
        } else if o == self.raton && o != 0 {
            self.raton_ev(orden, c);
        } else if o == self.teclado && o != 0 {
            self.teclado_ev(orden, c);
        } else if o == self.salida && o != 0 {
            // wl_output.mode: banderas, ancho, alto, refresco. El actual es el escritorio.
            if orden == wl_output::ev::MODE && palabra(c, 0) & 1 != 0 {
                let (w, h) = (palabra(c, 1), palabra(c, 2));
                if (64..=16384).contains(&w) && (64..=16384).contains(&h) {
                    self.pantalla = (w, h);
                    self.reescalar();
                }
            }
        } else if orden == wl_callback::ev::DONE && c.len() == 4 && o >= 2 {
            // Un sync (lo demás de la caja no pide `wl_callback`).
            self.sync_hecho = Some(o);
        }
    }

    /// S26: un evento del tema de DashOS. Los esquemas de un color que no está en la tabla del kit (la paleta del
    /// fondo) llegan hechos antes de su `tema`: así no se calculan aquí (sin FPU, cientos de milisegundos).
    fn tema_ev(&mut self, orden: u16, c: &[u8]) {
        match orden {
            dashos_tema_v1::ev::ESQUEMA => {
                kit::tema::recordar_evento_esquema(c);
                return;
            }
            dashos_tema_v1::ev::TEMA if c.len() >= 8 => {
                self.tema.color = Some(palabra(c, 0) | 0xff00_0000);
                self.tema.oscuro = palabra(c, 1) != 0;
            }
            dashos_tema_v1::ev::CAJAS => {
                let largo = palabra(c, 0) as usize;
                match c.get(4..4 + largo.saturating_sub(1)).map(wayland::aspecto::Ajuste::entender) {
                    Some(Ok(a)) => {
                        self.tema.ajuste = a;
                        self.reescalar();
                    }
                    _ => {
                        self.dichos.push(String::from("tema de DashOS: un aspecto de las cajas que no se entiende; se queda el de antes"));
                        return;
                    }
                }
            }
            _ => return,
        }
        if let Some(d) = self.tema.rehacer() {
            self.dichos.push(d);
        }
    }

    fn raton_ev(&mut self, orden: u16, c: &[u8]) {
        match orden {
            // enter: serie, superficie, x, y (24.8).
            wl_pointer::ev::ENTER => {
                self.puntero = self.por_objeto(|v| v.sup, palabra(c, 1));
                if let Some(s) = self.puntero {
                    let (x, y) = self.al_programa(s, palabra(c, 2), palabra(c, 3));
                    self.raton_en = (x, y);
                    self.eventos.push((s, Evento::Raton { x, y }));
                }
            }
            wl_pointer::ev::LEAVE => self.puntero = None,
            // motion: tiempo, x, y.
            wl_pointer::ev::MOTION => {
                if let Some(s) = self.puntero {
                    let (x, y) = self.al_programa(s, palabra(c, 1), palabra(c, 2));
                    self.raton_en = (x, y);
                    if self.arrastre.is_some_and(|a| a.s == s) {
                        self.seguir_arrastre(x, y);
                    } else {
                        self.eventos.push((s, Evento::Raton { x, y }));
                    }
                }
            }
            // button: serie, tiempo, botón (evdev), estado.
            wl_pointer::ev::BUTTON => {
                self.serie_boton = palabra(c, 0);
                let pulsado = palabra(c, 3) == 1;
                if !pulsado && let Some(a) = self.arrastre.take() {
                    self.acabar_arrastre(a);
                }
                if let Some(s) = self.puntero {
                    self.eventos.push((s, Evento::Boton { boton: palabra(c, 2), pulsado }));
                }
            }
            _ => {}
        }
    }

    fn teclado_ev(&mut self, orden: u16, c: &[u8]) {
        match orden {
            // keymap: formato, la llave (aparte) y su tamaño. La caja no lo usa: la llave se suelta.
            wl_keyboard::ev::KEYMAP => {
                if let Some(l) = self.t.tomar_llave() {
                    Transporte::cerrar(l);
                }
            }
            // enter: serie, superficie, teclas.
            wl_keyboard::ev::ENTER => {
                self.foco = self.por_objeto(|v| v.sup, palabra(c, 1));
                if let Some(s) = self.foco {
                    self.eventos.push((s, Evento::Foco(true)));
                }
            }
            wl_keyboard::ev::LEAVE => {
                if let Some(s) = self.foco.take() {
                    self.eventos.push((s, Evento::Foco(false)));
                }
            }
            // key: serie, tiempo, tecla (evdev), estado.
            wl_keyboard::ev::KEY => {
                if let Some(s) = self.foco {
                    let ev = Evento::Tecla { codigo: palabra(c, 2), pulsada: palabra(c, 3) == 1, repeticion: false, caracter: None };
                    self.eventos.push((s, ev));
                }
            }
            _ => {}
        }
    }

    fn configurar(&mut self, s: Superficie, serie: u32) {
        let Some(v) = self.ventanas.get_mut(&s) else { return };
        let primera = !v.configurada;
        v.configurada = true;
        let pedido = core::mem::take(&mut v.pedido);
        let (sup, xdg, buf, an, al, f) = (v.sup, v.xdg, v.buf, v.ancho, v.alto, v.f);
        // Otro tamaño desde el compositor (el borde de DashOS, maximizar): al programa, en sus coordenadas. Intuition lo
        // ajusta a sus límites; si no cambia, no pasa nada. Lo absurdo (negativo, enorme) no llega.
        if pedido.0 > 0 && pedido.1 > 0 && pedido.0 <= MAX_LADO as i32 * MAX_ESCALA as i32 && pedido.1 <= MAX_LADO as i32 * MAX_ESCALA as i32 {
            let (na, nl) = ((pedido.0 as u32 / f.max(1)).max(1), (pedido.1 as u32 / f.max(1)).max(1));
            if (na, nl) != (an, al) {
                self.eventos.push((s, Evento::Tamano { ancho: na, alto: nl }));
            }
        }
        let titulo = v.titulo.clone();
        let mut e = Escritor::new();
        e.empezar(xdg, xdg_surface::req::ACK_CONFIGURE).uint(serie).fin();
        if primera && buf != 0 {
            e.empezar(sup, wl_surface::req::ATTACH).uint(buf).int(0).int(0).fin();
            e.empezar(sup, wl_surface::req::DAMAGE).int(0).int(0).int((an * f) as i32).int((al * f) as i32).fin();
            e.empezar(sup, wl_surface::req::COMMIT).fin();
        }
        self.mandar(&e);
        if primera {
            // El tamaño es el del programa; con escala, después la que lleva.
            let a = if f > 1 { alloc::format!(", a {}x", f) } else { String::new() };
            let b = if self.ventanas.get(&s).is_some_and(|v| v.deco != 0) { ", con la barra de DashOS" } else { "" };
            self.dichos.push(alloc::format!("ventana Amiga en pantalla ({}x{}{}{}): {}", an, al, a, b, titulo));
        }
    }

    /// Crea el búfer de `v` (memoria, su pool y el `wl_buffer`) en `e`.
    fn crear_bufer(&mut self, s: Superficie, ancho: u32, alto: u32, e: &mut Escritor) -> Option<(u32, u32, u32, usize)> {
        let tam = (ancho as usize * alto as usize * 4).next_multiple_of(4096);
        let va = VA_BUFERES + (s as usize % MAX_SUPERFICIES as usize) * FRANJA;
        let (m, copia) = self.t.memoria(tam, va)?;
        let (pool, buf) = (self.nuevo_id(), self.nuevo_id());
        e.empezar(self.shm, wl_shm::req::CREATE_POOL).uint(pool).llave(copia as u32).int(tam as i32).fin();
        e.empezar(pool, wl_shm_pool::req::CREATE_BUFFER)
            .uint(buf)
            .int(0)
            .int(ancho as i32)
            .int(alto as i32)
            .int(ancho as i32 * 4)
            .uint(XRGB8888)
            .fin();
        Some((m, pool, buf, va))
    }

    fn soltar_bufer(&mut self, pool: u32, buf: u32, memoria: u32, va: usize, e: &mut Escritor) {
        e.empezar(buf, wl_buffer::req::DESTROY).fin();
        e.empezar(pool, wl_shm_pool::req::DESTROY).fin();
        self.t.soltar_memoria(memoria, va);
    }

    /// Dice al compositor dónde va la marca de la caja en la barra de intuition de la ventana `s` (sin la barra de
    /// DashOS): a la izquierda de sus gadgets de la derecha, ya con su factor (`dashos_tema_v1.marca`, versión 6).
    fn decir_hueco_marca(&self, s: Superficie, e: &mut Escritor) {
        let Some(v) = self.ventanas.get(&s) else { return };
        let Some((derecha, alto)) = v.hueco_marca else { return };
        if self.tema_obj == 0 || self.tema_version < 6 {
            return;
        }
        let f = v.f.max(1) as i32;
        e.empezar(self.tema_obj, dashos_tema_v1::req::MARCA).uint(v.sup).int(derecha.saturating_mul(f)).int(alto.saturating_mul(f)).fin();
    }

    /// El factor que pide el ajuste para un programa (su excepción, la de la caja o el automático de la pantalla).
    fn factor_pedido(&self, programa: &str) -> u32 {
        self.tema.ajuste.escala_para(Caja::Amiga, programa).factor(self.pantalla.0, self.pantalla.1)
    }

    /// El factor con que se ve una ventana de `ancho` x `alto` de `programa`: el pedido, si la ventana ampliada cabe.
    fn factor_de(&self, programa: &str, ancho: u32, alto: u32) -> u32 {
        escala::ajustar(self.factor_pedido(programa), MAX_ESCALA, ancho, alto, MAX_LADO, FRANJA as u64)
    }

    /// Un punto del ratón del compositor (24.8, en la superficie ampliada) en las coordenadas del programa.
    fn al_programa(&self, s: Superficie, x: u32, y: u32) -> (i32, i32) {
        let f = self.ventanas.get(&s).map_or(1, |v| v.f);
        (escala::al_programa(x as i32 >> 8, f), escala::al_programa(y as i32 >> 8, f))
    }

    /// Tras cambiar la pantalla o el ajuste: las ventanas que ahora van con otro factor se piden enteras (intuition
    /// las vuelve a mandar y `presentar` hace el búfer nuevo).
    fn reescalar(&mut self) {
        let mut otras: Vec<Superficie> = self
            .ventanas
            .iter()
            .filter(|(_, v)| v.padre.is_none() && self.factor_de(&v.programa, v.ancho, v.alto) != v.f)
            .map(|(s, _)| *s)
            .collect();
        // Las ventanas de una pantalla que cambia van con su factor nuevo.
        let hijas: Vec<Superficie> = otras.iter().filter_map(|p| self.hijas.get(p)).flatten().copied().collect();
        otras.extend(hijas);
        for s in otras {
            if !self.eventos.iter().any(|(x, e)| *x == s && *e == Evento::Exponer) {
                self.eventos.push((s, Evento::Exponer));
            }
        }
    }

    /// Una ventana de pantalla en marcha con el ratón en `(x, y)` (1x, en ella): movida (con el ratón donde se pulsó) o
    /// con el tamaño nuevo apuntado (se pide al soltar).
    fn seguir_arrastre(&mut self, x: i32, y: i32) {
        let Some(mut a) = self.arrastre else { return };
        if a.tamano {
            let an = (a.inicio.0 as i64 + (x - a.ancla.0) as i64).clamp(1, MAX_LADO as i64) as u32;
            let al = (a.inicio.1 as i64 + (y - a.ancla.1) as i64).clamp(1, MAX_LADO as i64) as u32;
            a.nuevo = (an, al);
            self.arrastre = Some(a);
            return;
        }
        let Some(v) = self.ventanas.get(&a.s) else { return };
        let (dx, dy) = (x - a.ancla.0, y - a.ancla.1);
        if (dx, dy) == (0, 0) {
            return;
        }
        let sitio = (v.sitio.0.saturating_add(dx), v.sitio.1.saturating_add(dy));
        self.colocar_hija(a.s, sitio);
    }

    /// Suelta el ratón: la ventana, en su sitio (intuition lo apunta) o con su tamaño nuevo.
    fn acabar_arrastre(&mut self, a: Arrastre) {
        if a.tamano {
            if a.nuevo != a.inicio {
                self.eventos.push((a.s, Evento::Tamano { ancho: a.nuevo.0, alto: a.nuevo.1 }));
            }
        } else if let Some(v) = self.ventanas.get(&a.s) {
            self.eventos.push((a.s, Evento::Posicion { x: v.sitio.0, y: v.sitio.1 }));
        }
    }

    /// Pone una ventana de pantalla en `sitio` (1x, en su pantalla).
    fn colocar_hija(&mut self, s: Superficie, sitio: (i32, i32)) {
        let Some(v) = self.ventanas.get_mut(&s) else { return };
        let Some(p) = v.padre else { return };
        v.sitio = (sitio.0.clamp(-(MAX_LADO as i32), MAX_LADO as i32), sitio.1.clamp(-(MAX_LADO as i32), MAX_LADO as i32));
        let (sub, f, (x, y)) = (v.sub, v.f.max(1) as i32, v.sitio);
        let Some(madre) = self.ventanas.get(&p).map(|m| m.sup) else { return };
        let mut e = Escritor::new();
        e.empezar(sub, wl_subsurface::req::SET_POSITION).int(x * f).int(y * f).fin();
        e.empezar(madre, wl_surface::req::COMMIT).fin();
        self.mandar(&e);
    }

    /// Una ventana de pantalla delante (o detrás) de sus hermanas.
    fn apilar_hija(&mut self, s: Superficie, frente: bool) {
        let Some((p, sub)) = self.ventanas.get(&s).and_then(|v| v.padre.map(|p| (p, v.sub))) else { return };
        let Some(pila) = self.hijas.get_mut(&p) else { return };
        let otra = if frente { pila.iter().rev().find(|&&x| x != s) } else { pila.iter().find(|&&x| x != s) }.copied();
        pila.retain(|&x| x != s);
        if frente {
            pila.push(s);
        } else {
            pila.insert(0, s);
        }
        let (Some(otra), Some(madre)) = (otra.and_then(|o| self.ventanas.get(&o)).map(|o| o.sup), self.ventanas.get(&p).map(|m| m.sup)) else {
            return;
        };
        let mut e = Escritor::new();
        let orden = if frente { wl_subsurface::req::PLACE_ABOVE } else { wl_subsurface::req::PLACE_BELOW };
        e.empezar(sub, orden).uint(otra).fin();
        e.empezar(madre, wl_surface::req::COMMIT).fin();
        self.mandar(&e);
    }

    /// Una ventana de primer nivel (una pantalla propia, con sus ventanas) delante o detrás de todas.
    fn apilar_ventana(&mut self, s: Superficie, frente: bool) {
        let Some(sup) = self.ventanas.get(&s).map(|v| v.sup) else { return };
        if self.tema_obj == 0 || self.tema_version < 7 {
            return;
        }
        let mut e = Escritor::new();
        e.empezar(self.tema_obj, dashos_tema_v1::req::APILAR).uint(sup).uint(!frente as u32).fin();
        self.mandar(&e);
    }

    /// El factor de una ventana de `ancho` x `alto`: el de su pantalla si va dentro de una, el suyo si no.
    fn factor_para(&self, v: &Ventana, ancho: u32, alto: u32) -> u32 {
        match v.padre.and_then(|p| self.ventanas.get(&p)) {
            Some(m) => m.f.max(1),
            None => self.factor_de(&v.programa, ancho, alto),
        }
    }

    /// Una ventana dentro de la pantalla propia `p` (su superficie `madre`, con su factor `f`): una `wl_subsurface` en su
    /// sitio, delante de sus hermanas, que se pinta sin esperar a la pantalla (`set_desync`).
    fn crear_hija(&mut self, s: Superficie, v: &NuevaVentana, p: Superficie, madre: u32, f: u32) -> Option<Superficie> {
        if v.ancho as usize * v.alto as usize * 4 * (f * f) as usize > FRANJA {
            return None;
        }
        let mut e = Escritor::new();
        let (memoria, pool, buf, va) = self.crear_bufer(s, v.ancho * f, v.alto * f, &mut e)?;
        let (sup, sub) = (self.nuevo_id(), self.nuevo_id());
        let (x, y) = (v.x.clamp(-(MAX_LADO as i32), MAX_LADO as i32), v.y.clamp(-(MAX_LADO as i32), MAX_LADO as i32));
        e.empezar(self.compositor, wl_compositor::req::CREATE_SURFACE).uint(sup).fin();
        e.empezar(self.subcompositor, wl_subcompositor::req::GET_SUBSURFACE).uint(sub).uint(sup).uint(madre).fin();
        e.empezar(sub, wl_subsurface::req::SET_POSITION).int(x * f as i32).int(y * f as i32).fin();
        e.empezar(sub, wl_subsurface::req::SET_DESYNC).fin();
        e.empezar(sup, wl_surface::req::ATTACH).uint(buf).int(0).int(0).fin();
        e.empezar(sup, wl_surface::req::DAMAGE).int(0).int(0).int((v.ancho * f) as i32).int((v.alto * f) as i32).fin();
        e.empezar(sup, wl_surface::req::COMMIT).fin();
        // El sitio y el orden de una subsuperficie valen con el siguiente commit de su madre.
        e.empezar(madre, wl_surface::req::COMMIT).fin();
        let titulo: String = v.titulo.chars().take(200).collect();
        self.ventanas.insert(
            s,
            Ventana {
                sup,
                xdg: 0,
                top: 0,
                pool,
                buf,
                memoria,
                va,
                ancho: v.ancho,
                alto: v.alto,
                f,
                configurada: true,
                titulo: titulo.clone(),
                programa: v.programa.chars().take(200).collect(),
                deco: 0,
                pedido: (0, 0),
                hueco_marca: None,
                padre: Some(p),
                sub,
                sitio: (x, y),
            },
        );
        self.hijas.entry(p).or_default().push(s);
        self.mandar(&e);
        self.dichos.push(alloc::format!("ventana Amiga en su pantalla ({}x{} en {}, {}): {}", v.ancho, v.alto, x, y, titulo));
        Some(s)
    }

    /// Las ventanas abiertas.
    pub fn abiertas(&self) -> usize {
        self.ventanas.len()
    }
}

impl Compositor for CompositorWayland {
    fn tamano_escritorio(&self) -> (u32, u32) {
        // La pantalla de Workbench lleva un lienzo de 32 bits de su tamaño en el montón de la caja (8 MB): como mucho
        // 1024 x 768. Con escala, la pantalla real entre el factor de la caja (1920 x 1080 a 2x: 960 x 540).
        let f = self.tema.ajuste.escala_de_caja(Caja::Amiga).factor(self.pantalla.0, self.pantalla.1);
        ((self.pantalla.0 / f).min(1024), (self.pantalla.1 / f).min(768))
    }

    fn crear_ventana(&mut self, v: &NuevaVentana) -> Option<Superficie> {
        if !self.conectado
            || v.ancho == 0
            || v.alto == 0
            || v.ancho > MAX_LADO
            || v.alto > MAX_LADO
            || v.ancho as usize * v.alto as usize * 4 > FRANJA
            || self.ventanas.len() >= MAX_SUPERFICIES as usize - 1
        {
            return None;
        }
        let madre = v.padre.filter(|_| self.subcompositor != 0).and_then(|p| self.ventanas.get(&p).map(|m| (p, m.sup, m.f)));
        // La superficie que no esté en uso (su franja de memoria la lleva su número).
        let mut s = self.siguiente_sup;
        while self.ventanas.contains_key(&(s % MAX_SUPERFICIES)) || s % MAX_SUPERFICIES == 0 {
            s += 1;
        }
        let s = s % MAX_SUPERFICIES;
        self.siguiente_sup = s + 1;
        if let Some((p, madre, f)) = madre {
            return self.crear_hija(s, v, p, madre, f.max(1));
        }
        let mut e = Escritor::new();
        let programa: String = v.programa.chars().take(200).collect();
        let f = self.factor_de(&programa, v.ancho, v.alto);
        let barra = v.barra_dashos && self.deco_mgr != 0;
        let (memoria, pool, buf, va) = self.crear_bufer(s, v.ancho * f, v.alto * f, &mut e)?;
        let (sup, xdg, top) = (self.nuevo_id(), self.nuevo_id(), self.nuevo_id());
        e.empezar(self.compositor, wl_compositor::req::CREATE_SURFACE).uint(sup).fin();
        e.empezar(self.xdg, xdg_wm_base::req::GET_XDG_SURFACE).uint(xdg).uint(sup).fin();
        e.empezar(xdg, xdg_surface::req::GET_TOPLEVEL).uint(top).fin();
        let titulo: String = v.titulo.chars().take(200).collect();
        e.empezar(top, xdg_toplevel::req::SET_TITLE).texto(Some(&titulo)).fin();
        e.empezar(top, xdg_toplevel::req::SET_APP_ID).texto(Some("caja-amiga")).fin();
        // La barra de DashOS: antes del primer commit (en la versión 1, una ventana con búfer ya no puede pedirla).
        let deco = if barra {
            let d = self.nuevo_id();
            e.empezar(self.deco_mgr, zxdg_decoration_manager_v1::req::GET_TOPLEVEL_DECORATION).uint(d).uint(top).fin();
            e.empezar(d, zxdg_toplevel_decoration_v1::req::SET_MODE).uint(MODO_SERVIDOR).fin();
            d
        } else {
            0
        };
        e.empezar(sup, wl_surface::req::COMMIT).fin();
        self.ventanas.insert(
            s,
            Ventana {
                sup,
                xdg,
                top,
                pool,
                buf,
                memoria,
                va,
                ancho: v.ancho,
                alto: v.alto,
                f,
                configurada: false,
                titulo,
                programa,
                deco,
                pedido: (0, 0),
                hueco_marca: if barra { None } else { v.hueco_marca },
                padre: None,
                sub: 0,
                sitio: (v.x, v.y),
            },
        );
        self.decir_hueco_marca(s, &mut e);
        self.mandar(&e);
        Some(s)
    }

    fn cerrar_ventana(&mut self, s: Superficie) {
        let Some(v) = self.ventanas.remove(&s) else { return };
        if self.puntero == Some(s) {
            self.puntero = None;
        }
        if self.foco == Some(s) {
            self.foco = None;
        }
        self.eventos.retain(|(x, _)| *x != s);
        let mut e = Escritor::new();
        if v.deco != 0 {
            e.empezar(v.deco, zxdg_toplevel_decoration_v1::req::DESTROY).fin();
        }
        if let Some(p) = v.padre {
            if let Some(pila) = self.hijas.get_mut(&p) {
                pila.retain(|&x| x != s);
            }
            if self.arrastre.is_some_and(|a| a.s == s) {
                self.arrastre = None;
            }
            e.empezar(v.sub, wl_subsurface::req::DESTROY).fin();
        } else {
            self.hijas.remove(&s);
            e.empezar(v.top, xdg_toplevel::req::DESTROY).fin();
            e.empezar(v.xdg, xdg_surface::req::DESTROY).fin();
        }
        e.empezar(v.sup, wl_surface::req::DESTROY).fin();
        if v.buf != 0 {
            self.soltar_bufer(v.pool, v.buf, v.memoria, v.va, &mut e);
        }
        self.mandar(&e);
    }

    fn presentar(&mut self, s: Superficie, pix: &[u32], ancho: u32, alto: u32, dano: Rect, textos: &[TextoNitido]) {
        if pix.len() < ancho as usize * alto as usize {
            return;
        }
        let Some(v) = self.ventanas.get(&s) else { return };
        let mut e = Escritor::new();
        let mut dano = dano;
        let f = self.factor_para(v, ancho, alto);
        if v.ancho != ancho || v.alto != alto || v.f != f {
            // Otro tamaño u otra escala: un búfer nuevo, y entero.
            if ancho == 0 || alto == 0 || ancho > MAX_LADO || alto > MAX_LADO || ancho as usize * alto as usize * 4 > FRANJA {
                return;
            }
            let (pool, buf, memoria, va) = (v.pool, v.buf, v.memoria, v.va);
            if buf != 0 {
                self.soltar_bufer(pool, buf, memoria, va, &mut e);
            }
            let Some((m, p, b, va)) = self.crear_bufer(s, ancho * f, alto * f, &mut e) else {
                // Sin memoria para el búfer nuevo: la ventana se queda sin búfer (`buf` 0) hasta el siguiente intento.
                self.mandar(&e);
                let v = self.ventanas.get_mut(&s).unwrap();
                (v.buf, v.pool, v.memoria, v.ancho, v.alto) = (0, 0, 0, 0, 0);
                return;
            };
            let v = self.ventanas.get_mut(&s).unwrap();
            let otro_f = v.f != f;
            (v.memoria, v.pool, v.buf, v.va, v.ancho, v.alto, v.f) = (m, p, b, va, ancho, alto, f);
            dano = Rect::de_tamano(ancho, alto);
            let (padre, sitio) = (v.padre, v.sitio);
            if otro_f {
                self.decir_hueco_marca(s, &mut e);
                // Una pantalla con otro factor: sus ventanas, con él (en su sitio, ampliado como ella).
                let hijas: Vec<Superficie> = self.hijas.get(&s).cloned().unwrap_or_default();
                for h in hijas {
                    if !self.eventos.iter().any(|(x, ev)| *x == h && *ev == Evento::Exponer) {
                        self.eventos.push((h, Evento::Exponer));
                    }
                }
                if padre.is_some() {
                    self.mandar(&e);
                    e = Escritor::new();
                    self.colocar_hija(s, sitio);
                }
            }
        }
        // Ampliada, los textos nítidos que tocan lo dañado se escriben enteros.
        if f > 1 && !textos.is_empty() {
            dano = nitido::dano_con_textos(dano, textos, pix, ancho, alto);
        }
        let v = self.ventanas.get(&s).unwrap();
        let (anf, alf) = (ancho as usize * f as usize, alto as usize * f as usize);
        // SEGURIDAD: el búfer de la ventana está proyectado en `v.va` con al menos ancho·f × alto·f × 4 bytes (lo creó
        // `crear_bufer` con este tamaño) y solo se quita al soltarlo, tras lo cual `v` ya no lo apunta.
        let dst = unsafe { slice::from_raw_parts_mut(v.va as *mut u32, anf * alf) };
        // XRGB8888 de Wayland es un u32 en little-endian: en la caja de ARM (big-endian), con los bytes dados la vuelta; en
        // la de x86 (X7), tal cual.
        let d = escala::ampliar(pix, ancho, alto, f, dano, dst, u32::to_le);
        if f > 1 && !textos.is_empty() {
            let letras = self.letras.get_or_insert_with(LetrasKit::nuevas);
            nitido::componer(letras, textos, pix, ancho, alto, f, dano, dst, u32::to_le);
        }
        if d.vacio() {
            self.mandar(&e);
            return;
        }
        if v.configurada && v.buf != 0 {
            e.empezar(v.sup, wl_surface::req::ATTACH).uint(v.buf).int(0).int(0).fin();
            e.empezar(v.sup, wl_surface::req::DAMAGE).int(d.x0).int(d.y0).int(d.ancho() as i32).int(d.alto() as i32).fin();
            e.empezar(v.sup, wl_surface::req::COMMIT).fin();
        }
        self.mandar(&e);
    }

    fn poner_titulo(&mut self, s: Superficie, titulo: &str) {
        let Some(v) = self.ventanas.get_mut(&s) else { return };
        v.titulo = titulo.chars().take(200).collect();
        let (top, t) = (v.top, v.titulo.clone());
        if top == 0 {
            return;
        }
        let mut e = Escritor::new();
        e.empezar(top, xdg_toplevel::req::SET_TITLE).texto(Some(&t)).fin();
        self.mandar(&e);
    }

    fn mover(&mut self, s: Superficie, x: i32, y: i32) {
        // Solo las de una pantalla (las de primer nivel las coloca el compositor).
        if self.ventanas.get(&s).is_some_and(|v| v.padre.is_some()) {
            self.colocar_hija(s, (x, y));
            if let Some(v) = self.ventanas.get(&s) {
                self.eventos.push((s, Evento::Posicion { x: v.sitio.0, y: v.sitio.1 }));
            }
        }
    }

    fn al_frente(&mut self, s: Superficie) {
        if self.ventanas.get(&s).is_some_and(|v| v.padre.is_some()) {
            self.apilar_hija(s, true);
        } else {
            self.apilar_ventana(s, true);
        }
    }

    fn al_fondo(&mut self, s: Superficie) {
        if self.ventanas.get(&s).is_some_and(|v| v.padre.is_some()) {
            self.apilar_hija(s, false);
        } else {
            self.apilar_ventana(s, false);
        }
    }

    fn cambiar_profundidad(&mut self, s: Superficie) {
        // Una de pantalla: detrás si es la de delante, delante si no. (De las de primer nivel no se sabe el orden.)
        let Some(p) = self.ventanas.get(&s).and_then(|v| v.padre) else { return };
        let delante = self.hijas.get(&p).and_then(|h| h.last()) == Some(&s);
        self.apilar_hija(s, !delante);
    }

    fn arrastrar(&mut self, s: Superficie) {
        let Some(v) = self.ventanas.get(&s) else { return };
        if v.padre.is_some() {
            self.arrastre = Some(Arrastre { s, tamano: false, ancla: self.raton_en, inicio: (v.ancho, v.alto), nuevo: (v.ancho, v.alto) });
            return;
        }
        if self.asiento == 0 {
            return;
        }
        let mut e = Escritor::new();
        e.empezar(v.top, xdg_toplevel::req::MOVE).uint(self.asiento).uint(self.serie_boton).fin();
        self.mandar(&e);
    }

    fn redimensionar(&mut self, s: Superficie) {
        let Some(v) = self.ventanas.get(&s) else { return };
        if v.padre.is_some() {
            self.arrastre = Some(Arrastre { s, tamano: true, ancla: self.raton_en, inicio: (v.ancho, v.alto), nuevo: (v.ancho, v.alto) });
            return;
        }
        if self.asiento == 0 {
            return;
        }
        let mut e = Escritor::new();
        e.empezar(v.top, xdg_toplevel::req::RESIZE).uint(self.asiento).uint(self.serie_boton).uint(BORDE_ABAJO_DERECHA).fin();
        self.mandar(&e);
    }

    fn sacar_eventos(&mut self) -> Vec<(Superficie, Evento)> {
        self.bombear();
        core::mem::take(&mut self.eventos)
    }
    /// S26: los colores de DashOS para un programa Amiga que va "como DashOS" (tema.rs).
    fn tema_amiga(&self, programa: &str) -> Option<Tokens> {
        self.tema.tokens_para(programa)
    }
    fn version_tema(&self) -> u32 {
        self.tema.version
    }
    fn pone_barra(&self) -> bool {
        self.deco_mgr != 0
    }
}
