# H4: los programas 68k de prueba de la caja Amiga, como binarios planos, cada uno en su dirección.
# Hace falta el ensamblador del Amiga de Dash (/opt/amiga). Los .bin van en git: la caja no depende de esa cadena.
A := /opt/amiga/bin/m68k-amigaos-
HUNK2BIN := ../../jit-host/pruebas68k/hunk2bin.py

all: hola.bin lineaa.bin bucle.bin mala.bin saludo.exe ventana.exe hostil.exe

hola.bin: BASE := 0x110000
lineaa.bin: BASE := 0x120000
bucle.bin: BASE := 0x130000
mala.bin: BASE := 0x140000

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
# entrada. Sin -g: no hace falta para el cargador, que ya lo prueba saludo.exe.
CFLAGS_VENTANA := -m68020 -O2 -ffreestanding -fno-builtin -nostdlib -nostartfiles -fomit-frame-pointer -Wall
%.exe: %.c saludo_entrada.s
	$(A)as -m68020 --register-prefix-optional saludo_entrada.s -o $*_entrada.o
	$(A)gcc $(CFLAGS_VENTANA) $*_entrada.o $< -o $@ $(LIBGCC)
	rm -f $*_entrada.o

.PHONY: all
