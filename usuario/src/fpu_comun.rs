// C28: dos procesos en el mismo núcleo llenan los 32 registros de SIMD con patrones distintos, giran un rato para
// que el reloj los cambie, y comprueban que los registros siguen igual. Cada uno incluye este fichero con su SEMILLA.
// También suma 1,5 + 2,25 con la FPU de verdad. Sale con 28 si todo va bien.
//
// En x86-64 (X4), los 16 registros xmm, y MXCSR y la palabra de control del x87, que cada uno pone distintas.

use core::arch::asm;

const RONDAS: u64 = 40;
const GIRO: u64 = 2_000_000;

#[cfg(target_arch = "aarch64")]
const N: usize = 32;
#[cfg(target_arch = "x86_64")]
const N: usize = 16;

#[repr(C, align(16))]
struct Registros([u128; N]);

#[cfg(target_arch = "aarch64")]
fn cargar(r: &Registros) {
    let p = r.0.as_ptr();
    unsafe {
        asm!(
        ".arch_extension simd",
        "ldp q0, q1, [{p}, #0]",
        "ldp q2, q3, [{p}, #32]",
        "ldp q4, q5, [{p}, #64]",
        "ldp q6, q7, [{p}, #96]",
        "ldp q8, q9, [{p}, #128]",
        "ldp q10, q11, [{p}, #160]",
        "ldp q12, q13, [{p}, #192]",
        "ldp q14, q15, [{p}, #224]",
        "ldp q16, q17, [{p}, #256]",
        "ldp q18, q19, [{p}, #288]",
        "ldp q20, q21, [{p}, #320]",
        "ldp q22, q23, [{p}, #352]",
        "ldp q24, q25, [{p}, #384]",
        "ldp q26, q27, [{p}, #416]",
        "ldp q28, q29, [{p}, #448]",
        "ldp q30, q31, [{p}, #480]",
        p = in(reg) p,
        options(nostack, readonly),
        )
    };
}

#[cfg(target_arch = "aarch64")]
fn guardar(r: &mut Registros) {
    let p = r.0.as_mut_ptr();
    unsafe {
        asm!(
        ".arch_extension simd",
        "stp q0, q1, [{p}, #0]",
        "stp q2, q3, [{p}, #32]",
        "stp q4, q5, [{p}, #64]",
        "stp q6, q7, [{p}, #96]",
        "stp q8, q9, [{p}, #128]",
        "stp q10, q11, [{p}, #160]",
        "stp q12, q13, [{p}, #192]",
        "stp q14, q15, [{p}, #224]",
        "stp q16, q17, [{p}, #256]",
        "stp q18, q19, [{p}, #288]",
        "stp q20, q21, [{p}, #320]",
        "stp q22, q23, [{p}, #352]",
        "stp q24, q25, [{p}, #384]",
        "stp q26, q27, [{p}, #416]",
        "stp q28, q29, [{p}, #448]",
        "stp q30, q31, [{p}, #480]",
        p = in(reg) p,
        options(nostack),
        )
    };
}

#[cfg(target_arch = "aarch64")]
fn girar(mut n: u64) {
    unsafe { asm!("1: subs {n}, {n}, #1", "b.ne 1b", n = inout(reg) n, options(nostack, nomem)) };
    let _ = n;
}

#[cfg(target_arch = "aarch64")]
fn sumar(a: f64, b: f64) -> f64 {
    let r: u64;
    unsafe {
        asm!(
            ".arch_extension fp",
            "fmov d0, {a}",
            "fmov d1, {b}",
            "fadd d0, d0, d1",
            "fmov {r}, d0",
            a = in(reg) a.to_bits(),
            b = in(reg) b.to_bits(),
            r = out(reg) r,
            options(nostack, nomem),
        )
    };
    f64::from_bits(r)
}

/// Con el target sin SSE, el compilador no toca estos registros: el ensamblador los usa a mano.
#[cfg(target_arch = "x86_64")]
fn cargar(r: &Registros) {
    let p = r.0.as_ptr();
    unsafe {
        asm!(
            "movdqa xmm0, [{p}]", "movdqa xmm1, [{p} + 16]", "movdqa xmm2, [{p} + 32]", "movdqa xmm3, [{p} + 48]",
            "movdqa xmm4, [{p} + 64]", "movdqa xmm5, [{p} + 80]", "movdqa xmm6, [{p} + 96]", "movdqa xmm7, [{p} + 112]",
            "movdqa xmm8, [{p} + 128]", "movdqa xmm9, [{p} + 144]", "movdqa xmm10, [{p} + 160]", "movdqa xmm11, [{p} + 176]",
            "movdqa xmm12, [{p} + 192]", "movdqa xmm13, [{p} + 208]", "movdqa xmm14, [{p} + 224]", "movdqa xmm15, [{p} + 240]",
            p = in(reg) p,
            options(nostack, readonly),
        )
    };
}

