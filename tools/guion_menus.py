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
6. En el Shell, `Ajuste menus dentro`: la fila vuelve a las ventanas (12); F10 abre el primero dentro (13); Esc.

Lo de la rama `menus-pulido` (docs/bitacoras/menus-pulido.md):

7. El teclado en la fila del Shell: F10, Derecha (a "Editar", con Copiar desactivado: el foco en Pegar), Abajo (14) e
   Intro: "Borrar la pantalla".
8. La ventana de Dash (kit-c), dentro: F10, Derecha (al menú "Largo", que no cabe en la ventana), Fin: la última se
   ve (15); Intro la elige. La rueda: un clic en "Largo", el ratón en su primera entrada y tres pasos: se ve desde la
   cuarta (16), y un clic la elige. Ráfagas de clics y teclas con el menú abriéndose y cerrándose; después, F10,
   Derecha e Intro siguen eligiendo. Y con "Largo" abierto, un clic en el título del Shell: el menú de la ventana de
   Dash se cierra (el teclado se fue).
9. `Ajuste menus barra` otra vez: sale `menus-prueba` (kit nativo, con un submenú de 32 entradas y un menú de 40) con
   sus menús en la barra. En la barra: F10 y las teclas hasta el submenú "Muchas", que no cabe en la pantalla, y abajo
   hasta la 13.ª: se ve (17); Esc cierra solo el submenú; otra vez e Intro la elige. La rueda en "Largo" (18) y en el
   submenú "Muchas" (19), y un clic elige lo que quedó debajo. El clic que cierra: con "Ver" abierto, un clic en el
   título del Shell solo lo cierra (20) y no le llega (la barra sigue con los menús de `menus-prueba`); el siguiente
   sí le llega. Ráfagas de clics y teclas abriendo y cerrando; y el programa cambia su menú veinte veces mientras está
   desplegado (21).
10. `Ajuste menus dentro`: en la ventana de `menus-prueba`, F10 y las teclas hasta la 10.ª de "Muchas", que no cabe
   en la ventana (22), e Intro; y la rueda en "Largo" (23).
11. `Ajuste menus barra` otra vez: `menus-prueba` cierra su ventana con "Largo" desplegado en la barra; la barra lo
   quita y un clic en el título del Shell le llega (24). `Ajuste menus dentro`.

Lo de la rama `centro-vivo` (docs/bitacoras/centro-vivo.md):

12. El menú "Ver" de la ventana de Dash (kit-c), dentro: una casilla, opciones y un submenú (25, 26); con el teclado,
   la casilla se desmarca, se elige la tercera opción y, en el submenú, la tercera entrada (la del submenú de dentro,
   que no vale, no está).
13. Ajustes > Barra (el botón derecho sobre la barra) y, con Ajustes abierta y delante, `Ajuste menus barra` desde el
   Shell (un guion de fondo, con `Run`): el centro lo aplica al momento (27); `Ajuste menus dentro`, igual; Esc la cierra. Lo hostil: una ráfaga de 16 cambios (un guion en
   `RAM:`, con `Run`) mientras Ajustes se abre y se cierra a ráfagas; queda el último (dentro), y Ajustes se abre y se
   cierra después como siempre. `Assign MenusHechos: RAM:`, que dice `sesiones`: el guion llegó al final.

Cada comprobación que no sale para el guion (lo apunta): sin `MenusHechos`, la prueba falla.

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
              "_": "shift-slash", '"': "shift-2", ">": "shift-less"}


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


class Fallo(Exception):
    pass


def debe(patron, segundos, desde, que):
    """Como `esperar`, pero si no sale, para el guion (y la prueba falla: no llega a `MenusHechos`)."""
    m = esperar(patron, segundos, desde)
    if not m:
        raise Fallo(que)
    return m


def teclas(*nombres, pausa=0.25):
    mandar(*["sendkey " + n for n in nombres], pausa=pausa)


def rel(dx, dy):
    """Mueve el ratón (dx, dy) desde donde está, a pasos cortos (para no saltar por encima de nada)."""
    pasos = []
    while dx or dy:
        px = max(-20, min(20, dx))
        py = max(-20, min(20, dy))
        pasos.append(f"mouse_move {px} {py}")
        dx, dy = dx - px, dy - py
    mandar(*pasos, pausa=0.05)
    time.sleep(0.3)


def pulsar(boton=1):
    """Un clic donde está el ratón."""
    mandar(f"mouse_button {boton}", "mouse_button 0", pausa=0.15)


# El signo de `dz` en `mouse_move` del monitor de QEMU: el que baja lo desplegado (lo mira la primera vez).
SIGNO = [None]


def rueda_abajo(n, patron_un_paso, desde):
    """`n` pasos de rueda hacia abajo. La primera vez prueba un signo y mira `patron_un_paso` (lo que se ve tras uno)."""
    if SIGNO[0] is None:
        for signo in (-1, 1):
            mandar(f"mouse_move 0 0 {signo}")
            if esperar(patron_un_paso, 3, desde):
                SIGNO[0] = signo
                apuntar(f"la rueda hacia abajo es dz={signo}")
                break
        if SIGNO[0] is None:
            raise Fallo("la rueda no mueve lo desplegado en ningún sentido")
        n -= 1
    mandar(*[f"mouse_move 0 0 {SIGNO[0]}"] * n, pausa=0.25)
    time.sleep(0.5)


def rueda_hasta(patron, objetivo, desde, que):
    """Pasos de rueda hacia abajo, de uno en uno, hasta que la primera cosa que se ve (el grupo de `patron`, en lo
    último que se dijo desde `desde`) sea al menos `objetivo`. Devuelve esa primera. El ratón de QEMU a veces pierde
    o junta pasos (y una vez se vio uno al revés): lo que se prueba es que la rueda desplaza y que el clic elige lo que
    quedó debajo, no cuántos pasos llegan."""
    rueda_abajo(1, patron.replace(r"(\d+)", "[2-9]"), desde)
    pasos = 1
    while True:
        vistas = [int(n) for n in re.findall(patron, texto()[desde:])]
        if vistas and vistas[-1] >= objetivo:
            if pasos != objetivo - 1:
                apuntar(f"la rueda: {pasos} pasos para pasar de la 1.ª a la {vistas[-1]}.ª ({vistas})")
            return vistas[-1]
        if pasos >= 3 * objetivo:
            raise Fallo(f"{que}: {vistas}")
        mandar(f"mouse_move 0 0 {SIGNO[0]}")
        pasos += 1
        time.sleep(0.6)


def fuera_de(punto, rects):
    return not any(x <= punto[0] < x + w and y <= punto[1] < y + h for x, y, w, h in rects)


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
    pulido(dx, dy, ancho_dash, menu_dash, sx, sy)


def pulido(dx, dy, ancho_dash, menu_dash, sx, sy):
    """Lo de `menus-pulido` (pasos 7 a 11)."""
    titulo_shell = (sx + 450, max(sy - 20, 1))
    # 7. El teclado en la fila del Shell.
    desde = largo()
    teclas("f10", "right", "down")
    capturar("14-shell-teclas", 1.5)
    teclas("ret")
    debe(r"\[shell\] menú: Borrar la pantalla", 15, desde, "Intro en el menú del Shell no eligió Borrar la pantalla")
    # 8. La ventana de Dash (kit-c), dentro: delante por su borde izquierdo (lo mismo que en el paso 5).
    clic(dx + 12, dy + 300)
    time.sleep(1)
    desde = largo()
    teclas("f10", "right", "end")
    debe(r'\[dash-ui\] se ve: Largo: de "Entrada \d+" a "Entrada 30", foco en "Entrada 30"', 15, desde,
         "Fin en Largo (kit-c) no deja ver la última")
    # Sin mover el ratón: de camino a la esquina pasaría por el título "Proyecto" y lo abriría en su lugar.
    capturar("15-kitc-teclas-fin", 1)
    desde = largo()
    teclas("ret")
    debe(r"\[dash-ventana\] menú: Entrada 30", 15, desde, "Intro en Largo (kit-c) no eligió la 30")
    if "Largo" not in menu_dash:
        raise Fallo(f"la ventana de Dash no enseña Largo: {menu_dash}")
    lx, ly = menu_dash["Largo"]
    desde = largo()
    clic(dx + lx, dy + ly)
    debe(r'\[dash-ui\] se ve: Largo: de "Entrada 1"', 10, desde, "un clic en Largo (kit-c) no lo abre")
    rel(30, 46)
    desde = largo()
    p = rueda_hasta(r'\[dash-ui\] se ve: Largo: de "Entrada (\d+)"', 4, desde, "la rueda en Largo (kit-c) no lo desplaza")
    capturar("16-kitc-rueda", 1)
    desde = largo()
    pulsar()
    debe(rf"\[dash-ventana\] menú: Entrada {p}\b", 15, desde, f"el clic tras la rueda (kit-c) no eligió la {p}.ª")
    # Hostil: ráfagas de clics en el título (abre, cierra) y de teclas (abrir, cerrar, moverse), sin esperar.
    ir_a(dx + lx, dy + ly)
    for _ in range(10):
        mandar("mouse_button 1", "mouse_button 0", pausa=0.03)
    teclas(*(["f10", "right", "down", "esc", "f10", "f10", "left", "esc"] * 4), pausa=0.04)
    time.sleep(2)
    teclas("esc", "esc")
    time.sleep(1)
    desde = largo()
    teclas("f10", "right", "ret")
    debe(r"\[dash-ventana\] menú: Entrada 1", 15, desde, "tras las ráfagas, F10, Derecha e Intro (kit-c) no eligen")
    apuntar("kit-c: las ráfagas no lo rompen")
    # Con Largo abierto, un clic en el título del Shell: el de la ventana de Dash se cierra.
    teclas("f10", "right")
    time.sleep(1)
    desde = largo()
    clic(*titulo_shell)
    debe(r"\[dash-ui\] menús cerrados: el teclado se fue", 15, desde, "el menú de la ventana de Dash sigue abierto sin el teclado")
    time.sleep(0.5)

    # 9. A la barra otra vez: sale `menus-prueba`, con sus menús allí.
    desde = largo()
    teclear("Ajuste menus barra")
    w = debe(r"\[menus-prueba\] ventana en pantalla \((\d+)x", 60, desde, "no sale la ventana de menus-prueba")
    v = debe(r"\[compositor\] ventana de menus-prueba en \((\d+), (\d+)\)", 10, desde, "el compositor no dice dónde va menus-prueba")
    mx, my = int(v[-1][0]), int(v[-1][1])
    # Su borde derecho: casi tan ancha como la pantalla, con el Shell delante sigue a la vista.
    cuerpo = (min(mx + int(w[-1]), 800) - 15, my + 160)
    apuntar(f"menus-prueba en ({mx}, {my})")
    time.sleep(1)
    desde = largo()
    clic(*cuerpo)
    b = debe(r'(\[barra\] menús en la barra: "Ver"[^\n]*)', 20, desde, "la barra no enseña los menús de menus-prueba")
    barra = sitios(b[-1])
    # 9a. El teclado en el submenú de la barra.
    desde = largo()
    teclas("f10")
    debe(r"\[barra\] menú abierto: Ver;", 10, desde, "F10 en menus-prueba no abre sus menús en la barra")
    teclas("down", "down", "down", "down", "right")
    debe(r'\[barra\] se ve: Ver: [^\n]*; Muchas: de "Opción 1" [^\n]*foco en "Opción 1"', 10, desde, "Derecha no abre el submenú en la barra")
    teclas(*["down"] * 12)
    debe(r'\[barra\] se ve: Ver: [^\n]*; Muchas: de "Opción ([2-9]|1\d)" a "Opción 13", foco en "Opción 13"', 10, desde,
         "abajo en el submenú de la barra: la 13.ª no se ve")
    capturar("17-barra-submenu-teclas", 1)
    desde = largo()
    teclas("esc")
    debe(r'\[barra\] se ve: Ver: [^\n]*foco en "Muchas"\r?\n', 10, desde, "Esc en el submenú de la barra no vuelve al menú")
    teclas("right", *["down"] * 12, "ret")
    debe(r"\[barra\] elige en el menú: Opción 13 \(113\)", 10, desde, "Intro en el submenú de la barra no elige la 13.ª")
    debe(r"\[menus-prueba\] elegido: 113 \(Opción 13\)", 10, desde, "lo elegido en el submenú de la barra no llega al programa")
    # 9b. La rueda en "Largo", que no cabe en la pantalla.
    desde = largo()
    clic(*barra["Largo"])
    a = debe(r"(\[barra\] menú abierto: Largo;[^\n]*)", 10, desde, "un clic en Largo de la barra no lo abre")
    entradas = sitios(a[-1])
    ir_a(*entradas["Entrada 2"])
    desde = largo()
    p = rueda_hasta(r'\[barra\] se ve: Largo: de "Entrada (\d+)"', 6, desde, "la rueda en Largo de la barra no lo desplaza")
    capturar("18-barra-largo-rueda", 1)
    desde = largo()
    pulsar()
    debe(rf"\[barra\] elige en el menú: Entrada {p + 1} \({200 + p + 1}\)", 10, desde,
         f"el clic tras la rueda en la barra no elige la {p + 1}.ª")
    debe(rf"\[menus-prueba\] elegido: {200 + p + 1} ", 10, desde, "lo elegido tras la rueda en la barra no llega al programa")
    # 9c. La rueda en el submenú "Muchas" de la barra: por la fila de "Muchas" hacia la derecha hasta el submenú.
    desde = largo()
    clic(*barra["Ver"])
    a = debe(r"(\[barra\] menú abierto: Ver;[^\n]*)", 10, desde, "un clic en Ver de la barra no lo abre")
    ver = sitios(a[-1])
    ir_a(*ver["Muchas"])
    a = debe(r"(\[barra\] menú abierto: Ver > Muchas;[^\n]*)", 10, desde, "pasar por Muchas no abre el submenú en la barra")
    opciones = {int(t.split()[1]): p for t, p in sitios(a[-1]).items() if t.startswith("Opción ")}
    yy = ver["Muchas"][1]
    k0, (ox, _) = min(opciones.items(), key=lambda kv: abs(kv[1][1] - yy))
    rel(ox - ver["Muchas"][0], 0)
    apuntar(f"en el submenú, sobre la Opción {k0}")
    desde = largo()
    p = rueda_hasta(r'\[barra\] se ve: Ver: [^\n]*; Muchas: de "Opción (\d+)"', 5, desde,
                    "la rueda en el submenú de la barra no lo desplaza")
    capturar("19-barra-submenu-rueda", 1)
    desde = largo()
    pulsar()
    k = k0 + p - 1
    debe(rf"\[barra\] elige en el menú: Opción {k} \({100 + k}\)", 10, desde,
         f"el clic tras la rueda en el submenú de la barra no elige la {k}.ª")
    # 9d. El clic que cierra: con "Ver" abierto, un clic en el título del Shell (otra ventana) solo lo cierra.
    if not fuera_de(titulo_shell, [(mx - 10, my - 40, int(w[-1]) + 20, 380)]):
        raise Fallo(f"menus-prueba tapa el título del Shell: {titulo_shell}")
    desde = largo()
    clic(*barra["Ver"])
    debe(r"\[barra\] menú abierto: Ver;", 10, desde, "un clic en Ver de la barra no lo abre")
    time.sleep(0.5)
    desde = largo()
    clic(*titulo_shell)
    debe(r"\[barra\] menús cerrados: un clic fuera", 10, desde, "un clic fuera no cierra lo desplegado en la barra")
    debe(r"\[compositor\] menús: un clic fuera cierra lo desplegado", 10, desde, "el compositor no se traga el clic que cierra")
    ir_a(790, 590)
    capturar("20-clic-que-cierra", 1.5)
    if re.search(r"\[barra\] menús del programa activo: Shell", texto()[desde:]):
        raise Fallo("el clic que cerró el menú de la barra llegó al Shell (se quedó el teclado)")
    apuntar("el clic que cierra no llegó a la ventana")
    desde = largo()
    clic(*titulo_shell)
    debe(r"\[barra\] menús del programa activo: Shell", 10, desde, "el clic siguiente no llega al Shell")
    desde = largo()
    clic(*cuerpo)
    debe(r"\[barra\] menús del programa activo: Ver", 10, desde, "menus-prueba no vuelve a tener el teclado")
    # 9e. Hostil: ráfagas de clics (abrir y cerrar en el título, y fuera) y de teclas, sin esperar.
    for _ in range(6):
        clic(*barra["Ver"])
        clic(*cuerpo)
    ir_a(*barra["Largo"])
    for _ in range(10):
        mandar("mouse_button 1", "mouse_button 0", pausa=0.03)
    teclas(*(["esc", "f10", "down", "right", "down", "left", "esc", "esc"] * 3), pausa=0.04)
    time.sleep(2)
    teclas("esc", "esc")
    time.sleep(1)
    cerrados = texto().count("[barra] menús cerrados: un clic fuera")
    desde = largo()
    clic(*cuerpo)
    time.sleep(1.5)
    if texto().count("[barra] menús cerrados: un clic fuera") != cerrados:
        apuntar("tras las ráfagas quedó algo desplegado: el clic lo cerró")
        desde = largo()
        clic(*cuerpo)
        time.sleep(1.5)
        if texto().count("[barra] menús cerrados: un clic fuera") != cerrados + 1:
            raise Fallo("tras las ráfagas, el compositor sigue tragándose los clics")
    desde = largo()
    clic(*barra["Ver"])
    a = debe(r"(\[barra\] menú abierto: Ver;[^\n]*)", 10, desde, "tras las ráfagas, la barra no abre Ver")
    apuntar("barra: las ráfagas no la rompen")
    # 9f. Hostil: el programa cambia su menú veinte veces mientras está desplegado.
    ver = sitios(a[-1])
    g = re.findall(r"\[barra\] menús del programa activo: Ver, Largo \(generación (\d+)\)", texto())
    cambiar = next(t for t in ver if t.startswith("Cambiar el menú"))
    desde = largo()
    clic(*ver[cambiar])
    debe(r"\[menus-prueba\] cambia su menú: vuelta 1 de", 10, desde, "menus-prueba no empieza a cambiar su menú")
    clic(*barra["Ver"])
    teclas("down", "down")
    capturar("21-cambia-desplegado", 1)
    debe(r"\[menus-prueba\] cambia su menú: vuelta 20 de 20", 20, desde, "menus-prueba no acaba de cambiar su menú")
    time.sleep(1)
    debe(r'\[barra\] se ve: Ver: de "Recargar \(20\)"', 10, desde, "lo desplegado en la barra no siguió los cambios del menú")
    nuevas = re.findall(r"\[barra\] menús del programa activo: [^\n]*\(generación (\d+)\)", texto()[desde:])
    if g and any(n != g[-1] for n in nuevas):
        raise Fallo(f"los cambios del menú cambiaron la generación: {g[-1]} y {nuevas}")
    apuntar(f"el menú cambió {len(nuevas)} veces en la barra con lo desplegado abierto, generación {g[-1] if g else '?'}")
    teclas("esc")
    time.sleep(0.5)

    # 10. Dentro otra vez: el teclado y la rueda en la ventana de `menus-prueba`.
    clic(*titulo_shell)
    time.sleep(0.5)
    desde = largo()
    teclear("Ajuste menus dentro")
    m = debe(r'(\[menus-prueba\] ventana en pantalla \(\d+x\d+\); "Ver"[^\n]*)', 30, desde, "menus-prueba no pone su fila")
    fila = sitios(m[-1])
    clic(*cuerpo)
    time.sleep(1)
    desde = largo()
    teclas("f10", "down", "down", "down", "right", *["down"] * 9)
    debe(r'\[menus-prueba\] se ve: Ver: [^\n]*; Muchas: de "Opción ([2-9])" a "Opción 10", foco en "Opción 10"', 10, desde,
         "abajo en el submenú de la ventana: la 10.ª no se ve")
    capturar("22-dentro-submenu-teclas", 1)
    desde = largo()
    teclas("ret")
    debe(r"\[menus-prueba\] elegido: 110 \(Opción 10\)", 10, desde, "Intro en el submenú de la ventana no elige la 10.ª")
    desde = largo()
    clic(mx + fila["Largo"][0], my + fila["Largo"][1])
    debe(r'\[menus-prueba\] se ve: Largo: de "Entrada 1"', 10, desde, "un clic en Largo de la ventana no lo abre")
    rel(30, 46)
    p = rueda_hasta(r'\[menus-prueba\] se ve: Largo: de "Entrada (\d+)"', 4, desde, "la rueda en Largo de la ventana no lo desplaza")
    capturar("23-dentro-largo-rueda", 1)
    desde = largo()
    pulsar()
    debe(rf"\[menus-prueba\] elegido: {200 + p} \(Entrada {p}\)", 10, desde, f"el clic tras la rueda en la ventana no elige la {p}.ª")

    # 11. La ventana que se cierra con su menú desplegado en la barra.
    clic(*titulo_shell)
    time.sleep(0.5)
    desde = largo()
    teclear("Ajuste menus barra")
    debe(r"\[menus-prueba\] menús: en la barra", 30, desde, "menus-prueba no se entera de la barra")
    clic(*cuerpo)
    b = debe(r'(\[barra\] menús en la barra: "Ver"[^\n]*)', 20, desde, "la barra no enseña los menús de menus-prueba")
    barra = sitios(b[-1])
    desde = largo()
    clic(*barra["Ver"])
    a = debe(r"(\[barra\] menú abierto: Ver;[^\n]*)", 10, desde, "un clic en Ver de la barra no lo abre")
    ver = sitios(a[-1])
    cerrar = next(t for t in ver if t.startswith("Cerrar la ventana"))
    antes_de_cerrar = largo()
    clic(*ver[cerrar])
    debe(r"\[menus-prueba\] cierra su ventana en 3 s", 10, desde, "menus-prueba no va a cerrar su ventana")
    desde = largo()
    clic(*barra["Largo"])
    debe(r"\[barra\] menú abierto: Largo;", 10, desde, "un clic en Largo de la barra no lo abre")
    debe(r"\[menus-prueba\] cierra su ventana \(", 15, desde, "menus-prueba no cierra su ventana")
    debe(r"\[barra\] menús del programa activo: (?!Ver)", 15, desde, "la barra sigue con los menús de la ventana que se fue")
    ir_a(790, 590)
    capturar("24-ventana-cerrada", 1.5)
    desde = largo()
    clic(*titulo_shell)
    debe(r"\[barra\] menús del programa activo: Shell", 10, antes_de_cerrar, "tras cerrarse la ventana, un clic en el Shell no le llega")
    time.sleep(1)
    if re.search(r"\[barra\] menús cerrados: un clic fuera", texto()[desde:]):
        raise Fallo("tras cerrarse la ventana, el compositor se tragó un clic")
    desde = largo()
    teclear("Ajuste menus dentro")
    debe(r"\[shell\] menús: dentro de la ventana", 30, desde, "los menús no vuelven dentro")
    ver_kitc(dx, dy)
    centro_vivo(sx, sy, titulo_shell)
    teclear("Assign MenusHechos: RAM:")
    apuntar("acabado")


