//! El tic del sistema: el temporizador genérico de ARM (el físico de EL1), o en x86-64 (X3) el del APIC local, con el
//! TSC como contador (`base/x86_64/tiempo.rs`). Las cuentas son las del contador: CNTPCT en ARM, el TSC en x86.

#[cfg(target_arch = "aarch64")]
use crate::gic;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

static INTID: AtomicU32 = AtomicU32::new(30);
static PERIODO: AtomicU64 = AtomicU64::new(0);
static ARRANQUE: AtomicU64 = AtomicU64::new(0);

pub use crate::base::cpu::{esperar_interrupcion, frecuencia, habilitar_irq};

pub fn ahora() -> u64 {
    crate::base::cpu::contador()
}

/// Programa la siguiente interrupción del temporizador de este núcleo dentro de `cuentas`. El planificador la
/// pone en cada decisión: el tic normal, o antes si un hilo de plazo agota su presupuesto o lo recupera (C14).
pub fn programar(cuentas: u64) {
    crate::base::cpu::temporizador(cuentas);
}

/// Tic periódico de `hz` veces por segundo. `ppi` es el PPI del temporizador físico no seguro (del árbol).
#[cfg(target_arch = "aarch64")]
pub fn iniciar(ppi: u32, hz: u64) {
    let intid = 16 + ppi;
    INTID.store(intid, Ordering::Relaxed);
    let periodo = frecuencia() / hz;
    PERIODO.store(periodo, Ordering::Relaxed);
    ARRANQUE.store(ahora(), Ordering::Relaxed);
    gic::habilitar(intid, 0x80);
    programar(periodo);
}

/// x86-64: tic periódico de `hz` veces por segundo. El APIC local de cada núcleo ya tiene su temporizador apuntando a
/// su vector (`apic::VECTOR_RELOJ`); aquí solo se programa.
#[cfg(target_arch = "x86_64")]
pub fn iniciar(hz: u64) {
    INTID.store(crate::base::apic::VECTOR_RELOJ as u32, Ordering::Relaxed);
    let periodo = frecuencia() / hz;
    PERIODO.store(periodo, Ordering::Relaxed);
    ARRANQUE.store(ahora(), Ordering::Relaxed);
    programar(periodo);
}

/// La parte de cada núcleo: el temporizador y su interrupción son privados de cada núcleo.
pub fn iniciar_nucleo() {
    #[cfg(target_arch = "aarch64")]
    gic::habilitar(INTID.load(Ordering::Relaxed), 0x80);
    programar(PERIODO.load(Ordering::Relaxed));
}

/// Tics normales transcurridos desde que se inició el reloj (se calcula con la hora, no se cuenta).
pub fn tics() -> u64 {
    let p = PERIODO.load(Ordering::Relaxed);
    if p == 0 { 0 } else { (ahora() - ARRANQUE.load(Ordering::Relaxed)) / p }
}

/// Cuentas de un tic normal.
pub fn periodo() -> u64 {
    PERIODO.load(Ordering::Relaxed)
}

/// ¿Es del temporizador? Deja una interrupción de reserva por si el planificador no reprograma; el planificador la
/// sustituye al decidir.
pub fn interrupcion(intid: u32) -> bool {
    if intid != INTID.load(Ordering::Relaxed) {
        return false;
    }
    programar(PERIODO.load(Ordering::Relaxed));
    // Los temporizadores de los procesos (C49) llegan a x86 con los objetos (X4).
    #[cfg(target_arch = "aarch64")]
    if crate::temporizadores::tic() {
        crate::interrupciones::despertar_repartidor();
    }
    true
}

