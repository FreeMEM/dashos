// Los invitados de prueba de la caja Linux (H9): AArch64 sin sistema, para una máquina virtual de DashOS. Los ensambla
// el propio servicio `caja-linux` (global_asm!) y los copia en la RAM del invitado. Cada uno empieza en su primera
// instrucción, en EL1, con la MMU apagada, la RAM en 0x40000000 (64 KB) y un PL011 en 0x09000000 que emula el
// servicio: escribir en DR (0x00) saca un carácter; FR (0x18) dice si hay sitio (el bit 5, TXFF, a 0).
// Van como datos (.rodata): el servicio no los ejecuta, solo los copia.

    .section .rodata.invitados, "a"

// Escribe un texto por el UART, esperando sitio antes de cada carácter, y acaba con HVC #0 y x0 = 0.
    .balign 4
    .globl  invitado_hola
invitado_hola:
    movz    x1, #0x0900, lsl #16        // el UART
    adr     x2, 3f
1:  ldrb    w3, [x2], #1
    cbz     w3, 2f
4:  ldr     w4, [x1, #0x18]             // FR
    tbnz    w4, #5, 4b                  // TXFF: lleno, a esperar
    str     w3, [x1]                    // DR
    b       1b
2:  mov     x0, #0
    hvc     #0
    b       2b
3:  .asciz  "Hola desde el invitado, por el UART emulado\n"
    .balign 4
    .globl  invitado_hola_fin
invitado_hola_fin:

// Para medir: HVC #1 sin parar. Cada uno es una ida y vuelta entera al servicio.
    .globl  invitado_eco
invitado_eco:
    mov     x0, #1
    hvc     #1
    b       invitado_eco
    .globl  invitado_eco_fin
invitado_eco_fin:

// Hostil: lee fuera de su memoria, en una dirección donde no hay nada.
    .globl  invitado_lee_fuera
invitado_lee_fuera:
    movz    x1, #0x7000, lsl #16
    ldr     x0, [x1]
    hvc     #0
    .globl  invitado_lee_fuera_fin
invitado_lee_fuera_fin:

// Hostil: escribe en 0x40200000, pasado su RAM. En la máquina de verdad (QEMU virt) es donde está el kernel de DashOS:
// para el invitado no existe.
    .globl  invitado_escribe_fuera
invitado_escribe_fuera:
    movz    x1, #0x4020, lsl #16
    movz    x0, #0xdead
    str     x0, [x1]
    hvc     #0
    .globl  invitado_escribe_fuera_fin
invitado_escribe_fuera_fin:

// Hostil: un bucle sin fin, con las interrupciones enmascaradas.
    .globl  invitado_bucle
invitado_bucle:
    b       invitado_bucle
    .globl  invitado_bucle_fin
invitado_bucle_fin:

// Hostil: pide al firmware que apague la máquina (PSCI SYSTEM_OFF, 0x84000008, por SMC).
    .globl  invitado_apaga
invitado_apaga:
    movz    x0, #0x8400, lsl #16
    add     x0, x0, #8
    smc     #0
    hvc     #0
    .globl  invitado_apaga_fin
invitado_apaga_fin:

// Hostil: apaga el temporizador físico, el del tic del kernel.
    .globl  invitado_reloj
invitado_reloj:
    msr     cntp_ctl_el0, xzr
    hvc     #0
    .globl  invitado_reloj_fin
invitado_reloj_fin:
