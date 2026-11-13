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
              "_": "shift-slash", "&": "shift-6", ";": "shift-comma"}


def teclear(orden):
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
    mandar(*["mouse_move -100 -100"] * 12, pausa=0.03)
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
    v = re.findall(r"\[compositor\] ventana de caja-linux \(un programa de su puente de Wayland\) en \((\d+), (\d+)\)", texto())
    return (int(v[n - 1][0]), int(v[n - 1][1])) if len(v) >= n else None


def clic(x, y):
    ir_a(x, y)
    mandar("mouse_button 1", "mouse_button 0", pausa=0.08)
    time.sleep(0.5)


def apuntar(texto_medida):
    """Una medida o un resultado del guion, en PREFIJO.medidas.txt (la salida del guion no va a ningún sitio)."""
    with open(prefijo + ".medidas.txt", "a") as f:
        f.write(texto_medida + "\n")


def teclear_sin_intro(orden):
    teclas = []
    for c in orden:
        if c in ESPECIALES:
            teclas.append("sendkey " + ESPECIALES[c])
        elif c.isupper():
            teclas.append("sendkey shift-" + c.lower())
        else:
            teclas.append("sendkey " + c)
    mandar(*teclas, pausa=0.12)


VENTANA_LINUX = r"\[compositor\] ventana de caja-linux \(un programa de su puente de Wayland\) en \((\d+), (\d+)\)(?: geometría \((-?\d+), (-?\d+), (\d+), (\d+)\))?(?: de (\d+)x(\d+))?"
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
            g = tuple(int(n) for n in w[2:]) if w[2] else None
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


if modo == "wayland":
    if not esperar(r"login: root \(automatic login\)", total):
        sys.exit(1)
    time.sleep(5.0)
    clic_en_ventana()
    paso("systemctl is-active dashos-puente", r"linux: active\s*$|linux: active\n", 120)
    paso("puente --hostil vsock:5000", r"puente hostil: hecho", 120)
    # 1. Un programa de Linux sin barra propia: lleva la de DashOS (xdg-decoration).
    n = len(ventanas_linux())
    teclear("ventana_wayland &")
    if not esperar(r"ventana_wayland: primer cuadro en pantalla", 180):
        sys.exit(1)
    time.sleep(1.0)
    v = esperar_ventana(n, 10)
    if v:
        x, y, _ = v
        clic(x + 160, y + 120)
        esperar(r"ventana_wayland: clic del botón", 30)
        mandar("sendkey a")
        esperar(r"ventana_wayland: tecla 30 pulsada", 30)
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
    # 3. Hostiles de Linux: decoraciones y tipos rotos; una fuente que escribe sin fin y otra que nunca cierra.
    a_la_terminal()
    paso("puente --cliente-hostil /run/wayland-0 roto", r"cliente hostil: roto: hecho", 120)
    # Su ventana sale sin el teclado (se acaba de teclear): un clic se lo da, y con él pone la selección.
    n = len(ventanas_linux())
    teclear("puente --cliente-hostil /run/wayland-0 sin-fin 60 &")
    h = esperar_ventana(n, 120)
    if h:
        clic(h[0] + 20, h[1] + 20)
    if esperar(r"cliente hostil: sin-fin: selección puesta", 60):
        time.sleep(1.0)
        clic_en_ventana()
        mandar("sendkey ctrl-shift-v")
        esperar(r"cliente hostil: sin-fin: le cortaron la tubería", 60)
        esperar(r"terminal: pegado \(4096 bytes\)", 30)
        time.sleep(3.0)
        mandar("sendkey ctrl-c")
        time.sleep(1.0)
    n = len(ventanas_linux())
    teclear("puente --cliente-hostil /run/wayland-0 nunca 60 &")
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
    # 4. Un programa grande: GTK4 (gtk4-widget-factory), dibujado por la CPU.
    a_la_terminal()
    n = len(ventanas_linux())
    teclear("prueba-gtk &")
    if esperar(r"gtk: empieza", 120):
        t0 = time.time()
        g = esperar_ventana(n, 900)
        if g:
            apuntar(f"gtk4-widget-factory: primera ventana a los {time.time() - t0:.1f} s (desde que empieza, en el reloj del PC)")
            # Para el registro (y las líneas esperadas): el guion la vio salir.
            clic_en_ventana()
            teclear("echo guion: gtk4 en pantalla")
            x, y, geo = g
            gx, gy, gw, gh = geo if geo else (0, 0, 600, 400)
            # Arriba otra vez (un clic en su barra, sin moverla).
            time.sleep(15.0)
            clic(x + gx + 200, y + gy + 12)
            time.sleep(5.0)
            capturar(prefijo + ".gtk")
            # La lista de la primera caja combinada (la flecha, a la derecha del primer campo): un emergente.
            clic(x + gx + 370, y + gy + 73)
            if esperar(r"\[compositor\] emergente de caja-linux", 120):
                time.sleep(8.0)
                capturar(prefijo + ".gtk-menu")
            mandar("sendkey esc")
            time.sleep(3.0)
            # Moverla por su barra (xdg_toplevel.move): los cuadros mientras se arrastra.
            antes = len(re.findall(r"\[compositor\] arrastre: ", texto()))
            arrastrar((x + gx + 200, y + gy + 20), 30, (3, 2))
            movidas[len(ventanas_linux()) - 1] = (90, 60)
            esperar(r"(\[compositor\] arrastre: (.|\n)*){%d}" % (antes + 1), 30)
            time.sleep(2.0)
            # Agrandarla por su borde izquierdo (resize; es más grande que la pantalla, la derecha no se ve).
            x2, y2 = x + 90, y + 60
            arrastrar((x2 + gx - 3, y2 + gy + 300), 20, (-3, 0))
            esperar(r"(\[compositor\] arrastre: (.|\n)*){%d}" % (antes + 2), 30)
            time.sleep(8.0)
            capturar(prefijo + ".gtk-grande")
        a_la_terminal()
        teclear("pkill -f gtk4-widget-factory")
        time.sleep(3.0)
    # 5. Firefox, si cabe: su primera ventana, una captura y fuera.
    # Solo con CAJA_FIREFOX=1: con 256 MB (y 768 MB de intercambio en el disco) no llega a abrir su ventana en 20
    # minutos, y la caja se para a los 1500 s.
    if os.environ.get("CAJA_FIREFOX") == "1":
        a_la_terminal()
        n = len(ventanas_linux())
        teclear("prueba-firefox &")
        if esperar(r"firefox: empieza", 120):
            t0 = time.time()
            f = esperar_ventana(n, 1200)
            if f:
                apuntar(f"firefox: primera ventana a los {time.time() - t0:.1f} s (desde que empieza, en el reloj del PC)")
                clic_en_ventana()
                teclear("echo guion: firefox en pantalla")
                time.sleep(45.0)
                clic(f[0] + (f[2][0] if f[2] else 0) + 200, f[1] + (f[2][1] if f[2] else 0) + 10)
                time.sleep(15.0)
                capturar(prefijo + ".firefox")
            else:
                apuntar("firefox: sin ventana en 20 minutos")
            clic_en_ventana()
            teclear("pkill firefox")
            time.sleep(5.0)
    time.sleep(2.0)
    a_la_terminal()
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
    # La terminal sigue viva (comparte conexión con la ventana de la pantalla).
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
time.sleep(1.0)
mandar("screendump " + prefijo + ".fin.ppm")
time.sleep(1.0)
teclear("poweroff -f")
