# Las pruebas PPC (F1, docs/PPC_LOOP.md): binarios planos en 0x00140000 y el oráculo para qemu-ppc.
TC  ?= $(HOME)/.local/opt/toolchains/powerpc-440fp--glibc--stable-2026.08-1/bin/powerpc-linux-
CC  := $(TC)gcc
OC  := $(TC)objcopy
# PPC genérico (sin las instrucciones del 440), sin datos pequeños (r2, r13): las dos cosas, como en WarpOS
FLAGS := -mcpu=powerpc -ffreestanding -nostdlib -fno-pic -fno-pie -no-pie -static -G 0 -msdata=none -fno-builtin -fno-stack-protector \
	-fno-asynchronous-unwind-tables -Wall
ENTEROS := -msoft-float

all: p1.bin p1-o0.bin t1a.bin t1b.bin t2.bin oraculo

p1.bin: p1.c prueba.ld
	$(CC) $(FLAGS) $(ENTEROS) -O2 -T prueba.ld -o p1.elf $< -lgcc && $(OC) -O binary p1.elf $@
p1-o0.bin: p1.c prueba.ld
	$(CC) $(FLAGS) $(ENTEROS) -O0 -T prueba.ld -o p1-o0.elf $< -lgcc && $(OC) -O binary p1-o0.elf $@

t1%.s: gen_t1.py
	python3 gen_t1.py $* > $@
t1%.bin: t1%.s prueba.ld
	$(CC) $(FLAGS) -T prueba.ld -o t1$*.elf $< && $(OC) -O binary t1$*.elf $@
t2.s: gen_t2.py
	python3 gen_t2.py > $@
t2.bin: t2.s prueba.ld
	$(CC) $(FLAGS) -T prueba.ld -o t2.elf $< && $(OC) -O binary t2.elf $@

oraculo: oraculo.c
	$(CC) -O2 -static -o $@ $<

clean:
	rm -f *.elf *.bin oraculo t1*.s t2.s
