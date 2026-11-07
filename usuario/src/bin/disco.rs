//! Prueba del servicio de disco (U6f): un cliente del canal "disco" (la llave 1, con su marca de `inicio`). Abre el
//! disco, lee los dos primeros sectores en su ventana, escribe el sector 2 y lo vuelve a leer, lee 4 KB que cruzan
//! una página de la ventana, y comprueba que se rechazan un sector fuera del disco y un trozo fuera de la ventana.
//! Después pide su anillo (U6l) y manda por él 16 lecturas de golpe y una que se debe rechazar.

#![no_std]
#![no_main]

use dashos::println;

dashos::programa!(principal);

const VA: usize = 0x40_0000_0000;
const VA_ANILLO: usize = 0x41_0000_0000;
const ANILLO: u32 = 3;
const MIRAR: u32 = 4;

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
    // El anillo, con su propio canal: por él llegan la respuesta y después los avisos de terminadas.
    let (l2, v2) = dashos::canal_crear();
    let Some(r) = pedir(l2, v2, ANILLO, 0, 0, 0).filter(|r| r.estado == 0) else {
        println!("  [disco] sin anillo");
        return 4;
    };
    if r.ventana.is_none_or(|m| dashos::memoria_proyectar(m, VA_ANILLO, true) != 0) {
        println!("  [disco] no se proyecta el anillo");
        return 5;
    }
    let (avisos, esperas, bien) = probar_anillo(l2);
    println!("  [disco] anillo: 16 lecturas y un rechazo: {}", si(bien));
    println!("  [disco] (anillo con {} avisos al servicio y {} esperas)", avisos, esperas);
    0
}

fn palabra(off: usize) -> &'static core::sync::atomic::AtomicU32 {
    unsafe { &*((VA_ANILLO + off) as *const core::sync::atomic::AtomicU32) }
}

/// Pone en el anillo lecturas de los sectores `0..n` (más una fuera del disco si `malo`), avisa si el servicio
/// duerme, y espera sus terminadas. Devuelve (avisos mandados, veces que esperó, todo bien).
fn probar_anillo(lectura: u32) -> (u32, u32, bool) {
    use core::sync::atomic::{fence, Ordering::*};
    let (mut avisos, mut esperas) = (0, 0);
    let total = 17u32;
    // 16 lecturas de un sector a la ventana desde 16 KB, con etiqueta 100 + i; la 17, fuera del disco (etiqueta 999).
    for i in 0..total {
        let e = 64 + 32 * (i % 32) as usize;
        let (primero, etiqueta) = if i < 16 { (i as u64, 100 + i) } else { (u64::MAX - 1, 999) };
        palabra(e).store(1, Relaxed);
        palabra(e + 4).store(1, Relaxed);
        palabra(e + 8).store(primero as u32, Relaxed);
        palabra(e + 12).store((primero >> 32) as u32, Relaxed);
        palabra(e + 16).store((16384 + 512 * (i % 16) as usize) as u32, Relaxed);
        palabra(e + 20).store(etiqueta, Relaxed);
    }
    palabra(4).store(total, Release);
    fence(SeqCst);
    if palabra(16).load(SeqCst) != 0 {
        let mut p = [0u8; 24];
        p[..4].copy_from_slice(&MIRAR.to_le_bytes());
        dashos::canal_escribir(1, &p, &[]);
        avisos += 1;
    }
    // Esperar las 17 terminadas: "espero" y mirar; si no están, dormir hasta el aviso del servicio.
    let mut buf = [0u8; 8];
    let mut ll = [0u32; 1];
    loop {
        palabra(20).store(1, SeqCst);
        fence(SeqCst);
        if palabra(12).load(Acquire).wrapping_sub(palabra(8).load(Relaxed)) >= total {
            break;
        }
        dashos::canal_leer(lectura, &mut buf, &mut ll);
        esperas += 1;
    }
    let mut bien = true;
    for i in 0..total {
        let c = 2048 + 16 * (i % 32) as usize;
        let (etiqueta, estado) = (palabra(c).load(Relaxed), palabra(c + 4).load(Relaxed) as i32);
        bien &= if etiqueta == 999 { estado == -2 } else { (100..116).contains(&etiqueta) && estado == 0 };
    }
    palabra(8).store(total, Release);
    bien &= empieza(16384, b"DashOS: sector 0") && empieza(16384 + 512, b"DashOS: sector 1") && empieza(16384 + 1024, b"DashOS: sector 2");
    (avisos, esperas, bien)
}
