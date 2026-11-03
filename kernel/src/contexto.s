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
