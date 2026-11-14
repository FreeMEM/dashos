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
// X4, el anillo 3. Si la excepción viene de un proceso (CS del marco con RPL 3), `swapgs` al entrar y al salir: dentro
// del kernel GS apunta a la zona del núcleo (`nucleos.rs`) y la base de GS del proceso espera en IA32_KERNEL_GS_BASE.
// Y `clac` siempre (con SMAP): un proceso puede dejar RFLAGS.AC encendido, y la puerta de interrupción no lo apaga.
// NMI, fallo doble y comprobación de máquina tienen pila propia (IST) y pueden llegar con la GS del proceso (entre
// `swapgs` y `sysret`): su tratamiento no mira GS.

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
    test qword ptr [rsp + 24], 3        // CS del marco: ¿desde el anillo 3?
    jz 1f
    swapgs
1:  push rax
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
    cmp byte ptr [rip + SMAP_X86], 0
    je 2f
    clac
2:  cld
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
    test qword ptr [rsp + 24], 3
    jz 3f
    swapgs
3:  add rsp, 16
    iretq

// La entrada de `syscall` (X4; IA32_LSTAR). La CPU deja la vuelta en rcx y RFLAGS en r11, quita de RFLAGS lo de
// IA32_FMASK (IF entre ello: hasta `sysret` no entra ninguna interrupción) y no cambia de pila. Aquí: GS del kernel,
// la pila del kernel del hilo (gs:[16], `nucleos::poner_pila_kernel`) y un marco igual que el de una interrupción
// desde el anillo 3, con 0x100 de vector. `llamada_x86` lee los argumentos y escribe los resultados en el marco.
//
// La vuelta, por `sysret`: rcx y r11 del marco (la vuelta y RFLAGS, que `llamada_x86` ha comprobado: RIP canónica,
// sin IOPL), la pila del proceso y su GS. Entre `pop rsp` y `sysretq` la pila ya es la del proceso: solo puede llegar
// una NMI o una comprobación de máquina, que tienen pila propia.
.section .text, "ax"
.globl entrada_llamada_x86
entrada_llamada_x86:
    swapgs
    mov gs:[8], rsp
    mov rsp, gs:[16]
    push 0x23                           // SS: datos de usuario
    push qword ptr gs:[8]               // RSP del proceso
    push r11                            // RFLAGS
    push 0x2b                           // CS: código de usuario
    push rcx                            // RIP de vuelta
    push 0
    push 0x100
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
    cmp byte ptr [rip + SMAP_X86], 0
    je 1f
    clac
1:  cld
    mov rdi, rsp
    call llamada_x86
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
    pop rcx                             // RIP
    add rsp, 8
    pop r11                             // RFLAGS
    pop rsp                             // la pila del proceso
    swapgs
    sysretq

// entrar_anillo3_x86(rip: rdi, rsp: rsi, arg: rdx): baja por primera vez al anillo 3, con las interrupciones abiertas
// allí (RFLAGS = 0x202) y `arg` en rdi (la frecuencia del contador, `planificador::bajar_a_el0`). Los demás registros
// a cero: nada del kernel. La pila del kernel del hilo se queda como esté; sus interrupciones llegarán a su tope (RSP0).
.globl entrar_anillo3_x86
entrar_anillo3_x86:
    cli
    push 0x23
    push rsi
    push 0x202
    push 0x2b
    push rdi
    mov rdi, rdx
    xor eax, eax
    xor ebx, ebx
    xor ecx, ecx
    xor edx, edx
    xor esi, esi
    xor ebp, ebp
    xor r8d, r8d
    xor r9d, r9d
    xor r10d, r10d
    xor r11d, r11d
    xor r12d, r12d
    xor r13d, r13d
    xor r14d, r14d
    xor r15d, r15d
    swapgs
    iretq

// Copias entre el kernel y la memoria del proceso actual (X4, como copia.s en ARM, C31), con su espacio puesto
// (`usuario.rs`). copiar_usuario_x86(destino, origen, n) -> 0 bien, 1 fallo. Con SMAP, `stac` deja tocar las páginas
// de usuario solo durante la copia. La CPU las comprueba como para el kernel con CR0.WP: no se escribe en una página de
// solo lectura del proceso (su código), y la mitad alta no la pide nunca `usuario.rs`. Un fallo en `rep movsb` vuelve
// por copia_fallo_x86 (excepciones.rs).
.section .text.copia, "ax"
.globl copiar_usuario_x86, copia_inicio_x86, copia_fin_x86, copia_fallo_x86
copiar_usuario_x86:
    mov rcx, rdx
    cmp byte ptr [rip + SMAP_X86], 0
    je 1f
    stac
1:
copia_inicio_x86:
    rep movsb
copia_fin_x86:
    cmp byte ptr [rip + SMAP_X86], 0
    je 2f
    clac
2:  xor eax, eax
    ret
copia_fallo_x86:
    cmp byte ptr [rip + SMAP_X86], 0
    je 3f
    clac
3:  mov eax, 1
    ret

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
