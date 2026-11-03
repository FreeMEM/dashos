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
    // --- Tablas de páginas del arranque (S3) ---
    // Una tabla de nivel 1 sirve a la vez para TTBR0 (la memoria física tal cual, para poder seguir aquí al
    // encender la MMU) y para TTBR1 (la misma memoria a partir de KERNEL_VA). Con 39 bits de dirección virtual,
    // cada entrada de nivel 1 es 1 GB: los tres primeros GB son memoria normal; el cuarto va a una tabla de
    // nivel 2 con bloques de 2 MB, porque los periféricos (0xFC000000 en adelante) son memoria de dispositivo.
    // Las tablas se escriben en big-endian: la MMU las lee con el orden de bytes de EL1 (SCTLR_EL1.EE).
    adrp    x0, __tabla_l1              // direcciones físicas (adrp es relativo al PC)
    adrp    x1, __tabla_l2_g3
    mov     x2, x0                      // poner a cero las dos tablas (8 KB seguidos)
    mov     x3, #1024
5:  str     xzr, [x2], #8
    subs    x3, x3, #1
    b.ne    5b

    ldr     x4, =0x705                  // bloque normal: válido, AttrIndx 1, compartible interno, AF
    str     x4, [x0, #0]
    orr     x5, x4, #(1 << 30)
    str     x5, [x0, #8]
    orr     x5, x4, #(2 << 30)
    str     x5, [x0, #16]
    orr     x5, x1, #3                  // entrada de tabla hacia el nivel 2
    str     x5, [x0, #24]

    mov     x2, #0                      // nivel 2 del cuarto GB: 512 bloques de 2 MB
    ldr     x6, =0xC0000000             // dirección física del bloque
    ldr     x7, =0xFC000000             // desde aquí, periféricos
    ldr     x8, =0x0060000000000401     // bloque de dispositivo: válido, AttrIndx 0, AF, PXN y UXN
6:  cmp     x6, x7
    csel    x9, x4, x8, lo
    orr     x9, x9, x6
    str     x9, [x1, x2, lsl #3]
    add     x6, x6, #0x200000
    add     x2, x2, #1
    cmp     x2, #512
    b.ne    6b

    ldr     x2, =0x44FF00               // MAIR: 0 dispositivo nGnRnE, 1 normal con caché, 2 normal sin caché
    msr     mair_el1, x2
    ldr     x2, =0x4B5193519            // TCR: T0SZ=T1SZ=25, 4 KB, caché y compartible interno, IPS 44 bits
    msr     tcr_el1, x2
    msr     ttbr0_el1, x0
    msr     ttbr1_el1, x0
    isb
    ic      iallu
    tlbi    vmalle1
    dsb     ish
    isb
    mrs     x2, sctlr_el1               // MMU, caché de datos y de instrucciones
    orr     x2, x2, #(1 << 0)
    orr     x2, x2, #(1 << 2)
    orr     x2, x2, #(1 << 12)
    msr     sctlr_el1, x2
    isb

    ldr     x2, =en_alto                // saltar a la mitad alta
    br      x2

en_alto:
    ldr     x1, =__pilas_fin            // pila del núcleo 0, ya en direcciones altas
    mov     sp, x1

    ldr     x1, =__bss_inicio           // limpiar .bss
    ldr     x2, =__bss_fin
2:  cmp     x1, x2
    b.hs    3f
    str     xzr, [x1], #8
    b       2b

3:  mov     x0, x20                     // árbol de dispositivos (dirección física)
    bl      kmain
4:  wfe
    b       4b
