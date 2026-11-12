#!/usr/bin/env python3
"""La letra de la consola del kernel en pantalla (pi5, diagnóstico): Roboto Mono (OFL 1.1, `kit/fuentes/`) pasada a
mapa de bits de 1 bit, celdas de 12 x 24, de U+0020 a U+00FF (Latin-1: las letras con tilde y la eñe).

    tools/fuente_consola.py            escribe kernel/fuentes/consola-12x24.bin

Formato: 224 glifos seguidos; cada uno, 24 filas de u16 little-endian, el bit 15 a la izquierda. Es una obra derivada de
Roboto Mono y sigue con su licencia (kit/fuentes/OFL-RobotoMono.txt). Solo hace falta volver a sacarla si cambia el tamaño.
"""
import os
import struct
import sys

from PIL import Image, ImageDraw, ImageFont

RAIZ = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ANCHO, ALTO = 12, 24


def main():
    ttf = os.path.join(RAIZ, "kit/fuentes/RobotoMono-Regular.ttf")
    salida = os.path.join(RAIZ, "kernel/fuentes/consola-12x24.bin")
    # El tamaño que hace que el avance de la letra quepa en 12 píxeles.
    tam = 20
    fuente = ImageFont.truetype(ttf, tam)
    while fuente.getlength("M") > ANCHO and tam > 8:
        tam -= 1
        fuente = ImageFont.truetype(ttf, tam)
    asc, desc = fuente.getmetrics()
    base = (ALTO - (asc + desc)) // 2 + asc
    datos = bytearray()
    for c in range(0x20, 0x100):
        img = Image.new("L", (ANCHO, ALTO), 0)
        # Los de control (U+007F a U+009F) y los que no sabe dibujar, en blanco.
        if not 0x7F <= c < 0xA0:
            try:
                x = (ANCHO - fuente.getlength(chr(c))) / 2
                ImageDraw.Draw(img).text((x, base), chr(c), fill=255, font=fuente, anchor="ls")
            except (RuntimeError, OSError):
                pass
        for y in range(ALTO):
            fila = 0
            for x in range(ANCHO):
                if img.getpixel((x, y)) >= 100:
                    fila |= 1 << (15 - x)
            datos += struct.pack("<H", fila)
    with open(salida, "wb") as f:
        f.write(datos)
    print(f"{salida}: {len(datos)} bytes, Roboto Mono a {tam} px")


if __name__ == "__main__":
    sys.exit(main())
