# Compila el JIT de Emu68 como programa Linux AArch64 big-endian, sin libc,
# para ejecutarlo en shiva con qemu-aarch64_be.
TC      ?= $(HOME)/.local/opt/toolchains/aarch64be--glibc--stable-2026.08-1/bin/aarch64_be-linux-
CC      := $(TC)gcc
CXX     := $(TC)g++
EMU68   ?= ../third_party/Emu68
B       := build

CONTEXT_RESERVE := -ffixed-q19 -ffixed-v19 -ffixed-d19 -ffixed-q20 -ffixed-v20 -ffixed-d20 \
	-ffixed-q21 -ffixed-v21 -ffixed-d21 -ffixed-v22 -ffixed-d22 -ffixed-v23 -ffixed-d23 \
	-ffixed-v24 -ffixed-d24 -ffixed-v25 -ffixed-d25 -ffixed-v26 -ffixed-d26
M68K_FIXED := -ffixed-x12 -ffixed-x18 -ffixed-x19 -ffixed-x20 -ffixed-x21 -ffixed-x22 -ffixed-x23 \
	-ffixed-x24 -ffixed-x25 -ffixed-x26 -ffixed-x13 -ffixed-x14 -ffixed-x15 -ffixed-x16 -ffixed-x17 \
	-ffixed-x27 -ffixed-x28 -ffixed-x29
M68K_SAVED := -fcall-saved-x4 -fcall-saved-x5 -fcall-saved-x6 -fcall-saved-x7 -fcall-saved-x8 \
	-fcall-saved-x9 -fcall-saved-x10 -fcall-saved-x11

COMMON  := -mbig-endian -ffreestanding -fno-exceptions -fno-unwind-tables -fno-stack-protector \
	-fno-asynchronous-unwind-tables -fno-pic -fno-pie -march=armv8-a+crc -O2 -fomit-frame-pointer \
	-ffixed-x12 -falign-functions=32 -mno-outline-atomics -fno-builtin \
	-DEMU68_VERSION_MAJOR=1 -DEMU68_VERSION_MINOR=1 -DDASHOS_HOST=1 \
	-I$(EMU68)/include -I$(EMU68)/external/capstone/include -Iinclude $(CONTEXT_RESERVE)
CFLAGS  := $(COMMON) -std=gnu23 -Wno-error
CXXFLAGS:= $(COMMON) -std=gnu++23 -fno-rtti -Wno-error

JIT_C := M68k_Translator M68k_SR M68k_MULDIV M68k_MOVE M68k_EA M68k_LINE0 M68k_LINE4 M68k_LINE5 \
	M68k_LINE6 M68k_LINE8 M68k_LINE9 M68k_LINEB M68k_LINEC M68k_LINED M68k_LINEE M68k_LINEF \
	M68k_Exception M68k_ExceptionEntry M68k_CC support tlsf cache md5
JIT_CXX := TranslatorContext LRUCache ReturnStack cpp_support
MATH := $(basename $(notdir $(wildcard $(EMU68)/src/math/*.c)))

OBJS := $(JIT_C:%=$(B)/%.o) $(JIT_CXX:%=$(B)/%.o) $(MATH:%=$(B)/math_%.o) \
	$(B)/RegisterAllocator64.o $(B)/ExecutionLoop.o $(B)/host.o $(B)/emu68_context.o $(B)/entry.o

all: $(B)/jit-host

run: $(B)/jit-host
	qemu-aarch64_be $(B)/jit-host

$(B)/%.o: $(EMU68)/src/%.c | $(B)
	$(CC) $(CFLAGS) -c $< -o $@
$(B)/%.o: $(EMU68)/src/%.cpp | $(B)
	$(CXX) $(CXXFLAGS) -c $< -o $@
$(B)/RegisterAllocator64.o: $(EMU68)/src/aarch64/RegisterAllocator64.c | $(B)
	$(CC) $(CFLAGS) -c $< -o $@
# El bucle de ejecución rompe la ABI: los registros del 68k viven fijos en x13-x29.
$(B)/ExecutionLoop.o: $(EMU68)/src/ExecutionLoop.c | $(B)
	$(CC) $(CFLAGS) $(M68K_FIXED) -c $< -o $@
$(B)/M68k_ExceptionEntry.o: $(EMU68)/src/M68k_ExceptionEntry.c | $(B)
	$(CC) $(CFLAGS) $(M68K_SAVED) $(M68K_FIXED) -c $< -o $@
$(B)/math_%.o: $(EMU68)/src/math/%.c | $(B)
	$(CC) $(CFLAGS) $(M68K_SAVED) $(M68K_FIXED) -c $< -o $@
$(B)/host.o: host.c | $(B)
	$(CC) $(CFLAGS) -c $< -o $@
$(B)/emu68_context.o: emu68_context.c | $(B)
	$(CC) $(CFLAGS) $(M68K_FIXED) -c $< -o $@
$(B)/entry.o: entry.S | $(B)
	$(CC) $(COMMON) -c $< -o $@

$(B)/jit-host: $(OBJS)
	$(CXX) -mbig-endian -static -nostdlib -nostartfiles -Wl,-EB -Wl,-Map=$(B)/jit-host.map -o $@ $^ -lgcc

$(B):
	mkdir -p $(B)

clean:
	rm -rf $(B)
.PHONY: all clean run
