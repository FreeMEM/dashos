//! S28: el bloque de arranque, sin firmas: no arranca, sin código, como el de AmigaDOS, sospechoso y otro.

use amigados::arranque::{Arranque, TAMANO, analizar, suma};

/// Un bloque de arranque con `codigo` desde el byte 12 y la suma bien.
fn bloque(codigo: &[u8]) -> [u8; TAMANO] {
    let mut b = [0u8; TAMANO];
    b[..4].copy_from_slice(b"DOS\x01");
    b[8..12].copy_from_slice(&880u32.to_be_bytes());
    b[12..12 + codigo.len()].copy_from_slice(codigo);
    let s = suma(&b);
    b[4..8].copy_from_slice(&s.to_be_bytes());
    b
}

/// Un código como el de AmigaDOS, escrito a mano: lea nombre(pc),a1; jsr FindResident(a6); tst.l d0; beq.s fallo;
/// move.l d0,a0; move.l 22(a0),a0; moveq #0,d0; rts; fallo: moveq #-1,d0; rts; "dos.library".
fn como_amigados() -> Vec<u8> {
    let mut c = vec![
        0x43, 0xFA, 0x00, 0x18, 0x4E, 0xAE, 0xFF, 0xA0, 0x4A, 0x80, 0x67, 0x0A, 0x20, 0x40, 0x20, 0x68, 0x00, 0x16, 0x70,
        0x00, 0x4E, 0x75, 0x70, 0xFF, 0x4E, 0x75,
    ];
    c.extend_from_slice(b"dos.library\0");
    c
}

#[test]
fn clases() {
    // Sin DOS, o con la suma mal.
    assert_eq!(analizar(&[0u8; TAMANO]), Arranque::NoArranca);
    let mut b = bloque(&como_amigados());
    b[4] ^= 1;
    assert_eq!(analizar(&b), Arranque::NoArranca);
    // La suma: el bloque de arranque con su suma suma 0xFFFFFFFF con acarreo.
    assert_eq!(analizar(&bloque(&[])), Arranque::SinCodigo);
    assert_eq!(analizar(&bloque(&como_amigados())), Arranque::ComoAmigaDos);
    // Un "virus" de laboratorio: SetFunction de DoIO y CoolCapture.
    let mut v = como_amigados();
    v.splice(0..0, [0x2D, 0x48, 0x00, 0x2E, 0x4E, 0xAE, 0xFE, 0x5C, 0x2D, 0x7C, 0x00, 0x07, 0xEC, 0x00, 0x02, 0x26]);
    match analizar(&bloque(&v)) {
        Arranque::Sospechoso { toca, .. } => assert_eq!(toca, ["CoolCapture", "SetFunction", "KickTagPtr"]),
        a => panic!("{a:?}"),
    }
    // Un cargador: código sin dos.library.
    assert!(matches!(analizar(&bloque(&[0x4E, 0x71, 0x4E, 0x71, 0x4E, 0x75])), Arranque::Otro { bytes: 6 }));
    // Lo mismo que el de AmigaDOS, pero muy largo: otro.
    let mut largo = como_amigados();
    largo.extend(std::iter::repeat_n(0x4E71u16.to_be_bytes(), 200).flatten());
    assert!(matches!(analizar(&bloque(&largo)), Arranque::Otro { .. }));
    // Las frases.
    assert!(analizar(&bloque(&v)).frase().starts_with("AVISO"));
}

/// Hostil: bloques al azar (con la suma arreglada o no). Nunca se cae; con la suma mal, no arranca.
#[test]
fn al_azar() {
    let mut x: u64 = 0x2545_F491_4F6C_DD1D;
    for k in 0..20_000 {
        let mut b = [0u8; TAMANO];
        for v in b.iter_mut() {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *v = x as u8;
        }
        b[..3].copy_from_slice(b"DOS");
        if k % 2 == 0 {
            let s = suma(&b);
            b[4..8].copy_from_slice(&s.to_be_bytes());
            let _ = analizar(&b).frase();
        } else if suma(&b) != u32::from_be_bytes(b[4..8].try_into().unwrap()) {
            assert_eq!(analizar(&b), Arranque::NoArranca);
        }
    }
}
