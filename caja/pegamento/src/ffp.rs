//! mathffp.library nativa: la coma flotante rápida de Motorola (FFP) del Amiga, que en un Amiga está en la ROM y un
//! AmigaOS en disco no trae. La piden LightWave 5 (Layout y Modeler) y la mathtrans.library del usuario.
//!
//! Escrita del autodoc de mathffp (Includes and Autodocs 3.1) y de `mathffp_lib.fd`. El formato FFP en 32 bits: la
//! mantisa en los bits 31-8 (24 bits, normalizada: el de arriba a 1), el signo en el bit 7 y el exponente en los bits
//! 6-0, en exceso 64: valor = mantisa / 2^24 · 2^(exponente - 64). El cero es 0 (todo a 0). No hay NaN ni infinitos: lo
//! que se sale se queda en el mayor (o el menor) número con su signo, y lo que no llega es 0.
//!
//! Los registros (el autodoc y el `.fd` no dicen lo mismo para `SPCmp`; manda el formato del `.fd`, el que siguen los
//! compiladores): `SPAdd`, `SPSub`, `SPMul`, `SPDiv` y `SPCmp` toman D1 y D0, y operan "D0 con D1" como la biblioteca
//! FFP de Motorola (el minuendo y el dividendo, en D0): `SPSub` = D0 - D1 y `SPDiv` = D0 / D1 ("fnum3 = fnum2/fnum1",
//! con fnum1 en D1). `SPCmp` da +1 si D0 > D1, -1 si D0 < D1 y 0 si son iguales, y deja esos códigos de condición
//! (el `TST.L D0` de `crear_biblioteca_nativa_con_cc`). `SPTst` mira D1. Se calcula en `f64` (exacto para 24 bits) y
//! se redondea al más cercano al volver a FFP.
//!
//! Sin dependencias: se prueba en el PC con `rustc --edition 2024 --test caja/pegamento/src/ffp.rs`.

pub const FUNCIONES: [&str; 12] =
    ["SPFix", "SPFlt", "SPCmp", "SPTst", "SPAbs", "SPNeg", "SPAdd", "SPSub", "SPMul", "SPDiv", "SPFloor", "SPCeil"];

/// Los índices de `SPCmp` y `SPTst`, que dejan los códigos de condición.
pub const CON_CC: [usize; 2] = [2, 3];

/// El mayor número FFP positivo: mantisa toda a 1, exponente 127.
const MAXIMO: u32 = 0xFFFF_FF7F;

/// 2^n en `f64`, sin libm (n de -200 a 200).
fn pot2(n: i32) -> f64 {
    let mut x = 1.0f64;
    if n >= 0 {
        for _ in 0..n {
            x *= 2.0;
        }
    } else {
        for _ in 0..(-n) {
            x *= 0.5;
        }
    }
    x
}

/// De FFP a `f64` (exacto).
pub fn a_f64(v: u32) -> f64 {
    let m = v >> 8;
    if m == 0 {
        return 0.0;
    }
    let e = (v & 0x7F) as i32 - 64;
    let x = m as f64 * pot2(e - 24);
    if v & 0x80 != 0 { -x } else { x }
}

/// De `f64` a FFP, redondeando al más cercano; lo que se sale, al mayor con su signo; lo que no llega, 0.
pub fn de_f64(x: f64) -> u32 {
    if x != x || x == 0.0 {
        return 0;
    }
    let neg = x < 0.0;
    let mut a = if neg { -x } else { x };
    if a >= pot2(63) {
        return (MAXIMO & !0x80) | if neg { 0x80 } else { 0 };
    }
    // a = f · 2^e con f en [0,5, 1).
    let mut e = 0i32;
    while a >= 1.0 {
        a *= 0.5;
        e += 1;
    }
    while a < 0.5 {
        a *= 2.0;
        e -= 1;
        if e < -80 {
            return 0;
        }
    }
    let mut m = (a * 16_777_216.0 + 0.5) as u64;
    if m >= 1 << 24 {
        m >>= 1;
        e += 1;
    }
    let ex = e + 64;
    if ex > 127 {
        return (MAXIMO & !0x80) | if neg { 0x80 } else { 0 };
    }
    if ex < 0 {
        return 0;
    }
    ((m as u32) << 8) | if neg { 0x80 } else { 0 } | ex as u32
}

fn signo(a: f64, b: f64) -> u32 {
    if a > b {
        1
    } else if a < b {
        u32::MAX
    } else {
        0
    }
}

/// Parte entera hacia cero, sin libm (desde 2^24 todo FFP ya es entero).
fn truncar(x: f64) -> f64 {
    if x >= 16_777_216.0 || x <= -16_777_216.0 {
        return x;
    }
    x as i64 as f64
}

/// La función `i` con D0 y D1: D0 al volver.
pub fn llamar(i: usize, d0: u32, d1: u32) -> u32 {
    let (x0, x1) = (a_f64(d0), a_f64(d1));
    match i {
        // SPFix: la parte entera, hacia cero; si no cabe, el mayor entero con su signo.
        0 => {
            let t = truncar(x0);
            if t >= 2_147_483_647.0 {
                i32::MAX as u32
            } else if t <= -2_147_483_648.0 {
                i32::MIN as u32
            } else {
                t as i64 as i32 as u32
            }
        }
        1 => de_f64(d0 as i32 as f64),
        2 => signo(x0, x1),
        3 => signo(x1, 0.0),
        4 => d0 & !0x80,
        5 => {
            if d0 >> 8 == 0 {
                0
            } else {
                d0 ^ 0x80
            }
        }
        6 => de_f64(x0 + x1),
        7 => de_f64(x0 - x1),
        8 => de_f64(x0 * x1),
        // SPDiv entre 0: el mayor con el signo del dividendo (no hay infinito en FFP).
        9 => {
            if x1 == 0.0 {
                if d0 >> 8 == 0 { 0 } else { (MAXIMO & !0x80) | (d0 & 0x80) }
            } else {
                de_f64(x0 / x1)
            }
        }
        10 | 11 => {
            let t = truncar(x0);
            let r = if i == 10 && t > x0 {
                t - 1.0
            } else if i == 11 && t < x0 {
                t + 1.0
            } else {
                t
            };
            de_f64(r)
        }
        _ => 0,
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    const UNO: u32 = 0x8000_0041; // 0,5 · 2^1
    const DOS: u32 = 0x8000_0042;

    #[test]
    fn formato() {
        assert_eq!(a_f64(UNO), 1.0);
        assert_eq!(a_f64(DOS), 2.0);
        assert_eq!(a_f64(UNO | 0x80), -1.0);
        assert_eq!(de_f64(1.0), UNO);
        assert_eq!(de_f64(-2.0), DOS | 0x80);
        assert_eq!(de_f64(0.0), 0);
        // 0,1 en FFP: 0xCCCCCD7D (la mantisa redondeada).
        assert_eq!(de_f64(0.1), 0xCCCC_CD3D);
        assert_eq!(a_f64(MAXIMO & !0x80) > 9.2e18, true);
        for n in [-100000i32, -3, -1, 0, 1, 7, 12345, 16777215, i32::MAX, i32::MIN] {
            assert_eq!(llamar(0, llamar(1, n as u32, 0), 0) as i32, if n.unsigned_abs() > 16777216 { llamar(0, de_f64(n as f64), 0) as i32 } else { n });
        }
    }

    #[test]
    fn operaciones() {
        let tres = de_f64(3.0);
        let siete = de_f64(7.0);
        assert_eq!(a_f64(llamar(6, tres, siete)), 10.0);
        assert_eq!(a_f64(llamar(7, siete, tres)), 4.0); // D0 - D1
        assert_eq!(a_f64(llamar(8, tres, siete)), 21.0);
        assert_eq!(a_f64(llamar(9, de_f64(21.0), siete)), 3.0); // D0 / D1
        assert_eq!(llamar(2, siete, tres), 1);
        assert_eq!(llamar(2, tres, siete), u32::MAX);
        assert_eq!(llamar(2, tres, tres), 0);
        assert_eq!(llamar(3, 0, tres | 0x80), u32::MAX);
        assert_eq!(llamar(3, 0, 0), 0);
        assert_eq!(a_f64(llamar(10, de_f64(-2.5), 0)), -3.0);
        assert_eq!(a_f64(llamar(11, de_f64(-2.5), 0)), -2.0);
        assert_eq!(a_f64(llamar(11, de_f64(2.25), 0)), 3.0);
        assert_eq!(llamar(0, de_f64(-2.75), 0) as i32, -2);
        assert_eq!(a_f64(llamar(4, tres | 0x80, 0)), 3.0);
        assert_eq!(llamar(5, 0, 0), 0);
    }

    #[test]
    fn hostil() {
        // Lo que se sale se queda en el mayor; lo que no llega, 0; entre 0, el mayor; basura no cuelga.
        let gran = MAXIMO & !0x80;
        assert_eq!(llamar(8, gran, gran), gran);
        assert_eq!(llamar(8, gran | 0x80, gran), gran | 0x80);
        assert_eq!(llamar(9, de_f64(1e-15), de_f64(1e15)), 0);
        assert_eq!(llamar(9, UNO, 0), gran);
        assert_eq!(llamar(9, 0, 0), 0);
        let mut x = 0x1234_5678u32;
        for _ in 0..20000 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            for f in 0..12 {
                let r = llamar(f, x, x.rotate_left(7));
                if ![0, 2, 3, 4, 5].contains(&f) && r != 0 {
                    assert!(r >> 31 == 1, "{} {:x} da {:x} sin normalizar", f, x, r);
                }
            }
        }
    }
}
