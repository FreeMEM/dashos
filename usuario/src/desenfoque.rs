//! S27, oleada 6: el fondo desenfocado de la pantalla de bloqueo, como el de DMS (`Modules/Lock/LockScreenBackground.qml`:
//! el fondo con `blur` 0,8 de un máximo de 32 y un velo negro al 40 %, `lockScreenScrimAlpha`).
//!
//! Portado de DMS (DankMaterialShell v2.1.2; MIT, copyright (c) 2025 Avenge Media LLC; aviso entero en
//! `docs/licencias/DankMaterialShell-MIT.txt`). DMS lo hace en la GPU; aquí, barato y sin coma flotante: el programa
//! `fondo` reduce la imagen a un octavo de la pantalla (lo que ya sabe hacer `kit::imagen`), la desenfoca ahí con dos
//! pasadas de caja (una caja de 5 en la pequeña son unos 40 píxeles en la grande) y se la da a `guardian`, que la agranda
//! con interpolación bilineal (que también suaviza) y le pone el velo.
//!
//! Sin `alloc` ni nada de DashOS: se prueba en el PC con `rustc --edition 2024 --test usuario/src/desenfoque.rs`.

/// Cuánto se reduce la pantalla para desenfocar (y lo que se agranda luego).
pub const REDUCCION: usize = 8;
/// El radio de la caja, en píxeles de la imagen pequeña, y cuántas pasadas.
pub const RADIO: usize = 2;
pub const PASADAS: usize = 2;
/// El velo de DMS: negro al 40 % (`lockScreenScrimAlpha`), en 256.
pub const VELO: u32 = 102;
/// Lo más que mide la imagen pequeña por lado (una pantalla de 8192 entre 8): lo que se acepta al recibirla.
pub const MAX_LADO: usize = 1024;

/// Desenfoca `px` (`ancho` × `alto`, ARGB opaco) con `pasadas` de caja de radio `radio`, en horizontal y en vertical.
/// `linea` es un búfer de trabajo de `ancho.max(alto)` píxeles como poco (si es menor, no se hace nada). Los bordes
/// repiten el último píxel.
pub fn desenfocar(px: &mut [u32], ancho: usize, alto: usize, radio: usize, pasadas: usize, linea: &mut [u32]) {
    if ancho == 0 || alto == 0 || px.len() < ancho * alto || linea.len() < ancho.max(alto) || radio == 0 {
        return;
    }
    for _ in 0..pasadas {
        for y in 0..alto {
            caja(&mut px[y * ancho..(y + 1) * ancho], 1, ancho, radio, linea);
        }
        for x in 0..ancho {
            caja(&mut px[x..], ancho, alto, radio, linea);
        }
    }
}

/// Una caja de radio `radio` sobre `n` píxeles de `px` separados `paso`: la media de los `2 radio + 1` de alrededor.
fn caja(px: &mut [u32], paso: usize, n: usize, radio: usize, linea: &mut [u32]) {
    for i in 0..n {
        linea[i] = px[i * paso];
    }
    let ventana = (2 * radio + 1) as u32;
    let en = |i: isize| linea[i.clamp(0, n as isize - 1) as usize];
    let mut suma = [0u32; 3];
    for d in -(radio as isize)..=radio as isize {
        let c = en(d);
        suma[0] += c >> 16 & 0xff;
        suma[1] += c >> 8 & 0xff;
        suma[2] += c & 0xff;
    }
    for i in 0..n {
        px[i * paso] = 0xff00_0000 | (suma[0] / ventana) << 16 | (suma[1] / ventana) << 8 | suma[2] / ventana;
        let (sale, entra) = (en(i as isize - radio as isize), en(i as isize + radio as isize + 1));
        suma[0] = suma[0] + (entra >> 16 & 0xff) - (sale >> 16 & 0xff);
        suma[1] = suma[1] + (entra >> 8 & 0xff) - (sale >> 8 & 0xff);
        suma[2] = suma[2] + (entra & 0xff) - (sale & 0xff);
    }
}

/// Oscurece un color con el velo (`velo` de 256 de negro encima).
pub fn con_velo(c: u32, velo: u32) -> u32 {
    let k = 256 - velo.min(256);
    let canal = |s: u32| ((c >> s & 0xff) * k >> 8) << s;
    0xff00_0000 | canal(16) | canal(8) | canal(0)
}

/// Pinta en `destino` (de `dw` × `dh`, con `linea` píxeles por fila) la zona (`x`, `y`, `w`, `h`) del fondo pequeño
/// `fuente` (`sw` × `sh`) agrandado a toda la pantalla con interpolación bilineal, y el velo encima. Lo que caiga fuera
/// del destino no se pinta.
#[allow(clippy::too_many_arguments)]
pub fn agrandar(destino: &mut [u32], dw: usize, dh: usize, linea: usize, zona: (i32, i32, i32, i32), fuente: &[u32], sw: usize, sh: usize, velo: u32) {
    if sw == 0 || sh == 0 || fuente.len() < sw * sh || dw == 0 || dh == 0 {
        return;
    }
    let x0 = zona.0.clamp(0, dw as i32) as usize;
    let y0 = zona.1.clamp(0, dh as i32) as usize;
    let x1 = (zona.0.saturating_add(zona.2)).clamp(0, dw as i32) as usize;
    let y1 = (zona.1.saturating_add(zona.3)).clamp(0, dh as i32) as usize;
    if destino.len() < (y1.max(1) - 1) * linea + x1 {
        return;
    }
    // En 16.16: dónde cae el centro de cada píxel de la pantalla en la imagen pequeña.
    let paso_x = ((sw as u64) << 16) / dw as u64;
    let paso_y = ((sh as u64) << 16) / dh as u64;
    let posicion = |i: usize, paso: u64, lado: usize| -> (usize, usize, u32) {
        let p = (i as i64 * paso as i64 + paso as i64 / 2 - 32768).clamp(0, ((lado as i64) - 1) << 16);
        let a = (p >> 16) as usize;
        (a, (a + 1).min(lado - 1), (p as u32 & 0xffff) >> 8)
    };
    let mezcla = |a: u32, b: u32, t: u32| -> u32 {
        let canal = |s: u32| (((a >> s & 0xff) * (256 - t) + (b >> s & 0xff) * t) >> 8) << s;
        canal(16) | canal(8) | canal(0)
    };
    for y in y0..y1 {
        let (fa, fb, ty) = posicion(y, paso_y, sh);
        let (fila_a, fila_b) = (&fuente[fa * sw..(fa + 1) * sw], &fuente[fb * sw..(fb + 1) * sw]);
        let d = &mut destino[y * linea..y * linea + x1];
        for x in x0..x1 {
            let (ca, cb, tx) = posicion(x, paso_x, sw);
            let arriba = mezcla(fila_a[ca], fila_a[cb], tx);
            let abajo = mezcla(fila_b[ca], fila_b[cb], tx);
            d[x] = con_velo(mezcla(arriba, abajo, ty), velo);
        }
    }
}

