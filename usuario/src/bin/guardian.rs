//! U4, de prueba: hace de `guardian` en lo de preguntar al usuario, solo en `virt`. Se apunta en `sesiones` para
//! recibir las preguntas ("¿dejas a este programa usar este servicio?") y se las hace al usuario en una capa encima de
//! todo; en la prueba contesta el guion de QEMU: a `pide` que sí; a nadie más. Cuenta lo que le preguntan: a `pide`,
//! una sola vez por servicio (lo concedido se guarda y no se vuelve a preguntar); y al hostil, que pide `eco` sin
//! permiso, al menos una.
//!
//! U3: al acabar, bloquea la sesión en el compositor (G5) con su pantalla, que lleva la frase que eligió la usuaria
//! (para saber que es el sistema y no un programa que lo imita), y espera su contraseña por el teclado; con la buena
//! (la comprueba `sesiones`, VOLVER), desbloquea.
//!
//! U6, cambio rápido de usuario (`docs/diseño/U6-cambio-rapido.md`): en la pantalla de bloqueo, Esc pasa a "Cambiar
//! de usuario" (nombre y contraseña). En la prueba, tras entrar Ana, lanza `Programas/cambio` como ella, bloquea y
//! entra Carla con Ana detrás; lanza `cambio` como Carla, que intenta llegar a lo de Ana; y vuelve Ana.
//!
//! U6, paso 2: en la prueba contesta él mismo (como hace con las acciones de administrador) a lo que pide `cambio`: el
//! permiso de plazo, que sí si es de Ana y que no si es de Carla (lo que Ana concede no vale para Carla). Al acabar,
//! retira lo concedido para que el disco quede como estaba.
//!
//! Apagar y reiniciar (decisión del usuario del 2026-11-13): si el panel lo pide con otras sesiones abiertas, `sesiones`
//! pregunta aquí ("*apagar", "*reiniciar"), con el mismo diálogo de teclado. "*salir" (Cerrar sesión, del panel) cierra
//! la sesión de delante y pone la pantalla de entrada. Al acabar la prueba se queda atendiendo eso y las demás
//! preguntas (`atender`).
//!
//! S27, oleada 6: la pantalla de bloqueo (y la de entrada) como la de DMS: el fondo desenfocado (de `fondo`), el reloj
//! grande con la fecha (en la zona horaria del sistema, que le dice el centro), el avatar con la inicial y el campo de
//! la contraseña con su contorno, el estado (red y volumen, del compositor) y los avisos según su ajuste (de `sesiones`,
//! que se lo pide al servicio de avisos). Dónde va cada cosa: `bloqueo_vista.rs`.
//!
//! Llaves (por su nombre en la tabla de llaves): "ventanas" (la 1, con el poder de escritorio), "sesiones" (la 2),
//! "fondo", "centro" y, en algunas imágenes, "uso" y "codigos".

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
#[allow(dead_code)]
mod ventana_cliente;

#[path = "../preguntas_ficheros.rs"]
#[allow(dead_code)]
mod preguntas_ficheros;
#[path = "../bloqueo_vista.rs"]
mod bloqueo_vista;
use bloqueo_vista as bv;
#[path = "../desenfoque.rs"]
#[allow(dead_code)]
mod desenfoque;
#[path = "../reloj.rs"]
#[allow(dead_code)]
mod reloj;
#[path = "../dms_medidas.rs"]
#[allow(dead_code)]
mod dms_medidas;
use preguntas_ficheros as pf;

use alloc::string::String;
use dashos::permisos_texto as pt;
use dashos::println;
use dashos::tanda;
use dashos::protocolo::sesiones::{ENTRAR, FICHA, LANZAR, POR_PAGINA, PREGUNTA, PREGUNTAS, PROCESOS, RESPUESTA, RESPUESTA_ADMIN, RETIRAR, SALIR, VOLVER};
use kit::lienzo::{Lienzo, Rect};
use kit::tema::Tema;
use kit::teclado::{Tecla, Teclado};
use kit::texto::Fuente;
use ventana_cliente::*;
use wayland::tablas::{
    ext_idle_notification_v1, ext_idle_notifier_v1, ext_session_lock_manager_v1, ext_session_lock_surface_v1,
    ext_session_lock_v1, wl_compositor, wl_display, wl_keyboard, wl_pointer,
    wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface, zwlr_layer_shell_v1, zwlr_layer_surface_v1,
};
use wayland::Escritor;

dashos::programa!(principal);
dashos::monton!();

const SESIONES: u32 = 2;
/// El canal del centro de control (S10c): por él se le pide que abra una categoría y (S27, oleada 6) la zona horaria
/// del sistema. Por su nombre en la tabla de llaves: desde la oleada 6, `guardian` tiene también el canal "fondo", que va
/// antes en el orden de los canales, y el centro dejó de ser la llave 3.
fn centro() -> u32 {
    dashos::llaves::buscar("centro").unwrap_or(0)
}
/// La frase que eligió cada usuario al crear su cuenta (S6b): sale en su pantalla de bloqueo. Provisional, como las
/// cuentas de `sesiones`.
const FRASES: &[(&str, &str)] = &[("Carla", "Las gaviotas vuelven al puerto"), ("Ana", "El faro de la isla")];
/// El código de Esc en `wl_keyboard` (el del kit no la da): en la pantalla de bloqueo, cambiar de usuario (U6).
const TECLA_ESC: u32 = 1;

/// Una orden a `sesiones` con estas llaves de más (tras la de respuesta); el estado que conteste.
fn pedir(m: &[u8], mas: &[u32]) -> i32 {
    let (lectura, suyo) = dashos::canal_crear();
    let mut llaves = [suyo; 2];
    llaves[1..1 + mas.len()].copy_from_slice(mas);
    // Rama `cuentas-2`: entrar y volver contestan 8 bytes cuando hay que esperar (el estado y los segundos).
    let mut r = [0u8; 8];
    let n = if dashos::canal_escribir(SESIONES, m, &llaves[..1 + mas.len()]) == 0 {
        dashos::canal_leer_plazo(lectura, &mut r, &mut [], 3_000_000).0
    } else {
        -1
    };
    dashos::cerrar(lectura);
    if n == 4 || n == 8 { i32::from_le_bytes(r[0..4].try_into().unwrap()) } else { i32::MIN }
}

/// Rama `cuentas-2`: como `pedir`, sin llaves de más, con la respuesta de 8 bytes (el estado y los segundos de espera).
fn pedir_8(m: &[u8]) -> (i32, u32) {
    let (lectura, suyo) = dashos::canal_crear();
    let mut r = [0u8; 8];
    let n = if dashos::canal_escribir(SESIONES, m, &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], 10_000_000).0 } else { -1 };
    dashos::cerrar(lectura);
    match n {
        8 => (i32::from_le_bytes(r[0..4].try_into().unwrap()), u32::from_le_bytes(r[4..8].try_into().unwrap())),
        4 => (i32::from_le_bytes(r[0..4].try_into().unwrap()), 0),
        _ => (i32::MIN, 0),
    }
}

/// La imagen para usar (la llave "uso"): sin las cuentas de la prueba (cuántos intentos malos espera el guion).
static USO: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

fn uso() -> bool {
    USO.load(core::sync::atomic::Ordering::Relaxed)
}

/// Rama `cuentas-2`: con la llave "codigos" (la imagen de `make arranque-virt-cuentas`), la de usar escribe también los
/// códigos de recuperación en la consola, como la de las pruebas: su guion los teclea.
static CODIGOS: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Si los códigos de recuperación se escriben en la consola (solo en las imágenes de prueba).
fn codigos_a_la_consola() -> bool {
    !uso() || CODIGOS.load(core::sync::atomic::Ordering::Relaxed)
}

/// Rama `cuentas-2`: el último código de recuperación que se enseñó (la cuenta y el código), para la prueba de recuperar
/// (en las pruebas, `guardian` contesta también por la persona). Solo el hilo principal lo toca.
struct Ultimo(core::cell::UnsafeCell<([u8; 16], [u8; dashos::cuentas::LARGO_CODIGO])>);
// SAFETY: `guardian` tiene un solo hilo.
unsafe impl Sync for Ultimo {}
static ULTIMO_CODIGO: Ultimo = Ultimo(core::cell::UnsafeCell::new(([0; 16], [0; dashos::cuentas::LARGO_CODIGO])));

fn principal() -> u64 {
    // La imagen se sabe por el nombre de las llaves (su tabla), no por cuántas trae: `guardian` es el primero que
    // nombra "uso" (y "codigos"), así que los lee y nace también con "yo:uso" (y "yo:codigos"), las copias para
    // escribirse. En la imagen de usar, la llave 5 era "yo:uso", y mirar si había llave 5 daba por buena "codigos":
    // los códigos de recuperación salían por la consola y Ajustes se abría en Cuentas al entrar.
    let tabla = dashos::llaves::Tabla::leer();
    if tabla.buscar("uso").is_some() {
        USO.store(true, core::sync::atomic::Ordering::Relaxed);
        // Rama `cuentas-2`: solo en la imagen de `make arranque-virt-cuentas`.
        CODIGOS.store(tabla.buscar("codigos").is_some(), core::sync::atomic::Ordering::Relaxed);
    }
    dashos::monton::con_nombre(&"guardian");
    let (preguntas, para_sesiones) = dashos::canal_crear();
    // `sesiones` puede tardar en arrancar (lee su registro y sus permisos del disco): se reintenta un rato.
    // Al arrancar, la cola de `sesiones` puede estar llena (las altas de `inicio` llegan antes de que lea): medio
    // segundo entre intentos, hasta 20 s.
    let mut apuntado = i32::MIN;
    for _ in 0..40 {
        let copia = dashos::llave_duplicar(para_sesiones, u64::MAX, 0);
        apuntado = pedir(&PREGUNTAS.to_le_bytes(), &[copia as u32]);
        if apuntado == 0 {
            break;
        }
        // La copia no se cierra: si llegó a mandarse, ya no es suya (y el número podría ser ya de otra llave).
        dashos::dormir_us(500_000);
    }
    dashos::cerrar(para_sesiones);
    if apuntado != 0 {
        println!("  [guardian] sesiones no le deja atender preguntas: {}", apuntado);
        return 1;
    }
    // La imagen para usar a mano (`make virt-uso`): `inicio` le da un canal más (llave 4, "uso"). Sin el guion de las
    // pruebas: la pantalla de entrada hasta que alguien entre, y luego atender (preguntas, "*bloquear" del panel,
    // apagar y cerrar sesión, que vuelve a la entrada).
    if uso() {
        // Lo dice: en la imagen de usar, los códigos de recuperación no salen de su pantalla.
        let codigos = if CODIGOS.load(core::sync::atomic::Ordering::Relaxed) { "también en la consola (prueba)" } else { "solo en su pantalla" };
        println!("  [guardian] modo de uso: pantalla de entrada; los códigos de recuperación, {}", codigos);
        loop {
            if let Some(quien) = pantalla(None) {
                println!("  [guardian] entra {}", quien);
                break;
            }
        }
        // Rama `cuentas-2`: en la imagen de `make arranque-virt-cuentas` (la llave 5), Ajustes se abre en Cuentas, como
        // si se pulsara, para su guion (en la de usar no hay aún un botón para abrir Ajustes que el guion encuentre).
        if CODIGOS.load(core::sync::atomic::Ordering::Relaxed) {
            println!("  [guardian] (prueba) pide al centro abrir Cuentas: {}", dashos::canal_escribir(centro(), b"abrir cuentas", &[]));
        }
        atender_uso(preguntas);
    }
    let (mut a_pide, mut a_otros) = (0, 0);
    // U6: lo concedido es por programa y usuario: a `pide` se le pregunta una vez por servicio y usuario (servicio y
    // usuario de cada pregunta, y si se repitió alguna).
    let mut vistas: alloc::vec::Vec<(alloc::vec::Vec<u8>, u32)> = alloc::vec::Vec::new();
    let mut repetida = false;
    // Cuántas veces dijo que sí el usuario: a `pide`, a las dos; a los demás, a ninguna.
    let (mut si_pide, mut si_otros) = (0, 0);
    // U5: las acciones de administrador. A `pide` le da la contraseña buena de Ana (administradora); al hostil, la
    // primera vez la de Ana mal escrita y la segunda la buena de Carla, que no es administradora.
    let (mut admin_pide, mut admin_hostil) = (0, 0);
    // U3: a quién se pidió bloquear. Se bloquea al acabar el rato de preguntas: en la prueba, bloquear antes pararía
    // las ventanas de las demás pruebas, que no reciben nada con la sesión bloqueada.
    let mut por_bloquear: Option<alloc::string::String> = None;
    // Atiende 40 s: lo que dura la prueba. S28: y hasta que pregunte la caja Amiga lo suyo (`CAJA_ESPERADAS`: sus cuatro
    // virus de laboratorio y dos veces `actualiza.exe`), 120 s como mucho: va después de su ventana, que espera al guion.
    // Y 20 s más tras la última suya: lo que sigue (el centro de control, los bloqueos) se lleva el teclado y el guion,
    // y la caja, después, abre las ventanas de Dash que el guion pulsa (antes de S28 caían dentro de los 40 s).
    let empieza = dashos::ahora_us();
    let fin = empieza + 40_000_000;
    let (mut caja, mut caja_si, mut ultima_caja) = (0u32, 0u32, 0u64);
    while dashos::ahora_us() < fin
        || (caja < CAJA_ESPERADAS && dashos::ahora_us() < empieza + 120_000_000)
        || (caja > 0 && dashos::ahora_us() < ultima_caja + 20_000_000)
    {
        let mut q = [0u8; PREGUNTA];
        let (n, _, _) = dashos::canal_leer_plazo(preguntas, &mut q, &mut [], 1_000_000);
        if n as usize != PREGUNTA {
            continue;
        }
        let campo = |b: &[u8]| -> usize { b.iter().position(|&c| c == 0).unwrap_or(b.len()) };
        let programa = &q[8..8 + campo(&q[8..24])];
        let servicio = &q[24..24 + campo(&q[24..40])];
        if de_apagar(&q) || de_permiso(&q) || de_tanda(&q) {
            continue;
        }
        // S28: la guardia de ficheros de la caja Amiga (un permiso sin llave): siempre, esta vez o no.
        if pf::es_permiso(servicio) {
            let r = preguntar_fichero(core::str::from_utf8(programa).unwrap_or("?"), servicio, &q[40..]);
            caja += 1;
            caja_si += (r != pf::RESPUESTA_NO) as u32;
            ultima_caja = dashos::ahora_us();
            let mut m = [0u8; 12];
            m[0..4].copy_from_slice(&RESPUESTA.to_le_bytes());
            m[4..8].copy_from_slice(&q[0..4]);
            m[8..12].copy_from_slice(&r.to_le_bytes());
            pedir(&m, &[]);
            continue;
        }
        // U3: bloquear la sesión de un usuario (lo pide uno de sus programas a `sesiones`).
        if servicio == b"*bloquear" {
            let usuario = core::str::from_utf8(programa).unwrap_or("?");
            println!("  [guardian] se bloquea la sesión de {}", usuario);
            por_bloquear = Some(alloc::string::String::from(usuario));
            continue;
        }
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
        // Se le pregunta al usuario (U4). En la prueba, el guion contesta que sí a `pide` y que no a los demás.
        let (prog, serv) = (core::str::from_utf8(programa).unwrap_or("?"), core::str::from_utf8(servicio).unwrap_or("?"));
        let si = preguntar(prog, serv);
        if programa == b"pide" {
            a_pide += 1;
            let uid = u32::from_le_bytes(q[4..8].try_into().unwrap());
            repetida |= vistas.iter().any(|v| v.0 == servicio && v.1 == uid);
            vistas.push((servicio.to_vec(), uid));
        } else {
            a_otros += 1
        }
        if si { if programa == b"pide" { si_pide += 1 } else { si_otros += 1 } }
        let mut r = [0u8; 12];
        r[0..4].copy_from_slice(&RESPUESTA.to_le_bytes());
        r[4..8].copy_from_slice(&q[0..4]);
        r[8..12].copy_from_slice(&(si as u32).to_le_bytes());
        pedir(&r, &[]);
    }
    // A `pide`, por eco y por plazo, una vez por usuario: lo concedido se guarda (U6: para ese usuario; las copias de
    // `pide` de otro usuario que esperaban la misma pregunta reciben la llave esa vez, y si piden otra vez, se les
    // pregunta).
    let ambas = [&b"eco"[..], b"plazo"].iter().all(|sv| vistas.iter().any(|v| v.0 == *sv));
    println!("  [guardian] preguntas de la caja Amiga (guardia de ficheros): {}, a {} dijo que sí", caja, caja_si);
    // S28: el permiso sin llave que el usuario dio "siempre" a `actualiza.exe` en la caja Amiga, fuera (retirar uno sin
    // llave, como los demás). Ya, antes del centro de control, que enseña lo concedido.
    println!("  [guardian] S28: retirar a caja-amiga el permiso ejec:actualiza: {}", retirar("caja-amiga", "ejec:actualiza"));
    let bien = ambas && !repetida && a_otros >= 1 && si_pide == a_pide && si_otros == 0;
    println!("  [guardian] preguntas de pide, una por servicio y usuario: {} ({}; y {} de otros)", if bien { "bien" } else { "MAL" }, a_pide, a_otros);
    if si_pide != a_pide || si_otros != 0 {
        println!("  [guardian] el usuario dijo que sí a pide {} veces y a otros {}", si_pide, si_otros);
    }
    // Y al final, el usuario se lo retira: se borra de lo guardado. Retirarlo otra vez ya no encuentra nada. El de
    // `plazo`, aquí; el de `eco`, en el centro de control (S10c): se le pide que abra Permisos, como un clic en la
    // barra, 3 s después de decirlo (en la prueba, el guion deja de teclear para que la ventana se lleve el teclado).
    let mut p2 = [0u8; 21];
    p2[0..4].copy_from_slice(&RETIRAR.to_le_bytes());
    p2[4..8].copy_from_slice(&4u32.to_le_bytes());
    p2[8..12].copy_from_slice(&5u32.to_le_bytes());
    p2[12..16].copy_from_slice(b"pide");
    p2[16..21].copy_from_slice(b"plazo");
    let (una, otra) = (pedir(&p2, &[]), pedir(&p2, &[]));
    let retirado = una == 0 && otra == -1;
    println!("  [guardian] retirar a pide el permiso de plazo {}, otra vez {}: {}", una, otra, if retirado { "bien" } else { "MAL" });
    println!("  [guardian] el centro de control abrirá Permisos en 3 s");
    dashos::dormir_us(3_000_000);
    // Con una llave de vuelta: el centro contesta al cerrarse, y hasta entonces no sigue (lo siguiente es bloquear).
    let (lectura, suyo) = dashos::canal_crear();
    let mandado = dashos::canal_escribir(centro(), b"abrir permisos", &[suyo]);
    let mut r4 = [0u8; 4];
    // Mientras está abierto, se siguen atendiendo las acciones de administrador (el centro comprueba el disco desde
    // Discos): en la prueba, a "centro" se le da la contraseña de Ana, como a `pide`.
    let mut cerrado = false;
    // Hasta que el centro diga que se cerró (el evento); el tope, 60 s (como antes; con 300, en `arranque-virt-2` el bloqueo no llegaba a tiempo): el tramo del centro en la
    // prueba depende de lo que tarden las demás ventanas del arranque (trasfondo, decisión del coordinador).
    let hasta = dashos::ahora_us() + 60_000_000;
    while mandado == 0 && dashos::ahora_us() < hasta {
        if dashos::canal_leer_plazo(lectura, &mut r4, &mut [], 200_000).0 == 4 {
            cerrado = true;
            break;
        }
        let mut q = [0u8; PREGUNTA];
        if dashos::canal_leer_plazo(preguntas, &mut q, &mut [], 1_000).0 as usize != PREGUNTA || de_tanda(&q) {
            continue;
        }
        let campo = |b: &[u8]| -> usize { b.iter().position(|&c| c == 0).unwrap_or(b.len()) };
        let (programa, servicio) = (&q[8..8 + campo(&q[8..24])], &q[24..24 + campo(&q[24..40])]);
        if programa == b"centro" && servicio.first() == Some(&b'*') {
            let (usuario, clave): (&[u8], &[u8]) = (b"Ana", b"clave de Ana");
            let mut r = [0u8; 64];
            r[0..4].copy_from_slice(&RESPUESTA_ADMIN.to_le_bytes());
            r[4..8].copy_from_slice(&q[0..4]);
            r[8..12].copy_from_slice(&(usuario.len() as u32).to_le_bytes());
            r[12..16].copy_from_slice(&(clave.len() as u32).to_le_bytes());
            r[16..16 + usuario.len()].copy_from_slice(usuario);
            r[16 + usuario.len()..16 + usuario.len() + clave.len()].copy_from_slice(clave);
            pedir(&r[..16 + usuario.len() + clave.len()], &[]);
            r.fill(0);
            println!("  [guardian] centro pide {}: contesta como Ana", core::str::from_utf8(&servicio[1..]).unwrap_or("?"));
        }
    }
    dashos::cerrar(lectura);
    println!("  [guardian] pide al centro abrir Permisos: {}, y se cerró: {}", mandado, if cerrado { "sí" } else { "no" });
    let admin = admin_pide == 2 && admin_hostil == 3;
    println!("  [guardian] acciones de administrador: de pide {}, del hostil {}: {}", admin_pide, admin_hostil, if admin { "bien" } else { "MAL" });
    // Si nadie pidió bloquear (U3), la prueba no lo vio: mal.
    let bloqueo = por_bloquear.is_some_and(|u| pantalla(Some(&u)).is_some());
    // U3: y la pantalla de entrada (en la prueba, la usuaria escribe "Ana" y su contraseña).
    let quien = pantalla(None);
    let entrada = quien.is_some();
    // U6: el cambio rápido de usuario, con Ana dentro.
    let cambio = quien.as_deref() == Some("Ana") && cambio_rapido(preguntas);
    // U6: lo concedido en la prueba, fuera (el de `cambio`, de Ana; y el de eco de `pide`, por si quedó el de otro
    // usuario: el centro solo quita lo de Ana y lo del sistema).
    let quitado = retirar("cambio", "plazo");
    println!("  [guardian] U6: retirar a cambio el permiso de plazo: {}; a pide el de eco: {}", quitado, retirar("pide", "eco"));
    let cambio = cambio && quitado == 0;
    // Shell nativo: se lanza como Ana, con su guion de prueba (lo que escribe va al registro), y se espera a que acabe.
    if quien.as_deref() == Some("Ana") {
        let (e_shell, p_shell) = lanzar_con_argumentos("System/Shell", 1000, "REGISTRO ORDEN Execute S:Prueba-Shell");
        let acaba = e_shell == 0 && esperar_atendiendo(p_shell, 240_000_000, preguntas);
        println!("  [guardian] Shell nativo: lanza System/Shell como Ana: {}, acaba: {}", e_shell, acaba);
        // Rama `cuentas-2`: con Berta hecha por el guion del Shell, la lista de entrada, recuperar y las esperas.
        let cuentas = prueba_cuentas();
        println!("  [guardian] cuentas-2: lista, primer arranque, recuperar y esperas: {}", if cuentas { "bien" } else { "MAL" });
    }
    // U6, paso 2, prueba hostil: `Programas/panel` (una copia de `cambio`), lanzado del disco con el nombre del panel
    // rápido, no tiene sus poderes en el compositor (lo comprueba él). Se lanza como `guardian` (sin `Home:`).
    let (e_panel, p_panel) = lanzar_como("Programas/panel", 0);
    let impostor = e_panel == 0 && esperar_fin(p_panel, 20_000_000);
    println!("  [guardian] U6: lanza Programas/panel: {}, acaba: {}", e_panel, impostor);
    // U3: y bloquear por tiempo. En la prueba se sigue tecleando 3 s tras pedir el aviso y luego nada: con un plazo de
    // 2 s, si la entrada no reiniciara la cuenta, el aviso llegaría a los 2 s. Llega después de 5 y se bloquea la sesión
    // de quien acaba de entrar (en la prueba, Ana).
    let por_tiempo = quien.is_some()
        && match esperar_inactividad(2_000, 30_000_000) {
            Some(ms) => {
                let bien = ms >= 4_000;
                println!("  [guardian] sin entrada durante 2 s: aviso tras {} ms, {}; se bloquea la sesión de {}", ms, if bien { "bien" } else { "MAL" }, quien.as_deref().unwrap_or("?"));
                let desbloqueado = bien && quien.as_deref().and_then(|q| pantalla(Some(q))).is_some();
                println!("  [guardian] bloqueo por tiempo: {}", if desbloqueado { "bien" } else { "MAL" });
                desbloqueado
            }
            None => {
                println!("  [guardian] sin aviso de inactividad: MAL");
                false
            }
        };
    // U3: y bloquear con la tecla, Super+L: un aviso de 60 s que llega antes porque el compositor lo da en el acto. En
    // la prueba, el guion pulsa Super+L al ver la espera y después escribe la contraseña.
    let con_tecla = por_tiempo
        && match esperar_inactividad(60_000, 30_000_000) {
            Some(ms) if ms < 60_000 => {
                println!("  [guardian] Super+L: se bloquea la sesión de {} tras {} ms", quien.as_deref().unwrap_or("?"), ms);
                let desbloqueado = quien.as_deref().and_then(|q| pantalla(Some(q))).is_some();
                println!("  [guardian] bloqueo con la tecla: {}", if desbloqueado { "bien" } else { "MAL" });
                desbloqueado
            }
            _ => {
                println!("  [guardian] Super+L no bloqueó: MAL");
                false
            }
        };
    let todo = bien && retirado && admin && bloqueo && entrada && cambio && por_tiempo && con_tecla;
    println!("  [guardian] prueba acabada: {}; sigue atendiendo", if todo { "bien" } else { "MAL" });
    atender(preguntas)
}

