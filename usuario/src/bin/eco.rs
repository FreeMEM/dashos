//! U4: un servicio de prueba que contesta lo que le llega, solo en `virt`. Nadie lo nombra en el arranque salvo
//! `sesiones`, que reparte su llave a quien tiene el permiso. Llave 1: su canal "eco".
//!
//! Cada petición trae la llave por la que contestar; se contesta "eco: " y lo que llegó. Lo que llega sin marca es el
//! directorio (de `sesiones`, al dar una llave): no se contesta.

#![no_std]
#![no_main]

use dashos::servicio::{cerrar_llaves, recibir, Recibido, MAX_DATOS, MAX_LLAVES};

dashos::programa!(principal);

fn principal() -> u64 {
    let mut buf = [0u8; MAX_DATOS];
    let mut ll = [0u32; MAX_LLAVES];
    loop {
        let (n, k, marca) = match recibir(1, &mut buf, &mut ll) {
            Recibido::Mensaje { n, k, marca } => (n, k, marca),
            Recibido::Cerrado => return 0,
            Recibido::Nada => continue,
        };
        if marca != 0 && k >= 1 {
            let mut r = [0u8; MAX_DATOS];
            let p = b"eco: ";
            r[..p.len()].copy_from_slice(p);
            let largo = n.min(MAX_DATOS - p.len());
            r[p.len()..p.len() + largo].copy_from_slice(&buf[..largo]);
            dashos::canal_escribir(ll[0], &r[..p.len() + largo], &[]);
        }
        cerrar_llaves(&ll[..k]);
    }
}
