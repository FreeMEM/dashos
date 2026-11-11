//! U4: un programa con permiso para `eco`. Pide su llave a `sesiones`, la usa, renuncia y comprueba que la llave ya
//! no sirve (revocada). Llave 1: el canal "sesiones".

#![no_std]
#![no_main]

use dashos::println;
use dashos::protocolo::sesiones::{Nombres, DEJAR, NOMBRES, SERVICIO};

dashos::programa!(principal);

const SESIONES: u32 = 1;

/// Una petición a `sesiones` con el nombre de un servicio: (estado, llave que llegue).
fn pedir(orden: u32, servicio: &str) -> (i32, u32) {
    let mut p = [0u8; 64];
    p[0..4].copy_from_slice(&orden.to_le_bytes());
    p[4..8].copy_from_slice(&(servicio.len() as u32).to_le_bytes());
    p[12..12 + servicio.len()].copy_from_slice(servicio.as_bytes());
    let (lectura, suyo) = dashos::canal_crear();
    let mut r = [0u8; 4];
    let mut ll = [0u32; 1];
    let (n, k, _) = if dashos::canal_escribir(SESIONES, &p[..12 + servicio.len()], &[suyo]) == 0 {
        let (n, k, m) = dashos::canal_leer_plazo(lectura, &mut r, &mut ll, 5_000_000);
        (n, k, m)
    } else {
        (-1, 0, 0)
    };
    dashos::cerrar(lectura);
    (if n == 4 { i32::from_le_bytes(r) } else { i32::MIN }, if k == 1 { ll[0] } else { 0 })
}

/// Manda "hola" por `eco` y devuelve si contestó "eco: hola", o el error de escribir.
fn usar(eco: u32) -> Result<bool, i64> {
    let (lectura, suyo) = dashos::canal_crear();
    let e = dashos::canal_escribir(eco, b"hola", &[suyo]);
    if e != 0 {
        dashos::cerrar(lectura);
        dashos::cerrar(suyo);
        return Err(e);
    }
    let mut r = [0u8; 32];
    let (n, _, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut [], 2_000_000);
    dashos::cerrar(lectura);
    Ok(n > 0 && &r[..n as usize] == b"eco: hola")
}

fn principal() -> u64 {
    dashos::dormir_us(1_500_000);
    let (estado, eco) = pedir(SERVICIO, "eco");
    let contesta = eco != 0 && usar(eco) == Ok(true);
    // Sin el derecho de transferirla (S4): no puede regalarla.
    let (a, b) = dashos::canal_crear();
    let regalar = dashos::canal_escribir(a, b"toma", &[eco]);
    dashos::cerrar(a);
    dashos::cerrar(b);
    let (dejar, _) = pedir(DEJAR, "eco");
    let tras_dejar = usar(eco);
    let bien = estado == 0 && contesta && regalar == -3 && dejar == 0 && tras_dejar.is_err();
    println!(
        "  [pide] eco: con permiso {} y contesta {}; regalarla da {}; tras dejarla, usarla da {:?}: {}",
        estado,
        contesta,
        regalar,
        tras_dejar,
        if bien { "bien" } else { "MAL" }
    );
    if bien {
        println!("  [pide] U4: bien");
    }
    let (nombres, progdir) = nombres();
    if nombres {
        println!("  [pide] U2, tabla de nombres {}: bien", if progdir { "con PROGDIR" } else { "sin PROGDIR (no salió del disco)" });
    }
    if bien && nombres { 0 } else { 1 }
}

/// U2: su tabla de nombres. Usuario 1 (sin cuenta): sin `Home:`. Lanzado del disco por `entrada`, con `PROGDIR:` en
/// el cajón de donde salió; lanzado por `inicio`, sin él. Lo que intenta salir del subcamino no se resuelve. Devuelve
/// si todo fue bien y si tenía `PROGDIR:`.
fn nombres() -> (bool, bool) {
    let (lectura, suyo) = dashos::canal_crear();
    let mut r = [0u8; 256];
    let n = if dashos::canal_escribir(SESIONES, &NOMBRES.to_le_bytes(), &[suyo]) == 0 {
        dashos::canal_leer_plazo(lectura, &mut r, &mut [], 5_000_000).0
    } else {
        -1
    };
    dashos::cerrar(lectura);
    if n < 4 {
        println!("  [pide] U2: sin tabla de nombres ({})", n);
        return (false, false);
    }
    let t = Nombres::desde(&r[..n as usize]);
    for i in 0..t.len() {
        if let Some((nombre, vol, sub)) = t.entrada(i) {
            fn s(x: &[u8]) -> &str {
                core::str::from_utf8(x).unwrap_or("?")
            }
            println!("  [pide] {}: en {} {}", s(nombre), if vol.is_empty() { "el disco," } else { s(vol) }, s(sub));
        }
    }
    let mut sal = [0u8; 64];
    let mut va = |c: &str, vol: &str, ruta: Option<&str>| {
        let r = t.resolver(c, &mut sal);
        let bien = match (r, ruta) {
            (Some((v, x)), Some(e)) => v == vol.as_bytes() && x == e,
            (None, None) => true,
            _ => false,
        };
        if !bien {
            println!("  [pide] U2: {} no resuelve como debe", c);
        }
        bien
    };
    let mut probe = [0u8; 64];
    let progdir = t.resolver("PROGDIR:", &mut probe).is_some();
    let a = va("PROGDIR:pide", "", progdir.then_some("Programas/pide"));
    let b = va("progdir:otro/x", "", progdir.then_some("Programas/otro/x"));
    let c = va("SYS:", "", Some(""));
    let d = va("Home:notas", "", None);
    let e = va("PROGDIR:/hola", "", None);
    let f = va("SYS:a/../../b", "", None);
    let g = va("sin dos puntos", "", None);
    (t.len() == 2 + progdir as usize && a && b && c && d && e && f && g, progdir)
}
