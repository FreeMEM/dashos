//! El oráculo de los sprites: las escenas de `caja/pruebas68k/sprites_escenas.h`, con la misma lista de Copper, los
//! mismos planos y las mismas listas de sprites que pone `sprites.c`, pintadas con los Chips y comparadas con lo que
//! se vio en FS-UAE (A1200, Kickstart 3.1): la imagen de cada escena que se queda quieta, línea a línea (como
//! `denise_oraculo.rs`), y `CLXDAT` de un cuadro entero en todas.
//!
//! Lo de FS-UAE está en `denise_oraculo/sprites_huellas.txt` (de `captura.sh DIR sprites` y `huellas.py DIR sprites`)
//! y en `denise_oraculo/sprites_clxdat.txt` (lo que escribe `sprites.exe` en la consola).

use caja_chips::denise::{ALTO, ANCHO, ConPantalla, V0};
use caja_chips::{CICLOS_POR_LINEA, Chips, MemoriaChip, dma};

const CUADRO: u64 = 313 * CICLOS_POR_LINEA;
const ESCENAS: &str = include_str!("../../pruebas68k/sprites_escenas.h");
const HUELLAS: &str = include_str!("denise_oraculo/sprites_huellas.txt");
const CLXDAT_FS_UAE: &str = include_str!("denise_oraculo/sprites_clxdat.txt");

/// Dónde van la lista, el mapa y las listas de los sprites (en FS-UAE, donde los dé AllocMem: da igual).
const LISTA: u32 = 0x1_0000;
const MAPA: u32 = 0x2_0000;
const ZONA: u32 = 0x4_0000;
const TAM_CANAL: u32 = 1024;

#[derive(Clone, Copy, Debug)]
struct Escena {
    bplcon0: u16,
    bplcon2: u16,
    bplcon2_abajo: u16,
    ddfstrt: u16,
    ddfstop: u16,
    clxcon: u16,
    dibujo: u8,
    grupo: i64,
    segundos: i64,
}

#[derive(Clone, Copy, Debug)]
struct Sprite {
    grupo: i64,
    canal: u32,
    hstart: u16,
    vstart: u16,
    alto: u16,
    dibujo: u8,
    attach: bool,
}

fn numero(t: &str) -> i64 {
    let t = t.trim();
    if let Some(h) = t.strip_prefix("0x") { i64::from_str_radix(h, 16).unwrap() } else { t.parse().unwrap() }
}

fn campos<'a>(nombre: &'a str) -> impl Iterator<Item = Vec<i64>> + 'a {
    // Solo las líneas que empiezan así (no las de los comentarios).
    ESCENAS.lines().filter_map(move |l| {
        let v: Vec<i64> = l.strip_prefix(nombre)?.strip_suffix(')')?.split(',').map(numero).collect();
        Some(v)
    })
}

fn escenas() -> Vec<Escena> {
    campos("ESCENA(")
        .map(|v| {
            assert_eq!(v.len(), 9);
            Escena {
                bplcon0: v[0] as u16,
                bplcon2: v[1] as u16,
                bplcon2_abajo: v[2] as u16,
                ddfstrt: v[3] as u16,
                ddfstop: v[4] as u16,
                clxcon: v[5] as u16,
                dibujo: v[6] as u8,
                grupo: v[7],
                segundos: v[8],
            }
        })
        .collect()
}

fn sprites() -> Vec<Sprite> {
    campos("SPRITE(")
        .map(|v| {
            assert_eq!(v.len(), 7);
            Sprite {
                grupo: v[0],
                canal: v[1] as u32,
                hstart: v[2] as u16,
                vstart: v[3] as u16,
                alto: v[4] as u16,
                dibujo: v[5] as u8,
                attach: v[6] != 0,
            }
        })
        .collect()
}

fn paleta(n: u16) -> u16 {
    (n * 0x137 + 0x0A5) & 0xFFF
}

fn planos(e: &Escena) -> usize {
    usize::from(e.bplcon0 >> 12 & 7)
}

/// `bytes_fila()` de sprites.c.
fn bytes_fila(e: &Escena) -> usize {
    let unidades = usize::from((e.ddfstop - e.ddfstrt) / 8 + 1);
    unidades * if e.bplcon0 & 0x8000 != 0 { 4 } else { 2 }
}

/// `indice()` de sprites.c.
fn indice(dibujo: u8, planos: usize, x: usize, y: usize) -> u8 {
    let mascara = (1usize << planos) - 1;
    (match dibujo {
        1 => ((x >> 2) + (y >> 3)) & mascara,
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
        6 => (x >> 5) & mascara,
        _ => ((x >> 3) + (y >> 3)) & mascara,
    }) as u8
}

