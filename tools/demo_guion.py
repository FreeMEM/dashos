"""El guion de la demo (`make demo`, `tools/demo.py grabar`): lo que hace una persona delante de DashOS, tramo a tramo.

Cada tramo empieza con `d.marca(nombre)`; el montaje (`tools/demo_montar.py`) lo junta con su locución y con su
escenario de EnMarcha. Todo espera su línea en la salida serie antes de seguir, y deja un rato para verlo (el montaje
acelera lo que sobra). Los sitios salen de la salida serie cuando la hay; los fijos son los de una pantalla de
1280x720 (la barra arriba, los discos del Workbench a la derecha, Ajustes maximizado).

La caja Amiga va a su ritmo desde el arranque (es la batería de pruebas de la caja, en el modo de las capturas): sus
preguntas a la guardia salen en el primer minuto y medio, y los programas del usuario, a los tres o cuatro minutos. Por
eso el Amiga va justo después de entrar, y la caja Linux al final (se para sola a los 25 minutos de arrancar).
"""
import re
import threading
import time

# Los discos del Workbench (columna de la derecha) y lo de Ajustes maximizado.
SYS = (1227, 87)
AJ_SEGURIDAD = (90, 147)
AJ_APARIENCIA = (90, 315)
AJ_ACERCA = (90, 483)
AJ_OSCURO = (450, 215)
AJ_CERRAR = (1223, 383)


def cerrar_de(x, y, ancho):
    """El botón de cerrar de la barra que pone el compositor encima de una ventana (40 de alto)."""
    return x + ancho - 20, max(y - 20, 1)


def ventanas(d, de):
    """Las ventanas de `de` que ha puesto el compositor, en orden: (x, y, ancho, alto)."""
    return [tuple(map(int, m)) for m in re.findall(
        r"\[compositor\] ventana de " + de + r" en \((-?\d+), (-?\d+)\)(?: geometría \([^)]*\))? de (\d+)x(\d+)", d.texto())]


def contestar_preguntas(d, hechas):
    """Contesta "no" (N) a cada pregunta nueva de la guardia, a los 3 s, como una persona que la lee."""
    n = d.cuenta(r"\[guardian\] pregunta en pantalla: ")
    if n > hechas:
        time.sleep(3)
        d.tecla("n")
        return n
    return hechas


TAPAR = [(r"ventana Amiga en pantalla \([^)]*\): Dash en DashOS", r"Dash ventana\.exe acab|Dash \S*ventana\S* acab")]


class Tapar(threading.Thread):
    """Apunta en tapar.json dónde y cuándo sale una ventana con algo que no debe salir en el vídeo (la prueba de Dash
    de la caja escribe el nombre del lenguaje, que lleva el de la cuenta del usuario): el montaje la emborrona."""

    def __init__(self, d):
        super().__init__(daemon=True)
        self.d = d
        self.hechas = []

    def run(self):
        import json
        vistos = [0] * len(TAPAR)
        while True:
            t = self.d.texto()
            for k, (abre, cierra) in enumerate(TAPAR):
                m = list(re.finditer(abre, t))
                if len(m) > vistos[k]:
                    vistos[k] = len(m)
                    # Su sitio lo dice el compositor (antes o después): la ventana de su tamaño más la barra (40).
                    an, al = map(int, re.search(r"\((\d+)x(\d+)", m[-1].group(0)).groups())
                    t0 = self.d.t()
                    sitio = None
                    while sitio is None and self.d.t() - t0 < 10:
                        s = re.findall(r"\[compositor\] ventana de caja-amiga en \((-?\d+), (-?\d+)\) de %dx%d" % (an, al + 40),
                                       self.d.texto())
                        sitio = tuple(map(int, s[-1])) if s else None
                        time.sleep(0.3)
                    x, y = sitio or (0, 0)
                    w, h = (an + 8, al + 48) if sitio else (self.d.ancho, self.d.alto)
                    while not re.search(cierra, self.d.texto()[m[-1].end():]) and self.d.t() - t0 < 120:
                        time.sleep(0.5)
                    self.hechas.append([t0 - 2, self.d.t() + 2, max(0, x - 4), max(0, y - 44), w, h])
                    json.dump(self.hechas, open(self.d.d / "tapar.json", "w"))
            time.sleep(0.5)


def correr(d):
    Tapar(d).start()
    d.marca("arranque")
    if not d.esperar(r"\[guardian\] pantalla de entrada pintada", 400):
        return
    d.pausa(2)

    # La entrada.
    d.marca("entrada")
    d.ir(640, 600)
    d.teclear("Ana", pausa=0.3)
    d.pausa(0.6)
    d.tecla("ret")
    d.pausa(1)
    d.teclear("clave de Ana", pausa=0.2)
    d.pausa(0.6)
    d.tecla("ret")
    if not d.esperar(r"\[guardian\] entra Ana", 60):
        return

    # El escritorio: lo que hay, con el puntero de paseo.
    d.marca("escritorio")
    d.pausa(2)
    d.ir(150, 300, pasos=20, s=1.2)
    d.pausa(2)
    d.ir(1227, 200, pasos=20, s=1.2)
    d.pausa(2)
    d.ir(640, 22, pasos=20, s=1.0)
    d.pausa(3)

    # La caja Amiga: las preguntas de la guardia (los virus de laboratorio) y, después, sus programas.
    hechas = 0
    hasta = time.time() + 240
    if d.cuenta(r"\[guardian\] pregunta en pantalla: ") == 0:
        d.esperar(r"\[guardian\] pregunta en pantalla: ", 180)
    d.marca("guardia")
    while time.time() < hasta:
        hechas = contestar_preguntas(d, hechas)
        t = d.texto()
        if "Dash hola.exe acab" in t or "ventana Amiga en pantalla (400x150" in t:
            break
        time.sleep(0.5)
    d.marca("dash")
    # Hasta Tachyon (el primer programa del usuario), contestando lo que quede.
    hasta = time.time() + 400
    while time.time() < hasta and not re.search(r"ventana Amiga en pantalla \([^)]*\): Tachyon", d.texto()):
        hechas = contestar_preguntas(d, hechas)
        time.sleep(0.5)
    d.marca("tachyon")
    d.pausa(4)
    w = ventanas(d, "caja-amiga")
    if w:
        x, y, an, al = w[-1]
        d.ir(x + an // 2, y + 60, pasos=20, s=1.5)
        d.pausa(3)
        d.ir(x + an // 3, y + al // 2, pasos=20, s=1.5)
    # El tiempo de Tachyon en la caja de las capturas: 30 s; luego la Calculator (falla al momento) y Forja.
    d.esperar(r"programa del usuario \S*Tachyon acab", 60)
    d.marca("calculadora")
    if not d.esperar(r"ventana Amiga en pantalla \([^)]*\): Forja", 150):
        d.apuntar("Forja no sale")
    d.marca("forja")
    d.pausa(5)
    w = ventanas(d, "caja-amiga")
    if w:
        x, y, an, al = w[-1]
        d.ir(x + 120, y + 100, pasos=20, s=1.5)
        d.pausa(4)
        d.ir(x + an // 2, y + al // 2, pasos=20, s=1.5)
    # GadTools: la ventana de la caja con un control de cada tipo (90 s en el modo de las capturas). Las ventanas de la
    # caja no se cierran a mano: el sitio de su botón depende de cómo las coloca la cascada, y la caja las cierra sola
    # al acabar cada programa (el montaje acelera la espera).
    if d.esperar(r"ventana Amiga en pantalla \([^)]*\): GadTools en DashOS", 120):
        d.marca("gadtools")
        d.pausa(2)
        w = ventanas(d, "caja-amiga")
        if w:
            x, y, an, al = w[-1]
            d.ir(x + an // 2, y + al // 2, pasos=15, s=1.0)
    # Hasta que acaba la caja: otra demo de GadTools en Dash (se cierra sola) y el error de un programa de Dash en un
    # requester, que en el modo de las capturas espera a que alguien pulse "Vale" (a 179, 62 de su sitio).
    hasta = time.time() + 400
    vales = 0
    while time.time() < hasta and not re.search(r"\[inicio\] caja-amiga acabó", d.texto()):
        t = d.texto()
        i = t.rfind("su error, en un requester")
        if i >= 0:
            r = re.findall(r"\[compositor\] ventana de caja-amiga en \((-?\d+), (-?\d+)\) de 352x115", t[i:])
            if len(r) > vales:
                vales = len(r)
                time.sleep(4)
                x, y = map(int, r[-1])
                d.clic(x + 179, y + 62)
        time.sleep(0.5)
    d.pausa(2)

    # El monitor del sistema (Trasfondo) desde la barra, y de ahí Ajustes.
    d.marca("monitor")
    abiertos = d.cuenta(r"\[barra\] procesos abiertos")
    d.clic(1090, 20)
    m = d.esperar(r"\[barra\] procesos abiertos: [^\n]*abrir el monitor en \((\d+), (\d+)\)", 15, desde=abiertos)
    d.pausa(4)
    d.ir(950, 250, pasos=15, s=1.0)
    d.pausa(2)
    centro = d.cuenta(r"\[compositor\] ventana de centro en")
    # El botón del pie, donde dice la barra (en el desplegable, que sale con su esquina en (762, 50)).
    bx, by = (762 + int(m[0]), 50 + int(m[1])) if m else (1028, 452)
    d.clic(bx, by)
    w = d.esperar(r"\[compositor\] ventana de centro en \((\d+), (\d+)\) de (\d+)x", 20, desde=centro)
    d.pausa(2)
    if w:
        x, y, an = map(int, w)
        d.clic(x + an // 2, max(y - 20, 1))
        d.pausa(0.5)
        d.tecla("meta_l-up")
    d.pausa(5)

    d.marca("ajustes")
    d.clic(*AJ_APARIENCIA)
    d.pausa(3)
    d.clic(*AJ_OSCURO, veces=2)
    d.pausa(5)
    d.clic(*AJ_SEGURIDAD)
    d.pausa(5)
    d.clic(*AJ_ACERCA)
    d.pausa(4)
    d.clic(*AJ_CERRAR)
    d.pausa(2)

    # El explorador: SYS: desde el Workbench, la lista, el volumen del Amiga y sus programas.
    d.marca("explorador")
    n = d.cuenta(r"\[explorador\] ventana \d+ en pantalla")
    d.clic(*SYS, veces=2)
    w = d.esperar(r"\[compositor\] ventana de explorador en \((\d+), (\d+)\) de (\d+)x", 20, desde=0)
    d.esperar(r"\[explorador\] ventana \d+ en pantalla", 20, desde=n)
    d.pausa(4)
    if w:
        x, y, an = map(int, w)
        d.clic(x + an - 153, y + 26)  # Vista: en lista
        d.pausa(4)
        d.clic(x + an - 153, y + 26)  # y otra vez en iconos
        d.pausa(2)
        d.clic(x + 74, y + 305)  # el volumen del Amiga, en la columna de la izquierda
        d.pausa(4)
        d.clic(x + 355, y + 110, veces=2)  # su primer cajón
        d.pausa(4)
        d.clic(x + 74, y + 179)  # SYS:
        d.pausa(3)
        d.clic(x + 355, y + 110, veces=2)  # Amiga, en SYS:
        d.pausa(3)
        d.clic(x + 479, y + 110, veces=2)  # Programas (Tachyon, la Calculator, Forja y los iconos)
        d.pausa(5)
        d.clic(x + an - 26, y + 26)  # Cerrar
    d.pausa(2)

    # El Shell, desde el lanzador de la barra.
    d.marca("shell")
    bb = d.esperar(r"\[barra\] botones: lanzador en \((\d+), (\d+)\)", 5)
    lx, ly = (int(bb[0]), int(bb[1])) if bb else (30, 20)
    abiertos = d.cuenta(r"\[barra\] lanzador abierto")
    d.clic(lx, ly)
    d.esperar(r"\[barra\] lanzador abierto", 10, desde=abiertos)
    d.pausa(1)
    d.teclear("shell", pausa=0.25)
    d.pausa(1)
    shells = d.cuenta(r"\[compositor\] ventana de [Ss]hell en")
    d.tecla("ret")
    w = d.esperar(r"\[compositor\] ventana de [Ss]hell en \((\d+), (\d+)\) de (\d+)x", 30, desde=shells)
    d.pausa(2)
    if w:
        x, y, an = map(int, w)
        d.clic(x + 150, max(y - 20, 1))
        d.pausa(0.5)
        d.tecla("meta_l-up")
        d.pausa(2)
    d.ir(1270, 700, pasos=10, s=0.5)
    d.teclear("List SYS:", intro=True)
    d.pausa(4)
    d.teclear('Echo "hola desde el Shell de DashOS"', intro=True)
    d.pausa(2)
    d.teclear("Ech")
    d.pausa(3)
    d.tecla("ctrl-x")
    d.teclear("Dir SYS:S")
    d.tecla("tab", "tab", pausa=0.8)
    d.pausa(3)
    d.tecla("esc", "ctrl-x", pausa=0.3)
    d.tecla("ctrl-shift-e")
    d.pausa(2)
    d.teclear("Dir SYS:C", intro=True)
    d.pausa(3)
    d.tecla("ctrl-shift-t")
    d.pausa(2)
    d.teclear("Version", intro=True)
    d.pausa(4)
    # Se cierra con EndShell en cada panel y pestaña (tres: dos paneles en la primera pestaña y la segunda), uno tras
    # otro: el botón de su barra cambia de sitio al maximizarla. Ni uno más: el siguiente iría a otra ventana.
    for _ in range(3):
        d.teclear("EndShell", intro=True)
        d.pausa(1.5)
    d.esperar(r"\[shell\] acaba con el código", 10)
    d.pausa(2)

    # La caja Linux: su terminal, un programa de GTK 4 y Firefox, por el puente de Wayland.
    d.marca("linux")
    t = ventanas(d, "caja-linux")
    tx, ty, tan, tal = t[0] if t else (72, 112, 656, 416)
    d.clic(tx + tan // 2, ty + tal - 40)
    d.pausa(1)
    linux = len(ventanas(d, r"caja-linux \(un programa de su puente de Wayland\)"))
    # Las dos órdenes de una vez: GTK ahora y Firefox medio minuto después. Así no hace falta volver a la terminal
    # cuando GTK (maximizada) la tapa; un clic a destiempo mandaba las teclas a la ventana que aún no se había cerrado.
    d.teclear("prueba-gtk &", intro=True)
    d.pausa(0.5)
    d.teclear("(sleep 40; prueba-firefox) &", intro=True)
    for _ in range(240):
        if len(ventanas(d, r"caja-linux \(un programa de su puente de Wayland\)")) > linux:
            break
        time.sleep(0.5)
    d.pausa(8)
    d.ir(640, 360, pasos=15, s=1.0)
    d.pausa(2)
    d.clic(1258, 62)  # cerrar (maximizada: su propio botón, arriba a la derecha)
    d.pausa(2)
    linux = len(ventanas(d, r"caja-linux \(un programa de su puente de Wayland\)"))
    d.marca("firefox")
    for _ in range(360):
        if len(ventanas(d, r"caja-linux \(un programa de su puente de Wayland\)")) > linux:
            break
        time.sleep(0.5)
    d.pausa(10)
    d.ir(500, 300, pasos=15, s=1.0)
    d.pausa(4)
    d.clic(1258, 62)
    d.pausa(3)

    d.marca("cierre")
    d.ir(640, 400, pasos=15, s=1.0)
    d.pausa(8)
    d.marca("fin")
