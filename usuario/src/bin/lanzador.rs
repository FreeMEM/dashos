//! C43: un proceso que lanza pruebas. Su llave 1 es un objeto de memoria con un paquete de ELF:
//! - u32 cuántos;
//! - por cada uno, una cabecera de 24 bytes: u32 código esperado, u32 tamaño, 16 bytes de nombre (rellenos con 0);
//! - después, los ELF seguidos, cada uno desde un múltiplo de 8.
//!
//! Cada prueba corre en un grupo propio de 64 MB (C42), desde una copia en un objeto de memoria nuevo (C39). Sale
//! con 43 si todas salen con su código, o con 100 + el número de la que falló.

#![no_std]
#![no_main]

use dashos::{grupo_crear, memoria_crear, memoria_proyectar, println, proceso_crear_en, proceso_esperar};

dashos::programa!(principal);

const PAQUETE: usize = 0x10_0000_0000;
const COPIA: usize = 0x20_0000_0000;

fn u32_en(p: usize) -> u32 {
    unsafe { core::ptr::read_volatile(p as *const u32) }
}

fn principal() -> u64 {
    if memoria_proyectar(1, PAQUETE, false) != 0 {
        return 1;
    }
    let n = u32_en(PAQUETE) as usize;
    let mut datos = PAQUETE + 4 + 24 * n;
    for i in 0..n {
        let cab = PAQUETE + 4 + 24 * i;
        let (esperado, tamano) = (u32_en(cab) as u64, u32_en(cab + 4) as usize);
        let nombre = unsafe { core::slice::from_raw_parts((cab + 8) as *const u8, 16) };
        let nombre = core::str::from_utf8(&nombre[..nombre.iter().position(|&b| b == 0).unwrap_or(16)]).unwrap_or("?");
        datos = (datos + 7) & !7;
        // Una copia del ELF en un objeto propio, que es lo que pide proceso_crear.
        let m = memoria_crear(tamano);
        let destino = COPIA + i * (8 << 20);
        if (m as i32) <= 0 || memoria_proyectar(m, destino, true) != 0 {
            return 100 + i as u64;
        }
        unsafe { core::ptr::copy_nonoverlapping(datos as *const u8, destino as *mut u8, tamano) };
        datos += tamano;
        let g = grupo_crear(64 << 20);
        let p = if g > 0 { proceso_crear_en(g as u32, m, tamano, &[]) } else { g };
        if p <= 0 {
            return 100 + i as u64;
        }
        let (clase, codigo) = proceso_esperar(p as u32);
        println!("  [lanzador] {}: clase {}, código {}", nombre, clase, codigo);
        if clase != 0 || codigo != esperado {
            return 100 + i as u64;
        }
    }
    println!("  [lanzador] {} pruebas lanzadas desde un proceso, cada una en su grupo", n);
    43
}
