//! El controlador de interrupciones de la Raspberry Pi 3 (P3, docs/MULTITAREA_LOOP.md): no tiene GIC. Dos piezas:
//!
//! - Los periféricos locales de los núcleos (QA7, "BCM2836 ARM-local peripherals", rev 3.4): por núcleo, a qué IRQ van
//!   sus temporizadores (0x40 + 4n), sus buzones (0x50 + 4n), de dónde viene su IRQ (0x60 + 4n), y cuatro buzones de
//!   32 bits para avisarse (poner bits en 0x80 + 16n + 4m; leer y borrar en 0xC0 + 16n + 4m). Las IRQ de la GPU van a
//!   un núcleo (0x0C).
//! - El controlador de la BCM2835 (ARMCTRL, "BCM2835 ARM Peripherals", cap. 7): las 64 IRQ de los periféricos, en
//!   dos bancos de pendientes (0x04, 0x08), habilitar (0x10, 0x14) e inhabilitar (0x1C, 0x20), desde su `reg`.
//!
//! Escrito con esos dos documentos. Da los mismos números que el GIC para que el resto del kernel no cambie: 0 a 15,
//! avisos entre núcleos (bits del buzón 0); 16 a 31, los de cada núcleo (el temporizador físico no seguro, 30, como el
//! PPI 14 del GIC); 32 a 95, los de los periféricos (32 + número de ARMCTRL).

use crate::mmio::RegLe32;
use crate::mmu;
use core::sync::atomic::{AtomicUsize, Ordering};

static LOCAL: AtomicUsize = AtomicUsize::new(0);
static ARMCTRL: AtomicUsize = AtomicUsize::new(0);

const GPU_A_NUCLEO: usize = 0x0C;
const TEMPORIZADORES: usize = 0x40;
const BUZONES: usize = 0x50;
const ORIGEN: usize = 0x60;
const BUZON_PONER: usize = 0x80;
const BUZON_BORRAR: usize = 0xC0;

const PENDIENTES: [usize; 2] = [0x04, 0x08];
const HABILITAR: [usize; 2] = [0x10, 0x14];
const INHABILITAR: [usize; 2] = [0x1C, 0x20];

/// Bits de ORIGEN: los cuatro temporizadores (en el orden de su registro de control), el buzón 0 y la GPU.
const ORIGEN_BUZON0: u32 = 1 << 4;
const ORIGEN_GPU: u32 = 1 << 8;
/// El número de cada temporizador del núcleo, en el orden de los bits 0 a 3 de su control y de ORIGEN: físico seguro
/// (29), físico no seguro (30), de hipervisor (26) y virtual (27), como sus PPI en el GIC.
const TEMPORIZADOR_INTID: [u32; 4] = [29, 30, 26, 27];

fn local(off: usize) -> RegLe32 {
    RegLe32::new(LOCAL.load(Ordering::Relaxed) + off)
}

fn armctrl(off: usize) -> RegLe32 {
    RegLe32::new(ARMCTRL.load(Ordering::Relaxed) + off)
}

fn nucleo() -> usize {
    crate::nucleos::numero() as usize & 3
}

/// `local_fisica`: los periféricos locales (0x40000000); `armctrl_fisica`: el ARMCTRL (desde su `reg`, 0x3F00B200).
pub fn iniciar(local_fisica: u64, armctrl_fisica: u64) {
    LOCAL.store(mmu::virt(local_fisica as usize), Ordering::Relaxed);
    ARMCTRL.store(mmu::virt(armctrl_fisica as usize), Ordering::Relaxed);
    for banco in INHABILITAR {
        armctrl(banco).escribir(0xffff_ffff);
    }
    local(GPU_A_NUCLEO).escribir(0);
    iniciar_cpu();
}

/// La parte de cada núcleo: su buzón 0 da IRQ (los avisos entre núcleos), sus temporizadores aún no.
pub fn iniciar_cpu() {
    let n = nucleo();
    local(TEMPORIZADORES + 4 * n).escribir(0);
    local(BUZON_BORRAR + 16 * n).escribir(0xffff_ffff);
    local(BUZONES + 4 * n).escribir(1);
}

pub fn habilitar(intid: u32) {
    match intid {
        16..=31 => {
            if let Some(b) = TEMPORIZADOR_INTID.iter().position(|&t| t == intid) {
                let r = local(TEMPORIZADORES + 4 * nucleo());
                r.escribir(r.leer() | 1 << b);
            }
        }
        32..=95 => armctrl(HABILITAR[(intid as usize - 32) / 32]).escribir(1 << ((intid - 32) % 32)),
        _ => {}
    }
}

pub fn deshabilitar(intid: u32) {
    match intid {
        16..=31 => {
            if let Some(b) = TEMPORIZADOR_INTID.iter().position(|&t| t == intid) {
                let r = local(TEMPORIZADORES + 4 * nucleo());
                r.escribir(r.leer() & !(1 << b));
            }
        }
        32..=95 => armctrl(INHABILITAR[(intid as usize - 32) / 32]).escribir(1 << ((intid - 32) % 32)),
        _ => {}
    }
}

/// Las IRQ de los periféricos van todas al mismo núcleo: dirigir una las dirige todas.
pub fn dirigir(intid: u32, nucleo: u32) {
    if intid >= 32 {
        local(GPU_A_NUCLEO).escribir(nucleo & 3);
    }
}

/// La interrupción pendiente de este núcleo, ya reconocida (un aviso se borra del buzón aquí), o `gic::FALSA`.
pub fn reconocer() -> u32 {
    let n = nucleo();
    let origen = local(ORIGEN + 4 * n).leer();
    if origen & ORIGEN_BUZON0 != 0 {
        let b = local(BUZON_BORRAR + 16 * n);
        let bits = b.leer();
        if bits != 0 {
            let aviso = bits.trailing_zeros();
            b.escribir(1 << aviso);
            return aviso;
        }
    }
    for (bit, &intid) in TEMPORIZADOR_INTID.iter().enumerate() {
        if origen & (1 << bit) != 0 {
            return intid;
        }
    }
    if origen & ORIGEN_GPU != 0 {
        for (i, &p) in PENDIENTES.iter().enumerate() {
            let pendientes = armctrl(p).leer();
            if pendientes != 0 {
                return 32 + 32 * i as u32 + pendientes.trailing_zeros();
            }
        }
    }
    crate::gic::FALSA
}

/// Un aviso al núcleo `nucleo`: el bit `aviso` de su buzón 0.
pub fn avisar(nucleo: u32, aviso: u32) {
    crate::base::cpu::barrera_escrituras();
    local(BUZON_PONER + 16 * (nucleo as usize & 3)).escribir(1 << (aviso & 31));
}
