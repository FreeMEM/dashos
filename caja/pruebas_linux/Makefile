# La caja Amiga en Linux (docs/CAJA_LINUX_LOOP.md): los programas 68k de sus pruebas (prueba.sh), ejecutables hunk
# de AmigaOS. Hace falta la cadena del Amiga de Dash (/opt/amiga). Los .exe van en git: las pruebas no dependen de ella.
A := /opt/amiga/bin/m68k-amigaos-
CFLAGS_68K := -m68020 -O2 -ffreestanding -fno-builtin -nostdlib -nostartfiles -fomit-frame-pointer -Wall
LIBGCC = -L$(dir $(shell $(A)gcc -m68020 -print-libgcc-file-name)) -lgcc

all: cajones.exe

%.exe: entrada.s %.c
	$(A)as -m68020 --register-prefix-optional entrada.s -o entrada.o
	$(A)gcc $(CFLAGS_68K) entrada.o $*.c -o $@ $(LIBGCC)
	rm -f entrada.o
