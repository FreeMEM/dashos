// DashOS: el trozo del hipervisor que corre en EL2 (caja Linux, H9; docs/bitacoras/caja-linux.md).
//
// La Cortex-A72 es ARMv8.0, sin VHE: el kernel corre en EL1 y aquí solo queda lo que tiene que estar en EL2, el
// cambio de mundo entre el kernel (el anfitrión) y una máquina virtual (el invitado). Lo demás (la etapa 2, qué
// hacer con cada salida) se decide en EL1 (hiper.rs y maquinas.rs) y en el servicio `caja-linux`.
//
// El kernel llama aquí con `hvc #0` y el número de la orden en x0:
//   1 INSTALAR (x1 = VBAR_EL2, x2 = TTBR0_EL2, x3 = zona de este núcleo): pone la tabla de vectores de verdad, la
//     traducción de EL2 (identidad, solo la RAM) y enciende su MMU. La única orden que entiende la tabla del arranque.
//   2 CORRER (x1 = física del "mundo" de la vCPU): guarda el anfitrión en el mundo, carga el invitado y entra en él.
//     Vuelve con x0 = 0 cuando el invitado sale (HVC, una trampa, un fallo de la etapa 2 o una interrupción física).
//   3 OLVIDAR (x1 = VTTBR con el VMID): vacía de las TLB de todos los núcleos lo de ese VMID.
// x0 a x17 no se conservan; x18 a x30 y SP_EL1, sí.
//
// Mientras corre un invitado, TPIDR_EL2 apunta a su mundo y SP_EL2 también; en el anfitrión, TPIDR_EL2 vale 0 y
// SP_EL2 apunta a la zona del núcleo. Las dos tienen en ESCR un hueco donde la entrada guarda x0 y x1 antes de mirar.
//
// El mundo (una página; mismo orden que hiper.rs):
//   0x000 invitado: x0..x30, PC (ELR_EL2), PSTATE (SPSR_EL2) y sus 26 registros de EL1 (SIS); ESCR en 0x1E0
//   0x200 anfitrión: x18..x30, ELR_EL2, SPSR_EL2, SP_EL2, MDCR_EL2, CPTR_EL2, CNTHCTL_EL2, HCR_EL2, sus SIS y
//         VMPIDR_EL2 (lo que lee el kernel como MPIDR_EL1: su número de núcleo)
//   0x400 configuración del invitado (VTTBR, HCR, MDCR, CPTR, CNTHCTL, VTCR), la salida (ESR, FAR, tipo, HPFAR),
//         el MPIDR que ve el invitado y si hay que vaciar la TLB de este núcleo para su VMID antes de entrar

.equ HV_INSTALAR, 1
.equ HV_CORRER, 2
.equ HV_OLVIDAR, 3

.equ ESCR, 0x1e0
.equ I_PC, 0xf8
.equ I_SIS, 0x108
.equ ANF, 0x200
.equ A_X18, 0x00
.equ A_ELR, 0x68
.equ A_SP, 0x78
.equ A_MDCR, 0x80
.equ A_CNTHCTL, 0x90
.equ A_SIS, 0xa0
.equ A_VMPIDR, 0x170
.equ CFG, 0x400
.equ C_VTTBR, 0x00
.equ C_MDCR, 0x10
.equ C_CNTHCTL, 0x20
.equ S_ESR, 0x30
.equ S_TIPO, 0x40
.equ C_VMPIDR, 0x50
.equ C_LIMPIAR, 0x58

// Tipos de salida (S_TIPO): por qué volvió el invitado.
.equ SALIDA_SINCRONA, 1
.equ SALIDA_IRQ, 2
.equ SALIDA_FIQ, 3
.equ SALIDA_SERROR, 4

