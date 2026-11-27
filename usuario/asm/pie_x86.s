# C29 en x86-64 (X4): el PIE más pequeño posible, como pie.s. Escribe un saludo y sale con 29 si su código y su pila
# están por encima de 4 GB (como pone el cargador a los PIE); si no, con 1. Sin reubicaciones: lo que usa lo encuentra
# relativo a rip.
    .intel_syntax noprefix
    .text
    .globl  _start
_start:
    lea     rdi, [rip + saludo]
    mov     esi, offset fin - saludo
    mov     eax, 1                      # escribir
    syscall
    lea     r9, [rip + _start]
    mov     r10, rsp
    mov     edi, 1
    shr     r9, 32
    shr     r10, 32
    test    r9, r9
    jz      1f
    test    r10, r10
    jz      1f
    mov     edi, 29
1:  mov     eax, 2                      # salir
    syscall
    jmp     .

    .section .rodata
saludo:
    .ascii  "  [pie] hola desde un PIE por encima de 4 GB\n"
fin:
