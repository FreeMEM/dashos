use super::*;
use std::f64::consts::PI;

const FS: u32 = 48_000;

fn seno_buf(hz: f64, amp: f64, n: usize) -> Vec<Cuadro> {
    (0..n)
        .map(|i| {
            let v = ((2.0 * PI * hz * i as f64 / FS as f64).sin() * amp * UNO as f64) as i32;
            [v, v]
        })
        .collect()
}

fn pico(b: &[Cuadro]) -> f64 {
    b.iter().map(|c| c[0].unsigned_abs().max(c[1].unsigned_abs())).max().unwrap_or(0) as f64 / UNO as f64
}

fn salto(b: &[Cuadro]) -> f64 {
    b.windows(2).map(|w| (w[1][0] - w[0][0]).unsigned_abs().max((w[1][1] - w[0][1]).unsigned_abs())).max().unwrap_or(0) as f64 / UNO as f64
}

fn db(cdb: i32) -> f64 {
    cdb as f64 / 100.0
}

/// La respuesta del ecualizador: cada banda donde debe, y lo plano, plano.
#[test]
fn ecualizador_respuesta() {
    let mut eq = Ecualizador::nuevo(FS);
    assert!(medir_respuesta(&mut eq, FS, 1000).abs() <= 5);
    let mut eq = Ecualizador::nuevo(FS);
    eq.poner(5 * 4 + 1, 900); // +9 dB a 1 kHz
    eq.poner(8 * 4 + 1, -600); // -6 dB a 8 kHz
    // La rampa: unos cuantos períodos para llegar.
    let mut calentar = seno_buf(100.0, 0.1, 48_000);
    for t in calentar.chunks_mut(256) {
        eq.procesar(t);
    }
    let a1k = db(medir_respuesta(&mut eq, FS, 1000));
    let a8k = db(medir_respuesta(&mut eq, FS, 8000));
    let a100 = db(medir_respuesta(&mut eq, FS, 100));
    assert!((a1k - 9.0).abs() < 0.5, "1 kHz: {a1k}");
    assert!((a8k + 6.0).abs() < 0.5, "8 kHz: {a8k}");
    assert!(a100.abs() < 0.6, "100 Hz: {a100}");
}

#[test]
fn ecualizador_preajustes_y_graves() {
    let mut eq = Ecualizador::nuevo(FS);
    assert!(eq.poner(P_PREAJUSTE, 1)); // Graves
    assert_eq!(eq.leer(P_PREAJUSTE), Some(1));
    let mut calentar = seno_buf(100.0, 0.1, 48_000);
    for t in calentar.chunks_mut(256) {
        eq.procesar(t);
    }
    let a40 = db(medir_respuesta(&mut eq, FS, 40));
    let a5k = db(medir_respuesta(&mut eq, FS, 5000));
    // Lo que da el "Audio EQ Cookbook" calculado aparte (en Python, con números complejos): +3,82 dB.
    assert!((a40 - 3.82).abs() < 0.3, "40 Hz: {a40}");
    assert!(a5k.abs() < 0.8, "5 kHz: {a5k}");
    // Tocar una banda a mano deja el preajuste en "a mano".
    eq.poner(1, 100);
    assert_eq!(eq.leer(P_PREAJUSTE), Some(-1));
    assert!(!eq.poner(P_PREAJUSTE, 99));
    assert_eq!(preajuste(1).unwrap()[0], 600);
}

/// Cambiar la ganancia de una banda en marcha no chasca: la señal no salta más de lo que salta un seno.
#[test]
fn ecualizador_sin_chasquidos() {
    let mut eq = Ecualizador::nuevo(FS);
    let mut b = seno_buf(200.0, 0.5, 48_000);
    let (a, resto) = b.split_at_mut(12_000);
    for t in a.chunks_mut(256) {
        eq.procesar(t);
    }
    eq.poner(3 * 4 + 1, 1500);
    eq.poner(2 * 4 + 1, -1500);
    for t in resto.chunks_mut(256) {
        eq.procesar(t);
    }
    // Con +15 dB a 250 Hz el seno de 200 Hz sube a ~2,3 veces: su salto natural, ~0,035.
    let s = salto(&b[1000..]);
    assert!(s < 0.05, "salto {s}");
}

/// Hostil: parámetros absurdos se recortan; el filtro sigue estable con lo más extremo.
#[test]
fn ecualizador_parametros_absurdos() {
    let mut eq = Ecualizador::nuevo(FS);
    for p in 0..4 * NUM_BANDAS as u32 {
        assert!(eq.poner(p, i32::MAX));
    }
    assert!(!eq.poner(9999, 5));
    assert!(eq.poner(P_PREGANANCIA, i32::MIN));
    assert_eq!(eq.leer(1), Some(1500));
    assert_eq!(eq.leer(0), Some(20_000));
    assert_eq!(eq.leer(2), Some(1000));
    let mut b = seno_buf(1000.0, 1.0, 48_000);
    for t in b.chunks_mut(256) {
        eq.procesar(t);
    }
    for p in 0..4 * NUM_BANDAS as u32 {
        eq.poner(p, i32::MIN);
    }
    let mut b2 = seno_buf(30.0, 1.0, 48_000);
    for t in b2.chunks_mut(64) {
        eq.procesar(t);
    }
    // Acotado (los filtros recortan a ±8,0): ni NaN ni desbordes.
    assert!(pico(&b) <= 8.0 && pico(&b2) <= 8.0);
}

/// El limitador nunca deja pasar del umbral, y no chasca.
#[test]
fn limitador_nunca_satura() {
    let mut l = Limitador::nuevo(FS);
    let umbral = 10f64.powf(-1.0 / 20.0);
    // Un seno a 3 veces el fondo de escala, con un golpe de 6 veces en medio.
    let mut b = seno_buf(440.0, 3.0, 48_000);
    for c in b[20_000..20_010].iter_mut() {
        *c = [6 * UNO, -6 * UNO];
    }
    for t in b.chunks_mut(256) {
        l.procesar(t);
    }
    assert!(pico(&b) <= umbral + 1e-4, "pico {}", pico(&b));
    assert!(l.actuaciones > 0);
    // Lo que no pasa del umbral no cambia (tras el retraso).
    let mut l = Limitador::nuevo(FS);
    let original = seno_buf(440.0, 0.5, 4096);
    let mut b = original.clone();
    for t in b.chunks_mut(256) {
        l.procesar(t);
    }
    let r = l.retraso();
    assert_eq!(&b[r..], &original[..4096 - r]);
    // Sin escalones al entrar en un golpe fuerte: lo más que salta es lo que salta el seno de 440 Hz al umbral más
    // la bajada de ganancia repartida en 1,5 ms.
    let mut l = Limitador::nuevo(FS);
    let mut b = seno_buf(440.0, 0.3, 9600);
    for c in b[4800..].iter_mut() {
        *c = c.map(|x| x * 10);
    }
    for t in b.chunks_mut(256) {
        l.procesar(t);
    }
    assert!(salto(&b) < 0.08, "salto {}", salto(&b));
}

#[test]
fn limitador_parametros() {
    let mut l = Limitador::nuevo(FS);
    assert!(l.poner(0, -600));
    assert!(l.poner(1, i32::MAX));
    assert_eq!(l.leer(1), Some(2000));
    assert!(l.poner(0, i32::MAX));
    assert_eq!(l.leer(0), Some(0));
    assert!(!l.poner(7, 0));
    l.poner(0, -600);
    let mut b = seno_buf(100.0, 1.0, 48_000);
    for t in b.chunks_mut(256) {
        l.procesar(t);
    }
    assert!(pico(&b) <= 0.5013, "{}", pico(&b));
}

/// El espacial: apagado no toca nada; ensanchar sube el lado y deja el centro; el crossfeed pasa un poco al otro lado.
#[test]
fn espacial() {
    let mut e = Espacial::nuevo(FS);
    let orig: Vec<Cuadro> = (0..4800).map(|i| if i % 2 == 0 { [UNO / 4, 0] } else { [UNO / 8, UNO / 8] }).collect();
    let mut b = orig.clone();
    for t in b.chunks_mut(256) {
        e.procesar(t);
    }
    assert_eq!(b, orig);
    // Ensanchar: un sonido solo a la izquierda pasa a tener algo negativo a la derecha; el centro no cambia.
    e.poner(0, 1);
    e.poner(1, 100);
    let mut b: Vec<Cuadro> = vec![[UNO / 4, 0]; 9600];
    for t in b.chunks_mut(256) {
        e.procesar(t);
    }
    let ult = b[9599];
    assert!(ult[0] > UNO / 4 && ult[1] < 0, "{ult:?}");
    let mut c: Vec<Cuadro> = vec![[UNO / 4, UNO / 4]; 1024];
    e.procesar(&mut c);
    assert_eq!(c[1000], [UNO / 4, UNO / 4]);
    // Auriculares: un grave solo a la izquierda llega algo a la derecha, sin escalones al cambiar de modo.
    e.poner(0, 2);
    let mut b = seno_buf(200.0, 0.5, 9600);
    for c in b.iter_mut() {
        c[1] = 0;
    }
    for t in b.chunks_mut(256) {
        e.procesar(t);
    }
    let derecha = b[4800..].iter().map(|c| c[1].unsigned_abs()).max().unwrap() as f64 / UNO as f64;
    assert!(derecha > 0.1 && derecha < 0.3, "{derecha}");
    assert!(salto(&b) < 0.05);
    assert!(e.poner(1, -50) && e.leer(1) == Some(0));
    assert!(e.poner(0, 99) && e.leer(0) == Some(2));
}

#[test]
fn crear_por_nombre() {
    for n in ["eq", "limitador", "espacial", "ganancia"] {
        assert_eq!(crear(n, FS).unwrap().nombre(), n);
    }
    assert!(crear("fuego", FS).is_none());
}

/// Tras sonar, el silencio vuelve a ser silencio: ningún preajuste deja la salida atascada en un valor (se midió 355
/// constante en el WAV antes de la realimentación del error).
#[test]
fn ecualizador_vuelve_al_silencio() {
    for n in 0..PREAJUSTES.len() as i32 {
        let mut eq = Ecualizador::nuevo(FS);
        eq.poner(P_PREAJUSTE, n);
        let mut b = seno_buf(55.0, 0.9, 48_000);
        b.extend(core::iter::repeat([0, 0]).take(48_000));
        for t in b.chunks_mut(256) {
            eq.procesar(t);
        }
        let cola = &b[48_000 + 24_000..];
        let resto = cola.iter().map(|c| c[0].unsigned_abs().max(c[1].unsigned_abs())).max().unwrap();
        assert!(resto <= 1, "preajuste {n}: queda {resto}");
    }
}
