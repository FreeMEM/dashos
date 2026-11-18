#!/usr/bin/env python3
"""Las capturas del sonido (rama sonido-pro, docs/bitacoras/sonido-pro.md), por el monitor de QEMU, con la imagen de
usar y el reproductor de prueba (`melodia`), como lo haría una persona.

Uso (lo lanza `tools/prueba_uefi.sh` con SONIDO_CAPTURAS=prefijo): guion_sonido.py MONITOR SEGUNDOS SERIE PREFIJO

1. Entra Ana (con DMS_SHIVA=1, el tema oscuro y la barra de shiva, como `guion_dms.py`).
2. Super+A: el centro de control; el cuerpo del mosaico de la salida abre su detalle, con lo que suena
   (PREFIJO.1-panel-salida). Su última fila abre la ventana "Sonido".
3. La ventana "Sonido" con la música sonando: programas, salidas, vúmetros, espectro (PREFIJO.2-mezclador).
4. El preajuste "Graves" del ecualizador (PREFIJO.3-mezclador-graves). La hora del clic, en el .guion.txt: con ella,
   `tools/sonido_partir.py` saca del WAV un trozo de antes y otro de después.
5. Pausa (PREFIJO.4-mezclador-pausa) y vuelve a sonar.

Cada captura queda también en PNG. Lo que hace, con la hora, en PREFIJO.guion.txt.
"""
import os
import re
import socket
import sys
import time

ruta, total, serie, prefijo = sys.argv[1], float(sys.argv[2]), sys.argv[3], sys.argv[4]
inicio = time.time()
ANCHO = int(os.environ.get("DMS_ANCHO", "1920"))


def apuntar(texto_):
    with open(prefijo + ".guion.txt", "a") as f:
        f.write(f"{time.time() - inicio:6.1f} s: {texto_}\n")


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


def esperar(patron, segundos, desde=0):
    hasta = time.time() + segundos
    while time.time() < hasta and time.time() - inicio < total:
        m = re.findall(patron, texto()[desde:])
        if m:
            return m
        time.sleep(0.25)
    apuntar(f"no sale: {patron}")
    return None


def teclear(orden, intro=True):
    especiales = {" ": "spc", "-": "slash", ".": "dot", "/": "shift-7", ":": "shift-dot"}
    teclas = ["sendkey " + (especiales.get(c) or ("shift-" + c.lower() if c.isupper() else c)) for c in orden]
    if intro:
        teclas.append("sendkey ret")
    mandar(*teclas, pausa=0.12)


def ir_a(x, y):
    mandar(*["mouse_move -100 -100"] * 30, pausa=0.03)
    pasos = []
    while x > 0 or y > 0:
        dx, dy = min(x, 100), min(y, 100)
        pasos.append(f"mouse_move {dx} {dy}")
        x, y = x - dx, y - dy
    mandar(*pasos, pausa=0.05)
    time.sleep(0.3)


def clic(x, y):
    ir_a(x, y)
    mandar("mouse_button 1", "mouse_button 0", pausa=0.1)


def capturar(nombre, espera=1.5):
    time.sleep(espera)
    ppm = f"{prefijo}.{nombre}.ppm"
    mandar("screendump " + ppm)
    time.sleep(1)
    try:
        from PIL import Image
        Image.open(ppm).save(ppm[:-4] + ".png")
        os.remove(ppm)
    except Exception as e:
        apuntar(f"sin PNG de {nombre}: {e}")
    apuntar(f"captura {nombre}")


def lanzar(nombre):
    bb = esperar(r"\[barra\] botones: lanzador en \((\d+), (\d+)\)", 10)
    if not bb:
        return False
    abiertos = texto().count("[barra] lanzador abierto")
    clic(int(bb[-1][0]), int(bb[-1][1]))
    hasta = time.time() + 10
    while time.time() < hasta and texto().count("[barra] lanzador abierto") <= abiertos:
        time.sleep(0.25)
    time.sleep(0.5)
    mandar(*["sendkey " + c for c in nombre], pausa=0.25)
    esperar(r'\[barra\] busca "%s": ' % nombre, 15)
    time.sleep(0.5)
    mandar("sendkey ret")
    return True


def shiva():
    marca = len(texto())
    if not lanzar("shell") or not esperar(r"\[shell\] ventana en pantalla", 60, marca):
        return
    v = esperar(r"\[compositor\] ventana de [Ss]hell en \((\d+), (\d+)\)", 10, marca)
    time.sleep(2)
    if v:
        clic(int(v[-1][0]) + 120, max(int(v[-1][1]) - 20, 1))
    teclear("Ajuste tema oscuro")
    time.sleep(2)
    teclear("Ajuste DMS SYS:Prefs/Presets/DMS/shiva.json")
    esperar(r"\[barra\] barras: 1 configuradas", 15, marca)
    time.sleep(2)
    apuntar("perfil de shiva puesto")


