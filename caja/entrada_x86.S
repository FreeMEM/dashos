// DashOS, X7: entrada de la caja Amiga en x86-64, como entrada.S en ARM. Un PIE estático (enlace.ld) que el kernel
// carga en una base al azar por encima de 128 GB, con la pila entre 256 y 384 GB: fuera de los 4 GB del 68k. El kernel
// deja en rdi la frecuencia del TSC (se guarda en caja_frecuencia_tsc) y no aplica reubicaciones: antes de nada, las
// R_X86_64_RELATIVE de .rela.dyn (en la dirección base + r_offset va base + r_addend; la base es la dirección de
// _start, enlazada en 0). Otro tipo de reubicación no se espera: la caja acaba con 70. Después, otra pila propia
// (caja_pila, en 12 GB, que guarda la del kernel), y con ella caja_main. Al volver, se acaba el proceso entero.
        .intel_syntax noprefix
        .section .text._start, "ax"
        .globl  _start
_start:
        mov     r12, rdi                        // la frecuencia del TSC
        lea     r9, [rip + _start]              // r9 = base de carga (_start está enlazada en 0)
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
        mov     eax, 44                         // proceso_salir
        syscall
3:      mov     [rip + caja_frecuencia_tsc], r12
        mov     rdi, rsp
        and     rsp, -16
        call    caja_pila
        test    rax, rax
        jz      1f
        mov     rsp, rax
1:      and     rsp, -16
        call    caja_main
        mov     edi, eax
        mov     eax, 44                         // proceso_salir
        syscall
        jmp     .
