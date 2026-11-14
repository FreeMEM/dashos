// DashOS: los primeros programas de usuario en x86-64, para las pruebas de X4 (como usuario.s en ARM, C11 a C18).
// Sintaxis de Intel. Llamadas: número en rax, argumentos en rdi, rsi, rdx, r10 y r8; resultado en rax (y rdx). El
// kernel copia este código en una página del proceso (0x400000); los datos están en 0x500000 y la pila en 0x7ff000.

    .section .rodata.usuario, "a"
    .globl usuario_inicio, usuario_fin
// Lee de la página de datos un byte con la longitud y el texto detrás, lo escribe y sale con 42.
usuario_inicio:
    mov ebx, 0x500000
    movzx esi, byte ptr [rbx]           // longitud
    lea rdi, [rbx + 1]                  // texto
    mov eax, 1                          // escribir(texto, longitud)
    syscall
    mov edi, 42
    mov eax, 2                          // salir(42)
    syscall
1:  jmp 1b
usuario_fin:

// Intenta leer la memoria del kernel (el mapa lineal, 1 MB físico): tiene que morir él y seguir el sistema.
    .globl usuario_malo_inicio, usuario_malo_fin
usuario_malo_inicio:
    movabs rbx, 0xffffff8000100000
    mov rax, qword ptr [rbx]
    mov edi, 1
    mov eax, 2
    syscall
2:  jmp 2b
usuario_malo_fin:

// Intenta leer donde otro proceso tiene su memoria (0x600000): en su espacio no hay nada, y muere.
    .globl espia_inicio, espia_fin
espia_inicio:
    mov eax, dword ptr [0x600000]
    mov edi, 1
    mov eax, 2
    syscall
3:  jmp 3b
espia_fin:

// C12: servidor. Espera un mensaje en la llave 1 (con una llave dentro), lo escribe y responde por esa llave.
// Página de datos: 0x000 longitud y texto de la respuesta; 0x100 búfer; 0x280 llaves recibidas.
    .globl servidor_inicio, servidor_fin
servidor_inicio:
    mov ebx, 0x500000
    mov edi, 1                          // canal_leer(1, búfer, 64, llaves, 4)
    lea rsi, [rbx + 0x100]
    mov edx, 64
    lea r10, [rbx + 0x280]
    mov r8d, 4
    mov eax, 5
    syscall
    mov r12, rax
    lea rdi, [rbx + 0x100]              // escribir lo recibido
    mov rsi, r12
    mov eax, 1
    syscall
    mov edi, dword ptr [rbx + 0x280]    // canal_escribir(llave recibida, respuesta, longitud, -, 0)
    lea rsi, [rbx + 1]
    movzx edx, byte ptr [rbx]
    xor r10d, r10d
    xor r8d, r8d
    mov eax, 4
    syscall
    xor edi, edi                        // salir(0)
    mov eax, 2
    syscall
4:  jmp 4b
servidor_fin:

// C12: cliente. Crea un canal, manda al servidor un texto con uno de los extremos, y espera la respuesta en el otro.
// Página de datos: 0x000 longitud y texto; 0x100 búfer; 0x200 llaves que viajan; 0x280 llaves recibidas.
    .globl cliente_inicio, cliente_fin
cliente_inicio:
    mov ebx, 0x500000
    mov eax, 3                          // canal_crear() -> (rax, rdx)
    syscall
    mov r12, rax                        // aquí llegará la respuesta
    mov dword ptr [rbx + 0x200], edx    // el otro extremo viaja al servidor
    mov edi, 1                          // canal_escribir(1, texto, longitud, llaves, 1)
    lea rsi, [rbx + 1]
    movzx edx, byte ptr [rbx]
    lea r10, [rbx + 0x200]
    mov r8d, 1
    mov eax, 4
    syscall
    mov rdi, r12                        // canal_leer(respuesta, búfer, 64, llaves, 4)
    lea rsi, [rbx + 0x100]
    mov edx, 64
    lea r10, [rbx + 0x280]
    mov r8d, 4
    mov eax, 5
    syscall
    mov r13, rax
    lea rdi, [rbx + 0x100]              // escribir la respuesta
    mov rsi, r13
    mov eax, 1
    syscall
    mov edi, 7                          // salir(7)
    mov eax, 2
    syscall
