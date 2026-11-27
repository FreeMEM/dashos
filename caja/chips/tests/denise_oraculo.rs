//! El oráculo de la imagen: los modos de `caja/pruebas68k/denise_modos.h`, con la misma lista de Copper y los mismos
//! planos que pone `denise.c`, pintados con los Chips y comparados con lo que se vio en FS-UAE (A1200, Kickstart 3.1).
//!
//! Lo de FS-UAE está en `denise_oraculo/huellas.txt`: la huella de cada línea de la captura de cada modo (ver
//! `denise_oraculo/captura.sh` y `huellas.py`). La captura "full" de FS-UAE cubre lo mismo que la imagen de los Chips
//! (líneas 26 a 311, posiciones $5C a $1D3), así que la comparación es línea a línea y píxel a píxel.

use caja_chips::denise::{ALTO, ANCHO, ConPantalla, V0};
use caja_chips::{CICLOS_POR_LINEA, Chips, MemoriaChip, dma};

const CUADRO: u64 = 313 * CICLOS_POR_LINEA;
const MODOS: &str = include_str!("../../pruebas68k/denise_modos.h");
const HUELLAS: &str = include_str!("denise_oraculo/huellas.txt");

/// Dónde van la lista y el mapa (en FS-UAE, donde los dé AllocMem: da igual).
const LISTA: u32 = 0x1_0000;
const MAPA: u32 = 0x2_0000;

/// Un modo de `denise_modos.h`.
#[derive(Clone, Copy, Debug)]
struct Modo {
    bplcon0: u16,
    bplcon1: u16,
    bplcon2: u16,
    bplcon2_abajo: u16,
    ddfstrt: u16,
    ddfstop: u16,
    diwstrt: u16,
    diwstop: u16,
    bytes_fila: usize,
    filas: usize,
    bpl1mod: i16,
    bpl2mod: i16,
    dibujo: u8,
}

fn numero(t: &str) -> i64 {
    let t = t.trim();
    if let Some(h) = t.strip_prefix("0x") { i64::from_str_radix(h, 16).unwrap() } else { t.parse().unwrap() }
}

fn modos() -> Vec<Modo> {
    MODOS
        .lines()
        .filter_map(|l| l.trim().strip_prefix("MODO(")?.strip_suffix(')'))
        .map(|l| {
            let v: Vec<i64> = l.split(',').map(numero).collect();
            assert_eq!(v.len(), 13, "{l}");
            Modo {
                bplcon0: v[0] as u16,
                bplcon1: v[1] as u16,
                bplcon2: v[2] as u16,
                bplcon2_abajo: v[3] as u16,
                ddfstrt: v[4] as u16,
                ddfstop: v[5] as u16,
                diwstrt: v[6] as u16,
                diwstop: v[7] as u16,
                bytes_fila: v[8] as usize,
                filas: v[9] as usize,
                bpl1mod: v[10] as i16,
                bpl2mod: v[11] as i16,
                dibujo: v[12] as u8,
            }
        })
        .collect()
}

/// Como en denise.c.
fn paleta(n: u16) -> u16 {
    (n * 0x137 + 0x0A5) & 0xFFF
}
const BARRAS: [u16; 6] = [0xF00, 0xF80, 0xFF0, 0x0F0, 0x00F, 0x80F];
fn banda(n: u16) -> u16 {
    n * 0x111
}

/// `indice()` de denise.c.
fn indice(dibujo: u8, planos: u32, x: usize, y: usize) -> u8 {
    let mascara = (1usize << planos) - 1;
    (match dibujo {
        0 => ((x >> 3) + (y >> 3)) & mascara,
        1 => ((x >> 2) + (y >> 3)) & mascara,
        2 => ((x >> 2) + y) & 63,
        3 => {
            if x & 15 == 0 {
                (y >> 4) & 15
            } else {
                (1 + (x >> 4) % 3) << 4 | (x & 15)
            }
        }
        4 => {
            let (i1, i2) = ((x >> 4) & 7, (y >> 4) & 7);
            (i1 & 1) | (i2 & 1) << 1 | (i1 & 2) << 1 | (i2 & 2) << 2 | (i1 & 4) << 2 | (i2 & 4) << 3
        }
        _ => ((x >> 3) + (y >> 2)) & mascara,
    }) as u8
}

fn planos(m: &Modo) -> u32 {
    u32::from(m.bplcon0 >> 12 & 7)
}

/// Los planos del mapa, como `dibujar()` de denise.c.
fn mapa(m: &Modo) -> Vec<u8> {
    let n = planos(m) as usize;
    let tam = m.bytes_fila * m.filas;
    let mut v = vec![0u8; n * tam];
    for y in 0..m.filas {
        for b in 0..m.bytes_fila {
            for k in 0..8 {
                let i = indice(m.dibujo, n as u32, b * 8 + k, y);
                for p in 0..n {
                    if i >> p & 1 != 0 {
                        v[p * tam + y * m.bytes_fila + b] |= 0x80 >> k;
                    }
                }
            }
        }
    }
    v
}