#[cfg(target_arch = "x86_64")]
fn guardar(r: &mut Registros) {
    let p = r.0.as_mut_ptr();
    unsafe {
        asm!(
            "movdqa [{p}], xmm0", "movdqa [{p} + 16], xmm1", "movdqa [{p} + 32], xmm2", "movdqa [{p} + 48], xmm3",
            "movdqa [{p} + 64], xmm4", "movdqa [{p} + 80], xmm5", "movdqa [{p} + 96], xmm6", "movdqa [{p} + 112], xmm7",
            "movdqa [{p} + 128], xmm8", "movdqa [{p} + 144], xmm9", "movdqa [{p} + 160], xmm10", "movdqa [{p} + 176], xmm11",
            "movdqa [{p} + 192], xmm12", "movdqa [{p} + 208], xmm13", "movdqa [{p} + 224], xmm14", "movdqa [{p} + 240], xmm15",
            p = in(reg) p,
            options(nostack),
        )
    };
}

#[cfg(target_arch = "x86_64")]
fn girar(mut n: u64) {
    unsafe { asm!("2: dec {n}", "jnz 2b", n = inout(reg) n, options(nostack, nomem)) };
    let _ = n;
}

#[cfg(target_arch = "x86_64")]
fn sumar(a: f64, b: f64) -> f64 {
    let r: u64;
    let (a, b) = (a.to_bits(), b.to_bits());
    unsafe {
        asm!(
            "sub rsp, 16",
            "mov [rsp], {a}",
            "mov [rsp + 8], {b}",
            "movsd xmm0, [rsp]",
            "addsd xmm0, [rsp + 8]",
            "movsd [rsp], xmm0",
            "mov {r}, [rsp]",
            "add rsp, 16",
            a = in(reg) a,
            b = in(reg) b,
            r = lateout(reg) r,
        )
    };
    f64::from_bits(r)
}

/// x86: MXCSR y la palabra de control del x87 propios (redondeo hacia abajo en uno, hacia arriba en el otro), que
/// también son de cada hilo. (MXCSR, FCW) leídos.
#[cfg(target_arch = "x86_64")]
fn controles(poner: Option<(u32, u16)>) -> (u32, u16) {
    let (mut m, mut c) = (0u32, 0u16);
    unsafe {
        if let Some((pm, pc)) = poner {
            asm!("ldmxcsr [{}]", "fldcw [{}]", in(reg) &pm, in(reg) &pc, options(nostack));
        }
        asm!("stmxcsr [{}]", "fnstcw [{}]", in(reg) &mut m, in(reg) &mut c, options(nostack));
    }
    (m, c)
}

fn principal() -> u64 {
    // Al empezar, los valores iniciales: MXCSR 0x1F80 y FCW 0x37F (sin excepciones, al más cercano).
    #[cfg(target_arch = "x86_64")]
    let mios = {
        if controles(None) != (0x1f80, 0x037f) {
            return 3;
        }
        let abajo = SEMILLA & 1 == 0;
        let m = (0x1f80 | if abajo { 0x2000 } else { 0x4000 }, 0x037f | if abajo { 0x0400 } else { 0x0800 });
        controles(Some(m));
        m
    };
    let mut a = Registros([0; N]);
    let mut x = SEMILLA;
    for (i, q) in a.0.iter_mut().enumerate() {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407 + i as u64);
        *q = ((x as u128) << 64) | (x.rotate_left(17) as u128);
    }
    let suma_bien = sumar(1.5, 2.25).to_bits() == 3.75f64.to_bits();
    let mut b = Registros([0; N]);
    for _ in 0..RONDAS {
        cargar(&a);
        girar(GIRO);
        guardar(&mut b);
        if b.0 != a.0 {
            return 1;
        }
        #[cfg(target_arch = "x86_64")]
        if controles(None) != mios {
            return 4;
        }
    }
    if suma_bien { 28 } else { 2 }
}
