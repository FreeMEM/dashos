//! Los vúmetros y el espectro que se ven (encargo `sonido-pro`): lo que llega del servicio de sonido unas 30 veces por
//! segundo se suaviza para que se mueva sin saltos, como un vúmetro de aguja: sube deprisa, cae despacio, y el pico se
//! queda un momento arriba antes de caer.
//!
//! Sin nada de DashOS: se prueba en el PC con `rustc --edition 2024 --test usuario/src/sonido_medidor.rs`.

#![allow(dead_code)]

/// Un vúmetro estéreo, en milésimas de la escala que se pinta (0 a 1000).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vumetro {
    pub nivel: [i32; 2],
    pub pico: [i32; 2],
    /// Cuánto tiempo (µs) le queda al pico quieto arriba.
    quieto: [i64; 2],
}

/// Lo que dura el pico quieto y lo que cae el nivel por segundo (en milésimas de la escala).
const PICO_QUIETO_US: i64 = 900_000;
const CAE_NIVEL: i64 = 1400;
const CAE_PICO: i64 = 700;

/// Un nivel del servicio (1,0 = 32.768) en la escala del vúmetro: de -54 dB (0) a 0 dB (1000), en dB lineales, que es
/// como se leen los vúmetros de verdad (los niveles bajos se ven).
pub fn escala(nivel: u32) -> i32 {
    if nivel == 0 {
        return 0;
    }
    // 20·log10(n/32768) ≈ 6,02·(log2(n) - 15), con la mantisa en lineal.
    let n = nivel as i64;
    let l2 = 63 - (n as u64).leading_zeros() as i64;
    let mantisa_mil = (((n as u64) << 16 >> l2) as i64 - 65_536) * 1000 / 65_536;
    let db_mil = (l2 * 1000 + mantisa_mil - 15_000) * 6021 / 1000;
    ((db_mil + 54_000) * 1000 / 54_000).clamp(0, 1000) as i32
}

impl Vumetro {
    /// Lo que llega (pico de cada lado, en la escala del servicio), `dt` µs después de lo anterior.
    pub fn llegar(&mut self, pico: [u32; 2], dt_us: u64) {
        let dt = dt_us.min(500_000) as i64;
        for k in 0..2 {
            let v = escala(pico[k]);
            let cae = (CAE_NIVEL * dt / 1_000_000) as i32;
            self.nivel[k] = if v > self.nivel[k] { v } else { (self.nivel[k] - cae).max(v) };
            if v >= self.pico[k] {
                self.pico[k] = v;
                self.quieto[k] = PICO_QUIETO_US;
            } else if self.quieto[k] > 0 {
                self.quieto[k] -= dt;
            } else {
                self.pico[k] = (self.pico[k] - (CAE_PICO * dt / 1_000_000) as i32).max(self.nivel[k]);
            }
        }
    }

    /// Entre dos llegadas, para que la caída se vea suave a más cuadros por segundo de los que llegan.
    pub fn avanzar(&mut self, dt_us: u64) {
        let dt = dt_us.min(500_000) as i64;
        for k in 0..2 {
            self.nivel[k] = (self.nivel[k] - (CAE_NIVEL * dt / 1_000_000) as i32).max(0);
            if self.quieto[k] > 0 {
                self.quieto[k] -= dt;
            } else {
                self.pico[k] = (self.pico[k] - (CAE_PICO * dt / 1_000_000) as i32).max(self.nivel[k]);
            }
        }
    }

    /// ¿Se mueve algo? (para dejar de pintar cuando todo está en reposo).
    pub fn quieto(&self) -> bool {
        self.nivel == [0, 0] && self.pico == [0, 0]
    }
}

/// Las barras del espectro (0 a 1000), suavizadas entre llegadas: van hacia lo último que llegó.
#[derive(Clone, Debug, Default)]
pub struct Espectro {
    pub barras: alloc::vec::Vec<i32>,
    objetivo: alloc::vec::Vec<i32>,
}

extern crate alloc;

impl Espectro {
    pub fn llegar(&mut self, bandas: &[u16]) {
        if self.barras.len() != bandas.len() {
            self.barras = alloc::vec![0; bandas.len()];
        }
        self.objetivo = bandas.iter().map(|&b| (b as i32).min(1000)).collect();
    }

    /// Un cuadro de `dt` µs: cada barra recorre la mitad de lo que le falta cada 30 ms.
    pub fn avanzar(&mut self, dt_us: u64) {
        let pasos = (dt_us / 15_000).clamp(1, 8);
        for (b, o) in self.barras.iter_mut().zip(self.objetivo.iter()) {
            for _ in 0..pasos {
                *b += (*o - *b + if *o > *b { 3 } else { -3 }) / 4;
                if (*o - *b).abs() < 4 {
                    *b = *o;
                }
            }
        }
    }

    pub fn quieto(&self) -> bool {
        self.barras.iter().zip(self.objetivo.iter()).all(|(b, o)| b == o) && self.objetivo.iter().all(|&o| o == 0)
    }

    /// Sin sonido: todo a cero (cae suave).
    pub fn apagar(&mut self) {
        self.objetivo.iter_mut().for_each(|o| *o = 0);
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn escala_en_decibelios() {
        assert_eq!(escala(0), 0);
        assert_eq!(escala(32_768), 1000);
        // -6 dB: 1000 - 6/54.
        assert!((escala(16_384) - 889).abs() <= 3, "{}", escala(16_384));
        assert!(escala(33) <= 10, "-60 dB no se ve: {}", escala(33));
        assert_eq!(escala(u32::MAX), 1000);
    }

    #[test]
    fn sube_deprisa_cae_despacio_y_el_pico_espera() {
        let mut v = Vumetro::default();
        v.llegar([32_768, 16_384], 33_000);
        assert_eq!(v.nivel[0], 1000);
        assert_eq!(v.pico[0], 1000);
        // Silencio: el nivel cae 1400/s; el pico se queda 0,9 s.
        v.llegar([0, 0], 100_000);
        assert_eq!(v.nivel[0], 860);
        assert_eq!(v.pico[0], 1000);
        for _ in 0..20 {
            v.avanzar(50_000);
        }
        assert!(v.pico[0] < 1000 && v.pico[0] >= v.nivel[0]);
        for _ in 0..100 {
            v.avanzar(50_000);
        }
        assert!(v.quieto());
        // Hostil: un dt enorme o cero no rompe nada.
        v.llegar([u32::MAX, 0], u64::MAX);
        v.avanzar(0);
        assert_eq!(v.nivel[0], 1000);
    }

    #[test]
    fn espectro_suave() {
        let mut e = Espectro::default();
        e.llegar(&[1000, 0, 500]);
        e.avanzar(15_000);
        assert!(e.barras[0] > 200 && e.barras[0] < 1000);
        for _ in 0..40 {
            e.avanzar(15_000);
        }
        assert_eq!(e.barras, [1000, 0, 500]);
        e.apagar();
        for _ in 0..60 {
            e.avanzar(30_000);
        }
        assert!(e.quieto());
        e.llegar(&[u16::MAX; 5]);
        assert_eq!(e.barras.len(), 5);
    }
}
