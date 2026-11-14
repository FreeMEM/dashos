//! Trasfondo: un programa del disco (`Programas/espera`) que no hace nada durante 90 s y acaba. Lo lanza la barra como
//! la persona de delante, para que el monitor del sistema tenga un proceso suyo que cerrar en la prueba.

#![no_std]
#![no_main]

use dashos::println;

dashos::programa!(principal);

fn principal() -> u64 {
    println!("  [espera] en marcha: 90 s sin hacer nada");
    for _ in 0..90 {
        dashos::dormir_us(1_000_000);
    }
    println!("  [espera] acaba");
    0
}