/// Lo que mide la imagen pequeña para una pantalla de `ancho` × `alto` (un octavo, como poco 1).
pub fn medida_pequena(ancho: usize, alto: usize) -> (usize, usize) {
    ((ancho / REDUCCION).clamp(1, MAX_LADO), (alto / REDUCCION).clamp(1, MAX_LADO))
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn liso_sigue_liso() {
        let mut px = [0xff40_8020u32; 12 * 7];
        let mut l = [0u32; 12];
        desenfocar(&mut px, 12, 7, RADIO, PASADAS, &mut l);
        assert!(px.iter().all(|&c| c == 0xff40_8020));
    }

    #[test]
    fn un_punto_se_reparte() {
        let mut px = [0xff00_0000u32; 9 * 9];
        px[4 * 9 + 4] = 0xffff_ffff;
        let mut l = [0u32; 9];
        desenfocar(&mut px, 9, 9, 1, 1, &mut l);
        // 255 / 9 en cada canal en los nueve de alrededor; nada fuera.
        assert_eq!(px[4 * 9 + 4] & 0xff, 255 / 3 / 3);
        assert_eq!(px[3 * 9 + 3] & 0xff, 255 / 3 / 3);
        assert_eq!(px[2 * 9 + 2] & 0xff, 0);
        assert!(px.iter().all(|&c| c >> 24 == 0xff));
    }

    #[test]
    fn hostiles_no_tumban() {
        // Búfer de trabajo corto, medidas que no cuadran, ceros: no se hace nada y no hay pánico.
        let mut px = [1u32; 16];
        let mut corto = [0u32; 2];
        desenfocar(&mut px, 4, 4, 2, 2, &mut corto);
        assert_eq!(px, [1; 16]);
        desenfocar(&mut px, 5, 5, 2, 2, &mut [0u32; 8]);
        desenfocar(&mut px, 0, 4, 2, 2, &mut [0u32; 8]);
        let mut d = [0u32; 16];
        agrandar(&mut d, 4, 4, 4, (0, 0, 4, 4), &px, 0, 4, VELO);
        agrandar(&mut d, 4, 4, 4, (0, 0, 4, 4), &px[..3], 2, 2, VELO);
        // Zonas fuera, negativas o enormes: se recortan.
        agrandar(&mut d, 4, 4, 4, (-100, -100, i32::MAX, i32::MAX), &[0xffff_ffff; 4], 2, 2, 0);
        assert!(d.iter().all(|&c| c == 0xffff_ffff));
        agrandar(&mut d, 4, 4, 4, (10, 10, 5, 5), &[0; 4], 2, 2, 0);
        // Un destino más corto de lo que dice: nada.
        let mut corto = [7u32; 3];
        agrandar(&mut corto, 4, 4, 4, (0, 0, 4, 4), &[0; 4], 2, 2, 0);
        assert_eq!(corto, [7; 3]);
    }

    #[test]
    fn agrandar_interpola_y_vela() {
        // Dos columnas, negra y blanca: en medio de la pantalla, gris; en los bordes, los de la imagen.
        let fuente = [0xff00_0000, 0xffff_ffff, 0xff00_0000, 0xffff_ffff];
        let mut d = [0u32; 16 * 2];
        agrandar(&mut d, 16, 2, 16, (0, 0, 16, 2), &fuente, 2, 2, 0);
        assert_eq!(d[0] & 0xff, 0);
        assert_eq!(d[15] & 0xff, 255);
        assert!((100..160).contains(&(d[8] & 0xff)));
        // Con el velo de DMS, el blanco queda al 60 %.
        agrandar(&mut d, 16, 2, 16, (0, 0, 16, 2), &fuente, 2, 2, VELO);
        assert_eq!(d[15] & 0xff, 255 * (256 - VELO) / 256);
        // Solo la zona: lo de fuera no se toca.
        let mut d = [5u32; 16 * 2];
        agrandar(&mut d, 16, 2, 16, (4, 1, 2, 1), &fuente, 2, 2, 0);
        assert_eq!(d.iter().filter(|&&c| c != 5).count(), 2);
    }

    #[test]
    fn medidas() {
        assert_eq!(medida_pequena(1920, 1080), (240, 135));
        assert_eq!(medida_pequena(4, 4), (1, 1));
        assert_eq!(medida_pequena(100_000, 8), (MAX_LADO, 1));
    }
}
