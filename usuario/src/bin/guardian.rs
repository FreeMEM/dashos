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
//! Llaves: 1 el canal "ventanas" (con el poder de escritorio), 2 el canal "sesiones".

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
#[allow(dead_code)]
mod ventana_cliente;

#[path = "../preguntas_ficheros.rs"]
#[allow(dead_code)]
mod preguntas_ficheros;
use preguntas_ficheros as pf;

use alloc::string::String;
use dashos::println;
use dashos::protocolo::sesiones::{ENTRAR, FICHA, LANZAR, POR_PAGINA, PREGUNTA, PREGUNTAS, PROCESOS, RESPUESTA, RESPUESTA_ADMIN, RETIRAR, SALIR, VOLVER};
use kit::lienzo::{Lienzo, Rect};
use kit::tema::Tema;
use kit::teclado::{Tecla, Teclado};
use kit::texto::Fuente;
use ventana_cliente::*;
use wayland::tablas::{
    ext_idle_notification_v1, ext_idle_notifier_v1, ext_session_lock_manager_v1, ext_session_lock_surface_v1,
    ext_session_lock_v1, wl_compositor, wl_display, wl_keyboard,
    wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface, zwlr_layer_shell_v1, zwlr_layer_surface_v1,
};
use wayland::Escritor;

dashos::programa!(principal);
dashos::monton!();

const SESIONES: u32 = 2;
/// El canal del centro de control (S10c): por él solo se le pide que abra una categoría.
const CENTRO: u32 = 3;
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
    let mut r = [0u8; 4];
    let n = if dashos::canal_escribir(SESIONES, m, &llaves[..1 + mas.len()]) == 0 {
        dashos::canal_leer_plazo(lectura, &mut r, &mut [], 3_000_000).0
    } else {
        -1
    };
    dashos::cerrar(lectura);
    if n == 4 { i32::from_le_bytes(r) } else { i32::MIN }
}

/// La imagen para usar (llave 4, "uso"): sin las cuentas de la prueba (cuántos intentos malos espera el guion).
static USO: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

fn uso() -> bool {
    USO.load(core::sync::atomic::Ordering::Relaxed)
}

fn principal() -> u64 {
    // Antes de crear nada (cada llave nueva toma el siguiente número libre, y la 4 sería una suya): ¿hay llave 4?
    let copia = dashos::llave_duplicar(4, u64::MAX, 0);
    if copia > 0 {
        dashos::cerrar(copia as u32);
        USO.store(true, core::sync::atomic::Ordering::Relaxed);
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
        println!("  [guardian] modo de uso: pantalla de entrada");
        loop {
            if let Some(quien) = pantalla(None) {
                println!("  [guardian] entra {}", quien);
                break;
            }
        }
        atender(preguntas);
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
        if de_apagar(&q) {
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
    let mandado = dashos::canal_escribir(CENTRO, b"abrir permisos", &[suyo]);
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
        if dashos::canal_leer_plazo(preguntas, &mut q, &mut [], 1_000).0 as usize != PREGUNTA {
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
        let acaba = e_shell == 0 && esperar_fin(p_shell, 240_000_000);
        println!("  [guardian] Shell nativo: lanza System/Shell como Ana: {}, acaba: {}", e_shell, acaba);
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
                pantalla(None);
            }
            true
        }
        _ => false,
    }
}