5:  jmp 5b
cliente_fin:

// C13: productor. Crea un objeto de memoria, lo proyecta en 0x600000, copia su texto dentro, manda la llave por la
// llave 1, espera la respuesta y escribe lo que haya entonces en la memoria compartida.
    .globl productor_inicio, productor_fin
productor_inicio:
    mov ebx, 0x500000
    mov edi, 4096                       // memoria_crear(4096)
    mov eax, 7
    syscall
    mov r12, rax
    mov rdi, r12                        // memoria_proyectar(m, 0x600000, leer y escribir)
    mov esi, 0x600000
    mov edx, 3
    mov eax, 8
    syscall
    mov r13d, 0x600000                  // copiar longitud y texto
    movzx ecx, byte ptr [rbx]
    inc ecx
    xor esi, esi
6:  mov al, byte ptr [rbx + rsi]
    mov byte ptr [r13 + rsi], al
    inc rsi
    cmp rsi, rcx
    jne 6b
    mov dword ptr [rbx + 0x200], r12d   // canal_escribir(1, -, 0, [m], 1)
    mov edi, 1
    lea rsi, [rbx + 1]
    xor edx, edx
    lea r10, [rbx + 0x200]
    mov r8d, 1
    mov eax, 4
    syscall
    mov edi, 1                          // canal_leer(1, búfer, 64, llaves, 4): esperar
    lea rsi, [rbx + 0x100]
    mov edx, 64
    lea r10, [rbx + 0x280]
    mov r8d, 4
    mov eax, 5
    syscall
    movzx esi, byte ptr [r13]           // escribir lo que hay ahora en la memoria compartida
    lea rdi, [r13 + 1]
    mov eax, 1
    syscall
    mov edi, 3
    mov eax, 2
    syscall
7:  jmp 7b
productor_fin:

// C13: consumidor. Recibe la llave de memoria, la proyecta en otra dirección (0x700000), escribe lo que lee, pone
// su propio texto y avisa al productor.
    .globl consumidor_inicio, consumidor_fin
consumidor_inicio:
    mov ebx, 0x500000
    mov edi, 1                          // canal_leer(1, búfer, 64, llaves, 4)
    lea rsi, [rbx + 0x100]
    mov edx, 64
    lea r10, [rbx + 0x280]
    mov r8d, 4
    mov eax, 5
    syscall
    mov edi, dword ptr [rbx + 0x280]    // memoria_proyectar(llave recibida, 0x700000, leer y escribir)
    mov esi, 0x700000
    mov edx, 3
    mov eax, 8
    syscall
    mov r13d, 0x700000
    movzx esi, byte ptr [r13]           // escribir lo que dejó el productor
    lea rdi, [r13 + 1]
    mov eax, 1
    syscall
    movzx ecx, byte ptr [rbx]           // poner su propio texto
    inc ecx
    xor esi, esi
8:  mov al, byte ptr [rbx + rsi]
    mov byte ptr [r13 + rsi], al
    inc rsi
    cmp rsi, rcx
    jne 8b
    mov edi, 1                          // canal_escribir(1, -, 0, -, 0): avisar
    lea rsi, [rbx + 1]
    xor edx, edx
    xor r10d, r10d
    xor r8d, r8d
    mov eax, 4
    syscall
    mov edi, 4
    mov eax, 2
    syscall
9:  jmp 9b
consumidor_fin:

// C18: un programa que solo sale.
    .globl usuario_nada_inicio, usuario_nada_fin
usuario_nada_inicio:
    xor edi, edi
    mov eax, 2
    syscall
10: jmp 10b
usuario_nada_fin:

// X4: los registros que el kernel tiene que conservar en una llamada. Pone valores conocidos en todos menos rax, rcx,
// rdx, r8 y r11 (los del resultado y los que pisa `syscall`), hace una llamada (escribir 0 bytes) y los compara, con
// rdi, rsi y r10, que eran argumentos.
// Sale con 44 si siguen igual, con 1 si no.
    .globl conserva_inicio, conserva_fin
