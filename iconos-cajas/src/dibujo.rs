//! Pintar un icono de las cajas (ARGB sin premultiplicar, con transparencia) en un lienzo del kit.
//!
//! El icono va centrado en un cuadro de `s` × `s`. Si cabe, a su tamaño o multiplicado por el entero más grande que
//! quepa (cada píxel, un cuadrado: los iconos del Amiga son dibujos de píxeles y así se ven nítidos). Si no cabe, se
//! reduce sin deformar, cada píxel la media del trozo que tapa (con su alfa: el borde no se ensucia de negro).

use kit::imagen::Imagen;
use kit::lienzo::Lienzo;

/// Lo que mide `img` pintada en un cuadro de `s`: (ancho, alto).
pub fn medida(img: &Imagen, s: i32) -> (i32, i32) {
    let (w, h, s) = (img.ancho as i64, img.alto as i64, s.max(0) as i64);
    if w == 0 || h == 0 || s == 0 {
        return (0, 0);
    }
    let lado = w.max(h);
    if lado <= s {
        let k = s / lado;
        ((w * k) as i32, (h * k) as i32)
    } else {
        (((w * s / lado).max(1)) as i32, ((h * s / lado).max(1)) as i32)
    }
}

/// El color del píxel (x, y) de la imagen pintada a `dw` × `dh`.
fn muestra(img: &Imagen, dw: i32, dh: i32, x: i32, y: i32) -> u32 {
    let (w, h) = (img.ancho as i64, img.alto as i64);
    let (x0, x1) = (x as i64 * w / dw as i64, ((x as i64 + 1) * w / dw as i64).max(x as i64 * w / dw as i64 + 1).min(w));
    let (y0, y1) = (y as i64 * h / dh as i64, ((y as i64 + 1) * h / dh as i64).max(y as i64 * h / dh as i64 + 1).min(h));
    if x1 - x0 == 1 && y1 - y0 == 1 {
        return img.px[(y0 * w + x0) as usize];
    }
    // La media del trozo, pesando cada color por su alfa.
    let (mut a, mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
    for sy in y0..y1 {
        for sx in x0..x1 {
            let c = img.px[(sy * w + sx) as usize];
            let ca = (c >> 24) as u64;
            a += ca;
            r += ((c >> 16) & 0xff) as u64 * ca;
            g += ((c >> 8) & 0xff) as u64 * ca;
            b += (c & 0xff) as u64 * ca;
            n += 1;
        }
    }
    if a == 0 {
        return 0;
    }
    ((a / n) as u32) << 24 | ((r / a) as u32) << 16 | ((g / a) as u32) << 8 | (b / a) as u32
}

/// Pinta `img` en el cuadro de `s` × `s` de (x, y).
pub fn pintar(l: &mut Lienzo, x: i32, y: i32, s: i32, img: &Imagen) {
    let (dw, dh) = medida(img, s);
    if dw == 0 || dh == 0 || img.px.len() < img.ancho * img.alto {
        return;
    }
    let (ox, oy) = (x + (s - dw) / 2, y + (s - dh) / 2);
    let corte = l.recorte();
    for dy in 0..dh {
        let py = oy + dy;
        if py < corte.y || py >= corte.y + corte.h {
            continue;
        }
        for dx in 0..dw {
            let px = ox + dx;
            if px < corte.x || px >= corte.x + corte.w {
                continue;
            }
            let c = muestra(img, dw, dh, dx, dy);
            if c >> 24 != 0 {
                l.punto(px, py, c, 255);
            }
        }
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use alloc::vec;

    fn imagen(ancho: usize, alto: usize, c: u32) -> Imagen {
        Imagen { ancho, alto, px: vec![c; ancho * alto] }
    }

    #[test]
    fn medidas() {
        // Cabe dos veces: el doble. Cabe una: tal cual. No cabe: reducida sin deformar.
        assert_eq!(medida(&imagen(32, 16, 0), 72), (64, 32));
        assert_eq!(medida(&imagen(46, 46, 0), 72), (46, 46));
        assert_eq!(medida(&imagen(64, 32, 0), 28), (28, 14));
        assert_eq!(medida(&imagen(500, 1, 0), 28), (28, 1));
        assert_eq!(medida(&imagen(0, 10, 0), 28), (0, 0));
        assert_eq!(medida(&imagen(10, 10, 0), -5), (0, 0));
    }

    #[test]
    fn pinta_dentro_y_respeta_lo_transparente() {
        let mut px = vec![0xff00_00ffu32; 100 * 100];
        let mut l = Lienzo::new(&mut px, 100, 100, 100);
        let mut img = imagen(4, 4, 0xffff_0000);
        img.px[0] = 0; // una esquina transparente
        // Fuera de la pantalla por todos lados: no pasa nada.
        pintar(&mut l, -50, -50, 48, &img);
        pintar(&mut l, 90, 90, 48, &img);
        pintar(&mut l, 0, 0, 8, &img);
        // ×2 centrada en 8: de (0,0) a (8,8); la esquina transparente deja el azul.
        assert_eq!(px[0], 0xff00_00ff);
        assert_eq!(px[3 * 100 + 3], 0xffff_0000);
    }

    #[test]
    fn reducir_media_con_alfa() {
        // La mitad transparente y la mitad roja: el borde sale rojo a medias, no oscurecido.
        let mut img = imagen(2, 1, 0xffff_0000);
        img.px[1] = 0;
        let c = muestra(&img, 1, 1, 0, 0);
        assert_eq!(c >> 24, 127);
        assert_eq!(c & 0x00ff_ffff, 0xff_0000);
    }
}
