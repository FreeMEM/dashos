#!/usr/bin/env python3
"""Las teclas de `make arranque-virt-linux` (la terminal de la caja Linux, H9), por el monitor de QEMU.

Uso (lo lanza `tools/prueba_uefi.sh` con CAJA_LINUX=prefijo): guion_caja_linux.py MONITOR SEGUNDOS SERIE PREFIJO [MODO]

Con la caja Linux en marcha y su ventana de terminal en pantalla, y la shell de busybox lista en hvc0:

1. Un clic en la ventana (sale en la salida serie dónde la puso el compositor), para que tenga el teclado.
2. Teclea `uname -a` y espera su respuesta; luego `echo hola desde DashOS` y espera "hola desde DashOS".
3. Captura la pantalla (PREFIJO.ppm, y PREFIJO.png al lado).
4. Hostil: `seq 1 30000` inunda la consola (unas 170 KB: al registro solo sale una parte); después `echo sigue
   viva` tiene que contestar.
5. Si hay red, `ping -c 2 10.0.2.2` (la puerta de enlace de QEMU, por el servicio de red de DashOS).
6. `poweroff -f`: Linux apaga la máquina por PSCI.

Con MODO "arch" (`make arranque-virt-arch`), Arch Linux ARM con systemd: espera el prompt de root en hvc0 (getty
entra solo), hace clic en la ventana y teclea `uname -a`, `systemctl is-system-running --wait`, `ip -4 addr show eth0`
(la dirección por DHCP), `date`, `pacman -Sy --noconfirm` (baja el índice de paquetes de Arch Linux ARM) y
`ls /var/lib/pacman/sync`; captura la pantalla y apaga con `poweroff` (systemd para todo y apaga por PSCI).

Con MODO "wayland" (`make arranque-virt-wayland`), lo mismo hasta el prompt de root y después las ventanas de Linux
(H9, el puente de Wayland): mira que el puente está en marcha, lanza `puente --hostil` (tramas rotas por vsock: DashOS
las corta y sigue), abre `ventana_wayland` (un cliente de libwayland con wl_shm), hace clic en su ventana y pulsa "a"
(las dos cosas le llegan), espera su medida de cuadros por segundo, captura la pantalla (PREFIJO.png) y la cierra con
"q". Mide después cuadros por segundo sin copia y con copia (`/run/dashos-puente-copia`), de 320 x 240 y de
1280 x 720, 8 s cada una, captura la grande (PREFIJO.grande.png) y saca lo que dijo el puente (`journalctl`, con lo
que gastó de CPU). Después abre foot (una terminal de Wayland de verdad) con `prueba-foot`, teclea "hola" en su ventana (la shell
de dentro lo escribe en hvc0), la captura (PREFIJO.foot.png) y apaga.

Con MODO "gpu" (`make arranque-virt-gpu`, S25), lo mismo hasta el prompt de root y después programas de GL de Linux
(Mesa con llvmpipe): `cuadros_gl wayland 20` (por el puente: búferes de wl_shm copiados por vsock; después 15 s sin
esperar al compositor entre cuadros), `es2gears_wayland`
20 s, `time vkcube --wsi wayland --c 100` (Vulkan con lavapipe) y `cuadros_gl kms 20` (por la virtio-gpu de la caja: su pantalla sale en una ventana de DashOS). Captura cada uno
(PREFIJO.wayland.png, PREFIJO.gears.png, PREFIJO.kms.png), espera sus cuadros por segundo y apaga.

Con MODO "aspecto" (`make arranque-virt-aspecto`, S26), lo mismo hasta el prompt de root y después el aspecto de los
programas de Linux: espera al agente del aspecto y abre `muestra-gtk4` y foot, uno a uno, con su captura
(PREFIJO.gtk4.png y PREFIJO.foot.png). GTK 3, aún no (aborta en la caja al cargar sus iconos).

Cada paso espera su línea en la salida serie y repite la orden (hasta tres veces) si no llega. Teclado español: el
guion de la orden "-" es la tecla de la barra en el de EE. UU.
"""
import os
import re
import socket
import sys
import time

ruta, total, serie, prefijo = sys.argv[1], float(sys.argv[2]), sys.argv[3], sys.argv[4]
modo = sys.argv[5] if len(sys.argv) > 5 else "busybox"
inicio = time.time()


def mandar(*ordenes, pausa=0.05):
    s = socket.socket(socket.AF_UNIX)
    s.connect(ruta)
    for o in ordenes:
        s.sendall(o.encode() + b"\n")
        time.sleep(pausa)
    s.close()


def texto():
    try:
        return open(serie, errors="replace").read()
    except OSError:
        return ""


def esperar_n(patron, n, segundos):
    """Espera a que `patron` salga `n` veces."""
    hasta = time.time() + segundos
    while time.time() < hasta and time.time() - inicio < total:
        if len(re.findall(patron, texto())) >= n:
            return True
        time.sleep(0.25)
    return False


def esperar(patron, segundos):
    hasta = time.time() + segundos
    while time.time() < hasta and time.time() - inicio < total:
        if re.search(patron, texto()):
            return True
        time.sleep(0.25)
    return False


# Teclado español: lo que hay que pulsar (nombres de `sendkey` de QEMU, posición de EE. UU.) para cada letra.
ESPECIALES = {" ": "spc", "-": "slash", ".": "dot", ",": "comma", "/": "shift-7", "=": "shift-0", ":": "shift-dot",
              "_": "shift-slash", "&": "shift-6", ";": "shift-comma", "<": "less", ">": "shift-less"}


# La terminal de la caja (su ventana nativa), por su título en la barra de DashOS.
TERMINAL = r"^Linux · Linux \(caja\)$"


