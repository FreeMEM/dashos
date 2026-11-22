#!/usr/bin/env python3
"""Desenchufar y enchufar el adaptador de red USB de `make arranque-virt-dominio` (S35, paso 2 con USB;
docs/bitacoras/dominio-usb.md), por el monitor de QEMU, cuando lo pide la prueba de dentro (`dominio-prueba`).

Uso (lo lanza `tools/prueba_uefi.sh` con DOMINIO_USB=<el -device del adaptador>): guion_dominio_usb.py MONITOR
SEGUNDOS SERIE APARATO

Mira la salida serie: cada "[dominio-prueba] pide desenchufar el adaptador USB" quita el aparato (`device_del`, por su
id) y cada "[dominio-prueba] pide enchufar el adaptador USB" lo vuelve a poner como estaba (`device_add`, en el mismo
puerto del xHCI y con la misma red). Lo que hace, en la salida de error.
"""
import re
import socket
import sys
import time

monitor, segundos, serie, aparato = sys.argv[1], float(sys.argv[2]), sys.argv[3], sys.argv[4]
ident = re.search(r"(?:^|,)id=([^,]+)", aparato)
ident = ident.group(1) if ident else "usbred"


def orden(texto):
    s = socket.socket(socket.AF_UNIX)
    s.connect(monitor)
    s.settimeout(2.0)
    s.sendall(texto.encode() + b"\n")
    datos = b""
    try:
        while True:
            t = s.recv(65536)
            if not t:
                break
            datos += t
    except OSError:
        pass
    s.close()
    print(f"{time.strftime('%H:%M:%S')} {texto}: {datos.decode(errors='replace')[-200:]!r}", file=sys.stderr, flush=True)


hechas = {"desenchufar": 0, "enchufar": 0}
fin = time.time() + segundos
while time.time() < fin:
    try:
        texto = open(serie, errors="replace").read()
    except OSError:
        texto = ""
    for que in hechas:
        pedidas = texto.count(f"[dominio-prueba] pide {que} el adaptador USB")
        while hechas[que] < pedidas:
            hechas[que] += 1
            orden(f"device_del {ident}" if que == "desenchufar" else f"device_add {aparato}")
    time.sleep(0.2)
