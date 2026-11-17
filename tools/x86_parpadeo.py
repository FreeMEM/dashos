#!/usr/bin/env python3
"""Mide el parpadeo de la pantalla de la imagen de usar de x86 (`make -C kernel x86-uso`), sin ventana, por QMP. No es
una prueba que pase o falle: cuenta y deja los cuadros malos para mirarlos.

Uso: x86_parpadeo.py QMP SERIE CARPETA [SEGUNDOS]

Como `tools/x86_uso_capturas.py` (QEMU con VENTANA=none, QMP=ruta y PANTALLA=1280x720): entra Ana y, mientras se usa el
escritorio (abrir y cerrar el Shell desde el lanzador, el panel rápido, el desplegable de procesos, mover el ratón), un
hilo saca capturas seguidas (`screendump`, lo que QEMU enseñaría en la ventana en ese momento) durante SEGUNDOS.

Un cuadro es "pasajero" donde se diferencia del anterior y del siguiente y esos dos son iguales entre sí: algo que
salió y se fue en un solo cuadro (una ventana que desaparece un momento, la pantalla que se va a negro o al color de
fondo). Si lo pasajero pasa del 2 % de la pantalla, el cuadro cuenta como parpadeo y se guarda con sus vecinos en
CARPETA (PNG). El ratón que va y vuelve también es pasajero, pero es mucho menos del 2 %.

Con VIDEO=ruta (QEMU con VENTANA=gtk en un X sin pantalla, `Xvfb`, en DISPLAY), en vez de las capturas por QMP se
graba con ffmpeg lo que pinta la ventana de GTK, a 60 cuadros por segundo y sin pérdidas, y se mira igual después.

Lo que dice, en CARPETA/guion.txt y por la salida: cuadros, cuadros por segundo y parpadeos.
"""
import json
import os
import re
import socket
import subprocess
import sys
import threading
import time

import numpy as np
from PIL import Image

qmp_ruta, serie, carpeta = sys.argv[1], sys.argv[2], sys.argv[3]
segundos = float(sys.argv[4]) if len(sys.argv) > 4 else 60
inicio = time.time()
ANCHO, ALTO = (int(v) for v in os.environ.get("PANTALLA", "1280x720").split("x"))
UMBRAL = float(os.environ.get("UMBRAL", "0.02"))
# Con VIDEO=ruta.mkv (y QEMU con ventana en un Xvfb, en DISPLAY): se graba la pantalla de X en vez de sacar capturas
# por QMP. VIDEO_TAM, lo que se graba desde la esquina (la ventana de QEMU, con su menú).
VIDEO = os.environ.get("VIDEO")
VIDEO_TAM = os.environ.get("VIDEO_TAM", "1920x1200")
os.makedirs(carpeta, exist_ok=True)
TEMPORAL = os.path.join("/dev/shm" if os.path.isdir("/dev/shm") else carpeta, f"parpadeo-{os.getpid()}.ppm")


def apuntar(t):
    linea = f"{time.time() - inicio:6.1f} s: {t}"
    print(linea, flush=True)
    with open(os.path.join(carpeta, "guion.txt"), "a") as f:
        f.write(linea + "\n")


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
        self.cerrojo = threading.Lock()
        self.orden("qmp_capabilities")

    def orden(self, nombre, **argumentos):
        m = {"execute": nombre}
        if argumentos:
            m["arguments"] = argumentos
        with self.cerrojo:
            self.f.write(json.dumps(m).encode() + b"\n")
            self.f.flush()
            while True:
                r = json.loads(self.f.readline())
                if "return" in r or "error" in r:
                    return r


q = None


def texto():
    try:
        return re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", open(serie, errors="replace").read())
    except OSError:
        return ""


def esperar(patron, plazo, desde=0):
    hasta = time.time() + plazo
    while time.time() < hasta:
        m = re.findall(patron, texto()[desde:])
        if m:
            return m
        time.sleep(0.25)
    apuntar(f"no sale: {patron}")
    return None


