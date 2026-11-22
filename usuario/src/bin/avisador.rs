//! S10: la prueba del servicio `avisos`, como un programa cualquiera que tiene su llave (de `inicio`, canal "avisos";
//! llave 1). Avisa bien una vez y después intenta lo que no debe: hacerse pasar por otro, mensajes mal hechos, bytes
//! que no son texto e inundar. Al final, otro aviso bueno: el servicio sigue.
//!
//! S27, oleada 3: también un aviso con acciones y lo que no debe con ellas (acciones falsas a mano, mensajes enormes), las
//! órdenes de la barra, del panel y del centro sin serlo (descartar, pulsar acciones, "no molestar", los ajustes), y
//! sustituir un aviso suyo (eso sí puede).

#![no_std]
#![no_main]

use dashos::avisos_cliente::{avisar, avisar_con, mandar};
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
    // S27, oleada 6: el resumen de la pantalla de bloqueo como si fuera `sesiones` (con su marca, no vale): no es un
    // resumen (el servicio no lo entiende), ni cuántos ni de quién.
    let mut resumen = [0u8; s::LARGO_AVISO_RESUMEN];
    resumen[0..4].copy_from_slice(&s::AVISO_RESUMEN.to_le_bytes());
    resumen[4..8].copy_from_slice(&1000u32.to_le_bytes());
    let r_resumen = mandar(AVISOS, &resumen);
    let sin_resumen = r_resumen.is_some_and(|r| r.estado == MAL);
    println!("  [avisador] el resumen de la pantalla de bloqueo sin ser sesiones: {}", if sin_resumen { "no lo da, bien" } else { "MAL" });
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
    let acciones = acciones_hostiles();
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
    if bueno && impostor && sin_resumen && rechazos && inunda && acciones { 0 } else { 1 }
}

/// S27: avisos con acciones. Uno bueno (con su llave para oírlas) y su sustituto; acciones falsas escritas a mano; un
/// mensaje enorme; y lo que solo pueden la barra, el panel o el centro.
fn acciones_hostiles() -> bool {
    let (_oigo, escucha) = dashos::canal_crear();
    let bueno = avisar_con(AVISOS, NORMAL, "Descarga acabada", "informe.pdf (2 MB)", 0, &[("default", "Abrir"), ("carpeta", "Ver en la carpeta")], Some(escucha));
    let numero = bueno.as_ref().filter(|r| r.estado == BIEN).map_or(0, |r| r.numero);
    // Sustituir el suyo, sí.
    let suyo = avisar_con(AVISOS, NORMAL, "Descarga acabada", "informe.pdf, en Descargas", numero, &[("carpeta", "Ver en la carpeta")], None);
    let sustituye = numero != 0 && suyo.as_ref().is_some_and(|r| r.estado == BIEN && r.numero > numero);
    println!("  [avisador] aviso con acciones: {} (número {}); sustituir el suyo: {}", bueno.as_ref().map_or(i32::MIN, |r| r.estado), numero,
        if sustituye { "bien" } else { "MAL" });
    // Acciones falsas a mano: cinco acciones, un identificador con espacios o con un byte de control, repetido, una
    // etiqueta vacía, largos que se salen, bytes de más.
    let mut m = [0u8; 256];
    let n = dashos::protocolo::avisos::pedir_aviso_con(&mut m, NORMAL, "título", "texto", 0, &[("a", "Uno"), ("b", "Dos")]).unwrap_or(0);
    let i = CABECERA_AVISAR_CON + "título".len() + "texto".len();
    let cambia = |f: &dyn Fn(&mut [u8; 256]) -> usize| {
        let mut x = m;
        let k = f(&mut x);
        mandar(AVISOS, &x[..k]).is_some_and(|r| r.estado == MAL)
    };
    let falsas = [
        cambia(&|x| { x[20..24].copy_from_slice(&5u32.to_le_bytes()); n }),
        cambia(&|x| { x[20..24].copy_from_slice(&u32::MAX.to_le_bytes()); n }),
        cambia(&|x| { x[i + 2] = b' '; n }),
        cambia(&|x| { x[i + 2] = 0x1b; n }),
        cambia(&|x| { x[i + 2 + 1 + 3 + 2] = b'a'; n }),
        cambia(&|x| { x[i + 1] = 0; n }),
        cambia(&|x| { x[i] = 250; n }),
        cambia(&|_| n + 3),
        cambia(&|_| n - 1),
    ];
    let rechazadas = falsas.iter().filter(|&&x| x).count();
    // Enorme: un mensaje de 300 bytes no cabe en el canal (no llega); uno de 256 con largos falsos, MAL.
    let enorme = [0x41u8; 300];
    let no_llega = mandar(AVISOS, &enorme).is_none();
    // Lo de la barra, el panel y el centro, sin serlo: PROTEGIDO, y nada cambia.
    let orden = |o: u32, a: u32, b: u32| {
        let mut x = [0u8; 12];
        x[0..4].copy_from_slice(&o.to_le_bytes());
        x[4..8].copy_from_slice(&a.to_le_bytes());
        x[8..12].copy_from_slice(&b.to_le_bytes());
        mandar(AVISOS, &x).is_some_and(|r| r.estado == PROTEGIDO)
    };
    let mut ajustes = [0u8; 40];
    ajustes[0..4].copy_from_slice(&AJUSTES.to_le_bytes());
    ajustes[4..8].copy_from_slice(&25u32.to_le_bytes());
    let protegidas = [
        orden(ACCION, numero, 0),
        orden(ACCION, numero, POR_DEFECTO),
        orden(LEER_ACCIONES, numero, 0),
        orden(DESCARTAR, numero, 0),
        orden(QUITAR, numero, 0),
        orden(DESCARTAR_TODO, 0, 0),
        orden(BORRAR_TODO, 0, 0),
        orden(LEER, u32::MAX, 0),
        orden(LEER_AJUSTES, 0, 0),
        orden(NO_MOLESTAR, 1, 0),
        orden(ESTADO, 0, 0),
        mandar(AVISOS, &ajustes[..33]).is_some_and(|r| r.estado == PROTEGIDO),
    ];
    let negadas = protegidas.iter().filter(|&&x| x).count();
    let ok = sustituye && rechazadas == falsas.len() && no_llega && negadas == protegidas.len();
    println!("  [avisador] acciones falsas rechazadas {} de {}; 300 bytes no llegan: {}; lo de la barra, el panel y el centro: {} de {} negadas: {}",
        rechazadas, falsas.len(), if no_llega { "sí" } else { "no" }, negadas, protegidas.len(), if ok { "bien" } else { "MAL" });
    ok
}

/// "Aviso 00".."Aviso 99", sin `alloc`.
fn alloc_free_numero(i: u32) -> [u8; 8] {
    let mut t = *b"Aviso 00";
    t[6] = b'0' + (i / 10 % 10) as u8;
    t[7] = b'0' + (i % 10) as u8;
    t
}
