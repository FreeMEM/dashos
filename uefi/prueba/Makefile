# U2: el núcleo de prueba, enlazado en 0x40200000 y pasado a binario plano (como kernel8.img).
nucleo.bin: nucleo.c
	clang --target=aarch64-none-elf -ffreestanding -nostdlib -mstrict-align -mgeneral-regs-only -O2 -c nucleo.c -o nucleo.o
	ld.lld -T nucleo.ld nucleo.o -o nucleo.elf
	llvm-objcopy -O binary nucleo.elf nucleo.bin
