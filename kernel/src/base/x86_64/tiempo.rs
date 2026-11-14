//! El tiempo en x86-64 (S21, X3): el TSC como contador, calibrado contra el HPET (o el temporizador de gestión de
//! energía de ACPI si no hay HPET), y el temporizador del APIC local para las interrupciones.
//!
//! Escrito con el manual de Intel (SDM, volumen 3: 18.17, el TSC; 11.5.4, el temporizador del APIC y el modo de plazo
//! del TSC), la especificación del HPET (IA-PC HPET 1.0a, 2.3) y la de ACPI 6.5 (4.8.3.3, el PM timer).
//!
//! `contador` y `frecuencia` son los del TSC; `temporizador(cuentas)` pide una interrupción dentro de `cuentas` del
//! TSC. Con el modo de plazo del TSC (CPUID 1, ECX bit 24) se escribe la hora del TSC en IA32_TSC_DEADLINE. Si no lo
//! hay, el temporizador del APIC va en modo de un disparo, con su frecuencia medida también contra la referencia.

use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, AtomicUsize, Ordering};

static TSC_HZ: AtomicU64 = AtomicU64::new(1_000_000_000);
static APIC_HZ: AtomicU64 = AtomicU64::new(0);
static PLAZO_TSC: AtomicBool = AtomicBool::new(false);

/// El HPET: dirección virtual de sus registros y femtosegundos por cuenta.
static HPET: AtomicUsize = AtomicUsize::new(0);
static HPET_FS: AtomicU64 = AtomicU64::new(0);
static HPET_64: AtomicBool = AtomicBool::new(false);
/// El PM timer: su puerto (0 si no hay) y si cuenta 32 bits.
static PM: AtomicU16 = AtomicU16::new(0);
static PM_32: AtomicBool = AtomicBool::new(false);
const PM_HZ: u64 = 3_579_545;

const HPET_CAPACIDADES: usize = 0x000;
const HPET_CONFIG: usize = 0x010;
const HPET_CONTADOR: usize = 0x0f0;

/// IA32_TSC_DEADLINE.
const MSR_PLAZO: u32 = 0x6e0;

/// La referencia con la que se mide el TSC.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Referencia {
    Hpet,
    PmTimer,
    /// Sin reloj de referencia: lo que dice CPUID (hojas 0x15 o 0x16), o 1 GHz si no dice nada.
    Cpuid,
}

/// El TSC, tras `lfence`: que no se lea antes que lo de antes.
pub fn contador() -> u64 {
    let (lo, hi): (u32, u32);
    unsafe { core::arch::asm!("lfence", "rdtsc", out("eax") lo, out("edx") hi, options(nomem, nostack, preserves_flags)) };
    (hi as u64) << 32 | lo as u64
}

/// Cuentas del TSC por segundo.
pub fn frecuencia() -> u64 {
    TSC_HZ.load(Ordering::Relaxed)
}

fn hpet_leer(off: usize) -> u64 {
    let b = HPET.load(Ordering::Relaxed);
    unsafe { ((b + off) as *const u64).read_volatile() }
}

fn hpet_escribir(off: usize, v: u64) {
    let b = HPET.load(Ordering::Relaxed);
    unsafe { ((b + off) as *mut u64).write_volatile(v) };
}

fn pm_leer() -> u32 {
    let v: u32;
    unsafe { core::arch::asm!("in eax, dx", out("eax") v, in("dx") PM.load(Ordering::Relaxed), options(nomem, nostack, preserves_flags)) };
    v
}

/// La referencia en marcha: (cuentas ahora, cuentas por segundo, máscara del contador). `None` si no hay.
fn referencia() -> Option<(u64, u64, u64)> {
    if HPET.load(Ordering::Relaxed) != 0 {
        let mascara = if HPET_64.load(Ordering::Relaxed) { u64::MAX } else { u32::MAX as u64 };
        return Some((hpet_leer(HPET_CONTADOR) & mascara, 1_000_000_000_000_000 / HPET_FS.load(Ordering::Relaxed), mascara));
    }
    if PM.load(Ordering::Relaxed) != 0 {
        let mascara = if PM_32.load(Ordering::Relaxed) { u32::MAX as u64 } else { 0xff_ffff };
        return Some((pm_leer() as u64 & mascara, PM_HZ, mascara));
    }
    None
}

/// Nanosegundos según la referencia (el HPET o el PM timer), que dan la vuelta con su contador: para medir intervalos
/// cortos con `diferencia_ns`. 0 si no hay referencia.
pub fn referencia_cuentas() -> u64 {
    referencia().map(|r| r.0).unwrap_or(0)
}

/// Nanosegundos entre dos lecturas de `referencia_cuentas`.
pub fn diferencia_ns(antes: u64, despues: u64) -> u64 {
    match referencia() {
        Some((_, hz, mascara)) => ((despues.wrapping_sub(antes) & mascara) as u128 * 1_000_000_000 / hz as u128) as u64,
        None => 0,
    }
}

/// Espera, mirando la referencia, a que pasen `ns` nanosegundos; devuelve (cuentas de la referencia, del TSC).
fn medir(ns: u64) -> (u64, u64) {
    let (r0, hz, mascara) = referencia().unwrap_or((0, 1, 0));
    let t0 = contador();
    let objetivo = (ns as u128 * hz as u128 / 1_000_000_000) as u64;
    loop {
        let (r, ..) = referencia().unwrap_or((0, 1, 0));
        let d = r.wrapping_sub(r0) & mascara;
        if d >= objetivo {
            return (d, contador() - t0);
        }
        core::hint::spin_loop();
    }
}

/// Pone en marcha la referencia y calibra el TSC. Una vez, en el núcleo 0, con el mapa del kernel ya puesto.
pub fn iniciar(acpi: &super::acpi::Acpi) -> Referencia {
    if let Some(d) = acpi.hpet.filter(|&d| super::mapa::se_ve(d, 0x400)) {
        let v = crate::mmu::virt(d as usize);
        HPET.store(v, Ordering::Relaxed);
        let cap = hpet_leer(HPET_CAPACIDADES);
        let fs = cap >> 32;
        // El periodo, de 0 a 100 ns (HPET 1.0a, 2.3.4); si no, no es un HPET.
        if fs == 0 || fs > 100_000_000 {
            HPET.store(0, Ordering::Relaxed);
        } else {
            HPET_FS.store(fs, Ordering::Relaxed);
            HPET_64.store(cap & (1 << 13) != 0, Ordering::Relaxed);
            // En marcha (ENABLE_CNF), sin el modo de compatibilidad (LEG_RT_CNF): sus comparadores no se usan.
            hpet_escribir(HPET_CONFIG, (hpet_leer(HPET_CONFIG) & !0b11) | 1);
        }
    }
    if HPET.load(Ordering::Relaxed) == 0 {
        if let Some((puerto, de32)) = acpi.pm_timer {
            PM.store(puerto, Ordering::Relaxed);
            PM_32.store(de32, Ordering::Relaxed);
        }
    }
    let referencia_usada = if HPET.load(Ordering::Relaxed) != 0 {
        Referencia::Hpet
    } else if PM.load(Ordering::Relaxed) != 0 {
        Referencia::PmTimer
    } else {
        Referencia::Cpuid
    };
    let hz = match referencia() {
        Some((_, rhz, _)) => {
            let (r, t) = medir(50_000_000);
            (t as u128 * rhz as u128 / r.max(1) as u128) as u64
        }
        None => {
            let (max, ..) = super::cpu::cpuid(0, 0);
            let (den, num, cristal, _) = if max >= 0x15 { super::cpu::cpuid(0x15, 0) } else { (0, 0, 0, 0) };
            let (mhz, ..) = if max >= 0x16 { super::cpu::cpuid(0x16, 0) } else { (0, 0, 0, 0) };
            if den != 0 && num != 0 && cristal != 0 {
                cristal as u64 * num as u64 / den as u64
            } else if mhz != 0 {
                mhz as u64 * 1_000_000
            } else {
                1_000_000_000
            }
        }
    };
    TSC_HZ.store(hz.max(1_000_000), Ordering::Relaxed);
    let (_, _, ecx, _) = super::cpu::cpuid(1, 0);
    PLAZO_TSC.store(ecx & (1 << 24) != 0, Ordering::Relaxed);
    referencia_usada
}

/// ¿Va el temporizador del APIC en el modo de plazo del TSC?
pub fn plazo_tsc() -> bool {
    PLAZO_TSC.load(Ordering::Relaxed)
}

/// Mide la frecuencia del temporizador del APIC en modo de un disparo (sin modo de plazo del TSC). La llama `apic`
/// en el núcleo 0, con el temporizador enmascarado; `arrancar` lo pone a contar desde `u32::MAX` y `leer` da la
/// cuenta que le queda.
pub(super) fn calibrar_apic(arrancar: impl Fn(), leer: impl Fn() -> u32) {
    arrancar();
    let t0 = contador();
    let (_, t) = match referencia() {
        Some(_) => medir(20_000_000),
        None => {
            while contador() - t0 < frecuencia() / 50 {
                core::hint::spin_loop();
            }
            (0, contador() - t0)
        }
    };
    let contadas = (u32::MAX - leer()) as u64;
    APIC_HZ.store((contadas as u128 * frecuencia() as u128 / t.max(1) as u128) as u64, Ordering::Relaxed);
}

/// Cuentas del temporizador del APIC por segundo (0 si va en modo de plazo del TSC).
pub fn frecuencia_apic() -> u64 {
    APIC_HZ.load(Ordering::Relaxed)
}

/// Pide la interrupción del temporizador de este núcleo dentro de `cuentas` del TSC.
pub fn temporizador(cuentas: u64) {
    let cuentas = cuentas.max(1);
    if PLAZO_TSC.load(Ordering::Relaxed) {
        super::cpu::escribir_msr(MSR_PLAZO, contador().saturating_add(cuentas));
    } else {
        let a = (cuentas as u128 * APIC_HZ.load(Ordering::Relaxed) as u128 / frecuencia() as u128).clamp(1, u32::MAX as u128);
        super::apic::cuenta_inicial(a as u32);
    }
}

/// Espera activa de `us` microsegundos, con el TSC.
pub fn esperar_us(us: u64) {
    let t0 = contador();
    let n = us * frecuencia() / 1_000_000;
    while contador() - t0 < n {
        core::hint::spin_loop();
    }
}
