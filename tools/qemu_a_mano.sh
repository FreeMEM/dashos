#!/usr/bin/env bash
# DashOS en QEMU con ventana, para usarlo a mano (no es una prueba: nada comprueba la salida).
#
#   tools/qemu_a_mano.sh [ANCHOxALTO]        (por defecto 1920x1080)
#
# Compila la imagen de `virt` y el cargador UEFI de este árbol, copia el disco de prueba (el original no se toca) y
# abre QEMU en una ventana. El ratón es una tableta USB (posición absoluta): va donde esté el del PC, sin capturarlo.
# Cerrar la ventana lo acaba.
# Lo que dice DashOS por el puerto serie queda en target/a-mano/serie.txt.
#
# Ojo: de momento es la imagen de las pruebas automáticas (lanza sus programas de prueba y bloquea la pantalla a los
# pocos segundos). Usuarios: Ana ("clave de Ana") y Carla ("clave de Carla").
set -eu
cd "$(dirname "$0")/../kernel"
PANTALLA=${1:-1920x1080}
make virt target/nvme.img
NUCLEO=$(realpath target/kernel8-virt.img)
(cd ../uefi && DASHOS_PANTALLA="$PANTALLA" DASHOS_NUCLEO="$NUCLEO" cargo build --release)
D=target/a-mano
rm -rf "$D"
mkdir -p "$D/arranque/EFI/BOOT"
cp ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi "$D/arranque/EFI/BOOT/BOOTAA64.EFI"
cp --reflink=auto target/nvme.img "$D/disco.img"
exec qemu-system-aarch64 -name DashOS -M virt,gic-version=2,acpi=off,highmem=off -cpu cortex-a72 -smp 4 -m 2G \
    -bios /usr/share/edk2/aarch64/QEMU_EFI.fd -drive file=fat:rw:"$D/arranque",format=raw,if=virtio \
    -display gtk,zoom-to-fit=on -serial file:"$D/serie.txt" -device ramfb \
    -device qemu-xhci,id=xhci -device usb-hub,bus=xhci.0,port=1 -device usb-kbd,bus=xhci.0,port=1.1 \
    -device usb-tablet,bus=xhci.0,port=2 \
    -drive file="$D/disco.img",if=none,id=nv,format=raw,discard=unmap -device nvme,serial=dashos,drive=nv \
    -audiodev pipewire,id=snd0 -device virtio-sound-pci,audiodev=snd0 \
    -netdev user,id=red0 -device virtio-net-pci,netdev=red0,disable-legacy=on,mac=52:54:00:12:34:56,romfile=
