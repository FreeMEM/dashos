#!/usr/bin/env python3
"""Las teclas y los clics de `make arranque-virt-iconos` (plan de iconos, parte 3; docs/bitacoras/iconos-cajas.md), por
el monitor de QEMU. La imagen es la de usar, con el disco de pruebas (los `.info` de `tools/iconos_amiga_prueba.py` en
el cajón de la caja Amiga).

Uso (lo lanza `tools/prueba_uefi.sh` con ICONOS_CAPTURAS=prefijo): guion_iconos.py MONITOR SEGUNDOS SERIE PREFIJO

1. Entra Ana; doble clic en el icono de SYS: del Workbench: el explorador.
2. `Amiga:Iconos` (Ctrl+L): un `.info` suelto de cada formato, cada uno con su icono (01); un clic en el de GlowIcons:
   se ve su imagen de seleccionado (02).
3. `Amiga:`: el cajón `Iconos` con el icono de su `Iconos.info`, que no se ve (ni el `Disk.info`); el volumen de la
   barra lateral con el de `Disk.info` (03). Ctrl+H: se ven los dos, cada uno con su icono (04). Ctrl+2: la lista, con
   los iconos reducidos a 28 (05).
4. Al final escribe el camino `Amiga:Iconos/Hechos` (no existe): la línea que dice el explorador marca que el guion
   llegó al final con todo bien.

Cada captura queda en PPM y en PNG. Lo que hace el guion, con la hora, en PREFIJO.guion.txt.
"""
import re
import socket
import sys
import time

ruta, total, serie, prefijo = sys.argv[1], float(sys.argv[2]), sys.argv[3], sys.argv[4]
inicio = time.time()


def apuntar(t):
    with open(prefijo + ".guion.txt", "a") as f:
        f.write(f"{time.time() - inicio:6.1f} s: {t}\n")


def mandar(*ordenes, pausa=0.05):
    s = socket.socket(socket.AF_UNIX)
    s.connect(ruta)
    for o in ordenes:
        s.sendall(o.encode() + b"\n")
        time.sleep(pausa)
    s.close()


def texto():
    try:
        return re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", open(serie, errors="replace").read())
    except OSError:
        return ""


def largo():
    return len(texto())


class Fallo(Exception):
    pass


def debe(patron, segundos, desde=0, que=None):
    hasta = time.time() + segundos
    while time.time() < hasta and time.time() - inicio < total:
        m = re.findall(patron, texto()[desde:])
        if m:
            return m
        time.sleep(0.25)
    raise Fallo(que or f"no sale: {patron}")


ESPECIALES = {" ": "spc", "-": "slash", ".": "dot", ",": "comma", "/": "shift-7", ":": "shift-dot"}


def teclear(t, intro=True):
    teclas = ["sendkey " + (ESPECIALES[c] if c in ESPECIALES else "shift-" + c.lower() if c.isupper() else c) for c in t]
    if intro:
        teclas.append("sendkey ret")
    mandar(*teclas, pausa=0.12)


def teclas(*nombres):
    mandar(*["sendkey " + n for n in nombres], pausa=0.25)


def ir_a(x, y):
    mandar(*["mouse_move -100 -100"] * 20, pausa=0.03)
    pasos = []
    while x > 0 or y > 0:
        dx, dy = min(x, 100), min(y, 100)
        pasos.append(f"mouse_move {dx} {dy}")
        x, y = x - dx, y - dy
    mandar(*pasos, pausa=0.05)
    time.sleep(0.3)


def clic(x, y, doble=False):
    ir_a(x, y)
    veces = 2 if doble else 1
    mandar(*["mouse_button 1", "mouse_button 0"] * veces, pausa=0.08)


def capturar(nombre, espera=1.5):
    time.sleep(espera)
    ppm = f"{prefijo}.{nombre}.ppm"
    mandar("screendump " + ppm)
    time.sleep(1)
    try:
        from PIL import Image
        Image.open(ppm).save(ppm[:-4] + ".png")
    except Exception as e:
        apuntar(f"sin PNG de {nombre}: {e}")
    apuntar(f"captura {nombre}")


