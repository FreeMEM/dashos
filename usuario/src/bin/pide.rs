//! U4: un programa con permiso para `eco`. Pide su llave a `sesiones`, la usa, renuncia y comprueba que la llave ya
//! no sirve (revocada). Llave 1: el canal "sesiones".

#![no_std]
#![no_main]

use dashos::println;
use dashos::protocolo::sesiones::{DEJAR, SERVICIO};

dashos::programa!(principal);

const SESIONES: u32 = 1;

/// Una petición a `sesiones` con el nombre de un servicio: (estado, llave que llegue).
fn pedir(orden: u32, servicio: &str) -> (i32, u32) {
    let mut p = [0u8; 64];
    p[0..4].copy_from_slice(&orden.to_le_bytes());
    p[4..8].copy_from_slice(&(servicio.len() as u32).to_le_bytes());
    p[12..12 + servicio.len()].copy_from_slice(servicio.as_bytes());
    let (lectura, suyo) = dashos::canal_crear();
    let mut r = [0u8; 4];
    let mut ll = [0u32; 1];
    let (n, k, _) = if dashos::canal_escribir(SESIONES, &p[..12 + servicio.len()], &[suyo]) == 0 {
        let (n, k, m) = dashos::canal_leer_plazo(lectura, &mut r, &mut ll, 5_000_000);
        (n, k, m)
    } else {
        (-1, 0, 0)
    };
    dashos::cerrar(lectura);
    (if n == 4 { i32::from_le_bytes(r) } else { i32::MIN }, if k == 1 { ll[0] } else { 0 })
}

/// Manda "hola" por `eco` y devuelve si contestó "eco: hola", o el error de escribir.
fn usar(eco: u32) -> Result<bool, i64> {
    let (lectura, suyo) = dashos::canal_crear();
    let e = dashos::canal_escribir(eco, b"hola", &[suyo]);
    if e != 0 {
        dashos::cerrar(lectura);
        dashos::cerrar(suyo);
        return Err(e);
    }
    let mut r = [0u8; 32];
    let (n, _, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut [], 2_000_000);
    dashos::cerrar(lectura);
    Ok(n > 0 && &r[..n as usize] == b"eco: hola")
}

fn principal() -> u64 {
    dashos::dormir_us(1_500_000);
    let (estado, eco) = pedir(SERVICIO, "eco");
    let contesta = eco != 0 && usar(eco) == Ok(true);
    // Sin el derecho de transferirla (S4): no puede regalarla.
    let (a, b) = dashos::canal_crear();
    let regalar = dashos::canal_escribir(a, b"toma", &[eco]);
    dashos::cerrar(a);
    dashos::cerrar(b);
    let (dejar, _) = pedir(DEJAR, "eco");
    let tras_dejar = usar(eco);
    let bien = estado == 0 && contesta && regalar == -3 && dejar == 0 && tras_dejar.is_err();
    println!(
        "  [pide] eco: con permiso {} y contesta {}; regalarla da {}; tras dejarla, usarla da {:?}: {}",
        estado,
        contesta,
        regalar,
        tras_dejar,
        if bien { "bien" } else { "MAL" }
    );
    if bien {
        println!("  [pide] U4: bien");
    }
    if bien { 0 } else { 1 }
}
