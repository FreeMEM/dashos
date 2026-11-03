// DashOS: entrada del kernel en AArch64.
//
// El firmware de la Pi (y QEMU raspi4b) salta a _start en EL2, con la MMU apagada, en little-endian y con la
// dirección del árbol de dispositivos en x0. Solo entra el núcleo 0. Los demás esperan en la tabla de espera del
// firmware hasta que el núcleo 0 les da una dirección: entonces saltan a _secundario, también en EL2 (C9).
//
// El kernel es little-endian desde C33 (S16, A), como el firmware: EE y E0E quedan a 0. El orden de los datos de
// cada proceso lo pone el planificador con E0E (C30).

// Deja el núcleo en EL1, en little-endian, y salta a \destino. Vale tanto si el firmware entra en EL2 como en EL1.
.macro A_EL1 destino
    mrs     x2, CurrentEL
    lsr     x2, x2, #2
    cmp     x2, #2
    b.ne    9f

    mrs     x2, sctlr_el2               // little-endian en EL2
    bic     x2, x2, #(1 << 25)
    msr     sctlr_el2, x2
    isb

    mov     x2, #(1 << 31)              // HCR_EL2.RW: EL1 en AArch64
    msr     hcr_el2, x2
    mrs     x2, cnthctl_el2             // temporizador físico accesible desde EL1
    orr     x2, x2, #3
    msr     cnthctl_el2, x2
    msr     cntvoff_el2, xzr

    ldr     x2, =0x30D00800             // SCTLR_EL1: bits RES1, little-endian; MMU y cachés apagadas
    msr     sctlr_el1, x2

    mov     x2, #0x3c5                  // SPSR_EL2: EL1h con D, A, I y F enmascaradas
    msr     spsr_el2, x2
    adr     x2, \destino
    msr     elr_el2, x2
    eret

9:  mrs     x2, sctlr_el1               // el firmware ya nos dejó en EL1: little-endian aquí
    bic     x2, x2, #(3 << 24)
    msr     sctlr_el1, x2
    isb
    b       \destino
.endm

// Enciende la MMU con las tablas de __tabla_l1 (ya construidas por el núcleo 0) y salta a \destino_alto.
.macro MMU destino_alto
    adrp    x0, __tabla_l1
    ldr     x2, =0x44FF00               // MAIR: 0 dispositivo nGnRnE, 1 normal con caché, 2 normal sin caché
    msr     mair_el1, x2
    ldr     x2, =0x14B5193519           // TCR: T0SZ=T1SZ=25, 4 KB, caché y compartible interno, IPS 44 bits, ASID de 16 bits (AS)
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
    ldr     x2, =\destino_alto          // saltar a la mitad alta
    br      x2
.endm

    .section .text.arranque, "ax"
    .globl _start
_start:
    mov     x20, x0                     // árbol de dispositivos
    mrs     x1, mpidr_el1
    and     x1, x1, #0xff
    cbz     x1, 1f
0:  wfe                                 // otro núcleo que entra por aquí: esperar
    b       0b

1:  A_EL1   en_el1

en_el1:
    // --- Tablas de páginas del arranque (S3, C6) ---
    // Una tabla de nivel 1 sirve a la vez para TTBR0 (la memoria física tal cual, para poder seguir aquí al
    // encender la MMU) y para TTBR1 (la misma memoria a partir de KERNEL_VA). Con 39 bits de dirección virtual,
    // cada entrada de nivel 1 es 1 GB: los tres primeros GB son memoria normal; el cuarto va a una tabla de
    // nivel 2 con bloques de 2 MB, porque los periféricos (0xFC000000 en adelante) son memoria de dispositivo.
    // Las tablas se escriben en el orden de EL1 (little-endian), que es como las lee la MMU (SCTLR_EL1.EE).
    adrp    x0, __tabla_l1              // direcciones físicas (adrp es relativo al PC)
    adrp    x1, __tabla_l2_g3
    mov     x2, x0                      // poner a cero las dos tablas (8 KB seguidos)
    mov     x3, #1024
5:  str     xzr, [x2], #8
    subs    x3, x3, #1
    b.ne    5b

    ldr     x4, =0x0040000000000705     // bloque normal: válido, AttrIndx 1, compartible interno, AF, UXN (EL0 nunca
                                        // ejecuta por aquí; auditoría, hallazgo 12)
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
    dsb     ish                         // que los demás núcleos vean las tablas al leerlas sin caché

    MMU     en_alto

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

