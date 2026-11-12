#!/usr/bin/env python3
"""U6, paso 2 (`docs/diseño/U6-cambio-rapido.md`): la ventana de una sesión de detrás no se ve.

Uso: captura_cambio.py ANA.ppm CARLA.ppm

`cambio`, lanzado como Ana, abre una ventana de 240 x 160 de un color que no tiene nadie más (0xe010b0). El guion de
QEMU captura la pantalla con Ana delante (ANA.ppm: la ventana tiene que verse) y con Carla delante y Ana detrás
(CARLA.ppm: no puede quedar ni un píxel de ese color).
"""
import sys

COLOR = (0xE0, 0x10, 0xB0)


def leer_ppm(ruta):
    """Un PPM de 8 bits (P6), como lo guarda `screendump` de QEMU: ancho, alto y los píxeles RGB."""
    datos = open(ruta, "rb").read()
    partes, i = [], 0
    while len(partes) < 4:
        while datos[i : i + 1].isspace():
            i += 1
        if datos[i : i + 1] == b"#":
            while datos[i : i + 1] != b"\n":
                i += 1
            continue
        j = i
        while not datos[j : j + 1].isspace():
            j += 1
        partes.append(datos[i:j])
        i = j
    assert partes[0] == b"P6" and partes[3] == b"255", "no es un PPM de 8 bits"
    return int(partes[1]), int(partes[2]), datos[i + 1 :]


def cuantos(ruta):
    ancho, alto, px = leer_ppm(ruta)
    n = 0
    for k in range(0, 3 * ancho * alto, 3):
        if abs(px[k] - COLOR[0]) <= 2 and abs(px[k + 1] - COLOR[1]) <= 2 and abs(px[k + 2] - COLOR[2]) <= 2:
            n += 1
    return n


def main():
    try:
        ana, carla = cuantos(sys.argv[1]), cuantos(sys.argv[2])
    except OSError as e:
        print(f"MAL: la ventana de cambio: falta una captura ({e})")
        sys.exit(1)
    fallos = []
    # La ventana entera, salvo lo que tape el cursor (o una sombra en el borde).
    if ana < 240 * 160 * 9 // 10:
        fallos.append(f"con Ana delante, su ventana se ve en {ana} píxeles, no en unos {240 * 160}")
    if carla != 0:
        fallos.append(f"con Carla delante, la ventana de Ana se ve en {carla} píxeles")
    if fallos:
        print("MAL: la ventana de cambio:")
        for f in fallos:
            print("  " + f)
        sys.exit(1)
    print(f"BIEN: la ventana de Ana ({ana} píxeles con ella delante, ninguno con Carla delante)")


if __name__ == "__main__":
    main()
