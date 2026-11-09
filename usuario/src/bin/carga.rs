//! Carga para la prueba de cortes del sonido (S19, A6): espera medio segundo (a que suenen los tonos) y gira sin parar
//! durante 6 s, sin dormir ni llamar al kernel (leer el reloj no es una llamada). Cuatro copias ocupan los núcleos de
//! QEMU `virt`.

#![no_std]
#![no_main]

use dashos::println;

dashos::programa!(principal);

fn principal() -> u64 {
    dashos::dormir_us(500_000);
    let fin = dashos::ahora_us() + 6_000_000;
    let mut vueltas = 0u64;
    while dashos::ahora_us() < fin {
        vueltas = vueltas.wrapping_add(1);
        core::hint::black_box(vueltas);
    }
    println!("  [carga] 6 s ocupado");
    0
}
