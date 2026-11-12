// DashOS, H4: los programas 68k de prueba de la caja: binarios planos (pruebas68k/, cada uno enlazado en su dirección)
// y un ejecutable hunk de AmigaOS (saludo.exe), que carga hunk.c.
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
        programa saludo, "pruebas68k/saludo.exe"
