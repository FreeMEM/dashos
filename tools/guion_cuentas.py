#!/usr/bin/env python3
"""Las teclas y el ratón de `make arranque-virt-cuentas` (rama cuentas-2, docs/bitacoras/cuentas-2.md), por el monitor de
QEMU.

Uso (lo lanza `tools/prueba_uefi.sh` con CUENTAS_CAPTURAS=prefijo): guion_cuentas.py MONITOR SEGUNDOS SERIE PREFIJO

La imagen es la de usar con `cuentas` (`guardian` escribe los códigos de recuperación en la consola) y el disco no tiene
cuentas: es un primer arranque. Como lo haría una persona:

1. El asistente del primer arranque (01): "admin" (reservado, 02), y luego Eva; su código de recuperación (03); entra.
2. Ajustes se abre en Cuentas (04, `guardian` lo pide en esta imagen): "Fran" en el campo y "Nueva cuenta": la ventana
   del sistema con la contraseña de Eva y la de Fran (05) y el código de Fran (06). Fran al grupo "fotos" (07), se
   borra (08) y se devuelve; "Mi contraseña" cambia la de Eva (09). Se cierra Ajustes.
3. El panel, "Cerrar sesión" (en la imagen de usar nadie bloquea aún con Super+L): la pantalla de entrada con la lista
   (10); Fran, Tab (olvidé la contraseña, 11); un código malo (12) y el bueno, tecleado en minúsculas y sin guiones: el
   código nuevo (13); entra Fran.
4. Cierra la sesión de Fran: la entrada otra vez (14); Eva (15), con su contraseña nueva.

Cada captura queda también en PNG al lado. Lo que hace el guion, con la hora, en PREFIJO.guion.txt.
"""
import re
import socket
import sys
import time

ruta, total, serie, prefijo = sys.argv[1], float(sys.argv[2]), sys.argv[3], sys.argv[4]
inicio = time.time()
ANCHO = 800


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
    """Lo que case con `patron` desde la vez `desde` (las de antes no cuentan)."""
    hasta = time.time() + segundos
    while time.time() < hasta and time.time() - inicio < total:
        m = re.findall(patron, texto())
        if len(m) > desde:
            return m[desde:]
        time.sleep(0.25)
    apuntar(f"no sale: {patron}")
    return None


def cuenta(patron):
    return len(re.findall(patron, texto()))


def teclear(orden, intro=True):
    teclas = []
    for c in orden:
        if c == " ":
            teclas.append("sendkey spc")
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


def capturar(nombre, espera=1.5):
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


def codigo_de(quien, desde=0):
    m = esperar(r"\(prueba\) el código de recuperación de " + quien + r" es ([A-Z0-9-]{29})", 60, desde)
    return m[0] if m else None


def tandas():
    return cuenta(r"\[guardian\] tanda en pantalla: centro pide")


def tanda(antes, espera_contrasenas, captura=None):
    """La ventana de la tanda que pidió Ajustes (la siguiente a las `antes` que ya hubo): la contraseña de Eva (la
    propone ya como administradora) y, si crea una cuenta, la de la nueva."""
    if not esperar(r"\[guardian\] tanda en pantalla: centro pide", 40, antes):
        return False
    time.sleep(1.3)
    if captura:
        capturar(captura, 0.2)
    for c in espera_contrasenas:
        teclear(c)
    return True


