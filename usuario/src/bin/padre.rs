//! C39: crea, mata y espera procesos. Llave 1: un objeto de memoria con el ELF de `hijo`; llave 2: un canal por el que
//! llega su tamaño (8 bytes). Sale con 39 si todo va bien, o con el número del paso que falló.

#![no_std]
#![no_main]

use dashos::{llamada, println};

dashos::programa!(principal);

const E_ARGUMENTO: i64 = -1;
const E_LLAVE: i64 = -2;
const E_TAMANO: i64 = -5;
const E_TIPO: i64 = -6;
const E_CERRADO: i64 = -4;

fn hijo(tamano: usize, orden: &[u8]) -> (i64, u32) {
    let (a, b) = dashos::canal_crear();
    if !orden.is_empty() && dashos::canal_escribir(a, orden, &[]) != 0 {
        return (-100, a);
    }
    (dashos::proceso_crear(1, tamano, &[b]), a)
}

fn esperar_ms(ms: u64) {
    let (t, f): (u64, u64);
    unsafe { core::arch::asm!("isb", "mrs {}, cntvct_el0", "mrs {}, cntfrq_el0", out(reg) t, out(reg) f) };
    let fin = t + f * ms / 1000;
    loop {
        let a: u64;
        unsafe { core::arch::asm!("isb", "mrs {}, cntvct_el0", out(reg) a) };
        if a >= fin {
            return;
        }
    }
}

fn principal() -> u64 {
    let mut buf = [0u8; 8];
    let mut llaves = [0u32; 1];
    if dashos::canal_leer(2, &mut buf, &mut llaves).0 != 8 {
        return 1;
    }
    let tamano = u64::from_le_bytes(buf) as usize;
    // 2: un hijo que sale con 7.
    let (p, _) = hijo(tamano, &[1, 7]);
    if p <= 0 {
        return 2;
    }
    if dashos::proceso_esperar(p as u32) != (0, 7) {
        return 3;
    }
    // 4: esperar otra vez a uno que ya acabó da lo mismo, al momento.
    if dashos::proceso_esperar(p as u32) != (0, 7) {
        return 4;
    }
    // 5: uno que gira en EL0 (en otro núcleo): matarlo y esperarlo.
    let (p, _) = hijo(tamano, &[2]);
    if p <= 0 {
        return 5;
    }
    esperar_ms(30);
    if dashos::proceso_matar(p as u32) != 0 || dashos::proceso_esperar(p as u32).0 != 1 {
        return 6;
    }
    // 7: uno bloqueado leyendo un canal vacío: matarlo y esperarlo.
    let (p, _) = hijo(tamano, &[]);
    if p <= 0 {
        return 7;
    }
    esperar_ms(30);
    if dashos::proceso_matar(p as u32) != 0 || dashos::proceso_esperar(p as u32).0 != 1 {
        return 8;
    }
    // 9: uno que falla.
    let (p, _) = hijo(tamano, &[3]);
    if p <= 0 || dashos::proceso_esperar(p as u32).0 != 2 {
        return 9;
    }
    // 10-13: lo hostil. Un ELF que no es ELF (los 8 bytes del tamaño en un objeto nuevo), un tamaño mayor que el
    // objeto, una llave que no es de memoria y una llave de proceso que no existe.
    let basura = dashos::memoria_crear(4096);
    if dashos::proceso_crear(basura, 64, &[]) != E_ARGUMENTO {
        return 10;
    }
    if dashos::proceso_crear(1, 1 << 30, &[]) != E_TAMANO {
        return 11;
    }
    if dashos::proceso_crear(2, 64, &[]) != E_TIPO {
        return 12;
    }
    if llamada(13, [999, 0, 0, 0, 0]).0 as i64 != E_LLAVE {
        return 13;
    }
    // 14: matarse a sí mismo no: no tiene su propia llave. Matar a un hijo dos veces y esperar sigue valiendo.
    let (p, _) = hijo(tamano, &[]);
    if dashos::proceso_matar(p as u32) != 0 || dashos::proceso_matar(p as u32) != 0 || dashos::proceso_esperar(p as u32).0 != 1 {
        return 14;
    }
    // C40. 15: el hijo tiene el otro lado de un canal; al salir, el lado se cierra y el padre, que lee, se entera.
    let (p, a) = hijo(tamano, &[1, 5]);
    if p <= 0 || dashos::canal_leer(a, &mut buf, &mut llaves).0 != E_CERRADO {
        return 15;
    }
    if dashos::canal_escribir(a, b"x", &[]) != E_CERRADO || dashos::proceso_esperar(p as u32) != (0, 5) {
        return 16;
    }
    // 17: lo mismo matándolo mientras está bloqueado leyendo.
    let (p, a) = hijo(tamano, &[]);
    esperar_ms(20);
    if dashos::proceso_matar(p as u32) != 0 || dashos::proceso_esperar(p as u32).0 != 1 {
        return 17;
    }
    if dashos::canal_escribir(a, b"x", &[]) != E_CERRADO {
        return 18;
    }
    // 19-20: sin ciclos. Ni la pareja ni el propio lado por su canal.
    let (x, y) = dashos::canal_crear();
    if dashos::canal_escribir(x, b"c", &[y]) != E_ARGUMENTO || dashos::canal_escribir(x, b"c", &[x]) != E_ARGUMENTO {
        return 19;
    }
    // 21: un lado que se pierde dentro de un mensaje que nadie leerá. Se manda z0 por x hacia y, y se cierra y: el
    // mensaje se suelta, z0 con él, y quien lea en z1 se entera.
    let (z0, z1) = dashos::canal_crear();
    if dashos::canal_escribir(x, b"z", &[z0]) != 0 || llamada(6, [y as u64, 0, 0, 0, 0]).0 != 0 {
        return 20;
    }
    if dashos::canal_leer(z1, &mut buf, &mut llaves).0 != E_CERRADO {
        return 21;
    }
    // 22-23: dos hijos leen el mismo lado (con una copia de la llave); llegan dos mensajes y despiertan los dos
    // (auditoría de la fase 3, M3: antes uno se quedaba colgado).
    let (a, b) = dashos::canal_crear();
    let b2 = dashos::llave_duplicar(b, dashos::LEER | dashos::TRANSFERIR, 0);
    let p1 = dashos::proceso_crear(1, tamano, &[b]);
    let p2 = dashos::proceso_crear(1, tamano, &[b2 as u32]);
    if b2 <= 0 || p1 <= 0 || p2 <= 0 {
        return 22;
    }
    esperar_ms(30);
    if dashos::canal_escribir(a, &[1, 3], &[]) != 0 || dashos::canal_escribir(a, &[1, 4], &[]) != 0 {
        return 23;
    }
    let (c1, c2) = (dashos::proceso_esperar(p1 as u32), dashos::proceso_esperar(p2 as u32));
    if c1.0 != 0 || c2.0 != 0 || c1.1 + c2.1 != 7 {
        return 24;
    }
    println!("  [padre] hijos que salen, giran, se bloquean y fallan: creados, matados y esperados");
    println!("  [padre] lados cerrados al morir, sin ciclos, y lados perdidos en mensajes cerrados");
    39
}
