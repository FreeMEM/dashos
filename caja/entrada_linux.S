// DashOS, la caja Amiga en Linux (docs/CAJA_LINUX_LOOP.md, L3): la entrada, como entrada.S y entrada_x86.S. Un PIE
// estático sin intérprete (enlace.ld): Linux lo carga muy por encima de los 4 GB del 68k y no aplica reubicaciones.
// Antes de nada, las RELATIVE de .rela.dyn (en base + r_offset va base + r_addend; la base es la dirección de _start,
// enlazada en 0); otro tipo no se espera: la caja acaba con 70. Linux deja en la pila argc, argv y el entorno: se
// guarda ese SP (caja_pila, que da otra pila propia en 12 GB) y caja_linux_main lo lee. Al volver, exit_group.

#if defined(__x86_64__)
        .intel_syntax noprefix
        .section .text._start, "ax"
        .globl  _start
_start:
        xor     ebp, ebp
        lea     r9, [rip + _start]              // r9 = base de carga
        lea     r10, [rip + __rela_inicio]
        lea     r11, [rip + __rela_fin]
2:      cmp     r10, r11
        jae     3f
        mov     rcx, [r10]                      // r_offset
        mov     edx, [r10 + 8]                  // el tipo (los 32 bits de abajo de r_info)
        mov     rax, [r10 + 16]                 // r_addend
        add     r10, 24
        cmp     edx, 8                          // R_X86_64_RELATIVE
        jne     4f
        add     rax, r9
        mov     [rcx + r9], rax
        jmp     2b
4:      mov     edi, 70
        mov     eax, 231                        // exit_group
        syscall
3:      mov     rdi, rsp
        and     rsp, -16
        call    caja_pila
        test    rax, rax
        jz      1f
        mov     rsp, rax
1:      and     rsp, -16
        call    caja_linux_main
        mov     edi, eax
        mov     eax, 231                        // exit_group
        syscall
        jmp     .
#else
        .section .text._start, "ax"
        .globl  _start
_start:
        mov     x29, #0
        mov     x30, #0
        adr     x9, _start                      // x9 = base de carga
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
        mov     x8, #94                         // exit_group
        svc     #0
3:      mov     x0, sp
        bl      caja_pila
        cbz     x0, 1f
        mov     sp, x0
1:      bl      caja_linux_main
        mov     x8, #94                         // exit_group
        svc     #0
        b       .
#endif
        .section .note.GNU-stack, "", %progbits
