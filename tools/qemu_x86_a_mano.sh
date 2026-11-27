#!/usr/bin/env bash
# DashOS de x86-64 en QEMU q35 con ventana, para usarlo a mano (no es una prueba: nada comprueba la salida). Lo lanza
# `make -C kernel x86-uso`, que antes compila la imagen de usar de x86 y su disco; como `tools/qemu_a_mano.sh` en ARM.
#
# Variables:
#   ACCEL=kvm          con KVM y la CPU del PC (lo pone el Makefile si hay /dev/kvm); vacío, TCG con -cpu max (lento).
#   PANTALLA=AxB       el modo del GOP que pide el cargador (por omisión 1920x1080; OVMF da los de la VGA de QEMU).
#   VENTANA=gtk|sdl    la ventana de QEMU (por omisión gtk); none, sin ventana (para capturas por QMP).
#   GTK_AJUSTAR=on     con GTK, la imagen se escala a la ventana (off: la ventana mide lo que la pantalla).
#   SONIDO_X86=hda     Intel HDA con hda-duplex (por omisión) o virtio (virtio-sound).
#   GPU_X86=virtio     la tarjeta gráfica: virtio (por omisión), `virtio-vga`, la virtio-gpu con la VGA de siempre
#                      delante (el GOP de OVMF va por ella; DashOS pasa a la virtio-gpu, que no se ve a medias); vga, la
#                      VGA de QEMU sola (solo el GOP, como un PC sin controlador de su tarjeta).
#   AUDIO=pipewire     el sonido del PC (por omisión pipewire; none, en silencio).
#   QMP=ruta           un socket de QMP en esa ruta (para capturas y entrada por guion, `tools/x86_uso_capturas.py`).
#
# El ratón es una tableta USB (posición absoluta): va donde esté el del PC, sin capturarlo. El disco es una copia del de
# serie (`target/nvme-x86.img`): lo que se haga en DashOS se pierde al volver a lanzarlo. Lo que dice DashOS por el puerto
# serie queda en kernel/target/x86-uso/serie.txt. Usuarios: Ana ("clave de Ana") y Carla ("clave de Carla").
# Sin la caja Linux ni la caja Amiga: aún no van en x86.
set -eu
cd "$(dirname "$0")/../kernel"
PANTALLA=${PANTALLA:-1920x1080}
NUCLEO=$(realpath target/kernel-x86-uso.elf)
# El cargador UEFI con la imagen dentro; el modo de pantalla va en él al compilar (DASHOS_PANTALLA).
(cd ../uefi && DASHOS_PANTALLA="$PANTALLA" DASHOS_NUCLEO="$NUCLEO" cargo build --release --target x86_64-unknown-uefi)
D=target/x86-uso
rm -rf "$D"
mkdir -p "$D/arranque/EFI/BOOT"
cp ../uefi/target/x86_64-unknown-uefi/release/dashos-uefi.efi "$D/arranque/EFI/BOOT/BOOTX64.EFI"
cp --reflink=auto target/nvme-x86.img "$D/disco.img"
OVMF=${OVMF:-/usr/share/edk2/x64/OVMF_CODE.4m.fd}
cp "${OVMF_VARS:-/usr/share/edk2/x64/OVMF_VARS.4m.fd}" "$D/vars.fd"
if [ "${ACCEL:-}" = kvm ]; then
    CPU="-accel kvm -cpu host"
else
    CPU="-cpu max"
fi
case "${VENTANA:-gtk}" in
    gtk) PANTALLA_QEMU="-display gtk,zoom-to-fit=${GTK_AJUSTAR:-on}" ;;
    sdl) PANTALLA_QEMU="-display sdl" ;;
    none) PANTALLA_QEMU="-display none" ;;
    *) echo "VENTANA tiene que ser gtk, sdl o none" >&2; exit 1 ;;
esac
W=${PANTALLA%x*}
H=${PANTALLA#*x}
case "${GPU_X86:-virtio}" in
    virtio) GPU="-vga none -device virtio-vga,xres=$W,yres=$H" ;;
    vga) GPU="-vga std" ;;
    *) echo "GPU_X86 tiene que ser virtio o vga" >&2; exit 1 ;;
esac
case "${SONIDO_X86:-hda}" in
    hda) SONIDO="-device intel-hda -device hda-duplex,audiodev=snd0" ;;
    virtio) SONIDO="-device virtio-sound-pci,audiodev=snd0" ;;
    *) echo "SONIDO_X86 tiene que ser hda o virtio" >&2; exit 1 ;;
esac
# shellcheck disable=SC2086 # CPU, PANTALLA_QEMU, GPU y SONIDO son varias opciones de QEMU.
exec qemu-system-x86_64 -name DashOS -M q35 $CPU -smp 4 -m 2G \
    -drive if=pflash,format=raw,readonly=on,file="$OVMF" -drive if=pflash,format=raw,file="$D/vars.fd" \
    -drive file=fat:rw:"$D/arranque",format=raw,if=virtio \
    $PANTALLA_QEMU $GPU -serial file:"$D/serie.txt" ${QMP:+-qmp unix:"$QMP",server,nowait} \
    -device qemu-xhci,id=xhci -device usb-hub,bus=xhci.0,port=1 -device usb-kbd,bus=xhci.0,port=1.1 \
    -device usb-tablet,bus=xhci.0,port=2 \
    -drive file="$D/disco.img",if=none,id=nv,format=raw,discard=unmap -device nvme,serial=dashos,drive=nv \
    -audiodev "${AUDIO:-pipewire}",id=snd0 $SONIDO \
    -netdev user,id=red0 -device virtio-net-pci,netdev=red0,disable-legacy=on,mac=52:54:00:12:34:56,romfile=
