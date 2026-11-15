//! Prueba de `audio.device` nativo (S19, fase 2; `make sonido-paula`): un programa nativo que lo usa como lo usaría
//! un programa del Amiga, y lo que suena sale por el servicio de sonido.
//!
//! - Pide el canal 0 (izquierda) y escribe un ciclo de seno de 32 muestras a período 252: 3.546.895 / 252 / 32 =
//!   439,8 Hz, 440 ciclos (1 s).
//! - Pide el canal 1 (derecha) y escribe otro a período 168 (659,8 Hz), sin fin, a volumen 48.
//! - A los 0,5 s, otra asignación con más precedencia roba el canal 1: la escritura sin fin vuelve con
//!   `ADIOERR_CHANNELSTOLEN` y el nuevo dueño escribe 879,7 Hz (período 126), 440 ciclos (0,5 s).
//!
//! `tools/sonido_wav.py --paula` comprueba el WAV de QEMU: 440 Hz solo a la izquierda durante 1 s, 660 Hz a la
//! derecha 0,5 s y luego 880 Hz otros 0,5 s.

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../sonido_cliente.rs"]
mod sonido_cliente;

use alloc::vec;
use alloc::vec::Vec;
use audiodevice::*;
use dashos::println;

dashos::programa!(principal);
dashos::monton!();

const VA: usize = 0x40_0000_0000;
const F: u32 = 48_000;

/// Un ciclo de seno en 32 muestras de 8 bits (127 · sen(2πi/32), redondeado).
const SENO: [i8; 32] = [
    0, 25, 49, 71, 90, 106, 117, 125, 127, 125, 117, 106, 90, 71, 49, 25, 0, -25, -49, -71, -90, -106, -117, -125, -127, -125, -117, -106, -90, -71, -49, -25,
];

fn asignar(d: &mut AudioDevice, id: Id, combinacion: u8, prec: i8) -> Option<(u16, u8)> {
    d.pedir(Peticion { id, orden: ADCMD_ALLOCATE, banderas: ADIOF_NOWAIT, prec, datos: vec![combinacion as i8], ..Default::default() });
    match d.avisos().as_slice() {
        [Aviso::Hecha { error: 0, llave, unidad, .. }] => Some((*llave, *unidad)),
        a => {
            println!("  [paula] no se asigna {}: {:?}", combinacion, a);
            None
        }
    }
}

fn escribir(id: Id, unidad: u8, llave: u16, periodo: u16, volumen: u16, ciclos: u16) -> Peticion {
    Peticion { id, orden: CMD_WRITE, banderas: ADIOF_PERVOL, unidad, llave, datos: SENO.to_vec(), periodo, volumen, ciclos, ..Default::default() }
}

fn principal() -> u64 {
    let Some(mut f) = sonido_cliente::abrir(1, sonido_cliente::I16, 2, F, VA, 32 * 1024) else {
        println!("  [paula] no abre el flujo");
        return 1;
    };
    let mut d = AudioDevice::nuevo(RELOJ_PAL, F);
    let Some((k_izq, u_izq)) = asignar(&mut d, 1, 1, 0) else { return 2 };
    let Some((k_der, u_der)) = asignar(&mut d, 2, 2, 0) else { return 2 };
    d.pedir(escribir(10, u_izq, k_izq, 252, 64, 440));
    d.pedir(escribir(11, u_der, k_der, 168, 48, 0));
    println!("  [paula] canal 0: 440 Hz a la izquierda; canal 1: 660 Hz a la derecha, sin fin");

    let mut cuadros = [[0i16; 2]; 256];
    let mut bytes: Vec<u8> = vec![0; 256 * 4];
    let mut hechos = 0u32;
    let mut robado = false;
    let (mut fin_izq, mut fin_der) = (false, false);
    let mut avisos: Vec<(u32, Aviso)> = Vec::new();
    // Hasta que acaben las dos, y 0,1 s más de silencio.
    let mut cola = 0;
    while cola < F / 10 {
        if !robado && hechos >= F / 2 {
            robado = true;
            let Some((k, u)) = asignar_robando(&mut d, &mut avisos, hechos) else { return 3 };
            d.pedir(escribir(12, u, k, 126, 48, 440));
        }
        d.mezclar(&mut cuadros);
        hechos += cuadros.len() as u32;
        for a in d.avisos() {
            avisos.push((hechos, a));
            match a {
                Aviso::Hecha { id: 10, .. } => fin_izq = true,
                Aviso::Hecha { id: 12, .. } => fin_der = true,
                _ => {}
            }
        }
        if fin_izq && fin_der {
            cola += cuadros.len() as u32;
        }
        for (i, c) in cuadros.iter().enumerate() {
            bytes[4 * i..4 * i + 2].copy_from_slice(&c[0].to_le_bytes());
            bytes[4 * i + 2..4 * i + 4].copy_from_slice(&c[1].to_le_bytes());
        }
        f.escribir_todo(&bytes);
    }
    for (cuando, a) in avisos {
        if let Aviso::Hecha { id, error, unidad, .. } = a {
            println!("  [paula] a los {} ms vuelve la petición {} (canales {}): error {}", cuando as u64 * 1000 / F as u64, id, unidad, error);
        }
    }
    f.cerrar();
    println!("  [paula] fin");
    0
}

/// Otra asignación, con más precedencia, se lleva el canal 1; anota lo que vuelve.
fn asignar_robando(d: &mut AudioDevice, avisos: &mut Vec<(u32, Aviso)>, hechos: u32) -> Option<(u16, u8)> {
    d.pedir(Peticion { id: 3, orden: ADCMD_ALLOCATE, banderas: ADIOF_NOWAIT, prec: 10, datos: vec![2], ..Default::default() });
    let mut r = None;
    for a in d.avisos() {
        if let Aviso::Hecha { id: 3, error: 0, llave, unidad } = a {
            r = Some((llave, unidad));
        }
        avisos.push((hechos, a));
    }
    if r.is_none() {
        println!("  [paula] no se roba el canal 1");
    }
    r
}
