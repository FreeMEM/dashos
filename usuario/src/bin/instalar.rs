//! S32: la ventana "Instalar DashOS". Solo en vivo (arrancado del pendrive): la lanza `inicio` junto al instalador y
//! sale en el escritorio. Hace lo mismo que la orden `Instalar` del Shell, por el mismo servicio:
//!
//! 1. Enseña los discos y lo que tienen (y lo que no se toca y por qué).
//! 2. Con un botón por disco y por modo ("Usar el espacio libre", "Borrar el disco entero"), pide el plan y lo enseña,
//!    con el dibujo del disco antes y después. Hasta aquí no se escribe nada.
//! 3. "Instalar": la ventana del sistema (`guardian`) pide la contraseña de un administrador para ese plan (la tanda
//!    "instalar <número>"); con ella, el instalador lo aplica y aquí sale el registro.
//!
//! Llaves, por su nombre: "vivo-original" (solo para que `inicio` la lance en vivo), "ventanas", "instalador" y
//! "sesiones".

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
mod ventana_cliente;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use dashos::println;
use kit::controles::{Boton, Hijo, Interfaz, Texto};
use kit::lienzo::{Lienzo, Rect};
use kit::objetos::{atr, Arbol, Obj, Valor};
use kit::tema::Tema;
use ventana_cliente::*;
use wayland::tablas::{wl_compositor, wl_display, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface, xdg_surface, xdg_toplevel, xdg_wm_base};
use wayland::Escritor;

dashos::programa!(principal);
dashos::monton!();

const ANCHO: i32 = 960;
const ALTO: i32 = 720;
const LINEAS: usize = 24;
const VA_TEXTO: usize = 0x47_0000_0000;
const VA_LISTA: usize = 0x46_0000_0000;
const MAX_TEXTO: usize = 64 * 1024;

/// Una orden al instalador (como `ordenes/src/base/instalar.rs`): (estado, texto).
fn pedir(llave: u32, orden: u32, entrada: &str, plazo: u64) -> Option<(i32, String)> {
    let m = dashos::memoria_crear(MAX_TEXTO);
    if (m as i32) <= 0 || dashos::memoria_proyectar(m, VA_TEXTO, true) != 0 {
        return None;
    }
    let n = entrada.len().min(MAX_TEXTO);
    unsafe { core::slice::from_raw_parts_mut(VA_TEXTO as *mut u8, n) }.copy_from_slice(&entrada.as_bytes()[..n]);
    let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
    let (lectura, suyo) = dashos::canal_crear();
    let mut p = [0u8; 8];
    p[0..4].copy_from_slice(&orden.to_le_bytes());
    p[4..8].copy_from_slice(&(n as u32).to_le_bytes());
    let mut r = [0u8; 8];
    let res = if copia > 0 && dashos::canal_escribir(llave, &p, &[suyo, copia as u32]) == 0 {
        let (k, _, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut [], plazo);
        (k == 8).then(|| {
            let largo = (u32::from_le_bytes(r[4..8].try_into().unwrap()) as usize).min(MAX_TEXTO);
            let t = unsafe { core::slice::from_raw_parts(VA_TEXTO as *const u8, largo) };
            (i32::from_le_bytes(r[0..4].try_into().unwrap()), String::from_utf8_lossy(t).into_owned())
        })
    } else {
        None
    };
    dashos::cerrar(lectura);
    dashos::memoria_quitar(VA_TEXTO);
    dashos::cerrar(m);
    res
}

/// La tanda "instalar <n>" a `sesiones`: `guardian` pide la contraseña de un administrador. 0 si se confirmó.
fn confirmar(sesiones: u32, n: u32) -> i32 {
    use dashos::protocolo::sesiones as s;
    let lista = format!("instalar-sistema {}\n", n);
    let mem = dashos::memoria_crear(lista.len());
    if (mem as i32) <= 0 || dashos::memoria_proyectar(mem, VA_LISTA, true) != 0 {
        return -1;
    }
    unsafe { core::slice::from_raw_parts_mut(VA_LISTA as *mut u8, lista.len()) }.copy_from_slice(lista.as_bytes());
    dashos::memoria_quitar(VA_LISTA);
    let (lectura, suyo) = dashos::canal_crear();
    let mut m = [0u8; 8];
    m[0..4].copy_from_slice(&s::TANDA.to_le_bytes());
    m[4..8].copy_from_slice(&(lista.len() as u32).to_le_bytes());
    let r = if dashos::canal_escribir(sesiones, &m, &[suyo, mem]) == 0 {
        let mut r = [0u8; 256];
        let (n, _, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut [], 160_000_000);
        if n > 0 { dashos::tanda::Resultado::de_bytes(&r[..n as usize]).map_or(-1, |r| r.estado) } else { -1 }
    } else {
        -1
    };
    dashos::cerrar(lectura);
    r
}

fn crc32(t: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in t {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
        }
    }
    !c
}

/// Lo que se enseña: las líneas de un texto, en los rótulos (las que no caben, fuera; los comentarios del plan, sin "#").
fn poner_texto(i: &mut Interfaz, lineas: &[Obj], t: &str) {
    let mut v: Vec<String> = t.lines().map(|l| String::from(l.strip_prefix("# ").unwrap_or(l))).filter(|l| !l.is_empty()).collect();
    v.truncate(lineas.len());
    for (k, o) in lineas.iter().enumerate() {
        i.arbol.poner(*o, atr::TEXTO, Valor::Texto(v.get(k).cloned().unwrap_or_default()));
    }
}

fn principal() -> u64 {
    dashos::monton::con_nombre(&"instalar");
    let tabla = dashos::llaves::Tabla::leer();
    let (Some(inst), Some(ses)) = (tabla.buscar("instalador"), tabla.buscar("sesiones")) else {
        println!("  [instalar] sin el instalador: solo en vivo");
        return 0;
    };
    // Los discos, antes de la ventana: el primero que se puede usar, para sus botones.
    let ver = pedir(inst, 1, "", 30_000_000).map(|r| r.1).unwrap_or_else(|| String::from("El instalador no contesta."));
    let disco = ver
        .lines()
        .filter(|l| l.starts_with("disco ") && !l.contains("no se ofrece"))
        .find_map(|l| l.split_ascii_whitespace().nth(1).map(String::from));
    // La llave del compositor no es la 1 (antes va la del pendrive, que solo hace que se lance en vivo).
    if let Some(v) = tabla.buscar("ventanas") {
        usar_llave(v);
    }
    let Some(mut c) = Cliente::conectar() else { return 1 };
    let Some(globales) = c.globales() else { return 1 };
    let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
    let (Some(gc), Some(gs), Some(gx), Some(ga)) = (global("wl_compositor"), global("wl_shm"), global("xdg_wm_base"), global("wl_seat")) else { return 1 };

    let mut a = Arbol::new();
    let titulo = a.crear(Texto, &[(atr::TEXTO, Valor::Texto("Instalar DashOS".into())), (atr::ESTILO, Valor::Int(1))]);
    let nota = a.crear(Texto, &[(atr::TEXTO, Valor::Texto("Nada se escribe hasta que confirmes el plan. Lo de otros sistemas no se toca.".into())), (atr::ESTILO, Valor::Int(2))]);
    let lineas: Vec<Obj> = (0..LINEAS).map(|_| a.crear(Texto, &[(atr::TEXTO, Valor::Texto(String::new())), (atr::ESTILO, Valor::Int(3))])).collect();
    let d = disco.clone().unwrap_or_else(|| String::from("?"));
    let hueco = a.crear(Boton, &[(atr::TEXTO, Valor::Texto(format!("Usar el espacio libre de {}", d))), (atr::ESTILO, Valor::Int(2))]);
    let entero = a.crear(Boton, &[(atr::TEXTO, Valor::Texto(format!("Borrar {} entero", d))), (atr::ESTILO, Valor::Int(2))]);
    let instalar = a.crear(Boton, &[(atr::TEXTO, Valor::Texto("Instalar".into())), (atr::DESACTIVADO, Valor::Bool(true))]);
    // Su CLIC pasa a sí y vuelve a no en el mismo paso: cada uno lo apunta en un rótulo que no se ve (fuera del
    // diseño), que se lee y se limpia tras cada evento.
    let marcas: Vec<Obj> = [hueco, entero, instalar]
        .iter()
        .map(|b| {
            let m = a.crear(Texto, &[(atr::TEXTO, Valor::Texto(String::new()))]);
            a.notificar(*b, atr::CLIC, kit::objetos::Cuando::Igual(Valor::Bool(true)), m, atr::TEXTO, kit::objetos::Que::Fijo(Valor::Texto("si".into())));
            m
        })
        .collect();
    // Los botones arriba, bajo el título: el texto (el plan con su dibujo) ocupa lo demás.
    let mut hijos = alloc::vec![Hijo::Obj(titulo), Hijo::Obj(nota), Hijo::Fila(alloc::vec![Hijo::Obj(hueco), Hijo::Obj(entero), Hijo::Obj(instalar)]), Hijo::Espacio(8)];
    hijos.extend(lineas.iter().map(|o| Hijo::Obj(*o)));
    let mut i = Interfaz::new(a, Hijo::Columna(hijos), Tema::CLARO);
    i.margen = 24;
    i.esquinas = kit::tema::RADIO_VENTANA;
    poner_texto(&mut i, &lineas, &ver);
    i.colocar(Rect::new(0, 0, ANCHO, ALTO));

    let tamano = (ANCHO * ALTO * 4) as usize;
    let m = dashos::memoria_crear(tamano);
    if (m as i32) <= 0 || dashos::memoria_proyectar(m, VA, true) != 0 {
        return 1;
    }
    let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::TRANSFERIR, 0);
    let px = unsafe { core::slice::from_raw_parts_mut(VA as *mut u32, (ANCHO * ALTO) as usize) };
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
    e.empezar(VENTANA, xdg_toplevel::req::SET_TITLE).texto(Some("Instalar DashOS")).fin();
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
    let (mut x, mut y, mut pulsado) = (0i32, 0i32, false);
    let mut configurada = false;
    let mut plan: Option<String> = None;
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
            (XDG_SUPERFICIE, xdg_surface::ev::CONFIGURE) => {
                let mut e = Escritor::new();
                e.empezar(XDG_SUPERFICIE, xdg_surface::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                c.mandar(&e);
                if !configurada {
                    configurada = true;
                    presentar(&mut c, &mut i, px, true);
                    println!("  [instalar] ventana en pantalla; disco que se ofrece: {:?}", disco);
                    // Para la prueba: dónde están los botones, dentro de la ventana.
                    let centro = |o: &Obj| i.sitios.get(o).map_or((0, 0), |r| (r.x + r.w / 2, r.y + r.h / 2));
                    println!("  [instalar] botones: hueco en {:?}, entero en {:?}, instalar en {:?}", centro(&hueco), centro(&entero), centro(&instalar));
                }
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
            (1, wl_display::ev::ERROR) => return 1,
            _ => {}
        }
        // Los botones pulsados (sus marcas).
        let mut pulsados = [false; 3];
        for (k, m) in marcas.iter().enumerate() {
            if i.arbol.leer(*m, atr::TEXTO).is_some_and(|v| v.texto() == "si") {
                i.arbol.poner(*m, atr::TEXTO, Valor::Texto(String::new()));
                pulsados[k] = true;
            }
        }
        if let (Some(d), true) = (&disco, pulsados[0] || pulsados[1]) {
            let modo = if pulsados[0] { "hueco" } else { "disco-entero" };
            match pedir(inst, 2, &format!("disco {}\npedido {}\n", d, modo), 30_000_000) {
                Some((0, t)) => {
                    poner_texto(&mut i, &lineas, &t);
                    plan = Some(t);
                    i.arbol.poner(instalar, atr::DESACTIVADO, Valor::Bool(false));
                    println!("  [instalar] plan de {} ({}) en pantalla", d, modo);
                }
                Some((_, t)) => {
                    poner_texto(&mut i, &lineas, &format!("No se puede: {}", t));
                    plan = None;
                    i.arbol.poner(instalar, atr::DESACTIVADO, Valor::Bool(true));
                }
                None => poner_texto(&mut i, &lineas, "El instalador no contesta."),
            }
            cambio = true;
        }
        if pulsados[2] {
            if let Some(p) = plan.take() {
                i.arbol.poner(instalar, atr::DESACTIVADO, Valor::Bool(true));
                poner_texto(&mut i, &lineas, "Se pide la contraseña de un administrador...");
                presentar(&mut c, &mut i, px, false);
                let r = confirmar(ses, crc32(p.as_bytes()));
                let t = if r != 0 {
                    String::from("No se instala: no se confirmó con la contraseña de un administrador.")
                } else {
                    poner_texto(&mut i, &lineas, "Instalando. No apagues el ordenador.");
                    presentar(&mut c, &mut i, px, false);
                    match pedir(inst, 3, &p, 600_000_000) {
                        Some((0, t)) => format!("Instalado. Al reiniciar arrancará DashOS (una vez); después, elígelo en el menú del ordenador.\n{}", t),
                        Some((_, t)) => format!("No se instaló: {}", t),
                        None => String::from("El instalador no contesta."),
                    }
                };
                println!("  [instalar] {}", t.lines().next().unwrap_or(""));
                poner_texto(&mut i, &lineas, &t);
                cambio = true;
            }
        }
        if cambio && configurada {
            presentar(&mut c, &mut i, px, false);
        }
    }
}
