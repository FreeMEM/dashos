// DashOS: tabla de vectores de excepción de EL1.
//
// Cada entrada guarda x0 y x1, pone el tipo en x1 y salta a "comun", que guarda el resto en un marco en la pila
// (struct Marco en excepciones.rs), llama a excepcion() en Rust y vuelve con eret.
//
// Desde EL0 la excepción llega con la pila del kernel del hilo (SP_EL1), que es la que tenía al bajar a EL0.

    .section .text.vectores, "ax"

// Antes de escribir el marco se comprueba que cabe en la pila (C34). Toda pila del kernel es la mitad de arriba de
// una ranura de 32 KB alineada (mapa.rs), así que una dirección válida tiene el bit 14 a 1; con el bit 14 a 0, el
// marco caería en la guarda. Sin registros libres, SP y x0 se intercambian con sumas, sin tocar la memoria (como
// Linux). Si no cabe, se pasa a la pila de emergencia del núcleo y el kernel para con un mensaje.
.macro ENTRADA tipo
    .balign 0x80
    sub     sp, sp, #288
    add     sp, sp, x0                  // sp' = sp + x0
    sub     x0, sp, x0                  // x0 = sp
    tbz     x0, #14, desbordada
    sub     x0, sp, x0                  // x0 = x0 de antes
    sub     sp, sp, x0                  // sp = sp de antes
    stp     x0, x1, [sp, #0]
    mov     x1, #\tipo
    b       comun
.endm

    .balign 0x800
    .globl  vectores_el1
vectores_el1:
    // EL1 con SP_EL0
    ENTRADA 0
    ENTRADA 1
    ENTRADA 2
    ENTRADA 3
    // EL1 con SP_EL1
    ENTRADA 4
    ENTRADA 5
    ENTRADA 6
    ENTRADA 7
    // EL0 en AArch64
    ENTRADA 8
    ENTRADA 9
    ENTRADA 10
    ENTRADA 11
    // EL0 en AArch32
    ENTRADA 12
    ENTRADA 13
    ENTRADA 14
    ENTRADA 15

// La pila del kernel se ha desbordado: x0 = el SP que no cabía.
desbordada:
    mrs     x1, mpidr_el1
    and     x1, x1, #0xff
    ldr     x2, =pila_emergencia_fin
    sub     x2, x2, x1, lsl #12
    mov     sp, x2
    mrs     x1, elr_el1
    bl      pila_desbordada
0:  wfe
    b       0b

    .section .bss.emergencia, "aw", %nobits
    .balign 4096
pila_emergencia:
    .space  4 * 4096
pila_emergencia_fin:

    .section .text.vectores, "ax"
comun:
    stp     x2, x3, [sp, #16]
    stp     x4, x5, [sp, #32]
    stp     x6, x7, [sp, #48]
    stp     x8, x9, [sp, #64]
    stp     x10, x11, [sp, #80]
    stp     x12, x13, [sp, #96]
    stp     x14, x15, [sp, #112]
    stp     x16, x17, [sp, #128]
    stp     x18, x19, [sp, #144]
    stp     x20, x21, [sp, #160]
    stp     x22, x23, [sp, #176]
    stp     x24, x25, [sp, #192]
    stp     x26, x27, [sp, #208]
    stp     x28, x29, [sp, #224]
    str     x30, [sp, #240]
    mrs     x2, elr_el1
    mrs     x3, spsr_el1
    stp     x2, x3, [sp, #248]
    mrs     x2, esr_el1
    mrs     x3, far_el1
    stp     x2, x3, [sp, #264]
    mrs     x2, sp_el0                  // pila de usuario: cada hilo tiene la suya (C11)
    str     x2, [sp, #280]

    mov     x0, sp                  // excepcion(&mut Marco, tipo)
    bl      excepcion

    ldp     x2, x3, [sp, #248]
    msr     elr_el1, x2
    msr     spsr_el1, x3
    ldr     x2, [sp, #280]
    msr     sp_el0, x2
    ldp     x2, x3, [sp, #16]
    ldp     x4, x5, [sp, #32]
    ldp     x6, x7, [sp, #48]
    ldp     x8, x9, [sp, #64]
    ldp     x10, x11, [sp, #80]
    ldp     x12, x13, [sp, #96]
    ldp     x14, x15, [sp, #112]
    ldp     x16, x17, [sp, #128]
    ldp     x18, x19, [sp, #144]
    ldp     x20, x21, [sp, #160]
    ldp     x22, x23, [sp, #176]
    ldp     x24, x25, [sp, #192]
    ldp     x26, x27, [sp, #208]
    ldp     x28, x29, [sp, #224]
    ldr     x30, [sp, #240]
    ldp     x0, x1, [sp, #0]
    add     sp, sp, #288
    eret
