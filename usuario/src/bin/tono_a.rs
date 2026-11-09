//! Programa de prueba del sonido A (S19, A4 y A5): un tono de 1 kHz a un cuarto de la escala en mono de 16 bits a
//! 48 kHz, por el servicio "sonido". A los 1,5 s escritos falla a propósito (escribe en la dirección 0x10) y el kernel
//! lo mata sin que cierre su flujo: el servicio lo nota al no poder despertarlo y lo quita con un fundido (A5), a
//! mitad de un ciclo.

#![no_std]
#![no_main]

#[path = "../sonido_cliente.rs"]
mod sonido_cliente;

use dashos::println;

dashos::programa!(principal);

const VA: usize = 0x40_0000_0000;

fn principal() -> u64 {
    let Some(mut f) = sonido_cliente::abrir(1, sonido_cliente::I16, 1, 48_000, VA, 16 * 1024) else {
        println!("  [tono-a] no abre el flujo");
        return 1;
    };
    // Un ciclo de 1 kHz a 48 kHz son 48 muestras: la tabla, por la recurrencia del seno en f64.
    let (c, s) = (0.991_444_861_373_810_4f64, 0.130_526_192_220_051_6f64);
    let mut ciclo = [0u8; 96];
    let (mut y2, mut y1) = (-s, 0.0f64);
    for i in 0..48 {
        let v = (y1 * 0.25 * 32767.0 + if y1 >= 0.0 { 0.5 } else { -0.5 }) as i16;
        ciclo[2 * i..2 * i + 2].copy_from_slice(&v.to_le_bytes());
        let y = 2.0 * c * y1 - y2;
        y2 = y1;
        y1 = y;
    }
    for _ in 0..1500 {
        f.escribir_todo(&ciclo);
    }
    // Un cuarto de ciclo más: el tono se corta en lo alto del seno, no en un cruce por cero.
    f.escribir_todo(&ciclo[..24]);
    println!("  [tono-a] 1,5 s escritos; ahora falla a propósito");
    unsafe { core::ptr::write_volatile(0x10 as *mut u32, 0) };
    f.cerrar();
    0
}
