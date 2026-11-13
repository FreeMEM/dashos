//! S29: malware de laboratorio de DashOS (`docs/diseño/S29-malware-linux-y-dashos.md`). Un programa propio e
//! inofensivo que hace lo que haría uno malo instalado por el usuario, con lo que cualquier programa tiene: la llave 1,
//! el canal "ventanas" (el compositor), como cualquier aplicación con ventanas. Tiene que fallar en todo:
//!
//! 1. Llenar la memoria: pide objetos de 1 MB sin parar. Se para en el cupo de su grupo (16 MB), no en la memoria de la
//!    máquina, y lo suelta.
//! 2. Ver la pantalla o mandar teclas a otras ventanas: busca en los globales que anuncia el compositor los protocolos
//!    de Linux que lo permiten (capturar la pantalla, teclado y ratón virtuales, leer el portapapeles sin foco, la lista
//!    de ventanas de los demás, un método de entrada) y los enlaza igual por número: ninguno está, y enlazarlos es un
//!    error de protocolo.
//! 3. Hacerse pasar por la caja Linux: manda una subconexión (`TROZO_SUB_CONECTAR`), que solo se admite de
//!    "caja-linux" por el nombre que da `inicio`: no le contesta nadie, y su conexión de siempre sigue.
//! 4. Escuchar: con teclado, ratón, portapapeles y el tema (por donde la barra recibe el título de la ventana activa),
//!    pero sin ventana, escucha `ESCUCHA_S` segundos. En ese rato el guion de la prueba teclea contraseñas (el
//!    bloqueo y la entrada de `guardian`) y mueve el ratón. No le llega ni una tecla, ni el foco, ni el ratón, ni
//!    ofertas del portapapeles, ni títulos de otras ventanas. Y poner el portapapeles sin foco (para cambiar lo que el
//!    usuario copió, como hace el malware que cambia direcciones de monederos) se cancela.
//!
//! Saca una línea por intento, con "bien" o "MAL".

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use dashos::println;
use wayland::canal::{TROZO_ADIOS, TROZO_CONECTAR, TROZO_DATOS, TROZO_SUB_CONECTAR};
use wayland::tablas::{dashos_tema_v1, wl_callback, wl_data_device, wl_data_device_manager, wl_data_source, wl_display, wl_keyboard, wl_pointer, wl_registry, wl_seat, WL_REGISTRY};
use wayland::{leer_args, leer_cabecera, Escritor};

dashos::programa!(principal);
dashos::monton!();

/// El canal "ventanas" (el compositor).
const VENTANAS: u32 = 1;
/// Lo que escucha, en segundos: lanzado con los últimos (a unos 40 s del encendido), cubre el bloqueo y la entrada de
/// `guardian`, que el guion teclea hacia el minuto y medio.
const ESCUCHA_S: u64 = 120;
const PLAZO_US: u64 = 3_000_000;

/// Los protocolos para espiar o mandar entrada a otros que un cliente no tiene que ver (los de wlroots y los `ext-`).
const PROHIBIDOS: &[&str] = &[
    "zwlr_screencopy_manager_v1",
    "ext_image_copy_capture_manager_v1",
    "ext_output_image_capture_source_manager_v1",
    "zwlr_export_dmabuf_manager_v1",
    "zwp_virtual_keyboard_manager_v1",
    "zwlr_virtual_pointer_manager_v1",
    "zwlr_data_control_manager_v1",
    "ext_data_control_manager_v1",
    "zwlr_foreign_toplevel_manager_v1",
    "ext_foreign_toplevel_list_v1",
    "zwp_input_method_manager_v2",
    "zwlr_layer_shell_v1",
    "ext_session_lock_manager_v1",
    "ext_idle_notifier_v1",
    "zwp_keyboard_shortcuts_inhibit_manager_v1",
];

fn bien(b: bool) -> &'static str {
    if b { "bien" } else { "MAL" }
}

fn escribir(datos: &[u8], llaves: &[u32]) -> bool {
    for _ in 0..200 {
        match dashos::canal_escribir(VENTANAS, datos, llaves) {
            0 => return true,
            -4 => return false,
            _ => dashos::dormir_us(1000),
        }
    }
    false
}

