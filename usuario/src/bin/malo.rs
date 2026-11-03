//! C31: un programa hostil. Pasa al kernel punteros a memoria vacía, a su propio código (solo lectura) y fuera del
//! espacio de usuario, y comprueba que recibe errores en vez de tumbar el kernel; y que un mensaje que no se puede
//! entregar se queda en la cola, con sus llaves. Desde C32 también prueba los hallazgos de la auditoría del 03/11
//! (`docs/seguridad/`), un paso por cada uno. Sale con 31 si todo va bien, o con el número del paso que falló.

#![no_std]
#![no_main]

use dashos::{llamada, println};

dashos::programa!(principal);

const E_ARGUMENTO: i64 = -1;
const E_CERRADO: i64 = -4;
const E_TAMANO: i64 = -5;
const E_MEMORIA: i64 = -7;
const E_LIMITE: i64 = -8;
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
    // 12: la misma llave dos veces en un mensaje (hallazgo 6): error, no pánico, y la llave sigue siendo suya.
    let m2 = dashos::memoria_crear(4096);
    if dashos::canal_escribir(a, b"x", &[m2, m2]) != E_ARGUMENTO {
        return 12;
    }
    if dashos::memoria_proyectar(m2, 0x20_0001_0000, true) != 0 {
        return 13;
    }
    // 14: mandar una llave a un canal cerrado: E_CERRADO, y la llave vuelve a su número.
    let m3 = dashos::memoria_crear(4096);
    if x0(6, [b as u64, 0, 0, 0, 0]) != 0 {
        return 14;
    }
    if dashos::canal_escribir(a, b"x", &[m3]) != E_CERRADO {
        return 15;
    }
    if dashos::memoria_proyectar(m3, 0x20_0002_0000, true) != 0 {
        return 16;
    }
    // 17-18: proyectar en la dirección 0 y encima de algo ya proyectado (la página de m3, paso 16).
    if dashos::memoria_proyectar(m2, 0, true) != E_ARGUMENTO {
        return 17;
    }
    if dashos::memoria_proyectar(m2, 0x20_0002_0000, true) != E_ARGUMENTO {
        return 18;
    }
    // 19: la cola de un canal tiene límite (hallazgo 8): el mensaje 17 sin leer da E_LIMITE.
    let (c, d) = dashos::canal_crear();
    for _ in 0..16 {
        if dashos::canal_escribir(c, b"lleno", &[]) != 0 {
            return 19;
        }
    }
    if dashos::canal_escribir(c, b"lleno", &[]) != E_LIMITE {
        return 20;
    }
    let _ = d;
    // 21-22: comerse la memoria a trozos de 4 MB, el tope por llamada desde C35. Tiene que acabar en E_MEMORIA, no
    // en pánico; y al soltarla, se puede volver a pedir. 28: más de 4 MB de una vez da E_TAMANO.
    let mut trozos = [0u32; 900];
    let mut n = 0;
    let fin = loop {
        let k = x0(7, [4 << 20, 0, 0, 0, 0]);
        if k < 0 || n == trozos.len() {
            break k;
        }
        trozos[n] = k as u32;
        n += 1;
    };
    if fin != E_MEMORIA {
        return 21;
    }
    for &k in &trozos[..n] {
        x0(6, [k as u64, 0, 0, 0, 0]);
    }
    if x0(7, [8 << 20, 0, 0, 0, 0]) != E_TAMANO {
        return 28;
    }
    let k = x0(7, [4 << 20, 0, 0, 0, 0]);
    if k < 0 {
        return 22;
    }
    x0(6, [k as u64, 0, 0, 0, 0]);
    // 23: llenar la tabla de llaves con canales: acaba en E_LIMITE, no en pánico.
    let mut canales = 0;
    let fin = loop {
        let r = x0(3, [0; 5]);
        if r < 0 || canales > 2000 {
            break r;
        }
        canales += 1;
    };
    if fin != E_LIMITE {
        return 23;
    }
    // 24: una cadena de canales, cada uno con los dos extremos del siguiente en su cola (hallazgo 3). Al cerrar
    // el primero se suelta toda; por recursión desbordaba la pila del kernel. Primero se vacía la tabla del paso 23.
    for k in 1..=1100u64 {
        x0(6, [k, 0, 0, 0, 0]);
    }
    // Desde N1 (cierre de la fase 1), los canales vivos de un proceso tienen límite aunque no estén en su tabla: la
    // cadena para en E_LIMITE, hacia los 500.
    let (mut a, mut b) = dashos::canal_crear();
    let mut largo = 1;
    let fin = loop {
        let r = dashos::llamada(3, [0; 5]);
        if (r.0 as i64) < 0 || largo > 5000 {
            break r.0 as i64;
        }
        let (c, d) = (r.0 as u32, r.1 as u32);
        if dashos::canal_escribir(c, b"cadena", &[a, b]) != 0 {
            return 24;
        }
        (a, b) = (c, d);
        largo += 1;
    };
    if fin != E_LIMITE {
        return 26;
    }
    x0(6, [a as u64, 0, 0, 0, 0]);
    x0(6, [b as u64, 0, 0, 0, 0]);
    // 27: PAN por software (C34). Dentro del kernel, fuera de las copias, la memoria del proceso no se ve.
    let local = 0u64;
    if x0(99, [&local as *const u64 as u64, 0, 0, 0, 0]) != 0 {
        return 27;
    }
    // 25: una secuencia de escape no llega al terminal: sale "?" en lugar del ESC.
    dashos::escribir("  [malo] escape: \x1b[31m\n");
    println!("  [malo] memoria agotada en {} trozos de 4 MB, tabla llena en {} canales y cadena de {} canales, sin pánico", n, canales, largo);
    println!("  [malo] punteros malos, llaves repetidas y canales cerrados: todo rechazado sin perder nada");
    31
}
