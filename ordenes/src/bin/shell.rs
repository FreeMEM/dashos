//! El Shell nativo de DashOS, al estilo del de AmigaOS (`docs/bitacoras/shell-nativa.md`). Está en `SYS:System/Shell`.
//!
//! Tres cosas en un programa: la ventana (una consola de líneas con desplazamiento, selección, portapapeles e
//! historial; `shell::consola`), el intérprete de órdenes y guiones (`shell::interprete`) y los trabajos (las órdenes
//! lanzadas, que le piden cosas por su puerta; `shell::protocolo`).
//!
//! `Shell [FROM guion] [REGISTRO] [SINARRANQUE] [ORDEN línea]`: de serie hace `S:Shell-Startup` y pide órdenes. Con
//! `FROM`, ese guion en vez del de arranque. Con `ORDEN`, hace esa línea y acaba con su código. Con `REGISTRO`, lo que
//! sale por la consola va también al registro del sistema (para las pruebas).
//!
//! Lanzado por otro Shell (`Run` de un guion o de una orden interna) no abre ventana: su consola es la del Shell que
//! lo lanzó.
//!
//! Llave 1: `sesiones`; 2: archivos; 3: el compositor; 4: el canal de arranque, si lo hay.

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../../../usuario/src/portapapeles_cliente.rs"]
mod portapapeles_cliente;

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use dashos::asignaciones::{self as asig, Cambio};
use dashos::println;
use dashos::protocolo::archivos as a;
use dashos::protocolo::sesiones as ps;
use kit::lienzo::{Lienzo, Rect};
use kit::teclado::{Tecla, Teclado};
use kit::tema::Tema;
use kit::texto::Fuente;
use ordenes::{self as orden, Cli, Sistema, SESIONES};
use shell::consola::{self, Consola, Punto};
use shell::interprete::{Interprete, Mundo, Paso};
use shell::linea::Orden as OrdenLinea;
use shell::protocolo as p;
use shell::{plantilla, ruta, RC_AVISO, RC_BIEN, RC_ERROR, RC_FALLO};
use wayland::canal::{self, Juntador, TROZO_CONECTAR, TROZO_DATOS};
use wayland::tablas::{
    dashos_tema_v1, wl_buffer, wl_callback, wl_compositor, wl_display, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool,
    wl_surface, xdg_surface, xdg_toplevel, xdg_wm_base,
};
use wayland::{leer_args, Escritor};

dashos::programa!(principal);
dashos::monton!();

const VENTANAS: u32 = 3;
/// Dónde se proyecta un programa mientras se lee del disco, y los píxeles de la ventana (dos sitios: al cambiar de
/// tamaño se pinta en el otro).
const VA_PROGRAMA: usize = 0x48_0000_0000;
const VA_PIXELES: [usize; 2] = [0x50_0000_0000, 0x54_0000_0000];
/// Lo más que mide un programa (lo que admite `sesiones`).
const MAX_PROGRAMA: usize = 4 << 20;
/// Los programas que se guardan ya leídos (un guion que llama cien veces a `Echo` no lo lee cien veces).
const MAX_GUARDADOS: usize = 8;
/// Lo más que se guarda de la salida de una orden para la siguiente de una tubería, y de un fichero de entrada.
const MAX_TUBO: usize = 1 << 20;
/// Lo que se espera a una orden rota con Ctrl-C antes de matarla.
const PLAZO_ROTO_US: u64 = 2_000_000;
/// Los trabajos a la vez.
const MAX_TRABAJOS: usize = 16;
/// Lo que se espera a `sesiones`.
const PLAZO_SESIONES_US: u64 = 10_000_000;

// Los objetos de la ventana.
const REGISTRO: u32 = 2;
const SYNC: u32 = 3;
const COMPOSITOR: u32 = 4;
const SHM: u32 = 5;
const XDG: u32 = 6;
const SUPERFICIE: u32 = 9;
const XDG_SUPERFICIE: u32 = 10;
const VENTANA: u32 = 11;
const ASIENTO: u32 = 12;
const PUNTERO: u32 = 13;
const TECLADO: u32 = 14;
const POOLES: [u32; 2] = [20, 22];
const BUFERES: [u32; 2] = [21, 23];
const TEMA: u32 = 43;
const AVISO: u32 = 100;

/// La letra (Roboto Mono, la de ancho fijo del kit) y el borde, en píxeles.
const LETRA: u32 = 13;
const BORDE: i32 = 8;
const COLUMNAS: usize = 80;
const FILAS: usize = 25;
/// Los 16 colores de una terminal, para fondo oscuro y para fondo claro.
const PALETA_OSCURA: [u32; 16] = [
    0xff1d1b20, 0xffd0605e, 0xff7fb36d, 0xffd9b85c, 0xff6f9bd6, 0xffb48ad9, 0xff5fb8b8, 0xffcac4cf, 0xff6b6670, 0xfff2898a, 0xffa6d68f, 0xfff0d58a,
    0xff9cbdf0, 0xffd0b0f0, 0xff8ad8d8, 0xffffffff,
];
const PALETA_CLARA: [u32; 16] = [
    0xff1d1b20, 0xffb3261e, 0xff2e7d32, 0xff8a6d00, 0xff1a5fb4, 0xff7b3fa0, 0xff00796b, 0xff605d66, 0xff49454f, 0xffd32f2f, 0xff388e3c, 0xffa07800,
    0xff1976d2, 0xff9c27b0, 0xff00897b, 0xff1d1b20,
];

// ---------------------------------------------------------------------------------------------------------------
// El buzón: el único canal donde espera el Shell. Por él llegan los trozos del compositor (sin marca) y lo que piden
// las órdenes por su puerta (con la marca de su trabajo).

struct Buzon {
    lectura: u32,
    /// Una llave para escribir en el buzón, de la que salen las puertas de los trabajos.
    puerta: u32,
    juntador: Juntador,
    peticiones: VecDeque<(u64, Vec<u8>)>,
    /// Si el compositor cerró (o nunca hubo).
    cerrado: bool,
}

impl Buzon {
    fn nuevo() -> (Buzon, u32) {
        let (lectura, suyo) = dashos::canal_crear();
        let puerta = dashos::llave_duplicar(suyo, dashos::ESCRIBIR | dashos::TRANSFERIR, 0).max(0) as u32;
        (Buzon { lectura, puerta, juntador: Juntador::new(), peticiones: VecDeque::new(), cerrado: false }, suyo)
    }

    /// Lee un mensaje, esperando como mucho `plazo_us`. Falso si no llegó nada.
    fn recoger(&mut self, plazo_us: u64) -> bool {
        let mut buf = [0u8; canal::MAX_CANAL];
        let mut ll = [0u32; canal::MAX_LLAVES];
        let (n, k, marca) = dashos::canal_leer_plazo(self.lectura, &mut buf, &mut ll, plazo_us.max(1));
        if n < 0 {
            if n == dashos::E_CERRADO {
                self.cerrado = true;
            }
            return false;
        }
        let n = n as usize;
        if marca != 0 {
            // De un trabajo: sus llaves no hacen falta.
            ll[..k].iter().for_each(|&l| {
                dashos::cerrar(l);
            });
            if self.peticiones.len() < 64 {
                self.peticiones.push_back((marca, Vec::from(&buf[..n])));
            }
            return true;
        }
        if n > 0 && buf[0] == TROZO_DATOS {
            if self.juntador.meter(&buf[1..n], &ll[..k]).is_err() {
                self.cerrado = true;
            }
        } else {
            ll[..k].iter().for_each(|&l| {
                dashos::cerrar(l);
            });
        }
        true
    }

    fn evento(&mut self) -> Option<(u32, u16, Vec<u8>)> {
        match self.juntador.siguiente() {
            Ok(Some(m)) => Some((m.objeto, m.orden, m.cuerpo)),
            Ok(None) => None,
            Err(_) => {
                self.cerrado = true;
                None
            }
        }
    }

