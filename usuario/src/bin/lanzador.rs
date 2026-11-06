//! C43: un proceso que lanza pruebas. Su llave 1 es un objeto de memoria con un paquete de ELF:
//! - u32 cuántos;
//! - por cada uno, una cabecera de 32 bytes: u32 código esperado (0xFFFFFFFF: no se lanza, solo es ayudante de otro),
//!   u32 tamaño, u32 ayudante (0 ninguno; si no, el número de la entrada más 1), u32 0 y 16 bytes de nombre;
//! - después, los ELF seguidos, cada uno desde un múltiplo de 8.
//!
//! Cada prueba corre en un grupo propio de 64 MB (C42), desde una copia en un objeto de memoria nuevo (C39). Si tiene
//! ayudante (C51), recibe como llave 1 un objeto de memoria con su ELF y como llave 2 un canal con un mensaje de 8
//! bytes con su tamaño: lo que esperan `padre` y `grupos` para lanzar a `hijo`. Al acabar, su grupo tiene que quedar
//! a cero: nada de lo que creó sigue vivo. Sale con 43 si todas salen con su código, o con 100 + el número de la que
//! falló.

#![no_std]
#![no_main]

use dashos::{canal_crear, canal_escribir, canal_leer, cerrar, grupo_crear, grupo_uso, memoria_crear, memoria_proyectar, println,
    proceso_crear_en, proceso_esperar, temporizador_crear};

dashos::programa!(principal);

const PAQUETE: usize = 0x10_0000_0000;
const COPIA: usize = 0x20_0000_0000;
const CABECERA: usize = 32;
const AYUDANTE: u32 = u32::MAX;
const MAX: usize = 32;

fn u32_en(p: usize) -> u32 {
    unsafe { core::ptr::read_volatile(p as *const u32) }
}

/// Un objeto de memoria nuevo con una copia de `tamano` bytes desde `origen`. La copia se ve en `destino`.
fn copia(origen: usize, tamano: usize, destino: usize) -> Option<u32> {
    let m = memoria_crear(tamano);
    if (m as i32) <= 0 || memoria_proyectar(m, destino, true) != 0 {
        return None;
    }
    unsafe { core::ptr::copy_nonoverlapping(origen as *const u8, destino as *mut u8, tamano) };
    Some(m)
}

/// Espera a que el grupo quede a cero, como mucho medio segundo: lo de un proceso muerto se suelta enseguida, pero
/// no siempre antes de que `proceso_esperar` conteste.
fn grupo_a_cero(g: u32) -> u64 {
    let (a, b) = canal_crear();
    let t = temporizador_crear(b, 10_000, 10_000);
    let mut usado = grupo_uso(g).1;
    for _ in 0..50 {
        if usado == 0 || t <= 0 {
            break;
        }
        let mut buf = [0u8; 8];
        let mut ll = [0u32; 1];
        canal_leer(a, &mut buf, &mut ll);
        usado = grupo_uso(g).1;
    }
    for k in [t as u32, a, b] {
        cerrar(k);
    }
    usado
}

fn principal() -> u64 {
    if memoria_proyectar(1, PAQUETE, false) != 0 {
        return 1;
    }
    let n = (u32_en(PAQUETE) as usize).min(MAX);
    // Dónde empieza cada ELF.
    let mut inicio = [0usize; MAX];
    let mut datos = PAQUETE + 4 + CABECERA * n;
    for (i, ini) in inicio.iter_mut().enumerate().take(n) {
        datos = (datos + 7) & !7;
        *ini = datos;
        datos += u32_en(PAQUETE + 4 + CABECERA * i + 4) as usize;
    }
    let mut lanzadas = 0;
    let mut zona = 0;
    for i in 0..n {
        let cab = PAQUETE + 4 + CABECERA * i;
        let (esperado, tamano, ayudante) = (u32_en(cab), u32_en(cab + 4) as usize, u32_en(cab + 8) as usize);
        if esperado == AYUDANTE {
            continue;
        }
        let nombre = unsafe { core::slice::from_raw_parts((cab + 16) as *const u8, 16) };
        let nombre = core::str::from_utf8(&nombre[..nombre.iter().position(|&b| b == 0).unwrap_or(16)]).unwrap_or("?");
        // Una copia del ELF en un objeto propio, que es lo que pide proceso_crear.
        let Some(m) = copia(inicio[i], tamano, COPIA + zona * (8 << 20)) else { return 100 + i as u64 };
        zona += 1;
        let mut suyas = [0u32; 2];
        let mut k = 0;
        let mut nuestro = 0;
        if ayudante > 0 && ayudante <= n {
            let j = ayudante - 1;
            let t = u32_en(PAQUETE + 4 + CABECERA * j + 4) as usize;
            let Some(h) = copia(inicio[j], t, COPIA + zona * (8 << 20)) else { return 100 + i as u64 };
            zona += 1;
            let (a, b) = canal_crear();
            canal_escribir(a, &(t as u64).to_le_bytes(), &[]);
            suyas = [h, b];
            k = 2;
            nuestro = a;
        }
        let g = grupo_crear(64 << 20);
        let p = if g > 0 { proceso_crear_en(g as u32, m, tamano, &suyas[..k]) } else { g };
        if p <= 0 {
            return 100 + i as u64;
        }
        let (clase, codigo) = proceso_esperar(p as u32);
        if nuestro != 0 {
            cerrar(nuestro);
        }
        let usado = grupo_a_cero(g as u32);
        println!("  [lanzador] {}: clase {}, código {}", nombre, clase, codigo);
        if clase != 0 || codigo != esperado as u64 {
            return 100 + i as u64;
        }
        if usado != 0 {
            println!("  [lanzador] {}: su grupo no queda a cero ({} bytes)", nombre, usado);
            return 100 + i as u64;
        }
        cerrar(p as u32);
        cerrar(g as u32);
        lanzadas += 1;
    }
    println!("  [lanzador] {} pruebas lanzadas desde un proceso, cada una en su grupo, que queda a cero", lanzadas);
    43
}
