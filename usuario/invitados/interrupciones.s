// Los invitados de prueba de las interrupciones de la caja Linux (H9, tercer paso): el GIC virtual y el temporizador
// virtual. Como los de pruebas.s: AArch64 sin sistema, en EL1 con la MMU apagada, la RAM en 0x40000000 (64 KB). El
// servicio les proyecta la GICV del GIC en 0x08010000, donde esperan la interfaz de CPU de su GIC (GICC, IHI 0048B
// §4.4: CTLR en 0x00, PMR en 0x04, IAR en 0x0c, EOIR en 0x10).
//
// Cada uno empieza en una dirección alineada a 2 KB y su tabla de vectores va 2 KB más allá (VBAR_EL1 pide 2 KB):
// copiado al principio de la RAM, la tabla queda en 0x40000800. Entradas de la tabla (EL1 con SP_EL1): síncrona en
// 0x200, IRQ en 0x280; las demás acaban con HVC #2 (no deberían llegar).

    .section .rodata.invitados_irq, "a"

// El temporizador y una interrupción del servicio. Arma el temporizador virtual a 2000 cuentas tres veces (en cada
// interrupción, IAR = 27, lo vuelve a armar; a la tercera lo apaga), espera con WFI, y luego pide al servicio una SPI
// escribiendo en el "timbre" 0x0a000000: el servicio pone la 48 en un registro de lista. Después pregunta la versión
// de PSCI por HVC #0 (x0 = 0x84000000; el servicio contesta en x0) y acaba con HVC #1: x0 = interrupciones del
// temporizador, x1 = de la SPI, x2 = versión de PSCI.
// x20, x21: cuentas. x9..x12: del manejador; x13, del que arma el temporizador.
    .balign 0x800
    .globl  invitado_irq
