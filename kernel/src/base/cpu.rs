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

/// La interfaz de CPU del GICv3 por registros del sistema (U3b): encenderla en este núcleo (SRE, máscara de
/// prioridad, punto binario y grupo 1).
pub fn gicv3_iniciar_cpu(mascara: u8) {
    unsafe {
        asm!(
            "mrs {t}, S3_0_C12_C12_5", // ICC_SRE_EL1
            "orr {t}, {t}, #1",
            "msr S3_0_C12_C12_5, {t}",
            "isb",
            "msr S3_0_C4_C6_0, {m}", // ICC_PMR_EL1
            "msr S3_0_C12_C12_3, xzr", // ICC_BPR1_EL1
            "mov {t}, #1",
            "msr S3_0_C12_C12_7, {t}", // ICC_IGRPEN1_EL1
            "isb",
            t = out(reg) _,
            m = in(reg) mascara as u64,
        )
    };
}

/// ICC_IAR1_EL1: reconoce la interrupción de grupo 1 pendiente.
pub fn gicv3_reconocer() -> u32 {
    let v: u64;
    unsafe { asm!("mrs {}, S3_0_C12_C12_0", out(reg) v) };
    v as u32
}

/// ICC_EOIR1_EL1.
pub fn gicv3_terminar(iar: u32) {
    unsafe { asm!("msr S3_0_C12_C12_1, {}", "isb", in(reg) iar as u64) };
}

/// ICC_SGI1R_EL1: interrupción por software al núcleo de afinidad 0 `nucleo` (afinidades 1 a 3 a cero).
pub fn gicv3_sgi(nucleo: u32, sgi: u32) {
    let v = ((sgi as u64 & 0xf) << 24) | (1u64 << (nucleo & 15));
    unsafe { asm!("dsb ishst", "msr S3_0_C12_C11_5, {}", "isb", in(reg) v) };
}
