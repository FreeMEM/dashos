//! U4, de prueba: hace de `guardian` en lo de preguntar al usuario, solo en `virt`. Se apunta en `sesiones` para
//! recibir las preguntas ("¿dejas a este programa usar este servicio?") y contesta como lo haría el usuario de la
//! prueba: a `pide` le deja usar `eco`; a nadie más. Cuenta lo que le preguntan: a `pide`, una sola vez (lo concedido
//! se guarda y no se vuelve a preguntar); y al hostil, que pide `eco` sin permiso, al menos una.
//!
//! Llave 1: el canal "sesiones".

#![no_std]
#![no_main]

use dashos::println;
use dashos::protocolo::sesiones::{PREGUNTA, PREGUNTAS, RESPUESTA};

dashos::programa!(principal);

const SESIONES: u32 = 1;

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
    let apuntado = pedir(&PREGUNTAS.to_le_bytes(), &[para_sesiones]);
    if apuntado != 0 {
        println!("  [guardian] sesiones no le deja atender preguntas: {}", apuntado);
        return 1;
    }
    let (mut a_pide, mut a_otros) = (0, 0);
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
        let si = programa == b"pide" && servicio == b"eco";
        if si { a_pide += 1 } else { a_otros += 1 }
        let mut r = [0u8; 12];
        r[0..4].copy_from_slice(&RESPUESTA.to_le_bytes());
        r[4..8].copy_from_slice(&q[0..4]);
        r[8..12].copy_from_slice(&(si as u32).to_le_bytes());
        pedir(&r, &[]);
        println!("  [guardian] ¿{} puede usar {}? {}", core::str::from_utf8(programa).unwrap_or("?"),
            core::str::from_utf8(servicio).unwrap_or("?"), if si { "sí" } else { "no" });
    }
    let bien = a_pide == 1 && a_otros >= 1;
    println!("  [guardian] preguntas: de pide {}, de otros {}: {}", a_pide, a_otros, if bien { "bien" } else { "MAL" });
    if bien { 0 } else { 1 }
}
