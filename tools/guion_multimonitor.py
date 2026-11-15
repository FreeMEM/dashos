#!/usr/bin/env python3
"""Las teclas y el ratón de `make arranque-virt-2` (multimonitor), por el monitor de QEMU.

Uso (lo lanza `tools/prueba_uefi.sh` con MULTI=prefijo): guion_multimonitor.py MONITOR SEGUNDOS SERIE PREFIJO

Va solo, sin el guion de siempre (TECLAS): así no depende de lo que tarden las preguntas, el selector o los bloqueos de
las demás pruebas. Con la virtio-gpu de dos cabezas (id "vgpu") ya puesta y Ajustes en pantalla:

1. Captura cada cabeza (PREFIJO.cabeza0.ppm y .cabeza1.ppm).
2. Lleva el cursor de la primera a la segunda (el compositor dice "el cursor pasa a DASHOS-2").
3. Da a Ajustes el teclado con un clic a la derecha de su título y la lleva a la segunda con Super+Mayúsculas+Derecha.
4. La maximiza allí (Super+Arriba: 1024x768, sin barra) y captura (PREFIJO.max0.ppm y .max1.ppm).
5. La devuelve (Super+Abajo) y la trae a la primera (Super+Mayúsculas+Izquierda); captura (PREFIJO.vuelta*.ppm).
6. Cuando `guardian` abre el centro de control (Permisos), baja a Pantallas (Mayúsculas+Tab y tres flechas abajo) y
   captura las dos cabezas (PREFIJO.pantallas0.ppm y .pantallas1.ppm): el centro sale en la del cursor.

Cada paso espera su línea en la salida serie y lo reintenta (hasta tres veces) si no llega. Mientras, contesta las
preguntas de permisos de `guardian` (si no, cada una espera 30 s y el centro sale tarde): un clic en la pregunta (por
si el clic en Ajustes le quitó el teclado) y, pasado el primer segundo, "s" a `pide` y "n" a los demás.
"""
import re
import socket
import sys
import time

ruta, total, serie, prefijo = sys.argv[1], float(sys.argv[2]), sys.argv[3], sys.argv[4]
inicio = time.time()


def mandar(*ordenes, pausa=0.05):
    s = socket.socket(socket.AF_UNIX)
    s.connect(ruta)
    for o in ordenes:
        s.sendall(o.encode() + b"\n")
        time.sleep(pausa)
    s.close()


def ir_a(x, y):
    # A la esquina (0, 0) y de ahí en pasos de 100 como mucho.
    mandar(*["mouse_move -100 -100"] * 20, pausa=0.03)
    pasos = []
    while x > 0 or y > 0:
        dx, dy = min(x, 100), min(y, 100)
        pasos.append(f"mouse_move {dx} {dy}")
        x, y = x - dx, y - dy
    mandar(*pasos, pausa=0.05)
    time.sleep(0.3)


def clic():
    mandar("mouse_button 1", "mouse_button 0", pausa=0.1)
    time.sleep(0.3)


def texto():
    try:
        return open(serie, errors="replace").read()
    except OSError:
        return ""


contestadas = 0


def contestar(t):
    """Las preguntas de `guardian` que aún no se contestaron."""
    global contestadas
    # S28: también las de la caja Amiga ("la caja Amiga: X quiere cambiar ..."), que no empiezan por "¿": "n".
    vistas = [m or "?" for m in re.findall(r"\[guardian\] pregunta en pantalla: (?:¿(\S+) puede)?", t)]
    while contestadas < len(vistas):
        quien = vistas[contestadas]
        contestadas += 1
        time.sleep(1.3)
        capa = re.findall(r"\[compositor\] capa de guardian en \((\d+), (\d+)\)", t)
        if capa:
            ir_a(int(capa[-1][0]) + 40, int(capa[-1][1]) + 20)
            clic()
        mandar("sendkey " + ("s" if quien == "pide" else "n"))
        time.sleep(0.5)


def esperar(cond, plazo):
    fin = time.time() + plazo
    while time.time() < fin and time.time() - inicio < total:
        t = texto()
        contestar(t)
        if cond(t):
            return t
        time.sleep(0.25)
    return None


def capturar(nombre):
    time.sleep(0.8)
    mandar(f"screendump {prefijo}.{nombre}0.ppm vgpu 0")
    time.sleep(0.6)
    mandar(f"screendump {prefijo}.{nombre}1.ppm vgpu 1")
    time.sleep(0.6)


def a_ajustes(t):
    v = re.findall(r"\[compositor\] ventana de ajustes (?:a DASHOS-\d )?en \((\d+), (\d+)\)", t)
    if v:
        # A la derecha de su título: ahí no hay nada que pulsar.
        ir_a(int(v[-1][0]) + 296, int(v[-1][1]) + 26)
        clic()


def paso(orden, linea, antes=None, veces=3, plazo=5):
    """Manda `orden` (y antes hace `antes`) hasta que sale `linea` (una vez más de las que había)."""
    ya = texto().count(linea)
    for _ in range(veces):
        if antes:
            antes()
        mandar(orden)
        if esperar(lambda t: t.count(linea) > ya, plazo):
            return True
    return False


def principal():
    t = esperar(lambda t: "salidas del controlador gráfico" in t and "[ajustes] ventana del kit en pantalla" in t, total)
    if t is None:
        return
    time.sleep(2)
    capturar("cabeza")
    # El cursor, de la primera a la segunda: pasa por x = 800.
    ir_a(700, 300)
    mandar(*["mouse_move 100 0"] * 3, pausa=0.1)
    esperar(lambda t: "el cursor pasa a DASHOS-2" in t, 5)
    if not paso("sendkey meta_l-shift-right", "[compositor] ventana de ajustes a DASHOS-2", antes=lambda: a_ajustes(texto())):
        return
    time.sleep(0.5)
    # Antes de cada tecla, el clic en Ajustes: una pregunta o el selector pueden haberle quitado el teclado.
    if paso("sendkey meta_l-up", "[ajustes] el compositor pide 1024x768", antes=lambda: a_ajustes(texto())):
        capturar("max")
    paso("sendkey meta_l-down", "[ajustes] vuelve a 360x408", antes=lambda: a_ajustes(texto()))
    time.sleep(0.5)
    paso("sendkey meta_l-shift-left", "[compositor] ventana de ajustes a DASHOS-1", antes=lambda: a_ajustes(texto()))
    capturar("vuelta")
    # El centro de control: lo abre `guardian` en Permisos. Si no se lleva el teclado (la barra dice su título), un clic
    # a la derecha de su título.
    t = esperar(lambda t: "[centro] Permisos en pantalla" in t, total)
    if t is None:
        return
    if esperar(lambda t: "[barra] título: Centro de control" in t, 3) is None:
        v = re.findall(r"\[compositor\] ventana de centro en \((\d+), (\d+)\)", texto())
        if v:
            ir_a(int(v[-1][0]) + 420, int(v[-1][1]) + 20)
            clic()
    time.sleep(0.5)
    mandar("sendkey shift-tab")
    time.sleep(0.4)
    for linea in ("[centro] Seguridad, ", "[centro] Discos: ", "[centro] Pantallas: "):
        if not paso("sendkey down", linea, veces=2, plazo=4):
            break
    if "[centro] Pantallas: " in texto():
        # El centro sale en la salida del cursor: se capturan las dos.
        capturar("pantallas")


try:
    principal()
except OSError:
    pass