/// Apagar, reiniciar y cerrar la sesión (del panel, por `sesiones`): con otras sesiones abiertas, "*apagar" o
/// "*reiniciar" se preguntan al usuario con el diálogo de teclado, y "*salir" cierra la sesión de delante y pone la
/// pantalla de entrada. Si `q` no es de eso, falso.
fn de_apagar(q: &[u8; PREGUNTA]) -> bool {
    let campo = |b: &[u8]| -> usize { b.iter().position(|&c| c == 0).unwrap_or(b.len()) };
    let programa = core::str::from_utf8(&q[8..8 + campo(&q[8..24])]).unwrap_or("?");
    let servicio = &q[24..24 + campo(&q[24..40])];
    match servicio {
        b"*apagar" | b"*reiniciar" => {
            let si = preguntar(programa, core::str::from_utf8(servicio).unwrap_or("?"));
            let mut r = [0u8; 12];
            r[0..4].copy_from_slice(&RESPUESTA.to_le_bytes());
            r[4..8].copy_from_slice(&q[0..4]);
            r[8..12].copy_from_slice(&(si as u32).to_le_bytes());
            pedir(&r, &[]);
            true
        }
        b"*salir" => {
            // `programa` es aquí el nombre de la cuenta.
            let mut m = [0u8; 28];
            let n = programa.len().min(16);
            m[0..4].copy_from_slice(&SALIR.to_le_bytes());
            m[4..8].copy_from_slice(&(n as u32).to_le_bytes());
            m[12..12 + n].copy_from_slice(&programa.as_bytes()[..n]);
            let estado = pedir(&m[..12 + n], &[]);
            println!("  [guardian] cerrar la sesión de {}: {}", programa, estado);
            if estado == 0 {
                // Rama `cuentas-2`: quién entra luego (la pantalla de entrada, con la lista), como al arrancar.
                if let Some(quien) = pantalla(None) {
                    println!("  [guardian] entra {} (tras cerrar la sesión de {})", quien, programa);
                }
            }
            true
        }
        _ => false,
    }
}

/// Tras la prueba: las preguntas que lleguen, para siempre (`atender_una`).
fn atender(preguntas: u32) -> ! {
    loop {
        let mut q = [0u8; PREGUNTA];
        if dashos::canal_leer(preguntas, &mut q, &mut []).0 as usize != PREGUNTA {
            continue;
        }
        atender_una(&q);
    }
}

/// S27, oleada 6: quien está delante (lo pone `pantalla` cuando alguien entra o desbloquea; solo el hilo principal),
/// para bloquear su sesión con Super+L, el candado del panel o la inactividad. 16 bytes en dos palabras.
static DELANTE: [core::sync::atomic::AtomicU64; 2] = [const { core::sync::atomic::AtomicU64::new(0) }; 2];

fn poner_delante(nombre: &str) {
    let mut b = [0u8; 16];
    let n = nombre.len().min(16);
    b[..n].copy_from_slice(&nombre.as_bytes()[..n]);
    for (k, d) in DELANTE.iter().enumerate() {
        d.store(u64::from_le_bytes(b[8 * k..8 * k + 8].try_into().unwrap()), core::sync::atomic::Ordering::Relaxed);
    }
}

fn delante() -> String {
    let mut b = [0u8; 16];
    for (k, d) in DELANTE.iter().enumerate() {
        b[8 * k..8 * k + 8].copy_from_slice(&d.load(core::sync::atomic::Ordering::Relaxed).to_le_bytes());
    }
    String::from(core::str::from_utf8(&b[..b.iter().position(|&c| c == 0).unwrap_or(16)]).unwrap_or(""))
}

/// S27, oleada 6: en la imagen de usar, las preguntas de `sesiones` las recoge un hilo y las deja en esta cola; el bucle
/// principal espera a la vez al compositor (para bloquear) y a la cola, por la puerta de su conexión (un proceso solo
/// espera en un canal), como el centro y `fondo`.
static COLA_PREGUNTAS: dashos::hilos::Cola<[u8; PREGUNTA], 8> = dashos::hilos::Cola::nueva();
static SACADAS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
/// La puerta de la conexión con el compositor mientras se espera (0: ninguna). Se cambia y se usa con el cerrojo de la
/// cola tomado: el hilo nunca escribe por una llave ya cerrada.
static PUERTA_PREGUNTAS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
const MARCA_PUERTA: u64 = 0x6a7d_0001;
/// Sin tocar el teclado ni el ratón, la sesión se bloquea a los 30 minutos: lo de shiva (`acLockTimeout` 1800 de DMS).
const BLOQUEO_INACTIVO_MS: u32 = 30 * 60 * 1000;

/// El hilo de las preguntas: las lee de `sesiones` y las deja en la cola.
fn hilo_preguntas(preguntas: u64) -> u64 {
    use core::sync::atomic::Ordering::Acquire;
    loop {
        let mut q = [0u8; PREGUNTA];
        let n = dashos::canal_leer(preguntas as u32, &mut q, &mut []).0;
        if n < 0 {
            dashos::dormir_us(100_000);
            continue;
        }
        if n as usize != PREGUNTA {
            continue;
        }
        let mut m = Some(q);
        loop {
            let visto = SACADAS.load(Acquire);
            m = COLA_PREGUNTAS.con(|c| m.take().and_then(|x| c.meter(x).err()));
            if m.is_none() {
                break;
            }
            dashos::hilos::esperar(&SACADAS, visto, 0);
        }
        COLA_PREGUNTAS.con(|_| {
            let p = PUERTA_PREGUNTAS.load(Acquire);
            if p != 0 {
                dashos::canal_escribir(p, &[1], &[]);
            }
        });
    }
}

fn sacar_pregunta() -> Option<[u8; PREGUNTA]> {
    let (llena, m) = COLA_PREGUNTAS.con(|c| (c.llena(), c.sacar()));
    if m.is_some() && llena {
        SACADAS.fetch_add(1, core::sync::atomic::Ordering::Release);
        dashos::hilos::despertar(&SACADAS, 1);
    }
    m
}

/// S27, oleada 6: espera a la vez una pregunta (en la cola) y que haya que bloquear: Super+L, el candado del panel (los
/// dos dan ya los avisos de inactividad, U3) o `BLOQUEO_INACTIVO_MS` sin tocar nada. `true` si hay que bloquear. Al
/// volver se despide del compositor (`pantalla` y las preguntas abren su propia conexión).
fn esperar_bloqueo() -> bool {
    use core::sync::atomic::Ordering::AcqRel;
    const AVISOS: u32 = 30;
    const INACTIVO: u32 = 31;
    let Some((mut c, puerta)) = Cliente::conectar_con_puerta(MARCA_PUERTA) else {
        dashos::dormir_us(1_000_000);
        return false;
    };
    let soltar = || {
        COLA_PREGUNTAS.con(|_| {
            let antes = PUERTA_PREGUNTAS.swap(0, AcqRel);
            if antes != 0 {
                dashos::cerrar(antes);
            }
        });
        dashos::canal_escribir(VENTANAS, &[wayland::canal::TROZO_ADIOS], &[]);
    };
    COLA_PREGUNTAS.con(|_| PUERTA_PREGUNTAS.store(puerta, core::sync::atomic::Ordering::Release));
    let globales = c.globales();
    let global = |n: &str| globales.as_ref().and_then(|g| g.iter().find(|g| g.1 == n).map(|g| g.0));
    let (Some(ga), Some(gi)) = (global("wl_seat"), global("ext_idle_notifier_v1")) else {
        println!("  [guardian] el compositor no da avisos de inactividad: no se bloquea solo");
        soltar();
        dashos::dormir_us(5_000_000);
        return false;
    };
    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gi).texto(Some("ext_idle_notifier_v1")).uint(2).uint(AVISOS).fin();
    e.empezar(AVISOS, ext_idle_notifier_v1::req::GET_IDLE_NOTIFICATION).uint(INACTIVO).uint(BLOQUEO_INACTIVO_MS).uint(ASIENTO).fin();
    c.mandar(&e);
    let bloquear = loop {
        if COLA_PREGUNTAS.con(|c| !c.is_empty()) {
            break false;
        }
        match c.evento_plazo(60_000_000) {
            Ok(Some((o, ev, _))) if o == INACTIVO && ev == ext_idle_notification_v1::ev::IDLED => break true,
            Ok(_) => {}
            Err(()) => break false,
        }
    };
    soltar();
    bloquear
}

/// La imagen de usar, tras entrar: las preguntas (`atender_una`) y bloquear la sesión de delante cuando toque.
fn atender_uso(preguntas: u32) -> ! {
    let pila: &'static mut [u8] = alloc::vec![0u8; 32 << 10].leak();
    if dashos::hilos::crear(hilo_preguntas, preguntas as u64, pila, 0) < 0 {
        println!("  [guardian] no se pudo lanzar el hilo de las preguntas: sin bloqueo con Super+L");
        atender(preguntas);
    }
    println!("  [guardian] atiende: preguntas y bloquear (Super+L, el panel o {} min sin tocar nada)", BLOQUEO_INACTIVO_MS / 60_000);
    loop {
        while let Some(q) = sacar_pregunta() {
            atender_una(&q);
        }
        if esperar_bloqueo() {
            let quien = delante();
            if quien.is_empty() {
                continue;
            }
            println!("  [guardian] bloquear la sesión de {}", quien);
            pantalla(Some(&quien));
        }
    }
}

/// Una pregunta de `sesiones`, tras la prueba o en la imagen de usar. Las de apagar, como `de_apagar`; bloquear, con
/// su pantalla; los permisos, al usuario; las acciones de administrador, con su ventana.
fn atender_una(q: &[u8; PREGUNTA]) {
    {
        if de_apagar(q) || de_permiso(q) || de_tanda(q) {
            return;
        }
        let campo = |b: &[u8]| -> usize { b.iter().position(|&c| c == 0).unwrap_or(b.len()) };
        let programa = core::str::from_utf8(&q[8..8 + campo(&q[8..24])]).unwrap_or("?");
        let servicio = &q[24..24 + campo(&q[24..40])];
        if servicio == b"*bloquear" {
            pantalla(Some(programa));
            return;
        }
        // S28: un fichero de la caja Amiga: siempre, esta vez o no.
        if pf::es_permiso(servicio) {
            let r = preguntar_fichero(programa, servicio, &q[40..]);
            let mut m = [0u8; 12];
            m[0..4].copy_from_slice(&RESPUESTA.to_le_bytes());
            m[4..8].copy_from_slice(&q[0..4]);
            m[8..12].copy_from_slice(&r.to_le_bytes());
            pedir(&m, &[]);
            return;
        }
        // S36, decisión 1: una acción de administrador suelta, con la misma ventana que una tanda.
        if de_accion(q) {
            return;
        }
        let mut r = [0u8; 16];
        let largo = if servicio.first() == Some(&b'*') {
            r[0..4].copy_from_slice(&RESPUESTA_ADMIN.to_le_bytes());
            r[4..8].copy_from_slice(&q[0..4]);
            16
        } else {
            let si = preguntar(programa, core::str::from_utf8(servicio).unwrap_or("?"));
            r[0..4].copy_from_slice(&RESPUESTA.to_le_bytes());
            r[4..8].copy_from_slice(&q[0..4]);
            r[8..12].copy_from_slice(&(si as u32).to_le_bytes());
            12
        };
        pedir(&r[..largo], &[]);
    }
}

/// Retira a `programa` el permiso de `servicio` (para todos los usuarios: `guardian` ve todo).
fn retirar(programa: &str, servicio: &str) -> i32 {
    let mut p = [0u8; 48];
    p[0..4].copy_from_slice(&RETIRAR.to_le_bytes());
    p[4..8].copy_from_slice(&(programa.len() as u32).to_le_bytes());
    p[8..12].copy_from_slice(&(servicio.len() as u32).to_le_bytes());
    p[12..12 + programa.len()].copy_from_slice(programa.as_bytes());
    p[12 + programa.len()..12 + programa.len() + servicio.len()].copy_from_slice(servicio.as_bytes());
    pedir(&p[..12 + programa.len() + servicio.len()], &[])
}

/// U6, paso 2: atiende las preguntas de `cambio` durante `max_us` o hasta que conteste `hasta_n`: el permiso de plazo,
/// sí a la de Ana (1000) y no a la de Carla (1001). Lo demás se niega. Devuelve cuántas contestó de cada una.
fn contestar_a_cambio(preguntas: u32, max_us: u64, hasta: impl Fn((u32, u32)) -> bool) -> (u32, u32) {
    let fin = dashos::ahora_us() + max_us;
    let mut cuentas = (0, 0);
    while dashos::ahora_us() < fin && !hasta(cuentas) {
        let mut q = [0u8; PREGUNTA];
        if dashos::canal_leer_plazo(preguntas, &mut q, &mut [], 200_000).0 as usize != PREGUNTA || de_permiso(&q) || de_tanda(&q) {
            continue;
        }
        let campo = |b: &[u8]| -> usize { b.iter().position(|&c| c == 0).unwrap_or(b.len()) };
        let (programa, servicio) = (&q[8..8 + campo(&q[8..24])], &q[24..24 + campo(&q[24..40])]);
        let uid = u32::from_le_bytes(q[4..8].try_into().unwrap());
        let si = programa == b"cambio" && servicio == b"plazo" && uid == 1000;
        if programa == b"cambio" && servicio == b"plazo" {
            if uid == 1000 { cuentas.0 += 1 } else { cuentas.1 += 1 }
        }
        println!("  [guardian] U6: ¿{} puede usar {} (usuario {})? Contesta {}", core::str::from_utf8(programa).unwrap_or("?"),
            core::str::from_utf8(servicio).unwrap_or("?"), uid, if si { "sí" } else { "no" });
        // Una acción de administrador no se pregunta aquí: se contesta con nada (sesiones la da por negada).
        let mut r = [0u8; 16];
        let largo = if servicio.first() == Some(&b'*') {
            r[0..4].copy_from_slice(&RESPUESTA_ADMIN.to_le_bytes());
            r[4..8].copy_from_slice(&q[0..4]);
            16
        } else {
            r[0..4].copy_from_slice(&RESPUESTA.to_le_bytes());
            r[4..8].copy_from_slice(&q[0..4]);
            r[8..12].copy_from_slice(&(si as u32).to_le_bytes());
            12
        };
        pedir(&r[..largo], &[]);
    }
    cuentas
}

/// U6: lanza el programa del disco `camino` como el usuario `como` (que tiene que haber entrado): (estado, número).
fn lanzar_como(camino: &str, como: u32) -> (i32, u64) {
    let mut p = [0u8; 64];
    p[0..4].copy_from_slice(&LANZAR.to_le_bytes());
    p[4..8].copy_from_slice(&(camino.len() as u32).to_le_bytes());
    p[8..12].copy_from_slice(&como.to_le_bytes());
    p[12..12 + camino.len()].copy_from_slice(camino.as_bytes());
    let (lectura, suyo) = dashos::canal_crear();
    let mut r = [0u8; 12];
    let n = if dashos::canal_escribir(SESIONES, &p[..12 + camino.len()], &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], 5_000_000).0 } else { -1 };
    dashos::cerrar(lectura);
    if n == 12 { (i32::from_le_bytes(r[0..4].try_into().unwrap()), u64::from_le_bytes(r[4..12].try_into().unwrap())) } else { (i32::MIN, 0) }
}

/// Shell nativo: como `lanzar_como`, con argumentos: van por el canal de arranque del programa ("arranque"), ya
/// puestos en su cola (`shell::protocolo`). Nadie atiende ese canal después: el programa no tiene a quién pedirle nada.
fn lanzar_con_argumentos(camino: &str, como: u32, args: &str) -> (i32, u64) {
    use shell::protocolo as sp;
    let (nuestro, suyo) = dashos::canal_crear();
    let mut m = [0u8; 256];
    m[0..4].copy_from_slice(&sp::ARRANQUE.to_le_bytes());
    m[12..16].copy_from_slice(&(args.len() as u32).to_le_bytes());
    let mut bien = dashos::canal_escribir(nuestro, &m[..sp::LARGO_ARRANQUE], &[]) == 0;
    for trozo in args.as_bytes().chunks(sp::POR_DATOS) {
        m[0..4].copy_from_slice(&sp::DATOS.to_le_bytes());
        m[4..4 + trozo.len()].copy_from_slice(trozo);
        bien &= dashos::canal_escribir(nuestro, &m[..4 + trozo.len()], &[]) == 0;
    }
    dashos::cerrar(nuestro);
    if !bien {
        dashos::cerrar(suyo);
        return (i32::MIN, 0);
    }
    let mut p = [0u8; 64];
    p[0..4].copy_from_slice(&LANZAR.to_le_bytes());
    p[4..8].copy_from_slice(&(camino.len() as u32).to_le_bytes());
    p[8..12].copy_from_slice(&como.to_le_bytes());
    p[12..12 + camino.len()].copy_from_slice(camino.as_bytes());
    let (lectura, respuesta) = dashos::canal_crear();
    let mut r = [0u8; 12];
    let n = if dashos::canal_escribir(SESIONES, &p[..12 + camino.len()], &[respuesta, suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], 5_000_000).0 } else { -1 };
    dashos::cerrar(lectura);
    if n == 12 { (i32::from_le_bytes(r[0..4].try_into().unwrap()), u64::from_le_bytes(r[4..12].try_into().unwrap())) } else { (i32::MIN, 0) }
}

/// U6: si el proceso `pid` sigue en la lista de `sesiones` (`guardian` los ve todos).
fn vivo(pid: u64) -> bool {
    let mut desde = 0u64;
    loop {
        let mut p = [0u8; 12];
        p[0..4].copy_from_slice(&PROCESOS.to_le_bytes());
        p[4..12].copy_from_slice(&desde.to_le_bytes());
        let (lectura, suyo) = dashos::canal_crear();
        let mut r = [0u8; 8 + FICHA * POR_PAGINA];
        let n = if dashos::canal_escribir(SESIONES, &p, &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], 3_000_000).0 } else { -1 };
        dashos::cerrar(lectura);
        if n < 8 {
            return false;
        }
        let van = u32::from_le_bytes(r[0..4].try_into().unwrap()) as usize;
        if (0..van.min(POR_PAGINA)).any(|i| r[8 + FICHA * i..8 + FICHA * i + 8] == pid.to_le_bytes()) {
            return true;
        }
        if van < POR_PAGINA {
            return false;
        }
        desde += van as u64;
    }
}

/// U6: espera hasta `max_us` a que acabe el proceso `pid`.
fn esperar_fin(pid: u64, max_us: u64) -> bool {
    let hasta = dashos::ahora_us() + max_us;
    while dashos::ahora_us() < hasta {
        if !vivo(pid) {
            return true;
        }
        dashos::dormir_us(500_000);
    }
    false
}

/// U6, el cambio rápido de usuario, de punta a punta. Ana está dentro: se lanza `cambio` como ella (el testigo), se
/// bloquea su sesión y, en la pantalla, la usuaria pulsa Esc y entra Carla (en la prueba, el guion). Con Ana detrás, se
/// lanza `cambio` como Carla (la espía) y se espera a que acabe; se bloquea la sesión de Carla, Esc, y vuelve Ana. El
/// testigo, que siguió en marcha detrás, acaba al ver que vuelve a estar delante.
fn cambio_rapido(preguntas: u32) -> bool {
    // `RAM:` en memoria: con Ana recién entrada, su prueba (llena su `RAM:` y la vacía, y deja un secreto en ella).
    let ram_ana = prueba_ram(1000);
    let (e_ana, ana) = lanzar_como("Programas/cambio", 1000);
    println!("  [guardian] U6: lanza cambio como Ana: {} (proceso {})", e_ana, ana);
    // Paso 2: el testigo pide el permiso de plazo (Ana dice que sí); luego, que empiece delante un rato (con su ventana
    // en pantalla para la captura del guion).
    let (si_ana, _) = contestar_a_cambio(preguntas, 30_000_000, |c| c.0 >= 1);
    dashos::dormir_us(2_000_000);
    println!("  [guardian] U6: se bloquea la sesión de Ana; Esc para cambiar de usuario");
    let a_carla = pantalla(Some("Ana"));
    let carla = a_carla.as_deref() == Some("Carla");
    let (e_carla, espia) = if carla { lanzar_como("Programas/cambio", 1001) } else { (i32::MIN, 0) };
    println!("  [guardian] U6: lanza cambio como Carla: {} (proceso {})", e_carla, espia);
    // Paso 2: la espía pide el mismo permiso de plazo; Carla no lo concedió (dice que no). Mientras, se espera a que acabe.
    let mut no_carla = 0;
    let hasta = dashos::ahora_us() + 30_000_000;
    let mut espia_acaba = false;
    while e_carla == 0 && dashos::ahora_us() < hasta {
        no_carla += contestar_a_cambio(preguntas, 500_000, |_| false).1;
        if !vivo(espia) {
            espia_acaba = true;
            break;
        }
    }
    // `RAM:` en memoria: Carla, con la sesión de Ana detrás, no llega a la `RAM:` de Ana; deja algo en la suya.
    let ram_carla = e_carla == 0 && prueba_ram(1001);
    // Carla sigue un rato con su sesión (el guion teclea y pulsa, y captura la pantalla: la ventana de Ana no está).
    dashos::dormir_us(3_000_000);
    println!("  [guardian] U6: se bloquea la sesión de Carla; Esc para volver a la de Ana");
    let a_ana = pantalla(Some("Carla"));
    let vuelve = a_ana.as_deref() == Some("Ana");
    let testigo_acaba = e_ana == 0 && esperar_fin(ana, 30_000_000);
    let permisos = si_ana == 1 && no_carla == 1;
    println!("  [guardian] U6: el permiso de plazo de cambio, preguntado como Ana {} y como Carla {}: {}", si_ana, no_carla, if permisos { "bien" } else { "MAL" });
    // `RAM:` en memoria: Carla sale y vuelve a entrar; lo que dejó en su `RAM:` no está. Con las contraseñas de las
    // cuentas de la prueba (las mismas que teclea el guion), sin pasar por la pantalla. Después vuelve Ana delante, con
    // Carla detrás, como estaba.
    let mut salir = -13;
    for _ in 0..20 {
        salir = sesion_de_prueba(SALIR, "Carla", "");
        if salir != -13 {
            break;
        }
        dashos::dormir_us(100_000);
    }
    let entrar = sesion_de_prueba(ENTRAR, "Carla", "clave de Carla");
    let ram_otra_vez = salir == 0 && entrar == 0 && prueba_ram(1001);
    let volver = sesion_de_prueba(VOLVER, "Ana", "clave de Ana");
    let ram = ram_ana && ram_carla && ram_otra_vez && volver == 0;
    println!(
        "  [guardian] RAM: en memoria: la de Ana {}, la de Carla {}; Carla sale {} y entra {}, la nueva {}; vuelve Ana {}: {}",
        ram_ana, ram_carla, salir, entrar, ram_otra_vez, volver, if ram { "bien" } else { "MAL" }
    );
    let bien = carla && espia_acaba && vuelve && testigo_acaba && permisos && ram;
    println!("  [guardian] cambio rápido: Ana, Carla y otra vez Ana: {}", if bien { "bien" } else { "MAL" });
    if !bien {
        println!("  [guardian] U6: entra Carla {}, la espía acaba {}, vuelve Ana {}, el testigo acaba {}", carla, espia_acaba, vuelve, testigo_acaba);
    }
    bien
}

