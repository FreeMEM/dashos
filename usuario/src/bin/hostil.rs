//! Un cliente hostil de los servicios (R1, B6 de `docs/REVISION_ARQUITECTURA.md`). Ataca al servicio de archivos, al
//! de sonido y al compositor con lo que cualquier programa puede mandar, y después comprueba que cada uno sigue
//! contestando a una petición normal, con un plazo. Llaves, marcadas por `inicio`, en el orden en que se nombra cada
//! canal por primera vez en `PROGRAMAS` (no en el de la lista de este programa): 1 "ventanas", 2 "archivos", 3
//! "sonido".
//!
//! Ataques:
//! - un mensaje vacío (E1: el compositor leía el primer byte del mensaje anterior);
//! - cientos de mensajes con 8 llaves que el servicio no usa (E2: si no las cierra, se le llena la tabla de llaves y
//!   gira para siempre sin poder leer).
//!
//! Saca una línea por servicio: "sigue: sí" o "sigue: no".
//!
//! Y los permisos del servicio de archivos (R6): es el usuario 1001, encerrado en el cajón `C` de un disco que es todo
//! del 1000. No llega a lo de fuera de `C`, no entra en `C/Privado` (0700), no escribe, crea, borra ni protege lo
//! ajeno, y no administra volúmenes.

#![no_std]
#![no_main]

extern crate alloc;

use dashos::println;
use dashos::protocolo::archivos::*;
use wayland::canal::{TROZO_ADIOS, TROZO_CONECTAR, TROZO_DATOS};
use wayland::tablas::{wl_callback, wl_display};
use wayland::{Escritor, leer_cabecera};

dashos::programa!(principal);
dashos::monton!();

// Las llaves van en el orden en que se nombra cada canal por primera vez en el arranque: ventanas, archivos,
// sesiones (que va tras archivos, U3) y sonido.
const VENTANAS: u32 = 1;
const ARCHIVOS: u32 = 2;
const SESIONES: u32 = 3;
const SONIDO: u32 = 4;
const E_LIMITE: i64 = -8;
/// Mensajes con 8 llaves por ataque: más que la tabla de llaves de un proceso (1.024).
const ATAQUES: usize = 160;
const PLAZO_US: u64 = 3_000_000;
const VA_SONIDO: usize = 0x40_0000_0000;

fn dormir_ms(ms: u64) {
    dashos::dormir_us(ms * 1000);
}

/// Escribe con reintentos si la cola está llena. Devuelve si se pudo.
fn escribir(canal: u32, datos: &[u8], llaves: &[u32]) -> bool {
    for _ in 0..200 {
        match dashos::canal_escribir(canal, datos, llaves) {
            0 => return true,
            -4 => return false, // cerrado: el servicio murió
            _ => dormir_ms(1),
        }
    }
    false
}

/// `n` copias de una llave de memoria, para mandarlas de relleno.
fn relleno(memoria: u32, n: usize) -> alloc::vec::Vec<u32> {
    (0..n).map(|_| dashos::llave_duplicar(memoria, dashos::LEER | dashos::TRANSFERIR, 0) as u32).collect()
}

/// Manda `ATAQUES` mensajes con `cuerpo` y 8 llaves cada uno. Para si el servicio deja de leer (cola llena mucho
/// rato); las llaves que no salen se cierran.
fn inundar(canal: u32, memoria: u32, cuerpo: &[u8]) {
    for _ in 0..ATAQUES {
        let ll = relleno(memoria, 8);
        if !escribir(canal, cuerpo, &ll) {
            ll.iter().for_each(|&l| { dashos::cerrar(l); });
            return;
        }
    }
}

/// Lee de `lectura` hasta `PLAZO_US`, con un temporizador sobre `copia` (una copia del extremo de escritura). Devuelve
/// los bytes del primer mensaje que no sea del temporizador, o `None` si vence el plazo o se cierra el canal.
fn leer_con_plazo(lectura: u32, copia: u32, buf: &mut [u8]) -> Option<usize> {
    let desde = dashos::ahora_us();
    let r = leer_con_plazo_sin_aviso(lectura, copia, buf);
    if r.is_none() {
        println!("  [hostil] sin respuesta tras {} ms", (dashos::ahora_us() - desde) / 1000);
    }
    r
}

