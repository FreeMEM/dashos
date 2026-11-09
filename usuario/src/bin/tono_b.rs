//! Programa de prueba del sonido B (S19, A4): un tono de 600 Hz a 0,4 de la escala, 2 s, en estéreo de coma flotante
//! a 44,1 kHz (el servicio lo remuestrea), con el volumen a la mitad: suena a 0,2. 2 s de 600 Hz son 1.200 ciclos
//! enteros: acaba en un cruce por cero.

#![no_std]
#![no_main]

#[path = "../sonido_cliente.rs"]
mod sonido_cliente;

use dashos::println;

dashos::programa!(principal);

const VA: usize = 0x40_0000_0000;

fn principal() -> u64 {
    let Some(mut f) = sonido_cliente::abrir(1, sonido_cliente::F32, 2, 44_100, VA, 32 * 1024) else {
        println!("  [tono-b] no abre el flujo");
        return 1;
    };
    f.volumen(0.5, 0.5);
    // El seno por giros de (cos w, sin w), w = 2π·600/44.100, en f64, renormalizando de vez en cuando.
    let (cw, sw) = (0.996_348_338_021_206_9f64, 0.085_381_434_295_628_47f64);
    let (mut c, mut s) = (1.0f64, 0.0f64);
    let mut trozo = [0u8; 8 * 128];
    let total = 2 * 44_100;
    let mut hechas = 0;
    while hechas < total {
        let n = (total - hechas).min(128);
        for i in 0..n {
            let v = (s * 0.4) as f32;
            trozo[8 * i..8 * i + 4].copy_from_slice(&v.to_le_bytes());
            trozo[8 * i + 4..8 * i + 8].copy_from_slice(&v.to_le_bytes());
            (c, s) = (c * cw - s * sw, s * cw + c * sw);
        }
        // Vuelta al radio 1: un paso de Newton de 1/√r, que con r casi 1 basta.
        let k = (3.0 - (c * c + s * s)) / 2.0;
        (c, s) = (c * k, s * k);
        f.escribir_todo(&trozo[..8 * n]);
        hechas += n;
    }
    f.cerrar();
    println!("  [tono-b] 2 s escritos");
    0
}
