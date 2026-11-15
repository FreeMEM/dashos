# DashOS, H4: la caja Amiga. El JIT de Emu68 (como en jit-host/) en un programa de usuario de DashOS, big-endian y sin
# libc, con las bibliotecas nativas en Rust (pegamento/: exec, dos, graphics e intuition): build/caja-amiga, un
# ET_EXEC que `inicio` lanza en `make -C kernel arranque-virt`.
TC      ?= $(HOME)/.local/opt/toolchains/aarch64be--glibc--stable-2026.08-1/bin/aarch64_be-linux-
CC      := $(TC)gcc
CXX     := $(TC)g++
EMU68   ?= ../third_party/Emu68
B       := build

# Los registros del 68k viven fijos en registros ARM (x13-x29, v19-v26): mismas opciones que jit-host/Makefile.
CONTEXT_RESERVE := -ffixed-q19 -ffixed-v19 -ffixed-d19 -ffixed-q20 -ffixed-v20 -ffixed-d20 \
	-ffixed-q21 -ffixed-v21 -ffixed-d21 -ffixed-v22 -ffixed-d22 -ffixed-v23 -ffixed-d23 \
	-ffixed-v24 -ffixed-d24 -ffixed-v25 -ffixed-d25 -ffixed-v26 -ffixed-d26
M68K_FIXED := -ffixed-x12 -ffixed-x18 -ffixed-x19 -ffixed-x20 -ffixed-x21 -ffixed-x22 -ffixed-x23 \
	-ffixed-x24 -ffixed-x25 -ffixed-x26 -ffixed-x13 -ffixed-x14 -ffixed-x15 -ffixed-x16 -ffixed-x17 \
	-ffixed-x27 -ffixed-x28 -ffixed-x29
M68K_SAVED := -fcall-saved-x4 -fcall-saved-x5 -fcall-saved-x6 -fcall-saved-x7 -fcall-saved-x8 \
	-fcall-saved-x9 -fcall-saved-x10 -fcall-saved-x11

COMMON  := -MMD -MP -mbig-endian -ffreestanding -fno-exceptions -fno-unwind-tables -fno-stack-protector \
	-fno-asynchronous-unwind-tables -fpie -march=armv8-a+crc -O2 -fomit-frame-pointer \
	-ffixed-x12 -falign-functions=32 -mno-outline-atomics -fno-builtin \
	-DEMU68_VERSION_MAJOR=1 -DEMU68_VERSION_MINOR=1 -DDASHOS_HOST=1 -DDASHOS_NATIVE_CALLS=1 \
	-I$(EMU68)/include -I$(EMU68)/external/capstone/include -I../jit-host/include $(CONTEXT_RESERVE)
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
	$(B)/caja.o $(B)/hunk.o $(B)/entrada.o $(B)/programas.o

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
DASH_EXES := $(B)/dash/hola.exe $(B)/dash/texto.exe $(B)/dash/texto_gcc.exe $(B)/dash/ventana.exe \
	$(B)/dash/interp.exe $(B)/dash/listas.exe $(B)/dash/ctrl_c.exe $(B)/dash/ctrl_c_gcc.exe $(B)/dash/ctrl_c_for.exe \
	$(B)/dash/trap_ilegal.exe $(B)/dash/trap_divcero.exe $(B)/dash/test_gadtools.exe \
	$(B)/dash/test_reaction.exe $(B)/dash/test_doc_reaction.exe
$(B)/programas.o: programas.S pruebas68k/sistema.bin pruebas68k/hola.bin pruebas68k/lineaa.bin pruebas68k/bucle.bin pruebas68k/mala.bin pruebas68k/saludo.exe pruebas68k/ventana.exe pruebas68k/hostil.exe pruebas68k/anidado.exe pruebas68k/gadtools.exe pruebas68k/boopsi.exe pruebas68k/ramlib.exe pruebas68k/capa.exe pruebas68k/glotona.exe pruebas68k/memh.exe pruebas68k/procesos.exe pruebas68k/actualiza.exe $(MALWARE) $(DASH_EXES) | $(B)
	$(CC) $(COMMON) -c $< -o $@

define compila_dash
	@mkdir -p $(B)/dash
	@if [ -x $(DASH) ]; then echo "dash $(2) $(1)"; $(DASH) $(2) $(1) -o $@.tmp >$@.log 2>&1 && mv $@.tmp $@ \
		|| { cat $@.log; echo "AVISO: $(notdir $@) no compila: vacío"; : > $@; }; \
		 else echo "AVISO: sin $(DASH): $(notdir $@) vacío (la caja no ejecuta programas de Dash)"; : > $@; fi
endef
$(B)/dash/hola.exe: $(wildcard $(DASH_EJ)/hola.dash) | $(B)
	$(call compila_dash,$(DASH_EJ)/hola.dash,)
