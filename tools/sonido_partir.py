#!/usr/bin/env python3
"""Saca del WAV de `make arranque-virt-sonido` dos trozos de 4 s: antes y después de poner el ecualizador en "Graves"
(rama sonido-pro), y mide la energía de los graves (por debajo de 150 Hz) en cada uno.

Uso: sonido_partir.py SONIDO.wav GUION.txt CARPETA

La hora del clic sale del .guion.txt (segundos desde que arrancó el guion, que es casi cuando arrancó QEMU): el trozo
de antes acaba 1 s antes del clic y el de después empieza 2 s tras él, para que no caiga en la rampa del ecualizador
ni en un error de alineación de un segundo.
"""
import math
import os
import re
import struct
import sys


def leer(ruta):
    with open(ruta, "rb") as f:
        b = f.read()
    _, canales, ritmo, _, _, bits = struct.unpack("<HHIIHH", b[20:36])
    i = 12
    while i + 8 <= len(b) and b[i : i + 4] != b"data":
        i += 8 + struct.unpack("<I", b[i + 4 : i + 8])[0]
    datos = b[i + 8 :]
    datos = datos[: len(datos) - len(datos) % (2 * canales)]
    return ritmo, canales, datos


def escribir(ruta, ritmo, canales, datos):
    with open(ruta, "wb") as f:
        f.write(b"RIFF" + struct.pack("<I", 36 + len(datos)) + b"WAVE")
        f.write(b"fmt " + struct.pack("<IHHIIHH", 16, 1, canales, ritmo, ritmo * canales * 2, canales * 2, 16))
        f.write(b"data" + struct.pack("<I", len(datos)) + datos)


def graves(datos, canales, ritmo):
    """La fracción de energía por debajo de 150 Hz (filtro de un polo, en Python: lento pero corto)."""
    m = struct.unpack(f"<{len(datos) // 2}h", datos)[::canales]
    a = 1 - math.exp(-2 * math.pi * 150 / ritmo)
    y, eb, et = 0.0, 0.0, 0.0
    for x in m:
        y += a * (x - y)
        eb += y * y
        et += x * x
    return eb / et if et else 0.0


def main():
    wav, guion, carpeta = sys.argv[1:4]
    t = open(guion).read()
    clic = re.search(r"([\d.]+) s: ecualizador: Graves \(antes", t)
    if not clic:
        print("sonido_partir: el guion no llegó a poner Graves")
        return
    ritmo, canales, datos = leer(wav)
    cuadro = 2 * canales
    t0 = float(clic.group(1))
    trozos = {"antes": (t0 - 5, t0 - 1), "despues": (t0 + 2, t0 + 6)}
    os.makedirs(carpeta, exist_ok=True)
    for nombre, (a, b) in trozos.items():
        i, j = int(max(a, 0) * ritmo) * cuadro, int(b * ritmo) * cuadro
        trozo = datos[i:j]
        escribir(os.path.join(carpeta, f"ecualizador-{nombre}.wav"), ritmo, canales, trozo)
        print(f"sonido_partir: {nombre}: {len(trozo) // cuadro / ritmo:.1f} s, graves (<150 Hz) {100 * graves(trozo, canales, ritmo):.1f} % de la energía")


main()
