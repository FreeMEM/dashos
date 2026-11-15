#!/usr/bin/env python3
"""Mira las capturas de las dos cabezas de la virtio-gpu tras `make arranque-virt-2` (multimonitor).

Uso: captura_multimonitor.py PREFIJO

Lee PREFIJO.cabeza0.ppm y .cabeza1.ppm (al empezar), .max0.ppm y .max1.ppm (con Ajustes maximizada en la segunda) y
.vuelta0.ppm y .vuelta1.ppm (con Ajustes de vuelta en la primera), las que haya.

- La primera mide 800 x 600 y la segunda 1024 x 768.
- Al empezar, la segunda es solo fondo: ni ventanas ni la sombra de la barra de la primera en su esquina.
- Con Ajustes maximizada, la segunda es casi toda ventana (no fondo), y la primera sigue con su barra arriba.
- De vuelta, la segunda otra vez solo fondo.
"""
import os
import sys

FONDO = (0x20, 0x40, 0x60)


def leer_ppm(ruta):
    datos = open(ruta, "rb").read()
    partes = []
    i = 0
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
    i += 1
    assert partes[0] == b"P6" and partes[3] == b"255", "no es un PPM de 8 bits"
    ancho, alto = int(partes[1]), int(partes[2])
    return ancho, alto, datos[i:]


def pixel(img, x, y):
    ancho, _, d = img
    k = 3 * (y * ancho + x)
    return tuple(d[k : k + 3])


def parte_de_fondo(img, paso=8):
    ancho, alto, _ = img
    total = fondo = 0
    for y in range(0, alto, paso):
        for x in range(0, ancho, paso):
            total += 1
            fondo += pixel(img, x, y) == FONDO
    return fondo / total


def main():
    prefijo = sys.argv[1]
    fallos = []
    vistas = 0

    def mirar(nombre, ancho, alto):
        ruta = f"{prefijo}.{nombre}.ppm"
        if not os.path.exists(ruta):
            return None
        img = leer_ppm(ruta)
        if (img[0], img[1]) != (ancho, alto):
            fallos.append(f"{nombre}: mide {img[0]}x{img[1]}, no {ancho}x{alto}")
            return None
        return img

    c0, c1 = mirar("cabeza0", 800, 600), mirar("cabeza1", 1024, 768)
    if c0 and c1:
        vistas += 1
        if parte_de_fondo(c1) < 0.98:
            fallos.append(f"cabeza1: solo {parte_de_fondo(c1):.0%} de fondo al empezar")
        esquina = [pixel(c1, x, y) for y in range(0, 60) for x in range(0, 20)]
        if any(p != FONDO for p in esquina):
            fallos.append("cabeza1: la esquina de arriba a la izquierda no es fondo (¿la sombra de la barra?)")
        if pixel(c0, 400, 20) == FONDO:
            fallos.append("cabeza0: sin barra arriba")
    m0, m1 = mirar("max0", 800, 600), mirar("max1", 1024, 768)
    if m0 and m1:
        vistas += 1
        if parte_de_fondo(m1) > 0.05:
            fallos.append(f"max1: Ajustes maximizada deja {parte_de_fondo(m1):.0%} de fondo")
        if pixel(m0, 400, 20) == FONDO:
            fallos.append("max0: la primera perdió su barra")
    v0, v1 = mirar("vuelta0", 800, 600), mirar("vuelta1", 1024, 768)
    if v0 and v1:
        vistas += 1
        if parte_de_fondo(v1) < 0.98:
            fallos.append(f"vuelta1: solo {parte_de_fondo(v1):.0%} de fondo con Ajustes de vuelta")
    if vistas < 3:
        fallos.append(f"faltan capturas: {vistas} de 3 pares")
    for f in fallos:
        print("MAL:", f)
    if fallos:
        return 1
    print("BIEN: las dos cabezas (al empezar, con Ajustes maximizada en la segunda y de vuelta)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
