//! El controlador de discos NVMe (U6d), fuera del kernel. Escrito con la especificación NVM Express Base 2.0 (registros
//! del controlador, cap. 3.1; colas, cap. 4; órdenes de administración, cap. 5) y la del juego de órdenes NVM (Read).
//!
//! Llaves: la 1, su permiso DMA; la 2, sus registros (la BAR 0).
//!
//! Reinicia el controlador, monta las colas de administración, pide sus datos (Identify) y los del espacio de nombres
//! 1, crea una cola de E/S y lee los dos primeros sectores. Sin interrupciones: mira la cola de terminadas girando
//! (solo al arrancar). Toda su memoria es un objeto DMA del permiso, sin caché; su dirección de bus es la física.
//! Con páginas de 4 KB (CC.MPS = 0): cada búfer cabe en una página y basta PRP1.

#![no_std]
#![no_main]

use dashos::println;

dashos::programa!(principal);

const VA: usize = 0x0090_0000;

// Registros (3.1).
const CAP: usize = 0x00;
const VS: usize = 0x08;
const CC: usize = 0x14;
const CSTS: usize = 0x1C;
const AQA: usize = 0x24;
const ASQ: usize = 0x28;
const ACQ: usize = 0x30;
const TIMBRES: usize = 0x1000;
const CC_EN: u32 = 1 << 0;
/// Entradas de 64 bytes en las colas de envío (2^6) y de 16 en las de terminadas (2^4).
const CC_TAMANOS: u32 = (6 << 16) | (4 << 20);
const CSTS_RDY: u32 = 1 << 0;
const CSTS_CFS: u32 = 1 << 1;

// Memoria (un objeto DMA de 64 KB): dónde va cada cosa, cada una en su página.
const VA_DMA: usize = 0x30_0000_0000;
const TAM_DMA: usize = 64 * 1024;
const M_ADM_SQ: usize = 0x0000;
const M_ADM_CQ: usize = 0x1000;
const M_ES_SQ: usize = 0x2000;
const M_ES_CQ: usize = 0x3000;
const M_DATOS: usize = 0x4000;
const ENTRADAS: usize = 16;

// Órdenes (5 y NVM).
const O_CREAR_SQ: u8 = 0x01;
const O_CREAR_CQ: u8 = 0x05;
const O_IDENTIFICAR: u8 = 0x06;
const O_LEER: u8 = 0x02;

fn leer(off: usize) -> u32 {
    u32::from_le(unsafe { core::ptr::read_volatile((VA + off) as *const u32) })
}

fn escribir(off: usize, v: u32) {
    unsafe { core::ptr::write_volatile((VA + off) as *mut u32, v.to_le()) }
}

fn escribir64(off: usize, v: u64) {
    escribir(off, v as u32);
    escribir(off + 4, (v >> 32) as u32);
}

fn dma8(off: usize) -> u8 {
    unsafe { core::ptr::read_volatile((VA_DMA + off) as *const u8) }
}

fn dma32(off: usize) -> u32 {
    u32::from_le(unsafe { core::ptr::read_volatile((VA_DMA + off) as *const u32) })
}

fn poner32(off: usize, v: u32) {
    unsafe { core::ptr::write_volatile((VA_DMA + off) as *mut u32, v.to_le()) }
}

/// Espera a que se cumpla `cond`, como mucho `ms` milisegundos (girando: solo al arrancar).
fn esperar(ms: u64, cond: impl Fn() -> bool) -> bool {
    let fin = dashos::ahora_us() + ms * 1000;
    while dashos::ahora_us() < fin {
        if cond() {
            return true;
        }
    }
    cond()
}

/// Una pareja de colas (envío y terminadas) con su número.
struct Cola {
    id: usize,
    sq: usize,
    cq: usize,
    cola: usize,
    cabeza: usize,
    fase: u32,
    cid: u16,
}

struct Nvme {
    paso: usize, // la separación de los timbres (4 << CAP.DSTRD)
    espera_ms: u64,
}

impl Nvme {
    /// Manda una orden por `c` y espera a que termine: su estado (0 = bien) y su DW0.
    fn orden(&self, c: &mut Cola, opcode: u8, nsid: u32, prp1: u64, cdw: [u32; 6]) -> Result<u32, u32> {
        let e = c.sq + 64 * c.cola;
        for i in 0..16 {
            poner32(e + 4 * i, 0);
        }
        c.cid = c.cid.wrapping_add(1);
        poner32(e, opcode as u32 | (c.cid as u32) << 16);
        poner32(e + 4, nsid);
        poner32(e + 24, prp1 as u32);
        poner32(e + 28, (prp1 >> 32) as u32);
        for (i, v) in cdw.iter().enumerate() {
            poner32(e + 40 + 4 * i, *v);
        }
        unsafe { core::arch::asm!("dmb oshst") };
        c.cola = (c.cola + 1) % ENTRADAS;
        escribir(TIMBRES + (2 * c.id) * self.paso, c.cola as u32);
        // La terminada llega cuando su bit de fase (DW3, bit 16) es el que se espera (4.6).
        let t = c.cq + 16 * c.cabeza;
        let fase = c.fase;
        if !esperar(self.espera_ms, || (dma32(t + 12) >> 16) & 1 == fase) {
            return Err(u32::MAX);
        }
        unsafe { core::arch::asm!("dmb oshld") };
        let (dw0, dw3) = (dma32(t), dma32(t + 12));
        c.cabeza = (c.cabeza + 1) % ENTRADAS;
        if c.cabeza == 0 {
            c.fase ^= 1;
        }
        escribir(TIMBRES + (2 * c.id + 1) * self.paso, c.cabeza as u32);
        let estado = (dw3 >> 17) & 0x7fff;
        if dw3 & 0xffff != c.cid as u32 || estado != 0 {
            return Err(estado);
        }
        Ok(dw0)
    }
}

