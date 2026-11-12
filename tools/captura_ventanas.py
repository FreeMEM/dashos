#!/usr/bin/env python3
"""Mira la captura de pantalla de QEMU tras la prueba del compositor Wayland (W7 de S10b).

Uso: captura_ventanas.py CAPTURA.ppm

Las dos ventanas de prueba salen en cascada: la primera en (40, 40) y la segunda en (72, 72), encima. Cuál es cuál
depende de quién se conecta antes, así que se aceptan los dos órdenes.

- ventana-a: 160 x 120, roja (0xcc3333, XRGB), con sus últimas 10 líneas blancas tras el último cuadro.
- ventana-b: 120 x 100, ARGB: cuadros de 10 x 10, verdes opacos (0x33cc33) y negros a medias (alfa 128), que dejan ver
  lo de debajo oscurecido a la mitad. Sus 10 primeras líneas no cambian: verde en las columnas pares.

C3 de S10c: debajo de todo, el fondo de escritorio. Si la salida de la prueba (`salida_virt.txt`, junto a la captura)
dice que `fondo` puso una imagen, cada punto del fondo tiene que ser el de esa imagen escalada para cubrir la pantalla
(se calcula como el kit, con `fondos_prueba.cubrir_en`); si no, el color liso del compositor. Y Ajustes sigue el tema
que dijo la última vez (su superficie y su primario, de la misma salida).
"""
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fondos_prueba  # noqa: E402

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
    dir_ = os.path.dirname(os.path.abspath(sys.argv[1]))
    ruta_salida = os.path.join(dir_, "salida_virt.txt")
    salida = open(ruta_salida, errors="replace").read() if os.path.exists(ruta_salida) else ""
    # El fondo que se ve: el último que `fondo` dijo que puso.
    puestos = re.findall(r"\[fondo\] (\S+) en pantalla:", salida)
    imagen = fondos_prueba.pixeles(os.path.join(dir_, "fondos-prueba", puestos[-1])) if puestos else None

    def fondo_en(x, y):
        return fondos_prueba.cubrir_en(imagen, ancho, alto, x, y) if imagen else FONDO

    def en(x, y):
        k = 3 * (y * ancho + x)
        return tuple(px[k : k + 3])

    def casi(a, b):
        return all(abs(x - y) <= 2 for x, y in zip(a, b))

    def casi_o_sombra(a, b):
        """El color, o el mismo oscurecido por sombras (Material 3: alfa de 70 cada una; cerca de las dos ventanas de
        prueba se suman dos, hasta un 47 % más oscuro)."""
        return casi(a, b) or all(y * 134 // 255 - 3 <= x <= y + 2 for x, y in zip(a, b))

    fallos = []
    if not casi(en(5, 5), fondo_en(5, 5)):
        fallos.append(f"fondo en (5, 5): {en(5, 5)}, y debía ser {fondo_en(5, 5)}")
    # Orden 1: ventana-a en (40, 40) debajo y ventana-b en (72, 72) encima.
    orden1 = [
        ((45, 45), ROJO),
        ((45, 40 + 115), BLANCO),
        ((75, 75), VERDE),
        ((85, 75), mitad(ROJO)),  # columna impar de b sobre a
        # b por debajo de a (y >= 160), con la fase del último cuadro (29): verde en (75, 165), negro a medias
        # sobre el fondo en (85, 165).
        ((75, 165), VERDE),
        ((85, 165), mitad(fondo_en(85, 165))),
    ]
    # Orden 2: ventana-b en (40, 40) debajo y ventana-a en (72, 72) encima.
    orden2 = [
        ((45, 45), VERDE),
        ((55, 45), mitad(fondo_en(55, 45))),
        ((75, 75), ROJO),
        ((75, 72 + 115), BLANCO),
    ]
    # Un clic (W8) puede subir la de (40, 40) encima de la otra.
    # Orden 3: ventana-a en (40, 40) encima de ventana-b en (72, 72).
    orden3 = [
        ((45, 45), ROJO),
        ((45, 40 + 115), BLANCO),
        ((75, 75), ROJO),
        ((75, 165), VERDE),
        ((85, 165), mitad(fondo_en(85, 165))),
    ]
    # Orden 4: ventana-b en (40, 40) encima de ventana-a en (72, 72): en (75, 75) el negro a medias de b sobre a.
    orden4 = [
        ((45, 45), VERDE),
        ((55, 45), mitad(fondo_en(55, 45))),
        ((75, 75), mitad(ROJO)),
        ((85, 75), VERDE),
        ((75, 165), ROJO),
        ((75, 72 + 115), BLANCO),
    ]
    ordenes = (orden1, orden2, orden3, orden4)
    # Con sombras, los puntos cerca de un borde o bajo lo semitransparente pueden salir oscurecidos.
    vale = [all(casi_o_sombra(en(x, y), c) for (x, y), c in o) for o in ordenes]
    if not any(vale):
        detalle = ", ".join(f"({x}, {y}) = {en(x, y)}" for (x, y), _ in orden1 + orden2 + orden3 + orden4)
        fallos.append(f"las ventanas no están donde deben: {detalle}")
    # La aplicación del kit (K7), tercera en la cascada, en (104, 104): su superficie (Material 3, claro) abajo a la
    # derecha, donde no llega ninguna de las otras (un clic puede subir una de ellas encima), y el primario de sus
    # botones (al menos un botón entero de 40 de alto).
    # S10c: o con el tema del sistema que puso el centro de control en la prueba (oscuro, azul): Ajustes lo sigue.
    SUPERFICIE = (0xFE, 0xF7, 0xFF)
    PRIMARIO = (0x67, 0x50, 0xA4)
    if casi(en(450, 270), (0x11, 0x14, 0x18)):
        SUPERFICIE, PRIMARIO = (0x11, 0x14, 0x18), (0x9F, 0xCA, 0xFF)
    # C3: el último tema que dijo Ajustes (el del fondo, en la prueba), con sus colores.
    temas = re.findall(r"\[ajustes\] tema del sistema: .*superficie ([0-9a-f]{6}), primario ([0-9a-f]{6})", salida)
    if temas:
        SUPERFICIE, PRIMARIO = (tuple(bytes.fromhex(c)) for c in temas[-1])
    if not casi(en(450, 270), SUPERFICIE):
        fallos.append(f"la ventana del kit: superficie en (450, 270): {en(450, 270)}")
    # Abajo a la derecha, lejos de todas las ventanas, el fondo: si no, se ha quedado pintado algo que ya no está
    # (Ajustes, maximizada y devuelta, dejaba su parte de 800x600).
    if not casi(en(700, 500), fondo_en(700, 500)):
        fallos.append(f"restos de una ventana en (700, 500): {en(700, 500)}, y el fondo es {fondo_en(700, 500)}")
    primario = sum(1 for y in range(104, 104 + 250) for x in range(104, 104 + 360) if casi(en(x, y), PRIMARIO))
    if primario < 2000:
        fallos.append(f"la ventana del kit: {primario} píxeles del primario")
    if fallos:
        print("MAL: la captura del compositor:", *fallos, sep="\n  ")
        sys.exit(1)
    con = f", fondo {puestos[-1]}" if puestos else ""
    print(f"BIEN: la captura del compositor ({ancho}x{alto}, orden {vale.index(True) + 1}{con})")


main()
