//! Lo que comparten las pruebas: una caja de 8 MB con una tarea, graphics e intuition instaladas sobre el
//! compositor de prueba, y ayudas para llamar a las funciones como lo haría el JIT.
#![allow(dead_code)]

pub mod puente;

use caja_exec::estructuras::memf;
use caja_exec::{Accion, Config, Exec, Memoria, MemoriaVec, Registros};
use caja_intuition::{CajaGrafica, CompositorDePrueba, Numeros};

pub const TAMANO: u32 = 8 << 20;
pub type Caja = (Exec<MemoriaVec>, CajaGrafica<CompositorDePrueba>, u32);

/// Una caja con una tarea "principal" en marcha y un escritorio de 640 x 480.
pub fn caja() -> Caja {
    let mut e = Exec::arrancar(MemoriaVec::nueva(TAMANO as usize), Config::para(TAMANO)).expect("arranca");
    let t = e.crear_tarea("principal", 0, 4096, 0x10_0000).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(t));
    let g = CajaGrafica::instalar(&mut e, CompositorDePrueba::nuevo(640, 480), Numeros::default()).expect("instala");
    (e, g, t)
}

/// Llama a una función de graphics por su nombre, como lo haría el JIT.
pub fn gfx(c: &mut Caja, nombre: &str, r: &mut Registros) -> Accion {
    let n = c.1.numero_graphics(nombre).expect("función de graphics");
    c.1.llamar(&mut c.0, n, r)
}

/// Llama a una función de intuition por su nombre.
pub fn intui(c: &mut Caja, nombre: &str, r: &mut Registros) -> Accion {
    let n = c.1.numero_intuition(nombre).expect("función de intuition");
    c.1.llamar(&mut c.0, n, r)
}

/// Registros con D0-D7 y A0-A3 dados.
pub fn regs(d: &[u32], a: &[u32]) -> Registros {
    let mut r = Registros::default();
    r.d[..d.len()].copy_from_slice(d);
    r.a[..a.len()].copy_from_slice(a);
    r
}

/// Llama y devuelve D0.
pub fn g(c: &mut Caja, nombre: &str, d: &[u32], a: &[u32]) -> u32 {
    let mut r = regs(d, a);
    gfx(c, nombre, &mut r);
    r.d[0]
}

pub fn i(c: &mut Caja, nombre: &str, d: &[u32], a: &[u32]) -> u32 {
    let mut r = regs(d, a);
    intui(c, nombre, &mut r);
    r.d[0]
}

pub fn reservar(c: &mut Caja, largo: u32) -> u32 {
    let d = c.0.alloc_mem(largo, memf::CLEAR | memf::PUBLIC).unwrap();
    assert_ne!(d, 0);
    d
}

pub fn cadena(c: &mut Caja, s: &str) -> u32 {
    let d = reservar(c, s.len() as u32 + 1);
    c.0.mem.leer_mut(d, s.len() as u32).unwrap().copy_from_slice(s.as_bytes());
    d
}

pub fn l8(c: &Caja, d: u32) -> u8 {
    c.0.mem.leer8(d).unwrap()
}
pub fn l16(c: &Caja, d: u32) -> u16 {
    c.0.mem.leer16(d).unwrap()
}
pub fn l32(c: &Caja, d: u32) -> u32 {
    c.0.mem.leer32(d).unwrap()
}
pub fn e16(c: &mut Caja, d: u32, v: u16) {
    c.0.mem.escribir16(d, v).unwrap()
}
pub fn e32(c: &mut Caja, d: u32, v: u32) {
    c.0.mem.escribir32(d, v).unwrap()
}

/// Un `RastPort` nuevo (con `InitRastPort`) que dibuja en `bm`.
pub fn rastport(c: &mut Caja, bm: u32) -> u32 {
    let rp = reservar(c, 100);
    g(c, "InitRastPort", &[], &[0, rp]);
    e32(c, rp + 4, bm);
    rp
}

/// Un `BitMap` nativo de 32 bits (con `AllocBitMap` de 32 planos) y su `RastPort`.
pub fn nativo(c: &mut Caja, ancho: u32, alto: u32) -> (u32, u32) {
    let bm = g(c, "AllocBitMap", &[ancho, alto, 32, 0], &[0]);
    assert_ne!(bm, 0);
    let rp = rastport(c, bm);
    (bm, rp)
}

