#!/usr/bin/env python3
# DashOS: quien entra desde fuera, para la prueba de escuchar (S13, segunda parte), desde el PC. QEMU reenvía un puerto
# del PC (127.0.0.1) al 7777 de DashOS (`hostfwd`, en el Makefile). Este programa intenta entrar por él una y otra vez
# desde que arranca QEMU: mientras nadie escucha en DashOS, su cortafuegos tira el intento sin contestar (y lo cuenta);
# cuando `red-prueba` escucha, entra, manda una línea, espera el eco y acaba.
#
#   tools/red_entra.py <puerto del PC> [segundos]
import socket
import sys
import time

puerto = int(sys.argv[1])
fin = time.time() + (float(sys.argv[2]) if len(sys.argv) > 2 else 300.0)
LINEA = b"hola desde fuera\n"
while time.time() < fin:
    try:
        with socket.create_connection(("127.0.0.1", puerto), timeout=2) as c:
            c.settimeout(3)
            c.sendall(LINEA)
            vuelto = b""
            while len(vuelto) < len(LINEA):
                trozo = c.recv(64)
                if not trozo:
                    break
                vuelto += trozo
            if vuelto == LINEA:
                break
    except OSError:
        pass
    time.sleep(1)
