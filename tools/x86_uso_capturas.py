#!/usr/bin/env python3
"""Capturas de la imagen de usar de x86 (`make -C kernel x86-uso`), sin ventana, por QMP. No es una prueba: dice lo que
ve y deja las capturas para mirarlas.

Uso: x86_uso_capturas.py QMP SERIE CARPETA [SEGUNDOS]

Lo lanza quien arranca `tools/qemu_x86_a_mano.sh` con VENTANA=none y QMP=ruta, con la pantalla de 1280x720
(PANTALLA=1280x720: los sitios fijos de abajo son de ese tamaño, como los de `tools/demo_guion.py` de la rama `demo`).
Como una persona, con el teclado y la tableta USB de QEMU:

Antes de Ajustes, el Shell se cierra con el botón de su barra.

1. La pantalla de entrada (01). Entra Ana: el escritorio (02).
2. El lanzador de la barra con "shell" (03) e Intro: el Shell, con `List SYS:` (04).
3. Sus menús, dentro de la ventana: "Editar" abierto (05); `Ajuste menus barra`: en la barra (06), "Shell" abierto
   (07); `Ajuste menus dentro`.
4. El desplegable de procesos de la barra (08) y, desde su pie, Ajustes (el monitor del sistema, 09), maximizado; sus
   categorías Apariencia (10) y Acerca de (11).
5. El panel rápido, Super+A (12). Y el explorador desde el disco SYS: del Workbench (13).

Lo que hace, con la hora, en CARPETA/guion.txt. Las capturas, en PNG (QEMU 7.1 o más).
"""
import json
import os
import re
import socket
import sys
import time

qmp_ruta, serie, carpeta = sys.argv[1], sys.argv[2], sys.argv[3]
total = float(sys.argv[4]) if len(sys.argv) > 4 else 600
inicio = time.time()
# El tamaño de la pantalla, el de PANTALLA (como `qemu_x86_a_mano.sh`); los sitios fijos de abajo son de 1280x720.
ANCHO, ALTO = map(int, os.environ.get("PANTALLA", "1280x720").split("x"))
os.makedirs(carpeta, exist_ok=True)


def apuntar(t):
    with open(os.path.join(carpeta, "guion.txt"), "a") as f:
        f.write(f"{time.time() - inicio:6.1f} s: {t}\n")


class Qmp:
    def __init__(self, ruta):
        for _ in range(100):
            try:
                self.s = socket.socket(socket.AF_UNIX)
                self.s.connect(ruta)
                break
            except OSError:
                time.sleep(0.2)
        self.f = self.s.makefile("rwb")
        self.f.readline()
        self.orden("qmp_capabilities")

    def orden(self, nombre, **argumentos):
        m = {"execute": nombre}
        if argumentos:
            m["arguments"] = argumentos
        self.f.write(json.dumps(m).encode() + b"\n")
        self.f.flush()
        while True:
            r = json.loads(self.f.readline())
            if "return" in r or "error" in r:
                if "error" in r:
                    apuntar(f"QMP {nombre}: {r['error']}")
                return r


q = None


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


def largo():
    return len(texto())


def ir(x, y):
    q.orden("input-send-event", events=[
        {"type": "abs", "data": {"axis": "x", "value": int(x * 32767 / (ANCHO - 1))}},
        {"type": "abs", "data": {"axis": "y", "value": int(y * 32767 / (ALTO - 1))}}])
    time.sleep(0.15)


def clic(x, y, veces=1, boton="left"):
    ir(x, y)
    for _ in range(veces):
        q.orden("input-send-event", events=[{"type": "btn", "data": {"down": True, "button": boton}}])
        time.sleep(0.08)
        q.orden("input-send-event", events=[{"type": "btn", "data": {"down": False, "button": boton}}])
        time.sleep(0.12)


def tecla(*nombres):
    """Una combinación, con los nombres de QMP ("ctrl", "shift", "meta_l", "ret", "a"...)."""
    q.orden("send-key", keys=[{"type": "qcode", "data": n} for n in nombres])
    time.sleep(0.12)


# Teclado español: lo que hay que pulsar (posición de EE. UU.) para cada letra, como en `tools/guion_shell.py`.
ESPECIALES = {" ": ("spc",), "-": ("slash",), ".": ("dot",), ",": ("comma",), "/": ("shift", "7"), "=": ("shift", "0"),
              ":": ("shift", "dot"), "_": ("shift", "slash"), '"': ("shift", "2"), "?": ("shift", "minus")}


def teclear(t, intro=True):
    for c in t:
        if c in ESPECIALES:
            tecla(*ESPECIALES[c])
        elif c.isupper():
            tecla("shift", c.lower())
        else:
            tecla(c)
    if intro:
        tecla("ret")


def capturar(nombre, espera=2.0):
    time.sleep(espera)
    ruta = os.path.join(carpeta, nombre + ".png")
    r = q.orden("screendump", filename=os.path.abspath(ruta), format="png")
    apuntar(f"captura {nombre}" + ("" if "return" in r else " MAL"))


def ventana(de, desde=0):
    w = re.findall(r"\[compositor\] ventana de " + de + r" en \((-?\d+), (-?\d+)\)(?: geometría \([^)]*\))? de (\d+)x(\d+)",
                   texto()[desde:])
    return tuple(map(int, w[-1])) if w else None


