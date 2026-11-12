// El invitado de prueba de varias vCPU de la caja Linux (H9): dos vCPU, PSCI y SGI entre ellas. Como los de
// interrupciones.s: AArch64 sin sistema, en EL1 con la MMU apagada, la RAM en 0x40000000 (64 KB), la GICV en
// 0x08010000 (su GICC) y el distribuidor que emula el servicio en 0x08000000 (IHI 0048B: GICD_CTLR 0x000,
// GICD_ISENABLER0 0x100, GICD_SGIR 0xf00; GICC_CTLR 0x00, PMR 0x04, IAR 0x0c, EOIR 0x10). PSCI por HVC #0 (DEN 0022).
//
// La vCPU 0 empieza en `invitado_smp` con x0 = el modo:
//   0: PSCI (CPU_ON a una vCPU que no existe, a la 1 de verdad y otra vez a la 1; AFFINITY_INFO), cien idas y vueltas
//      de SGI con la 1 (la 0 le manda la SGI 1, la 1 contesta con la 2), y la tormenta: la 1 manda la SGI 3 a la 0 sin
//      parar mientras la 0 hace un trabajo. Acaba con HVC #1: x0 = cuentas del contador de las cien vueltas, x1 =
//      vueltas, x2 = respuestas de PSCI que no son las esperadas, x3 = SGI de la tormenta recibidas; y deja en x4 lo que
//      tardó el trabajo sola (antes de encender a la 1) y en x5 lo que tardó con la tormenta. La 1 sigue en su tormenta:
//      la máquina se para con ella dentro.
//   1: enciende a la 1, que se queda en un bucle sin fin con todo enmascarado, hace el mismo trabajo (x5; x4, sola) y
//      apaga la máquina por PSCI SYSTEM_OFF con la 1 aún en su bucle.
//
// Los datos compartidos, en RAM + 0x8000: [0x08] la 1 escribe ahí su contexto al empezar (0x55), [0x10] la 0 pide la
// tormenta, [0x18] el modo, para la 1.
// Cada vCPU tiene su tabla de vectores (2 KB alineada): IRQ en +0x280; lo demás acaba con HVC #2.
// Registros: x24 la GICC, x26 el GICD, x27 los datos; x20 SGI 2 recibidas, x21 SGI 3 recibidas (en la 0). Los
// manejadores usan x14 a x16.

    .section .rodata.invitados_smp, "a"

    .balign 0x800
    .globl  invitado_smp
