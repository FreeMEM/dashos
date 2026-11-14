// DashOS: entradas de las excepciones de la CPU en x86-64 (S21, X2). Sintaxis de Intel.
//
// Un vector por excepción (0 a 31). Los que no traen código de error meten un 0, para que el marco sea siempre igual;
// luego el número del vector. La entrada común guarda los 15 registros generales y llama a `excepcion_x86` con el
// marco (excepciones.rs, `Marco`: mismo orden). Al volver, los repone y vuelve con iretq; el tratamiento puede haber
// cambiado rip, rsp o rax del marco (las sondas de las pruebas).
//
// La pila queda alineada a 16 en la llamada: la CPU la alinea antes de meter SS, RSP, RFLAGS, CS y RIP (40 bytes), y
// con el código, el vector y los 15 registros son 176.
//
// De 32 a 255 (X3), las interrupciones: sin código de error, con el mismo marco; `excepcion_x86` las pasa al APIC.
//
// Falta para X4 (anillo 3): swapgs y clac al entrar desde un proceso.

.macro VECTOR n, codigo
vector_x86_\n:
.if \codigo == 0
    push 0
.endif
    push \n
    jmp excepcion_comun_x86
.endm

.section .text.vectores, "ax"
// Los que traen código de error (SDM, vol. 3, 6.15): 8, 10 a 14, 17, 21, 29 y 30.
VECTOR 0, 0
VECTOR 1, 0
VECTOR 2, 0
VECTOR 3, 0
VECTOR 4, 0
VECTOR 5, 0
VECTOR 6, 0
VECTOR 7, 0
VECTOR 8, 1
VECTOR 9, 0
VECTOR 10, 1
VECTOR 11, 1
VECTOR 12, 1
VECTOR 13, 1
VECTOR 14, 1
VECTOR 15, 0
VECTOR 16, 0
VECTOR 17, 1
VECTOR 18, 0
VECTOR 19, 0
VECTOR 20, 0
VECTOR 21, 1
VECTOR 22, 0
VECTOR 23, 0
VECTOR 24, 0
VECTOR 25, 0
VECTOR 26, 0
VECTOR 27, 0
VECTOR 28, 0
VECTOR 29, 1
VECTOR 30, 1
VECTOR 31, 0

// Las interrupciones, de 0x20 a 0xff: dos cifras hexadecimales.
.irp a, 2,3,4,5,6,7,8,9,a,b,c,d,e,f
.irp b, 0,1,2,3,4,5,6,7,8,9,a,b,c,d,e,f
vector_x86_0x\a\()\b:
    push 0
    push 0x\a\()\b
    jmp excepcion_comun_x86
.endr
.endr

excepcion_comun_x86:
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15
    cld
    mov rdi, rsp
    call excepcion_x86
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax
    add rsp, 16
    iretq

// Las direcciones de las 256 entradas, para la IDT.
.section .rodata.vectores, "a"
.balign 8
.globl vectores_x86
vectores_x86:
.irp n, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31
    .quad vector_x86_\n
.endr
.irp a, 2,3,4,5,6,7,8,9,a,b,c,d,e,f
.irp b, 0,1,2,3,4,5,6,7,8,9,a,b,c,d,e,f
    .quad vector_x86_0x\a\()\b
.endr
.endr

// Sondas para las pruebas: un acceso que puede fallar. Si falla, la excepción ve que rip está en la sonda y vuelve a
// su `_fallo`, que devuelve 1. Sin fallo, devuelven 0.
.section .text.sondas, "ax"
.globl sonda_escribir_x86, sonda_leer_x86, sonda_ejecutar_x86
.globl sonda_inicio_x86, sonda_fin_x86, sonda_fallo_x86, sonda_ejecutar_fallo_x86
sonda_escribir_x86:
sonda_inicio_x86:
    mov byte ptr [rdi], sil
    xor eax, eax
    ret
sonda_leer_x86:
    movzx eax, byte ptr [rdi]
    mov [rsi], al
    xor eax, eax
    ret
sonda_fin_x86:
sonda_fallo_x86:
    mov eax, 1
    ret

// Llama a rdi. Si la CPU no deja ejecutar allí, el fallo llega con rip = rdi y la dirección de vuelta en la pila: la
// excepción la quita y vuelve a sonda_ejecutar_fallo_x86.
sonda_ejecutar_x86:
    call rdi
    xor eax, eax
    ret
sonda_ejecutar_fallo_x86:
    mov eax, 1
    ret
