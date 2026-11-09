//! Grupos que se colocan solos (K4), como los de MUI: cada objeto dice su mínimo, su máximo y su peso; un grupo
//! horizontal o vertical los pone en fila, con un espacio entre ellos y un margen alrededor, y reparte lo que sobra
//! según los pesos sin pasar del máximo de nadie. A lo ancho de la fila (lo alto en uno horizontal), cada uno ocupa
//! lo que haya hasta su máximo, centrado si no llega.

use alloc::vec::Vec;

use crate::lienzo::Rect;

/// Sin máximo.
pub const SIN_MAX: i32 = i32::MAX / 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Medida {
    pub min: (i32, i32),
    pub max: (i32, i32),
    /// El peso al repartir lo que sobra (100 por defecto, como en MUI).
    pub peso: u32,
}

impl Medida {
    pub const fn fija(w: i32, h: i32) -> Self {
        Self { min: (w, h), max: (w, h), peso: 100 }
    }

    /// Al menos `w` × `h`, y crece todo lo que haga falta.
    pub const fn elastica(w: i32, h: i32) -> Self {
        Self { min: (w, h), max: (SIN_MAX, SIN_MAX), peso: 100 }
    }
}

pub enum Nodo {
    Hoja(Medida),
    Grupo { horizontal: bool, espacio: i32, margen: i32, hijos: Vec<Nodo> },
}

/// Lo que mide un nodo: el de un grupo sale de sus hijos.
pub fn medir(n: &Nodo) -> Medida {
    match n {
        Nodo::Hoja(m) => *m,
        Nodo::Grupo { horizontal, espacio, margen, hijos } => {
            let ms: Vec<Medida> = hijos.iter().map(medir).collect();
            let huecos = espacio * (ms.len() as i32 - 1).max(0) + 2 * margen;
            // A lo largo: la suma; a lo ancho: el mayor de los mínimos y el menor máximo que no baje de él.
            let largo = |m: &Medida, de_min: bool| -> (i32, i32) {
                let (a, b) = if de_min { m.min } else { m.max };
                if *horizontal { (a, b) } else { (b, a) }
            };
            let suma = |de_min: bool| -> i32 {
                ms.iter().map(|m| largo(m, de_min).0).fold(0i32, |s, v| s.saturating_add(v)).saturating_add(huecos)
            };
            let ancho_min = ms.iter().map(|m| largo(m, true).1).max().unwrap_or(0) + 2 * margen;
            let ancho_max = ms.iter().map(|m| largo(m, false).1).max().unwrap_or(0).saturating_add(2 * margen).max(ancho_min);
            let (min_l, max_l) = (suma(true), suma(false).min(SIN_MAX));
            let peso = ms.iter().map(|m| m.peso).max().unwrap_or(100);
            if *horizontal {
                Medida { min: (min_l, ancho_min), max: (max_l, ancho_max.min(SIN_MAX)), peso }
            } else {
                Medida { min: (ancho_min, min_l), max: (ancho_max.min(SIN_MAX), max_l), peso }
            }
        }
    }
}

/// Reparte `total` entre unos mínimos, máximos y pesos: cada uno empieza en su mínimo y lo que sobra va por pesos,
/// sin pasar de su máximo. Lo que no quepa en nadie queda sin repartir.
pub fn repartir(total: i32, mins: &[i32], maxs: &[i32], pesos: &[u32]) -> Vec<i32> {
    let mut v: Vec<i32> = mins.to_vec();
    let mut sobra = total - mins.iter().sum::<i32>();
    while sobra > 0 {
        let libres: Vec<usize> = (0..v.len()).filter(|&i| v[i] < maxs[i] && pesos[i] > 0).collect();
        let peso_total: u64 = libres.iter().map(|&i| pesos[i] as u64).sum();
        if libres.is_empty() || peso_total == 0 {
            break;
        }
        let mut dado = 0;
        for &i in &libres {
            let parte = (sobra as u64 * pesos[i] as u64 / peso_total) as i32;
            let parte = parte.min(maxs[i] - v[i]);
            v[i] += parte;
            dado += parte;
        }
        // Lo que queda por redondeo, de uno en uno a los que aún caben, por orden.
        if dado == 0 {
            for &i in &libres {
                if sobra - dado == 0 {
                    break;
                }
                if v[i] < maxs[i] {
                    v[i] += 1;
                    dado += 1;
                }
            }
        }
        sobra -= dado;
    }
    v
}

/// Coloca un nodo en `r`; deja en `out` el rectángulo de cada hoja, en orden.
pub fn colocar(n: &Nodo, r: Rect, out: &mut Vec<Rect>) {
    match n {
        Nodo::Hoja(_) => out.push(r),
        Nodo::Grupo { horizontal, espacio, margen, hijos } => {
            let dentro = r.dentro(*margen);
            let ms: Vec<Medida> = hijos.iter().map(medir).collect();
            let (largo, ancho) = if *horizontal { (dentro.w, dentro.h) } else { (dentro.h, dentro.w) };
            let huecos = espacio * (ms.len() as i32 - 1).max(0);
            let eje = |p: (i32, i32)| if *horizontal { p.0 } else { p.1 };
            let cruz = |p: (i32, i32)| if *horizontal { p.1 } else { p.0 };
            let mins: Vec<i32> = ms.iter().map(|m| eje(m.min)).collect();
            let maxs: Vec<i32> = ms.iter().map(|m| eje(m.max)).collect();
            let pesos: Vec<u32> = ms.iter().map(|m| m.peso).collect();
            let tams = repartir(largo - huecos, &mins, &maxs, &pesos);
            let mut pos = 0;
            for ((hijo, m), t) in hijos.iter().zip(&ms).zip(&tams) {
                // A lo ancho: todo lo que haya hasta su máximo, centrado.
                let a = ancho.min(cruz(m.max)).max(cruz(m.min));
                let off = (ancho - a) / 2;
                let rh = if *horizontal {
                    Rect::new(dentro.x + pos, dentro.y + off, *t, a)
                } else {
                    Rect::new(dentro.x + off, dentro.y + pos, a, *t)
                };
                colocar(hijo, rh, out);
                pos += t + espacio;
            }
        }
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use alloc::vec;

    fn fila(hijos: Vec<Nodo>) -> Nodo {
        Nodo::Grupo { horizontal: true, espacio: 8, margen: 16, hijos }
    }

    fn columna(hijos: Vec<Nodo>) -> Nodo {
        Nodo::Grupo { horizontal: false, espacio: 8, margen: 16, hijos }
    }

    fn colocados(n: &Nodo, r: Rect) -> Vec<Rect> {
        let mut out = Vec::new();
        colocar(n, r, &mut out);
        out
    }

    #[test]
    fn dos_botones_iguales() {
        // Dos botones elásticos de 80 × 40 mínimo en 400 × 100: 16 de margen y 8 entre ellos.
        let n = fila(vec![Nodo::Hoja(Medida::elastica(80, 40)), Nodo::Hoja(Medida::elastica(80, 40))]);
        assert_eq!(medir(&n).min, (16 + 80 + 8 + 80 + 16, 16 + 40 + 16));
        let r = colocados(&n, Rect::new(0, 0, 400, 100));
        assert_eq!(r, [Rect::new(16, 16, 180, 68), Rect::new(204, 16, 180, 68)]);
    }

    #[test]
    fn pesos_y_maximos() {
        // Pesos 100 y 300 reparten 200 de sobra en 50 y 150; un fijo no crece y cede lo suyo.
        let mut a = Medida::elastica(0, 10);
        a.peso = 100;
        let mut b = Medida::elastica(0, 10);
        b.peso = 300;
        assert_eq!(repartir(200, &[0, 0], &[SIN_MAX, SIN_MAX], &[100, 300]), [50, 150]);
        let n = Nodo::Grupo { horizontal: true, espacio: 0, margen: 0, hijos: vec![Nodo::Hoja(a), Nodo::Hoja(Medida::fija(40, 10)), Nodo::Hoja(b)] };
        let r = colocados(&n, Rect::new(0, 0, 240, 10));
        assert_eq!(r.iter().map(|r| r.w).collect::<Vec<_>>(), [50, 40, 150]);
        // Un máximo corta su parte y lo demás va a los otros.
        assert_eq!(repartir(100, &[0, 0], &[10, SIN_MAX], &[100, 100]), [10, 90]);
        // Un reparto que no es exacto no pierde píxeles.
        assert_eq!(repartir(10, &[0, 0, 0], &[SIN_MAX; 3], &[100; 3]).iter().sum::<i32>(), 10);
    }

    #[test]
    fn a_lo_ancho_centrado() {
        // Un botón de altura fija en una fila más alta: centrado.
        let n = Nodo::Grupo { horizontal: true, espacio: 0, margen: 0, hijos: vec![Nodo::Hoja(Medida { min: (50, 20), max: (SIN_MAX, 20), peso: 100 })] };
        assert_eq!(colocados(&n, Rect::new(0, 0, 100, 60)), [Rect::new(0, 20, 100, 20)]);
    }

    #[test]
    fn grupos_dentro_de_grupos() {
        // Una ventana: un texto arriba, y abajo una fila con dos botones.
        let n = columna(vec![
            Nodo::Hoja(Medida { min: (100, 20), max: (SIN_MAX, 20), peso: 100 }),
            fila(vec![Nodo::Hoja(Medida::fija(80, 40)), Nodo::Hoja(Medida::fija(80, 40))]),
        ]);
        let m = medir(&n);
        // Lo ancho: la fila (16 + 80 + 8 + 80 + 16 = 200) más el margen de la columna; lo alto: 20 + 8 + 72 + 32.
        assert_eq!(m.min, (232, 132));
        let r = colocados(&n, Rect::new(0, 0, 300, 200));
        assert_eq!(r[0], Rect::new(16, 16, 268, 20));
        // La fila de botones fijos no crece: 200 de ancho, centrada en los 268 de dentro.
        assert_eq!(r[1].w, 80);
        assert_eq!(r[1].x, 16 + (268 - 200) / 2 + 16);
        assert_eq!(r[2].x, r[1].x + 80 + 8);
    }

    #[test]
    fn sin_sitio() {
        // Menos sitio que el mínimo: cada uno se queda en su mínimo (se sale; no se aplasta).
        let n = fila(vec![Nodo::Hoja(Medida::fija(80, 40)), Nodo::Hoja(Medida::fija(80, 40))]);
        let r = colocados(&n, Rect::new(0, 0, 100, 100));
        assert_eq!((r[0].w, r[1].w), (80, 80));
    }
}
