#!/usr/bin/env python3
"""Las teclas y los clics de `make arranque-virt-menus` (S10, los menús de los programas; docs/bitacoras/menus.md), por
el monitor de QEMU.

Uso (lo lanza `tools/prueba_uefi.sh` con MENUS_CAPTURAS=prefijo): guion_menus.py MONITOR SEGUNDOS SERIE PREFIJO

La imagen es la de usar con la prueba de kit-c (`dash-ventana`, un programa de Dash con un menú "Proyecto"). Como lo
haría una persona, con el teclado y el ratón de QEMU:

1. Entra Ana. La ventana de Dash (kit-c), delante: su fila de menús (01) y "Proyecto" abierto dentro (02); Esc.
2. Abre el Shell (el kit nativo) desde el lanzador: su fila (03) y "Editar" abierto dentro, con Copiar desactivado
   (04); Esc.
3. Teclea en el Shell `Ajuste menus barra`: el centro de control lo aplica y el compositor lo dice a todos; la fila se
   va de las ventanas y los menús del Shell salen en la barra (05). Un clic en "Shell" de la barra lo abre (06) y otro
   en "Partir en vertical" lo elige: el Shell parte su panel (07).
4. El botón derecho sobre la ventana del Shell abre sus menús en la barra, como en el Amiga (08); Esc.
5. Un clic en la ventana de Dash: la barra enseña los suyos (09); abre "Proyecto" (10) y elige "Acerca de...": el
   programa de Dash pregunta (11), y un clic en "Vale".
6. En el Shell, `Ajuste menus dentro`: la fila vuelve a las ventanas (12); F10 abre el primero dentro (13); Esc. Y
   `Assign MenusHechos: RAM:`, que dice `sesiones`: el guion llegó al final.

Cada captura queda también en PNG al lado. Lo que hace el guion, con la hora, en PREFIJO.guion.txt.
"""
import re
import socket
import sys
import time

ruta, total, serie, prefijo = sys.argv[1], float(sys.argv[2]), sys.argv[3], sys.argv[4]
inicio = time.time()


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
    """Las coincidencias de `patron` en lo que salió desde el carácter `desde`."""
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


# Teclado español: lo que hay que pulsar (nombres de `sendkey` de QEMU, posición de EE. UU.) para cada letra.
ESPECIALES = {" ": "spc", "-": "slash", ".": "dot", ",": "comma", "/": "shift-7", "=": "shift-0", ":": "shift-dot",
              "_": "shift-slash", '"': "shift-2"}


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
    mandar(*["mouse_move -100 -100"] * 20, pausa=0.03)
    pasos = []
    while x > 0 or y > 0:
        dx, dy = min(x, 100), min(y, 100)
        pasos.append(f"mouse_move {dx} {dy}")
        x, y = x - dx, y - dy
    mandar(*pasos, pausa=0.05)
    time.sleep(0.3)


def clic(x, y, boton=1):
    ir_a(x, y)
    mandar(f"mouse_button {boton}", "mouse_button 0", pausa=0.15)


def capturar(nombre, espera=2.0):
    time.sleep(espera)
    ppm = f"{prefijo}.{nombre}.ppm"
    mandar("screendump " + ppm)
    time.sleep(1)
    try:
        from PIL import Image
        Image.open(ppm).save(ppm[:-4] + ".png")
    except Exception as e:  # sin PIL, se queda el PPM
        apuntar(f"sin PNG de {nombre}: {e}")
    apuntar(f"captura {nombre}")


def sitios(linea):
    """Los "título" en (x, y) de una línea."""
    return {t: (int(x), int(y)) for t, x, y in re.findall(r'"([^"]+)" en \((\d+), (\d+)\)', linea)}


