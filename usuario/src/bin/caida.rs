//! E6 (`docs/REVISION_ARQUITECTURA.md`): un servicio de prueba que se cae a propósito, solo en `virt`. Nombra el canal
//! "caida" antes que nadie, así que para `inicio` es un servicio: cuando acaba, lo relanza con las mismas llaves, con
//! espera creciente y como mucho tres veces. Cada arranque dice cuántos mensajes hay ya en su canal: `inicio` le deja
//! uno antes de cada relanzado, así que el primero ve 0 y los relanzados 1: el canal sigue siendo el mismo.

#![no_std]
#![no_main]

use dashos::println;

dashos::programa!(principal);

fn principal() -> u64 {
    let mut b = [0u8; 64];
    let mut vistos = 0;
    // Lo que ya haya en el canal, sin esperar: un plazo de 1 ms (K7).
    while dashos::canal_leer_plazo(1, &mut b, &mut [], 1_000).0 >= 0 {
        vistos += 1;
    }
    println!("  [caida] arranca ({} avisos de inicio en su canal) y cae a propósito", vistos);
    3
}