invitado_irq:
    adr     x0, invitado_irq + 0x800
    msr     vbar_el1, x0
    isb
    movz    x1, #0x0801, lsl #16        // la GICC del invitado (la GICV)
    mov     w2, #0xf0
    str     w2, [x1, #4]                // PMR: deja pasar las de prioridad menor que 0xf0
    mov     w2, #1
    str     w2, [x1, #0]                // CTLR: encendida
    mov     x20, #0
    mov     x21, #0
    bl      1f
    msr     daifclr, #2                 // IRQ
2:  wfi
    cmp     x20, #3
    b.lt    2b
    movz    x3, #0x0a00, lsl #16
    str     wzr, [x3]                   // el timbre
3:  wfi
    cbz     x21, 3b
    msr     daifset, #2
    movz    x0, #0x8400, lsl #16        // PSCI_VERSION
    hvc     #0
    mov     x2, x0
    mov     x0, x20
    mov     x1, x21
    hvc     #1
4:  b       4b
// El temporizador virtual, a 2000 cuentas de ahora (32 µs a 62,5 MHz).
1:  mrs     x13, cntvct_el0
    add     x13, x13, #2000
    msr     cntv_cval_el0, x13
    mov     x13, #1
    msr     cntv_ctl_el0, x13
    isb
    ret
5:  movz    x9, #0x0801, lsl #16
    ldr     w10, [x9, #0x0c]            // IAR
    and     w11, w10, #0x3ff
    cmp     w11, #27
    b.ne    6f
    add     x20, x20, #1
    cmp     x20, #3
    b.ge    7f
    mov     x12, x30
    bl      1b
    mov     x30, x12
    b       8f
7:  msr     cntv_ctl_el0, xzr
    b       8f
6:  cmp     w11, #48
    b.ne    8f
    add     x21, x21, #1
8:  str     w10, [x9, #0x10]            // EOIR (el GIC ignora el de una espuria, 1023)
    eret

    .balign 0x800
    .rept   4
    .balign 0x80
    hvc     #2
    .endr
    .balign 0x80                        // 0x200: síncrona
    hvc     #2
    .balign 0x80                        // 0x280: IRQ
    b       5b
    .rept   10
    .balign 0x80
    hvc     #2
    .endr
    .balign 0x80
    .globl  invitado_irq_fin
invitado_irq_fin:

// Hostil con el GIC: 4096 escrituras de 32 bits al azar (un generador lineal) por las dos páginas de su GICV,
// lecturas de IAR y escrituras de EOIR con números al azar, 4096 escrituras al azar por su distribuidor (el que emula
// el servicio, en 0x08000000) y lecturas y escrituras en 0x08030000, donde en QEMU está la GICH del anfitrión: para
// el invitado no hay nada (el servicio le da un aborto externo, que su vector cuenta y salta). Acaba con HVC #0 y
// x0 = abortos vistos.
// x20: abortos. x21: estado del generador.
    .balign 0x800
    .globl  invitado_gic_hostil
invitado_gic_hostil:
    adr     x0, invitado_gic_hostil + 0x800
    msr     vbar_el1, x0
    isb
    mov     x20, #0
    movz    x21, #0x1234
    movz    x22, #0x4c95, lsl #16       // multiplicador (cualquiera impar sirve)
    movk    x22, #0x7f2d
    movz    x1, #0x0801, lsl #16        // la GICV
    mov     x5, #4096
1:  madd    x21, x21, x22, x22
    lsr     x2, x21, #20
    and     x2, x2, #0x1ffc             // desplazamiento en las dos páginas, alineado a 4
    lsr     x3, x21, #32
    str     w3, [x1, x2]
    ldr     w4, [x1, #0x0c]             // IAR
    str     w3, [x1, #0x10]             // EOIR con un número cualquiera
    subs    x5, x5, #1
    b.ne    1b
    movz    x1, #0x0800, lsl #16        // su distribuidor
    mov     x5, #4096
2:  madd    x21, x21, x22, x22
    lsr     x2, x21, #20
    and     x2, x2, #0xffc
    lsr     x3, x21, #32
    str     w3, [x1, x2]
    ldr     w4, [x1, x2]
    subs    x5, x5, #1
    b.ne    2b
    movz    x1, #0x0803, lsl #16        // la GICH del anfitrión (en QEMU)
    ldr     w4, [x1, #0x100]
    str     w4, [x1, #0x000]
    mov     x0, x20
    hvc     #0
3:  b       3b
// Un aborto síncrono: se cuenta y se salta la instrucción.
4:  add     x20, x20, #1
    mrs     x9, elr_el1
    add     x9, x9, #4
    msr     elr_el1, x9
    eret

    .balign 0x800
    .rept   4
    .balign 0x80
    hvc     #2
    .endr
    .balign 0x80                        // 0x200: síncrona
    b       4b
    .rept   11
    .balign 0x80
    hvc     #2
    .endr
    .balign 0x80
    .globl  invitado_gic_hostil_fin
invitado_gic_hostil_fin:

// Hostil: una tormenta del temporizador. Lo arma ya disparado (CVAL = 0) y su vector de IRQ vuelve sin reconocerla:
// la interrupción sigue pendiente y entra otra vez, sin fin. El reloj del anfitrión lo saca igual.
    .balign 0x800
    .globl  invitado_tormenta
invitado_tormenta:
    adr     x0, invitado_tormenta + 0x800
    msr     vbar_el1, x0
    isb
    movz    x1, #0x0801, lsl #16
    mov     w2, #0xf0
    str     w2, [x1, #4]
    mov     w2, #1
    str     w2, [x1, #0]
    msr     cntv_cval_el0, xzr
    mov     x9, #1
    msr     cntv_ctl_el0, x9
    isb
    msr     daifclr, #2
1:  b       1b

    .balign 0x800
    .rept   10
    .balign 0x80
    eret
    .endr
    .rept   6
    .balign 0x80
    eret
    .endr
    .balign 0x80
    .globl  invitado_tormenta_fin
invitado_tormenta_fin:

// Hostil: escribe en un registro de EL2 (VTTBR_EL2). Desde EL1 no existe: la excepción va a su propio vector, que con
// VBAR_EL1 = 0 está fuera de su memoria, y la máquina se para por el fallo de la etapa 2.
    .balign 4
    .globl  invitado_el2
invitado_el2:
    msr     vttbr_el2, xzr
    hvc     #0
    .globl  invitado_el2_fin
invitado_el2_fin:
