//! S27: las medidas, los radios y las animaciones de DMS que usan los avisos, el centro de control y el Dash.
//!
//! Portado de DMS (DankMaterialShell v2.1.2: `Common/Theme.qml`, `DCommon/Common/Shape.js`; MIT, copyright (c) 2025
//! Avenge Media LLC; aviso entero en `docs/licencias/DankMaterialShell-MIT.txt`). Los radios van con la "fuerza" del
//! escritorio del usuario en shiva (`radiusStrength = 38`: escala 0,76) y el radio de las ventanas emergentes con el de
//! su niri (`niriLayoutRadiusOverride = 3`, que DMS usa como `windowRadius`).
//!
//! Las curvas son las cúbicas de Bézier de Material 3 que usa DMS; el muelle, el `SpringMotion` de Qt con la rigidez y
//! la amortiguación que DMS calcula para cada duración. Sin nada de DashOS ni coma flotante del sistema (`f32` de
//! `core` sí): se prueba en el PC con `rustc --edition 2024 --test usuario/src/dms_medidas.rs`.

/// Espaciados (`Theme.spacing*`).
pub const XXS: i32 = 2;
pub const XS: i32 = 4;
pub const S: i32 = 8;
pub const M: i32 = 12;
pub const L: i32 = 16;
pub const XL: i32 = 24;
/// Letras (`fontSize*`, con escala 1).
pub const LETRA_PEQUENA: u32 = 12;
pub const LETRA_MEDIANA: u32 = 14;
pub const LETRA_GRANDE: u32 = 16;

/// La fuerza de los radios (`radiusStrength`), de 0 a 100; 50 es la escala 1. La de shiva.
pub const FUERZA_RADIOS: i32 = 38;
/// El radio de las ventanas emergentes (`windowRadius`; en niri, `niriLayoutRadiusOverride`). El de shiva.
pub const RADIO_VENTANA: i32 = 3;

/// Un radio de `Shape.js` (xs 4, s 8, m 12, l 16, xl 28...) con la fuerza: `round(token × fuerza / 50)`.
pub const fn radio(token: i32) -> i32 {
    (token * FUERZA_RADIOS + 25) / 50
}

/// El radio de una píldora de alto `h` (`fullRadius`): la mitad, con la escala si es menor que 1.
pub const fn pildora(h: i32) -> i32 {
    if FUERZA_RADIOS < 50 { (h * FUERZA_RADIOS + 50) / 100 } else { h / 2 }
}

/// Una curva de Bézier cúbica de (0,0) a (1,1) con sus dos puntos de control, como `Easing.BezierSpline` de Qt.
#[derive(Clone, Copy, Debug)]
pub struct Curva(pub f32, pub f32, pub f32, pub f32);

pub const ENFATIZADA_DECELERA: Curva = Curva(0.05, 0.7, 0.1, 1.0);
pub const ESTANDAR_ACELERA: Curva = Curva(0.3, 0.0, 1.0, 1.0);
pub const ESTANDAR: Curva = Curva(0.2, 0.0, 0.0, 1.0);
pub const EXPRESIVA_ESPACIAL: Curva = Curva(0.38, 1.21, 0.22, 1.0);
pub const EXPRESIVA_RAPIDA: Curva = Curva(0.42, 1.67, 0.21, 0.9);
pub const EXPRESIVA_EFECTOS: Curva = Curva(0.34, 0.8, 0.34, 1.0);

impl Curva {
    fn bez(a: f32, b: f32, t: f32) -> f32 {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    }

    /// El valor en `x` (de 0 a 1, lo que va del tiempo): se busca el `t` de la curva cuya x es esa (bisección) y se da
    /// su y.
    pub fn en(&self, x: f32) -> f32 {
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..24 {
            let t = (lo + hi) * 0.5;
            if Self::bez(self.0, self.2, t) < x {
                lo = t;
            } else {
                hi = t;
            }
        }
        Self::bez(self.1, self.3, (lo + hi) * 0.5)
    }
}

/// Lo que va de una animación que empezó en `desde_us` y dura `dura_ms`, de 0 a 1 (con duración 0, ya acabó).
pub fn avance(desde_us: u64, ahora_us: u64, dura_ms: u32) -> f32 {
    if dura_ms == 0 {
        return 1.0;
    }
    (ahora_us.saturating_sub(desde_us) as f32 / (dura_ms as f32 * 1000.0)).min(1.0)
}

fn raiz(x: f32) -> f32 {
    // Newton, sin `std`.
    if x <= 0.0 {
        return 0.0;
    }
    let mut r = if x > 1.0 { x } else { 1.0 };
    for _ in 0..30 {
        r = 0.5 * (r + x / r);
    }
    r
}

fn exp(x: f32) -> f32 {
    // e^x = (e^(x/64))^64, con la serie para lo pequeño.
    if x < -60.0 {
        return 0.0;
    }
    let y = x / 64.0;
    let mut e = 1.0 + y + y * y / 2.0 + y * y * y / 6.0 + y * y * y * y / 24.0;
    for _ in 0..6 {
        e *= e;
    }
    e
}