invitado_smp:
    mov     x28, x0
    adr     x1, invitado_smp + 0x800
    msr     vbar_el1, x1
    isb
    movz    x27, #0x4000, lsl #16
    add     x27, x27, #0x8000
    stp     xzr, xzr, [x27]
    stp     xzr, xzr, [x27, #16]
    str     x28, [x27, #0x18]
    movz    x26, #0x0800, lsl #16       // el GICD
    mov     w1, #1
    str     w1, [x26]                   // GICD_CTLR: el grupo 0
    mov     w1, #0xe
    str     w1, [x26, #0x100]           // GICD_ISENABLER0 (el de esta vCPU): SGI 1, 2 y 3
    movz    x24, #0x0801, lsl #16       // la GICC (la GICV)
    mov     w1, #0xf0
    str     w1, [x24, #4]               // PMR
    mov     w1, #1
    str     w1, [x24]                   // CTLR
    mov     x20, #0
    mov     x21, #0
    mov     x25, #0
    bl      trabajo
    mov     x4, x13                     // el trabajo, sola
    cbnz    x28, 50f

    // CPU_ON (64 bits) a la vCPU 7, que no existe: INVALID_PARAMETERS (-2).
    movz    x0, #0xc400, lsl #16
    movk    x0, #3
    mov     x1, #7
    adr     x2, secundaria
    mov     x3, #0
    hvc     #0
    cmn     x0, #2
    b.eq    1f
    add     x25, x25, #1
1:  // CPU_ON a la 1 con contexto 0x55: SUCCESS.
    movz    x0, #0xc400, lsl #16
    movk    x0, #3
    mov     x1, #1
    adr     x2, secundaria
    mov     x3, #0x55
    hvc     #0
    cbz     x0, 2f
    add     x25, x25, #1
2:  // Otra vez: ALREADY_ON (-4) u ON_PENDING (-5).
    movz    x0, #0xc400, lsl #16
    movk    x0, #3
    mov     x1, #1
    adr     x2, secundaria
    mov     x3, #0x55
    hvc     #0
    cmn     x0, #4
    b.eq    3f
    cmn     x0, #5
    b.eq    3f
    add     x25, x25, #1
3:  ldr     x1, [x27, #8]               // espera a que la 1 diga que está
    cmp     x1, #0x55
    b.ne    3b
    // AFFINITY_INFO (64 bits) de la 1, nivel 0: ON (0).
    movz    x0, #0xc400, lsl #16
    movk    x0, #4
    mov     x1, #1
    mov     x2, #0
    hvc     #0
    cbz     x0, 4f
    add     x25, x25, #1
4:  msr     daifclr, #2
    // Cien idas y vueltas: la SGI 1 a la 1 (lista de destinos con solo la CPU 1) y espera su SGI 2.
    mov     x22, #0
    mov     x23, #0
    movz    w9, #2, lsl #16
    movk    w9, #1
5:  isb
    mrs     x10, cntvct_el0
    add     x11, x20, #1
    str     w9, [x26, #0xf00]           // GICD_SGIR
6:  wfi
    cmp     x20, x11
    b.lt    6b
    isb
    mrs     x12, cntvct_el0
    sub     x12, x12, x10
    add     x22, x22, x12
    add     x23, x23, #1
    cmp     x23, #100
    b.lt    5b
    // La tormenta: se la pide a la 1 (y la despierta con otra SGI 1) y trabaja mientras le llueven SGI 3.
    mov     x1, #1
    str     x1, [x27, #0x10]
    str     w9, [x26, #0xf00]
7:  cbz     x21, 7b                     // hasta que llegue la primera
    mov     x21, #0
    bl      trabajo
    mov     x5, x13
    mov     x0, x22
    mov     x1, x23
    mov     x2, x25
    mov     x3, x21
    hvc     #1
8:  b       8b

    // Modo 1: la 1 en un bucle sin fin; la 0 trabaja y apaga la máquina.
50: movz    x0, #0xc400, lsl #16
    movk    x0, #3
    mov     x1, #1
    adr     x2, secundaria
    mov     x3, #0x55
    hvc     #0
51: ldr     x1, [x27, #8]
    cmp     x1, #0x55
    b.ne    51b
    bl      trabajo
    mov     x5, x13
    movz    x0, #0x8400, lsl #16        // SYSTEM_OFF
    movk    x0, #8
    hvc     #0
52: b       52b

// Un trabajo fijo: 4M vueltas de un bucle. En x13, las cuentas del contador que tardó. Usa x12 y x13.
trabajo:
    isb
    mrs     x12, cntvct_el0
    movz    x13, #0x40, lsl #16
1:  subs    x13, x13, #1
    b.ne    1b
    isb
    mrs     x13, cntvct_el0
    sub     x13, x13, x12
    ret

// IRQ de la 0: SGI 2 (la vuelta) o SGI 3 (la tormenta).
irq0:
    ldr     w14, [x24, #0x0c]           // IAR
    and     w15, w14, #0x3ff
    cmp     w15, #2
    b.ne    1f
    add     x20, x20, #1
    b       2f
1:  cmp     w15, #3
    b.ne    2f
    add     x21, x21, #1
2:  str     w14, [x24, #0x10]           // EOIR
    eret

    .balign 0x800
    .rept   4
    .balign 0x80
    hvc     #2
    .endr
    .balign 0x80                        // 0x200: síncrona
    hvc     #2
    .balign 0x80                        // 0x280: IRQ
    b       irq0
    .rept   10
    .balign 0x80
    hvc     #2
    .endr

// La vCPU 1, encendida por CPU_ON con x0 = el contexto.
    .balign 0x800
secundaria:
    adr     x1, secundaria + 0x800
    msr     vbar_el1, x1
    isb
    movz    x27, #0x4000, lsl #16
    add     x27, x27, #0x8000
    movz    x26, #0x0800, lsl #16
    mov     w1, #0xe
    str     w1, [x26, #0x100]           // su GICD_ISENABLER0
    movz    x24, #0x0801, lsl #16
    mov     w1, #0xf0
    str     w1, [x24, #4]
    mov     w1, #1
    str     w1, [x24]
    ldr     x28, [x27, #0x18]
    str     x0, [x27, #8]               // aquí está
    cbnz    x28, 3f
    msr     daifclr, #2
1:  ldr     x1, [x27, #0x10]
    cbnz    x1, 2f
    wfi
    b       1b
    // La tormenta: la SGI 3 a la 0, sin parar y sin mirar nada más.
2:  msr     daifset, #2
    movz    w1, #1, lsl #16
    movk    w1, #3
4:  str     w1, [x26, #0xf00]
    b       4b
    // Modo 1: un bucle sin fin con todo enmascarado.
3:  msr     daifset, #0xf
5:  b       5b

// IRQ de la 1: a la SGI 1 contesta con la SGI 2 a la 0.
irq1:
    ldr     w14, [x24, #0x0c]
    and     w15, w14, #0x3ff
    cmp     w15, #1
    b.ne    1f
    movz    w16, #1, lsl #16
    movk    w16, #2
    str     w16, [x26, #0xf00]
1:  str     w14, [x24, #0x10]
    eret

    .balign 0x800
    .rept   4
    .balign 0x80
    hvc     #2
    .endr
    .balign 0x80
    hvc     #2
    .balign 0x80
    b       irq1
    .rept   10
    .balign 0x80
    hvc     #2
    .endr
    .balign 0x80
    .globl  invitado_smp_fin
invitado_smp_fin:
