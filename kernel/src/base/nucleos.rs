//! Los cuatro núcleos (C9).
//!
//! El firmware de la Pi (y QEMU) deja los núcleos 1 a 3 esperando en la tabla de espera ("spin table"): cada uno
//! lee una dirección de 64 bits en 0xe0, 0xe8 y 0xf0 y salta a ella cuando deja de ser cero. Esos núcleos
//! siguen en little-endian y sin MMU, así que la dirección se escribe en little-endian, se saca de la caché y se
//! avisa con `sev`.

use crate::{excepciones, gic, kprintln, mmu};
use core::sync::atomic::{AtomicU32, Ordering};

static EN_MARCHA: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" {
    static _secundario: u8;
}

pub fn numero() -> u32 {
    let mpidr: u64;
    unsafe { core::arch::asm!("mrs {}, mpidr_el1", out(reg) mpidr) };
    (mpidr & 0xff) as u32
}

/// Cómo se encienden los otros núcleos: con la tabla de espera de la Pi, o con PSCI (U3: QEMU `virt` y las máquinas
/// con UEFI), llamando al firmware o al hipervisor con `smc` o `hvc` según diga el árbol.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Encendido {
    TablaDeEspera,
    PsciHvc,
    PsciSmc,
}

/// PSCI 0.2, CPU_ON de 64 bits: núcleo (MPIDR), dirección física de entrada y un valor que llega en x0.
const PSCI_CPU_ON: u64 = 0xC400_0003;

fn psci_cpu_on(como: Encendido, mpidr: u64, entrada: u64) -> i64 {
    let r: i64;
    unsafe {
        match como {
            Encendido::PsciHvc => core::arch::asm!("hvc #0", inlateout("x0") PSCI_CPU_ON as i64 => r, in("x1") mpidr, in("x2") entrada, in("x3") 0u64),
            _ => core::arch::asm!("smc #0", inlateout("x0") PSCI_CPU_ON as i64 => r, in("x1") mpidr, in("x2") entrada, in("x3") 0u64),
        }
    }
    r
}

/// Despierta los núcleos 1 a 3 y espera a que se presenten. Devuelve cuántos lo hicieron.
pub fn despertar(como: Encendido) -> u32 {
    // Solo una vez: los núcleos ya despiertos no vuelven a la tabla de espera.
    static HECHO: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
    if HECHO.swap(true, Ordering::AcqRel) {
        return EN_MARCHA.load(Ordering::Acquire);
    }
    let entrada = mmu::fisica(unsafe { &_secundario as *const u8 as usize }) as u64;
    if como == Encendido::TablaDeEspera {
        for n in 1..4u64 {
            let ranura = mmu::virt(0xd8 + 8 * n as usize) as *mut u64;
            unsafe {
                ranura.write_volatile(entrada.to_le());
                core::arch::asm!("dc civac, {}", in(reg) ranura);
            }
        }
        unsafe { core::arch::asm!("dsb sy", "sev") };
    } else {
        // Las tablas de arranque las leen sin caché: que estén en la memoria (dsb), y a encenderlos uno a uno.
        unsafe { core::arch::asm!("dsb sy") };
        for n in 1..4u64 {
            let r = psci_cpu_on(como, n, entrada);
            if r != 0 {
                kprintln!("  PSCI no enciende el núcleo {}: {}", n, r);
            }
        }
    }
    let inicio = crate::reloj::ahora();
    let limite = crate::reloj::frecuencia(); // un segundo
    while EN_MARCHA.load(Ordering::Acquire) < 3 && crate::reloj::ahora() - inicio < limite {
        core::hint::spin_loop();
    }
    let n = EN_MARCHA.load(Ordering::Acquire);
    if n == 3 {
        // Los cuatro núcleos ya usan el mapa del kernel: fuera las tablas del arranque (C34). Si alguno no ha
        // llegado, se quedan: lo necesitaría para arrancar.
        crate::mapa::olvidar_arranque();
    }
    n
}

#[unsafe(no_mangle)]
extern "C" fn kmain_secundario(nucleo: u64) -> ! {
    crate::mapa::activar();
    excepciones::iniciar();
    gic::iniciar_cpu();
    crate::planificador::iniciar_nucleo();
    crate::reloj::iniciar_nucleo();
    kprintln!("  núcleo {} en marcha (MPIDR {})", nucleo, numero());
    EN_MARCHA.fetch_add(1, Ordering::Release);
    crate::planificador::ocioso();
}