/// Los `n` bytes de texto de `off` en la memoria DMA, sin los espacios del final (los de Identify van rellenos).
fn texto(off: usize, n: usize, b: &mut [u8; 64]) -> &str {
    let mut l = 0;
    for i in 0..n.min(64) {
        let c = dma8(off + i);
        b[i] = if (0x20..0x7f).contains(&c) { c } else { b'?' };
        if c != b' ' && c != 0 {
            l = i + 1;
        }
    }
    core::str::from_utf8(&b[..l]).unwrap_or("?")
}

fn principal() -> u64 {
    if dashos::registros_proyectar(2, VA) != 0 {
        println!("  [nvme] sin registros");
        return 1;
    }
    let cap = (leer(CAP) as u64) | (leer(CAP + 4) as u64) << 32;
    let vs = leer(VS);
    let n = Nvme { paso: 4 << ((cap >> 32) & 0xf), espera_ms: 500 * ((cap >> 24) & 0xff).max(1) };
    if (cap >> 48) & 0xf != 0 {
        println!("  [nvme] no admite páginas de 4 KB");
        return 2;
    }
    // Apagar (por si el firmware lo dejó encendido) y esperar a que deje de estar listo (3.5.2).
    escribir(CC, leer(CC) & !CC_EN);
    if !esperar(n.espera_ms, || leer(CSTS) & CSTS_RDY == 0) {
        println!("  [nvme] no se apaga");
        return 3;
    }
    // Con el controlador apagado, lo que dejara en cuarentena uno anterior puede volver (M5).
    dashos::dma_liberar(1);
    let (m, bus) = dashos::memoria_dma(1, TAM_DMA);
    if m <= 0 || dashos::memoria_proyectar(m as u32, VA_DMA, true) != 0 {
        println!("  [nvme] sin memoria DMA");
        return 4;
    }
    for i in (0..TAM_DMA).step_by(4) {
        poner32(i, 0);
    }
    // Colas de administración: tamaños (menos uno), direcciones, y encender (3.5.1).
    let q = (ENTRADAS - 1) as u32;
    escribir(AQA, q | q << 16);
    escribir64(ASQ, bus + M_ADM_SQ as u64);
    escribir64(ACQ, bus + M_ADM_CQ as u64);
    escribir(CC, CC_TAMANOS | CC_EN);
    if !esperar(n.espera_ms, || leer(CSTS) & (CSTS_RDY | CSTS_CFS) != 0) || leer(CSTS) & CSTS_CFS != 0 {
        println!("  [nvme] no se enciende");
        return 5;
    }
    let mut adm = Cola { id: 0, sq: M_ADM_SQ, cq: M_ADM_CQ, cola: 0, cabeza: 0, fase: 1, cid: 0 };
    let datos = bus + M_DATOS as u64;
    // Identify del controlador (CNS 1): modelo en los bytes 24 a 63.
    if n.orden(&mut adm, O_IDENTIFICAR, 0, datos, [1, 0, 0, 0, 0, 0]).is_err() {
        println!("  [nvme] Identify falla");
        return 6;
    }
    let mut b = [0u8; 64];
    println!("  [nvme] versión {}.{}, modelo \"{}\"", vs >> 16, (vs >> 8) & 0xff, texto(M_DATOS + 24, 40, &mut b));
    // Identify del espacio de nombres 1 (CNS 0): sectores (NSZE) y su tamaño (el formato que dice FLBAS).
    if n.orden(&mut adm, O_IDENTIFICAR, 1, datos, [0, 0, 0, 0, 0, 0]).is_err() {
        println!("  [nvme] sin espacio de nombres 1");
        return 7;
    }
    let sectores = dma32(M_DATOS) as u64 | (dma32(M_DATOS + 4) as u64) << 32;
    let formato = (dma8(M_DATOS + 26) & 0xf) as usize;
    let tam_sector = 1usize << ((dma32(M_DATOS + 128 + 4 * formato) >> 16) & 0xff);
    println!("  [nvme] disco de {} sectores de {} bytes ({} KB)", sectores, tam_sector, sectores * tam_sector as u64 / 1024);
    if tam_sector > 2048 {
        println!("  [nvme] sectores demasiado grandes");
        return 8;
    }
    // Una cola de E/S, la 1: primero la de terminadas (sin interrupción) y luego la de envío (5.4, 5.5).
    let tam = q << 16 | 1;
    if n.orden(&mut adm, O_CREAR_CQ, 0, bus + M_ES_CQ as u64, [tam, 1, 0, 0, 0, 0]).is_err()
        || n.orden(&mut adm, O_CREAR_SQ, 0, bus + M_ES_SQ as u64, [tam, 1 << 16 | 1, 0, 0, 0, 0]).is_err()
    {
        println!("  [nvme] no crea la cola de E/S");
        return 9;
    }
    let mut es = Cola { id: 1, sq: M_ES_SQ, cq: M_ES_CQ, cola: 0, cabeza: 0, fase: 1, cid: 0 };
    // Los dos primeros sectores (NLB va menos uno).
    if let Err(e) = n.orden(&mut es, O_LEER, 1, datos, [0, 0, 1, 0, 0, 0]) {
        println!("  [nvme] la lectura falla (estado {:#x})", e);
        return 10;
    }
    for s in 0..2 {
        let off = M_DATOS + s * tam_sector;
        let firma = dma8(off + 510) as u16 | (dma8(off + 511) as u16) << 8;
        let l = (0..64).find(|&i| dma8(off + i) == 0).unwrap_or(64);
        println!("  [nvme] sector {}: \"{}\", firma {:#06x}", s, texto(off, l, &mut b), firma);
    }
    0
}