def ver_kitc(dx, dy):
    """12. El menú "Ver" de la ventana de Dash (kit-c): una casilla, opciones y un submenú."""
    debe(r"\[dash-ventana\] submenú dentro de otro: 0", 5, 0, "kit-c no da error con un submenú dentro de otro")
    clic(dx + 12, dy + 300)
    time.sleep(1)
    desde = largo()
    teclas("f10", "right", "right")
    debe(r'\[dash-ui\] se ve: Ver: [^\n]*foco en "Rejilla"', 15, desde, "F10 y dos veces Derecha no abren Ver (kit-c)")
    capturar("25-kitc-ver", 1)
    desde = largo()
    teclas("ret")
    debe(r"\[dash-ventana\] Rejilla: 0", 15, desde, "Intro en la casilla Rejilla (kit-c) no la desmarca")
    desde = largo()
    teclas("f10", "right", "right", "down", "down", "down", "ret")
    debe(r"\[dash-ventana\] tamaño: pequeño 0, mediano 0, grande 1", 15, desde, "la opción Grande (kit-c) no desmarca las otras")
    desde = largo()
    teclas("f10", "right", "right", "down", "down", "down", "down", "right")
    debe(r'\[dash-ui\] se ve: Ver: [^\n]*; Orden: de "Por nombre" a "Por tamaño", foco en "Por nombre"', 15, desde,
         "Derecha en Orden (kit-c) no abre el submenú")
    capturar("26-kitc-submenu", 1)
    desde = largo()
    # Si lo del submenú de dentro se hubiera puesto en este, la tercera sería "No sale".
    teclas("down", "down")
    debe(r'\[dash-ui\] se ve: Ver: [^\n]*; Orden: [^\n]*foco en "Por tamaño"', 15, desde,
         "la tercera del submenú (kit-c) no es Por tamaño: lo del submenú de dentro se puso")
    desde = largo()
    teclas("ret")
    debe(r"\[dash-ventana\] orden: 27", 15, desde, "Intro en Por tamaño (kit-c) no la elige")
    apuntar("kit-c: casilla, opciones y submenú")


