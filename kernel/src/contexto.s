// DashOS: cambio de contexto entre hilos del kernel (C10).
//
// cambiar_contexto(viejo: *mut Contexto, nuevo: *const Contexto)
//   Guarda en "viejo" los registros que la convención de llamada obliga a conservar (x19-x29), el de vuelta (x30)
//   y la pila, y carga los de "nuevo". Vuelve en el hilo nuevo, donde este se quedó (o en hilo_entrada si es nuevo).
//
// Contexto: x19..x29 (11), x30, sp  = 13 palabras.

    .section .text, "ax"
    .globl  cambiar_contexto
cambiar_contexto:
    stp     x19, x20, [x0, #0]
    stp     x21, x22, [x0, #16]
    stp     x23, x24, [x0, #32]
    stp     x25, x26, [x0, #48]
    stp     x27, x28, [x0, #64]
    stp     x29, x30, [x0, #80]
    mov     x9, sp
    str     x9, [x0, #96]

    ldp     x19, x20, [x1, #0]
    ldp     x21, x22, [x1, #16]
    ldp     x23, x24, [x1, #32]
    ldp     x25, x26, [x1, #48]
    ldp     x27, x28, [x1, #64]
    ldp     x29, x30, [x1, #80]
    ldr     x9, [x1, #96]
    mov     sp, x9
    ret

// Primera vez que corre un hilo: x19 = función, x20 = argumento.
    .globl  hilo_entrada
hilo_entrada:
    bl      hilo_estreno                // suelta lo que dejó el planificador y habilita interrupciones
    mov     x0, x20
    blr     x19
    bl      hilo_fin                    // no vuelve
1:  wfe
    b       1b

// entrar_el0(pc, sp): baja a EL0 en "pc" con la pila de usuario "sp", con las IRQ y los SError habilitados (C11).
// La pila del kernel del hilo (SP_EL1) se queda como está: ahí llegarán sus excepciones.
    .globl  entrar_el0
entrar_el0:
    msr     elr_el1, x0
    msr     sp_el0, x1
    mov     x2, #(1 << 6)               // EL0t con FIQ enmascaradas: DashOS no las usa (auditoría, hallazgo 11)
    msr     spsr_el1, x2
    mov     x0, xzr                     // no dejar datos del kernel en los registros
    mov     x1, xzr
    mov     x2, xzr
    mov     x3, xzr
    mov     x4, xzr
    mov     x5, xzr
    mov     x6, xzr
    mov     x7, xzr
    mov     x8, xzr
    mov     x9, xzr
    mov     x10, xzr
    mov     x11, xzr
    mov     x12, xzr
    mov     x13, xzr
    mov     x14, xzr
    mov     x15, xzr
    mov     x16, xzr
    mov     x17, xzr
    mov     x18, xzr
    mov     x19, xzr
    mov     x20, xzr
    mov     x21, xzr
    mov     x22, xzr
    mov     x23, xzr
    mov     x24, xzr
    mov     x25, xzr
    mov     x26, xzr
    mov     x27, xzr
    mov     x28, xzr
    mov     x29, xzr
    mov     x30, xzr
    eret

// Registros de coma flotante y SIMD de un hilo de usuario (C28). El kernel es softfloat y no los usa nunca; solo los
// guarda y los carga aquí, cuando cambia el dueño de la FPU de un núcleo.
//
// fp_guardar(p) / fp_cargar(p): p apunta a 32 registros de 16 bytes (q0..q31), FPCR y FPSR: 528 bytes, alineados a 16.
    .arch_extension fp
    .arch_extension simd
    .globl  fp_guardar
fp_guardar:
    stp     q0, q1, [x0, #0]
    stp     q2, q3, [x0, #32]
    stp     q4, q5, [x0, #64]
    stp     q6, q7, [x0, #96]
    stp     q8, q9, [x0, #128]
    stp     q10, q11, [x0, #160]
    stp     q12, q13, [x0, #192]
    stp     q14, q15, [x0, #224]
    stp     q16, q17, [x0, #256]
    stp     q18, q19, [x0, #288]
    stp     q20, q21, [x0, #320]
    stp     q22, q23, [x0, #352]
    stp     q24, q25, [x0, #384]
    stp     q26, q27, [x0, #416]
    stp     q28, q29, [x0, #448]
    stp     q30, q31, [x0, #480]
    mrs     x1, fpcr
    mrs     x2, fpsr
    str     x1, [x0, #512]
    str     x2, [x0, #520]
    ret

    .globl  fp_cargar
fp_cargar:
    ldp     q0, q1, [x0, #0]
    ldp     q2, q3, [x0, #32]
    ldp     q4, q5, [x0, #64]
    ldp     q6, q7, [x0, #96]
    ldp     q8, q9, [x0, #128]
    ldp     q10, q11, [x0, #160]
    ldp     q12, q13, [x0, #192]
    ldp     q14, q15, [x0, #224]
    ldp     q16, q17, [x0, #256]
    ldp     q18, q19, [x0, #288]
    ldp     q20, q21, [x0, #320]
    ldp     q22, q23, [x0, #352]
    ldp     q24, q25, [x0, #384]
    ldp     q26, q27, [x0, #416]
    ldp     q28, q29, [x0, #448]
    ldp     q30, q31, [x0, #480]
    ldr     x1, [x0, #512]
    ldr     x2, [x0, #520]
    msr     fpcr, x1
    msr     fpsr, x2
    ret