// cambiar_ttbr1(x0 = tabla nueva, x1 = tabla vacía, x2 = __tabla_l1; las tres físicas): pasa la mitad alta de este
// núcleo a las tablas del mapa del kernel (mapa.rs, C34). Sigue la regla de romper antes de hacer: corre desde la
// proyección identidad, deja TTBR1 vacío, vacía la TLB y solo entonces pone la tabla nueva. Entre medias no toca la
// memoria, ni la pila. Al volver, TTBR0 queda como estaba y se vacía la TLB otra vez, para que no queden entradas
// globales de la proyección identidad.
    .globl  cambiar_ttbr1
cambiar_ttbr1:
    mrs     x9, daif
    msr     daifset, #0xf
    mrs     x10, ttbr0_el1
    msr     ttbr0_el1, x2
    isb
    tlbi    vmalle1
    dsb     nsh
    isb
    ldr     x4, =0xFFFFFF8000000000     // KERNEL_VA
    adr     x3, 1f
    sub     x3, x3, x4
    br      x3                          // a la misma instrucción por su dirección física
1:  msr     ttbr1_el1, x1
    isb
    tlbi    vmalle1
    dsb     nsh
    isb
    msr     ttbr1_el1, x0
    isb
    tlbi    vmalle1
    dsb     nsh
    isb
    adr     x3, 2f
    add     x3, x3, x4
    br      x3                          // de vuelta a la mitad alta, ya con el mapa nuevo
2:  msr     ttbr0_el1, x10
    isb
    tlbi    vmalle1
    dsb     nsh
    isb
    msr     daif, x9
    ret

// trasladar(x0 = tabla nueva, x1 = tabla vacía, x2 = __tabla_l1, x3 = destino, x4 = origen, x5 = tamaño; todas
// físicas): como cambiar_ttbr1, pero con TTBR1 vacío copia la imagen del kernel de origen a destino y la lleva a la
// caché de instrucciones antes de poner la tabla nueva, que ve la imagen en el destino (C38). Entre medias no toca la
// memoria salvo para la copia, así que la copia es una foto exacta: pila, variables y todo.
    .globl  trasladar
trasladar:
    mrs     x9, daif
    msr     daifset, #0xf
    mrs     x10, ttbr0_el1
    msr     ttbr0_el1, x2
    isb
    tlbi    vmalle1
    dsb     nsh
    isb
    ldr     x11, =0xFFFFFF8000000000    // KERNEL_VA
    adr     x12, 5f
    sub     x12, x12, x11
    br      x12                         // a la proyección identidad
5:  msr     ttbr1_el1, x1
    isb
    tlbi    vmalle1
    dsb     nsh
    isb
    mov     x6, x4                      // copiar, de 16 en 16 bytes
    mov     x7, x3
    add     x8, x4, x5
6:  ldp     x12, x13, [x6], #16
    stp     x12, x13, [x7], #16
    cmp     x6, x8
    b.lo    6b
    mov     x7, x3                      // la copia, hasta la RAM (punto de coherencia): los núcleos 1 a 3 la leen
    add     x8, x3, x5                  // con las cachés apagadas (auditoría de la fase 3, M5); y de ahí a la de
7:  dc      cvac, x7                    // instrucciones
    add     x7, x7, #64
    cmp     x7, x8
    b.lo    7b
    dsb     sy
    ic      iallu
    dsb     nsh
    isb
    msr     ttbr1_el1, x0
    isb
    tlbi    vmalle1
    dsb     nsh
    isb
    adr     x12, 8f
    add     x12, x12, x11
    br      x12                         // a la mitad alta: ya es la copia
8:  msr     ttbr0_el1, x10
    isb
    tlbi    vmalle1
    dsb     nsh
    isb
    msr     daif, x9
    ret

// --- Núcleos 1 a 3 ---
// Entran aquí desde la tabla de espera del firmware, en EL2, little-endian y sin MMU (C9).
    .globl _secundario
_secundario:
    mrs     x19, mpidr_el1
    and     x19, x19, #0xff             // número de núcleo
    A_EL1   sec_el1
sec_el1:
    MMU     sec_alto
sec_alto:
    // Al mapa del kernel antes de usar la pila (C38): con la imagen trasladada, la pila de este núcleo está en la
    // copia, y lo que se pusiera antes en la pila vieja se perdería al cambiar.
    ldr     x3, =MAPA_L1_SECUNDARIOS
    ldr     x0, [x3]
    cbz     x0, 1f
    ldr     x3, =MAPA_VACIA_SECUNDARIOS
    ldr     x1, [x3]
    mrs     x2, ttbr0_el1
    bl      cambiar_ttbr1
1:  ldr     x1, =__pilas_fin            // pila: una ranura de 32 KB por núcleo, hacia abajo desde el final
    lsl     x2, x19, #15
    sub     x1, x1, x2
    mov     sp, x1
    mov     x0, x19
    bl      kmain_secundario
7:  wfe
    b       7b
