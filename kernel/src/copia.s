// DashOS: copias entre el kernel y la memoria de un proceso (C31).
//
// El lado del proceso se toca con LDTRB/STTRB: accesos "sin privilegio", que la MMU comprueba con los permisos de EL0.
// Así el kernel no puede leer por un proceso lo que el proceso no puede leer, ni escribir en su código o en sus
// páginas de solo lectura. Si un acceso falla (página vacía o sin permiso), excepciones.rs ve que el fallo es de
// este tramo de código, pone ELR en copia_fallo y la función devuelve 1 en vez de tumbar el kernel.
//
// desde_usuario(destino_kernel, origen_usuario, n) -> 0 bien, 1 fallo
// hacia_usuario(destino_usuario, origen_kernel, n) -> 0 bien, 1 fallo

    .section .text.copia, "ax"
    .globl  copia_inicio, copia_fin, desde_usuario, hacia_usuario
copia_inicio:
desde_usuario:
    cbz     x2, 2f
1:  ldtrb   w3, [x1]
    strb    w3, [x0], #1
    add     x1, x1, #1
    subs    x2, x2, #1
    b.ne    1b
2:  mov     x0, #0
    ret

hacia_usuario:
    cbz     x2, 2f
1:  ldrb    w3, [x1], #1
    sttrb   w3, [x0]
    add     x0, x0, #1
    subs    x2, x2, #1
    b.ne    1b
2:  mov     x0, #0
    ret
copia_fin:

    .globl  copia_fallo
copia_fallo:
    mov     x0, #1
    ret
