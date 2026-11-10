//! Un cliente hostil de los servicios (R1, B6 de `docs/REVISION_ARQUITECTURA.md`). Ataca al servicio de archivos, al
//! de sonido y al compositor con lo que cualquier programa puede mandar, y después comprueba que cada uno sigue
//! contestando a una petición normal, con un plazo. Llaves: 1 "ventanas", 2 "sonido", 3 "archivos", marcadas por
//! `inicio`.
//!
//! Ataques:
//! - un mensaje vacío (E1: el compositor leía el primer byte del mensaje anterior);
//! - cientos de mensajes con 8 llaves que el servicio no usa (E2: si no las cierra, se le llena la tabla de llaves y
//!   gira para siempre sin poder leer).
//!
//! Saca una línea por servicio: "sigue: sí" o "sigue: no".

#![no_std]
#![no_main]

extern crate alloc;

use dashos::println;
use wayland::canal::{TROZO_ADIOS, TROZO_CONECTAR, TROZO_DATOS};
use wayland::tablas::{wl_callback, wl_display};
use wayland::{Escritor, leer_cabecera};

dashos::programa!(principal);
dashos::monton!();

const VENTANAS: u32 = 1;
const SONIDO: u32 = 2;
const ARCHIVOS: u32 = 3;
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
    let mut p = alloc::vec![0u8; 32 + nombre.len()];
    p[0..4].copy_from_slice(&orden.to_le_bytes());
    p[24..28].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
    p[32..].copy_from_slice(nombre.as_bytes());
    p
}

/// Archivos: sesión, ataques y, después, buscar `LEEME.txt` tiene que contestar bien.
fn archivos(memoria: u32) -> bool {
    let (lectura, suyo, copia) = canal_respuesta();
    let mut b = [0u8; 64];
    if !escribir(ARCHIVOS, &peticion_archivos(0, ""), &[suyo]) || leer_con_plazo(lectura, copia, &mut b) != Some(32) {
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
    escribir(ARCHIVOS, &peticion_archivos(1, "LEEME.txt"), &[])
        && leer_con_plazo(lectura, copia, &mut b) == Some(32)
        && i32::from_le_bytes(b[0..4].try_into().unwrap()) == 0
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
        return false;
    }
    let mut b = [0u8; 16];
    let vivo = leer_con_plazo(lectura, copia, &mut b) == Some(4);
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

fn principal() -> u64 {
    // Se deja pasar a las demás pruebas del arranque: lo que sigue carga a los servicios.
    dormir_ms(2000);
    let memoria = dashos::memoria_crear(4096);
    let a = archivos(memoria);
    println!("  [hostil] archivos sigue: {}", si(a));
    let s = sonido(memoria);
    println!("  [hostil] sonido sigue: {}", si(s));
    let c = compositor(memoria);
    println!("  [hostil] compositor sigue: {}", si(c));
    let _ = E_LIMITE;
    if a && s && c { 0 } else { 1 }
}
