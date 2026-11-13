// DashOS, H4: entrada de la caja Amiga, un PIE estático (enlace.ld). El kernel la carga en una base al azar por encima
// de 128 GB, con la pila entre 256 y 384 GB: fuera de los 4 GB del 68k. El kernel no aplica reubicaciones: antes de
// nada, las R_AARCH64_RELATIVE de .rela.dyn (en la dirección base + r_offset va base + r_addend; la base es la dirección
// de _start, enlazada en 0). Otro tipo de reubicación no se espera: la caja acaba con 70. Después, otra pila
// propia (caja_pila, en 12 GB, que guarda el SP del kernel), y con ella caja_main. Al volver, se acaba el proceso entero con su código.
        .section .text._start, "ax"
        .globl  _start
_start:
        mov     x29, #0
        mov     x30, #0
        adr     x9, _start                      // x9 = base de carga (_start está enlazada en 0)
        adrp    x10, __rela_inicio
        add     x10, x10, :lo12:__rela_inicio
        adrp    x11, __rela_fin
        add     x11, x11, :lo12:__rela_fin
        mov     x14, #1027                      // R_AARCH64_RELATIVE
2:      cmp     x10, x11
        b.hs    3f
        ldp     x12, x13, [x10], #16            // r_offset, r_info
        ldr     x15, [x10], #8                  // r_addend
        cmp     w13, w14
        b.ne    4f
        add     x15, x15, x9
        str     x15, [x12, x9]
        b       2b
4:      mov     x0, #70
        mov     x8, #44                         // proceso_salir
        svc     #0
3:      mov     x0, sp
        bl      caja_pila
        cbz     x0, 1f
        mov     sp, x0
1:      bl      caja_main
        mov     x8, #44                         // proceso_salir
        svc     #0
        b       .
