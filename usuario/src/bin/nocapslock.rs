//! NoCapsLock (S10, `SYS:Tools/Commodities/NoCapsLock`; `docs/bitacoras/commodities.md`): Bloq Mayús no hace nada
//! mientras esté en marcha. No ve el teclado: pide al compositor el comportamiento "sin Bloq Mayús", que dura mientras
//! viva y esté activo (Exchange lo apaga al desactivarlo). Permiso `fondo`.
//!
//! Llaves (lanzado del disco o desde el Shell): 1 `sesiones`, 2 archivos, 3 el compositor.

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
/// `dashos_commodity_v1.comportamiento`: sin Bloq Mayús.
const SIN_BLOQ_MAYUS: u32 = 1;

fn principal() -> u64 {
    dashos::monton::con_nombre(&"nocapslock");
    let e = commodity_cliente::pedir_permiso(SESIONES, "fondo");
    if e != 0 {
        println!("  [nocapslock] sin el permiso de fondo ({})", e);
        return 10;
    }
    ventana_cliente::usar_llave(COMPOSITOR);
    let mut cx = match Cx::conectar() {
        Ok(c) => c,
        Err(e) => {
            println!("  [nocapslock] {}", e);
            return 20;
        }
    };
    let Some(yo) = cx.registrar("NoCapsLock", "NoCapsLock", "Bloq Mayús no hace nada") else { return 20 };
    cx.comportamiento(yo, SIN_BLOQ_MAYUS, true);
    println!("  [nocapslock] en marcha: Bloq Mayús no llega a nadie");
    loop {
        match cx.evento(60_000_000) {
            Ok(Some(Evento::Quitar { .. })) | Ok(Some(Evento::Retirado { .. })) => return 0,
            Ok(Some(Evento::Error(t))) => {
                println!("  [nocapslock] el compositor cortó: {}", t);
                return 20;
            }
            Err(()) => return 20,
            _ => {}
        }
    }
}
