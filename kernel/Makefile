# DashOS: compilar el kernel y sacar la imagen que carga el firmware de la Pi (o QEMU raspi4b).
#
# make         la imagen normal, sin pruebas (target/kernel8.img)
# make prueba  la imagen con las pruebas del arranque y los programas de usuario que lanzan, y la prueba en QEMU
# make arranque  la imagen normal en QEMU: tiene que llegar a "DashOS en marcha"
# make virt    la imagen para QEMU virt (U3, docs/UEFI_LOOP.md), que carga el programa de UEFI
# make arranque-virt  esa imagen dentro del programa de UEFI, arrancada por EDK2 en QEMU virt
BOOTLIN ?= $(HOME)/.local/opt/toolchains/aarch64be--glibc--stable-2026.08-1/bin
OBJCOPY ?= $(BOOTLIN)/aarch64_be-linux-objcopy
PIE     := ../usuario/target/pie
PERFIL  ?= release
ELF     := target/aarch64-unknown-none-softfloat/$(PERFIL)/dashos-kernel
IMAGEN  := target/kernel8.img
PRUEBAS := target/kernel8-pruebas.img
CARGO_PERFIL := $(if $(filter release,$(PERFIL)),--release,)

all: $(IMAGEN)

$(IMAGEN): usuario FORCE
	cargo build $(CARGO_PERFIL)
	$(OBJCOPY) -O binary $(ELF) $(IMAGEN)

usuario: FORCE
	cd ../usuario && cargo build --release --features archivos,ventanas,sonido
	cd ../usuario && cargo build --release --target aarch64_be-unknown-none-softfloat --bin hola --bin fpu_b
	$(BOOTLIN)/aarch64_be-linux-as -EL -o $(PIE).o ../usuario/asm/pie.s
	$(BOOTLIN)/aarch64_be-linux-ld -EL -pie --no-dynamic-linker -z noexecstack -z stack-size=32768 -o $(PIE) $(PIE).o

$(PRUEBAS): usuario FORCE
	cargo build $(CARGO_PERFIL) --features pruebas
	$(OBJCOPY) -O binary $(ELF) $(PRUEBAS)

prueba: $(PRUEBAS)
	../tools/prueba_qemu.sh $(PRUEBAS) pruebas/esperado.txt 90

arranque: $(IMAGEN)
	../tools/prueba_qemu.sh $(IMAGEN) pruebas/arranque.txt 5

VIRT := target/kernel8-virt.img

virt: usuario FORCE
	cargo build $(CARGO_PERFIL) --features virt
	$(OBJCOPY) -O binary $(ELF) $(VIRT)

# Un disco de 8 MB para el NVMe (U6d): en el primer MB, un texto y la firma 0x55AA en el sector 0 y otro texto en el
# 1; desde el MB 1, un RedoxFS para el servicio de archivos (FS1, `tools/disco_archivos.py`). Nuevo en cada prueba: el
# cliente de U6f escribe el sector 2, y lo de una vuelta anterior no debe servir de prueba.
DISCO_NVME := target/nvme.img
REDOXFS_AR := ../archivos/redoxfs/target/release/redoxfs-ar
VER := ../archivos/herramientas/target/release/ver
$(DISCO_NVME): FORCE
	cd ../archivos/redoxfs && cargo build --release --bin redoxfs-ar --bin redoxfs-volume
	cd ../archivos/herramientas && cargo build --release
	python3 ../tools/disco_archivos.py $@ $(REDOXFS_AR)

arranque-virt: virt $(DISCO_NVME)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT)) cargo build --release
	rm -f target/captura.ppm target/sonido.wav
	GUARDAR=$(abspath target/salida_virt.txt) CAPTURA=$(abspath target/captura.ppm) TECLAS=1 MAQUINA=highmem=off QEMU_EXTRA="-device ramfb -device qemu-xhci,id=xhci -device usb-hub,bus=xhci.0,port=1 -device usb-kbd,bus=xhci.0,port=1.1 -device usb-mouse,bus=xhci.0,port=2 -drive file=$(abspath $(DISCO_NVME)),if=none,id=nv,format=raw -device nvme,serial=dashos,drive=nv -audiodev wav,id=snd0,path=$(abspath target/sonido.wav),out.frequency=48000 -device virtio-sound-pci,audiodev=snd0" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/arranque_virt.txt 60
	python3 ../tools/disco_archivos.py --comprobar $(DISCO_NVME) $(VER)
	python3 ../tools/captura_ventanas.py target/captura.ppm
	python3 ../tools/sonido_wav.py target/sonido.wav

# Los cortes del sonido con los cuatro núcleos ocupados (S19, A6): la imagen de virt con cuatro programas de carga,
# sin disco ni USB. Mide; el WAV se comprueba, pero un fallo de la comprobación no para la orden.
VIRT_CARGA := target/kernel8-virt-carga.img
sonido-carga: usuario FORCE
	cargo build $(CARGO_PERFIL) --features virt,carga
	$(OBJCOPY) -O binary $(ELF) $(VIRT_CARGA)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT_CARGA)) cargo build --release
	rm -f target/sonido-carga.wav
	GUARDAR=$(abspath target/salida_carga.txt) MAQUINA=highmem=off QEMU_EXTRA="-device ramfb -audiodev wav,id=snd0,path=$(abspath target/sonido-carga.wav),out.frequency=48000 -device virtio-sound-pci,audiodev=snd0" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/sonido_carga.txt 90
	grep -E "\[(sonido|carga)\]" target/salida_carga.txt
	-python3 ../tools/sonido_wav.py target/sonido-carga.wav

PRUEBAS_VIRT := target/kernel8-pruebas-virt.img

# Las pruebas del arranque en QEMU virt por UEFI (U3c): las que valen para las dos placas.
prueba-virt: usuario FORCE
	cargo build $(CARGO_PERFIL) --features pruebas,virt
	$(OBJCOPY) -O binary $(ELF) $(PRUEBAS_VIRT)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(PRUEBAS_VIRT)) cargo build --release
	QEMU_EXTRA="-device ramfb" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/esperado_virt.txt 120

clean:
	cargo clean

FORCE:
