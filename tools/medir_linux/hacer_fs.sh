#!/usr/bin/env bash
# Paso F (docs/MULTITAREA_LOOP.md): initramfs con el banco de ficheros de Linux como /init, para el kernel genérico de
# Debian arm64 en QEMU virt (nada de esto va al repo: ~/.cache/dashos-linux/virt). Lleva los módulos de NVMe y ext4
# sacados del paquete del kernel, y deja un disco ext4 de 64 MB.
set -eu
AQUI=$(cd "$(dirname "$0")" && pwd)
V=${CACHE:-$HOME/.cache/dashos-linux}/virt
CC=${CC:-$HOME/.local/opt/toolchains/aarch64--musl--stable-2026.08-1/bin/aarch64-buildroot-linux-musl-gcc}
VER=6.12.107+deb13-arm64
mkdir -p "$V"
[ -f "$V/linux" ] || curl -sSfL -o "$V/linux" https://deb.debian.org/debian/dists/stable/main/installer-arm64/current/images/netboot/debian-installer/arm64/linux
if [ ! -d "$V/deb/usr/lib/modules/$VER" ]; then
    curl -sSfL -o "$V/k.deb" "https://deb.debian.org/debian/pool/main/l/linux-signed-arm64/linux-image-${VER}_6.12.107-1_arm64.deb"
    mkdir -p "$V/deb" && (cd "$V/deb" && ar x ../k.deb && tar xf data.tar.*)
fi
RAIZ=$(mktemp -d)
mkdir -p "$RAIZ/proc" "$RAIZ/dev" "$RAIZ/mnt" "$RAIZ/m"
for m in nvme-auth nvme-core nvme crc16 mbcache jbd2 crc32c_generic libcrc32c ext4; do
    cp "$(find "$V/deb/usr/lib/modules/$VER" -name "$m.ko.xz" | head -1)" "$RAIZ/m/"
done
"$CC" -O2 -static -o "$RAIZ/init" "$AQUI/fs.c"
(cd "$RAIZ" && find . | cpio -o -H newc --quiet | gzip -9) > "$V/fs.cpio.gz"
rm -rf "${RAIZ:?}"
rm -f "$V/ext4.img"
truncate -s 64M "$V/ext4.img"
mkfs.ext4 -q -F "$V/ext4.img"
echo "$V/fs.cpio.gz"
