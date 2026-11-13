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

/// El número de este núcleo: Aff0 de MPIDR_EL1, o Aff1 si la CPU marca MT (el bit 24), como el Cortex-A76 de la Pi 5,
/// cuyos núcleos son 0x000, 0x100, 0x200 y 0x300 (arranque.s hace lo mismo).
pub fn numero() -> u32 {
    let mpidr: u64;
    unsafe { core::arch::asm!("mrs {}, mpidr_el1", out(reg) mpidr) };
    if mpidr & (1 << 24) != 0 { ((mpidr >> 8) & 0xff) as u32 } else { (mpidr & 0xff) as u32 }
}

/// La afinidad que pide PSCI para encender el núcleo `n`: como la de este, con `n` donde va el número.
fn afinidad(n: u64) -> u64 {
    let mpidr: u64;
    unsafe { core::arch::asm!("mrs {}, mpidr_el1", out(reg) mpidr) };
    if mpidr & (1 << 24) != 0 { n << 8 } else { n }
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

/// Cómo se habla con PSCI, si se puede (para apagar y reiniciar): 0 no hay, 1 `hvc`, 2 `smc`. Lo pone `despertar`.
static PSCI: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

/// PSCI 0.2, SYSTEM_OFF y SYSTEM_RESET (Arm DEN 0022, §5.10 y §5.11): no vuelven si van bien.
const PSCI_SYSTEM_OFF: u64 = 0x8400_0008;
const PSCI_SYSTEM_RESET: u64 = 0x8400_0009;

/// Apaga (`reiniciar` falso) o reinicia la máquina (llamada 51). Con PSCI si el árbol lo trae: QEMU `virt`, las
/// máquinas con UEFI y la Pi 5, cuyo firmware (el BL31 que carga el cargador de arranque) lo da; allí SYSTEM_OFF deja
/// la placa parada, y que se corte la corriente o no lo decide la EEPROM (`POWER_OFF_ON_HALT`), que no se toca. Sin
/// PSCI, en la Pi 4 y la Pi 3, con el perro guardián de la placa (PM_RSTC/PM_WDOG): reiniciar es dejar que venza; para
/// apagar se marca antes la partición 63 en PM_RSTS, que el firmware entiende como "quedarse parada". Si vuelve, es
/// que no se pudo.
pub fn apagar(reiniciar: bool) {
    let funcion = if reiniciar { PSCI_SYSTEM_RESET } else { PSCI_SYSTEM_OFF };
    // Lo escrito, que salga antes por el puerto serie.
    let hasta = crate::reloj::ahora() + crate::reloj::frecuencia() / 50;
    while crate::reloj::ahora() < hasta {
        core::hint::spin_loop();
    }
    unsafe {
        match PSCI.load(Ordering::Acquire) {
            1 => core::arch::asm!("hvc #0", inlateout("x0") funcion => _, options(nostack)),
            2 => core::arch::asm!("smc #0", inlateout("x0") funcion => _, options(nostack)),
            _ => {}
        }
    }
    #[cfg(not(any(feature = "virt", feature = "pi5")))]
    perro_guardian(reiniciar);
}

/// El perro guardián de la BCM2835 y la BCM2711 (bloque PM: 0x7E100000 en el bus; RSTC en +0x1C, RSTS en +0x20, WDOG
/// en +0x24; cada escritura lleva la contraseña 0x5A en el byte alto). Con WDOG a 10 tics (unos 150 µs) y RSTC en
/// reinicio completo (0x20), la placa se reinicia al vencer.
#[cfg(not(any(feature = "virt", feature = "pi5")))]
fn perro_guardian(reiniciar: bool) {
    #[cfg(feature = "pi3")]
    const PM: usize = 0x3F10_0000;
    #[cfg(not(feature = "pi3"))]
    const PM: usize = 0xFE10_0000;
    const CLAVE: u32 = 0x5A00_0000;
    let rstc = super::mmio::RegLe32::new(crate::mmu::virt(PM + 0x1c));
    let rsts = super::mmio::RegLe32::new(crate::mmu::virt(PM + 0x20));
    let wdog = super::mmio::RegLe32::new(crate::mmu::virt(PM + 0x24));
    if !reiniciar {
        // La partición de arranque (bits 0, 2, 4, 6, 8 y 10) a 63: 0x555.
        rsts.escribir(CLAVE | (rsts.leer() & 0x00FF_FFFF) | 0x555);
    }
    wdog.escribir(CLAVE | 10);
    rstc.escribir(CLAVE | (rstc.leer() & 0x00FF_FFCF) | 0x20);
    let hasta = crate::reloj::ahora() + crate::reloj::frecuencia();
    while crate::reloj::ahora() < hasta {
        core::hint::spin_loop();
    }
}

/// Despierta los núcleos 1 a 3 y espera a que se presenten. Devuelve cuántos lo hicieron.
pub fn despertar(como: Encendido) -> u32 {
    PSCI.store(
        match como {
            Encendido::PsciHvc => 1,
            Encendido::PsciSmc => 2,
            Encendido::TablaDeEspera => 0,
        },
        Ordering::Release,
    );
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
            let r = psci_cpu_on(como, afinidad(n), entrada);
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
extern "C" fn kmain_secundario(nucleo: u64, el2: u64) -> ! {
    super::hiper::arranco_en(nucleo as u32, el2);
    crate::mapa::activar();
    super::hiper::instalar_nucleo();
    excepciones::iniciar();
    gic::iniciar_cpu();
    crate::planificador::iniciar_nucleo();
    crate::reloj::iniciar_nucleo();
    kprintln!("  núcleo {} en marcha (MPIDR {})", nucleo, numero());
    EN_MARCHA.fetch_add(1, Ordering::Release);
    crate::planificador::ocioso();
}
