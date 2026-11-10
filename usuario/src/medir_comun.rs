//! Lo común del banco de medidas de la multitarea (M0, docs/MULTITAREA_LOOP.md): texto en un búfer fijo, el reloj y
//! la llamada 29.

#![allow(dead_code)]

use core::fmt;

/// Un texto de hasta 256 bytes (lo que cabe en un mensaje), sin montón.
pub struct Texto {
    pub b: [u8; 256],
    pub n: usize,
}

impl Texto {
    pub const fn nuevo() -> Self {
        Self { b: [0; 256], n: 0 }
    }
    pub fn bytes(&self) -> &[u8] {
        &self.b[..self.n]
    }
}

impl fmt::Write for Texto {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let s = s.as_bytes();
        let n = s.len().min(self.b.len() - self.n);
        self.b[self.n..self.n + n].copy_from_slice(&s[..n]);
        self.n += n;
        Ok(())
    }
}

#[macro_export]
macro_rules! texto {
    ($($arg:tt)*) => {{
        let mut t = Texto::nuevo();
        let _ = core::fmt::Write::write_fmt(&mut t, format_args!($($arg)*));
        t
    }};
}

/// Cuentas del contador de la CPU y su frecuencia.
pub fn cuentas() -> u64 {
    let t: u64;
    unsafe { core::arch::asm!("isb", "mrs {}, cntvct_el0", out(reg) t) };
    t
}

pub fn frecuencia() -> u64 {
    let f: u64;
    unsafe { core::arch::asm!("mrs {}, cntfrq_el0", out(reg) f) };
    f.max(1)
}

pub fn a_us(c: u64) -> u64 {
    (c as u128 * 1_000_000 / frecuencia() as u128) as u64
}

pub fn de_us(us: u64) -> u64 {
    (us as u128 * frecuencia() as u128 / 1_000_000) as u64
}

/// El núcleo en que corre quien llama (llamada 29).
pub fn nucleo() -> u64 {
    dashos::llamada(29, [0, 0, 0, 0, 0]).0
}

/// Fija el hilo a su núcleo (llamada 31), para que el reparto entre núcleos no lo mueva, y dice cuál es.
pub fn fijar() -> u64 {
    dashos::llamada(31, [1, 0, 0, 0, 0]);
    nucleo()
}

/// Un contador del kernel del núcleo `n`: 1 = tics, 2 = cambios de hilo, 3 = µs ocioso.
pub fn contador(que: u64, n: u64) -> u64 {
    dashos::llamada(29, [que, n, 0, 0, 0]).0
}

/// Las palabras de un texto separadas por espacios, como números (las que no lo son, 0).
pub fn numeros<const N: usize>(t: &[u8]) -> [u64; N] {
    let mut r = [0u64; N];
    let mut i = 0;
    for p in t.split(|&c| c == b' ').filter(|p| !p.is_empty()) {
        if i == N {
            break;
        }
        r[i] = p.iter().take_while(|c| c.is_ascii_digit()).fold(0u64, |a, &c| a * 10 + (c - b'0') as u64);
        i += 1;
    }
    r
}
