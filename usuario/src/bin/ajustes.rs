//! Una aplicación de prueba del kit de interfaz (K7): la ventana de `kit/examples/vista.rs` (un texto, un deslizador,
//! una casilla, un campo de texto y dos botones de Material 3, atados con avisos) como cliente de Wayland. Responde al
//! ratón y al teclado que le manda el compositor; el foco empieza en el campo. Espera dos segundos antes de conectarse, para salir la tercera en la cascada de la prueba (W7).

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
    wl_compositor, wl_display, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface,
    xdg_surface, xdg_toplevel, xdg_wm_base,
};
use wayland::Escritor;

dashos::programa!(principal);
dashos::monton!();

const ANCHO: i32 = 360;
const ALTO: i32 = 250;

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
    let m = dashos::memoria_crear(tamano);
    if (m as i32) <= 0 || dashos::memoria_proyectar(m as u32, VA, true) != 0 {
        return 1;
    }
    let px = unsafe { core::slice::from_raw_parts_mut(VA as *mut u32, (ANCHO * ALTO) as usize) };
    let copia = dashos::llave_duplicar(m as u32, dashos::LEER | dashos::TRANSFERIR, 0);

    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gx).texto(Some("xdg_wm_base")).uint(6).uint(XDG).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
    e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
    e.empezar(ASIENTO, wl_seat::req::GET_KEYBOARD).uint(TECLADO).fin();
    e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(POOL).llave(copia as u32).int(tamano as i32).fin();
    e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER).int(0).int(ANCHO).int(ALTO).int(ANCHO * 4).uint(wayland::shm::XRGB8888).fin();
    e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUPERFICIE).fin();
    e.empezar(XDG, xdg_wm_base::req::GET_XDG_SURFACE).uint(XDG_SUPERFICIE).uint(SUPERFICIE).fin();
    e.empezar(XDG_SUPERFICIE, xdg_surface::req::GET_TOPLEVEL).uint(VENTANA).fin();
    e.empezar(VENTANA, xdg_toplevel::req::SET_TITLE).texto(Some("Ajustes")).fin();
    e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
    c.mandar(&e);

    let pintar = |i: &mut Interfaz, px: &mut [u32]| {
        let mut l = Lienzo::new(px, ANCHO, ALTO, ANCHO as usize);
        i.dibujar(&mut l);
    };
    let presentar = |c: &mut Cliente, attach: bool| {
        let mut e = Escritor::new();
        if attach {
            e.empezar(SUPERFICIE, wl_surface::req::ATTACH).uint(BUFER).int(0).int(0).fin();
        }
        e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(ANCHO).int(ALTO).fin();
        e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
        c.mandar(&e);
    };

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
            (XDG_SUPERFICIE, xdg_surface::ev::CONFIGURE) if !configurada => {
                configurada = true;
                let mut e = Escritor::new();
                e.empezar(XDG_SUPERFICIE, xdg_surface::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                c.mandar(&e);
                pintar(&mut i, px);
                presentar(&mut c, true);
                println!("  [ajustes] ventana del kit en pantalla ({}x{})", ANCHO, ALTO);
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
            pintar(&mut i, px);
            presentar(&mut c, false);
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
