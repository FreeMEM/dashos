//! Un cliente glotón de la red (S13), solo en `virt`. Llave 1: el canal "red".
//!
//! Abre conexiones con el eco del PC sin parar: el servicio le da `POR_PROGRAMA` y después `LIMITE`, por muchas veces
//! que lo pida. Las tiene abiertas mientras `red-prueba` hace su eco (los demás siguen), las cierra y comprueba que
//! puede abrir otra.

#![no_std]
#![no_main]

use dashos::println;
use dashos::protocolo::red as p;
use dashos::red_cliente::{estado, Conexion};

dashos::programa!(principal);

const SERVICIO: u32 = 1;
const VA: usize = 0x50_0000_0000;
const HUECO: usize = 2 << 20;
const ECO: [u8; 4] = [10, 0, 2, 100];

fn principal() -> u64 {
    let t0 = dashos::ahora_us();
    while !matches!(estado(SERVICIO, 2_000_000), Ok(e) if e[p::E_CONFIGURADA] == 1) {
        if dashos::ahora_us() - t0 > 60_000_000 {
            println!("  [red-gloton] la red no tiene dirección tras 60 s");
            return 1;
        }
        dashos::dormir_us(200_000);
    }
    let mut abiertas: [Option<Conexion>; p::POR_PROGRAMA] = Default::default();
    let mut n = 0;
    // Sin fin: hasta que el servicio diga que no 50 veces seguidas.
    let (mut noes, mut ultimo) = (0, 0);
    let mut intentos = 0;
    while noes < 50 && intentos < 200 {
        intentos += 1;
        let va = VA + n.min(p::POR_PROGRAMA) * HUECO;
        match Conexion::conectar(SERVICIO, ECO, 7, va, 4096, 15_000_000) {
            Ok(c) if n < abiertas.len() => {
                abiertas[n] = Some(c);
                n += 1;
            }
            Ok(c) => {
                println!("  [red-gloton] el servicio da una conexión de más (la {})", n + 1);
                c.cerrar();
                return 2;
            }
            Err(e) => {
                noes += 1;
                ultimo = e;
            }
        }
    }
    let bien = n == p::POR_PROGRAMA && ultimo == p::LIMITE;
    println!("  [red-gloton] tope en {} conexiones; {} intentos más dan {}: {}", n, noes, ultimo, if bien { "bien" } else { "MAL" });
    // Mientras `red-prueba` hace lo suyo (se ve en el total del servicio), se tienen. Como mucho 3 minutos.
    let t1 = dashos::ahora_us();
    let mut visto = false;
    while dashos::ahora_us() - t1 < 180_000_000 {
        let total = estado(SERVICIO, 2_000_000).map(|e| e[p::E_CONEXIONES] as usize).unwrap_or(n);
        if total > n {
            visto = true;
        } else if visto {
            break;
        }
        dashos::dormir_us(200_000);
    }
    for c in abiertas.iter_mut() {
        if let Some(c) = c.take() {
            c.cerrar();
        }
    }
    // Lo cerrado se suelta en el servicio poco después: otra vez puede abrir.
    let t2 = dashos::ahora_us();
    loop {
        match Conexion::conectar(SERVICIO, ECO, 7, VA, 4096, 15_000_000) {
            Ok(c) => {
                c.cerrar();
                println!("  [red-gloton] cerradas las {}; otra vez abre: bien", n);
                return 0;
            }
            Err(p::LIMITE) if dashos::ahora_us() - t2 < 10_000_000 => dashos::dormir_us(100_000),
            Err(e) => {
                println!("  [red-gloton] cerradas las {}; abrir otra da {} (MAL)", n, e);
                return 3;
            }
        }
    }
}
