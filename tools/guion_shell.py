#!/usr/bin/env python3
"""Las teclas de `make arranque-virt-shell` (el Shell nativo en pantalla, docs/bitacoras/shell-nativa.md), por el
monitor de QEMU.

Uso (lo lanza `tools/prueba_uefi.sh` con SHELL_CAPTURAS=prefijo): guion_shell.py MONITOR SEGUNDOS SERIE PREFIJO

La imagen es la de usar (`virt-uso`): la pantalla de entrada de verdad y nada de los programas de prueba. Como lo haría
una persona, con el teclado y el ratón de QEMU (así pasa por el mapa de teclado y los modificadores, que la prueba del
Shell por guion, con `Teclea`, se salta):

1. Entra Ana con su contraseña.
2. Abre el lanzador de la barra, busca "shell" e Intro: sale el Shell. Un clic en su título, para que tenga el teclado.
3. Captura el Shell recién abierto, con su indicador (PREFIJO.1-recien-abierto.ppm).
4. `List SYS:` (colores por tipo): PREFIJO.2-list.ppm.
5. Una orden al historial, y a medio teclear: `Ech` con la sugerencia en gris (PREFIJO.3-sugerencia.ppm); y una
   orden que no existe, con una ruta y una cadena (PREFIJO.4-orden-mala.ppm).
6. `Dir SYS:S` y Tab dos veces: el menú de completar con el primero elegido (PREFIJO.5-menu-tab.ppm).
7. Ctrl+Mayús+E (partir en vertical) y otra orden en el panel nuevo (PREFIJO.6-paneles.ppm).
8. Ctrl+Mayús+T (pestaña nueva) y otra orden en ella (PREFIJO.7-pestana.ppm).
9. `Assign CapturasHechas: RAM:`: `sesiones` lo dice en el registro, y es la última línea que espera la prueba.

Cada captura queda también en PNG al lado. Lo que hace el guion, con la hora, en PREFIJO.guion.txt.
Teclado español: lo que hay que pulsar para cada letra, en `ESPECIALES` (nombres de `sendkey`, posición de EE. UU.).
"""
import os
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


def esperar(patron, segundos):
    hasta = time.time() + segundos
    while time.time() < hasta and time.time() - inicio < total:
        m = re.findall(patron, texto())
        if m:
            return m
        time.sleep(0.25)
    apuntar(f"no sale: {patron}")
    return None


# Teclado español: lo que hay que pulsar (nombres de `sendkey` de QEMU, posición de EE. UU.) para cada letra.
ESPECIALES = {" ": "spc", "-": "slash", ".": "dot", ",": "comma", "/": "shift-7", "=": "shift-0", ":": "shift-dot",
              "_": "shift-slash", "&": "shift-6", ";": "shift-comma", "<": "less", ">": "shift-less", '"': "shift-2",
              "#": "alt_r-3", "?": "shift-minus", "|": "alt_r-1"}


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


def clic(x, y):
    ir_a(x, y)
    mandar("mouse_button 1", "mouse_button 0", pausa=0.1)


def capturar(nombre, espera=2.0):
    # Lo que se ha tecleado tiene que estar ya pintado: el Shell repinta al momento, el compositor en el siguiente cuadro.
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


def principal():
    time.sleep(3)
    # 1. La pantalla de entrada: Ana y su contraseña.
    if not esperar(r"\[guardian\] pantalla de entrada pintada", 200):
        return
    time.sleep(1)
    teclear("Ana")
    time.sleep(0.5)
    teclear("clave de Ana")
    if not esperar(r"\[guardian\] entra Ana", 60):
        return
    apuntar("entra Ana")
    # 2. El lanzador de la barra: "shell" e Intro. Hasta tres veces (con el PC cargado, la barra tarda en salir).
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
    if not esperar(r"\[shell\] ventana en pantalla", 60):
        return
    v = esperar(r"\[compositor\] ventana de [Ss]hell en \((\d+), (\d+)\)", 10)
    time.sleep(2)
    if v:
        # Un clic en su título (la barra de DashOS va encima de la ventana): que tenga el teclado.
        x, y = int(v[-1][0]), int(v[-1][1])
        apuntar(f"la ventana del Shell en ({x}, {y})")
        clic(x + 120, max(y - 20, 1))
    ir_a(790, 590)
    # 3. Recién abierto.
    capturar("1-recien-abierto")
    # 4. List con colores.
    teclear("List SYS:")
    capturar("2-list", 3)
    # 5. La sugerencia del historial y el resaltado.
    teclear("Cls")
    time.sleep(1)
    teclear('Echo "hola desde el teclado"')
    time.sleep(1.5)
    teclear("Ech", intro=False)
    capturar("3-sugerencia")
    mandar("sendkey ctrl-x")
    teclear('NoExiste SYS:S "sin cerrar', intro=False)
    capturar("4-orden-mala")
    # 6. El menú de Tab.
    mandar("sendkey ctrl-x")
    teclear("Dir SYS:S", intro=False)
    mandar("sendkey tab", "sendkey tab", pausa=0.8)
    capturar("5-menu-tab")
    mandar("sendkey esc", "sendkey ctrl-x", pausa=0.3)
    # 7. Partir en vertical y otra orden en el panel nuevo (que se lleva el teclado).
    mandar("sendkey ctrl-shift-e")
    time.sleep(2)
    teclear("Dir SYS:C")
    capturar("6-paneles", 3)
    # 8. Una pestaña nueva, y otra orden.
    mandar("sendkey ctrl-shift-t")
    time.sleep(2)
    teclear("Version")
    capturar("7-pestana", 3)
    # 9. La señal del final: `sesiones` dice la asignación.
    teclear("Assign CapturasHechas: RAM:")
    apuntar("fin")


try:
    principal()
except Exception as e:
    apuntar(f"falla el guion: {e!r}")
