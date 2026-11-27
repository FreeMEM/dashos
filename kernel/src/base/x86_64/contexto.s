// DashOS: cambio de contexto entre hilos del kernel en x86-64 (S30, X3). Sintaxis de Intel. Como contexto.s de ARM.
//
// cambiar_contexto(viejo: *mut Contexto (rdi), nuevo: *const Contexto (rsi))
//   Mete en la pila los registros que la convención de llamada obliga a conservar (rbp, rbx, r12 a r15), guarda rsp
//   en "viejo", toma el rsp de "nuevo" y saca los suyos. La vuelta (ret) es al hilo nuevo, donde se quedó; o a
//   hilo_entrada si es nuevo (planificador.rs, `contexto_nuevo`, le deja ese marco en la pila).
//
// Las interrupciones van cerradas (RFLAGS.IF): cada hilo las repone al salir del planificador. El kernel no usa la
// FPU (softfloat): no hay más que guardar.

.section .text, "ax"
.globl cambiar_contexto
cambiar_contexto:
    push rbp
    push rbx
    push r12
    push r13
    push r14
    push r15
    mov [rdi], rsp
    mov rsp, [rsi]
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp
    ret

// Primera vez que corre un hilo: r12 = función, r13 = argumento; la pila, alineada a 16.
.globl hilo_entrada
hilo_entrada:
    call hilo_estreno                   // suelta lo que dejó el planificador y abre las interrupciones
    mov rdi, r13
    call r12
    call hilo_fin                       // no vuelve
    ud2
