//! Registros del sistema y barreras que usa el resto del kernel, con una interfaz segura.

use core::arch::asm;

/// Nivel de excepción actual.
pub fn nivel_excepcion() -> u64 {
    let el: u64;
    unsafe { asm!("mrs {}, CurrentEL", out(reg) el) };
    (el >> 2) & 3
}

/// MIDR_EL1: quién hizo la CPU y qué modelo es (implementador en los bits 31-24, pieza en los 15-4).
pub fn midr() -> u64 {
    let v: u64;
    unsafe { asm!("mrs {}, midr_el1", out(reg) v) };
    v
}

/// ID_AA64MMFR0_EL1: lo que admite la MMU (tamaños de página, orden de bytes...).
pub fn mmfr0() -> u64 {
    let v: u64;
    unsafe { asm!("mrs {}, id_aa64mmfr0_el1", out(reg) v) };
    v
}

/// Si la CPU admite big-endian en EL0 (SCTLR_EL1.E0E): ID_AA64MMFR0_EL1.BigEnd (bits 11-8, en EL0 y EL1) o
/// BigEndEL0 (19-16, solo en EL0). Las de Apple no lo tienen (U7).
pub fn big_endian_el0() -> bool {
    let m = mmfr0();
    (m >> 8) & 0xf != 0 || (m >> 16) & 0xf != 0
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
