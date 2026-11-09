//! Cliente de Wayland de prueba A (W7): una ventana roja de 160 × 120 (XRGB) con una banda blanca que baja por sus
//! últimas 10 líneas de izquierda a derecha, un trozo por cuadro.

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../ventana_cliente.rs"]
mod ventana_cliente;

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let bien = ventana_cliente::ventana("ventana-a", "Ventana A", 160, 120, wayland::shm::XRGB8888, |x, y, n| {
        if y >= 110 && x < (n + 1) * 160 / ventana_cliente::CUADROS { 0x00ff_ffff } else { 0x00cc_3333 }
    });
    if bien { 0 } else { 1 }
}
