//! Las cuentas en coma flotante que hacen falta para calcular los coeficientes de los efectos (seno, coseno,
//! exponencial, raíz). El servicio corre sin `std` ni FPU de hardware (cada operación de `f64` es una llamada): se
//! usan solo al cambiar un parámetro, nunca por muestra. Escritas aquí con series de Taylor y reducción de rango, sin
//! bibliotecas de fuera.

use core::f64::consts::{FRAC_PI_2, LN_2, PI};

/// Seno, con error menor que 1e-12 en todo el rango.
pub fn seno(x: f64) -> f64 {
    if !x.is_finite() {
        return 0.0;
    }
    // A [-π, π], y luego a [-π/2, π/2] con sen(π - x) = sen(x).
    let dos_pi = 2.0 * PI;
    let mut r = x - dos_pi * redondear(x / dos_pi);
    if r > FRAC_PI_2 {
        r = PI - r;
    } else if r < -FRAC_PI_2 {
        r = -PI - r;
    }
    let r2 = r * r;
    // Taylor hasta x^17: en |x| <= π/2 el término siguiente es < 1e-14.
    let mut termino = r;
    let mut suma = r;
    let mut k = 1.0;
    for _ in 0..8 {
        termino *= -r2 / ((k + 1.0) * (k + 2.0));
        suma += termino;
        k += 2.0;
    }
    suma
}

pub fn coseno(x: f64) -> f64 {
    seno(x + FRAC_PI_2)
}

/// El entero más cercano (sin `std`).
fn redondear(x: f64) -> f64 {
    let t = x as i64 as f64;
    if x - t >= 0.5 {
        t + 1.0
    } else if x - t <= -0.5 {
        t - 1.0
    } else {
        t
    }
}

/// e^x para |x| <= 700 (fuera, se satura).
pub fn exp(x: f64) -> f64 {
    if x.is_nan() {
        return 1.0;
    }
    let x = x.clamp(-700.0, 700.0);
    // e^x = 2^k · e^r, con |r| <= ln2 / 2.
    let k = redondear(x / LN_2);
    let r = x - k * LN_2;
    let mut termino = 1.0;
    let mut suma = 1.0;
    for i in 1..20 {
        termino *= r / i as f64;
        suma += termino;
    }
    // 2^k a mano, por los bits del exponente.
    let k = k as i64;
    suma * f64::from_bits(((k + 1023).clamp(1, 2046) as u64) << 52)
}

/// 10^(x/20): la ganancia lineal de `x` decibelios (de amplitud).
pub fn db_a_lineal(db: f64) -> f64 {
    exp(db * core::f64::consts::LN_10 / 20.0)
}

/// Raíz cuadrada por Newton (0 para lo negativo o lo que no es número).
pub fn raiz(x: f64) -> f64 {
    if !(x > 0.0) || !x.is_finite() {
        return 0.0;
    }
    // Primera aproximación por los bits del exponente, y seis pasos de Newton.
    let mut y = f64::from_bits((x.to_bits() >> 1) + (1023u64 << 51));
    for _ in 0..6 {
        y = 0.5 * (y + x / y);
    }
    y
}

/// Logaritmo en base 10 de un entero positivo, en centésimas (para pasar niveles a decibelios en la pantalla, sin
/// coma flotante): 100 · log10(x). Exacto a una centésima para x de 1 a 2^62.
pub fn log10_centesimas(x: u64) -> i32 {
    if x == 0 {
        return i32::MIN;
    }
    // log2 entero y fracción por cuadrados sucesivos (8 bits de fracción), y log10 = log2 · 0,30103.
    let entero = 63 - x.leading_zeros() as i64;
    // La mantisa en [1, 2) con 62 bits de fracción (al cuadrado cabe en 128).
    let mut m: u128 = if entero <= 62 { (x as u128) << (62 - entero) as u32 } else { (x as u128) >> (entero - 62) as u32 };
    let mut frac: i64 = 0;
    for i in 0..16 {
        m = (m * m) >> 62;
        if m >= (2u128 << 62) {
            m >>= 1;
            frac |= 1 << (15 - i);
        }
    }
    let log2_q16 = (entero << 16) + frac;
    ((log2_q16 * 30_103 / 1000 + (1 << 15)) >> 16) as i32
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn contra_std() {
        for i in -2000..2000 {
            let x = i as f64 * 0.0137;
            assert!((seno(x) - x.sin()).abs() < 1e-12, "sen {x}");
            assert!((coseno(x) - x.cos()).abs() < 1e-12, "cos {x}");
        }
        for i in -400..400 {
            let x = i as f64 * 0.1;
            let e = x.exp();
            assert!(((exp(x) - e) / e).abs() < 1e-13, "exp {x}");
        }
        for db in [-60.0, -12.0, -3.0, 0.0, 6.0, 12.0f64] {
            assert!((db_a_lineal(db) - 10f64.powf(db / 20.0)).abs() < 1e-12);
        }
        for x in [1e-9, 0.5, 2.0, 9.0, 1e9] {
            assert!(((raiz(x) - x.sqrt()) / x.sqrt()).abs() < 1e-14);
        }
        assert_eq!(raiz(-1.0), 0.0);
        assert_eq!(raiz(f64::NAN), 0.0);
        assert_eq!(seno(f64::INFINITY), 0.0);
        for x in [1u64, 2, 10, 1000, 32768, 1 << 40, u64::MAX >> 2] {
            let esperado = (100.0 * (x as f64).log10()).round() as i32;
            assert!((log10_centesimas(x) - esperado).abs() <= 1, "{x}: {} {}", log10_centesimas(x), esperado);
        }
    }
}