fn seno(x: f32) -> f32 {
    // Reducido a [-π, π] y la serie de Taylor hasta x^11.
    let dos_pi = 6.283_185_5f32;
    let mut x = x % dos_pi;
    if x > 3.141_592_7 {
        x -= dos_pi;
    }
    if x < -3.141_592_7 {
        x += dos_pi;
    }
    let x2 = x * x;
    x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0 * (1.0 - x2 / 72.0 * (1.0 - x2 / 110.0)))))
}

fn coseno(x: f32) -> f32 {
    seno(x + 1.570_796_3)
}

/// El muelle de DMS (`springPreset("default", duracion)`: rigidez 300/f² y amortiguación 24/f con f = duración/500,
/// masa 1), de 0 a 1: dónde está a los `t_ms`. Subamortiguado: pasa algo de 1 y vuelve.
pub fn muelle(t_ms: f32, duracion_ms: u32) -> f32 {
    if duracion_ms == 0 || t_ms <= 0.0 {
        return if duracion_ms == 0 { 1.0 } else { 0.0 };
    }
    let f = duracion_ms as f32 / 500.0;
    let (k, c) = (300.0 / (f * f), 24.0 / f);
    let w0 = raiz(k);
    let z = c / (2.0 * w0);
    let t = t_ms / 1000.0;
    if z >= 1.0 {
        return 1.0 - exp(-w0 * t) * (1.0 + w0 * t);
    }
    let wd = w0 * raiz(1.0 - z * z);
    1.0 - exp(-z * w0 * t) * (coseno(wd * t) + z * w0 / wd * seno(wd * t))
}

/// Cuánto tarda el muelle en quedarse quieto (a menos del 0,5 %): para dejar de repintar.
pub fn muelle_quieto_ms(duracion_ms: u32) -> u32 {
    duracion_ms.saturating_mul(3).max(1)
}

/// Mezcla `a` y `b` (0 → a, 1 → b), redondeando.
pub fn entre(a: i32, b: i32, x: f32) -> i32 {
    let v = a as f32 + (b - a) as f32 * x;
    if v >= 0.0 { (v + 0.5) as i32 } else { (v - 0.5) as i32 }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn radios_de_shiva() {
        // Con 38: xs 3, s 6, m 9, l 12, xl 21 (lo que dice el informe de su DMS).
        assert_eq!((radio(4), radio(8), radio(12), radio(16), radio(28)), (3, 6, 9, 12, 21));
        assert_eq!(pildora(40), 15);
        assert_eq!(pildora(64), 24);
    }

    #[test]
    fn curvas() {
        for c in [ENFATIZADA_DECELERA, ESTANDAR_ACELERA, ESTANDAR, EXPRESIVA_ESPACIAL, EXPRESIVA_RAPIDA, EXPRESIVA_EFECTOS] {
            assert_eq!(c.en(0.0), 0.0);
            assert_eq!(c.en(1.0), 1.0);
            assert!(c.en(-3.0) == 0.0 && c.en(7.0) == 1.0);
        }
        // La de entrar va deprisa al principio; la de salir, despacio.
        assert!(ENFATIZADA_DECELERA.en(0.2) > 0.6);
        assert!(ESTANDAR_ACELERA.en(0.2) < 0.1);
        // Las expresivas se pasan de 1 y vuelven.
        assert!((0..100).any(|i| EXPRESIVA_RAPIDA.en(i as f32 / 100.0) > 1.0));
        // Monótona en lo que no se pasa.
        let mut antes = 0.0;
        for i in 0..=100 {
            let v = ESTANDAR.en(i as f32 / 100.0);
            assert!(v + 1e-4 >= antes);
            antes = v;
        }
    }

    #[test]
    fn el_muelle() {
        assert_eq!(muelle(0.0, 250), 0.0);
        assert_eq!(muelle(10.0, 0), 1.0);
        // Con 250 ms (rigidez 1200, amortiguación 48): se pasa algo de 1 y a los 750 ms está quieto.
        let max = (0..1000).map(|t| muelle(t as f32, 250)).fold(0.0f32, f32::max);
        assert!(max > 1.0 && max < 1.15, "{}", max);
        assert!((muelle(muelle_quieto_ms(250) as f32, 250) - 1.0).abs() < 0.005);
        assert!((muelle(muelle_quieto_ms(200) as f32, 200) - 1.0).abs() < 0.005);
        // Tiempos enormes no rompen.
        assert!((muelle(1e9, 250) - 1.0).abs() < 0.01);
    }

    #[test]
    fn el_avance() {
        assert_eq!(avance(100, 50, 200), 0.0);
        assert_eq!(avance(0, 100_000, 200), 0.5);
        assert_eq!(avance(0, u64::MAX, 200), 1.0);
        assert_eq!(avance(0, 0, 0), 1.0);
        assert_eq!((entre(0, 10, 0.5), entre(10, 0, 0.26), entre(-4, 4, 0.5)), (5, 7, 0));
    }
}
