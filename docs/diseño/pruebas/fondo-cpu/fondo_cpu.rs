//! S37: lo que cuesta en la CPU un fondo animado a 1920 x 1080, con tres técnicas baratas. Sin GPU, sin coma flotante
//! en el bucle (como el compositor de DashOS) y sin nada de DashOS: se mide en el PC.
//!
//!     rustc --edition 2024 -O --test docs/diseño/pruebas/fondo-cpu/fondo_cpu.rs -o /tmp/fondo_cpu
//!     /tmp/fondo_cpu --ignored --nocapture      (las medidas)
//!     /tmp/fondo_cpu                            (las pruebas de que pintan lo que deben)
//!
//! 1. Un "plasma" por píxel a pantalla entera, con una tabla de senos de enteros.
//! 2. El mismo plasma a un cuarto de lado (480 x 270) y ampliado por 4 (vecino más cercano, como un fondo pixelado).
//! 3. Ciclo de paleta (el "color cycling" de los Amiga): la imagen son índices de 8 bits y cada cuadro solo cambia la
//!    paleta; se convierten a XRGB solo los píxeles cuyos índices ciclan (los de un rango), que es también el daño.

const ANCHO: usize = 1920;
const ALTO: usize = 1080;

/// sin(2π i / 256) en 1/127, como tabla.
fn tabla_senos() -> [i32; 256] {
    let mut t = [0; 256];
    for (i, v) in t.iter_mut().enumerate() {
        *v = ((i as f64 * core::f64::consts::TAU / 256.0).sin() * 127.0).round() as i32;
    }
    t
}

/// Una paleta de 256 colores suave (XRGB).
fn paleta(fase: usize) -> [u32; 256] {
    let s = tabla_senos();
    let mut p = [0u32; 256];
    for (i, c) in p.iter_mut().enumerate() {
        let k = i + fase;
        let r = (128 + s[k & 255]) as u32 / 2 + 40;
        let g = (128 + s[(k + 85) & 255]) as u32 / 2 + 40;
        let b = (128 + s[(k + 170) & 255]) as u32 / 2 + 40;
        *c = 0xFF00_0000 | r << 16 | g << 8 | b;
    }
    p
}

/// El plasma de (x, y) en el cuadro `t`: un índice de 0 a 255.
#[inline(always)]
fn plasma(s: &[i32; 256], x: usize, y: usize, t: usize) -> u8 {
    let v = s[(x / 4 + t) & 255] + s[(y / 3 + 2 * t) & 255] + s[((x + y) / 6 + 3 * t) & 255] + s[((x ^ y) / 8 + t) & 255];
    ((v + 508) >> 2) as u8
}

fn plasma_entero(px: &mut [u32], t: usize, s: &[i32; 256], p: &[u32; 256]) {
    for y in 0..ALTO {
        let fila = &mut px[y * ANCHO..(y + 1) * ANCHO];
        for (x, d) in fila.iter_mut().enumerate() {
            *d = p[plasma(s, x, y, t) as usize];
        }
    }
}

fn plasma_cuarto(px: &mut [u32], peq: &mut [u32], t: usize, s: &[i32; 256], p: &[u32; 256]) {
    let (w, h) = (ANCHO / 4, ALTO / 4);
    for y in 0..h {
        for x in 0..w {
            peq[y * w + x] = p[plasma(s, x * 4, y * 4, t) as usize];
        }
    }
    for y in 0..ALTO {
        let o = &peq[(y / 4) * w..(y / 4 + 1) * w];
        let fila = &mut px[y * ANCHO..(y + 1) * ANCHO];
        for (x, d) in fila.chunks_exact_mut(4).enumerate() {
            d.fill(o[x]);
        }
    }
}

/// Ciclo de paleta: los índices en [desde, hasta) giran; solo esos píxeles se repintan. Devuelve cuántos.
fn ciclo(px: &mut [u32], indices: &[u8], p: &[u32; 256], desde: u8, hasta: u8) -> usize {
    let mut n = 0;
    for (d, &i) in px.iter_mut().zip(indices) {
        if i >= desde && i < hasta {
            *d = p[i as usize];
            n += 1;
        }
    }
    n
}