/// `RAM:` en memoria: lanza su prueba (`Programas/ram`) como el usuario `como` y espera a que acabe (hasta dos minutos:
/// llenar el `RAM:` de Ana tarda). Lo que comprueba lo dice ella.
fn prueba_ram(como: u32) -> bool {
    let (e, p) = lanzar_como("Programas/ram", como);
    let acaba = e == 0 && esperar_fin(p, 120_000_000);
    println!("  [guardian] RAM: lanza Programas/ram como el usuario {}: {}, acaba: {}", como, e, acaba);
    acaba
}

/// Entrar, volver o salir de una sesión desde la prueba, sin la pantalla: el estado que da `sesiones`.
fn sesion_de_prueba(orden: u32, usuaria: &str, clave: &str) -> i32 {
    let mut m = [0u8; 96];
    let (u, c) = (usuaria.as_bytes(), clave.as_bytes());
    m[0..4].copy_from_slice(&orden.to_le_bytes());
    m[4..8].copy_from_slice(&(u.len() as u32).to_le_bytes());
    m[8..12].copy_from_slice(&(c.len() as u32).to_le_bytes());
    m[12..12 + u.len()].copy_from_slice(u);
    m[12 + u.len()..12 + u.len() + c.len()].copy_from_slice(c);
    pedir(&m[..12 + u.len() + c.len()], &[])
}

/// Rama `cuentas-2`, sin pantalla (la pantalla, en `make arranque-virt-cuentas`): con Berta recién creada por el guion del
/// Shell (y su código de recuperación, que enseñó esta ventana), lo que pide la pantalla de entrada a `sesiones`:
/// - la lista de cuentas (Ana, Carla y Berta), sin primer arranque; y crear "la primera" con cuentas ya hechas, no;
/// - recuperar a Berta: con un código malo, no; con el bueno tecleado de cualquier manera y una contraseña corta, no; con
///   el bueno, sí, y da otro; el gastado, otra vez, no; una cuenta que no existe, no;
/// - las esperas: la contraseña de antes de Berta y una mala (con el código gastado, tres seguidas): hay que esperar, y ni
///   la buena se prueba; pasada la espera, la nueva entra (`VOLVER`: sin abrir su volumen, que no tiene sesión).
fn prueba_cuentas() -> bool {
    use dashos::cuentas::Fallo;
    use dashos::protocolo::sesiones::{OCUPADO, PRIMERA_CUENTA, PROTEGIDO, RECUPERAR};
    let mut todo = true;
    let mut paso = |que: core::fmt::Arguments, si: bool| {
        todo &= si;
        println!("  [guardian] cuentas-2: {}: {}", que, if si { "bien" } else { "MAL" });
    };
    let (lista, primer) = cuentas_de_entrada();
    let nombres: alloc::vec::Vec<&str> = lista.iter().map(|c| c.0.as_str()).collect();
    paso(format_args!("la lista de la entrada: {:?}, primer arranque {}", nombres, primer), nombres == ["Ana", "Carla", "Berta"] && !primer && lista[2].1 == "Berta Gil");
    let (e, ..) = pedir_con_codigo(PRIMERA_CUENTA, "Eva", "", "clave de Eva");
    paso(format_args!("crear la primera cuenta con cuentas ya hechas, {}", e), e == Fallo::HayCuentas.estado());
    // SAFETY: un solo hilo (`Ultimo`).
    let (cuenta, codigo) = unsafe { *ULTIMO_CODIGO.0.get() };
    let codigo = String::from(core::str::from_utf8(&codigo).unwrap_or(""));
    if &cuenta[..5] != b"Berta" || codigo.len() != dashos::cuentas::LARGO_CODIGO {
        println!("  [guardian] cuentas-2: no hay código de Berta guardado: MAL");
        return false;
    }
    // Tecleado de otra manera: en minúsculas y sin guiones.
    let tecleado: String = codigo.chars().filter(|&c| c != '-').map(|c| c.to_ascii_lowercase()).collect();
    let (malo, ..) = pedir_con_codigo(RECUPERAR, "Berta", "AAAA-AAAA-AAAA-AAAA-AAAA-AAAA", "nueva de Berta");
    let (corta, ..) = pedir_con_codigo(RECUPERAR, "Berta", &tecleado, "corta");
    paso(format_args!("recuperar a Berta con un código malo, {}; con el bueno y una contraseña corta, {}", malo, corta), malo == PROTEGIDO && corta == Fallo::NuevaCorta.estado());
    let (bien, _, nuevo) = pedir_con_codigo(RECUPERAR, "Berta", &tecleado, "nueva de Berta");
    if codigos_a_la_consola() && bien == 0 {
        println!("  [guardian] (prueba) el código de recuperación de Berta es {}", nuevo);
    }
    // Otro, y que sea un código (no ceros: se borraba antes de copiarlo).
    let otro = dashos::cuentas::normalizar_codigo(nuevo.as_bytes()).is_some_and(|c| c[..] == *nuevo.as_bytes()) && nuevo != codigo;
    paso(format_args!("recuperar a Berta con su código, {}; da otro: {}", bien, otro), bien == 0 && otro);
    let (gastado, ..) = pedir_con_codigo(RECUPERAR, "Berta", &codigo, "otra de Berta");
    let (nadie, ..) = pedir_con_codigo(RECUPERAR, "Nadie", &codigo, "otra de Nadie");
    paso(format_args!("el código gastado otra vez, {}; una cuenta que no existe, {}", gastado, nadie), gastado == PROTEGIDO && nadie == PROTEGIDO);
    // Las esperas: la de antes (2.ª mala seguida) y otra (3.ª): empieza la espera; ni la buena se prueba.
    let vieja = sesion_de_prueba(VOLVER, "Berta", "clave de Berta");
    let mala = sesion_de_prueba(VOLVER, "Berta", "tampoco es");
    let mut m = [0u8; 64];
    let clave = b"nueva de Berta";
    m[0..4].copy_from_slice(&VOLVER.to_le_bytes());
    m[4..8].copy_from_slice(&5u32.to_le_bytes());
    m[8..12].copy_from_slice(&(clave.len() as u32).to_le_bytes());
    m[12..17].copy_from_slice(b"Berta");
    m[17..17 + clave.len()].copy_from_slice(clave);
    let (esperando, segundos) = pedir_8(&m[..17 + clave.len()]);
    paso(
        format_args!("la de antes, {}; una mala, {}; la buena, sin probarla: {} con {} s de espera", vieja, mala, esperando, segundos),
        vieja == dashos::protocolo::archivos::PROTEGIDO && mala == dashos::protocolo::archivos::PROTEGIDO && esperando == OCUPADO && (1..=5).contains(&segundos),
    );
    dashos::dormir_us(segundos as u64 * 1_000_000 + 300_000);
    let (despues, _) = pedir_8(&m[..17 + clave.len()]);
    m.fill(0);
    paso(format_args!("pasada la espera, la nueva de Berta vale: {}", despues), despues == 0);
    todo
}

/// S28: lo que se espera de la caja Amiga en la prueba: una pregunta de cada virus de laboratorio que cambia ficheros
/// (arranque, enlace, guion y troyano) y dos de `actualiza.exe` (la tercera vez ya lo recuerda `sesiones`).
const CAJA_ESPERADAS: u32 = 6;

/// U4: pregunta al usuario si deja a `programa` usar `servicio`, en una capa encima de todo, en el centro, con el
/// teclado (solo `guardian` puede poner capas: un programa no puede pintar una igual por encima). Sí con S y no con N;
/// lo que se pulse en el primer segundo no cuenta (quien estaba escribiendo no contesta sin querer), y sin respuesta en
/// 30 s, no.
fn preguntar(programa: &str, servicio: &str) -> bool {
    let mut memoria = 0;
    let r = preguntar_dentro(programa, &Pregunta::Servicio(servicio), &mut memoria);
    dashos::canal_escribir(VENTANAS, &[wayland::canal::TROZO_ADIOS], &[]);
    dashos::memoria_quitar(VA);
    if memoria > 0 {
        dashos::cerrar(memoria);
    }
    let r = r == pf::RESPUESTA_SIEMPRE;
    match servicio.strip_prefix('*') {
        Some(que) => println!("  [guardian] {} con otras sesiones abiertas: el usuario dice {}", que, if r { "sí" } else { "no" }),
        None => println!("  [guardian] ¿{} puede usar {}? El usuario dice {}", programa, servicio, if r { "sí" } else { "no" }),
    }
    r
}

/// S6b, S36: si `q` es un cambio de permiso ("+dar" o "+quitar", de la orden `Permiso`), se pregunta al usuario y se
/// contesta a `sesiones`. Si no, falso.
fn de_permiso(q: &[u8; PREGUNTA]) -> bool {
    let campo = |b: &[u8]| -> usize { b.iter().position(|&c| c == 0).unwrap_or(b.len()) };
    let pide = core::str::from_utf8(&q[8..8 + campo(&q[8..24])]).unwrap_or("?");
    let dar = match &q[24..24 + campo(&q[24..40])] {
        b"+dar" => true,
        b"+quitar" => false,
        _ => return false,
    };
    let (programa, permiso) = pf::leer_detalle(&q[40..]);
    let (programa, permiso) = (limpiar(programa, "un programa"), limpiar(permiso, "?"));
    let mut memoria = 0;
    let r = preguntar_dentro(pide, &Pregunta::Cambio { dar, programa: &programa, permiso: &permiso }, &mut memoria);
    dashos::canal_escribir(VENTANAS, &[wayland::canal::TROZO_ADIOS], &[]);
    dashos::memoria_quitar(VA);
    if memoria > 0 {
        dashos::cerrar(memoria);
    }
    let si = r == pf::RESPUESTA_SIEMPRE;
    println!("  [guardian] cambiar permiso: ¿{} puede {} a {} {}? El usuario dice {}", pide, if dar { "dar" } else { "quitar" }, programa, permiso, if si { "sí" } else { "no" });
    let mut m = [0u8; 12];
    m[0..4].copy_from_slice(&RESPUESTA.to_le_bytes());
    m[4..8].copy_from_slice(&q[0..4]);
    m[8..12].copy_from_slice(&(si as u32).to_le_bytes());
    pedir(&m, &[]);
    true
}

/// S36, decisión 1: si `q` es una tanda de acciones de administrador ("*tanda"), pide a `sesiones` la lista en llano
/// (`TANDA_LISTA`), la enseña entera y pide el nombre y la contraseña de un administrador, una vez; contesta con
/// `RESPUESTA_ADMIN` (con el usuario vacío si dijo que no). La contraseña la comprueba `sesiones`. Si no es una tanda,
/// falso.
fn de_tanda(q: &[u8; PREGUNTA]) -> bool {
    let campo = |b: &[u8]| -> usize { b.iter().position(|&c| c == 0).unwrap_or(b.len()) };
    // Rama `cuentas-2`: cambiar la propia contraseña (`CLAVE_PROPIA`).
    if de_clave(q) {
        return true;
    }
    // S6b, rama `cuentas`: tras una tanda que creó una cuenta, su código de recuperación, para apuntarlo.
    if &q[24..24 + campo(&q[24..40])] == b"*codigo" {
        ensenar_codigo(q);
        return true;
    }
    if &q[24..24 + campo(&q[24..40])] != b"*tanda" {
        return false;
    }
    // S6b, rama `cuentas`: si la tanda crea una cuenta, su nombre: se pide también su contraseña.
    let nueva = core::str::from_utf8(&q[48..48 + campo(&q[48..64])]).ok().filter(|n| !n.is_empty()).map(|n| limpiar(n, "?"));
    let programa = limpiar(core::str::from_utf8(&q[8..8 + campo(&q[8..24])]).unwrap_or("?"), "un programa");
    // La lista, por páginas. Lo que no llegue entero no se enseña: se contesta que no.
    let mut texto: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    let mut admin = String::new();
    let mut entera = false;
    for _ in 0..64 {
        let mut m = [0u8; 12];
        m[0..4].copy_from_slice(&dashos::protocolo::sesiones::TANDA_LISTA.to_le_bytes());
        m[4..8].copy_from_slice(&q[0..4]);
        m[8..12].copy_from_slice(&(texto.len() as u32).to_le_bytes());
        let (lectura, suyo) = dashos::canal_crear();
        let mut r = [0u8; 256];
        let n = if dashos::canal_escribir(SESIONES, &m, &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], 3_000_000).0 } else { -1 };
        dashos::cerrar(lectura);
        if n < 32 || r[0..4] != [0; 4] {
            break;
        }
        let total = u32::from_le_bytes(r[4..8].try_into().unwrap()) as usize;
        let van = (u32::from_le_bytes(r[8..12].try_into().unwrap()) as usize).min(n as usize - 32);
        admin = limpiar(core::str::from_utf8(&r[16..16 + campo(&r[16..32])]).unwrap_or(""), "");
        texto.extend_from_slice(&r[32..32 + van]);
        if texto.len() >= total || van == 0 {
            entera = texto.len() == total;
            break;
        }
    }
    let lineas: alloc::vec::Vec<String> = String::from_utf8_lossy(&texto).lines().take(tanda::MAX_ACCIONES).map(|l| limpiar(l, "?")).collect();
    confirmar_admin(q, &programa, lineas, admin, entera, nueva);
    true
}

/// S6b, rama `cuentas`: el código de recuperación de una cuenta recién creada ("*codigo": la cuenta donde va el programa
/// y el código tras la pregunta), en la ventana del sistema, para apuntarlo en papel. Se cierra con Intro. Solo aquí:
/// el programa que pidió la cuenta no lo ve. En la imagen de las pruebas se escribe también en la consola (la prueba lo
/// comprueba con el disco); en la de usar, no.
fn ensenar_codigo(q: &[u8; PREGUNTA]) {
    let campo = |b: &[u8]| -> usize { b.iter().position(|&c| c == 0).unwrap_or(b.len()) };
    let cuenta = limpiar(core::str::from_utf8(&q[8..8 + campo(&q[8..24])]).unwrap_or("?"), "?");
    let mut codigo = String::from(core::str::from_utf8(&q[40..40 + dashos::cuentas::LARGO_CODIGO]).unwrap_or("?"));
    if codigos_a_la_consola() {
        println!("  [guardian] (prueba) el código de recuperación de {} es {}", cuenta, codigo);
        // SAFETY: un solo hilo (`Ultimo`).
        let u = unsafe { &mut *ULTIMO_CODIGO.0.get() };
        u.0 = [0; 16];
        u.0[..cuenta.len().min(16)].copy_from_slice(&cuenta.as_bytes()[..cuenta.len().min(16)]);
        u.1.copy_from_slice(&q[40..40 + dashos::cuentas::LARGO_CODIGO]);
    }
    let mut memoria = 0;
    let r = preguntar_dentro("sesiones", &Pregunta::Codigo { cuenta: &cuenta, codigo: &codigo }, &mut memoria);
    dashos::canal_escribir(VENTANAS, &[wayland::canal::TROZO_ADIOS], &[]);
    if memoria > 0 {
        // La imagen con el código, a cero antes de soltarla.
        let tamano = (DIALOGO_CODIGO.0 * DIALOGO_CODIGO.1 * 4) as usize;
        unsafe { core::slice::from_raw_parts_mut(VA as *mut u8, tamano) }.fill(0);
    }
    dashos::memoria_quitar(VA);
    if memoria > 0 {
        dashos::cerrar(memoria);
    }
    // SAFETY: solo letras ASCII; se ponen a cero.
    unsafe { codigo.as_bytes_mut().fill(0) };
    println!("  [guardian] código de recuperación de {}: {}", cuenta, if r == pf::RESPUESTA_SIEMPRE { "visto" } else { "la ventana se cerró sin Intro" });
}

/// S36, decisión 1: una acción de administrador suelta (la de `ADMINISTRAR`, "*comprobar", "*matar"...) es una tanda de
/// una: la misma ventana, con su frase. `sesiones` pone tras la pregunta el número del proceso de "matar".
fn de_accion(q: &[u8; PREGUNTA]) -> bool {
    let campo = |b: &[u8]| -> usize { b.iter().position(|&c| c == 0).unwrap_or(b.len()) };
    let Some(accion) = q[24..24 + campo(&q[24..40])].strip_prefix(b"*") else { return false };
    let Some(cual) = tanda::ACCIONES.iter().position(|a| a.0.as_bytes() == accion) else { return false };
    let programa = limpiar(core::str::from_utf8(&q[8..8 + campo(&q[8..24])]).unwrap_or("?"), "un programa");
    let objetivo = u64::from_le_bytes(q[40..48].try_into().unwrap());
    let frase = alloc::format!("1. {}", tanda::Frase { accion: tanda::Accion::de(cual, objetivo), programa: "", persona: "" });
    confirmar_admin(q, &programa, alloc::vec![frase], String::new(), true, None);
    true
}

/// La ventana de una tanda (o de una acción suelta) y la respuesta a `sesiones`. Con `nueva` (el nombre de la cuenta que
/// crea), una caja más: su contraseña.
fn confirmar_admin(q: &[u8; PREGUNTA], programa: &str, lineas: alloc::vec::Vec<String>, admin: String, entera: bool, nueva: Option<String>) {
    let nombre = core::cell::RefCell::new(admin);
    let clave = core::cell::RefCell::new(String::new());
    let otra = core::cell::RefCell::new(String::new());
    let foco = core::cell::Cell::new(if nombre.borrow().is_empty() { 0u8 } else { 1 });
    let si = entera && !lineas.is_empty() && {
        let mut memoria = 0;
        let nueva = nueva.as_deref().map(|n| (n, &otra));
        let r = preguntar_dentro(programa, &Pregunta::Tanda { lineas: &lineas, nombre: &nombre, clave: &clave, foco: &foco, nueva }, &mut memoria);
        dashos::canal_escribir(VENTANAS, &[wayland::canal::TROZO_ADIOS], &[]);
        dashos::memoria_quitar(VA);
        if memoria > 0 {
            dashos::cerrar(memoria);
        }
        r == pf::RESPUESTA_SIEMPRE
    };
    let (usuario, mut clave, mut otra) = if si { (nombre.into_inner(), clave.into_inner(), otra.into_inner()) } else { (String::new(), clave.into_inner(), otra.into_inner()) };
    let c = if si { clave.as_bytes() } else { b"" };
    let o = if si { otra.as_bytes() } else { b"" };
    let (u, c, o) = (&usuario.as_bytes()[..usuario.len().min(16)], &c[..c.len().min(128)], &o[..o.len().min(64)]);
    let mut r = [0u8; 16 + 16 + 128 + 4 + 64];
    r[0..4].copy_from_slice(&RESPUESTA_ADMIN.to_le_bytes());
    r[4..8].copy_from_slice(&q[0..4]);
    r[8..12].copy_from_slice(&(u.len() as u32).to_le_bytes());
    r[12..16].copy_from_slice(&(c.len() as u32).to_le_bytes());
    r[16..16 + u.len()].copy_from_slice(u);
    r[16 + u.len()..16 + u.len() + c.len()].copy_from_slice(c);
    // S6b, rama `cuentas`: y detrás, la contraseña de la cuenta nueva (su largo y ella), si la hay.
    let k = 16 + u.len() + c.len();
    r[k..k + 4].copy_from_slice(&(o.len() as u32).to_le_bytes());
    r[k + 4..k + 4 + o.len()].copy_from_slice(o);
    pedir(&r[..k + 4 + o.len()], &[]);
    // Las contraseñas, fuera de la memoria (S6b, regla 9).
    r.fill(0);
    unsafe { clave.as_bytes_mut().fill(0) };
    unsafe { otra.as_bytes_mut().fill(0) };
    println!("  [guardian] tanda de {}: {}", programa, if si { alloc::format!("se confirma como {}", usuario) } else { String::from("el usuario dice que no") });
}

/// Shell nativo, con S36: espera a que acabe el proceso `pid` (como `esperar_fin`), atendiendo mientras las preguntas
/// que le lleguen: el guion del Shell usa `Permiso` y `Registro TODO`, que preguntan. Los cambios de permiso, al usuario
/// (en la prueba contesta el guion de QEMU); la acción de administrador "registro" del Shell, con la contraseña de Ana,
/// como hace con `pide` y el centro; las demás acciones, negadas; los permisos, al usuario.
fn esperar_atendiendo(pid: u64, max_us: u64, preguntas: u32) -> bool {
    let hasta = dashos::ahora_us() + max_us;
    let mut mirado = 0u64;
    while dashos::ahora_us() < hasta {
        if dashos::ahora_us() >= mirado + 500_000 {
            if !vivo(pid) {
                return true;
            }
            mirado = dashos::ahora_us();
        }
        let mut q = [0u8; PREGUNTA];
        if dashos::canal_leer_plazo(preguntas, &mut q, &mut [], 300_000).0 as usize != PREGUNTA || de_apagar(&q) || de_permiso(&q) || de_tanda(&q) {
            continue;
        }
        let campo = |b: &[u8]| -> usize { b.iter().position(|&c| c == 0).unwrap_or(b.len()) };
        let programa = &q[8..8 + campo(&q[8..24])];
        let servicio = &q[24..24 + campo(&q[24..40])];
        if pf::es_permiso(servicio) {
            let r = preguntar_fichero(core::str::from_utf8(programa).unwrap_or("?"), servicio, &q[40..]);
            let mut m = [0u8; 12];
            m[0..4].copy_from_slice(&RESPUESTA.to_le_bytes());
            m[4..8].copy_from_slice(&q[0..4]);
            m[8..12].copy_from_slice(&r.to_le_bytes());
            pedir(&m, &[]);
            continue;
        }
        let mut r = [0u8; 64];
        let largo = if servicio.first() == Some(&b'*') {
            let (usuario, clave): (&[u8], &[u8]) = if programa == b"Shell" && servicio == b"*registro" { (b"Ana", b"clave de Ana") } else { (b"", b"") };
            r[0..4].copy_from_slice(&RESPUESTA_ADMIN.to_le_bytes());
            r[4..8].copy_from_slice(&q[0..4]);
            r[8..12].copy_from_slice(&(usuario.len() as u32).to_le_bytes());
            r[12..16].copy_from_slice(&(clave.len() as u32).to_le_bytes());
            r[16..16 + usuario.len()].copy_from_slice(usuario);
            r[16 + usuario.len()..16 + usuario.len() + clave.len()].copy_from_slice(clave);
            println!("  [guardian] {} pide {}: {}", core::str::from_utf8(programa).unwrap_or("?"), core::str::from_utf8(&servicio[1..]).unwrap_or("?"),
                if usuario.is_empty() { "no" } else { "contesta como Ana" });
            16 + usuario.len() + clave.len()
        } else {
            let si = preguntar(core::str::from_utf8(programa).unwrap_or("?"), core::str::from_utf8(servicio).unwrap_or("?"));
            r[0..4].copy_from_slice(&RESPUESTA.to_le_bytes());
            r[4..8].copy_from_slice(&q[0..4]);
            r[8..12].copy_from_slice(&(si as u32).to_le_bytes());
            12
        };
        pedir(&r[..largo], &[]);
        r.fill(0);
    }
    false
}

