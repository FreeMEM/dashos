#!/usr/bin/env python3
"""Las capturas de S27, oleada 3 (avisos, el Dash del reloj y el centro de control, como DMS;
docs/bitacoras/dms-avisos.md), por el monitor de QEMU, con la imagen de usar (`virt-uso`), como lo haría una persona.

Uso (lo lanza `tools/prueba_uefi.sh` con DMS_CAPTURAS=prefijo): guion_dms.py MONITOR SEGUNDOS SERIE PREFIJO

1. Entra Ana. Abre el lanzador y lanza `avisa`, que avisa con acciones: el emergente (PREFIJO.1-emergente) y, con el
   ratón encima, su cruz (PREFIJO.2-emergente-encima). Pulsa su botón "Deshacer" (le llega a `avisa`).
2. La campana: el centro de avisos (PREFIJO.3-centro) y su pestaña Historial (PREFIJO.4-historial). Lo cierra.
3. El reloj: el Dash (PREFIJO.5-dash) y el mes siguiente (PREFIJO.6-dash-mes). Esc.
4. Super+A: el centro de control (PREFIJO.7-panel), el detalle de la red (PREFIJO.8-panel-detalle) y el modo de
   edición (PREFIJO.9-panel-edicion); Descartar y Super+A.

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
    especiales = {" ": "spc"}
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


def capa_tras(marca):
    t = texto()
    i = t.rfind(marca)
    return re.findall(r"\[compositor\] capa de barra en \((-?\d+), (-?\d+)\)", t[i:]) if i >= 0 else []


def principal():
    time.sleep(3)
    if not esperar(r"\[guardian\] pantalla de entrada pintada", 240):
        return
    time.sleep(1)
    teclear("Ana")
    time.sleep(0.5)
    teclear("clave de Ana")
    if not esperar(r"\[guardian\] entra Ana", 60):
        return
    apuntar("entra Ana")
    # 1. El lanzador: "avisa" e Intro.
    for vez in range(3):
        bb = esperar(r"\[barra\] botones: lanzador en \((\d+), (\d+)\)", 60)
        if not bb:
            return
        abiertos = texto().count("[barra] lanzador abierto")
        time.sleep(1)
        clic(int(bb[-1][0]), int(bb[-1][1]))
        hasta = time.time() + 10
        while time.time() < hasta and texto().count("[barra] lanzador abierto") <= abiertos:
            time.sleep(0.25)
        if texto().count("[barra] lanzador abierto") > abiertos:
            break
    time.sleep(0.5)
    mandar(*["sendkey " + c for c in "avisa"], pausa=0.25)
    esperar(r'\[barra\] busca "avisa": ', 15)
    time.sleep(0.5)
    ir_a(ANCHO - 10, 700)
    mandar("sendkey ret")
    e = esperar(r'\[barra\] emergente \d+ de avisa: .*tarjeta \((\d+), (\d+)\).*Deshacer \((\d+), (\d+)\)', 30)
    time.sleep(0.3)
    capa = capa_tras("de avisa: \"Copia terminada\"")
    capturar("1-emergente", 0.8)
    if e and capa:
        x0, y0 = int(capa[0][0]), int(capa[0][1])
        tx, ty, bx, by = map(int, e[-1])
        ir_a(x0 + tx, y0 + ty)
        capturar("2-emergente-encima", 0.6)
        clic(x0 + bx, y0 + by)
        esperar(r"\[avisa\] acción pulsada", 10)
    # 2. La campana: el centro de avisos y su historial.
    ca = esperar(r"\[barra\] campana en \((\d+), (\d+)\)", 5)
    if not ca:
        return
    marca = len(texto())
    clic(int(ca[-1][0]), int(ca[-1][1]))
    c = esperar(r"\[barra\] centro de avisos abierto: .*Historial \((\d+), (\d+)\)", 10, marca)
    ir_a(ANCHO - 10, 1000)
    capturar("3-centro")
    capa = capa_tras("[barra] centro de avisos abierto")
    if c and capa:
        clic(int(capa[0][0]) + int(c[-1][0]), int(capa[0][1]) + int(c[-1][1]))
        ir_a(ANCHO - 10, 1000)
        capturar("4-historial")
    clic(int(ca[-1][0]), int(ca[-1][1]))
    time.sleep(1)
    # 3. El reloj: el Dash.
    rl = esperar(r"\[barra\] reloj en \((\d+), (\d+)\)", 5)
    if rl:
        marca = len(texto())
        clic(int(rl[-1][0]), int(rl[-1][1]))
        d = esperar(r"\[barra\] Dash: tarjeta de .*siguiente \((\d+), (\d+)\)", 10, marca)
        ir_a(10, 1000)
        capturar("5-dash")
        capa = capa_tras("[barra] Dash abierto")
        if d and capa:
            clic(int(capa[0][0]) + int(d[-1][0]), int(capa[0][1]) + int(d[-1][1]))
            ir_a(10, 1000)
            capturar("6-dash-mes")
        mandar("sendkey esc")
        time.sleep(1)
    # 4. El centro de control.
    marca = len(texto())
    mandar("sendkey meta_l-a")
    p = esperar(r"\[panel\] se ven: .*editar a \((\d+), (\d+)\)", 15, marca)
    capa = esperar(r"\[compositor\] capa de panel en \((\d+), (\d+)\)", 5, marca)
    dy = int(capa[-1][1]) - 8 if capa else 40
    ir_a(10, 1000)
    capturar("7-panel")
    red = re.findall(r"\[panel\] red a \((\d+), (\d+)\)", texto()[marca:])
    if red:
        clic(ANCHO - int(red[-1][0]), int(red[-1][1]) + dy)
        ir_a(10, 1000)
        capturar("8-panel-detalle")
        clic(ANCHO - int(red[-1][0]), int(red[-1][1]) + dy)
    if p:
        clic(ANCHO - int(p[-1][0]), int(p[-1][1]) + dy)
        ir_a(10, 1000)
        capturar("9-panel-edicion")
        h = esperar(r"\[panel\] edición \d+: hecho \((\d+), (\d+)\)", 5)
        if h:
            clic(ANCHO - int(h[-1][0]), int(h[-1][1]) + dy)
    time.sleep(1)
    mandar("sendkey meta_l-a")
    apuntar("hecho")


try:
    principal()
except Exception as e:
    apuntar(f"falla el guion: {e!r}")
