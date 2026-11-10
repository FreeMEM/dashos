//! U3: hace de `guardian` en las pruebas de `virt`. Pide a `sesiones` entrar como "Carla" con una contraseña mala (no
//! entra), con la buena (entra: su volumen cifrado se abre) y salir (se cierra). El `guardian` de verdad dibujará la
//! pantalla de entrada y leerá la contraseña del teclado.

#![no_std]
#![no_main]

use dashos::println;
use dashos::protocolo::sesiones::{ENTRADA_REGISTRO, ENTRAR, LANZAR, POR_PAGINA_REGISTRO, REGISTRO, R_ENTRAR, R_MATAR, R_SERVICIO, SALIR, VOLVER};

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
    // U2: lanzar un programa del disco, y uno que no es un programa.
    let lanzar = |camino: &str| -> (i32, u64) {
        let mut p = [0u8; 64];
        p[0..4].copy_from_slice(&LANZAR.to_le_bytes());
        p[4..8].copy_from_slice(&(camino.len() as u32).to_le_bytes());
        p[12..12 + camino.len()].copy_from_slice(camino.as_bytes());
        let (lectura, suyo) = dashos::canal_crear();
        let mut r = [0u8; 12];
        let n = if dashos::canal_escribir(SESIONES, &p[..12 + camino.len()], &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], 5_000_000).0 } else { -1 };
        dashos::cerrar(lectura);
        if n == 12 { (i32::from_le_bytes(r[0..4].try_into().unwrap()), u64::from_le_bytes(r[4..12].try_into().unwrap())) } else { (i32::MIN, 0) }
    };
    let (hola, pid) = lanzar("Programas/hola");
    let (texto, _) = lanzar("LEEME.txt");
    let lanza = hola == 0 && pid >= 0x1_0000 && texto < 0;
    println!("  [entrada] lanzar del disco: Programas/hola {} (proceso {}), un texto {}: {}", hola, pid, texto, if lanza { "bien" } else { "MAL" });
    // U6: el registro de seguridad tiene que acabar teniendo los tres intentos del hostil (matar al compositor, la llave
    // de `eco` y entrar como Carla). Se mira cada medio segundo, hasta 20 s.
    let mut visto = (0, [false; 3]);
    for _ in 0..40 {
        visto = registro_del_hostil();
        if visto.1.iter().all(|&x| x) {
            break;
        }
        dashos::dormir_us(500_000);
    }
    let todo = visto.1.iter().all(|&x| x);
    println!("  [entrada] registro de seguridad: {} entradas; las del hostil (matar, servicio, entrar): {}", visto.0, if todo { "bien" } else { "FALTAN" });
    if bien && todo && lanza { 0 } else { 1 }
}

/// Lee el registro entero: cuántas entradas hay y si están las tres del hostil.
fn registro_del_hostil() -> (usize, [bool; 3]) {
    let mut vistos = [false; 3];
    let mut desde = 0;
    let mut total = 0;
    loop {
        let mut p = [0u8; 12];
        p[0..4].copy_from_slice(&REGISTRO.to_le_bytes());
        p[4..12].copy_from_slice(&(desde as u64).to_le_bytes());
        let (lectura, suyo) = dashos::canal_crear();
        let mut r = [0u8; 8 + ENTRADA_REGISTRO * POR_PAGINA_REGISTRO];
        let n = if dashos::canal_escribir(SESIONES, &p, &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], 2_000_000).0 } else { -1 };
        dashos::cerrar(lectura);
        if n < 8 {
            return (total, vistos);
        }
        let van = u32::from_le_bytes(r[0..4].try_into().unwrap()) as usize;
        total = u32::from_le_bytes(r[4..8].try_into().unwrap()) as usize;
        for i in 0..van {
            let e = &r[8 + ENTRADA_REGISTRO * i..8 + ENTRADA_REGISTRO * (i + 1)];
            let tipo = u32::from_le_bytes(e[0..4].try_into().unwrap());
            if &e[8..15] == b"hostil\0" {
                match tipo {
                    R_MATAR => vistos[0] = true,
                    R_SERVICIO => vistos[1] = true,
                    R_ENTRAR => vistos[2] = true,
                    _ => {}
                }
            }
        }
        desde += van;
        if van == 0 || desde >= total {
            return (total, vistos);
        }
    }
}
