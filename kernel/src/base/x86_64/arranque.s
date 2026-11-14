// DashOS: entrada del kernel en x86-64 (S21, X1). Sintaxis de Intel (la de `global_asm!` en x86).
//
// El cargador UEFI (uefi/src/x86_64.rs) salta aquí en modo largo, con las interrupciones cerradas, las tablas de
// páginas del firmware (memoria física 1:1) y en rdi la dirección de la InfoArranque. Se pone a cero el .bss, se
// toma la pila del kernel y se entra en Rust.

.section .text.arranque, "ax"
.globl _start
_start:
    cli
    cld
    mov r12, rdi
    lea rdi, [rip + __bss_inicio]
    lea rcx, [rip + __bss_fin]
    sub rcx, rdi
    xor eax, eax
    rep stosb
    lea rsp, [rip + __pila_fin]
    xor ebp, ebp
    mov rdi, r12
    call kmain_x86
1:
    cli
    hlt
    jmp 1b
