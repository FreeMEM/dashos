//! El analizador para el visualizador (encargo `sonido-pro`, paso 4): el espectro en bandas y la forma de onda de lo
//! que sale, como el "audio visualizer" de DMS (allí lo hace `cava`).
//!
//! Barato: una FFT de 512 puntos en coma fija (radix 2, giros en Q15) cada vez que se pide, unas 30 veces por segundo
//! y solo mientras alguien mira (el servicio lo deja de alimentar si nadie lo pide). Las bandas, repartidas en escala
//! logarítmica de 50 Hz a 12 kHz, con su nivel pasado a una escala de raíz (como DMS: `sqrt(v)`), y suavizadas: suben
//! deprisa y caen despacio.

use crate::mates::{coseno, seno};
use crate::Cuadro;
use alloc::vec;
use alloc::vec::Vec;

pub const PUNTOS: usize = 512;
const BITS: u32 = 9;
pub const MAX_BANDAS: usize = 64;
/// Las muestras de la forma de onda.
pub const ONDA: usize = 128;

pub struct Analizador {
    frecuencia: u32,
    /// Las últimas muestras (mono), en anillo.
    ultimas: Vec<i32>,
    pos: usize,
    giros: Vec<(i32, i32)>,
    ventana: Vec<i32>,
    suaves: Vec<i32>,
    /// Los cajones de cada banda, calculados una vez para cada número de bandas.
    bordes: Vec<(usize, usize)>,
    /// La sensibilidad automática (como `autosens` de cava): el nivel más alto de lo último, que baja despacio; las
    /// bandas se enseñan respecto a él, para que la música baja también se vea. Nunca por debajo de `TECHO_MIN` (si no,
    /// el ruido de fondo llenaría las barras).
    techo: i32,
}

const TECHO_MIN: i32 = 250;

impl Analizador {
    pub fn nuevo(frecuencia: u32) -> Self {
        let giros = (0..PUNTOS / 2)
            .map(|k| {
                let a = -2.0 * core::f64::consts::PI * k as f64 / PUNTOS as f64;
                ((coseno(a) * 32767.0) as i32, (seno(a) * 32767.0) as i32)
            })
            .collect();
        // Ventana de Hann en Q15.
        let ventana = (0..PUNTOS).map(|i| ((0.5 - 0.5 * coseno(2.0 * core::f64::consts::PI * i as f64 / PUNTOS as f64)) * 32767.0) as i32).collect();
        Analizador { frecuencia, ultimas: vec![0; PUNTOS], pos: 0, giros, ventana, suaves: vec![0; MAX_BANDAS], bordes: Vec::new(), techo: TECHO_MIN }
    }

    /// Lo que acaba de salir.
    pub fn alimentar(&mut self, buf: &[Cuadro]) {
        for c in buf {
            self.ultimas[self.pos] = ((c[0] as i64 + c[1] as i64) / 2).clamp(-32768, 32767) as i32;
            self.pos = (self.pos + 1) % PUNTOS;
        }
    }

    /// Silencio: lo que había se olvida (para cuando deja de sonar o vuelve a mirarse).
    pub fn olvidar(&mut self) {
        self.ultimas.fill(0);
        self.suaves.fill(0);
        self.techo = TECHO_MIN;
    }

    /// La forma de onda: `ONDA` muestras de las últimas (1 de cada 4), de -32768 a 32767.
    pub fn onda(&self, dst: &mut [i16]) {
        for (i, d) in dst.iter_mut().take(ONDA).enumerate() {
            let k = (self.pos + PUNTOS - ONDA * 4 + i * 4) % PUNTOS;
            *d = self.ultimas[k] as i16;
        }
    }

    /// Las bandas (de 0 a 1000) en `dst` (como mucho `MAX_BANDAS`).
    pub fn bandas(&mut self, dst: &mut [u16]) {
        let n = dst.len().min(MAX_BANDAS);
        if n == 0 {
            return;
        }
        let mut re: Vec<i32> = (0..PUNTOS).map(|i| (self.ultimas[(self.pos + i) % PUNTOS] * self.ventana[i]) >> 15).collect();
        let mut im = vec![0i32; PUNTOS];
        fft(&mut re, &mut im, &self.giros);
        // Magnitudes de los cajones (0 a PUNTOS/2), en escala de raíz.
        if self.bordes.len() != n {
            let hz_cajon = self.frecuencia as u64 * 1000 / PUNTOS as u64; // milihercios
            let (bajo, alto) = (50u64 * 1000, 12_000u64 * 1000);
            self.bordes = (0..n)
                .map(|b| {
                    // Bordes logarítmicos: bajo · (alto/bajo)^(b/n).
                    let c0 = ((log_reparto(bajo, alto, b, n) / hz_cajon) as usize).clamp(1, PUNTOS / 2 - 1);
                    let c1 = ((log_reparto(bajo, alto, b + 1, n) / hz_cajon) as usize).clamp(c0 + 1, PUNTOS / 2);
                    (c0, c1)
                })
                .collect();
        }
        let mut niveles = [0i32; MAX_BANDAS];
        for b in 0..n {
            let (c0, c1) = self.bordes[b];
            let mut m: u64 = 0;
            for k in c0..c1 {
                let (r, i) = (re[k] as i64, im[k] as i64);
                m = m.max((r * r + i * i) as u64);
            }
            // La magnitud llega a ~PUNTOS/4 · amplitud con la ventana (con el escalado por etapa: /PUNTOS). Raíz de la
            // raíz: la escala de raíz de DMS sobre la amplitud.
            let amp = raiz(m); // en Q15 escalado
            let nivel = (raiz(amp << 20) * 1000 / raiz(16384u64 << 20)).min(1000) as i32;
            niveles[b] = nivel;
        }
        // La sensibilidad: sube con el pico, baja un 1 % cada vez.
        let pico = niveles[..n].iter().copied().max().unwrap_or(0);
        self.techo = pico.max(self.techo * 99 / 100).max(TECHO_MIN);
        for b in 0..n {
            let nivel = (niveles[b] * 900 / self.techo).min(1000);
            // Sube deprisa, cae despacio.
            let s = &mut self.suaves[b];
            *s = if nivel > *s { (*s + nivel * 3) / 4 } else { (*s * 7 + nivel) / 8 };
            dst[b] = (*s).clamp(0, 1000) as u16;
        }
        re.clear();
    }
}

