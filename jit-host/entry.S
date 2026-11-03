// Punto de entrada sin libc: Linux deja la pila lista; llamamos a host_main.
        .text
        .globl  _start
_start:
        mov     x29, #0
        mov     x30, #0
        bl      host_main
        mov     x0, #3
        mov     x8, #94
        svc     #0
