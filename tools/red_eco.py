#!/usr/bin/env python3
# DashOS: el eco del PC para la prueba de la red (S13). QEMU lo lanza por cada conexión a 10.0.2.100:7 (`guestfwd` con
# `cmd:`, en el Makefile), con la conexión como entrada y salida estándar. Devuelve lo que llega, tal cual, hasta que
# el otro lado cierra; entonces acaba. Cuando QEMU se para, la conexión se cierra y acaba también.
import os

while True:
    try:
        datos = os.read(0, 65536)
    except OSError:
        break
    if not datos:
        break
    while datos:
        try:
            n = os.write(1, datos)
        except OSError:
            raise SystemExit(0)
        datos = datos[n:]
