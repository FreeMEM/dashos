//! La herramienta de fondos (rama `fondos`; lo pidió el usuario: "una herramienta espectacular para cambiar de fondo").
//! Va dentro del propio `fondo`: así la vista previa se pinta sin más viajes, con la misma transición que al aplicar.
//!
//! Es una capa de layer-shell encima de todo (overlay), pegada abajo y a lo ancho, con la mitad de alto: arriba se ve
//! el escritorio de verdad, donde sale la vista previa al pasar el ratón por una miniatura (tras un momento quieto) o
//! al elegirla con las flechas, con la transición que esté puesta (el círculo crece desde la miniatura). Se sabe dónde
//! está cada miniatura en la pantalla porque la capa está donde se pidió.
//!
//! - Arriba: las pantallas ("Todas" o una), claro u oscuro, buscar (se teclea sin más) y cerrar.
//! - Debajo: las carpetas (Home:Fondos, SYS:Fondos y los animados), el ajuste (llenar, encajar, centrar, mosaico,
//!   estirar), la transición, el pase automático (cada N minutos) y el efecto encima (luciérnagas, nieve, aurora).
//! - La rejilla de miniaturas: se hacen en otro hilo, una a una y solo las que se ven (y alguna más), y se guardan.
//! - A la derecha: la elegida en grande, su nombre y lo que mide, y el color de acento que saldría de ella (la semilla y
//!   los colores del tema en el modo elegido) antes de aplicar. Aplicar (Intro o doble clic) y Cancelar (Esc), que
//!   vuelve al fondo de antes.
//!
//! Lo elegido se guarda en `Escritorio/fondo` (`ENV:`, y en `ENVARC:` por `sesiones`). Arrastrar una imagen desde el
//! explorador no se puede todavía: el compositor no tiene arrastrar y soltar entre programas (apuntado en la bitácora).

use crate::fondo_lector as lector;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use dashos::println;
use kit::fondos::ajustes::Config;
use kit::fondos::{animado, Ajuste, Marco};
use kit::imagen::Imagen;
use kit::lienzo::{Color, Lienzo, Rect};
use kit::tema::{con_opacidad, Tema};
use kit::texto::Fuente;
use wayland::tablas::{wl_compositor, wl_pointer, wl_shm_pool, wl_surface, zwlr_layer_shell_v1, zwlr_layer_surface_v1};
use wayland::Escritor;

// Sus objetos en la conexión del fondo (los de `ventana_cliente` van del 2 al 14, el panel del 60 al 65, las franjas
// desde el 100 hacia arriba, los avisos de las franjas en su tramo).
const SUP: u32 = 0x0020_0001;
const CAPA: u32 = 0x0020_0002;
const POOL: u32 = 0x0020_0003;
const BUFER: u32 = 0x0020_0004;
const PRIMER_AVISO: u32 = 0x0020_1000;
const VA: usize = 0x5a_0000_0000;
/// Las carpetas.
pub const CARPETAS: [&str; 3] = ["Home:Fondos", "SYS:Fondos", "Animados"];
/// Lo más que se enseña de una carpeta.
pub const MAX_ENTRADAS: usize = 2000;
/// Lo más que se guarda de miniaturas hechas (las demás se vuelven a hacer al verse).
const MAX_MINIATURAS: usize = 96;
/// Cuánto hay que estar quieto encima de una miniatura para ver su vista previa.
const QUIETO_US: u64 = 450_000;
const PASES: [u32; 6] = [0, 1, 5, 15, 30, 60];
const EFECTOS: [&str; 4] = ["ninguno", "luciernagas", "nieve", "aurora"];
const TRANSICIONES: [&str; 6] = ["circulo", "fundido", "barrido", "disolver", "azar", "ninguna"];

// --- El hilo de las miniaturas ------------------------------------------------------------------------------------------

pub enum Trabajo {
    /// La miniatura de `camino` a `w` × `h`.
    Mini { id: u32, camino: String, w: usize, h: usize },
    /// Los colores del tema que saldrían de `semilla` (lo que cuesta: de 180 a 340 ms en QEMU).
    Acento { semilla: u32, oscuro: bool },
}

pub enum Hecho {
    Mini { id: u32, img: Option<Imagen>, semilla: u32, medida: (u32, u32), porque: String },
    Acento { semilla: u32, oscuro: bool, colores: [Color; 6] },
}

pub static TRABAJOS: dashos::hilos::Cola<Trabajo, 64> = dashos::hilos::Cola::nueva();
pub static HECHOS: dashos::hilos::Cola<Hecho, 64> = dashos::hilos::Cola::nueva();
static TIMBRE: AtomicU32 = AtomicU32::new(0);
/// La puerta de la conexión del fondo (para despertar al bucle principal al acabar algo).
pub static PUERTA: AtomicU32 = AtomicU32::new(0);
/// Lo más que se lee de un fichero para una miniatura y los píxeles que se dejan (una foto 4K).
const MAX_FICHERO: u64 = 32 << 20;
const MAX_PX: u64 = 9 << 20;

fn pedir_trabajo(t: Trabajo) -> bool {
    let ok = TRABAJOS.con(|c| c.meter(t).is_ok());
    TIMBRE.fetch_add(1, Ordering::Release);
    dashos::hilos::despertar(&TIMBRE, 1);
    ok
}

pub fn hilo_miniaturas(_: u64) -> u64 {
    loop {
        let visto = TIMBRE.load(Ordering::Acquire);
        let Some(t) = TRABAJOS.con(|c| c.sacar()) else {
            dashos::hilos::esperar(&TIMBRE, visto, 5_000_000);
            continue;
        };
        let hecho = match t {
            Trabajo::Mini { id, camino, w, h } => miniatura(id, &camino, w, h),
            Trabajo::Acento { semilla, oscuro } => {
                let t = Tema::del_sistema(semilla, oscuro);
                Hecho::Acento { semilla, oscuro, colores: [semilla | 0xff00_0000, t.primario, t.contenedor_primario, t.secundario, t.contenedor_terciario, t.superficie] }
            }
        };
        // Si la cola de vuelta está llena, se espera un poco (el bucle principal la vacía en cada vuelta).
        let mut h = Some(hecho);
        for _ in 0..200 {
            match HECHOS.con(|c| c.meter(h.take().unwrap())) {
                Ok(()) => break,
                Err(x) => {
                    h = Some(x);
                    dashos::dormir_us(10_000);
                }
            }
        }
        let p = PUERTA.load(Ordering::Acquire);
        if p != 0 {
            dashos::canal_escribir(p, &[2], &[]);
        }
    }
}