/// S28: la guardia de ficheros de la caja Amiga. `programa` es quien pide en `sesiones` (la caja), `permiso` el permiso
/// sin llave (`ejec:...`) y `detalle` el programa 68k y el fichero (`preguntas_ficheros::detalle`), que no se fían: se
/// enseñan limpios. Lo mismo que `preguntar`, con tres respuestas: Mayús+S siempre (se guarda), E solo esta vez y N no.
/// Devuelve la respuesta para `sesiones` (`RESPUESTA_*`).
fn preguntar_fichero(programa: &str, permiso: &[u8], detalle: &[u8]) -> u32 {
    let (prog68k, fichero) = pf::leer_detalle(detalle);
    let (prog68k, fichero) = (limpiar(prog68k, "un programa"), limpiar(fichero, "un fichero"));
    let frase = pf::clase_de(permiso).map_or("algo protegido", |c| c.1);
    let quien = if programa == "caja-amiga" { "la caja Amiga" } else { programa };
    let mut memoria = 0;
    let r = preguntar_dentro(quien, &Pregunta::Fichero { prog68k: &prog68k, fichero: &fichero, frase }, &mut memoria);
    dashos::canal_escribir(VENTANAS, &[wayland::canal::TROZO_ADIOS], &[]);
    dashos::memoria_quitar(VA);
    if memoria > 0 {
        dashos::cerrar(memoria);
    }
    let dice = match r {
        pf::RESPUESTA_SIEMPRE => "sí, siempre",
        pf::RESPUESTA_ESTA_VEZ => "sí, solo esta vez",
        _ => "no",
    };
    println!("  [guardian] {}: ¿{} puede cambiar {}? El usuario dice {}", quien, prog68k, fichero, dice);
    r
}

/// Lo que llega de fuera, para enseñarlo: sin letras de control y, si queda vacío, `si_no`.
fn limpiar(t: &str, si_no: &str) -> String {
    let t: String = t.chars().map(|c| if c.is_control() { '?' } else { c }).collect();
    if t.trim().is_empty() { String::from(si_no) } else { t }
}

/// Lo que se pregunta: un servicio (U4) o un cambio en un fichero de la caja Amiga (S28).
enum Pregunta<'a> {
    Servicio(&'a str),
    Fichero { prog68k: &'a str, fichero: &'a str, frase: &'a str },
    /// S6b, S36: dar o quitar un permiso a un programa, que pide otro (la orden `Permiso` del Shell).
    Cambio { dar: bool, programa: &'a str, permiso: &'a str },
    /// S36, decisión 1: una tanda de acciones de administrador (o una sola): la lista entera en llano, y el nombre y la
    /// contraseña del administrador, que se teclean aquí (Tab cambia de uno a otro).
    /// S6b, rama `cuentas`: y si crea una cuenta, su nombre y su contraseña (una caja más). `foco`: la caja que se
    /// teclea (0 el administrador, 1 su contraseña, 2 la de la cuenta nueva).
    Tanda {
        lineas: &'a [String],
        nombre: &'a core::cell::RefCell<String>,
        clave: &'a core::cell::RefCell<String>,
        foco: &'a core::cell::Cell<u8>,
        nueva: Option<(&'a str, &'a core::cell::RefCell<String>)>,
    },
    /// S6b, rama `cuentas`: el código de recuperación de una cuenta nueva, para apuntarlo.
    Codigo { cuenta: &'a str, codigo: &'a str },
    /// Rama `cuentas-2`: cambiar la propia contraseña: la cuenta, las tres cajas (la de ahora, la nueva y repítela), la
    /// que se teclea y lo que dijo `sesiones` del intento de antes (vacío si nada).
    Clave { cuenta: &'a str, cajas: &'a [core::cell::RefCell<String>; 3], foco: &'a core::cell::Cell<u8>, aviso: &'a core::cell::RefCell<String> },
}

/// Rama `cuentas-2`: si `q` es un cambio de la propia contraseña ("*clave", de `CLAVE_PROPIA`), la ventana con la de ahora
/// y la nueva dos veces; cada intento va a `sesiones` (`RESPUESTA_CLAVE`), que la cambia o dice qué pasa (la de ahora no
/// es, hay que esperar, la nueva es corta): entonces se vuelve a enseñar, con el aviso. Esc, o dos minutos sin acabar,
/// es que no. Si no es eso, falso.
fn de_clave(q: &[u8; PREGUNTA]) -> bool {
    use core::cell::{Cell, RefCell};
    use dashos::protocolo::sesiones::{NO_EXISTE, OCUPADO, PROTEGIDO, RESPUESTA_CLAVE};
    let campo = |b: &[u8]| -> usize { b.iter().position(|&c| c == 0).unwrap_or(b.len()) };
    if &q[24..24 + campo(&q[24..40])] != b"*clave" {
        return false;
    }
    let cuenta = limpiar(core::str::from_utf8(&q[8..8 + campo(&q[8..24])]).unwrap_or("?"), "?");
    let quien = limpiar(core::str::from_utf8(&q[40..40 + campo(&q[40..56])]).unwrap_or("?"), "un programa");
    let cajas = [RefCell::new(String::new()), RefCell::new(String::new()), RefCell::new(String::new())];
    let foco = Cell::new(0u8);
    let aviso = RefCell::new(String::new());
    let mandar = |vieja: &[u8], nueva: &[u8]| -> (i32, u32) {
        let (v, n) = (&vieja[..vieja.len().min(100)], &nueva[..nueva.len().min(100)]);
        let mut m = [0u8; 16 + 200];
        m[0..4].copy_from_slice(&RESPUESTA_CLAVE.to_le_bytes());
        m[4..8].copy_from_slice(&q[0..4]);
        m[8..12].copy_from_slice(&(v.len() as u32).to_le_bytes());
        m[12..16].copy_from_slice(&(n.len() as u32).to_le_bytes());
        m[16..16 + v.len()].copy_from_slice(v);
        m[16 + v.len()..16 + v.len() + n.len()].copy_from_slice(n);
        let r = pedir_8(&m[..16 + v.len() + n.len()]);
        m.fill(0);
        r
    };
    // Unas cuantas vueltas como mucho (cada una, una ventana nueva con el aviso).
    for _ in 0..12 {
        let mut memoria = 0;
        let r = preguntar_dentro(&quien, &Pregunta::Clave { cuenta: &cuenta, cajas: &cajas, foco: &foco, aviso: &aviso }, &mut memoria);
        dashos::canal_escribir(VENTANAS, &[wayland::canal::TROZO_ADIOS], &[]);
        dashos::memoria_quitar(VA);
        if memoria > 0 {
            dashos::cerrar(memoria);
        }
        if r != pf::RESPUESTA_SIEMPRE {
            mandar(b"", b"");
            println!("  [guardian] cambio de contraseña de {}: el usuario dice que no", cuenta);
            break;
        }
        let (estado, espera) = mandar(cajas[0].borrow().as_bytes(), cajas[1].borrow().as_bytes());
        let dice = match estado {
            0 => {
                println!("  [guardian] cambio de contraseña de {}: cambiada", cuenta);
                break;
            }
            PROTEGIDO => {
                borrar(&mut cajas[0].borrow_mut());
                foco.set(0);
                String::from("La contraseña de ahora no es esa.")
            }
            OCUPADO => {
                borrar(&mut cajas[0].borrow_mut());
                foco.set(0);
                alloc::format!("Demasiados intentos. Espera {} s y prueba otra vez.", espera)
            }
            e if dashos::cuentas::Fallo::de_estado(e) == Some(dashos::cuentas::Fallo::NuevaCorta) => {
                borrar(&mut cajas[1].borrow_mut());
                borrar(&mut cajas[2].borrow_mut());
                foco.set(1);
                String::from("La nueva es corta: 8 letras o más.")
            }
            NO_EXISTE => {
                println!("  [guardian] cambio de contraseña de {}: sesiones ya no lo espera", cuenta);
                break;
            }
            e => {
                println!("  [guardian] cambio de contraseña de {}: no se pudo ({})", cuenta, e);
                break;
            }
        };
        println!("  [guardian] cambio de contraseña de {}: {}", cuenta, dice);
        *aviso.borrow_mut() = dice;
    }
    for c in cajas.iter() {
        borrar(&mut c.borrow_mut());
    }
    true
}

/// S36, decisión 1: el alto del diálogo de una tanda de `n` líneas (una por acción). Con una cuenta nueva, una caja más.
fn alto_tanda(n: usize, nueva: bool) -> i32 {
    350 + 20 * n.min(tanda::MAX_ACCIONES) as i32 + if nueva { 44 } else { 0 }
}

fn preguntar_dentro(programa: &str, pregunta: &Pregunta, memoria: &mut u32) -> u32 {
    const CAPAS: u32 = 20;
    const CAPA: u32 = 21;
    // El diálogo; su sombra la pone el compositor, como a las demás capas. El de un fichero, más grande.
    let fichero = matches!(pregunta, Pregunta::Fichero { .. });
    // S6b, S36: el nombre interno del permiso, solo en "Detalles" (con la D). El de un cambio de permiso (la orden
    // `Permiso`) es el diálogo grande, como el de un fichero de la caja; el de un permiso, el de siempre (los detalles
    // van en la línea de "Contesta con el teclado").
    let con_detalles = matches!(pregunta, Pregunta::Cambio { .. }) || matches!(pregunta, Pregunta::Servicio(s) if !s.starts_with('*'));
    let mut detalles = false;
    let (ancho_d, alto_d) = match pregunta {
        Pregunta::Tanda { lineas, nueva, .. } => (DIALOGO_TANDA, alto_tanda(lineas.len(), nueva.is_some())),
        Pregunta::Codigo { .. } => DIALOGO_CODIGO,
        Pregunta::Clave { .. } => DIALOGO_CLAVE,
        _ if fichero || matches!(pregunta, Pregunta::Cambio { .. }) => DIALOGO_FICHERO,
        _ => DIALOGO,
    };
    let tanda = matches!(pregunta, Pregunta::Tanda { .. } | Pregunta::Codigo { .. } | Pregunta::Clave { .. });
    const ESPERA_US: u64 = 1_000_000;
    /// El tema del sistema (S10c): llega al enlazar, antes que la configuración de la capa.
    const TEMA: u32 = 22;
    let mut tema = Tema::OSCURO;
    let (mut fuente, mut medio) = (Fuente::regular(), Fuente::medium());
    let Some(mut c) = Cliente::conectar() else { return pf::RESPUESTA_NO };
    let Some(globales) = c.globales() else { return pf::RESPUESTA_NO };
    let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
    let (Some(gc), Some(gs), Some(ga), Some(gl)) =
        (global("wl_compositor"), global("wl_shm"), global("wl_seat"), global("zwlr_layer_shell_v1"))
    else {
        println!("  [guardian] el compositor no le da capas");
        return pf::RESPUESTA_NO;
    };
    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gl).texto(Some("zwlr_layer_shell_v1")).uint(4).uint(CAPAS).fin();
    if let Some(gt) = global("dashos_tema_v1") {
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gt).texto(Some("dashos_tema_v1")).uint(1).uint(TEMA).fin();
    }
    e.empezar(ASIENTO, wl_seat::req::GET_KEYBOARD).uint(TECLADO).fin();
    e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUPERFICIE).fin();
    // Encima de todo (3), sin anclas (en el centro), con el teclado en exclusiva (1).
    e.empezar(CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(CAPA).uint(SUPERFICIE).uint(0).uint(3).texto(Some("guardian")).fin();
    e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_SIZE).uint(ancho_d as u32).uint(alto_d as u32).fin();
    e.empezar(CAPA, zwlr_layer_surface_v1::req::SET_KEYBOARD_INTERACTIVITY).uint(1).fin();
    e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
    c.mandar(&e);
    let mut teclado = Teclado::new();
    // Desde cuándo se ve (con el sync de su primer cuadro).
    let mut vista: Option<u64> = None;
    // guion-panel: si la ventana tiene el teclado, y se dice (tras "en pantalla", para que en la salida vaya siempre detrás
    // de la línea de su ventana): quien la conteste (el guion de las pruebas) espera a tenerlo, y no teclea en otra.
    let mut con_teclado = false;
    // Teclear una contraseña lleva más que una tecla: la de una tanda espera dos minutos.
    let fin = dashos::ahora_us() + if tanda { 120_000_000 } else { 30_000_000 };
    loop {
        if dashos::ahora_us() > fin {
            return pf::RESPUESTA_NO;
        }
        let Ok(ev) = c.evento_plazo(100_000) else { return pf::RESPUESTA_NO };
        let Some((o, orden, cuerpo)) = ev else { continue };
        let palabra = |i: usize| u32::from_le_bytes(cuerpo[4 * i..4 * i + 4].try_into().unwrap());
        match (o, orden) {
            (CAPA, zwlr_layer_surface_v1::ev::CONFIGURE) if vista.is_none() => {
                let tamano = (ancho_d * alto_d * 4) as usize;
                let m = dashos::memoria_crear(tamano);
                if (m as i32) <= 0 {
                    return pf::RESPUESTA_NO;
                }
                *memoria = m as u32;
                if dashos::memoria_proyectar(m as u32, VA, true) != 0 {
                    return pf::RESPUESTA_NO;
                }
                let px = unsafe { core::slice::from_raw_parts_mut(VA as *mut u32, tamano / 4) };
                pintar_pregunta(px, ancho_d, alto_d, programa, pregunta, detalles, &mut fuente, &mut medio, &tema);
                let copia = dashos::llave_duplicar(m as u32, dashos::LEER | dashos::TRANSFERIR, 0);
                let mut e = Escritor::new();
                e.empezar(CAPA, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(POOL).llave(copia as u32).int(tamano as i32).fin();
                e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER).int(0).int(ancho_d).int(alto_d).int(ancho_d * 4).uint(0).fin();
                e.empezar(SUPERFICIE, wl_surface::req::ATTACH).uint(BUFER).int(0).int(0).fin();
                e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(ancho_d).int(alto_d).fin();
                e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
                e.empezar(1, wl_display::req::SYNC).uint(SYNC).fin();
                c.mandar(&e);
            }
            (CAPA, zwlr_layer_surface_v1::ev::CONFIGURE) => {
                let mut e = Escritor::new();
                e.empezar(CAPA, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                c.mandar(&e);
            }
            (CAPA, zwlr_layer_surface_v1::ev::CLOSED) => return pf::RESPUESTA_NO,
            (TEMA, wayland::tablas::dashos_tema_v1::ev::TEMA) => tema = Tema::del_sistema(palabra(0) | 0xff00_0000, palabra(1) != 0),
            (SYNC, wayland::tablas::wl_callback::ev::DONE) if vista.is_none() => {
                vista = Some(dashos::ahora_us());
                match pregunta {
                    // Apagar o reiniciar con otras sesiones abiertas: una confirmación.
                    Pregunta::Servicio(servicio) if servicio.starts_with('*') => println!("  [guardian] confirmación en pantalla: ¿{}?", &servicio[1..]),
                    Pregunta::Servicio(servicio) => println!("  [guardian] pregunta en pantalla: ¿{} puede usar {}?", programa, servicio),
                    Pregunta::Fichero { prog68k, fichero, frase } => {
                        println!("  [guardian] pregunta en pantalla: {}: {} quiere cambiar {}, {}", programa, prog68k, fichero, frase)
                    }
                    Pregunta::Cambio { dar, programa: p, permiso } => {
                        println!("  [guardian] pregunta en pantalla: cambiar permiso: {} quiere {} a {} {}", programa, if *dar { "dar" } else { "quitar" }, p, permiso)
                    }
                    Pregunta::Tanda { lineas, nombre, nueva, .. } => {
                        let con = nueva.map_or(String::new(), |(n, _)| alloc::format!(" (y la contraseña de la cuenta nueva {})", n));
                        println!("  [guardian] tanda en pantalla: {} pide {} (administrador propuesto: {}){}: {}", programa, lineas.len(), nombre.borrow(), con, lineas.join(" | "))
                    }
                    Pregunta::Codigo { cuenta, .. } => println!("  [guardian] código de recuperación en pantalla: {}", cuenta),
                    Pregunta::Clave { cuenta, aviso, .. } => {
                        let a = aviso.borrow();
                        println!("  [guardian] cambio de contraseña en pantalla: {} (lo pide {}){}", cuenta, programa, if a.is_empty() { String::new() } else { alloc::format!("; {}", a) })
                    }
                }
                if con_teclado {
                    println!("  [guardian] ventana del sistema: tiene el teclado");
                }
            }
            (TECLADO, wl_keyboard::ev::ENTER) => {
                con_teclado = true;
                if vista.is_some() {
                    println!("  [guardian] ventana del sistema: tiene el teclado");
                }
            }
            (TECLADO, wl_keyboard::ev::LEAVE) => {
                con_teclado = false;
                if vista.is_some() {
                    println!("  [guardian] ventana del sistema: pierde el teclado");
                }
            }
            (TECLADO, wl_keyboard::ev::KEYMAP) => {
                if let Some(l) = c.juntador.tomar_llave() {
                    dashos::cerrar(l);
                }
            }
            (TECLADO, wl_keyboard::ev::KEY) => {
                let tecla = teclado.tecla(palabra(2), palabra(3) == 1);
                let a_tiempo = vista.is_some_and(|v| dashos::ahora_us() - v >= ESPERA_US);
                // S36, decisión 1: en una tanda se teclea el nombre y la contraseña; Intro confirma y Esc dice que no. Lo
                // del primer segundo no cuenta (quien escribía en otra ventana no confirma sin querer).
                // S6b, rama `cuentas`: el código de recuperación se cierra con Intro (pasado el primer segundo).
                if let Pregunta::Codigo { .. } = pregunta {
                    match tecla.filter(|_| a_tiempo) {
                        Some(Tecla::Intro) => return pf::RESPUESTA_SIEMPRE,
                        _ => continue,
                    }
                }
                // Rama `cuentas-2`: cambiar la propia contraseña: tres cajas; Intro con todas puestas la manda, si las dos
                // nuevas son iguales; si falta alguna, pasa a la siguiente. Tras "la de ahora no es esa", las nuevas
                // siguen puestas: basta teclear la de ahora e Intro.
                if let Pregunta::Clave { cajas, foco, aviso, .. } = pregunta {
                    let Some(tecla) = tecla.filter(|_| a_tiempo) else { continue };
                    let f = foco.get() as usize;
                    match tecla {
                        Tecla::Escape => return pf::RESPUESTA_NO,
                        Tecla::Intro if cajas.iter().any(|c| c.borrow().is_empty()) => foco.set(((f + 1) % 3) as u8),
                        Tecla::Intro if *cajas[1].borrow() != *cajas[2].borrow() => {
                            borrar(&mut cajas[1].borrow_mut());
                            borrar(&mut cajas[2].borrow_mut());
                            foco.set(1);
                            *aviso.borrow_mut() = String::from("Las dos nuevas no son iguales.");
                            println!("  [guardian] cambio de contraseña: las dos nuevas no son iguales");
                        }
                        Tecla::Intro => return pf::RESPUESTA_SIEMPRE,
                        Tecla::Tabulador | Tecla::Abajo => foco.set(((f + 1) % 3) as u8),
                        Tecla::TabuladorAtras | Tecla::Arriba => foco.set(((f + 2) % 3) as u8),
                        Tecla::Retroceso => {
                            cajas[f].borrow_mut().pop();
                        }
                        Tecla::Letra(ch) if cajas[f].borrow().len() < 100 => cajas[f].borrow_mut().push(ch),
                        _ => continue,
                    }
                    if *memoria != 0 {
                        let tamano = (ancho_d * alto_d * 4) as usize;
                        let px = unsafe { core::slice::from_raw_parts_mut(VA as *mut u32, tamano / 4) };
                        pintar_pregunta(px, ancho_d, alto_d, programa, pregunta, detalles, &mut fuente, &mut medio, &tema);
                        let mut e = Escritor::new();
                        e.empezar(SUPERFICIE, wl_surface::req::ATTACH).uint(BUFER).int(0).int(0).fin();
                        e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(ancho_d).int(alto_d).fin();
                        e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
                        c.mandar(&e);
                    }
                    continue;
                }
                if let Pregunta::Tanda { nombre, clave, foco, nueva, .. } = pregunta {
                    let Some(tecla) = tecla.filter(|_| a_tiempo) else { continue };
                    let cajas = if nueva.is_some() { 3 } else { 2 };
                    let caja = |f: u8| -> &core::cell::RefCell<String> {
                        match (f, nueva) {
                            (0, _) => nombre,
                            (2, Some((_, otra))) => otra,
                            _ => clave,
                        }
                    };
                    let llenas = !clave.borrow().is_empty() && !nombre.borrow().is_empty() && nueva.is_none_or(|(_, o)| !o.borrow().is_empty());
                    match tecla {
                        Tecla::Intro if llenas => return pf::RESPUESTA_SIEMPRE,
                        // Intro con una caja vacía pasa a la siguiente.
                        Tecla::Intro => foco.set((foco.get() + 1) % cajas),
                        Tecla::Escape => return pf::RESPUESTA_NO,
                        Tecla::Tabulador => foco.set((foco.get() + 1) % cajas),
                        Tecla::TabuladorAtras => foco.set((foco.get() + cajas - 1) % cajas),
                        Tecla::Retroceso => {
                            caja(foco.get()).borrow_mut().pop();
                        }
                        Tecla::Letra(ch) if foco.get() == 0 && nombre.borrow().chars().count() < 16 => nombre.borrow_mut().push(ch),
                        Tecla::Letra(ch) if foco.get() == 1 && clave.borrow().len() < 128 => clave.borrow_mut().push(ch),
                        Tecla::Letra(ch) if foco.get() == 2 && caja(2).borrow().len() < 64 => caja(2).borrow_mut().push(ch),
                        _ => continue,
                    }
                    if *memoria != 0 {
                        let tamano = (ancho_d * alto_d * 4) as usize;
                        let px = unsafe { core::slice::from_raw_parts_mut(VA as *mut u32, tamano / 4) };
                        pintar_pregunta(px, ancho_d, alto_d, programa, pregunta, detalles, &mut fuente, &mut medio, &tema);
                        let mut e = Escritor::new();
                        e.empezar(SUPERFICIE, wl_surface::req::ATTACH).uint(BUFER).int(0).int(0).fin();
                        e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(ancho_d).int(alto_d).fin();
                        e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
                        c.mandar(&e);
                    }
                    continue;
                }
                match tecla {
                    Some(Tecla::Letra(ch)) if !a_tiempo => {
                        println!("  [guardian] tecla {:?} antes de tiempo: no cuenta", ch);
                    }
                    // "Siempre" (ficheros de la caja) y "Sí" (permisos de servicios) piden Mayúsculas y S (decisiones del
                    // usuario del 2026-11-13): una "s" suelta, tecleando en otra ventana cuando sale la pregunta, no debe
                    // dar un permiso.
                    Some(Tecla::Letra('S')) => return pf::RESPUESTA_SIEMPRE,
                    Some(Tecla::Letra('e' | 'E')) if fichero => return pf::RESPUESTA_ESTA_VEZ,
                    // S6b, S36: la D enseña (o esconde) el nombre interno del permiso: el detalle, a una tecla.
                    Some(Tecla::Letra('d' | 'D')) if con_detalles && *memoria != 0 => {
                        detalles = !detalles;
                        let tamano = (ancho_d * alto_d * 4) as usize;
                        let px = unsafe { core::slice::from_raw_parts_mut(VA as *mut u32, tamano / 4) };
                        pintar_pregunta(px, ancho_d, alto_d, programa, pregunta, detalles, &mut fuente, &mut medio, &tema);
                        let mut e = Escritor::new();
                        e.empezar(SUPERFICIE, wl_surface::req::ATTACH).uint(BUFER).int(0).int(0).fin();
                        e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(ancho_d).int(alto_d).fin();
                        e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
                        c.mandar(&e);
                        println!("  [guardian] detalles de la pregunta: {}", if detalles { "a la vista" } else { "escondidos" });
                    }
                    Some(Tecla::Letra('n' | 'N')) => return pf::RESPUESTA_NO,
                    _ => {}
                }
            }
            (1, wl_display::ev::ERROR) => {
                println!("  [guardian] error de protocolo en la pregunta");
                return pf::RESPUESTA_NO;
            }
            _ => {}
        }
    }
}

/// Lo que mide el diálogo de la pregunta (Material 3: de 280 a 560 de ancho). El de un fichero de la caja Amiga (S28),
/// más ancho y alto: el texto, en hasta cuatro líneas, y tres respuestas.
const DIALOGO: (i32, i32) = (440, 208);
const DIALOGO_FICHERO: (i32, i32) = (520, 268);
/// S36, decisión 1: el ancho del de una tanda (el alto, según cuántas acciones: `alto_tanda`).
const DIALOGO_TANDA: i32 = 560;
/// S6b, rama `cuentas`: el del código de recuperación.
const DIALOGO_CODIGO: (i32, i32) = (520, 300);
/// Rama `cuentas-2`: el de cambiar la propia contraseña.
const DIALOGO_CLAVE: (i32, i32) = (560, 400);
/// Lo más largo que se enseña de un nombre que llega de fuera (el resto, "…").
const MAX_NOMBRE_VISTO: usize = 60;

