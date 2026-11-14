//! Pruebas del planificador y de los hilos del kernel que no dependen de la placa (X3): las mismas en ARM
//! (`pruebas.rs`) y en x86-64 (`pruebas_x86.rs`). Solo tocan `reloj` y `planificador`.
//!
//! - C8: el tic del reloj llega a su ritmo.
//! - C10: tres hilos que compiten por el núcleo 1, más uno que acaba solo.
//! - C14: un hilo de plazo en el núcleo 1, con los de C10 ocupándolo.

use crate::{kprintln, planificador, reloj};

/// C8: el temporizador y su interrupción (el GIC en ARM, el APIC en x86).
pub fn prueba_reloj() {
    let inicio = reloj::ahora();
    let desde = reloj::tics();
    while reloj::tics() < desde + 10 {
        reloj::esperar_interrupcion();
    }
    let ms = (reloj::ahora() - inicio) * 1000 / reloj::frecuencia();
    kprintln!("  frecuencia del temporizador {} Hz; 10 tics en {} ms", reloj::frecuencia(), ms);
    kprintln!("C8: interrupciones {}", if (80..=130).contains(&ms) { "bien" } else { "MAL" });
}

/// C10: tres hilos que compiten por el núcleo 1, más uno que acaba solo.
static CONTADORES: [core::sync::atomic::AtomicU64; 3] =
    [core::sync::atomic::AtomicU64::new(0), core::sync::atomic::AtomicU64::new(0), core::sync::atomic::AtomicU64::new(0)];
static TERMINADO: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

static PARAR: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

extern "C" fn contar(i: usize) {
    while !PARAR.load(core::sync::atomic::Ordering::Relaxed) {
        CONTADORES[i].fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }
}

/// Para los hilos de C10, que si no ocupan el núcleo 1 para siempre (x86, antes de las pruebas de procesos de X4).
#[cfg(target_arch = "x86_64")]
pub fn parar_contadores() {
    PARAR.store(true, core::sync::atomic::Ordering::Relaxed);
}

extern "C" fn acabar(_: usize) {
    TERMINADO.store(true, core::sync::atomic::Ordering::Release);
}

pub fn prueba_hilos() {
    use core::sync::atomic::Ordering;
    for i in 0..3 {
        planificador::crear(contar, i, 1);
    }
    planificador::crear(acabar, 0, 2);
    let inicio = reloj::tics();
    while reloj::tics() < inicio + 30 {
        reloj::esperar_interrupcion();
    }
    let c: [u64; 3] = core::array::from_fn(|i| CONTADORES[i].load(Ordering::Relaxed));
    let (min, max) = (*c.iter().min().unwrap(), *c.iter().max().unwrap());
    let cambios = planificador::cambios();
    kprintln!("  en 300 ms: contadores {:?}, {} cambios de hilo, hilo corto terminado: {}", c, cambios, TERMINADO.load(Ordering::Acquire));
    let bien = min > 0 && min * 2 > max && cambios >= 20 && TERMINADO.load(Ordering::Acquire);
    kprintln!("C10: hilos {}", if bien { "bien" } else { "MAL" });
}

// C14: un hilo de plazo (2 ms cada 10 ms) en el núcleo 1, que ya tiene tres hilos justos ocupados (C10).
const ACTIVACIONES: usize = 64;
static VECES: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static MOMENTOS: [core::sync::atomic::AtomicU64; ACTIVACIONES] = [const { core::sync::atomic::AtomicU64::new(0) }; ACTIVACIONES];

extern "C" fn hilo_de_plazo(_: usize) {
    use core::sync::atomic::Ordering;
    let ms = reloj::frecuencia() / 1000;
    loop {
        let i = VECES.load(Ordering::Relaxed);
        if i >= ACTIVACIONES {
            return;
        }
        MOMENTOS[i].store(reloj::ahora(), Ordering::Relaxed);
        VECES.store(i + 1, Ordering::Release);
        let t = reloj::ahora();
        while reloj::ahora() - t < ms {} // 1 ms de trabajo
        planificador::esperar_periodo();
    }
}

pub fn prueba_plazo() {
    use core::sync::atomic::Ordering;
    let antes: u64 = CONTADORES.iter().map(|c| c.load(Ordering::Relaxed)).sum();
    if planificador::crear_plazo(hilo_de_plazo, 0, 1, 2_000, 10_000).is_none() {
        kprintln!("C14: plazo no admitido");
        return;
    }
    // Pedir más del 80 % tiene que fallar.
    let rechazado = planificador::crear_plazo(hilo_de_plazo, 0, 1, 7_000, 10_000).is_none();
    let inicio = reloj::tics();
    while reloj::tics() < inicio + 30 {
        reloj::esperar_interrupcion();
    }
    let n = VECES.load(Ordering::Acquire).min(ACTIVACIONES);
    let periodo = reloj::frecuencia() / 100;
    // Desviación entre activaciones seguidas respecto al periodo (de la segunda en adelante: la primera llega
    // cuando se crea el hilo, a mitad de un periodo).
    let mut retraso_max = 0u64;
    for k in 2..n {
        let d = MOMENTOS[k].load(Ordering::Relaxed) - MOMENTOS[k - 1].load(Ordering::Relaxed);
        retraso_max = retraso_max.max(d.abs_diff(periodo));
    }
    let despues: u64 = CONTADORES.iter().map(|c| c.load(Ordering::Relaxed)).sum();
    let us = retraso_max * 1_000_000 / reloj::frecuencia();
    kprintln!("  {} activaciones en 300 ms; desviación máxima del periodo {} µs; los hilos justos siguen: {}; exceso rechazado: {}",
        n, us, despues > antes, rechazado);
    let bien = n >= 25 && us < 1000 && despues > antes && rechazado;
    kprintln!("C14: clase de plazo {}", if bien { "bien" } else { "MAL" });
}
