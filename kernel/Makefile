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
	cd ../usuario && cargo build --release --features archivos,ventanas,sonido,imagenes,red
	cd ../usuario && cargo build --release --target aarch64_be-unknown-none-softfloat --bin hola --bin fpu_b
	$(BOOTLIN)/aarch64_be-linux-as -EL -o $(PIE).o ../usuario/asm/pie.s
	$(BOOTLIN)/aarch64_be-linux-ld -EL -pie --no-dynamic-linker -z noexecstack -z stack-size=32768 -o $(PIE) $(PIE).o

$(PRUEBAS): usuario FORCE
	cargo build $(CARGO_PERFIL) --features pruebas
	$(OBJCOPY) -O binary $(ELF) $(PRUEBAS)

# Antes de QEMU, las pruebas de PC de las bibliotecas de los servicios (B6): Wayland y la mezcla del sonido.
prueba: $(PRUEBAS)
	cd ../wayland && cargo test --quiet
	cd ../sonido && cargo test --quiet
	cd ../amigados && cargo test --quiet --release
	cd ../archivos/herramientas && cargo test --quiet --release
	cd ../kit && cargo test --quiet
	cd ../usuario && mkdir -p target && rustc --edition 2024 --test src/panel_mosaicos.rs -o target/panel_mosaicos_pruebas && ./target/panel_mosaicos_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/lanzables.rs -o target/lanzables_pruebas && ./target/lanzables_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/protocolo/avisos.rs -o target/avisos_pruebas && ./target/avisos_pruebas -q
	../puente/pruebas/g0.sh
	../puente/pruebas/g1.sh
	../puente/pruebas/g4.sh && ../puente/pruebas/g5.sh && ../puente/pruebas/g6.sh
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

# La red (S13): una tarjeta virtio-net en un concentrador con la red de usuario de QEMU (DHCP, 10.0.2.15; el eco del PC
# en 10.0.2.100:7, `tools/red_eco.py`, lanzado por QEMU para cada conexión) y con `tools/red_basura.py`, que manda
# tramas malas por un socket Unix durante 20 s desde el primer TCP de DashOS (se para también al acabar la prueba).
RED := $(abspath target/red)
RED_MAC := 52:54:00:12:34:56
RED_QEMU := -netdev user,id=red0,guestfwd=tcp:10.0.2.100:7-cmd:$(abspath ../tools/red_eco.py) -netdev dgram,id=red1,local.type=unix,local.path=$(RED)/qemu.sock,remote.type=unix,remote.path=$(RED)/pc.sock -netdev hubport,id=red2,hubid=0,netdev=red0 -netdev hubport,id=red3,hubid=0,netdev=red1 -netdev hubport,id=red4,hubid=0 -device virtio-net-pci,netdev=red4,disable-legacy=on,mac=$(RED_MAC),romfile=

arranque-virt: virt $(DISCO_NVME)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT)) cargo build --release
	rm -f target/captura.ppm target/sonido.wav $(RED)/qemu.sock $(RED)/pc.sock target/u6.ana.ppm target/u6.carla.ppm
	mkdir -p $(RED)
	python3 -I ../tools/red_basura.py $(RED) $(RED_MAC) 300 & BASURA=$$!; \
	GUARDAR=$(abspath target/salida_virt.txt) CAPTURA=$(abspath target/captura.ppm) CAPTURA_U6=$(abspath target/u6) TECLAS=1 MAQUINA=highmem=off QEMU_EXTRA="-device ramfb -device qemu-xhci,id=xhci -device usb-hub,bus=xhci.0,port=1 -device usb-kbd,bus=xhci.0,port=1.1 -device usb-mouse,bus=xhci.0,port=2 -drive file=$(abspath $(DISCO_NVME)),if=none,id=nv,format=raw,discard=unmap -device nvme,serial=dashos,drive=nv -audiodev wav,id=snd0,path=$(abspath target/sonido.wav),out.frequency=48000 -device virtio-sound-pci,audiodev=snd0 $(RED_QEMU)" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/arranque_virt.txt 200; \
	R=$$?; kill $$BASURA 2>/dev/null; exit $$R
	python3 ../tools/disco_archivos.py --comprobar $(DISCO_NVME) $(VER)
	python3 ../tools/captura_ventanas.py target/captura.ppm
	python3 ../tools/sonido_wav.py target/sonido.wav
	python3 ../tools/captura_cambio.py target/u6.ana.ppm target/u6.carla.ppm

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

