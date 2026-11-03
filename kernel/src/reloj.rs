//! Temporizador genérico de ARM (el físico de EL1) y el tic del sistema.

use crate::gic;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

static INTID: AtomicU32 = AtomicU32::new(30);
static TICS: AtomicU64 = AtomicU64::new(0);
static PERIODO: AtomicU64 = AtomicU64::new(0);

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

fn programar(cuentas: u64) {
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
    gic::habilitar(intid, 0x80);
    programar(periodo);
}

pub fn tics() -> u64 {
    TICS.load(Ordering::Relaxed)
}

/// Lo llama la interrupción: cuenta el tic y vuelve a programar el siguiente.
pub fn interrupcion(intid: u32) -> bool {
    if intid != INTID.load(Ordering::Relaxed) {
        return false;
    }
    TICS.fetch_add(1, Ordering::Relaxed);
    programar(PERIODO.load(Ordering::Relaxed));
    true
}

pub fn habilitar_irq() {
    unsafe { core::arch::asm!("msr daifclr, #2") };
}

pub fn esperar_interrupcion() {
    unsafe { core::arch::asm!("wfi") };
}
