//! C39: el hijo que lanza `padre`. Lee una orden en su llave 1 y la cumple:
//! 1, N: sale con el código N;
//! 2: gira para siempre en EL0 (para matarlo en otro núcleo);
//! 3: falla a propósito (lee la dirección 8);
//! 4: se come la memoria a trozos de 64 KB y sale con cuántos le dieron (250 si no acaba en E_MEMORIA);
//! 5, T (8 bytes): crea un subgrupo de 64 MB (más que el propio: desde S3 los cupos son topes), lanza ahí un nieto que
//! se come la memoria (4), con el ELF de su llave 2 (de T bytes), y sale con los trozos que le dieron al nieto;
//! 6: deja un ciclo de dos canales, cada uno con un lado del otro en su cola, y sale con 0 (C44);
//! 7: espera al proceso de su llave 2 y sale con la clase de su fin (C44);
//! 8, T (8 bytes): crea sin parar hijos que giran, con el ELF de su llave 2 (de T bytes), hasta que no le dejan (C44);
//! 9: proyecta memoria, la quita y la lee: tiene que morir por fallo (C45);
//! 10: proyecta la memoria de su llave 2, escribe 1 al principio y la lee sin parar: muere por fallo cuando la
//! revocan (C45).
//! 11, N: crea y suelta N objetos de memoria de 4 MB, uno tras otro, y sale con 0 (S2: llamadas largas).
//! 12, N: duerme N veces 500 µs y sale con lo tarde que despertó, en µs: mediana, p90, p99 y máximo (S2).
//! 13, T: como 5, pero el nieto se come la memoria y gira (14), y el hijo lo espera (S3: matar el grupo con todo).
//! 14: se come la memoria a trozos de 64 KB y gira (S3).
//! 16: intenta subir a 1 TB el tope del grupo de su llave 2 (el suyo) y sale con el error en positivo (S3).
//! 15: se come la memoria a trozos de 16 MB (el máximo) y sale con cuántos le dieron (250 si no acaba en E_MEMORIA; S3).
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
        5 | 13 => {
            let tamano = u64::from_le_bytes([buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7], buf[8]]) as usize;
            let g = dashos::grupo_crear(64 << 20);
            if g <= 0 {
                return 251;
            }
            let (a, b) = dashos::canal_crear();
            let orden = if buf[0] == 5 { 4 } else { 14 };
            if dashos::canal_escribir(a, &[orden], &[]) != 0 {
                return 252;
            }
            let p = dashos::proceso_crear_en(g as u32, 2, tamano, &[b]);
            if p <= 0 {
                return 253;
            }
            match dashos::proceso_esperar(p as u32) {
                (0, trozos) => trozos,
                _ => 254,
            }
        }
        14 => {
            while (dashos::llamada(7, [64 * 1024, 0, 0, 0, 0]).0 as i64) > 0 {}
            loop {
                core::hint::spin_loop();
            }
        }
        16 => (-dashos::grupo_tope(2, 1 << 40)) as u64,
        15 => {
            let mut n = 0;
            loop {
                let k = dashos::llamada(7, [16 << 20, 0, 0, 0, 0]).0 as i64;
                if k < 0 {
                    return if k == -7 { n } else { 250 };
                }
                n += 1;
            }
        }
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
        12 => {
            let reloj = || {
                let t: u64;
                unsafe { core::arch::asm!("isb", "mrs {}, cntvct_el0", out(reg) t) };
                t
            };
            let fr: u64;
            unsafe { core::arch::asm!("mrs {}, cntfrq_el0", out(reg) fr) };
            let mut r = [0u64; 255];
            let n = buf[1] as usize;
            for x in r.iter_mut().take(n) {
                let t = reloj();
                dashos::dormir_us(500);
                *x = ((reloj() - t) * 1_000_000 / fr).saturating_sub(500);
            }
            let r = &mut r[..n.max(1)];
            r.sort_unstable();
            let p = |q: usize| r[(r.len() - 1) * q / 100].min(0xffff);
            // Mediana, percentil 90, percentil 99 y máximo, 16 bits cada uno.
            p(50) | p(90) << 16 | p(99) << 32 | p(100) << 48
        }
        11 => {
            for _ in 0..buf[1] {
                let m = dashos::memoria_crear(4 << 20);
                if (m as i32) < 0 {
                    return 255;
                }
                dashos::cerrar(m);
            }
            0
        }
        9 => {
            let va = 0x30_0000_0000usize;
            let m = dashos::memoria_crear(4096);
            if dashos::memoria_proyectar(m, va, true) != 0 || dashos::memoria_quitar(va) != 0 {
                return 253;
            }
            unsafe { core::ptr::read_volatile(va as *const u64) }
        }
        10 => {
            let va = 0x30_0000_0000usize;
            if dashos::memoria_proyectar(2, va, true) != 0 {
                return 254;
            }
            unsafe { core::ptr::write_volatile(va as *mut u64, 1) };
            loop {
                unsafe { core::ptr::read_volatile((va + 8) as *const u64) };
            }
        }
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