# audio.device nativo (S19, fase 2): la imagen de virt con `paula-prueba` en lugar de los dos tonos, sin disco ni
# USB. El WAV se comprueba con `sonido_wav.py --paula`.
VIRT_PAULA := target/kernel8-virt-paula.img
sonido-paula: usuario FORCE
	cargo build $(CARGO_PERFIL) --features virt,paula
	$(OBJCOPY) -O binary $(ELF) $(VIRT_PAULA)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT_PAULA)) cargo build --release
	rm -f target/sonido-paula.wav
	GUARDAR=$(abspath target/salida_paula.txt) MAQUINA=highmem=off QEMU_EXTRA="-device ramfb -audiodev wav,id=snd0,path=$(abspath target/sonido-paula.wav),out.frequency=48000 -device virtio-sound-pci,audiodev=snd0" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/sonido_paula.txt 60
	python3 ../tools/sonido_wav.py --paula target/sonido-paula.wav

# El banco de medidas de la multitarea (M0, docs/MULTITAREA_LOOP.md): la imagen de raspi4b con solo la consola y el
# orquestador `medir`, y los hijos de prueba en Rust, C y Dash. VUELTAS arranques; medianas en target/medir.json.
MEDIR := target/kernel8-medir.img
VUELTAS ?= 7
medir: usuario FORCE
	python3 ../tools/medir.py paquete ../usuario/target/medir.paq
	cargo build $(CARGO_PERFIL) --features medir
	$(OBJCOPY) -O binary $(ELF) $(MEDIR)
	python3 ../tools/medir.py correr $(MEDIR) target/medir.json -n $(VUELTAS)

# Paso F (docs/MULTITAREA_LOOP.md): el banco del sistema de archivos en QEMU virt, con un disco de 1 + 64 MB con un
# RedoxFS vacío, nuevo en cada vuelta. Linux, con `tools/medir.py linux-fs`.
MEDIR_FS := target/kernel8-medir-fs.img
medir-fs: usuario FORCE
	cd ../archivos/redoxfs && cargo build --release --bin redoxfs-ar
	cd ../archivos/herramientas && cargo build --release
	python3 ../tools/medir.py disco-fs target/medir-fs.img $(REDOXFS_AR)
	cargo build $(CARGO_PERFIL) --features virt,medir_fs
	$(OBJCOPY) -O binary $(ELF) $(MEDIR_FS)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(MEDIR_FS)) cargo build --release
	python3 ../tools/medir.py dashos-fs ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi target/medir-fs.img target/medir-fs.json -n $(VUELTAS)

# P3 (docs/MULTITAREA_LOOP.md): el mismo banco en la Raspberry Pi 3 (QEMU raspi3b, o una de verdad con
# `tools/sd_pi3.py`).
MEDIR_PI3 := target/kernel8-medir-pi3.img
medir-pi3: usuario FORCE
	python3 ../tools/medir.py paquete ../usuario/target/medir.paq
	cargo build $(CARGO_PERFIL) --features pi3,medir
	$(OBJCOPY) -O binary $(ELF) $(MEDIR_PI3)
	python3 ../tools/medir.py pi3 $(MEDIR_PI3) target/medir-pi3.json -n $(VUELTAS)

PRUEBAS_VIRT := target/kernel8-pruebas-virt.img

# Las pruebas del arranque en QEMU virt por UEFI (U3c): las que valen para las dos placas. Con GIC v3 (y su ITS): el
# v2 ya lo prueban `make prueba` (la Pi) y `make arranque-virt`.
prueba-virt: usuario FORCE
	cargo build $(CARGO_PERFIL) --features pruebas,virt
	$(OBJCOPY) -O binary $(ELF) $(PRUEBAS_VIRT)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(PRUEBAS_VIRT)) cargo build --release
	GIC=3 QEMU_EXTRA="-device ramfb" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/esperado_virt.txt 120

clean:
	cargo clean

FORCE:
