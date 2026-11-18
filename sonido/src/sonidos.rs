//! Los sonidos del sistema (encargo `sonido-pro`, paso 5): avisos, bloqueo, error, conectar y desconectar un aparato,
//! el tic del volumen y entrar. No son ficheros: se sintetizan aquí, al pedirse, con osciladores y envolventes en coma
//! fija. Son de DashOS (hechos en este repositorio, MPL-2.0 como el resto): no hay licencias de terceros que cuidar.
//!
//! Tres temas: "Cristal" (campanas: parciales inarmónicos que se apagan deprisa), "Suave" (como una marimba: senos con
//! caída larga) y "Retro" (ondas cuadradas cortas, a lo chip de 8 bits). Cada sonido son unas pocas notas.

use crate::mates::seno;
use alloc::vec::Vec;

pub const TEMAS: [&str; 3] = ["Cristal", "Suave", "Retro"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Evento {
    Aviso = 1,
    Bloqueo = 2,
    Error = 3,
    Conectar = 4,
    Desconectar = 5,
    Volumen = 6,
    Entrar = 7,
}

impl Evento {
    pub fn desde(n: u32) -> Option<Evento> {
        Some(match n {
            1 => Evento::Aviso,
            2 => Evento::Bloqueo,
            3 => Evento::Error,
            4 => Evento::Conectar,
            5 => Evento::Desconectar,
            6 => Evento::Volumen,
            7 => Evento::Entrar,
            _ => return None,
        })
    }
}

/// Una nota: cuándo empieza (ms), su frecuencia (Hz), cuánto dura (ms) y su volumen (0 a 100).
type Nota = (u32, u32, u32, u32);

/// Las notas de cada evento (en Hz: La4 = 440).
fn notas(e: Evento) -> &'static [Nota] {
    match e {
        // Mi6 y La6: una pregunta que sube.
        Evento::Aviso => &[(0, 1319, 260, 70), (110, 1760, 420, 80)],
        // Bajando: Sol5, Re5.
        Evento::Bloqueo => &[(0, 784, 200, 70), (120, 587, 380, 70)],
        // Grave y con una segunda menor: no se confunde con nada.
        Evento::Error => &[(0, 220, 260, 80), (0, 233, 260, 60), (180, 196, 340, 80)],
        // Arpegio de Do mayor hacia arriba / hacia abajo.
        Evento::Conectar => &[(0, 1047, 160, 60), (70, 1319, 160, 60), (140, 1568, 320, 70)],
        Evento::Desconectar => &[(0, 1568, 160, 60), (70, 1319, 160, 60), (140, 1047, 320, 70)],
        // Un tic corto.
        Evento::Volumen => &[(0, 1760, 60, 50)],
        // Entrar: cuatro notas que abren (Do, Sol, Do, Mi).
        Evento::Entrar => &[(0, 523, 400, 50), (120, 784, 400, 50), (240, 1047, 500, 55), (360, 1319, 700, 60)],
    }
}

/// La tabla del seno: 1024 puntos en Q15, calculada una vez.
pub struct Sintetizador {
    tabla: Vec<i32>,
    frecuencia: u32,
}

const TABLA: usize = 1024;

impl Sintetizador {
    pub fn nuevo(frecuencia: u32) -> Self {
        let tabla = (0..TABLA).map(|i| (seno(2.0 * core::f64::consts::PI * i as f64 / TABLA as f64) * 32767.0) as i32).collect();
        Sintetizador { tabla, frecuencia }
    }

    /// Una onda en la fase `fase` (32 bits = un ciclo): seno, o cuadrada (para "Retro").
    fn onda(&self, fase: u32, cuadrada: bool) -> i32 {
        if cuadrada {
            if fase < 0x8000_0000 { 20_000 } else { -20_000 }
        } else {
            self.tabla[(fase >> 22) as usize]
        }
    }

    /// Las muestras de un evento en un tema (mono, 16 bits, a la frecuencia del sintetizador), con `volumen` de 0 a 100.
    pub fn generar(&self, tema: u32, e: Evento, volumen: u32) -> Vec<i16> {
        let fs = self.frecuencia as u64;
        let notas = notas(e);
        let fin_ms = notas.iter().map(|n| n.0 + n.2).max().unwrap_or(0) + 20;
        let n = (fin_ms as u64 * fs / 1000) as usize;
        let mut acc = alloc::vec![0i64; n];
        let volumen = volumen.min(100) as i64;
        for &(empieza, hz, dura, vol) in notas {
            // Los parciales de cada tema: (múltiplo de la frecuencia en milésimas, volumen relativo en %, caída en ms).
            let parciales: &[(u64, i64, u64)] = match tema {
                0 => &[(1000, 100, 0), (2756, 45, 3), (5404, 20, 6)],
                1 => &[(1000, 100, 0), (4000, 12, 4)],
                _ => &[(1000, 100, 0)],
            };
            let cuadrada = tema >= 2;
            let i0 = (empieza as u64 * fs / 1000) as usize;
            let largo = (dura as u64 * fs / 1000) as usize;
            for &(mult, rel, divisor_caida) in parciales {
                let paso = ((hz as u64 * mult / 1000) << 32) / fs;
                if paso >= 1 << 31 {
                    continue; // por encima de Nyquist
                }
                let mut fase: u32 = 0;
                // Envolvente: ataque de 4 ms y caída exponencial (más corta en los parciales altos).
                let ataque = (fs * 4 / 1000).max(1) as i64;
                let tau = (largo as i64 / if divisor_caida == 0 { 3 } else { divisor_caida as i64 * 2 }).max(1);
                let k = 65_536 - (65_536 / tau).max(1);
                let mut env: i64 = 65_536;
                for t in 0..largo {
                    let i = i0 + t;
                    if i >= n {
                        break;
                    }
                    let a = if (t as i64) < ataque { 65_536 * t as i64 / ataque } else { env };
                    if t as i64 >= ataque {
                        env = env * k >> 16;
                    }
                    // Los últimos 5 ms bajan a cero (sin chasquido al cortar).
                    let cola = (fs * 5 / 1000) as usize;
                    let a = if largo - t < cola { a * (largo - t) as i64 / cola as i64 } else { a };
                    let s = self.onda(fase, cuadrada) as i64;
                    acc[i] += s * a / 65_536 * rel / 100 * vol as i64 / 100;
                    fase = fase.wrapping_add(paso as u32);
                }
            }
        }
        // Todos al mismo pico (0,6 del fondo de escala), por el volumen.
        let pico = acc.iter().map(|x| x.abs()).max().unwrap_or(0).max(1);
        acc.iter().map(|&x| (x * 19_660 / pico * volumen / 100).clamp(-32_767, 32_767) as i16).collect()
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn suenan_sin_saturar_ni_chasquear() {
        let s = Sintetizador::new_prueba();
        for tema in 0..3 {
            for e in 1..=7 {
                let ev = Evento::desde(e).unwrap();
                let m = s.generar(tema, ev, 100);
                assert!(m.len() > 1000 && m.len() < 48_000 * 2);
                let pico = m.iter().map(|x| x.unsigned_abs()).max().unwrap();
                assert!(pico > 3000 && pico < 32_767, "tema {tema} {ev:?}: {pico}");
                // Empieza y acaba en silencio.
                assert!(m[0].unsigned_abs() < 200 && m[m.len() - 1] == 0);
                // Sin escalones de más: lo que más salta de una muestra a otra no pasa de lo que da la suma de las
                // pendientes de sus parciales (2π·f/fs·amplitud: ~3.500 la nota más aguda y ~3.700 su parcial de
                // 9,5 kHz en "Cristal"). Los de la cuadrada de "Retro" son de la propia onda.
                let salto = m.windows(2).map(|w| (w[1] as i32 - w[0] as i32).unsigned_abs()).max().unwrap();
                if tema < 2 {
                    assert!(salto < 12_000, "tema {tema} {ev:?}: salto {salto}");
                }
            }
        }
        assert!(Evento::desde(0).is_none() && Evento::desde(99).is_none());
        // El volumen 0 es silencio; uno absurdo, como 100.
        assert!(s.generar(0, Evento::Aviso, 0).iter().all(|&x| x == 0));
        assert_eq!(s.generar(0, Evento::Aviso, 1000), s.generar(0, Evento::Aviso, 100));
        assert_eq!(s.generar(77, Evento::Aviso, 50), s.generar(2, Evento::Aviso, 50));
    }

    impl Sintetizador {
        fn new_prueba() -> Self {
            Sintetizador::nuevo(48_000)
        }
    }
}
