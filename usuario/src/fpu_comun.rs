// C28: dos procesos en el mismo núcleo llenan los 32 registros de SIMD con patrones distintos, giran un rato para
// que el reloj los cambie, y comprueban que los registros siguen igual. Cada uno incluye este fichero con su SEMILLA.
// También suma 1,5 + 2,25 con la FPU de verdad. Sale con 28 si todo va bien.

use core::arch::asm;

const RONDAS: u64 = 40;
const GIRO: u64 = 2_000_000;

#[repr(C, align(16))]
struct Registros([u128; 32]);

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

fn girar(mut n: u64) {
    unsafe { asm!("1: subs {n}, {n}, #1", "b.ne 1b", n = inout(reg) n, options(nostack, nomem)) };
    let _ = n;
}

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

fn principal() -> u64 {
    let mut a = Registros([0; 32]);
    let mut x = SEMILLA;
    for (i, q) in a.0.iter_mut().enumerate() {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407 + i as u64);
        *q = ((x as u128) << 64) | (x.rotate_left(17) as u128);
    }
    let suma_bien = sumar(1.5, 2.25).to_bits() == 3.75f64.to_bits();
    let mut b = Registros([0; 32]);
    for _ in 0..RONDAS {
        cargar(&a);
        girar(GIRO);
        guardar(&mut b);
        if b.0 != a.0 {
            return 1;
        }
    }
    if suma_bien { 28 } else { 2 }
}
