//! U3: hace de `guardian` en las pruebas de `virt`. Pide a `sesiones` entrar como "Carla" con una contraseña mala (no
//! entra), con la buena (entra: su volumen cifrado se abre) y salir (se cierra). El `guardian` de verdad dibujará la
//! pantalla de entrada y leerá la contraseña del teclado.

#![no_std]
#![no_main]

use dashos::println;
use dashos::protocolo::sesiones::{ENTRAR, SALIR, VOLVER};

dashos::programa!(principal);

/// Llave 1: el canal "sesiones".
const SESIONES: u32 = 1;

fn pedir(orden: u32, usuario: &str, clave: &str) -> i32 {
    let mut p = [0u8; 128];
    p[0..4].copy_from_slice(&orden.to_le_bytes());
    p[4..8].copy_from_slice(&(usuario.len() as u32).to_le_bytes());
    p[8..12].copy_from_slice(&(clave.len() as u32).to_le_bytes());
    p[12..12 + usuario.len()].copy_from_slice(usuario.as_bytes());
    p[12 + usuario.len()..12 + usuario.len() + clave.len()].copy_from_slice(clave.as_bytes());
    let (lectura, suyo) = dashos::canal_crear();
    let mut r = [0u8; 4];
    let n = if dashos::canal_escribir(SESIONES, &p[..12 + usuario.len() + clave.len()], &[suyo]) == 0 {
        dashos::canal_leer_plazo(lectura, &mut r, &mut [], 5_000_000).0
    } else {
        -1
    };
    dashos::cerrar(lectura);
    p.fill(0);
    if n == 4 { i32::from_le_bytes(r) } else { i32::MIN }
}

fn principal() -> u64 {
    // Se deja arrancar a `sesiones` y a archivos.
    dashos::dormir_us(1_000_000);
    let mala = pedir(ENTRAR, "Carla", "no es esta");
    let buena = pedir(ENTRAR, "Carla", "clave de Carla");
    // Volver de un bloqueo: la contraseña se comprueba sin tocar el volumen, que sigue abierto.
    let volver_mala = pedir(VOLVER, "Carla", "tampoco");
    let volver = pedir(VOLVER, "Carla", "clave de Carla");
    let salir = pedir(SALIR, "Carla", "");
    let bien = mala == -12 && buena == 0 && volver_mala == -12 && volver == 0 && salir == 0;
    println!(
        "  [entrada] Carla: contraseña mala {}, buena {}, volver con mala {} y con buena {}, salir {}: {}",
        mala,
        buena,
        volver_mala,
        volver,
        salir,
        if bien { "bien" } else { "MAL" }
    );
    if bien { 0 } else { 1 }
}
