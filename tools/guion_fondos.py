#!/usr/bin/env python3
"""Rama `fondos`: la herramienta de fondos, las transiciones y los fondos animados, en pantalla y con capturas, por el
monitor de QEMU, con la imagen de usar (`virt-uso`), como lo haría una persona (`make arranque-virt-fondos`).

Uso (lo lanza `tools/prueba_uefi.sh` con FONDOS_CAPTURAS=prefijo): guion_fondos.py MONITOR SEGUNDOS SERIE PREFIJO

1. Entra Ana. Con FONDOS_SHIVA=1, el tema oscuro y la barra de shiva (como `guion_dms.py`). Abre un Shell.
2. Las transiciones: un guion del Shell (de fondo, con `Run`) cambia de fondo con cada transición (2 s cada una para
   verlas) mientras el Shell se va al espacio 2 (Super+Mayús+2): ráfagas de capturas (PREFIJO.t-<clase>-NN).
3. La paleta animada (SYS:Fondos/mar.png): dos capturas seguidas.
4. La herramienta (la orden `Fondo`): abierta (PREFIJO.h1-herramienta), el ratón quieto en una miniatura (la vista
   previa sale con su transición: PREFIJO.h2-vista), los animados (PREFIJO.h3-animados), la aurora en vista previa
   (PREFIJO.h4-aurora-vista), Aplicar (PREFIJO.h5-aurora).
5. Lo que gasta cada fondo animado en reposo (25 s cada uno, el fondo y el compositor lo dicen cada 10 s), con una
   captura de cada uno; y sin verse (la pantalla bloqueada: el fondo se para).

Lo que hace, con la hora, en PREFIJO.guion.txt. Las capturas, en PNG.
"""
import os
import re
import socket
import sys
import time

ruta, total, serie, prefijo = sys.argv[1], float(sys.argv[2]), sys.argv[3], sys.argv[4]
inicio = time.time()
ANCHO = int(os.environ.get("FONDOS_ANCHO", "1920"))
ESPECIALES = {" ": "spc", "-": "slash", ".": "dot", ",": "comma", "/": "shift-7", "=": "shift-0", ":": "shift-dot",
              "_": "shift-slash", '"': "shift-2", ">": "shift-less"}


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
        time.sleep(0.2)
    apuntar(f"no sale: {patron}")
    return None


def teclear(orden, intro=True):
    teclas = []
    for c in orden:
        if c in ESPECIALES:
            teclas.append("sendkey " + ESPECIALES[c])
        elif c.isupper():
            teclas.append("sendkey shift-" + c.lower())
        else:
            teclas.append("sendkey " + c)
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


def capturar(nombre, espera=1.0):
    time.sleep(espera)
    ppm = f"{prefijo}.{nombre}.ppm"
    mandar("screendump " + ppm)
    time.sleep(0.6)
    try:
        from PIL import Image
        Image.open(ppm).save(ppm[:-4] + ".png")
        os.remove(ppm)
    except Exception as e:
        apuntar(f"sin PNG de {nombre}: {e}")
    apuntar(f"captura {nombre}")


def rafaga(nombre, n, cada=0.2):
    """`n` capturas seguidas (los .ppm se pasan a PNG al final, para no frenar)."""
    hechas = []
    for k in range(n):
        ppm = f"{prefijo}.{nombre}-{k:02d}.ppm"
        mandar("screendump " + ppm)
        hechas.append(ppm)
        time.sleep(cada)
    apuntar(f"ráfaga {nombre}: {n}")
    return hechas


def a_png(ppms):
    time.sleep(1)
    try:
        from PIL import Image
        for p in ppms:
            if os.path.exists(p):
                Image.open(p).save(p[:-4] + ".png")
                os.remove(p)
    except Exception as e:
        apuntar(f"sin PNG: {e}")


def lanzar(nombre):
    """Abre el lanzador de la barra, busca `nombre` y lo lanza con Intro."""
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


SHELL_EN_2 = [False]


def guion_shell(nombre, lineas):
    """Escribe `lineas` en RAM:<nombre> desde el Shell y lo lanza de fondo (`Run Execute`); el Shell se va al espacio 2
    y se vuelve al 1, que queda despejado."""
    if SHELL_EN_2[0]:
        mandar("sendkey meta_l-2")
        time.sleep(1.5)
    for k, l in enumerate(lineas):
        teclear(f'Echo >{">" if k else ""}RAM:fondos-{nombre} "{l}"')
        time.sleep(0.3)
    teclear(f"Run Execute RAM:fondos-{nombre}")
    time.sleep(0.5)
    if SHELL_EN_2[0]:
        mandar("sendkey meta_l-1")
    else:
        mandar("sendkey meta_l-shift-2")
        SHELL_EN_2[0] = True
    time.sleep(0.8)
    apuntar(f"guion {nombre}: {len(lineas)} líneas")


def esperar_sitios(desde):
    """Dónde dice la herramienta que está cada cosa (la última vez desde `desde`): {nombre: (x, y)}."""
    m = esperar(r"\[fondo\] herramienta en pantalla: ([^\n]*)", 30, desde)
    if not m:
        return None
    return {n: (int(x), int(y)) for n, x, y in re.findall(r"(\w+) en \((\d+), (\d+)\)", m[-1])}


def principal():
    time.sleep(3)
    if not esperar(r"\[guardian\] pantalla de entrada pintada", 240):
        return
    ir_a(ANCHO - 10, 10)
    teclear("Ana")
    time.sleep(0.5)
    teclear("clave de Ana")
    if not esperar(r"\[guardian\] entra Ana", 60):
        return
    apuntar("entra Ana")
    marca = len(texto())
    if not lanzar("shell") or not esperar(r"\[shell\] ventana en pantalla", 60, marca):
        return
    v = esperar(r"\[compositor\] ventana de [Ss]hell en \((\d+), (\d+)\)", 10, marca)
    time.sleep(2)
    if v:
        clic(int(v[-1][0]) + 120, max(int(v[-1][1]) - 20, 1))
    if os.environ.get("FONDOS_SHIVA"):
        teclear("Ajuste tema oscuro")
        time.sleep(2)
        teclear("Ajuste DMS SYS:Prefs/Presets/DMS/shiva.json")
        esperar(r"\[barra\] barras: 1 configuradas", 15, marca)
        time.sleep(2)
        apuntar("perfil de shiva puesto")
    capturar("0-escritorio", 1)
    # 2. Las transiciones, de 2 s, una tras otra.
    pasos = [("circulo", "SYS:Fondos/mar.png"), ("barrido", "atardecer.png"), ("fundido", "ondas.png"), ("disolver", "SYS:Fondos/mar.png")]
    lineas = ["Fondo DURACION 2000"]
    for clase, fondo in pasos:
        lineas += [f"Fondo TRANSICION {clase}", "Wait 2", f"Fondo {fondo}", "Wait 6"]
    lineas += ["Fondo DURACION 900", "Fondo TRANSICION circulo"]
    marca = len(texto())
    guion_shell("t", lineas)
    rafagas = []
    for clase, fondo in pasos:
        if esperar(r"\[fondo\] %s en pantalla: .*transición %s" % (re.escape(fondo), clase), 40, marca):
            rafagas += rafaga(f"t-{clase}", 9, 0.12)
            marca = len(texto())
            esperar(r"transición %s acabada" % clase, 10, marca)
    a_png(rafagas)
    # 3. La paleta animada (lo último que quedó es mar.png).
    time.sleep(1)
    capturar("p-paleta-1", 0.2)
    capturar("p-paleta-2", 0.3)
    # 4. La herramienta, con la orden `Fondo`.
    marca = len(texto())
    guion_shell("h", ["Fondo"])
    sitios = esperar_sitios(marca)
    if sitios:
        capturar("h0-herramienta", 2.5)
        # A Home:Fondos (hay más), y el ratón quieto encima de la miniatura 1: la vista previa sale con su transición.
        marca = len(texto())
        clic(*sitios["home"])
        sitios = esperar_sitios(marca) or sitios
        time.sleep(4)
        capturar("h1-herramienta", 0.5)
        if "1" in sitios:
            marca = len(texto())
            ir_a(*sitios["1"])
            if esperar(r"\[fondo\] herramienta: vista previa de", 10, marca):
                a_png(rafaga("h2-vista", 6, 0.15))
                capturar("h2-vista-fin", 2.5)
        # Los animados.
        marca = len(texto())
        clic(*sitios["animados"])
        sitios = esperar_sitios(marca) or sitios
        time.sleep(5)
        capturar("h3-animados", 0.5)
        # La aurora (la primera), quieto encima: la vista previa; luego clic, el color de acento, y Aplicar.
        marca = len(texto())
        ir_a(*sitios["0"])
        esperar(r"\[fondo\] herramienta: vista previa de animado:aurora", 10, marca)
        capturar("h4-aurora-vista", 3)
        clic(*sitios["0"])
        time.sleep(2)
        capturar("h4b-aurora-acento", 0.5)
        marca = len(texto())
        clic(*sitios["aplicar"])
        esperar(r"\[fondo\] herramienta: aplicado", 10, marca)
        ir_a(ANCHO - 10, 5)
        capturar("h5-aurora", 2)
        capturar("h5-aurora-b", 0.4)
    # Por si se quedó abierta (tiene el teclado): Esc.
    mandar("sendkey esc")
    time.sleep(1)
    # 5. En reposo: cada animado 25 s (el fondo y el compositor dicen lo suyo cada 10 s), con su captura.
    animados = ["aurora", "luciernagas", "nieve", "respira"]
    # Primero, quieto (atardecer.png, sin nada encima): lo que gasta el escritorio sin animar, para comparar.
    lineas = ["Fondo atardecer.png", "Wait 25"]
    for a in animados:
        lineas += [f"Fondo animado:{a}", "Wait 25"]
    lineas += ["Fondo SYS:Fondos/mar.png", "Wait 25", "Fondo EFECTO nieve", "Fondo atardecer.png", "Wait 25", "Fondo EFECTO ninguno", "Fondo animado:aurora"]
    marca = len(texto())
    guion_shell("m", lineas)
    ir_a(ANCHO - 10, 5)
    for a in animados + ["ciclos", "nieve"]:
        if esperar(r"\[fondo\] DASHOS-1: %s: " % a, 40, marca):
            capturar(f"m-{a}-{len(texto()) % 1000:03d}", 0.2)
            marca = len(texto())
            esperar(r"\[fondo\] DASHOS-1: %s: " % a, 12, marca)
            marca = len(texto())
    # Sin verse: la pantalla bloqueada (Super+L), 25 s: el fondo no pinta nada (el compositor no le da su aviso).
    time.sleep(3)
    mandar("sendkey meta_l-l")
    apuntar("bloqueada")
    esperar(r"\[guardian\] pantalla de bloqueo pintada", 20)
    time.sleep(25)
    capturar("z-bloqueada", 0.2)
    teclear("clave de Ana")
    esperar(r"\[guardian\] desbloqueado", 20)
    capturar("z-desbloqueada", 2)
    apuntar("fin")


try:
    principal()
except Exception as e:  # noqa: BLE001
    apuntar(f"el guion se cae: {e!r}")
