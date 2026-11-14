//! Lee los eventos del teclado y el ratón que mandan los controladores de USB y los escribe (X5). En ARM los lee el
//! compositor; en x86, hasta que tenga pantalla (X6), este programa ocupa su sitio en el canal "ventanas" (llave 1)
//! para probar el xHCI. Formato de los eventos en `docs/referencia/servicios.md`. Escribe los cuatro primeros de cada
//! clase y después, cada 20, cuántos van.

#![no_std]
#![no_main]

use dashos::println;

dashos::programa!(principal);

fn principal() -> u64 {
    let mut escritor = dashos::hid::Entrada::nueva(None, "entrada");
    let (mut teclas, mut raton) = (0u32, 0u32);
    let mut buf = [0u8; 256];
    let mut ll = [0u32; 4];
    loop {
        let (n, k, marca) = dashos::canal_leer_marca(1, &mut buf, &mut ll);
        if n < 0 {
            return 1;
        }
        for &l in &ll[..k.min(ll.len())] {
            dashos::cerrar(l);
        }
        // Sin marca, de `inicio`: un cliente nuevo (8 bytes de marca y 16 de nombre).
        if marca == 0 {
            if n == 24 {
                let nombre = &buf[8..24];
                let largo = nombre.iter().position(|&c| c == 0).unwrap_or(16);
                println!("  [entrada] cliente: {}", core::str::from_utf8(&nombre[..largo]).unwrap_or("?"));
            }
            continue;
        }
        if n != 8 {
            continue;
        }
        let mut ev = [0u8; 8];
        ev.copy_from_slice(&buf[..8]);
        let (cuenta, clase) = if ev[0] == 1 { (&mut teclas, "teclado") } else { (&mut raton, "ratón") };
        *cuenta += 1;
        if *cuenta <= 4 {
            escritor.mandar(ev);
        } else if *cuenta % 20 == 0 {
            println!("  [entrada] {} eventos del {}", cuenta, clase);
        }
    }
}
