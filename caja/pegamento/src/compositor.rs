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

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::slice;

use caja_intuition::compositor::{Compositor, Evento, NuevaVentana, Superficie, Tokens};
use caja_intuition::escala;
use caja_intuition::lienzo::Rect;
use wayland::canal::{self, Juntador, TROZO_CONECTAR, TROZO_DATOS};
use crate::tema::TemaCaja;
use wayland::tablas::{
    dashos_tema_v1, wl_buffer, wl_callback, wl_compositor, wl_display, wl_keyboard, wl_output, wl_pointer, wl_registry, wl_seat,
    wl_shm, wl_shm_pool, wl_surface, xdg_surface, xdg_toplevel, xdg_wm_base,
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
}

pub struct CompositorWayland {
    llave: u32,
    respuesta: u32,
    juntador: Juntador,
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
    pub tema: TemaCaja,
}

fn palabra(c: &[u8], i: usize) -> u32 {
    c.get(4 * i..4 * i + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

impl CompositorWayland {
    /// Sin compositor: no se abre ninguna ventana (`crear_ventana` da `None`) y el escritorio mide 640 x 480.
    pub fn sin_conectar() -> CompositorWayland {
        CompositorWayland {
            llave: 0,
            respuesta: 0,
            juntador: Juntador::new(),
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
            tema: TemaCaja::default(),
        }
    }

    /// Se conecta al compositor por la llave `llave` (el canal "ventanas" que da `inicio`), pide los globales y los
    /// ata. Si no puede, se queda sin conectar.
    pub fn conectar(llave: u32) -> CompositorWayland {
        let mut c = CompositorWayland::sin_conectar();
        let (mio, suyo) = dashos::canal_crear();
        let mut r = -1;
        for _ in 0..5_000 {
            r = dashos::canal_escribir(llave, &[TROZO_CONECTAR], &[suyo]);
            if r == 0 || r == dashos::E_CERRADO {
                break;
            }
            dashos::dormir_us(1_000);
        }
        if r != 0 {
            dashos::cerrar(mio);
            dashos::cerrar(suyo);
            return c;
        }
        c.llave = llave;
        c.respuesta = mio;
        c.conectado = true;
        // Los globales, con un sync.
        let registro = c.nuevo_id();
        let sync = c.nuevo_id();
        let mut e = Escritor::new();
        e.empezar(1, wl_display::req::GET_REGISTRY).uint(registro).fin();
        e.empezar(1, wl_display::req::SYNC).uint(sync).fin();
        c.mandar(&e);
        let mut globales: Vec<(u32, String, u32)> = Vec::new();
        let hasta = dashos::ahora_us() + 5_000_000;
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
            e.empezar(registro, wl_registry::req::BIND).uint(gt.0).texto(Some("dashos_tema_v1")).uint(gt.1.min(3)).uint(c.tema_obj).fin();
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
        let Ok(trozos) = canal::trocear(e) else { return };
        for t in trozos {
            let mut hecho = false;
            for _ in 0..2000 {
                let r = dashos::canal_escribir(self.llave, &t.bytes, &t.llaves);
                if r == 0 {
                    hecho = true;
                    break;
                }
                if r == dashos::E_CERRADO {
                    break;
                }
                // Con su cola llena, el compositor puede estar esperando a que leamos: se recoge lo suyo.
                if !self.recoger(1) {
                    dashos::dormir_us(1_000);
                }
            }
            if !hecho {
                self.dichos.push(String::from("el compositor no admite lo que se le manda: se deja"));
                self.conectado = false;
                for l in &t.llaves {
                    dashos::cerrar(*l);
                }
                return;
            }
        }
    }

    /// Lee un trozo del compositor, esperando como mucho `us`. `false` si no llegó nada.
    fn recoger(&mut self, us: u64) -> bool {
        if !self.conectado {
            return false;
        }
        let mut buf = [0u8; canal::MAX_CANAL];
        let mut ll = [0u32; canal::MAX_LLAVES];
        let (n, k, _) = dashos::canal_leer_plazo(self.respuesta, &mut buf, &mut ll, us.max(1));
        if n == dashos::E_PLAZO {
            return false;
        }
        if n <= 0 {
            self.dichos.push(alloc::format!("el compositor ha cerrado la conexión ({})", n));
            self.conectado = false;
            return false;
        }
        if buf[0] == TROZO_DATOS && self.juntador.meter(&buf[1..n as usize], &ll[..k]).is_ok() {
            return true;
        }
        for &l in &ll[..k] {
            dashos::cerrar(l);
        }
        true
    }

    /// El siguiente mensaje, esperando hasta el instante `hasta` (µs).
    fn siguiente(&mut self, hasta: u64) -> Option<(u32, u16, Vec<u8>)> {
        loop {
            if let Ok(Some(m)) = self.juntador.siguiente() {
                return Some((m.objeto, m.orden, m.cuerpo));
            }
            let ahora = dashos::ahora_us();
            if ahora >= hasta || !self.conectado {
                return None;
            }
            self.recoger(hasta - ahora);
        }
    }

    /// Atiende lo que haya llegado, sin esperar.
    fn bombear(&mut self) {
        while self.recoger(1) {}
        while let Ok(Some(m)) = self.juntador.siguiente() {
            self.atender(m.objeto, m.orden, &m.cuerpo);
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
        let hasta = dashos::ahora_us() + us;
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
            if orden == xdg_toplevel::ev::CLOSE {
                self.eventos.push((s, Evento::Cerrar));
            }
            return;
        }
        // wl_buffer.release y wl_surface.enter/leave: nada que hacer.
        if self.por_objeto(|v| v.buf, o).is_some() || self.por_objeto(|v| v.sup, o).is_some() {
            return;
        }
        if o == self.tema_obj && o != 0 {
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
                    self.eventos.push((s, Evento::Raton { x, y }));
                }
            }
            wl_pointer::ev::LEAVE => self.puntero = None,
            // motion: tiempo, x, y.
            wl_pointer::ev::MOTION => {
                if let Some(s) = self.puntero {
                    let (x, y) = self.al_programa(s, palabra(c, 1), palabra(c, 2));
                    self.eventos.push((s, Evento::Raton { x, y }));
                }
            }
            // button: serie, tiempo, botón (evdev), estado.
            wl_pointer::ev::BUTTON => {
                self.serie_boton = palabra(c, 0);
                if let Some(s) = self.puntero {
                    self.eventos.push((s, Evento::Boton { boton: palabra(c, 2), pulsado: palabra(c, 3) == 1 }));
                }
            }
            _ => {}
        }
    }

    fn teclado_ev(&mut self, orden: u16, c: &[u8]) {
        match orden {
            // keymap: formato, la llave (aparte) y su tamaño. La caja no lo usa: la llave se suelta.
            wl_keyboard::ev::KEYMAP => {
                if let Some(l) = self.juntador.tomar_llave() {
                    dashos::cerrar(l);
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
        let (sup, xdg, buf, an, al, f) = (v.sup, v.xdg, v.buf, v.ancho, v.alto, v.f);
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
            self.dichos.push(alloc::format!("ventana Amiga en pantalla ({}x{}{}): {}", an, al, a, titulo));
        }
    }

    /// Crea el búfer de `v` (memoria, su pool y el `wl_buffer`) en `e`.
    fn crear_bufer(&mut self, s: Superficie, ancho: u32, alto: u32, e: &mut Escritor) -> Option<(u32, u32, u32, usize)> {
        let tam = (ancho as usize * alto as usize * 4).next_multiple_of(4096);
        let va = VA_BUFERES + (s as usize % MAX_SUPERFICIES as usize) * FRANJA;
        let m = dashos::memoria_crear(tam);
        if (m as i32) <= 0 {
            return None;
        }
        let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::TRANSFERIR, 0);
        if copia <= 0 || dashos::memoria_proyectar(m, va, true) != 0 {
            if copia > 0 {
                dashos::cerrar(copia as u32);
            }
            dashos::cerrar(m);
            return None;
        }
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
        dashos::memoria_quitar(va);
        dashos::cerrar(memoria);
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
        let otras: Vec<Superficie> =
            self.ventanas.iter().filter(|(_, v)| self.factor_de(&v.programa, v.ancho, v.alto) != v.f).map(|(s, _)| *s).collect();
        for s in otras {
            if !self.eventos.iter().any(|(x, e)| *x == s && *e == Evento::Exponer) {
                self.eventos.push((s, Evento::Exponer));
            }
        }
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
        // La superficie que no esté en uso (su franja de memoria la lleva su número).
        let mut s = self.siguiente_sup;
        while self.ventanas.contains_key(&(s % MAX_SUPERFICIES)) || s % MAX_SUPERFICIES == 0 {
            s += 1;
        }
        let s = s % MAX_SUPERFICIES;
        self.siguiente_sup = s + 1;
        let mut e = Escritor::new();
        let programa: String = v.programa.chars().take(200).collect();
        let f = self.factor_de(&programa, v.ancho, v.alto);
        let (memoria, pool, buf, va) = self.crear_bufer(s, v.ancho * f, v.alto * f, &mut e)?;
        let (sup, xdg, top) = (self.nuevo_id(), self.nuevo_id(), self.nuevo_id());
        e.empezar(self.compositor, wl_compositor::req::CREATE_SURFACE).uint(sup).fin();
        e.empezar(self.xdg, xdg_wm_base::req::GET_XDG_SURFACE).uint(xdg).uint(sup).fin();
        e.empezar(xdg, xdg_surface::req::GET_TOPLEVEL).uint(top).fin();
        let titulo: String = v.titulo.chars().take(200).collect();
        e.empezar(top, xdg_toplevel::req::SET_TITLE).texto(Some(&titulo)).fin();
        e.empezar(top, xdg_toplevel::req::SET_APP_ID).texto(Some("caja-amiga")).fin();
        e.empezar(sup, wl_surface::req::COMMIT).fin();
        self.ventanas.insert(
            s,
            Ventana { sup, xdg, top, pool, buf, memoria, va, ancho: v.ancho, alto: v.alto, f, configurada: false, titulo, programa },
        );
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
        e.empezar(v.top, xdg_toplevel::req::DESTROY).fin();
        e.empezar(v.xdg, xdg_surface::req::DESTROY).fin();
        e.empezar(v.sup, wl_surface::req::DESTROY).fin();
        if v.buf != 0 {
            self.soltar_bufer(v.pool, v.buf, v.memoria, v.va, &mut e);
        }
        self.mandar(&e);
    }

    fn presentar(&mut self, s: Superficie, pix: &[u32], ancho: u32, alto: u32, dano: Rect) {
        if pix.len() < ancho as usize * alto as usize {
            return;
        }
        let Some(v) = self.ventanas.get(&s) else { return };
        let mut e = Escritor::new();
        let mut dano = dano;
        let f = self.factor_de(&v.programa, ancho, alto);
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
            (v.memoria, v.pool, v.buf, v.va, v.ancho, v.alto, v.f) = (m, p, b, va, ancho, alto, f);
            dano = Rect::de_tamano(ancho, alto);
        }
        let v = self.ventanas.get(&s).unwrap();
        let (anf, alf) = (ancho as usize * f as usize, alto as usize * f as usize);
        // SEGURIDAD: el búfer de la ventana está proyectado en `v.va` con al menos ancho·f × alto·f × 4 bytes (lo creó
        // `crear_bufer` con este tamaño) y solo se quita al soltarlo, tras lo cual `v` ya no lo apunta.
        let dst = unsafe { slice::from_raw_parts_mut(v.va as *mut u32, anf * alf) };
        // XRGB8888 de Wayland es un u32 en little-endian: sus bytes, dados la vuelta en la caja.
        let d = escala::ampliar(pix, ancho, alto, f, dano, dst, u32::swap_bytes);
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
        let mut e = Escritor::new();
        e.empezar(top, xdg_toplevel::req::SET_TITLE).texto(Some(&t)).fin();
        self.mandar(&e);
    }

    fn arrastrar(&mut self, s: Superficie) {
        let Some(v) = self.ventanas.get(&s) else { return };
        if self.asiento == 0 {
            return;
        }
        let mut e = Escritor::new();
        e.empezar(v.top, xdg_toplevel::req::MOVE).uint(self.asiento).uint(self.serie_boton).fin();
        self.mandar(&e);
    }

    fn redimensionar(&mut self, s: Superficie) {
        let Some(v) = self.ventanas.get(&s) else { return };
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
}
