//! C31: un programa hostil. Pasa al kernel punteros a memoria vacía, a su propio código (solo lectura) y fuera del
//! espacio de usuario, y comprueba que recibe errores en vez de tumbar el kernel; y que un mensaje que no se puede
//! entregar se queda en la cola, con sus llaves. Sale con 31 si todo va bien, o con el número del paso que falló.

#![no_std]
#![no_main]

use dashos::{llamada, println};

dashos::programa!(principal);

const E_ARGUMENTO: i64 = -1;
const E_TAMANO: i64 = -5;
const VACIA: u64 = 0x10_0000_0000; // 64 GB: nada proyectado
const KERNEL: u64 = 0xffff_0000_0000_0000;

fn x0(n: u64, a: [u64; 5]) -> i64 {
    llamada(n, a).0 as i64
}

fn principal() -> u64 {
    // 1-3: escribir desde memoria vacía, desde el kernel, y un tramo que da la vuelta.
    if x0(1, [VACIA, 16, 0, 0, 0]) != E_ARGUMENTO {
        return 1;
    }
    if x0(1, [KERNEL, 16, 0, 0, 0]) != E_ARGUMENTO {
        return 2;
    }
    if x0(1, [u64::MAX - 4, 16, 0, 0, 0]) != E_ARGUMENTO {
        return 3;
    }
    let (a, b) = dashos::canal_crear();
    // 4: mandar datos desde memoria vacía.
    if x0(4, [a as u64, VACIA, 10, 0, 0]) != E_ARGUMENTO {
        return 4;
    }
    // 5: un mensaje con 20 bytes y una llave de memoria.
    let m = dashos::memoria_crear(4096);
    let texto = *b"veinte bytes exactos";
    if dashos::canal_escribir(a, &texto, &[m]) != 0 {
        return 5;
    }
    let mut buf = [0u8; 64];
    let mut llaves = [0u32; 4];
    // 6: búfer pequeño: E_TAMANO y el mensaje se queda.
    if x0(5, [b as u64, buf.as_mut_ptr() as u64, 4, llaves.as_mut_ptr() as u64, 4]) != E_TAMANO {
        return 6;
    }
    // 7: leer encima de su propio código, que en EL0 es de solo lectura: el kernel no puede escribir ahí por él.
    let codigo = principal as *const () as u64;
    if x0(5, [b as u64, codigo, 64, llaves.as_mut_ptr() as u64, 4]) != E_ARGUMENTO {
        return 7;
    }
    // 8: datos bien, pero la tabla de llaves en memoria vacía: tampoco se entrega.
    if x0(5, [b as u64, buf.as_mut_ptr() as u64, 64, VACIA, 4]) != E_ARGUMENTO {
        return 8;
    }
    // 9: una capacidad de llaves que desborda al multiplicar por 4.
    if x0(5, [b as u64, buf.as_mut_ptr() as u64, 64, llaves.as_mut_ptr() as u64, u64::MAX / 2]) != E_ARGUMENTO {
        return 9;
    }
    // 10: ahora sí: el mensaje entero, con su llave, que sirve.
    let (n, k) = dashos::canal_leer(b, &mut buf, &mut llaves);
    if n != 20 || k != 1 || buf[..20] != texto {
        return 10;
    }
    if dashos::memoria_proyectar(llaves[0], 0x20_0000_0000, true) != 0 {
        return 11;
    }
    println!("  [malo] punteros malos rechazados y el mensaje entregado entero después");
    31
}