def ir(x, y):
    q.orden("input-send-event", events=[
        {"type": "abs", "data": {"axis": "x", "value": int(x * 32767 / (ANCHO - 1))}},
        {"type": "abs", "data": {"axis": "y", "value": int(y * 32767 / (ALTO - 1))}}])
    time.sleep(0.05)


def clic(x, y):
    ir(x, y)
    time.sleep(0.1)
    q.orden("input-send-event", events=[{"type": "btn", "data": {"down": True, "button": "left"}}])
    time.sleep(0.08)
    q.orden("input-send-event", events=[{"type": "btn", "data": {"down": False, "button": "left"}}])
    time.sleep(0.12)


def tecla(*nombres):
    q.orden("send-key", keys=[{"type": "qcode", "data": n} for n in nombres])
    time.sleep(0.12)


ESPECIALES = {" ": ("spc",)}


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


def ventana(de, desde=0):
    w = re.findall(r"\[compositor\] ventana de " + de + r" en \((-?\d+), (-?\d+)\)(?: geometría \([^)]*\))? de (\d+)x(\d+)",
                   texto()[desde:])
    return tuple(map(int, w[-1])) if w else None


class Capturas(threading.Thread):
    """Saca capturas seguidas y cuenta los cuadros pasajeros. Guarda solo los malos (y sus vecinos)."""

    def __init__(self):
        super().__init__(daemon=True)
        self.parar = False
        self.cuadros = 0
        self.malos = []
        self.peor = 0.0

    def captura(self):
        r = q.orden("screendump", filename=TEMPORAL)
        if "return" not in r:
            return None
        t = time.time() - inicio
        with Image.open(TEMPORAL) as im:
            return t, np.asarray(im.convert("RGB")).copy()

    def video(self):
        """Los cuadros del vídeo, uno tras otro, por el mismo camino que las capturas."""
        an, al = (int(v) for v in VIDEO_TAM.split("x"))
        p = subprocess.Popen(["ffmpeg", "-loglevel", "error", "-i", VIDEO, "-f", "rawvideo", "-pix_fmt", "rgb0", "-"],
                             stdout=subprocess.PIPE)
        k = 0
        # Cada píxel en una palabra de 32 bits (R, G, B y un byte a cero): una comparación por píxel.
        fuente = iter(lambda: p.stdout.read(an * al * 4), b"")
        self.mirar((k / 60, np.frombuffer(b, np.uint32).reshape(al, an)) for k, b in enumerate(fuente) if len(b) == an * al * 4)
        p.wait()

    def run(self):
        self.mirar(c for c in iter(self.captura_o_fin, False) if c is not None)

    def captura_o_fin(self):
        return False if self.parar else self.captura()

    def mirar(self, cuadros):
        previos = []
        for c in cuadros:
            self.cuadros += 1
            previos.append(c)
            if len(previos) < 3:
                continue
            previos = previos[-3:]
            (t0, a), (t1, b), (t2, c2) = previos
            if a.shape != b.shape or b.shape != c2.shape:
                continue
            ab, bc, ac = (np.any(x != y, axis=2) if x.ndim == 3 else x != y for x, y in ((a, b), (b, c2), (a, c2)))
            pasajero = ab & bc & ~ac
            parte = pasajero.mean()
            self.peor = max(self.peor, parte)
            if parte > UMBRAL:
                n = len(self.malos)
                self.malos.append((t1, parte))
                ys, xs = np.nonzero(pasajero)
                apuntar(f"parpadeo {n}: a los {t1:.2f} s, {parte * 100:.1f} % de la pantalla pasajero, entre "
                        f"({xs.min()}, {ys.min()}) y ({xs.max()}, {ys.max()})")
                if n < 20:
                    for nombre, im in (("a", a), ("b", b), ("c", c2)):
                        if im.ndim == 2:
                            im = im.view(np.uint8).reshape(im.shape[0], im.shape[1], 4)[:, :, :3]
                        Image.fromarray(im).save(os.path.join(carpeta, f"parpadeo-{n:02d}-{nombre}.png"))
                    Image.fromarray((pasajero * 255).astype(np.uint8)).save(os.path.join(carpeta, f"parpadeo-{n:02d}-mascara.png"))