// Los 26 registros de EL1 (y del temporizador virtual) que tiene cada mundo, en pares, desde [\b, #\o]. Usa x2 y x3.
.macro GUARDAR_EL1 b, o
    mrs     x2, sp_el0
    mrs     x3, sp_el1
    stp     x2, x3, [\b, #(\o + 0)]
    mrs     x2, elr_el1
    mrs     x3, spsr_el1
    stp     x2, x3, [\b, #(\o + 16)]
    mrs     x2, sctlr_el1
    mrs     x3, ttbr0_el1
    stp     x2, x3, [\b, #(\o + 32)]
    mrs     x2, ttbr1_el1
    mrs     x3, tcr_el1
    stp     x2, x3, [\b, #(\o + 48)]
    mrs     x2, mair_el1
    mrs     x3, amair_el1
    stp     x2, x3, [\b, #(\o + 64)]
    mrs     x2, vbar_el1
    mrs     x3, contextidr_el1
    stp     x2, x3, [\b, #(\o + 80)]
    mrs     x2, tpidr_el0
    mrs     x3, tpidrro_el0
    stp     x2, x3, [\b, #(\o + 96)]
    mrs     x2, tpidr_el1
    mrs     x3, esr_el1
    stp     x2, x3, [\b, #(\o + 112)]
    mrs     x2, far_el1
    mrs     x3, afsr0_el1
    stp     x2, x3, [\b, #(\o + 128)]
    mrs     x2, afsr1_el1
    mrs     x3, par_el1
    stp     x2, x3, [\b, #(\o + 144)]
    mrs     x2, cpacr_el1
    mrs     x3, cntkctl_el1
    stp     x2, x3, [\b, #(\o + 160)]
    mrs     x2, csselr_el1
    mrs     x3, mdscr_el1
    stp     x2, x3, [\b, #(\o + 176)]
    mrs     x2, cntv_ctl_el0
    mrs     x3, cntv_cval_el0
    stp     x2, x3, [\b, #(\o + 192)]
.endm

.macro CARGAR_EL1 b, o
    ldp     x2, x3, [\b, #(\o + 0)]
    msr     sp_el0, x2
    msr     sp_el1, x3
    ldp     x2, x3, [\b, #(\o + 16)]
    msr     elr_el1, x2
    msr     spsr_el1, x3
    ldp     x2, x3, [\b, #(\o + 32)]
    msr     sctlr_el1, x2
    msr     ttbr0_el1, x3
    ldp     x2, x3, [\b, #(\o + 48)]
    msr     ttbr1_el1, x2
    msr     tcr_el1, x3
    ldp     x2, x3, [\b, #(\o + 64)]
    msr     mair_el1, x2
    msr     amair_el1, x3
    ldp     x2, x3, [\b, #(\o + 80)]
    msr     vbar_el1, x2
    msr     contextidr_el1, x3
    ldp     x2, x3, [\b, #(\o + 96)]
    msr     tpidr_el0, x2
    msr     tpidrro_el0, x3
    ldp     x2, x3, [\b, #(\o + 112)]
    msr     tpidr_el1, x2
    msr     esr_el1, x3
    ldp     x2, x3, [\b, #(\o + 128)]
    msr     far_el1, x2
    msr     afsr0_el1, x3
    ldp     x2, x3, [\b, #(\o + 144)]
    msr     afsr1_el1, x2
    msr     par_el1, x3
    ldp     x2, x3, [\b, #(\o + 160)]
    msr     cpacr_el1, x2
    msr     cntkctl_el1, x3
    ldp     x2, x3, [\b, #(\o + 176)]
    msr     csselr_el1, x2
    msr     mdscr_el1, x3
    ldp     x2, x3, [\b, #(\o + 192)]
    msr     cntv_ctl_el0, x2
    msr     cntv_cval_el0, x3
.endm

// Una entrada que no debería llegar nunca (una excepción en el propio EL2, o desde AArch32): el núcleo se para aquí,
// sin tocar nada. Con QEMU, `-d int` dice dónde.
.macro PARAR
    .balign 0x80
    b       el2_parado
.endm

// Una entrada desde EL1 o EL0 en AArch64. Con un invitado en marcha, es una salida; si no, una llamada del kernel
// (solo las síncronas: una interrupción no se manda a EL2 fuera de un invitado).
.macro DESDE_ABAJO tipo
    .balign 0x80
    stp     x0, x1, [sp, #ESCR]
    mrs     x0, tpidr_el2
    mov     x1, #\tipo
    cbnz    x0, salida
.if \tipo == SALIDA_SINCRONA
    b       anfitrion
.else
    b       el2_parado
.endif
.endm

    .section .text.hiper, "ax"

// La tabla del arranque (arranque.s, A_EL1): solo entiende INSTALAR, sin pila ni memoria (la MMU de EL2 está apagada
// y aún no hay zona del núcleo).
    .balign 0x800
    .globl  vectores_el2_arranque
vectores_el2_arranque:
    PARAR
    PARAR
    PARAR
    PARAR
    PARAR
    PARAR
    PARAR
    PARAR
    .balign 0x80
    cmp     x0, #HV_INSTALAR
    b.eq    instalar
    mov     x0, #-1
    eret
    PARAR
    PARAR
    PARAR
    PARAR
    PARAR
    PARAR
    PARAR

    .balign 0x800
    .globl  vectores_el2
vectores_el2:
    PARAR
    PARAR
    PARAR
    PARAR
    PARAR
    PARAR
    PARAR
    PARAR
    DESDE_ABAJO SALIDA_SINCRONA
    DESDE_ABAJO SALIDA_IRQ
    DESDE_ABAJO SALIDA_FIQ
    DESDE_ABAJO SALIDA_SERROR
    PARAR
    PARAR
    PARAR
    PARAR

el2_parado:
    wfe
    b       el2_parado

// INSTALAR: x1 = VBAR_EL2, x2 = TTBR0_EL2 (tabla de nivel 1, identidad), x3 = zona de este núcleo (SP_EL2).
// MAIR como la de EL1 (0 dispositivo, 1 normal con caché); TCR_EL2: RES1 (31 y 23), PS de 40 bits, granulo de 4 KB,
// compartible interno y con caché, T0SZ = 25 (39 bits, desde el nivel 1). SCTLR_EL2: sus RES1 de ARMv8.0, MMU y
// cachés, little-endian. Se vacían las TLB de EL2 y de EL1: así no queda nada de antes con un VMID que luego
// tenga un invitado.
instalar:
    msr     vbar_el2, x1
    movz    x0, #0xff00
    movk    x0, #0x44, lsl #16
    msr     mair_el2, x0
    movz    x0, #0x3519
    movk    x0, #0x8082, lsl #16
    msr     tcr_el2, x0
    msr     ttbr0_el2, x2
    mov     sp, x3
    msr     tpidr_el2, xzr
    msr     vttbr_el2, xzr
    isb
    tlbi    alle2
    tlbi    alle1
    dsb     nsh
    isb
    movz    x0, #0x1835
    movk    x0, #0x30c5, lsl #16
    msr     sctlr_el2, x0
    isb
    mov     x0, #0
    eret

// Una llamada del kernel (TPIDR_EL2 = 0). Solo HVC de 64 bits.
anfitrion:
    ldp     x0, x1, [sp, #ESCR]
    mrs     x2, esr_el2
    lsr     x2, x2, #26
    cmp     x2, #0x16
    b.ne    el2_parado
    cmp     x0, #HV_CORRER
    b.eq    correr
    cmp     x0, #HV_OLVIDAR
    b.eq    olvidar
    cmp     x0, #HV_INSTALAR
    b.eq    instalar
    mov     x0, #-1
    eret

// OLVIDAR: x1 = VTTBR con el VMID que se suelta. Las TLB de todos los núcleos (IS), etapas 1 y 2.
olvidar:
    msr     vttbr_el2, x1
    isb
    tlbi    vmalls12e1is
    dsb     ish
    msr     vttbr_el2, xzr
    isb
    mov     x0, #0
    eret

// CORRER: x1 = mundo (física; EL2 ve la RAM en identidad).
correr:
    add     x9, x1, #ANF
    add     x10, x1, #CFG
    stp     x18, x19, [x9, #(A_X18 + 0)]
    stp     x20, x21, [x9, #(A_X18 + 16)]
    stp     x22, x23, [x9, #(A_X18 + 32)]
    stp     x24, x25, [x9, #(A_X18 + 48)]
    stp     x26, x27, [x9, #(A_X18 + 64)]
    stp     x28, x29, [x9, #(A_X18 + 80)]
    str     x30, [x9, #(A_X18 + 96)]
    mrs     x2, elr_el2
    mrs     x3, spsr_el2
    stp     x2, x3, [x9, #A_ELR]
    mov     x2, sp
    str     x2, [x9, #A_SP]
    mrs     x2, mdcr_el2
    mrs     x3, cptr_el2
    stp     x2, x3, [x9, #A_MDCR]
    mrs     x2, cnthctl_el2
    mrs     x3, hcr_el2
    stp     x2, x3, [x9, #A_CNTHCTL]
    GUARDAR_EL1 x9, A_SIS
    mrs     x2, vmpidr_el2
    str     x2, [x9, #A_VMPIDR]

    // El modo del invitado: su etapa 2 y su VMID, lo que se atrapa (HCR, MDCR, CPTR) y sin el temporizador físico.
    ldp     x2, x3, [x10, #C_VTTBR]
    msr     vttbr_el2, x2
    msr     hcr_el2, x3
    ldp     x2, x3, [x10, #C_MDCR]
    msr     mdcr_el2, x2
    msr     cptr_el2, x3
    ldp     x2, x3, [x10, #C_CNTHCTL]
    msr     cnthctl_el2, x2
    msr     vtcr_el2, x3
    ldr     x2, [x10, #C_VMPIDR]
    msr     vmpidr_el2, x2
    // Otra vCPU de la misma máquina corrió antes en este núcleo: lo de su VMID, fuera de la TLB de este núcleo.
    ldr     x2, [x10, #C_LIMPIAR]
    cbz     x2, 1f
    isb
    tlbi    vmalls12e1
    dsb     nsh
1:
    CARGAR_EL1 x1, I_SIS
    ldp     x2, x3, [x1, #I_PC]
    msr     elr_el2, x2
    msr     spsr_el2, x3
    msr     tpidr_el2, x1
    mov     sp, x1
    ldp     x2, x3, [x1, #16]
    ldp     x4, x5, [x1, #32]
    ldp     x6, x7, [x1, #48]
    ldp     x8, x9, [x1, #64]
    ldp     x10, x11, [x1, #80]
    ldp     x12, x13, [x1, #96]
    ldp     x14, x15, [x1, #112]
    ldp     x16, x17, [x1, #128]
    ldp     x18, x19, [x1, #144]
    ldp     x20, x21, [x1, #160]
    ldp     x22, x23, [x1, #176]
    ldp     x24, x25, [x1, #192]
    ldp     x26, x27, [x1, #208]
    ldp     x28, x29, [x1, #224]
    ldr     x30, [x1, #240]
    ldp     x0, x1, [x1, #0]
    eret

// Sale el invitado: x0 = su mundo (= SP_EL2), x1 = tipo, y sus x0 y x1 en ESCR. Se guarda todo lo suyo y se vuelve
// al kernel justo después de su HVC de CORRER, con x0 = 0.
salida:
    stp     x2, x3, [x0, #16]
    stp     x4, x5, [x0, #32]
    stp     x6, x7, [x0, #48]
    stp     x8, x9, [x0, #64]
    stp     x10, x11, [x0, #80]
    stp     x12, x13, [x0, #96]
    stp     x14, x15, [x0, #112]
    stp     x16, x17, [x0, #128]
    stp     x18, x19, [x0, #144]
    stp     x20, x21, [x0, #160]
    stp     x22, x23, [x0, #176]
    stp     x24, x25, [x0, #192]
    stp     x26, x27, [x0, #208]
    stp     x28, x29, [x0, #224]
    str     x30, [x0, #240]
    ldp     x2, x3, [x0, #ESCR]
    stp     x2, x3, [x0, #0]
    mrs     x2, elr_el2
    mrs     x3, spsr_el2
    stp     x2, x3, [x0, #I_PC]
    add     x9, x0, #ANF
    add     x10, x0, #CFG
    mrs     x2, esr_el2
    mrs     x3, far_el2
    stp     x2, x3, [x10, #S_ESR]
    mrs     x2, hpfar_el2
    stp     x1, x2, [x10, #S_TIPO]
    GUARDAR_EL1 x0, I_SIS

    // Otra vez el anfitrión: sin VMID de invitado (el kernel usa el 0), sin trampas y con sus registros.
    msr     tpidr_el2, xzr
    msr     vttbr_el2, xzr
    ldp     x2, x3, [x9, #A_MDCR]
    msr     mdcr_el2, x2
    msr     cptr_el2, x3
    ldp     x2, x3, [x9, #A_CNTHCTL]
    msr     cnthctl_el2, x2
    msr     hcr_el2, x3
    ldr     x2, [x9, #A_VMPIDR]
    msr     vmpidr_el2, x2
    CARGAR_EL1 x9, A_SIS
    ldp     x2, x3, [x9, #A_ELR]
    msr     elr_el2, x2
    msr     spsr_el2, x3
    ldr     x2, [x9, #A_SP]
    mov     sp, x2
    ldp     x18, x19, [x9, #(A_X18 + 0)]
    ldp     x20, x21, [x9, #(A_X18 + 16)]
    ldp     x22, x23, [x9, #(A_X18 + 32)]
    ldp     x24, x25, [x9, #(A_X18 + 48)]
    ldp     x26, x27, [x9, #(A_X18 + 64)]
    ldp     x28, x29, [x9, #(A_X18 + 80)]
    ldr     x30, [x9, #(A_X18 + 96)]
    mov     x0, #0
    eret
