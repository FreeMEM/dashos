#!/usr/bin/env python3
# DashOS: basura para la tarjeta de red (S13), desde el PC. QEMU une la tarjeta, la red de usuario y este programa en un
# concentrador (`-netdev hubport`); este habla con QEMU por un socket Unix de datagramas (`-netdev dgram`, en el
# Makefile). Empieza cuando ve el primer segmento TCP que sale de DashOS (10.0.2.15): así no pisa el arranque y no
# depende de lo que tarde. Desde ahí, durante 20 s, cada 0,2 s manda un lote de tramas malas: cortas (1 a 13 bytes), largas (más de 1518, que el controlador
# tira), enormes (hasta 64 KB, que QEMU no cabe en un búfer y tira), IPv4 rotas hacia la dirección de DashOS (suma mal,
# cabecera corta, largo mayor que la trama), ARP rotas y tipos que nadie conoce. Ninguna es válida: ni la red de
# usuario de QEMU ni la pila de DashOS deben hacer nada con ellas, y nadie debe caerse.
#
#   tools/red_basura.py <directorio de los sockets> <MAC de DashOS> [segundos]
#
# Lee lo que QEMU le manda (lo que DashOS envía también llega aquí), para verlo y para que su cola no se llene. Acaba
# 20 s después de empezar, al pasar los segundos dados (300 de serie) o cuando lo paran.
import os
import random
import socket
import struct
import sys
import time

directorio, mac = sys.argv[1], bytes.fromhex(sys.argv[2].replace(":", ""))
segundos = float(sys.argv[3]) if len(sys.argv) > 3 else 300.0
qemu = os.path.join(directorio, "qemu.sock")
yo = os.path.join(directorio, "pc.sock")
try:
    os.unlink(yo)
except FileNotFoundError:
    pass
s = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
s.bind(yo)
s.setblocking(False)
azar = random.Random(13)
origen = bytes([0x02, 0x00, 0x00, 0xBA, 0x5A, 0x00])


def bytes_al_azar(n):
    return bytes(azar.getrandbits(8) for _ in range(n))


def suma(cabecera):
    t = 0
    for i in range(0, len(cabecera), 2):
        t += (cabecera[i] << 8) | cabecera[i + 1]
    while t >> 16:
        t = (t & 0xFFFF) + (t >> 16)
    return (~t) & 0xFFFF


def ipv4(manera):
    # Desde 192.0.2.x (TEST-NET) hacia 10.0.2.15, con datos al azar.
    datos = bytes_al_azar(azar.randint(8, 400))
    largo = 20 + len(datos)
    ihl = 5
    if manera == "corta":
        ihl = azar.randint(0, 4)
    if manera == "largo":
        largo += azar.randint(100, 3000)
    proto = azar.choice([1, 6, 17])
    c = bytearray(struct.pack("!BBHHHBBH4s4s", (4 << 4) | ihl, 0, largo & 0xFFFF, azar.getrandbits(16), 0, 64, proto, 0,
                              bytes([192, 0, 2, azar.randint(1, 254)]), bytes([10, 0, 2, 15])))
    s_ = suma(bytes(c))
    if manera == "suma":
        s_ ^= 0x0101  # mal a propósito
    c[10:12] = struct.pack("!H", s_)
    return mac + origen + b"\x08\x00" + bytes(c) + datos


def lote():
    t = []
    for _ in range(3):
        t.append(bytes_al_azar(azar.randint(1, 13)))
    for _ in range(3):
        t.append(mac + origen + b"\x08\x00" + bytes_al_azar(azar.randint(1519 - 14, 2030 - 14)))
    t.append(mac + origen + b"\x88\xb5" + bytes_al_azar(azar.randint(9000, 65000)))
    for manera in ("suma", "corta", "largo"):
        t.append(ipv4(manera))
    # ARP con medidas imposibles (largo de dirección 0): nadie la toma.
    t.append(b"\xff" * 6 + origen + b"\x08\x06" + struct.pack("!HHBBH", 1, 0x0800, 0, 0, 1) + bytes_al_azar(20))
    t.append(mac + origen + struct.pack("!H", azar.choice([0x88B5, 0x88B6, 0x1234])) + bytes_al_azar(azar.randint(46, 1500)))
    return t




def tcp_de_dashos(t):
    # Ethernet + IPv4 desde 10.0.2.15 con protocolo 6.
    return len(t) >= 34 and t[12:14] == b"\x08\x00" and t[23] == 6 and t[26:30] == bytes([10, 0, 2, 15])


fin = time.time() + segundos
empieza = None
while time.time() < fin:
    try:
        while True:
            t = s.recv(70000)
            if empieza is None and tcp_de_dashos(t):
                empieza = time.time()
                fin = min(fin, empieza + 20)
    except (BlockingIOError, InterruptedError):
        pass
    if empieza is not None:
        for trama in lote():
            try:
                s.sendto(trama, qemu)
            except OSError:
                pass  # QEMU se paró
    time.sleep(0.2)
s.close()
os.unlink(yo)
