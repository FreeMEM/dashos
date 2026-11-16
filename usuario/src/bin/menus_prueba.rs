//! S10, los menús de los programas (rama `menus-pulido`, `docs/bitacoras/menus-pulido.md`): un programa del kit nativo
//! (`Interfaz`) con los menús que no tienen el Shell ni la prueba de Dash, para `make arranque-virt-menus`: un submenú
//! corto ("Ordenar por", con opciones), uno que no cabe en la pantalla ("Muchas", 32 entradas) y un menú que no cabe
//! ni en la ventana ni en la pantalla ("Largo", 40). Con ellos se prueban, dentro de la ventana y en la barra, las
//! teclas en un submenú y la rueda en lo que no cabe.
//!
//! Solo va en la imagen de los menús (la característica `menus` del kernel). Para no estorbar a lo que el guion hace
//! antes con el Shell y la ventana de Dash, abre su ventana la segunda vez que los menús van a la barra (el `sitio` de
//! `dashos_menus_v1`), ya con ellos allí. Llave: 1 "ventanas".
//!
//! Lo que dice por la consola: dónde están sus títulos, lo que se elige (`elegido: número (título)`, también lo que
//! llega de la barra), lo que se ve de lo desplegado cuando cambia (`se ve: ...`) y dónde van los menús.
//!
//! Lo hostil, a petición del guion (sus dos últimas entradas de "Ver"): cambiar su menú veinte veces seguidas, una
//! cada 150 ms, mientras el guion lo tiene desplegado (el título de "Recargar" y la casilla); y cerrar su ventana a los
//! 3 s, con el menú desplegado en la barra.

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
mod ventana_cliente;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use dashos::println;
use kit::controles::{Hijo, Interfaz, Texto};
use kit::lienzo::{Lienzo, Rect};
use kit::menus::Datos;
use kit::objetos::{atr, Arbol, Valor};
use kit::tema::Tema;
use ventana_cliente::*;
use wayland::tablas::{
    dashos_menu_v1, dashos_menus_v1, wl_compositor, wl_display, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface,
    xdg_surface, xdg_toplevel, xdg_wm_base,
};
use wayland::Escritor;

dashos::programa!(principal);
dashos::monton!();

/// Ancha (casi la pantalla de la prueba, 800): con el Shell delante, su borde derecho sigue a la vista y el guion
/// puede darle el teclado con un clic.
const ANCHO: i32 = 780;
const ALTO: i32 = 320;
/// `dashos_menus_v1` y el menú de su ventana, tras los de `ventana_cliente`.
const MENUS_G: u32 = 40;
const MENU_V: u32 = 41;
const SITIO_BARRA: u32 = 1;

const M_CAMBIAR: u32 = 6;
const M_CERRAR: u32 = 7;
/// Las veces que cambia su menú (M_CAMBIAR), y cada cuánto.
const CAMBIOS: u32 = 20;
const CADA_US: u64 = 150_000;

/// El menú: "Ver" con una entrada, una casilla, un submenú de opciones, uno de 32 entradas y las dos de lo hostil;
/// "Largo", de 40. `vuelta`: cuántas veces ha cambiado (va en el título de "Recargar"; la casilla, marcada en las pares).
fn menu(vuelta: u32) -> Datos {
    let recargar = if vuelta == 0 { String::from("Recargar") } else { format!("Recargar ({})", vuelta) };
    let marca = if vuelta % 2 == 0 { "cx" } else { "c" };
    let mut t = format!("menus 1\nM\tVer\nE\t1\t{recargar}\tCtrl+R\nE\t2\tBarra de estado\t\t{marca}\n-\nS\tOrdenar por\n>E\t3\tNombre\t\tox\n>E\t4\tTamaño\t\to\n>E\t5\tFecha\t\to\nS\tMuchas\n");
    for k in 1..=32 {
        t.push_str(&format!(">E\t{}\tOpción {}\n", 100 + k, k));
    }
    t.push_str(&format!("-\nE\t{M_CAMBIAR}\tCambiar el menú {CAMBIOS} veces\nE\t{M_CERRAR}\tCerrar la ventana en 3 s\n"));
    t.push_str("M\tLargo\n");
    for k in 1..=40 {
        t.push_str(&format!("E\t{}\tEntrada {}\n", 200 + k, k));
    }
    Datos::leer(t.as_bytes()).unwrap_or_default()
}

fn principal() -> u64 {
    dashos::monton::con_nombre(&"menus-prueba");
    let Some(mut c) = Cliente::conectar() else { return 1 };
    let Some(globales) = c.globales() else { return 1 };
    let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
    let (Some(gc), Some(gs), Some(gx), Some(ga), Some(gm)) =
        (global("wl_compositor"), global("wl_shm"), global("xdg_wm_base"), global("wl_seat"), global("dashos_menus_v1"))
    else {
        println!("  [menus-prueba] sin compositor o sin menús");
        return 1;
    };
    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gm).texto(Some("dashos_menus_v1")).uint(1).uint(MENUS_G).fin();
    c.mandar(&e);
    // Espera a que los menús vayan a la barra por segunda vez (lo de antes del guion, con el Shell y la ventana de
    // Dash, ya está hecho): así su ventana no tapa nada de antes.
    let (mut veces, mut antes) = (0, 0);
    while veces < 2 {
        let Some((o, orden, cuerpo)) = c.evento() else { return 0 };
        if (o, orden) == (MENUS_G, dashos_menus_v1::ev::SITIO) {
            let sitio = cuerpo.get(..4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
            if sitio == SITIO_BARRA && antes != SITIO_BARRA {
                veces += 1;
            }
            antes = sitio;
        }
    }
    println!("  [menus-prueba] los menús van a la barra por segunda vez: abre su ventana");

    let mut a = Arbol::new();
    let titulo = a.crear(Texto, &[(atr::TEXTO, Valor::Texto("Menús de prueba".into())), (atr::ESTILO, Valor::Int(1))]);
    let texto = a.crear(Texto, &[(atr::TEXTO, Valor::Texto("Nada elegido".into())), (atr::ESTILO, Valor::Int(2))]);
    let mut i = Interfaz::new(a, Hijo::Columna(vec![Hijo::Obj(titulo), Hijo::Obj(texto)]), Tema::CLARO);
    i.margen = 24;
    i.esquinas = kit::tema::RADIO_VENTANA;
    // En la barra: así se acaba de quedar el sitio.
    i.poner_menus(menu(0));
    i.menus_fuera(true);
    i.colocar(Rect::new(0, 0, ANCHO, ALTO));

    let tamano = (ANCHO * ALTO * 4) as usize;
    let m = dashos::memoria_crear(tamano);
    if (m as i32) <= 0 || dashos::memoria_proyectar(m as u32, VA, true) != 0 {
        println!("  [menus-prueba] sin memoria");
        return 1;
    }
    let copia = dashos::llave_duplicar(m as u32, dashos::LEER | dashos::TRANSFERIR, 0);
    // SAFETY: la memoria de `tamano` bytes acaba de proyectarse en VA, solo para este programa, y no se suelta.
    let px = unsafe { core::slice::from_raw_parts_mut(VA as *mut u32, (ANCHO * ALTO) as usize) };

    let datos = i.menus.as_ref().map(|m| m.datos.escribir().unwrap_or_default()).unwrap_or_default();
    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gx).texto(Some("xdg_wm_base")).uint(6).uint(XDG).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
    e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
    e.empezar(ASIENTO, wl_seat::req::GET_KEYBOARD).uint(TECLADO).fin();
    e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(POOL).llave(copia as u32).int(tamano as i32).fin();
    e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER).int(0).int(ANCHO).int(ALTO).int(ANCHO * 4).uint(wayland::shm::ARGB8888).fin();
    e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUPERFICIE).fin();
    e.empezar(XDG, xdg_wm_base::req::GET_XDG_SURFACE).uint(XDG_SUPERFICIE).uint(SUPERFICIE).fin();
    e.empezar(XDG_SUPERFICIE, xdg_surface::req::GET_TOPLEVEL).uint(VENTANA).fin();
    e.empezar(VENTANA, xdg_toplevel::req::SET_TITLE).texto(Some("Menús de prueba")).fin();
    e.empezar(MENUS_G, dashos_menus_v1::req::MENU).uint(MENU_V).uint(SUPERFICIE).fin();
    e.empezar(MENU_V, dashos_menu_v1::req::PONER).array(&datos).fin();
    e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
    c.mandar(&e);

    let presentar = |c: &mut Cliente, i: &mut Interfaz, px: &mut [u32], adjuntar: bool| {
        let mut l = Lienzo::new(px, ANCHO, ALTO, ANCHO as usize);
        i.dibujar(&mut l);
        let mut e = Escritor::new();
        if adjuntar {
            e.empezar(SUPERFICIE, wl_surface::req::ATTACH).uint(BUFER).int(0).int(0).fin();
        }
        e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(ANCHO).int(ALTO).fin();
        e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
        c.mandar(&e);
    };
    let decir_titulos = |i: &Interfaz| {
        let Some(m) = i.menus.as_ref() else { return };
        if m.fuera {
            println!("  [menus-prueba] ventana en pantalla ({}x{}); menús en la barra", ANCHO, ALTO);
            return;
        }
        let sitios: alloc::vec::Vec<String> = (0..m.datos.menus.len())
            .filter_map(|k| m.sitio_titulo(k).map(|r| format!("\"{}\" en ({}, {})", m.datos.menus[k].titulo, r.x + r.w / 2, r.y + r.h / 2)))
            .collect();
        println!("  [menus-prueba] ventana en pantalla ({}x{}); {}", ANCHO, ALTO, sitios.join("; "));
    };

    let (mut x, mut y, mut pulsado) = (0i32, 0i32, false);
    let mut configurada = false;
    let mut visto = String::new();
    let mut dicho = datos;
    // Lo hostil: los cambios que quedan y cuándo se cierra la ventana.
    let mut vuelta = 0u32;
    let mut cerrar_en: Option<u64> = None;
    loop {
        let ahora = dashos::ahora_us();
        if cerrar_en.is_some_and(|t| ahora >= t) {
            let abierto = i.menus.as_ref().is_some_and(|m| m.abierto());
            println!("  [menus-prueba] cierra su ventana (su menú, desplegado dentro: {})", if abierto { "sí" } else { "no" });
            let mut e = Escritor::new();
            e.empezar(MENU_V, dashos_menu_v1::req::DESTROY).fin();
            e.empezar(VENTANA, xdg_toplevel::req::DESTROY).fin();
            e.empezar(XDG_SUPERFICIE, xdg_surface::req::DESTROY).fin();
            e.empezar(SUPERFICIE, wl_surface::req::DESTROY).fin();
            c.mandar(&e);
            // Un rato más conectado (sin ventana), para que lo que pase en la barra sea por la ventana y no por el
            // programa que se va.
            let hasta = dashos::ahora_us() + 5_000_000;
            while dashos::ahora_us() < hasta {
                if c.evento_plazo(500_000).is_err() {
                    break;
                }
            }
            return 0;
        }
        let espera = if vuelta > 0 && vuelta < CAMBIOS { Some(CADA_US) } else { cerrar_en.map(|t| t.saturating_sub(ahora).max(1)) };
        let ev = match espera {
            Some(us) => match c.evento_plazo(us) {
                Ok(ev) => ev,
                Err(()) => return 1,
            },
            None => c.evento(),
        };
        let Some((o, orden, cuerpo)) = ev else {
            // Sin nada en el plazo: el cambio siguiente del menú (si toca).
            if vuelta > 0 && vuelta < CAMBIOS {
                vuelta += 1;
                let abierto = i.menus.as_ref().is_some_and(|m| m.abierto());
                i.poner_menus(menu(vuelta));
                let d = i.menus.as_ref().map(|m| m.datos.escribir().unwrap_or_default()).unwrap_or_default();
                let mut e = Escritor::new();
                e.empezar(MENU_V, dashos_menu_v1::req::PONER).array(&d).fin();
                c.mandar(&e);
                dicho = d;
                if vuelta == 1 || vuelta == CAMBIOS {
                    println!("  [menus-prueba] cambia su menú: vuelta {} de {} (desplegado dentro: {})", vuelta, CAMBIOS, if abierto { "sí" } else { "no" });
                }
                if configurada {
                    presentar(&mut c, &mut i, px, false);
                }
            }
            continue;
        };
        let palabra = |k: usize| cuerpo.get(4 * k..4 * k + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
        let mut cambio = false;
        match (o, orden) {
            (XDG, xdg_wm_base::ev::PING) => {
                let mut e = Escritor::new();
                e.empezar(XDG, xdg_wm_base::req::PONG).uint(palabra(0)).fin();
                c.mandar(&e);
            }
            (XDG_SUPERFICIE, xdg_surface::ev::CONFIGURE) => {
                let mut e = Escritor::new();
                e.empezar(XDG_SUPERFICIE, xdg_surface::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                c.mandar(&e);
                if !configurada {
                    configurada = true;
                    presentar(&mut c, &mut i, px, true);
                    decir_titulos(&i);
                }
            }
            (MENUS_G, dashos_menus_v1::ev::SITIO) => {
                let fuera = palabra(0) == SITIO_BARRA;
                println!("  [menus-prueba] menús: {}", if fuera { "en la barra" } else { "dentro de la ventana" });
                i.menus_fuera(fuera);
                cambio = true;
                if !fuera && configurada {
                    presentar(&mut c, &mut i, px, false);
                    decir_titulos(&i);
                }
            }
            (MENU_V, dashos_menu_v1::ev::ELEGIDO) => {
                let n = palabra(0);
                let ok = i.elegir_menu_fuera(n);
                println!("  [menus-prueba] elegido en la barra: {} ({})", n, if ok { "vale" } else { "no se puede elegir" });
                cambio = true;
            }
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
            (PUNTERO, wl_pointer::ev::AXIS) if palabra(1) == 0 => cambio = i.rueda(x, y, kit::controles::pasos_rueda(palabra(2))),
            (PUNTERO, wl_pointer::ev::LEAVE) => {
                cambio = i.raton(-1, -1, false);
                pulsado = false;
            }
            (TECLADO, wl_keyboard::ev::KEYMAP) => {
                if let Some(l) = c.juntador.tomar_llave() {
                    dashos::cerrar(l);
                }
            }
            (TECLADO, wl_keyboard::ev::KEY) => cambio = i.tecla(palabra(2), palabra(3) == 1),
            (TECLADO, wl_keyboard::ev::LEAVE) => {
                if i.teclado_sale() {
                    println!("  [menus-prueba] menús cerrados: el teclado se fue");
                    cambio = true;
                }
            }
            (1, wl_display::ev::ERROR) => {
                println!("  [menus-prueba] error de protocolo");
                return 1;
            }
            _ => {}
        }
        // Lo que dejaron los menús: lo elegido, F10 con los menús en la barra, las marcas cambiadas y lo que se ve.
        while let Some(n) = i.elegido_menu() {
            let t = i.menus.as_ref().and_then(|m| m.datos.buscar(n)).map(|e| e.titulo.clone()).unwrap_or_default();
            println!("  [menus-prueba] elegido: {} ({})", n, t);
            i.arbol.poner(texto, atr::TEXTO, Valor::Texto(format!("Elegido: {}", t)));
            cambio = true;
            if n == M_CAMBIAR {
                // La primera vuelta enseguida (el plazo), y las demás cada CADA_US.
                vuelta = 1;
                i.poner_menus(menu(vuelta));
                dicho = alloc::vec::Vec::new();
                if let Some(m) = i.menus.as_mut() {
                    m.cambiado = true;
                }
                println!("  [menus-prueba] cambia su menú: vuelta 1 de {}", CAMBIOS);
            } else if n == M_CERRAR {
                cerrar_en = Some(dashos::ahora_us() + 3_000_000);
                println!("  [menus-prueba] cierra su ventana en 3 s");
            }
        }
        let mut e = Escritor::new();
        if core::mem::take(&mut i.abrir_fuera) {
            e.empezar(MENU_V, dashos_menu_v1::req::ABRIR).fin();
        }
        if let Some(m) = i.menus.as_mut()
            && core::mem::take(&mut m.cambiado)
        {
            let d = m.datos.escribir().unwrap_or_default();
            if d != dicho {
                e.empezar(MENU_V, dashos_menu_v1::req::PONER).array(&d).fin();
                dicho = d;
            }
        }
        if !e.bytes.is_empty() {
            c.mandar(&e);
        }
        let v = i.menus.as_ref().map(|m| m.lo_que_se_ve()).unwrap_or_default();
        if v != visto {
            if !v.is_empty() {
                println!("  [menus-prueba] se ve: {}", v);
            }
            visto = v;
        }
        if cambio && configurada {
            presentar(&mut c, &mut i, px, false);
        }
    }
}
