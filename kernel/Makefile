# DashOS: compilar el kernel y sacar la imagen que carga el firmware de la Pi (o QEMU raspi4b).
BOOTLIN ?= $(HOME)/.local/opt/toolchains/aarch64be--glibc--stable-2026.08-1/bin
OBJCOPY ?= $(BOOTLIN)/aarch64_be-linux-objcopy
PIE     := ../usuario/target/pie
PERFIL  ?= release
ELF     := target/aarch64_be-unknown-none-softfloat/$(PERFIL)/dashos-kernel
IMAGEN  := target/kernel8.img

all: $(IMAGEN)

$(IMAGEN): FORCE
	cd ../usuario && cargo build --release
	cd ../usuario && cargo build --release --target aarch64_be-unknown-none-softfloat --bin hola --bin fpu_b
	$(BOOTLIN)/aarch64_be-linux-as -EL -o $(PIE).o ../usuario/asm/pie.s
	$(BOOTLIN)/aarch64_be-linux-ld -EL -pie --no-dynamic-linker -z noexecstack -z stack-size=32768 -o $(PIE) $(PIE).o
	cargo build $(if $(filter release,$(PERFIL)),--release,)
	$(OBJCOPY) -O binary $(ELF) $(IMAGEN)

prueba: $(IMAGEN)
	../tools/prueba_qemu.sh $(IMAGEN) pruebas/esperado.txt 15

clean:
	cargo clean

FORCE:
.PHONY: all prueba clean FORCE
