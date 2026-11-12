//! C2 de S10c: el panel rápido. Se abre y se cierra con Super+A (el compositor solo se lo dice a él, por el evento
//! `panel` de `dashos_tema_v1`) en una capa arriba a la derecha, y no se lleva el teclado de otra ventana sin un clic
//! (teclado "a demanda"). De momento, dos mosaicos: el modo oscuro y bloquear (como Super+L, por la petición `bloquear`
//! de `dashos_tema_v1`, que el compositor solo le acepta a él). No pone el tema él: se lo pide al centro de control
//! ("modo oscuro" o "modo claro" por su canal, que solo se lo acepta a `panel`), que es quien lo guarda. Sigue el tema
//! del sistema, como lo demás.
//!
//! Y el volumen general: "−" y "+" de 10 en 10 %, por la orden `GENERAL` del servicio de sonido (solo se la acepta a
//! él).
//!
//! Llaves: 1 el canal "ventanas" (con el poder de escritorio, para la capa), 2 el canal "centro", 3 el canal "sonido".

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
#[allow(dead_code)]
mod ventana_cliente;

use dashos::println;
use kit::lienzo::{Lienzo, Rect};
use kit::tema::Tema;
use kit::texto::Fuente;
use ventana_cliente::*;
use wayland::tablas::{dashos_tema_v1, wl_compositor, wl_display, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface, zwlr_layer_shell_v1, zwlr_layer_surface_v1};
use wayland::Escritor;

dashos::programa!(principal);
dashos::monton!();

const CENTRO: u32 = 2;
const SONIDO: u32 = 3;
const GENERAL: u32 = 13;
const CAPAS: u32 = 20;
const CAPA: u32 = 21;
const TEMA: u32 = 22;
const ANCHO: i32 = 300;
const ALTO: i32 = 288;
/// La distancia a los bordes de arriba y de la derecha.
const MARGEN: i32 = 8;
/// El mosaico del modo oscuro, dentro del panel.
const MOSAICO: Rect = Rect { x: 16, y: 64, w: 268, h: 64 };
/// El de bloquear, debajo.
const BLOQUEAR: Rect = Rect { x: 16, y: 136, w: 268, h: 64 };
/// El del volumen: la mitad izquierda baja, la derecha sube.
const VOLUMEN: Rect = Rect { x: 16, y: 208, w: 268, h: 64 };
const BOTON_IZQUIERDO: u32 = 0x110;

fn pintar(px: &mut [u32], t: &Tema, oscuro: bool, volumen: u32, fuente: &mut Fuente, titulo: &mut Fuente) {
    // Material 3: el panel sobre el contenedor de superficie, con esquinas de 28 (fuera, transparente: el búfer es
    // ARGB); los mosaicos con esquinas de 20, el encendido en el primario y los demás en el contenedor alto.
    px.fill(0);
    let mut l = Lienzo::new(px, ANCHO, ALTO, ANCHO as usize);
    l.redondeado(Rect::new(0, 0, ANCHO, ALTO), 28, t.contenedor());
    titulo.dibujar(&mut l, 24, 22, "Panel rápido", 22, t.sobre_superficie);
    let mosaico = |l: &mut Lienzo, fuente: &mut Fuente, r: Rect, encendido: bool, arriba: &str, abajo: &str| {
        let (fondo, texto, suave) = if encendido {
            (t.primario, t.sobre_primario, t.sobre_primario)
        } else {
            (t.contenedor_alto(), t.sobre_superficie, t.sobre_variante_superficie)
        };
        l.redondeado(r, 20, fondo);
        fuente.dibujar(l, r.x + 20, r.y + 12, arriba, 16, texto);
        fuente.dibujar(l, r.x + 20, r.y + 36, abajo, 14, suave);
    };
    mosaico(&mut l, fuente, MOSAICO, oscuro, "Modo oscuro", if oscuro { "Encendido" } else { "Apagado" });
    mosaico(&mut l, fuente, BLOQUEAR, false, "Bloquear", "Super+L");
    mosaico(&mut l, fuente, VOLUMEN, false, "Volumen", &alloc::format!("{} %", volumen));
    // Un guion de los de siempre: la fuente no tiene el signo menos (U+2212).
    fuente.dibujar(&mut l, VOLUMEN.x + VOLUMEN.w - 94, VOLUMEN.y + 16, "-", 28, t.sobre_superficie);
    fuente.dibujar(&mut l, VOLUMEN.x + VOLUMEN.w - 44, VOLUMEN.y + 16, "+", 28, t.sobre_superficie);
}

/// El volumen general al servicio de sonido (sin respuesta).
fn pedir_volumen(por_ciento: u32) {
    let p: [u32; 2] = [GENERAL, (por_ciento as f32 / 100.0).to_bits()];
    let b: alloc::vec::Vec<u8> = p.iter().flat_map(|w| w.to_le_bytes()).collect();
    dashos::canal_escribir(SONIDO, &b, &[]);
}

/// Pide el modo al centro de control; su estado (0 si lo puso).
fn pedir_modo(oscuro: bool) -> i32 {
    let (lectura, escritura) = dashos::canal_crear();
    let pedido: &[u8] = if oscuro { b"modo oscuro" } else { b"modo claro" };
    let mut r = [0u8; 4];
    let e = if dashos::canal_escribir(CENTRO, pedido, &[escritura]) == 0 && dashos::canal_leer_plazo(lectura, &mut r, &mut [], 5_000_000).0 == 4 {
        i32::from_le_bytes(r)
    } else {
        i32::MIN
    };
    dashos::cerrar(lectura);
    e
}

fn principal() -> u64 {
    let (mut fuente, mut titulo) = (Fuente::regular(), Fuente::medium());
    let Some(mut c) = Cliente::conectar() else {
        println!("  [panel] sin compositor");
        return 1;
    };
    let Some(globales) = c.globales() else { return 2 };
    let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
    let (Some(gc), Some(gs), Some(ga), Some(gl), Some(gt)) =
        (global("wl_compositor"), global("wl_shm"), global("wl_seat"), global("zwlr_layer_shell_v1"), global("dashos_tema_v1"))
    else {
        println!("  [panel] el compositor no le da capas o tema");
        return 3;
    };
    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gl).texto(Some("zwlr_layer_shell_v1")).uint(4).uint(CAPAS).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gt).texto(Some("dashos_tema_v1")).uint(1).uint(TEMA).fin();
    e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
    c.mandar(&e);
    // La memoria del dibujo, una vez.
    let tamano = (ANCHO * ALTO * 4) as usize;
    let m = dashos::memoria_crear(tamano);
    if (m as i32) <= 0 || dashos::memoria_proyectar(m as u32, VA, true) != 0 {
        println!("  [panel] sin memoria");
        return 4;
    }
    let copia = dashos::llave_duplicar(m as u32, dashos::LEER | dashos::TRANSFERIR, 0);
    let mut e = Escritor::new();
    e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(POOL).llave(copia as u32).int(tamano as i32).fin();
    e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER).int(0).int(ANCHO).int(ALTO).int(ANCHO * 4).uint(0).fin();
    c.mandar(&e);
    println!("  [panel] en marcha: Super+A lo abre");
    let mut tema = Tema::CLARO;
    let mut oscuro = false;
    let mut abierto = false;
    let mut volumen = 100u32;
    let (mut x, mut y, mut dentro) = (0i32, 0i32, false);
    let pintar_y_mandar = |c: &mut Cliente, tema: &Tema, oscuro: bool, volumen: u32, fuente: &mut Fuente, titulo: &mut Fuente| {
        let px = unsafe { core::slice::from_raw_parts_mut(VA as *mut u32, tamano / 4) };
        pintar(px, tema, oscuro, volumen, fuente, titulo);
        let mut e = Escritor::new();
        e.empezar(SUPERFICIE, wl_surface::req::ATTACH).uint(BUFER).int(0).int(0).fin();
        e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(ANCHO).int(ALTO).fin();
        e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
        c.mandar(&e);
    };
    loop {
        let Some((o, orden, cuerpo)) = c.evento() else {
            println!("  [panel] se cortó la conexión con el compositor");
            return 5;
        };
        let palabra = |i: usize| u32::from_le_bytes(cuerpo[4 * i..4 * i + 4].try_into().unwrap());
        match (o, orden) {
            (TEMA, dashos_tema_v1::ev::TEMA) => {
                oscuro = palabra(1) != 0;
                tema = Tema::del_sistema(palabra(0) | 0xff00_0000, oscuro);
                if abierto {
                    pintar_y_mandar(&mut c, &tema, oscuro, volumen, &mut fuente, &mut titulo);
                }
            }
            (TEMA, dashos_tema_v1::ev::PANEL) if abierto => {
                abierto = false;
                let mut e = Escritor::new();
                e.empezar(CAPA, zwlr_layer_surface_v1::req::DESTROY).fin();
                e.empezar(SUPERFICIE, wl_surface::req::DESTROY).fin();
                c.mandar(&e);
                println!("  [panel] cerrado");
            }
            (TEMA, dashos_tema_v1::ev::PANEL) => {
                // Arriba a la derecha (anclas 1 arriba y 8 derecha), en la capa de encima (2), con el teclado a demanda (2).
                abierto = true;
                let mut e = Escritor::new();
                e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUPERFICIE).fin();
                e.empezar(CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(CAPA).uint(SUPERFICIE).uint(0).uint(2).texto(Some("panel")).fin();
                e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_SIZE).uint(ANCHO as u32).uint(ALTO as u32).fin();
                e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(1 | 8).fin();
                e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_MARGIN).int(MARGEN).int(MARGEN).int(0).int(0).fin();
                e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_KEYBOARD_INTERACTIVITY).uint(2).fin();
                e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
                c.mandar(&e);
            }
            (CAPA, zwlr_layer_surface_v1::ev::CONFIGURE) => {
                let mut e = Escritor::new();
                e.empezar(CAPA, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                c.mandar(&e);
                pintar_y_mandar(&mut c, &tema, oscuro, volumen, &mut fuente, &mut titulo);
                // Para las pruebas: dónde quedan los mosaicos, desde la esquina de arriba a la derecha de la pantalla.
                println!("  [panel] abierto; mosaico a ({}, {}) de la esquina de arriba a la derecha",
                    MARGEN + ANCHO - MOSAICO.x - MOSAICO.w / 2, MARGEN + MOSAICO.y + MOSAICO.h / 2);
                println!("  [panel] volumen: menos a ({}, {}), más a ({}, {})",
                    MARGEN + ANCHO - VOLUMEN.x - VOLUMEN.w / 4, MARGEN + VOLUMEN.y + VOLUMEN.h / 2,
                    MARGEN + ANCHO - VOLUMEN.x - 3 * VOLUMEN.w / 4, MARGEN + VOLUMEN.y + VOLUMEN.h / 2);
            }
            (CAPA, zwlr_layer_surface_v1::ev::CLOSED) => abierto = false,
            (PUNTERO, wl_pointer::ev::ENTER) => {
                dentro = true;
                (x, y) = (palabra(2) as i32 >> 8, palabra(3) as i32 >> 8);
            }
            (PUNTERO, wl_pointer::ev::LEAVE) => dentro = false,
            (PUNTERO, wl_pointer::ev::MOTION) => (x, y) = (palabra(1) as i32 >> 8, palabra(2) as i32 >> 8),
            (PUNTERO, wl_pointer::ev::BUTTON) if palabra(2) == BOTON_IZQUIERDO && palabra(3) == 1 && dentro && abierto => {
                if MOSAICO.contiene(x, y) {
                    let estado = pedir_modo(!oscuro);
                    println!("  [panel] modo oscuro {}: {}", if oscuro { "apagado" } else { "encendido" }, estado);
                } else if VOLUMEN.contiene(x, y) {
                    volumen = if x < VOLUMEN.x + VOLUMEN.w / 2 { volumen.saturating_sub(10) } else { (volumen + 10).min(100) };
                    pedir_volumen(volumen);
                    pintar_y_mandar(&mut c, &tema, oscuro, volumen, &mut fuente, &mut titulo);
                } else if BLOQUEAR.contiene(x, y) {
                    // Se cierra el panel y se pide el bloqueo: lo hace quien bloquea (`guardian`), como con Super+L.
                    abierto = false;
                    let mut e = Escritor::new();
                    e.empezar(CAPA, zwlr_layer_surface_v1::req::DESTROY).fin();
                    e.empezar(SUPERFICIE, wl_surface::req::DESTROY).fin();
                    e.empezar(TEMA, dashos_tema_v1::req::BLOQUEAR).fin();
                    c.mandar(&e);
                    println!("  [panel] bloquear");
                }
            }
            (1, wl_display::ev::ERROR) => {
                println!("  [panel] error de protocolo en el objeto {}", palabra(0));
                return 6;
            }
            _ => {}
        }
    }
}
