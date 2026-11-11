//! U4, de prueba: hace de `guardian` en lo de preguntar al usuario, solo en `virt`. Se apunta en `sesiones` para
//! recibir las preguntas ("¿dejas a este programa usar este servicio?") y contesta como lo haría el usuario de la
//! prueba: a `pide` le deja usar `eco`; a nadie más. Cuenta lo que le preguntan: a `pide`, una sola vez (lo concedido
//! se guarda y no se vuelve a preguntar); y al hostil, que pide `eco` sin permiso, al menos una.
//!
//! U3: al acabar, bloquea la sesión en el compositor (G5) con su pantalla, que lleva la frase que eligió la usuaria
//! (para saber que es el sistema y no un programa que lo imita), y espera su contraseña por el teclado; con la buena
//! (la comprueba `sesiones`, VOLVER), desbloquea.
//!
//! Llaves: 1 el canal "ventanas" (con el poder de escritorio), 2 el canal "sesiones".

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
#[allow(dead_code)]
mod ventana_cliente;

use alloc::string::String;
use dashos::println;
use dashos::protocolo::sesiones::{PREGUNTA, PREGUNTAS, RESPUESTA, RESPUESTA_ADMIN, RETIRAR, VOLVER};
use kit::lienzo::{Lienzo, Rect};
use kit::teclado::{Tecla, Teclado};
use kit::texto::Fuente;
use ventana_cliente::*;
use wayland::tablas::{
    ext_session_lock_manager_v1, ext_session_lock_surface_v1, ext_session_lock_v1, wl_compositor, wl_display, wl_keyboard,
    wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface,
};
use wayland::Escritor;

dashos::programa!(principal);
dashos::monton!();

const SESIONES: u32 = 2;
/// La usuaria de la sesión que se bloquea en la prueba, y la frase que eligió al crear la cuenta (S6b).
const USUARIA: &str = "Carla";
const FRASE: &str = "Las gaviotas vuelven al puerto";

/// Una orden a `sesiones` con estas llaves de más (tras la de respuesta); el estado que conteste.
fn pedir(m: &[u8], mas: &[u32]) -> i32 {
    let (lectura, suyo) = dashos::canal_crear();
    let mut llaves = [suyo; 2];
    llaves[1..1 + mas.len()].copy_from_slice(mas);
    let mut r = [0u8; 4];
    let n = if dashos::canal_escribir(SESIONES, m, &llaves[..1 + mas.len()]) == 0 {
        dashos::canal_leer_plazo(lectura, &mut r, &mut [], 3_000_000).0
    } else {
        -1
    };
    dashos::cerrar(lectura);
    if n == 4 { i32::from_le_bytes(r) } else { i32::MIN }
}

fn principal() -> u64 {
    let (preguntas, para_sesiones) = dashos::canal_crear();
    // `sesiones` puede tardar en arrancar (lee su registro y sus permisos del disco): se reintenta un rato.
    let mut apuntado = i32::MIN;
    for _ in 0..10 {
        let copia = dashos::llave_duplicar(para_sesiones, u64::MAX, 0);
        apuntado = pedir(&PREGUNTAS.to_le_bytes(), &[copia as u32]);
        if apuntado == 0 {
            break;
        }
    }
    dashos::cerrar(para_sesiones);
    if apuntado != 0 {
        println!("  [guardian] sesiones no le deja atender preguntas: {}", apuntado);
        return 1;
    }
    let (mut a_pide, mut a_otros) = (0, 0);
    // U5: las acciones de administrador. A `pide` le da la contraseña buena de Ana (administradora); al hostil, la
    // primera vez la de Ana mal escrita y la segunda la buena de Carla, que no es administradora.
    let (mut admin_pide, mut admin_hostil) = (0, 0);
    // Atiende 40 s: lo que dura la prueba.
    let fin = dashos::ahora_us() + 40_000_000;
    while dashos::ahora_us() < fin {
        let mut q = [0u8; PREGUNTA];
        let (n, _, _) = dashos::canal_leer_plazo(preguntas, &mut q, &mut [], 1_000_000);
        if n as usize != PREGUNTA {
            continue;
        }
        let campo = |b: &[u8]| -> usize { b.iter().position(|&c| c == 0).unwrap_or(b.len()) };
        let programa = &q[8..8 + campo(&q[8..24])];
        let servicio = &q[24..24 + campo(&q[24..40])];
        if servicio.first() == Some(&b'*') {
            let (usuario, clave): (&[u8], &[u8]) = match (programa, admin_hostil) {
                (b"pide", _) => (b"Ana", b"clave de Ana"),
                (b"hostil", 0) => (b"Ana", b"no es esta"),
                (b"hostil", _) => (b"Carla", b"clave de Carla"),
                _ => (b"", b""),
            };
            if programa == b"pide" { admin_pide += 1 } else { admin_hostil += 1 }
            let mut r = [0u8; 64];
            r[0..4].copy_from_slice(&RESPUESTA_ADMIN.to_le_bytes());
            r[4..8].copy_from_slice(&q[0..4]);
            r[8..12].copy_from_slice(&(usuario.len() as u32).to_le_bytes());
            r[12..16].copy_from_slice(&(clave.len() as u32).to_le_bytes());
            r[16..16 + usuario.len()].copy_from_slice(usuario);
            r[16 + usuario.len()..16 + usuario.len() + clave.len()].copy_from_slice(clave);
            pedir(&r[..16 + usuario.len() + clave.len()], &[]);
            r.fill(0);
            println!("  [guardian] {} pide {}: contesta como {}", core::str::from_utf8(programa).unwrap_or("?"),
                core::str::from_utf8(&servicio[1..]).unwrap_or("?"), core::str::from_utf8(usuario).unwrap_or("?"));
            continue;
        }
        let si = programa == b"pide" && (servicio == b"eco" || servicio == b"plazo");
        if si { a_pide += 1 } else { a_otros += 1 }
        let mut r = [0u8; 12];
        r[0..4].copy_from_slice(&RESPUESTA.to_le_bytes());
        r[4..8].copy_from_slice(&q[0..4]);
        r[8..12].copy_from_slice(&(si as u32).to_le_bytes());
        pedir(&r, &[]);
        println!("  [guardian] ¿{} puede usar {}? {}", core::str::from_utf8(programa).unwrap_or("?"),
            core::str::from_utf8(servicio).unwrap_or("?"), if si { "sí" } else { "no" });
    }
    // A `pide`, dos preguntas (eco y plazo), una vez: lo concedido se guarda.
    let bien = a_pide == 2 && a_otros >= 1;
    println!("  [guardian] preguntas: de pide {}, de otros {}: {}", a_pide, a_otros, if bien { "bien" } else { "MAL" });
    // Y al final, el usuario se lo retira: se borra de lo guardado. Retirarlo otra vez ya no encuentra nada.
    let mut r = [0u8; 19];
    r[0..4].copy_from_slice(&RETIRAR.to_le_bytes());
    r[4..8].copy_from_slice(&4u32.to_le_bytes());
    r[8..12].copy_from_slice(&3u32.to_le_bytes());
    r[12..16].copy_from_slice(b"pide");
    r[16..19].copy_from_slice(b"eco");
    let (una, otra) = (pedir(&r, &[]), pedir(&r, &[]));
    let mut p2 = [0u8; 21];
    p2[0..4].copy_from_slice(&RETIRAR.to_le_bytes());
    p2[4..8].copy_from_slice(&4u32.to_le_bytes());
    p2[8..12].copy_from_slice(&5u32.to_le_bytes());
    p2[12..16].copy_from_slice(b"pide");
    p2[16..21].copy_from_slice(b"plazo");
    let el_plazo = pedir(&p2, &[]);
    let retirado = una == 0 && otra == -1 && el_plazo == 0;
    println!("  [guardian] retirar a pide el permiso de eco {}, otra vez {}: {}", una, otra, if retirado { "bien" } else { "MAL" });
    let admin = admin_pide == 2 && admin_hostil == 3;
    println!("  [guardian] acciones de administrador: de pide {}, del hostil {}: {}", admin_pide, admin_hostil, if admin { "bien" } else { "MAL" });
    let bloqueo = bloquear();
    if bien && retirado && admin && bloqueo { 0 } else { 1 }
}