/// `dibujar()` de sprites.c.
fn mapa(e: &Escena) -> Vec<u8> {
    let (n, bf) = (planos(e), bytes_fila(e));
    let tam = bf * 256;
    let mut v = vec![0u8; n * tam];
    for y in 0..256 {
        for b in 0..bf {
            for k in 0..8 {
                let i = indice(e.dibujo, n, b * 8 + k, y);
                for p in 0..n {
                    if i >> p & 1 != 0 {
                        v[p * tam + y * bf + b] |= 0x80 >> k;
                    }
                }
            }
        }
    }
    v
}

/// `datos()` de sprites.c.
fn datos(d: u8, y: u16) -> (u16, u16) {
    match d {
        0 => (0xFFFF, 0),
        1 => (0, 0xFFFF),
        2 => (0xFFFF, 0xFFFF),
        3 => (0xF0F0u16.rotate_left(u32::from(y & 7)), 0xFF00),
        4 => (0xAAAA, 0xCCCC),
        _ => (0xF0F0, 0xFF00),
    }
}

/// `listas()` de sprites.c: las palabras de cada canal, desde `ZONA + TAM_CANAL * canal`.
fn listas(grupo: i64, mem: &mut [u16]) {
    for c in 0..8 {
        let mut w = Vec::new();
        for s in sprites().iter().filter(|s| s.grupo == grupo && s.canal == c) {
            let vstop = s.vstart + s.alto;
            w.push((s.vstart & 0xFF) << 8 | (s.hstart >> 1 & 0xFF));
            w.push(
                (vstop & 0xFF) << 8
                    | if s.attach { 0x80 } else { 0 }
                    | (s.vstart >> 8 & 1) << 2
                    | (vstop >> 8 & 1) << 1
                    | (s.hstart & 1),
            );
            for y in 0..s.alto {
                let (a, b) = datos(s.dibujo, y);
                w.extend_from_slice(&[a, b]);
            }
        }
        w.extend_from_slice(&[0, 0]);
        let i = ((ZONA + TAM_CANAL * c) / 2) as usize;
        mem[i..i + w.len()].copy_from_slice(&w);
    }
}

/// `copper()` de sprites.c.
fn lista(e: &Escena) -> Vec<u16> {
    let mut l = Vec::new();
    let mut mov = |r: u16, v: u16| l.extend_from_slice(&[r, v]);
    for c in 0..8u16 {
        let d = ZONA + TAM_CANAL * u32::from(c);
        mov(0x120 + 4 * c, (d >> 16) as u16);
        mov(0x122 + 4 * c, d as u16);
    }
    mov(0x1FC, 0);
    mov(0x106, 0x0C00);
    mov(0x10C, 0x0011);
    mov(0x098, e.clxcon);
    mov(0x08E, 0x2C81);
    mov(0x090, 0x2CC1);
    mov(0x092, e.ddfstrt);
    mov(0x094, e.ddfstop);
    mov(0x108, 0);
    mov(0x10A, 0);
    mov(0x100, e.bplcon0);
    mov(0x102, 0);
    mov(0x104, e.bplcon2);
    let tam = (bytes_fila(e) * 256) as u32;
    for p in 0..planos(e) as u32 {
        let d = MAPA + p * tam;
        mov(0x0E0 + 4 * p as u16, (d >> 16) as u16);
        mov(0x0E2 + 4 * p as u16, d as u16);
    }
    for i in 0..32 {
        mov(0x180 + 2 * i, paleta(i));
    }
    let wait = |l: &mut Vec<u16>, v: u16, h: u16| l.extend_from_slice(&[((v & 0xFF) << 8) | (h & 0xFE) | 1, 0xFFFE]);
    if e.grupo == 1 {
        for v in 0x80..0x88 {
            wait(&mut l, v, 0x20);
            l.extend_from_slice(&[0x148, 0x7E50]);
            wait(&mut l, v, 0x70);
            l.extend_from_slice(&[0x148, 0x7E80]);
        }
    }
    wait(&mut l, 0xAC, 0x07);
    l.extend_from_slice(&[0x104, e.bplcon2_abajo]);
    if e.grupo == 1 {
        wait(&mut l, 0xB0, 0x10);
        l.extend_from_slice(&[0x168, 0x0088, 0x16A, 0x0000, 0x16E, 0x0FF0, 0x16C, 0x3C3C]);
        wait(&mut l, 0xC0, 0x10);
        l.extend_from_slice(&[0x16A, 0x0000]);
    }
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

/// La escena `e` con los Chips: la imagen del tercer cuadro tras encender la lista, y `CLXDAT` leído al empezar el
/// tercer cuadro tras borrarlo al empezar el segundo (como sprites.c, en la línea 1).
fn pintar(e: &Escena) -> (Vec<u32>, u16) {
    let mut mem = Memoria(vec![0; 256 * 1024]);
    let l = lista(e);
    mem.0[LISTA as usize / 2..LISTA as usize / 2 + l.len()].copy_from_slice(&l);
    for (i, w) in mapa(e).chunks(2).enumerate() {
        mem.0[MAPA as usize / 2 + i] = u16::from_be_bytes([w[0], w[1]]);
    }
    listas(e.grupo, &mut mem.0);
    let mut cuadro = vec![0u32; ANCHO * ALTO];
    let mut c = Chips::nuevo();
    let mut p = ConPantalla::nueva(&mut mem, &mut cuadro);
    c.escribir_con(0xDFF080, LISTA, 4, 0, &mut p);
    let bits = 0x8000 | dma::DMAEN | dma::COPEN | dma::BPLEN | dma::SPREN;
    c.escribir_con(0xDFF096, u32::from(bits), 2, 0, &mut p);
    c.leer_con(0xDFF00E, 2, 2 * CUADRO + CICLOS_POR_LINEA, &mut p);
    let clxdat = c.leer_con(0xDFF00E, 2, 3 * CUADRO + CICLOS_POR_LINEA, &mut p) as u16;
    c.avanzar_con(3 * CUADRO, &mut p);
    c.avanzar_con(4 * CUADRO, &mut p);
    assert_eq!(p.cuadros, 4);
    (cuadro, clxdat)
}

/// Escribe la imagen de cada escena quieta en `$SPRITES_VOLCADO/escenaN.ppm`.
#[test]
#[ignore]
fn volcar_escenas() {
    let dir = std::env::var("SPRITES_VOLCADO").expect("SPRITES_VOLCADO");
    for (n, e) in escenas().iter().filter(|e| e.segundos > 0).enumerate() {
        let (img, _) = pintar(e);
        let mut ppm = format!("P6\n{ANCHO} {ALTO}\n255\n").into_bytes();
        for p in img {
            ppm.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
        }
        std::fs::write(format!("{dir}/escena{}.ppm", n + 1), ppm).unwrap();
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
fn la_imagen_de_cada_escena_es_la_de_fs_uae() {
    let quietas: Vec<Escena> = escenas().into_iter().filter(|e| e.segundos > 0).collect();
    let mut huellas = vec![Vec::new(); quietas.len()];
    for l in HUELLAS.lines().filter(|l| !l.starts_with('#')) {
        let v: Vec<&str> = l.split_whitespace().collect();
        let n: usize = v[0].parse().unwrap();
        let linea: u64 = v[1].parse().unwrap();
        assert_eq!(linea, V0 + huellas[n - 1].len() as u64);
        huellas[n - 1].push(u32::from_str_radix(v[2], 16).unwrap());
    }
    let mut fallos = Vec::new();
    for (n, e) in quietas.iter().enumerate() {
        assert_eq!(huellas[n].len(), ALTO, "escena {}", n + 1);
        let (img, _) = pintar(e);
        let malas: Vec<u64> = (0..ALTO)
            .filter(|&f| huella(&img[f * ANCHO..(f + 1) * ANCHO]) != huellas[n][f])
            .map(|f| V0 + f as u64)
            .collect();
        if !malas.is_empty() {
            fallos.push(format!("escena {}: {} líneas distintas, desde la {:#x}", n + 1, malas.len(), malas[0]));
        }
    }
    assert!(fallos.is_empty(), "{fallos:#?}");
}

#[test]
fn clxdat_de_cada_escena_es_el_de_fs_uae() {
    // "escena N clxdat XXXX", una por escena.
    let fs_uae: Vec<(usize, u16)> = CLXDAT_FS_UAE
        .lines()
        .filter_map(|l| {
            let v: Vec<&str> = l.split_whitespace().collect();
            (v.len() == 4 && v[0] == "escena").then(|| (v[1].parse().unwrap(), u16::from_str_radix(v[3], 16).unwrap()))
        })
        .collect();
    let escenas = escenas();
    assert_eq!(fs_uae.len(), escenas.len());
    let mut fallos = Vec::new();
    for (i, e) in escenas.iter().enumerate() {
        let (n, esperado) = fs_uae[i];
        assert_eq!(n, i + 1);
        let (_, clxdat) = pintar(e);
        // FS-UAE, con CLXCON sin ningún ENBP, da siempre los ocho choques de sprites con campos (bits 1-8), aunque no
        // haya sprites (escena 20). Ni el manual ni vAmiga lo dicen: aquí no se imita, y en esas escenas se mira lo
        // demás (ver la cabecera de src/sprites.rs).
        let clxdat = if e.clxcon & 0x0FC0 == 0 {
            assert_eq!(esperado & 0x01FE, 0x01FE, "escena {n}");
            clxdat | 0x01FE
        } else {
            clxdat
        };
        if clxdat != esperado {
            fallos.push(format!("escena {n}: {clxdat:04x}, FS-UAE {esperado:04x}"));
        }
    }
    assert!(fallos.is_empty(), "{fallos:#?}");
}
