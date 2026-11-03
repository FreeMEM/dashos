//! Temporizador genérico de ARM (el físico de EL1) y el tic del sistema.

use crate::gic;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

static INTID: AtomicU32 = AtomicU32::new(30);
static PERIODO: AtomicU64 = AtomicU64::new(0);
static ARRANQUE: AtomicU64 = AtomicU64::new(0);

pub fn frecuencia() -> u64 {
    let f: u64;
    unsafe { core::arch::asm!("mrs {}, cntfrq_el0", out(reg) f) };
    f
}

pub fn ahora() -> u64 {
    let t: u64;
    unsafe { core::arch::asm!("isb", "mrs {}, cntpct_el0", out(reg) t) };
    t
}

/// Programa la siguiente interrupción del temporizador de este núcleo dentro de `cuentas`. El planificador la
/// pone en cada decisión: el tic normal, o antes si un hilo de plazo agota su presupuesto o lo recupera (C14).
pub fn programar(cuentas: u64) {
    unsafe {
        core::arch::asm!("msr cntp_tval_el0, {}", in(reg) cuentas);
        core::arch::asm!("msr cntp_ctl_el0, {}", in(reg) 1u64); // encendido, sin máscara
    }
}

/// Tic periódico de `hz` veces por segundo. `ppi` es el PPI del temporizador físico no seguro (del árbol).
pub fn iniciar(ppi: u32, hz: u64) {
    let intid = 16 + ppi;
    INTID.store(intid, Ordering::Relaxed);
    let periodo = frecuencia() / hz;
    PERIODO.store(periodo, Ordering::Relaxed);
    ARRANQUE.store(ahora(), Ordering::Relaxed);
    gic::habilitar(intid, 0x80);
    programar(periodo);
}

/// La parte de cada núcleo: el temporizador y su PPI son privados de cada núcleo.
pub fn iniciar_nucleo() {
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
    true
}

pub fn habilitar_irq() {
    unsafe { core::arch::asm!("msr daifclr, #2") };
}

pub fn esperar_interrupcion() {
    unsafe { core::arch::asm!("wfi") };
}