fn miniatura(id: u32, camino: &str, w: usize, h: usize) -> Hecho {
    let mal = |porque: String| Hecho::Mini { id, img: None, semilla: 0, medida: (0, 0), porque };
    if w == 0 || h == 0 || w > 1024 || h > 1024 {
        return mal(String::from("medida mala"));
    }
    let mut px = alloc::vec![0u32; w * h];
    let (semilla, medida) = if let Some(nombre) = camino.strip_prefix("animado:") {
        let Some(mut a) = animado::crear(nombre, w, h) else { return mal(String::from("no existe")) };
        let mut base = alloc::vec![0u32; w * h];
        a.pintar_base(&mut Marco::entero(w, h, &mut base).unwrap());
        px.copy_from_slice(&base);
        // Un cuadro a los 3 s, para que se vea lo que se mueve.
        a.paso(3000, &Marco::entero(w, h, &mut base.clone()).unwrap(), &mut Marco::entero(w, h, &mut px).unwrap());
        (kit::semilla::semilla(&px, w, h).unwrap_or(0), (0, 0))
    } else {
        let bytes = match lector::leer(lector::VA_MINIATURAS, camino, MAX_FICHERO) {
            Ok(b) => b,
            Err(e) => return mal(e),
        };
        let img = match kit::png::leer(&bytes, MAX_PX) {
            Ok(i) => i,
            Err(e) => return mal(String::from(e.texto())),
        };
        drop(bytes);
        kit::fondos::pintar_marco(&img, Ajuste::Llenar, 0xff20_4060, &mut Marco::entero(w, h, &mut px).unwrap());
        (kit::semilla::semilla(&img.px, img.ancho, img.alto).unwrap_or(0), (img.ancho as u32, img.alto as u32))
    };
    Hecho::Mini { id, img: Some(Imagen { ancho: w, alto: h, px }), semilla, medida, porque: String::new() }
}

// --- La herramienta -----------------------------------------------------------------------------------------------------

/// Lo que la herramienta pide al fondo.
pub enum Pide {
    /// Vista previa de este fondo, con la transición saliendo de este punto de la pantalla.
    Vista(String, (i32, i32)),
    /// Volver a lo aplicado (al cancelar o al salir de la rejilla).
    Volver((i32, i32)),
    /// Aplicar estos ajustes (y guardarlos).
    Aplicar(Config, (i32, i32)),
}

enum Mini {
    Falta,
    Pedida,
    Hecha(Imagen),
    Mala(String),
}

struct Entrada {
    camino: String,
    nombre: String,
    tam: u64,
    mini: Mini,
    semilla: u32,
    medida: (u32, u32),
}

#[derive(Clone, Copy, PartialEq)]
enum Accion {
    Cerrar,
    Aplicar,
    Cancelar,
    Carpeta(usize),
    Pantalla(Option<usize>),
    Modo(bool),
    Ajuste(Ajuste),
    Transicion,
    Pase,
    Efecto,
    Miniatura(usize),
}

pub struct Herramienta {
    pub abierta: bool,
    /// Lo último que hizo pide cerrarla (Aplicar, Cancelar, Esc).
    pub pide_cerrar: bool,
    configurada: bool,
    ancho: i32,
    alto: i32,
    /// Dónde está en la pantalla (para saber dónde cae cada miniatura).
    x0: i32,
    y0: i32,
    pantalla: (i32, i32),
    lamina: Option<crate::ventana_cliente::Lamina>,
    aviso: u32,
    siguiente_aviso: u32,
    sucia: bool,
    tema: Tema,
    regular: Fuente,
    medium: Fuente,
    teclado: kit::teclado::Teclado,
    raton: (i32, i32),
    dentro: bool,
    pulsado_en: Option<(Accion, u64)>,
    ultimo_clic: (Option<usize>, u64),
    // Lo que se ve.
    carpeta: usize,
    entradas: Vec<Entrada>,
    filtradas: Vec<usize>,
    busca: String,
    elegida: Option<usize>,
    encima: Option<(usize, u64)>,
    vista_de: Option<usize>,
    desplaza: i32,
    pub salidas: Vec<String>,
    salida: Option<usize>,
    oscuro: bool,
    /// Si se eligió a mano el modo (entonces se aplica solo a ese).
    modo_tocado: bool,
    pub config: Config,
    acento: Option<(u32, bool, [Color; 6])>,
    botones: Vec<(Rect, Accion)>,
    rejilla: Rect,
    celda: (i32, i32, i32),
    columnas: i32,
    /// Si ya dijo dónde está cada cosa (para las pruebas con el ratón), desde que se abrió.
    dicho: bool,
}

impl Herramienta {
    pub fn nueva() -> Herramienta {
        Herramienta {
            abierta: false,
            pide_cerrar: false,
            configurada: false,
            ancho: 0,
            alto: 0,
            x0: 0,
            y0: 0,
            pantalla: (0, 0),
            lamina: None,
            aviso: 0,
            siguiente_aviso: 0,
            sucia: false,
            tema: Tema::OSCURO,
            regular: Fuente::regular(),
            medium: Fuente::medium(),
            teclado: kit::teclado::Teclado::new(),
            raton: (-1, -1),
            dentro: false,
            pulsado_en: None,
            ultimo_clic: (None, 0),
            carpeta: 0,
            entradas: Vec::new(),
            filtradas: Vec::new(),
            busca: String::new(),
            elegida: None,
            encima: None,
            vista_de: None,
            desplaza: 0,
            salidas: Vec::new(),
            salida: None,
            oscuro: false,
            modo_tocado: false,
            config: Config::default(),
            acento: None,
            botones: Vec::new(),
            rejilla: Rect::default(),
            celda: (0, 0, 0),
            columnas: 1,
            dicho: false,
        }
    }

    pub fn es_suyo(objeto: u32) -> bool {
        (SUP..=BUFER).contains(&objeto) || (PRIMER_AVISO..PRIMER_AVISO + 256).contains(&objeto)
    }

    pub fn superficie() -> u32 {
        SUP
    }

    pub fn tema(&mut self, color: u32, oscuro: bool) {
        self.tema = Tema::del_sistema(color, oscuro);
        if !self.abierta {
            self.oscuro = oscuro;
        }
        self.sucia = true;
    }

