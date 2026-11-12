//! Un programa sin el permiso de máquinas virtuales (caja Linux, H9), solo en `virt`: `inicio` no le da ninguna llave.
//! Prueba a crear una máquina con todas las llaves que podría tener, y con una suya de memoria como permiso: ninguna
//! sirve. Las llaves no se adivinan.

#![no_std]
#![no_main]

use dashos::println;

dashos::programa!(principal);

fn principal() -> u64 {
    let mem = dashos::memoria_crear(64 * 1024);
    let mut crean = 0;
    for k in 1..=1024u32 {
        if dashos::maquina_crear(k, mem, 0x4000_0000, 0x4000_0000) > 0 {
            crean += 1;
        }
    }
    // Con su propia memoria por permiso: el tipo no vale.
    let tipo = dashos::maquina_crear(mem, mem, 0x4000_0000, 0x4000_0000);
    let bien = crean == 0 && tipo == -6;
    println!("  [sin-maquina] sin el permiso: 1024 llaves probadas, crean una máquina {}; con su memoria por permiso, {}: {}", crean, tipo, if bien { "bien" } else { "MAL" });
    if bien { 0 } else { 1 }
}