fn log_reparto(bajo: u64, alto: u64, b: usize, n: usize) -> u64 {
    // bajo · (alto/bajo)^(b/n) con enteros: exp(ln(alto/bajo) · b / n), ln(240) ≈ 5,4806.
    let x = crate::mates::exp((alto as f64 / bajo as f64).ln_aprox() * b as f64 / n as f64);
    (bajo as f64 * x) as u64
}

trait LnAprox {
    fn ln_aprox(self) -> f64;
}

impl LnAprox for f64 {
    /// Logaritmo natural (para x > 0), por Newton sobre `exp`. Solo al calcular bordes.
    fn ln_aprox(self) -> f64 {
        if !(self > 0.0) {
            return 0.0;
        }
        let mut y = 0.0;
        for _ in 0..40 {
            let e = crate::mates::exp(y);
            y += 2.0 * (self - e) / (self + e);
        }
        y
    }
}

fn raiz(x: u64) -> u64 {
    if x == 0 {
        return 0;
    }
    let mut r = 1u64 << ((64 - x.leading_zeros()).div_ceil(2));
    loop {
        let s = (r + x / r) / 2;
        if s >= r {
            return r;
        }
        r = s;
    }
}

/// FFT de `PUNTOS` en su sitio, con un desplazamiento de un bit por etapa (no desborda).
fn fft(re: &mut [i32], im: &mut [i32], giros: &[(i32, i32)]) {
    let n = re.len();
    // Orden de bits invertido.
    for i in 0..n {
        let j = (i as u32).reverse_bits() >> (32 - BITS);
        let j = j as usize;
        if j > i {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut largo = 2;
    while largo <= n {
        let paso = n / largo;
        for inicio in (0..n).step_by(largo) {
            for k in 0..largo / 2 {
                let (wr, wi) = giros[k * paso];
                let (a, b) = (inicio + k, inicio + k + largo / 2);
                let tr = ((re[b] as i64 * wr as i64 - im[b] as i64 * wi as i64) >> 15) as i32;
                let ti = ((re[b] as i64 * wi as i64 + im[b] as i64 * wr as i64) >> 15) as i32;
                let (ar, ai) = (re[a], im[a]);
                re[a] = (ar + tr) >> 1;
                im[a] = (ai + ti) >> 1;
                re[b] = (ar - tr) >> 1;
                im[b] = (ai - ti) >> 1;
            }
        }
        largo *= 2;
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn tono(a: &mut Analizador, hz: f64, amp: f64) {
        let b: Vec<Cuadro> = (0..2048)
            .map(|i| {
                let v = ((2.0 * std::f64::consts::PI * hz * i as f64 / 48_000.0).sin() * amp * 32767.0) as i32;
                [v, v]
            })
            .collect();
        a.alimentar(&b);
    }

    #[test]
    fn un_tono_cae_en_su_banda() {
        let mut a = Analizador::nuevo(48_000);
        let mut bandas = [0u16; 16];
        for (hz, esperada) in [(100.0, 2usize), (1000.0, 8), (8000.0, 14)] {
            a.olvidar();
            tono(&mut a, hz, 0.8);
            for _ in 0..12 {
                a.bandas(&mut bandas);
            }
            let max = (0..16).max_by_key(|&i| bandas[i]).unwrap();
            assert!((max as i64 - esperada as i64).abs() <= 1, "{hz} Hz en la banda {max}: {bandas:?}");
            assert!(bandas[max] > 500, "{bandas:?}");
            // Lejos del tono, poco.
            let lejos = if max > 8 { 0 } else { 15 };
            assert!(bandas[lejos] < 150, "{bandas:?}");
        }
        // Silencio: cae a cero despacio, sin saltos.
        a.olvidar();
        a.bandas(&mut bandas);
        assert!(bandas.iter().all(|&b| b == 0));
        let mut onda = [0i16; ONDA];
        tono(&mut a, 440.0, 0.5);
        a.onda(&mut onda);
        assert!(onda.iter().map(|x| x.unsigned_abs()).max().unwrap() > 15_000);
    }
}
