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
              "_": "shift-slash", "&": "shift-6"}


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
    # y pueden tapar el centro; un clic en otra le daría el teclado.
    v = re.search(r"\[compositor\] ventana de caja-linux en \((\d+), (\d+)\)", texto())
    if v:
        ir_a(int(v.group(1)) + 12, int(v.group(2)) + 380)
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


if modo == "wayland":
    if not esperar(r"login: root \(automatic login\)", total):
        sys.exit(1)
    time.sleep(5.0)
    clic_en_ventana()
    paso("systemctl is-active dashos-puente", r"linux: active\s*$|linux: active\n", 120)
    paso("puente --hostil vsock:5000", r"puente hostil: hecho", 120)
    # El programa de Linux: su ventana sale en el escritorio de DashOS.
    teclear("ventana_wayland &")
    if not esperar(r"ventana_wayland: primer cuadro en pantalla", 180):
        sys.exit(1)
    time.sleep(1.0)
    v = ventana_linux(1)
    if v:
        clic(v[0] + 160, v[1] + 120)
        esperar(r"ventana_wayland: clic del botón", 30)
        mandar("sendkey a")
        esperar(r"ventana_wayland: tecla 30 pulsada", 30)
    esperar(r"ventana_wayland: en marcha", 60)
    time.sleep(1.0)
    capturar(prefijo)
    mandar("sendkey q")
    esperar(r"ventana_wayland: .* adiós", 30)
    # Medidas: sin copia y con copia (el puente copia a los programas que conectan con el fichero puesto), de 320 x 240
    # y de 1280 x 720, 8 s cada una.
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
    # foot, una terminal de Wayland de verdad: la primera vez hace la caché de letras de fontconfig.
    clic_en_ventana()
    teclear("prueba-foot &")
    if esperar(r"foot: lista", 600):
        time.sleep(3.0)
        v = ventana_linux(2)
        if v:
            clic(v[0] + 100, v[1] + 100)
            teclear("hola")
            esperar(r"foot: leido hola", 60)
            capturar(prefijo + ".foot")
    time.sleep(4.0)
    clic_en_ventana()
    teclear("poweroff")
    sys.exit(0)

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
        teclear(f"prueba-aspecto muestra-gtk{n} 30 >/dev/null 2>&1 &")
        if esperar(rf"muestra-gtk: GTK {n} en pantalla", 400):
            time.sleep(4.0)
            capturar(prefijo + "." + nombre)
        esperar_n(r"muestra-gtk: adiós|Aborted", antes + 1, 120)
        time.sleep(2.0)
    clic_en_ventana()
    teclear("clear")
    time.sleep(1.0)
    teclear("prueba-foot >/dev/null 2>&1 &")
    if esperar(r"foot: lista", 400):
        time.sleep(4.0)
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
