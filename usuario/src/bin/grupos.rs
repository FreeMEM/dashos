//! C42: cupos por grupo. Llave 1: el ELF de `hijo` en un objeto de memoria; llave 2: un canal con su tamaño. Sale con
//! 42 si todo va bien, o con el número del paso que falló.

#![no_std]
#![no_main]

use dashos::{canal_crear, canal_escribir, grupo_crear, grupo_matar, grupo_uso, println, proceso_crear_en, proceso_esperar};

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

dashos::programa!(principal);

const MB: u64 = 1 << 20;

fn hijo_en(grupo: u32, tamano: usize, orden: &[u8]) -> i64 {
    let (a, b) = canal_crear();
    if !orden.is_empty() && canal_escribir(a, orden, &[]) != 0 {
        return -100;
    }
    proceso_crear_en(grupo, 1, tamano, &[b])
}

fn principal() -> u64 {
    let mut buf = [0u8; 8];
    let mut llaves = [0u32; 1];
    if dashos::canal_leer(2, &mut buf, &mut llaves).0 != 8 {
        return 1;
    }
    let tamano = u64::from_le_bytes(buf) as usize;
    // 2: un grupo de 1 MB, vacío al empezar.
    let g = grupo_crear(MB);
    if g <= 0 || grupo_uso(g as u32) != (MB as i64, 0) {
        return 2;
    }
    let g = g as u32;
    // 3: un hijo que se come la memoria dentro del grupo: le dan algo, menos de 1 MB, y acaba en E_MEMORIA.
    let p = hijo_en(g, tamano, &[4]);
    if p <= 0 {
        return 3;
    }
    let (clase, trozos) = proceso_esperar(p as u32);
    if clase != 0 || trozos == 0 || trozos >= 16 {
        return 4;
    }
    // 5: al acabar el hijo, todo lo cobrado vuelve al grupo.
    if grupo_uso(g).1 != 0 {
        return 5;
    }
    // 6: dentro del grupo de 1 MB no cabe un subgrupo de 2 MB.
    let p = hijo_en(g, tamano, &[5]);
    if p <= 0 || proceso_esperar(p as u32) != (0, 0) {
        return 6;
    }
    // 7-9: matar un grupo mata a todos sus procesos.
    let g2 = grupo_crear(4 * MB);
    if g2 <= 0 {
        return 7;
    }
    let p1 = hijo_en(g2 as u32, tamano, &[]);
    let p2 = hijo_en(g2 as u32, tamano, &[]);
    if p1 <= 0 || p2 <= 0 || grupo_matar(g2 as u32) != 0 {
        return 8;
    }
    if proceso_esperar(p1 as u32).0 != 1 || proceso_esperar(p2 as u32).0 != 1 {
        return 9;
    }
    // 10: el propio grupo no se ha tocado: aquí aún caben 4 MB.
    if dashos::memoria_crear(4 * MB as usize) as i32 <= 0 {
        return 10;
    }
    // C44. 11-12: un hijo deja un ciclo de dos canales y sale. Lo del ciclo sigue cobrado a su grupo; al matar el
    // grupo, sus canales se cierran y el grupo vuelve a 0.
    let g3 = grupo_crear(MB);
    if g3 <= 0 {
        return 11;
    }
    let p = hijo_en(g3 as u32, tamano, &[6]);
    if p <= 0 || proceso_esperar(p as u32) != (0, 0) || grupo_uso(g3 as u32).1 == 0 {
        return 12;
    }
    if grupo_matar(g3 as u32) != 0 || grupo_uso(g3 as u32).1 != 0 {
        return 13;
    }
    // 14: estrés de A2. Un proceso X gira; seis hijos, en otro grupo, esperan a X con copias de su llave. Se matan a
    // la vez los que esperan y X, diez veces. Si alguien despertara a un hilo ya liberado, el kernel caería.
    for _ in 0..10 {
        let gx = grupo_crear(MB);
        let gw = grupo_crear(2 * MB);
        if gx <= 0 || gw <= 0 {
            return 14;
        }
        let x = hijo_en(gx as u32, tamano, &[2]);
        if x <= 0 {
            return 15;
        }
        for _ in 0..6 {
            let (a, b) = canal_crear();
            let copia = dashos::llave_duplicar(x as u32, dashos::LEER | dashos::TRANSFERIR, 0);
            if canal_escribir(a, &[7], &[]) != 0 || proceso_crear_en(gw as u32, 1, tamano, &[b, copia as u32]) <= 0 {
                return 16;
            }
        }
        esperar_ms(5);
        grupo_matar(gw as u32);
        dashos::proceso_matar(x as u32);
        if proceso_esperar(x as u32).0 != 1 {
            return 17;
        }
        // Soltar las llaves de los grupos y de X: su presupuesto vuelve cuando acaben de morir todos.
        for k in [gx, gw, x] {
            dashos::llamada(6, [k as u64, 0, 0, 0, 0]);
        }
    }
    // 18: estrés de M2. Un hijo crea hijos sin parar en su grupo; se mata el grupo. Si alguno se escapara al nacer
    // durante la matanza, seguiría girando y el grupo no volvería nunca a 0.
    let g4 = grupo_crear(16 * MB);
    if g4 <= 0 {
        return 18;
    }
    let mut orden = [0u8; 9];
    orden[0] = 8;
    orden[1..9].copy_from_slice(&(tamano as u64).to_le_bytes());
    let (a, b) = canal_crear();
    let copia_elf = dashos::llave_duplicar(1, dashos::LEER | dashos::TRANSFERIR, 0);
    if canal_escribir(a, &orden, &[]) != 0 || proceso_crear_en(g4 as u32, 1, tamano, &[b, copia_elf as u32]) <= 0 {
        return 19;
    }
    esperar_ms(20);
    grupo_matar(g4 as u32);
    let mut vuelto = false;
    for _ in 0..200 {
        esperar_ms(10);
        if grupo_uso(g4 as u32).1 == 0 {
            vuelto = true;
            break;
        }
    }
    if !vuelto {
        return 20;
    }
    println!("  [grupos] cupo agotado en {} trozos de 64 KB, devuelto entero, y grupos que se matan", trozos);
    println!("  [grupos] ciclos soltados al matar el grupo, y matanzas a la vez sin caerse");
    42
}
