# K6: el decodificador de los fallos en los chips (../fallo_a64.h) contra la CPU, con qemu-user: en big-endian con la
# toolchain de la caja y en little-endian con la de musl, si está. `make -C caja/pruebas_fallo`; la lanza también
# `make -C kernel prueba`.
TCS    := $(HOME)/.local/opt/toolchains
CC_BE  ?= $(TCS)/aarch64be--glibc--stable-2026.08-1/bin/aarch64_be-linux-gcc
CC_LE  ?= $(TCS)/aarch64--musl--stable-2026.08-1/bin/aarch64-linux-gcc
CFLAGS := -O2 -static -std=gnu2x -Wall -Wextra -Werror
B := ../build-pc

ifneq ($(wildcard $(CC_LE)),)
all: $(B)/prueba_fallo_be $(B)/prueba_fallo_le
	qemu-aarch64_be $(B)/prueba_fallo_be
	qemu-aarch64 $(B)/prueba_fallo_le
else
all: $(B)/prueba_fallo_be
	qemu-aarch64_be $(B)/prueba_fallo_be
	@echo "fallo_a64 (little-endian): sin $(CC_LE), no se prueba"
endif

$(B)/prueba_fallo_be: prueba_fallo.c ../fallo_a64.h ../motor.h | $(B)
	$(CC_BE) $(CFLAGS) prueba_fallo.c -o $@
$(B)/prueba_fallo_le: prueba_fallo.c ../fallo_a64.h ../motor.h | $(B)
	$(CC_LE) $(CFLAGS) prueba_fallo.c -o $@

$(B):
	mkdir -p $(B)

.PHONY: all
