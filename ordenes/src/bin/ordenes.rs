//! `SYS:C/Ordenes`: las órdenes de base del Shell nativo juntas en un programa (`docs/bitacoras/shell-nativa.md`,
//! "Dónde están las órdenes"). Hace la que le diga quien lo lanza, por el nombre del arranque: el Shell lo lanza cuando
//! le piden `Dir` y no hay un programa `Dir` en el camino.
//!
//! Por qué juntas: DashOS no tiene aún bibliotecas compartidas, y cada orden suelta lleva su copia de lo común (unos
//! 40 KB). Las 17, sueltas, ocupan 880 KB; juntas, la cuarta parte.

#![no_std]
#![no_main]

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let mut o = ordenes::Orden::arrancar("Ordenes");
    match ordenes::base::buscar(&o.llamada.clone()) {
        Some((nombre, f)) => {
            o.nombre = nombre;
            f(&mut o)
        }
        None => {
            let nombres: alloc::vec::Vec<&str> = ordenes::base::BASE.iter().map(|x| x.0).collect();
            o.error(format_args!("se lanza por el nombre de una de sus órdenes: {}", nombres.join(", ")));
            o.fin(shell::RC_FALLO)
        }
    }
}

extern crate alloc;
