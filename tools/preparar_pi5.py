#!/usr/bin/env python3
"""La tarjeta (o el SSD) para el primer arranque de DashOS en una Raspberry Pi 5 (docs/PI5_PRIMER_ARRANQUE.md).

    tools/preparar_pi5.py IMAGEN_DEL_KERNEL SALIDA.img [ESCRITORIO [USB]]   (lo llama `make -C kernel tarjeta-pi5`)

Hace SALIDA.img, una imagen de disco para grabar tal cual (dd, Raspberry Pi Imager con "Use custom", balenaEtcher): una
tabla MBR con una partición FAT32 de arranque de 64 MB y en ella:

- config.txt: que el firmware cargue kernel_2712.img en 0x80000, sin comprobar que sea Linux (os_check=0), con el
  UART de depuración y el registro del firmware por él;
- kernel_2712.img: DashOS con el diagnóstico en pantalla (`make -C kernel pi5`);
- dashos-escritorio.img, si se da: DashOS con el escritorio en la pantalla del firmware (`make -C kernel pi5-escritorio`).
  Para probarlo, en config.txt se cambia la línea `kernel=`; con teclado y ratón USB, si el USB va;
- dashos-usb.img, si se da: DashOS con el diagnóstico del USB (el PCIe del RP1, el RP1, sus dos xHCI, los aparatos y
  las teclas) en pantalla (`make -C kernel pi5-usb`). También se elige con la línea `kernel=`;
- los árboles de dispositivos oficiales de la Pi 5 (bcm2712-rpi-5-b.dtb y, para las placas con el chip D0,
  bcm2712d0-rpi-5-b.dtb), bajados de github.com/raspberrypi/firmware (rama stable). No van en git: se bajan aquí, a
  ~/.cache/dashos-pi5/firmware-stable. El firmware de la Pi 5 está en su EEPROM: en la tarjeta no hace falta start*.elf;
- LEEME.txt: qué es esto, para quien abra la tarjeta.

Nada de esto toca la EEPROM de la Pi. Sin root: la FAT se hace con mkfs.fat y mtools en un fichero.
"""
import os
import shutil
import subprocess
import sys
import tempfile

FIRMWARE = "https://github.com/raspberrypi/firmware/raw/stable/boot/"
CACHE = os.path.expanduser("~/.cache/dashos-pi5/firmware-stable")
ARBOLES = ["bcm2712-rpi-5-b.dtb", "bcm2712d0-rpi-5-b.dtb"]
MB = 1 << 20
TAM_FAT = 64 * MB

CONFIG = """\
# DashOS: primer arranque en la Raspberry Pi 5 (docs/PI5_PRIMER_ARRANQUE.md del repositorio de DashOS).
# El firmware de la EEPROM lee este fichero; nada de aquí cambia la EEPROM.
[pi5]
# El diagnóstico (lo primero). Para el diagnóstico del USB (teclado y ratón), cambiar esta línea por
# kernel=dashos-usb.img
# y para el escritorio, por
# kernel=dashos-escritorio.img
kernel=kernel_2712.img
arm_64bit=1
# DashOS está enlazado para 0x80000 (si el firmware lo deja en otro sitio, DashOS se copia allí él solo).
kernel_address=0x80000
# No es un kernel de Linux: que el firmware no lo compruebe.
os_check=0
# El puerto serie de depuración (el conector de 3 patillas entre los dos HDMI), a 115200 baudios, con el registro del
# propio firmware. No hace falta para la pantalla.
enable_uart=1
uart_2ndstage=1
# La pantalla la da el firmware (DashOS se la pide por el buzón): sin el controlador de Linux, y aunque el monitor tarde
# en decir que está.
disable_fw_kms_setup=1
hdmi_force_hotplug=1
# Que el firmware no reinicie el PCIe del RP1 (el chip del USB) al saltar a DashOS: DashOS usa el enlace y las ventanas
# que dejó el firmware (documentado en config.txt de Raspberry Pi, "pciex4_reset"). No persiste: solo vale esta vez.
pciex4_reset=0
[all]
"""

LEEME = """\
DashOS para la Raspberry Pi 5: primer arranque (diagnostico en pantalla).

Esta tarjeta no tiene Linux. El firmware de la Pi 5 carga kernel_2712.img (DashOS), que escribe en la pantalla (HDMI 0,
el que esta junto a la toma de corriente USB-C) lo que ve de la maquina: CPU, si admite big-endian, memoria y la caja
Amiga. Hacer una foto de la pantalla entera.

Para el diagnostico del USB (teclado y raton): en config.txt, kernel=dashos-usb.img en lugar de
kernel=kernel_2712.img. Con un teclado y un raton USB enchufados. Foto de la pantalla; luego pulsar teclas y otra foto.

Para probar el escritorio: en config.txt, kernel=dashos-escritorio.img.

Nada de esto cambia la EEPROM de la Pi. Para volver a lo de antes, quitar la tarjeta.
Los .dtb son los oficiales de Raspberry Pi (github.com/raspberrypi/firmware).
"""


def bajar(nombre):
    os.makedirs(CACHE, exist_ok=True)
    f = os.path.join(CACHE, nombre)
    if not os.path.exists(f) or os.path.getsize(f) == 0:
        subprocess.run(["curl", "-sSfL", "-o", f + ".parte", FIRMWARE + nombre], check=True)
        os.replace(f + ".parte", f)
    with open(f, "rb") as d:
        if d.read(4) != b"\xd0\x0d\xfe\xed":
            sys.exit(f"{f}: no es un árbol de dispositivos (bórralo y vuelve a probar)")
    return f


def main():
    if len(sys.argv) not in (3, 4, 5):
        sys.exit(__doc__)
    kernel, salida = sys.argv[1], os.path.expanduser(sys.argv[2])
    escritorio = sys.argv[3] if len(sys.argv) >= 4 else None
    usb = sys.argv[4] if len(sys.argv) == 5 else None
    if escritorio and not os.path.exists(escritorio):
        sys.exit(f"falta {escritorio}: make -C kernel pi5-escritorio")
    if usb and not os.path.exists(usb):
        sys.exit(f"falta {usb}: make -C kernel pi5-usb")
    if not os.path.exists(kernel):
        sys.exit(f"falta {kernel}: make -C kernel pi5")
    for programa in ("mkfs.fat", "mcopy", "sfdisk"):
        if shutil.which(programa) is None:
            sys.exit(f"falta {programa} (dosfstools, mtools, util-linux)")
    arboles = [bajar(n) for n in ARBOLES]
    os.makedirs(os.path.dirname(os.path.abspath(salida)), exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        for nombre, texto in (("config.txt", CONFIG), ("LEEME.txt", LEEME)):
            with open(os.path.join(tmp, nombre), "w", newline="\r\n") as f:
                f.write(texto)
        shutil.copyfile(kernel, os.path.join(tmp, "kernel_2712.img"))
        fat = os.path.join(tmp, "fat.img")
        subprocess.run(["mkfs.fat", "-F", "32", "-n", "DASHOS", "-C", fat, str(TAM_FAT // 1024)], check=True,
                       stdout=subprocess.DEVNULL)
        ficheros = [os.path.join(tmp, n) for n in ("config.txt", "kernel_2712.img", "LEEME.txt")] + arboles
        if escritorio:
            shutil.copyfile(escritorio, os.path.join(tmp, "dashos-escritorio.img"))
            ficheros.append(os.path.join(tmp, "dashos-escritorio.img"))
        if usb:
            shutil.copyfile(usb, os.path.join(tmp, "dashos-usb.img"))
            ficheros.append(os.path.join(tmp, "dashos-usb.img"))
        subprocess.run(["mcopy", "-i", fat] + ficheros + ["::"], check=True)
        # El disco: 1 MB para la tabla y la FAT detrás, de tipo 0x0c (FAT32 con LBA) y marcada de arranque.
        parcial = salida + ".parte"
        with open(parcial, "wb") as d:
            d.truncate(MB + TAM_FAT)
        subprocess.run(["sfdisk", "--quiet", parcial], input=f"label: dos\nstart=2048, size={TAM_FAT // 512}, type=c, bootable\n",
                       text=True, check=True)
        with open(parcial, "r+b") as d, open(fat, "rb") as f:
            d.seek(MB)
            shutil.copyfileobj(f, d)
        os.replace(parcial, salida)
    listado = subprocess.run(["mdir", "-i", f"{salida}@@{MB}", "-b", "::"], capture_output=True, text=True).stdout.split()
    print(f"{salida}: {os.path.getsize(salida) // MB} MB; en la FAT: {', '.join(os.path.basename(x) for x in listado)}")
    print("Grabar en una microSD (lo más seguro), por ejemplo:")
    print(f"  sudo dd if={salida} of=/dev/sdX bs=4M conv=fsync status=progress   (sdX: la tarjeta, mirar con lsblk)")


if __name__ == "__main__":
    main()
