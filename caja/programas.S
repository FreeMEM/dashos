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
        programa sistema, "pruebas68k/sistema.bin"
        programa hola, "pruebas68k/hola.bin"
        programa lineaa, "pruebas68k/lineaa.bin"
        programa bucle, "pruebas68k/bucle.bin"
        programa mala, "pruebas68k/mala.bin"
        programa saludo, "pruebas68k/saludo.exe"
        programa ventana, "pruebas68k/ventana.exe"
        programa hostil, "pruebas68k/hostil.exe"
        programa anidado, "pruebas68k/anidado.exe"
        programa gadtools, "pruebas68k/gadtools.exe"
        programa boopsi, "pruebas68k/boopsi.exe"
        programa ramlib, "pruebas68k/ramlib.exe"
        programa capa, "pruebas68k/capa.exe"
        programa glotona, "pruebas68k/glotona.exe"
        programa memh, "pruebas68k/memh.exe"
        programa procesos, "pruebas68k/procesos.exe"
        programa ils, "pruebas68k/ils.exe"
        programa dtprueba, "pruebas68k/dtprueba.exe"
        programa cgx, "pruebas68k/cgx.exe"
// K3: los chips del Amiga (los CIA y el haz).
        programa chips, "pruebas68k/chips.exe"
// K7: lecturas de VPOSR seguidas, para medir lo que cuesta un acceso a los chips (en $138000).
        programa vposr, "pruebas68k/vposr.bin"
// S28: los virus de laboratorio (pruebas68k/malware/).
        programa mal_arranque, "pruebas68k/malware/arranque.exe"
        programa mal_enlace, "pruebas68k/malware/enlace.exe"
        programa mal_parche, "pruebas68k/malware/parche.exe"
        programa mal_residente, "pruebas68k/malware/residente.exe"
        programa mal_guion, "pruebas68k/malware/guion.exe"
        programa mal_troyano, "pruebas68k/malware/troyano.exe"
        programa mal_memoria, "pruebas68k/malware/memoria.exe"
// S28: un programa honrado que actualiza otro (la guardia pregunta al usuario).
        programa actualiza, "pruebas68k/actualiza.exe"
// Programas de Dash (FreeMEMLang): los compila el Makefile en build/dash/ si está `dash`; si no, están vacíos.
        programa dash_hola, "build/dash/hola.exe"
        programa dash_texto, "build/dash/texto.exe"
        programa dash_texto_gcc, "build/dash/texto_gcc.exe"
        programa dash_ventana, "build/dash/ventana.exe"
        programa dash_interp, "build/dash/interp.exe"
        programa dash_listas, "build/dash/listas.exe"
        programa dash_ctrl_c, "build/dash/ctrl_c.exe"
        programa dash_ctrl_c_gcc, "build/dash/ctrl_c_gcc.exe"
        programa dash_ctrl_c_for, "build/dash/ctrl_c_for.exe"
        programa dash_trap_ilegal, "build/dash/trap_ilegal.exe"
        programa dash_trap_divcero, "build/dash/trap_divcero.exe"
        programa dash_test_gadtools, "build/dash/test_gadtools.exe"
        programa dash_test_reaction, "build/dash/test_reaction.exe"
        programa dash_test_doc_reaction, "build/dash/test_doc_reaction.exe"