    /// Manda al compositor. Con su cola llena se espera, recogiendo mientras lo que llegue (si no, él espera a que
    /// leamos y nosotros a que lea).
    fn mandar(&mut self, e: &Escritor) {
        let Ok(trozos) = canal::trocear(e) else { return };
        for t in trozos {
            for _ in 0..2000 {
                match dashos::canal_escribir(VENTANAS, &t.bytes, &t.llaves) {
                    0 => break,
                    dashos::E_LIMITE => {
                        if !self.recoger(1_000) {
                            dashos::dormir_us(500);
                        }
                    }
                    _ => {
                        self.cerrado = true;
                        return;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// La ventana.

struct Ventana {
    consola: Consola,
    fuente: Fuente,
    /// La celda: ancho, alto y línea base.
    celda: (i32, i32, i32),
    tema: Tema,
    tema_visto: (u32, bool),
    /// El búfer en uso (0 o 1), su memoria y su tamaño en píxeles.
    cual: usize,
    mem: u32,
    medida: (i32, i32),
    pedida: (i32, i32),
    configurada: bool,
    /// El aviso de frame que se espera (0, ninguno) y el siguiente número.
    esperando: u32,
    siguiente_aviso: u32,
    esperando_desde: u64,
    sucia: bool,
    /// Lo que se pintó en cada fila la última vez (para no repintar lo que no cambia).
    pintadas: Vec<u64>,
    teclado: Teclado,
    control: u8,
    mayusculas: u8,
    raton: (i32, i32),
    arrastre: Option<Punto>,
    pp: portapapeles_cliente::Portapapeles,
    con_portapapeles: bool,
    serie: u32,
}

/// Lo que pide una tecla al Shell.
enum Accion {
    Nada,
    Linea(String),
    Romper,
    FinDeEntrada,
    Cerrar,
}

/// El color de pantalla de un color de celda: el de serie, uno de la paleta (los 16 del tema, el cubo de 6×6×6 y los
/// 24 grises de xterm), uno directo, o el gris de la sugerencia.
fn color_de(c: consola::Color, de_serie: u32, fondo: u32, paleta: &[u32; 16]) -> u32 {
    match c >> 24 {
        1 => match (c & 0xff) as u32 {
            n @ 0..=15 => paleta[n as usize],
            n @ 16..=231 => {
                let nivel = |v: u32| if v == 0 { 0 } else { 55 + 40 * v };
                let k = n - 16;
                0xff00_0000 | nivel(k / 36) << 16 | nivel(k / 6 % 6) << 8 | nivel(k % 6)
            }
            n => {
                let g = 8 + 10 * (n - 232);
                0xff00_0000 | g << 16 | g << 8 | g
            }
        },
        2 => 0xff00_0000 | (c & 0xff_ffff),
        3 => kit::tema::mezcla(de_serie, fondo, 130),
        _ => de_serie,
    }
}

fn techo(x: f32) -> i32 {
    let i = x as i32;
    if (i as f32) < x { i + 1 } else { i }
}

impl Ventana {
    /// Conecta con el compositor y pide la ventana. `None` si no hay compositor.
    fn abrir(b: &mut Buzon, suyo: u32) -> Option<Ventana> {
        let mut intentos = 0;
        loop {
            let e = dashos::canal_escribir(VENTANAS, &[TROZO_CONECTAR], &[suyo]);
            if e == 0 {
                break;
            }
            intentos += 1;
            if e != dashos::E_LIMITE || intentos > 5000 {
                println!("  [shell] no se puede conectar al compositor: {}", e);
                dashos::cerrar(suyo);
                return None;
            }
            dashos::dormir_us(1_000);
        }
        let mut e = Escritor::new();
        e.empezar(1, wl_display::req::GET_REGISTRY).uint(REGISTRO).fin();
        e.empezar(1, wl_display::req::SYNC).uint(SYNC).fin();
        b.mandar(&e);
        let mut globales: Vec<(u32, String, u32)> = Vec::new();
        let hasta = dashos::ahora_us() + 10_000_000;
        'globales: loop {
            while let Some((o, orden, cuerpo)) = b.evento() {
                if o == REGISTRO && orden == wl_registry::ev::GLOBAL {
                    let args = leer_args(&wayland::tablas::WL_REGISTRY.events[0], &cuerpo, &mut core::iter::empty()).unwrap_or_default();
                    if args.len() == 3 {
                        globales.push((args[0].uint(), String::from(args[1].texto()), args[2].uint()));
                    }
                } else if o == SYNC && orden == wl_callback::ev::DONE {
                    break 'globales;
                }
            }
            if b.cerrado || dashos::ahora_us() > hasta {
                println!("  [shell] el compositor no contesta");
                return None;
            }
            b.recoger(100_000);
        }
        let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
        let (Some(gc), Some(gs), Some(gx), Some(ga)) = (global("wl_compositor"), global("wl_shm"), global("xdg_wm_base"), global("wl_seat")) else {
            println!("  [shell] faltan globales del compositor");
            return None;
        };
        let mut fuente = Fuente::mono();
        let cw = techo(fuente.avance('M', LETRA)).max(1);
        let ch = LETRA as i32 + 3;
        let base = (ch * 3 + 3) / 4;
        let mut e = Escritor::new();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gx).texto(Some("xdg_wm_base")).uint(6).uint(XDG).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
        e.empezar(ASIENTO, wl_seat::req::GET_KEYBOARD).uint(TECLADO).fin();
        e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
        if let Some(gt) = global("dashos_tema_v1") {
            e.empezar(REGISTRO, wl_registry::req::BIND).uint(gt).texto(Some("dashos_tema_v1")).uint(1).uint(TEMA).fin();
        }
        let mut pp = portapapeles_cliente::Portapapeles::default();
        let con_portapapeles = pp.enlazar(&mut e, &globales, REGISTRO, ASIENTO);
        e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUPERFICIE).fin();
        e.empezar(XDG, xdg_wm_base::req::GET_XDG_SURFACE).uint(XDG_SUPERFICIE).uint(SUPERFICIE).fin();
        e.empezar(XDG_SUPERFICIE, xdg_surface::req::GET_TOPLEVEL).uint(VENTANA).fin();
        e.empezar(VENTANA, xdg_toplevel::req::SET_TITLE).texto(Some("Shell")).fin();
        e.empezar(VENTANA, xdg_toplevel::req::SET_APP_ID).texto(Some("shell")).fin();
        e.empezar(VENTANA, xdg_toplevel::req::SET_MIN_SIZE).int(20 * cw + 2 * BORDE).int(4 * ch + 2 * BORDE).fin();
        e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
        b.mandar(&e);
        Some(Ventana {
            consola: Consola::nueva(COLUMNAS, FILAS),
            fuente,
            celda: (cw, ch, base),
            tema: Tema::OSCURO,
            tema_visto: (0xff67_50a4, true),
            cual: 0,
            mem: 0,
            medida: (0, 0),
            pedida: (0, 0),
            configurada: false,
            esperando: 0,
            siguiente_aviso: AVISO,
            esperando_desde: 0,
            sucia: true,
            pintadas: Vec::new(),
            teclado: Teclado::new(),
            control: 0,
            mayusculas: 0,
            raton: (0, 0),
            arrastre: None,
            pp,
            con_portapapeles,
            serie: 0,
        })
    }

    /// Un tamaño nuevo: memoria y búfer nuevos (en el otro sitio), y la consola se parte a su ancho.
    fn medir(&mut self, b: &mut Buzon, w: i32, h: i32) -> bool {
        let (cw, ch, _) = self.celda;
        let (w, h) = (w.clamp(20 * cw + 2 * BORDE, 3840), h.clamp(4 * ch + 2 * BORDE, 2160));
        if self.mem != 0 && (w, h) == self.medida {
            return true;
        }
        let tam = (w * h * 4) as usize;
        let otra = if self.mem == 0 { 0 } else { 1 - self.cual };
        let m = dashos::memoria_crear(tam);
        if (m as i32) <= 0 || dashos::memoria_proyectar(m, VA_PIXELES[otra], true) != 0 {
            if (m as i32) > 0 {
                dashos::cerrar(m);
            }
            println!("  [shell] sin memoria para una ventana de {}x{}", w, h);
            return false;
        }
        let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::TRANSFERIR, 0);
        let mut e = Escritor::new();
        if self.mem != 0 {
            e.empezar(BUFERES[self.cual], wl_buffer::req::DESTROY).fin();
            e.empezar(POOLES[self.cual], wl_shm_pool::req::DESTROY).fin();
        }
        e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(POOLES[otra]).llave(copia as u32).int(tam as i32).fin();
        e.empezar(POOLES[otra], wl_shm_pool::req::CREATE_BUFFER).uint(BUFERES[otra]).int(0).int(w).int(h).int(w * 4).uint(wayland::shm::XRGB8888).fin();
        b.mandar(&e);
        if self.mem != 0 {
            dashos::memoria_quitar(VA_PIXELES[self.cual]);
            dashos::cerrar(self.mem);
        }
        self.mem = m;
        self.cual = otra;
        self.medida = (w, h);
        self.consola.medir(((w - 2 * BORDE) / cw).max(1) as usize, ((h - 2 * BORDE) / ch).max(1) as usize);
        self.pintadas.clear();
        self.sucia = true;
        true
    }

    fn oscuro(&self) -> bool {
        let c = self.tema.superficie;
        ((c >> 16) & 0xff) + ((c >> 8) & 0xff) + (c & 0xff) < 384
    }

    /// Pinta las filas que hayan cambiado y las presenta. Como mucho una vez por cuadro.
    fn pintar(&mut self, b: &mut Buzon) {
        if !self.configurada || self.mem == 0 || !self.sucia {
            return;
        }
        // Si el aviso de frame no llega (la sesión está detrás), no se pinta: no hay quien lo vea. Pasado un rato se
        // pinta igual, por si se perdió.
        if self.esperando != 0 && dashos::ahora_us().saturating_sub(self.esperando_desde) < 1_000_000 {
            return;
        }
        self.sucia = false;
        let (w, h) = self.medida;
        let (cw, ch, base) = self.celda;
        // SAFETY: la memoria del búfer en uso, proyectada en `medir` con ese tamaño; solo la toca este hilo.
        let px = unsafe { core::slice::from_raw_parts_mut(VA_PIXELES[self.cual] as *mut u32, (w * h) as usize) };
        let mut l = Lienzo::new(px, w, h, w as usize);
        let (fondo, letra) = (self.tema.superficie, self.tema.sobre_superficie);
        let paleta = if self.oscuro() { &PALETA_OSCURA } else { &PALETA_CLARA };
        let vista = self.consola.vista();
        let filas = self.consola.filas();
        if self.pintadas.len() != filas + 1 {
            self.pintadas = alloc::vec![u64::MAX; filas + 1];
            l.borrar(Rect::new(0, 0, w, h), fondo);
        }
        let mut cambio = false;
        for y in 0..filas {
            let fila = vista.get(y);
            // Lo que se ve en la fila, resumido en un número.
            let mut suma = 0xcbf2_9ce4_8422_2325u64 ^ fondo as u64;
            let mut mezclar = |v: u64| suma = (suma ^ v).wrapping_mul(0x100_0000_01b3);
            if let Some(f) = fila {
                mezclar(f.cursor.map_or(u64::MAX, |c| c as u64));
                for (x, c) in f.celdas.iter().enumerate() {
                    mezclar(c.c as u64 | (c.estilo as u64) << 32 | (self.consola.seleccionada(f.linea, f.desde + x) as u64) << 40);
                    mezclar(c.letra as u64 | (c.fondo as u64) << 32);
                }
            }
            if self.pintadas[y] == suma {
                continue;
            }
            self.pintadas[y] = suma;
            cambio = true;
            let py = BORDE + y as i32 * ch;
            l.borrar(Rect::new(0, py, w, ch), fondo);
            let Some(f) = fila else { continue };
            for x in 0..self.consola.columnas() {
                let c = f.celdas.get(x);
                let px_ = BORDE + x as i32 * cw;
                // Los colores de la celda: los suyos, o los del tema; en negrita, los ocho primeros pasan a los vivos.
                let (mut color, mut detras, estilo) = match c {
                    Some(c) => {
                        let viva = |k: consola::Color| if c.estilo & consola::NEGRITA != 0 && (consola::paleta(0)..consola::paleta(8)).contains(&k) { k + 8 } else { k };
                        (color_de(viva(c.letra), letra, fondo, paleta), color_de(c.fondo, fondo, fondo, paleta), c.estilo)
                    }
                    None => (letra, fondo, 0),
                };
                if estilo & consola::INVERSO != 0 {
                    core::mem::swap(&mut color, &mut detras);
                }
                if estilo & consola::TENUE != 0 {
                    color = kit::tema::mezcla(color, detras, 110);
                }
                if f.cursor == Some(x) {
                    detras = self.tema.primario;
                    color = self.tema.sobre_primario;
                } else if c.is_some() && self.consola.seleccionada(f.linea, f.desde + x) {
                    detras = self.tema.contenedor_primario;
                    color = self.tema.sobre_contenedor_primario;
                }
                if detras != fondo {
                    l.borrar(Rect::new(px_, py, cw, ch), detras);
                }
                if let Some(c) = c.filter(|c| c.c != ' ') {
                    // Lo que la fuente no tiene sale como "?": así la caché de letras no crece sin fin.
                    let letra_ = if self.fuente.tiene(c.c) { c.c } else { '?' };
                    let mut buf = [0u8; 4];
                    self.fuente.dibujar(&mut l, px_, py + base, letra_.encode_utf8(&mut buf), LETRA, color);
                }
                if estilo & consola::SUBRAYADO != 0 && c.is_some() {
                    l.borrar(Rect::new(px_, py + base + 2, cw, 1), color);
                }
            }
        }
        if !cambio {
            return;
        }
        let mut e = Escritor::new();
        e.empezar(SUPERFICIE, wl_surface::req::ATTACH).uint(BUFERES[self.cual]).int(0).int(0).fin();
        e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(w).int(h).fin();
        e.empezar(SUPERFICIE, wl_surface::req::FRAME).uint(self.siguiente_aviso).fin();
        e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
        self.esperando = self.siguiente_aviso;
        self.esperando_desde = dashos::ahora_us();
        self.siguiente_aviso = if self.siguiente_aviso >= 0x00ff_ffff { AVISO } else { self.siguiente_aviso + 1 };
        b.mandar(&e);
    }

    fn punto_del_raton(&self) -> Option<Punto> {
        let (cw, ch, _) = self.celda;
        let col = ((self.raton.0 - BORDE).max(0) / cw) as usize;
        let fila = ((self.raton.1 - BORDE).max(0) / ch) as usize;
        self.consola.punto(col.min(self.consola.columnas() - 1), fila)
    }

    /// Un evento del compositor. Lo que pida al Shell, en la acción.
    fn evento(&mut self, b: &mut Buzon, o: u32, orden: u16, cuerpo: &[u8], registro: bool) -> Accion {
        let palabra = |i: usize| cuerpo.get(4 * i..4 * i + 4).map_or(0, |x| u32::from_le_bytes(x.try_into().unwrap()));
        match (o, orden) {
            (XDG, xdg_wm_base::ev::PING) => {
                let mut e = Escritor::new();
                e.empezar(XDG, xdg_wm_base::req::PONG).uint(palabra(0)).fin();
                b.mandar(&e);
            }
            (VENTANA, xdg_toplevel::ev::CONFIGURE) => self.pedida = (palabra(0) as i32, palabra(1) as i32),
            (VENTANA, xdg_toplevel::ev::CLOSE) => return Accion::Cerrar,
            (XDG_SUPERFICIE, xdg_surface::ev::CONFIGURE) => {
                let mut e = Escritor::new();
                e.empezar(XDG_SUPERFICIE, xdg_surface::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                b.mandar(&e);
                let (cw, ch, _) = self.celda;
                let (w, h) = if self.pedida.0 > 0 && self.pedida.1 > 0 {
                    self.pedida
                } else if self.configurada {
                    self.medida
                } else {
                    (COLUMNAS as i32 * cw + 2 * BORDE, FILAS as i32 * ch + 2 * BORDE)
                };
                let primera = !self.configurada;
                if self.medir(b, w, h) {
                    self.configurada = true;
                    self.esperando = 0;
                    if primera || registro {
                        println!(
                            "  [shell] ventana {}: {}x{} letras, {}x{} px{}",
                            if primera { "en pantalla" } else { "con otro tamaño" },
                            self.consola.columnas(),
                            self.consola.filas(),
                            self.medida.0,
                            self.medida.1,
                            if primera && self.con_portapapeles { ", con portapapeles" } else { "" }
                        );
                    }
                }
            }
            (aviso, wl_callback::ev::DONE) if aviso == self.esperando => self.esperando = 0,
            (TEMA, dashos_tema_v1::ev::ESQUEMA) => {
                kit::tema::recordar_evento_esquema(cuerpo);
            }
            (TEMA, dashos_tema_v1::ev::TEMA) => {
                let visto = (palabra(0) | 0xff00_0000, palabra(1) != 0);
                if visto != self.tema_visto || self.pintadas.is_empty() {
                    self.tema_visto = visto;
                    self.tema = Tema::del_sistema(visto.0, visto.1);
                    self.pintadas.clear();
                    self.sucia = true;
                }
            }
            (TECLADO, wl_keyboard::ev::KEYMAP) => {
                if let Some(l) = b.juntador.tomar_llave() {
                    dashos::cerrar(l);
                }
            }
            (TECLADO, wl_keyboard::ev::LEAVE) => {
                self.control = 0;
                self.mayusculas = 0;
            }
            (TECLADO, wl_keyboard::ev::KEY) => {
                self.serie = palabra(0);
                return self.tecla(b, palabra(2), palabra(3) == 1);
            }
            (PUNTERO, wl_pointer::ev::ENTER) => self.raton = ((palabra(2) as i32) >> 8, (palabra(3) as i32) >> 8),
            (PUNTERO, wl_pointer::ev::MOTION) => {
                self.raton = ((palabra(1) as i32) >> 8, (palabra(2) as i32) >> 8);
                if let (Some(desde), Some(aqui)) = (self.arrastre, self.punto_del_raton()) {
                    self.consola.seleccionar(desde, aqui);
                    self.sucia = true;
                }
            }
            (PUNTERO, wl_pointer::ev::BUTTON) => {
                let (serie, boton, pulsado) = (palabra(0), palabra(2), palabra(3) == 1);
                match (boton, pulsado) {
                    (272, true) => {
                        self.consola.quitar_seleccion();
                        self.arrastre = self.punto_del_raton();
                        self.sucia = true;
                    }
                    (272, false) => {
                        if self.arrastre.take().is_some() {
                            let texto = self.consola.texto_seleccionado();
                            if texto.is_empty() {
                                self.consola.quitar_seleccion();
                            } else {
                                // Al soltar, lo seleccionado es la selección primaria.
                                let mut e = Escritor::new();
                                self.pp.copiar(&mut e, true, texto, serie);
                                b.mandar(&e);
                            }
                            self.sucia = true;
                        }
                    }
                    (274, true) => {
                        let mut e = Escritor::new();
                        if self.pp.pegar(&mut e, true) {
                            b.mandar(&e);
                        }
                    }
                    _ => {}
                }
            }
            // La rueda (eje vertical): tres filas por paso.
            (PUNTERO, wl_pointer::ev::AXIS) if palabra(1) == 0 => {
                self.consola.desplazar(-3 * kit::controles::pasos_rueda(palabra(2)) as isize);
                self.sucia = true;
            }
            (1, wl_display::ev::ERROR) => {
                println!("  [shell] error de protocolo en el objeto {}: {}", palabra(0), palabra(1));
                return Accion::Cerrar;
            }
            (o, orden) => {
                let mut e = Escritor::new();
                if self.pp.evento(&mut e, o, orden, cuerpo, &mut || b.juntador.tomar_llave()) && !e.bytes.is_empty() {
                    b.mandar(&e);
                }
            }
        }
        Accion::Nada
    }

    fn tecla(&mut self, b: &mut Buzon, codigo: u32, pulsada: bool) -> Accion {
        match codigo {
            29 | 97 => {
                self.control = if pulsada { self.control.saturating_add(1) } else { self.control.saturating_sub(1) };
                return Accion::Nada;
            }
            42 | 54 => self.mayusculas = if pulsada { self.mayusculas.saturating_add(1) } else { self.mayusculas.saturating_sub(1) },
            _ => {}
        }
        // Ctrl+Mayús+C copia lo seleccionado y Ctrl+Mayús+V pega (Ctrl-C a secas es romper, como en el Amiga).
        if self.control > 0 && self.mayusculas > 0 && (codigo == 46 || codigo == 47) {
            if pulsada {
                let mut e = Escritor::new();
                if codigo == 46 {
                    let texto = self.consola.texto_seleccionado();
                    self.pp.copiar(&mut e, false, texto, self.serie);
                    b.mandar(&e);
                } else if self.pp.pegar(&mut e, false) {
                    b.mandar(&e);
                }
            }
            return Accion::Nada;
        }
        // RePág y AvPág: lo escrito, hacia atrás y hacia delante.
        if pulsada && (codigo == 104 || codigo == 109) {
            let paso = self.consola.filas().saturating_sub(1).max(1) as isize;
            self.consola.desplazar(if codigo == 104 { paso } else { -paso });
            self.sucia = true;
            return Accion::Nada;
        }
        let Some(t) = self.teclado.tecla(codigo, pulsada) else { return Accion::Nada };
        self.sucia = true;
        let ed = &mut self.consola.editor;
        if self.control > 0 {
            if let Tecla::Letra(c) = t {
                match c.to_ascii_lowercase() {
                    'c' => return Accion::Romper,
                    'd' | '\\' => return Accion::FinDeEntrada,
                    'a' => ed.inicio(),
                    'e' => ed.fin(),
                    'k' => ed.borrar_hasta_el_fin(),
                    'u' | 'x' => ed.borrar(),
                    'w' => ed.borrar_palabra(),
                    'l' => self.consola.limpiar(),
                    _ => {}
                }
                self.consola.al_final();
                return Accion::Nada;
            }
        }
        self.consola.al_final();
        self.consola.quitar_seleccion();
        let ed = &mut self.consola.editor;
        match t {
            Tecla::Letra(c) => ed.insertar(c),
            Tecla::Retroceso => ed.retroceso(),
            Tecla::Suprimir => ed.suprimir(),
            Tecla::Izquierda => ed.izquierda(),
            Tecla::Derecha => ed.derecha(),
            Tecla::Inicio => ed.inicio(),
            Tecla::Fin => ed.fin(),
            Tecla::Arriba | Tecla::AltArriba => ed.arriba(),
            Tecla::Abajo | Tecla::AltAbajo => ed.abajo(),
            Tecla::Intro => return Accion::Linea(self.consola.intro()),
            Tecla::Tabulador | Tecla::TabuladorAtras | Tecla::Escape => {}
        }
        Accion::Nada
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Los trabajos.

/// A dónde va lo que escribe una orden.
enum Destino {
    Consola,
    Fichero { asa: u32, pos: u64 },
    /// A la orden siguiente de la tubería.
    Tubo,
}

/// De dónde lee una orden.
enum Fuente_ {
    Teclado,
    /// Un fichero o la salida de la orden anterior, ya leídos: los bytes y por dónde va.
    Datos(Vec<u8>, usize),
    Nada,
}

struct Trabajo {
    /// Su marca en el buzón.
    id: u64,
    /// Su número en `sesiones`, para matarlo.
    pid: u64,
    nombre: String,
    /// Nuestro extremo de su canal de arranque: por él se le contesta, y se cierra cuando muere.
    canal: u32,
    primer_plano: bool,
    /// Su número de trabajo (`Status`, `Break`), si es de fondo.
    numero: usize,
    salida: Destino,
    entrada: Fuente_,
    /// Si espera una línea del teclado.
    leyendo: bool,
    /// Lo que queda de una línea tecleada que no cupo en una respuesta.
    resto: Vec<u8>,
    /// Cuándo se le dijo "roto".
    roto: Option<u64>,
}

/// Una tubería en marcha: sus órdenes, por cuál va, lo que escribió la anterior y el código de la última.
struct Tuberia {
    ordenes: Vec<OrdenLinea>,
    i: usize,
    tubo: Vec<u8>,
    rc: i32,
    r2: i32,
}

/// Un programa ya leído del disco.
struct Guardado {
    raiz: u8,
    camino: String,
    tamano: u64,
    fecha: u64,
    mem: u32,
}

/// Lo que es una orden que no es interna.
enum Hallada {
    Programa { ruta: String },
    Guion { ruta: String },
}

struct Shell {
    s: Sistema,
    int: Interprete,
    buzon: Buzon,
    ventana: Option<Ventana>,
    /// El Shell que nos lanzó, si nuestra consola es la suya.
    padre: Option<Cli>,
    registro: bool,
    /// La línea a medias que va al registro.
    para_registro: String,
    trabajos: Vec<Trabajo>,
    siguiente_id: u64,
    tuberia: Option<Tuberia>,
    /// Los cajones de `Path`, además del actual y de `C:`.
    camino: Vec<String>,
    guardados: Vec<Guardado>,
    /// Las líneas tecleadas que esperan.
    tecleadas: VecDeque<String>,
    /// La línea de `ORDEN`, hasta que le toca.
    orden_pendiente: Option<String>,
    /// Con `ORDEN`: se acaba al terminarla.
    de_una_orden: bool,
    indicador_puesto: bool,
    salir: Option<i32>,
    vigilado_us: u64,
    pulso_us: u64,
    columnas: usize,
}

/// El intérprete necesita el mundo mientras el Shell lo tiene prestado: esto le da lo justo.
struct MundoShell<'a> {
    shell: &'a mut Shell,
}

impl Mundo for MundoShell<'_> {
    fn decir(&mut self, texto: &str) {
        self.shell.di_linea(texto);
    }

    fn leer_guion(&mut self, ruta: &str) -> Result<String, i32> {
        // Como una orden: tal cual, o en `S:` si no lleva ruta.
        let d = match self.shell.s.leer_entero(ruta, orden::MAX_ENTERO) {
            Err(a::NO_EXISTE) if !ruta.contains([':', '/']) => self.shell.s.leer_entero(&format!("S:{}", ruta), orden::MAX_ENTERO),
            x => x,
        }?;
        Ok(String::from_utf8_lossy(&d).into_owned())
    }

    fn existe(&mut self, ruta: &str) -> bool {
        self.shell.s.mirar(ruta).is_ok()
    }

    fn env(&mut self, nombre: &str) -> Option<String> {
        self.shell.env(nombre)
    }
}

fn nombre_de_variable(n: &str) -> bool {
    !n.is_empty() && n.len() <= 30 && n.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '-')
}

impl Shell {
    // --- La consola ---

    /// Escribe en la consola (la ventana, o la del Shell que nos lanzó) y, con `REGISTRO`, en el registro del sistema.
    fn di(&mut self, texto: &str) {
        if texto.is_empty() {
            return;
        }
        if self.registro {
            for c in texto.chars() {
                if c == '\n' {
                    println!("  [shell] {}", self.para_registro);
                    self.para_registro.clear();
                } else if self.para_registro.len() < 200 && (c as u32) >= 0x20 {
                    self.para_registro.push(c);
                }
            }
        }
        if let Some(v) = &mut self.ventana {
            v.consola.escribir(texto);
            v.sucia = true;
        } else if let Some(c) = &mut self.padre {
            let mut resto = texto;
            while !resto.is_empty() {
                let mut n = resto.len().min(p::POR_ESCRITURA);
                while !resto.is_char_boundary(n) {
                    n -= 1;
                }
                let mut m = [0u8; 4 + p::POR_ESCRITURA];
                m[0..4].copy_from_slice(&p::SALIDA.to_le_bytes());
                m[4..4 + n].copy_from_slice(&resto.as_bytes()[..n]);
                if c.pedir(p::ESCRIBIR, &m[..4 + n], 60_000_000).is_none() {
                    // Quien nos lanzó ya no está.
                    self.salir = Some(RC_FALLO);
                    return;
                }
                resto = &resto[n..];
            }
        } else if !self.registro {
            dashos::escribir(texto);
        }
    }

    /// Una línea entera, empezando en una línea nueva si la de ahora está a medias.
    fn di_linea(&mut self, texto: &str) {
        if self.ventana.as_ref().is_some_and(|v| !v.consola.al_principio()) {
            self.di("\n");
        }
        self.di(texto);
        self.di("\n");
    }

    fn env(&mut self, nombre: &str) -> Option<String> {
        if !nombre_de_variable(nombre) {
            return None;
        }
        let d = self.s.leer_entero(&format!("ENV:{}", nombre), 4096).ok()?;
        Some(String::from_utf8_lossy(&d).trim_end_matches(['\n', '\r']).to_string())
    }

    /// Hace algo con el intérprete, dándole el Shell como su mundo.
    fn con_interprete<R>(&mut self, f: impl FnOnce(&mut Interprete, &mut MundoShell) -> R) -> R {
        let mut int = core::mem::take(&mut self.int);
        let r = f(&mut int, &mut MundoShell { shell: self });
        self.int = int;
        r
    }

    // --- Las órdenes ---

    /// Dónde está la orden `nombre`: con ruta, solo ahí; sin ella, en el cajón actual, en los de `Path` y en `C:`.
    fn buscar(&mut self, nombre: &str) -> Result<Hallada, i32> {
        let mut sitios: Vec<String> = Vec::new();
        if nombre.contains([':', '/']) {
            sitios.push(String::from(nombre));
        } else {
            sitios.push(String::from(nombre));
            sitios.extend(self.camino.iter().map(|c| ruta::unir(c, nombre)));
            sitios.push(format!("C:{}", nombre));
            // Las órdenes de base van juntas en `C:Ordenes`, que hace la que se le diga: un programa suelto con ese
            // nombre en el camino va antes.
            if ordenes::base::es_de_base(nombre) {
                sitios.push(String::from("C:Ordenes"));
            }
        }
        let mut error = a::NO_EXISTE;
        for sitio in sitios {
            let r = match self.s.ruta(&sitio) {
                Ok(r) => r,
                Err(e) => {
                    error = e;
                    continue;
                }
            };
            match self.s.mirar(&r) {
                Ok(d) if d.cajon() => error = a::ES_CAJON,
                // Un guion (el bit "s" del Amiga) o un programa (con permiso de ejecutar).
                Ok(d) if d.bits & 4 != 0 => return Ok(Hallada::Guion { ruta: r }),
                Ok(d) if d.modo & 0o111 != 0 => return Ok(Hallada::Programa { ruta: r }),
                Ok(_) => error = a::PROTEGIDO,
                Err(e) if e != a::NO_EXISTE && error == a::NO_EXISTE => error = e,
                Err(_) => {}
            }
        }
        Err(error)
    }

    /// El programa de `ruta`, leído a memoria (o de los guardados): una llave de su memoria para `sesiones` (solo de
    /// leer), lo que mide, y dónde estaba (la raíz y el cajón, para su `PROGDIR:`).
    fn cargar(&mut self, ruta_: &str) -> Result<(u32, usize, u8, String), i32> {
        let (raiz, camino, d) = self.s.donde(ruta_)?;
        let cajon = camino.rfind('/').map_or("", |i| &camino[..i]).to_string();
        let copia = |mem: u32| -> Result<u32, i32> {
            let c = dashos::llave_duplicar(mem, dashos::LEER | dashos::TRANSFERIR, 0);
            if c > 0 { Ok(c as u32) } else { Err(a::SIN_SITIO) }
        };
        if let Some(g) = self.guardados.iter().find(|g| g.raiz == raiz && g.camino == camino && g.tamano == d.tamano && g.fecha == d.fecha) {
            return Ok((copia(g.mem)?, d.tamano as usize, raiz, cajon));
        }
        let tam = d.tamano as usize;
        if tam < 64 || tam > MAX_PROGRAMA {
            return Err(a::MAL_FORMADO);
        }
        let x = self.s.fs.orden(a::BUSCAR, raiz as u32, 0, 0, camino.as_bytes());
        if x.e != a::BIEN {
            return Err(x.e);
        }
        let mem = dashos::memoria_crear(tam);
        if (mem as i32) <= 0 || dashos::memoria_proyectar(mem, VA_PROGRAMA, true) != 0 {
            if (mem as i32) > 0 {
                dashos::cerrar(mem);
            }
            self.s.cerrar(x.asa);
            return Err(a::SIN_SITIO);
        }
        // SAFETY: recién proyectada aquí, `tam` bytes; se quita antes de salir.
        let destino = unsafe { core::slice::from_raw_parts_mut(VA_PROGRAMA as *mut u8, tam) };
        let mut hecho = 0;
        let mut r = Ok(());
        while hecho < tam {
            match self.s.leer(x.asa, hecho as u64, &mut destino[hecho..]) {
                Ok(0) => {
                    r = Err(a::ERROR_DEL_DISCO);
                    break;
                }
                Ok(n) => hecho += n,
                Err(e) => {
                    r = Err(e);
                    break;
                }
            }
        }
        // Un programa de DashOS es un ELF; lo demás no se manda a `sesiones`.
        if r.is_ok() && &destino[..4] != b"\x7fELF" {
            r = Err(a::MAL_FORMADO);
        }
        dashos::memoria_quitar(VA_PROGRAMA);
        self.s.cerrar(x.asa);
        if let Err(e) = r {
            dashos::cerrar(mem);
            return Err(e);
        }
        if self.guardados.len() >= MAX_GUARDADOS {
            let viejo = self.guardados.remove(0);
            dashos::cerrar(viejo.mem);
        }
        self.guardados.push(Guardado { raiz, camino, tamano: d.tamano, fecha: d.fecha, mem });
        Ok((copia(mem)?, tam, raiz, cajon))
    }

    /// Lanza el programa de `ruta` con `args`, como un ayudante con la ficha del Shell. El trabajo queda apuntado.
    fn lanzar(&mut self, ruta_: &str, nombre: &str, args: &str, primer_plano: bool, entrada: Fuente_, salida: Destino, servido: bool) -> Result<u64, i32> {
        if self.trabajos.len() >= MAX_TRABAJOS {
            return Err(a::SIN_SITIO);
        }
        if self.s.actual.len() + nombre.len() + args.len() > p::MAX_ARRANQUE {
            return Err(asig::NO_CABE);
        }
        let (mem, tam, raiz, cajon) = self.cargar(ruta_)?;
        let corto = ruta::ultimo(ruta_);
        let id = self.siguiente_id;
        self.siguiente_id += 1;
        // El canal de arranque, con todo ya en su cola: el cajón actual, los argumentos y, si se le atiende, la puerta.
        let (nuestro, suyo) = dashos::canal_crear();
        let puerta = if servido { dashos::llave_duplicar(self.buzon.puerta, dashos::ESCRIBIR | dashos::TRANSFERIR | dashos::UNA_VEZ, id) } else { 0 };
        let cerrar_todo = |e: i32| -> i32 {
            for l in [mem, nuestro, suyo] {
                dashos::cerrar(l);
            }
            e
        };
        if servido && puerta <= 0 {
            return Err(cerrar_todo(a::SIN_SITIO));
        }
        let mut m = [0u8; 256];
        let interactiva = primer_plano && matches!(entrada, Fuente_::Teclado);
        let marcas = if servido { p::SERVIDO } else { 0 } | if interactiva { 0 } else { p::ENTRADA_NO_INTERACTIVA };
        m[0..4].copy_from_slice(&p::ARRANQUE.to_le_bytes());
        m[4..8].copy_from_slice(&marcas.to_le_bytes());
        m[8..12].copy_from_slice(&(self.s.actual.len() as u32).to_le_bytes());
        m[12..16].copy_from_slice(&(args.len() as u32).to_le_bytes());
        m[16..18].copy_from_slice(&(self.columnas.min(u16::MAX as usize) as u16).to_le_bytes());
        m[20..24].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
        let llaves = [puerta as u32];
        if dashos::canal_escribir(nuestro, &m[..p::LARGO_ARRANQUE], if servido { &llaves } else { &[] }) != 0 {
            if puerta > 0 {
                dashos::cerrar(puerta as u32);
            }
            return Err(cerrar_todo(a::SIN_SITIO));
        }
        let mut datos = Vec::from(self.s.actual.as_bytes());
        datos.extend_from_slice(nombre.as_bytes());
        datos.extend_from_slice(args.as_bytes());
        for trozo in datos.chunks(p::POR_DATOS) {
            m[0..4].copy_from_slice(&p::DATOS.to_le_bytes());
            m[4..4 + trozo.len()].copy_from_slice(trozo);
            if dashos::canal_escribir(nuestro, &m[..4 + trozo.len()], &[]) != 0 {
                return Err(cerrar_todo(asig::NO_CABE));
            }
        }
        // A `sesiones`: el ELF en memoria, de dónde salió y su canal de arranque.
        // Un cajón demasiado hondo no cabe en la petición: el programa se lanza igual, sin `PROGDIR:`.
        let (raiz, cajon) = if cajon.len() <= ps::MAX_CAJON_AYUDANTE { (raiz, cajon.as_str()) } else { (ps::SIN_RAIZ, "") };
        let mut pet = Vec::with_capacity(12 + corto.len() + cajon.len());
        pet.extend_from_slice(&ps::LANZAR_AYUDANTE.to_le_bytes());
        pet.extend_from_slice(&(tam as u32).to_le_bytes());
        pet.push(raiz);
        pet.push(corto.len().min(255) as u8);
        pet.extend_from_slice(&(cajon.len() as u16).to_le_bytes());
        pet.extend_from_slice(corto.as_bytes());
        pet.extend_from_slice(cajon.as_bytes());
        if pet.len() > 250 {
            return Err(cerrar_todo(asig::NO_CABE));
        }
        let (lectura, respuesta) = dashos::canal_crear();
        let mut r = [0u8; 12];
        let n = if dashos::canal_escribir(SESIONES, &pet, &[respuesta, mem, suyo]) == 0 {
            dashos::canal_leer_plazo(lectura, &mut r, &mut [], PLAZO_SESIONES_US).0
        } else {
            for l in [respuesta, mem, suyo] {
                dashos::cerrar(l);
            }
            -1
        };
        dashos::cerrar(lectura);
        let estado = if n == 12 { i32::from_le_bytes(r[0..4].try_into().unwrap()) } else { a::ERROR_DEL_DISCO };
        if estado != 0 {
            dashos::cerrar(nuestro);
            return Err(estado);
        }
        let pid = u64::from_le_bytes(r[4..12].try_into().unwrap());
        let numero = if primer_plano { 0 } else { (1..).find(|n| !self.trabajos.iter().any(|t| t.numero == *n)).unwrap_or(1) };
        self.trabajos.push(Trabajo { id, pid, nombre: String::from(nombre), canal: nuestro, primer_plano, numero, salida, entrada, leyendo: false, resto: Vec::new(), roto: None });
        Ok(pid)
    }

    /// Lleva lo que escribe una orden a su destino.
    fn a_destino(&mut self, destino: &mut Destino, datos: &[u8]) -> i32 {
        match destino {
            Destino::Consola => {
                self.di(&String::from_utf8_lossy(datos));
                0
            }
            Destino::Fichero { asa, pos } => match self.s.escribir(*asa, *pos, datos) {
                Ok(()) => {
                    *pos += datos.len() as u64;
                    0
                }
                Err(e) => e,
            },
            Destino::Tubo => {
                let Some(t) = &mut self.tuberia else { return 0 };
                if t.tubo.len() + datos.len() > MAX_TUBO {
                    return a::LLENO;
                }
                t.tubo.extend_from_slice(datos);
                0
            }
        }
    }

    fn cerrar_destino(&mut self, destino: Destino) {
        if let Destino::Fichero { asa, .. } = destino {
            self.s.cerrar(asa);
        }
    }

    /// Abre la entrada y la salida de una orden según sus redirecciones. Si no se puede, lo dice.
    fn abrir_redirecciones(&mut self, o: &OrdenLinea, entrada_de_serie: Fuente_, salida_de_serie: Destino) -> Result<(Fuente_, Destino), i32> {
        let entrada = match &o.entrada {
            Some(f) => match self.s.leer_entero(f, MAX_TUBO) {
                Ok(d) => Fuente_::Datos(d, 0),
                Err(e) => {
                    self.di_linea(&format!("No se puede leer de {}: {}", f, orden::texto_error(e)));
                    self.cerrar_destino(salida_de_serie);
                    return Err(e);
                }
            },
            None => entrada_de_serie,
        };
        let salida = match &o.salida {
            Some(f) => match self.s.abrir_para_escribir(f, o.anadir) {
                Ok((asa, pos)) => {
                    self.cerrar_destino(salida_de_serie);
                    Destino::Fichero { asa, pos }
                }
                Err(e) => {
                    self.di_linea(&format!("No se puede escribir en {}: {}", f, orden::texto_error(e)));
                    self.cerrar_destino(salida_de_serie);
                    return Err(e);
                }
            },
            None => salida_de_serie,
        };
        Ok((entrada, salida))
    }

    /// Sigue con la tubería en marcha: lanza la orden que toca, o acaba y avisa al intérprete.
    fn seguir_tuberia(&mut self) {
        loop {
            let Some(t) = &mut self.tuberia else { return };
            if t.i >= t.ordenes.len() {
                let (rc, r2) = (t.rc, t.r2);
                self.tuberia = None;
                self.con_interprete(|i, m| i.acabada(m, rc, r2));
                return;
            }
            let o = t.ordenes[t.i].clone();
            let (primera, ultima, sola) = (t.i == 0, t.i + 1 == t.ordenes.len(), t.ordenes.len() == 1);
            let entrada = if primera { Fuente_::Teclado } else { Fuente_::Datos(core::mem::take(&mut t.tubo), 0) };
            let salida = if ultima { Destino::Consola } else { Destino::Tubo };
            let (rc, r2) = match self.una_orden(&o, entrada, salida, sola) {
                // Lanzada: se sigue cuando acabe. O era un guion, y el intérprete ya está en él.
                None => return,
                Some(x) => x,
            };
            if let Some(t) = &mut self.tuberia {
                t.rc = rc;
                t.r2 = r2;
                t.i += 1;
            }
        }
    }

    /// Hace una orden en primer plano. `Some` con su código si ya está hecha (era interna, o no se pudo lanzar); `None`
    /// si quedó en marcha (o era un guion).
    fn una_orden(&mut self, o: &OrdenLinea, entrada: Fuente_, salida: Destino, sola: bool) -> Option<(i32, i32)> {
        let (entrada, mut salida) = match self.abrir_redirecciones(o, entrada, salida) {
            Ok(x) => x,
            Err(e) => return Some((RC_FALLO, e)),
        };
        let mut texto = String::new();
        if let Some(rc) = self.interna(o, &mut texto) {
            let e = self.a_destino(&mut salida, texto.as_bytes());
            self.cerrar_destino(salida);
            return Some(if e != 0 { (RC_FALLO, e) } else { (rc, 0) });
        }
        match self.buscar(&o.nombre) {
            Ok(Hallada::Guion { ruta }) if sola => {
                self.cerrar_destino(salida);
                let leido = self.s.leer_entero(&ruta, orden::MAX_ENTERO);
                match leido {
                    Ok(d) => {
                        let texto = String::from_utf8_lossy(&d).into_owned();
                        self.tuberia = None;
                        self.con_interprete(|i, m| i.guion(m, &ruta, &texto, &o.args));
                        None
                    }
                    Err(e) => {
                        self.di_linea(&format!("{}: {}", o.nombre, orden::texto_error(e)));
                        Some((RC_FALLO, e))
                    }
                }
            }
            Ok(Hallada::Guion { .. }) => {
                self.cerrar_destino(salida);
                self.di_linea(&format!("{}: un guion no va en una tubería", o.nombre));
                Some((RC_FALLO, 0))
            }
            Ok(Hallada::Programa { ruta }) => {
                // Si no se lanza, la salida (un fichero abierto) se cierra dentro: aquí ya no se tiene.
                let asa = if let Destino::Fichero { asa, .. } = &salida { Some(*asa) } else { None };
                match self.lanzar(&ruta, &o.nombre, &o.args, true, entrada, salida, true) {
                    Ok(_) => None,
                    Err(e) => {
                        if let Some(asa) = asa {
                            self.s.cerrar(asa);
                        }
                        let porque = match e {
                            a::MAL_FORMADO => "no es un programa de DashOS",
                            ps::PROTEGIDO => "sesiones no deja lanzarlo",
                            e => orden::texto_error(e),
                        };
                        self.di_linea(&format!("{}: no se puede lanzar: {}", o.nombre, porque));
                        Some((RC_FALLO, e))
                    }
                }
            }
            Err(e) => {
                self.cerrar_destino(salida);
                let porque = match e {
                    a::NO_EXISTE => "orden desconocida",
                    a::PROTEGIDO => "no tiene permiso de ejecutar",
                    e => orden::texto_error(e),
                };
                self.di_linea(&format!("{}: {}", o.nombre, porque));
                Some((RC_ERROR, e))
            }
        }
    }

    // --- Las órdenes internas ---

    /// Las órdenes que tocan el estado del Shell. `None` si `o` no es una. Lo que escriben, en `sal`.
    fn interna(&mut self, o: &OrdenLinea, sal: &mut String) -> Option<i32> {
        let n = o.nombre.to_ascii_lowercase();
        let internas = ["set", "unset", "get", "setenv", "getenv", "unsetenv", "alias", "unalias", "path", "which", "run", "status", "break", "why", "cls", "newshell"];
        if !internas.contains(&n.as_str()) {
            return None;
        }
        let plantilla_ = match n.as_str() {
            "set" | "setenv" | "alias" => "NAME,STRING/F",
            "unset" | "unsetenv" | "unalias" | "get" | "getenv" => "NAME/A",
            "path" => "PATH/M,ADD/S,SHOW/S,RESET/S",
            "which" => "FILE/A",
            "run" => "COMMAND/F",
            "break" => "PROCESS/N",
            _ => "",
        };
        let args = match plantilla::leer(plantilla_, &o.args) {
            Ok(x) => x,
            Err(plantilla::Error::Ayuda) => {
                sal.push_str(&format!("{}\n", plantilla_));
                return Some(RC_BIEN);
            }
            Err(e) => {
                self.di_linea(&format!("{}: {}", o.nombre, e));
                return Some(RC_FALLO);
            }
        };
        let nombre = args.texto("NAME").unwrap_or("");
        let valor = args.texto("STRING").unwrap_or("");
        Some(match n.as_str() {
            "set" if nombre.is_empty() => {
                for (k, v) in &self.int.locales {
                    sal.push_str(&format!("{:<16} {}\n", k, v));
                }
                RC_BIEN
            }
            "set" => {
                // `Set nombre=valor` también vale.
                let (nombre, valor) = match nombre.split_once('=') {
                    Some((k, v)) if valor.is_empty() => (k, v),
                    _ => (nombre, valor),
                };
                if !nombre_de_variable(nombre) || self.int.locales.len() >= 256 || valor.len() > 1024 {
                    self.di_linea("Set: ese nombre o ese valor no valen");
                    return Some(RC_ERROR);
                }
                self.int.poner_local(nombre, valor);
                RC_BIEN
            }
            "unset" => {
                self.int.locales.retain(|(k, _)| !k.eq_ignore_ascii_case(nombre));
                RC_BIEN
            }
            "get" => match self.int.locales.iter().find(|(k, _)| k.eq_ignore_ascii_case(nombre)) {
                Some((_, v)) => {
                    sal.push_str(&format!("{}\n", v));
                    RC_BIEN
                }
                None => RC_AVISO,
            },
            "alias" if nombre.is_empty() => {
                for (k, v) in &self.int.alias {
                    sal.push_str(&format!("{:<16} {}\n", k, v));
                }
                RC_BIEN
            }
            "alias" => {
                self.int.alias.retain(|(k, _)| !k.eq_ignore_ascii_case(nombre));
                if !valor.is_empty() && self.int.alias.len() < 64 {
                    self.int.alias.push((String::from(nombre), String::from(valor)));
                }
                RC_BIEN
            }
            "unalias" => {
                self.int.alias.retain(|(k, _)| !k.eq_ignore_ascii_case(nombre));
                RC_BIEN
            }
            "setenv" if nombre.is_empty() => match self.s.listar("ENV:") {
                Ok(entradas) => {
                    for e in entradas.iter().filter(|e| !e.datos.cajon()) {
                        let v = self.env(&e.nombre).unwrap_or_default();
                        sal.push_str(&format!("{:<16} {}\n", e.nombre, v));
                    }
                    RC_BIEN
                }
                Err(e) => {
                    self.di_linea(&format!("SetEnv: ENV: {}", orden::texto_error(e)));
                    RC_ERROR
                }
            },
            "setenv" => {
                if !nombre_de_variable(nombre) {
                    self.di_linea("SetEnv: ese nombre no vale");
                    return Some(RC_ERROR);
                }
                let f = format!("ENV:{}", nombre);
                let r = self.s.abrir_para_escribir(&f, false).and_then(|(asa, _)| {
                    let r = self.s.escribir(asa, 0, valor.as_bytes());
                    self.s.cerrar(asa);
                    r
                });
                match r {
                    Ok(()) => RC_BIEN,
                    Err(e) => {
                        self.di_linea(&format!("SetEnv: {}: {}", f, orden::texto_error(e)));
                        RC_ERROR
                    }
                }
            }
            "getenv" => match self.env(nombre) {
                Some(v) => {
                    sal.push_str(&format!("{}\n", v));
                    RC_BIEN
                }
                None => RC_AVISO,
            },
            "unsetenv" => {
                if nombre_de_variable(nombre) {
                    let _ = self.s.borrar(&format!("ENV:{}", nombre));
                }
                RC_BIEN
            }
            "path" => {
                let mut rc = RC_BIEN;
                if args.si("RESET") {
                    self.camino.clear();
                }
                let nuevos = args.varios("PATH");
                if !nuevos.is_empty() && !args.si("ADD") {
                    self.camino.clear();
                }
                for c in nuevos {
                    match self.s.ruta(c).and_then(|r| self.s.mirar(&r).map(|d| (r, d))) {
                        Ok((r, d)) if d.cajon() => {
                            if !self.camino.iter().any(|x| x.eq_ignore_ascii_case(&r)) && self.camino.len() < 32 {
                                self.camino.push(r);
                            }
                        }
                        Ok(_) => {
                            self.di_linea(&format!("Path: {}: no es un cajón", c));
                            rc = RC_ERROR;
                        }
                        Err(e) => {
                            self.di_linea(&format!("Path: {}: {}", c, orden::texto_error(e)));
                            rc = RC_ERROR;
                        }
                    }
                }
                if nuevos.is_empty() || args.si("SHOW") {
                    sal.push_str("El cajón actual\n");
                    for c in &self.camino {
                        sal.push_str(&format!("{}\n", c));
                    }
                    sal.push_str("C:\n");
                }
                rc
            }
            "which" => {
                let f = args.texto("FILE").unwrap_or("");
                if internas.contains(&f.to_ascii_lowercase().as_str()) || ["if", "else", "endif", "skip", "lab", "endskip", "quit", "failat", "execute", "endshell", "endcli"].contains(&f.to_ascii_lowercase().as_str()) {
                    sal.push_str("INTERNA\n");
                    RC_BIEN
                } else {
                    match self.buscar(f) {
                        Ok(Hallada::Programa { ruta }) | Ok(Hallada::Guion { ruta }) => {
                            // Dónde está de verdad (qué capa de `C:`), si se sabe.
                            let capa = self.capa_de(&ruta).unwrap_or(ruta);
                            sal.push_str(&format!("{}\n", capa));
                            RC_BIEN
                        }
                        Err(_) => RC_AVISO,
                    }
                }
            }
            "run" => self.run(args.texto("COMMAND").unwrap_or(""), sal),
            "status" => {
                for t in &self.trabajos {
                    let que = if t.primer_plano { String::from("primer plano") } else { format!("[{}] de fondo", t.numero) };
                    sal.push_str(&format!("Proceso {:>6}: {} ({}{})\n", t.pid, t.nombre, que, if t.roto.is_some() { ", roto" } else { "" }));
                }
                if self.trabajos.is_empty() {
                    sal.push_str("No hay trabajos en marcha\n");
                }
                RC_BIEN
            }
            "break" => {
                let cual = args.numero("PROCESS");
                let ahora = dashos::ahora_us();
                let mut alguno = false;
                for t in self.trabajos.iter_mut().filter(|t| !t.primer_plano && cual.is_none_or(|c| c == t.numero as i64 || c == t.pid as i64)) {
                    t.roto.get_or_insert(ahora);
                    alguno = true;
                }
                if alguno {
                    RC_BIEN
                } else {
                    self.di_linea("Break: no hay ese trabajo");
                    RC_AVISO
                }
            }
            "why" => {
                let (rc, r2) = (self.int.rc, self.int.resultado2);
                if rc == 0 {
                    sal.push_str("La última orden acabó bien\n");
                } else if r2 != 0 {
                    sal.push_str(&format!("La última orden acabó con el código {}: {} ({})\n", rc, orden::texto_error(r2), r2));
                } else {
                    sal.push_str(&format!("La última orden acabó con el código {}\n", rc));
                }
                RC_BIEN
            }
            "cls" => {
                if let Some(v) = &mut self.ventana {
                    v.consola.limpiar();
                    v.sucia = true;
                }
                RC_BIEN
            }
            "newshell" => match self.buscar("SYS:System/Shell") {
                Ok(Hallada::Programa { ruta }) => match self.lanzar(&ruta, "Shell", &o.args, false, Fuente_::Nada, Destino::Consola, false) {
                    Ok(_) => {
                        // No se le atiende: no es un trabajo de este Shell.
                        if let Some(t) = self.trabajos.pop() {
                            dashos::cerrar(t.canal);
                        }
                        RC_BIEN
                    }
                    Err(e) => {
                        self.di_linea(&format!("NewShell: {}", orden::texto_error(e)));
                        RC_FALLO
                    }
                },
                _ => {
                    self.di_linea("NewShell: no está SYS:System/Shell");
                    RC_FALLO
                }
            },
            _ => RC_BIEN,
        })
    }

    /// La ruta de `ruta_` con el nombre del volumen donde está de verdad (`C:Dir` es `SYS:C/Dir`).
    fn capa_de(&mut self, ruta_: &str) -> Option<String> {
        let (raiz, camino, _) = self.s.donde(ruta_).ok()?;
        let tabla = self.s.tabla.clone();
        let vol = tabla.volumenes().find(|l| l.destinos.strip_prefix(b"@").and_then(|d| core::str::from_utf8(d).ok()).and_then(|d| d.parse::<u8>().ok()) == Some(raiz))?;
        Some(format!("{}:{}", core::str::from_utf8(vol.nombre).ok()?, camino))
    }

    /// `Run`: lanza una orden de fondo. Un programa, tal cual; lo demás (una interna, un guion, una tubería), en otro
    /// Shell sin ventana, que escribe en esta consola.
    fn run(&mut self, linea_: &str, sal: &mut String) -> i32 {
        let ordenes = match shell::linea::partir(linea_) {
            Ok(o) if !o.is_empty() => o,
            Ok(_) => {
                self.di_linea("Run: falta la orden");
                return RC_FALLO;
            }
            Err(e) => {
                self.di_linea(&format!("Run: {}", e));
                return RC_FALLO;
            }
        };
        let directa = if ordenes.len() == 1 && self.interna_o_control(&ordenes[0].nombre).is_none() {
            match self.buscar(&ordenes[0].nombre) {
                Ok(Hallada::Programa { ruta }) => Some(ruta),
                Ok(Hallada::Guion { .. }) => None,
                Err(e) => {
                    self.di_linea(&format!("Run: {}: {}", ordenes[0].nombre, if e == a::NO_EXISTE { "orden desconocida" } else { orden::texto_error(e) }));
                    return RC_ERROR;
                }
            }
        } else {
            None
        };
        let r = match directa {
            Some(ruta) => {
                let o = &ordenes[0];
                match self.abrir_redirecciones(o, Fuente_::Nada, Destino::Consola) {
                    Ok((entrada, salida)) => {
                        let asa = if let Destino::Fichero { asa, .. } = &salida { Some(*asa) } else { None };
                        let r = self.lanzar(&ruta, &o.nombre, &o.args, false, entrada, salida, true);
                        if let (Err(_), Some(asa)) = (&r, asa) {
                            self.s.cerrar(asa);
                        }
                        r
                    }
                    Err(e) => Err(e),
                }
            }
            None => match self.buscar("SYS:System/Shell") {
                Ok(Hallada::Programa { ruta }) => {
                    let nombre = format!("Shell ({})", ordenes[0].nombre);
                    self.lanzar(&ruta, &nombre, &format!("SINARRANQUE ORDEN {}", linea_), false, Fuente_::Nada, Destino::Consola, true)
                }
                _ => Err(a::NO_EXISTE),
            },
        };
        match r {
            Ok(pid) => {
                let numero = self.trabajos.last().map_or(0, |t| t.numero);
                sal.push_str(&format!("[{}] {} (proceso {})\n", numero, ordenes[0].nombre, pid));
                RC_BIEN
            }
            Err(e) => {
                self.di_linea(&format!("Run: no se puede lanzar: {}", orden::texto_error(e)));
                RC_FALLO
            }
        }
    }

    fn interna_o_control(&self, nombre: &str) -> Option<()> {
        let n = nombre.to_ascii_lowercase();
        [
            "set", "unset", "get", "setenv", "getenv", "unsetenv", "alias", "unalias", "path", "which", "run", "status", "break", "why", "cls", "newshell", "if",
            "else", "endif", "skip", "lab", "endskip", "quit", "failat", "execute", "endshell", "endcli",
        ]
        .contains(&n.as_str())
        .then_some(())
    }

    // --- Lo que piden los trabajos ---

    fn contestar(&self, k: usize, estado: i32, datos: &[u8]) {
        let t = &self.trabajos[k];
        let mut r = [0u8; 256];
        let l = datos.len().min(248);
        r[0..4].copy_from_slice(&estado.to_le_bytes());
        r[4..8].copy_from_slice(&(if t.roto.is_some() { p::ROTO } else { 0 }).to_le_bytes());
        r[8..8 + l].copy_from_slice(&datos[..l]);
        dashos::canal_escribir(t.canal, &r[..8 + l], &[]);
    }

    /// Una línea (o un trozo) de la entrada del trabajo `k`, si hay: `None` si tiene que esperar al teclado.
    fn entrada_de(&mut self, k: usize) -> Option<Vec<u8>> {
        let t = &mut self.trabajos[k];
        if !t.resto.is_empty() {
            let n = t.resto.len().min(p::POR_LECTURA);
            return Some(t.resto.drain(..n).collect());
        }
        match &mut t.entrada {
            Fuente_::Nada => Some(Vec::new()),
            Fuente_::Datos(d, pos) => {
                let resto = &d[*pos..];
                let n = resto.iter().position(|&c| c == b'\n').map_or(resto.len(), |i| i + 1).min(p::POR_LECTURA);
                *pos += n;
                Some(Vec::from(&resto[..n]))
            }
            Fuente_::Teclado if !t.primer_plano => Some(Vec::new()),
            Fuente_::Teclado => {
                // Sin ventana no hay teclado: es el final.
                if self.ventana.is_none() {
                    return Some(Vec::new());
                }
                let mut l = self.tecleadas.pop_front()?.into_bytes();
                l.push(b'\n');
                let t = &mut self.trabajos[k];
                t.resto = l;
                let n = t.resto.len().min(p::POR_LECTURA);
                Some(t.resto.drain(..n).collect())
            }
        }
    }

    fn peticion(&mut self, marca: u64, m: &[u8]) {
        let Some(k) = self.trabajos.iter().position(|t| t.id == marca) else { return };
        if m.len() < 4 {
            return;
        }
        let cuerpo = &m[4..];
        match u32::from_le_bytes(m[0..4].try_into().unwrap()) {
            p::ESCRIBIR if cuerpo.len() >= 4 => {
                let mut destino = core::mem::replace(&mut self.trabajos[k].salida, Destino::Consola);
                // Los errores van siempre a la consola, aunque la salida esté redirigida.
                let estado = if u32::from_le_bytes(cuerpo[0..4].try_into().unwrap()) == p::ERRORES {
                    self.a_destino(&mut Destino::Consola, &cuerpo[4..])
                } else {
                    self.a_destino(&mut destino, &cuerpo[4..])
                };
                self.trabajos[k].salida = destino;
                self.contestar(k, estado, &[]);
            }
            p::LEER => match self.entrada_de(k) {
                Some(d) => self.contestar(k, d.len() as i32, &d),
                None => self.trabajos[k].leyendo = true,
            },
            p::PULSO => self.contestar(k, 0, &[]),
            p::CD => {
                let estado = match core::str::from_utf8(cuerpo).ok().map(|c| (c, self.s.mirar(c))) {
                    Some((c, Ok(d))) if d.cajon() && c.contains(':') => {
                        self.s.actual = String::from(c);
                        0
                    }
                    Some((_, Ok(_))) => a::NO_ES_CAJON,
                    Some((_, Err(e))) => e,
                    None => a::MAL_FORMADO,
                };
                self.contestar(k, estado, &[]);
            }
            p::ASIGNAR if cuerpo.len() >= 4 => {
                let estado = match Cambio::desde(u32::from_le_bytes(cuerpo[0..4].try_into().unwrap())) {
                    Some(c) => asig::asignar_en_sesiones(SESIONES, c, &cuerpo[4..]),
                    None => a::MAL_FORMADO,
                };
                if estado == 0 {
                    self.s.refrescar();
                }
                self.contestar(k, estado, &[]);
            }
            p::SALIR if cuerpo.len() >= 8 => {
                let rc = i32::from_le_bytes(cuerpo[0..4].try_into().unwrap());
                let r2 = i32::from_le_bytes(cuerpo[4..8].try_into().unwrap());
                self.acabar(k, rc, r2);
            }
            _ => self.contestar(k, a::MAL_FORMADO, &[]),
        }
    }

    /// El trabajo `k` acabó: se suelta lo suyo y se sigue con lo que esperaba por él.
    fn acabar(&mut self, k: usize, rc: i32, r2: i32) {
        let t = self.trabajos.remove(k);
        dashos::cerrar(t.canal);
        self.cerrar_destino(t.salida);
        if !t.primer_plano {
            if rc != 0 || t.roto.is_some() {
                self.di_linea(&format!("[{}] {} acabó con el código {}", t.numero, t.nombre, rc));
            }
            return;
        }
        if let Some(tu) = &mut self.tuberia {
            tu.rc = rc;
            tu.r2 = r2;
            tu.i += 1;
            self.seguir_tuberia();
        } else {
            self.con_interprete(|i, m| i.acabada(m, rc, r2));
        }
    }

    /// Mira los trabajos: el que murió sin avisar (su canal se cerró) y el que no acaba tras romperlo.
    fn vigilar(&mut self) {
        let ahora = dashos::ahora_us();
        let mut k = 0;
        while k < self.trabajos.len() {
            let t = &self.trabajos[k];
            let mut b = [0u8; 256];
            let mut ll = [0u32; 8];
            let (n, kk, _) = dashos::canal_leer_plazo(t.canal, &mut b, &mut ll, 1);
            ll[..kk.min(8)].iter().for_each(|&l| {
                dashos::cerrar(l);
            });
            if n == dashos::E_CERRADO {
                let nombre = t.nombre.clone();
                if t.roto.is_none() {
                    self.di_linea(&format!("{}: acabó sin avisar (un fallo del programa)", nombre));
                }
                self.acabar(k, RC_FALLO, 0);
                continue;
            }
            if t.roto.is_some_and(|r| ahora.saturating_sub(r) > PLAZO_ROTO_US) {
                // No hace caso: `sesiones` lo mata (es del mismo usuario).
                let (pid, nombre) = (t.pid, t.nombre.clone());
                let estado = matar(pid);
                self.di_linea(&format!("{}: no acababa y se ha matado ({})", nombre, estado));
                self.acabar(k, RC_FALLO, 0);
                continue;
            }
            k += 1;
        }
    }

    /// Ctrl-C: rompe lo que hay en primer plano (una orden, una tubería, un guion).
    fn romper(&mut self) {
        let ahora = dashos::ahora_us();
        let mut alguno = false;
        for k in 0..self.trabajos.len() {
            if self.trabajos[k].primer_plano {
                self.trabajos[k].roto.get_or_insert(ahora);
                alguno = true;
                if core::mem::take(&mut self.trabajos[k].leyendo) {
                    self.contestar(k, 0, &[]);
                }
            }
        }
        // Lo que quedara de la tubería y del guion, fuera.
        if let Some(t) = &mut self.tuberia {
            t.ordenes.truncate(t.i + 1);
        }
        let habia = self.con_interprete(|i, m| i.romper(m));
        if alguno && !habia {
            // Una orden sin intérprete por medio (no pasa hoy): que se diga igual.
            self.di_linea("***Break");
        }
        if !alguno && !habia {
            // Nada en marcha: la línea que se estaba tecleando se deja.
            if let Some(v) = &mut self.ventana {
                v.consola.editor.borrar();
                v.sucia = true;
            }
        }
    }

    /// Final de la entrada (Ctrl-D): a la orden que lee; sin ninguna y sin nada tecleado, el Shell acaba.
    fn fin_de_entrada(&mut self) {
        if let Some(k) = self.trabajos.iter().position(|t| t.primer_plano && t.leyendo) {
            self.trabajos[k].leyendo = false;
            self.contestar(k, 0, &[]);
        } else if !self.int.ocupado() && self.trabajos.iter().all(|t| !t.primer_plano) && self.ventana.as_ref().is_some_and(|v| v.consola.editor.vacio()) {
            self.salir = Some(self.int.rc);
        }
    }

    /// Una línea tecleada: a la orden que la espera, o a la cola.
    fn linea_tecleada(&mut self, l: String) {
        if self.tecleadas.len() < 256 {
            self.tecleadas.push_back(l);
        }
        self.indicador_puesto = false;
        if let Some(k) = self.trabajos.iter().position(|t| t.primer_plano && t.leyendo)
            && let Some(d) = self.entrada_de(k)
        {
            self.trabajos[k].leyendo = false;
            self.contestar(k, d.len() as i32, &d);
        }
    }

    // --- El bucle ---

    /// Si hay algo en primer plano por lo que esperar.
    fn esperando_a_una_orden(&self) -> bool {
        self.trabajos.iter().any(|t| t.primer_plano)
    }

    /// Avanza el intérprete mientras pueda, sin pasar de unos milisegundos (la ventana tiene que seguir viva).
    fn avanzar(&mut self) {
        let hasta = dashos::ahora_us() + 4_000;
        for _ in 0..200 {
            if self.salir.is_some() || self.esperando_a_una_orden() || self.tuberia.is_some() {
                return;
            }
            match self.con_interprete(|i, m| i.paso(m)) {
                Paso::Nada => {}
                Paso::Ejecutar(ordenes) => {
                    self.tuberia = Some(Tuberia { ordenes, i: 0, tubo: Vec::new(), rc: 0, r2: 0 });
                    self.seguir_tuberia();
                }
                Paso::Fin(rc) => {
                    self.salir = Some(rc);
                    return;
                }
                Paso::Pedir => {
                    if let Some(l) = self.orden_pendiente.take() {
                        self.con_interprete(|i, _| i.linea(&l));
                    } else if self.de_una_orden || self.ventana.is_none() {
                        self.salir = Some(self.int.rc);
                        return;
                    } else if let Some(l) = self.tecleadas.pop_front() {
                        self.con_interprete(|i, _| i.linea(&l));
                    } else {
                        if !self.indicador_puesto {
                            self.indicador_puesto = true;
                            let indicador = format!("1.{}> ", self.s.actual);
                            if self.ventana.as_ref().is_some_and(|v| !v.consola.al_principio()) {
                                self.di("\n");
                            }
                            // El indicador no va al registro: solo a la ventana.
                            if let Some(v) = &mut self.ventana {
                                v.consola.escribir(&indicador);
                                v.sucia = true;
                            }
                        }
                        return;
                    }
                }
            }
            if dashos::ahora_us() > hasta {
                return;
            }
        }
    }

    fn bucle(&mut self) -> i32 {
        loop {
            self.avanzar();
            if let Some(rc) = self.salir {
                return rc;
            }
            // Cuánto esperar: nada si el intérprete tiene más que hacer; poco con trabajos (hay que vigilarlos).
            let con_prisa = self.int.ocupado() && !self.esperando_a_una_orden() && self.tuberia.is_none();
            let plazo = if con_prisa {
                1
            } else if !self.trabajos.is_empty() || self.ventana.as_ref().is_some_and(|v| v.pp.ocupado() || (v.sucia && v.esperando != 0)) {
                20_000
            } else {
                500_000
            };
            let mut llego = self.buzon.recoger(plazo);
            for _ in 0..32 {
                if !self.buzon.recoger(1) {
                    break;
                }
                llego = true;
            }
            // Lo de la ventana.
            while let Some((o, orden_, cuerpo)) = self.buzon.evento() {
                let Some(v) = &mut self.ventana else { continue };
                match v.evento(&mut self.buzon, o, orden_, &cuerpo, self.registro) {
                    Accion::Nada => {}
                    Accion::Linea(l) => self.linea_tecleada(l),
                    Accion::Romper => self.romper(),
                    Accion::FinDeEntrada => self.fin_de_entrada(),
                    Accion::Cerrar => return self.int.rc,
                }
            }
            if self.ventana.is_some() && self.buzon.cerrado {
                println!("  [shell] el compositor cerró la conexión");
                return RC_FALLO;
            }
            // Lo pegado, como si se tecleara.
            let pegado = self.ventana.as_mut().and_then(|v| v.pp.mover());
            if let Some(datos) = pegado {
                let texto = String::from_utf8_lossy(&datos).into_owned();
                let lineas = self.ventana.as_mut().map(|v| {
                    v.sucia = true;
                    v.consola.al_final();
                    v.consola.pegar(&texto)
                });
                for l in lineas.unwrap_or_default() {
                    self.linea_tecleada(l);
                }
            }
            // Lo de los trabajos.
            while let Some((marca, m)) = self.buzon.peticiones.pop_front() {
                self.peticion(marca, &m);
            }
            let ahora = dashos::ahora_us();
            if !llego && !self.trabajos.is_empty() && ahora.saturating_sub(self.vigilado_us) > 100_000 {
                self.vigilado_us = ahora;
                self.vigilar();
            }
            // Sin ventana: de vez en cuando se pregunta a quien nos lanzó si nos han roto.
            if self.padre.is_some() && ahora.saturating_sub(self.pulso_us) > 100_000 {
                self.pulso_us = ahora;
                let c = self.padre.as_mut().unwrap();
                if c.pedir(p::PULSO, &[], 60_000_000).is_none() {
                    return RC_FALLO;
                }
                if c.roto {
                    self.romper();
                    // Lo que quede en marcha se mata al salir; el código, el de roto.
                    for t in &self.trabajos {
                        matar(t.pid);
                    }
                    return p::RC_ROTO;
                }
            }
            if let Some(v) = &mut self.ventana {
                self.columnas = v.consola.columnas();
                v.pintar(&mut self.buzon);
            }
        }
    }
}

/// Pide a `sesiones` que mate el proceso `pid` (uno nuestro): su estado.
fn matar(pid: u64) -> i32 {
    let mut m = [0u8; 12];
    m[0..4].copy_from_slice(&ps::MATAR.to_le_bytes());
    m[4..12].copy_from_slice(&pid.to_le_bytes());
    let (lectura, suyo) = dashos::canal_crear();
    let mut r = [0u8; 4];
    let n = if dashos::canal_escribir(SESIONES, &m, &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], PLAZO_SESIONES_US).0 } else { -1 };
    dashos::cerrar(lectura);
    if n == 4 { i32::from_le_bytes(r) } else { i32::MIN }
}

fn principal() -> u64 {
    let arr = orden::arranque();
    let Some(mut s) = Sistema::nuevo() else {
        println!("  [shell] sin el servicio de archivos");
        return RC_FALLO as u64;
    };
    let (mut args, mut padre, mut columnas) = (String::new(), None, COLUMNAS);
    if let Some(x) = arr {
        if !x.actual.is_empty() {
            s.actual = x.actual;
        }
        args = x.args;
        padre = x.puerta.map(Cli::nuevo);
        if x.columnas > 0 {
            columnas = x.columnas;
        }
    }
    let (mut buzon, suyo) = Buzon::nuevo();
    let ventana = if padre.is_some() {
        dashos::cerrar(suyo);
        None
    } else {
        Ventana::abrir(&mut buzon, suyo)
    };
    let mut sh = Shell {
        s,
        int: Interprete::nuevo(),
        buzon,
        ventana,
        padre,
        registro: false,
        para_registro: String::new(),
        trabajos: Vec::new(),
        siguiente_id: 1,
        tuberia: None,
        camino: Vec::new(),
        guardados: Vec::new(),
        tecleadas: VecDeque::new(),
        orden_pendiente: None,
        de_una_orden: false,
        indicador_puesto: false,
        salir: None,
        vigilado_us: 0,
        pulso_us: dashos::ahora_us(),
        columnas,
    };
    let (mut desde, mut sin_arranque) = (None, false);
    match plantilla::leer("FROM/K,REGISTRO/S,SINARRANQUE/S,ORDEN/K/F", &args) {
        Ok(x) => {
            sh.registro = x.si("REGISTRO");
            sin_arranque = x.si("SINARRANQUE");
            desde = x.texto("FROM").map(String::from);
            if let Some(o) = x.texto("ORDEN") {
                sh.orden_pendiente = Some(String::from(o));
                sh.de_una_orden = true;
            }
        }
        Err(e) => sh.di_linea(&format!("Shell: {}", e)),
    }
    if sh.ventana.is_none() && sh.padre.is_none() {
        println!("  [shell] sin ventana: no hay compositor");
        if !sh.de_una_orden {
            return RC_FALLO as u64;
        }
    }
    // El guion de arranque: el de `FROM`, o `S:Shell-Startup` (el del usuario tapa al del sistema, por capas).
    if !sin_arranque {
        let guion = desde.unwrap_or_else(|| String::from("S:Shell-Startup"));
        match sh.s.leer_entero(&guion, orden::MAX_ENTERO) {
            Ok(d) => {
                let texto = String::from_utf8_lossy(&d).into_owned();
                sh.con_interprete(|i, m| i.guion(m, &guion, &texto, ""));
            }
            Err(a::NO_EXISTE) if guion == "S:Shell-Startup" => {}
            Err(e) => sh.di_linea(&format!("Shell: {}: {}", guion, orden::texto_error(e))),
        }
    }
    let rc = sh.bucle();
    if sh.registro {
        println!("  [shell] acaba con el código {}", rc);
    }
    if let Some(c) = &sh.padre {
        c.salir(rc, sh.int.resultado2);
    }
    rc as u64
}