def principal():
    time.sleep(3)
    # 1. La pantalla de entrada: Ana.
    if not esperar(r"\[guardian\] pantalla de entrada pintada", 200):
        return
    time.sleep(1)
    teclear("Ana")
    time.sleep(0.5)
    teclear("clave de Ana")
    if not esperar(r"\[guardian\] entra Ana", 60):
        return
    apuntar("entra Ana")
    # La ventana de Dash: dónde está (el compositor) y dónde su menú (ella).
    d = esperar(r'(\[dash-ui\] ventana "Dash (?:en|on) DashOS" en pantalla \((\d+)x\d+\)[^\n]*)', 90)
    v = esperar(r"\[compositor\] ventana de dash-ventana en \((\d+), (\d+)\)", 30)
    if not d or not v:
        return
    dx, dy = int(v[-1][0]), int(v[-1][1])
    ancho_dash = int(d[-1][1])
    menu_dash = sitios(d[-1][0])
    apuntar(f"la ventana de Dash en ({dx}, {dy}), su menú: {menu_dash}")
    time.sleep(2)
    # Delante: un clic en su margen de arriba a la derecha (bajo la fila de menús no hay nada a la derecha).
    clic(dx + ancho_dash - 12, dy + 60)
    ir_a(790, 590)
    capturar("01-kitc-dentro")
    if "Proyecto" in menu_dash:
        x, y = menu_dash["Proyecto"]
        clic(dx + x, dy + y)
        ir_a(dx + x + 40, dy + y + 60)
        capturar("02-kitc-dentro-abierto")
        mandar("sendkey esc")
        time.sleep(0.5)
    # 2. El Shell, desde el lanzador de la barra.
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
        apuntar("el lanzador no se abre: otra vez")
    time.sleep(0.5)
    mandar(*["sendkey " + c for c in "shell"], pausa=0.25)
    if not esperar(r'\[barra\] busca "shell": ', 15):
        return
    time.sleep(0.5)
    mandar("sendkey ret")
    m = esperar(r"(\[shell\] menús de la ventana: [^\n]*)", 60)
    v = esperar(r"\[compositor\] ventana de [Ss]hell en \((\d+), (\d+)\)", 10)
    if not m or not v:
        return
    sx, sy = int(v[-1][0]), int(v[-1][1])
    menu_shell = sitios(m[-1])
    apuntar(f"el Shell en ({sx}, {sy}), su menú: {menu_shell}")
    time.sleep(2)
    # Un clic en su título (la barra de DashOS va encima de la ventana): que tenga el teclado.
    clic(sx + 120, max(sy - 20, 1))
    ir_a(790, 590)
    capturar("03-shell-dentro")
    if "Editar" in menu_shell:
        x, y = menu_shell["Editar"]
        clic(sx + x, sy + y)
        ir_a(sx + x + 40, sy + y + 70)
        capturar("04-shell-dentro-abierto")
        mandar("sendkey esc")
        time.sleep(0.5)
    # 3. Los menús, a la barra (con la orden, tecleada en el Shell).
    desde = largo()
    teclear("Ajuste menus barra")
    b = esperar(r"(\[barra\] menús en la barra: [^\n]*)", 30, desde)
    esperar(r"\[shell\] menús: en la barra", 10, desde)
    if not b:
        return
    ir_a(790, 590)
    capturar("05-barra-shell")
    barra = sitios(b[-1])
    if "Shell" not in barra:
        apuntar(f"la barra no enseña los menús del Shell: {barra}")
        return
    desde = largo()
    clic(*barra["Shell"])
    a = esperar(r"(\[barra\] menú abierto: Shell[^\n]*)", 15, desde)
    if not a:
        return
    entradas = sitios(a[-1])
    ir_a(barra["Shell"][0] + 60, barra["Shell"][1] + 90)
    capturar("06-barra-shell-abierto")
    if "Partir en vertical" in entradas:
        desde = largo()
        clic(*entradas["Partir en vertical"])
        esperar(r"\[shell\] menú: Partir en vertical", 15, desde)
    ir_a(790, 590)
    capturar("07-barra-elegido")
    # 4. El botón derecho sobre la ventana del Shell.
    desde = largo()
    clic(sx + 200, sy + 200, boton=2)
    esperar(r"\[barra\] menú abierto: Shell", 15, desde)
    capturar("08-barra-boton-derecho")
    mandar("sendkey esc")
    time.sleep(1)
    # 5. La ventana de Dash, otra vez delante: sus menús en la barra.
    desde = largo()
    # Por su borde izquierdo, que el Shell no tapa.
    clic(dx + 12, dy + 300)
    b = esperar(r"(\[barra\] menús en la barra: \"Proyecto\"[^\n]*)", 20, desde)
    ir_a(790, 590)
    capturar("09-barra-kitc")
    if b:
        barra = sitios(b[-1])
        desde = largo()
        clic(*barra["Proyecto"])
        a = esperar(r"(\[barra\] menú abierto: Proyecto[^\n]*)", 15, desde)
        if a:
            entradas = sitios(a[-1])
            ir_a(barra["Proyecto"][0] + 60, barra["Proyecto"][1] + 70)
            capturar("10-barra-kitc-abierto")
            acerca = next((t for t in entradas if t.startswith("Acerca")), None)
            if acerca:
                desde = largo()
                clic(*entradas[acerca])
                p = esperar(r'(\[dash-ui\] ventana "Acerca de" en pantalla[^\n]*)', 20, desde)
                vp = esperar(r"\[compositor\] ventana de dash-ventana en \((\d+), (\d+)\)", 10, desde)
                ir_a(790, 590)
                capturar("11-kitc-elegido")
                botones = sitios(p[-1]) if p else {}
                if vp and "Vale" in botones:
                    clic(int(vp[-1][0]) + botones["Vale"][0], int(vp[-1][1]) + botones["Vale"][1])
                    esperar(r"\[dash-ventana\] acerca de: 1", 15, desde)
    # 6. Dentro otra vez; F10 en el Shell. (Un clic en la parte de su título que no tapa la ventana de Dash.)
    clic(sx + 450, max(sy - 20, 1))
    time.sleep(0.5)
    desde = largo()
    teclear("Ajuste menus dentro")
    esperar(r"\[shell\] menús: dentro de la ventana", 30, desde)
    ir_a(790, 590)
    capturar("12-dentro-otra-vez")
    mandar("sendkey f10")
    capturar("13-f10")
    mandar("sendkey esc")
    time.sleep(0.5)
    teclear("Assign MenusHechos: RAM:")
    apuntar("acabado")


try:
    principal()
except Exception as e:
    apuntar(f"falla el guion: {e!r}")