def principal():
    time.sleep(3)
    if not esperar(r"\[guardian\] pantalla de entrada pintada", 240):
        return
    teclear("Ana")
    time.sleep(0.5)
    teclear("clave de Ana")
    if not esperar(r"\[guardian\] entra Ana", 60):
        return
    apuntar("entra Ana")
    if os.environ.get("DMS_SHIVA"):
        shiva()
    esperar(r"\[sonido\] melodia dice lo que suena", 30)
    time.sleep(2)
    # 2. El centro de control y el detalle de la salida.
    marca = len(texto())
    mandar("sendkey meta_l-a")
    s = esperar(r"\[panel\] salida a \((\d+), (\d+)\)", 15, marca)
    capa = esperar(r"\[compositor\] capa de panel en \((\d+), (\d+)\)", 5, marca)
    dy = int(capa[-1][1]) - 8 if capa else 40
    if not s:
        return
    marca = len(texto())
    clic(ANCHO - int(s[-1][0]), int(s[-1][1]) + dy)
    m = esperar(r"\[panel\] detalle de la salida: \d+ filas; el mezclador a \((\d+), (\d+)\), sus ajustes a \((\d+), (\d+)\)", 10, marca)
    ir_a(10, 1000)
    capturar("1-panel-salida")
    if not m:
        return
    marca = len(texto())
    clic(ANCHO - int(m[-1][0]), int(m[-1][1]) + dy)
    # 3. La ventana "Sonido".
    if not esperar(r"\[mezclador\] ventana Sonido en pantalla", 20, marca):
        return
    v = esperar(r"\[compositor\] ventana de mezclador en \((-?\d+), (-?\d+)\)", 5, marca)
    sitios = esperar(r"\[mezclador\] sitios: graves \((\d+), (\d+)\) plano \((\d+), (\d+)\) auriculares \((\d+), (\d+)\) espacial-apagado \((\d+), (\d+)\) pausa \((\d+), (\d+)\)", 10, marca)
    ir_a(10, 1060)
    capturar("2-mezclador", 2)
    if not (v and sitios):
        return
    vx, vy = int(v[-1][0]), int(v[-1][1])
    g = sitios[-1]
    # 4. Graves.
    time.sleep(2)
    apuntar("ecualizador: Graves (antes, sin ecualizar)")
    clic(vx + int(g[0]), vy + int(g[1]))
    esperar(r"\[mezclador\] ecualizador: Graves", 10, marca)
    apuntar("ecualizador: Graves puesto")
    ir_a(10, 1060)
    capturar("3-mezclador-graves", 3)
    time.sleep(3)
    apuntar("ecualizador: Graves (después)")
    # 5. Pausa y otra vez a sonar.
    marca = len(texto())
    clic(vx + int(g[8]), vy + int(g[9]))
    esperar(r"\[melodia\] orden 1: en pausa", 10, marca)
    ir_a(10, 1060)
    capturar("4-mezclador-pausa", 2.5)
    clic(vx + int(g[8]), vy + int(g[9]))
    esperar(r"\[melodia\] orden 1: suena", 10, marca)
    # 6. El OSD del volumen, que dice por dónde suena (la tecla de bajar y la de subir: queda como estaba).
    marca = len(texto())
    mandar("sendkey volumedown")
    time.sleep(0.6)
    capturar("5-osd-volumen", 0.3)
    mandar("sendkey volumeup")
    time.sleep(1)
    # 7. Ajustes > Sonido, desde la primera fila del detalle de la salida.
    marca = len(texto())
    mandar("sendkey meta_l-a")
    s2 = esperar(r"\[panel\] salida a \((\d+), (\d+)\)", 15, marca)
    if s2:
        marca = len(texto())
        clic(ANCHO - int(s2[-1][0]), int(s2[-1][1]) + dy)
        m2 = esperar(r"\[panel\] detalle de la salida: .* sus ajustes a \((\d+), (\d+)\)", 25, marca)
        if m2:
            clic(ANCHO - int(m2[-1][0]), int(m2[-1][1]) + dy)
            esperar(r"\[centro\] Sonido en pantalla", 20, marca)
            ir_a(10, 1060)
            capturar("6-ajustes-sonido", 2)
    # 8. La pantalla de bloqueo, con lo que suena (Super+L), y vuelta.
    marca = len(texto())
    mandar("sendkey meta_l-l")
    if esperar(r"\[guardian\] pantalla de bloqueo pintada", 30, marca):
        capturar("7-bloqueo-musica", 2)
        teclear("clave de Ana")
        esperar(r"\[guardian\] desbloqueado", 30, marca)
    apuntar("hecho")


try:
    principal()
except Exception as e:
    apuntar(f"error: {e}")