/// Manda a `sesiones` la contraseña de la usuaria para comprobarla (VOLVER: sin tocar su volumen) y la borra. No
/// espera: devuelve el canal por el que llegará la respuesta (tras una mala, `sesiones` tarda a propósito, y mientras
/// tanto hay que seguir leyendo al compositor).
fn volver(clave: &mut String) -> Option<u32> {
    // Cuántas letras (no cuáles): para ver una tecla perdida en las pruebas.
    println!("  [guardian] se comprueba una contraseña de {} letras", clave.chars().count());
    let mut m = [0u8; 160];
    let (u, c) = (USUARIA.as_bytes(), clave.as_bytes());
    let c = &c[..c.len().min(128)];
    m[0..4].copy_from_slice(&VOLVER.to_le_bytes());
    m[4..8].copy_from_slice(&(u.len() as u32).to_le_bytes());
    m[8..12].copy_from_slice(&(c.len() as u32).to_le_bytes());
    m[12..12 + u.len()].copy_from_slice(u);
    m[12 + u.len()..12 + u.len() + c.len()].copy_from_slice(c);
    let (lectura, suyo) = dashos::canal_crear();
    let r = dashos::canal_escribir(SESIONES, &m[..12 + u.len() + c.len()], &[suyo]);
    m.fill(0);
    // La contraseña, fuera de la memoria (S6b, regla 9).
    unsafe { clave.as_bytes_mut().fill(0) };
    clave.clear();
    if r != 0 {
        dashos::cerrar(lectura);
        dashos::cerrar(suyo);
        return None;
    }
    Some(lectura)
}

/// Pinta la pantalla de bloqueo: el fondo, la hora de DashOS, la frase de la usuaria, un punto por letra tecleada y,
/// si la anterior fue mala, el aviso.
fn pintar(px: &mut [u32], ancho: i32, alto: i32, letras: usize, mala: bool, fuente: &mut Fuente, titulo: &mut Fuente) {
    let mut l = Lienzo::new(px, ancho, alto, ancho as usize);
    l.rellenar(Rect::new(0, 0, ancho, alto), 0xff0d_1320);
    let centrado = |f: &mut Fuente, t: &str, tam: u32| (ancho - f.medir(t, tam).ancho) / 2;
    let y = alto / 2 - 90;
    let x = centrado(titulo, "Sesión bloqueada", 34);
    titulo.dibujar(&mut l, x, y, "Sesión bloqueada", 34, 0xffe7_ecf5);
    let frase = alloc::format!("\"{}\"", FRASE);
    let x = centrado(fuente, &frase, 22);
    fuente.dibujar(&mut l, x, y + 48, &frase, 22, 0xffff_9d3b);
    let x = centrado(fuente, USUARIA, 20);
    fuente.dibujar(&mut l, x, y + 100, USUARIA, 20, 0xff9a_a7bf);
    // La caja de la contraseña, con un punto por letra.
    let caja = Rect::new(ancho / 2 - 160, y + 120, 320, 48);
    l.redondeado(caja, 24, 0xff16_1f31);
    l.borde(caja, 24, 2, if mala { 0xffe0_5050 } else { 0xff2a_3754 });
    for i in 0..letras.min(20) as i32 {
        l.redondeado(Rect::new(caja.x + 24 + i * 14, caja.y + 19, 10, 10), 5, 0xffe7_ecf5);
    }
    let aviso = if mala { "Contraseña incorrecta" } else { "Escribe tu contraseña y pulsa Intro" };
    let x = centrado(fuente, aviso, 16);
    fuente.dibujar(&mut l, x, y + 200, aviso, 16, if mala { 0xffe0_5050 } else { 0xff9a_a7bf });
}

