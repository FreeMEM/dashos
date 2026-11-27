//! Pruebas del lector de la tabla de particiones (A14, `usuario/src/particiones.rs`): tablas buenas y, sobre todo,
//! rotas a propósito. Van aquí porque el crate de usuario es `no_std` y sin pruebas en el PC.

use super::particiones::{Clase, Fallo, TIPO_GPT, TIPO_GPT_CAJA_LINUX, TIPO_MBR, Tabla, crc32, leer_tabla};

const EFI: [u8; 16] = [0x28, 0x73, 0x2a, 0xc1, 0x1f, 0xf8, 0xd2, 0x11, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9, 0x3b];

fn leer(d: &[u8], ts: usize, sectores: u64) -> Result<Tabla, Fallo> {
    let mut f = |s: u64, b: &mut [u8]| {
        let o = s as usize * ts;
        if o + ts > d.len() {
            return false;
        }
        b.copy_from_slice(&d[o..o + ts]);
        true
    };
    leer_tabla(&mut f, sectores, ts)
}

/// Un disco de `sectores` de 512 bytes con un MBR de estas entradas (tipo, desde, cuántos).
fn mbr(sectores: u64, entradas: &[(u8, u32, u32)]) -> Vec<u8> {
    let mut d = vec![0u8; sectores as usize * 512];
    for (k, &(t, a, n)) in entradas.iter().enumerate() {
        let o = 446 + 16 * k;
        d[o + 4] = t;
        d[o + 8..o + 12].copy_from_slice(&a.to_le_bytes());
        d[o + 12..o + 16].copy_from_slice(&n.to_le_bytes());
    }
    d[510] = 0x55;
    d[511] = 0xAA;
    d
}

