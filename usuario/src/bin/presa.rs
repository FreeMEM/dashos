//! U2: un programa del mismo usuario que `hostil` (1001), que espera sin hacer nada, solo en `virt`. `hostil` lo mata
//! por `sesiones`, que sí se lo deja: es suyo. Nombra el canal "sesiones" solo para no ser un programa sin llaves (a
//! esos, `inicio` los espera).

#![no_std]
#![no_main]

use dashos::println;

dashos::programa!(principal);

fn principal() -> u64 {
    println!("  [presa] espera");
    loop {
        dashos::dormir_us(1_000_000);
    }
}
