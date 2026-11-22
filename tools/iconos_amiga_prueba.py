#!/usr/bin/env python3
"""Los iconos de Amiga de prueba (`.info`, plan de iconos, parte 3): dibujados aquí con formas calculadas, sin iconos
de nadie. Uno de cada formato que lee `iconos-cajas/src/amiga.rs`, escritos según la descripción pública de los
formatos (Dirk Stöcker, 2002) y, el ARGB de OS4, como se ve en los bytes de los iconos (10 bytes y zlib).

- `Cajon.info`: clásico, cajón, 4 colores, dos imágenes (la segunda, el cajón abierto).
- `Disk.info`: clásico, disco, 4 colores, seleccionado por complemento (el del volumen).
- `Programa.info`: clásico con NewIcons (36 colores; la segunda imagen, más clara).
- `Brillo.info`: OS 3.5 (GlowIcons): `FACE` y dos `IMAG`, la primera con repeticiones.
- `Cristal.info`: OS4: `FACE` y dos `ARGB` de 48 × 48 (con alfa de verdad).
- `Estrella.info`: OS4 y MorphOS: PNG con su trozo `icOn` y un segundo PNG detrás.
- `Roto.info`: un clásico que dice medir 300 × 400 con 8 planos y se acaba enseguida (el explorador pone el de
  reserva).

Uso: iconos_amiga_prueba.py DIR   escribe en DIR los iconos
"""
import math
import os
import struct
import sys
import zlib

WB = [(0xAA, 0xAA, 0xAA), (0, 0, 0), (0xFF, 0xFF, 0xFF), (0x66, 0x88, 0xBB)]


# --- Lo clásico ---------------------------------------------------------------------------------------------------

