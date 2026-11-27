#!/usr/bin/env python3
"""Los fondos de escritorio de prueba (C3 de S10c) y los PNG de las pruebas del kit. Todo generado aquí, sin imágenes
de nadie: degradados y ondas calculados.

- `ondas()`: 960 × 600, color (tipo 2): azul marino a verde azulado en diagonal, con tres ondas claras.
- `atardecer()`: 640 × 480, color con alfa (tipo 6, opaco): violeta a rosa y naranja, con un sol.
- `rota()`: `atardecer` con un byte cambiado en sus datos (el CRC no cuadra). Antes era `ondas` (153 KB); el
  disco de pruebas va justo (A15) y con `atardecer` (23 KB) la prueba es la misma.
- `enorme()`: una cabecera de 40000 × 40000 con datos de nada.

Uso: fondos_prueba.py --pruebas DIR   escribe en DIR los PNG de las pruebas del kit (`kit/src/png.rs`)
     fondos_prueba.py --fondos DIR    escribe en DIR los fondos de prueba

`cubrir_en(imagen, ancho, alto, x, y)` hace lo mismo que `Imagen::cubrir` del kit (`kit/src/imagen.rs`) para un
píxel: lo usa `captura_ventanas.py` para saber qué color del fondo debe salir en la captura.
"""
import math
import os
import struct
import sys
import zlib

ADAM7 = [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)]
CANALES = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}


def trozo(tipo, datos):
    return struct.pack(">I", len(datos)) + tipo + datos + struct.pack(">I", zlib.crc32(tipo + datos) & 0xFFFFFFFF)


def paeth(a, b, c):
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    return a if pa <= pb and pa <= pc else (b if pb <= pc else c)


def filtrar(filas, bpp, filtros):
    """Las líneas (bytes) con su filtro delante; el filtro de cada una, por turnos si `filtros` es una lista."""
    salida = bytearray()
    arriba = bytes(len(filas[0])) if filas else b""
    for i, f in enumerate(filas):
        t = filtros[i % len(filtros)]
        out = bytearray([t])
        for k, v in enumerate(f):
            a = f[k - bpp] if k >= bpp else 0
            b = arriba[k]
            c = arriba[k - bpp] if k >= bpp else 0
            pred = [0, a, b, (a + b) // 2, paeth(a, b, c)][t]
            out.append((v - pred) & 0xFF)
        salida += out
        arriba = f
    return bytes(salida)


def empaquetar(muestras, bits):
    """Una línea de muestras (enteros) en bytes, con `bits` bits cada una."""
    if bits == 8:
        return bytes(muestras)
    if bits == 16:
        return b"".join(struct.pack(">H", m) for m in muestras)
    out = bytearray()
    acc, n = 0, 0
    for m in muestras:
        acc = acc << bits | m
        n += bits
        if n == 8:
            out.append(acc)
            acc, n = 0, 0
    if n:
        out.append(acc << (8 - n))
    return bytes(out)


def png(ancho, alto, tipo, bits, muestra, entrelazada=False, filtros=(0, 1, 2, 3, 4), paleta=None, trns=None, nivel=9):
    """Un PNG. `muestra(x, y)` da la tupla de muestras del píxel (los canales del tipo)."""
    canales = CANALES[tipo]
    bpp = max(1, canales * bits // 8)
    pasadas = ADAM7 if entrelazada else [(0, 0, 1, 1)]
    datos = b""
    for x0, y0, dx, dy in pasadas:
        xs = list(range(x0, ancho, dx))
        ys = list(range(y0, alto, dy))
        if not xs or not ys:
            continue
        filas = [empaquetar([v for x in xs for v in muestra(x, y)], bits) for y in ys]
        datos += filtrar(filas, bpp, list(filtros))
    cab = struct.pack(">IIBBBBB", ancho, alto, bits, tipo, 0, 0, 1 if entrelazada else 0)
    r = b"\x89PNG\r\n\x1a\n" + trozo(b"IHDR", cab)
    if paleta:
        r += trozo(b"PLTE", b"".join(bytes(c) for c in paleta))
    if trns is not None:
        r += trozo(b"tRNS", trns)
    # Los datos en dos IDAT, para probar que se juntan.
    z = zlib.compress(datos, nivel)
    mitad = len(z) // 2
    r += trozo(b"IDAT", z[:mitad]) + trozo(b"IDAT", z[mitad:]) + trozo(b"IEND", b"")
    return r


# ----------------------------------------------------------------------------------------------------------------------
# Los PNG de las pruebas del kit (37 x 23, con la fórmula de `formula` en kit/src/png.rs).

def formula(x, y):
    return (x * 7) & 0xFF, (y * 11) & 0xFF, (x * y * 3) & 0xFF, (255 - x * 3) & 0xFF


def pruebas(dir_):
    os.makedirs(dir_, exist_ok=True)
    W, H = 37, 23
    ficheros = {
        "color.png": png(W, H, 2, 8, lambda x, y: formula(x, y)[:3]),
        "alfa.png": png(W, H, 6, 8, lambda x, y: formula(x, y)),
        "gris.png": png(W, H, 0, 8, lambda x, y: (formula(x, y)[0],)),
        "gris_alfa.png": png(W, H, 4, 8, lambda x, y: (formula(x, y)[0], formula(x, y)[3])),
        "color16.png": png(W, H, 2, 16, lambda x, y: tuple(v << 8 | ((x + y) & 0xFF) for v in formula(x, y)[:3])),
        "paleta.png": png(W, H, 3, 4, lambda x, y: ((x + y) % 16,),
                          paleta=[(i * 16, 255 - i * 16, i * 8) for i in range(16)], trns=b"\x00"),
        "bits1.png": png(W, H, 0, 1, lambda x, y: ((x + y) % 2,)),
        "adam7.png": png(W, H, 2, 8, lambda x, y: formula(x, y)[:3], entrelazada=True),
        "enorme.png": enorme(),
    }
    for nombre, datos in ficheros.items():
        with open(os.path.join(dir_, nombre), "wb") as f:
            f.write(datos)
    with open(os.path.join(dir_, "hola.zlib"), "wb") as f:
        f.write(zlib.compress(b"hola " * 40 + "ñandú".encode(), 9))


# ----------------------------------------------------------------------------------------------------------------------
# Los fondos.

def mezcla(a, b, t):
    t = min(1.0, max(0.0, t))
    return tuple(round(x + (y - x) * t) for x, y in zip(a, b))


def suave(a, b, x):
    t = min(1.0, max(0.0, (x - a) / (b - a)))
    return t * t * (3 - 2 * t)


def ondas_px(ancho=960, alto=600):
    marino, verdeazul, claro = (0x14, 0x21, 0x3D), (0x1F, 0x7A, 0x8C), (0xBF, 0xDB, 0xF7)
    px = []
    for y in range(alto):
        for x in range(ancho):
            c = mezcla(marino, verdeazul, 0.6 * x / ancho + 0.4 * y / alto)
            for k in range(3):
                centro = alto * (0.42 + 0.14 * k) + 38 * math.sin(2 * math.pi * x / ancho * (1.2 + 0.35 * k) + 1.7 * k)
                d = (y - centro) / (34 + 10 * k)
                c = mezcla(c, claro, 0.22 * math.exp(-d * d))
            px.append(c)
    return ancho, alto, px


def atardecer_px(ancho=640, alto=480):
    arriba, medio, abajo, sol = (0x2B, 0x10, 0x55), (0xD6, 0x55, 0x6A), (0xF7, 0xB2, 0x67), (0xFF, 0xE0, 0x8A)
    mar = (0x3A, 0x1C, 0x5C)
    horizonte = 0.72 * alto
    px = []
    for y in range(alto):
        for x in range(ancho):
            t = y / horizonte
            c = mezcla(arriba, medio, t / 0.6) if t < 0.6 else mezcla(medio, abajo, (t - 0.6) / 0.4)
            r = math.hypot(x - ancho * 0.5, y - alto * 0.62)
            c = mezcla(c, sol, 1 - suave(alto * 0.11, alto * 0.11 + 3, r))
            if y >= horizonte:
                # El mar: oscuro, con el reflejo del sol en franjas suaves.
                c = mezcla(abajo, mar, (y - horizonte) / (alto - horizonte) * 1.5 + 0.35)
                reflejo = math.exp(-((x - ancho * 0.5) / (ancho * 0.08)) ** 2) * (0.5 + 0.5 * math.sin(y * 0.9))
                c = mezcla(c, sol, 0.35 * reflejo)
            px.append(c)
    return ancho, alto, px


def ondas():
    w, h, px = ondas_px()
    return png(w, h, 2, 8, lambda x, y: px[y * w + x], filtros=(1, 2, 4))


def atardecer():
    w, h, px = atardecer_px()
    return png(w, h, 6, 8, lambda x, y: px[y * w + x] + (255,), filtros=(4, 2, 3, 1))


def rota():
    datos = bytearray(atardecer())
    datos[len(datos) // 2] ^= 0x55
    return bytes(datos)


def enorme():
    cab = struct.pack(">IIBBBBB", 40000, 40000, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + trozo(b"IHDR", cab) + trozo(b"IDAT", zlib.compress(bytes(1000))) + trozo(b"IEND", b"")


def paleta_mar(ancho=640, alto=400):
    """Rama `fondos`: un fondo de paleta animada ("color cycling" de Deluxe Paint), hecho aquí: un atardecer sobre el
    mar a 640 × 400 con 256 colores. El cielo, el sol y las olas de cerca están quietos; el agua junto al horizonte
    (colores 64 a 79) y el reflejo del sol (80 a 95) giran, y las estrellas (96 a 103) titilan. Lo que gira es poco
    (un 12 % de la pantalla): cada cuadro compone poco. Los tramos van en el trozo de texto `DashOS-ciclos` (lo lee
    `kit::fondos::ciclos`)."""
    pal = [(0, 0, 0)] * 256
    # El cielo: 48 tonos de violeta oscuro a naranja (0 a 47).
    arriba, medio, abajo = (20, 12, 48), (120, 40, 110), (250, 150, 80)
    for i in range(48):
        t = i / 47
        a, b, u = (arriba, medio, t * 2) if t < 0.5 else (medio, abajo, t * 2 - 1)
        pal[i] = tuple(round(a[k] + (b[k] - a[k]) * u) for k in range(3))
    pal[48], pal[49] = (255, 230, 150), (255, 200, 110)  # el sol y su borde
    pal[50] = (30, 20, 40)  # la costa
    for i in range(8):
        u = 0.5 - 0.5 * math.cos(i / 8 * 2 * math.pi)
        pal[51 + i] = (round(18 + 30 * u), round(28 + 45 * u), round(70 + 70 * u))
    # El agua: 16 azules que suben y bajan (un ciclo suave).
    for i in range(16):
        u = 0.5 - 0.5 * math.cos(i / 16 * 2 * math.pi)
        pal[64 + i] = (round(20 + 40 * u), round(30 + 60 * u), round(80 + 90 * u))
    # El reflejo del sol: 16 de naranja oscuro a casi blanco.
    for i in range(16):
        u = 0.5 - 0.5 * math.cos(i / 16 * 2 * math.pi)
        pal[80 + i] = (round(140 + 115 * u), round(70 + 150 * u), round(40 + 110 * u))
    # Las estrellas: 8 brillos.
    for i in range(8):
        u = 0.5 - 0.5 * math.cos(i / 8 * 2 * math.pi)
        v = round(60 + 195 * u)
        pal[96 + i] = (v, v, min(255, v + 20))
    horizonte = alto * 55 // 100
    cx, cy, r = ancho // 2, horizonte - alto // 12, alto // 9
    filas = bytearray()
    estado = 12345
    for y in range(alto):
        filas.append(0)
        for x in range(ancho):
            if y < horizonte:
                d2 = (x - cx) ** 2 + (y - cy) ** 2
                if d2 <= r * r:
                    c = 48 if d2 <= (r - 3) ** 2 else 49
                else:
                    # El cielo, con un tramado ordenado entre dos tonos.
                    t = y / horizonte * 47
                    base = int(t)
                    c = min(47, base + (1 if (t - base) > ((x * 7 + y * 3) % 8) / 8 else 0))
                    estado = (estado * 1103515245 + 12345) & 0x7FFFFFFF
                    if y < horizonte * 2 // 3 and estado % 997 == 0:
                        c = 96 + (x + y) % 8
                # La costa, a la izquierda.
                if y > horizonte - 18 - (x // 9) % 7 and x < ancho // 3 - (y % 5):
                    c = 50
            else:
                prof = y - horizonte
                # El reflejo: una columna que se abre con la distancia.
                ancho_r = r // 3 + prof // 3
                ola = (x // (5 + prof // 15) + prof // 2 + (prof * prof) // 90) % 16
                if abs(x - cx) < ancho_r - (ola % 5) * (2 + prof // 20) and ola % 4 != 0:
                    c = 80 + (prof // 2 + abs(x - cx) // 5) % 16
                elif prof < alto // 22:
                    # Junto al horizonte, el agua brilla (gira).
                    c = 64 + (x // (6 + prof // 12) + prof // 2 + (prof * prof) // 90) % 16
                else:
                    # Más cerca, olas quietas: ocho azules fijos (51 a 58), del mismo ciclo de tonos.
                    c = 51 + ((x // (6 + prof // 12) + prof // 2 + (prof * prof) // 90) % 16) // 2
            filas.append(c)
    cab = struct.pack(">IIBBBBB", ancho, alto, 8, 3, 0, 0, 0)
    texto = b"DashOS-ciclos\x0064-79 6\n80-95 10 atras\n96-103 3"
    return (b"\x89PNG\r\n\x1a\n" + trozo(b"IHDR", cab) + trozo(b"PLTE", b"".join(bytes(c) for c in pal))
            + trozo(b"tEXt", texto) + trozo(b"IDAT", zlib.compress(bytes(filas), 9)) + trozo(b"IEND", b""))


FONDOS = {"ondas.png": ondas, "atardecer.png": atardecer, "rota.png": rota, "enorme.png": enorme}
# Los que se ven: sus píxeles, para `cubrir_en`.
PIXELES = {"ondas.png": ondas_px, "atardecer.png": atardecer_px}


def fondos(dir_):
    """Escribe los fondos en `dir_`. Hacerlos cuesta unos segundos: si ya están y son de este fichero, no se rehacen."""
    os.makedirs(dir_, exist_ok=True)
    yo = os.path.getmtime(os.path.abspath(__file__))
    for nombre, hacer in FONDOS.items():
        ruta = os.path.join(dir_, nombre)
        if os.path.exists(ruta) and os.path.getmtime(ruta) >= yo:
            continue
        with open(ruta + ".nuevo", "wb") as f:
            f.write(hacer())
        os.replace(ruta + ".nuevo", ruta)


def pixeles(ruta):
    """Los píxeles de un fondo ya escrito (con PIL, que lee PNG): (ancho, alto, lista de (r, g, b))."""
    from PIL import Image
    img = Image.open(ruta).convert("RGB")
    return img.width, img.height, list(img.getdata())


# ----------------------------------------------------------------------------------------------------------------------
# Lo mismo que `Imagen::cubrir` del kit, para un píxel (con enteros, igual que allí). Sin reducir por cuadros: los
# fondos de prueba no encogen a menos de la mitad.

def encuadre(sw, sh, w, h):
    if w * sh >= h * sw:
        vw, vh = sw << 16, (sw << 16) * h // w
    else:
        vw, vh = (sh << 16) * w // h, sh << 16
    return ((sw << 16) - vw) // 2, ((sh << 16) - vh) // 2, vw // w, vh // h


def cubrir_en(imagen, w, h, x, y):
    sw, sh, px = imagen
    ox, oy, pxx, pyy = encuadre(sw, sh, w, h)
    sx = min(max(ox + x * pxx + pxx // 2 - 32768, 0), (sw - 1) << 16)
    sy = min(max(oy + y * pyy + pyy // 2 - 32768, 0), (sh - 1) << 16)
    x0, y0 = sx >> 16, sy >> 16
    x1, y1 = min(x0 + 1, sw - 1), min(y0 + 1, sh - 1)
    fx, fy = (sx >> 8) & 0xFF, (sy >> 8) & 0xFF
    a, b, c, e = px[y0 * sw + x0], px[y0 * sw + x1], px[y1 * sw + x0], px[y1 * sw + x1]
    res = []
    for k in range(3):
        arriba = a[k] * (256 - fx) + b[k] * fx
        abajo = c[k] * (256 - fx) + e[k] * fx
        res.append((arriba * (256 - fy) + abajo * fy + 32768) >> 16)
    return tuple(res)


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--pruebas":
        pruebas(sys.argv[2])
    elif len(sys.argv) == 3 and sys.argv[1] == "--fondos":
        fondos(sys.argv[2])
    else:
        print(__doc__)
        sys.exit(2)