/// La lista, como `copper()` de denise.c.
fn lista(m: &Modo, mapa: u32) -> Vec<u16> {
    let mut l = Vec::new();
    let mut mov = |r: u16, v: u16| l.extend_from_slice(&[r, v]);
    mov(0x1FC, 0);
    mov(0x106, 0x0C00);
    mov(0x10C, 0x0011);
    for i in 0..8 {
        mov(0x142 + 8 * i, 0);
    }
    mov(0x08E, m.diwstrt);
    mov(0x090, m.diwstop);
    mov(0x092, m.ddfstrt);
    mov(0x094, m.ddfstop);
    mov(0x108, m.bpl1mod as u16);
    mov(0x10A, m.bpl2mod as u16);
    mov(0x100, m.bplcon0);
    mov(0x102, m.bplcon1);
    mov(0x104, m.bplcon2);
    let tam = (m.bytes_fila * m.filas) as u32;
    for p in 0..planos(m) {
        let d = mapa + p * tam;
        mov(0x0E0 + 4 * p as u16, (d >> 16) as u16);
        mov(0x0E2 + 4 * p as u16, d as u16);
    }
    for i in 0..32 {
        mov(0x180 + 2 * i, paleta(i));
    }
    let wait = |l: &mut Vec<u16>, v: u16, h: u16| l.extend_from_slice(&[((v & 0xFF) << 8) | (h & 0xFE) | 1, 0xFFFE]);
    for (i, &b) in BARRAS.iter().enumerate() {
        wait(&mut l, 0x20 + 2 * i as u16, 0x07);
        l.extend_from_slice(&[0x180, b]);
    }
    wait(&mut l, 0x2B, 0x40);
    for i in 0..16 {
        l.extend_from_slice(&[0x180, banda(i)]);
    }
    wait(&mut l, 0x2C, 0x07);
    l.extend_from_slice(&[0x180, paleta(0)]);
    wait(&mut l, 0x90, 0x70);
    l.extend_from_slice(&[0x182, 0xFFF]);
    wait(&mut l, 0x91, 0x07);
    l.extend_from_slice(&[0x182, paleta(1)]);
    wait(&mut l, 0xAC, 0x07);
    l.extend_from_slice(&[0x104, m.bplcon2_abajo]);
    l.extend_from_slice(&[0xFFFF, 0xFFFE]);
    l
}

struct Memoria(Vec<u16>);

impl MemoriaChip for Memoria {
    fn leer_palabra(&mut self, dir: u32) -> u16 {
        self.0.get(dir as usize / 2).copied().unwrap_or(0)
    }

    fn escribir_palabra(&mut self, dir: u32, valor: u16) {
        if let Some(w) = self.0.get_mut(dir as usize / 2) {
            *w = valor;
        }
    }
}

/// La imagen del modo `m` con los Chips: el segundo cuadro completo tras encender la lista.
fn pintar(m: &Modo) -> Vec<u32> {
    let mut mem = Memoria(vec![0; 256 * 1024]);
    let l = lista(m, MAPA);
    mem.0[LISTA as usize / 2..LISTA as usize / 2 + l.len()].copy_from_slice(&l);
    for (i, w) in mapa(m).chunks(2).enumerate() {
        mem.0[MAPA as usize / 2 + i] = u16::from_be_bytes([w[0], w[1]]);
    }
    let mut cuadro = vec![0u32; ANCHO * ALTO];
    let mut c = Chips::nuevo();
    let mut p = ConPantalla::nueva(&mut mem, &mut cuadro);
    c.escribir_con(0xDFF080, LISTA, 4, 0, &mut p);
    c.escribir_con(0xDFF096, u32::from(0x8000 | dma::DMAEN | dma::COPEN | dma::BPLEN), 2, 0, &mut p);
    c.avanzar_con(3 * CUADRO, &mut p);
    assert_eq!(p.cuadros, 3);
    cuadro
}

/// Escribe la imagen de cada modo en `$DENISE_VOLCADO/modoN.ppm` (para mirarlas o compararlas a mano).
#[test]
#[ignore]
fn volcar_modos() {
    let dir = std::env::var("DENISE_VOLCADO").expect("DENISE_VOLCADO");
    for (n, m) in modos().iter().enumerate() {
        let img = pintar(m);
        let mut ppm = format!("P6\n{ANCHO} {ALTO}\n255\n").into_bytes();
        for p in img {
            ppm.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
        }
        std::fs::write(format!("{dir}/modo{}.ppm", n + 1), ppm).unwrap();
    }
}

/// FNV-1a de 32 bits de los bytes R, G, B de `pixeles`, como `huellas.py`.
fn huella(pixeles: &[u32]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for p in pixeles {
        for b in [(p >> 16) as u8, (p >> 8) as u8, *p as u8] {
            h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
        }
    }
    h
}

#[test]
fn la_imagen_de_cada_modo_es_la_de_fs_uae() {
    let modos = modos();
    let mut huellas = vec![Vec::new(); modos.len()];
    for l in HUELLAS.lines().filter(|l| !l.starts_with('#')) {
        let v: Vec<&str> = l.split_whitespace().collect();
        let modo: usize = v[0].parse().unwrap();
        let linea: u64 = v[1].parse().unwrap();
        assert_eq!(linea, V0 + huellas[modo - 1].len() as u64);
        huellas[modo - 1].push(u32::from_str_radix(v[2], 16).unwrap());
    }
    let mut fallos = Vec::new();
    for (n, m) in modos.iter().enumerate() {
        assert_eq!(huellas[n].len(), ALTO, "modo {}", n + 1);
        let img = pintar(m);
        let malas: Vec<u64> = (0..ALTO)
            .filter(|&f| huella(&img[f * ANCHO..(f + 1) * ANCHO]) != huellas[n][f])
            .map(|f| V0 + f as u64)
            .collect();
        if !malas.is_empty() {
            fallos.push(format!("modo {}: {} líneas distintas, desde la {:#x}", n + 1, malas.len(), malas[0]));
        }
    }
    assert!(fallos.is_empty(), "{fallos:#?}");
}
