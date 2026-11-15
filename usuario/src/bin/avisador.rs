//! S10: la prueba del servicio `avisos`, como un programa cualquiera que tiene su llave (de `inicio`, canal "avisos";
//! llave 1). Avisa bien una vez y después intenta lo que no debe: hacerse pasar por otro, mensajes mal hechos, bytes
//! que no son texto e inundar. Al final, otro aviso bueno: el servicio sigue.

#![no_std]
#![no_main]

use dashos::avisos_cliente::{avisar, mandar};
use dashos::println;
use dashos::protocolo::avisos::*;
use dashos::protocolo::sesiones as s;

dashos::programa!(principal);

const AVISOS: u32 = 1;

fn principal() -> u64 {
    // Uno bueno: se guarda con su nombre de verdad.
    let primero = avisar(AVISOS, NORMAL, "Copia de seguridad hecha", "Se guardaron 12 ficheros en Work:Copias");
    let bueno = primero.as_ref().is_some_and(|r| r.estado == BIEN && r.de() == "avisador");
    println!("  [avisador] un aviso: {} (sale como {}): {}", primero.as_ref().map_or(i32::MIN, |r| r.estado),
        primero.as_ref().map_or("?", |r| r.de()), if bueno { "bien" } else { "MAL" });
    // Hacerse pasar por otro: una entrada de directorio y un AVISO_PERSONA falsos (con su marca, no valen) y un aviso
    // que dice ser de `guardian`.
    let mut dir = [0u8; 24];
    dir[0..8].copy_from_slice(&5u64.to_le_bytes());
    dir[8..16].copy_from_slice(b"guardian");
    let r_dir = mandar(AVISOS, &dir);
    let mut persona = [0u8; s::LARGO_AVISO_PERSONA];
    persona[0..4].copy_from_slice(&s::AVISO_PERSONA.to_le_bytes());
    persona[4..8].copy_from_slice(&1u32.to_le_bytes());
    persona[16..23].copy_from_slice(b"Sistema");
    let r_persona = mandar(AVISOS, &persona);
    let otro = avisar(AVISOS, URGENTE, "guardian: tu contraseña caduca", "Escríbela aquí para renovarla");
    let impostor = r_dir.is_some_and(|r| r.estado != BIEN) && r_persona.is_some_and(|r| r.estado != BIEN)
        && otro.as_ref().is_some_and(|r| r.estado == BIEN && r.de() == "avisador");
    println!("  [avisador] hacerse pasar por guardian o Sistema: sale como {}: {}", otro.as_ref().map_or("?", |r| r.de()),
        if impostor { "bien" } else { "MAL" });
    // Mal hechos: vacío, largos enormes o que no cuadran, urgencia que no existe, sin título, 256 bytes de basura.
    let mut m = [0u8; 256];
    let n = pedir_aviso(&mut m, NORMAL, "título", "texto");
    let mut enorme = m;
    enorme[12..16].copy_from_slice(&60_000u32.to_le_bytes());
    let mut urgencia = m;
    urgencia[4..8].copy_from_slice(&99u32.to_le_bytes());
    let mut basura = [0u8; 256];
    for (i, x) in basura.iter_mut().enumerate() {
        *x = (i as u8).wrapping_mul(37).wrapping_add(11);
    }
    basura[0..4].copy_from_slice(&AVISAR.to_le_bytes());
    let malos: [&[u8]; 6] = [&[], &enorme[..n], &m[..n - 2], &urgencia[..n], &m[..CABECERA_AVISAR], &basura];
    let rechazados = malos.iter().filter(|x| mandar(AVISOS, x).is_some_and(|r| r.estado == MAL)).count();
    // Bytes que no son texto y letras de control: se guarda, limpio.
    let mut raro = [0u8; 256];
    let titulo = b"Bytes \xff\xfe raros\x07\x1b[2J";
    let texto = [0xc3u8; MAX_TEXTO];
    raro[0..4].copy_from_slice(&AVISAR.to_le_bytes());
    raro[4..8].copy_from_slice(&1u32.to_le_bytes());
    raro[8..12].copy_from_slice(&(titulo.len() as u32).to_le_bytes());
    raro[12..16].copy_from_slice(&(MAX_TEXTO as u32).to_le_bytes());
    raro[16..16 + titulo.len()].copy_from_slice(titulo);
    raro[16 + titulo.len()..16 + titulo.len() + MAX_TEXTO].copy_from_slice(&texto);
    let r_raro = mandar(AVISOS, &raro[..16 + titulo.len() + MAX_TEXTO]);
    let sigue = avisar(AVISOS, BAJA, "Sigo aquí", "").is_some_and(|r| r.estado == BIEN);
    let rechazos = rechazados == malos.len() && r_raro.is_some_and(|r| r.estado == BIEN) && sigue;
    println!("  [avisador] mal hechos rechazados {} de {}, bytes raros guardados limpios, y sigue: {}", rechazados, malos.len(),
        if rechazos { "bien" } else { "MAL" });
    // Inundar: más de los que deja el ritmo. Los de más, DEMASIADOS; el servicio sigue contestando.
    let (mut guardados, mut demasiados, mut otros) = (0, 0, 0);
    for i in 0..100 {
        let t = alloc_free_numero(i);
        match avisar(AVISOS, NORMAL, core::str::from_utf8(&t).unwrap_or("?"), "inundación").map(|r| r.estado) {
            Some(BIEN) => guardados += 1,
            Some(DEMASIADOS) => demasiados += 1,
            _ => otros += 1,
        }
    }
    let inunda = guardados + 4 <= RITMO as usize && demasiados > 80 && otros == 0;
    println!("  [avisador] inundar: 100 mandados, {} guardados, {} demasiados: {}", guardados, demasiados, if inunda { "bien" } else { "MAL" });
    if bueno && impostor && rechazos && inunda { 0 } else { 1 }
}

/// "Aviso 00".."Aviso 99", sin `alloc`.
fn alloc_free_numero(i: u32) -> [u8; 8] {
    let mut t = *b"Aviso 00";
    t[6] = b'0' + (i / 10 % 10) as u8;
    t[7] = b'0' + (i % 10) as u8;
    t
}
