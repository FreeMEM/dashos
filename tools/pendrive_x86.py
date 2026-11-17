#!/usr/bin/env python3
"""S32: la imagen del pendrive en vivo de DashOS para un PC (x86-64, UEFI).

    pendrive_x86.py <salida.img> <cargador.efi> <disco de serie> [--sin-manos <fichero>]

Un disco GPT con una sola partición, la de arranque EFI (FAT32), con:
  EFI/BOOT/BOOTX64.EFI   el cargador de DashOS (con el kernel dentro), el que arranca el firmware de un pendrive;
  EFI/DashOS/VIVO.IMG    el sistema: el DashFS del disco de serie (`tools/disco_archivos.py`), que el cargador lee
                         entero a la memoria. DashOS arranca de él en vivo: lo que se cambia queda en la memoria y se
                         pierde al apagar; no toca los discos del PC (solo el instalador, y con permiso).
  EFI/DashOS/LEEME.TXT   qué es esto, en llano.
Con --sin-manos, el fichero va como `EFI/DashOS/SINMANOS.TXT`: un plan del instalador (o un pedido: "disco <nombre>"
y "pedido <...>") que el instalador aplica nada más arrancar, sin preguntar (para las pruebas del banco,
`tools/prueba_instalador.py`).

Usa mtools (mformat, mmd, mcopy) sobre la imagen, sin montar nada.
"""
import os
import struct
import subprocess
import sys
import tempfile
import uuid
import zlib

SECTOR = 512
MIB = 1 << 20
ESP = uuid.UUID("C12A7328-F81F-11D2-BA4B-00A0C93EC93B")


def gpt(sectores, particiones, disco_guid):
    """El MBR protector y las dos GPT (principio y final) para `particiones`: [(tipo, guid, desde, hasta, nombre)]."""
    lista = bytearray(128 * 128)
    for k, (tipo, guid, desde, hasta, nombre) in enumerate(particiones):
        e = tipo.bytes_le + guid.bytes_le + struct.pack("<QQQ", desde, hasta - 1, 0) + nombre.encode("utf-16-le").ljust(72, b"\0")
        lista[k * 128:(k + 1) * 128] = e
    crc_lista = zlib.crc32(lista)
    sl = len(lista) // SECTOR

    def cabecera(propio, otro, lista_lba):
        c = bytearray(struct.pack("<8sIIIIQQQQ16sQIII", b"EFI PART", 0x10000, 92, 0, 0, propio, otro, 2048, sectores - 2 - sl, disco_guid.bytes_le, lista_lba, 128, 128, crc_lista))
        c[16:20] = struct.pack("<I", zlib.crc32(c[:92]))
        return bytes(c).ljust(SECTOR, b"\0")

    mbr = bytearray(SECTOR)
    mbr[446:462] = bytes([0, 0, 2, 0, 0xEE, 0xFF, 0xFF, 0xFF]) + struct.pack("<II", 1, min(sectores - 1, 0xFFFFFFFF))
    mbr[510:512] = b"\x55\xaa"
    principio = bytes(mbr) + cabecera(1, sectores - 1, 2) + bytes(lista)
    final = bytes(lista) + cabecera(sectores - 1, 1, sectores - 1 - sl)
    return principio, final


def dashfs_de(disco):
    """El DashFS del disco de serie: la partición de DashOS de su MBR (tipo 0x7F)."""
    with open(disco, "rb") as f:
        s0 = f.read(SECTOR)
        for k in range(4):
            e = s0[446 + 16 * k:446 + 16 * (k + 1)]
            if e[4] == 0x7F:
                desde, n = struct.unpack("<II", e[8:16])
                f.seek(desde * SECTOR)
                return f.read(n * SECTOR)
    sys.exit(f"{disco}: no tiene partición de DashOS (MBR, tipo 0x7F)")


LEEME = """DashOS en vivo

Este pendrive arranca DashOS sin instalarlo: lo que hagas se queda en la
memoria y se pierde al apagar. No toca los discos del ordenador.

Para instalarlo, abre "Instalar DashOS" en el escritorio (o la orden
Instalar en el Shell). Te enseña lo que va a hacer, con un dibujo del disco
antes y después, y no escribe nada hasta que lo confirmes. No borra otros
sistemas: usa el espacio libre o encoge Windows, y añade DashOS al menú de
arranque del ordenador sin quitar a nadie.
"""


def main():
    args = sys.argv[1:]
    sin_manos = None
    if "--sin-manos" in args:
        i = args.index("--sin-manos")
        sin_manos = args[i + 1]
        del args[i:i + 2]
    if len(args) != 3:
        sys.exit(__doc__)
    salida, cargador, disco = args
    sistema = dashfs_de(disco)
    with tempfile.TemporaryDirectory() as t:
        tam_esp = max(64 * MIB, (len(sistema) + os.path.getsize(cargador) + 16 * MIB + MIB - 1) // MIB * MIB)
        sectores = (tam_esp + 2 * MIB) // SECTOR
        desde, hasta = 2048, 2048 + tam_esp // SECTOR
        principio, final = gpt(sectores, [(ESP, uuid.uuid4(), desde, hasta, "DashOS en vivo")], uuid.uuid4())
        with open(salida, "wb") as f:
            f.truncate(sectores * SECTOR)
            f.write(principio)
            f.seek(sectores * SECTOR - len(final))
            f.write(final)
        destino = f"{salida}@@{desde * SECTOR}"
        tam_k = tam_esp // 1024
        subprocess.run(["mformat", "-i", destino, "-F", "-T", str(tam_esp // SECTOR), "-h", "64", "-s", "32", "-v", "DASHOS", "::"], check=True)
        leeme = os.path.join(t, "LEEME.TXT")
        with open(leeme, "w", newline="\r\n") as f:
            f.write(LEEME)
        img = os.path.join(t, "VIVO.IMG")
        with open(img, "wb") as f:
            f.write(sistema)
        for d in ("::EFI", "::EFI/BOOT", "::EFI/DashOS"):
            subprocess.run(["mmd", "-i", destino, d], check=True)
        ficheros = [(cargador, "::EFI/BOOT/BOOTX64.EFI"), (img, "::EFI/DashOS/VIVO.IMG"), (leeme, "::EFI/DashOS/LEEME.TXT")]
        if sin_manos is not None:
            ficheros.append((sin_manos, "::EFI/DashOS/SINMANOS.TXT"))
        for de, a in ficheros:
            subprocess.run(["mcopy", "-i", destino, de, a], check=True)
        print(f"{salida}: pendrive en vivo de {sectores * SECTOR // MIB} MB (sistema de {len(sistema) // 1024} KB, ESP de {tam_k // 1024} MB)")


if __name__ == "__main__":
    main()
