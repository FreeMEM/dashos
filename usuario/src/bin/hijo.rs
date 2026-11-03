//! C39: el hijo que lanza `padre`. Lee una orden en su llave 1 y la cumple:
//! 1, N: sale con el código N;
//! 2: gira para siempre en EL0 (para matarlo en otro núcleo);
//! 3: falla a propósito (lee la dirección 8);
//! 4: se come la memoria a trozos de 64 KB y sale con cuántos le dieron (250 si no acaba en E_MEMORIA);
//! 5: intenta un subgrupo de 2 MB y sale con 0 si le dicen E_MEMORIA (C42);
//! 6: deja un ciclo de dos canales, cada uno con un lado del otro en su cola, y sale con 0 (C44);
//! 7: espera al proceso de su llave 2 y sale con la clase de su fin (C44);
//! 8, T (8 bytes): crea sin parar hijos que giran, con el ELF de su llave 2 (de T bytes), hasta que no le dejan (C44).
//! Si no le llega nada, se queda bloqueado leyendo (para matarlo bloqueado).

#![no_std]
#![no_main]

dashos::programa!(principal);

fn principal() -> u64 {
    let mut buf = [0u8; 16];
    let mut llaves = [0u32; 1];
    let (n, _) = dashos::canal_leer(1, &mut buf, &mut llaves);
    if n < 1 {
        return 200;
    }
    match buf[0] {
        1 => buf[1] as u64,
        2 => loop {
            core::hint::spin_loop();
        },
        3 => unsafe { core::ptr::read_volatile(8 as *const u64) },
        4 => {
            let mut n = 0;
            loop {
                let k = dashos::llamada(7, [64 * 1024, 0, 0, 0, 0]).0 as i64;
                if k < 0 {
                    return if k == -7 { n } else { 250 };
                }
                n += 1;
            }
        }
        5 => if dashos::grupo_crear(2 << 20) == -7 { 0 } else { 251 },
        6 => {
            let (a0, a1) = dashos::canal_crear();
            let (b0, b1) = dashos::canal_crear();
            // b1 va a la cola de a1, y a1 a la de b1: ninguno se suelta ya, salvo que maten al grupo.
            if dashos::canal_escribir(a0, b"b1", &[b1]) != 0 || dashos::canal_escribir(b0, b"a1", &[a1]) != 0 {
                return 252;
            }
            0
        }
        7 => dashos::proceso_esperar(2).0 as u64,
        8 => {
            let tamano = u64::from_le_bytes([buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7], buf[8]]) as usize;
            loop {
                let (a, b) = dashos::canal_crear();
                if (a as i32) < 0 || dashos::canal_escribir(a, &[2], &[]) != 0 || dashos::proceso_crear(2, tamano, &[b]) < 0 {
                    // Sin cupo o con el grupo muerto: a esperar a que lo maten.
                    loop {
                        core::hint::spin_loop();
                    }
                }
            }
        }
        _ => 201,
    }
}
