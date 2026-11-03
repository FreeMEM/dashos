# DashOS: compilar el kernel y sacar la imagen que carga el firmware de la Pi (o QEMU raspi4b).
OBJCOPY ?= $(HOME)/.local/opt/toolchains/aarch64be--glibc--stable-2026.08-1/bin/aarch64_be-linux-objcopy
PERFIL  ?= release
ELF     := target/aarch64_be-unknown-none-softfloat/$(PERFIL)/dashos-kernel
IMAGEN  := target/kernel8.img

all: $(IMAGEN)

$(IMAGEN): FORCE
	cd ../usuario && cargo build --release
	cargo build $(if $(filter release,$(PERFIL)),--release,)
	$(OBJCOPY) -O binary $(ELF) $(IMAGEN)

prueba: $(IMAGEN)
	../tools/prueba_qemu.sh $(IMAGEN) pruebas/esperado.txt

clean:
	cargo clean

FORCE:
.PHONY: all prueba clean FORCE