def planar(indices, ancho, alto, prof):
    """Los planos de una imagen de índices (fila a fila), con las filas redondeadas a 16 bits."""
    fila = (ancho + 15) // 16 * 2
    datos = bytearray(prof * alto * fila)
    for p in range(prof):
        for y in range(alto):
            for x in range(ancho):
                if indices[y * ancho + x] >> p & 1:
                    datos[p * alto * fila + y * fila + x // 8] |= 0x80 >> (x % 8)
    return bytes(datos)


def imagen(indices, ancho, alto, prof):
    return struct.pack(">hhhhhIBBI", 0, 0, ancho, alto, prof, 1, (1 << prof) - 1, 0, 0) + planar(indices, ancho, alto, prof)


def texto(t):
    t = t + b"\0"
    return struct.pack(">I", len(t)) + t


def disk_object(tipo, ancho, alto, img1, img2=None, flags=None, herramienta=None, tooltypes=None, cajon=False, extra=b""):
    if flags is None:
        flags = 4 | (2 if img2 else 0)
    gadget = struct.pack(">IhhhhHHHIIIiIHI", 0, 0, 0, ancho, alto, flags, 3, 1, 1, 1 if img2 else 0, 0, 0, 0, 0, 1)
    assert len(gadget) == 44
    cab = struct.pack(">HH", 0xE310, 1) + gadget + struct.pack(">BBIIIIIIi", tipo, 0, 1 if herramienta else 0,
                                                               1 if tooltypes is not None else 0, 0x80000000,
                                                               0x80000000, 1 if cajon else 0, 0, 4096)
    assert len(cab) == 0x4E
    d = bytearray(cab)
    if cajon:
        d += struct.pack(">hhhhBBIIIIIIIhhHHH", 50, 50, 400, 200, 255, 255, 0, 0, 0, 0, 0, 0, 0, 94, 65, 0xFFFF,
                         0xFFFF, 1) + struct.pack(">ii", 0, 0)
    d += img1
    if img2:
        d += img2
    if herramienta:
        d += texto(herramienta)
    if tooltypes is not None:
        d += struct.pack(">I", (len(tooltypes) + 1) * 4)
        for t in tooltypes:
            d += texto(t)
    if cajon:
        d += struct.pack(">IH", 0, 0)
    return bytes(d) + extra


def lienzo(ancho, alto, f):
    return [f(x, y) for y in range(alto) for x in range(ancho)]


def cajon(abierto):
    """Un cajón de 44 × 22: frente azul con tirador negro, borde blanco arriba y negro abajo."""
    def f(x, y):
        if not (2 <= x <= 41 and 3 <= y <= 20):
            return 0
        if abierto and 3 <= y <= 7 and 6 <= x <= 37:
            return 1                   # el hueco del cajón abierto
        if y == 3 or x == 2:
            return 2
        if y == 20 or x == 41:
            return 1
        if 10 <= y <= 13 and 17 <= x <= 26:
            return 1 if y in (10, 13) or x in (17, 26) else 2
        return 3
    return lienzo(44, 22, f)


def disco():
    """Un disquete de 36 × 20: cuerpo azul, ventana metálica arriba y etiqueta blanca."""
    def f(x, y):
        if not (4 <= x <= 31 and 1 <= y <= 18):
            return 0
        if x in (4, 31) or y in (1, 18):
            return 1
        if 2 <= y <= 6 and 10 <= x <= 25:
            return 1 if 18 <= x <= 21 and 3 <= y <= 5 else 0
        if 10 <= y <= 17 and 8 <= x <= 27:
            return 2
        return 3
    return lienzo(36, 20, f)


# --- NewIcons -----------------------------------------------------------------------------------------------------

def letras(valores, bits, sitio):
    """Valores de `bits` bits en letras de 7 bits, sin pasar de `sitio` letras: (letras, valores que cupieron)."""
    sal, acc, n, usados = bytearray(), 0, 0, 0
    for v in valores:
        if len(sal) + (n + bits + 6) // 7 > sitio:
            break
        acc, n = acc << bits | v, n + bits
        usados += 1
        while n >= 7:
            n -= 7
            sal.append((acc >> n) & 0x7F)
            acc &= (1 << n) - 1
    if n:
        sal.append((acc << (7 - n)) & 0x7F)
    # Las tiras de grupos a cero, comprimidas (0xD1 + n - 1).
    out, i = bytearray(), 0
    while i < len(sal):
        if sal[i] == 0:
            j = i
            while j < len(sal) and sal[j] == 0 and j - i < 47:
                j += 1
            out.append(0xD0 + (j - i))
            i = j
        else:
            g = sal[i]
            out.append(g + 0x20 if g < 0x50 else g + 0x51)
            i += 1
    return bytes(out), usados


def newicon(prefijo, transparente, ancho, alto, paleta, indices):
    n = len(paleta)
    bits = 1
    while (1 << bits) < n:
        bits += 1
    cab = bytes([ord("B" if transparente else "C"), ancho + 0x21, alto + 0x21, (n >> 6) + 0x21, (n & 0x3F) + 0x21])
    lineas = []
    rgb = [c for p in paleta for c in p]
    primera = True
    while rgb:
        sitio = 127 - 4 - (5 if primera else 0)
        l, k = letras(rgb, 8, sitio)
        lineas.append(prefijo + (cab if primera else b"") + l)
        primera, rgb = False, rgb[k:]
    resto = list(indices)
    while resto:
        l, k = letras(resto, bits, 127 - 4)
        lineas.append(prefijo + l)
        resto = resto[k:]
    return lineas


def programa_newicons(claro):
    """Un engranaje de 40 × 40 sobre un círculo con degradado (36 colores)."""
    paleta = [(0, 0, 0)] + [(min(255, 40 + i * 6 + (60 if claro else 0)), min(255, 90 + i * 4 + (60 if claro else 0)),
                              min(255, 160 + i * 2 + (40 if claro else 0))) for i in range(30)]
    paleta += [(250, 250, 250), (30, 30, 40), (240, 180, 40), (200, 120, 20), (90, 90, 100)]

    def f(x, y):
        dx, dy = x - 19.5, y - 19.5
        r = math.hypot(dx, dy)
        a = math.atan2(dy, dx)
        diente = 13 + (2.5 if math.cos(a * 8) > 0.3 else 0)
        if r < 5:
            return 0 if r < 3 else 34
        if r < diente:
            return 33 if r > diente - 1.5 else 32
        if r < 19:
            return 1 + min(29, int((dx + dy + 28) / 56 * 30))
        return 0
    return paleta, lienzo(40, 40, f)


# --- OS 3.5 y OS4 ---------------------------------------------------------------------------------------------------

def iff(tipo, datos):
    return tipo + struct.pack(">I", len(datos)) + datos + (b"\0" if len(datos) & 1 else b"")


def repeticiones(valores, bits):
    """Las repeticiones de GlowIcons (las de ILBM) sobre una tira de `bits` bits por valor."""
    salida = []   # (control, [valores])
    i = 0
    while i < len(valores):
        j = i
        while j < len(valores) and valores[j] == valores[i] and j - i < 128:
            j += 1
        if j - i >= 3:
            salida.append((257 - (j - i), [valores[i]]))
            i = j
            continue
        k = i
        while k < len(valores) and k - i < 128:
            if k + 2 < len(valores) and valores[k] == valores[k + 1] == valores[k + 2]:
                break
            k += 1
        salida.append((k - i - 1, valores[i:k]))
        i = k
    acc, n, out = 0, 0, bytearray()

    def poner(v, b):
        nonlocal acc, n
        acc, n = acc << b | v, n + b
        while n >= 8:
            n -= 8
            out.append((acc >> n) & 0xFF)
            acc &= (1 << n) - 1
    for c, vs in salida:
        poner(c, 8)
        for v in vs:
            poner(v, bits)
    if n:
        out.append((acc << (8 - n)) & 0xFF)
    return bytes(out)


def imag(indices, paleta, transparente, comprimir, bits=8):
    if comprimir:
        img, formato = repeticiones(indices, bits), 1
    else:
        img, formato = bytes(indices), 0
    pal = bytes(c for p in paleta for c in p) if paleta else b""
    opciones = (1 if transparente is not None else 0) | (2 if paleta else 0)
    cab = struct.pack(">BBBBBBHH", transparente or 0, (len(paleta) if paleta else 1) - 1, opciones, formato, 0, bits,
                      len(img) - 1, max(len(pal), 1) - 1)
    return iff(b"IMAG", cab + img + pal)


def brillo(sel):
    """Una hoja de 48 × 48 con una bombilla con halo (paleta de 24)."""
    paleta = [(0, 0, 0), (250, 250, 245), (200, 200, 195), (60, 60, 70)] + \
             [(255, 200 + i * 3, 60 + i * 8) for i in range(10)] + [(255 - i * 8, 230 - i * 10, 120) for i in range(10)]

    def f(x, y):
        if 8 <= x <= 39 and 3 <= y <= 44:
            if x in (8, 39) or y in (3, 44):
                return 3
            r = math.hypot(x - 24, y - 20)
            if r < 9:
                return 4 + min(9, int(r)) if not sel else 14 + min(9, int(r))
            if 20 <= x <= 28 and 29 <= y <= 34:
                return 3 if y % 2 else 2
            return 2 if x > 35 or y > 40 else 1
        return 0
    return paleta, lienzo(48, 48, f)


def argb_cristal(sel):
    """Una esfera de cristal de 48 × 48 con alfa suave (la luz en 8 escalones: así comprime bien)."""
    px = bytearray()
    for y in range(48):
        for x in range(48):
            r = math.hypot(x - 23.5, y - 23.5)
            a = max(0, min(255, int((21 - r) * 60)))
            luz = int(max(0, 1 - math.hypot(x - 16, y - 15) / 23) * 8) / 8
            base = (40, 150, 120) if not sel else (150, 90, 200)
            c = [min(255, int(b + (255 - b) * luz * 0.9)) for b in base]
            px += bytes([a] + c)
    return bytes(px)


def argb_trozo(px):
    z = zlib.compress(px, 9)
    return iff(b"ARGB", struct.pack(">IIH", 1, len(z) - 1, 0) + z)


def png(ancho, alto, rgba, extra=b""):
    def trozo(tipo, d):
        return struct.pack(">I", len(d)) + tipo + d + struct.pack(">I", zlib.crc32(tipo + d) & 0xFFFFFFFF)
    filas = b"".join(b"\0" + rgba[y * ancho * 4:(y + 1) * ancho * 4] for y in range(alto))
    return (b"\x89PNG\r\n\x1a\n" + trozo(b"IHDR", struct.pack(">IIBBBBB", ancho, alto, 8, 6, 0, 0, 0)) + extra +
            trozo(b"IDAT", zlib.compress(filas, 9)) + trozo(b"IEND", b""))


def estrella(sel):
    """Una estrella de cinco puntas de 52 × 52, amarilla (roja al seleccionar), con borde suave."""
    px = bytearray()
    for y in range(52):
        for x in range(52):
            dx, dy = x - 25.5, y - 26.5
            a = math.atan2(dy, dx) + math.pi / 2
            r = math.hypot(dx, dy)
            k = (a / (2 * math.pi / 5)) % 1
            borde = 11 + 13 * abs(1 - 2 * k) ** 1.5
            alfa = max(0, min(255, int((borde - r) * 128)))
            c = (240, 190, 30) if not sel else (220, 60, 50)
            px += bytes(c) + bytes([alfa])
    return bytes(px)


def icon_trozo():
    """Un `icOn` de OS4: etiquetas (u32) y valores; aquí solo el tipo (proyecto) y el fin."""
    def trozo(tipo, d):
        return struct.pack(">I", len(d)) + tipo + d + struct.pack(">I", zlib.crc32(tipo + d) & 0xFFFFFFFF)
    return trozo(b"icOn", struct.pack(">IIII", 0x8000_1009, 4, 0, 0))


def iconos():
    d = {}
    d["Cajon.info"] = disk_object(2, 44, 22, imagen(cajon(False), 44, 22, 2), imagen(cajon(True), 44, 22, 2), cajon=True)
    d["Disk.info"] = disk_object(1, 36, 20, imagen(disco(), 36, 20, 2), flags=4, cajon=True)
    p1, i1 = programa_newicons(False)
    p2, i2 = programa_newicons(True)
    tt = [b"PUBSCREEN=Workbench", b" ", b"*** DON'T EDIT THE FOLLOWING LINES!! ***"] + \
         newicon(b"IM1=", True, 40, 40, p1, i1) + newicon(b"IM2=", True, 40, 40, p2, i2)
    vacio = [0] * (16 * 8)
    d["Programa.info"] = disk_object(3, 16, 8, imagen(vacio, 16, 8, 2), tooltypes=tt)
    pa, ia = brillo(False)
    _, ib = brillo(True)
    form = iff(b"FACE", struct.pack(">BBBBH", 47, 47, 0, 0x11, len(pa) * 3 - 1)) + imag(ia, pa, 0, True, 5) + imag(ib, None, 0, False)
    d["Brillo.info"] = disk_object(4, 16, 8, imagen(vacio, 16, 8, 2), herramienta=b"SYS:Utilities/MultiView",
                                   extra=b"FORM" + struct.pack(">I", len(form) + 4) + b"ICON" + form)
    form = iff(b"FACE", struct.pack(">BBBBH", 47, 47, 1, 0x11, 0)) + argb_trozo(argb_cristal(False)) + argb_trozo(argb_cristal(True))
    d["Cristal.info"] = disk_object(3, 16, 8, imagen(vacio, 16, 8, 2), extra=b"FORM" + struct.pack(">I", len(form) + 4) + b"ICON" + form)
    d["Estrella.info"] = png(52, 52, estrella(False), icon_trozo()) + png(52, 52, estrella(True))
    roto = bytearray(disk_object(3, 16, 8, imagen(vacio, 16, 8, 2)))
    roto[0x4E + 4:0x4E + 10] = struct.pack(">hhh", 300, 400, 8)
    d["Roto.info"] = bytes(roto[:0x4E + 20 + 100])
    return d


def escribir(directorio):
    os.makedirs(directorio, exist_ok=True)
    for n, datos in iconos().items():
        with open(os.path.join(directorio, n), "wb") as f:
            f.write(datos)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    escribir(sys.argv[1])
