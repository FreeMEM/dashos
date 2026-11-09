//! Un cliente de Wayland de prueba (W7), común a `ventana-a` y `ventana-b`: se conecta al compositor por el canal
//! "ventanas" (llave 1, marcada por `inicio`), pide una ventana con `xdg-shell`, la pinta en memoria compartida y la
//! anima 30 cuadros, cada uno cuando el compositor avisa con `frame`. Después se queda con la ventana abierta.

use alloc::vec::Vec;
use dashos::println;
use wayland::canal::{self, Juntador, TROZO_CONECTAR, TROZO_DATOS};
use wayland::tablas::{
    self, wl_callback, wl_compositor, wl_display, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool,
    wl_surface, xdg_surface, xdg_toplevel, xdg_wm_base,
};
use wayland::{Escritor, leer_args};

const VENTANAS: u32 = 1;
const VA: usize = 0x40_0000_0000;
pub const CUADROS: u32 = 30;

// Los objetos de este cliente.
const REGISTRO: u32 = 2;
const SYNC: u32 = 3;
const COMPOSITOR: u32 = 4;
const SHM: u32 = 5;
const XDG: u32 = 6;
const POOL: u32 = 7;
const BUFER: u32 = 8;
const SUPERFICIE: u32 = 9;
const XDG_SUPERFICIE: u32 = 10;
const VENTANA: u32 = 11;
const ASIENTO: u32 = 12;
const PUNTERO: u32 = 13;
const TECLADO: u32 = 14;
/// Los avisos de frame, uno por cuadro desde aquí.
const AVISO: u32 = 100;

pub struct Cliente {
    respuesta: u32,
    juntador: Juntador,
}

impl Cliente {
    fn mandar(&mut self, e: &Escritor) {
        for t in canal::trocear(e).expect("mensajes propios bien formados") {
            // Con la cola llena, se espera un poco y se reintenta.
            for _ in 0..1000 {
                if dashos::canal_escribir(VENTANAS, &t.bytes, &t.llaves) == 0 {
                    break;
                }
                dormir();
            }
        }
    }

    /// El siguiente evento: objeto, orden y cuerpo.
    fn evento(&mut self) -> Option<(u32, u16, Vec<u8>)> {
        loop {
            if let Ok(Some(m)) = self.juntador.siguiente() {
                return Some((m.objeto, m.orden, m.cuerpo));
            }
            let mut buf = [0u8; canal::MAX_CANAL];
            let mut ll = [0u32; canal::MAX_LLAVES];
            let (n, k) = dashos::canal_leer(self.respuesta, &mut buf, &mut ll);
            if n <= 0 {
                return None;
            }
            if buf[0] == TROZO_DATOS {
                self.juntador.meter(&buf[1..n as usize], &ll[..k]).ok()?;
            }
        }
    }
}

fn dormir() {
    let (a, b) = dashos::canal_crear();
    let t = dashos::temporizador_crear(b, 1_000, 0);
    if t > 0 {
        let mut buf = [0u8; 8];
        let mut ll = [0u32; 1];
        dashos::canal_leer(a, &mut buf, &mut ll);
        dashos::cerrar(t as u32);
    }
    dashos::cerrar(a);
    dashos::cerrar(b);
}

/// Pinta el cuadro `n` de una ventana de `ancho` × `alto` en `px` con `pintar(x, y, n) -> ARGB`.
fn pintar(px: &mut [u32], ancho: u32, alto: u32, n: u32, pintar: &impl Fn(u32, u32, u32) -> u32) {
    for y in 0..alto {
        for x in 0..ancho {
            px[(y * ancho + x) as usize] = pintar(x, y, n);
        }
    }
}

/// Abre una ventana de `ancho` × `alto` con `formato` (de `wl_shm`) y la anima. Devuelve si todo fue como debía.
pub fn ventana(
    nombre: &'static str,
    titulo: &str,
    ancho: u32,
    alto: u32,
    formato: u32,
    color: impl Fn(u32, u32, u32) -> u32,
) -> bool {
    // La conexión: un canal propio, cuyo otro extremo se le da al compositor.
    let (mio, suyo) = dashos::canal_crear();
    if dashos::canal_escribir(VENTANAS, &[TROZO_CONECTAR], &[suyo]) != 0 {
        println!("  [{}] no se puede conectar", nombre);
        return false;
    }
    let mut c = Cliente { respuesta: mio, juntador: Juntador::new() };

    // Los globales, hasta el sync.
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::GET_REGISTRY).uint(REGISTRO).fin();
    e.empezar(1, wl_display::req::SYNC).uint(SYNC).fin();
    c.mandar(&e);
    let mut globales = Vec::new();
    loop {
        let Some((o, orden, cuerpo)) = c.evento() else { return false };
        if o == REGISTRO && orden == wl_registry::ev::GLOBAL {
            let a = leer_args(&tablas::WL_REGISTRY.events[0], &cuerpo, &mut core::iter::empty()).unwrap_or_default();
            if a.len() == 3 {
                globales.push((a[0].uint(), alloc::string::String::from(a[1].texto()), a[2].uint()));
            }
        } else if o == SYNC && orden == wl_callback::ev::DONE {
            break;
        }
    }
    let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
    let (Some(gc), Some(gs), Some(gx), Some(ga)) =
        (global("wl_compositor"), global("wl_shm"), global("xdg_wm_base"), global("wl_seat"))
    else {
        println!("  [{}] faltan globales", nombre);
        return false;
    };

    // La memoria: se pinta aquí y el compositor la lee con una copia de solo lectura de la llave.
    let tamano = (ancho * alto * 4) as usize;
    let m = dashos::memoria_crear(tamano);
    if (m as i32) <= 0 || dashos::memoria_proyectar(m as u32, VA, true) != 0 {
        println!("  [{}] sin memoria", nombre);
        return false;
    }
    let px = unsafe { core::slice::from_raw_parts_mut(VA as *mut u32, (ancho * alto) as usize) };
    let copia = dashos::llave_duplicar(m as u32, dashos::LEER | dashos::TRANSFERIR, 0);

    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gx).texto(Some("xdg_wm_base")).uint(6).uint(XDG).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
    e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
    e.empezar(ASIENTO, wl_seat::req::GET_KEYBOARD).uint(TECLADO).fin();
    e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(POOL).llave(copia as u32).int(tamano as i32).fin();
    e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER)
        .uint(BUFER)
        .int(0)
        .int(ancho as i32)
        .int(alto as i32)
        .int(ancho as i32 * 4)
        .uint(formato)
        .fin();
    e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUPERFICIE).fin();
    e.empezar(XDG, xdg_wm_base::req::GET_XDG_SURFACE).uint(XDG_SUPERFICIE).uint(SUPERFICIE).fin();
    e.empezar(XDG_SUPERFICIE, xdg_surface::req::GET_TOPLEVEL).uint(VENTANA).fin();
    e.empezar(VENTANA, xdg_toplevel::req::SET_TITLE).texto(Some(titulo)).fin();
    e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
    c.mandar(&e);

    // Un solo bucle: su configure y sus cuadros, el ping, y la entrada (W8).
    let mut cuadro = 0;
    let mut configurada = false;
    let (mut raton, mut tecla) = (false, false);
    loop {
        let Some((o, orden, cuerpo)) = c.evento() else { return cuadro >= CUADROS };
        let palabra = |i: usize| u32::from_le_bytes(cuerpo[4 * i..4 * i + 4].try_into().unwrap());
        match (o, orden) {
            (XDG, xdg_wm_base::ev::PING) => {
                let mut e = Escritor::new();
                e.empezar(XDG, xdg_wm_base::req::PONG).uint(palabra(0)).fin();
                c.mandar(&e);
            }
            (XDG_SUPERFICIE, xdg_surface::ev::CONFIGURE) if !configurada => {
                configurada = true;
                pintar(px, ancho, alto, 0, &color);
                let mut e = Escritor::new();
                e.empezar(XDG_SUPERFICIE, xdg_surface::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                e.empezar(SUPERFICIE, wl_surface::req::ATTACH).uint(BUFER).int(0).int(0).fin();
                e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(ancho as i32).int(alto as i32).fin();
                e.empezar(SUPERFICIE, wl_surface::req::FRAME).uint(AVISO).fin();
                e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
                c.mandar(&e);
            }
            (aviso, wl_callback::ev::DONE) if cuadro < CUADROS && aviso == AVISO + cuadro => {
                cuadro += 1;
                if cuadro == CUADROS {
                    println!("  [{}] {} cuadros: bien", nombre, CUADROS);
                    continue;
                }
                pintar(px, ancho, alto, cuadro, &color);
                let mut e = Escritor::new();
                e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(ancho as i32).int(alto as i32).fin();
                e.empezar(SUPERFICIE, wl_surface::req::FRAME).uint(AVISO + cuadro).fin();
                e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
                c.mandar(&e);
            }
            // El mapa de teclas llega con una llave de memoria: sin mapa todavía, se suelta.
            (TECLADO, wl_keyboard::ev::KEYMAP) => {
                if let Some(l) = c.juntador.tomar_llave() {
                    dashos::cerrar(l);
                }
            }
            (PUNTERO, wl_pointer::ev::ENTER) if !raton => {
                raton = true;
                println!("  [ventana] ratón dentro ({})", nombre);
            }
            // La a (tecla 30 de evdev), pulsada.
            (TECLADO, wl_keyboard::ev::KEY) if !tecla && palabra(2) == 30 && palabra(3) == 1 => {
                tecla = true;
                println!("  [ventana] tecla 30 recibida ({})", nombre);
            }
            (1, wl_display::ev::ERROR) => {
                println!("  [{}] error de protocolo", nombre);
                return false;
            }
            _ => {}
        }
    }
}
