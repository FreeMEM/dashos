//! Registros del sistema y barreras que usa el resto del kernel, con una interfaz segura.

use core::arch::asm;

/// Nivel de excepción actual.
pub fn nivel_excepcion() -> u64 {
    let el: u64;
    unsafe { asm!("mrs {}, CurrentEL", out(reg) el) };
    (el >> 2) & 3
}

/// Frecuencia del contador del sistema (CNTFRQ_EL0).
pub fn frecuencia() -> u64 {
    let f: u64;
    unsafe { asm!("mrs {}, cntfrq_el0", out(reg) f) };
    f
}

/// Contador físico (CNTPCT_EL0).
pub fn contador() -> u64 {
    let t: u64;
    unsafe { asm!("isb", "mrs {}, cntpct_el0", out(reg) t) };
    t
}

/// Programa el temporizador físico de este núcleo para dentro de `cuentas` y lo habilita sin máscara.
pub fn temporizador(cuentas: u64) {
    unsafe { asm!("msr cntp_tval_el0, {}", "msr cntp_ctl_el0, {}", "isb", in(reg) cuentas, in(reg) 1u64) };
}

/// Abre las IRQ de este núcleo.
pub fn habilitar_irq() {
    unsafe { asm!("msr daifclr, #2") };
}

/// Espera una interrupción.
pub fn esperar_interrupcion() {
    unsafe { asm!("wfi") };
}

/// Espera un evento, para siempre (núcleo parado).
pub fn parar() -> ! {
    loop {
        unsafe { asm!("wfe") };
    }
}

/// Que las escrituras de antes lleguen a los aparatos y a los otros núcleos antes que las de después.
pub fn barrera_escrituras() {
    unsafe { asm!("dsb ishst") };
}