/// Pinta la pregunta de U4 como un diálogo de Material 3, con los colores del tema del sistema (S10c): el contenedor
/// alto con esquinas de 28 (fuera, transparente; la sombra, del compositor), 24 de relleno, el título (headline small, 24), el texto (body
/// medium, 14, en el color suave) y abajo a la derecha las respuestas. No se pulsan (`guardian` solo atiende al
/// teclado): cada una lleva su tecla en una tecla dibujada, con contorno, y su nombre en el primario.
/// S28: la de un fichero de la caja Amiga: "Programa de Amiga" de título, y "<programa> quiere cambiar <fichero>, un
/// programa ejecutable. ¿Lo dejas?" partido en líneas; las respuestas, "Mayús+S Siempre", "E Esta vez" y "N No".
#[allow(clippy::too_many_arguments)]
fn pintar_pregunta(px: &mut [u32], ancho: i32, alto: i32, programa: &str, pregunta: &Pregunta, detalles: bool, fuente: &mut Fuente, medio: &mut Fuente, t: &Tema) {
    let mut l = Lienzo::new(px, ancho, alto, ancho as usize);
    l.borrar(Rect::new(0, 0, ancho, alto), 0);
    let caja = Rect::new(0, 0, ancho, alto);
    l.redondeado(caja, kit::tema::RADIO_DIALOGO, t.contenedor_alto());
    let x = caja.x + 24;
    let ancho_texto = caja.w - 48;
    // El título: la pregunta. Si el nombre es largo y no cabe a 24, a 22 y luego a 20. Apagar y reiniciar (con otras
    // sesiones abiertas), con su propio texto.
    let titulo = match pregunta {
        Pregunta::Servicio("*apagar") => String::from("¿Apagar el equipo?"),
        Pregunta::Servicio("*reiniciar") => String::from("¿Reiniciar el equipo?"),
        Pregunta::Servicio(_) => alloc::format!("¿Dar permiso a \"{}\"?", programa),
        Pregunta::Fichero { .. } => String::from("Programa de Amiga"),
        Pregunta::Cambio { .. } => String::from("¿Cambiar un permiso?"),
        Pregunta::Tanda { lineas, .. } if lineas.len() == 1 => String::from("Acción de administrador"),
        Pregunta::Tanda { lineas, .. } => alloc::format!("{} acciones de administrador", lineas.len()),
        Pregunta::Codigo { cuenta, .. } => alloc::format!("Código de recuperación de {}", recortar(cuenta).trim_matches('"')),
        Pregunta::Clave { cuenta, .. } => alloc::format!("Cambiar la contraseña de {}", recortar(cuenta).trim_matches('"')),
    };
    let tam = [24u32, 22, 20].into_iter().find(|&tam| fuente.medir(&titulo, tam).ancho <= ancho_texto).unwrap_or(20);
    l.recortar(Rect::new(x, caja.y, ancho_texto, caja.h));
    fuente.dibujar(&mut l, x, caja.y + 24 + 24, &titulo, tam, t.sobre_superficie);
    // El texto, a 16 del título (de 32 de alto); líneas de 20.
    let y = caja.y + 24 + 32 + 16;
    let (lineas, respuestas): (alloc::vec::Vec<String>, &[(&str, &str)]) = match pregunta {
        Pregunta::Servicio(servicio) if servicio.starts_with('*') => (
            alloc::vec![String::from("Hay otras sesiones abiertas y se cerrarán."), String::from("Contesta con el teclado.")],
            &[("Mayús+S", "Sí"), ("N", "No")],
        ),
        // S6b, S36: la frase en llano del permiso; el nombre interno, solo en "Detalles".
        Pregunta::Servicio(servicio) => {
            let mut v = partir(fuente, &alloc::format!("Quiere {}.", pt::frase(servicio)), ancho_texto, 2);
            v.push(if detalles {
                alloc::format!("Detalles: permiso \"{}\" (D: esconder).", recortar(servicio).trim_matches('"'))
            } else {
                String::from("Contesta con el teclado (D: detalles).")
            });
            (v, &[("Mayús+S", "Sí"), ("N", "No")])
        }
        Pregunta::Cambio { dar, programa: p, permiso } => {
            let texto = alloc::format!(
                "{} quiere {} {} {} {}.",
                recortar(programa),
                if *dar { "dar a" } else { "quitar a" },
                recortar(p),
                if *dar { "permiso para" } else { "el permiso de" },
                pt::frase(permiso)
            );
            let mut v = partir(fuente, &texto, ancho_texto, 3);
            v.push(String::from(if detalles { "Contesta con el teclado (D: esconder los detalles)." } else { "Contesta con el teclado (D: detalles)." }));
            if detalles {
                v.push(alloc::format!("Detalles: permiso \"{}\"", recortar(permiso).trim_matches('"')));
            }
            (v, &[("Mayús+S", "Sí"), ("N", "No")])
        }
        // S36, decisión 1: quién lo pide, la lista entera (una línea por acción, numerada) y cómo se contesta. El
        // nombre y la contraseña van debajo, en sus cajas.
        // El código va aparte, más grande, en el hueco de las dos líneas vacías.
        Pregunta::Codigo { .. } => (
            alloc::vec![
                String::from("Apúntalo en papel y dáselo a quien es la cuenta: es la"),
                String::from("otra llave de su Home:, por si olvida la contraseña."),
                String::new(),
                String::new(),
                String::new(),
                String::from("No se vuelve a enseñar. Intro: ya lo he apuntado."),
            ],
            &[("Intro", "Hecho")],
        ),
        // Rama `cuentas-2`: quién lo pide, la frase de la cuenta (si no sale, no es el sistema) y el aviso o las teclas.
        Pregunta::Clave { cuenta, aviso, .. } => {
            let mut v = partir(fuente, &alloc::format!("{} pide cambiar tu contraseña. Lo que hay en tu Home: no cambia.", recortar(programa).trim_matches('"')), ancho_texto, 2);
            match FRASES.iter().find(|f| f.0 == *cuenta) {
                Some(f) => v.push(alloc::format!("Tu frase: \"{}\"", f.1)),
                None => v.push(String::new()),
            }
            let a = aviso.borrow();
            v.push(if a.is_empty() { String::from("Tab: la caja siguiente. Intro: cambiarla. Esc: no.") } else { a.clone() });
            (v, &[("Intro", "Cambiar"), ("Esc", "No")])
        }
        Pregunta::Tanda { lineas: acciones, nombre, foco, nueva, .. } => {
            let mut v = alloc::vec![alloc::format!("{} pide hacer esto, en este orden:", recortar(programa).trim_matches('"'))];
            for a in acciones.iter() {
                v.extend(partir(fuente, a, ancho_texto, 1));
            }
            v.push(String::new());
            // La frase que eligió el administrador (S6b): si no sale, no es el sistema.
            let nombre = nombre.borrow();
            match FRASES.iter().find(|f| f.0 == nombre.as_str()) {
                Some(f) => v.push(alloc::format!("Tu frase: \"{}\"", f.1)),
                None => v.push(String::from("Sin frase: escribe el nombre de un administrador.")),
            }
            v.push(String::from(match (foco.get(), nueva.is_some()) {
                (0, _) => "Tab: a la contraseña. Intro: hacerlo. Esc: no.",
                (1, true) => "Tab: a la contraseña de la cuenta nueva. Intro: hacerlo. Esc: no.",
                _ => "Tab: cambiar de administrador. Intro: hacerlo. Esc: no.",
            }));
            (v, &[("Intro", "Hacer"), ("Esc", "No")])
        }
        Pregunta::Fichero { prog68k, fichero, frase } => {
            let texto = alloc::format!(
                "{} quiere cambiar {}, {}. ¿Lo dejas? Lo pide {}.",
                recortar(prog68k),
                recortar(fichero),
                frase,
                programa
            );
            let mut v = partir(fuente, &texto, ancho_texto, 4);
            v.push(String::from("Contesta con el teclado."));
            (v, &[("Mayús+S", "Siempre"), ("E", "Esta vez"), ("N", "No")])
        }
    };
    // Rama `cuentas-2`: el aviso de cambiar la contraseña (la última línea), en el color de error.
    let con_aviso = matches!(pregunta, Pregunta::Clave { aviso, .. } if !aviso.borrow().is_empty());
    for (i, linea) in lineas.iter().enumerate() {
        let color = if con_aviso && i + 1 == lineas.len() { t.error } else { t.sobre_variante_superficie };
        fuente.dibujar(&mut l, x, y + 20 * i as i32 + 15, linea, 14, color);
    }
    // Rama `cuentas-2`: las tres cajas de cambiar la contraseña, con un punto por letra.
    if let Pregunta::Clave { cajas, foco, .. } = pregunta {
        let y0 = y + 20 * lineas.len() as i32 + 12;
        for (k, etiqueta) in ["Ahora", "Nueva", "Repítela"].iter().enumerate() {
            let yc = y0 + 44 * k as i32;
            fuente.dibujar(&mut l, x, yc + 24, etiqueta, 14, t.sobre_variante_superficie);
            let caja = Rect::new(x + 120, yc, ancho_texto - 120, 36);
            l.redondeado(caja, 8, t.contenedor());
            if foco.get() as usize == k {
                l.borde(caja, 8, 2, t.primario);
            }
            fuente.dibujar(&mut l, caja.x + 12, yc + 24, &"•".repeat(cajas[k].borrow().chars().count().min(24)), 14, t.sobre_superficie);
        }
    }
    // S6b, rama `cuentas`: el código de recuperación, a 24 y centrado, para copiarlo sin errores.
    if let Pregunta::Codigo { codigo, .. } = pregunta {
        let ancho_codigo = medio.medir(codigo, 24).ancho;
        medio.dibujar(&mut l, x + (ancho_texto - ancho_codigo).max(0) / 2, y + 20 * 3 + 22, codigo, 24, t.sobre_superficie);
    }
    // S36, decisión 1: las dos cajas de la tanda, bajo el texto: el administrador y su contraseña (un punto por letra).
    // La que se teclea, con el borde en el primario.
    if let Pregunta::Tanda { nombre, clave, foco, nueva, .. } = pregunta {
        let y0 = y + 20 * lineas.len() as i32 + 12;
        let mut cajas = alloc::vec![("Administrador", nombre.borrow().clone(), foco.get() == 0), ("Contraseña", "•".repeat(clave.borrow().chars().count().min(24)), foco.get() == 1)];
        // S6b, rama `cuentas`: la contraseña de la cuenta nueva, también en puntos.
        if let Some((_, otra)) = nueva {
            cajas.push(("Para la nueva", "•".repeat(otra.borrow().chars().count().min(24)), foco.get() == 2));
        }
        for (k, (etiqueta, valor, activa)) in cajas.iter().enumerate() {
            let yc = y0 + 44 * k as i32;
            fuente.dibujar(&mut l, x, yc + 24, etiqueta, 14, t.sobre_variante_superficie);
            let caja = Rect::new(x + 120, yc, ancho_texto - 120, 36);
            l.redondeado(caja, 8, t.contenedor());
            if *activa {
                l.borde(caja, 8, 2, t.primario);
            }
            fuente.dibujar(&mut l, caja.x + 12, yc + 24, valor, 14, t.sobre_superficie);
        }
    }
    l.soltar_recorte();
    // Las respuestas, abajo a la derecha (24 del borde), 8 entre una y otra: botones tonales de Material (píldoras de
    // 40 de alto) con la palabra dentro y su tecla destacada (la primera letra, subrayada y en el color primario). Si
    // la tecla pide Mayúsculas ("Mayús+S"), "Mayús" va pequeño detrás de la palabra, dentro del botón.
    let fila = caja.y + caja.h - 24 - 40;
    let mut derecha = caja.x + caja.w - 24;
    for (tecla, nombre) in respuestas.iter().rev() {
        let con_mayus = tecla.starts_with("Mayús");
        let mut letras = nombre.chars();
        let primera: String = letras.next().map(String::from).unwrap_or_default();
        let resto: String = letras.collect();
        let mp = medio.medir(&primera, 14);
        let mr = medio.medir(&resto, 14);
        let mm = medio.medir("Mayús", 11);
        let ancho_texto = mp.ancho + mr.ancho + if con_mayus { 6 + mm.ancho } else { 0 };
        let ancho_resp = 24 + ancho_texto + 24;
        let r = Rect::new(derecha - ancho_resp, fila, ancho_resp, 40);
        l.redondeado(r, 20, t.contenedor_primario);
        let x0 = r.x + 24;
        let y_base = fila + (40 - mp.alto()) / 2 + mp.sube;
        medio.dibujar(&mut l, x0, y_base, &primera, 14, t.primario);
        // El subrayado de la tecla, de 2 de grueso, bajo la letra.
        l.redondeado(Rect::new(x0, fila + 40 / 2 + mp.alto() / 2 + 1, mp.ancho.max(2), 2), 1, t.primario);
        medio.dibujar(&mut l, x0 + mp.ancho, y_base, &resto, 14, t.sobre_contenedor_primario);
        if con_mayus {
            let ym = fila + (40 - mm.alto()) / 2 + mm.sube;
            medio.dibujar(&mut l, x0 + mp.ancho + mr.ancho + 6, ym, "Mayús", 11, t.sobre_variante_superficie);
        }
        derecha = r.x - 8;
    }
}

/// Un nombre que llega de fuera, entre comillas y recortado a `MAX_NOMBRE_VISTO` letras.
fn recortar(n: &str) -> String {
    if n.chars().count() <= MAX_NOMBRE_VISTO {
        return alloc::format!("\"{}\"", n);
    }
    let corto: String = n.chars().take(MAX_NOMBRE_VISTO - 1).collect();
    alloc::format!("\"{}…\"", corto)
}

/// Parte `texto` en líneas de `ancho` como mucho (a 14), por las palabras; una palabra que no cabe sola se parte por
/// las letras. Hasta `max` líneas: si sobra, la última acaba en "…".
fn partir(fuente: &mut Fuente, texto: &str, ancho: i32, max: usize) -> alloc::vec::Vec<String> {
    let mut lineas: alloc::vec::Vec<String> = alloc::vec::Vec::new();
    let mut linea = String::new();
    for palabra in texto.split(' ') {
        let prueba = if linea.is_empty() { String::from(palabra) } else { alloc::format!("{} {}", linea, palabra) };
        if fuente.medir(&prueba, 14).ancho <= ancho {
            linea = prueba;
            continue;
        }
        if !linea.is_empty() {
            lineas.push(core::mem::take(&mut linea));
        }
        // La palabra sola: si no cabe, por letras.
        for ch in palabra.chars() {
            linea.push(ch);
            if fuente.medir(&linea, 14).ancho > ancho {
                linea.pop();
                lineas.push(core::mem::take(&mut linea));
                linea.push(ch);
            }
        }
    }
    if !linea.is_empty() {
        lineas.push(linea);
    }
    if lineas.len() > max {
        lineas.truncate(max);
        let ultima = &mut lineas[max - 1];
        while !ultima.is_empty() && fuente.medir(&alloc::format!("{}…", ultima), 14).ancho > ancho {
            ultima.pop();
        }
        ultima.push('…');
    }
    lineas
}

/// U3: pide al compositor un aviso cuando nadie toque el teclado ni el ratón en `ms` milisegundos
/// (`ext_idle_notifier_v1`) y lo espera, como mucho `max_us`. Devuelve cuánto tardó en llegar, en milisegundos.
fn esperar_inactividad(ms: u32, max_us: u64) -> Option<u64> {
    const AVISOS: u32 = 30;
    const INACTIVO: u32 = 31;
    let mut c = Cliente::conectar()?;
    let globales = c.globales()?;
    let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
    let (Some(ga), Some(gi)) = (global("wl_seat"), global("ext_idle_notifier_v1")) else {
        println!("  [guardian] el compositor no da avisos de inactividad");
        return None;
    };
    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gi).texto(Some("ext_idle_notifier_v1")).uint(2).uint(AVISOS).fin();
    e.empezar(AVISOS, ext_idle_notifier_v1::req::GET_IDLE_NOTIFICATION).uint(INACTIVO).uint(ms).uint(ASIENTO).fin();
    // Un sync: hasta su respuesta, el compositor no ha creado el aviso. Antes de eso, un Super+L no tenía a quién
    // avisar y se perdía (le pasaba a la prueba con el PC cargado).
    const LISTO: u32 = 32;
    e.empezar(1, wl_display::req::SYNC).uint(LISTO).fin();
    c.mandar(&e);
    let desde = dashos::ahora_us();
    loop {
        match c.evento_plazo(100_000) {
            Ok(Some((o, ev, _))) if o == LISTO && ev == wayland::tablas::wl_callback::ev::DONE => break,
            Ok(_) if dashos::ahora_us() - desde < max_us => {}
            _ => {
                dashos::canal_escribir(VENTANAS, &[wayland::canal::TROZO_ADIOS], &[]);
                return None;
            }
        }
    }
    let desde = dashos::ahora_us();
    println!("  [guardian] esperando {} ms sin entrada", ms);
    let r = loop {
        let pasado = dashos::ahora_us() - desde;
        if pasado > max_us {
            break None;
        }
        match c.evento_plazo(100_000) {
            Ok(Some((o, ev, _))) if o == INACTIVO && ev == ext_idle_notification_v1::ev::IDLED => break Some(pasado / 1000),
            Err(()) => break None,
            _ => {}
        }
    };
    dashos::canal_escribir(VENTANAS, &[wayland::canal::TROZO_ADIOS], &[]);
    r
}

/// Rama `cuentas-2`: en qué está la pantalla de entrada (o de bloqueo).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Modo {
    /// Elegir la persona en la lista de cuentas (con "Otra persona…" al final, para teclear el nombre).
    Lista,
    /// Teclear el nombre.
    Nombre,
    /// Teclear la contraseña de `nombre`.
    Clave,
    /// Olvidé la contraseña: el código de recuperación y la nueva dos veces.
    Recuperar,
    /// El primer arranque (S6b): la primera cuenta, administradora.
    Asistente,
    /// El código de recuperación nuevo (tras recuperar o crear la primera cuenta), para apuntarlo; Intro entra.
    Codigo,
}

/// Lo que se ve y se teclea en la pantalla de entrada o de bloqueo.
struct Pantalla {
    /// La de entrada (nadie dentro), no un bloqueo.
    entrada: bool,
    /// U6: en "Cambiar de usuario" (Esc en la de bloqueo); quien estaba bloqueado sigue detrás.
    cambiando: bool,
    modo: Modo,
    /// Las cuentas de la lista (nombre y nombre completo) y si toca el primer arranque.
    cuentas: alloc::vec::Vec<(String, String)>,
    primer: bool,
    /// La fila elegida de la lista (`cuentas.len()` es "Otra persona…").
    elegida: usize,
    /// Si el nombre salió de la lista (tras una contraseña mala, se sigue con la misma persona) o se tecleó (se vuelve a
    /// pedir el nombre).
    de_lista: bool,
    nombre: String,
    clave: String,
    /// Las cajas de recuperar (código, nueva, repítela) y del asistente (nombre, nombre completo, contraseña, repítela).
    cajas: [String; 4],
    foco: usize,
    /// Lo que se dice debajo, en rojo (contraseña mala, un código que no es...).
    aviso: Option<String>,
    /// El código de recuperación nuevo, para enseñarlo.
    codigo: String,
}

/// Una contraseña (o un código) fuera de la memoria (S6b, regla 9).
fn borrar(s: &mut String) {
    // SAFETY: se escriben ceros, que son UTF-8 válido, y se vacía.
    unsafe { s.as_bytes_mut().fill(0) };
    s.clear();
}

/// Rama `cuentas-2`: las filas de la lista que se ven a la vez.
const FILAS_LISTA: usize = 5;
/// Los códigos de `wl_keyboard` de Tab e Intro (los botones del pie hacen lo que su tecla).
const TECLA_TAB: u32 = 15;
const TECLA_INTRO: u32 = 28;

/// Lo que se pulsa con el ratón en la pantalla de entrada.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pulsado {
    Fila(usize),
    Caja(usize),
    Tecla(u32),
}
const ALTO_FILA: i32 = 56;

impl Pantalla {
    fn vacia(entrada: bool) -> Pantalla {
        Pantalla {
            entrada,
            cambiando: false,
            modo: Modo::Clave,
            cuentas: alloc::vec::Vec::new(),
            primer: false,
            elegida: 0,
            de_lista: false,
            nombre: String::new(),
            clave: String::new(),
            cajas: Default::default(),
            foco: 0,
            aviso: None,
            codigo: String::new(),
        }
    }

