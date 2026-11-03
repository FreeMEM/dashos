//! El programa de inicio (C47). Lo lanza el kernel con las llaves de arranque: la 1 es el paquete de arranque, que
//! dice qué es cada una de las demás y trae los ELF de los servicios (formato en `kernel/src/inicio.rs`). Lanza cada
//! servicio en su grupo y se queda esperando: no acaba nunca.

#![no_std]
#![no_main]

use dashos::{grupo_crear, memoria_crear, memoria_proyectar, println, proceso_crear_en, proceso_esperar};

dashos::programa!(principal);

const PAQUETE: usize = 0x10_0000_0000;
const COPIA: usize = 0x20_0000_0000;

fn u32_en(p: usize) -> u32 {
    unsafe { core::ptr::read_volatile(p as *const u32) }
}

fn nombre(p: usize) -> &'static str {
    let b = unsafe { core::slice::from_raw_parts(p as *const u8, 16) };
    core::str::from_utf8(&b[..b.iter().position(|&c| c == 0).unwrap_or(16)]).unwrap_or("?")
}

fn principal() -> u64 {
    if memoria_proyectar(1, PAQUETE, false) != 0 {
        println!("  [inicio] sin paquete de arranque");
        return 1;
    }
    let mut p = PAQUETE;
    let n = u32_en(p) as usize;
    p += 4;
    let aparatos = p;
    println!("  [inicio] en marcha, con {} llaves de aparatos:", n);
    for _ in 0..n {
        let tipo = match u32_en(p + 16) {
            1 => "registros",
            2 => "interrupción",
            3 => "permiso DMA",
            4 => "registro",
            _ => "?",
        };
        println!("  [inicio]   llave {}: {} de {}", u32_en(p + 20), tipo, nombre(p));
        p += 24;
    }
    let programas = u32_en(p) as usize;
    p += 4;
    let cabeceras = p;
    let mut datos = cabeceras + 24 * programas;
    for i in 0..programas {
        let cab = cabeceras + 24 * i;
        let tamano = u32_en(cab) as usize;
        let mascara = u32_en(cab + 4);
        // Las llaves de los aparatos que le tocan, en orden; pasan a ser la 1, la 2... del servicio.
        let mut suyas = [0u32; 8];
        let mut k = 0;
        for a in 0..n.min(32) {
            if mascara & (1 << a) != 0 && k < suyas.len() {
                suyas[k] = u32_en(aparatos + 24 * a + 20);
                k += 1;
            }
        }
        datos = (datos + 7) & !7;
        let m = memoria_crear(tamano);
        let destino = COPIA + i * (8 << 20);
        if (m as i32) <= 0 || memoria_proyectar(m, destino, true) != 0 {
            println!("  [inicio] {}: sin memoria", nombre(cab + 8));
            continue;
        }
        unsafe { core::ptr::copy_nonoverlapping(datos as *const u8, destino as *mut u8, tamano) };
        datos += tamano;
        let g = grupo_crear(16 << 20);
        let proceso = if g > 0 { proceso_crear_en(g as u32, m, tamano, &suyas[..k]) } else { g };
        if proceso <= 0 {
            println!("  [inicio] {}: no se pudo lanzar ({})", nombre(cab + 8), proceso);
            continue;
        }
        println!("  [inicio] {} lanzado en su grupo, con {} llaves", nombre(cab + 8), k);
        // Los servicios de verdad (consola, USB) no acaban; los de prueba, sí: a esos se les espera.
        if k == 0 {
            let (clase, codigo) = proceso_esperar(proceso as u32);
            println!("  [inicio] {} acabó: clase {}, código {}", nombre(cab + 8), clase, codigo);
        }
    }
    println!("  [inicio] todo lanzado; se queda esperando");
    let (a, _b) = dashos::canal_crear();
    let mut buf = [0u8; 1];
    let mut ll = [0u32; 1];
    loop {
        dashos::canal_leer(a, &mut buf, &mut ll);
    }
}
