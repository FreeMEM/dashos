// Punto de entrada sin libc: Linux deja la pila lista; llamamos a host_main(sp) (sp: argc, argv...).
        .text
        .globl  _start
_start:
        mov     x29, #0
        mov     x30, #0
        mov     x0, sp
        bl      host_main
        mov     x0, #3
        mov     x8, #94
        svc     #0