/// Una imagen de índices: un cielo (0..64, fijo), agua que cicla (64..96) abajo y una cascada (96..112) en medio.
fn imagen_indices() -> Vec<u8> {
    let mut v = vec![0u8; ANCHO * ALTO];
    for y in 0..ALTO {
        for x in 0..ANCHO {
            v[y * ANCHO + x] = if y > ALTO * 4 / 5 {
                64 + ((x / 7 + y / 3) % 32) as u8
            } else if (900..1000).contains(&x) && y > 200 {
                96 + ((y / 5) % 16) as u8
            } else {
                (y * 64 / ALTO) as u8
            };
        }
    }
    v
}

fn medir(nombre: &str, vueltas: u32, mut f: impl FnMut(usize)) {
    let mut t: Vec<u128> = Vec::new();
    for i in 0..vueltas as usize {
        let t0 = std::time::Instant::now();
        f(i);
        t.push(t0.elapsed().as_micros());
    }
    t.sort_unstable();
    let med = t[t.len() / 2];
    println!(
        "S37 fondo en la CPU, {nombre}: mediana {med} µs por cuadro; a 60 cuadros {:.0} % de un núcleo, a 10 cuadros {:.1} %",
        med as f64 * 60.0 / 10_000.0,
        med as f64 * 10.0 / 10_000.0
    );
}

#[test]
#[ignore]
fn medir_fondos() {
    let s = tabla_senos();
    let p = paleta(0);
    let mut px = vec![0u32; ANCHO * ALTO];
    let mut peq = vec![0u32; ANCHO * ALTO / 16];
    medir("plasma por píxel, pantalla entera", 30, |t| plasma_entero(&mut px, t, &s, &p));
    medir("plasma a un cuarto de lado, ampliado", 60, |t| plasma_cuarto(&mut px, &mut peq, t, &s, &p));
    let indices = imagen_indices();
    let mut n = 0;
    medir("ciclo de paleta, agua y cascada (índices 64 a 111)", 60, |t| {
        let p = paleta(t);
        n = ciclo(&mut px, &indices, &p, 64, 112);
    });
    println!("S37 fondo en la CPU: el ciclo de paleta repinta {} de {} píxeles ({:.0} %)", n, ANCHO * ALTO, n as f64 * 100.0 / (ANCHO * ALTO) as f64);
}

#[test]
fn el_ciclo_solo_toca_lo_que_cicla() {
    let indices = imagen_indices();
    let mut px = vec![0xDEAD_BEEFu32; ANCHO * ALTO];
    let p = paleta(3);
    let n = ciclo(&mut px, &indices, &p, 64, 112);
    assert!(n > 0 && n < ANCHO * ALTO);
    for (d, &i) in px.iter().zip(&indices) {
        if (64..112).contains(&i) {
            assert_eq!(*d, p[i as usize]);
        } else {
            assert_eq!(*d, 0xDEAD_BEEF, "tocó un píxel que no cicla");
        }
    }
}

#[test]
fn el_cuarto_es_el_plasma_a_saltos() {
    let s = tabla_senos();
    let p = paleta(0);
    let mut px = vec![0u32; ANCHO * ALTO];
    let mut peq = vec![0u32; ANCHO * ALTO / 16];
    plasma_cuarto(&mut px, &mut peq, 7, &s, &p);
    for (x, y) in [(0, 0), (1919, 1079), (961, 540), (5, 1077)] {
        assert_eq!(px[y * ANCHO + x], p[plasma(&s, x / 4 * 4, y / 4 * 4, 7) as usize]);
    }
    let mut todo = vec![0u32; ANCHO * ALTO];
    plasma_entero(&mut todo, 7, &s, &p);
    assert_eq!(todo[540 * ANCHO + 960], px[540 * ANCHO + 960]);
}
