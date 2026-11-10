#!/usr/bin/env python3
"""Una tarjeta SD para medir en una Raspberry Pi 3 de verdad (P3, docs/MULTITAREA_LOOP.md).

    tools/sd_pi3.py dashos SALIDA     el firmware de la Pi, el árbol y DashOS con el banco (kernel/target/kernel8-medir-pi3.img)
    tools/sd_pi3.py linux SALIDA      el firmware, el kernel de Raspberry Pi OS y el banco de Linux como initramfs
                                      (~/.cache/dashos-linux/banco.cpio.gz, de tools/medir_linux/hacer.sh)

Deja en SALIDA lo que va en la partición FAT32 de la tarjeta: se copia tal cual. El firmware (bootcode.bin, start.elf,
fixup.dat), los árboles y el kernel de Linux se bajan del repositorio raspberrypi/firmware: no van al repo de DashOS.

La salida va por el puerto serie de la Pi (GPIO 14 y 15, 115200 baudios): hace falta un cable USB a serie de 3,3 V. Con
`dtoverlay=disable-bt` el PL011 queda en esos pines, que es donde escriben DashOS y Linux.
"""
import os
import shutil
import subprocess
import sys

RAIZ = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FIRMWARE = "https://github.com/raspberrypi/firmware/raw/stable/boot/"
CACHE = os.path.expanduser("~/.cache/dashos-linux/pi3")


def bajar(nombre):
    os.makedirs(CACHE, exist_ok=True)
    f = os.path.join(CACHE, nombre.replace("/", "_"))
    if not os.path.exists(f):
        subprocess.run(["curl", "-sSfL", "-o", f, FIRMWARE + nombre], check=True)
    return f


def preparar(que, salida):
    os.makedirs(salida, exist_ok=True)
    for n in ["bootcode.bin", "start.elf", "fixup.dat", "bcm2710-rpi-3-b.dtb", "bcm2710-rpi-3-b-plus.dtb",
              "overlays/disable-bt.dtbo"]:
        destino = os.path.join(salida, n)
        os.makedirs(os.path.dirname(destino), exist_ok=True)
        shutil.copyfile(bajar(n), destino)
    config = ["arm_64bit=1", "enable_uart=1", "dtoverlay=disable-bt", "kernel=kernel8.img"]
    if que == "dashos":
        imagen = os.path.join(RAIZ, "kernel/target/kernel8-medir-pi3.img")
        if not os.path.exists(imagen):
            sys.exit("falta kernel/target/kernel8-medir-pi3.img: make -C kernel medir-pi3 VUELTAS=0")
        shutil.copyfile(imagen, os.path.join(salida, "kernel8.img"))
    else:
        shutil.copyfile(bajar("kernel8.img"), os.path.join(salida, "kernel8.img"))
        cpio = os.path.expanduser("~/.cache/dashos-linux/banco.cpio.gz")
        if not os.path.exists(cpio):
            sys.exit("falta ~/.cache/dashos-linux/banco.cpio.gz: tools/medir_linux/hacer.sh")
        shutil.copyfile(cpio, os.path.join(salida, "banco.cpio.gz"))
        config.append("initramfs banco.cpio.gz followkernel")
        with open(os.path.join(salida, "cmdline.txt"), "w") as f:
            f.write("console=ttyAMA0,115200 rdinit=/init loglevel=3\n")
    with open(os.path.join(salida, "config.txt"), "w") as f:
        f.write("\n".join(config) + "\n")
    print(f"{salida}: copia su contenido a la partición FAT32 de una tarjeta SD")


if __name__ == "__main__":
    if len(sys.argv) != 3 or sys.argv[1] not in ("dashos", "linux"):
        print(__doc__)
        sys.exit(2)
    preparar(sys.argv[1], sys.argv[2])
