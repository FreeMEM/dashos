//! Cliente de Wayland de prueba B (W7): una ventana de 120 × 100 (ARGB) con un tablero de cuadros verdes opacos y
//! negros a medias (alfa 128, premultiplicado), que deja ver lo de debajo. Cambia de fase cada cuadro.

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
mod ventana_cliente;

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let bien = ventana_cliente::ventana("ventana-b", "Ventana B", 120, 100, wayland::shm::ARGB8888, |x, y, n| {
        // Las primeras 10 líneas no cambian (para mirarlas en la captura); el resto, con la fase del cuadro.
        let fase = if y < 10 { 0 } else { n };
        if ((x / 10) + (y / 10) + fase) % 2 == 0 { 0xff33_cc33 } else { 0x8000_0000 }
    });
    if bien { 0 } else { 1 }
}