fn leer_con_plazo_sin_aviso(lectura: u32, copia: u32, buf: &mut [u8]) -> Option<usize> {
    let t = dashos::temporizador_crear(copia, PLAZO_US, 0);
    let mut ll = [0u32; 8];
    let r = loop {
        let (n, k) = dashos::canal_leer(lectura, buf, &mut ll);
        ll[..k].iter().for_each(|&l| { dashos::cerrar(l); });
        if n < 0 {
            break None;
        }
        // El aviso del temporizador: 8 bytes. Ninguna respuesta de estos servicios mide 8.
        if n == 8 {
            break None;
        }
        break Some(n as usize);
    };
    if t > 0 {
        dashos::cerrar(t as u32);
    }
    r
}

/// Un canal de respuesta: (lectura, extremo para el servicio, copia de escritura para el temporizador).
fn canal_respuesta() -> (u32, u32, u32) {
    let (lectura, suyo) = dashos::canal_crear();
    let copia = dashos::llave_duplicar(suyo, dashos::ESCRIBIR, 0) as u32;
    (lectura, suyo, copia)
}

fn peticion_archivos(orden: u32, nombre: &str) -> alloc::vec::Vec<u8> {
    peticion_con(orden, 0, 0, 0, nombre)
}

fn peticion_con(orden: u32, asa: u32, a: u64, b: u64, nombre: &str) -> alloc::vec::Vec<u8> {
    let mut p = alloc::vec![0u8; 32 + nombre.len()];
    p[0..4].copy_from_slice(&orden.to_le_bytes());
    p[4..8].copy_from_slice(&asa.to_le_bytes());
    p[8..16].copy_from_slice(&a.to_le_bytes());
    p[16..24].copy_from_slice(&b.to_le_bytes());
    p[24..28].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
    p[32..].copy_from_slice(nombre.as_bytes());
    p
}

/// Una orden al servicio de archivos: (estado, asa), o `None` si no contesta a tiempo.
fn pedir(lectura: u32, copia: u32, orden: u32, asa: u32, a: u64, b: u64, nombre: &str) -> Option<(i32, u32)> {
    let mut r = [0u8; 64];
    if !escribir(ARCHIVOS, &peticion_con(orden, asa, a, b, nombre), &[]) || leer_con_plazo(lectura, copia, &mut r) != Some(32) {
        return None;
    }
    Some((i32::from_le_bytes(r[0..4].try_into().unwrap()), u32::from_le_bytes(r[4..8].try_into().unwrap())))
}


/// Los permisos del servicio de archivos (R6). Devuelve lo que falló, o "bien".
fn permisos(lectura: u32, copia: u32) -> &'static str {
    let p = |orden, asa, a, b, nombre| pedir(lectura, copia, orden, asa, a, b, nombre);
    // Lo de fuera de su raíz no existe para él.
    if p(BUSCAR, 0, 0, 0, "LEEME.txt").map(|r| r.0) != Some(NO_EXISTE) {
        return "sale de su raíz";
    }
    if p(BUSCAR, 0, 0, 0, "Privado/nota.txt").map(|r| r.0) != Some(PROTEGIDO) || p(28, 0, 0, 64, "Privado/nota.txt").map(|r| r.0) != Some(PROTEGIDO) {
        return "entra en lo privado";
    }
    let Some((0, privado)) = p(BUSCAR, 0, 0, 0, "Privado") else { return "no ve el cajón privado" };
    let listar = p(LISTAR, privado, 0, 0, "").map(|r| r.0);
    p(CERRAR, privado, 0, 0, "");
    if listar != Some(PROTEGIDO) {
        return "lista lo privado";
    }
    let Some((0, grande)) = p(BUSCAR, 0, 0, 0, "grande.txt") else { return "no lee lo de todos" };
    let escribir = p(ESCRIBIR, grande, 0, 1, "").map(|r| r.0);
    let proteger = p(PROTEGER, grande, 0, 0o777, "").map(|r| r.0);
    p(CERRAR, grande, 0, 0, "");
    if escribir != Some(PROTEGIDO) {
        return "escribe lo ajeno";
    }
    if proteger != Some(PROTEGIDO) {
        return "cambia los permisos de lo ajeno";
    }
    if p(CREAR, 0, 1, 0, "mio.txt").map(|r| r.0) != Some(PROTEGIDO) {
        return "crea en un cajón ajeno";
    }
    if p(BORRAR, 0, 0, 0, "grande.txt").map(|r| r.0) != Some(PROTEGIDO) {
        return "borra lo ajeno";
    }
    if p(VOLUMENES, 0, 0, 0, "").map(|r| r.0) != Some(PROTEGIDO) || p(VOLUMEN, 0, 0, 0, "Ana").map(|r| r.0) != Some(PROTEGIDO) {
        return "administra volúmenes";
    }
    "bien"
}

/// Archivos: sesión, ataques y, después, buscar `grande.txt` (en su raíz, `C`) tiene que contestar bien.
fn archivos(memoria: u32) -> bool {
    let (lectura, suyo, copia) = canal_respuesta();
    let mut b = [0u8; 64];
    // Pide una ventana enorme (A4): se queda en 1 MB.
    if !escribir(ARCHIVOS, &peticion_con(0, 0, u64::MAX, 0, ""), &[suyo]) {
        println!("  [hostil] archivos: no se puede pedir la sesión");
        return false;
    }
    let r = leer_con_plazo(lectura, copia, &mut b);
    if r != Some(32) {
        println!("  [hostil] archivos: sin sesión ({:?})", r);
        return false;
    }
    let ventana = u64::from_le_bytes(b[8..16].try_into().unwrap());
    if ventana != 1 << 20 {
        println!("  [hostil] archivos: pidió una ventana enorme y le dieron {} bytes", ventana);
        return false;
    }
    escribir(ARCHIVOS, &[], &[]);
    // Una orden que no existe, con 8 llaves.
    inundar(ARCHIVOS, memoria, &peticion_archivos(999, ""));
    // Las respuestas a los ataques: se tiran.
    loop {
        let t = dashos::temporizador_crear(copia, 200_000, 0);
        let mut ll = [0u32; 8];
        let (n, k) = dashos::canal_leer(lectura, &mut b, &mut ll);
        ll[..k].iter().for_each(|&l| { dashos::cerrar(l); });
        dashos::cerrar(t as u32);
        if n == 8 || n < 0 {
            break;
        }
    }
    // Archivos suelta a un cliente que no lee sus respuestas (la cola llena): es lo previsto. Se abre sesión otra vez,
    // por otro canal, y se le pide algo normal.
    let (lectura, suyo, copia) = canal_respuesta();
    let r = if escribir(ARCHIVOS, &peticion_archivos(0, ""), &[suyo]) { leer_con_plazo(lectura, copia, &mut b) } else { None };
    if r != Some(32) {
        println!("  [hostil] archivos: sin sesión tras los ataques ({:?})", r);
        return false;
    }
    let r = if escribir(ARCHIVOS, &peticion_archivos(BUSCAR, "grande.txt"), &[]) { leer_con_plazo(lectura, copia, &mut b) } else { None };
    let vivo = r == Some(32) && i32::from_le_bytes(b[0..4].try_into().unwrap()) == 0;
    if !vivo {
        println!("  [hostil] archivos: buscar grande.txt da {:?}, estado {}", r, i32::from_le_bytes(b[0..4].try_into().unwrap()));
    }
    if vivo {
        let asa = u32::from_le_bytes(b[4..8].try_into().unwrap());
        pedir(lectura, copia, CERRAR, asa, 0, 0, "");
        println!("  [hostil] archivos, permisos: {}", permisos(lectura, copia));
    }
    vivo
}

/// Sonido: ataques y, después, abrir un flujo tiene que dar el aviso de "pide muestras".
fn sonido(memoria: u32) -> bool {
    escribir(SONIDO, &[], &[]);
    // VOLUMEN sin flujo abierto, con 8 llaves.
    let volumen: [u32; 3] = [11, 0, 0];
    let bytes: alloc::vec::Vec<u8> = volumen.iter().flat_map(|w| w.to_le_bytes()).collect();
    inundar(SONIDO, memoria, &bytes);
    let tam = 4096;
    let m = dashos::memoria_crear(64 + tam);
    if (m as i32) <= 0 || dashos::memoria_proyectar(m, VA_SONIDO, true) != 0 {
        println!("  [hostil] sonido: sin memoria para el anillo");
        return false;
    }
    let cab = VA_SONIDO as *mut u32;
    unsafe {
        cab.write_volatile(0);
        cab.add(1).write_volatile(0);
        cab.add(2).write_volatile(tam as u32);
    }
    let (lectura, suyo, copia) = canal_respuesta();
    let abrir: [u32; 4] = [10, 0, 1, 48_000];
    let bytes: alloc::vec::Vec<u8> = abrir.iter().flat_map(|w| w.to_le_bytes()).collect();
    let anillo = dashos::llave_duplicar(m, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR, 0) as u32;
    if !escribir(SONIDO, &bytes, &[anillo, suyo]) {
        println!("  [hostil] sonido: no se puede pedir el flujo");
        return false;
    }
    let mut b = [0u8; 16];
    let r = leer_con_plazo(lectura, copia, &mut b);
    let vivo = r == Some(4);
    if !vivo {
        println!("  [hostil] sonido: el flujo no pide muestras ({:?})", r);
    }
    escribir(SONIDO, &12u32.to_le_bytes(), &[]);
    vivo
}

/// Conecta con el compositor; devuelve (lectura, copia de escritura).
fn conectar() -> Option<(u32, u32)> {
    let (lectura, suyo, copia) = canal_respuesta();
    escribir(VENTANAS, &[TROZO_CONECTAR], &[suyo]).then_some((lectura, copia))
}

fn sync(id: u32) -> alloc::vec::Vec<u8> {
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::SYNC).uint(id).fin();
    let mut t = alloc::vec![TROZO_DATOS];
    t.extend_from_slice(&e.bytes);
    t
}

/// Manda un `sync` y espera su `done`.
fn compositor_contesta(lectura: u32, copia: u32, id: u32) -> bool {
    if !escribir(VENTANAS, &sync(id), &[]) {
        return false;
    }
    let mut b = [0u8; 256];
    let mut flujo = alloc::vec::Vec::new();
    while let Some(n) = leer_con_plazo(lectura, copia, &mut b) {
        if b[0] != TROZO_DATOS {
            continue;
        }
        flujo.extend_from_slice(&b[1..n]);
        while let Ok((objeto, orden, tam)) = leer_cabecera(&flujo) {
            if objeto == id && orden == wl_callback::ev::DONE {
                return true;
            }
            flujo.drain(..tam);
        }
    }
    false
}

/// Compositor: un mensaje vacío justo después de uno de datos (E1), y llaves que se acumulan en una conexión (E2).
fn compositor(memoria: u32) -> bool {
    let Some((lectura, copia)) = conectar() else { return false };
    if !compositor_contesta(lectura, copia, 50) {
        return false;
    }
    // E1: datos y, detrás, un mensaje vacío.
    escribir(VENTANAS, &sync(51), &[]);
    escribir(VENTANAS, &[], &[]);
    if !compositor_contesta(lectura, copia, 52) {
        return false;
    }
    // E2: la cabecera de un mensaje de 4.000 bytes y, después, de byte en byte, cada trozo con 8 llaves.
    let mut cabecera = alloc::vec![TROZO_DATOS];
    cabecera.extend_from_slice(&1u32.to_le_bytes());
    cabecera.extend_from_slice(&((4000u32 << 16) | 0).to_le_bytes());
    escribir(VENTANAS, &cabecera, &[]);
    inundar(VENTANAS, memoria, &[TROZO_DATOS, 0]);
    // Esta conexión ya no vale: se cierra y se abre otra.
    escribir(VENTANAS, &[TROZO_ADIOS], &[]);
    let Some((lectura, copia)) = conectar() else { return false };
    compositor_contesta(lectura, copia, 53)
}

fn si(b: bool) -> &'static str {
    if b { "sí" } else { "no" }
}

/// U2: por `sesiones`, la lista de procesos y matar. Al compositor (un servicio, usuario 1) no puede; a `presa`
/// (1001, el suyo), sí; a uno que no existe, tampoco. `caida-cliente`, que acabó solo, ya no está (su baja llegó con
/// el aviso de fin). Devuelve si todo fue como debe.
fn sesiones(memoria: u32) -> bool {
    // Antes, una inundación de peticiones que no existen con 8 llaves cada una (E2): tiene que seguir contestando.
    inundar(SESIONES, memoria, &[99, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let pedir = |orden: u32, objetivo: u64, buf: &mut [u8]| -> i64 {
        let (lectura, suyo) = dashos::canal_crear();
        let mut p = [0u8; 12];
        p[0..4].copy_from_slice(&orden.to_le_bytes());
        p[4..12].copy_from_slice(&objetivo.to_le_bytes());
        // Con reintentos: tras la inundación, la cola de `sesiones` puede estar llena un rato.
        let n = if escribir(SESIONES, &p, &[suyo]) {
            dashos::canal_leer_plazo(lectura, buf, &mut [], 2_000_000).0
        } else {
            -1
        };
        dashos::cerrar(lectura);
        n
    };
    // La lista, por páginas de hasta 7 fichas (un mensaje son 256 bytes).
    let mut lista = [[0u8; 32]; 64];
    let mut cuantos = 0;
    loop {
        let mut b = [0u8; 256];
        let n = pedir(dashos::protocolo::sesiones::PROCESOS, cuantos as u64, &mut b);
        if n < 8 {
            println!("  [hostil] sesiones: sin lista de procesos ({})", n);
            return false;
        }
        let van = u32::from_le_bytes(b[0..4].try_into().unwrap()) as usize;
        let total = u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
        for i in 0..van.min(7) {
            if cuantos < lista.len() {
                lista[cuantos].copy_from_slice(&b[8 + 32 * i..8 + 32 * i + 32]);
                cuantos += 1;
            }
        }
        if van == 0 || cuantos >= total.min(lista.len()) {
            break;
        }
    }
    let marca_de = |nombre: &[u8]| {
        lista[..cuantos].iter().find(|f| &f[16..16 + nombre.len()] == nombre && f.get(16 + nombre.len()) == Some(&0)).map(|f| u64::from_le_bytes(f[..8].try_into().unwrap()))
    };
    let (Some(ajeno), Some(presa)) = (marca_de(b"compositor"), marca_de(b"presa")) else {
        println!("  [hostil] sesiones: en la lista no están el compositor y presa ({} procesos)", cuantos);
        return false;
    };
    let sin_baja = marca_de(b"caida-cliente").is_some();
    let mut r = [0u8; 4];
    let matar = |m: u64, r: &mut [u8; 4]| if pedir(dashos::protocolo::sesiones::MATAR, m, r) == 4 { i32::from_le_bytes(*r) } else { i32::MIN };
    let (a, c, d) = (matar(ajeno, &mut r), matar(presa, &mut r), matar(u64::MAX, &mut r));
    use dashos::protocolo::sesiones as s;
    // U3: entrar como Carla, con su contraseña buena: solo `guardian` puede pedirlo.
    let mut p = [0u8; 40];
    let (usuario, clave) = (b"Carla", b"clave de Carla");
    p[0..4].copy_from_slice(&s::ENTRAR.to_le_bytes());
    p[4..8].copy_from_slice(&(usuario.len() as u32).to_le_bytes());
    p[8..12].copy_from_slice(&(clave.len() as u32).to_le_bytes());
    p[12..17].copy_from_slice(usuario);
    p[17..31].copy_from_slice(clave);
    let (lectura, suyo) = dashos::canal_crear();
    let mut r4 = [0u8; 4];
    let entrar = if escribir(SESIONES, &p[..31], &[suyo]) && dashos::canal_leer_plazo(lectura, &mut r4, &mut [], 2_000_000).0 == 4 {
        i32::from_le_bytes(r4)
    } else {
        i32::MIN
    };
    dashos::cerrar(lectura);
    if entrar != dashos::protocolo::archivos::PROTEGIDO {
        println!("  [hostil] sesiones: entrar como Carla da {} (tenía que dar -12)", entrar);
    }
    let bien = a == s::PROTEGIDO && c == s::BIEN && d == s::NO_EXISTE && !sin_baja && entrar == dashos::protocolo::archivos::PROTEGIDO;
    if bien {
        println!("  [hostil] sesiones: ve {} procesos; no mata al compositor (otro usuario), sí a presa (el suyo); los que acabaron, fuera", cuantos);
    } else {
        println!("  [hostil] sesiones: MAL (compositor {}, presa {}, inexistente {}, caida-cliente en la lista: {})", a, c, d, sin_baja);
    }
    bien
}

fn principal() -> u64 {
    // Se deja pasar a las demás pruebas del arranque: lo que sigue carga a los servicios.
    dormir_ms(2000);
    let memoria = dashos::memoria_crear(4096);
    let ses = sesiones(memoria);
    let a = archivos(memoria);
    println!("  [hostil] archivos sigue: {}", si(a));
    let s = sonido(memoria);
    println!("  [hostil] sonido sigue: {}", si(s));
    let c = compositor(memoria);
    println!("  [hostil] compositor sigue: {}", si(c));
    let _ = E_LIMITE;
    if a && s && c && ses { 0 } else { 1 }
}
