#!/usr/bin/env python3
"""Una captura de la pantalla de QEMU por su servidor VNC (RFB 3.8, sin contraseña, codificación "raw"), sin programas
de fuera. Hace falta para la pantalla de una GPU con 3D (virtio-gpu-gl con -display egl-headless): ahí el screendump
de QMP no tiene superficie ("no surface") y el VNC sí recibe los cuadros (egl-headless los lee de la GPU).

    tools/vnc_captura.py <socket unix o host:puerto> <salida.png|.ppm>

Escrito con el RFC 6143 (el protocolo RFB): 7.1 (versión y seguridad), 7.3 (ClientInit, ServerInit), 7.4 (formato
del píxel), 7.5.1 a 7.5.3 y 7.6.1 (pedir y recibir la pantalla)."""

import socket
import struct
import sys
from pathlib import Path


def _leer(s, n):
    b = bytearray()
    while len(b) < n:
        t = s.recv(n - len(b))
        if not t:
            raise ConnectionError("el servidor VNC cerró")
        b += t
    return bytes(b)


def capturar(donde, destino, plazo=10.0):
    """Una imagen de toda la pantalla en `destino` (PNG con PIL, o PPM). Devuelve (ancho, alto)."""
    if ":" in str(donde) and not Path(str(donde)).exists():
        host, puerto = str(donde).rsplit(":", 1)
        s = socket.create_connection((host, int(puerto)), timeout=plazo)
    else:
        s = socket.socket(socket.AF_UNIX)
        s.settimeout(plazo)
        s.connect(str(donde))
    try:
        _leer(s, 12)
        s.sendall(b"RFB 003.008\n")
        n = _leer(s, 1)[0]
        if n == 0:
            raise ConnectionError("el servidor VNC no deja entrar")
        if 1 not in _leer(s, n):
            raise ConnectionError("el servidor VNC pide contraseña")
        s.sendall(b"\x01")
        if struct.unpack(">I", _leer(s, 4))[0] != 0:
            raise ConnectionError("seguridad rechazada")
        s.sendall(b"\x01")  # ClientInit: compartida
        ancho, alto = struct.unpack(">HH", _leer(s, 4))
        _leer(s, 16)
        _leer(s, struct.unpack(">I", _leer(s, 4))[0])
        # SetPixelFormat: 32 bits, profundidad 24, little-endian, color verdadero, 255 por canal, R<<16 G<<8 B.
        s.sendall(struct.pack(">BxxxBBBBHHHBBBxxx", 0, 32, 24, 0, 1, 255, 255, 255, 16, 8, 0))
        s.sendall(struct.pack(">BxHi", 2, 1, 0))  # SetEncodings: solo raw
        s.sendall(struct.pack(">BBHHHH", 3, 0, 0, 0, ancho, alto))  # toda la pantalla, no incremental
        px = bytearray(ancho * alto * 3)
        faltan = ancho * alto
        while faltan > 0:
            tipo = _leer(s, 1)[0]
            if tipo == 2:  # Bell
                continue
            if tipo == 3:  # ServerCutText
                _leer(s, 3)
                _leer(s, struct.unpack(">I", _leer(s, 4))[0])
                continue
            if tipo == 1:  # SetColourMapEntries (no se pidió paleta)
                _leer(s, 3)
                _leer(s, 6 * struct.unpack(">H", _leer(s, 2))[0])
                continue
            if tipo != 0:
                raise ConnectionError(f"mensaje VNC desconocido {tipo}")
            _leer(s, 1)
            for _ in range(struct.unpack(">H", _leer(s, 2))[0]):
                x, y, w, h, cod = struct.unpack(">HHHHi", _leer(s, 12))
                if cod != 0:
                    raise ConnectionError(f"codificación VNC {cod}")
                datos = _leer(s, w * h * 4)
                for fila in range(h):
                    o = ((y + fila) * ancho + x) * 3
                    f = datos[fila * w * 4:(fila + 1) * w * 4]
                    px[o:o + 3 * w:3] = f[2::4]
                    px[o + 1:o + 3 * w:3] = f[1::4]
                    px[o + 2:o + 3 * w:3] = f[0::4]
                faltan -= w * h
    finally:
        s.close()
    destino = Path(destino)
    if destino.suffix == ".png":
        from PIL import Image
        Image.frombytes("RGB", (ancho, alto), bytes(px)).save(destino)
    else:
        destino.write_bytes(f"P6\n{ancho} {alto}\n255\n".encode() + bytes(px))
    return ancho, alto


if __name__ == "__main__":
    print(capturar(sys.argv[1], sys.argv[2]))
