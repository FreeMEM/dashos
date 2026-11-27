//! Un hijo del banco de medidas de la multitarea (M0, docs/MULTITAREA_LOOP.md), en Rust. Las versiones en C
//! (`medir/hijo.c`) y en Dash (`medir/hijo.dash`) hablan igual.
//!
//! Llave 1: el canal con el orquestador (`medir`), que manda órdenes de texto y recibe una respuesta de texto por
//! cada una. Llave 2, si la hay: el canal con el otro hijo de la prueba; también puede llegar con una orden.
//!
//! - `eco`: devuelve por la llave 2 lo que llega por ella, hasta que se cierra. Responde `eco N` (mensajes).
//! - `ping N`: 500 idas y vueltas de calentamiento, N seguidas (el tiempo total, `us`, como siempre) y otras N mirando
//!   el reloj en cada una (R7, B2: a un histograma; leer el reloj cuesta, por eso van aparte). De 8 bytes por la
//!   llave 2. Responde `ping N us nucleo p50_ns p99_ns max_ns`.
//! - `carga` (con la llave de una memoria): calcula hasta que el primer byte de esa memoria deja de ser 0 (R7, B1).
//!   Responde `carga unidades`.
//! - `calc MS`: calcula MS milisegundos en unidades fijas de trabajo. Responde
//!   `calc unidades hueco_max_us huecos_us nucleo_al_empezar nucleo_al_acabar`; un hueco es un tramo de más de
//!   200 µs entre dos lecturas seguidas del reloj (el proceso no corría).
//! - `temp US N`: N esperas de US microsegundos con un temporizador de un disparo. Responde `temp p50 p99 max` de lo
//!   que tarda de más, en µs.
//! - `dormir MS`: duerme MS milisegundos y responde `dormir real_us`.
//! - `nucleo`: se fija a su núcleo (llamada 31, para que el reparto entre núcleos no lo mueva) y responde `nucleo N`.
//! - `fin`: acaba.

#![no_std]
#![no_main]

#[path = "../medir_comun.rs"]
#[macro_use]
mod medir_comun;

use dashos::{canal_escribir, canal_leer, cerrar};
use medir_comun::*;

dashos::programa!(principal);

/// Dónde se ve la memoria de la bandera de `carga`.
const BANDERA: usize = 0x40_0000_0000;

/// Histograma de rondas (R7, B2): de 100 en 100 ns hasta 102,4 µs, y de 10 en 10 µs hasta unos 10 ms.
struct Histo([u16; 2048]);

impl Histo {
    fn meter(&mut self, ns: u64) {
        let i = if ns < 102_400 { ns / 100 } else { 1024 + ((ns - 102_400) / 10_000).min(1023) };
        self.0[i as usize] = self.0[i as usize].saturating_add(1);
    }

    /// El valor (cota de arriba de su cubo) por debajo del que queda el `q` por mil de las rondas.
    fn cuantil(&self, q: u64, n: u64) -> u64 {
        let meta = (n * q).div_ceil(1000).max(1);
        let mut suma = 0u64;
        for (i, &v) in self.0.iter().enumerate() {
            suma += v as u64;
            if suma >= meta {
                let i = i as u64;
                return if i < 1024 { (i + 1) * 100 } else { 102_400 + (i - 1023) * 10_000 };
            }
        }
        0
    }
}

const CONTROL: u32 = 1;

/// Una unidad de trabajo: 256 pasos de un generador congruencial de 32 bits, igual en C y en Dash.
#[inline(never)]
fn unidad(mut x: u32) -> u32 {
    for _ in 0..256 {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
    }
    x
}

fn responder(t: &Texto) {
    canal_escribir(CONTROL, t.bytes(), &[]);
}