/// Un `BitMap` planar montado a mano (InitBitMap y AllocRaster), como hacen los programas, y su `RastPort`.
pub fn planar(c: &mut Caja, prof: u32, ancho: u32, alto: u32) -> (u32, u32) {
    let bm = reservar(c, 40);
    g(c, "InitBitMap", &[prof, ancho, alto], &[bm]);
    for p in 0..prof {
        let r = g(c, "AllocRaster", &[ancho, alto], &[]);
        assert_ne!(r, 0);
        g(c, "BltClear", &[(ancho.div_ceil(16) * 2) * alto, 0], &[0, r]);
        e32(c, bm + 8 + 4 * p, r);
    }
    let rp = rastport(c, bm);
    (bm, rp)
}

/// La pluma de un punto (con `ReadPixel`).
pub fn pluma(c: &mut Caja, rp: u32, x: i32, y: i32) -> i32 {
    g(c, "ReadPixel", &[x as u32, y as u32], &[0, rp]) as i32
}

pub fn pen(c: &mut Caja, rp: u32, a: u32) {
    g(c, "SetAPen", &[a], &[0, rp]);
}

pub fn mover(c: &mut Caja, rp: u32, x: i32, y: i32) {
    g(c, "Move", &[x as u32, y as u32], &[0, rp]);
}

pub fn draw(c: &mut Caja, rp: u32, x: i32, y: i32) {
    g(c, "Draw", &[x as u32, y as u32], &[0, rp]);
}

pub fn rect(c: &mut Caja, rp: u32, x0: i32, y0: i32, x1: i32, y1: i32) {
    g(c, "RectFill", &[x0 as u32, y0 as u32, x1 as u32, y1 as u32], &[0, rp]);
}

/// Los puntos de un mapa con una pluma distinta de 0, como cadenas de `#` y `.` (para ver los dibujos).
pub fn ver(c: &mut Caja, rp: u32, ancho: i32, alto: i32) -> Vec<String> {
    (0..alto).map(|y| (0..ancho).map(|x| if pluma(c, rp, x, y) > 0 { '#' } else { '.' }).collect()).collect()
}

/// Escribe un PNG de color verdadero sin comprimir (bloques "stored" de zlib): para mirar lo que dibujan las
/// pruebas.
pub fn png(ancho: u32, alto: u32, argb: &[u32]) -> Vec<u8> {
    fn crc(datos: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in datos {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn trozo(out: &mut Vec<u8>, tipo: &[u8], datos: &[u8]) {
        out.extend_from_slice(&(datos.len() as u32).to_be_bytes());
        let mut t = tipo.to_vec();
        t.extend_from_slice(datos);
        out.extend_from_slice(&t);
        out.extend_from_slice(&crc(&t).to_be_bytes());
    }
    let mut crudo = Vec::new();
    for y in 0..alto {
        crudo.push(0);
        for x in 0..ancho {
            let p = argb[(y * ancho + x) as usize];
            crudo.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
        }
    }
    let mut z = vec![0x78, 0x01];
    for (i, b) in crudo.chunks(65535).enumerate() {
        let ultimo = (i + 1) * 65535 >= crudo.len();
        z.push(ultimo as u8);
        z.extend_from_slice(&(b.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(b.len() as u16)).to_le_bytes());
        z.extend_from_slice(b);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &crudo {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&(b << 16 | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&ancho.to_be_bytes());
    ihdr.extend_from_slice(&alto.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    trozo(&mut out, b"IHDR", &ihdr);
    trozo(&mut out, b"IDAT", &z);
    trozo(&mut out, b"IEND", &[]);
    out
}

/// Llama a una función de gadtools.library por su nombre.
pub fn gt(c: &mut Caja, nombre: &str, d: &[u32], a: &[u32]) -> u32 {
    let n = c.1.numero_gadtools(nombre).expect("función de gadtools");
    let mut r = regs(d, a);
    c.1.llamar(&mut c.0, n, &mut r);
    r.d[0]
}
