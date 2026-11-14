// DashOS: entrada del kernel en x86-64 (S21, X1 y X2). Sintaxis de Intel (la de `global_asm!` en x86).
//
// El cargador UEFI (uefi/src/x86_64.rs) salta aquí en modo largo, a la dirección física de la entrada, con las
// interrupciones cerradas, las tablas de páginas del firmware (memoria física 1:1) y en rdi la dirección física de la
// InfoArranque. El kernel está enlazado en los 2 GB de arriba (enlace_x86.ld), y mientras corre aquí abajo solo usa
// direcciones relativas a rip, que dan la física.
//
// Se pone a cero el .bss y se hacen las tablas de arranque, como en ARM (arranque.s): los 4 primeros GB de memoria
// física con páginas de 2 MB, vistos tres veces con el mismo PDPT y los mismos PD:
// - 1:1 (PML4[0]), para seguir aquí abajo hasta el salto;
// - en el mapa lineal, desde 0xFFFF_FF80_0000_0000 (PML4[511], PDPT[0..4]), como KERNEL_VA en ARM;
// - la imagen, en 0xFFFF_FFFF_8000_0000 (PML4[511], PDPT[510], el primer GB).
// Después se carga CR3, se salta arriba, se toma la pila y se entra en Rust. mapa.rs hace luego las de verdad.

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

    // Los cuatro PD seguidos: 2048 páginas de 2 MB (presente, escritura, PS).
    lea rdi, [rip + __arranque_pd]
    mov eax, 0x83
    mov ecx, 2048
1:
    mov [rdi], rax
    add rax, 0x200000
    add rdi, 8
    dec ecx
    jnz 1b

    // PDPT: [0..4] los cuatro PD; [510] el primero otra vez (la imagen).
    lea rdi, [rip + __arranque_pdpt]
    lea rax, [rip + __arranque_pd]
    or rax, 3
    mov [rdi + 510 * 8], rax
    mov [rdi], rax
    add rax, 0x1000
    mov [rdi + 8], rax
    add rax, 0x1000
    mov [rdi + 16], rax
    add rax, 0x1000
    mov [rdi + 24], rax

    // PML4: [0] y [511], el mismo PDPT.
    lea rdi, [rip + __arranque_pml4]
    lea rax, [rip + __arranque_pdpt]
    or rax, 3
    mov [rdi], rax
    mov [rdi + 511 * 8], rax

    lea rax, [rip + __arranque_pml4]
    mov cr3, rax
    movabs rax, offset .Lalto
    jmp rax
.Lalto:
    movabs rsp, offset __pila_fin
    xor ebp, ebp
    mov rdi, r12
    call kmain_x86
3:
    cli
    hlt
    jmp 3b
