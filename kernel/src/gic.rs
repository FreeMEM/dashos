//! Controlador de interrupciones GIC-400 (GICv2) de la Pi 4.

use crate::mmio::RegLe32;
use crate::mmu;
use core::sync::atomic::{AtomicUsize, Ordering};

static DIST: AtomicUsize = AtomicUsize::new(0);
static CPU: AtomicUsize = AtomicUsize::new(0);

fn d(off: usize) -> RegLe32 {
    RegLe32::new(DIST.load(Ordering::Relaxed) + off)
}
fn c(off: usize) -> RegLe32 {
    RegLe32::new(CPU.load(Ordering::Relaxed) + off)
}

const GICD_CTLR: usize = 0x000;
const GICD_ISENABLER: usize = 0x100;
const GICD_ICENABLER: usize = 0x180;
const GICD_IPRIORITYR: usize = 0x400;
const GICC_CTLR: usize = 0x000;
const GICC_PMR: usize = 0x004;
const GICC_BPR: usize = 0x008;
const GICC_IAR: usize = 0x00C;
const GICC_EOIR: usize = 0x010;

/// Interrupción falsa: el GIC no tenía nada que dar.
pub const FALSA: u32 = 1023;

/// Distribuidor y CPU en direcciones físicas (del árbol de dispositivos, C4).
pub fn iniciar(dist_fisica: u64, cpu_fisica: u64) {
    DIST.store(mmu::virt(dist_fisica as usize), Ordering::Relaxed);
    CPU.store(mmu::virt(cpu_fisica as usize), Ordering::Relaxed);
    d(GICD_CTLR).escribir(0);
    // Todo apagado salvo lo que se pida.
    for i in 0..32 {
        d(GICD_ICENABLER + 4 * i).escribir(0xffff_ffff);
    }
    d(GICD_CTLR).escribir(1);
    iniciar_cpu();
}

/// La parte de cada núcleo (las interrupciones privadas y la interfaz de CPU son por núcleo).
pub fn iniciar_cpu() {
    c(GICC_PMR).escribir(0xf0);
    c(GICC_BPR).escribir(0);
    c(GICC_CTLR).escribir(1);
}

/// Habilita una interrupción con una prioridad (menor número = más prioridad).
pub fn habilitar(intid: u32, prioridad: u8) {
    let i = intid as usize;
    // IPRIORITYR: un byte por interrupción; en un registro little-endian, el byte n es el de la interrupción 4k+n.
    let r = d(GICD_IPRIORITYR + (i & !3));
    let desp = (i & 3) * 8;
    r.escribir((r.leer() & !(0xff << desp)) | ((prioridad as u32) << desp));
    d(GICD_ISENABLER + 4 * (i / 32)).escribir(1 << (i % 32));
}

/// Reconoce la interrupción pendiente y devuelve su número.
pub fn reconocer() -> u32 {
    c(GICC_IAR).leer() & 0x3ff
}

pub fn terminar(intid: u32) {
    c(GICC_EOIR).escribir(intid);
}