    /// Abre la capa en una pantalla de `w` × `h`, con los ajustes de ahora.
    pub fn abrir(&mut self, c: &mut crate::ventana_cliente::Cliente, (w, h): (i32, i32), config: &Config, oscuro: bool) {
        if self.abierta {
            return;
        }
        let margen = if w >= 1600 { 24 } else { 12 };
        self.ancho = w - 2 * margen;
        // La mitad de la pantalla; en una pequeña (800 × 600), más: si no, no cabe nada.
        self.alto = if h >= 900 { h * 52 / 100 } else { (h * 78 / 100).min(h - 40) }.clamp(240.min(h), h);
        (self.x0, self.y0, self.pantalla) = (margen, h - self.alto - margen / 2, (w, h));
        let tam = (self.ancho * self.alto * 4) as usize;
        let mut e = Escritor::new();
        let Some(l) = crate::ventana_cliente::Lamina::nueva(&mut e, tam, VA, POOL) else {
            println!("  [fondo] herramienta: sin memoria para su capa");
            return;
        };
        self.lamina = Some(l);
        e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER).int(0).int(self.ancho).int(self.alto).int(self.ancho * 4).uint(wayland::shm::ARGB8888).fin();
        e.empezar(crate::ventana_cliente::COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUP).fin();
        // Encima de todo (3), pegada abajo (2) y a los lados (4 | 8), con el teclado para ella (1, exclusivo).
        e.empezar(crate::fondo_pantalla::CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(CAPA).uint(SUP).uint(0).uint(3).texto(Some("fondos")).fin();
        e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(2 | 4 | 8).fin();
        e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_SIZE).uint(0).uint(self.alto as u32).fin();
        e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_MARGIN).int(0).int(margen).int(margen / 2).int(margen).fin();
        e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_KEYBOARD_INTERACTIVITY).uint(1).fin();
        e.empezar(SUP, wl_surface::req::COMMIT).fin();
        c.mandar(&e);
        self.abierta = true;
        self.dicho = false;
        self.pide_cerrar = false;
        self.configurada = false;
        self.config = config.clone();
        self.oscuro = oscuro;
        self.modo_tocado = false;
        self.salida = None;
        self.busca.clear();
        self.vista_de = None;
        self.encima = None;
        self.acento = None;
        // La carpeta del fondo de ahora.
        let ahora = config.elegir(self.salidas.first().map_or("", |s| s.as_str()), oscuro);
        self.carpeta = if ahora.starts_with("animado:") { 2 } else if ahora.starts_with("SYS:") { 1 } else { 0 };
        self.cargar_carpeta();
        self.elegida = self.filtradas.iter().position(|&i| self.entradas[i].camino == ahora);
        println!("  [fondo] herramienta abierta: {} × {} en ({}, {}), {} en {}", self.ancho, self.alto, self.x0, self.y0, self.entradas.len(), CARPETAS[self.carpeta]);
    }

    pub fn cerrar(&mut self, c: &mut crate::ventana_cliente::Cliente) {
        if !self.abierta {
            return;
        }
        let mut e = Escritor::new();
        e.empezar(BUFER, wayland::tablas::wl_buffer::req::DESTROY).fin();
        e.empezar(CAPA, zwlr_layer_surface_v1::req::DESTROY).fin();
        e.empezar(SUP, wl_surface::req::DESTROY).fin();
        if let Some(l) = self.lamina.take() {
            l.soltar(&mut e);
        }
        c.mandar(&e);
        self.abierta = false;
        self.aviso = 0;
        // Las miniaturas se quedan (se reabre deprisa); las de más, fuera.
        println!("  [fondo] herramienta cerrada");
    }

    fn cargar_carpeta(&mut self) {
        // Dónde queda cada cosa se vuelve a decir (las pruebas con el ratón lo leen).
        self.dicho = false;
        self.entradas.clear();
        self.desplaza = 0;
        if self.carpeta == 2 {
            for n in animado::NOMBRES {
                self.entradas.push(Entrada { camino: alloc::format!("animado:{}", n), nombre: String::from(nombre_animado(n)), tam: 0, mini: Mini::Falta, semilla: 0, medida: (0, 0) });
            }
        } else {
            match lector::listar(lector::VA_PRINCIPAL, CARPETAS[self.carpeta], MAX_ENTRADAS) {
                Ok(v) => {
                    for (n, tam) in v.into_iter().filter(|(n, _)| n.to_ascii_lowercase().ends_with(".png")) {
                        self.entradas.push(Entrada { camino: alloc::format!("{}/{}", CARPETAS[self.carpeta], n), nombre: n, tam, mini: Mini::Falta, semilla: 0, medida: (0, 0) });
                    }
                }
                Err(e) => println!("  [fondo] herramienta: {} no se lista: {}", CARPETAS[self.carpeta], e),
            }
        }
        println!("  [fondo] herramienta: carpeta {}, {} fondos", CARPETAS[self.carpeta], self.entradas.len());
        self.filtrar();
    }

    fn filtrar(&mut self) {
        let b = self.busca.to_lowercase();
        self.filtradas = (0..self.entradas.len()).filter(|&i| b.is_empty() || self.entradas[i].nombre.to_lowercase().contains(&b)).collect();
        self.elegida = None;
        self.desplaza = 0;
        self.sucia = true;
    }

    /// Lo que llegó del hilo de las miniaturas.
    pub fn hecho(&mut self, h: Hecho) {
        match h {
            Hecho::Mini { id, img, semilla, medida, porque } => {
                if let Some(e) = self.entradas.get_mut(id as usize) {
                    e.mini = match img {
                        Some(i) => Mini::Hecha(i),
                        None => {
                            println!("  [fondo] herramienta: sin miniatura de {}: {}", e.camino, porque);
                            Mini::Mala(porque)
                        }
                    };
                    (e.semilla, e.medida) = (semilla, medida);
                }
                // Que no se guarden demasiadas: las que no se ven, fuera.
                let hechas = self.entradas.iter().filter(|e| matches!(e.mini, Mini::Hecha(_))).count();
                if hechas > MAX_MINIATURAS {
                    let vistas = self.vistas();
                    for (i, e) in self.entradas.iter_mut().enumerate() {
                        if matches!(e.mini, Mini::Hecha(_)) && !vistas.contains(&i) {
                            e.mini = Mini::Falta;
                        }
                    }
                }
            }
            Hecho::Acento { semilla, oscuro, colores } => self.acento = Some((semilla, oscuro, colores)),
        }
        self.sucia = true;
    }

    /// Las entradas que se ven ahora (y una fila más).
    fn vistas(&self) -> Vec<usize> {
        let (_, alto_celda, hueco) = self.celda;
        if alto_celda <= 0 {
            return Vec::new();
        }
        let fila0 = (self.desplaza / (alto_celda + hueco)).max(0);
        let filas = self.rejilla.h / (alto_celda + hueco) + 2;
        let desde = (fila0 * self.columnas) as usize;
        let hasta = ((fila0 + filas) * self.columnas) as usize;
        self.filtradas.iter().skip(desde).take(hasta.saturating_sub(desde)).copied().collect()
    }

    /// Pide las miniaturas que faltan de lo que se ve (pocas a la vez: el hilo va de una en una).
    fn pedir_miniaturas(&mut self) {
        let (w, h, _) = self.celda;
        if w <= 0 {
            return;
        }
        let pedidas = self.entradas.iter().filter(|e| matches!(e.mini, Mini::Pedida)).count();
        let mut van = pedidas;
        for i in self.vistas() {
            if van >= 4 {
                break;
            }
            if matches!(self.entradas[i].mini, Mini::Falta) {
                let e = &mut self.entradas[i];
                if pedir_trabajo(Trabajo::Mini { id: i as u32, camino: e.camino.clone(), w: w as usize, h: (h - 22) as usize }) {
                    e.mini = Mini::Pedida;
                    van += 1;
                }
            }
        }
    }

    /// Un evento del compositor para ella. Lo que pide al fondo, si algo.
    pub fn evento(&mut self, c: &mut crate::ventana_cliente::Cliente, o: u32, orden: u16, cuerpo: &[u8]) -> Option<Pide> {
        let palabra = |k: usize| cuerpo.get(4 * k..4 * k + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
        match (o, orden) {
            (CAPA, zwlr_layer_surface_v1::ev::CONFIGURE) => {
                let mut e = Escritor::new();
                e.empezar(CAPA, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                c.mandar(&e);
                self.configurada = true;
                self.sucia = true;
            }
            (CAPA, zwlr_layer_surface_v1::ev::CLOSED) => {
                self.cerrar(c);
                return Some(Pide::Volver(self.centro()));
            }
            (o, wayland::tablas::wl_callback::ev::DONE) if o == self.aviso => self.aviso = 0,
            _ => {}
        }
        None
    }

    fn centro(&self) -> (i32, i32) {
        (self.pantalla.0 / 2, self.pantalla.1 / 3)
    }

    /// El ratón (en la capa) y sus botones. Lo que pide al fondo.
    pub fn raton(&mut self, orden: u16, cuerpo: &[u8], ahora: u64) -> Option<Pide> {
        let palabra = |k: usize| cuerpo.get(4 * k..4 * k + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
        match orden {
            wl_pointer::ev::ENTER => {
                self.dentro = true;
                self.raton = ((palabra(2) as i32) >> 8, (palabra(3) as i32) >> 8);
            }
            wl_pointer::ev::LEAVE => {
                self.dentro = false;
                self.encima = None;
                self.sucia = true;
            }
            wl_pointer::ev::MOTION => {
                self.raton = ((palabra(1) as i32) >> 8, (palabra(2) as i32) >> 8);
                let sobre = self.accion_en(self.raton);
                let mini = match sobre {
                    Some(Accion::Miniatura(k)) => Some(k),
                    _ => None,
                };
                if mini != self.encima.map(|e| e.0) {
                    self.encima = mini.map(|k| (k, ahora));
                    self.sucia = true;
                }
            }
            wl_pointer::ev::AXIS if palabra(1) == 0 => {
                let v = palabra(2) as i32 >> 8;
                self.desplazar(if v > 0 { 60 } else { -60 });
            }
            wl_pointer::ev::BUTTON if palabra(2) == 0x110 => {
                if palabra(3) == 1 {
                    self.pulsado_en = self.accion_en(self.raton).map(|a| (a, ahora));
                } else if let Some((a, _)) = self.pulsado_en.take()
                    && self.accion_en(self.raton) == Some(a)
                {
                    return self.hacer(a, ahora);
                }
            }
            _ => {}
        }
        None
    }

    fn desplazar(&mut self, d: i32) {
        let (_, alto_celda, hueco) = self.celda;
        let filas = (self.filtradas.len() as i32 + self.columnas - 1) / self.columnas.max(1);
        let max = (filas * (alto_celda + hueco) - self.rejilla.h).max(0);
        self.desplaza = (self.desplaza + d).clamp(0, max);
        self.sucia = true;
    }

    fn accion_en(&self, (x, y): (i32, i32)) -> Option<Accion> {
        self.botones.iter().rev().find(|(r, _)| r.contiene(x, y)).map(|b| b.1)
    }

    /// El punto de la pantalla en el centro de la miniatura `k` (de las filtradas), si se ve; si no, el ratón.
    fn punto_de(&self, k: usize) -> (i32, i32) {
        match self.botones.iter().find(|(_, a)| *a == Accion::Miniatura(k)) {
            Some((r, _)) => (self.x0 + r.x + r.w / 2, self.y0 + r.y + r.h / 2),
            None => (self.x0 + self.raton.0.max(0), self.y0 + self.raton.1.max(0)),
        }
    }

    fn hacer(&mut self, a: Accion, ahora: u64) -> Option<Pide> {
        self.sucia = true;
        match a {
            Accion::Cerrar | Accion::Cancelar => {
                self.pide_cerrar = true;
                return Some(Pide::Volver(self.centro()));
            }
            Accion::Aplicar => return self.aplicar(),
            Accion::Carpeta(k) => {
                if k != self.carpeta {
                    self.carpeta = k;
                    self.cargar_carpeta();
                }
            }
            Accion::Pantalla(s) => self.salida = s,
            Accion::Modo(o) => {
                self.oscuro = o;
                self.modo_tocado = true;
            }
            Accion::Ajuste(aj) => self.config.ajuste = aj,
            Accion::Transicion => {
                let i = TRANSICIONES.iter().position(|t| *t == self.config.transicion).unwrap_or(0);
                self.config.transicion = String::from(TRANSICIONES[(i + 1) % TRANSICIONES.len()]);
            }
            Accion::Pase => {
                let i = PASES.iter().position(|&p| p == self.config.pase).unwrap_or(0);
                self.config.pase = PASES[(i + 1) % PASES.len()];
            }
            Accion::Efecto => {
                let ahora_e = self.config.efecto.as_deref().unwrap_or("ninguno");
                let i = EFECTOS.iter().position(|e| *e == ahora_e).unwrap_or(0);
                let n = EFECTOS[(i + 1) % EFECTOS.len()];
                self.config.efecto = (n != "ninguno").then(|| String::from(n));
            }
            Accion::Miniatura(k) => {
                let doble = self.ultimo_clic.0 == Some(k) && ahora.saturating_sub(self.ultimo_clic.1) < 500_000;
                self.ultimo_clic = (Some(k), ahora);
                self.elegida = Some(k);
                self.pedir_acento();
                if doble {
                    return self.aplicar();
                }
                return self.vista_previa(k);
            }
        }
        None
    }

    fn vista_previa(&mut self, k: usize) -> Option<Pide> {
        if self.vista_de == Some(k) {
            return None;
        }
        let i = *self.filtradas.get(k)?;
        self.vista_de = Some(k);
        println!("  [fondo] herramienta: vista previa de {}", self.entradas[i].camino);
        Some(Pide::Vista(self.entradas[i].camino.clone(), self.punto_de(k)))
    }

    fn pedir_acento(&mut self) {
        let Some(i) = self.elegida.and_then(|k| self.filtradas.get(k).copied()) else { return };
        let s = self.entradas[i].semilla;
        if s != 0 && self.acento.is_none_or(|a| a.0 != s || a.1 != self.oscuro) {
            pedir_trabajo(Trabajo::Acento { semilla: s, oscuro: self.oscuro });
        }
    }

    fn aplicar(&mut self) -> Option<Pide> {
        let i = self.elegida.and_then(|k| self.filtradas.get(k).copied());
        let punto = self.elegida.map_or(self.centro(), |k| self.punto_de(k));
        if let Some(i) = i {
            let camino = self.entradas[i].camino.clone();
            let salida = self.salida.and_then(|s| self.salidas.get(s)).cloned();
            // Si no se ha tocado "Claro" ni "Oscuro", para los dos modos (lo que espera quien no piensa en modos); si
            // se eligió uno, solo para ese.
            if self.modo_tocado {
                self.config.poner(salida.as_deref(), self.oscuro, &camino);
            } else {
                self.config.poner_los_dos(salida.as_deref(), &camino);
            }
        }
        self.pide_cerrar = true;
        Some(Pide::Aplicar(self.config.clone(), punto))
    }

    /// Una tecla. Lo que pide al fondo.
    pub fn tecla(&mut self, codigo: u32, pulsada: bool) -> Option<Pide> {
        use kit::teclado::Tecla;
        let t = self.teclado.tecla(codigo, pulsada)?;
        self.sucia = true;
        let n = self.filtradas.len();
        let mover = |s: &mut Self, d: i32| -> Option<Pide> {
            if n == 0 {
                return None;
            }
            let k = (s.elegida.map_or(0, |k| k as i32 + d)).clamp(0, n as i32 - 1) as usize;
            s.elegida = Some(k);
            s.asegurar_visible(k);
            s.pedir_acento();
            s.encima = Some((k, dashos::ahora_us()));
            None
        };
        match t {
            Tecla::Escape => {
                self.pide_cerrar = true;
                Some(Pide::Volver(self.centro()))
            }
            Tecla::Intro => self.aplicar(),
            Tecla::Derecha => mover(self, 1),
            Tecla::Izquierda => mover(self, -1),
            Tecla::Abajo => mover(self, self.columnas),
            Tecla::Arriba => mover(self, -self.columnas),
            Tecla::Tabulador => {
                self.carpeta = (self.carpeta + 1) % CARPETAS.len();
                self.cargar_carpeta();
                None
            }
            Tecla::Retroceso => {
                self.busca.pop();
                self.filtrar();
                None
            }
            Tecla::Letra(ch) if !ch.is_control() && self.busca.chars().count() < 40 => {
                self.busca.push(ch);
                self.filtrar();
                None
            }
            _ => None,
        }
    }

    fn asegurar_visible(&mut self, k: usize) {
        let (_, alto_celda, hueco) = self.celda;
        let fila = k as i32 / self.columnas.max(1);
        let y = fila * (alto_celda + hueco);
        if y < self.desplaza {
            self.desplaza = y;
        } else if y + alto_celda > self.desplaza + self.rejilla.h {
            self.desplaza = y + alto_celda - self.rejilla.h;
        }
    }

    /// Lo que toca con el tiempo: la vista previa de lo que tiene el ratón encima (o lo elegido con el teclado) cuando
    /// lleva un rato quieto. Lo que pide al fondo.
    pub fn tic(&mut self, ahora: u64) -> Option<Pide> {
        if !self.abierta {
            return None;
        }
        if let Some((k, desde)) = self.encima
            && ahora >= desde + QUIETO_US
        {
            self.encima = Some((k, u64::MAX / 2));
            if self.elegida != Some(k) && self.dentro {
                self.elegida = Some(k);
                self.pedir_acento();
                self.sucia = true;
            }
            return self.vista_previa(k);
        }
        None
    }

    /// Cuándo hay que volver a mirarla (µs).
    pub fn plazo(&self) -> u64 {
        match self.encima {
            Some((_, d)) if d < u64::MAX / 2 => d + QUIETO_US,
            _ => u64::MAX,
        }
    }

    /// Repinta si hace falta (y no espera ya un aviso de `frame`).
    pub fn pintar(&mut self, c: &mut crate::ventana_cliente::Cliente) {
        if !self.abierta || !self.configurada || !self.sucia || self.aviso != 0 {
            return;
        }
        self.sucia = false;
        let (w, h) = (self.ancho, self.alto);
        let Some(mut lam) = self.lamina.take() else { return };
        {
            let px = lam.px((w * h) as usize);
            let mut l = Lienzo::new(px, w, h, w as usize);
            self.dibujar(&mut l);
        }
        self.lamina = Some(lam);
        self.pedir_miniaturas();
        let mut e = Escritor::new();
        self.siguiente_aviso = (self.siguiente_aviso + 1) % 256;
        self.aviso = PRIMER_AVISO + self.siguiente_aviso;
        e.empezar(SUP, wl_surface::req::ATTACH).uint(BUFER).int(0).int(0).fin();
        e.empezar(SUP, wl_surface::req::DAMAGE_BUFFER).int(0).int(0).int(w).int(h).fin();
        e.empezar(SUP, wl_surface::req::FRAME).uint(self.aviso).fin();
        e.empezar(SUP, wl_surface::req::COMMIT).fin();
        c.mandar(&e);
        if !self.dicho {
            self.dicho = true;
            let sitio = |q: &str| self.sitio(q).map_or(String::from("?"), |(x, y)| alloc::format!("({}, {})", x, y));
            println!(
                "  [fondo] herramienta en pantalla: miniatura 0 en {}, 1 en {}, 2 en {}; aplicar en {}, cancelar en {}, animados en {}, sys en {}, home en {}, efecto en {}, oscuro en {}, transición en {}",
                sitio("0"), sitio("1"), sitio("2"), sitio("aplicar"), sitio("cancelar"), sitio("animados"), sitio("sys"), sitio("home"), sitio("efecto"), sitio("oscuro"), sitio("transicion")
            );
        }
    }

    /// Un chip en `(x, y)`; si no cabe antes de `hasta`, en la línea de debajo (desde `desde`).
    #[allow(clippy::too_many_arguments)]
    fn chip(&mut self, l: &mut Lienzo, (x, y): (&mut i32, &mut i32), (desde, hasta): (i32, i32), texto: &str, activo: bool, a: Accion) {
        let t = self.tema;
        let m = self.medium.medir(texto, 13);
        if *x + m.ancho + 28 > hasta && *x > desde {
            *x = desde;
            *y += 40;
        }
        let r = Rect::new(*x, *y, m.ancho + 28, 32);
        if activo {
            l.redondeado(r, 16, t.contenedor_secundario());
        } else {
            l.borde(r, 16, 1, t.variante_contorno);
        }
        if self.dentro && r.contiene(self.raton.0, self.raton.1) {
            l.redondeado(r, 16, con_opacidad(t.sobre_superficie, 20));
        }
        self.medium.dibujar(l, r.x + 14, r.y + 21, texto, 13, if activo { t.sobre_contenedor_secundario() } else { t.sobre_variante_superficie });
        self.botones.push((r, a));
        *x += r.w + 8;
    }

    fn dibujar(&mut self, l: &mut Lienzo) {
        let t = self.tema;
        let (w, h) = (self.ancho, self.alto);
        self.botones.clear();
        l.borrar(Rect::new(0, 0, w, h), 0);
        l.redondeado(Rect::new(0, 0, w, h), 28, con_opacidad(t.contenedor_bajo(), 246));
        let pad = 20;
        // Fila 1: título, pantallas, modo, buscar y cerrar.
        self.medium.dibujar(l, pad, pad + 26, "Fondos", 22, t.sobre_superficie);
        let mut x = pad + 120;
        let y1 = pad + 4;
        let mut y = y1;
        // Buscar: un campo a la derecha (se teclea sin más), más estrecho en una pantalla pequeña.
        let ancho_b = if w >= 1200 { 260 } else { 150 };
        let lim1 = w - pad - 48 - ancho_b - 8;
        // Las pantallas, solo si hay más de una.
        if self.salidas.len() > 1 {
            self.chip(l, (&mut x, &mut y), (pad + 120, lim1), "Todas las pantallas", self.salida.is_none(), Accion::Pantalla(None));
            for k in 0..self.salidas.len().min(4) {
                let n = self.salidas[k].clone();
                self.chip(l, (&mut x, &mut y), (pad + 120, lim1), &n, self.salida == Some(k), Accion::Pantalla(Some(k)));
            }
            x += 16;
        }
        self.chip(l, (&mut x, &mut y), (pad + 120, lim1), "Claro", !self.oscuro, Accion::Modo(false));
        self.chip(l, (&mut x, &mut y), (pad + 120, lim1), "Oscuro", self.oscuro, Accion::Modo(true));
        let rb = Rect::new(w - pad - 48 - ancho_b, y1, ancho_b, 32);
        l.redondeado(rb, 16, t.contenedor_alto());
        let texto_b = if self.busca.is_empty() { String::from("Buscar…") } else { alloc::format!("{}|", self.busca) };
        self.regular.dibujar(l, rb.x + 16, rb.y + 21, &texto_b, 13, if self.busca.is_empty() { t.sobre_variante_superficie } else { t.sobre_superficie });
        let rc = Rect::new(w - pad - 36, y1 - 2, 36, 36);
        if self.dentro && rc.contiene(self.raton.0, self.raton.1) {
            l.redondeado(rc, 18, con_opacidad(t.sobre_superficie, 24));
        }
        l.trazo(((rc.x + 12) * 8, (rc.y + 12) * 8), ((rc.x + 24) * 8, (rc.y + 24) * 8), 14, t.sobre_superficie);
        l.trazo(((rc.x + 24) * 8, (rc.y + 12) * 8), ((rc.x + 12) * 8, (rc.y + 24) * 8), 14, t.sobre_superficie);
        self.botones.push((rc, Accion::Cerrar));
        // Fila 2 (y las que hagan falta): carpetas, ajuste, transición, pase y efecto.
        let mut y = y + 44;
        let mut x = pad;
        let lim = (pad, w - pad);
        for k in 0..CARPETAS.len() {
            self.chip(l, (&mut x, &mut y), lim, CARPETAS[k], self.carpeta == k, Accion::Carpeta(k));
        }
        x += 16;
        for a in Ajuste::TODOS {
            self.chip(l, (&mut x, &mut y), lim, nombre_ajuste(a), self.config.ajuste == a, Accion::Ajuste(a));
        }
        x += 16;
        let tr = alloc::format!("Transición: {}", nombre_transicion(&self.config.transicion));
        self.chip(l, (&mut x, &mut y), lim, &tr, false, Accion::Transicion);
        let pase = if self.config.pase == 0 { String::from("Pase: no") } else { alloc::format!("Pase: cada {} min", self.config.pase) };
        self.chip(l, (&mut x, &mut y), lim, &pase, self.config.pase != 0, Accion::Pase);
        let ef = alloc::format!("Efecto: {}", self.config.efecto.as_deref().map_or("ninguno", nombre_animado));
        self.chip(l, (&mut x, &mut y), lim, &ef, self.config.efecto.is_some(), Accion::Efecto);
        let y2 = y;
        // La columna de la derecha: la elegida, su acento y los botones.
        let ancho_det = if w >= 1400 { 300 } else { 220 };
        let det = Rect::new(w - pad - ancho_det, y2 + 48, ancho_det, h - y2 - 48 - pad);
        self.rejilla = Rect::new(pad, y2 + 48, det.x - 2 * pad, h - y2 - 48 - pad);
        self.detalle(l, det);
        // La rejilla.
        let ancho_mini = if self.pantalla.0 >= 1600 { 200 } else { 150 };
        let alto_mini = ancho_mini * self.pantalla.1.max(1) / self.pantalla.0.max(1);
        let hueco = 14;
        self.columnas = ((self.rejilla.w + hueco) / (ancho_mini + hueco)).max(1);
        self.celda = (ancho_mini, alto_mini + 22, hueco);
        l.recortar(self.rejilla);
        if self.filtradas.is_empty() {
            let msg = if self.busca.is_empty() { alloc::format!("No hay fondos en {}", CARPETAS[self.carpeta]) } else { alloc::format!("Nada se llama así ({})", self.busca) };
            self.regular.dibujar(l, self.rejilla.x + 8, self.rejilla.y + 30, &msg, 14, t.sobre_variante_superficie);
        }
        for k in 0..self.filtradas.len() {
            let (fila, col) = (k as i32 / self.columnas, k as i32 % self.columnas);
            let r = Rect::new(self.rejilla.x + col * (ancho_mini + hueco), self.rejilla.y + fila * (alto_mini + 22 + hueco) - self.desplaza, ancho_mini, alto_mini);
            if r.y + r.h + 22 < self.rejilla.y || r.y > self.rejilla.y + self.rejilla.h {
                continue;
            }
            let i = self.filtradas[k];
            let elegida = self.elegida == Some(k);
            let encima = self.encima.is_some_and(|e| e.0 == k) && self.dentro;
            if elegida {
                l.redondeado(Rect::new(r.x - 4, r.y - 4, r.w + 8, r.h + 8), 16, t.primario);
            } else if encima {
                l.redondeado(Rect::new(r.x - 3, r.y - 3, r.w + 6, r.h + 6), 15, con_opacidad(t.sobre_superficie, 60));
            }
            match &self.entradas[i].mini {
                Mini::Hecha(img) => pegar(l, img, r, 12),
                Mini::Mala(porque) => {
                    l.redondeado(r, 12, t.contenedor_alto());
                    self.regular.dibujar(l, r.x + 12, r.y + r.h / 2 - 4, "No se puede leer", 12, t.error);
                    let p = recortar_texto(&mut self.regular, porque, 11, r.w - 24);
                    self.regular.dibujar(l, r.x + 12, r.y + r.h / 2 + 12, &p, 11, t.sobre_variante_superficie);
                }
                _ => l.redondeado(r, 12, t.contenedor_alto()),
            }
            let nombre = recortar_texto(&mut self.regular, &self.entradas[i].nombre, 12, r.w);
            self.regular.dibujar(l, r.x + 2, r.y + r.h + 16, &nombre, 12, if elegida { t.primario } else { t.sobre_variante_superficie });
            let toque = Rect::new(r.x, r.y, r.w, r.h + 20).cruce(self.rejilla);
            if !toque.vacio() {
                self.botones.push((toque, Accion::Miniatura(k)));
            }
        }
        l.soltar_recorte();
        // La barra de desplazar, si no cabe.
        let filas = (self.filtradas.len() as i32 + self.columnas - 1) / self.columnas;
        let total = filas * (alto_mini + 22 + hueco);
        if total > self.rejilla.h && total > 0 {
            let alto_b = (self.rejilla.h * self.rejilla.h / total).max(24);
            let y_b = self.rejilla.y + self.desplaza * (self.rejilla.h - alto_b) / (total - self.rejilla.h).max(1);
            l.redondeado(Rect::new(self.rejilla.x + self.rejilla.w + 6, y_b, 4, alto_b), 2, t.variante_contorno);
        }
        l.esquinas_transparentes(Rect::new(0, 0, w, h), 28);
    }

    fn detalle(&mut self, l: &mut Lienzo, det: Rect) {
        let t = self.tema;
        l.redondeado(det, 20, t.contenedor());
        // La vista en grande, con la proporción de la pantalla, sin pisar el texto ni los botones de abajo.
        let alto_pv = ((det.w - 24) * self.pantalla.1.max(1) / self.pantalla.0.max(1)).min(det.h - 12 - 60 - 64).max(40);
        let ancho_pv = (alto_pv * self.pantalla.0.max(1) / self.pantalla.1.max(1)).min(det.w - 24);
        let pv = Rect::new(det.x + 12, det.y + 12, ancho_pv, alto_pv);
        let elegida = self.elegida.and_then(|k| self.filtradas.get(k).copied());
        match elegida.map(|i| &self.entradas[i]) {
            Some(e) => {
                match &e.mini {
                    Mini::Hecha(img) => pegar_escalada(l, img, pv, 12),
                    _ => l.redondeado(pv, 12, t.contenedor_alto()),
                }
                let nombre = recortar_texto(&mut self.medium, &e.nombre, 15, pv.w);
                self.medium.dibujar(l, pv.x, pv.y + pv.h + 24, &nombre, 15, t.sobre_superficie);
                let info = if e.camino.starts_with("animado:") {
                    String::from("Animado, sin GPU: pocos cuadros y solo lo que cambia")
                } else if e.medida.0 > 0 {
                    alloc::format!("{} × {} · {} KB · {}", e.medida.0, e.medida.1, e.tam.div_ceil(1024), CARPETAS[self.carpeta])
                } else {
                    alloc::format!("{} KB · {}", e.tam.div_ceil(1024), CARPETAS[self.carpeta])
                };
                let info = recortar_texto(&mut self.regular, &info, 12, pv.w);
                self.regular.dibujar(l, pv.x, pv.y + pv.h + 44, &info, 12, t.sobre_variante_superficie);
                // El acento que saldría (la semilla y los colores del tema en el modo elegido).
                let ya = pv.y + pv.h + 60;
                if ya + 60 < det.y + det.h - 52 {
                    self.regular.dibujar(l, pv.x, ya + 14, &alloc::format!("Color de acento ({})", if self.oscuro { "oscuro" } else { "claro" }), 12, t.sobre_variante_superficie);
                    match self.acento.filter(|a| a.0 == e.semilla && a.1 == self.oscuro && e.semilla != 0) {
                        Some((_, _, colores)) => {
                            for (k, &col) in colores.iter().enumerate() {
                                let r = Rect::new(pv.x + k as i32 * 34, ya + 24, 28, 28);
                                l.redondeado(r, 14, col);
                                l.borde(r, 14, 1, t.variante_contorno);
                            }
                        }
                        None => {
                            let txt = if e.semilla == 0 && matches!(e.mini, Mini::Hecha(_)) { "Sin color propio: el violeta de serie" } else { "Calculando…" };
                            self.regular.dibujar(l, pv.x, ya + 44, txt, 12, t.sobre_variante_superficie);
                        }
                    }
                }
            }
            None => {
                l.redondeado(pv, 12, t.contenedor_alto());
                self.regular.dibujar(l, pv.x + 12, pv.y + pv.h / 2 + 5, "Pasa el ratón por un fondo", 13, t.sobre_variante_superficie);
            }
        }
        // Aplicar (relleno) y Cancelar (texto), abajo.
        let ancho_boton = ((det.w - 32) / 2).min(110);
        let ra = Rect::new(det.x + det.w - 12 - ancho_boton, det.y + det.h - 52, ancho_boton, 40);
        l.redondeado(ra, 20, t.primario);
        let m = self.medium.medir("Aplicar", 14);
        self.medium.dibujar(l, ra.x + (ra.w - m.ancho) / 2, ra.y + 26, "Aplicar", 14, t.sobre_primario);
        self.botones.push((ra, Accion::Aplicar));
        let rc = Rect::new(ra.x - 8 - ancho_boton, ra.y, ancho_boton, 40);
        if self.dentro && rc.contiene(self.raton.0, self.raton.1) {
            l.redondeado(rc, 20, con_opacidad(t.primario, 30));
        }
        let m = self.medium.medir("Cancelar", 14);
        self.medium.dibujar(l, rc.x + (rc.w - m.ancho) / 2, rc.y + 26, "Cancelar", 14, t.primario);
        self.botones.push((rc, Accion::Cancelar));
    }

    /// Dónde dice que está cada cosa (para las pruebas con el ratón): miniatura `k`, Aplicar, etc., en la pantalla.
    pub fn sitio(&self, que: &str) -> Option<(i32, i32)> {
        let a = match que {
            "aplicar" => Accion::Aplicar,
            "cancelar" => Accion::Cancelar,
            "cerrar" => Accion::Cerrar,
            "efecto" => Accion::Efecto,
            "animados" => Accion::Carpeta(2),
            "sys" => Accion::Carpeta(1),
            "home" => Accion::Carpeta(0),
            "oscuro" => Accion::Modo(true),
            "claro" => Accion::Modo(false),
            "mosaico" => Accion::Ajuste(Ajuste::Mosaico),
            "llenar" => Accion::Ajuste(Ajuste::Llenar),
            "transicion" => Accion::Transicion,
            "pase" => Accion::Pase,
            n => Accion::Miniatura(n.parse().ok()?),
        };
        self.botones.iter().find(|b| b.1 == a).map(|(r, _)| (self.x0 + r.x + r.w / 2, self.y0 + r.y + r.h / 2))
    }
}

/// Pega `img` en `r` (ya del tamaño; si no, lo que quepa), con las esquinas redondeadas de `radio`.
fn pegar(l: &mut Lienzo, img: &Imagen, r: Rect, radio: i32) {
    let rc = r.cruce(l.recorte());
    for y in rc.y..rc.y + rc.h {
        let iy = (y - r.y) as usize;
        if iy >= img.alto {
            break;
        }
        // Lo que se come la esquina en esta fila.
        let dy = if y - r.y < radio { radio - (y - r.y) } else if r.y + r.h - 1 - y < radio { radio - (r.y + r.h - 1 - y) } else { 0 };
        let come = if dy > 0 { radio - ((radio * radio - dy * dy).max(0) as u32).isqrt() as i32 } else { 0 };
        for x in rc.x.max(r.x + come)..rc.x.saturating_add(rc.w).min(r.x + r.w - come) {
            let ix = (x - r.x) as usize;
            if ix < img.ancho {
                l.punto(x, y, img.px[iy * img.ancho + ix] | 0xff00_0000, 255);
            }
        }
    }
}

/// Como `pegar`, escalada a `r` (vecino más cercano: la miniatura ya viene bien reducida).
fn pegar_escalada(l: &mut Lienzo, img: &Imagen, r: Rect, radio: i32) {
    if img.ancho == 0 || img.alto == 0 || r.w <= 0 || r.h <= 0 {
        return;
    }
    let (w, h) = (r.w as usize, r.h as usize);
    let px = (0..w * h).map(|i| img.px[((i / w) * img.alto / h) * img.ancho + (i % w) * img.ancho / w]).collect();
    pegar(l, &Imagen { ancho: w, alto: h, px }, r, radio);
}

/// El texto, cortado con "…" para que quepa en `ancho`.
fn recortar_texto(f: &mut Fuente, t: &str, tam: u32, ancho: i32) -> String {
    if f.medir(t, tam).ancho <= ancho {
        return String::from(t);
    }
    let mut s: String = t.into();
    while !s.is_empty() && f.medir(&alloc::format!("{}…", s), tam).ancho > ancho {
        s.pop();
    }
    alloc::format!("{}…", s)
}

pub fn nombre_animado(n: &str) -> &'static str {
    match n {
        "aurora" => "Aurora",
        "luciernagas" => "Luciérnagas",
        "nieve" => "Nieve",
        "respira" => "Respira",
        "ciclos" => "Paleta animada",
        _ => "Ninguno",
    }
}

fn nombre_ajuste(a: Ajuste) -> &'static str {
    match a {
        Ajuste::Llenar => "Llenar",
        Ajuste::Encajar => "Encajar",
        Ajuste::Centrar => "Centrar",
        Ajuste::Mosaico => "Mosaico",
        Ajuste::Estirar => "Estirar",
    }
}

fn nombre_transicion(t: &str) -> &'static str {
    match t {
        "circulo" => "círculo",
        "fundido" => "fundido",
        "barrido" => "barrido",
        "disolver" => "disolver",
        "azar" => "al azar",
        _ => "ninguna",
    }
}
