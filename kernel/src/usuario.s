// DashOS: el primer programa de usuario, para la prueba de C11.
//
// Lee de la página de datos (0x500000) un byte con la longitud y el texto detrás, lo escribe con la llamada 1 y
// termina con la llamada 2 y el código 42. El kernel copia este código en una página del proceso.

    .section .rodata.usuario, "a"
    .balign 4
    .globl usuario_inicio, usuario_fin
usuario_inicio:
    movz    x9, #0x50, lsl #16          // 0x500000: página de datos
    ldrb    w1, [x9]                    // longitud
    add     x0, x9, #1                  // texto
    mov     x8, #1                      // escribir(texto, longitud)
    svc     #0
    mov     x0, #42
    mov     x8, #2                      // salir(42)
    svc     #0
1:  b       1b
usuario_fin:

// Un programa que intenta escribir en la memoria del kernel: tiene que morir él y seguir el sistema.
    .globl usuario_malo_inicio, usuario_malo_fin
usuario_malo_inicio:
    movz    x9, #0xffff, lsl #48        // 0xFFFFFF8000080000: el kernel
    movk    x9, #0xff80, lsl #32
    movk    x9, #0x0008, lsl #16
    str     xzr, [x9]
    mov     x0, #1
    mov     x8, #2
    svc     #0
2:  b       2b
usuario_malo_fin:
