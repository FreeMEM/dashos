// DashOS: el trampolín de los demás núcleos en x86-64 (S30, X3). Sintaxis de Intel.
//
// No se ejecuta aquí: nucleos.rs lo copia a una página libre por debajo de 1 MB y llena sus datos. La SIPI hace
// empezar al núcleo en modo real en esa página (CS = página × 256, IP = 0). De ahí (SDM, volumen 3, 9.9: el paso a
// modo protegido y a modo largo):
//   1. modo real: ebx = dirección física de la página; la GDT de aquí (código de 32 y de 64 bits, datos); modo
//      protegido y salto lejano a 32 bits;
//   2. 32 bits: PAE, CR3 = las tablas del arranque (1:1 en los primeros 4 GB y la imagen arriba), EFER (modo largo y
//      NX), paginación con la caché encendida (tras INIT, CR0 la deja apagada: CD y NW); salto a 64 bits;
//   3. 64 bits: salta a _secundario_x86 (arranque.s), en la imagen, con el número del núcleo, su pila y el CR3 del
//      kernel.
// Solo usa direcciones relativas al principio del trampolín (ebx): vale en cualquier página. Los datos van en
// desplazamientos fijos (.org), que el ensamblador comprueba: el código no puede pisarlos.

.equ T_GDT, 0x100
.equ T_GDTR, 0x120
.equ T_LEJOS, 0x128
.equ T_DATOS, 0x130
.equ T_CR3, T_DATOS
.equ T_EFER, T_DATOS + 4
.equ T_CR3_KERNEL, T_DATOS + 8
.equ T_PILA, T_DATOS + 16
.equ T_NUCLEO, T_DATOS + 24
.equ T_ENTRADA, T_DATOS + 32
.equ T_MODO32, 0x40
.equ T_MODO64, 0xa0

.section .rodata.trampolin, "a"
.balign 16
.globl trampolin_x86, trampolin_x86_datos, trampolin_x86_fin
.code16
trampolin_x86:
    cli
    cld
    mov ax, cs
    mov ds, ax
    xor ebx, ebx
    mov bx, ax
    shl ebx, 4
    lea eax, [ebx + T_GDT]
    mov dword ptr [T_GDTR + 2], eax
    lgdt [T_GDTR]
    lea eax, [ebx + T_MODO32]
    mov dword ptr [T_LEJOS], eax
    mov eax, cr0
    or eax, 1
    mov cr0, eax
    // jmp far dword ptr [t_lejos]: 66 FF /5 con desplazamiento de 16 bits.
    .byte 0x66, 0xff, 0x2e
    .word T_LEJOS

.code32
.org T_MODO32
t_modo32:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    lea esp, [ebx + 0x1000]             // una pila al final de la página, para el salto con retf
    mov eax, cr4
    or eax, 0x20                        // PAE
    mov cr4, eax
    mov eax, [ebx + T_CR3]
    mov cr3, eax
    mov ecx, 0xc0000080                 // IA32_EFER
    mov eax, [ebx + T_EFER]
    xor edx, edx
    wrmsr
    mov eax, cr0
    and eax, 0x9fffffff                 // fuera CD y NW
    or eax, 0x80000021                  // PG, NE y PE
    mov cr0, eax
    push 0x18
    lea eax, [ebx + T_MODO64]
    push eax
    retf

.code64
.org T_MODO64
t_modo64:
    mov ebx, ebx                        // la parte alta de rbx, a cero
    xor eax, eax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov rdi, [rbx + T_NUCLEO]
    mov rsi, [rbx + T_PILA]
    mov rdx, [rbx + T_CR3_KERNEL]
    mov rax, [rbx + T_ENTRADA]
    jmp rax

.org T_GDT
t_gdt:
    .quad 0
    .quad 0x00cf9a000000ffff            // 0x08: código de 32 bits
    .quad 0x00cf92000000ffff            // 0x10: datos
    .quad 0x00af9a000000ffff            // 0x18: código de 64 bits
.org T_GDTR
t_gdtr:
    .word 31
    .long 0
.org T_LEJOS
t_lejos:
    .long 0
    .word 0x08

// Lo que llena nucleos.rs (`DatosTrampolin`): mismo orden.
.org T_DATOS
trampolin_x86_datos:
t_cr3:          .long 0
t_efer:         .long 0
t_cr3_kernel:   .quad 0
t_pila:         .quad 0
t_nucleo:       .quad 0
t_entrada:      .quad 0
trampolin_x86_fin:
