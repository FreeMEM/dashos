//! Las ventanas Amiga a mayor tamaño (x2, x3) sin que el programa lo sepa: él pinta y recibe el ratón en sus
//! coordenadas de siempre; quien habla con el compositor (`caja/pegamento/src/compositor.rs`) manda cada píxel como un
//! cuadrado de `f` x `f` (vecino más cercano, nítido como en un monitor del Amiga) y divide las coordenadas del ratón
//! entre `f`. Aquí está lo que no depende del compositor, para probarlo en el PC.

use crate::lienzo::Rect;

/// El factor que se usa de verdad para una ventana de `ancho` x `alto`: el pedido (de 1 a `tope`), bajado hasta que
/// la ventana ampliada quepa en `max_lado` por lado y en `max_bytes` (a 4 bytes el píxel). Nunca menos de 1: una
/// ventana que no cabe ni a 1x la rechaza quien la crea.
pub fn ajustar(pedido: u32, tope: u32, ancho: u32, alto: u32, max_lado: u32, max_bytes: u64) -> u32 {
    let mut f = pedido.clamp(1, tope.max(1));
    while f > 1 {
        let (an, al) = (ancho as u64 * f as u64, alto as u64 * f as u64);
        if an <= max_lado as u64 && al <= max_lado as u64 && an * al * 4 <= max_bytes {
            break;
        }
        f -= 1;
    }
    f
}

/// Una coordenada del compositor (en píxeles de la superficie ampliada) en las del programa. Fuera de la ventana
/// (negativa, o más allá) sigue fuera: redondea hacia abajo también en los negativos.
pub fn al_programa(v: i32, f: u32) -> i32 {
    v.div_euclid(f.clamp(1, i32::MAX as u32) as i32)
}

/// Un rectángulo del programa (dentro de `ancho` x `alto`) en la superficie ampliada. Vacío si cae fuera.
pub fn ampliar_rect(r: Rect, f: u32, ancho: u32, alto: u32) -> Rect {
    let d = r.interseccion(&Rect::de_tamano(ancho, alto));
    if d.vacio() {
        return d;
    }
    let f = f.max(1) as i64;
    let a = |v: i64| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    Rect { x0: a(d.x0 as i64 * f), y0: a(d.y0 as i64 * f), x1: a((d.x1 as i64 + 1) * f - 1), y1: a((d.y1 as i64 + 1) * f - 1) }
}

