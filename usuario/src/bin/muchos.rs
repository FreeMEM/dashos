//! Segunda vuelta de la auditoría de la fase 4, A3: un proceso con 900 temporizadores periódicos de 10 ms (un tic) que
//! vencen todos a la vez en cada tic. Lee sus avisos sin fin; lo mata el kernel. La prueba (`pruebas.rs`, C51) mira
//! que un temporizador de otro grupo no se retrasa por ellos.

#![no_std]
#![no_main]

use dashos::{canal_crear, canal_leer, println, temporizador_crear};

dashos::programa!(principal);

fn principal() -> u64 {
    let (a, b) = canal_crear();
    let mut n = 0;
    for _ in 0..900 {
        if temporizador_crear(b, 10_000, 10_000) > 0 {
            n += 1;
        }
    }
    println!("  [muchos] {} temporizadores de 10 ms", n);
    let mut buf = [0u8; 8];
    let mut ll = [0u32; 1];
    loop {
        canal_leer(a, &mut buf, &mut ll);
    }
}
