#!/usr/bin/env python3
"""Mira la captura del diagnóstico en pantalla (`make arranque-virt-diag`, docs/bitacoras/pi5.md): que el panel de arriba
esté pintado (su fondo gris oscuro y letra), que haya líneas en verde (lo que fue bien: la CPU con big-endian y la prueba
de verdad) y que el registro de abajo tenga texto.

    tools/captura_diag.py captura.ppm
"""
import sys


def leer_ppm(ruta):
    with open(ruta, "rb") as f:
        datos = f.read()
    partes = []
    i = 0
    while len(partes) < 4:
        while datos[i:i + 1].isspace():
            i += 1
        if datos[i:i + 1] == b"#":
            while datos[i:i + 1] != b"\n":
                i += 1
            continue
        j = i
        while not datos[j:j + 1].isspace():
            j += 1
        partes.append(datos[i:j])
        i = j
    ancho, alto = int(partes[1]), int(partes[2])
    return ancho, alto, datos[i + 1:]


def main():
    ancho, alto, px = leer_ppm(sys.argv[1])
    def color(x, y):
        k = 3 * (y * ancho + x)
        return px[k], px[k + 1], px[k + 2]
    panel = alto * 6 // 10
    fondo = verdes = letra = 0
    for y in range(0, panel, 2):
        for x in range(0, ancho, 2):
            r, g, b = color(x, y)
            if (r, g, b) == (0x20, 0x20, 0x20):
                fondo += 1
            elif g > 0xc0 and r < 0x80 and b < 0x80:
                verdes += 1
            elif r > 0xa0 and g > 0xa0 and b > 0xa0:
                letra += 1
    abajo = sum(1 for y in range(panel, alto, 2) for x in range(0, ancho, 2) if sum(color(x, y)) > 0x180)
    bien = fondo > 1000 and verdes > 100 and letra > 300 and abajo > 100
    print(f"captura del diagnóstico: panel {fondo} de fondo, {letra} de letra, {verdes} en verde; registro {abajo}: "
          f"{'BIEN' if bien else 'MAL'}")
    return 0 if bien else 1


if __name__ == "__main__":
    sys.exit(main())
