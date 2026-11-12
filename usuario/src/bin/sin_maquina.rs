//! Un programa sin el permiso de máquinas virtuales (caja Linux, H9), solo en `virt`. Desde que el permiso lo reparte
//! `sesiones` (decisión del usuario del 2026-11-12), se lo pide: no es la caja del sistema, así que se le pregunta a
//! quien está delante, que no dice que sí (en la prueba, el guion dice que no; sin nadie que conteste, la pregunta
//! caduca). Después prueba a crear una máquina con todas las llaves que podría tener, y con una suya de memoria como
//! permiso: ninguna sirve. Las llaves no se adivinan.
//!
//! Llaves: 1 "sistema" (la RAM y si hay hipervisor), 2 el canal "sesiones".

#![no_std]
#![no_main]

use dashos::println;
use dashos::protocolo::sesiones::SERVICIO;

dashos::programa!(principal);

const SISTEMA: u32 = 1;
const SESIONES: u32 = 2;
const VA_SISTEMA: usize = 0x20_0000_0000;
/// Lo que espera la respuesta de `sesiones`: si pregunta al usuario, la pregunta dura hasta 30 s.
const ESPERA_US: u64 = 45_000_000;

/// Pide a `sesiones` el permiso de máquinas virtuales: el estado y la llave, si llega una.
fn pedir_permiso() -> (i64, Option<u32>) {
    let (lectura, suyo) = dashos::canal_crear();
    let nombre = b"maquinas";
    let mut m = [0u8; 12 + 8];
    m[0..4].copy_from_slice(&SERVICIO.to_le_bytes());
    m[4..8].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
    m[12..20].copy_from_slice(nombre);
    // Al arrancar, la cola de `sesiones` puede estar llena: se reintenta un rato.
    let mut e = -1;
    for _ in 0..100 {
        e = dashos::canal_escribir(SESIONES, &m, &[suyo]);
        if e == 0 {
            break;
        }
        dashos::dormir_us(100_000);
    }
    if e != 0 {
        dashos::cerrar(lectura);
        dashos::cerrar(suyo);
        return (e, None);
    }
    let mut r = [0u8; 4];
    let mut ll = [0u32; 1];
    let (n, k, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut ll, ESPERA_US);
    dashos::cerrar(lectura);
    if n == dashos::E_PLAZO {
        return (n, None);
    }
    let estado = if n == 4 { i32::from_le_bytes(r) as i64 } else { n };
    (estado, (k == 1).then_some(ll[0]))
}

fn principal() -> u64 {
    // SAFETY: recién proyectado aquí, solo de lectura; el objeto mide una página.
    let hiper = dashos::memoria_proyectar(SISTEMA, VA_SISTEMA, false) == 0
        && dashos::maquina::sistema(unsafe { core::slice::from_raw_parts(VA_SISTEMA as *const u8, 16) }).is_some_and(|s| s.1);
    // Sin hipervisor no hay nada que pedir (y no se molesta a quien está delante con una pregunta).
    let (pedido, llave) = if hiper { pedir_permiso() } else { (0, None) };
    let mut con_la_llave = 0;
    if let Some(l) = llave {
        // No debería llegar: si llega, se cuenta.
        let mem = dashos::memoria_crear(64 * 1024);
        let r = dashos::maquina_crear(l, mem, 0x4000_0000, 0x4000_0000, 0);
        if r > 0 {
            con_la_llave = 1;
            dashos::cerrar(r as u32);
        }
        dashos::cerrar(mem);
        dashos::cerrar(l);
    }
    let mem = dashos::memoria_crear(64 * 1024);
    let mut crean = 0;
    for k in 1..=1024u32 {
        if dashos::maquina_crear(k, mem, 0x4000_0000, 0x4000_0000, 0) > 0 {
            crean += 1;
        }
    }
    // Con su propia memoria por permiso: el tipo no vale.
    let tipo = dashos::maquina_crear(mem, mem, 0x4000_0000, 0x4000_0000, 0);
    let negado = !hiper || (llave.is_none() && pedido != 0);
    let bien = crean == 0 && tipo == -6 && negado && con_la_llave == 0;
    if hiper {
        println!(
            "  [sin-maquina] pedir el permiso a sesiones: {} ({})",
            pedido,
            match (llave, pedido) {
                (Some(_), _) => "se lo dan: MAL",
                (None, dashos::E_PLAZO) => "sin respuesta: nadie dijo que sí",
                (None, -2) => "negado",
                _ => "no",
            }
        );
    }
    println!("  [sin-maquina] sin el permiso: 1024 llaves probadas, crean una máquina {}; con su memoria por permiso, {}: {}", crean, tipo, if bien { "bien" } else { "MAL" });
    if bien { 0 } else { 1 }
}
