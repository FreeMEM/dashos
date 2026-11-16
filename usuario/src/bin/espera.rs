//! Trasfondo: un programa del disco (`Programas/espera`) que no hace nada durante 90 s y acaba. Lo lanza la barra como
//! la persona de delante, para que el monitor del sistema tenga un proceso suyo que cerrar en la prueba.
//!
//! S6b, S36 (rama `admin-desarrollo`): además, las pruebas del encierro y de la tanda (`prueba_encierro.rs`), según
//! cómo se lance, para no gastar el disco de pruebas con dos programas más:
//! - abierto encerrado (`Abrir espera`: sin `Home:` en su tabla), la aplicación encerrada; y su ayudante (sin `Home:` ni
//!   `PROGDIR:`), lo suyo;
//! - lanzado desde el Shell (con canal de arranque), la prueba hostil de la tanda;
//! - si no (la barra, o los procesos de prueba de la tanda), esperar 90 s.

#![no_std]
#![no_main]

extern crate alloc;

use dashos::println;

#[path = "../prueba_encierro.rs"]
mod prueba_encierro;

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let papel = prueba_encierro::Archivos::abrir().zip(dashos::asignaciones::pedir_tabla(1));
    if let Some((arch, t)) = &papel {
        let (home, progdir) = (t.buscar(b"Home").is_some(), t.buscar(b"PROGDIR").is_some());
        match (home, progdir, dashos::llaves::buscar("arranque").is_some()) {
            (false, false, _) => return prueba_encierro::ayudante(arch),
            (false, true, _) => return prueba_encierro::encerrada(arch, t),
            (true, _, true) => return prueba_encierro::tanda(arch, t),
            _ => {}
        }
    }
    println!("  [espera] en marcha: 90 s sin hacer nada");
    for _ in 0..90 {
        dashos::dormir_us(1_000_000);
    }
    println!("  [espera] acaba");
    0
}
