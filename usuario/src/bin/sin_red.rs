//! Un programa sin la llave del canal "red" (S13), solo en `virt`: `inicio` no le da ninguna. Prueba a pedir una
//! conexión por todas las llaves que podría tener y por una suya: ninguna llega al servicio. Las llaves no se adivinan.

#![no_std]
#![no_main]

use dashos::println;
use dashos::protocolo::red as p;

dashos::programa!(principal);

fn principal() -> u64 {
    let pet = [p::CONECTAR, p::IPV4, 7, 4096, u32::from_le_bytes([10, 0, 2, 100]), 0, 0, 0];
    let bytes = unsafe { core::slice::from_raw_parts(pet.as_ptr() as *const u8, 32) };
    let mut llegan = 0;
    for k in 1..=1024u32 {
        if dashos::canal_escribir(k, bytes, &[]) == 0 {
            llegan += 1;
        }
    }
    // Su propio canal sí funciona: el fallo de arriba es por no tener la llave, no por el mensaje.
    let (lee, escribe) = dashos::canal_crear();
    let propio = dashos::canal_escribir(escribe, bytes, &[]) == 0;
    dashos::cerrar(lee);
    dashos::cerrar(escribe);
    let bien = llegan == 0 && propio;
    println!("  [sin-red] sin la llave: 1024 llaves probadas, llegan {}; por un canal propio sí: {}", llegan, if bien { "bien" } else { "MAL" });
    0
}
