#!/usr/bin/env python3
"""Mira la captura de pantalla de QEMU tras la prueba del compositor Wayland (W7 de S10b).

Uso: captura_ventanas.py CAPTURA.ppm

Las dos ventanas de prueba salen en cascada: la primera en (40, 40) y la segunda en (72, 72), encima. Cuál es cuál
depende de quién se conecta antes, así que se aceptan los dos órdenes.

- ventana-a: 160 x 120, roja (0xcc3333, XRGB), con sus últimas 10 líneas blancas tras el último cuadro.
- ventana-b: 120 x 100, ARGB: cuadros de 10 x 10, verdes opacos (0x33cc33) y negros a medias (alfa 128), que dejan ver
  lo de debajo oscurecido a la mitad. Sus 10 primeras líneas no cambian: verde en las columnas pares.
"""
import sys

FONDO = (0x20, 0x40, 0x60)
ROJO = (0xCC, 0x33, 0x33)
BLANCO = (0xFF, 0xFF, 0xFF)
VERDE = (0x33, 0xCC, 0x33)


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


def mitad(c):
    """Lo de debajo, tapado por negro con alfa 128 premultiplicado (como compone el compositor)."""
    return tuple((v * 127 + 127) // 255 for v in c)


def main():
    ancho, alto, px = leer_ppm(sys.argv[1])

    def en(x, y):
        k = 3 * (y * ancho + x)
        return tuple(px[k : k + 3])

    def casi(a, b):
        return all(abs(x - y) <= 2 for x, y in zip(a, b))

    fallos = []
    if not casi(en(5, 5), FONDO):
        fallos.append(f"fondo en (5, 5): {en(5, 5)}")
    # Orden 1: ventana-a en (40, 40) debajo y ventana-b en (72, 72) encima.
    orden1 = [
        ((45, 45), ROJO),
        ((45, 40 + 115), BLANCO),
        ((75, 75), VERDE),
        ((85, 75), mitad(ROJO)),  # columna impar de b sobre a
        # b por debajo de a (y >= 160), con la fase del último cuadro (29): verde en (75, 165), negro a medias
        # sobre el fondo en (85, 165).
        ((75, 165), VERDE),
        ((85, 165), mitad(FONDO)),
    ]
    # Orden 2: ventana-b en (40, 40) debajo y ventana-a en (72, 72) encima.
    orden2 = [
        ((45, 45), VERDE),
        ((55, 45), mitad(FONDO)),
        ((75, 75), ROJO),
        ((75, 72 + 115), BLANCO),
    ]
    vale = [all(casi(en(x, y), c) for (x, y), c in o) for o in (orden1, orden2)]
    if not any(vale):
        detalle = ", ".join(f"({x}, {y}) = {en(x, y)}" for (x, y), _ in orden1 + orden2)
        fallos.append(f"las ventanas no están donde deben: {detalle}")
    if fallos:
        print("MAL: la captura del compositor:", *fallos, sep="\n  ")
        sys.exit(1)
    print(f"BIEN: la captura del compositor ({ancho}x{alto}, orden {'1' if vale[0] else '2'})")


main()
