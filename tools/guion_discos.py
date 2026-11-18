#!/usr/bin/env python3
"""Las capturas de los discos del escritorio (rama dock-discos; docs/bitacoras/dock-discos.md), por el monitor de QEMU,
con la imagen de usar (`virt-uso`), como lo haría una persona.

Uso (lo lanza `tools/prueba_uefi.sh` con DISCOS_CAPTURAS=prefijo; `make capturas-discos`):
    guion_discos.py MONITOR SEGUNDOS SERIE PREFIJO

1. Entra Ana; con DMS_SHIVA=1, el tema oscuro y la barra de shiva (desde un Shell, como `guion_dms.py`). Mientras
   entran `Home:` y `RAM:` (la sesión), una tira de cuadros (PREFIJO.anim-entrar-NN).
2. Los discos quietos (PREFIJO.1-discos), el ratón encima de SYS: (PREFIJO.2-encima), un clic en Home: con su onda
   (PREFIJO.3-onda) y elegido (PREFIJO.4-elegido).
3. El botón derecho en Work: (PREFIJO.5-menu) e Información (PREFIJO.6-informacion, la ventana del Workbench).
4. Doble clic en SYS: (el explorador, PREFIJO.7-explorador).
5. Los ajustes en marcha, con la orden `Ajuste` en el Shell: a la izquierda y abajo, grande (PREFIJO.8-izquierda), y
   sin la barra del espacio, pequeño, en el centro (PREFIJO.9-compacto); luego, lo de serie. Al final, esconderlos y
   sacarlos otra vez: la tira de cuadros de su entrada (PREFIJO.anim-aparecer-NN).
6. El dock: el menú del Shell (PREFIJO.12-dock-menu) y Fijar; se cierra el Shell y se abre desde el fijado (la tira
   del rebote, PREFIJO.anim-rebote-NN); con otro Shell y la lupa encendida (`Ajuste Escritorio/dock.aumento si`), el
   ratón de lado a lado (PREFIJO.anim-lupa-NN), sobre un icono (PREFIJO.10-dock-lupa) y entre dos (PREFIJO.11-...).

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
ALTO = int(os.environ.get("DMS_ALTO", "1080"))


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
    especiales = {" ": "spc", "-": "slash", ".": "dot", "/": "shift-7", ":": "shift-dot", "ñ": "semicolon"}
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


def clic(x, y, boton=1):
    ir_a(x, y)
    mandar(f"mouse_button {boton}", "mouse_button 0", pausa=0.1)


def ppm_a_png(ppm):
    try:
        from PIL import Image
        Image.open(ppm).save(ppm[:-4] + ".png")
        os.remove(ppm)
    except Exception as e:
        apuntar(f"sin PNG de {ppm}: {e}")


def capturar(nombre, espera=1.5):
    time.sleep(espera)
    ppm = f"{prefijo}.{nombre}.ppm"
    mandar("screendump " + ppm)
    time.sleep(1)
    ppm_a_png(ppm)
    apuntar(f"captura {nombre}")


def tira(nombre, segundos, cada=0.06):
    """Cuadros seguidos (sin pasar a PNG hasta el final: así salen más juntos)."""
    hechos = []
    hasta = time.time() + segundos
    k = 0
    while time.time() < hasta:
        ppm = f"{prefijo}.{nombre}-{k:02d}.ppm"
        mandar("screendump " + ppm, pausa=0)
        hechos.append((ppm, time.time() - inicio))
        k += 1
        time.sleep(cada)
    time.sleep(1)
    for ppm, _ in hechos:
        ppm_a_png(ppm)
    apuntar(f"tira {nombre}: {k} cuadros en {segundos} s")


def capa_discos(t):
    """Dónde está la capa de los discos: la última "capa de workbench" que no es la de un menú (la que sale justo tras
    "[workbench] menú de")."""
    sitios = []
    for m in re.finditer(r"\[compositor\] capa de workbench en \((-?\d+), (-?\d+)\)", t):
        antes = t.rfind("[workbench]", 0, m.start())
        if antes >= 0 and t.startswith("[workbench] menú de", antes):
            continue
        sitios.append((m.group(1), m.group(2)))
    return sitios


def mover_desde(x0, y0, x1, y1):
    """Del sitio (x0, y0), donde está el ratón, a (x1, y1), sin pasar por la esquina."""
    dx, dy = x1 - x0, y1 - y0
    pasos = []
    while dx or dy:
        px, py = max(-100, min(100, dx)), max(-100, min(100, dy))
        pasos.append(f"mouse_move {px} {py}")
        dx, dy = dx - px, dy - py
    mandar(*pasos, pausa=0.05)
    time.sleep(0.3)


def sitio(nombre):
    """El centro de la tarjeta de `nombre` en la pantalla: la capa de los discos más lo que dice el Workbench."""
    t = texto()
    c = capa_discos(t)
    i = re.findall(r"\[workbench\] (?:en pantalla: \d+x\d+, )?icono de " + nombre + r" en \((\d+), (\d+)\)", t)
    if not c or not i:
        return None
    return int(c[-1][0]) + int(i[-1][0]), int(c[-1][1]) + int(i[-1][1])


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


def shell():
    """Un Shell, pulsado para que tenga el teclado. Dónde quedó."""
    marca = len(texto())
    if not lanzar("shell") or not esperar(r"\[shell\] ventana en pantalla", 60, marca):
        return None
    v = esperar(r"\[compositor\] ventana de [Ss]hell en \((\d+), (\d+)\)", 10, marca)
    time.sleep(2)
    if v:
        clic(int(v[-1][0]) + 120, max(int(v[-1][1]) - 20, 1))
        return int(v[-1][0]), int(v[-1][1])
    return None


def sitios_dock(linea):
    return [(n.strip(), int(x), int(y)) for n, x, y in re.findall(r"([^,]+?) en \((\d+), (\d+)\)", linea)]


def tira_moviendo(nombre, dx, veces):
    """Mueve el ratón `dx` cada vez y captura tras cada paso (en la misma tanda de órdenes del monitor)."""
    ordenes = []
    for k in range(veces):
        ordenes += [f"mouse_move {dx} 0", f"screendump {prefijo}.{nombre}-{k:02d}.ppm"]
    mandar(*ordenes, pausa=0.06)
    time.sleep(1)
    for k in range(veces):
        ppm_a_png(f"{prefijo}.{nombre}-{k:02d}.ppm")
    apuntar(f"tira {nombre}: {veces} cuadros")


def revelar(marca):
    """Saca el dock (el ratón a la raya de abajo) y dice dónde quedó cada cosa."""
    ir_a(ANCHO // 2, ALTO - 1)
    sv = esperar(r"\[dock\] se ve: ([^\n]*)", 10, marca)
    return sitios_dock(sv[-1]) if sv else []


def esconder(marca):
    ir_a(ANCHO // 2, ALTO // 2)
    esperar(r"\[dock\] se esconde", 10, marca)


def dock(v):
    """El dock: fija el Shell desde su menú; cierra el Shell y lo abre desde el fijado (el rebote hasta que sale su
    ventana); con otro Shell, la lupa encendida y el ratón de lado a lado."""
    teclear("Ajuste Escritorio/dock.aumento si")
    time.sleep(2)
    marca = len(texto())
    s = revelar(marca)
    if len(s) < 2:
        return
    # El menú del Shell abierto y Fijar.
    mover_desde(ANCHO // 2, ALTO - 1, s[1][1], s[1][2])
    mandar("mouse_button 2", "mouse_button 0", pausa=0.1)
    f = esperar(r"\[dock\] menú de .*; fijar en \((\d+), (\d+)\)", 10, marca)
    if not f:
        return
    capturar("12-dock-menu", 0.3)
    mover_desde(s[1][1], s[1][2], int(f[-1][0]), int(f[-1][1]))
    mandar("mouse_button 1", "mouse_button 0", pausa=0.1)
    if not esperar(r"\[dock\] fija ", 10, marca):
        return
    # Se cierra el Shell (queda el fijado, sin ventanas) y se abre desde el dock: rebota hasta que sale su ventana.
    esconder(marca)
    clic(v[0] + 120, max(v[1] - 20, 1))
    marca = len(texto())
    teclear("EndShell")
    if not esperar(r"\[dock\] ventanas: 0", 15, marca):
        return
    time.sleep(1)
    marca = len(texto())
    s = revelar(marca)
    if len(s) < 2:
        return
    mover_desde(ANCHO // 2, ALTO - 1, s[1][1], s[1][2])
    marca = len(texto())
    mandar("mouse_button 1", "mouse_button 0", pausa=0.1)
    if esperar(r"\[dock\] .* rebota hasta que abra su ventana", 5, marca):
        tira("anim-rebote", 2.5, cada=0.05)
    esperar(r"\[shell\] ventana en pantalla", 30, marca)
    # Otro Shell: tres iconos (el lanzador, el fijado con su ventana y el otro). La lupa, sin salir del dock (fuera se
    # esconde, y escondido solo lo saca la raya de abajo).
    shell()
    esconder(len(texto()))
    marca = len(texto())
    s = revelar(marca)
    if len(s) < 2:
        return
    x0, y0, x1 = s[0][1], s[0][2], s[-1][1]
    mover_desde(ANCHO // 2, ALTO - 1, x0 - 10, y0)
    pasos = (x1 - x0 + 20) // 6
    tira_moviendo("anim-lupa", 6, pasos)
    xa = x0 - 10 + 6 * pasos
    mover_desde(xa, y0, s[-1][1], s[-1][2])
    capturar("10-dock-lupa", 0.4)
    mover_desde(s[-1][1], s[-1][2], (s[-2][1] + s[-1][1]) // 2, s[-1][2])
    capturar("11-dock-lupa-entre", 0.4)


def principal():
    time.sleep(3)
    if not esperar(r"\[guardian\] pantalla de entrada pintada", 240):
        return
    ir_a(ANCHO // 2, ALTO - 10)
    teclear("Ana")
    time.sleep(0.5)
    teclear("clave de Ana")
    # Entran Home: y RAM: (con la sesión): la tira de la animación.
    if not esperar(r"\[guardian\] entra Ana", 60):
        return
    apuntar("entra Ana")
    if esperar(r"\[workbench\] discos: entra", 20):
        tira("anim-entrar", 1.6)
    v = None
    if os.environ.get("DMS_SHIVA"):
        v = shell()
        if v:
            teclear("Ajuste tema oscuro")
            time.sleep(2)
            teclear("Ajuste DMS SYS:Prefs/Presets/DMS/shiva.json")
            esperar(r"\[barra\] barras: 1 configuradas", 15)
            time.sleep(2)
            apuntar("perfil de shiva puesto")
            teclear("EndShell")
            v = None
            time.sleep(2)
    esperar(r"\[workbench\] icono de Home en", 20)
    ir_a(ANCHO // 2, ALTO // 2)
    capturar("1-discos", 2)
    s = sitio("SYS")
    if s:
        ir_a(*s)
        capturar("2-encima", 0.6)
    h = sitio("Home")
    if h:
        ir_a(*h)
        time.sleep(0.4)
        mandar("mouse_button 1", "mouse_button 0", "screendump " + f"{prefijo}.3-onda.ppm", pausa=0.08)
        time.sleep(1)
        ppm_a_png(f"{prefijo}.3-onda.ppm")
        apuntar("captura 3-onda")
        ir_a(ANCHO // 2, ALTO // 2)
        capturar("4-elegido", 1)
    w = sitio("Work")
    if w:
        marca = len(texto())
        clic(*w, boton=2)
        if esperar(r"\[workbench\] menú de Work: ", 10, marca):
            capa = esperar(r"\[compositor\] capa de workbench en \((-?\d+), (-?\d+)\)", 5, marca)
            time.sleep(0.3)
            capturar("5-menu", 0.5)
            # La segunda fila (Información): 6 + 40 + 20 en la capa del menú. Sin pasar por la esquina (el menú se cierra
            # con el ratón fuera).
            if capa:
                mover_desde(w[0], w[1], int(capa[-1][0]) + 60, int(capa[-1][1]) + 66)
                mandar("mouse_button 1", "mouse_button 0", pausa=0.1)
                if esperar(r"\[workbench\] ventana \d+ de Información de Work: en pantalla", 15, marca):
                    capturar("6-informacion", 1)
                    mandar("sendkey esc")
    s = sitio("SYS")
    if s:
        marca = len(texto())
        ir_a(*s)
        mandar("mouse_button 1", "mouse_button 0", "mouse_button 1", "mouse_button 0", pausa=0.08)
        if esperar(r"\[explorador\] ventana \d+ en SYS:", 20, marca):
            capturar("7-explorador", 2)
            mandar("sendkey ctrl-w")
            time.sleep(1)
    # Los ajustes en marcha, por la orden `Ajuste` (en el Shell de antes o en uno nuevo).
    if not v:
        v = shell()
    else:
        clic(v[0] + 120, max(v[1] - 20, 1))
    if v:
        marca = len(texto())
        for orden in ("Escritorio/discos.lado izquierda", "Escritorio/discos.altura abajo", "Escritorio/discos.tamano grande"):
            teclear("Ajuste " + orden)
            time.sleep(1.5)
        esperar(r"\[workbench\] discos: ajustes en marcha: a la vista, a la izquierda, abajo, grande", 15, marca)
        ir_a(ANCHO // 2, 30)
        capturar("8-izquierda", 2)
        marca = len(texto())
        for orden in ("Escritorio/discos.lado derecha", "Escritorio/discos.altura centro", "Escritorio/discos.tamano pequeno", "Escritorio/discos.espacio no"):
            teclear("Ajuste " + orden)
            time.sleep(1.5)
        ir_a(ANCHO // 2, 30)
        capturar("9-compacto", 2)
        for orden in ("Escritorio/discos.altura arriba", "Escritorio/discos.tamano mediano", "Escritorio/discos.espacio si"):
            teclear("Ajuste " + orden)
            time.sleep(1.5)
        # Esconderlos y sacarlos otra vez: el panel entra y sus tarjetas tras él (la tira de la animación).
        teclear("Ajuste Escritorio/discos.mostrar no")
        time.sleep(2)
        teclear("Ajuste Escritorio/discos.mostrar si", intro=False)
        mandar("sendkey ret")
        tira("anim-aparecer", 2.0)
        dock(v)
        # La señal de que acabó (la última línea de pruebas/capturas_discos.txt).
        teclear("Ajuste Escritorio/discos.opacidad 81")
    apuntar("hecho")


if __name__ == "__main__":
    try:
        principal()
    except Exception as e:
        apuntar(f"falla: {e!r}")