def centro_vivo(sx, sy, titulo_shell):
    """13. Con Ajustes abierta, el centro atiende su canal: `Ajuste menus` se aplica al momento."""
    bb = debe(r"\[barra\] botones: lanzador en \((\d+), (\d+)\)", 5, 0, "la barra no dice dónde está")
    barra = (500, int(bb[-1][1]))
    clic(*titulo_shell)
    time.sleep(0.5)
    # Las órdenes, de un guion del Shell lanzado de fondo (`Run`): así Ajustes se queda delante y con el teclado (un
    # programa no puede traerse delante: con el Shell delante, Ajustes queda tapada y Esc no le llega).
    for linea in ['Echo >RAM:m "Wait 5"', 'Echo >>RAM:m "Ajuste menus barra"', 'Echo >>RAM:m "Wait 4"',
                  'Echo >>RAM:m "Ajuste menus dentro"']:
        teclear(linea)
        time.sleep(0.3)
    desde = abierta = largo()
    teclear("Run Execute RAM:m")
    time.sleep(0.5)
    clic(*barra, boton=2)
    debe(r"\[centro\] Barra en pantalla", 30, desde, "el botón derecho sobre la barra no abre Ajustes > Barra")
    a = debe(r"\[compositor\] ventana de centro en \((\d+), (\d+)\)", 10, desde, "el compositor no dice dónde va Ajustes")
    # Un clic en el título de la categoría (un texto: no hace nada): Ajustes, con el teclado (una ventana nueva no
    # siempre se lo lleva).
    en_ajustes = (int(a[-1][0]) + 300, int(a[-1][1]) + 40)
    t0 = time.time()
    debe(r"\[centro\] menús en marcha: en la barra, de ENV:Escritorio/menus; en el compositor: sí", 30, desde,
         "con Ajustes abierta, el centro no aplica `Ajuste menus barra`")
    apuntar(f"con Ajustes abierta, los menús a la barra a los {time.time() - t0:.1f} s de abrirse Ajustes (el guion espera 5)")
    debe(r"\[shell\] menús: en la barra", 10, desde, "con Ajustes abierta, el Shell no se entera de la barra")
    capturar("27-ajustes-abierta-menus-barra", 0.5)
    debe(r"\[centro\] menús en marcha: dentro de las ventanas", 20, desde, "con Ajustes abierta, el centro no aplica `Ajuste menus dentro`")
    debe(r"\[shell\] menús: dentro de la ventana", 10, desde, "con Ajustes abierta, el Shell no vuelve dentro")
    if "[centro] cerrado" in texto()[abierta:]:
        raise Fallo("Ajustes se cerró antes de tiempo")
    apuntar("con Ajustes abierta, los dos cambios aplicados")
    clic(*en_ajustes)
    time.sleep(0.5)
    desde = largo()
    mandar("sendkey esc")
    debe(r"\[centro\] cerrado", 15, desde, "Esc no cierra Ajustes")
    clic(*titulo_shell)
    time.sleep(0.5)
    # Hostil: 16 cambios seguidos (r1, dos; r2, r1 dos veces...), de fondo, mientras Ajustes se abre y se cierra a
    # ráfagas (el botón derecho sobre la barra, Esc). Queda el último: dentro.
    for linea in ['Echo >RAM:r1 "Ajuste menus barra"', 'Echo >>RAM:r1 "Ajuste menus dentro"',
                  'Echo >RAM:r2 "Execute RAM:r1"', 'Echo >>RAM:r2 "Execute RAM:r1"',
                  'Echo >RAM:r3 "Execute RAM:r2"', 'Echo >>RAM:r3 "Execute RAM:r2"',
                  'Echo >RAM:r4 "Execute RAM:r3"', 'Echo >>RAM:r4 "Execute RAM:r3"']:
        teclear(linea)
        time.sleep(0.3)
    time.sleep(1)
    desde = largo()
    teclear("Run Execute RAM:r4")
    time.sleep(1)
    for _ in range(8):
        clic(*barra, boton=2)
        time.sleep(0.4)
        clic(*en_ajustes)
        mandar("sendkey esc")
        time.sleep(0.3)
    # Hasta que no cambie nada en 8 s (como mucho 120).
    hasta, antes, quieto = time.time() + 120, -1, time.time()
    while time.time() < hasta:
        n = len(re.findall(r"\[centro\] menús en marcha", texto()[desde:]))
        if n != antes:
            antes, quieto = n, time.time()
        elif time.time() - quieto > 8:
            break
        time.sleep(0.5)
    hechos = re.findall(r"\[centro\] menús en marcha: ([^,]+),", texto()[desde:])
    shell = re.findall(r"\[shell\] menús: ([^\n]+)", texto()[desde:])
    abiertas = len(re.findall(r"\[centro\] Barra en pantalla", texto()[desde:]))
    cerradas = texto()[desde:].count("[centro] cerrado")
    apuntar(f"ráfaga: {len(hechos)} cambios aplicados, Ajustes abierta {abiertas} veces y cerrada {cerradas}")
    if not hechos or hechos[-1] != "dentro de las ventanas" or not shell or not shell[-1].startswith("dentro"):
        raise Fallo(f"tras la ráfaga no queda el último: centro {hechos[-3:]}, Shell {shell[-3:]}")
    if re.search(r"\[centro\] error de protocolo|número de objeto nuevo malo", texto()[desde:]):
        raise Fallo("tras la ráfaga, error de protocolo")
    # Y después, como siempre: se abre y se cierra.
    for _ in range(2):
        clic(*en_ajustes)
        mandar("sendkey esc")
        time.sleep(0.5)
    if texto()[abierta:].count("[centro] Barra en pantalla") > texto()[abierta:].count("[centro] cerrado"):
        raise Fallo("tras la ráfaga, Ajustes sigue abierta y Esc no la cierra")
    desde = largo()
    clic(*barra, boton=2)
    debe(r"\[centro\] Barra en pantalla", 30, desde, "tras la ráfaga, el botón derecho no abre Ajustes")
    time.sleep(1)
    clic(*en_ajustes)
    time.sleep(0.5)
    mandar("sendkey esc")
    debe(r"\[centro\] cerrado", 15, desde, "tras la ráfaga, Esc no cierra Ajustes")
    clic(*titulo_shell)
    time.sleep(0.5)
    apuntar("centro: con Ajustes abierta atiende su canal; la ráfaga no lo rompe")


try:
    principal()
except Fallo as e:
    apuntar(f"FALLA: {e}")
except Exception as e:
    apuntar(f"falla el guion: {e!r}")
