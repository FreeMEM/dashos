//! A15, el primer paso del comprobador aparte (plan en `docs/REVISION_LOOP.md`): un programa con la partición de DashOS
//! solo para leer. De momento abre el disco, lee la cabecera del sistema de archivos y comprueba que escribir y
//! descartar se le niegan (-6) y que la cabecera sigue igual. Más adelante recorrerá una instantánea con la biblioteca
//! de RedoxFS, como `ver`, sin parar al servicio de archivos.
//!
//! Llaves: 1 el canal "disco".

#![no_std]
#![no_main]

use dashos::println;

dashos::programa!(principal);

const VA: usize = 0x43_0000_0000;
const ABRIR: u32 = 0;
const LEER: u32 = 1;
const ESCRIBIR: u32 = 2;
const DESCARTAR: u32 = 6;
const SIN_PERMISO: i32 = -6;

/// Una petición al servicio de disco: (estado, sectores de su tramo, llave de la ventana si la trae).
fn pedir(lectura: u32, vuelta: u32, orden: u32, cuantos: u32, primero: u64, off: u32) -> Option<(i32, u64, Option<u32>)> {
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
    Some((u32_en(0) as i32, u32_en(8) as u64 | (u32_en(12) as u64) << 32, (k > 0).then_some(ll[0])))
}

fn firma() -> [u8; 8] {
    core::array::from_fn(|i| unsafe { core::ptr::read_volatile((VA + i) as *const u8) })
}

fn principal() -> u64 {
    let (lectura, vuelta) = dashos::canal_crear();
    let Some((0, sectores, Some(v))) = pedir(lectura, vuelta, ABRIR, 0, 0, 0) else {
        println!("  [comprobador] no se abre el disco");
        return 1;
    };
    if dashos::memoria_proyectar(v, VA, true) != 0 {
        println!("  [comprobador] no se proyecta la ventana");
        return 2;
    }
    let estado = |orden, cuantos, primero| pedir(lectura, vuelta, orden, cuantos, primero, 0).map_or(i32::MIN, |r| r.0);
    // La cabecera: el primer bloque de 4 KB de su tramo, que es la partición de DashOS.
    let leida = estado(LEER, 8, 0) == 0;
    let antes = firma();
    let es_dashfs = &antes == b"DashFS\0\0" || &antes == b"RedoxFS\0";
    // Escribir encima (la ventana tiene la cabecera: si pasara, no la estropearía) y descartar: los dos, negados.
    let escribir = estado(ESCRIBIR, 8, 0);
    let descartar = estado(DESCARTAR, 8, 0);
    let fuera = estado(LEER, 1, sectores);
    let despues = estado(LEER, 8, 0) == 0 && firma() == antes;
    let bien = leida && es_dashfs && escribir == SIN_PERMISO && descartar == SIN_PERMISO && fuera == -2 && despues;
    let texto = core::str::from_utf8(&antes[..6]).unwrap_or("?");
    println!("  [comprobador] la partición de DashOS, solo para leer: {} sectores, firma {}; escribir {}, descartar {}, fuera {}: {}",
        sectores, texto, escribir, descartar, fuera, if bien { "bien" } else { "MAL" });
    if bien { 0 } else { 3 }
}
