# H4: los programas 68k de prueba de la caja Amiga, como binarios planos, cada uno en su dirección.
# Hace falta el ensamblador del Amiga de Dash (/opt/amiga). Los .bin van en git: la caja no depende de esa cadena.
A := /opt/amiga/bin/m68k-amigaos-
HUNK2BIN := ../../jit-host/pruebas68k/hunk2bin.py

all: hola.bin lineaa.bin bucle.bin mala.bin

hola.bin: BASE := 0x110000
lineaa.bin: BASE := 0x120000
bucle.bin: BASE := 0x130000
mala.bin: BASE := 0x140000

%.bin: %.s
	$(A)as -m68040 --register-prefix-optional $< -o $*.o
	$(A)ld $*.o -o $*.exe
	python3 $(HUNK2BIN) $*.exe $@ $(BASE)
	rm -f $*.o $*.exe

.PHONY: all
