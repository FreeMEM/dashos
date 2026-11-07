//! El programa de inicio (C47). Lo lanza el kernel con las llaves de arranque: la 1 es el paquete de arranque, que
//! dice qué es cada una de las demás y trae los ELF de los servicios (formato en `kernel/src/inicio.rs`). Lanza cada
//! servicio en su grupo, con copias de las llaves de sus aparatos y los extremos de los canales que comparte con otros
//! (C50), y se queda esperando: no acaba nunca. En cada canal, el primer servicio que lo nombra es el servidor; los
//! demás reciben una copia marcada del otro extremo, y el servidor un mensaje sin marca por cliente: 8 bytes de marca y
//! 16 de nombre (auditoría de la fase 4, M6).

#![no_std]
#![no_main]

use dashos::{canal_crear, grupo_crear, llave_duplicar, memoria_crear, memoria_proyectar, println, proceso_crear_en, proceso_esperar};

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

/// Espera un tic sin gastar CPU (un temporizador de una vez).
fn dormir_un_tic() {
    let (a, b) = canal_crear();
    let t = dashos::temporizador_crear(b, 10_000, 0);
    if t > 0 {
        let mut buf = [0u8; 8];
        let mut ll = [0u32; 1];
        dashos::canal_leer(a, &mut buf, &mut ll);
        dashos::cerrar(t as u32);
    }
    dashos::cerrar(a);
    dashos::cerrar(b);
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
            5 => "memoria del firmware",
            _ => "?",
        };
        println!("  [inicio]   llave {}: {} de {}", u32_en(p + 20), tipo, nombre(p));
        p += 24;
    }
    let programas = u32_en(p) as usize;
    p += 4;
    let cabeceras = p;
    const CABECERA: usize = 32;
    let mut datos = cabeceras + CABECERA * programas;
    // Los canales entre servicios: el extremo que reciben, en copias, los demás programas que lo nombran (0: aún no
    // creado). El primero recibe el otro extremo.
    let mut pendientes = [0u32; 32];
    for i in 0..programas {
        let cab = cabeceras + CABECERA * i;
        let tamano = u32_en(cab) as usize;
        let de_canales = u32_en(cab + 4);
        let nombre_cab = cab + 16;
        // Las llaves que le tocan, en orden; pasan a ser la 1, la 2... del servicio. De los aparatos, una copia: así
        // `inicio` conserva la suya y puede dar el mismo aparato a otro (el permiso DMA, por ejemplo).
        let mut suyas = [0u32; 8];
        let mut k = 0;
        for j in 0..8 {
            let a = unsafe { *((cab + 8 + j) as *const u8) } as usize;
            if a < n && k < suyas.len() {
                let copia = llave_duplicar(u32_en(aparatos + 24 * a + 20), 7, 0);
                if copia > 0 {
                    suyas[k] = copia as u32;
                    k += 1;
                }
            }
        }
        for c in 0..32 {
            if de_canales & (1 << c) != 0 && k < suyas.len() {
                if pendientes[c] == 0 {
                    let (a, b) = canal_crear();
                    pendientes[c] = b;
                    suyas[k] = a;
                    k += 1;
                } else {
                    // Cada cliente, con su marca (el número del programa más 1), y el servidor sabe de quién es por un
                    // mensaje sin marca que solo puede mandar `inicio` (auditoría de la fase 4, M6).
                    let marca = i as u64 + 1;
                    let copia = llave_duplicar(pendientes[c], 7, marca);
                    if copia > 0 {
                        suyas[k] = copia as u32;
                        k += 1;
                        let mut dir = [0u8; 24];
                        dir[..8].copy_from_slice(&marca.to_le_bytes());
                        dir[8..].copy_from_slice(unsafe { core::slice::from_raw_parts(nombre_cab as *const u8, 16) });
                        // Si la cola del servidor está llena, se reintenta un rato: sin su entrada, el cliente no
                        // podría pedir lo suyo (segunda vuelta de la auditoría, B16).
                        for _ in 0..100 {
                            if dashos::canal_escribir(pendientes[c], &dir, &[]) == 0 {
                                break;
                            }
                            dormir_un_tic();
                        }
                    }
                }
            }
        }
        datos = (datos + 7) & !7;
        let origen = datos;
        datos += tamano;
        let m = memoria_crear(tamano);
        let destino = COPIA + i * (8 << 20);
        if (m as i32) <= 0 || memoria_proyectar(m, destino, true) != 0 {
            println!("  [inicio] {}: sin memoria", nombre(nombre_cab));
            continue;
        }
        unsafe { core::ptr::copy_nonoverlapping(origen as *const u8, destino as *mut u8, tamano) };
        let g = grupo_crear(16 << 20);
        let proceso = if g > 0 { proceso_crear_en(g as u32, m, tamano, &suyas[..k]) } else { g };
        if proceso <= 0 {
            println!("  [inicio] {}: no se pudo lanzar ({})", nombre(nombre_cab), proceso);
            continue;
        }
        println!("  [inicio] {} lanzado en su grupo, con {} llaves", nombre(nombre_cab), k);
        // Los servicios de verdad (consola, USB) no acaban; los de prueba, sí: a esos se les espera.
        if k == 0 {
            let (clase, codigo) = proceso_esperar(proceso as u32);
            println!("  [inicio] {} acabó: clase {}, código {}", nombre(nombre_cab), clase, codigo);
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