/// Una GPT en un disco de `sectores` de `ts` bytes, con 128 entradas de 128 bytes: (tipo, primer y último sector).
fn gpt(sectores: u64, ts: usize, partes: &[([u8; 16], u64, u64)]) -> Vec<u8> {
    let mut d = vec![0u8; sectores as usize * ts];
    d[446 + 4] = 0xEE;
    d[446 + 8..446 + 12].copy_from_slice(&1u32.to_le_bytes());
    d[446 + 12..446 + 16].copy_from_slice(&((sectores - 1).min(u32::MAX as u64) as u32).to_le_bytes());
    d[510] = 0x55;
    d[511] = 0xAA;
    let lista = 2u64;
    let sl = (128 * 128) / ts as u64;
    let mut l = vec![0u8; 128 * 128];
    for (k, &(t, a, b)) in partes.iter().enumerate() {
        let e = &mut l[k * 128..][..128];
        e[..16].copy_from_slice(&t);
        e[16] = k as u8 + 1; // GUID propio, cualquiera que no sea cero
        e[32..40].copy_from_slice(&a.to_le_bytes());
        e[40..48].copy_from_slice(&b.to_le_bytes());
    }
    d[lista as usize * ts..][..l.len()].copy_from_slice(&l);
    let c = &mut d[ts..2 * ts];
    c[..8].copy_from_slice(b"EFI PART");
    c[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
    c[12..16].copy_from_slice(&92u32.to_le_bytes());
    c[24..32].copy_from_slice(&1u64.to_le_bytes());
    c[32..40].copy_from_slice(&(sectores - 1).to_le_bytes());
    c[40..48].copy_from_slice(&(lista + sl).to_le_bytes());
    c[48..56].copy_from_slice(&(sectores - 1 - sl - 1).to_le_bytes());
    c[72..80].copy_from_slice(&lista.to_le_bytes());
    c[80..84].copy_from_slice(&128u32.to_le_bytes());
    c[84..88].copy_from_slice(&128u32.to_le_bytes());
    c[88..92].copy_from_slice(&crc32(0, &l).to_le_bytes());
    cerrar(&mut d, ts);
    d
}

/// Rehace el CRC de la cabecera (tras cambiarla a mano, para que lo que se pruebe sea el cambio y no el CRC).
fn cerrar(d: &mut [u8], ts: usize) {
    let c = &mut d[ts..2 * ts];
    c[16..20].fill(0);
    let n = u32::from_le_bytes(c[12..16].try_into().unwrap()).min(ts as u32) as usize;
    let crc = crc32(0, &c[..n]);
    c[16..20].copy_from_slice(&crc.to_le_bytes());
}

/// Rehace el CRC de la lista y luego el de la cabecera.
fn cerrar_lista(d: &mut [u8], ts: usize) {
    let crc = crc32(0, &d[2 * ts..][..128 * 128]);
    d[ts + 88..ts + 92].copy_from_slice(&crc.to_le_bytes());
    cerrar(d, ts);
}

fn poner64(d: &mut [u8], o: usize, v: u64) {
    d[o..o + 8].copy_from_slice(&v.to_le_bytes());
}

#[test]
fn crc_conocido() {
    assert_eq!(crc32(0, b"123456789"), 0xCBF4_3926);
    assert_eq!(crc32(crc32(0, b"1234"), b"56789"), 0xCBF4_3926);
}

#[test]
fn sin_tabla() {
    let d = vec![0u8; 64 * 512];
    assert_eq!(leer(&d, 512, 64), Ok(Tabla { clase: Clase::Ninguna, dashos: None, linux: None, primera: 64 }));
}

#[test]
fn mbr_bueno() {
    // Como la tarjeta de la Pi: arranque en FAT y luego DashOS.
    let d = mbr(16384, &[(0x0C, 2048, 2048), (TIPO_MBR, 4096, 12288)]);
    assert_eq!(leer(&d, 512, 16384), Ok(Tabla { clase: Clase::Mbr, dashos: Some((4096, 16384)), linux: None, primera: 2048 }));
    let d = mbr(16384, &[(0x83, 2048, 1000)]);
    assert_eq!(leer(&d, 512, 16384), Ok(Tabla { clase: Clase::Mbr, dashos: None, linux: None, primera: 2048 }));
    // Firma sin entradas: un sector 0 cualquiera con la firma (el de la prueba de U6d).
    let d = mbr(64, &[]);
    assert_eq!(leer(&d, 512, 64), Ok(Tabla { clase: Clase::Mbr, dashos: None, linux: None, primera: 64 }));
}

#[test]
fn mbr_roto() {
    let s = 16384;
    for (entradas, fallo) in [
        (vec![(TIPO_MBR, 2048, 14337)], Fallo::FueraDelDisco),
        (vec![(TIPO_MBR, 0, 100)], Fallo::FueraDelDisco),
        (vec![(TIPO_MBR, 2048, 0)], Fallo::FueraDelDisco),
        (vec![(TIPO_MBR, u32::MAX, u32::MAX)], Fallo::FueraDelDisco),
        (vec![(0x0C, 2048, 2049), (TIPO_MBR, 4096, 100)], Fallo::Pisadas),
        (vec![(TIPO_MBR, 4096, 100), (0x0C, 2048, 2049)], Fallo::Pisadas),
        (vec![(0x83, 100, 100), (0, 0, 0), (0x83, 150, 1)], Fallo::Pisadas),
        (vec![(TIPO_MBR, 2048, 100), (TIPO_MBR, 4096, 100)], Fallo::DosDeDashOS),
    ] {
        assert_eq!(leer(&mbr(s, &entradas), 512, s), Err(fallo), "{entradas:?}");
    }
}

#[test]
fn gpt_buena() {
    for ts in [512, 4096] {
        let s = if ts == 512 { 16384 } else { 2048 };
        let d = gpt(s, ts, &[(EFI, 100, 199), (TIPO_GPT, 200, s - 40)]);
        assert_eq!(leer(&d, ts, s), Ok(Tabla { clase: Clase::Gpt, dashos: Some((200, s - 39)), linux: None, primera: 1 }), "ts {ts}");
    }
    let d = gpt(16384, 512, &[(EFI, 100, 199)]);
    assert_eq!(leer(&d, 512, 16384).unwrap().dashos, None);
}

/// H9: la partición de la caja Linux, sola o junto a la de DashOS; una de Linux cualquiera no cuenta; dos, error.
#[test]
fn gpt_caja_linux() {
    const LINUX: [u8; 16] = [0xaf, 0x3d, 0xc6, 0x0f, 0x83, 0x84, 0x72, 0x47, 0x8e, 0x79, 0x3d, 0x69, 0xd8, 0x47, 0x7d, 0xe4];
    let s = 16384;
    let d = gpt(s, 512, &[(TIPO_GPT_CAJA_LINUX, 2048, s - 40)]);
    assert_eq!(leer(&d, 512, s), Ok(Tabla { clase: Clase::Gpt, dashos: None, linux: Some((2048, s - 39)), primera: 1 }));
    let d = gpt(s, 512, &[(TIPO_GPT, 100, 999), (TIPO_GPT_CAJA_LINUX, 1000, s - 40)]);
    assert_eq!(leer(&d, 512, s), Ok(Tabla { clase: Clase::Gpt, dashos: Some((100, 1000)), linux: Some((1000, s - 39)), primera: 1 }));
    let d = gpt(s, 512, &[(LINUX, 100, 999), (TIPO_GPT, 1000, s - 40)]);
    assert_eq!(leer(&d, 512, s).unwrap().linux, None);
    let d = gpt(s, 512, &[(TIPO_GPT_CAJA_LINUX, 100, 999), (TIPO_GPT_CAJA_LINUX, 1000, s - 40)]);
    assert_eq!(leer(&d, 512, s), Err(Fallo::DosDeDashOS));
}

#[test]
fn gpt_rota() {
    let (s, ts) = (16384u64, 512);
    let bueno = gpt(s, ts, &[(EFI, 100, 199), (TIPO_GPT, 200, 10000)]);
    let caso = |f: &dyn Fn(&mut Vec<u8>)| {
        let mut d = bueno.clone();
        f(&mut d);
        leer(&d, ts, s)
    };
    // Los CRC.
    assert_eq!(caso(&|d| d[ts + 100 - 9] ^= 1), Err(Fallo::Crc)); // dentro de los 92 bytes
    assert_eq!(caso(&|d| d[2 * ts + 128 + 50] ^= 1), Err(Fallo::Crc));
    assert_eq!(caso(&|d| d[2 * ts + 127 * 128] = 1), Err(Fallo::Crc)); // la última entrada, también (y no vale)
    // Fuera de los 92 bytes de la cabecera no cuenta.
    assert!(caso(&|d| d[ts + 200] = 7).is_ok());
    // La cabecera.
    assert_eq!(caso(&|d| d[ts] = b'X'), Err(Fallo::Cabecera)); // la firma se mira antes que el CRC
    let mal = |o: usize, v: Vec<u8>| {
        move |d: &mut Vec<u8>| {
            d[ts + o..ts + o + v.len()].copy_from_slice(&v);
            cerrar(d, ts);
        }
    };
    for (o, v) in [
        (0, b"EFI PARX".to_vec()),
        (12, 91u32.to_le_bytes().to_vec()),
        (12, 513u32.to_le_bytes().to_vec()),
        (24, 2u64.to_le_bytes().to_vec()),
        (80, 0u32.to_le_bytes().to_vec()),
        (80, 100_000u32.to_le_bytes().to_vec()),
        (84, 100u32.to_le_bytes().to_vec()),
        (84, 1024u32.to_le_bytes().to_vec()),
        (72, 1u64.to_le_bytes().to_vec()),
        (72, 3u64.to_le_bytes().to_vec()), // la lista se mete en lo utilizable
        (72, (u64::MAX - 3).to_le_bytes().to_vec()),
        (40, 20u64.to_le_bytes().to_vec()),
        (48, s.to_le_bytes().to_vec()),
        (40, 16351u64.to_le_bytes().to_vec()), // lo utilizable, al revés
    ] {
        assert_eq!(caso(&mal(o, v)), Err(Fallo::Cabecera), "byte {o}");
    }
    // Las entradas.
    let ent = |k: usize, a: u64, b: u64, t: [u8; 16]| {
        move |d: &mut Vec<u8>| {
            let o = 2 * ts + k * 128;
            d[o..o + 16].copy_from_slice(&t);
            poner64(d, o + 32, a);
            poner64(d, o + 40, b);
            cerrar_lista(d, ts);
        }
    };
    assert_eq!(caso(&ent(2, 33, 50, EFI)), Err(Fallo::FueraDelDisco)); // antes de lo utilizable
    assert_eq!(caso(&ent(2, 11000, s - 1, EFI)), Err(Fallo::FueraDelDisco)); // encima de la copia
    assert_eq!(caso(&ent(2, 11000, u64::MAX, EFI)), Err(Fallo::FueraDelDisco));
    assert_eq!(caso(&ent(2, 11000, 10999, EFI)), Err(Fallo::FueraDelDisco));
    assert_eq!(caso(&ent(2, 10000, 10500, EFI)), Err(Fallo::Pisadas));
    assert_eq!(caso(&ent(127, 50, 100, EFI)), Err(Fallo::Pisadas)); // la última pisa a la primera
    assert_eq!(caso(&ent(2, 11000, 12000, TIPO_GPT)), Err(Fallo::DosDeDashOS));
    assert!(caso(&ent(2, 11000, 12000, EFI)).is_ok());
    // Un disco que no se deja leer.
    let mut f = |s: u64, b: &mut [u8]| s == 0 && {
        b.copy_from_slice(&bueno[..512]);
        true
    };
    assert_eq!(leer_tabla(&mut f, s, ts), Err(Fallo::Lectura));
}

#[test]
fn sectores_raros() {
    let d = vec![0u8; 8192];
    assert_eq!(leer(&d, 300, 10), Err(Fallo::Sector));
    assert_eq!(leer(&d, 8192, 1), Err(Fallo::Sector));
    assert_eq!(leer(&d, 512, 1), Err(Fallo::Sector));
}

/// Tablas estropeadas al azar: nunca un pánico, y si sale una partición de DashOS, cabe en el disco, no tapa el
/// sector 0 y no pisa a la tabla.
#[test]
fn al_azar() {
    let s = 300u64;
    let buenos = [gpt(s, 512, &[(EFI, 40, 99), (TIPO_GPT, 100, 250)]), mbr(s, &[(0x0C, 8, 30), (TIPO_MBR, 40, 200)])];
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    let mut azar = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let mut aceptadas = 0;
    for v in 0..20_000 {
        let mut d = buenos[v % 2].clone();
        for _ in 0..1 + azar() % 4 {
            // Sobre todo en la cabecera y las primeras entradas, que es donde está lo que se lee.
            let o = (azar() % (if azar() % 2 == 0 { 3 * 512 } else { 34 * 512 })) as usize;
            d[o] = azar() as u8;
        }
        if v % 3 == 0 && v % 2 == 0 {
            cerrar(&mut d, 512);
        }
        if let Ok(t) = leer(&d, 512, s) {
            aceptadas += 1;
            if let Some((a, b)) = t.dashos {
                assert!(a >= 1 && a < b && b <= s, "{t:?}");
                if t.clase == Clase::Gpt {
                    assert!(a >= 2, "{t:?}");
                }
            }
            assert!(t.primera <= s);
        }
    }
    assert!(aceptadas > 100, "{aceptadas}");
}
