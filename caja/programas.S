// DashOS, H4: los programas 68k de prueba de la caja, como binarios planos (pruebas68k/, cada uno enlazado en su
// dirección).
        .section .rodata
        .macro  programa nombre, fichero
        .balign 4
        .globl  prog_\nombre, prog_\nombre\()_fin
prog_\nombre:
        .incbin "\fichero"
prog_\nombre\()_fin:
        .endm
        programa hola, "pruebas68k/hola.bin"
        programa lineaa, "pruebas68k/lineaa.bin"
        programa bucle, "pruebas68k/bucle.bin"
        programa mala, "pruebas68k/mala.bin"
