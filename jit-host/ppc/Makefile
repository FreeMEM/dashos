# DashOS, fase F1 de la caja PowerPC (docs/PPC_LOOP.md): el JIT PPC de Emu68 (src/PPC_*.cpp) como programa Linux
# AArch64 big-endian sin libc, como el banco del 68k (../Makefile), para ejecutarlo con qemu-aarch64_be.
# Las pruebas (pruebas/) se comparan con qemu-ppc: ../comparar_ppc.py.
TC      ?= $(HOME)/.local/opt/toolchains/aarch64be--glibc--stable-2026.08-1/bin/aarch64_be-linux-
CC      := $(TC)gcc
CXX     := $(TC)g++
EMU68   ?= ../../third_party/Emu68
# F1e: FPSCR=1 (por defecto), los bits de excepción de FPSCR tras cada operación de la FPU (PPC_FPSCR_EXCEPTIONS);
# FPSCR=0, el camino rápido de Emu68 (solo las formas con punto), en build-rapido/
FPSCR   ?= 1
ifeq ($(FPSCR),0)
B       := build-rapido
else
B       := build
endif

CONTEXT_RESERVE := -ffixed-q19 -ffixed-v19 -ffixed-d19 -ffixed-q20 -ffixed-v20 -ffixed-d20 \
	-ffixed-q21 -ffixed-v21 -ffixed-d21 -ffixed-v22 -ffixed-d22 -ffixed-v23 -ffixed-d23 \
	-ffixed-v24 -ffixed-d24 -ffixed-v25 -ffixed-d25 -ffixed-v26 -ffixed-d26
PPC_FIXED := -ffixed-x12 -ffixed-x18 -ffixed-x19 -ffixed-x20 -ffixed-x21 -ffixed-x22 -ffixed-x23 -ffixed-x24 \
	-ffixed-x25 -ffixed-x26 -ffixed-x27 -ffixed-x28 -ffixed-x29
PPC_USED := -fcall-used-x4 -fcall-used-x5 -fcall-used-x6 -fcall-used-x7 -fcall-used-x8 -fcall-used-x9 \
	-fcall-used-x10 -fcall-used-x11

COMMON  := -MMD -MP -mbig-endian -ffreestanding -fno-exceptions -fno-unwind-tables -fno-stack-protector \
	-fno-asynchronous-unwind-tables -fno-pic -fno-pie -march=armv8-a+crc -O2 -fomit-frame-pointer \
	-ffixed-x12 -falign-functions=32 -mno-outline-atomics -fno-builtin \
	-DEMU68_VERSION_MAJOR=1 -DEMU68_VERSION_MINOR=1 -DDASHOS_HOST=1 -DPPC_SO_PROPAGATION=1 -DPPC_FPSCR_EXCEPTIONS=$(FPSCR) -DDASHOS_NATIVE_CALLS=1 \
	-Iinclude -I$(EMU68)/include -I../include $(CONTEXT_RESERVE)
CFLAGS  := $(COMMON) -std=gnu23 -Wno-error
CXXFLAGS:= $(COMMON) -std=gnu++23 -fno-rtti -Wno-error -fno-threadsafe-statics

PPC     := PPC_TranslatorContext PPC_LoadStore PPC_FPU PPC_SystemInstructions PPC_Arithmetic
OBJS    := $(PPC:%=$(B)/%.o) $(B)/PPC_Translator.o $(B)/PPC_ExecutionLoop.o $(B)/LRUCache.o \
	$(B)/TranslatorContext.o $(B)/ReturnStack.o $(B)/cpp_support.o $(B)/support.o $(B)/tlsf.o $(B)/cache.o $(B)/md5.o \
	$(B)/host_ppc.o $(B)/entry.o
# La rama 1_1_wip de Emu68 junta sus variables globales en src/globals.c.
OBJS    += $(if $(wildcard $(EMU68)/src/globals.c),$(B)/globals.o)

all: $(B)/jit-ppc

$(B)/%.o: $(EMU68)/src/%.cpp | $(B)
	$(CXX) $(CXXFLAGS) -c $< -o $@
$(B)/%.o: $(EMU68)/src/%.c | $(B)
	$(CC) $(CFLAGS) -c $< -o $@
$(B)/PPC_Translator.o: $(EMU68)/src/PPC_Translator.cpp | $(B)
	$(CXX) $(CXXFLAGS) $(PPC_USED) $(PPC_FIXED) -c $< -o $@
$(B)/PPC_ExecutionLoop.o: $(EMU68)/src/PPC_ExecutionLoop.cpp | $(B)
	$(CXX) $(CXXFLAGS) $(PPC_FIXED) -c $< -o $@
$(B)/LRUCache.o: $(EMU68)/src/LRUCache.cpp | $(B)
	$(CXX) $(CXXFLAGS) $(PPC_FIXED) -c $< -o $@
$(B)/host_ppc.o: host_ppc.cpp | $(B)
	$(CXX) $(CXXFLAGS) -c $< -o $@
$(B)/entry.o: ../entry.S | $(B)
	$(CC) $(COMMON) -c $< -o $@

$(B)/jit-ppc: $(OBJS)
	$(CXX) -mbig-endian -static -nostdlib -nostartfiles -Wl,-EB -Wl,-Map=$(B)/jit-ppc.map -o $@ $^ -lgcc

$(B):
	mkdir -p $(B)

-include $(OBJS:.o=.d)
