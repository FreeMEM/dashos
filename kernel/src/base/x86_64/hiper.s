// DashOS: el cambio de mundo del hipervisor de x86-64 (X10, hiper.rs). Sintaxis de Intel.
//
// Escrito con los manuales públicos: AMD64 APM, vol. 2, cap. 15 (VMRUN, VMLOAD, VMSAVE, CLGI, STGI) e Intel SDM,
// vol. 3C (VMLAUNCH, VMRESUME y el estado del anfitrión, caps. 26 a 28).
//
// Los registros generales del invitado van en una tabla de 17 palabras (hiper.rs, `Vcpu::regs`): rax, rcx, rdx, rbx,
// rsp, rbp, rsi, rdi, r8 a r15 (el número de cada registro en la codificación de x86) y, en la 16, CR2 (solo VT-x: AMD
// la guarda en el VMCB). rsp no se toca aquí: va en el VMCB o en el VMCS. En AMD, rax tampoco (VMRUN la carga del
// VMCB).

// svm_correr_x86(vmcb: rdi, regs: rsi, anfitrion: rdx). Las tres direcciones del VMCB y de la zona del anfitrión son
// físicas. Entra con las interrupciones cerradas y vuelve igual.
//
// CLGI antes de nada: con GIF a 0 no entra ni una NMI mientras la CPU tiene la TR, la GS o las MSR de `syscall` del
// invitado (VMLOAD). STI justo antes de VMRUN: con V_INTR_MASKING, las interrupciones físicas las enmascara el IF del
// anfitrión, y con él a 1 una interrupción saca al invitado (la intercepción INTR). Tras la salida GIF sigue a 0: CLI
// antes de STGI, y la interrupción queda pendiente hasta que el kernel abra las suyas.
.section .text.hiper, "ax"
.global svm_correr_x86
svm_correr_x86:
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15
    push rsi                    // [rsp + 16] regs
    push rdi                    // [rsp + 8] VMCB
    push rdx                    // [rsp] zona del anfitrión
    clgi
    mov rax, rdx
    vmsave rax                  // FS, GS, TR, LDTR, KernelGsBase, STAR, LSTAR, CSTAR, SFMASK y SYSENTER del anfitrión
    mov rax, rdi
    vmload rax                  // los del invitado
    mov rcx, [rsi + 8]
    mov rdx, [rsi + 16]
    mov rbx, [rsi + 24]
    mov rbp, [rsi + 40]
    mov rdi, [rsi + 56]
    mov r8, [rsi + 64]
    mov r9, [rsi + 72]
    mov r10, [rsi + 80]
    mov r11, [rsi + 88]
    mov r12, [rsi + 96]
    mov r13, [rsi + 104]
    mov r14, [rsi + 112]
    mov r15, [rsi + 120]
    mov rsi, [rsi + 48]
    mov rax, [rsp + 8]
    sti
    vmrun rax
    // De vuelta: rax y rsp del anfitrión, GIF a 0.
    push rsi                    // el rsi del invitado; regs pasa a [rsp + 24]
    mov rsi, [rsp + 24]
    mov [rsi + 8], rcx
    mov [rsi + 16], rdx
    mov [rsi + 24], rbx
    mov [rsi + 40], rbp
    mov [rsi + 56], rdi
    mov [rsi + 64], r8
    mov [rsi + 72], r9
    mov [rsi + 80], r10
    mov [rsi + 88], r11
    mov [rsi + 96], r12
    mov [rsi + 104], r13
    mov [rsi + 112], r14
    mov [rsi + 120], r15
    pop rax
    mov [rsi + 48], rax
    mov rax, [rsp + 8]
    vmsave rax
    mov rax, [rsp]
    vmload rax
    cli
    stgi
    add rsp, 24
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx
    ret

// vmx_correr_x86(regs: rdi, lanzar: rsi) -> rax: 0 salió del invitado, 1 VMfailInvalid, 2 VMfailValid (el error en el
// VMCS). Con el VMCS de la vCPU ya cargado (VMPTRLD) y su estado escrito. VMLAUNCH si `lanzar` (el VMCS está limpio,
// tras VMCLEAR), VMRESUME si no. HOST_RSP y HOST_RIP se escriben aquí: la salida vuelve a `vmx_vuelta_x86` con la
// pila tal como estaba antes de entrar (con `regs` encima).
.global vmx_correr_x86
vmx_correr_x86:
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15
    push rdi                    // [rsp] regs
    mov rax, 0x6c14             // HOST_RSP
    vmwrite rax, rsp
    lea rbx, [rip + vmx_vuelta_x86]
    mov rax, 0x6c16             // HOST_RIP
    vmwrite rax, rbx
    mov rax, [rdi + 128]
    mov cr2, rax                // CR2 del invitado (VT-x no la cambia de mundo)
    test rsi, rsi               // las banderas no las toca ningún MOV de aquí abajo
    mov rax, [rdi]
    mov rcx, [rdi + 8]
    mov rdx, [rdi + 16]
    mov rbx, [rdi + 24]
    mov rbp, [rdi + 40]
    mov rsi, [rdi + 48]
    mov r8, [rdi + 64]
    mov r9, [rdi + 72]
    mov r10, [rdi + 80]
    mov r11, [rdi + 88]
    mov r12, [rdi + 96]
    mov r13, [rdi + 104]
    mov r14, [rdi + 112]
    mov r15, [rdi + 120]
    mov rdi, [rdi + 56]
    jnz 2f
    vmresume
    jmp 3f
2:  vmlaunch
3:  // No entró: CF a 1 es VMfailInvalid, ZF a 1 es VMfailValid.
    mov eax, 1
    jc 4f
    mov eax, 2
4:  pop rdi
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx
    ret

vmx_vuelta_x86:
    // Salida del invitado: rsp = HOST_RSP, con regs en [rsp]. RFLAGS a 0x2: interrupciones cerradas.
    push rdi
    mov rdi, [rsp + 8]
    mov [rdi], rax
    mov [rdi + 8], rcx
    mov [rdi + 16], rdx
    mov [rdi + 24], rbx
    mov [rdi + 40], rbp
    mov [rdi + 48], rsi
    mov [rdi + 64], r8
    mov [rdi + 72], r9
    mov [rdi + 80], r10
    mov [rdi + 88], r11
    mov [rdi + 96], r12
    mov [rdi + 104], r13
    mov [rdi + 112], r14
    mov [rdi + 120], r15
    pop rax
    mov [rdi + 56], rax
    mov rax, cr2
    mov [rdi + 128], rax
    xor eax, eax
    pop rdi
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx
    ret
