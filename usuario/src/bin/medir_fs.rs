//! Banco del sistema de archivos (paso F de docs/MULTITAREA_LOOP.md): las mismas operaciones que
//! `tools/medir_linux/fs.c` en Linux, por el protocolo de ficheros del servicio `archivos` (RedoxFS en el NVMe de QEMU
//! virt). La llave 1 es el canal "archivos". Cada resultado, una línea `[medir] fs op=... valor=... unidad=...`.
//!
//! El servicio guarda en el disco cada orden que cambia algo antes de contestar: lo que escribe aquí equivale a las
//! pruebas "sync" de Linux (fsync tras cada escritura o cada fichero).

#![no_std]
#![no_main]

#[path = "../medir_comun.rs"]
#[macro_use]
mod medir_comun;

use dashos::println;
use medir_comun::*;

dashos::programa!(principal);

const VA: usize = 0x40_0000_0000;
const SESION: u32 = 0;
const BUSCAR: u32 = 1;
const LEER: u32 = 2;
const ESCRIBIR: u32 = 3;
const LISTAR: u32 = 4;
const DATOS: u32 = 5;
const CERRAR: u32 = 6;
const CREAR: u32 = 7;
const BORRAR: u32 = 8;

const GRANDE: u64 = 8 << 20;
const TROZO: u64 = 64 << 10;
const PEQUENOS: u64 = 500;

#[derive(Clone, Copy)]
struct R {
    estado: i32,
    asa: u32,
    a: u64,
}

struct Cliente {
    lectura: u32,
}

impl Cliente {
    fn leer_respuesta(&self, llaves: &mut [u32]) -> Option<(R, usize)> {
        let mut b = [0u8; 32];
        let (n, k) = dashos::canal_leer(self.lectura, &mut b, llaves);
        if n != 32 {
            return None;
        }
        let u32_en = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
        let u64_en = |i: usize| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
        Some((R { estado: u32_en(0) as i32, asa: u32_en(4), a: u64_en(8) }, k))
    }

    fn pedir(&self, orden: u32, asa: u32, a: u64, b: u64, nombre: &[u8]) -> R {
        let mut p = [0u8; 96];
        p[0..4].copy_from_slice(&orden.to_le_bytes());
        p[4..8].copy_from_slice(&asa.to_le_bytes());
        p[8..16].copy_from_slice(&a.to_le_bytes());
        p[16..24].copy_from_slice(&b.to_le_bytes());
        p[24..28].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
        p[32..32 + nombre.len()].copy_from_slice(nombre);
        let malo = R { estado: i32::MIN, asa: 0, a: 0 };
        if dashos::canal_escribir(1, &p[..32 + nombre.len()], &[]) != 0 {
            return malo;
        }
        self.leer_respuesta(&mut []).map_or(malo, |r| r.0)
    }
}

fn ventana() -> &'static mut [u8] {
    unsafe { core::slice::from_raw_parts_mut(VA as *mut u8, TROZO as usize) }
}

/// Valor con un decimal, como `fs.c`.
fn dato(op: &str, por_segundo: u64, cuantos: u64, t: u64, unidad: &str) {
    let us = a_us(t).max(1);
    let v10 = cuantos * 10_000_000 / por_segundo.max(1) / us;
    println!("[medir] fs op={} valor={}.{} unidad={}", op, v10 / 10, v10 % 10, unidad);
}

/// "f0042" en un búfer.
fn nombre(i: u64, b: &mut [u8; 5]) -> &[u8] {
    b[0] = b'f';
    let mut x = i;
    for k in (1..5).rev() {
        b[k] = b'0' + (x % 10) as u8;
        x /= 10;
    }
    &b[..]
}

fn principal() -> u64 {
    println!("[medir] empieza");
    let (lectura, vuelta) = dashos::canal_crear();
    let c = Cliente { lectura };
    let mut p = [0u8; 32];
    p[0..4].copy_from_slice(&SESION.to_le_bytes());
    let mut ll = [0u32; 1];
    if dashos::canal_escribir(1, &p, &[vuelta]) != 0
        || !c.leer_respuesta(&mut ll).is_some_and(|(r, k)| r.estado == 0 && k == 1)
        || dashos::memoria_proyectar(ll[0], VA, true) != 0
    {
        println!("  [medir] sin servicio de archivos\n[medir] fin");
        return 1;
    }
    for (i, b) in ventana().iter_mut().enumerate() {
        *b = (i * 7) as u8;
    }
    let mb = 1u64 << 20;

    // Un fichero de 8 MB por trozos de 64 KB, y leerlo dos veces.
    let t0 = cuentas();
    let f = c.pedir(CREAR, 0, 1, 0, b"grande");
    let mut bien = f.estado == 0;
    let mut off = 0;
    while bien && off < GRANDE {
        bien = c.pedir(ESCRIBIR, f.asa, off, TROZO, b"").estado == 0;
        off += TROZO;
    }
    dato("escribir_8mb_sync", mb, GRANDE, cuentas() - t0, "MB/s");
    for op in ["leer_8mb_frio", "leer_8mb"] {
        let t0 = cuentas();
        let mut off = 0;
        while off < GRANDE {
            let r = c.pedir(LEER, f.asa, off, TROZO, b"");
            if r.estado != 0 || r.a == 0 {
                bien = false;
                break;
            }
            off += r.a;
        }
        dato(op, mb, GRANDE, cuentas() - t0, "MB/s");
    }
    c.pedir(CERRAR, f.asa, 0, 0, b"");

    // 500 ficheros de 4 KB en un cajón: crear y escribir, leer, sus datos, listar y borrar.
    let dir = c.pedir(CREAR, 0, 2, 0, b"peq");
    let mut nb = [0u8; 5];
    let t0 = cuentas();
    for i in 0..PEQUENOS {
        let g = c.pedir(CREAR, dir.asa, 1, 0, nombre(i, &mut nb));
        bien &= g.estado == 0 && c.pedir(ESCRIBIR, g.asa, 0, 4096, b"").estado == 0;
        c.pedir(CERRAR, g.asa, 0, 0, b"");
    }
    dato("crear_4k_sync", 1, PEQUENOS, cuentas() - t0, "ficheros/s");
    let t0 = cuentas();
    for i in 0..PEQUENOS {
        let g = c.pedir(BUSCAR, dir.asa, 0, 0, nombre(i, &mut nb));
        bien &= g.estado == 0 && c.pedir(LEER, g.asa, 0, 4096, b"").a == 4096;
        c.pedir(CERRAR, g.asa, 0, 0, b"");
    }
    dato("leer_4k", 1, PEQUENOS, cuentas() - t0, "ficheros/s");
    let t0 = cuentas();
    for i in 0..PEQUENOS {
        let g = c.pedir(BUSCAR, dir.asa, 0, 0, nombre(i, &mut nb));
        bien &= g.estado == 0 && c.pedir(DATOS, g.asa, 0, 0, b"").a == 4096;
        c.pedir(CERRAR, g.asa, 0, 0, b"");
    }
    dato("datos", 1, PEQUENOS, cuentas() - t0, "ops/s");
    let t0 = cuentas();
    let l = c.pedir(LISTAR, dir.asa, 0, 0, b"");
    dato("listar_500", 1, 1, cuentas() - t0, "listados/s");
    let listados = l.a;
    let t0 = cuentas();
    for i in 0..PEQUENOS {
        bien &= c.pedir(BORRAR, dir.asa, 0, 0, nombre(i, &mut nb)).estado == 0;
    }
    dato("borrar_sync", 1, PEQUENOS, cuentas() - t0, "ficheros/s");
    println!("[medir] fs-bien bien={} listados={}", bien as u8, listados);
    // Dónde se fue el tiempo dentro del servicio (orden 98): "orden=veces/µs" y lo mismo del disco.
    let r = c.pedir(98, 0, 0, 0, b"");
    if r.estado == 0 {
        let v = &ventana()[..(r.a as usize).min(4096)];
        println!("[medir] fs-perfil {}", core::str::from_utf8(v).unwrap_or("?"));
    }
    println!("[medir] fin");
    0
}