/// Tras la prueba: las preguntas que lleguen, para siempre. Las de apagar, como `de_apagar`; bloquear, con su pantalla;
/// los permisos, al usuario; las acciones de administrador, negadas (aquí no se pide la contraseña).
fn atender(preguntas: u32) -> ! {
    loop {
        let mut q = [0u8; PREGUNTA];
        if dashos::canal_leer(preguntas, &mut q, &mut []).0 as usize != PREGUNTA || de_apagar(&q) {
            continue;
        }
        let campo = |b: &[u8]| -> usize { b.iter().position(|&c| c == 0).unwrap_or(b.len()) };
        let programa = core::str::from_utf8(&q[8..8 + campo(&q[8..24])]).unwrap_or("?");
        let servicio = &q[24..24 + campo(&q[24..40])];
        if servicio == b"*bloquear" {
            pantalla(Some(programa));
            continue;
        }
        // S28: un fichero de la caja Amiga: siempre, esta vez o no.
        if pf::es_permiso(servicio) {
            let r = preguntar_fichero(programa, servicio, &q[40..]);
            let mut m = [0u8; 12];
            m[0..4].copy_from_slice(&RESPUESTA.to_le_bytes());
            m[4..8].copy_from_slice(&q[0..4]);
            m[8..12].copy_from_slice(&r.to_le_bytes());
            pedir(&m, &[]);
            continue;
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
        if dashos::canal_leer_plazo(preguntas, &mut q, &mut [], 200_000).0 as usize != PREGUNTA {
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
}

fn preguntar_dentro(programa: &str, pregunta: &Pregunta, memoria: &mut u32) -> u32 {
    const CAPAS: u32 = 20;
    const CAPA: u32 = 21;
    // El diálogo; su sombra la pone el compositor, como a las demás capas. El de un fichero, más grande.
    let fichero = matches!(pregunta, Pregunta::Fichero { .. });
    let (ancho_d, alto_d) = if fichero { DIALOGO_FICHERO } else { DIALOGO };
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
    let fin = dashos::ahora_us() + 30_000_000;
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
                pintar_pregunta(px, ancho_d, alto_d, programa, pregunta, &mut fuente, &mut medio, &tema);
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
                match tecla {
                    Some(Tecla::Letra(ch)) if !a_tiempo => {
                        println!("  [guardian] tecla {:?} antes de tiempo: no cuenta", ch);
                    }
                    // "Siempre" (ficheros de la caja) y "Sí" (permisos de servicios) piden Mayúsculas y S (decisiones del
                    // usuario del 2026-11-13): una "s" suelta, tecleando en otra ventana cuando sale la pregunta, no debe
                    // dar un permiso.
                    Some(Tecla::Letra('S')) => return pf::RESPUESTA_SIEMPRE,
                    Some(Tecla::Letra('e' | 'E')) if fichero => return pf::RESPUESTA_ESTA_VEZ,
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
/// Lo más largo que se enseña de un nombre que llega de fuera (el resto, "…").
const MAX_NOMBRE_VISTO: usize = 60;

/// Pinta la pregunta de U4 como un diálogo de Material 3, con los colores del tema del sistema (S10c): el contenedor
/// alto con esquinas de 28 (fuera, transparente; la sombra, del compositor), 24 de relleno, el título (headline small, 24), el texto (body
/// medium, 14, en el color suave) y abajo a la derecha las respuestas. No se pulsan (`guardian` solo atiende al
/// teclado): cada una lleva su tecla en una tecla dibujada, con contorno, y su nombre en el primario.
/// S28: la de un fichero de la caja Amiga: "Programa de Amiga" de título, y "<programa> quiere cambiar <fichero>, un
/// programa ejecutable. ¿Lo dejas?" partido en líneas; las respuestas, "Mayús+S Siempre", "E Esta vez" y "N No".
#[allow(clippy::too_many_arguments)]
fn pintar_pregunta(px: &mut [u32], ancho: i32, alto: i32, programa: &str, pregunta: &Pregunta, fuente: &mut Fuente, medio: &mut Fuente, t: &Tema) {
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
        // S10: los permisos de commodities, con su frase en llano (S6b); los demás, aún con su nombre.
        Pregunta::Servicio(servicio) => (
            alloc::vec![
                match dashos::protocolo::sesiones::PODERES_COMPOSITOR.iter().find(|p| p.0 == *servicio) {
                    Some(p) => alloc::format!("Quiere {}.", p.2),
                    None => alloc::format!("Quiere usar \"{}\".", servicio),
                },
                String::from("Contesta con el teclado."),
            ],
            &[("Mayús+S", "Sí"), ("N", "No")],
        ),
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
    for (i, linea) in lineas.iter().enumerate() {
        fuente.dibujar(&mut l, x, y + 20 * i as i32 + 15, linea, 14, t.sobre_variante_superficie);
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

/// Manda a `sesiones` la contraseña de la usuaria para comprobarla (VOLVER: sin tocar su volumen) y la borra. No
/// espera: devuelve el canal por el que llegará la respuesta (tras una mala, `sesiones` tarda a propósito, y mientras
/// tanto hay que seguir leyendo al compositor).
fn comprobar(orden: u32, usuaria: &str, clave: &mut String) -> Option<u32> {
    // Cuántas letras (no cuáles): para ver una tecla perdida en las pruebas.
    println!("  [guardian] se comprueba una contraseña de {} letras", clave.chars().count());
    let mut m = [0u8; 160];
    let (u, c) = (usuaria.as_bytes(), clave.as_bytes());
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
/// Con `todo`, la pantalla entera; si no, solo la caja y el aviso (lo que cambia al teclear), que es mucho menos: el
/// compositor suelta a quien tarda en leer (E8). Devuelve la zona pintada.
/// Con los colores del tema del sistema (S10c: `dashos_tema_v1`, el que pone el centro de control); el rojo del error
/// es el de Material 3 para el fondo claro u oscuro.
///
/// U6: con `cambio`, la de "Cambiar de usuario" (como la de entrada, sobre una sesión bloqueada). En la de bloqueo, al
/// pie, cómo cambiar de usuario (Esc); en la de cambio, cómo volver.
fn pintar(px: &mut [u32], ancho: i32, alto: i32, usuaria: &str, entrada: bool, cambio: bool, letras: usize, mala: bool, todo: bool, fuente: &mut Fuente, titulo: &mut Fuente, t: &Tema) -> Rect {
    let bloqueo = !entrada;
    let entrada = entrada || cambio;
    let mut l = Lienzo::new(px, ancho, alto, ancho as usize);
    let oscuro = (t.superficie >> 8 & 0xff) < 0x80;
    let error = if oscuro { 0xfff2_b8b5 } else { 0xffb3_261e };
    let y = alto / 2 - 90;
    // Al teclear, solo la franja que cambia: la caja y el aviso; en la de entrada, también la frase y el nombre.
    let zona = match (todo, entrada) {
        (true, _) => Rect::new(0, 0, ancho, alto),
        (false, false) => Rect::new(0, y + 116, ancho, 100),
        (false, true) => Rect::new(0, y + 20, ancho, 196),
    };
    l.rellenar(zona, t.superficie);
    let centrado = |f: &mut Fuente, t: &str, tam: u32| (ancho - f.medir(t, tam).ancho) / 2;
    if todo {
        let cabeza = if cambio { "Cambiar de usuario" } else if entrada { "Entrar en DashOS" } else { "Sesión bloqueada" };
        let x = centrado(titulo, cabeza, 34);
        titulo.dibujar(&mut l, x, y, cabeza, 34, t.sobre_superficie);
    }
    if todo || entrada {
        // La frase de quien entra, en cuanto se sabe quién es (S6b: así se sabe que es el sistema).
        let frase = FRASES.iter().find(|f| f.0 == usuaria).map_or(alloc::string::String::new(), |f| alloc::format!("\"{}\"", f.1));
        let x = centrado(fuente, &frase, 22);
        fuente.dibujar(&mut l, x, y + 48, &frase, 22, t.primario);
        let x = centrado(fuente, usuaria, 20);
        fuente.dibujar(&mut l, x, y + 100, usuaria, 20, t.sobre_variante_superficie);
    }
    // La caja de la contraseña, con un punto por letra.
    let caja = Rect::new(ancho / 2 - 160, y + 120, 320, 48);
    // Material 3: la caja en el contenedor alto, sin marco; con error, el borde rojo.
    l.redondeado(caja, kit::tema::RADIO_DIALOGO, t.contenedor_alto());
    if mala {
        l.borde(caja, 24, 2, error);
    }
    for i in 0..if letras == usize::MAX { 0 } else { letras.min(20) } as i32 {
        l.redondeado(Rect::new(caja.x + 24 + i * 14, caja.y + 19, 10, 10), 5, t.sobre_superficie);
    }
    let aviso = match (mala, entrada && letras == usize::MAX) {
        (true, _) => "Contraseña incorrecta",
        (false, true) => "Escribe tu nombre y pulsa Intro",
        (false, false) => "Escribe tu contraseña y pulsa Intro",
    };
    let x = centrado(fuente, aviso, 16);
    fuente.dibujar(&mut l, x, y + 200, aviso, 16, if mala { error } else { t.sobre_variante_superficie });
    // U6: al pie, en una pastilla del contenedor (Material 3), la tecla para cambiar de usuario o para volver.
    if todo && bloqueo {
        let pie = if cambio { "Esc: volver a la sesión bloqueada" } else { "Esc: cambiar de usuario" };
        let m = fuente.medir(pie, 15);
        let pastilla = Rect::new((ancho - m.ancho - 40) / 2, y + 236, m.ancho + 40, 36);
        l.redondeado(pastilla, 18, t.contenedor());
        fuente.dibujar(&mut l, pastilla.x + 20, pastilla.y + (36 + m.sube - m.baja) / 2, pie, 15, t.sobre_variante_superficie);
    }
    zona
}

/// U3: bloquea la sesión (G5) con su pantalla y espera la contraseña de la usuaria. Devuelve si desbloqueó con la
/// buena después de una mala, como hace la prueba.
/// La pantalla de bloqueo de `usuaria` (U3), o la de entrada con `None`: primero el nombre, luego la contraseña. Al
/// acabar se despide del compositor, para poder volver a conectarse. Devuelve quién entró o desbloqueó, si fue bien.
fn pantalla(usuaria: Option<&str>) -> Option<String> {
    let mut memoria = 0;
    let r = pantalla_dentro(usuaria, &mut memoria);
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

fn pantalla_dentro(usuaria: Option<&str>, memoria: &mut u32) -> Option<String> {
    let entrada = usuaria.is_none();
    let mut nombre = String::from(usuaria.unwrap_or(""));
    let mut en_nombre = entrada;
    // U6: en "Cambiar de usuario" (Esc en la de bloqueo); quien estaba bloqueado sigue detrás.
    let mut cambiando = false;
    // Si ya dijo que la pantalla está pintada y tiene el teclado.
    let mut pintada = false;
    const SALIDA: u32 = 20;
    const GESTOR: u32 = 21;
    const CERROJO: u32 = 22;
    const SUP_BLOQUEO: u32 = 23;
    /// El tema del sistema (`dashos_tema_v1`, S10c): llega al enlazar y cada vez que cambia.
    const TEMA: u32 = 24;
    // Multimonitor: las demás salidas, cada una con su superficie de bloqueo (ext-session-lock pide una por salida). En
    // ellas solo el color de la superficie del tema: lo de escribir va en la principal.
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
        pintar(&mut prueba, W, H, usuaria.unwrap_or("Ana"), entrada, false, 20, true, true, &mut fuente, &mut titulo, &Tema::OSCURO);
        pintar(&mut prueba, W, H, usuaria.unwrap_or("Carla"), entrada, false, 20, false, true, &mut fuente, &mut titulo, &Tema::OSCURO);
        if !entrada {
            pintar(&mut prueba, W, H, "Carla", false, true, usize::MAX, false, true, &mut fuente, &mut titulo, &Tema::OSCURO);
        }
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
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gt).texto(Some("dashos_tema_v1")).uint(1).uint(TEMA).fin();
    }
    e.empezar(ASIENTO, wl_seat::req::GET_KEYBOARD).uint(TECLADO).fin();
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
                    if cambiando {
                        println!("  [guardian] cambio de usuario: entra {} ({} intentos malos); {} sigue detrás", nombre, malas, usuaria.unwrap_or("?"));
                        return Some(nombre.clone());
                    }
                    if entrada {
                        // En la prueba, antes, un usuario que no existe: una mala.
                        if uso() {
                            return Some(nombre.clone());
                        }
                        let bien = malas == 1;
                        println!("  [guardian] entra {} tras {} intento malo: {}", nombre, malas, if bien { "bien" } else { "MAL" });
                        return bien.then(|| nombre.clone());
                    }
                    if uso() {
                        println!("  [guardian] desbloqueado");
                        return Some(nombre.clone());
                    }
                    let bien = malas == 1;
                    println!("  [guardian] desbloqueado con la contraseña buena tras {} mala: {}", malas, if bien { "bien" } else { "MAL" });
                    return bien.then(|| nombre.clone());
                }
                malas += 1;
                ultima_mala = true;
                // En la de entrada (y en la de cambio), se vuelve a pedir el nombre.
                if entrada || cambiando {
                    nombre.clear();
                    en_nombre = true;
                }
                println!("  [guardian] contraseña mala en la pantalla de bloqueo ({})", r);
                repintar = true;
            }
        }
        let ev = match (esperando.is_some(), repintar && buf) {
            (true, _) => c.evento_plazo(20_000),
            (false, true) => c.evento_plazo(1),
            (false, false) => c.evento().ok_or(()).map(Some),
        };
        let Ok(ev) = ev else {
            println!("  [guardian] se cortó la conexión con el compositor");
            return None;
        };
        if ev.is_none() && repintar && buf {
            repintar = false;
            let px = unsafe { core::slice::from_raw_parts_mut(VA as *mut u32, (ancho * alto) as usize) };
            let primera = entera;
            let letras = if en_nombre { usize::MAX } else { clave.chars().count() };
            let z = pintar(px, ancho, alto, &nombre, entrada, cambiando, letras, ultima_mala, entera, &mut fuente, &mut titulo, &tema);
            entera = false;
            let mut e = Escritor::new();
            e.empezar(SUPERFICIE, wl_surface::req::ATTACH).uint(BUFER).int(0).int(0).fin();
            e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(z.x).int(z.y).int(z.w).int(z.h).fin();
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
                println!("  [guardian] pantalla de {} pintada: tiene el teclado", if entrada { "entrada" } else { "bloqueo" })
            }
            (CERROJO, ext_session_lock_v1::ev::LOCKED) => println!("  [guardian] sesión bloqueada"),
            (TEMA, wayland::tablas::dashos_tema_v1::ev::TEMA) => {
                let (color, oscuro) = (palabra(0) | 0xff00_0000, palabra(1) != 0);
                tema = Tema::del_sistema(color, oscuro);
                println!("  [guardian] tema del sistema: {}, color {:06x}", if oscuro { "oscuro" } else { "claro" }, color & 0xff_ffff);
                entera = true;
                repintar = true;
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
                        px.fill(tema.superficie);
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
            // Mientras se comprueba una contraseña, lo tecleado no cuenta (pero se lee).
            (TECLADO, wl_keyboard::ev::KEY) if esperando.is_some() => {
                teclado.tecla(palabra(2), palabra(3) == 1);
            }
            // U6: Esc en la de bloqueo pasa a "Cambiar de usuario"; otra vez, vuelve. Lo tecleado se olvida.
            (TECLADO, wl_keyboard::ev::KEY) if !entrada && palabra(2) == TECLA_ESC => {
                if palabra(3) == 1 {
                    cambiando = !cambiando;
                    unsafe { clave.as_bytes_mut().fill(0) };
                    clave.clear();
                    nombre = String::from(if cambiando { "" } else { usuaria.unwrap_or("") });
                    en_nombre = cambiando;
                    ultima_mala = false;
                    entera = true;
                    repintar = true;
                    if cambiando {
                        println!("  [guardian] cambio de usuario: se pide el nombre");
                    } else {
                        println!("  [guardian] cambio de usuario: vuelve al bloqueo de {}", usuaria.unwrap_or("?"));
                    }
                }
            }
            (TECLADO, wl_keyboard::ev::KEY) => match teclado.tecla(palabra(2), palabra(3) == 1) {
                Some(Tecla::Letra(ch)) if en_nombre && nombre.len() < 16 => {
                    nombre.push(ch);
                    repintar = true;
                }
                Some(Tecla::Letra(ch)) if !en_nombre && clave.len() < 128 => {
                    clave.push(ch);
                    repintar = true;
                }
                Some(Tecla::Retroceso) => {
                    if en_nombre { nombre.pop() } else { clave.pop() };
                    repintar = true;
                }
                Some(Tecla::Intro) if en_nombre => {
                    en_nombre = nombre.is_empty();
                    repintar = true;
                }
                Some(Tecla::Intro) => {
                    esperando = comprobar(if entrada || cambiando { ENTRAR } else { VOLVER }, &nombre, &mut clave);
                    repintar = true;
                }
                _ => {}
            },
            (1, wl_display::ev::ERROR) => {
                println!("  [guardian] error de protocolo");
                return None;
            }
            _ => {}
        }
    }
}
