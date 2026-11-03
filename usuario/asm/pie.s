// C29: el PIE más pequeño posible, para probar el cargador con ET_DYN. Escribe un saludo y sale con 29 si su código y
// su pila están por encima de 4 GB (como pone el cargador a los PIE); si no, con 1. Sin reubicaciones: todo lo que
// usa lo encuentra relativo al contador de programa.
    .text
    .globl  _start
_start:
    adr     x0, saludo
    mov     x1, #(fin - saludo)
    mov     x8, #1                      // escribir
    svc     #0
    adr     x9, _start
    mov     x10, sp
    mov     x0, #1
    lsr     x9, x9, #32
    lsr     x10, x10, #32
    cbz     x9, 1f
    cbz     x10, 1f
    mov     x0, #29
1:  mov     x8, #2                      // salir
    svc     #0
    b       .

    .section .rodata
saludo:
    .ascii  "  [pie] hola desde un PIE por encima de 4 GB\n"
fin:
