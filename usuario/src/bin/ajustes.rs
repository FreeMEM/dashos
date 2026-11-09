//! Una aplicación de prueba del kit de interfaz (K7): la ventana de `kit/examples/vista.rs` (un texto, un deslizador,
//! una casilla, un campo de texto y dos botones de Material 3, atados con avisos) como cliente de Wayland. Responde al
//! ratón y al teclado que le manda el compositor; el foco empieza en el campo. Espera dos segundos antes de conectarse,
//! para salir la tercera en la cascada de la prueba (W7). Cambia de tamaño cuando el compositor se lo pide (Super+Arriba
//! la maximiza; Super+Abajo la devuelve): una memoria, un pool y un búfer nuevos de ese tamaño (los viejos se sueltan:
//! cada servicio tiene un cupo de 16 MB), y el kit vuelve a colocar.

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
mod ventana_cliente;

use alloc::vec;
use dashos::println;
use kit::controles::{Boton, Campo, Casilla, Deslizador, Hijo, Interfaz, Texto};
use kit::lienzo::{Lienzo, Rect};
use kit::objetos::{atr, Arbol, Cuando, Que, Valor};
use kit::tema::Tema;
use ventana_cliente::*;
use wayland::tablas::{
    wl_buffer, wl_compositor, wl_display, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface,
    xdg_surface, xdg_toplevel, xdg_wm_base,
};
use wayland::Escritor;

dashos::programa!(principal);
dashos::monton!();

const ANCHO: i32 = 360;
const ALTO: i32 = 250;
/// Lo más grande que se acepta.
const MAX_ANCHO: i32 = 1920;
const MAX_ALTO: i32 = 1200;
/// El segundo pool y su búfer, para cambiar de tamaño (los números de `ventana_cliente` llegan al 15), y dónde se
/// proyecta su memoria.
const POOL_2: u32 = 17;
const BUFER_2: u32 = 16;
const VA_2: usize = VA + 0x1000_0000;

/// Memoria para un búfer de `w`×`h`, proyectada en `va`: su llave, una copia para el compositor y los píxeles.
fn memoria(va: usize, (w, h): (i32, i32)) -> Option<(u32, u32, &'static mut [u32])> {
    let tamano = (w * h * 4) as usize;
    let m = dashos::memoria_crear(tamano);
    if (m as i32) <= 0 || dashos::memoria_proyectar(m as u32, va, true) != 0 {
        println!("  [ajustes] sin memoria para {}x{}", w, h);
        return None;
    }
    let copia = dashos::llave_duplicar(m as u32, dashos::LEER | dashos::TRANSFERIR, 0);
    let px = unsafe { core::slice::from_raw_parts_mut(va as *mut u32, (w * h) as usize) };
    Some((m as u32, copia as u32, px))
}

fn esperar(us: u64) {
    let (a, b) = dashos::canal_crear();
    let t = dashos::temporizador_crear(b, us, 0);
    if t > 0 {
        let mut buf = [0u8; 8];
        let mut ll = [0u32; 1];
        dashos::canal_leer(a, &mut buf, &mut ll);
        dashos::cerrar(t as u32);
    }
    dashos::cerrar(a);
    dashos::cerrar(b);
}

fn principal() -> u64 {
    esperar(2_000_000);
    let Some(mut c) = Cliente::conectar() else { return 1 };
    let Some(globales) = c.globales() else { return 1 };
    let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
    let (Some(gc), Some(gs), Some(gx), Some(ga)) =
        (global("wl_compositor"), global("wl_shm"), global("xdg_wm_base"), global("wl_seat"))
    else {
        return 1;
    };

    // La interfaz.
    let mut a = Arbol::new();
    let texto = a.crear(Texto, &[(atr::TEXTO, Valor::Texto("Volumen: 60 · Ñandú".into()))]);
    let desl = a.crear(Deslizador, &[(atr::VALOR, Valor::Int(60))]);
    let casilla = a.crear(Casilla, &[(atr::VALOR, Valor::Bool(false))]);
    let cancelar = a.crear(Boton, &[(atr::TEXTO, Valor::Texto("Cancelar".into()))]);
    let aceptar = a.crear(Boton, &[(atr::TEXTO, Valor::Texto("Aceptar".into()))]);
    let nombre = a.crear(Campo, &[(atr::TEXTO, Valor::Texto("DashOS".into()))]);
    a.notificar(nombre, atr::CLIC, Cuando::Igual(Valor::Bool(true)), texto, atr::TEXTO, Que::Fijo(Valor::Texto("Nombre guardado".into())));
    a.notificar(desl, atr::VALOR, Cuando::Siempre, texto, atr::TEXTO, Que::Formato("Volumen: {}".into()));
    a.notificar(casilla, atr::VALOR, Cuando::Siempre, aceptar, atr::DESACTIVADO, Que::Disparo);
    a.notificar(aceptar, atr::CLIC, Cuando::Igual(Valor::Bool(true)), texto, atr::TEXTO, Que::Fijo(Valor::Texto("Aceptado".into())));
    a.notificar(cancelar, atr::CLIC, Cuando::Igual(Valor::Bool(true)), texto, atr::TEXTO, Que::Fijo(Valor::Texto("Cancelado".into())));
    let raiz = Hijo::Columna(vec![
        Hijo::Obj(texto),
        Hijo::Fila(vec![Hijo::Obj(desl), Hijo::Obj(casilla)]),
        Hijo::Obj(nombre),
        Hijo::Fila(vec![Hijo::Obj(cancelar), Hijo::Obj(aceptar)]),
    ]);
    let mut i = Interfaz::new(a, raiz, Tema::CLARO);
    i.foco = Some(nombre);
    i.colocar(Rect::new(0, 0, ANCHO, ALTO));

    // La memoria del búfer.
    let tamano = (ANCHO * ALTO * 4) as usize;
    let Some((mut m, copia, mut px)) = memoria(VA, (ANCHO, ALTO)) else { return 1 };

    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gx).texto(Some("xdg_wm_base")).uint(6).uint(XDG).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
    e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
    e.empezar(ASIENTO, wl_seat::req::GET_KEYBOARD).uint(TECLADO).fin();
    e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(POOL).llave(copia).int(tamano as i32).fin();
    e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER).int(0).int(ANCHO).int(ALTO).int(ANCHO * 4).uint(wayland::shm::XRGB8888).fin();
    e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUPERFICIE).fin();
    e.empezar(XDG, xdg_wm_base::req::GET_XDG_SURFACE).uint(XDG_SUPERFICIE).uint(SUPERFICIE).fin();
    e.empezar(XDG_SUPERFICIE, xdg_surface::req::GET_TOPLEVEL).uint(VENTANA).fin();
    e.empezar(VENTANA, xdg_toplevel::req::SET_TITLE).texto(Some("Ajustes")).fin();
    e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
    c.mandar(&e);

    let pintar = |i: &mut Interfaz, px: &mut [u32], (w, h): (i32, i32)| {
        let mut l = Lienzo::new(&mut px[..(w * h) as usize], w, h, w as usize);
        i.dibujar(&mut l);
    };
    let presentar = |c: &mut Cliente, bufer: Option<u32>, (w, h): (i32, i32)| {
        let mut e = Escritor::new();
        if let Some(b) = bufer {
            e.empezar(SUPERFICIE, wl_surface::req::ATTACH).uint(b).int(0).int(0).fin();
        }
        e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(w).int(h).fin();
        e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
        c.mandar(&e);
    };
    // El tamaño de ahora, el que pide el compositor en su último configure y el búfer que se ve.
    let mut medida = (ANCHO, ALTO);
    let mut pedida = (0i32, 0i32);
    let mut bufer = BUFER;
    let mut pool = POOL;
    let mut va = VA;

    let (mut x, mut y, mut pulsado) = (0i32, 0i32, false);
    let mut configurada = false;
    let mut texto_antes = alloc::string::String::new();
    let mut nombre_antes = alloc::string::String::from("DashOS");
    loop {
        let Some((o, orden, cuerpo)) = c.evento() else { return 0 };
        let palabra = |k: usize| u32::from_le_bytes(cuerpo[4 * k..4 * k + 4].try_into().unwrap());
        let mut cambio = false;
        match (o, orden) {
            (XDG, xdg_wm_base::ev::PING) => {
                let mut e = Escritor::new();
                e.empezar(XDG, xdg_wm_base::req::PONG).uint(palabra(0)).fin();
                c.mandar(&e);
            }
            // El tamaño que pide el compositor llega antes que el configure de la superficie, que lo cierra.
            (VENTANA, xdg_toplevel::ev::CONFIGURE) => pedida = (palabra(0) as i32, palabra(1) as i32),
            (XDG_SUPERFICIE, xdg_surface::ev::CONFIGURE) if !configurada => {
                configurada = true;
                let mut e = Escritor::new();
                e.empezar(XDG_SUPERFICIE, xdg_surface::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                c.mandar(&e);
                pintar(&mut i, px, medida);
                presentar(&mut c, Some(bufer), medida);
                println!("  [ajustes] ventana del kit en pantalla ({}x{})", ANCHO, ALTO);
            }
            (XDG_SUPERFICIE, xdg_surface::ev::CONFIGURE) => {
                let mut e = Escritor::new();
                e.empezar(XDG_SUPERFICIE, xdg_surface::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                // 0 × 0: el tamaño propio; si no, el pedido, sin pasar de la memoria.
                let nueva = if pedida.0 > 0 && pedida.1 > 0 {
                    (pedida.0.min(MAX_ANCHO), pedida.1.min(MAX_ALTO))
                } else {
                    (ANCHO, ALTO)
                };
                let nueva_mem = if nueva != medida { memoria(if va == VA { VA_2 } else { VA }, nueva) } else { None };
                if let Some((m2, copia2, px2)) = nueva_mem {
                    // Pool y búfer nuevos con los otros números; los viejos se destruyen y su memoria se suelta.
                    let (otro_pool, otro) = if bufer == BUFER { (POOL_2, BUFER_2) } else { (POOL, BUFER) };
                    e.empezar(bufer, wl_buffer::req::DESTROY).fin();
                    e.empezar(pool, wl_shm_pool::req::DESTROY).fin();
                    e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(otro_pool).llave(copia2).int(nueva.0 * nueva.1 * 4).fin();
                    e.empezar(otro_pool, wl_shm_pool::req::CREATE_BUFFER).uint(otro).int(0).int(nueva.0).int(nueva.1).int(nueva.0 * 4).uint(wayland::shm::XRGB8888).fin();
                    c.mandar(&e);
                    dashos::memoria_quitar(va);
                    dashos::cerrar(m);
                    (m, px, va) = (m2, px2, if va == VA { VA_2 } else { VA });
                    bufer = otro;
                    pool = otro_pool;
                    medida = nueva;
                    i.colocar(Rect::new(0, 0, medida.0, medida.1));
                    pintar(&mut i, px, medida);
                    presentar(&mut c, Some(bufer), medida);
                    if pedida.0 > 0 {
                        println!("  [ajustes] el compositor pide {}x{}", medida.0, medida.1);
                    } else {
                        println!("  [ajustes] vuelve a {}x{}", medida.0, medida.1);
                    }
                } else {
                    c.mandar(&e);
                }
            }
            // El puntero: coordenadas de 24.8 dentro de la superficie.
            (PUNTERO, wl_pointer::ev::ENTER) => {
                (x, y) = ((palabra(2) as i32) >> 8, (palabra(3) as i32) >> 8);
                cambio = i.raton(x, y, pulsado);
            }
            (PUNTERO, wl_pointer::ev::MOTION) => {
                (x, y) = ((palabra(1) as i32) >> 8, (palabra(2) as i32) >> 8);
                cambio = i.raton(x, y, pulsado);
            }
            (PUNTERO, wl_pointer::ev::BUTTON) if palabra(2) == 0x110 => {
                pulsado = palabra(3) == 1;
                cambio = i.raton(x, y, pulsado);
            }
            // El teclado: el mapa (sin mapa todavía) trae una llave que se suelta; las teclas van a la interfaz.
            (TECLADO, wl_keyboard::ev::KEYMAP) => {
                if let Some(l) = c.juntador.tomar_llave() {
                    dashos::cerrar(l);
                }
            }
            (TECLADO, wl_keyboard::ev::KEY) => cambio = i.tecla(palabra(2), palabra(3) == 1),
            (PUNTERO, wl_pointer::ev::LEAVE) => {
                cambio = i.raton(-1, -1, false);
                pulsado = false;
            }
            (1, wl_display::ev::ERROR) => {
                println!("  [ajustes] error de protocolo");
                return 1;
            }
            _ => {}
        }
        if cambio && configurada {
            pintar(&mut i, px, medida);
            presentar(&mut c, None, medida);
            let t = i.arbol.leer(texto, atr::TEXTO).map(Valor::texto).unwrap_or_default();
            if t != texto_antes {
                println!("  [ajustes] el texto dice: {}", t);
                texto_antes = t;
            }
            let n = i.arbol.leer(nombre, atr::TEXTO).map(Valor::texto).unwrap_or_default();
            if n != nombre_antes {
                // La primera vez que cambia, para la prueba; después, nada (cada tecla lo cambiaría).
                if nombre_antes == "DashOS" {
                    println!("  [ajustes] el campo dice: {}", n);
                }
                nombre_antes = n;
            }
        }
    }
}
