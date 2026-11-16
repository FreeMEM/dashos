//! Blanker (S10, `SYS:Tools/Commodities/Blanker`; `docs/bitacoras/commodities.md`): apaga la pantalla tras un rato sin
//! tocar el teclado ni el ratón. No dibuja nada ni ve la entrada: pide al compositor el aviso de inactividad y, cuando
//! llega, que apague la pantalla (la pinta él, de negro, hasta la siguiente tecla o movimiento). Permiso `fondo`.
//!
//! Llaves (lanzado del disco o desde el Shell): 1 `sesiones`, 2 archivos, 3 el compositor. Se va cuando Exchange lo
//! pide, o si le quitan el permiso.

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
#[allow(dead_code)]
mod ventana_cliente;
#[path = "../commodity_cliente.rs"]
#[allow(dead_code)]
mod commodity_cliente;

use commodity_cliente::{Cx, Evento};
use dashos::println;

dashos::programa!(principal);
dashos::monton!();

const SESIONES: u32 = 1;
const COMPOSITOR: u32 = 3;
/// Cinco minutos, como el de serie del Amiga (que eran 60 s; aquí, uno más cómodo).
const SEGUNDOS: u32 = 300;

fn principal() -> u64 {
    dashos::monton::con_nombre(&"blanker");
    let e = commodity_cliente::pedir_permiso(SESIONES, "fondo");
    if e != 0 {
        println!("  [blanker] sin el permiso de fondo ({}): no puede saber cuándo no se usa el ordenador", e);
        return 10;
    }
    ventana_cliente::usar_llave(COMPOSITOR);
    let mut cx = match Cx::conectar() {
        Ok(c) => c,
        Err(e) => {
            println!("  [blanker] {}", e);
            return 20;
        }
    };
    let Some(yo) = cx.registrar("Blanker", "Blanker", "Apaga la pantalla tras un rato sin usar el ordenador") else { return 20 };
    cx.inactividad(yo, SEGUNDOS);
    println!("  [blanker] en marcha: apaga la pantalla tras {} s sin entrada", SEGUNDOS);
    loop {
        match cx.evento(60_000_000) {
            Ok(Some(Evento::Inactivo { si: true, .. })) => cx.oscurecer(yo),
            Ok(Some(Evento::Activo { si, .. })) => println!("  [blanker] {}", if si { "activado" } else { "desactivado" }),
            Ok(Some(Evento::Quitar { .. })) => {
                println!("  [blanker] Exchange pide que se vaya: adiós");
                return 0;
            }
            Ok(Some(Evento::Retirado { .. })) => {
                println!("  [blanker] le quitaron el permiso: adiós");
                return 0;
            }
            Ok(Some(Evento::Error(t))) => {
                println!("  [blanker] el compositor cortó: {}", t);
                return 20;
            }
            Err(()) => return 20,
            _ => {}
        }
    }
}
