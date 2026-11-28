# DashOS, H4: la caja Amiga. El JIT de Emu68 (como en jit-host/) en un programa de usuario de DashOS, big-endian y sin
# libc, con las bibliotecas nativas en Rust (pegamento/: exec, dos, graphics e intuition): build/caja-amiga, un
# ET_EXEC que `inicio` lanza en `make -C kernel arranque-virt`.
TC      ?= $(HOME)/.local/opt/toolchains/aarch64be--glibc--stable-2026.08-1/bin/aarch64_be-linux-
CC      := $(TC)gcc
CXX     := $(TC)g++
EMU68   ?= ../third_party/Emu68
# U7: `make ORDEN=le` saca la caja en little-endian (build-le/caja-amiga), para las CPU sin big-endian en EL0 (Apple):
# la memoria del 68k sigue en big-endian y el JIT da la vuelta a los bytes en cada acceso (parches 0004-0009 y 0012 de
# la rama dashos de Emu68; EMU68_HOST_LE). El kernel la corre en little-endian porque su ELF lo dice (EI_DATA).
ORDEN   ?= be
# L3 (docs/CAJA_LINUX_LOOP.md): con LINUX=1, la caja como programa de Linux (`make linux`, `make linux-x86`): siempre
# little-endian, con plataforma_linux.c, caja_linux.c y entrada_linux.S, y el pegamento sin la opción `dashos`.
LINUX   ?=
ifneq ($(LINUX),)
ORDEN   := le
endif
ifeq ($(ORDEN),le)
B       := build-le
ENDIAN  := -mlittle-endian
LDEND   := -Wl,-EL
INC_LE  := -I../jit-host/include-le
RUST_T  := aarch64-unknown-none-softfloat
else
B       := build
ENDIAN  := -mbig-endian
LDEND   := -Wl,-EB
INC_LE  :=
RUST_T  := aarch64_be-unknown-none-softfloat
endif
ifneq ($(LINUX),)
B       := build-linux
endif

# Los registros del 68k viven fijos en registros ARM (x13-x29, v19-v26): mismas opciones que jit-host/Makefile.
CONTEXT_RESERVE := -ffixed-q19 -ffixed-v19 -ffixed-d19 -ffixed-q20 -ffixed-v20 -ffixed-d20 \
	-ffixed-q21 -ffixed-v21 -ffixed-d21 -ffixed-v22 -ffixed-d22 -ffixed-v23 -ffixed-d23 \
	-ffixed-v24 -ffixed-d24 -ffixed-v25 -ffixed-d25 -ffixed-v26 -ffixed-d26
M68K_FIXED := -ffixed-x12 -ffixed-x18 -ffixed-x19 -ffixed-x20 -ffixed-x21 -ffixed-x22 -ffixed-x23 \
	-ffixed-x24 -ffixed-x25 -ffixed-x26 -ffixed-x13 -ffixed-x14 -ffixed-x15 -ffixed-x16 -ffixed-x17 \
	-ffixed-x27 -ffixed-x28 -ffixed-x29
M68K_SAVED := -fcall-saved-x4 -fcall-saved-x5 -fcall-saved-x6 -fcall-saved-x7 -fcall-saved-x8 \
	-fcall-saved-x9 -fcall-saved-x10 -fcall-saved-x11

COMMON  := -MMD -MP $(ENDIAN) -ffreestanding -fno-exceptions -fno-unwind-tables -fno-stack-protector \
	-fno-asynchronous-unwind-tables -fpie -march=armv8-a+crc -O2 -fomit-frame-pointer \
	-ffixed-x12 -falign-functions=32 -mno-outline-atomics -fno-builtin \
	-DEMU68_VERSION_MAJOR=1 -DEMU68_VERSION_MINOR=1 -DDASHOS_HOST=1 -DDASHOS_NATIVE_CALLS=1 \
	-I$(EMU68)/include -I$(EMU68)/external/capstone/include -I../jit-host/include $(INC_LE) $(CONTEXT_RESERVE)
ifneq ($(LINUX),)
# La página 0 en $10000 (parche 0018 de Emu68 y plataforma_linux.c).
COMMON  += -DDASHOS_PAGINA0=0x10000 -DCAJA_LINUX=1 -ffunction-sections -fdata-sections
endif
CFLAGS  := $(COMMON) -std=gnu23 -Wno-error
ifneq ($(CAJA_INTERACTIVA),)
CFLAGS  += -DCAJA_INTERACTIVA=$(CAJA_INTERACTIVA)
endif
CXXFLAGS:= $(COMMON) -std=gnu++23 -fno-rtti -Wno-error

JIT_C := M68k_Translator M68k_SR M68k_MULDIV M68k_MOVE M68k_EA M68k_LINE0 M68k_LINE4 M68k_LINE5 \
	M68k_LINE6 M68k_LINE8 M68k_LINE9 M68k_LINEB M68k_LINEC M68k_LINED M68k_LINEE M68k_LINEF \
	M68k_Exception M68k_ExceptionEntry M68k_CC support tlsf cache md5
JIT_CXX := TranslatorContext LRUCache ReturnStack cpp_support
MATH := $(basename $(notdir $(wildcard $(EMU68)/src/math/*.c)))

OBJS := $(JIT_C:%=$(B)/%.o) $(JIT_CXX:%=$(B)/%.o) $(MATH:%=$(B)/math_%.o) \
	$(B)/RegisterAllocator64.o $(B)/ExecutionLoop.o $(B)/emu68_context.o \
	$(B)/motor_emu68.o $(B)/trampolin_a64.o $(B)/hunk.o
ifneq ($(LINUX),)
OBJS += $(B)/caja_linux.o $(B)/plataforma_linux.o $(B)/entrada_linux.o $(B)/programas_linux.o
else
OBJS += $(B)/caja.o $(B)/plataforma_dashos.o $(B)/entrada.o $(B)/programas.o
endif

# La pila del ELF (64 KB, la de siempre) solo se usa al empezar: entrada.S pasa enseguida a una por encima de 4 GB.
all: $(B)/caja-amiga

$(B)/%.o: $(EMU68)/src/%.c | $(B)
	$(CC) $(CFLAGS) -c $< -o $@
$(B)/%.o: $(EMU68)/src/%.cpp | $(B)
	$(CXX) $(CXXFLAGS) -c $< -o $@
$(B)/RegisterAllocator64.o: $(EMU68)/src/aarch64/RegisterAllocator64.c | $(B)
	$(CC) $(CFLAGS) -c $< -o $@
$(B)/ExecutionLoop.o: $(EMU68)/src/ExecutionLoop.c | $(B)
	$(CC) $(CFLAGS) $(M68K_FIXED) -c $< -o $@
$(B)/M68k_ExceptionEntry.o: $(EMU68)/src/M68k_ExceptionEntry.c | $(B)
	$(CC) $(CFLAGS) $(M68K_SAVED) $(M68K_FIXED) -c $< -o $@
$(B)/math_%.o: $(EMU68)/src/math/%.c | $(B)
	$(CC) $(CFLAGS) $(M68K_SAVED) $(M68K_FIXED) -c $< -o $@
$(B)/emu68_context.o: ../jit-host/emu68_context.c | $(B)
	$(CC) $(CFLAGS) $(M68K_FIXED) -c $< -o $@
$(B)/caja.o: caja.c | $(B)
	$(CC) $(CFLAGS) -c $< -o $@
$(B)/plataforma_dashos.o: plataforma_dashos.c | $(B)
	$(CC) $(CFLAGS) -c $< -o $@
$(B)/caja_linux.o: caja_linux.c caja.c | $(B)
	$(CC) $(CFLAGS) -c $< -o $@
$(B)/plataforma_linux.o: plataforma_linux.c | $(B)
	$(CC) $(CFLAGS) -c $< -o $@
$(B)/entrada_linux.o: entrada_linux.S | $(B)
	$(CC) $(COMMON) -c $< -o $@
$(B)/programas_linux.o: programas_linux.S pruebas68k/sistema.bin | $(B)
	$(CC) $(COMMON) -c $< -o $@
$(B)/motor_emu68.o: motor_emu68.c | $(B)
	$(CC) $(CFLAGS) -c $< -o $@
# K7: los trampolines corren con los registros de la FPU y SIMD del JIT vivos y no los guardan: sin tocarlos.
$(B)/trampolin_a64.o: trampolin_a64.c | $(B)
	$(CC) $(CFLAGS) -mgeneral-regs-only -c $< -o $@
	@if $(TC)objdump -dr $@ | grep -E 'R_AARCH64_(CALL|JUMP)26[[:space:]]+(memset|memcpy|memmove)'; then \
		echo "trampolin_a64.o llama a memset o memcpy, que pueden usar SIMD"; rm -f $@; exit 1; fi
$(B)/hunk.o: hunk.c | $(B)
	$(CC) $(CFLAGS) -c $< -o $@
$(B)/entrada.o: entrada.S | $(B)
	$(CC) $(COMMON) -c $< -o $@

# Programas de Dash (el lenguaje de ~/Projects/FreeMEMLang): sus ejecutables hunk no van en git; se compilan aquí con
# `dash`, por los dos caminos (directo, y --gcc por C con m68k-amigaos-gcc). Sin `dash`, un fichero vacío y un aviso:
# la caja lo dice al arrancar y no los ejecuta. Las fuentes propias van en pruebas68k/dash/; hola.dash es de los
# ejemplos de FreeMEMLang. `dash --gcc` deja su C en build/ de FreeMEMLang (con el nombre de la fuente: por eso las
# nuestras se llaman caja_*).
DASH    ?= $(HOME)/Projects/FreeMEMLang/dash
DASH_EJ := $(dir $(DASH))examples
DASH_TESTS := $(dir $(DASH))tests
# S28: los virus de laboratorio (pruebas68k/malware/, con su Makefile; van en git).
MALWARE := $(patsubst %,pruebas68k/malware/%.exe,arranque enlace parche residente guion troyano memoria)
# Los programas de Dash son 68k: los mismos para los dos órdenes, siempre en build/dash (programas.S los busca ahí).
DASH_EXES := build/dash/hola.exe build/dash/texto.exe build/dash/texto_gcc.exe build/dash/ventana.exe \
	build/dash/interp.exe build/dash/listas.exe build/dash/ctrl_c.exe build/dash/ctrl_c_gcc.exe build/dash/ctrl_c_for.exe \
	build/dash/trap_ilegal.exe build/dash/trap_divcero.exe build/dash/test_gadtools.exe \
	build/dash/test_reaction.exe build/dash/test_doc_reaction.exe
PROGRAMAS_DEPS := programas.S pruebas68k/sistema.bin pruebas68k/hola.bin pruebas68k/lineaa.bin pruebas68k/bucle.bin pruebas68k/mala.bin pruebas68k/saludo.exe pruebas68k/ventana.exe pruebas68k/hostil.exe pruebas68k/anidado.exe pruebas68k/gadtools.exe pruebas68k/boopsi.exe pruebas68k/ramlib.exe pruebas68k/capa.exe pruebas68k/glotona.exe pruebas68k/memh.exe pruebas68k/procesos.exe pruebas68k/actualiza.exe pruebas68k/ils.exe pruebas68k/dtprueba.exe pruebas68k/cgx.exe pruebas68k/chips.exe pruebas68k/vposr.bin pruebas68k/ipl.bin $(MALWARE) $(DASH_EXES)
$(B)/programas.o: $(PROGRAMAS_DEPS) | $(B)
	$(CC) $(COMMON) -c $< -o $@

define compila_dash
	@mkdir -p build/dash
	@if [ -x $(DASH) ]; then echo "dash $(2) $(1)"; $(DASH) $(2) $(1) -o $@.tmp >$@.log 2>&1 && mv $@.tmp $@ \
		|| { cat $@.log; echo "AVISO: $(notdir $@) no compila: vacío"; : > $@; }; \
		 else echo "AVISO: sin $(DASH): $(notdir $@) vacío (la caja no ejecuta programas de Dash)"; : > $@; fi
endef
build/dash/hola.exe: $(wildcard $(DASH_EJ)/hola.dash) | $(B)
	$(call compila_dash,$(DASH_EJ)/hola.dash,)
build/dash/texto.exe: pruebas68k/dash/caja_texto.dash | $(B)
	$(call compila_dash,$<,)
build/dash/texto_gcc.exe: pruebas68k/dash/caja_texto.dash | $(B)
	$(call compila_dash,$<,--gcc)
build/dash/ventana.exe: pruebas68k/dash/caja_ventana.dash | $(B)
	$(call compila_dash,$<,)
# Dos pruebas de texto de FreeMEMLang, tal cual (su salida esperada, la de sus líneas "# expect-out").
build/dash/interp.exe: $(wildcard $(DASH_TESTS)/test_interp_comillas.dash) | $(B)
	$(call compila_dash,$(DASH_TESTS)/test_interp_comillas.dash,)
build/dash/listas.exe: $(wildcard $(DASH_TESTS)/test_lista_listas_literal.dash) | $(B)
	$(call compila_dash,$(DASH_TESTS)/test_lista_listas_literal.dash,)

# Ctrl-C en un bucle (las excepciones de tarea, por los dos caminos) y tres pruebas de FreeMEMLang tal cual: Ctrl-C
# por SetSignal a sí mismo, y dos excepciones del procesador que recoge el tc_TrapCode de Dash (llevan @native: van
# por C aunque se pida el camino directo).
build/dash/ctrl_c.exe: pruebas68k/dash/caja_ctrl_c.dash | $(B)
	$(call compila_dash,$<,)
build/dash/ctrl_c_gcc.exe: pruebas68k/dash/caja_ctrl_c.dash | $(B)
	$(call compila_dash,$<,--gcc)
build/dash/ctrl_c_for.exe: $(wildcard $(DASH_TESTS)/test_ctrl_c_for.dash) | $(B)
	$(call compila_dash,$(DASH_TESTS)/test_ctrl_c_for.dash,)
build/dash/trap_ilegal.exe: $(wildcard $(DASH_TESTS)/test_trap_illegal.dash) | $(B)
	$(call compila_dash,$(DASH_TESTS)/test_trap_illegal.dash,)
build/dash/trap_divcero.exe: $(wildcard $(DASH_TESTS)/test_trap_divzero.dash) | $(B)
	$(call compila_dash,$(DASH_TESTS)/test_trap_divzero.dash,)

# El ejemplo de GadTools de FreeMEMLang, tal cual.
build/dash/test_gadtools.exe: $(wildcard $(DASH_EJ)/test_gadtools.dash) | $(B)
	$(call compila_dash,$(DASH_EJ)/test_gadtools.dash,)
# ReAction: el ejemplo (espera a que pulsen) y la prueba de la documentación (acaba sola).
build/dash/test_reaction.exe: $(wildcard $(DASH_EJ)/test_reaction.dash) | $(B)
	$(call compila_dash,$(DASH_EJ)/test_reaction.dash,)
build/dash/test_doc_reaction.exe: $(wildcard $(DASH_TESTS)/test_doc_reaction.dash) | $(B)
	$(call compila_dash,$(DASH_TESTS)/test_doc_reaction.dash,)

# Las bibliotecas nativas (pegamento/, una staticlib de Rust para aarch64_be-unknown-none-softfloat). Su memcpy (de
# compiler_builtins, que entra con LTO) se queda dentro: el del C es el de Emu68 (support.c).
GC_LINUX := -Wl,--gc-sections
# En Linux, los segmentos alineados a 64 KB (enlace.ld los alinea a 4 KB): así sirve también con páginas de 16 KB
# (Asahi) o de 64 KB, sin que código y datos compartan página.
LD_LINUX = $(if $(LINUX),$(B)/enlace.ld,enlace.ld)
PAGINA_LINUX := -Wl,-z,max-page-size=65536
$(B)/enlace.ld: enlace.ld | $(B)
	sed 's/ALIGN(4096)/ALIGN(65536)/g' $< > $@
# La de Linux, sin la opción `dashos` y en su propio directorio (no pisa la de DashOS del mismo objetivo).
RUST_LINUX = $(if $(LINUX),--no-default-features --target-dir target-linux)
RUST_A := pegamento/$(if $(LINUX),target-linux,target)/$(RUST_T)/release/libcaja_pegamento.a
$(B)/pegamento.a: FORCE | $(B)
	cd pegamento && cargo build --release --quiet --target $(RUST_T) $(RUST_LINUX)
	$(TC)objcopy --localize-symbol=memcpy --localize-symbol=memset --localize-symbol=memmove $(RUST_A) $@.tmp
	cmp -s $@.tmp $@ || mv $@.tmp $@
	rm -f $@.tmp

# Un PIE estático (enlace.ld): el kernel lo pone por encima de 128 GB, fuera del alcance del 68k (S28). Solo puede llevar
# reubicaciones R_AARCH64_RELATIVE (las aplica entrada.S): se comprueba aquí.
$(B)/caja-amiga: $(OBJS) $(B)/pegamento.a $(LD_LINUX)
	$(CXX) $(ENDIAN) -static-pie -nostdlib -nostartfiles $(LDEND) -Wl,--build-id=none -Wl,-z,noexecstack \
		-Wl,-z,text $(if $(LINUX),$(PAGINA_LINUX)) -Wl,-T,$(LD_LINUX) -Wl,-Map=$(B)/caja-amiga.map $(if $(LINUX),$(GC_LINUX)) -o $@.tmp $(OBJS) $(B)/pegamento.a
	@if $(TC)readelf -rW $@.tmp | grep -E '^[0-9a-f]{16} ' | grep -v R_AARCH64_RELATIVE; then \
		echo "La caja lleva reubicaciones que entrada.S no aplica"; rm -f $@.tmp; exit 1; fi
	mv $@.tmp $@

$(B):
	mkdir -p $(B)

-include $(OBJS:.o=.d)
# Con otras opciones (de -fno-pie a -fpie, por ejemplo), todo de nuevo.
$(OBJS): Makefile

# El cargador hunk, en el PC (con ASan y UBSan).
pruebas-pc:
	$(MAKE) -C pruebas_hunk

clean:
	rm -rf build build-le $(BX) build-pc pegamento/target build-linux build-linux-x86 pegamento/target-linux

# L3: la caja como programa de Linux, en AArch64 (build-linux/caja-amiga) y en x86-64 (build-linux-x86/caja-amiga).
linux:
	$(MAKE) LINUX=1 build-linux/caja-amiga
linux-x86:
	$(MAKE) LINUX=1 build-linux-x86/caja-amiga
.PHONY: linux linux-x86
.PHONY: all clean pruebas-pc FORCE
FORCE:

# --- X7: la caja de x86-64 (make -C caja x86): build-x86/caja-amiga ---
# El C común (caja.c, hunk.c) con el motor RelevoJIT (motor_relevo.c; su crate va enlazado en pegamento/): su JIT (X8,
# MOTOR=jit, por omisión) o su intérprete (X7, MOTOR=interprete). La entrada de
# x86 (entrada_x86.S) y lo que en ARM da Emu68: x86/soporte.c, y tlsf.c de Emu68 con x86/support.h y x86/spinlock.h. Los
# programas 68k son los de ARM (los de Dash, en build/dash/). RelevoJIT sale de GitHub con el `rev` de
# pegamento/Cargo.toml (Cargo lo baja; su include/relevo.h se busca con `cargo metadata`). RELEVO_DIR=<ruta> usa un
# clon local en su lugar.
BX := $(if $(LINUX),build-linux-x86,build-x86)
CC_X86 ?= gcc
MOTOR ?= jit
ifeq ($(filter $(MOTOR),jit interprete),)
$(error MOTOR=$(MOTOR): tiene que ser jit o interprete)
endif
RELEVO_DIR ?=
# Se calcula una vez y solo si hace falta (las reglas de ARM no la tocan).
RELEVO_INC = $(eval RELEVO_INC := $(if $(RELEVO_DIR),$(abspath $(RELEVO_DIR))/include,$(shell cd pegamento && \
	cargo metadata --format-version 1 --filter-platform x86_64-unknown-none | python3 -c 'import json, os, sys; \
	print(next(os.path.dirname(p["manifest_path"]) for p in json.load(sys.stdin)["packages"] if p["name"] == "relevo"))')/include))$(RELEVO_INC)
CFLAGS_X86 = -MMD -MP -ffreestanding -fno-builtin -fno-stack-protector -fno-exceptions -fno-unwind-tables \
	-fno-asynchronous-unwind-tables -fpie -O2 -fno-tree-loop-distribute-patterns -std=gnu23 \
	-Ix86 -I. -I$(EMU68)/include -I$(RELEVO_INC) $(if $(LINUX),-DCAJA_LINUX=1 -ffunction-sections -fdata-sections)
OBJS_X86 := $(BX)/motor_relevo.o $(BX)/hunk.o $(BX)/soporte.o $(BX)/tlsf.o
ifneq ($(LINUX),)
OBJS_X86 += $(BX)/caja_linux.o $(BX)/plataforma_linux.o $(BX)/entrada_linux.o $(BX)/programas_linux.o
else
OBJS_X86 += $(BX)/caja.o $(BX)/plataforma_dashos.o $(BX)/entrada_x86.o $(BX)/programas.o
endif
RUST_A_X86 := pegamento/$(if $(LINUX),target-linux,target)/x86_64-unknown-none/release/libcaja_pegamento.a
RELEVO_PATCH := $(if $(RELEVO_DIR),--config 'patch."https://github.com/FreeMEM/RelevoJIT".relevo.path="$(abspath $(RELEVO_DIR))"')

x86: $(BX)/caja-amiga

$(BX)/%.o: %.c Makefile | $(BX)
	$(CC_X86) $(CFLAGS_X86) -c $< -o $@
# El motor elegido: si MOTOR cambia, motor_relevo.o se rehace (build-x86/motor, con el último).
$(BX)/motor_relevo.o: $(BX)/motor
$(BX)/motor: FORCE | $(BX)
	@echo $(MOTOR) | cmp -s - $@ || echo $(MOTOR) > $@
$(BX)/motor_relevo.o: CFLAGS_X86 += -DMOTOR_JIT=$(if $(filter jit,$(MOTOR)),1,0)
$(BX)/soporte.o: x86/soporte.c Makefile | $(BX)
	$(CC_X86) $(CFLAGS_X86) -c $< -o $@
$(BX)/tlsf.o: $(EMU68)/src/tlsf.c Makefile | $(BX)
	$(CC_X86) $(CFLAGS_X86) -c $< -o $@
$(BX)/entrada_x86.o: entrada_x86.S Makefile | $(BX)
	$(CC_X86) $(CFLAGS_X86) -c $< -o $@
$(BX)/entrada_linux.o: entrada_linux.S Makefile | $(BX)
	$(CC_X86) $(CFLAGS_X86) -c $< -o $@
$(BX)/programas_linux.o: programas_linux.S pruebas68k/sistema.bin | $(BX)
	$(CC_X86) $(CFLAGS_X86) -c $< -o $@
$(BX)/programas.o: $(PROGRAMAS_DEPS) | $(BX)
	$(CC_X86) $(CFLAGS_X86) -c programas.S -o $@
$(BX)/pegamento.a: FORCE | $(BX)
	cd pegamento && cargo build --release --quiet --target x86_64-unknown-none $(RELEVO_PATCH) $(RUST_LINUX)
	objcopy --localize-symbol=memcpy --localize-symbol=memset --localize-symbol=memmove $(RUST_A_X86) $@.tmp
	cmp -s $@.tmp $@ || mv $@.tmp $@
	rm -f $@.tmp

# Un PIE estático (enlace.ld), como en ARM; solo con reubicaciones R_X86_64_RELATIVE (las aplica entrada_x86.S).
$(BX)/caja-amiga: $(OBJS_X86) $(BX)/pegamento.a enlace.ld
	$(CC_X86) -static-pie -nostdlib -nostartfiles -Wl,--build-id=none -Wl,-z,noexecstack -Wl,-z,text \
		-Wl,-T,enlace.ld -Wl,-Map=$(BX)/caja-amiga.map $(if $(LINUX),$(GC_LINUX)) -o $@.tmp $(OBJS_X86) $(BX)/pegamento.a
	@if readelf -rW $@.tmp | grep -E '^[0-9a-f]{16} ' | grep -v R_X86_64_RELATIVE; then \
		echo "La caja de x86 lleva reubicaciones que entrada_x86.S no aplica"; rm -f $@.tmp; exit 1; fi
	mv $@.tmp $@

$(BX):
	mkdir -p $(BX)

-include $(OBJS_X86:.o=.d)
.PHONY: x86
