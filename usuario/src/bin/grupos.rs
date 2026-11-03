//! C42: cupos por grupo. Llave 1: el ELF de `hijo` en un objeto de memoria; llave 2: un canal con su tamaño. Sale con
//! 42 si todo va bien, o con el número del paso que falló.

#![no_std]
#![no_main]

use dashos::{canal_crear, canal_escribir, grupo_crear, grupo_matar, grupo_uso, println, proceso_crear_en, proceso_esperar};

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
    println!("  [grupos] cupo agotado en {} trozos de 64 KB, devuelto entero, y grupos que se matan", trozos);
    42
}