def principal():
    time.sleep(3)
    # 1. El primer arranque.
    if not esperar(r"\[guardian\] pantalla de entrada pintada", 300):
        return
    capturar("01-asistente")
    for t in ("admin", "Eva Ruiz", "clave de Eva", "clave de Eva"):
        teclear(t)
    if not esperar(r"\[guardian\] primer arranque: crear admin: no", 60):
        return
    capturar("02-asistente-reservado")
    # A la caja del nombre (tres atrás), borrarlo y "Eva"; Intro hasta la última, que lo pide.
    mandar("sendkey shift-tab", "sendkey shift-tab", "sendkey shift-tab", *["sendkey backspace"] * 6, pausa=0.15)
    # Con todo puesto, Intro lo pide (el resto de cajas sigue lleno).
    teclear("Eva")
    codigo = codigo_de("Eva")
    if not codigo:
        return
    apuntar(f"código de Eva: {codigo}")
    capturar("03-codigo-primera")
    mandar("sendkey ret")
    if not esperar(r"\[guardian\] entra Eva", 60):
        return
    apuntar("entra Eva")
    # 2. Ajustes, en Cuentas.
    sitios = esperar(r"\[centro\] Cuentas, sitios: lista \((\d+), (\d+)\) campo \((\d+), (\d+)\) nueva \((\d+), (\d+)\) grupo \((\d+), (\d+)\) mi contraseña \((\d+), (\d+)\) borrar \((\d+), (\d+)\)", 90)
    ventana = esperar(r"\[compositor\] ventana de centro en \((-?\d+), (-?\d+)\)", 30)
    if not sitios or not ventana:
        return
    s = [int(v) for v in sitios[-1]]
    vx, vy = int(ventana[-1][0]), int(ventana[-1][1])
    lista, campo, nueva, grupo, mia, borrar = [(vx + s[2 * k], vy + s[2 * k + 1]) for k in range(6)]

    def al_centro():
        # Tras cerrarse la ventana del sistema, el primer clic solo devuelve el teclado a Ajustes: uno en su borde derecho,
        # donde no hay nada que pulsar.
        clic(vx + 546, vy + 40)
        time.sleep(0.3)

    apuntar(f"Ajustes en ({vx}, {vy}); campo en {campo}, nueva en {nueva}")
    capturar("04-ajustes-cuentas")
    clic(*campo)
    teclear("Fran", intro=False)
    antes = tandas()
    clic(*nueva)
    if not tanda(antes, ["clave de Eva", "clave de Fran"], "05-tanda-nueva"):
        return
    fran = codigo_de("Fran")
    if not fran:
        return
    apuntar(f"código de Fran: {fran}")
    # La ventana del código (no cuenta lo pulsado en su primer segundo).
    if not esperar(r"\[guardian\] código de recuperación en pantalla: Fran", 30):
        return
    capturar("06-codigo-fran", 1.3)
    mandar("sendkey ret")
    if not esperar(r'\[centro\] Cuentas: "crear Fran 0": 0', 60):
        return
    # Fran, la segunda fila (de 56): al grupo "fotos".
    time.sleep(1)
    al_centro()
    clic(lista[0], lista[1] + 56 + 28)
    clic(*campo)
    teclear("fotos", intro=False)
    antes = tandas()
    clic(*grupo)
    if not tanda(antes, ["clave de Eva"]):
        return
    if not esperar(r'\[centro\] Cuentas: "meter Fran fotos": 0', 60):
        return
    capturar("07-cuentas-fran")
    antes = tandas()
    al_centro()
    clic(*borrar)
    if not tanda(antes, ["clave de Eva"]):
        return
    if not esperar(r'\[centro\] Cuentas: "borrar Fran": 0', 60):
        return
    capturar("08-borrada")
    antes = tandas()
    al_centro()
    clic(*borrar)
    if not tanda(antes, ["clave de Eva"]):
        return
    if not esperar(r'\[centro\] Cuentas: "devolver Fran": 0', 60):
        return
    al_centro()
    clic(*mia)
    if not esperar(r"\[guardian\] cambio de contraseña en pantalla: Eva", 40):
        return
    capturar("09-mi-contrasena", 1.3)
    for t in ("clave de Eva", "nueva de Eva", "nueva de Eva"):
        teclear(t)
    if not esperar(r"\[guardian\] cambio de contraseña de Eva: cambiada", 60):
        return
    # Se cierra Ajustes con Esc, con el teclado suyo otra vez.
    time.sleep(1)
    al_centro()
    mandar("sendkey esc")
    if not esperar(r"\[centro\] cerrado", 20):
        return
    # 3. Cerrar la sesión de Eva desde el panel: la pantalla de entrada, con la lista; Fran, olvidé la contraseña.
    if not cerrar_sesion():
        return
    if not esperar(r"\[guardian\] pantalla de entrada: 2 cuentas en la lista", 20):
        return
    capturar("10-entrada-lista")
    # Fran, con el ratón: la segunda fila de la lista.
    f = esperar(r"\[guardian\] la lista en \((\d+), (\d+)\), filas de (\d+)", 10)
    if f:
        x, y, alto_fila = (int(v) for v in f[-1])
        clic(x, y + alto_fila)
    else:
        mandar("sendkey down", "sendkey ret", pausa=0.4)
    if not esperar(r"\[guardian\] lista: elige a Fran", 20):
        return
    time.sleep(0.5)
    mandar("sendkey tab")
    if not esperar(r"\[guardian\] olvidé la contraseña: Fran", 20):
        return
    capturar("11-olvide")
    for t in ("A" * 24, "nueva de Fran", "nueva de Fran"):
        teclear(t)
    if not esperar(r"\[guardian\] recuperar Fran: no", 60):
        return
    capturar("12-codigo-malo")
    # El bueno, en minúsculas y sin guiones; las dos nuevas siguen puestas: con Intro se pide.
    teclear(fran.replace("-", "").lower())
    nuevo = codigo_de("Fran", 1)
    if not nuevo:
        return
    apuntar(f"código nuevo de Fran: {nuevo}")
    capturar("13-codigo-nuevo")
    mandar("sendkey ret")
    if not esperar(r"\[guardian\] entra Fran", 60):
        return
    apuntar("entra Fran")
    # 4. Cerrar la de Fran: otra vez la entrada; vuelve Eva con su contraseña nueva.
    if not cerrar_sesion():
        return
    capturar("14-entrada-otra-vez")
    mandar("sendkey ret")
    if not esperar(r"\[guardian\] lista: elige a Eva", 20):
        return
    capturar("15-entrada-clave", 1.0)
    teclear("nueva de Eva")
    apuntar("Eva entra otra vez" if esperar(r"\[guardian\] entra Eva \(tras cerrar", 60) else "Eva no entra otra vez")


def cerrar_sesion():
    """El panel (Super+A), su botón de apagado y "Cerrar sesión"; espera a la pantalla de entrada."""
    time.sleep(2)
    b = None
    for _ in range(3):
        abiertos = cuenta(r"\[panel\] botón de apagado a \(")
        mandar("sendkey meta_l-a")
        b = esperar(r"\[panel\] botón de apagado a \((\d+), (\d+)\)", 10, abiertos)
        if b:
            break
    if not b:
        return False
    capa = re.findall(r"\[compositor\] capa de panel en \((\d+), (\d+)\)", texto())
    sube = int(capa[-1][1]) - 8 if capa else 0
    time.sleep(0.5)
    menus = cuenta(r"\[panel\] menú de apagado: ")
    clic(ANCHO - int(b[-1][0]), int(b[-1][1]) + sube)
    m = esperar(r"\[panel\] menú de apagado: .*cerrar sesión \((\d+), (\d+)\)", 10, menus)
    if not m:
        return False
    time.sleep(0.5)
    entradas = cuenta(r"pantalla de entrada pintada")
    clic(ANCHO - int(m[-1][0]), int(m[-1][1]) + sube)
    return esperar(r"pantalla de entrada pintada", 40, entradas) is not None


try:
    principal()
except Exception as e:  # que se vea en el fichero del guion
    apuntar(f"el guion falla: {e!r}")