    /// El título de arriba.
    fn titulo(&self) -> &'static str {
        match self.modo {
            Modo::Asistente => "Te damos la bienvenida a DashOS",
            Modo::Recuperar => "Olvidé la contraseña",
            Modo::Codigo => "Tu código de recuperación",
            _ if self.cambiando => "Cambiar de usuario",
            _ if self.entrada => "Entrar en DashOS",
            _ => "Sesión bloqueada",
        }
    }

    /// El nombre que se enseña de quien entra: el completo, si está en la lista y lo tiene.
    fn visto(&self) -> String {
        match self.cuentas.iter().find(|c| c.0 == self.nombre) {
            Some((_, completo)) if !completo.is_empty() => completo.clone(),
            _ => self.nombre.clone(),
        }
    }

    /// Los botones del pie, en pastillas: lo que hacen y su tecla (el código de `wl_keyboard`). Se pulsan con el ratón
    /// o con la tecla (S0: todo se puede hacer con el ratón).
    fn pie(&self) -> &'static [(&'static str, u32)] {
        const ESC: u32 = TECLA_ESC;
        match self.modo {
            Modo::Lista if self.cambiando => &[("Esc: volver a la sesión bloqueada", ESC)],
            Modo::Lista => &[],
            Modo::Nombre if self.cambiando => &[("Esc: volver", ESC)],
            Modo::Nombre if !self.cuentas.is_empty() => &[("Esc: volver a la lista", ESC)],
            Modo::Nombre => &[],
            Modo::Clave if !self.entrada && !self.cambiando => &[("Esc: cambiar de usuario", ESC), ("Tab: olvidé la contraseña", TECLA_TAB)],
            Modo::Clave if self.cuentas.is_empty() && !self.cambiando => &[("Tab: olvidé la contraseña", TECLA_TAB)],
            Modo::Clave => &[("Esc: elegir a otra persona", ESC), ("Tab: olvidé la contraseña", TECLA_TAB)],
            Modo::Recuperar => &[("Esc: volver", ESC), ("Intro: cambiarla", TECLA_INTRO)],
            Modo::Asistente => &[("Intro: crear la cuenta", TECLA_INTRO)],
            Modo::Codigo => &[("Intro: ya lo he apuntado, entrar", TECLA_INTRO)],
        }
    }

    /// S27, oleada 6: si el modo va con la fila de DMS (el avatar y el campo): la contraseña y el nombre.
    fn de_fila(&self) -> bool {
        matches!(self.modo, Modo::Clave | Modo::Nombre)
    }

    /// Dónde va el título de la lista y de los formularios (su línea base): bajo el reloj y la fecha.
    fn arriba(ancho: i32, alto: i32) -> i32 {
        bv::disponer(ancho, alto).arriba_formularios + 30
    }

    /// Dónde acaba lo de en medio (la lista, las cajas del formulario o el código; en la fila, el campo), en una
    /// pantalla de `ancho` × `alto`: debajo van el aviso y el pie.
    fn abajo(&self, ancho: i32, alto: i32) -> i32 {
        let y = Self::arriba(ancho, alto);
        match self.modo {
            Modo::Lista => y + 40 + ALTO_FILA * (self.cuentas.len() + 1).min(FILAS_LISTA) as i32 + 8,
            Modo::Recuperar | Modo::Asistente => y + 68 + 48 * self.etiquetas().len() as i32 + 4,
            Modo::Codigo => y + 176,
            Modo::Clave | Modo::Nombre => bv::disponer(ancho, alto).aviso.y,
        }
    }

    /// La primera fila de la lista que se ve (si no caben todas, la elegida, a la vista).
    fn primera_fila(&self) -> usize {
        let filas = self.cuentas.len() + 1;
        self.elegida.saturating_sub(FILAS_LISTA - 1).min(filas.saturating_sub(FILAS_LISTA))
    }

    /// Las pastillas del pie (dónde y su tecla), en una pantalla de `ancho` × `alto`.
    fn pastillas(&self, ancho: i32, alto: i32, fuente: &mut Fuente) -> alloc::vec::Vec<(Rect, &'static str, u32)> {
        let botones = self.pie();
        let anchos: alloc::vec::Vec<i32> = botones.iter().map(|b| fuente.medir(b.0, 15).ancho + 40).collect();
        let total = anchos.iter().sum::<i32>() + 12 * (botones.len() as i32 - 1).max(0);
        let mut x = (ancho - total) / 2;
        let y = if self.de_fila() { bv::disponer(ancho, alto).pie_y } else { self.abajo(ancho, alto) + 24 + 20 };
        botones
            .iter()
            .zip(anchos)
            .map(|(b, w)| {
                let r = Rect::new(x, y, w, 36);
                x += w + 12;
                (r, b.0, b.1)
            })
            .collect()
    }

    /// Lo que hay en (x, y) de la pantalla: una fila de la lista, una caja del formulario o un botón del pie.
    fn en(&self, ancho: i32, alto: i32, x: i32, y: i32, fuente: &mut Fuente) -> Option<Pulsado> {
        if let Some(b) = self.pastillas(ancho, alto, fuente).iter().find(|b| b.0.contiene(x, y)) {
            return Some(Pulsado::Tecla(b.2));
        }
        // La flecha del campo: Intro (solo se ve con algo escrito).
        let lleno = if self.modo == Modo::Clave { !self.clave.is_empty() } else { !self.nombre.is_empty() };
        if self.de_fila() && lleno && boton_entrar(&bv::disponer(ancho, alto)).contiene(x, y) {
            return Some(Pulsado::Tecla(TECLA_INTRO));
        }
        let arriba = Self::arriba(ancho, alto);
        match self.modo {
            Modo::Lista => {
                let ancho_fila = 400.min(ancho - 32);
                let x0 = (ancho - ancho_fila) / 2;
                let primera = self.primera_fila();
                (primera..(primera + FILAS_LISTA).min(self.cuentas.len() + 1))
                    .enumerate()
                    .find(|(k, _)| Rect::new(x0, arriba + 40 + ALTO_FILA * *k as i32, ancho_fila, ALTO_FILA - 8).contiene(x, y))
                    .map(|(_, i)| Pulsado::Fila(i))
            }
            Modo::Recuperar | Modo::Asistente => {
                let ancho_f = 520.min(ancho - 32);
                let x0 = (ancho - ancho_f) / 2;
                (0..self.etiquetas().len()).find(|&i| Rect::new(x0, arriba + 68 + 48 * i as i32, ancho_f, 40).contiene(x, y)).map(Pulsado::Caja)
            }
            _ => None,
        }
    }

    /// Las cajas del modo (etiqueta, texto que se ve), si las tiene.
    fn etiquetas(&self) -> &'static [(&'static str, bool)] {
        match self.modo {
            Modo::Recuperar => &[("Código", false), ("Contraseña nueva", true), ("Repítela", true)],
            Modo::Asistente => &[("Nombre corto", false), ("Nombre completo", false), ("Contraseña", true), ("Repítela", true)],
            _ => &[],
        }
    }

    /// Lo más que cabe en la caja `i` del modo.
    fn tope(&self, i: usize) -> usize {
        match (self.modo, i) {
            (Modo::Asistente, 0) => 16,
            (Modo::Asistente, 1) => 48,
            (Modo::Recuperar, 0) => 40,
            _ => 128,
        }
    }

    /// A la lista (o a teclear el nombre, sin cuentas), con lo tecleado olvidado.
    fn a_la_lista(&mut self) {
        borrar(&mut self.clave);
        self.nombre.clear();
        self.de_lista = false;
        self.modo = if self.cuentas.is_empty() { Modo::Nombre } else { Modo::Lista };
    }

    fn borrar_cajas(&mut self) {
        for c in self.cajas.iter_mut() {
            borrar(c);
        }
        self.foco = 0;
    }
}

/// Rama `cuentas-2`: las cuentas para la lista de la pantalla de entrada (`CUENTAS_ENTRADA`, por páginas) y si toca el
/// primer arranque. Sin respuesta, ninguna (se teclea el nombre).
fn cuentas_de_entrada() -> (alloc::vec::Vec<(String, String)>, bool) {
    use dashos::protocolo::sesiones::{CUENTAS_ENTRADA, ENTRADA_CUENTA, POR_PAGINA_ENTRADA};
    let campo = |b: &[u8]| -> String { limpiar(core::str::from_utf8(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]).unwrap_or("?"), "") };
    let mut v = alloc::vec::Vec::new();
    let mut primer = false;
    for _ in 0..dashos::cuentas::MAX_CUENTAS {
        let mut m = [0u8; 8];
        m[0..4].copy_from_slice(&CUENTAS_ENTRADA.to_le_bytes());
        m[4..8].copy_from_slice(&(v.len() as u32).to_le_bytes());
        let (lectura, suyo) = dashos::canal_crear();
        let mut r = [0u8; 16 + ENTRADA_CUENTA * POR_PAGINA_ENTRADA];
        let n = if dashos::canal_escribir(SESIONES, &m, &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], 3_000_000).0 } else { -1 };
        dashos::cerrar(lectura);
        if n < 16 || r[0..4] != [0; 4] {
            break;
        }
        let total = u32::from_le_bytes(r[4..8].try_into().unwrap()) as usize;
        let van = (u32::from_le_bytes(r[8..12].try_into().unwrap()) as usize).min((n as usize - 16) / ENTRADA_CUENTA);
        primer = r[12] == 1;
        for i in 0..van {
            let e = &r[16 + ENTRADA_CUENTA * i..16 + ENTRADA_CUENTA * (i + 1)];
            let nombre = campo(&e[..16]);
            if !nombre.is_empty() {
                v.push((nombre, campo(&e[16..])));
            }
        }
        if van == 0 || v.len() >= total {
            break;
        }
    }
    (v, primer)
}

/// Rama `cuentas-2`: `RECUPERAR` o `PRIMERA_CUENTA` a `sesiones` con tres textos: (estado, segundos de espera, el código
/// nuevo si fue bien). Lo mandado se borra.
fn pedir_con_codigo(orden: u32, uno: &str, dos: &str, tres: &str) -> (i32, u32, String) {
    use dashos::protocolo::sesiones::LARGO_RECUPERAR;
    let (a, b, c) = (&uno.as_bytes()[..uno.len().min(16)], &dos.as_bytes()[..dos.len().min(64)], &tres.as_bytes()[..tres.len().min(128)]);
    let mut m = [0u8; 16 + 16 + 64 + 128];
    m[0..4].copy_from_slice(&orden.to_le_bytes());
    m[4..8].copy_from_slice(&(a.len() as u32).to_le_bytes());
    m[8..12].copy_from_slice(&(b.len() as u32).to_le_bytes());
    m[12..16].copy_from_slice(&(c.len() as u32).to_le_bytes());
    let mut k = 16;
    for t in [a, b, c] {
        m[k..k + t.len()].copy_from_slice(t);
        k += t.len();
    }
    let (lectura, suyo) = dashos::canal_crear();
    let mut r = [0u8; LARGO_RECUPERAR];
    // Crear la primera cuenta hace su volumen (Argon2 dos veces): hasta un minuto con la máquina cargada.
    let n = if dashos::canal_escribir(SESIONES, &m[..k], &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], 60_000_000).0 } else { -1 };
    m.fill(0);
    dashos::cerrar(lectura);
    if n < 8 {
        return (i32::MIN, 0, String::new());
    }
    let estado = i32::from_le_bytes(r[0..4].try_into().unwrap());
    let espera = u32::from_le_bytes(r[4..8].try_into().unwrap());
    let codigo = if estado == 0 && n as usize == LARGO_RECUPERAR { String::from(core::str::from_utf8(&r[8..]).unwrap_or("")) } else { String::new() };
    r.fill(0);
    core::hint::black_box(&r);
    (estado, espera, codigo)
}

/// Lo que se dice de un estado de `sesiones` al recuperar o crear una cuenta, en llano.
fn que_dice(e: i32, espera: u32) -> String {
    use dashos::protocolo::sesiones::{OCUPADO, PROTEGIDO};
    match e {
        OCUPADO => alloc::format!("Demasiados intentos. Espera {} s y prueba otra vez.", espera),
        PROTEGIDO => String::from("Ese código no es el de esta cuenta."),
        e => match dashos::cuentas::Fallo::de_estado(e) {
            Some(f) => {
                let t = f.texto();
                let mut c = t.chars();
                c.next().map_or(String::new(), |p| alloc::format!("{}{}.", p.to_uppercase(), c.as_str()))
            }
            None => alloc::format!("No se pudo ({}).", e),
        },
    }
}

/// Manda a `sesiones` la contraseña de la usuaria para comprobarla (VOLVER: sin tocar su volumen) y la borra. No
/// espera: devuelve el canal por el que llegará la respuesta (tras una mala, `sesiones` tarda a propósito, y mientras
/// tanto hay que seguir leyendo al compositor).
fn comprobar(orden: u32, usuaria: &str, clave: &mut String) -> Option<u32> {
    // Cuántas letras (no cuáles): para ver una tecla perdida en las pruebas.
    println!("  [guardian] se comprueba una contraseña de {} letras", clave.chars().count());
    let mut m = [0u8; 160];
    let (u, c) = (usuaria.as_bytes(), clave.as_bytes());
    let u = &u[..u.len().min(16)];
    let c = &c[..c.len().min(128)];
    m[0..4].copy_from_slice(&orden.to_le_bytes());
    m[4..8].copy_from_slice(&(u.len() as u32).to_le_bytes());
    m[8..12].copy_from_slice(&(c.len() as u32).to_le_bytes());
    m[12..12 + u.len()].copy_from_slice(u);
    m[12 + u.len()..12 + u.len() + c.len()].copy_from_slice(c);
    let (lectura, suyo) = dashos::canal_crear();
    let r = dashos::canal_escribir(SESIONES, &m[..12 + u.len() + c.len()], &[suyo]);
    m.fill(0);
    // La contraseña, fuera de la memoria (S6b, regla 9).
    borrar(clave);
    if r != 0 {
        dashos::cerrar(lectura);
        dashos::cerrar(suyo);
        return None;
    }
    Some(lectura)
}

/// S27, oleada 6: lo que se ve además de lo de escribir, como en la pantalla de bloqueo de DMS: el fondo desenfocado,
/// la hora, el estado (red y volumen), los avisos y Bloq Mayús.
struct Alrededor {
    /// El fondo pequeño y desenfocado de `fondo` (píxeles, ancho y alto); sin él, el color de la superficie del tema.
    fondo: Option<(alloc::vec::Vec<u32>, usize, usize)>,
    /// La fecha y la hora locales (si se sabe la hora), y el minuto en que se miró.
    hora: Option<reloj::Fecha>,
    minuto: u64,
    /// La red (2 conectada), el volumen y el silencio, como los da el panel a la barra (por el compositor).
    estado: Option<(u32, u32, u32)>,
    /// Los avisos, según su ajuste (solo en la de bloqueo).
    avisos: Option<ResumenAvisos>,
    bloq_mayus: bool,
    /// El primario del esquema oscuro del tema: el reloj sobre el fondo velado (oscuro) se lee con cualquier tema.
    primario_oscuro: kit::lienzo::Color,
    simbolos: kit::iconos::Simbolos,
}

/// Lo que dejan ver los avisos en la pantalla de bloqueo (`protocolo::avisos::resumen_bloqueo`).
#[derive(Clone, PartialEq, Eq, Debug)]
struct ResumenAvisos {
    modo: u32,
    cuantos: u32,
    programas: alloc::vec::Vec<(String, u32)>,
}

impl Alrededor {
    fn nuevo() -> Alrededor {
        Alrededor { fondo: None, hora: None, minuto: u64::MAX, estado: None, avisos: None, bloq_mayus: false, primario_oscuro: Tema::OSCURO.primario, simbolos: kit::iconos::Simbolos::new() }
    }

    /// El color del contenido (el texto, el estado) y el apagado: blanco sobre el fondo velado (como DMS,
    /// `lockScreenContentColor`); sin fondo, los del tema.
    fn colores(&self, t: &Tema) -> (kit::lienzo::Color, kit::lienzo::Color) {
        if self.fondo.is_some() { (0xffff_ffff, 0xb3ff_ffff) } else { (t.sobre_superficie, t.sobre_variante_superficie) }
    }
}

/// La zona horaria del sistema (su puesto en `dashos::zona::ZONAS`), que dice el centro de control; `usize::MAX` hasta
/// que conteste (mientras, UTC).
static ZONA: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(usize::MAX);

/// La zona horaria del sistema: la que se sabe, o se le pregunta al centro ("zona"); si no contesta, UTC esta vez.
fn zona_horaria() -> &'static dashos::zona::Zona {
    use core::sync::atomic::Ordering::Relaxed;
    if ZONA.load(Relaxed) == usize::MAX {
        let (lectura, suyo) = dashos::canal_crear();
        if dashos::canal_escribir(centro(), b"zona", &[suyo]) == 0 {
            let mut r = [0u8; 64];
            let n = dashos::canal_leer_plazo(lectura, &mut r, &mut [], 1_500_000).0;
            if n >= 0 {
                let nombre = core::str::from_utf8(&r[..n as usize]).unwrap_or("");
                let i = dashos::zona::ZONAS.iter().position(|z| z.nombre.eq_ignore_ascii_case(nombre)).unwrap_or(0);
                ZONA.store(i, Relaxed);
                println!("  [guardian] zona horaria: {}", dashos::zona::ZONAS.get(i).map_or("UTC", |z| z.nombre));
            }
        } else {
            dashos::cerrar(suyo);
        }
        dashos::cerrar(lectura);
    }
    dashos::zona::ZONAS.get(ZONA.load(Relaxed)).unwrap_or(&dashos::zona::UTC)
}

/// La fecha y la hora locales, y el minuto (local, desde 1970); sin hora, `None` y 0.
fn ahora_local() -> (Option<reloj::Fecha>, u64) {
    let ns = dashos::hora_ns();
    if ns == 0 {
        return (None, 0);
    }
    let local = dashos::zona::local_ns(zona_horaria(), ns);
    (Some(reloj::fecha(local)), local / 60_000_000_000)
}

/// El fondo de la pantalla de bloqueo, pequeño y desenfocado, que da `fondo` ("bloqueo", solo a `guardian`): se copia y
/// se suelta. Si no lo da, o lo que da no cuadra (medidas fuera de lo razonable, una memoria más pequeña), nada.
fn fondo_de_bloqueo() -> Option<(alloc::vec::Vec<u32>, usize, usize)> {
    const VA_FONDO: usize = 0x46_0000_0000;
    let llave = dashos::llaves::buscar("fondo")?;
    let (lectura, suyo) = dashos::canal_crear();
    if dashos::canal_escribir(llave, b"bloqueo", &[suyo]) != 0 {
        dashos::cerrar(lectura);
        dashos::cerrar(suyo);
        return None;
    }
    let mut r = [0u8; 12];
    let mut ll = [0u32; 1];
    let (n, k, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut ll, 1_500_000);
    dashos::cerrar(lectura);
    let palabra = |i: usize| u32::from_le_bytes(r[i..i + 4].try_into().unwrap());
    let (w, h) = (palabra(4) as usize, palabra(8) as usize);
    let bueno = n == 12 && k == 1 && palabra(0) == 0 && (1..=desenfoque::MAX_LADO).contains(&w) && (1..=desenfoque::MAX_LADO).contains(&h);
    if !bueno {
        if n >= 0 && k == 1 {
            dashos::cerrar(ll[0]);
        }
        println!("  [guardian] sin fondo para la pantalla ({}): el color del tema", if n == 4 { i32::from_le_bytes(r[0..4].try_into().unwrap()) } else { n as i32 });
        return None;
    }
    let tam = dashos::memoria_tamano(ll[0]);
    if tam < (w * h * 4) as i64 || dashos::memoria_proyectar(ll[0], VA_FONDO, false) != 0 {
        dashos::cerrar(ll[0]);
        println!("  [guardian] el fondo que da `fondo` no cuadra: el color del tema");
        return None;
    }
    // SAFETY: proyectada aquí, solo para leer, con al menos `w × h` píxeles (mirado arriba); se quita en seguida.
    let px = unsafe { core::slice::from_raw_parts(VA_FONDO as *const u32, w * h) }.to_vec();
    dashos::memoria_quitar(VA_FONDO);
    dashos::cerrar(ll[0]);
    Some((px, w, h))
}

/// Lo que dejan ver los avisos en la pantalla de bloqueo: se pide a `sesiones` (`AVISOS_BLOQUEO`), que se lo pasa al
/// servicio de avisos, que contesta aquí. Sin respuesta, o una que no cuadra, nada.
fn avisos_de_bloqueo() -> Option<ResumenAvisos> {
    use dashos::protocolo::avisos::{CABECERA_RESUMEN, ENTRADA_RESUMEN, LARGO_RESUMEN, RESUMEN_PROGRAMAS};
    let (lectura, suyo) = dashos::canal_crear();
    let mut r = [0u8; LARGO_RESUMEN];
    let n = if dashos::canal_escribir(SESIONES, &dashos::protocolo::sesiones::AVISOS_BLOQUEO.to_le_bytes(), &[suyo]) == 0 {
        dashos::canal_leer_plazo(lectura, &mut r, &mut [], 500_000).0
    } else {
        -1
    };
    dashos::cerrar(lectura);
    if n < CABECERA_RESUMEN as i64 || r[0..4] != [0; 4] {
        return None;
    }
    let u = |i: usize| u32::from_le_bytes(r[i..i + 4].try_into().unwrap());
    let van = (u(12) as usize).min(RESUMEN_PROGRAMAS).min((n as usize - CABECERA_RESUMEN) / ENTRADA_RESUMEN);
    let programas = (0..van)
        .map(|i| {
            let o = CABECERA_RESUMEN + ENTRADA_RESUMEN * i;
            let b = &r[o..o + 16];
            (limpiar(core::str::from_utf8(&b[..b.iter().position(|&c| c == 0).unwrap_or(16)]).unwrap_or("?"), "?"), u(o + 16))
        })
        .collect();
    Some(ResumenAvisos { modo: u(4).min(2), cuantos: u(8), programas })
}

/// Pinta en `px` el fondo de la zona `z`: el desenfocado agrandado con su velo, o el color de la superficie.
fn pintar_fondo(px: &mut [u32], ancho: i32, alto: i32, z: Rect, a: &Alrededor, t: &Tema) {
    match &a.fondo {
        Some((f, w, h)) => desenfoque::agrandar(px, ancho as usize, alto as usize, ancho as usize, (z.x, z.y, z.w, z.h), f, *w, *h, desenfoque::VELO),
        None => Lienzo::new(px, ancho, alto, ancho as usize).borrar(z, t.superficie),
    }
}

fn caja(c: bv::Caja) -> Rect {
    Rect::new(c.x, c.y, c.w, c.h)
}

/// El botón de entrar (la flecha) dentro del campo, como el de DMS (`buttonHeightXS`, 32).
fn boton_entrar(d: &bv::Disposicion) -> Rect {
    Rect::new(d.campo.x + d.campo.w - 12 - 32, d.campo.y + (d.campo.h - 32) / 2, 32, 32)
}

/// La zona de lo de en medio (lo que cambia al teclear), según el modo.
fn zona_medio(p: &Pantalla, ancho: i32, alto: i32) -> Rect {
    if p.de_fila() {
        return caja(bv::disponer(ancho, alto).franja_clave(ancho));
    }
    let y = Pantalla::arriba(ancho, alto);
    Rect::new(0, y + 12, ancho, (p.abajo(ancho, alto) + 32 - y - 12).max(0))
}

/// Arriba: el reloj, la fecha y el estado.
fn zona_arriba(ancho: i32, alto: i32) -> Rect {
    let d = bv::disponer(ancho, alto);
    Rect::new(0, 0, ancho, d.fecha.abajo() + 8)
}

/// La fila de los avisos.
fn zona_avisos(ancho: i32, alto: i32) -> Rect {
    let d = bv::disponer(ancho, alto);
    Rect::new(0, d.avisos.y - 2, ancho, d.avisos.h + 4)
}

