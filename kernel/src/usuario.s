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

// C12: servidor. Espera un mensaje en la llave 1 (con una llave dentro), lo escribe y responde por esa llave.
// Página de datos: 0x000 longitud y texto de la respuesta; 0x100 búfer; 0x280 llaves recibidas.
    .globl servidor_inicio, servidor_fin
servidor_inicio:
    movz    x19, #0x50, lsl #16
    mov     x0, #1                      // canal_leer(1, búfer, 64, llaves, 4)
    add     x1, x19, #0x100
    mov     x2, #64
    add     x3, x19, #0x280
    mov     x4, #4
    mov     x8, #5
    svc     #0
    mov     x20, x0
    add     x0, x19, #0x100             // escribir lo recibido
    mov     x1, x20
    mov     x8, #1
    svc     #0
    ldr     w0, [x19, #0x280]           // canal_escribir(llave recibida, respuesta, longitud, -, 0)
    add     x1, x19, #1
    ldrb    w2, [x19]
    mov     x3, xzr
    mov     x4, xzr
    mov     x8, #4
    svc     #0
    mov     x0, #0                      // salir(0)
    mov     x8, #2
    svc     #0
3:  b       3b
servidor_fin:

// C12: cliente. Crea un canal, manda al servidor un texto con uno de los extremos, y espera la respuesta en el otro.
// Página de datos: 0x000 longitud y texto; 0x100 búfer; 0x200 llaves que viajan; 0x280 llaves recibidas.
    .globl cliente_inicio, cliente_fin
cliente_inicio:
    movz    x19, #0x50, lsl #16
    mov     x8, #3                      // canal_crear() -> (x0, x1)
    svc     #0
    mov     x20, x0                     // aquí llegará la respuesta
    str     w1, [x19, #0x200]           // el otro extremo viaja al servidor
    mov     x0, #1                      // canal_escribir(1, texto, longitud, llaves, 1)
    add     x1, x19, #1
    ldrb    w2, [x19]
    add     x3, x19, #0x200
    mov     x4, #1
    mov     x8, #4
    svc     #0
    mov     x0, x20                     // canal_leer(respuesta, búfer, 64, llaves, 4)
    add     x1, x19, #0x100
    mov     x2, #64
    add     x3, x19, #0x280
    mov     x4, #4
    mov     x8, #5
    svc     #0
    mov     x21, x0
    add     x0, x19, #0x100             // escribir la respuesta
    mov     x1, x21
    mov     x8, #1
    svc     #0
    mov     x0, #7                      // salir(7)
    mov     x8, #2
    svc     #0
4:  b       4b
cliente_fin:

// C13: productor. Crea un objeto de memoria, lo proyecta en 0x600000, copia su texto dentro, manda la llave por la
// llave 1, espera la respuesta y escribe lo que haya entonces en la memoria compartida.
    .globl productor_inicio, productor_fin
productor_inicio:
    movz    x19, #0x50, lsl #16
    mov     x0, #4096                   // memoria_crear(4096)
    mov     x8, #7
    svc     #0
    mov     x20, x0
    mov     x0, x20                     // memoria_proyectar(m, 0x600000, leer y escribir)
    movz    x1, #0x60, lsl #16
    mov     x2, #3
    mov     x8, #8
    svc     #0
    movz    x21, #0x60, lsl #16         // copiar longitud y texto
    ldrb    w2, [x19]
    add     x2, x2, #1
    mov     x3, #0
5:  ldrb    w4, [x19, x3]
    strb    w4, [x21, x3]
    add     x3, x3, #1
    cmp     x3, x2
    b.ne    5b
    str     w20, [x19, #0x200]          // canal_escribir(1, -, 0, [m], 1)
    mov     x0, #1
    add     x1, x19, #1
    mov     x2, #0
    add     x3, x19, #0x200
    mov     x4, #1
    mov     x8, #4
    svc     #0
    mov     x0, #1                      // canal_leer(1, búfer, 64, llaves, 4): esperar
    add     x1, x19, #0x100
    mov     x2, #64
    add     x3, x19, #0x280
    mov     x4, #4
    mov     x8, #5
    svc     #0
    ldrb    w1, [x21]                   // escribir lo que hay ahora en la memoria compartida
    add     x0, x21, #1
    mov     x8, #1
    svc     #0
    mov     x0, #3
    mov     x8, #2
    svc     #0
6:  b       6b
productor_fin:

// C13: consumidor. Recibe la llave de memoria, la proyecta en otra dirección (0x700000), escribe lo que lee, pone
// su propio texto y avisa al productor.
    .globl consumidor_inicio, consumidor_fin
consumidor_inicio:
    movz    x19, #0x50, lsl #16
    mov     x0, #1                      // canal_leer(1, búfer, 64, llaves, 4)
    add     x1, x19, #0x100
    mov     x2, #64
    add     x3, x19, #0x280
    mov     x4, #4
    mov     x8, #5
    svc     #0
    ldr     w0, [x19, #0x280]           // memoria_proyectar(llave recibida, 0x700000, leer y escribir)
    movz    x1, #0x70, lsl #16
    mov     x2, #3
    mov     x8, #8
    svc     #0
    movz    x21, #0x70, lsl #16
    ldrb    w1, [x21]                   // escribir lo que dejó el productor
    add     x0, x21, #1
    mov     x8, #1
    svc     #0
    ldrb    w2, [x19]                   // poner su propio texto
    add     x2, x2, #1
    mov     x3, #0
7:  ldrb    w4, [x19, x3]
    strb    w4, [x21, x3]
    add     x3, x3, #1
    cmp     x3, x2
    b.ne    7b
    mov     x0, #1                      // canal_escribir(1, -, 0, -, 0): avisar
    add     x1, x19, #1
    mov     x2, #0
    mov     x3, xzr
    mov     x4, xzr
    mov     x8, #4
    svc     #0
    mov     x0, #4
    mov     x8, #2
    svc     #0
8:  b       8b
consumidor_fin:
