# H4: los programas 68k de prueba de la caja Amiga, como binarios planos, cada uno en su dirección.
# Hace falta el ensamblador del Amiga de Dash (/opt/amiga). Los .bin van en git: la caja no depende de esa cadena.
A := /opt/amiga/bin/m68k-amigaos-
HUNK2BIN := ../../jit-host/pruebas68k/hunk2bin.py

all: sistema.bin hola.bin lineaa.bin bucle.bin mala.bin vposr.bin ipl.bin saludo.exe ventana.exe hostil.exe anidado.exe gadtools.exe actualiza.exe boopsi.exe ramlib.exe capa.exe glotona.exe memh.exe procesos.exe ils.exe dtprueba.exe cgx.exe chips.exe blitter.exe copper.exe paula.exe denise.exe sprites.exe cia.exe prueba.library falsa.library miente.library dosasl.exe patron.exe formato.exe supervisor.exe icono.exe perdido.exe dos2.exe

sistema.bin: BASE := 0x101600
hola.bin: BASE := 0x110000
lineaa.bin: BASE := 0x120000
bucle.bin: BASE := 0x130000
mala.bin: BASE := 0x140000
vposr.bin: BASE := 0x138000
ipl.bin: BASE := 0x138000

%.bin: %.s
	$(A)as -m68040 --register-prefix-optional $< -o $*.o
	$(A)ld $*.o -o $*.exe
	python3 $(HUNK2BIN) $*.exe $@ $(BASE)
	rm -f $*.o $*.exe

# Un ejecutable hunk de verdad (no un binario plano): lo carga la caja con su cargador (../hunk.c). Con -g, para que
# lleve también HUNK_DEBUG y HUNK_SYMBOL. Va en git, como los .bin.
CFLAGS_68K := -m68020 -O2 -g -ffreestanding -fno-builtin -nostdlib -nostartfiles -fomit-frame-pointer -Wall
LIBGCC = -L$(dir $(shell $(A)gcc -m68020 -print-libgcc-file-name)) -lgcc
saludo.exe: saludo_entrada.s saludo.c
	$(A)as -m68020 --register-prefix-optional saludo_entrada.s -o saludo_entrada.o
	$(A)gcc $(CFLAGS_68K) saludo_entrada.o saludo.c -o $@ $(LIBGCC)
	rm -f saludo_entrada.o

# H5: una ventana Amiga (ventana.exe) y la prueba hostil de intuition, graphics y dos (hostil.exe), con la misma
# entrada. S28: actualiza.exe, un programa honrado que actualiza otro (la guardia pregunta). Sin -g: no hace falta para el cargador, que ya lo prueba saludo.exe.
CFLAGS_VENTANA := -m68020 -O2 -ffreestanding -fno-builtin -nostdlib -nostartfiles -fomit-frame-pointer -Wall
%.exe: %.c saludo_entrada.s
	$(A)as -m68020 --register-prefix-optional saludo_entrada.s -o $*_entrada.o
	$(A)gcc $(CFLAGS_VENTANA) $*_entrada.o $< -o $@ $(LIBGCC)
	rm -f $*_entrada.o

.PHONY: all

# ramlib: prueba.library (una biblioteca de disco hecha a mano, con su RomTag y RTF_AUTOINIT) y sus variantes
# hostiles (falsa: un RomTag que no se apunta a sí mismo; miente: dice la versión 1), y el programa que las abre.
prueba.library: prueba_lib.c biblioteca_entrada.s
	$(A)as -m68020 --register-prefix-optional biblioteca_entrada.s -o prueba_lib_entrada.o
	$(A)gcc $(CFLAGS_VENTANA) prueba_lib_entrada.o prueba_lib.c -o $@ $(LIBGCC)
	rm -f prueba_lib_entrada.o
falsa.library: prueba_lib.c biblioteca_entrada.s
	$(A)as -m68020 --register-prefix-optional biblioteca_entrada.s -o falsa_entrada.o
	$(A)gcc $(CFLAGS_VENTANA) -DFALSA falsa_entrada.o prueba_lib.c -o $@ $(LIBGCC)
	rm -f falsa_entrada.o
miente.library: prueba_lib.c biblioteca_entrada.s
	$(A)as -m68020 --register-prefix-optional biblioteca_entrada.s -o miente_entrada.o
	$(A)gcc $(CFLAGS_VENTANA) -DVERSION=1 miente_entrada.o prueba_lib.c -o $@ $(LIBGCC)
	rm -f miente_entrada.o

# B1: blitter.exe lee sus casos de blitter_casos.h (también los lee la prueba de Rust de ../chips).
blitter.exe: blitter_casos.h

# B4: denise.exe lee sus modos de denise_modos.h (también los lee la prueba de Rust de ../chips).
denise.exe: denise_modos.h

# B6: sprites.exe lee sus escenas de sprites_escenas.h (también las lee la prueba de Rust de ../chips).
sprites.exe: sprites_escenas.h
