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
"q". Después abre foot (una terminal de Wayland de verdad) con `prueba-foot`, teclea "hola" en su ventana (la shell
de dentro lo escribe en hvc0), la captura (PREFIJO.foot.png) y apaga.

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
    for _ in range(3):
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