def sitios(linea):
    return {t: (int(x), int(y)) for t, x, y in re.findall(r'"([^"]+)" en \((\d+), (\d+)\)', linea)}


def principal():
    global q
    q = Qmp(qmp_ruta)
    apuntar("QMP en marcha")
    # 1. La pantalla de entrada y Ana.
    if not esperar(r"\[guardian\] pantalla de entrada pintada", 300):
        capturar("00-sin-entrada", 0)
        return
    capturar("01-entrada", 1)
    teclear("Ana")
    time.sleep(0.5)
    teclear("clave de Ana")
    if not esperar(r"\[guardian\] entra Ana", 60):
        capturar("01b-no-entra", 0)
        return
    apuntar("entra Ana")
    esperar(r"\[barra\] botones: lanzador en", 60)
    ir(ANCHO - 40, ALTO - 40)
    capturar("02-escritorio", 6)
    # 2. El lanzador y el Shell.
    bb = esperar(r"\[barra\] botones: lanzador en \((\d+), (\d+)\)", 30)
    if not bb:
        return
    for _ in range(3):
        abiertos = texto().count("[barra] lanzador abierto")
        clic(int(bb[-1][0]), int(bb[-1][1]))
        hasta = time.time() + 10
        while time.time() < hasta and texto().count("[barra] lanzador abierto") <= abiertos:
            time.sleep(0.25)
        if texto().count("[barra] lanzador abierto") > abiertos:
            break
        apuntar("el lanzador no se abre: otra vez")
    time.sleep(0.5)
    for c in "shell":
        tecla(c)
        time.sleep(0.15)
    esperar(r'\[barra\] busca "shell": ', 15)
    capturar("03-lanzador", 1)
    desde = largo()
    tecla("ret")
    m = esperar(r"(\[shell\] menús de la ventana: [^\n]*)", 60, desde)
    v = ventana("[Ss]hell", desde)
    if not v:
        capturar("04-sin-shell", 0)
        return
    sx, sy, sw, sh = v
    apuntar(f"el Shell en ({sx}, {sy}) de {sw}x{sh}")
    time.sleep(2)
    clic(sx + 120, max(sy - 20, 1))
    ir(ANCHO - 40, ALTO - 40)
    teclear("List SYS:")
    capturar("04-shell", 3)
    # 3. Los menús, dentro y en la barra.
    menu = sitios(m[-1]) if m else {}
    apuntar(f"menús del Shell: {menu}")
    if "Editar" in menu:
        x, y = menu["Editar"]
        clic(sx + x, sy + y)
        ir(sx + x + 40, sy + y + 70)
        capturar("05-menu-dentro")
        tecla("esc")
        time.sleep(0.5)
    desde = largo()
    teclear("Ajuste menus barra")
    b = esperar(r"(\[barra\] menús en la barra: [^\n]*)", 30, desde)
    capturar("06-menus-en-la-barra")
    barra = sitios(b[-1]) if b else {}
    if "Shell" in barra:
        clic(*barra["Shell"])
        esperar(r"\[barra\] menú abierto: Shell", 15, desde)
        capturar("07-menu-en-la-barra")
        tecla("esc")
        time.sleep(0.5)
    clic(sx + 120, max(sy - 20, 1))
    teclear("Ajuste menus dentro")
    time.sleep(2)
    # El Shell se cierra con el botón de su barra (a 20 del borde derecho, en medio de la barra de 40): así Ajustes
    # sale solo.
    clic(sx + sw - 20, max(sy - 20, 1))
    esperar(r"\[shell\] [^\n]*(cierra|cerrad)", 10)
    time.sleep(1)
    # 4. El desplegable de procesos y Ajustes (el monitor del sistema), maximizado.
    abiertos = largo()
    p = esperar(r"\[barra\] CPU y memoria: [^\n]*; en \((\d+), (\d+)\)", 10)
    px, py = (int(p[-1][0]), int(p[-1][1])) if p else (1090, 20)
    clic(px, py)
    d = esperar(r"\[barra\] procesos abiertos: [^\n]*abrir el monitor en \((\d+), (\d+)\)", 15, abiertos)
    capturar("08-procesos")
    centro = largo()
    # El botón del pie, donde dice la barra (en el desplegable, con su esquina en (762, 50) a 1280 de ancho).
    clic(*((762 + int(d[-1][0]), 50 + int(d[-1][1])) if d else (1028, 452)))
    esperar(r"\[compositor\] ventana de centro en", 20, centro)
    w = ventana("centro", centro)
    if w:
        x, y, an, _ = w
        clic(x + an // 2, max(y - 20, 1))
        tecla("meta_l", "up")
    capturar("09-ajustes-monitor", 4)
    clic(90, 315)
    capturar("10-ajustes-apariencia", 3)
    clic(90, 483)
    capturar("11-ajustes-acerca", 3)
    clic(1223, 383)
    time.sleep(2)
    # 5. El panel rápido y el explorador.
    tecla("meta_l", "a")
    esperar(r"\[panel\] abierto", 10)
    capturar("12-panel", 2)
    # Se cierra con Super+A otra vez.
    tecla("meta_l", "a")
    esperar(r"\[panel\] cerrado", 10)
    time.sleep(1)
    clic(1227, 87, veces=2)
    esperar(r"\[explorador\] ventana \d+ en pantalla", 20)
    capturar("13-explorador", 4)
    apuntar("hecho")


try:
    principal()
except Exception as e:
    apuntar(f"falla el guion: {e!r}")