$(B)/dash/texto.exe: pruebas68k/dash/caja_texto.dash | $(B)
	$(call compila_dash,$<,)
$(B)/dash/texto_gcc.exe: pruebas68k/dash/caja_texto.dash | $(B)
	$(call compila_dash,$<,--gcc)
$(B)/dash/ventana.exe: pruebas68k/dash/caja_ventana.dash | $(B)
	$(call compila_dash,$<,)
# Dos pruebas de texto de FreeMEMLang, tal cual (su salida esperada, la de sus líneas "# expect-out").
$(B)/dash/interp.exe: $(wildcard $(DASH_TESTS)/test_interp_comillas.dash) | $(B)
	$(call compila_dash,$(DASH_TESTS)/test_interp_comillas.dash,)
$(B)/dash/listas.exe: $(wildcard $(DASH_TESTS)/test_lista_listas_literal.dash) | $(B)
	$(call compila_dash,$(DASH_TESTS)/test_lista_listas_literal.dash,)

# Ctrl-C en un bucle (las excepciones de tarea, por los dos caminos) y tres pruebas de FreeMEMLang tal cual: Ctrl-C
# por SetSignal a sí mismo, y dos excepciones del procesador que recoge el tc_TrapCode de Dash (llevan @native: van
# por C aunque se pida el camino directo).
$(B)/dash/ctrl_c.exe: pruebas68k/dash/caja_ctrl_c.dash | $(B)
	$(call compila_dash,$<,)
$(B)/dash/ctrl_c_gcc.exe: pruebas68k/dash/caja_ctrl_c.dash | $(B)
	$(call compila_dash,$<,--gcc)
$(B)/dash/ctrl_c_for.exe: $(wildcard $(DASH_TESTS)/test_ctrl_c_for.dash) | $(B)
	$(call compila_dash,$(DASH_TESTS)/test_ctrl_c_for.dash,)
$(B)/dash/trap_ilegal.exe: $(wildcard $(DASH_TESTS)/test_trap_illegal.dash) | $(B)
	$(call compila_dash,$(DASH_TESTS)/test_trap_illegal.dash,)
$(B)/dash/trap_divcero.exe: $(wildcard $(DASH_TESTS)/test_trap_divzero.dash) | $(B)
	$(call compila_dash,$(DASH_TESTS)/test_trap_divzero.dash,)

# El ejemplo de GadTools de FreeMEMLang, tal cual.
$(B)/dash/test_gadtools.exe: $(wildcard $(DASH_EJ)/test_gadtools.dash) | $(B)
	$(call compila_dash,$(DASH_EJ)/test_gadtools.dash,)
# ReAction: el ejemplo (espera a que pulsen) y la prueba de la documentación (acaba sola).
$(B)/dash/test_reaction.exe: $(wildcard $(DASH_EJ)/test_reaction.dash) | $(B)
	$(call compila_dash,$(DASH_EJ)/test_reaction.dash,)
$(B)/dash/test_doc_reaction.exe: $(wildcard $(DASH_TESTS)/test_doc_reaction.dash) | $(B)
	$(call compila_dash,$(DASH_TESTS)/test_doc_reaction.dash,)

# Las bibliotecas nativas (pegamento/, una staticlib de Rust para aarch64_be-unknown-none-softfloat). Su memcpy (de
# compiler_builtins, que entra con LTO) se queda dentro: el del C es el de Emu68 (support.c).
RUST_A := pegamento/target/aarch64_be-unknown-none-softfloat/release/libcaja_pegamento.a
$(B)/pegamento.a: FORCE | $(B)
	cd pegamento && cargo build --release --quiet
	$(TC)objcopy --localize-symbol=memcpy --localize-symbol=memset --localize-symbol=memmove $(RUST_A) $@.tmp
	cmp -s $@.tmp $@ || mv $@.tmp $@
	rm -f $@.tmp

# Un PIE estático (enlace.ld): el kernel lo pone por encima de 128 GB, fuera del alcance del 68k (S28). Solo puede llevar
# reubicaciones R_AARCH64_RELATIVE (las aplica entrada.S): se comprueba aquí.
$(B)/caja-amiga: $(OBJS) $(B)/pegamento.a enlace.ld
	$(CXX) -mbig-endian -static-pie -nostdlib -nostartfiles -Wl,-EB -Wl,--build-id=none -Wl,-z,noexecstack \
		-Wl,-z,text -Wl,-T,enlace.ld -Wl,-Map=$(B)/caja-amiga.map -o $@.tmp $(OBJS) $(B)/pegamento.a
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
	rm -rf $(B) build-pc pegamento/target
.PHONY: all clean pruebas-pc FORCE
FORCE:
