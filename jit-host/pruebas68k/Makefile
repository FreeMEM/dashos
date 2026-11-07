# U7: programas 68k de prueba para el banco del JIT (jit-host), como binarios planos que se cargan en $140000.
# Hace falta el gcc del Amiga de Dash (/opt/amiga). Los .bin van en git: el banco no depende de esa cadena.
A := /opt/amiga/bin/m68k-amigaos-
CFLAGS := -m68040 -m68881 -ffreestanding -fno-builtin -nostdlib -nostartfiles -fomit-frame-pointer -fno-toplevel-reorder
LIBGCC = -L$(dir $(shell $(A)gcc -m68040 -m68881 -print-libgcc-file-name)) -lgcc
FUENTES_C := $(wildcard *.c)
FUENTES_S := $(wildcard *.s)
BINS := $(FUENTES_C:.c=.bin) $(FUENTES_C:.c=-o0.bin) $(FUENTES_S:.s=.bin)

all: $(BINS)

# Se enlaza como ejecutable del Amiga y hunk2bin.py lo coloca en $140000; empieza por su primera función.
%.bin: %.c
	$(A)gcc $(CFLAGS) -O2 $< -o $*.exe $(LIBGCC)
	python3 hunk2bin.py $*.exe $@ 0x140000
%-o0.bin: %.c
	$(A)gcc $(CFLAGS) -O0 $< -o $*-o0.exe $(LIBGCC)
	python3 hunk2bin.py $*-o0.exe $@ 0x140000
%.bin: %.s
	$(A)as -m68040 -m68881 --register-prefix-optional $< -o $*.o
	$(A)ld $*.o -o $*.exe
	python3 hunk2bin.py $*.exe $@ 0x140000

.PHONY: all
