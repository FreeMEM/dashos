# U7: lo que cuesta el giro de bytes. Programas 68k de banco.c (uno por núcleo) y el plugin de QEMU que cuenta
# instrucciones ARM. Los .bin van en git (como los de pruebas68k); para rehacerlos hace falta el gcc del Amiga de Dash.
#   make            los .bin y build/cuenta.so
#   python3 medir.py
A := /opt/amiga/bin/m68k-amigaos-
CFLAGS := -m68040 -m68881 -O2 -ffreestanding -fno-builtin -nostdlib -nostartfiles -fomit-frame-pointer -fno-toplevel-reorder
LIBGCC = -L$(dir $(shell $(A)gcc -m68040 -m68881 -print-libgcc-file-name)) -lgcc
NUCLEOS := registros copiar ordenar crc lista fpu

all: $(NUCLEOS:%=b-%.bin) build/cuenta.so

b-%.bin: banco.c
	mkdir -p build
	$(A)gcc $(CFLAGS) -DK_$(shell echo $* | tr a-z A-Z) $< -o build/b-$*.exe $(LIBGCC)
	python3 ../pruebas68k/hunk2bin.py build/b-$*.exe $@ 0x140000

build/cuenta.so: cuenta.c
	mkdir -p build
	gcc -shared -fPIC -O2 -Wall $(shell pkg-config --cflags glib-2.0) $< -o $@

.PHONY: all