def camino(t):
    teclas("ctrl-l")
    time.sleep(0.4)
    teclas("ctrl-a")
    teclear(t)


def principal():
    time.sleep(3)
    debe(r"\[guardian\] pantalla de entrada pintada", 200)
    time.sleep(1)
    teclear("Ana")
    time.sleep(0.5)
    teclear("clave de Ana")
    debe(r"\[guardian\] entra Ana", 60)
    apuntar("entra Ana")
    # 1. El explorador, con un doble clic en SYS: del Workbench.
    for vez in range(3):
        sys_ = debe(r"\[workbench\] (?:en pantalla: \d+x\d+, )?icono de SYS en \((\d+), (\d+)\)", 90)[-1]
        capa = debe(r"\[compositor\] capa de workbench en \((\d+), (\d+)\)", 30)[-1]
        desde = largo()
        clic(int(capa[0]) + int(sys_[0]), int(capa[1]) + int(sys_[1]), doble=True)
        try:
            debe(r"\[explorador\] ventana 1 en SYS:,", 20, desde)
            break
        except Fallo:
            apuntar("el doble clic en SYS: no abre el explorador: otra vez")
    else:
        raise Fallo("el explorador no se abre")
    origen = debe(r"\[compositor\] ventana de explorador en \((\d+), (\d+)\)", 20)[-1]
    ox, oy = int(origen[0]), int(origen[1])
    apuntar(f"el explorador en ({ox}, {oy})")
    time.sleep(1)
    # 2. Amiga:Iconos.
    desde = largo()
    camino("Amiga:Iconos")
    linea = debe(r"\[explorador\] ventana 1 en Amiga:Iconos, ([^\n]*)", 20, desde)[-1]
    apuntar(f"Amiga:Iconos: {linea}")
    debe(r"\[explorador\] icono de Amiga:Iconos/Roto\.info", 20, desde, "no lee los .info de Amiga:Iconos")
    capturar("01-iconos")
    g = debe(r"\[explorador\] ventana 1, rejilla: [^\n]*?Brillo\.info en \((\d+), (\d+)\)", 10, desde)[-1]
    clic(ox + int(g[0]), oy + int(g[1]))
    ir_a(ox + 8, oy + 300)
    capturar("02-elegido")
    # 3. Amiga: (el volumen), sin y con los ocultos, y como lista.
    desde = largo()
    camino("Amiga:")
    raiz = debe(r"\[explorador\] ventana 1 en Amiga:, ([^\n]*)", 20, desde)[-1]
    apuntar(f"Amiga: {raiz}")
    nombres = raiz.split(": ", 1)[-1].split(" | ")
    if "Iconos" not in nombres or "Iconos.info" in nombres or "Disk.info" in nombres:
        raise Fallo(f"en Amiga: tendría que verse Iconos y no sus .info: {nombres}")
    debe(r"\[explorador\] icono de Amiga:Iconos\.info", 20, desde, "no lee Iconos.info")
    capturar("03-volumen")
    desde = largo()
    teclas("ctrl-h")
    raiz = debe(r"\[explorador\] ventana 1 en Amiga:, ([^\n]*)", 20, desde)[-1]
    nombres = raiz.split(": ", 1)[-1].split(" | ")
    if "Iconos.info" not in nombres or "Disk.info" not in nombres:
        raise Fallo(f"con los ocultos tendrían que verse Iconos.info y Disk.info: {nombres}")
    debe(r"\[explorador\] icono de Amiga:Disk\.info", 20, desde, "no lee Disk.info como fichero")
    capturar("04-ocultos")
    desde = largo()
    teclas("ctrl-2")
    debe(r"\[explorador\] ventana 1 como lista", 20, desde)
    capturar("05-lista")
    # 4. Todo bien: la marca del final.
    camino("Amiga:Iconos/Hechos")
    apuntar("acabado")


try:
    principal()
except Fallo as e:
    apuntar(f"FALLA: {e}")
except Exception as e:
    apuntar(f"falla el guion: {e!r}")
