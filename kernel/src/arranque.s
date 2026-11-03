// DashOS: entrada del kernel en AArch64.
//
// El firmware de la Pi (y QEMU raspi4b) salta aquí en EL2, con la MMU apagada, en little-endian y con la
// dirección del árbol de dispositivos en x0. Solo entra el núcleo 0: los demás esperan en la tabla de espera
// del firmware (se despiertan en C9).
//
// Ojo: hasta poner el bit EE no se puede leer ningún dato de la imagen (las constantes con "ldr =" están en
// big-endian). Antes de eso solo se usan mrs, msr y mov.

    .section .text.arranque, "ax"
    .globl _start
_start:
    mov     x20, x0                     // árbol de dispositivos
    mrs     x1, mpidr_el1
    and     x1, x1, #0xff
    cbz     x1, 1f
0:  wfe                                 // otro núcleo: esperar
    b       0b

1:  mrs     x2, CurrentEL
    lsr     x2, x2, #2
    cmp     x2, #2
    b.ne    desde_el1

    // --- En EL2 ---
    mrs     x2, sctlr_el2               // big-endian en EL2
    orr     x2, x2, #(1 << 25)
    msr     sctlr_el2, x2
    isb

    mov     x2, #(1 << 31)              // HCR_EL2.RW: EL1 en AArch64
    msr     hcr_el2, x2
    mrs     x2, cnthctl_el2             // temporizador físico accesible desde EL1
    orr     x2, x2, #3
    msr     cnthctl_el2, x2
    msr     cntvoff_el2, xzr

    ldr     x2, =0x33D00800             // SCTLR_EL1: bits RES1, EE (25) y E0E (24); MMU y cachés apagadas
    msr     sctlr_el1, x2

    mov     x2, #0x3c5                  // SPSR_EL2: EL1h con D, A, I y F enmascaradas
    msr     spsr_el2, x2
    adr     x2, en_el1
    msr     elr_el2, x2
    eret

desde_el1:
    mrs     x2, sctlr_el1               // si el firmware ya nos dejó en EL1: big-endian aquí
    orr     x2, x2, #(3 << 24)
    msr     sctlr_el1, x2
    isb

en_el1:
    ldr     x1, =__pilas_fin            // pila del núcleo 0
    mov     sp, x1

    ldr     x1, =__bss_inicio           // limpiar .bss
    ldr     x2, =__bss_fin
2:  cmp     x1, x2
    b.hs    3f
    str     xzr, [x1], #8
    b       2b

3:  mov     x0, x20
    bl      kmain
4:  wfe
    b       4b