def uso():
    """Lo que hace una persona: el Shell desde el lanzador (abrir y cerrar), el panel rápido, el desplegable de
    procesos y el ratón de un lado a otro."""
    bb = esperar(r"\[barra\] botones: lanzador en \((\d+), (\d+)\)", 30)
    p = esperar(r"\[barra\] CPU y memoria: [^\n]*; en \((\d+), (\d+)\)", 10)
    vuelta = 0
    while time.time() - inicio < fin:
        vuelta += 1
        if bb:
            desde = len(texto())
            clic(int(bb[-1][0]), int(bb[-1][1]))
            time.sleep(0.6)
            for c in "shell":
                tecla(c)
            time.sleep(0.3)
            tecla("ret")
            esperar(r"\[compositor\] ventana de [Ss]hell en", 15, desde)
            v = ventana("[Ss]hell", desde)
            time.sleep(1)
            if v:
                sx, sy, sw, _ = v
                # Arrastrar la ventana por su barra un poco y cerrarla con su botón.
                ir(sx + 150, max(sy - 20, 1))
                q.orden("input-send-event", events=[{"type": "btn", "data": {"down": True, "button": "left"}}])
                for k in range(20):
                    ir(sx + 150 + 10 * k, max(sy - 20, 1) + 5 * k)
                q.orden("input-send-event", events=[{"type": "btn", "data": {"down": False, "button": "left"}}])
                time.sleep(0.5)
                v2 = ventana("[Ss]hell", desde) or v
                clic(v2[0] + sw - 20, max(v2[1] - 20, 1))
                time.sleep(1)
        tecla("meta_l", "a")
        time.sleep(1.2)
        tecla("meta_l", "a")
        time.sleep(1)
        if p:
            clic(int(p[-1][0]), int(p[-1][1]))
            time.sleep(1)
            tecla("esc")
            time.sleep(0.5)
        for k in range(40):
            ir(100 + 25 * k, 200 + 8 * k)
        apuntar(f"vuelta {vuelta}")


def principal():
    global q, fin
    q = Qmp(qmp_ruta)
    if not esperar(r"\[guardian\] pantalla de entrada pintada", 300):
        return
    teclear("Ana")
    time.sleep(0.5)
    teclear("clave de Ana")
    if not esperar(r"\[guardian\] entra Ana", 60):
        return
    esperar(r"\[barra\] botones: lanzador en", 60)
    time.sleep(3)
    apuntar("entra Ana; empiezan las capturas")
    fin = time.time() - inicio + segundos
    if VIDEO:
        # Lo que pinta la ventana de QEMU de verdad (GTK en un X sin pantalla, Xvfb): un vídeo sin pérdidas de la
        # pantalla de X, que se mira después cuadro a cuadro.
        grabador = subprocess.Popen(["ffmpeg", "-loglevel", "error", "-y", "-f", "x11grab", "-framerate", "60",
                                     "-draw_mouse", "0", "-video_size", VIDEO_TAM, "-i", os.environ["DISPLAY"], "-c:v", "ffv1", VIDEO],
                                    stdin=subprocess.PIPE)
        uso()
        grabador.communicate(b"q")
        h = Capturas()
        h.video()
    else:
        h = Capturas()
        h.start()
        uso()
        h.parar = True
        h.join()
    dur = segundos
    apuntar(f"{h.cuadros} cuadros en {dur:.0f} s ({h.cuadros / dur:.1f} por segundo); {len(h.malos)} con parpadeo "
            f"(más del {UMBRAL * 100:.0f} % pasajero); lo más pasajero de un cuadro: {h.peor * 100:.2f} %")


fin = 0
try:
    principal()
except Exception as e:
    apuntar(f"falla el guion: {e!r}")
finally:
    try:
        os.remove(TEMPORAL)
    except OSError:
        pass