/// Pinta la pantalla de entrada o de bloqueo en la zona `zona` (solo eso: el compositor suelta a quien tarda en leer,
/// E8): el fondo, el reloj y la fecha, el estado, y según el modo, la fila de DMS (el avatar con la inicial y el campo
/// con un punto por letra, con el nombre y la frase de la persona encima), la lista de cuentas, las cajas de recuperar o
/// del primer arranque, o el código; debajo, el aviso, los avisos y, al pie, en pastillas, las teclas. Devuelve la zona.
/// Con los colores del tema del sistema (S10c: `dashos_tema_v1`, el que pone el centro de control); sobre el fondo, el
/// contenido en blanco, como DMS.
#[allow(clippy::too_many_arguments)]
fn pintar(px: &mut [u32], ancho: i32, alto: i32, p: &Pantalla, zona: Rect, fuente: &mut Fuente, titulo: &mut Fuente, t: &Tema, a: &Alrededor) -> Rect {
    let zona = zona.cruce(Rect::new(0, 0, ancho, alto));
    pintar_fondo(px, ancho, alto, zona, a, t);
    let mut l = Lienzo::new(px, ancho, alto, ancho as usize);
    l.recortar(zona);
    let d = bv::disponer(ancho, alto);
    let oscuro = (t.superficie >> 8 & 0xff) < 0x80;
    let (contenido, tenue) = a.colores(t);
    // El rojo del error de Material 3, el del fondo claro o el del oscuro (sobre el fondo velado, el del oscuro).
    let error = if oscuro || a.fondo.is_some() { 0xfff2_b8b5 } else { 0xffb3_261e };
    let centrado = |f: &mut Fuente, t: &str, tam: u32| (ancho - f.medir(t, tam).ancho) / 2;
    // El reloj (el "expressive" de DMS: horas y minutos en el primario, en una línea) y la fecha debajo.
    if let Some(f) = a.hora {
        let (hh, mm) = bv::hora(f.hora, f.minuto);
        let texto = alloc::format!("{}:{}", hh, mm);
        let tam = bv::letra_reloj(&d.reloj);
        let m = titulo.medir(&texto, tam);
        let color = if a.fondo.is_some() { a.primario_oscuro } else { t.primario };
        titulo.dibujar(&mut l, d.reloj.x + (d.reloj.w - m.ancho) / 2, d.reloj.y + (d.reloj.h + m.sube - m.baja) / 2, &texto, tam, color);
        let fecha = bv::fecha_larga(f.semana, f.dia, f.mes);
        let x = centrado(titulo, &fecha, 22);
        titulo.dibujar(&mut l, x, d.fecha.y + 22, &fecha, 22, contenido);
    }
    // El estado, arriba a la derecha (DMS: `LockStatusRow`): la red y el volumen; sin batería, que DashOS no lleva.
    if let Some((red, nivel, silencio)) = a.estado {
        let tam = bv::ICONO_ESTADO;
        let (xd, y) = d.estado;
        let x = xd - tam;
        let silencio = silencio != 0;
        a.simbolos.pintar(&mut l, x, y, tam, simbolo_sonido(nivel, silencio), if silencio { tenue } else { contenido });
        let x = x - tam - 16;
        if red == 2 {
            a.simbolos.pintar(&mut l, x, y, tam, kit::iconos::simbolo::LAN, contenido);
        } else {
            a.simbolos.pintar(&mut l, x, y, tam, kit::iconos::simbolo::LAN, tenue);
            l.trazo(((x + 2) * 8, (y + 2) * 8), ((x + tam - 2) * 8, (y + tam - 2) * 8), 14, tenue);
        }
    }
    let y = Pantalla::arriba(ancho, alto);
    if p.de_fila() {
        pintar_fila(&mut l, &d, p, fuente, titulo, t, a, contenido, tenue, error);
    } else {
        let cabeza = p.titulo();
        let x = centrado(titulo, cabeza, 30);
        titulo.dibujar(&mut l, x, y, cabeza, 30, contenido);
    }
    match p.modo {
        Modo::Clave | Modo::Nombre => {}
        Modo::Lista => {
            let filas = p.cuentas.len() + 1;
            let primera = p.primera_fila();
            let ancho_fila = 400.min(ancho - 32);
            let x0 = (ancho - ancho_fila) / 2;
            let mut yf = y + 40;
            for i in primera..(primera + FILAS_LISTA).min(filas) {
                let fila = Rect::new(x0, yf, ancho_fila, ALTO_FILA - 8);
                let elegida = i == p.elegida;
                l.redondeado(fila, 24, if elegida { t.contenedor_primario } else { t.contenedor_alto() });
                if elegida {
                    l.borde(fila, 24, 2, t.primario);
                }
                // Un círculo con la inicial (o un "+" para teclear otro nombre) y el nombre al lado.
                let (principal, segundo, inicial) = match p.cuentas.get(i) {
                    Some((n, c)) if !c.is_empty() && c != n => (c.clone(), n.clone(), bv::inicial(n)),
                    Some((n, _)) => (n.clone(), String::new(), bv::inicial(n)),
                    None => (String::from("Otra persona…"), String::new(), String::from("+")),
                };
                let circulo = Rect::new(fila.x + 8, fila.y + 6, 36, 36);
                l.redondeado(circulo, 18, t.primario);
                let mi = titulo.medir(&inicial, 18);
                titulo.dibujar(&mut l, circulo.x + (36 - mi.ancho) / 2, circulo.y + (36 + mi.sube - mi.baja) / 2, &inicial, 18, t.sobre_primario);
                let color = if elegida { t.sobre_contenedor_primario } else { t.sobre_superficie };
                l.recortar(Rect::new(fila.x + 56, fila.y, fila.w - 64, fila.h));
                fuente.dibujar(&mut l, fila.x + 56, fila.y + 30, &principal, 18, color);
                if !segundo.is_empty() {
                    let ancho_p = fuente.medir(&principal, 18).ancho;
                    fuente.dibujar(&mut l, fila.x + 56 + ancho_p + 10, fila.y + 30, &segundo, 14, t.sobre_variante_superficie);
                }
                l.soltar_recorte();
                yf += ALTO_FILA;
            }
        }
        Modo::Recuperar | Modo::Asistente => {
            let texto = if p.modo == Modo::Recuperar {
                alloc::format!("El código de recuperación de {} y una contraseña nueva.", p.visto())
            } else {
                String::from("Crea la primera cuenta: será la administradora del equipo.")
            };
            let x = centrado(fuente, &texto, 16);
            fuente.dibujar(&mut l, x, y + 48, &texto, 16, contenido);
            let ancho_f = 520.min(ancho - 32);
            let x0 = (ancho - ancho_f) / 2;
            let mut yc = y + 68;
            for (i, (etiqueta, secreta)) in p.etiquetas().iter().enumerate() {
                fuente.dibujar(&mut l, x0, yc + 26, etiqueta, 15, contenido);
                let caja = Rect::new(x0 + 170, yc, ancho_f - 170, 40);
                l.redondeado(caja, 12, t.contenedor_alto());
                l.borde(caja, 12, if i == p.foco { 2 } else { 1 }, if i == p.foco { t.primario } else { t.variante_contorno });
                let valor = if *secreta { "•".repeat(p.cajas[i].chars().count().min(24)) } else { p.cajas[i].clone() };
                l.recortar(caja.dentro(4));
                fuente.dibujar(&mut l, caja.x + 14, yc + 26, &valor, 16, t.sobre_superficie);
                l.soltar_recorte();
                yc += 48;
            }
        }
        Modo::Codigo => {
            let lineas = [
                alloc::format!("Es la otra llave del Home: de {}, por si olvidas la contraseña.", p.visto()),
                String::from("Apúntalo en papel y guárdalo: no se vuelve a enseñar."),
            ];
            for (i, linea) in lineas.iter().enumerate() {
                let x = centrado(fuente, linea, 16);
                fuente.dibujar(&mut l, x, y + 48 + 24 * i as i32, linea, 16, contenido);
            }
            let caja = Rect::new(ancho / 2 - 260, y + 92, 520, 64);
            l.redondeado(caja, 16, t.contenedor_alto());
            let x = centrado(titulo, &p.codigo, 26);
            titulo.dibujar(&mut l, x, y + 134, &p.codigo, 26, t.sobre_superficie);
        }
    }
    // El aviso: el error, o qué hacer (en la fila de DMS, solo el error y Bloq Mayús, como su campo).
    let aviso = match (&p.aviso, p.modo) {
        (Some(a), _) => Some(a.clone()),
        (None, Modo::Clave | Modo::Nombre) if a.bloq_mayus => Some(String::from("Bloq Mayús está activado")),
        (None, Modo::Clave | Modo::Nombre) => None,
        (None, Modo::Lista) => Some(String::from("¿Quién entra? Elige con las flechas o el ratón, o escribe tu nombre.")),
        (None, Modo::Recuperar) => Some(String::from("El código está en el papel que apuntaste al crear la cuenta.")),
        (None, Modo::Asistente) => Some(String::from("El nombre corto es para entrar: letras sin acentos y cifras.")),
        (None, Modo::Codigo) => Some(String::from("Con él, en esta pantalla, pones otra contraseña si la olvidas.")),
    };
    if let Some(texto) = aviso {
        let base = if p.de_fila() { d.aviso.y + 16 } else { p.abajo(ancho, alto) + 24 };
        let tam = if p.de_fila() { 14 } else { 16 };
        let x = centrado(fuente, &texto, tam);
        let rojo = p.aviso.is_some() || (p.de_fila() && a.bloq_mayus);
        fuente.dibujar(&mut l, x, base, &texto, tam, if rojo { error } else { tenue });
    }
    // Los avisos (solo en la de bloqueo, y lo que deja su ajuste).
    if p.de_fila() {
        pintar_avisos(&mut l, &d, a, fuente, titulo, t);
    }
    // Al pie, en pastillas del contenedor (Material 3), los botones, con su tecla: se pulsan o se teclean.
    for (pastilla, texto, _) in p.pastillas(ancho, alto, fuente) {
        let m = fuente.medir(texto, 15);
        l.redondeado(pastilla, 18, t.contenedor());
        fuente.dibujar(&mut l, pastilla.x + 20, pastilla.y + (36 + m.sube - m.baja) / 2, texto, 15, t.sobre_variante_superficie);
    }
    zona
}

/// La fila de DMS (`LockAuthWidget`, estilo "expressive"): el avatar (la inicial en un círculo del contenedor primario)
/// y el campo en el contenedor alto, con el contorno del primario (el de foco de Material 3; en rojo tras un error), el
/// candado en un cuadrado del contenedor primario, un punto por letra (o el nombre que se teclea) y la flecha para
/// entrar. Encima, quién entra y su frase (S6b: así se sabe que es el sistema).
#[allow(clippy::too_many_arguments)]
fn pintar_fila(l: &mut Lienzo, d: &bv::Disposicion, p: &Pantalla, fuente: &mut Fuente, titulo: &mut Fuente, t: &Tema, a: &Alrededor, contenido: kit::lienzo::Color, tenue: kit::lienzo::Color, error: kit::lienzo::Color) {
    let ancho = l.ancho;
    let centrado = |f: &mut Fuente, t: &str, tam: u32| (ancho - f.medir(t, tam).ancho) / 2;
    let clave = p.modo == Modo::Clave;
    // Encima: "Cambiar de usuario" si toca, quién y su frase (o qué hacer, al teclear el nombre).
    if p.cambiando {
        let x = centrado(fuente, "Cambiar de usuario", 16);
        fuente.dibujar(l, x, d.nombre.y - 14, "Cambiar de usuario", 16, tenue);
    }
    let visto = if clave { p.visto() } else { String::from("¿Quién eres?") };
    let x = centrado(titulo, &visto, 22);
    titulo.dibujar(l, x, d.nombre.y + 22, &visto, 22, contenido);
    if clave && let Some(f) = FRASES.iter().find(|f| f.0 == p.nombre) {
        let frase = alloc::format!("\"{}\"", f.1);
        let x = centrado(fuente, &frase, 17);
        let color = if a.fondo.is_some() { a.primario_oscuro } else { t.primario };
        fuente.dibujar(l, x, d.frase.y + 18, &frase, 17, color);
    }
    // El avatar.
    let av = caja(d.avatar);
    l.redondeado(av, av.h / 2, t.contenedor_primario);
    let ini = if clave { bv::inicial(&p.nombre) } else { bv::inicial(&p.nombre).replace('?', "") };
    if ini.is_empty() {
        a.simbolos.centrado(l, av, 28, kit::iconos::simbolo::MANAGE_ACCOUNTS, t.sobre_contenedor_primario);
    } else {
        let m = titulo.medir(&ini, 24);
        titulo.dibujar(l, av.x + (av.w - m.ancho) / 2, av.y + (av.h + m.sube - m.baja) / 2, &ini, 24, t.sobre_contenedor_primario);
    }
    // El campo.
    let campo = caja(d.campo);
    let radio = dms_medidas::radio(28);
    let fallo = p.aviso.is_some();
    l.redondeado(campo, radio, t.contenedor_alto());
    l.borde(campo, radio, 2, if fallo { error } else { t.primario });
    let icono = Rect::new(campo.x + 8, campo.y + 8, campo.h - 16, campo.h - 16);
    l.redondeado(icono, dms_medidas::radio(16), if fallo { kit::tema::con_opacidad(error, 72) } else { t.contenedor_primario });
    let simbolo = if clave { kit::iconos::simbolo::LOCK } else { kit::iconos::simbolo::MANAGE_ACCOUNTS };
    a.simbolos.centrado(l, icono, 20, simbolo, if fallo { t.sobre_superficie } else { t.sobre_contenedor_primario });
    let tx = icono.x + icono.w + 12;
    let hueco = Rect::new(tx, campo.y, campo.x + campo.w - tx - 52, campo.h);
    l.recortar(hueco);
    let vacio = if clave { p.clave.is_empty() } else { p.nombre.is_empty() };
    if vacio {
        let ph = if clave { "Contraseña…" } else { "Tu nombre…" };
        let m = fuente.medir(ph, 15);
        fuente.dibujar(l, tx, campo.y + (campo.h + m.sube - m.baja) / 2, ph, 15, t.contorno);
    } else if clave {
        // Un punto por letra (como el eco de contraseña de DMS), los últimos si no caben.
        let n = p.clave.chars().count() as i32;
        let cabe = ((hueco.w - 4) / 18).max(1);
        for i in 0..n.min(cabe) {
            l.redondeado(Rect::new(tx + 2 + i * 18, campo.y + campo.h / 2 - 5, 10, 10), 5, t.sobre_superficie);
        }
    } else {
        let m = fuente.medir(&p.nombre, 17);
        fuente.dibujar(l, tx, campo.y + (campo.h + m.sube - m.baja) / 2, &p.nombre, 17, t.sobre_superficie);
    }
    l.soltar_recorte();
    if !vacio {
        let b = boton_entrar(d);
        l.redondeado(b, b.h / 2, t.contenedor_primario);
        a.simbolos.centrado(l, b, 18, kit::iconos::simbolo::ARROW_FORWARD, t.sobre_contenedor_primario);
    }
}

/// Los avisos bajo el campo, como los de DMS (`LockNotificationsWidget`): con "contador", un chip con la campana y
/// cuántos; con "nombres", un chip por programa (la inicial, el nombre y cuántos), del mismo ancho, centrados.
fn pintar_avisos(l: &mut Lienzo, d: &bv::Disposicion, a: &Alrededor, fuente: &mut Fuente, medio: &mut Fuente, t: &Tema) {
    let Some(r) = &a.avisos else { return };
    let fila = caja(d.avisos);
    let radio = dms_medidas::radio(12);
    let chip = |l: &mut Lienzo, c: Rect| {
        l.redondeado(c, radio, t.contenedor());
        l.borde(c, radio, 1, t.variante_contorno);
    };
    match r.modo {
        1 => {
            let Some(texto) = bv::texto_contador(r.cuantos) else { return };
            let w = 12 + 16 + 8 + fuente.medir(&texto, 13).ancho + 14;
            let c = Rect::new(fila.x + (fila.w - w) / 2, fila.y, w, fila.h);
            chip(l, c);
            a.simbolos.pintar(l, c.x + 12, c.y + (c.h - 16) / 2, 16, kit::iconos::simbolo::NOTIFICATIONS, t.sobre_variante_superficie);
            let m = fuente.medir(&texto, 13);
            fuente.dibujar(l, c.x + 36, c.y + (c.h + m.sube - m.baja) / 2, &texto, 13, t.sobre_superficie);
        }
        2 if !r.programas.is_empty() => {
            let textos: alloc::vec::Vec<_> = r.programas.iter().map(|(n, k)| bv::texto_chip(n, *k)).collect();
            let ancho_de = |m: &mut Fuente, f: &mut Fuente, (n, k): &(String, Option<String>)| 10 + 16 + 6 + m.medir(n, 12).ancho + k.as_ref().map_or(0, |k| 6 + (f.medir(k, 11).ancho + 8).max(16)) + 12;
            let w = textos.iter().map(|x| ancho_de(medio, fuente, x)).max().unwrap_or(0);
            let cabe = (((l.ancho - 32) + 8) / (w + 8)).max(1) as usize;
            let van = textos.len().min(cabe);
            let total = van as i32 * w + (van as i32 - 1) * 8;
            let mut x = (l.ancho - total) / 2;
            for (n, k) in textos.iter().take(van) {
                let c = Rect::new(x, fila.y, w, fila.h);
                chip(l, c);
                let ic = Rect::new(c.x + 10, c.y + (c.h - 16) / 2, 16, 16);
                l.redondeado(ic, 8, t.contenedor_secundario());
                let ini = bv::inicial(n);
                let m = medio.medir(&ini, 10);
                medio.dibujar(l, ic.x + (16 - m.ancho) / 2, ic.y + (16 + m.sube - m.baja) / 2, &ini, 10, t.sobre_contenedor_secundario());
                let m = medio.medir(n, 12);
                medio.dibujar(l, c.x + 32, c.y + (c.h + m.sube - m.baja) / 2, n, 12, t.sobre_superficie);
                if let Some(k) = k {
                    let mk = fuente.medir(k, 11);
                    let b = Rect::new(c.x + 32 + m.ancho + 6, c.y + (c.h - 16) / 2, (mk.ancho + 8).max(16), 16);
                    l.redondeado(b, 8, t.primario);
                    fuente.dibujar(l, b.x + (b.w - mk.ancho) / 2, b.y + (16 + mk.sube - mk.baja) / 2, k, 11, t.sobre_primario);
                }
                x += w + 8;
            }
        }
        _ => {}
    }
}

/// El símbolo del altavoz para el nivel y el silencio (como el de la barra, `barra_widgets::simbolo_sonido`).
fn simbolo_sonido(nivel: u32, silencio: bool) -> kit::iconos::Simbolo {
    use kit::iconos::simbolo;
    match (silencio, nivel) {
        (true, _) => simbolo::VOLUME_OFF,
        (false, 0) => simbolo::VOLUME_MUTE,
        (false, 1..=50) => simbolo::VOLUME_DOWN,
        _ => simbolo::VOLUME_UP,
    }
}

/// La pantalla de bloqueo de `usuaria` (U3), o la de entrada con `None`: elegir la persona (o teclear su nombre) y
/// luego la contraseña; o recuperar la cuenta con su código, o, en el primer arranque, crear la primera. Al acabar se
/// despide del compositor, para poder volver a conectarse. Devuelve quién entró o desbloqueó, si fue bien.
fn pantalla(usuaria: Option<&str>) -> Option<String> {
    let mut memoria = 0;
    let r = pantalla_dentro(usuaria, &mut memoria);
    if let Some(quien) = &r {
        poner_delante(quien);
    }
    dashos::canal_escribir(VENTANAS, &[wayland::canal::TROZO_ADIOS], &[]);
    // Y la memoria de la pantalla fuera de su sitio, para la siguiente.
    dashos::memoria_quitar(VA);
    if memoria > 0 {
        dashos::cerrar(memoria);
    }
    r
}

/// Las superficies de bloqueo de las demás salidas: cuántas y la memoria de cada una, que se suelta al acabar.
#[derive(Default)]
struct Otras {
    n: usize,
    memoria: [u32; 3],
    va: [usize; 3],
}

impl Drop for Otras {
    fn drop(&mut self) {
        for k in 0..3 {
            if self.memoria[k] != 0 {
                dashos::memoria_quitar(self.va[k]);
                dashos::cerrar(self.memoria[k]);
            }
        }
    }
}