conserva_inicio:
    mov rbx, 0x1111111111111111
    mov rbp, 0x2222222222222222
    mov rsi, 0x3333333333333333
    mov r9, 0x5555555555555555
    mov r12, 0x6666666666666666
    mov r13, 0x7777777777777777
    mov r14, 0x8888888888888888
    mov r15, 0x9999999999999999
    mov edi, 0x500000                   // escribir(0x500000, 0): rdi y r10 se comprueban aparte
    xor r10d, r10d
    mov rsi, 0
    mov eax, 1
    syscall
    cmp rdi, 0x500000
    mov edi, 1
    jne 11f
    cmp rsi, 0
    jne 11f
    mov rax, 0x1111111111111111
    cmp rbx, rax
    jne 11f
    mov rax, 0x2222222222222222
    cmp rbp, rax
    jne 11f
    mov rax, 0x5555555555555555
    cmp r9, rax
    jne 11f
    mov rax, 0x6666666666666666
    cmp r12, rax
    jne 11f
    mov rax, 0x7777777777777777
    cmp r13, rax
    jne 11f
    mov rax, 0x8888888888888888
    cmp r14, rax
    jne 11f
    mov rax, 0x9999999999999999
    cmp r15, rax
    jne 11f
    test r10, r10
    jne 11f
    mov edi, 44
11: mov eax, 2
    syscall
12: jmp 12b
conserva_fin:

// X4: eco. Lee un mensaje por la llave 1 y lo devuelve por ella, 20 veces; sale con 0.
    .globl eco_inicio, eco_fin
eco_inicio:
    mov ebx, 0x500000
    mov r12d, 20
13: mov edi, 1                          // canal_leer(1, búfer, 64, llaves, 4)
    lea rsi, [rbx + 0x100]
    mov edx, 64
    lea r10, [rbx + 0x280]
    mov r8d, 4
    mov eax, 5
    syscall
    mov rdx, rax                        // canal_escribir(1, búfer, bytes, -, 0)
    mov edi, 1
    lea rsi, [rbx + 0x100]
    xor r10d, r10d
    xor r8d, r8d
    mov eax, 4
    syscall
    dec r12d
    jnz 13b
    xor edi, edi
    mov eax, 2
    syscall
14: jmp 14b
eco_fin:

// X6: los puertos de E/S. Pide los de su llave 1 (COM1, 0x3F8 a 0x3FF); sin ella, sale con 1. Escribe 0x5A en el
// registro de prueba de la UART (0x3FF) y lo relee: si no es lo mismo, sale con 2. Después lee 0x3F7, que no es suyo:
// tiene que morir ahí (#GP); si no, sale con 3.
    .globl puertos_inicio, puertos_fin
puertos_inicio:
    mov edi, 1
    mov eax, 29                         // puertos_permitir(1)
    syscall
    mov edi, 1
    test rax, rax
    jnz 1f
    mov dx, 0x3ff
    mov al, 0x5a
    out dx, al
    xor eax, eax
    in al, dx
    mov edi, 2
    cmp al, 0x5a
    jne 1f
    mov dx, 0x3f7
    in al, dx
    mov edi, 3
1:  mov eax, 2                          // salir
    syscall
2:  jmp 2b
puertos_fin:

// X6: usa el registro de prueba de COM1 sin haber pedido nada: tiene que morir (#GP). Si no, sale con 4.
    .globl puertos_sin_inicio, puertos_sin_fin
puertos_sin_inicio:
    mov dx, 0x3ff
    in al, dx
    mov edi, 4
    mov eax, 2
    syscall
3:  jmp 3b
puertos_sin_fin:

// X6: pide 16 veces un objeto de memoria de 4 MB y lo suelta. Sale con 0, o con 1 si alguna vez no se lo dan.
    .globl crear_mucho_inicio, crear_mucho_fin
crear_mucho_inicio:
    mov ebx, 16
1:  mov edi, 0x400000
    mov eax, 7                          // memoria_crear(4 MB)
    syscall
    test rax, rax
    js 2f
    mov rdi, rax
    mov eax, 6                          // cerrar
    syscall
    dec ebx
    jnz 1b
    xor edi, edi
    jmp 3f
2:  mov edi, 1
3:  mov eax, 2
    syscall
4:  jmp 4b
crear_mucho_fin:
