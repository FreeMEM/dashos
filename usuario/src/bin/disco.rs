//! Prueba del servicio de disco (U6f): un cliente del canal "disco" (la llave 1, con su marca de `inicio`). Abre el
//! disco, lee los dos primeros sectores en su ventana, escribe el sector 2 y lo vuelve a leer, lee 4 KB que cruzan
//! una página de la ventana, y comprueba que se rechazan un sector fuera del disco y un trozo fuera de la ventana.

#![no_std]
#![no_main]

use dashos::println;

dashos::programa!(principal);

const VA: usize = 0x40_0000_0000;

const ABRIR: u32 = 0;
const LEER: u32 = 1;
const ESCRIBIR: u32 = 2;

struct Respuesta {
    estado: i32,
    sectores: u64,
    tam_sector: u32,
    ventana: Option<u32>,
}

/// Una petición, con una copia del extremo de escritura de `vuelta` para la respuesta, que llega por `lectura`.
fn pedir(lectura: u32, vuelta: u32, orden: u32, cuantos: u32, primero: u64, off: u32) -> Option<Respuesta> {
    let mut p = [0u8; 24];
    p[0..4].copy_from_slice(&orden.to_le_bytes());
    p[4..8].copy_from_slice(&cuantos.to_le_bytes());
    p[8..16].copy_from_slice(&primero.to_le_bytes());
    p[16..20].copy_from_slice(&off.to_le_bytes());
    let copia = dashos::llave_duplicar(vuelta, dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
    let e = if copia > 0 { dashos::canal_escribir(1, &p, &[copia as u32]) } else { copia };
    if e != 0 {
        return None;
    }
    let mut r = [0u8; 24];
    let mut ll = [0u32; 1];
    let (n, k) = dashos::canal_leer(lectura, &mut r, &mut ll);
    if n != 24 {
        return None;
    }
    let u32_en = |i: usize| u32::from_le_bytes(r[i..i + 4].try_into().unwrap());
    Some(Respuesta {
        estado: u32_en(0) as i32,
        sectores: u32_en(8) as u64 | (u32_en(12) as u64) << 32,
        tam_sector: u32_en(16),
        ventana: (k > 0).then_some(ll[0]),
    })
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

fn principal() -> u64 {
    let (lectura, vuelta) = dashos::canal_crear();
    let Some(a) = pedir(lectura, vuelta, ABRIR, 0, 0, 0) else {
        println!("  [disco] sin servicio de disco");
        return 1;
    };
    let Some(v) = a.ventana.filter(|_| a.estado == 0) else {
        println!("  [disco] no se abre (estado {})", a.estado);
        return 2;
    };
    if dashos::memoria_proyectar(v, VA, true) != 0 {
        println!("  [disco] no se proyecta la ventana");
        return 3;
    }
    println!("  [disco] abierto: {} sectores de {} bytes", a.sectores, a.tam_sector);
    let t = a.tam_sector as usize;
    let estado = |orden, cuantos, primero, off| pedir(lectura, vuelta, orden, cuantos, primero, off).map_or(i32::MIN, |r| r.estado);
    // Los dos primeros sectores.
    let leidos = estado(LEER, 2, 0, 0) == 0 && empieza(0, b"DashOS: sector 0") && empieza(t, b"DashOS: sector 1");
    // Escribir el sector 2 y leerlo en otro sitio de la ventana.
    let texto = b"DashOS: sector 2, escrito por el cliente";
    poner(4096, &[0; 512]);
    poner(4096, texto);
    poner(8192, &[0xEE; 512]);
    let escrito = estado(ESCRIBIR, 1, 2, 4096) == 0 && estado(LEER, 1, 2, 8192) == 0 && (0..t).all(|i| byte(8192 + i) == byte(4096 + i));
    // 4 KB desde la mitad de una página: la segunda página va por PRP2.
    let cruzando = estado(LEER, (4096 / t) as u32, 0, 2048) == 0 && empieza(2048, b"DashOS: sector 0") && empieza(2048 + 2 * t, texto);
    // Lo que se debe rechazar: fuera del disco (-2) y fuera de la ventana (-3).
    let rechazos = estado(LEER, 1, a.sectores, 0) == -2 && estado(LEER, 2, 0, (64 * 1024 - t) as u32) == -3 && estado(LEER, 1, 0, 2) == -3;
    let si = |b: bool| if b { "bien" } else { "MAL" };
    println!("  [disco] leer: {}; escribir y releer: {}; cruzando páginas: {}; rechazos: {}", si(leidos), si(escrito), si(cruzando), si(rechazos));
    0
}