/// Las teclas: Esc va por su código (`TECLA_ESC`); lo demás, por el teclado del kit. En la contraseña, Tab es "olvidé la
/// contraseña" (rama `cuentas-2`).
fn pantalla_dentro(usuaria: Option<&str>, memoria: &mut u32) -> Option<String> {
    let entrada = usuaria.is_none();
    // Rama `cuentas-2`: la lista de cuentas (y si toca el primer arranque), para elegir en vez de teclear.
    let (cuentas, primer) = if entrada { cuentas_de_entrada() } else { (alloc::vec::Vec::new(), false) };
    let mut p = Pantalla::vacia(entrada);
    p.cuentas = cuentas;
    p.primer = primer;
    p.nombre = String::from(usuaria.unwrap_or(""));
    if entrada {
        p.modo = if p.primer && p.cuentas.is_empty() { Modo::Asistente } else if p.cuentas.is_empty() { Modo::Nombre } else { Modo::Lista };
        println!(
            "  [guardian] pantalla de entrada: {} cuentas en la lista{}",
            p.cuentas.len(),
            if p.modo == Modo::Asistente { "; primer arranque: el asistente" } else { "" }
        );
    }
    // Si ya dijo que la pantalla está pintada y tiene el teclado.
    let mut pintada = false;
    const SALIDA: u32 = 20;
    const GESTOR: u32 = 21;
    const CERROJO: u32 = 22;
    const SUP_BLOQUEO: u32 = 23;
    /// El tema del sistema (`dashos_tema_v1`, S10c): llega al enlazar y cada vez que cambia.
    const TEMA: u32 = 24;
    // Multimonitor: las demás salidas, cada una con su superficie de bloqueo (ext-session-lock pide una por salida). En
    // ellas solo el fondo (S27, oleada 6: el desenfocado, o el color de la superficie del tema): lo de escribir va en la
    // principal.
    const OTRAS: usize = 3;
    const SALIDA_OTRA: u32 = 40;
    const SUP_OTRA: u32 = 44;
    const BLOQ_OTRA: u32 = 48;
    const POOL_OTRA: u32 = 52;
    const BUFER_OTRA: u32 = 56;
    const VA_OTRAS: usize = 0x45_0000_0000;
    let mut otras = Otras::default();
    let mut tema = Tema::OSCURO;
    // Los glifos, rasterizados antes de bloquear (la primera vez es lento, y mientras tanto no se lee al compositor). En
    // un lienzo pequeño: las letras son del mismo tamaño en cualquier pantalla y se rasterizan aunque caigan fuera
    // (kit-memoria: con 800 × 600 eran 1,9 MB de montón que se quedaban reservados).
    let (mut fuente, mut titulo) = (Fuente::regular(), Fuente::medium());
    {
        const W: i32 = 400;
        const H: i32 = 300;
        let mut prueba = alloc::vec![0u32; (W * H) as usize];
        let mut q = Pantalla::vacia(entrada);
        q.cuentas = p.cuentas.clone();
        q.nombre = String::from(usuaria.unwrap_or("Ana"));
        q.clave = String::from("12345678901234567890");
        q.aviso = Some(String::from("Contraseña incorrecta"));
        let mut a = Alrededor::nuevo();
        a.hora = Some(reloj::fecha(0));
        let todo = Rect::new(0, 0, W, H);
        for modo in [Modo::Clave, Modo::Nombre, Modo::Lista] {
            q.modo = modo;
            pintar(&mut prueba, W, H, &q, todo, &mut fuente, &mut titulo, &Tema::OSCURO, &a);
        }
        q.nombre = String::from("Carla");
        q.aviso = None;
        q.modo = Modo::Clave;
        pintar(&mut prueba, W, H, &q, todo, &mut fuente, &mut titulo, &Tema::OSCURO, &a);
        borrar(&mut q.clave);
        // S27, oleada 6: las cifras del reloj y la fecha, del tamaño de las pantallas de siempre (1920 × 1080,
        // 1280 × 800 y 800 × 600): son grandes y lentas de hacer la primera vez.
        let mut l = Lienzo::new(&mut prueba, W, H, W as usize);
        for (w, h) in [(1920, 1080), (800, 600), (1280, 800)] {
            let tam = bv::letra_reloj(&bv::disponer(w, h).reloj);
            titulo.dibujar(&mut l, 0, 0, "0123456789:", tam, 0xffff_ffff);
        }
        titulo.dibujar(&mut l, 0, 0, "LMJVSD lunes, martes miércoles jueves viernes sábado domingo de enero febrero marzo abril mayo junio julio agosto septiembre octubre noviembre diciembre 0123456789", 22, 0xffff_ffff);
    }
    // S27, oleada 6: lo de alrededor: el fondo desenfocado de `fondo` (en las dos pantallas, como DMS, que pone el del
    // usuario también en la de entrada), la hora y, en la de bloqueo, los avisos según su ajuste.
    let mut alrededor = Alrededor::nuevo();
    alrededor.fondo = fondo_de_bloqueo();
    (alrededor.hora, alrededor.minuto) = ahora_local();
    if !entrada {
        alrededor.avisos = avisos_de_bloqueo();
    }
    println!(
        "  [guardian] alrededor: {}; hora {}",
        alrededor.fondo.as_ref().map_or(String::from("sin fondo (el color del tema)"), |f| alloc::format!("fondo desenfocado de {} x {}", f.1, f.2)),
        alrededor.hora.map_or(String::from("no se sabe"), |f| alloc::format!("{:02}:{:02}", f.hora, f.minuto)),
    );
    if let Some(a) = &alrededor.avisos {
        println!("  [guardian] avisos en la pantalla de bloqueo: modo {}, {} actuales, {} programas", a.modo, a.cuantos, a.programas.len());
    }
    let Some(mut c) = Cliente::conectar() else {
        println!("  [guardian] sin compositor");
        return None;
    };
    let globales = c.globales()?;
    let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
    let (Some(gc), Some(gs), Some(ga), Some(go), Some(gl)) = (
        global("wl_compositor"),
        global("wl_shm"),
        global("wl_seat"),
        global("wl_output"),
        global("ext_session_lock_manager_v1"),
    ) else {
        println!("  [guardian] el compositor no le da el bloqueo");
        return None;
    };
    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(go).texto(Some("wl_output")).uint(4).uint(SALIDA).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gl).texto(Some("ext_session_lock_manager_v1")).uint(1).uint(GESTOR).fin();
    if let Some(gt) = global("dashos_tema_v1") {
        // S27, oleada 6: la versión 5, para recibir el estado de la red y del volumen (como la barra).
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gt).texto(Some("dashos_tema_v1")).uint(5).uint(TEMA).fin();
    }
    e.empezar(ASIENTO, wl_seat::req::GET_KEYBOARD).uint(TECLADO).fin();
    // Rama `cuentas-2`: y el ratón (S0: todo se puede hacer con él): elegir en la lista, las cajas y los botones del pie.
    e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
    e.empezar(GESTOR, ext_session_lock_manager_v1::req::LOCK).uint(CERROJO).fin();
    e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUPERFICIE).fin();
    e.empezar(CERROJO, ext_session_lock_v1::req::GET_LOCK_SURFACE).uint(SUP_BLOQUEO).uint(SUPERFICIE).uint(SALIDA).fin();
    for (k, g) in globales.iter().filter(|g| g.1 == "wl_output" && g.0 != go).map(|g| g.0).take(OTRAS).enumerate() {
        let k = k as u32;
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(g).texto(Some("wl_output")).uint(4).uint(SALIDA_OTRA + k).fin();
        e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUP_OTRA + k).fin();
        e.empezar(CERROJO, ext_session_lock_v1::req::GET_LOCK_SURFACE).uint(BLOQ_OTRA + k).uint(SUP_OTRA + k).uint(SALIDA_OTRA + k).fin();
        otras.n += 1;
    }
    c.mandar(&e);
    let mut teclado = Teclado::new();
    let mut entera = true;
    let (mut ancho, mut alto, mut buf) = (0i32, 0i32, false);
    // Rama `cuentas-2`: dónde está el ratón, y las teclas que hacen los clics (un botón del pie es su tecla; una fila
    // de la lista, elegirla e Intro).
    let (mut rx, mut ry) = (-1i32, -1i32);
    let mut inyectadas: alloc::collections::VecDeque<u32> = alloc::collections::VecDeque::new();
    let mut malas = 0;
    // Esperando la respuesta de `sesiones` a una contraseña: su canal.
    let mut esperando: Option<u32> = None;
    // Hay que repintar: se hace cuando no queda nada por leer (pintar es lento, y si se pinta tras cada tecla, la cola
    // se llena y el compositor suelta a quien no lee).
    let mut repintar = false;
    // S27, oleada 6: otras zonas que repintar (el reloj, el estado, los avisos), además de lo de en medio.
    let mut extras: alloc::vec::Vec<Rect> = alloc::vec::Vec::new();
    loop {
        // S27, oleada 6: cada minuto, la hora (y en la de bloqueo, los avisos).
        if buf {
            let (hora, minuto) = ahora_local();
            if minuto != alrededor.minuto {
                (alrededor.hora, alrededor.minuto) = (hora, minuto);
                extras.push(zona_arriba(ancho, alto));
                if !entrada {
                    let avisos = avisos_de_bloqueo();
                    if avisos != alrededor.avisos {
                        alrededor.avisos = avisos;
                        extras.push(zona_avisos(ancho, alto));
                    }
                }
            }
        }
        if let Some(l) = esperando {
            let mut r = [0u8; 8];
            let (n, _, _) = dashos::canal_leer_plazo(l, &mut r, &mut [], 1);
            if n != dashos::E_PLAZO {
                dashos::cerrar(l);
                esperando = None;
                let estado = if n == 4 || n == 8 { i32::from_le_bytes(r[0..4].try_into().unwrap()) } else { i32::MIN };
                let espera = if n == 8 { u32::from_le_bytes(r[4..8].try_into().unwrap()) } else { 0 };
                if estado == 0 {
                    let mut e = Escritor::new();
                    e.empezar(SUP_BLOQUEO, ext_session_lock_surface_v1::req::DESTROY).fin();
                    e.empezar(SUPERFICIE, wl_surface::req::DESTROY).fin();
                    for k in 0..otras.n as u32 {
                        e.empezar(BLOQ_OTRA + k, ext_session_lock_surface_v1::req::DESTROY).fin();
                        e.empezar(SUP_OTRA + k, wl_surface::req::DESTROY).fin();
                    }
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
                    let nombre = p.nombre.clone();
                    if p.cambiando {
                        println!("  [guardian] cambio de usuario: entra {} ({} intentos malos); {} sigue detrás", nombre, malas, usuaria.unwrap_or("?"));
                        return Some(nombre);
                    }
                    if entrada {
                        // En la prueba, antes, un usuario que no existe: una mala.
                        if uso() {
                            return Some(nombre);
                        }
                        let bien = malas == 1;
                        println!("  [guardian] entra {} tras {} intento malo: {}", nombre, malas, if bien { "bien" } else { "MAL" });
                        return bien.then_some(nombre);
                    }
                    if uso() {
                        println!("  [guardian] desbloqueado");
                        return Some(nombre);
                    }
                    let bien = malas == 1;
                    println!("  [guardian] desbloqueado con la contraseña buena tras {} mala: {}", malas, if bien { "bien" } else { "MAL" });
                    return bien.then_some(nombre);
                }
                if estado == dashos::protocolo::sesiones::OCUPADO && n == 8 {
                    // Rama `cuentas-2`: tras varias malas seguidas, hay que esperar; no se probó.
                    p.aviso = Some(alloc::format!("Demasiados intentos. Espera {} s y prueba otra vez.", espera));
                    println!("  [guardian] demasiados intentos con {}: espera {} s", p.nombre, espera);
                } else {
                    malas += 1;
                    p.aviso = Some(String::from("Contraseña incorrecta"));
                    // Tras el código (no debería fallar), a la contraseña: el código ya no se enseña.
                    if p.modo == Modo::Codigo {
                        p.modo = Modo::Clave;
                        entera = true;
                    }
                    // En la de entrada (y en la de cambio), se vuelve a pedir el nombre si se tecleó; si se eligió en la
                    // lista, se sigue con la misma persona.
                    if (entrada || p.cambiando) && !p.de_lista {
                        p.nombre.clear();
                        p.modo = Modo::Nombre;
                    }
                    println!("  [guardian] contraseña mala en la pantalla de bloqueo ({})", estado);
                }
                repintar = true;
            }
        }
        // Lo que haya que repintar (también lo que acaba de cambiar arriba, la respuesta de `sesiones`).
        let toca = (repintar || !extras.is_empty()) && buf;
        // Rama `cuentas-2`: lo que hizo un clic, como si se tecleara (sin esperar a nada).
        let inyectada = if esperando.is_none() && !toca { inyectadas.pop_front() } else { None };
        let ev = match (inyectada, esperando.is_some(), toca) {
            (Some(codigo), ..) => {
                let mut cuerpo = alloc::vec![0u8; 16];
                cuerpo[8..12].copy_from_slice(&codigo.to_le_bytes());
                cuerpo[12..16].copy_from_slice(&1u32.to_le_bytes());
                Ok(Some((TECLADO, wl_keyboard::ev::KEY, cuerpo)))
            }
            (None, true, _) => c.evento_plazo(20_000),
            (None, false, true) => c.evento_plazo(1),
            (None, false, false) if !inyectadas.is_empty() => c.evento_plazo(1),
            // Sin nada que hacer, hasta el minuto que viene (el reloj).
            (None, false, false) => {
                let s = dashos::hora_ns() / 1_000_000_000 % 60;
                c.evento_plazo((60 - s.min(59)) * 1_000_000 + 50_000)
            }
        };
        let Ok(ev) = ev else {
            println!("  [guardian] se cortó la conexión con el compositor");
            return None;
        };
        if ev.is_none() && toca {
            let px = unsafe { core::slice::from_raw_parts_mut(VA as *mut u32, (ancho * alto) as usize) };
            let primera = entera;
            // Las zonas: entera, o lo de en medio (si cambió) y las de más (el reloj, el estado, los avisos).
            let mut zonas = core::mem::take(&mut extras);
            if entera {
                zonas = alloc::vec![Rect::new(0, 0, ancho, alto)];
            } else if repintar {
                zonas.push(zona_medio(&p, ancho, alto));
            }
            repintar = false;
            entera = false;
            let mut e = Escritor::new();
            e.empezar(SUPERFICIE, wl_surface::req::ATTACH).uint(BUFER).int(0).int(0).fin();
            let t0 = dashos::ahora_us();
            for zona in zonas {
                let z = pintar(px, ancho, alto, &p, zona, &mut fuente, &mut titulo, &tema, &alrededor);
                e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(z.x).int(z.y).int(z.w).int(z.h).fin();
            }
            if primera {
                // S27, oleada 6: lo que cuesta pintarla entera (con el fondo agrandado y el reloj).
                println!("  [guardian] pantalla entera pintada en {} ms", (dashos::ahora_us() - t0) / 1000);
            }
            e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
            // Y un sync: con su respuesta, el compositor ya tiene el búfer y la pantalla tiene el teclado.
            if primera {
                e.empezar(1, wl_display::req::SYNC).uint(SYNC).fin();
            }
            c.mandar(&e);
            continue;
        }
        let (o, orden, cuerpo) = ev.unwrap_or((0, 0, alloc::vec::Vec::new()));
        let palabra = |i: usize| u32::from_le_bytes(cuerpo[4 * i..4 * i + 4].try_into().unwrap());
        match (o, orden) {
            (0, _) => {}
            // Una vez: al repintar entera (Esc en el bloqueo, U6) llega otro sync, y el guion cuenta estas líneas.
            (SYNC, wayland::tablas::wl_callback::ev::DONE) if !pintada => {
                pintada = true;
                println!("  [guardian] pantalla de {} pintada: tiene el teclado", if entrada { "entrada" } else { "bloqueo" });
                // Para las pruebas del ratón: dónde está la primera fila de la lista.
                if p.modo == Modo::Lista {
                    println!("  [guardian] la lista en ({}, {}), filas de {}", ancho / 2, Pantalla::arriba(ancho, alto) + 40 + (ALTO_FILA - 8) / 2, ALTO_FILA);
                }
            }
            (CERROJO, ext_session_lock_v1::ev::LOCKED) => println!("  [guardian] sesión bloqueada"),
            (TEMA, wayland::tablas::dashos_tema_v1::ev::TEMA) => {
                let (color, oscuro) = (palabra(0) | 0xff00_0000, palabra(1) != 0);
                tema = Tema::del_sistema(color, oscuro);
                alrededor.primario_oscuro = if oscuro { tema.primario } else { Tema::del_sistema(color, true).primario };
                println!("  [guardian] tema del sistema: {}, color {:06x}", if oscuro { "oscuro" } else { "claro" }, color & 0xff_ffff);
                entera = true;
                repintar = true;
            }
            // S27, oleada 6: el estado de la red y del volumen (lo da el panel; el compositor lo pasa aquí también).
            (TEMA, wayland::tablas::dashos_tema_v1::ev::ESTADO) if cuerpo.len() >= 12 => {
                let nuevo = Some((palabra(0), palabra(1), palabra(2)));
                if nuevo != alrededor.estado {
                    println!("  [guardian] estado: red {}, volumen {}{}", palabra(0), palabra(1), if palabra(2) != 0 { ", silencio" } else { "" });
                    alrededor.estado = nuevo;
                    if buf {
                        let d = bv::disponer(ancho, alto);
                        extras.push(Rect::new(d.estado.0 - 120, d.estado.1 - 4, 124, bv::ICONO_ESTADO + 8));
                    }
                }
            }
            // Bloq Mayús (el bit 2 de los bloqueados de xkb): se dice bajo el campo, como DMS.
            (TECLADO, wl_keyboard::ev::MODIFIERS) if cuerpo.len() >= 16 => {
                let bloq = palabra(3) & 2 != 0;
                if bloq != alrededor.bloq_mayus {
                    alrededor.bloq_mayus = bloq;
                    repintar = true;
                }
            }
            (CERROJO, ext_session_lock_v1::ev::FINISHED) => {
                println!("  [guardian] el compositor no deja bloquear");
                return None;
            }
            (o, ext_session_lock_surface_v1::ev::CONFIGURE) if (BLOQ_OTRA..BLOQ_OTRA + otras.n as u32).contains(&o) => {
                let k = (o - BLOQ_OTRA) as usize;
                let mut e = Escritor::new();
                e.empezar(o, ext_session_lock_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                let (w, h) = (palabra(1) as i32, palabra(2) as i32);
                if otras.memoria[k] == 0 && w > 0 && h > 0 {
                    let tamano = (w * h * 4) as usize;
                    let va = VA_OTRAS + k * (32 << 20);
                    let m = dashos::memoria_crear(tamano);
                    if (m as i32) > 0 && dashos::memoria_proyectar(m as u32, va, true) == 0 {
                        otras.memoria[k] = m as u32;
                        otras.va[k] = va;
                        let px = unsafe { core::slice::from_raw_parts_mut(va as *mut u32, (w * h) as usize) };
                        // S27, oleada 6: el fondo desenfocado también en las demás salidas (como DMS, en todas); sin él,
                        // el color de la superficie.
                        pintar_fondo(px, w, h, Rect::new(0, 0, w, h), &alrededor, &tema);
                        let copia = dashos::llave_duplicar(m as u32, dashos::LEER | dashos::TRANSFERIR, 0);
                        let (pool, bufer, sup) = (POOL_OTRA + k as u32, BUFER_OTRA + k as u32, SUP_OTRA + k as u32);
                        e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(pool).llave(copia as u32).int(tamano as i32).fin();
                        e.empezar(pool, wl_shm_pool::req::CREATE_BUFFER).uint(bufer).int(0).int(w).int(h).int(w * 4).uint(0).fin();
                        e.empezar(sup, wl_surface::req::ATTACH).uint(bufer).int(0).int(0).fin();
                        e.empezar(sup, wl_surface::req::DAMAGE).int(0).int(0).int(w).int(h).fin();
                        e.empezar(sup, wl_surface::req::COMMIT).fin();
                        println!("  [guardian] pantalla de bloqueo también en la otra salida {} ({}x{})", k + 1, w, h);
                    } else {
                        if (m as i32) > 0 {
                            dashos::cerrar(m as u32);
                        }
                        println!("  [guardian] sin memoria para la otra salida {}", k + 1);
                    }
                }
                c.mandar(&e);
            }
            (SUP_BLOQUEO, ext_session_lock_surface_v1::ev::CONFIGURE) => {
                let mut e = Escritor::new();
                e.empezar(SUP_BLOQUEO, ext_session_lock_surface_v1::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                c.mandar(&e);
                if !buf {
                    (ancho, alto) = (palabra(1) as i32, palabra(2) as i32);
                    let tamano = (ancho * alto * 4) as usize;
                    let m = dashos::memoria_crear(tamano);
                    if (m as i32) > 0 {
                        *memoria = m as u32;
                    }
                    if (m as i32) <= 0 || dashos::memoria_proyectar(m as u32, VA, true) != 0 {
                        println!("  [guardian] sin memoria para la pantalla");
                        return None;
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
            (PUNTERO, wl_pointer::ev::ENTER) => (rx, ry) = ((palabra(2) as i32) >> 8, (palabra(3) as i32) >> 8),
            (PUNTERO, wl_pointer::ev::MOTION) => (rx, ry) = ((palabra(1) as i32) >> 8, (palabra(2) as i32) >> 8),
            (PUNTERO, wl_pointer::ev::LEAVE) => (rx, ry) = (-1, -1),
            // Un clic (el botón izquierdo, al pulsarlo): en una fila, elegirla e Intro; en una caja, a ella; en un botón
            // del pie, su tecla. Mientras se comprueba una contraseña, nada.
            (PUNTERO, wl_pointer::ev::BUTTON) if palabra(2) == 0x110 && palabra(3) == 1 && esperando.is_none() && buf => {
                match p.en(ancho, alto, rx, ry, &mut fuente) {
                    Some(Pulsado::Fila(i)) => {
                        p.elegida = i;
                        inyectadas.push_back(TECLA_INTRO);
                    }
                    Some(Pulsado::Caja(i)) => {
                        p.foco = i;
                        repintar = true;
                    }
                    // Intro en un formulario, con el botón: lo pide (desde la última caja).
                    Some(Pulsado::Tecla(TECLA_INTRO)) if matches!(p.modo, Modo::Recuperar | Modo::Asistente) => {
                        p.foco = p.etiquetas().len() - 1;
                        inyectadas.push_back(TECLA_INTRO);
                    }
                    Some(Pulsado::Tecla(t)) => inyectadas.push_back(t),
                    None => {}
                }
                if let Some(q) = inyectadas.back() {
                    println!("  [guardian] clic en ({}, {}): como la tecla {}", rx, ry, q);
                }
            }
            // Mientras se comprueba una contraseña, lo tecleado no cuenta (pero se lee).
            (TECLADO, wl_keyboard::ev::KEY) if esperando.is_some() => {
                teclado.tecla(palabra(2), palabra(3) == 1);
            }
            // Esc: en la de bloqueo, a "Cambiar de usuario" y vuelta (U6); en la de entrada, de la contraseña o del nombre a
            // la lista; de recuperar, a la contraseña. Lo tecleado se olvida.
            (TECLADO, wl_keyboard::ev::KEY) if palabra(2) == TECLA_ESC => {
                if palabra(3) != 1 {
                    continue;
                }
                p.aviso = None;
                match p.modo {
                    Modo::Recuperar => {
                        p.borrar_cajas();
                        p.modo = Modo::Clave;
                        println!("  [guardian] olvidé la contraseña: vuelve a la contraseña de {}", p.nombre);
                    }
                    Modo::Asistente | Modo::Codigo => continue,
                    // En el bloqueo, a cambiar de usuario, y de ahí (desde la lista, o sin lista), de vuelta.
                    _ if !entrada && (!p.cambiando || p.modo == Modo::Lista || p.cuentas.is_empty()) => {
                        p.cambiando = !p.cambiando;
                        borrar(&mut p.clave);
                        if p.cambiando {
                            if p.cuentas.is_empty() {
                                p.cuentas = cuentas_de_entrada().0;
                            }
                            p.elegida = p.cuentas.iter().position(|c| Some(c.0.as_str()) != usuaria).unwrap_or(0);
                            p.a_la_lista();
                            println!("  [guardian] cambio de usuario: se pide el nombre");
                        } else {
                            p.nombre = String::from(usuaria.unwrap_or(""));
                            p.modo = Modo::Clave;
                            println!("  [guardian] cambio de usuario: vuelve al bloqueo de {}", usuaria.unwrap_or("?"));
                        }
                    }
                    Modo::Clave | Modo::Nombre if !p.cuentas.is_empty() => p.a_la_lista(),
                    _ => continue,
                }
                entera = true;
                repintar = true;
            }
            (TECLADO, wl_keyboard::ev::KEY) => {
                let Some(tecla) = teclado.tecla(palabra(2), palabra(3) == 1) else { continue };
                repintar = true;
                match (p.modo, tecla) {
                    // La lista: flechas para elegir; Intro, a la contraseña de esa persona (o a teclear el nombre); una
                    // letra, a teclear el nombre con ella.
                    (Modo::Lista, Tecla::Arriba) => p.elegida = p.elegida.saturating_sub(1),
                    (Modo::Lista, Tecla::Abajo) => p.elegida = (p.elegida + 1).min(p.cuentas.len()),
                    (Modo::Lista, Tecla::Inicio) => p.elegida = 0,
                    (Modo::Lista, Tecla::Fin) => p.elegida = p.cuentas.len(),
                    (Modo::Lista, Tecla::Intro) => {
                        p.aviso = None;
                        match p.cuentas.get(p.elegida) {
                            Some((n, _)) => {
                                p.nombre = n.clone();
                                p.de_lista = true;
                                p.modo = Modo::Clave;
                                println!("  [guardian] lista: elige a {}", p.nombre);
                            }
                            None => {
                                p.nombre.clear();
                                p.de_lista = false;
                                p.modo = Modo::Nombre;
                                println!("  [guardian] lista: otra persona, se teclea el nombre");
                            }
                        }
                        entera = true;
                    }
                    (Modo::Lista, Tecla::Letra(ch)) if !ch.is_control() && ch != ' ' => {
                        p.aviso = None;
                        p.nombre.clear();
                        p.nombre.push(ch);
                        p.de_lista = false;
                        p.modo = Modo::Nombre;
                        entera = true;
                    }
                    (Modo::Nombre, Tecla::Letra(ch)) if p.nombre.len() < 16 => p.nombre.push(ch),
                    (Modo::Nombre, Tecla::Retroceso) => {
                        p.nombre.pop();
                    }
                    (Modo::Nombre, Tecla::Intro) if !p.nombre.is_empty() => {
                        p.aviso = None;
                        p.de_lista = false;
                        p.modo = Modo::Clave;
                    }
                    (Modo::Clave, Tecla::Letra(ch)) if p.clave.len() < 128 => p.clave.push(ch),
                    (Modo::Clave, Tecla::Retroceso) => {
                        p.clave.pop();
                    }
                    (Modo::Clave, Tecla::Intro) => {
                        let orden = if entrada || p.cambiando { ENTRAR } else { VOLVER };
                        let nombre = p.nombre.clone();
                        esperando = comprobar(orden, &nombre, &mut p.clave);
                    }
                    // Rama `cuentas-2`: olvidé la contraseña (S6b): con el código de recuperación, otra.
                    (Modo::Clave, Tecla::Tabulador) if !p.nombre.is_empty() => {
                        borrar(&mut p.clave);
                        p.borrar_cajas();
                        p.aviso = None;
                        p.modo = Modo::Recuperar;
                        entera = true;
                        println!("  [guardian] olvidé la contraseña: {} pone el código de recuperación", p.nombre);
                    }
                    // Los formularios: recuperar y el primer arranque. Tab y las flechas cambian de caja; Intro, a la
                    // siguiente, y en la última (con todo puesto), se pide.
                    (Modo::Recuperar | Modo::Asistente, Tecla::Tabulador | Tecla::Abajo) => p.foco = (p.foco + 1) % p.etiquetas().len(),
                    (Modo::Recuperar | Modo::Asistente, Tecla::TabuladorAtras | Tecla::Arriba) => {
                        p.foco = (p.foco + p.etiquetas().len() - 1) % p.etiquetas().len()
                    }
                    (Modo::Recuperar | Modo::Asistente, Tecla::Retroceso) => {
                        p.cajas[p.foco].pop();
                    }
                    (Modo::Recuperar | Modo::Asistente, Tecla::Letra(ch)) if p.cajas[p.foco].len() < p.tope(p.foco) => p.cajas[p.foco].push(ch),
                    (Modo::Recuperar | Modo::Asistente, Tecla::Intro) => {
                        let n = p.etiquetas().len();
                        // Con todo lo que hace falta puesto (el nombre completo puede ir vacío), Intro lo pide; si no,
                        // pasa a la caja siguiente. Tras un código malo, las nuevas siguen puestas.
                        if (0..n).any(|i| p.cajas[i].is_empty() && !(p.modo == Modo::Asistente && i == 1)) {
                            p.foco = (p.foco + 1) % n;
                            continue;
                        }
                        if p.cajas[n - 1] != p.cajas[n - 2] {
                            println!("  [guardian] {}: las dos contraseñas no son iguales", if p.modo == Modo::Recuperar { "recuperar" } else { "primer arranque" });
                            p.aviso = Some(String::from("Las dos contraseñas nuevas no son iguales."));
                            borrar(&mut p.cajas[n - 1]);
                            borrar(&mut p.cajas[n - 2]);
                            p.foco = n - 2;
                            entera = true;
                            continue;
                        }
                        let recuperar = p.modo == Modo::Recuperar;
                        let (estado, espera, codigo) = if recuperar {
                            pedir_con_codigo(dashos::protocolo::sesiones::RECUPERAR, &p.nombre, &p.cajas[0], &p.cajas[1])
                        } else {
                            pedir_con_codigo(dashos::protocolo::sesiones::PRIMERA_CUENTA, &p.cajas[0], &p.cajas[1], &p.cajas[2])
                        };
                        let quien = if recuperar { p.nombre.clone() } else { p.cajas[0].clone() };
                        if estado == 0 && !codigo.is_empty() {
                            println!("  [guardian] {} {}: hecho; el código nuevo, en pantalla", if recuperar { "recuperar" } else { "primer arranque: crear" }, quien);
                            if codigos_a_la_consola() {
                                println!("  [guardian] (prueba) el código de recuperación de {} es {}", quien, codigo);
                            }
                            // La contraseña nueva, para entrar tras apuntar el código.
                            p.clave = core::mem::take(&mut p.cajas[n - 2]);
                            p.nombre = quien;
                            p.borrar_cajas();
                            p.codigo = codigo;
                            p.aviso = None;
                            p.modo = Modo::Codigo;
                        } else {
                            let dice = que_dice(estado, espera);
                            println!("  [guardian] {} {}: no ({}: {})", if recuperar { "recuperar" } else { "primer arranque: crear" }, quien, estado, dice);
                            p.aviso = Some(dice);
                            if recuperar && estado == dashos::protocolo::sesiones::PROTEGIDO {
                                borrar(&mut p.cajas[0]);
                                p.foco = 0;
                            }
                        }
                        entera = true;
                    }
                    (Modo::Codigo, Tecla::Intro) => {
                        borrar(&mut p.codigo);
                        let orden = if entrada || p.cambiando { ENTRAR } else { VOLVER };
                        let nombre = p.nombre.clone();
                        println!("  [guardian] código apuntado: entra {}", nombre);
                        esperando = comprobar(orden, &nombre, &mut p.clave);
                        entera = true;
                    }
                    _ => repintar = false,
                }
            }
            (1, wl_display::ev::ERROR) => {
                println!("  [guardian] error de protocolo");
                return None;
            }
            _ => {}
        }
    }
}
