//! El cargador hunk de `LoadSeg` y de ramlib (`hunk.rs`): ejecutables y bibliotecas de verdad (de `caja/pruebas68k`,
//! hechos con m68k-amigaos-gcc), su lista de segmentos y sus reubicaciones; y ficheros rotos u hostiles (cortados,
//! con bytes cambiados al azar, reubicaciones fuera de sitio, tamaños enormes): siempre un error y nada sin soltar.

mod comun;

use caja_dos::{Memoria, hunk};
use comun::*;

fn fichero(n: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../pruebas68k/{n}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

#[test]
fn ejecutables_y_bibliotecas_de_verdad() {
    for n in ["saludo.exe", "boopsi.exe", "gadtools.exe", "prueba.library", "ramlib.exe"] {
        let mut c = CajaPrueba::nueva();
        let datos = fichero(n);
        let seg = hunk::cargar(&mut c, &datos).unwrap_or_else(|e| panic!("{n}: error {e}"));
        assert_ne!(seg, 0);
        let segs = hunk::segmentos(&c, seg);
        assert!(!segs.is_empty(), "{n}");
        // El primero es el código: empieza por lo del fichero (tras la cabecera del hunk de código).
        let (ini, bytes) = segs[0];
        assert!(bytes >= 4);
        let primera = c.leer32(ini).unwrap();
        assert_ne!(primera, 0, "{n}: el código está");
        // Reservado: un bloque por segmento; al soltar, nada.
        assert_eq!(c.vivos.len(), segs.len(), "{n}");
        hunk::descargar(&mut c, seg);
        assert!(c.vivos.is_empty(), "{n}: todo suelto");
    }
    // La biblioteca: su RomTag está en algún segmento, y apunta a sí mismo (las reubicaciones van bien).
    let mut c = CajaPrueba::nueva();
    let seg = hunk::cargar(&mut c, &fichero("prueba.library")).unwrap();
    let mut encontrado = false;
    for (ini, bytes) in hunk::segmentos(&c, seg) {
        let mut x = ini;
        while x + 6 <= ini + bytes {
            if c.leer16(x) == Some(0x4AFC) && c.leer32(x + 2) == Some(x) {
                encontrado = true;
            }
            x += 2;
        }
    }
    assert!(encontrado, "el RomTag de prueba.library, reubicado");
}

#[test]
fn ficheros_hostiles() {
    let buenos = [fichero("saludo.exe"), fichero("prueba.library")];
    // Cortados por todas partes.
    for d in &buenos {
        for n in (0..d.len()).step_by(3) {
            let mut c = CajaPrueba::nueva();
            if hunk::cargar(&mut c, &d[..n]).is_ok() {
                continue;
            }
            assert!(c.vivos.is_empty(), "cortado en {n}: algo sin soltar");
        }
    }
    // Bytes cambiados al azar.
    let mut semilla = 0x9E37_79B9_7F4A_7C15u64;
    let mut azar = || {
        semilla ^= semilla << 13;
        semilla ^= semilla >> 7;
        semilla ^= semilla << 17;
        semilla
    };
    for d in &buenos {
        for _ in 0..3000 {
            let mut m = d.clone();
            for _ in 0..1 + azar() % 4 {
                let i = (azar() as usize) % m.len();
                m[i] = azar() as u8;
            }
            let mut c = CajaPrueba::nueva();
            match hunk::cargar(&mut c, &m) {
                Ok(seg) => {
                    hunk::descargar(&mut c, seg);
                    assert!(c.vivos.is_empty());
                }
                Err(_) => assert!(c.vivos.is_empty(), "un error deja algo reservado"),
            }
        }
    }
    // A mano: tamaños enormes, una reubicación a un hunk que no existe, más hunks de los que dice.
    let mal = |v: &[u32]| v.iter().flat_map(|x| x.to_be_bytes()).collect::<Vec<u8>>();
    for f in [
        mal(&[0x3F3, 0, 1, 0, 0, 0x3FFF_FFFF]),
        mal(&[0x3F3, 0, 1, 0, 0, 2, 0x3E9, 2, 0x4E75_4E75, 0, 0x3EC, 1, 7, 0, 0, 0x3F2]),
        mal(&[0x3F3, 0, 1, 0, 0, 2, 0x3E9, 2, 0x4E75_4E75, 0, 0x3EC, 1, 0, 400, 0, 0x3F2]),
        mal(&[0x3F3, 0, 1, 0, 0, 1, 0x3E9, 1, 0, 0x3F2, 0x3E9, 1, 0, 0x3F2]),
        mal(&[0x3F3, 1, 0x41414141, 0, 1, 0, 0, 1]),
        mal(&[0x3F3, 0, 5000, 0, 4999]),
        mal(&[0x3F3, 0, 1, 0, 0, 1, 0x3F5]),
    ] {
        let mut c = CajaPrueba::nueva();
        assert!(hunk::cargar(&mut c, &f).is_err(), "{f:x?}");
        assert!(c.vivos.is_empty());
    }
    // Sin memoria a mitad: se suelta lo que había.
    let mut c = CajaPrueba::nueva();
    c.reservas_restantes = Some(1);
    assert!(hunk::cargar(&mut c, &fichero("boopsi.exe")).is_err());
    assert!(c.vivos.is_empty());
    // Una lista en círculo al soltar o recorrer: con tope, sin colgarse.
    let mut c = CajaPrueba::nueva();
    let seg = hunk::cargar(&mut c, &fichero("saludo.exe")).unwrap();
    let p = seg << 2;
    c.escribir32(p, seg).unwrap();
    assert!(hunk::segmentos(&c, seg).len() <= 1024);
}
