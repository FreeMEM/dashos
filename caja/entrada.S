// DashOS, H4: entrada de la caja Amiga. El kernel deja SP en el tope de la pila del ELF, que está por debajo de
// 2 GB, al alcance del 68k (sus direcciones son direcciones reales del proceso). Antes de nada, otra pila por encima
// de 4 GB (caja_pila), y con ella caja_main. Al volver, se acaba el proceso entero con su código.
        .section .text._start, "ax"
        .globl  _start
_start:
        mov     x29, #0
        mov     x30, #0
        bl      caja_pila
        cbz     x0, 1f
        mov     sp, x0
1:      bl      caja_main
        mov     x8, #44                 // proceso_salir
        svc     #0
        b       .