/// U3: bloquea la sesión (G5) con su pantalla y espera la contraseña de la usuaria. Devuelve si desbloqueó con la
/// buena después de una mala, como hace la prueba.
fn bloquear() -> bool {
    const SALIDA: u32 = 20;
    const GESTOR: u32 = 21;
    const CERROJO: u32 = 22;
    const SUP_BLOQUEO: u32 = 23;
    let Some(mut c) = Cliente::conectar() else {
        println!("  [guardian] sin compositor");
        return false;
    };
    let Some(globales) = c.globales() else { return false };
    let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
    let (Some(gc), Some(gs), Some(ga), Some(go), Some(gl)) = (
        global("wl_compositor"),
        global("wl_shm"),
        global("wl_seat"),
        global("wl_output"),
        global("ext_session_lock_manager_v1"),
    ) else {
        println!("  [guardian] el compositor no le da el bloqueo");
        return false;
    };
    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(go).texto(Some("wl_output")).uint(4).uint(SALIDA).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gl).texto(Some("ext_session_lock_manager_v1")).uint(1).uint(GESTOR).fin();
    e.empezar(ASIENTO, wl_seat::req::GET_KEYBOARD).uint(TECLADO).fin();
    e.empezar(GESTOR, ext_session_lock_manager_v1::req::LOCK).uint(CERROJO).fin();
    e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUPERFICIE).fin();
    e.empezar(CERROJO, ext_session_lock_v1::req::GET_LOCK_SURFACE).uint(SUP_BLOQUEO).uint(SUPERFICIE).uint(SALIDA).fin();
    c.mandar(&e);
    let (mut fuente, mut titulo) = (Fuente::regular(), Fuente::medium());
    let mut teclado = Teclado::new();
    let mut clave = String::new();
    let (mut ancho, mut alto, mut buf) = (0i32, 0i32, false);
    let mut malas = 0;
    let mut ultima_mala = false;
    // Esperando la respuesta de `sesiones` a una contraseña: su canal.
    let mut esperando: Option<u32> = None;
    // Hay que repintar: se hace cuando no queda nada por leer (pintar es lento, y si se pinta tras cada tecla, la cola
    // se llena y el compositor suelta a quien no lee).
    let mut repintar = false;
    loop {
        if let Some(l) = esperando {
            let mut r = [0u8; 4];
            let (n, _, _) = dashos::canal_leer_plazo(l, &mut r, &mut [], 1);
            if n != dashos::E_PLAZO {
                dashos::cerrar(l);
                esperando = None;
                let r = if n == 4 { i32::from_le_bytes(r) } else { i32::MIN };
                if r == 0 {
                    let mut e = Escritor::new();
                    e.empezar(SUP_BLOQUEO, ext_session_lock_surface_v1::req::DESTROY).fin();
                    e.empezar(SUPERFICIE, wl_surface::req::DESTROY).fin();
                    e.empezar(CERROJO, ext_session_lock_v1::req::UNLOCK_AND_DESTROY).fin();
                    // Y un sync: hasta que llegue su respuesta, el compositor no ha hecho lo de antes. Si se dejara de
                    // leer antes, podría soltarlo con el bloqueo puesto (y quedaría en negro, como debe).
                    e.empezar(1, wl_display::req::SYNC).uint(SYNC).fin();
                    c.mandar(&e);
                    while let Some((o, orden, _)) = c.evento() {
                        if o == SYNC && orden == wayland::tablas::wl_callback::ev::DONE {
                            break;
                        }
                    }
                    let bien = malas == 1;
                    println!("  [guardian] desbloqueado con la contraseña buena tras {} mala: {}", malas, if bien { "bien" } else { "MAL" });
                    return bien;
                }
                malas += 1;
                ultima_mala = true;
                println!("  [guardian] contraseña mala en la pantalla de bloqueo ({})", r);
                repintar = true;
            }
        }
        let ev = match (esperando.is_some(), repintar && buf) {
            (true, _) => c.evento_plazo(20_000),
            (false, true) => c.evento_plazo(1),
            (false, false) => c.evento().ok_or(()).map(Some),
        };
        let Ok(ev) = ev else { return false };
        if ev.is_none() && repintar && buf {
            repintar = false;
            let px = unsafe { core::slice::from_raw_parts_mut(VA as *mut u32, (ancho * alto) as usize) };
            pintar(px, ancho, alto, clave.chars().count(), ultima_mala, &mut fuente, &mut titulo);
            let mut e = Escritor::new();
            e.empezar(SUPERFICIE, wl_surface::req::ATTACH).uint(BUFER).int(0).int(0).fin();
            e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(ancho).int(alto).fin();
            e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
            c.mandar(&e);
            continue;
        }
        let (o, orden, cuerpo) = ev.unwrap_or((0, 0, alloc::vec::Vec::new()));
        let palabra = |i: usize| u32::from_le_bytes(cuerpo[4 * i..4 * i + 4].try_into().unwrap());
        match (o, orden) {
            (0, _) => {}
            (CERROJO, ext_session_lock_v1::ev::LOCKED) => println!("  [guardian] sesión bloqueada"),
            (CERROJO, ext_session_lock_v1::ev::FINISHED) => {
                println!("  [guardian] el compositor no deja bloquear");
                return false;
            }
            (SUP_BLOQUEO, ext_session_lock_surface_v1::ev::CONFIGURE) => {
                let mut e = Escritor::new();
                e.empezar(SUP_BLOQUEO, ext_session_lock_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                c.mandar(&e);
                if !buf {
                    (ancho, alto) = (palabra(1) as i32, palabra(2) as i32);
                    let tamano = (ancho * alto * 4) as usize;
                    let m = dashos::memoria_crear(tamano);
                    if (m as i32) <= 0 || dashos::memoria_proyectar(m as u32, VA, true) != 0 {
                        println!("  [guardian] sin memoria para la pantalla");
                        return false;
                    }
                    let copia = dashos::llave_duplicar(m as u32, dashos::LEER | dashos::TRANSFERIR, 0);
                    let mut e = Escritor::new();
                    e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(POOL).llave(copia as u32).int(tamano as i32).fin();
                    e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER).int(0).int(ancho).int(alto).int(ancho * 4).uint(0).fin();
                    c.mandar(&e);
                    buf = true;
                    println!("  [guardian] pantalla de bloqueo en pantalla ({}x{})", ancho, alto);
                }
                repintar = true;
            }
            (TECLADO, wl_keyboard::ev::KEYMAP) => {
                if let Some(l) = c.juntador.tomar_llave() {
                    dashos::cerrar(l);
                }
            }
            // Mientras se comprueba una contraseña, lo tecleado no cuenta (pero se lee).
            (TECLADO, wl_keyboard::ev::KEY) if esperando.is_some() => {
                teclado.tecla(palabra(2), palabra(3) == 1);
            }
            (TECLADO, wl_keyboard::ev::KEY) => match teclado.tecla(palabra(2), palabra(3) == 1) {
                Some(Tecla::Letra(ch)) if clave.len() < 128 => {
                    clave.push(ch);
                    repintar = true;
                }
                Some(Tecla::Retroceso) => {
                    clave.pop();
                    repintar = true;
                }
                Some(Tecla::Intro) => {
                    esperando = volver(&mut clave);
                    repintar = true;
                }
                _ => {}
            },
            (1, wl_display::ev::ERROR) => {
                println!("  [guardian] error de protocolo");
                return false;
            }
            _ => {}
        }
    }
}