/// Copia lo dañado `dano` de `pix` (`ancho` x `alto`, del programa) a `dst` (la superficie, `ancho·f` x `alto·f`),
/// cada píxel pasado por `conv` y repetido en un cuadrado de `f` x `f`. Devuelve lo dañado en la superficie. Si los
/// tamaños no cuadran con los búferes, no toca nada y da un rectángulo vacío.
pub fn ampliar(pix: &[u32], ancho: u32, alto: u32, f: u32, dano: Rect, dst: &mut [u32], conv: impl Fn(u32) -> u32) -> Rect {
    let f = f.max(1);
    let (an, al) = (ancho as usize, alto as usize);
    let (fu, anf) = (f as usize, an.saturating_mul(f as usize));
    let total = anf.checked_mul(al.saturating_mul(fu));
    if pix.len() < an.saturating_mul(al) || total.is_none_or(|t| dst.len() < t) {
        return Rect::de_tamano(0, 0);
    }
    let d = dano.interseccion(&Rect::de_tamano(ancho, alto));
    if d.vacio() {
        return d;
    }
    let (x0, x1) = (d.x0 as usize, d.x1 as usize);
    for y in d.y0 as usize..=d.y1 as usize {
        // La primera fila de las `f`, ampliada; las demás, copias de ella.
        let primera = y * fu * anf;
        let origen = &pix[y * an + x0..=y * an + x1];
        let fila = &mut dst[primera + x0 * fu..primera + (x1 + 1) * fu];
        for (k, p) in origen.iter().enumerate() {
            fila[k * fu..(k + 1) * fu].fill(conv(*p));
        }
        for r in 1..fu {
            let (antes, despues) = dst.split_at_mut(primera + r * anf);
            despues[x0 * fu..(x1 + 1) * fu].copy_from_slice(&antes[primera + x0 * fu..primera + (x1 + 1) * fu]);
        }
    }
    ampliar_rect(d, f, ancho, alto)
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use alloc::vec;

    #[test]
    fn ajustar_factor() {
        // Lo pedido, entre 1 y el tope.
        assert_eq!(ajustar(2, 3, 410, 192, 4096, 64 << 20), 2);
        assert_eq!(ajustar(0, 3, 410, 192, 4096, 64 << 20), 1);
        assert_eq!(ajustar(u32::MAX, 3, 410, 192, 4096, 64 << 20), 3);
        assert_eq!(ajustar(3, 0, 410, 192, 4096, 64 << 20), 1);
        // Bajado si no cabe: 1500 x 3 = 4500 > 4096 pasa a 2; por memoria, igual.
        assert_eq!(ajustar(3, 3, 1500, 100, 4096, 64 << 20), 2);
        assert_eq!(ajustar(3, 3, 4096, 4096, 4096, 64 << 20), 1);
        assert_eq!(ajustar(3, 3, 2000, 2000, 16384, 64 << 20), 2);
        assert_eq!(ajustar(3, 3, u32::MAX, u32::MAX, u32::MAX, u64::MAX), 1);
        assert_eq!(ajustar(3, 3, 0, 0, 4096, 64 << 20), 3);
    }

    #[test]
    fn coordenadas() {
        assert_eq!(al_programa(0, 2), 0);
        assert_eq!(al_programa(1, 2), 0);
        assert_eq!(al_programa(2, 2), 1);
        assert_eq!(al_programa(819, 2), 409);
        assert_eq!(al_programa(820, 2), 410); // el borde de fuera sigue fuera
        assert_eq!(al_programa(-1, 2), -1);
        assert_eq!(al_programa(-2, 2), -1);
        assert_eq!(al_programa(-3, 2), -2);
        assert_eq!(al_programa(i32::MIN, 3), i32::MIN.div_euclid(3));
        assert_eq!(al_programa(i32::MAX, 3), i32::MAX / 3);
        assert_eq!(al_programa(7, 0), 7);
        assert_eq!(al_programa(7, u32::MAX), 0);
        // Ida y vuelta: cada píxel ampliado vuelve al suyo.
        for f in 1..=3u32 {
            for x in 0..50i32 {
                for k in 0..f as i32 {
                    assert_eq!(al_programa(x * f as i32 + k, f), x);
                }
            }
        }
    }

    #[test]
    fn copia_vecino_mas_cercano() {
        let (an, al) = (3u32, 2u32);
        let pix: alloc::vec::Vec<u32> = (0..6).collect();
        for f in 1..=3u32 {
            let mut dst = vec![0xdead; (an * f * al * f) as usize];
            let d = ampliar(&pix, an, al, f, Rect::de_tamano(an, al), &mut dst, |p| p + 100);
            assert_eq!(d, Rect::de_tamano(an * f, al * f));
            for y in 0..al * f {
                for x in 0..an * f {
                    assert_eq!(dst[(y * an * f + x) as usize], pix[((y / f) * an + x / f) as usize] + 100, "f {} ({}, {})", f, x, y);
                }
            }
        }
        // Solo lo dañado: el píxel (1, 1) a 2x es el cuadrado (2..=3, 2..=3).
        let mut dst = vec![0; 6 * 4];
        let d = ampliar(&pix, an, al, 2, Rect { x0: 1, y0: 1, x1: 1, y1: 1 }, &mut dst, |p| p);
        assert_eq!(d, Rect { x0: 2, y0: 2, x1: 3, y1: 3 });
        let tocados: alloc::vec::Vec<usize> = (0..dst.len()).filter(|&i| dst[i] != 0).collect();
        assert_eq!(tocados, vec![2 * 6 + 2, 2 * 6 + 3, 3 * 6 + 2, 3 * 6 + 3]);
        assert!(tocados.iter().all(|&i| dst[i] == 4));
    }

    #[test]
    fn hostil() {
        let pix = vec![7u32; 12];
        // Daño fuera, negativo, enorme o al revés: se recorta o no hace nada.
        let mut dst = vec![0u32; 4 * 3 * 4];
        for r in [
            Rect { x0: -100, y0: -100, x1: -1, y1: -1 },
            Rect { x0: 4, y0: 0, x1: 9, y1: 2 },
            Rect { x0: 3, y0: 2, x1: 0, y1: 0 },
            Rect { x0: i32::MIN, y0: i32::MIN, x1: i32::MIN, y1: i32::MIN },
        ] {
            assert!(ampliar(&pix, 4, 3, 2, r, &mut dst, |p| p).vacio(), "{:?}", r);
        }
        assert!(dst.iter().all(|&p| p == 0));
        let d = ampliar(&pix, 4, 3, 2, Rect { x0: i32::MIN, y0: i32::MIN, x1: i32::MAX, y1: i32::MAX }, &mut dst, |p| p);
        assert_eq!(d, Rect::de_tamano(8, 6));
        assert!(dst.iter().all(|&p| p == 7));
        // Búferes que no cuadran (la ventana cambió de tamaño entre medias): nada.
        let mut corto = vec![0u32; 10];
        assert!(ampliar(&pix, 4, 3, 2, Rect::de_tamano(4, 3), &mut corto, |p| p).vacio());
        assert!(ampliar(&pix[..5], 4, 3, 1, Rect::de_tamano(4, 3), &mut dst, |p| p).vacio());
        assert!(ampliar(&pix, u32::MAX, u32::MAX, u32::MAX, Rect::de_tamano(4, 3), &mut dst, |p| p).vacio());
        assert!(corto.iter().all(|&p| p == 0));
        // Factor 0: como 1.
        let mut uno = vec![0u32; 12];
        assert_eq!(ampliar(&pix, 4, 3, 0, Rect::de_tamano(4, 3), &mut uno, |p| p), Rect::de_tamano(4, 3));
        assert!(uno.iter().all(|&p| p == 7));
        // Ventana vacía.
        assert!(ampliar(&[], 0, 0, 2, Rect::de_tamano(4, 3), &mut [], |p| p).vacio());
        // ampliar_rect en el borde y fuera.
        assert_eq!(ampliar_rect(Rect { x0: 3, y0: 2, x1: 3, y1: 2 }, 3, 4, 3), Rect { x0: 9, y0: 6, x1: 11, y1: 8 });
        assert!(ampliar_rect(Rect { x0: 4, y0: 3, x1: 5, y1: 5 }, 3, 4, 3).vacio());
    }
}
