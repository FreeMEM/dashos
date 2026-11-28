#!/usr/bin/env python3
"""La caja Amiga en Linux (docs/CAJA_LINUX_LOOP.md): un compositor de Wayland de mentira para las pruebas, sin tocar el
escritorio de quien las corre (ni su ratón ni su portapapeles).

    compositor_falso.py <socket> <png> [acción...]

Escucha en `socket` (la caja lo usa con WAYLAND_DISPLAY=<socket>, un camino absoluto), da a un cliente lo mínimo que
pide la caja (wl_compositor, wl_shm, xdg_wm_base, wl_seat con ratón y una wl_output de 800x600) y, cuando la primera
ventana tiene su imagen, la guarda en `png` y hace las acciones, en orden:

    mover:X,Y        el ratón a (X, Y) en la ventana (píxeles de su búfer)
    clic:X,Y         mover y un clic del botón izquierdo
    esperar:S        S segundos
    foto:FICHERO     la ventana en ese PNG, como esté
    fotoultima:FICHERO  la última ventana que se abrió, en ese PNG
    fotos:PREFIJO    cada ventana en PREFIJO-1.png, PREFIJO-2.png...

Escribe por la salida lo que pasa ("ventana 320x200: Título", "clic en 5,5", "otra ventana 200x100: Título"...) y acaba
cuando el cliente se va.
Wayland, de su protocolo (wayland.xml y xdg-shell.xml): los mensajes son objeto (u32), tamaño << 16 | orden (u32) y
los argumentos; los descriptores van aparte (SCM_RIGHTS).
"""
import mmap
import os
import select
import socket
import struct
import sys
import time
import zlib

SOCK, PNG = sys.argv[1], sys.argv[2]
ACCIONES = sys.argv[3:]

objetos = {1: "wl_display"}          # id -> interfaz
pools = {}                           # id del pool -> (mmap, tamaño)
buferes = {}                         # id -> (pool, desplazamiento, ancho, alto, paso)
superficies = {}                     # id -> búfer adjunto
xdg_de = {}                          # xdg_surface -> wl_surface
top_de = {}                          # xdg_toplevel -> xdg_surface
titulos = {}
puntero = 0
serie = 1
GLOBALES = [("wl_compositor", 4), ("wl_shm", 1), ("xdg_wm_base", 2), ("wl_seat", 5), ("wl_output", 2)]


def decir(t):
    print(t, flush=True)


def evento(c, obj, orden, args=b"", fds=()):
    m = struct.pack("<II", obj, (8 + len(args)) << 16 | orden) + args
    anc = [(socket.SOL_SOCKET, socket.SCM_RIGHTS, struct.pack("<%di" % len(fds), *fds))] if fds else []
    c.sendmsg([m], anc)


def cadena(s):
    b = s.encode() + b"\0"
    return struct.pack("<I", len(b)) + b + b"\0" * (-len(b) % 4)


def leer_cadena(b, i):
    n = struct.unpack_from("<I", b, i)[0]
    s = b[i + 4:i + 4 + n - 1].decode(errors="replace")
    return s, i + 4 + n + (-n % 4)


def fijo(v):
    return struct.pack("<i", int(v * 256))


def png(fichero, sup):
    b = superficies.get(sup)
    if not b or b not in buferes:
        return False
    pool, desp, w, h, paso = buferes[b]
    m = pools[pool][0]
    filas = b"".join(b"\0" + b"".join(bytes((m[desp + y * paso + 4 * x + 2], m[desp + y * paso + 4 * x + 1],
                                             m[desp + y * paso + 4 * x])) for x in range(w)) for y in range(h))
    def trozo(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
    with open(fichero, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n" + trozo(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
                + trozo(b"IDAT", zlib.compress(filas, 6)) + trozo(b"IEND", b""))
    return True


def peticion(c, obj, orden, b, fds):
    global puntero, serie
    tipo = objetos.get(obj, "?")
    if tipo == "wl_display":
        nid = struct.unpack_from("<I", b)[0]
        if orden == 0:      # sync
            evento(c, nid, 0, struct.pack("<I", serie))
            evento(c, 1, 1, struct.pack("<I", nid))     # delete_id
        elif orden == 1:    # get_registry
            objetos[nid] = "wl_registry"
            for n, (nombre, ver) in enumerate(GLOBALES, 1):
                evento(c, nid, 0, struct.pack("<I", n) + cadena(nombre) + struct.pack("<I", ver))
    elif tipo == "wl_registry" and orden == 0:  # bind
        n = struct.unpack_from("<I", b)[0]
        nombre, i = leer_cadena(b, 4)
        _ver, nid = struct.unpack_from("<II", b, i)
        objetos[nid] = nombre
        if nombre == "wl_seat":
            evento(c, nid, 0, struct.pack("<I", 1))         # capabilities: pointer
        elif nombre == "wl_shm":
            evento(c, nid, 0, struct.pack("<I", 1))         # format XRGB8888
        elif nombre == "wl_output":
            evento(c, nid, 1, struct.pack("<Iiii", 3, 800, 600, 60000))   # mode: actual y preferido
            evento(c, nid, 2)                                              # done
    elif tipo == "wl_compositor" and orden == 0:
        objetos[struct.unpack_from("<I", b)[0]] = "wl_surface"
    elif tipo == "wl_shm" and orden == 0:    # create_pool(id, fd, size)
        nid, tam = struct.unpack_from("<Ii", b)
        objetos[nid] = "wl_shm_pool"
        pools[nid] = (mmap.mmap(fds.pop(0), tam, mmap.MAP_SHARED, mmap.PROT_READ), tam)
    elif tipo == "wl_shm_pool" and orden == 0:   # create_buffer
        nid, desp, w, h, paso, _fmt = struct.unpack_from("<Iiiiii", b)
        objetos[nid] = "wl_buffer"
        buferes[nid] = (obj, desp, w, h, paso)
    elif tipo == "wl_surface":
        if orden == 1:      # attach
            superficies[obj] = struct.unpack_from("<I", b)[0]
        elif orden == 3:    # frame
            nid = struct.unpack_from("<I", b)[0]
            evento(c, nid, 0, struct.pack("<I", int(time.time() * 1000) & 0xffffffff))
            evento(c, 1, 1, struct.pack("<I", nid))
    elif tipo == "xdg_wm_base" and orden == 2:   # get_xdg_surface(id, surface)
        nid, sup = struct.unpack_from("<II", b)
        objetos[nid] = "xdg_surface"
        xdg_de[nid] = sup
    elif tipo == "xdg_surface" and orden == 1:   # get_toplevel
        nid = struct.unpack_from("<I", b)[0]
        objetos[nid] = "xdg_toplevel"
        top_de[nid] = obj
        evento(c, nid, 0, struct.pack("<ii", 0, 0) + struct.pack("<I", 0))   # configure 0x0, sin estados
        serie += 1
        evento(c, obj, 0, struct.pack("<I", serie))                            # xdg_surface.configure
    elif tipo == "xdg_toplevel" and orden == 2:  # set_title
        titulos[obj], _ = leer_cadena(b, 0)
    elif tipo == "wl_seat" and orden == 0:       # get_pointer
        puntero = struct.unpack_from("<I", b)[0]
        objetos[puntero] = "wl_pointer"
    elif tipo == "wl_seat" and orden == 1:       # get_keyboard: sin teclado
        objetos[struct.unpack_from("<I", b)[0]] = "wl_keyboard"


def ventanas():
    """Las ventanas con imagen, en el orden en que se abrieron: (wl_surface, ancho, alto, título)."""
    v = []
    for top, xs in top_de.items():
        sup = xdg_de.get(xs)
        b = superficies.get(sup)
        if b in buferes:
            v.append((sup, buferes[b][2], buferes[b][3], titulos.get(top, "")))
    return v


def ventana():
    """La primera ventana con imagen, o None."""
    v = ventanas()
    return v[0] if v else None


def main():
    global serie
    if os.path.exists(SOCK):
        os.unlink(SOCK)
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.bind(SOCK)
    s.listen(1)
    s.settimeout(30)
    c, _ = s.accept()
    resto, fds, hecha, dentro = b"", [], False, False
    vistas = set()
    acciones = list(ACCIONES)
    siguiente = 0.0
    while True:
        r, _, _ = select.select([c], [], [], 0.05)
        if r:
            datos, anc, _, _ = c.recvmsg(65536, socket.CMSG_SPACE(28 * 4))
            if not datos:
                break
            for nivel, tipo, d in anc:
                if nivel == socket.SOL_SOCKET and tipo == socket.SCM_RIGHTS:
                    fds += list(struct.unpack("<%di" % (len(d) // 4), d[:len(d) - len(d) % 4]))
            resto += datos
            while len(resto) >= 8:
                obj, tam = struct.unpack_from("<II", resto)
                tam >>= 16
                if len(resto) < tam:
                    break
                peticion(c, obj, struct.unpack_from("<I", resto, 4)[0] & 0xffff, resto[8:tam], fds)
                resto = resto[tam:]
        for w in ventanas()[1:]:
            if w[0] not in vistas:
                vistas.add(w[0])
                decir("otra ventana %dx%d: %s" % (w[1], w[2], w[3]))
        v = ventana()
        if v and not hecha:
            hecha = True
            time.sleep(0.5)
            png(PNG, v[0])
            decir("ventana %dx%d: %s" % (v[1], v[2], v[3]))
            siguiente = time.time()
        if hecha and acciones and time.time() >= siguiente:
            a = acciones.pop(0)
            que, _, arg = a.partition(":")
            sup = v[0] if v else 0
            if que in ("mover", "clic"):
                x, y = (float(t) for t in arg.split(","))
                serie += 1
                if not dentro:
                    evento(c, puntero, 0, struct.pack("<II", serie, sup) + fijo(x) + fijo(y))      # enter
                    dentro = True
                else:
                    evento(c, puntero, 2, struct.pack("<I", 0) + fijo(x) + fijo(y))               # motion
                evento(c, puntero, 5)                                                              # frame
                if que == "clic":
                    for estado in (1, 0):
                        serie += 1
                        evento(c, puntero, 3, struct.pack("<IIII", serie, 0, 0x110, estado))      # button
                        evento(c, puntero, 5)
                        time.sleep(0.05)
                decir("%s en %s" % (que, arg))
                siguiente = time.time() + 0.3
            elif que == "esperar":
                siguiente = time.time() + float(arg)
            elif que == "foto":
                png(arg, sup)
                decir("foto: %s" % arg)
            elif que == "fotos":
                for k, w in enumerate(ventanas(), 1):
                    png("%s-%d.png" % (arg, k), w[0])
                decir("fotos: %s-*.png" % arg)
            elif que == "fotoultima":
                w = ventanas()
                if w:
                    png(arg, w[-1][0])
                decir("foto de la última: %s" % arg)
    decir("el cliente se fue")


if __name__ == "__main__":
    main()