def a_la_terminal_si_no_la_tiene():
    """firefox-firme: antes de teclear una orden, que la terminal tenga el teclado. Entre una orden y otra (esperando su
    respuesta) pasan más de 2 s sin teclas, y una ventana nueva de otro programa puede llevárselo (S8): con la máquina
    cargada, las de la caja Amiga salen a cualquier hora, y la orden se tecleaba en ellas. Si la última ventana que
    nombró la barra no es la terminal, un clic en ella y se espera a que la barra la nombre."""
    t = titulos()
    if not t or re.search(TERMINAL, t[-1]):
        return
    clic_en_ventana()
    if not con_teclado(TERMINAL, 15):
        t = titulos()
        apuntar(f"la terminal no tiene el teclado tras el clic (lo tiene «{t[-1] if t else '?'}»)")


def teclear(orden, terminal=True):
    """Teclea la orden e Intro; en la terminal de la caja, salvo `terminal=False` (en otra ventana, como Firefox)."""
    if terminal:
        a_la_terminal_si_no_la_tiene()
    teclas = []
    for c in orden:
        if c in ESPECIALES:
            teclas.append("sendkey " + ESPECIALES[c])
        elif c.isupper():
            teclas.append("sendkey shift-" + c.lower())
        else:
            teclas.append("sendkey " + c)
    teclas.append("sendkey ret")
    mandar(*teclas, pausa=0.12)


def paso(orden, patron, segundos=20):
    for vez in range(3):
        # Al repetir, antes otro clic en la terminal: una ventana que sale después (la de la caja Amiga, que `inicio`
        # lanza tras la caja Linux) se lleva el teclado al salir.
        if vez > 0:
            clic_en_ventana()
        teclear(orden)
        if esperar(patron, segundos):
            return True
    return False


def ir_a(x, y):
    # A la esquina de arriba a la izquierda: hasta 2000 px (en Full HD, 12 pasos de 100 no llegaban desde la derecha).
    mandar(*["mouse_move -100 -100"] * 20, pausa=0.03)
    pasos = []
    while x > 0 or y > 0:
        dx, dy = min(x, 100), min(y, 100)
        pasos.append(f"mouse_move {dx} {dy}")
        x, y = x - dx, y - dy
    mandar(*pasos, pausa=0.05)
    time.sleep(0.3)


def a_png(ppm, png):
    try:
        from PIL import Image
        Image.open(ppm).save(png)
    except Exception as e:  # sin PIL, se queda el PPM
        print("sin PNG:", e)


def clic_en_ventana():
    # Abajo a la izquierda de la ventana (656x416): las que salen después (Ajustes) caen 32 px más a la derecha y abajo
    # y pueden tapar el centro; un clic en otra le daría el teclado. Con ventanas de Linux encima (H9), el primer sitio
    # de la terminal que no tapa ninguna.
    v = re.search(r"\[compositor\] ventana de caja-linux en \((\d+), (\d+)\)", texto())
    if v:
        tx, ty = int(v.group(1)), int(v.group(2))
        sitio = (tx + 12, ty + 380)
        encima = tapan()
        for dy in (380, 300, 200, 100, 12):
            for dx in (12, 150, 300, 450, 600, 640):
                px, py = tx + dx, ty + dy
                if not any(x <= px < x + w and y <= py < y + h for x, y, w, h in encima):
                    sitio = (px, py)
                    break
            else:
                continue
            break
        ir_a(*sitio)
        mandar("mouse_button 1", "mouse_button 0", pausa=0.08)
        time.sleep(0.5)


def capturar(nombre):
    mandar("screendump " + nombre + ".ppm")
    time.sleep(1.5)
    a_png(nombre + ".ppm", nombre + ".png")


def ventana_linux(n):
    """Dónde puso el compositor la ventana número `n` (desde 1) de un programa de Linux."""
    v = re.findall(r"\[compositor\] ventana de caja-linux \(un programa de su puente de Wayland\) en \((-?\d+), (-?\d+)\)", texto())
    return (int(v[n - 1][0]), int(v[n - 1][1])) if len(v) >= n else None


def clic(x, y):
    ir_a(x, y)
    mandar("mouse_button 1", "mouse_button 0", pausa=0.08)
    time.sleep(0.5)


def apuntar(texto_medida):
    """Una medida o un resultado del guion, en PREFIJO.medidas.txt (la salida del guion no va a ningún sitio)."""
    with open(prefijo + ".medidas.txt", "a") as f:
        f.write(texto_medida + "\n")


def fallo(motivo):
    """Algo que la prueba espera no pasó: se apunta como medida y en PREFIJO.fallo. Con ese fichero, `prueba_uefi.sh`
    para la prueba poco después de que acabe el guion (y lo enseña), en vez de esperar hasta su tope."""
    apuntar(motivo)
    with open(prefijo + ".fallo", "a") as f:
        f.write(motivo + "\n")


def titulos():
    """Los títulos que dijo la barra de DashOS, en orden: cada uno es el de la ventana que acaba de llevarse el teclado
    (o el nuevo de la que lo tiene)."""
    return re.findall(r"\[barra\] título: ([^\r\n]*)", texto())


def con_teclado(ventana, segundos):
    """Espera la señal de que la ventana (su título, `ventana`) tiene el teclado: es la última que nombró la barra."""
    hasta = time.time() + segundos
    while time.time() < hasta and time.time() - inicio < total:
        t = titulos()
        if t and re.search(ventana, t[-1]):
            return True
        time.sleep(0.25)
    return False


