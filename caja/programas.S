// DashOS, H4: los programas 68k de prueba de la caja: binarios planos (pruebas68k/, cada uno enlazado en su dirección)
// y ejecutables hunk de AmigaOS (saludo.exe, ventana.exe y hostil.exe, y los de Dash), que carga hunk.c.
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
        programa ventana, "pruebas68k/ventana.exe"
        programa hostil, "pruebas68k/hostil.exe"
// Programas de Dash (FreeMEMLang): los compila el Makefile en build/dash/ si está `dash`; si no, están vacíos.
        programa dash_hola, "build/dash/hola.exe"
        programa dash_texto, "build/dash/texto.exe"
        programa dash_texto_gcc, "build/dash/texto_gcc.exe"
        programa dash_ventana, "build/dash/ventana.exe"
