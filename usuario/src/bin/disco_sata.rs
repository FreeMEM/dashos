//! Prueba del servicio de discos SATA (X5c), como `disco` para el NVMe: un cliente del canal de `ahci` (la llave 1).
//! Abre cada disco por su número (0, 1...) hasta que el servicio dice que no hay más (-2). En cada uno lee los dos
//! primeros sectores, escribe el sector 2 y lo relee, lee 4 KB que cruzan una página de la ventana, comprueba los
//! rechazos (fuera del disco y de la ventana), vacía la caché, descarta 4 KB escritos (se leen a cero si el disco sabe
//! descartar) y manda por su anillo 16 lecturas de golpe, una escritura y su relectura, y una que se rechaza.
//!
//! Solo escribe en discos de prueba: los que empiezan por "DashOS: sector 0 del disco SATA" (`tools/disco_sata.py`).
//! En uno de verdad solo lee. Lo escrito se mira después en Linux (`tools/disco_sata.py comprobar`).

#![no_std]
#![no_main]

use core::sync::atomic::{fence, AtomicU32, Ordering::*};
use dashos::println;

dashos::programa!(principal);

const VA: usize = 0x40_0000_0000;
const VA_ANILLO: usize = 0x41_0000_0000;
const ABRIR: u32 = 0;
const LEER: u32 = 1;
const ESCRIBIR: u32 = 2;
const ANILLO: u32 = 3;
const MIRAR: u32 = 4;
const VACIAR: u32 = 5;
const DESCARTAR: u32 = 6;
const MARCA: &[u8] = b"DashOS: sector 0 del disco SATA";

/// Una petición, con una copia de `vuelta` para la respuesta, que llega por `lectura`: (estado, sectores, tamaño del
/// sector, llave que venga).
fn pedir(lectura: u32, vuelta: u32, orden: u32, cuantos: u32, primero: u64, off: u32) -> Option<(i32, u64, u32, Option<u32>)> {
    let mut p = [0u8; 24];
    p[0..4].copy_from_slice(&orden.to_le_bytes());
    p[4..8].copy_from_slice(&cuantos.to_le_bytes());
    p[8..16].copy_from_slice(&primero.to_le_bytes());
    p[16..20].copy_from_slice(&off.to_le_bytes());
    let copia = dashos::llave_duplicar(vuelta, dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
    if copia <= 0 || dashos::canal_escribir(1, &p, &[copia as u32]) != 0 {
        return None;
    }
    let mut r = [0u8; 24];
    let mut ll = [0u32; 1];
    let (n, k) = dashos::canal_leer(lectura, &mut r, &mut ll);
    if n != 24 {
        return None;
    }
    let u32_en = |i: usize| u32::from_le_bytes(r[i..i + 4].try_into().unwrap());
    Some((u32_en(0) as i32, u32_en(8) as u64 | (u32_en(12) as u64) << 32, u32_en(16), (k > 0).then_some(ll[0])))
}

fn byte(off: usize) -> u8 {
    unsafe { core::ptr::read_volatile((VA + off) as *const u8) }
}

fn poner(off: usize, b: &[u8]) {
    for (i, &c) in b.iter().enumerate() {
        unsafe { core::ptr::write_volatile((VA + off + i) as *mut u8, c) };
    }
}

fn empieza(off: usize, t: &[u8]) -> bool {
    t.iter().enumerate().all(|(i, &c)| byte(off + i) == c)
}

fn palabra(off: usize) -> &'static AtomicU32 {
    unsafe { &*((VA_ANILLO + off) as *const AtomicU32) }
}

fn si(b: bool) -> &'static str {
    if b { "bien" } else { "MAL" }
}

/// Un envío en la posición `i` del anillo.
fn envio(i: u32, orden: u32, primero: u64, off: usize, etiqueta: u32) {
    let e = 64 + 32 * (i % 32) as usize;
    palabra(e).store(orden, Relaxed);
    palabra(e + 4).store(1, Relaxed);
    palabra(e + 8).store(primero as u32, Relaxed);
    palabra(e + 12).store((primero >> 32) as u32, Relaxed);
    palabra(e + 16).store(off as u32, Relaxed);
    palabra(e + 20).store(etiqueta, Relaxed);
}

/// Pone en el anillo 16 lecturas de un sector (los 16 primeros, desde 16 KB de la ventana), una escritura del sector
/// 5 desde 32 KB y su lectura a 33 KB (tienen que ir en orden), y una lectura fuera del disco. Espera sus terminadas.
fn probar_anillo(lectura: u32, sector: usize, escribe: bool) -> bool {
    let base = palabra(4).load(Relaxed);
    let mut n = 0;
    for i in 0..16u32 {
        envio(base + n, LEER, i as u64, 16384 + sector * i as usize, 100 + i);
        n += 1;
    }
    if escribe {
        let mut s = [0u8; 64];
        let t = b"DashOS: sector 5, por el anillo";
        s[..t.len()].copy_from_slice(t);
        poner(32768, &s);
        envio(base + n, ESCRIBIR, 5, 32768, 200);
        envio(base + n + 1, LEER, 5, 33792, 201);
        n += 2;
    }
    envio(base + n, LEER, u64::MAX - 1, 0, 999);
    n += 1;
    palabra(4).store(base + n, Release);
    fence(SeqCst);
    if palabra(16).load(SeqCst) != 0 {
        let mut p = [0u8; 24];
        p[..4].copy_from_slice(&MIRAR.to_le_bytes());
        dashos::canal_escribir(1, &p, &[]);
    }
    let mut buf = [0u8; 8];
    let mut ll = [0u32; 1];
    let cq = palabra(8).load(Relaxed);
    let fin = dashos::ahora_us() + 20_000_000;
    loop {
        palabra(20).store(1, SeqCst);
        fence(SeqCst);
        if palabra(12).load(Acquire).wrapping_sub(cq) >= n || dashos::ahora_us() > fin {
            break;
        }
        dashos::canal_leer(lectura, &mut buf, &mut ll);
    }
    let mut bien = palabra(12).load(Acquire).wrapping_sub(cq) == n;
    for i in 0..n {
        let c = 2048 + 16 * (cq.wrapping_add(i) % 32) as usize;
        let (etiqueta, estado) = (palabra(c).load(Relaxed), palabra(c + 4).load(Relaxed) as i32);
        let esperada = if i < 16 { 100 + i } else if escribe && i < 18 { 200 + i - 16 } else { 999 };
        bien &= etiqueta == esperada && if etiqueta == 999 { estado == -2 } else { estado == 0 };
    }
    palabra(8).store(cq.wrapping_add(n), Release);
    bien &= empieza(16384, b"DashOS: sector 0") && empieza(16384 + sector, b"DashOS: sector 1");
    if escribe {
        bien &= empieza(33792, b"DashOS: sector 5, por el anillo");
    }
    bien
}

fn principal() -> u64 {
    let (lectura, vuelta) = dashos::canal_crear();
    let mut d = 0u64;
    let mut con_anillo = false;
    loop {
        let Some((estado, sectores, tam, v)) = pedir(lectura, vuelta, ABRIR, 0, d, 0) else {
            println!("  [disco-sata] sin servicio de discos");
            return 1;
        };
        if estado == -2 {
            println!("  [disco-sata] {} discos; el {} no existe: bien", d, d);
            return 0;
        }
        let Some(v) = v.filter(|_| estado == 0) else {
            println!("  [disco-sata] disco {}: no se abre (estado {})", d, estado);
            return 2;
        };
        // La ventana es la misma para todos los discos: se proyecta la primera vez.
        if d == 0 && dashos::memoria_proyectar(v, VA, true) != 0 {
            println!("  [disco-sata] no se proyecta la ventana");
            return 3;
        }
        if d > 0 {
            dashos::cerrar(v);
        }
        let t = tam as usize;
        let estado = |orden, cuantos, primero, off| pedir(lectura, vuelta, orden, cuantos, primero, off).map_or(i32::MIN, |r| r.0);
        let leidos = estado(LEER, 2, 0, 0) == 0 && empieza(t, b"DashOS: sector 1");
        let de_prueba = empieza(0, MARCA);
        println!("  [disco-sata] disco {}: {} sectores de {} bytes; leer: {}; de prueba: {}", d, sectores, tam, si(leidos), if de_prueba { "sí" } else { "no, solo se lee" });
        if !de_prueba {
            d += 1;
            continue;
        }
        let mut texto = *b"DashOS: sector 2, escrito en el disco SATA 0";
        let l = texto.len();
        texto[l - 1] = b'0' + d as u8;
        poner(4096, &[0; 512]);
        poner(4096, &texto);
        poner(8192, &[0xEE; 512]);
        let escrito = estado(ESCRIBIR, 1, 2, 4096) == 0 && estado(LEER, 1, 2, 8192) == 0 && (0..t).all(|i| byte(8192 + i) == byte(4096 + i));
        let cruzando = estado(LEER, (4096 / t) as u32, 0, 2048) == 0 && empieza(2048, MARCA) && empieza(2048 + 2 * t, &texto);
        let rechazos = estado(LEER, 1, sectores, 0) == -2 && estado(LEER, 2, 0, (128 * 1024 - t) as u32) == -3 && estado(LEER, 1, 0, 2) == -3
            && estado(LEER, 1, u64::MAX, 0) == -2;
        let vaciado = estado(VACIAR, 0, 0, 0) == 0;
        println!("  [disco-sata] disco {}: escribir y releer: {}; cruzando páginas: {}; rechazos: {}; vaciar: {}", d, si(escrito), si(cruzando), si(rechazos), si(vaciado));
        // Descartar 4 KB escritos: con TRIM (QEMU, discard=unmap) se leen a cero; sin él, contesta bien sin hacer nada.
        let ocho = (4096 / t) as u32;
        poner(4096, &[0x5A; 4096]);
        let descarta = estado(ESCRIBIR, ocho, 8, 4096) == 0 && estado(DESCARTAR, ocho, 8, 0) == 0 && estado(LEER, ocho, 8, 8192) == 0;
        let ceros = (0..4096).all(|i| byte(8192 + i) == 0);
        println!("  [disco-sata] disco {}: descartar: {}; se lee a cero: {}", d, si(descarta), if ceros { "sí" } else { "no" });
        if !con_anillo {
            let Some((0, _, _, Some(m))) = pedir(lectura, vuelta, ANILLO, 0, 0, 0) else {
                println!("  [disco-sata] sin anillo");
                return 4;
            };
            if dashos::memoria_proyectar(m, VA_ANILLO, true) != 0 {
                println!("  [disco-sata] no se proyecta el anillo");
                return 5;
            }
            con_anillo = true;
        }
        println!("  [disco-sata] disco {}: anillo: 16 lecturas, escribir y releer en orden y un rechazo: {}", d, si(probar_anillo(lectura, t, true)));
        d += 1;
    }
}
