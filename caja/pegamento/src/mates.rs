//! mathieeesingbas.library nativa: la coma flotante de precisión simple IEEE del Amiga (en la ROM de un Amiga; un
//! AmigaOS en disco no la trae). La pide MUI 5 (muimaster, por su biblioteca de C) y mathieeesingtrans.
//!
//! Escrita del autodoc (`mathieeesingbas.doc`, NDK 3.2) y su `.fd`. Los números van en D0 y D1 como los bits de un
//! `float`; `IEEESPCmp` e `IEEESPTst` devuelven 1, 0 o -1 y dejan los códigos de condición (la entrada salta a un
//! `TST.L D0` al volver: `crear_biblioteca_nativa_con_cc`). `IEEESPFix` redondea hacia cero y, si no cabe, da el
//! entero más grande con su signo.

use caja_exec::{Exec, Fallo, Memoria, Registros};

pub(crate) const FUNCIONES: [&str; 12] = [
    "IEEESPFix",
    "IEEESPFlt",
    "IEEESPCmp",
    "IEEESPTst",
    "IEEESPAbs",
    "IEEESPNeg",
    "IEEESPAdd",
    "IEEESPSub",
    "IEEESPMul",
    "IEEESPDiv",
    "IEEESPFloor",
    "IEEESPCeil",
];

/// Crea la biblioteca con los números nativos desde `n0`.
pub(crate) fn instalar<M: Memoria>(ex: &mut Exec<M>, n0: u16) -> Result<u32, Fallo> {
    let nums: alloc::vec::Vec<u16> = (0..FUNCIONES.len() as u16).map(|i| n0 + i).collect();
    ex.crear_biblioteca_nativa_con_cc("mathieeesingbas.library", 40, 1, &nums, 34, &[n0 + 2, n0 + 3])
}

fn f(x: u32) -> f32 {
    f32::from_bits(x)
}

fn signo(o: Option<core::cmp::Ordering>) -> u32 {
    match o {
        Some(core::cmp::Ordering::Greater) => 1,
        Some(core::cmp::Ordering::Less) => u32::MAX,
        // Iguales, o algún NaN: 0 (el autodoc no dice nada de NaN).
        _ => 0,
    }
}

/// `floor` (o `ceil`, con `arriba`): sin libm. Desde 2^23 todo `f32` ya es entero.
fn entero(x: f32, arriba: bool) -> f32 {
    if x.is_nan() || x.abs() >= 8_388_608.0 {
        return x;
    }
    let t = x as i32 as f32;
    if !arriba && t > x {
        t - 1.0
    } else if arriba && t < x {
        t + 1.0
    } else {
        t
    }
}

/// La función `i` con los registros `r`: D0 al volver.
pub(crate) fn llamar(i: usize, r: &Registros) -> Option<u32> {
    let (a, b) = (r.d[0], r.d[1]);
    Some(match i {
        0 => {
            let x = f(a);
            if x.is_nan() {
                0
            } else if x >= 2_147_483_648.0 {
                i32::MAX as u32
            } else if x < -2_147_483_648.0 {
                i32::MIN as u32
            } else {
                x as i32 as u32
            }
        }
        1 => (a as i32 as f32).to_bits(),
        2 => signo(f(a).partial_cmp(&f(b))),
        3 => signo(f(a).partial_cmp(&0.0)),
        4 => a & 0x7FFF_FFFF,
        5 => a ^ 0x8000_0000,
        6 => (f(a) + f(b)).to_bits(),
        7 => (f(a) - f(b)).to_bits(),
        8 => (f(a) * f(b)).to_bits(),
        9 => (f(a) / f(b)).to_bits(),
        10 => entero(f(a), false).to_bits(),
        11 => entero(f(a), true).to_bits(),
        _ => return None,
    })
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn r(a: f32, b: f32) -> Registros {
        let mut r = Registros::default();
        r.d[0] = a.to_bits();
        r.d[1] = b.to_bits();
        r
    }

    #[test]
    fn cuentas() {
        let v = |i, a, b| f32::from_bits(llamar(i, &r(a, b)).unwrap());
        assert_eq!(v(6, 1.5, 2.25), 3.75);
        assert_eq!(v(7, 1.5, 2.25), -0.75);
        assert_eq!(v(8, -3.0, 0.5), -1.5);
        assert_eq!(v(9, 1.0, 4.0), 0.25);
        assert_eq!(v(10, -1.5, 0.0), -2.0);
        assert_eq!(v(11, -1.5, 0.0), -1.0);
        assert_eq!(v(4, -2.0, 0.0), 2.0);
        assert_eq!(v(5, 2.0, 0.0), -2.0);
        assert!(v(9, 1.0, 0.0).is_infinite());
        let i = |i, a| llamar(i, &r(a, 0.0)).unwrap() as i32;
        assert_eq!(i(0, -7.9), -7);
        assert_eq!(i(0, 1e20), i32::MAX);
        assert_eq!(i(0, -1e20), i32::MIN);
        assert_eq!(i(0, f32::NAN), 0);
        assert_eq!(i(2, 1.0), 1);
        assert_eq!(llamar(2, &r(1.0, 2.0)).unwrap() as i32, -1);
        assert_eq!(llamar(3, &r(-0.0, 0.0)).unwrap(), 0);
        let mut x = Registros::default();
        x.d[0] = (-5i32) as u32;
        assert_eq!(f32::from_bits(llamar(1, &x).unwrap()), -5.0);
        assert!(llamar(12, &x).is_none());
    }
}
