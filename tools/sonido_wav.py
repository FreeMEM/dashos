#!/usr/bin/env python3
"""Comprueba el WAV que escribe QEMU con lo que sonó en DashOS (S19).

A4: dos programas suenan a la vez por el servicio "sonido": `tono-a`, 1 kHz a 0,25 de la escala (mono de 16 bits a
48 kHz), y `tono-b`, 600 Hz a 0,4 con el volumen a la mitad (0,2) durante 2 s (estéreo de coma flotante a 44,1 kHz,
remuestreado). A5: `tono-a` muere a los 1,5 s escritos, a mitad de un ciclo; el servicio lo nota cuando su anillo
baja de la mitad y lo quita con un fundido, sin sonar lo que quedaba (unos 85 ms): suena unos 1,42 s. Se mide cada
tono en ventanas de 0,1 s (las dos frecuencias caben enteras): `tono-a` con su nivel unas 13 a 15 ventanas, `tono-b` unas 20, juntos al menos 13, sin
huecos y sin saltos entre muestras más grandes que los que dan los dos senos (el fundido no chasca).
"""

import math
import struct
import sys


def leer(ruta):
    # QEMU escribe los tamaños de la cabecera al cerrar el WAV, y al pararlo con una señal no los escribe (quedan a
    # cero): se lee la cabecera a mano y los datos hasta el final del fichero.
    with open(ruta, "rb") as f:
        b = f.read()
    if b[:4] != b"RIFF" or b[8:12] != b"WAVE" or b[12:16] != b"fmt ":
        raise SystemExit(f"MAL: {ruta} no es un WAV")
    _, canales, ritmo, _, _, bits = struct.unpack("<HHIIHH", b[20:36])
    ancho = bits // 8
    i = 12
    while i + 8 <= len(b) and b[i : i + 4] != b"data":
        i += 8 + struct.unpack("<I", b[i + 4 : i + 8])[0]
    datos = b[i + 8 :]
    datos = datos[: len(datos) - len(datos) % (2 * canales)]
    if ancho != 2:
        raise SystemExit(f"MAL: el WAV es de {8 * ancho} bits, no de 16")
    muestras = struct.unpack(f"<{len(datos) // 2}h", datos)
    izquierda = muestras[::canales]
    derecha = muestras[1::canales] if canales > 1 else izquierda
    return ritmo, izquierda, derecha


TABLAS = {}


def nivel(x, hz, ritmo):
    """La amplitud del componente de `hz` en `x` (que contiene un número entero de sus ciclos)."""
    if (hz, len(x)) not in TABLAS:
        w = [2 * math.pi * hz * i / ritmo for i in range(len(x))]
        TABLAS[hz, len(x)] = ([math.cos(a) for a in w], [math.sin(a) for a in w])
    c, s = TABLAS[hz, len(x)]
    if not any(x):
        return 0.0
    re = sum(map(lambda a, b: a * b, x, c))
    im = sum(map(lambda a, b: a * b, x, s))
    return 2 * math.hypot(re, im) / len(x) / 32768


def comprobar(ruta):
    ritmo, izq, der = leer(ruta)
    fallos = []
    if ritmo != 48000:
        return ritmo, [f"el WAV va a {ritmo} Hz, no a 48000"], ""
    v = ritmo // 10
    ventanas = [(nivel(izq[i : i + v], 1000, ritmo), nivel(izq[i : i + v], 600, ritmo)) for i in range(0, len(izq) - v + 1, v)]
    con_a = [i for i, (a, _) in enumerate(ventanas) if abs(a - 0.25) < 0.01]
    con_b = [i for i, (_, b) in enumerate(ventanas) if abs(b - 0.2) < 0.01]
    juntos = set(con_a) & set(con_b)
    if not 12 <= len(con_a) <= 16:
        fallos.append(f"el tono de 1 kHz suena con su nivel {len(con_a)} ventanas de 0,1 s, no unas 14")
    if not 18 <= len(con_b) <= 21:
        fallos.append(f"el tono de 600 Hz suena con su nivel {len(con_b)} ventanas de 0,1 s, no unas 20")
    if con_a and con_b and con_a[-1] >= con_b[-1]:
        fallos.append("el tono de 1 kHz no acaba antes que el de 600 Hz: el programa muerto sigue sonando")
    if len(juntos) < 12:
        fallos.append(f"los dos tonos suenan juntos solo {len(juntos)} ventanas")
    for nombre, lista in (("1 kHz", con_a), ("600 Hz", con_b)):
        if lista and lista[-1] - lista[0] + 1 != len(lista):
            fallos.append(f"el tono de {nombre} tiene huecos: ventanas {lista}")
    if any(abs(x - y) > 2 for x, y in zip(izq, der)):
        fallos.append("la izquierda y la derecha no son iguales")
    # El salto más grande de una suma de senos: 2π(f1·A1 + f2·A2)/ritmo, en muestras de 16 bits, con margen.
    limite = 2 * math.pi * (1000 * 0.25 + 600 * 0.2) / ritmo * 32768 * 1.15
    saltos = [i for i in range(1, len(izq)) if abs(izq[i] - izq[i - 1]) > limite]
    if saltos:
        fallos.append(f"{len(saltos)} saltos (chasquidos), el primero en el cuadro {saltos[0]}")
    resumen = f"1 kHz en {len(con_a)} ventanas, 600 Hz en {len(con_b)}, juntos en {len(juntos)}, sin huecos ni chasquidos"
    return ritmo, fallos, resumen


def main():
    ruta = sys.argv[1]
    ritmo, fallos, resumen = comprobar(ruta)
    if fallos:
        print("MAL: el sonido:")
        for f in fallos:
            print("  " + f)
        sys.exit(1)
    print(f"BIEN: el sonido ({resumen})")


if __name__ == "__main__":
    main()
