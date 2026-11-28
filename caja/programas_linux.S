// DashOS, la caja Amiga en Linux (docs/CAJA_LINUX_LOOP.md, L3): de los programas 68k de programas.S, solo el código
// del sistema de la caja (pruebas68k/sistema.bin: el despacho de las excepciones, las trampas, la vuelta anidada).
// Las pruebas del arranque no van en la caja de Linux.
        .section .rodata
        .balign 4
        .globl  prog_sistema, prog_sistema_fin
prog_sistema:
        .incbin "pruebas68k/sistema.bin"
prog_sistema_fin:
        .section .note.GNU-stack, "", %progbits