fn principal() -> u64 {
    let mut b = [0u8; 256];
    // El canal con el otro hijo: la llave 2 si nació con ella, o la que llegue con una orden.
    let mut otro = 2u32;
    loop {
        let mut ll = [0u32; 1];
        let (n, k) = canal_leer(CONTROL, &mut b, &mut ll);
        if n < 0 {
            return 0;
        }
        if k > 0 {
            otro = ll[0];
        }
        let orden = &b[..n as usize];
        let palabra = orden.split(|&c| c == b' ').next().unwrap_or(b"");
        let [_, x, y] = numeros::<3>(orden);
        match palabra {
            b"eco" => {
                let mut m = [0u8; 256];
                let mut veces = 0u64;
                loop {
                    let (k, _) = canal_leer(otro, &mut m, &mut []);
                    if k < 0 || canal_escribir(otro, &m[..k as usize], &[]) != 0 {
                        break;
                    }
                    veces += 1;
                }
                responder(&texto!("eco {}", veces));
            }
            b"ping" => {
                let mut m = [0u8; 8];
                for i in 0..500u64 {
                    m.copy_from_slice(&i.to_le_bytes());
                    canal_escribir(otro, &m, &[]);
                    canal_leer(otro, &mut m, &mut []);
                }
                let t0 = cuentas();
                for i in 0..x {
                    m.copy_from_slice(&i.to_le_bytes());
                    canal_escribir(otro, &m, &[]);
                    canal_leer(otro, &mut m, &mut []);
                }
                let us = a_us(cuentas() - t0);
                let mut h = Histo([0; 2048]);
                let (mut max, f) = (0u64, frecuencia());
                let mut ant = cuentas();
                for i in 0..x {
                    m.copy_from_slice(&i.to_le_bytes());
                    canal_escribir(otro, &m, &[]);
                    canal_leer(otro, &mut m, &mut []);
                    let t = cuentas();
                    let ns = (t - ant) * 1_000_000_000 / f;
                    ant = t;
                    h.meter(ns);
                    max = max.max(ns);
                }
                cerrar(otro);
                responder(&texto!("ping {} {} {} {} {} {}", x, us, nucleo(), h.cuantil(500, x), h.cuantil(990, x), max));
            }
            b"carga" => {
                // Hasta que el orquestador escriba en la bandera: así la carga dura lo que dura la medida.
                let mut unidades = 0u64;
                if dashos::memoria_proyectar(otro, BANDERA, false) == 0 {
                    let mut acc = 1u32;
                    while unsafe { core::ptr::read_volatile(BANDERA as *const u8) } == 0 {
                        for _ in 0..64 {
                            acc = unidad(acc);
                        }
                        unidades += 64;
                    }
                    core::hint::black_box(acc);
                    dashos::memoria_quitar(BANDERA);
                }
                cerrar(otro);
                responder(&texto!("carga {}", unidades));
            }
            b"calc" => {
                let n0 = nucleo();
                let umbral = de_us(200);
                let t0 = cuentas();
                let fin = t0 + de_us(x * 1000);
                let (mut unidades, mut acc, mut ant) = (0u64, 1u32, t0);
                let (mut hueco_max, mut huecos) = (0u64, 0u64);
                loop {
                    acc = unidad(acc);
                    unidades += 1;
                    let t = cuentas();
                    let d = t - ant;
                    if d > umbral {
                        huecos += d;
                        hueco_max = hueco_max.max(d);
                    }
                    ant = t;
                    if t >= fin {
                        break;
                    }
                }
                core::hint::black_box(acc);
                responder(&texto!("calc {} {} {} {} {}", unidades, a_us(hueco_max), a_us(huecos), n0, nucleo()));
            }
            b"temp" => {
                // N esperas de X µs con un temporizador de un disparo: el retraso es lo que pasa de X.
                let mut retrasos = [0u64; 512];
                let n = (y as usize).min(retrasos.len());
                for r in retrasos.iter_mut().take(n) {
                    let t0 = cuentas();
                    dashos::dormir_us(x);
                    *r = a_us(cuentas() - t0).saturating_sub(x);
                }
                let r = &mut retrasos[..n];
                r.sort_unstable();
                let p = |q: usize| if n == 0 { 0 } else { r[(n - 1) * q / 100] };
                responder(&texto!("temp {} {} {}", p(50), p(99), p(100)));
            }
            b"dormir" => {
                let t0 = cuentas();
                dashos::dormir_us(x * 1000);
                responder(&texto!("dormir {}", a_us(cuentas() - t0)));
            }
            // Se fija a su núcleo y dice cuál: el orquestador lo pregunta para colocar a dos juntos o separados.
            b"nucleo" => responder(&texto!("nucleo {}", fijar())),
            _ => return 0,
        }
    }
}
