//! E6 (`docs/REVISION_ARQUITECTURA.md`): un servicio de prueba que se cae a propósito, solo en `virt`. Nombra el canal
//! "caida" antes que nadie, así que para `inicio` es un servicio: cuando acaba, lo relanza con las mismas llaves, con
//! espera creciente y como mucho tres veces. Cada arranque mira lo que hay ya en su canal: antes de relanzarlo, `inicio`
//! le deja un aviso de que es un relanzado y le vuelve a dar el directorio de sus clientes (uno: `caida-cliente`).
//! Así se comprueba que el canal es el mismo y que un servicio relanzado sabe quién es quién.

#![no_std]
#![no_main]

use dashos::println;

dashos::programa!(principal);

fn principal() -> u64 {
    let mut b = [0u8; 64];
    let (mut relanzado, mut directorio) = (false, 0);
    // Lo que ya haya en el canal, sin esperar: un plazo de 1 ms (K7). Lo de `inicio` llega sin marca.
    loop {
        let (n, _, marca) = dashos::canal_leer_plazo(1, &mut b, &mut [], 1_000);
        if n < 0 {
            break;
        }
        if marca == 0 && &b[..n as usize] == b"inicio: relanzado" {
            relanzado = true;
        } else if marca == 0 && n == 24 && &b[8..21] == b"caida-cliente" {
            directorio += 1;
        }
    }
    // En el primer arranque, la entrada de su cliente puede llegar antes o después: solo cuenta en los relanzados.
    if relanzado {
        println!("  [caida] relanzado, con {} cliente en el directorio; cae a propósito", directorio);
    } else {
        println!("  [caida] arranca y cae a propósito");
    }
    3
}