def teclear_en(ventana, pulsar, teclas, hecho, plazo, que):
    """Teclear en una ventana de Linux sin que lo tecleado se vaya a otra (firefox-firme). `pulsar()` le da el teclado
    (un clic); antes de `teclas()` se espera la señal de que lo tiene (la barra dice su título, `ventana`), y después, la
    línea que dice que llegó (`hecho`, una vez más que antes). Si no llega y entre medias la barra nombró otra ventana,
    otra se llevó el teclado (una ventana nueva de otro programa puede, pasados `SIN_ROBAR_MS` sin tecla ni clic: con
    la máquina cargada, las de la caja Amiga salen cuando quieren): se apunta y se repite, hasta tres veces. Si no la
    nombró, lo tecleado sí iba a la ventana: no se repite (es un fallo de verdad)."""
    for vez in range(3):
        antes = len(re.findall(hecho, texto()))
        pulsar()
        if not con_teclado(ventana, 30):
            t = titulos()
            apuntar(f"{que}: tras el clic, el teclado lo tiene «{t[-1] if t else '?'}», no la ventana; otra vez")
            continue
        n = len(titulos())
        teclas()
        if esperar_n(hecho, antes + 1, plazo):
            if vez > 0:
                apuntar(f"{que}: bien al intento {vez + 1}")
            return True
        otras = [t for t in titulos()[n:] if not re.search(ventana, t)]
        if not otras:
            return False
        apuntar(f"{que}: mientras se tecleaba, el teclado se fue a «{otras[0]}»; otra vez")
    return False


def teclear_sin_intro(orden):
    a_la_terminal_si_no_la_tiene()
    teclas = []
    for c in orden:
        if c in ESPECIALES:
            teclas.append("sendkey " + ESPECIALES[c])
        elif c.isupper():
            teclas.append("sendkey shift-" + c.lower())
        else:
            teclas.append("sendkey " + c)
    mandar(*teclas, pausa=0.12)


VENTANA_LINUX = r"\[compositor\] ventana de caja-linux \(un programa de su puente de Wayland\) en \((-?\d+), (-?\d+)\)(?: geometría \((-?\d+), (-?\d+), (\d+), (\d+)\))?(?: de (\d+)x(\d+))?"
# Lo que tapa cada ventana de Linux (x, y, ancho, alto; con la barra de DashOS), y lo que se movió (las que arrastra el
# guion): para hacer clic en la terminal de la caja donde no haya otra encima.
movidas = {}


def tapan():
    out = []
    for i, w in enumerate(re.findall(VENTANA_LINUX, texto())):
        x, y = int(w[0]), int(w[1])
        if w[6]:
            ancho, alto = int(w[6]), int(w[7])
            # La barra va encima de la superficie: lo que mide ya la cuenta.
            barra = 40 if not w[2] and alto > 40 else 0
            dx, dy = movidas.get(i, (0, 0))
            out.append((x + dx, y + dy - barra, ancho + 60, alto + 60))
    return out


def ventanas_linux():
    return re.findall(VENTANA_LINUX, texto())


def esperar_ventana(antes, segundos):
    """Espera una ventana de Linux nueva (más de `antes`): (x, y, geometría o None)."""
    hasta = time.time() + segundos
    while time.time() < hasta and time.time() - inicio < total:
        v = ventanas_linux()
        if len(v) > antes:
            w = v[antes]
            g = tuple(int(n) for n in w[2:6]) if w[2] else None
            return int(w[0]), int(w[1]), g
        time.sleep(0.25)
    return None


def arrastrar(desde, pasos, paso):
    """Pulsa en `desde` y mueve el ratón `pasos` veces `paso` con el botón pulsado; suelta."""
    ir_a(*desde)
    mandar("mouse_button 1", pausa=0.1)
    mandar(*[f"mouse_move {paso[0]} {paso[1]}" for _ in range(pasos)], pausa=0.05)
    mandar("mouse_button 0", pausa=0.1)
    time.sleep(0.5)


def a_la_terminal():
    """Un clic en la terminal de la caja y Control+C: la línea de la shell, vacía (si algo se quedó a medias, como un
    pegado con comillas, no se arrastra a lo siguiente)."""
    clic_en_ventana()
    mandar("sendkey ctrl-c")
    time.sleep(1.0)


def terminal_dashos():
    """Dónde está la terminal de la caja (la ventana nativa) y sus celdas: (x, y, ancho de celda, alto, borde)."""
    v = re.search(r"\[compositor\] ventana de caja-linux en \((\d+), (\d+)\)", texto())
    c = re.search(r"terminal: ventana en pantalla \(.*celdas de (\d+)x(\d+) desde \((\d+), ", texto())
    if not v or not c:
        return None
    return int(v.group(1)), int(v.group(2)), int(c.group(1)), int(c.group(2)), int(c.group(3))


# CAJA_SOLO=gtk, firefox o grandes (los dos; para probarlos sin esperar a lo demás, caja-linux-grandes): en el modo wayland, solo eso
# programa grande, sin la ventana de prueba, foot ni los hostiles. Las líneas esperadas de siempre no salen enteras.
solo = os.environ.get("CAJA_SOLO", "")

if modo == "wayland":
    if not esperar(r"login: root \(automatic login\)", total):
        sys.exit(1)
    time.sleep(5.0)
    clic_en_ventana()
    paso("systemctl is-active dashos-puente", r"linux: active\s*$|linux: active\n", 120)
    paso("puente --hostil vsock:5000", r"puente hostil: hecho", 120)
    if not solo:
        # 1. Un programa de Linux sin barra propia: lleva la de DashOS (xdg-decoration).
        n = len(ventanas_linux())
        teclear("ventana_wayland &")
        if not esperar(r"ventana_wayland: primer cuadro en pantalla", 180):
            sys.exit(1)
        time.sleep(1.0)
        v = esperar_ventana(n, 10)
        if v:
            x, y, _ = v
            # La tecla, justo tras el clic (sin esperar a que el programa diga que le llegó: con la máquina lenta, eso
            # tardaba más de los 2 s en que una ventana nueva de otro programa puede llevarse el teclado, S8).
            def pulsar():
                clic(x + 160, y + 120)
            if not teclear_en(r"^Linux · Hola desde Linux", pulsar, lambda: mandar("sendkey a"), r"ventana_wayland: tecla 30 pulsada", 30, "ventana_wayland"):
                fallo("ventana_wayland: no le llegó la tecla \"a\"")
            esperar(r"ventana_wayland: en marcha", 60)
            time.sleep(1.0)
            capturar(prefijo)
            # Moverla por su barra (la medida de cuadros al arrastrar) y cerrarla con su botón.
            arrastrar((x + 100, y - 20), 40, (4, 2))
            movidas[len(ventanas_linux()) - 1] = (160, 80)
            esperar(r"\[compositor\] arrastre: ", 20)
            clic(x + 160 + 320 - 4 - 16, y + 80 - 20)
            if not esperar(r"ventana_wayland: el compositor pide cerrar", 30):
                mandar("sendkey q")
        esperar(r"ventana_wayland: .* adiós", 30)
        # Medidas: sin copia y con copia (el puente copia a los programas que conectan con el fichero puesto), de 320 x 240
        # y de 1280 x 720, 8 s cada una.
        a_la_terminal()
        for copia, ancho, alto in [(False, 320, 240), (True, 320, 240), (False, 1280, 720), (True, 1280, 720)]:
            clic_en_ventana()
            teclear("touch /run/dashos-puente-copia" if copia else "rm -f /run/dashos-puente-copia")
            time.sleep(1.0)
            antes = len(re.findall(rf"ventana_wayland: medida de {ancho} x {alto}", texto()))
            teclear(f"ventana_wayland {ancho} {alto} 8")
            if ancho == 1280 and not copia and esperar_n(r"primer cuadro en pantalla .*\(1280 x 720\)", 1, 180):
                time.sleep(4.0)
                capturar(prefijo + ".grande")
            esperar_n(rf"ventana_wayland: medida de {ancho} x {alto}", antes + 1, 180)
            time.sleep(1.0)
        clic_en_ventana()
        teclear("rm -f /run/dashos-puente-copia")
        time.sleep(1.0)
        clic_en_ventana()
        teclear("journalctl -u dashos-puente -o cat --no-pager")
        esperar_n(r"CPU del puente", 5, 60)
        # 2. foot, y el portapapeles con DashOS.
        clic_en_ventana()
        n = len(ventanas_linux())
        teclear("prueba-foot &")
        t = terminal_dashos()
        if esperar(r"foot: lista", 600) and t:
            fv = esperar_ventana(n, 30)
            time.sleep(3.0)
            tx, ty, cw, ch, borde = t
            if fv:
                fx, fy, _ = fv
                # Tres clics en su primera línea: la selecciona (y va a la selección primaria); Control+Mayúsculas+C la copia.
                ir_a(fx + 40, fy + 10)
                mandar("mouse_button 1", "mouse_button 0", "mouse_button 1", "mouse_button 0", "mouse_button 1", "mouse_button 0", pausa=0.06)
                time.sleep(1.0)
                mandar("sendkey ctrl-shift-c")
                time.sleep(1.0)
                # Arriba del todo, por su barra: la terminal de la caja (casi toda la pantalla) la taparía entera al tener el
                # teclado, y así siempre se ve la franja de arriba de foot.
                sube = (fy - 80) // 8
                arrastrar((fx + 200, fy - 20), sube, (0, -8))
                fy -= sube * 8
                movidas[len(ventanas_linux()) - 1] = (0, -sube * 8)
                # Pegar en la terminal de DashOS (nativa): echo y lo pegado.
                a_la_terminal()
                teclear_sin_intro("echo ")
                mandar("sendkey ctrl-shift-v")
                esperar(r"terminal: pegado \(", 30)
                time.sleep(0.5)
                mandar("sendkey ret")
                if esperar(r"linux: Hola desde foot, en Arch Linux ARM, en DashOS", 30):
                    apuntar("portapapeles: de foot a DashOS, bien")
                # La primaria, con el botón central.
                teclear_sin_intro("echo primaria: ")
                ir_a(tx + 300, ty + 200)
                mandar("mouse_button 4", "mouse_button 0", pausa=0.1)
                esperar(r"terminal: pegado \(.*\n(.|\n)*terminal: pegado \(", 30)
                time.sleep(0.5)
                mandar("sendkey ret")
                if esperar(r"linux: primaria: Hola desde foot", 30):
                    apuntar("primaria: de foot a DashOS, bien")
                # De DashOS a foot: se selecciona con el ratón en la terminal nativa y Control+Mayúsculas+C.
                a_la_terminal()
                paso("clear; echo pegame-en-foot", r"linux: pegame-en-foot\s*$|linux: pegame-en-foot\n", 30)
                time.sleep(1.0)
                fila = ty + borde + ch // 2
                arrastrar((tx + borde + cw // 2, fila), 1, (13 * cw, 0))
                esperar(r"terminal: seleccionado \"pegame-en-foot\"", 20)
                mandar("sendkey ctrl-shift-c")
                esperar(r"terminal: copiado", 20)
                clic(fx + 100, fy + 40)
                mandar("sendkey ctrl-shift-v")
                time.sleep(2.0)
                capturar(prefijo + ".portapapeles")
                mandar("sendkey ret")
                if esperar(r"foot: leido pegame-en-foot", 60):
                    apuntar("portapapeles: de DashOS a foot, bien")
                capturar(prefijo + ".foot")
        time.sleep(4.0)
    # 3. Un programa grande: GTK4 (gtk4-widget-factory), dibujado por la CPU.
    a_la_terminal()
    n = len(ventanas_linux())
    # CAJA_GTK_DEPURAR=1: con WAYLAND_DEBUG (en /tmp/gtk.log del invitado; su final sale por la terminal al acabar).
    depurar = os.environ.get("CAJA_GTK_DEPURAR") == "1"
    # `prueba-gtk-guion` abre GTK, espera 300 s (lo que sigue la prueba) y lanza el hostil de la memoria con ella
    # abierta: no se teclea con su ventana delante (con 800 x 600 tapa entera la terminal de la caja).
    if solo != "firefox":
        teclear("WAYLAND_DEBUG=1 prueba-gtk-guion 2>/tmp/gtk.log &" if depurar else "prueba-gtk-guion &")
    if solo != "firefox" and esperar(r"gtk: empieza", 120):
        t0 = time.time()
        # Hasta su primera ventana (sin el zram del invitado, con 256 MB, abortaba al cargar su icono: la bitácora).
        g = esperar_ventana(n, 240)
        if not g:
            apuntar("gtk4-widget-factory: sin ventana (" + ("Gdk-ERROR al cargar su icono" if "Gdk-ERROR" in texto() else "?") + ")")
        if g:
            apuntar(f"gtk4-widget-factory: primera ventana a los {time.time() - t0:.1f} s (desde que empieza, en el reloj del PC)")
            x, y, geo = g
            gx, gy, gw, gh = geo if geo else (0, 0, 600, 400)
            # Lo que se ve de la pantalla y dónde está la terminal de la caja (con 800 x 600, GTK es más ancha que la
            # pantalla y sale en x negativa; en Full HD, la terminal tapa la izquierda de su barra).
            p = re.search(r"pantalla del firmware de (\d+)x(\d+)", texto())
            ancho = int(p.group(1)) if p else 800
            t = re.search(r"\[compositor\] ventana de caja-linux en \((\d+), (\d+)\) de (\d+)x(\d+)", texto())
            term = tuple(int(n) for n in t.groups()) if t else (0, 0, 0, 0)
            def libre(px, py):
                return 0 <= px < ancho and not (term[0] <= px < term[0] + term[2] and term[1] <= py < term[1] + term[3])
            # Su barra (la que se pinta GTK), donde no haya pestañas ni botones: con la ventana de Full HD (1666 de ancho),
            # entre las pestañas y el botón del menú; más estrecha, a la izquierda (las pestañas van en el centro).
            sitios = [(x + gx + gw - 400, y + gy + 16)] if gw >= 1600 else []
            sitios += [(x + gx + 60, y + gy + 16), (x + gx + 160, y + gy + 16)]
            barra = next((b for b in sitios if libre(*b)), sitios[0])
            # Arriba otra vez (un clic en su barra, sin moverla).
            time.sleep(15.0)
            clic(*barra)
            time.sleep(5.0)
            capturar(prefijo + ".gtk")
            # Su menú (el botón de las tres rayas, arriba a la derecha): un emergente (xdg_popup).
            emergentes = lambda: len(re.findall(r"\[compositor\] emergente de caja-linux", texto()))
            n_em = emergentes()
            menu = (x + gx + gw - 164, y + gy + 23)
            if libre(*menu):
                clic(*menu)
            else:
                # Fuera de la pantalla: el menú del botón derecho del primer campo de texto, también un emergente.
                ir_a(x + gx + 100, y + gy + 74)
                mandar("mouse_button 2", "mouse_button 0", pausa=0.08)
            if esperar_n(r"\[compositor\] emergente de caja-linux", n_em + 1, 60):
                time.sleep(8.0)
                capturar(prefijo + ".gtk-menu")
                apuntar("gtk4-widget-factory: " + ("su menú" if libre(*menu) else "el menú de un campo de texto") + " (emergente), abierto")
            mandar("sendkey esc")
            time.sleep(3.0)
            # La lista de una caja combinada (la de "Andrea"): otro emergente. Solo con la ventana de Full HD (1666 de
            # ancho): con otro tamaño, su sitio cambia.
            if gw >= 1600:
                n_em = emergentes()
                clic(x + gx + 621, y + gy + 250)
                if esperar_n(r"\[compositor\] emergente de caja-linux", n_em + 1, 60):
                    time.sleep(8.0)
                    capturar(prefijo + ".gtk-combo")
                    apuntar("gtk4-widget-factory: la caja combinada, abierta")
                mandar("sendkey esc")
                time.sleep(3.0)
            # Agrandarla por su borde izquierdo (xdg_toplevel.resize desde su sombra, que es de GTK) y después moverla
            # por su barra (xdg_toplevel.move; GTK lo pide tras unos píxeles de arrastre): los cuadros de cada cosa.
            # (Con su borde izquierdo fuera de la pantalla, como con 800 x 600, solo se mueve.)
            antes = len(re.findall(r"\[compositor\] arrastre: ", texto()))
            borde = x + gx - 4 >= 0
            if borde:
                arrastrar((x + gx - 4, y + gy + 300), 20, (-3, 0))
                esperar(r"(\[compositor\] arrastre: (.|\n)*){%d}" % (antes + 1), 30)
                time.sleep(5.0)
            arrastrar((barra[0], barra[1] + 4), 30, (3, 2))
            movidas[len(ventanas_linux()) - 1] = (90, 60)
            if esperar(r"(\[compositor\] arrastre: (.|\n)*){%d}" % (antes + 1 + borde), 30):
                m = re.findall(r"\[compositor\] arrastre: ([^\n]*)", texto())
                apuntar(f"gtk4-widget-factory: redimensionar, {m[antes]}; mover, {m[-1]}" if borde else f"gtk4-widget-factory: mover, {m[-1]}")
            time.sleep(8.0)
            capturar(prefijo + ".gtk-grande")
            # Hostil: con GTK abierta, `prueba-gtk-guion` lanza un programa que se come toda la memoria del invitado.
            # Linux lo mata; la ventana de GTK y DashOS siguen (el compositor dice los cuadros cada 10 s; otra captura).
            def en_pantalla():
                n = re.findall(r"\[compositor\] Wayland: (\d+) ventanas en pantalla", texto())
                return int(n[-1]) if n else -1
            antes = en_pantalla()
            if esperar(r"hostil memoria: tail acab", 900):
                time.sleep(2.0)
                m = re.search(r"hostil memoria: tail acabó con (\d+) a los ([\d.]+) s", texto())
                apuntar(f"hostil memoria: tail muerto ({m.group(1)}) a los {m.group(2)} s" if m else "hostil memoria: tail acabó")
                if esperar(r"gtk: sigue viva tras el hostil", 120):
                    apuntar("hostil memoria: el invitado sigue")
                time.sleep(5.0)
                capturar(prefijo + ".gtk-tras-hostil")
                apuntar(f"hostil memoria: ventanas en pantalla, antes {antes} y después {en_pantalla()}")
            else:
                apuntar("hostil memoria: tail no acabó en 15 minutos")
        esperar(r"gtk: hecho", 900)
        if depurar:
            a_la_terminal()
            teclear("tail -n 60 /tmp/gtk.log")
            time.sleep(20.0)
        time.sleep(3.0)
    if not solo:
        # 4. Hostiles de Linux: decoraciones y tipos rotos; una fuente que escribe sin fin y otra que nunca cierra.
        a_la_terminal()
        paso("puente --cliente-hostil /run/wayland-0 roto", r"cliente hostil: roto: hecho", 120)
        # Su ventana sale sin el teclado (se acaba de teclear): un clic se lo da, y con él pone la selección.
        n = len(ventanas_linux())
        teclear("puente --cliente-hostil /run/wayland-0 sin-fin 150 &")
        h = esperar_ventana(n, 120)
        if h:
            clic(h[0] + 20, h[1] + 20)
        if esperar(r"cliente hostil: sin-fin: selección puesta", 60):
            time.sleep(1.0)
            clic_en_ventana()
            mandar("sendkey ctrl-shift-v")
            # Que el programa hostil vea su tubería cerrada no siempre llega en el plazo (ver la bitácora): se apunta.
            apuntar("hostil sin fin: " + ("le cortaron la tubería" if esperar(r"cliente hostil: sin-fin: le cortaron la tubería", 60) else "sin ver el corte en 60 s"))
            esperar(r"terminal: pegado \(4096 bytes\)", 30)
            time.sleep(3.0)
            mandar("sendkey ctrl-c")
            time.sleep(1.0)
        n = len(ventanas_linux())
        teclear("puente --cliente-hostil /run/wayland-0 nunca 90 &")
        h = esperar_ventana(n, 120)
        if h:
            clic(h[0] + 20, h[1] + 20)
        if esperar(r"cliente hostil: nunca: selección puesta", 60):
            time.sleep(1.0)
            clic_en_ventana()
            mandar("sendkey ctrl-shift-v")
            esperar(r"terminal: pegado \(0 bytes\)", 40)
        # Después, todo sigue: otra ventana de Linux sale y se cierra.
        a_la_terminal()
        n = len(ventanas_linux())
        teclear("ventana_wayland &")
        v = esperar_ventana(n, 120)
        if v:
            time.sleep(3.0)
            clic(v[0] + 100, v[1] + 100)
            mandar("sendkey q")
            esperar(r"(ventana_wayland: .* adiós(.|\n)*){2}", 30)
    # 5. Firefox: su primera ventana con una página del disco, lo que gasta de memoria y, si hay red, una página de
    # verdad. Desde caja-linux-grandes (512 MB y el zram del invitado) sale en unos segundos; CAJA_FIREFOX=0 lo salta.
    if (os.environ.get("CAJA_FIREFOX", "1") != "0" and solo != "gtk") or solo == "firefox":
        # `prueba-firefox-guion` abre hola.html, dice su memoria a los 150 s, y otra vez con https://example.org. Antes
        # de esos 150 s, el guion pulsa en su barra de direcciones y teclea otra (firefox-entrada).
        a_la_terminal()
        n = len(ventanas_linux())
        # CAJA_FIREFOX_DEPURAR=1: con WAYLAND_DEBUG (en /tmp/ff.log del invitado); al acabar, lo de la entrada y las
        # subsuperficies sale por la terminal.
        depurar_ff = os.environ.get("CAJA_FIREFOX_DEPURAR") == "1"
        teclear("WAYLAND_DEBUG=1 prueba-firefox-guion 2>/tmp/ff.log &" if depurar_ff else "prueba-firefox-guion &")
        if esperar(r"firefox: empieza", 120):
            t0 = time.time()
            f = esperar_ventana(n, 600)
            # `prueba-firefox-guion` cierra la primera a los 150 s y abre la segunda (example.org): si al salir una
            # ventana ya van dos "empieza", la primera no salió a tiempo y esta es la segunda.
            tarde = bool(f) and len(re.findall(r"firefox: empieza", texto())) > 1
            if tarde:
                fallo("firefox: la primera ventana no salió en los 150 s de prueba-firefox-guion (la máquina, muy lenta)")
                f = None
            if f:
                apuntar(f"firefox: primera ventana a los {time.time() - t0:.1f} s (desde que empieza, en el reloj del PC)")
                esperar(r"título: Linux · Firefox en DashOS", 60)
                time.sleep(20.0)
                fx, fy = f[0] + (f[2][0] if f[2] else 0), f[1] + (f[2][1] if f[2] else 0)
                # Un clic en su barra de direcciones (la segunda fila, bajo las pestañas: Firefox se pinta su barra),
                # una dirección del disco tecleada e Intro. Si le llegan el ratón y el teclado, su título cambia (lo
                # dice la barra de DashOS).
                # Antes de teclear, la señal de que Firefox tiene el teclado (la barra lo nombra); si otra ventana se lo
                # lleva por medio, se repite (`teclear_en`). El segundo tras el clic no es para Firefox (lo tecleado le
                # llega en orden tras el clic): con más, una ventana nueva podría llevarse el teclado (S8).
                def pulsar():
                    clic(fx + 400, fy + 64)
                    time.sleep(1.0)

                def teclas():
                    mandar("sendkey ctrl-a")
                    teclear("file:///usr/local/share/dashos/", terminal=False)
                if teclear_en(r"Mozilla Firefox$", pulsar, teclas, r"título: Linux · Index of file:///usr/local/share/dashos/", 60, "firefox"):
                    apuntar(f"firefox: clic en su barra de direcciones y dirección tecleada, bien (a los {time.time() - t0:.1f} s)")
                else:
                    fallo("firefox: no hizo caso al clic ni a lo tecleado (título: " + (titulos() or ["?"])[-1] + ")")
                time.sleep(5.0)
                capturar(prefijo + ".firefox")
                if esperar(r"memoria: RAM", 240):
                    m = re.findall(r"linux: (memoria: RAM[^\n]*)", texto())
                    apuntar("firefox con hola.html: " + (m[-1] if m else "?"))
                if esperar_n(r"firefox: empieza", 2, 120):
                    t1 = time.time()
                    f2 = esperar_ventana(n + 1, 600)
                    if f2:
                        apuntar(f"firefox (otra vez, con example.org): primera ventana a los {time.time() - t1:.1f} s")
                        time.sleep(90.0)
                        capturar(prefijo + ".firefox-red")
                    if esperar_n(r"memoria: RAM", 2, 300):
                        m = re.findall(r"linux: (memoria: RAM[^\n]*)", texto())
                        apuntar("firefox con example.org: " + m[-1])
            elif not tarde:
                fallo("firefox: sin ventana en 10 minutos")
            esperar(r"firefox: hecho", 600)
            time.sleep(3.0)
            if depurar_ff:
                a_la_terminal()
                # Sin el movimiento del ratón (lo llena todo): los botones, el teclado y el foco, de la primera ventana.
                teclear("clear; grep -c . /tmp/ff.log; grep -v -e motion -e frame -e axis /tmp/ff.log > /tmp/ff2; "
                        "grep -e button -e wl_keyboard -e enter -e leave -e activated -e input_region /tmp/ff2 > /tmp/ff3; "
                        "grep -c . /tmp/ff3; head -n 260 /tmp/ff3")
                time.sleep(45.0)
    time.sleep(2.0)
    a_la_terminal()
    teclear("poweroff")
    sys.exit(0)

def arriba(dx, dy):
    """Un clic en la última ventana de Linux, en (dx, dy) de su esquina: la sube encima de las demás (la de la caja
    Amiga puede salir encima en cualquier momento)."""
    v = re.findall(r"\[compositor\] ventana de caja-linux \(un programa de su puente de Wayland\) en \((\d+), (\d+)\)", texto())
    if v:
        clic(int(v[-1][0]) + dx, int(v[-1][1]) + dy)
        time.sleep(1.0)


if modo == "aspecto":
    # S26 (`make arranque-virt-aspecto`): el aspecto de los programas de Linux. Espera a que el agente del aspecto
    # (dashos-aspecto, un servicio de systemd que escucha el tema de DashOS por el puente) haya escrito el tema, abre
    # una muestra de GTK 3 y otra de GTK 4 (los controles de siempre) y foot, y captura la pantalla con todo.
    if not esperar(r"login: root \(automatic login\)", total):
        sys.exit(1)
    time.sleep(5.0)
    clic_en_ventana()
    paso("systemctl is-active dashos-aspecto", r"linux: active\s*$|linux: active\n", 120)
    esperar(r"linux: aspecto: programas de Linux", 120)
    # Cada programa, solo y encima de la terminal (una ventana nueva sale arriba), con su captura; se cierra solo. GTK 3
    # aún no: en la caja aborta al cargar su primer icono (glycin sin bwrap; ver la bitácora de S26). Se limpia la
    # terminal y lo que dicen los programas va solo a hvc0.
    for n, nombre in ((4, "gtk4"),):
        antes = len(re.findall(r"muestra-gtk: adiós", texto()))
        clic_en_ventana()
        teclear("clear")
        time.sleep(1.0)
        teclear(f"prueba-aspecto muestra-gtk{n} 30 &")
        if esperar(rf"muestra-gtk: GTK {n} en pantalla", 400):
            time.sleep(4.0)
            arriba(420, 20)
            capturar(prefijo + "." + nombre)
        esperar_n(r"muestra-gtk: adiós|Aborted", antes + 1, 120)
        time.sleep(2.0)
    clic_en_ventana()
    teclear("clear")
    time.sleep(1.0)
    teclear("prueba-aspecto prueba-foot &")
    if esperar(r"foot: lista", 400):
        time.sleep(4.0)
        arriba(300, 60)
        capturar(prefijo + ".foot")
    clic_en_ventana()
    teclear("cat /root/.config/glib-2.0/settings/keyfile /run/dashos-aspecto/linux")
    time.sleep(3.0)
    clic_en_ventana()
    teclear("poweroff")
    sys.exit(0)

if modo == "gpu":
    if not esperar(r"login: root \(automatic login\)", total):
        sys.exit(1)
    time.sleep(5.0)
    clic_en_ventana()
    paso("systemctl is-active dashos-puente", r"linux: active\s*$|linux: active\n", 120)
    paso("ls /dev/dri", r"linux: .*card0", 60)
    # Hoy: GL por Wayland, con los búferes de wl_shm por el puente.
    teclear("cuadros_gl wayland 20")
    if esperar(r"cuadros_gl: wayland: primer cuadro", 300):
        time.sleep(8.0)
        capturar(prefijo + ".wayland")
    esperar(r"cuadros_gl: wayland: al acabar", 120)
    time.sleep(2.0)
    # Lo mismo sin esperar al compositor entre cuadros: lo que cuestan pintar y copiar.
    clic_en_ventana()
    teclear("cuadros_gl wayland 15 0")
    esperar(r"cuadros_gl: wayland: al acabar.*\n(.|\n)*cuadros_gl: wayland: al acabar", 200)
    time.sleep(2.0)
    clic_en_ventana()
    teclear("timeout 20 es2gears_wayland")
    if esperar(r"linux: \d+ frames in", 300):
        time.sleep(3.0)
        capturar(prefijo + ".gears")
    time.sleep(15.0)
    # Vulkan por software (lavapipe) por Wayland: vkcube no dice sus cuadros; 100 cuadros con `time`.
    clic_en_ventana()
    teclear("time vkcube --wsi wayland --c 100")
    if esperar(r"linux: real\s+\d+m[\d.]+s", 300):
        time.sleep(1.0)
    # Por la virtio-gpu: la pantalla del invitado en una ventana de DashOS.
    clic_en_ventana()
    teclear("cuadros_gl kms 20")
    if esperar(r"cuadros_gl: kms: primer cuadro", 300):
        time.sleep(8.0)
        capturar(prefijo + ".kms")
    esperar(r"cuadros_gl: kms: al acabar", 120)
    time.sleep(3.0)
    # La terminal sigue viva (comparte conexión con la ventana de la pantalla). Con su clic, como los demás pasos: al
    # quitarse la ventana de la pantalla de Linux, el teclado pasa a la ventana que quede arriba, sea la que sea.
    clic_en_ventana()
    paso("echo terminal viva", r"linux: terminal viva", 60)
    clic_en_ventana()
    teclear("poweroff")
    sys.exit(0)

if modo == "arch":
    # systemd tarda: hasta que getty entra como root, lo que dé el plazo entero. El prompt no acaba en salto de línea:
    # al registro sale con la orden que se teclee.
    if not esperar(r"login: root \(automatic login\)", total):
        sys.exit(1)
    time.sleep(5.0)
    clic_en_ventana()
    paso("uname -a", r"linux: Linux caja \S+-caja ", 60)
    paso("systemctl is-system-running --wait", r"linux: (running|degraded)\s*$|linux: (running|degraded)\n", 300)
    paso("ip -4 addr show eth0", r"linux: +inet 10\.0\.2\.\d+/24", 120)
    paso("date", r"linux: \w{3} \w{3} +\d+ [\d:]+ UTC 20\d\d", 60)
    # pacman: baja core, extra, alarm y aur (unos 12 MB por la red de QEMU). Con su barra de progreso; no se repite.
    teclear("pacman -Sy --noconfirm")
    time.sleep(5.0)
    paso("ls /var/lib/pacman/sync", r"linux: .*extra\.db", 900)
    time.sleep(1.5)
    capturar(prefijo)
    # S29 (docs/diseño/S29-malware-linux-y-dashos.md): el malware de laboratorio de la caja, con órdenes de Arch: lo que
    # haría un programa malo de Linux con la red de casa. El nombre de un servidor se resuelve (el DNS pasa); un puerto
    # del PC (10.0.2.2 en QEMU es el propio PC), otro aparato de la red local, buscar aparatos por mDNS y hacerse pasar
    # por la puerta de enlace por ARP (con su dirección puesta un momento), no: el cortafuegos de la caja, en el
    # servicio de red de DashOS, lo corta y lo dice. Antes, pacman ya bajó de Internet. Al final, para no estropear la
    # red de lo de antes.
    paso("getent hosts archlinuxarm.org", r"linux: \S+\s+\S*archlinuxarm\.org", 60)
    paso("curl -sS -m 6 --connect-timeout 5 telnet://10.0.2.2:9", r"linux: curl: \(\d+\)", 30)
    paso("curl -sS -m 6 --connect-timeout 5 http://192.168.255.254", r"linux: curl: \(\d+\)", 30)
    # S29: correo al puerto 25 de un servidor de Internet (correo basura): cortado aunque sea fuera de casa.
    paso("curl -sS -m 6 --connect-timeout 5 telnet://1.1.1.1:25", r"linux: curl: \(\d+\)", 30)
    paso("ping -c 1 -W 2 224.0.0.251", r"linux: 1 packets transmitted", 30)
    teclear("ip addr add 10.0.2.2/32 dev eth0")
    time.sleep(1.0)
    paso("arping -c 2 -U -I eth0 10.0.2.2", r"linux: Sent 2 probes|arping: ", 30)
    teclear("ip addr del 10.0.2.2/32 dev eth0")
    time.sleep(1.0)
    teclear("poweroff")
    sys.exit(0)

if not esperar(r"caja-linux: terminal lista", total):
    sys.exit(1)
time.sleep(1.0)
v = re.search(r"\[compositor\] ventana de caja-linux en \((\d+), (\d+)\)", texto())
if v:
    ir_a(int(v.group(1)) + 200, int(v.group(2)) + 150)
    mandar("mouse_button 1", "mouse_button 0", pausa=0.08)
    time.sleep(0.5)
paso("uname -a", r"linux: Linux caja \S+")
paso("echo hola desde DashOS", r"linux: hola desde DashOS\s*$|linux: hola desde DashOS\n")
time.sleep(1.5)
mandar("screendump " + prefijo + ".ppm")
time.sleep(1.5)
a_png(prefijo + ".ppm", prefijo + ".png")
# La inundación: no se repite. Lo que se teclea mientras sale va a la cola de la consola y la shell lo lee al acabar.
# El registro calla mientras dura y luego dice cuántas líneas se calló: entonces, la orden siguiente.
teclear("seq 1 30000")
esperar(r"líneas sin sacar al registro", 120)
paso("echo sigue viva", r"linux: sigue viva", 30)
if "eth0" in texto():
    paso("ping -c 2 10.0.2.2", r"linux: 2 packets transmitted", 30)
# aviso-cajas: antes de apagar, que el globo haya ido y vuelto (el aviso de memoria baja que provoca `presion`): el
# invitado da memoria a DashOS y, cuando vuelve a haber, la recupera.
esperar(r"\[caja-linux\] globo: vacío", 180)
time.sleep(1.0)
mandar("screendump " + prefijo + ".fin.ppm")
time.sleep(1.0)
teclear("poweroff -f")