/// Una conexión con el compositor: el canal por el que contesta y lo que llegó sin leer.
struct Con {
    lectura: u32,
    flujo: Vec<u8>,
}

impl Con {
    fn nueva() -> Option<Con> {
        let (lectura, suyo) = dashos::canal_crear();
        escribir(&[TROZO_CONECTAR], &[suyo]).then_some(Con { lectura, flujo: Vec::new() })
    }

    fn mandar(&self, e: &Escritor) -> bool {
        let mut t = alloc::vec![TROZO_DATOS];
        t.extend_from_slice(&e.bytes);
        escribir(&t, &[])
    }

    /// El siguiente evento (objeto, orden y argumentos), esperando hasta `plazo_us`. Las llaves que traiga (el mapa
    /// de teclado), cerradas.
    fn evento(&mut self, plazo_us: u64) -> Option<(u32, u16, Vec<u8>)> {
        loop {
            if let Ok((o, ord, tam)) = leer_cabecera(&self.flujo) {
                let args = self.flujo[8..tam].to_vec();
                self.flujo.drain(..tam);
                return Some((o, ord, args));
            }
            let mut b = [0u8; 256];
            let mut ll = [0u32; 8];
            let (n, k, _) = dashos::canal_leer_plazo(self.lectura, &mut b, &mut ll, plazo_us);
            // Con un error (el plazo), `k` no vale nada: no se mira.
            if n <= 0 {
                return None;
            }
            ll[..k.min(ll.len())].iter().for_each(|&l| {
                dashos::cerrar(l);
            });
            if b[0] == TROZO_DATOS {
                self.flujo.extend_from_slice(&b[1..n as usize]);
            }
        }
    }

    fn adios(self) {
        escribir(&[TROZO_ADIOS], &[]);
        dashos::cerrar(self.lectura);
    }
}

/// 1. La memoria: objetos de 1 MB proyectados y tocados hasta que el kernel diga que no.
fn memoria() -> bool {
    const VA: usize = 0x40_0000_0000;
    let mut llaves = Vec::new();
    let mut error = 0i64;
    for i in 0..4096usize {
        let m = dashos::memoria_crear(1 << 20) as i32;
        if m <= 0 {
            error = m as i64;
            break;
        }
        let va = VA + i * (1 << 20);
        let p = dashos::memoria_proyectar(m as u32, va, true);
        if p != 0 {
            error = p;
            dashos::cerrar(m as u32);
            break;
        }
        // Una escritura por página: que haya memoria de verdad detrás.
        for pag in 0..256 {
            unsafe { core::ptr::write_volatile((va + pag * 4096) as *mut u8, 0x5a) };
        }
        llaves.push((m as u32, va));
    }
    let mb = llaves.len();
    for (m, va) in llaves {
        dashos::memoria_quitar(va);
        dashos::cerrar(m);
    }
    // Y después vuelve a poder pedir (lo soltado volvió a su cupo).
    let otra = dashos::memoria_crear(1 << 20) as i32;
    if otra > 0 {
        dashos::cerrar(otra as u32);
    }
    let ok = error < 0 && mb <= 16 && otra > 0;
    println!("  [espia] llenar la memoria: se para en {} MB (error {}), el cupo de su grupo; lo suelta y vuelve a pedir: {}", mb, error, bien(ok));
    ok
}

/// Los globales anunciados (nombre e interfaz), con `get_registry` y un `sync`.
fn globales(c: &mut Con) -> Vec<(u32, String)> {
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
    e.empezar(1, wl_display::req::SYNC).uint(90).fin();
    c.mandar(&e);
    let mut out = Vec::new();
    while let Some((o, ord, a)) = c.evento(PLAZO_US) {
        if o == 2 && ord == wl_registry::ev::GLOBAL {
            if let Ok(v) = leer_args(&WL_REGISTRY.events[0], &a, &mut core::iter::empty()) {
                out.push((v[0].uint(), String::from(v[1].texto())));
            }
        }
        if o == 90 && ord == wl_callback::ev::DONE {
            break;
        }
    }
    out
}

