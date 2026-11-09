#!/usr/bin/env bash
# Paso L (docs/MULTITAREA_LOOP.md): initramfs con el banco de Linux como /init, para el kernel de Raspberry Pi OS
# (kernel8.img del repositorio raspberrypi/firmware, no va al repo: se baja a ~/.cache/dashos-linux).
#   tools/medir_linux/hacer.sh            deja ~/.cache/dashos-linux/banco.cpio.gz
# Necesita un compilador de aarch64 para Linux con musl (Bootlin, $CC).
set -eu
AQUI=$(cd "$(dirname "$0")" && pwd)
CACHE=${CACHE:-$HOME/.cache/dashos-linux}
CC=${CC:-$HOME/.local/opt/toolchains/aarch64--musl--stable-2026.08-1/bin/aarch64-buildroot-linux-musl-gcc}
mkdir -p "$CACHE"
for f in kernel8.img bcm2711-rpi-4-b.dtb; do
    [ -f "$CACHE/$f" ] || curl -sSfL -o "$CACHE/$f" "https://raw.githubusercontent.com/raspberrypi/firmware/stable/boot/$f"
done
# El PL011 lo coge el Bluetooth (serdev) y no queda consola: una copia del árbol sin ese nodo.
cp "$CACHE/bcm2711-rpi-4-b.dtb" "$CACHE/banco.dtb"
fdtput -r "$CACHE/banco.dtb" /soc/serial@7e201000/bluetooth
RAIZ=$(mktemp -d)
mkdir -p "$RAIZ/proc" "$RAIZ/dev"
"$CC" -O2 -static -o "$RAIZ/init" "$AQUI/banco.c"
(cd "$RAIZ" && find . | cpio -o -H newc --quiet | gzip -9) > "$CACHE/banco.cpio.gz"
rm -rf "${RAIZ:?}"
echo "$CACHE/banco.cpio.gz"
