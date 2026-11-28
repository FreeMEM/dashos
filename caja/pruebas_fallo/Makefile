# K6: el decodificador de los fallos en los chips (../fallo_a64.h) contra la CPU, con qemu-user: en big-endian con la
# toolchain de la caja y en little-endian con la de musl, si está. K7: los trampolines (../trampolin_a64.c), igual.
# `make -C caja/pruebas_fallo`; la lanza también `make -C kernel prueba`.
TCS    := $(HOME)/.local/opt/toolchains
CC_BE  ?= $(TCS)/aarch64be--glibc--stable-2026.08-1/bin/aarch64_be-linux-gcc
CC_LE  ?= $(TCS)/aarch64--musl--stable-2026.08-1/bin/aarch64-linux-gcc
CFLAGS := -O2 -static -std=gnu2x -Wall -Wextra -Werror
# Como en la caja (../Makefile): los trampolines no tocan la FPU ni SIMD.
TFLAGS := -O2 -std=gnu2x -Wall -Wextra -Werror -mgeneral-regs-only -fno-builtin -fno-stack-protector
B := ../build-pc

ifneq ($(wildcard $(CC_LE)),)
all: $(B)/prueba_fallo_be $(B)/prueba_fallo_le $(B)/prueba_trampolin_be $(B)/prueba_trampolin_le
	qemu-aarch64_be $(B)/prueba_fallo_be
	qemu-aarch64 $(B)/prueba_fallo_le
	qemu-aarch64_be $(B)/prueba_trampolin_be
	qemu-aarch64 $(B)/prueba_trampolin_le
else
all: $(B)/prueba_fallo_be $(B)/prueba_trampolin_be
	qemu-aarch64_be $(B)/prueba_fallo_be
	qemu-aarch64_be $(B)/prueba_trampolin_be
	@echo "fallo_a64 y trampolin_a64 (little-endian): sin $(CC_LE), no se prueban"
endif

$(B)/prueba_fallo_be: prueba_fallo.c ../fallo_a64.h ../fallo_a64_decodifica.h ../motor.h | $(B)
	$(CC_BE) $(CFLAGS) prueba_fallo.c -o $@
$(B)/prueba_fallo_le: prueba_fallo.c ../fallo_a64.h ../fallo_a64_decodifica.h ../motor.h | $(B)
	$(CC_LE) $(CFLAGS) prueba_fallo.c -o $@

T_DEPS := prueba_trampolin.c ../trampolin_a64.c ../trampolin_a64.h ../fallo_a64.h ../fallo_a64_decodifica.h ../motor.h
$(B)/prueba_trampolin_be: $(T_DEPS) | $(B)
	$(CC_BE) $(TFLAGS) -c ../trampolin_a64.c -o $(B)/trampolin_a64_be.o
	$(CC_BE) $(CFLAGS) prueba_trampolin.c $(B)/trampolin_a64_be.o -o $@
$(B)/prueba_trampolin_le: $(T_DEPS) | $(B)
	$(CC_LE) $(TFLAGS) -c ../trampolin_a64.c -o $(B)/trampolin_a64_le.o
	$(CC_LE) $(CFLAGS) prueba_trampolin.c $(B)/trampolin_a64_le.o -o $@

$(B):
	mkdir -p $(B)

.PHONY: all