/// Si el compositor contesta a un `bind` con un error de protocolo (antes que al `sync` de detrás).
fn bind_da_error(nombre: u32, interfaz: &str) -> bool {
    let Some(mut c) = Con::nueva() else { return false };
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
    e.empezar(2, wl_registry::req::BIND).uint(nombre).texto(Some(interfaz)).uint(1).uint(60).fin();
    e.empezar(1, wl_display::req::SYNC).uint(91).fin();
    c.mandar(&e);
    let mut error = false;
    while let Some((o, ord, _)) = c.evento(PLAZO_US) {
        if o == 1 && ord == wl_display::ev::ERROR {
            error = true;
            break;
        }
        if o == 91 && ord == wl_callback::ev::DONE {
            break;
        }
    }
    c.adios();
    error
}

/// 2. Los protocolos para espiar: ni anunciados ni enlazables (por los números de los que no se ven, del 1 al 20).
fn protocolos() -> bool {
    let Some(mut c) = Con::nueva() else { return false };
    let g = globales(&mut c);
    c.adios();
    let anunciados: Vec<&str> = g.iter().map(|x| x.1.as_str()).filter(|i| PROHIBIDOS.contains(i)).collect();
    let mut enlazados = 0;
    let mut intentos = 0;
    for nombre in 1..=20u32 {
        if g.iter().any(|x| x.0 == nombre) {
            continue;
        }
        // Uno de cada clase por número libre: capturar, teclado virtual, portapapeles sin foco, capas y bloqueo.
        for interfaz in ["zwlr_screencopy_manager_v1", "zwp_virtual_keyboard_manager_v1", "zwlr_data_control_manager_v1", "zwlr_layer_shell_v1", "ext_session_lock_manager_v1"] {
            if nombre > 10 && interfaz != "zwlr_screencopy_manager_v1" {
                continue;
            }
            intentos += 1;
            if !bind_da_error(nombre, interfaz) {
                enlazados += 1;
            }
        }
    }
    let ok = !g.is_empty() && anunciados.is_empty() && enlazados == 0 && intentos > 0;
    println!(
        "  [espia] capturar la pantalla, teclas o ratón falsos, el portapapeles sin foco, capas o el bloqueo: {} globales, {} de espiar anunciados, {} de {} enlaces por número aceptados: {}",
        g.len(),
        anunciados.len(),
        enlazados,
        intentos,
        bien(ok)
    );
    ok
}

/// 3. Una subconexión (como las de la caja Linux): no se admite; la conexión de siempre sigue.
fn subconexion() -> bool {
    let (lectura, suyo) = dashos::canal_crear();
    let mut t = alloc::vec![TROZO_SUB_CONECTAR];
    t.extend_from_slice(&7u32.to_le_bytes());
    escribir(&t, &[suyo]);
    // Lo que diga por ese canal (nada: la llave se cierra) y una petición por la subconexión.
    let mut t = alloc::vec![wayland::canal::TROZO_SUB_DATOS];
    t.extend_from_slice(&7u32.to_le_bytes());
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::SYNC).uint(92).fin();
    t.extend_from_slice(&e.bytes);
    escribir(&t, &[]);
    let mut b = [0u8; 256];
    let (n, _, _) = dashos::canal_leer_plazo(lectura, &mut b, &mut [], 2_000_000);
    dashos::cerrar(lectura);
    let sin_respuesta = n <= 0 || b[0] != TROZO_DATOS;
    // La de siempre contesta.
    let sigue = Con::nueva().is_some_and(|mut c| {
        let mut e = Escritor::new();
        e.empezar(1, wl_display::req::SYNC).uint(93).fin();
        c.mandar(&e);
        let mut ok = false;
        while let Some((o, ord, _)) = c.evento(PLAZO_US) {
            if o == 93 && ord == wl_callback::ev::DONE {
                ok = true;
                break;
            }
        }
        c.adios();
        ok
    });
    let ok = sin_respuesta && sigue;
    println!("  [espia] hacerse pasar por la caja Linux (subconexión): sin respuesta {}, el compositor sigue {}: {}", sin_respuesta, sigue, bien(ok));
    ok
}

/// 4. Escuchar sin ventana.
fn escuchar() -> bool {
    let Some(mut c) = Con::nueva() else { return false };
    let g = globales(&mut c);
    let num = |i: &str| g.iter().find(|x| x.1 == i).map(|x| x.0);
    let (Some(seat), Some(pp), Some(tema)) = (num("wl_seat"), num("wl_data_device_manager"), num("dashos_tema_v1")) else {
        println!("  [espia] escuchar: faltan globales ({} anunciados): MAL", g.len());
        c.adios();
        return false;
    };
    // Objetos: asiento 3, teclado 4, ratón 5, portapapeles 6, su dispositivo 7, el tema 8 y una fuente 9.
    let mut e = Escritor::new();
    e.empezar(2, wl_registry::req::BIND).uint(seat).texto(Some("wl_seat")).uint(7).uint(3).fin();
    e.empezar(3, wl_seat::req::GET_KEYBOARD).uint(4).fin();
    e.empezar(3, wl_seat::req::GET_POINTER).uint(5).fin();
    e.empezar(2, wl_registry::req::BIND).uint(pp).texto(Some("wl_data_device_manager")).uint(3).uint(6).fin();
    e.empezar(6, wl_data_device_manager::req::GET_DATA_DEVICE).uint(7).uint(3).fin();
    e.empezar(2, wl_registry::req::BIND).uint(tema).texto(Some("dashos_tema_v1")).uint(1).uint(8).fin();
    // Poner el portapapeles sin foco: se tiene que cancelar.
    e.empezar(6, wl_data_device_manager::req::CREATE_DATA_SOURCE).uint(9).fin();
    e.empezar(9, wl_data_source::req::OFFER).texto(Some("text/plain;charset=utf-8")).fin();
    e.empezar(7, wl_data_device::req::SET_SELECTION).uint(9).uint(1).fin();
    c.mandar(&e);
    let desde = dashos::ahora_us();
    let (mut teclas, mut foco, mut raton, mut ofertas, mut titulos, mut cancelada, mut roto) = (0, 0, 0, 0, 0, false, false);
    while dashos::ahora_us() - desde < ESCUCHA_S * 1_000_000 {
        let Some((o, ord, a)) = c.evento(1_000_000) else {
            // Sin nada en un segundo (o el canal cerrado, que sería un error: se mira abajo).
            continue;
        };
        match (o, ord) {
            (1, wl_display::ev::ERROR) => {
                roto = true;
                break;
            }
            (4, wl_keyboard::ev::KEY) => teclas += 1,
            (4, wl_keyboard::ev::ENTER) => foco += 1,
            (5, wl_pointer::ev::ENTER | wl_pointer::ev::MOTION | wl_pointer::ev::BUTTON) => raton += 1,
            (7, wl_data_device::ev::DATA_OFFER) => ofertas += 1,
            (7, wl_data_device::ev::SELECTION) if a.len() >= 4 && a[0..4] != [0; 4] => ofertas += 1,
            (8, dashos_tema_v1::ev::TITULO) if a.len() > 4 && u32::from_le_bytes(a[0..4].try_into().unwrap()) > 1 => titulos += 1,
            (9, wl_data_source::ev::CANCELLED) => cancelada = true,
            _ => {}
        }
    }
    let segundos = (dashos::ahora_us() - desde) / 1_000_000;
    c.adios();
    let ok = !roto && teclas == 0 && foco == 0 && raton == 0 && ofertas == 0 && titulos == 0 && cancelada;
    println!(
        "  [espia] escuchar sin ventana {} s (el guion teclea contraseñas): {} teclas, {} veces el foco, {} del ratón, {} ofertas del portapapeles, {} títulos ajenos; poner el portapapeles sin foco, cancelado: {}: {}",
        segundos,
        teclas,
        foco,
        raton,
        ofertas,
        titulos,
        cancelada,
        bien(ok)
    );
    ok
}

fn principal() -> u64 {
    // Que el compositor esté listo.
    dashos::dormir_us(1_000_000);
    let a = memoria();
    let b = protocolos();
    let c = subconexion();
    let d = escuchar();
    let todo = a && b && c && d;
    println!("  [espia] malware de laboratorio: {}", if todo { "no consiguió nada: bien" } else { "consiguió algo: MAL" });
    if todo { 0 } else { 1 }
}
