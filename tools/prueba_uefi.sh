#!/usr/bin/env bash
# DashOS: arranca un programa de UEFI en QEMU `virt` con el firmware EDK2 y comprueba la salida del puerto serie.
#
#   tools/prueba_uefi.sh <programa.efi> <fichero de esperados> [segundos]
#
# El programa va como EFI/BOOT/BOOTAA64.EFI en un disco FAT (el directorio que QEMU presenta como disco), que EDK2
# arranca solo. Mismo formato de esperados que prueba_qemu.sh: líneas en orden, "~" en cualquier sitio, "#" ignoradas.
set -u
EFI=$1
ESPERADOS=$2
SEGUNDOS=${3:-30}
FIRMWARE=${EDK2:-/usr/share/edk2/aarch64/QEMU_EFI.fd}
DISCO=$(mktemp -d)
mkdir -p "$DISCO/EFI/BOOT"
cp "$EFI" "$DISCO/EFI/BOOT/BOOTAA64.EFI"
SALIDA=$(mktemp)
# GIC=2 por ahora (el kernel maneja el GICv2 desde U3); con GIC=3, el de los portátiles con Snapdragon y las placas de
# servidor (los Mac usan el AIC de Apple). acpi=off: así EDK2 da el árbol de dispositivos (como U-Boot en los Mac).
# Los portátiles dan ACPI: queda para después.
MONITOR=$SALIDA.mon
timeout "$SEGUNDOS" qemu-system-aarch64 -monitor unix:"$MONITOR",server,nowait -M virt,gic-version=${GIC:-2},acpi=${ACPI:-off}${MAQUINA:+,$MAQUINA} -cpu cortex-a72 -smp 4 -m ${MEMORIA:-2G} \
    -bios "$FIRMWARE" -drive file=fat:rw:"$DISCO",format=raw,if=virtio \
    -display none -serial file:"$SALIDA" -net none ${QEMU_EXTRA:-} >/dev/null 2>&1 &
QEMU=$!
# Con TECLAS=1, una "a" y un movimiento del ratón (10, 5) cada medio segundo desde el segundo 3 (U6: el xHCI). Desde
# el paso 15 también un clic (W8): el cursor del compositor, que empieza en (0, 0), ya está en (150, 75), sobre la
# ventana de prueba de arriba, que se lleva el teclado sin cambiar el orden de las ventanas.
# Ajustes sale cuando ya se está tecleando en otra ventana, así que no se lleva el teclado al salir (S8,
# docs/REVISION_LOOP.md). Cuando la salida serie dice que está en pantalla, el cursor va a la esquina y de ahí a
# (400, 130), donde solo está ella (junto a su título); un clic le da el teclado, el cursor sigue en (300, 300), y
# Super+Arriba la maximiza y Super+Abajo la devuelve.
# Cuando `guardian` pregunta por un permiso (U4), se contesta: a `pide`, sí; a otro, una "s" antes de tiempo, que no
# debe contar, y después no.
# Cuando `guardian` pone la pantalla de bloqueo (U3), se dejan de mandar esas teclas y se teclea una contraseña mala
# y la buena, con Intro tras cada una; al desbloquear, sigue lo de antes. Igual con la pantalla de entrada y, tras
# ella, con el bloqueo por tiempo: 3 s después de que `guardian` pida el aviso, nada hasta que bloquea.
# U6, el cambio rápido de usuario: tras entrar Ana, en las dos pantallas de bloqueo siguientes, Esc y otra persona
# (Carla, y luego otra vez Ana) con su contraseña. Con CAPTURA_CAMBIO=fichero.ppm, la primera de esas dos pantallas
# (fichero.ppm) y la de "Cambiar de usuario" con el nombre ya puesto (fichero.nombre.ppm).
# U6, paso 2: con CAPTURA_U6=prefijo, la pantalla con la ventana de `cambio` de Ana delante (prefijo.ana.ppm) y la de la
# sesión de Carla, con Ana detrás (prefijo.carla.ppm): ahí no tiene que estar (lo mira `tools/captura_cambio.py`).
if [ -n "${TECLAS:-}" ]; then
    python3 -I - "$MONITOR" "$SEGUNDOS" "$SALIDA" "${CAPTURA_PREGUNTA:-}" "${DIAG_REGISTROS:-}" "${CAPTURA_SELECTOR:-}" "${CAPTURA_CENTRO:-}" <<'PY' >/dev/null 2>&1 &
import os, socket, sys, time
ruta, total, serie, captura_pregunta, diag_registros, captura_selector, captura_centro = sys.argv[1], float(sys.argv[2]), sys.argv[3], sys.argv[4], sys.argv[5], sys.argv[6], sys.argv[7]
diag_hecho = False

def preguntar_monitor(orden):
    s = socket.socket(socket.AF_UNIX)
    s.connect(ruta)
    s.settimeout(1.0)
    s.sendall(orden.encode() + b"\n")
    datos = b""
    try:
        while True:
            t = s.recv(65536)
            if not t:
                break
            datos += t
    except OSError:
        pass
    s.close()
    return datos.decode(errors="replace")
inicio = time.time()
time.sleep(3)
paso = 0
ajustes = False
bloqueo = 0

def teclear(texto):
    ordenes = []
    for ch in texto:
        if ch == " ":
            ordenes.append("sendkey spc")
        elif ch.isupper():
            ordenes.append("sendkey shift-" + ch.lower())
        else:
            ordenes.append("sendkey " + ch)
    ordenes.append("sendkey ret")
    mandar(*ordenes, pausa=0.15)

def mandar(*ordenes, pausa=0.0):
    s = socket.socket(socket.AF_UNIX)
    s.connect(ruta)
    for o in ordenes:
        s.sendall(o.encode() + b"\n")
        time.sleep(pausa)
    s.close()

def ir_a(x, y):
    # El cursor a (x, y) de la pantalla: primero a la esquina, y de ahí en pasos de 100 como mucho.
    mandar(*["mouse_move -100 -100"] * 12, pausa=0.05)
    pasos = []
    while x > 0 or y > 0:
        dx, dy = min(x, 100), min(y, 100)
        pasos.append(f"mouse_move {dx} {dy}")
        x, y = x - dx, y - dy
    mandar(*pasos, pausa=0.05)

menu = 0
preguntas = 0
selector = 0
entradas_antes = 0
menu_intentos = 0
menu_desde = 0
menus_antes = 0
espera_carpeta = 0
centro = 0
panel = 0
barra = 0
avisos_paso = 0
reintento_avisos = 0
wb = 0
wb_ventana = {}
bloqueo_capturado = False
cambio = 0
captura_u6_hecha = 0
amiga = 0
amiga_desde = 0
v_amiga = None
dash_amiga = 0
gt_amiga = 0
gt_dash = 0
gt_mirado = 0
gt_movida = 0
req_dash = 0

while time.time() - inicio < total:
    try:
        texto = open(serie, errors="replace").read()
        # Con ESTADO_GUION=fichero: el estado del guion cada 2 s (para ver en qué paso se queda).
        if os.environ.get("ESTADO_GUION") and time.time() - globals().get("estado_dicho", 0) > 2:
            estado_dicho = time.time()
            with open(os.environ["ESTADO_GUION"], "a") as f:
                f.write(f"{time.time() - inicio:.0f} s: paso {paso} ajustes {ajustes} menu {menu} selector {selector} preguntas {preguntas} centro {centro} bloqueo {bloqueo} cambio {cambio} panel {panel} barra {barra} avisos {avisos_paso} caja {amiga}/{dash_amiga}/{gt_amiga}/{gt_dash}/{req_dash}\n")
        # Los pasos de las capturas de la caja Amiga (GadTools, el ejemplo de Dash y el requester), al principio de la
        # vuelta: los pasos de más abajo esperan con "continue" y no se llegaría a ellos.
        libre_caja = texto.count("[guardian] pregunta en pantalla: ") <= texto.count("El usuario dice ")
        # La ventana de GadTools (`caja/pruebas68k/gadtools.c`), tras la de Dash: cuando se ve entera (una captura
        # comprueba que sus cuatro esquinas y un sitio vacío de dentro son de los colores del Workbench: si otra ventana
        # del arranque la tapa, los clics irían a esa; se mira otra vez en la vuelta siguiente), la captura
        # (CAPTURA_GT), clics en la casilla, el ciclo, la tercera opción del mx, el tercer elemento de la lista y
        # "Aceptar", y el menú con el botón derecho: sobre "Proyecto", abajo hasta "Salir" (la captura con el menú
        # abierto, CAPTURA_GT_MENU) y soltar, que cierra el programa. Las posiciones, las de gadtools.c.
        if dash_amiga == 1 and gt_amiga == 0 and libre_caja and time.time() > gt_mirado + 2:
            import re
            gt_mirado = time.time()
            corte = texto.find("[caja-amiga] gadtools-90: ")   # (solo con CAJA_INTERACTIVA=1: si no, 5 s)
            if corte >= 0 and "[caja-amiga] gadtools: ventana abierta y dibujada" in texto[corte:] \
                    and "[caja-amiga] gadtools.exe acabó" not in texto[corte:]:
                v_gt = list(re.finditer(r"\[compositor\] ventana de caja-amiga en \((\d+), (\d+)\)", texto[corte:]))
                if v_gt:
                    x0, y0 = int(v_gt[-1].group(1)), int(v_gt[-1].group(2))
                    # Con escala (pantallas grandes), la caja dice "a 2x": las posiciones de gadtools.c, por el factor.
                    m_f = list(re.finditer(r"ventana Amiga en pantalla \(\d+x\d+(?:, a (\d+)x)?\)", texto[corte:]))
                    f_gt = int(m_f[-1].group(1) or 1) if m_f else 1
                    x, y = x0, y0
                    foto = os.environ.get("CAPTURA_GT") or "/tmp/dashos-gt-mira.ppm"
                    mandar("screendump " + foto)
                    time.sleep(0.7)
                    def pixeles(ruta, puntos):
                        try:
                            d = open(ruta, "rb").read()
                            cab = d.split(b"\n", 3)
                            an = int(cab[1].split()[0])
                            datos = cab[3]
                            return [tuple(datos[3 * (py * an + px):3 * (py * an + px) + 3]) for px, py in puntos]
                        except Exception:
                            return []
                    wb = {(0xAA, 0xAA, 0xAA), (0, 0, 0), (0xFF, 0xFF, 0xFF), (0x66, 0x88, 0xBB)}
                    vistos = pixeles(foto, [(x + 1 * f_gt, y + 1 * f_gt), (x + 408 * f_gt, y + 1 * f_gt), (x + 1 * f_gt, y + 190 * f_gt), (x + 408 * f_gt, y + 190 * f_gt)])
                    vacio = pixeles(foto, [(x + 200 * f_gt, y + 183 * f_gt)])
                    ancho_pantalla = 0
                    try:
                        ancho_pantalla = int(open(foto, "rb").read().split(b"\n", 3)[1].split()[0])
                    except Exception:
                        pass
                    clasica = len(vistos) == 4 and all(c in wb for c in vistos) and vacio == [(0xAA, 0xAA, 0xAA)]
                    # Con el aspecto de DashOS (S26): las dos esquinas de arriba, de la barra; las de abajo y el sitio
                    # vacío, de la superficie (otra ventana encima no da eso).
                    # Y la casilla marcada (en (103, 25)), del primario: no es la barra ni la superficie (una ventana
                    # del kit encima tiene la misma superficie).
                    casilla = pixeles(foto, [(x + 103 * f_gt, y + 25 * f_gt)])
                    material = len(vistos) == 4 and len(vacio) == 1 and vistos[0] == vistos[1] != vistos[2] \
                        and vistos[2] == vistos[3] == vacio[0] and len(casilla) == 1 and casilla[0] not in (vistos[0], vacio[0]) \
                        and max(casilla[0]) - min(casilla[0]) > 40
                    tapada = not (clasica or material)
                    if tapada and gt_movida < 3:
                        # Un clic en su borde izquierdo (no es ningún gadget; suele verse: las otras ventanas salen más a
                        # la derecha) la sube encima de las demás; se mira otra vez en la vuelta siguiente.
                        gt_movida += 1
                        ir_a(x + 2 * f_gt, y + 100 * f_gt)
                        time.sleep(0.3)
                        mandar("mouse_button 1", "mouse_button 0", pausa=0.15)
                        time.sleep(0.5)
                        continue
                    if not tapada:
                        gt_amiga = 1
                        for dx, dy in [(103, 25), (140, 43), (98, 82), (300, 54), (290, 168)]:
                            ir_a(x + dx * f_gt, y + dy * f_gt)
                            time.sleep(0.3)
                            mandar("mouse_button 1", "mouse_button 0", pausa=0.15)
                            time.sleep(0.4)
                        ir_a(x + 25 * f_gt, y + 5 * f_gt)
                        time.sleep(0.3)
                        mandar("mouse_button 2", pausa=0.3)
                        mandar(f"mouse_move 0 {15 * f_gt}", f"mouse_move 0 {14 * f_gt}", pausa=0.3)
                        time.sleep(0.5)
                        if os.environ.get("CAPTURA_GT_MENU"):
                            mandar("screendump " + os.environ["CAPTURA_GT_MENU"])
                            time.sleep(0.5)
                        mandar("mouse_button 0", pausa=0.2)
                        w = re.search(r"\[compositor\] ventana de ajustes en \((\d+), (\d+)\)", texto)
                        if menu == 2:
                            ir_a(790, 590)
                        elif ajustes and w:
                            ir_a(int(w.group(1)) + 196, int(w.group(2)) + 196)
                        else:
                            ir_a(10 * paso, 5 * paso)
        # El ejemplo de GadTools de FreeMEMLang (`examples/test_gadtools.dash`): la captura (CAPTURA_GT_DASH) y un clic en
        # su "Cancel" (gt_button(120, 20, 80, 20) en una ventana GIMMEZEROZERO: desde la esquina de dentro, 4 y 11).
        if dash_amiga == 1 and gt_dash == 0 and libre_caja:
            import re
            corte = texto.find("[caja-amiga] test_gadtools: ")
            if corte >= 0:
                v_gd = list(re.finditer(r"\[compositor\] ventana de caja-amiga en \((\d+), (\d+)\)", texto[corte:]))
                if v_gd:
                    gt_dash = 1
                    x, y = int(v_gd[-1].group(1)), int(v_gd[-1].group(2))
                    time.sleep(1.0)
                    if os.environ.get("CAPTURA_GT_DASH"):
                        mandar("screendump " + os.environ["CAPTURA_GT_DASH"])
                        time.sleep(0.5)
                    m_f = list(re.finditer(r"ventana Amiga en pantalla \(\d+x\d+(?:, a (\d+)x)?\)", texto[corte:]))
                    f_gd = int(m_f[-1].group(1) or 1) if m_f else 1
                    ir_a(x + 164 * f_gd, y + 41 * f_gd)
                    time.sleep(0.3)
                    mandar("mouse_button 1", "mouse_button 0", pausa=0.15)
                    w = re.search(r"\[compositor\] ventana de ajustes en \((\d+), (\d+)\)", texto)
                    if menu == 2:
                        ir_a(790, 590)
                    elif ajustes and w:
                        ir_a(int(w.group(1)) + 196, int(w.group(2)) + 196)
                    else:
                        ir_a(10 * paso, 5 * paso)
        # El error de Dash en un requester (solo con la caja hecha con CAJA_INTERACTIVA=1, para las capturas): su
        # captura (CAPTURA_REQ) y un clic en su botón, centrado abajo.
        if req_dash == 0 and os.environ.get("CAPTURA_REQ"):
            import re
            corte = texto.find("[caja-amiga] test_trap_illegal (sin consola)")
            if corte >= 0:
                m_r = re.search(r"ventana Amiga en pantalla \((\d+)x(\d+)(?:, a (\d+)x)?\): Dash", texto[corte:])
                v_r = list(re.finditer(r"\[compositor\] ventana de caja-amiga en \((\d+), (\d+)\)", texto[corte:]))
                if m_r and v_r:
                    req_dash = 1
                    x, y = int(v_r[-1].group(1)), int(v_r[-1].group(2))
                    an, al = int(m_r.group(1)), int(m_r.group(2))
                    f_r = int(m_r.group(3) or 1)
                    time.sleep(1.0)
                    mandar("screendump " + os.environ["CAPTURA_REQ"])
                    time.sleep(0.5)
                    ir_a(x + an // 2 * f_r, y + (al - 12) * f_r)
                    time.sleep(0.3)
                    mandar("mouse_button 1", "mouse_button 0", pausa=0.15)
        # Diagnóstico: con DIAG_REGISTROS=fichero, si el compositor suelta a alguien, los registros de los núcleos (tres
        # veces, para ver si se mueven).
        if diag_registros and not diag_hecho and os.environ.get("DIAG_TEXTO", "se le suelta") in texto:
            diag_hecho = True
            time.sleep(float(os.environ.get("DIAG_ESPERA", "0")))
            with open(diag_registros, "w") as f:
                for _ in range(3):
                    f.write(preguntar_monitor("info registers -a"))
                    f.write("\n=====\n")
                    time.sleep(0.5)
        # Con CAPTURA_BLOQUEO=fichero.ppm, la primera pantalla de bloqueo o de entrada de `guardian`, ya pintada.
        if os.environ.get("CAPTURA_BLOQUEO") and not bloqueo_capturado and "[guardian] pantalla de " in texto:
            bloqueo_capturado = True
            time.sleep(0.5)
            mandar("screendump " + os.environ["CAPTURA_BLOQUEO"])
        # U6, paso 2: las dos capturas de la ventana de `cambio` (no mandan teclas).
        captura_u6 = os.environ.get("CAPTURA_U6")
        if captura_u6 and captura_u6_hecha == 0 and "[cambio] Ana: ventana en pantalla: true" in texto:
            captura_u6_hecha = 1
            time.sleep(0.3)
            mandar("screendump " + captura_u6 + ".ana.ppm")
        # La de Carla, al acabar la espía: `guardian` deja la sesión de Carla 3 s más sin bloquear.
        if captura_u6 and captura_u6_hecha == 1 and "[cambio] Carla delante (" in texto:
            captura_u6_hecha = 2
            time.sleep(0.3)
            mandar("screendump " + captura_u6 + ".carla.ppm")
        # U4: las preguntas de permisos de `guardian`. A `pide`, que sí; a los demás, una "s" enseguida (en el primer
        # segundo no cuenta) y luego que no.
        vistas = [l for l in texto.splitlines() if "[guardian] pregunta en pantalla: " in l]
        if len(vistas) > preguntas:
            linea = vistas[preguntas]
            preguntas += 1
            # Con CAPTURA_PREGUNTA=fichero.ppm, la pantalla con la primera pregunta.
            if preguntas == 1 and captura_pregunta:
                time.sleep(0.3)
                mandar("screendump " + captura_pregunta)
            if "¿pide " in linea:
                time.sleep(1.3)
                mandar("sendkey s")
            else:
                mandar("sendkey s")
                time.sleep(1.3)
                mandar("sendkey n")
            time.sleep(0.5)
            continue
        # Mientras el selector espera teclas, el centro no teclea: si los dos tecleaban a la vez, las teclas se las llevaba
        # la ventana que tuviera el teclado y el selector caducaba al minuto. El selector va primero (caduca); el centro
        # sigue después.
        selector_espera = selector in (1, 2, 4, 5, 17, 18)
        # S10c: el centro de control. `guardian` avisa 3 s antes de pedirle que abra Permisos: se deja de teclear, para que
        # su ventana se lleve el teclado (S8: 2 s sin teclas). Dentro, flecha abajo (pide · eco) y Supr para quitarlo;
        # luego Tab (a la lista de categorías) y flecha abajo (Seguridad), y Esc para cerrar. Con CAPTURA_CENTRO=fichero.ppm,
        # la ventana con la fila marcada, y en fichero.ppm.seg.ppm, la de Seguridad.
        if not selector_espera and centro == 0 and "[guardian] el centro de control abrirá Permisos" in texto:
            centro = 1
        if not selector_espera and centro == 1:
            if "[centro] Permisos en pantalla" not in texto:
                time.sleep(0.25)
                continue
            centro = 2
            time.sleep(0.5)
            # Con el ratón: clic en la primera fila (pide · eco) y en el botón Quitar. Si no se sabe dónde, Supr.
            import re
            v = re.search(r"\[compositor\] ventana de centro en \((\d+), (\d+)\)", texto)
            l = re.search(r"\[centro\] lista en \((\d+), (\d+)\)", texto)
            b = re.search(r"\[centro\] botón Quitar en \((\d+), (\d+)\)", texto)
            def ir(x, y):
                mandar(*["mouse_move -100 -100"] * 12, pausa=0.05)
                pasos = []
                while x > 0 or y > 0:
                    dx, dy = min(x, 100), min(y, 100)
                    pasos.append(f"mouse_move {dx} {dy}")
                    x, y = x - dx, y - dy
                mandar(*pasos, pausa=0.05)
                time.sleep(0.3)
            if v and l and b:
                vx, vy = int(v.group(1)), int(v.group(2))
                ir(vx + int(l.group(1)) + 60, vy + int(l.group(2)) + 28)
                mandar("mouse_button 1", "mouse_button 0", pausa=0.08)
                if captura_centro:
                    time.sleep(0.6)
                    mandar("screendump " + captura_centro)
                time.sleep(0.6)
                ir(vx + int(b.group(1)), vy + int(b.group(2)))
                mandar("mouse_button 1", "mouse_button 0", pausa=0.08)
            else:
                mandar("sendkey down")
                time.sleep(0.3)
                mandar("sendkey delete")
            reintento = time.time() + 4
        if not selector_espera and centro == 2:
            if "[centro] quita a " not in texto:
                if time.time() > reintento:
                    reintento = time.time() + 4
                    mandar("sendkey down")
                    time.sleep(0.3)
                    mandar("sendkey delete")
                time.sleep(0.25)
                continue
            centro = 5
            time.sleep(0.5)
            mandar("sendkey shift-tab")
            time.sleep(0.3)
            mandar("sendkey down")
            reintento = time.time() + 4
        if not selector_espera and centro == 5:
            if "[centro] Seguridad, " not in texto:
                if time.time() > reintento:
                    reintento = time.time() + 4
                    mandar("sendkey shift-tab")
                    time.sleep(0.3)
                    mandar("sendkey down")
                time.sleep(0.25)
                continue
            centro = 6
            if captura_centro:
                time.sleep(0.8)
                mandar("screendump " + captura_centro + ".seg.ppm")
            time.sleep(0.5)
            mandar("sendkey down")
            reintento = time.time() + 4
        # Y una más abajo, Discos; otra, Acerca de; después, Esc.
        if not selector_espera and centro == 6:
            if "[centro] Discos: " not in texto:
                if time.time() > reintento:
                    reintento = time.time() + 4
                    mandar("sendkey down")
                time.sleep(0.25)
                continue
            centro = 14
            time.sleep(0.5)
            # Comprobar el disco: Tab a la lista, Fin (la última fila) e Intro; `guardian` contesta como administradora.
            mandar("sendkey tab", "sendkey end", "sendkey ret", pausa=0.3)
            reintento = time.time() + 30
        if not selector_espera and centro == 14:
            if "[centro] Comprobar el disco: " not in texto and time.time() < reintento:
                time.sleep(0.25)
                continue
            centro = 11
            if captura_centro:
                time.sleep(0.8)
                mandar("screendump " + captura_centro + ".discos.ppm")
            time.sleep(0.5)
            mandar("sendkey shift-tab", "sendkey down", pausa=0.3)
            reintento = time.time() + 4
        # Pantallas; y otra abajo, Apariencia.
        if not selector_espera and centro == 11:
            if "[centro] Pantallas: " not in texto:
                if time.time() > reintento:
                    reintento = time.time() + 4
                    mandar("sendkey down")
                time.sleep(0.25)
                continue
            centro = 8
            time.sleep(0.5)
            mandar("sendkey down")
            reintento = time.time() + 4
        # Apariencia: Tab a su lista, dos abajo (modo oscuro) e Intro; luego Tab a las categorías y abajo.
        if not selector_espera and centro == 8:
            if "[centro] Apariencia: Modo claro" not in texto:
                if time.time() > reintento:
                    reintento = time.time() + 4
                    mandar("sendkey down")
                time.sleep(0.25)
                continue
            centro = 9
            time.sleep(0.5)
            mandar("sendkey tab", "sendkey down", "sendkey down", "sendkey ret", pausa=0.3)
            reintento = time.time() + 4
        if not selector_espera and centro == 9:
            if "[centro] Apariencia: oscuro" not in texto:
                time.sleep(0.25)
                continue
            centro = 10
            if captura_centro:
                time.sleep(0.8)
                mandar("screendump " + captura_centro + ".apariencia.ppm")
            # Con el ratón: doble clic en la cuarta fila (color azul). Dónde: lo dicen el compositor (la ventana) y el
            # centro (su lista, dentro de la ventana). El puntero, a la esquina (0, 0) y de ahí, en pasos de 100 como
            # mucho, al centro de la fila.
            import re
            v = re.search(r"\[compositor\] ventana de centro en \((\d+), (\d+)\)", texto)
            l = re.search(r"\[centro\] lista en \((\d+), (\d+)\)", texto)
            if v and l:
                x, y = int(v.group(1)) + int(l.group(1)) + 60, int(v.group(2)) + int(l.group(2)) + 56 * 3 + 28
                mandar(*["mouse_move -100 -100"] * 12, pausa=0.05)
                pasos = []
                while x > 0 or y > 0:
                    dx, dy = min(x, 100), min(y, 100)
                    pasos.append(f"mouse_move {dx} {dy}")
                    x, y = x - dx, y - dy
                mandar(*pasos, pausa=0.05)
                time.sleep(0.3)
                mandar("mouse_button 1", "mouse_button 0", "mouse_button 1", "mouse_button 0", pausa=0.08)
            reintento = time.time() + 5
        # C3: los fondos y el color del fondo. Con el teclado en la lista de Apariencia (Inicio, abajo hasta la fila,
        # Intro): poner ondas.png (en el disco, el guardado es el enorme, que no se pone), elegir "Paleta del fondo",
        # poner rota.png (no se pone: se quedan el fondo y el tema) y poner atardecer.png (el tema sale del nuevo).
        # Las filas, de lo que dice el centro al entrar en Apariencia. Con CAPTURA_CENTRO, fichero.fondo-ondas.ppm,
        # fichero.fondo-color.ppm y fichero.fondo-atardecer.ppm.
        def fila_apariencia(empieza):
            import re
            m = re.search(r"\[centro\] Apariencia: (Modo claro[^\n]*)", texto)
            filas = m.group(1).split("; ") if m else []
            return next((k for k, f in enumerate(filas) if f.startswith(empieza)), None)
        def elegir_fila(empieza):
            k = fila_apariencia(empieza)
            if k is not None:
                mandar("sendkey home", *["sendkey down"] * k, "sendkey ret", pausa=0.2)
        if not selector_espera and centro == 10:
            if "[centro] Apariencia: oscuro, color Azul" not in texto and time.time() < reintento:
                time.sleep(0.25)
                continue
            centro = 15
            time.sleep(0.5)
            elegir_fila("Fondo: ondas.png")
            reintento = time.time() + 30
        if not selector_espera and centro == 15:
            if "[centro] Fondo ondas.png: " not in texto and time.time() < reintento:
                time.sleep(0.25)
                continue
            centro = 16
            if captura_centro:
                time.sleep(0.8)
                mandar("screendump " + captura_centro + ".fondo-ondas.ppm")
            time.sleep(0.5)
            elegir_fila("Paleta del fondo")
            reintento = time.time() + 30
        if not selector_espera and centro == 16:
            if "[centro] Apariencia: oscuro, color Del fondo" not in texto and time.time() < reintento:
                time.sleep(0.25)
                continue
            centro = 17
            if captura_centro:
                time.sleep(0.8)
                mandar("screendump " + captura_centro + ".fondo-color.ppm")
            time.sleep(0.5)
            elegir_fila("Fondo: rota.png")
            reintento = time.time() + 30
        if not selector_espera and centro == 17:
            if "[centro] Fondo rota.png: " not in texto and time.time() < reintento:
                time.sleep(0.25)
                continue
            centro = 18
            time.sleep(0.5)
            elegir_fila("Fondo: atardecer.png")
            reintento = time.time() + 30
        if not selector_espera and centro == 18:
            if texto.count("[centro] Apariencia: oscuro, color Del fondo") < 2 and time.time() < reintento:
                time.sleep(0.25)
                continue
            centro = 7
            if captura_centro:
                time.sleep(0.8)
                mandar("screendump " + captura_centro + ".fondo-atardecer.ppm")
            time.sleep(0.5)
            mandar("sendkey shift-tab", "sendkey down", pausa=0.3)
            reintento = time.time() + 4
        if not selector_espera and centro == 7:
            if "[centro] Acerca de: " not in texto:
                if time.time() > reintento:
                    reintento = time.time() + 4
                    mandar("sendkey down")
                time.sleep(0.25)
                continue
            centro = 12
            if captura_centro:
                time.sleep(0.8)
                mandar("screendump " + captura_centro + ".acerca.ppm")
            time.sleep(0.5)
            # Super+Arriba la maximiza y Super+Abajo la devuelve; luego, Esc.
            mandar("sendkey meta_l-up")
            reintento = time.time() + 4
        if not selector_espera and centro == 12:
            if "[centro] el compositor pide " not in texto:
                time.sleep(0.25)
                if time.time() < reintento:
                    continue
            centro = 13
            if captura_centro:
                time.sleep(0.8)
                mandar("screendump " + captura_centro + ".max.ppm")
            time.sleep(0.5)
            mandar("sendkey meta_l-down")
            reintento = time.time() + 4
        if not selector_espera and centro == 13:
            if "[centro] vuelve a " not in texto:
                time.sleep(0.25)
                if time.time() < reintento:
                    continue
            centro = 3
            time.sleep(0.5)
            mandar("sendkey esc")
            reintento = time.time() + 4
        if not selector_espera and centro == 3:
            if "[centro] cerrado" not in texto:
                if time.time() > reintento:
                    reintento = time.time() + 4
                    mandar("sendkey esc")
                time.sleep(0.25)
                continue
            centro = 4
        # C2: el panel rápido. Cuando `guardian` acaba sus bloqueos (con la sesión bloqueada, Super+A no hace nada): Super+A lo abre; un clic en el mosaico apaga el modo oscuro y
        # otro lo vuelve a encender (lo pone el centro, que lo guarda); "−" y "+" del volumen. Luego el modo de edición: Editar, quitar Bloquear, Hecho (el
        # centro lo guarda y el panel ya no lo enseña); otra vez Editar, ponerlo (va al final), subirlo y Hecho: como estaba (el disco acaba con los de serie).
        # Super+A lo cierra. La pantalla mide 800 de ancho; los sitios los dice el panel, desde la esquina de arriba a la derecha.
        # Con CAPTURA_CENTRO, también fichero.panel.ppm (abierto, en oscuro), fichero.panel-claro.ppm, fichero.panel-edicion.ppm (en edición, Bloquear quitado)
        # y fichero.panel-sin.ppm (sin Bloquear).
        if panel == 0 and "[guardian] bloqueo con la tecla: " in texto:
            panel = 1
            time.sleep(0.5)
            mandar("sendkey meta_l-a")
            reintento = time.time() + 6
        if 1 <= panel <= 13:
            import re
            def ir_panel(x, y):
                # El panel dice sus sitios como si estuviera a 8 del borde de arriba; con la barra (S10) sale más abajo:
                # dónde, lo dice el compositor.
                capa = re.findall(r"\[compositor\] capa de panel en \((\d+), (\d+)\)", texto)
                y += int(capa[-1][1]) - 8 if capa else 0
                mandar(*["mouse_move -100 -100"] * 12, pausa=0.05)
                pasos = []
                while x > 0 or y > 0:
                    dx, dy = min(x, 100), min(y, 100)
                    pasos.append(f"mouse_move {dx} {dy}")
                    x, y = x - dx, y - dy
                mandar(*pasos, pausa=0.05)
                time.sleep(0.3)
                mandar("mouse_button 1", "mouse_button 0", pausa=0.08)
            def ultima(patron):
                todas_ = re.findall(patron, texto)
                return todas_[-1] if todas_ else None
            def clic_en(sitio):
                if sitio:
                    ir_panel(800 - int(sitio[0]), int(sitio[1]))
            m = re.search(r"\[panel\] abierto; mosaico a \((\d+), (\d+)\)", texto)
            vol = re.search(r"\[panel\] volumen: menos a \((\d+), (\d+)\), más a \((\d+), (\d+)\)", texto)
            editar = ultima(r"\[panel\] se ven: [a-z ]+; editar a \((\d+), (\d+)\)")
            # Cada vez que pinta en edición, el panel dice dónde está cada botón, una línea por fila con el número de la vez.
            veces = [(int(k), l) for k, l in re.findall(r"\[panel\] edición (\d+): (.*)", texto)]
            edicion = sorted({k for k, _ in veces})
            QUITAR_BLOQUEAR = r"bloquear: subir \(\d+, \d+\) bajar \(\d+, \d+\) quitar"
            def en_edicion(que):
                ultima_vez = "\n".join(l for k, l in veces if edicion and k == edicion[-1])
                g = re.search(que + r" \((\d+), (\d+)\)", ultima_vez)
                return g.groups() if g else None
            espera = {1: m is not None and vol is not None and "[compositor] capa de panel en" in texto, 2: "[centro] modo claro (lo pide el panel)" in texto,
                      3: "[centro] modo oscuro (lo pide el panel)" in texto,
                      4: "[sonido] volumen general: 90 % (lo pide panel)" in texto,
                      5: "[sonido] volumen general: 100 % (lo pide panel)" in texto,
                      6: len(edicion) >= 1 and en_edicion(QUITAR_BLOQUEAR) is not None,
                      7: "[panel] quita bloquear" in texto and len(edicion) >= 2 and en_edicion("bloquear: poner") is not None,
                      8: "[panel] se ven: oscuro volumen;" in texto,
                      9: len(edicion) >= 3 and en_edicion("bloquear: poner") is not None,
                      10: "[panel] pone bloquear" in texto and len(edicion) >= 4 and en_edicion("bloquear: subir") is not None,
                      11: "[panel] sube bloquear" in texto and len(edicion) >= 5 and en_edicion("volumen: subir") is not None,
                      12: texto.count("[panel] se ven: oscuro bloquear volumen;") >= 2,
                      13: "[panel] cerrado" in texto}[panel]
            if not espera:
                if time.time() > reintento and panel < 13:
                    # No llegó: se cierra el panel (las líneas que falten lo dirán).
                    mandar("sendkey meta_l-a")
                    panel = 13
                    reintento = time.time() + 6
                time.sleep(0.25)
                continue
            if panel in (1, 2) and m:
                if captura_centro:
                    time.sleep(0.6)
                    mandar("screendump " + captura_centro + (".panel.ppm" if panel == 1 else ".panel-claro.ppm"))
                ir_panel(800 - int(m.group(1)), int(m.group(2)))
            elif panel == 3:
                # El volumen general: "−" (90 %) y luego "+" (100 %, como estaba).
                ir_panel(800 - int(vol.group(1)), int(vol.group(2)))
            elif panel == 4:
                ir_panel(800 - int(vol.group(3)), int(vol.group(4)))
            elif panel in (5, 8):
                if panel == 8 and captura_centro:
                    time.sleep(0.6)
                    mandar("screendump " + captura_centro + ".panel-sin.ppm")
                clic_en(editar)
            elif panel == 6:
                clic_en(en_edicion(QUITAR_BLOQUEAR))
            elif panel == 7:
                if captura_centro:
                    time.sleep(0.6)
                    mandar("screendump " + captura_centro + ".panel-edicion.ppm")
                clic_en(en_edicion("hecho"))
            elif panel == 9:
                clic_en(en_edicion("bloquear: poner"))
            elif panel == 10:
                clic_en(en_edicion("bloquear: subir"))
            elif panel == 11:
                clic_en(en_edicion("hecho"))
            elif panel == 12:
                time.sleep(0.5)
                mandar("sendkey meta_l-a")
            panel += 1
            reintento = time.time() + 6
            # Sin lo de siempre (un clic de más daría otra vuelta al modo) hasta acabar.
            continue
        # S10: la barra y el lanzador, tras el panel (con la sesión de Ana delante, desbloqueada). Un clic en el botón del
        # lanzador lo abre; "ho" busca (queda hola) e Intro lo lanza por `sesiones`. Super sola lo vuelve a abrir y Esc lo
        # cierra. El botón del panel lo abre (la segunda vez que se abre) y otro clic lo cierra. Los botones los dice la
        # barra, que está en (0, 0). Con CAPTURA_BARRA=fichero.ppm: el lanzador abierto (fichero.ppm), con la búsqueda
        # (fichero.busca.ppm) y la barra con el panel (fichero.panel.ppm).
        if barra == 0 and panel == 14 and "[barra] botones: " in texto:
            barra = 1
            import re
            bb = re.search(r"\[barra\] botones: lanzador en \((\d+), (\d+)\), panel en \((\d+), (\d+)\)", texto)
            def ir_barra(x, y):
                mandar(*["mouse_move -100 -100"] * 20, pausa=0.05)
                pasos = []
                while x > 0 or y > 0:
                    dx, dy = min(x, 100), min(y, 100)
                    pasos.append(f"mouse_move {dx} {dy}")
                    x, y = x - dx, y - dy
                mandar(*pasos, pausa=0.05)
                time.sleep(0.3)
                mandar("mouse_button 1", "mouse_button 0", pausa=0.08)
            time.sleep(0.5)
            ir_barra(int(bb.group(1)), int(bb.group(2)))
            reintento = time.time() + 8
        if 1 <= barra <= 10:
            captura_barra = os.environ.get("CAPTURA_BARRA")
            espera = {1: texto.count("[barra] lanzador abierto") >= 1,
                      2: '[barra] busca "ho": hola' in texto,
                      3: "[sesiones] barra lanza Programas/hola: 0" in texto,
                      4: texto.count("[barra] lanzador abierto") >= 2,
                      5: texto.count("[barra] lanzador cerrado") >= 2,
                      6: "[panel] abierto por 2.ª vez" in texto and texto.count("[compositor] capa de panel en") >= 2,
                      7: texto.count("[panel] cerrado") >= 2,
                      # S27: Super+2 (se ve el 2), el clic en el 1 de la barra (se ve el 1) y las teclas del volumen
                      # en ráfaga: un solo OSD que las junta.
                      8: "[barra] espacios: se ve el 2" in texto,
                      9: "[barra] espacios: se ve el 1" in texto[texto.find("[barra] espacios: se ve el 2"):],
                      10: "[panel] OSD cerrado: " in texto}[barra]
            if barra == 10 and captura_barra and "[panel] OSD abierto" in texto and not globals().get("osd_capturado"):
                globals()["osd_capturado"] = True
                mandar("screendump " + captura_barra[:-4] + ".osd.ppm")
            if not espera:
                if time.time() > reintento:
                    # No llegó: se deja (las líneas que falten lo dirán).
                    barra = 11
                time.sleep(0.25)
                continue
            time.sleep(0.6)
            if barra == 1:
                if captura_barra:
                    mandar("screendump " + captura_barra)
                    time.sleep(0.5)
                mandar("sendkey h", "sendkey o", pausa=0.3)
            elif barra == 2:
                if captura_barra:
                    mandar("screendump " + captura_barra[:-4] + ".busca.ppm")
                    time.sleep(0.5)
                mandar("sendkey ret")
            elif barra == 3:
                mandar("sendkey meta_l")
            elif barra == 4:
                mandar("sendkey esc")
            elif barra == 5:
                ir_barra(int(bb.group(3)), int(bb.group(4)))
            elif barra == 6:
                if captura_barra:
                    mandar("screendump " + captura_barra[:-4] + ".panel.ppm")
                    time.sleep(0.5)
                ir_barra(int(bb.group(3)), int(bb.group(4)))
            elif barra == 7:
                mandar("sendkey meta_l-2")
            elif barra == 8:
                if captura_barra:
                    mandar("screendump " + captura_barra[:-4] + ".espacio2.ppm")
                    time.sleep(0.5)
                uno = re.findall(r"\[barra\] espacios: se ve el 2 de \d+; el 1 en \((\d+), (\d+)\)", texto)
                if uno:
                    ir_barra(int(uno[-1][0]), int(uno[-1][1]))
            elif barra == 9:
                mandar(*["sendkey volumedown"] * 6, "sendkey volumeup", pausa=0.05)
            barra += 1
            reintento = time.time() + 8
            continue
        # S10: los avisos, tras la barra (con Ana delante). Se abre el lanzador y se lanza `avisa`, que avisa: sale el toast
        # (arriba a la derecha) y un clic lo cierra. La campana abre el historial; "Borrar todo" lo vacía y la campana lo
        # cierra. Con la sesión bloqueada, se espera. Los sitios los dicen la barra y el compositor (dónde sale cada capa).
        # Con CAPTURA_AVISOS=fichero.ppm: el toast (fichero.ppm), el historial (fichero.hist.ppm) y vacío
        # (fichero.vacio.ppm).
        if avisos_paso == 0 and barra == 11 and "[barra] campana en (" in texto and "[avisador] inundar" in texto:
            avisos_paso = 1
            reintento_avisos = time.time() + 10
        if 1 <= avisos_paso <= 8:
            import re
            def ultimo_a(t):
                return texto.rfind(t)
            if ultimo_a("[guardian] sesión bloqueada") > max(ultimo_a("[guardian] desbloqueado"), ultimo_a("[guardian] entra ")):
                time.sleep(0.25)
                reintento_avisos = time.time() + 10
                continue
            captura_avisos = os.environ.get("CAPTURA_AVISOS")
            bb = re.search(r"\[barra\] botones: lanzador en \((\d+), (\d+)\)", texto)
            ca = re.search(r"\[barra\] campana en \((\d+), (\d+)\)", texto)
            # El compositor dice dónde sale cada capa con el nombre de su cliente (la barra): la del toast es la primera tras
            # su línea, y la del historial, igual.
            def capa_tras(marca):
                i = texto.rfind(marca)
                return re.findall(r"\[compositor\] capa de barra en \((\d+), (\d+)\)", texto[i:]) if i >= 0 else []
            capa_aviso = capa_tras('[barra] aviso de avisa: "Copia terminada"')
            capa_hist = capa_tras("[barra] historial abierto")
            borrar = re.findall(r"\[barra\] historial abierto: .*Borrar todo en \((\d+), (\d+)\)", texto)
            def clic_en(x, y):
                mandar(*["mouse_move -100 -100"] * 20, pausa=0.05)
                pasos = []
                while x > 0 or y > 0:
                    dx, dy = min(x, 100), min(y, 100)
                    pasos.append(f"mouse_move {dx} {dy}")
                    x, y = x - dx, y - dy
                mandar(*pasos, pausa=0.05)
                time.sleep(0.3)
                mandar("mouse_button 1", "mouse_button 0", pausa=0.08)
            # Lo que se espera antes de cada paso: 1 nada; 2 el lanzador abierto; 3 la búsqueda; 4 el toast de `avisa`, en
            # pantalla; 5 cerrado con el clic; 6 el historial en pantalla; 7 borrado; 8 cerrado.
            espera = {1: True,
                      2: texto.count("[barra] lanzador abierto") >= 3,
                      3: '[barra] busca "avisa": avisa' in texto,
                      4: '[barra] aviso de avisa: "Copia terminada"' in texto and len(capa_aviso) >= 1,
                      5: "[barra] aviso cerrado: con un clic" in texto,
                      6: "[barra] historial abierto" in texto and len(capa_hist) >= 1 and len(borrar) >= 1,
                      7: "[barra] borrar todo: " in texto,
                      8: "[barra] historial cerrado" in texto}[avisos_paso]
            if not espera:
                if time.time() > reintento_avisos:
                    # No llegó: se deja (las líneas que falten lo dirán).
                    avisos_paso = 99
                time.sleep(0.25)
                continue
            time.sleep(0.6)
            if avisos_paso == 1:
                clic_en(int(bb.group(1)), int(bb.group(2)))
            elif avisos_paso == 2:
                mandar("sendkey a", "sendkey v", "sendkey i", "sendkey s", "sendkey a", pausa=0.3)
            elif avisos_paso == 3:
                mandar("sendkey ret")
            elif avisos_paso == 4:
                time.sleep(0.4)
                if captura_avisos:
                    mandar("screendump " + captura_avisos)
                    time.sleep(0.5)
                x, y = capa_aviso[0]
                clic_en(int(x) + 180, int(y) + 44)
            elif avisos_paso == 5:
                clic_en(int(ca.group(1)), int(ca.group(2)))
            elif avisos_paso == 6:
                time.sleep(0.4)
                if captura_avisos:
                    mandar("screendump " + captura_avisos[:-4] + ".hist.ppm")
                    time.sleep(0.5)
                x, y = capa_hist[0]
                # La rueda sobre la lista (a media altura del historial): dos pasos en un sentido y dos en el otro (el signo
                # de `dz` en el monitor de QEMU no está claro): en algún momento la lista pasa por la fila 3.
                mandar(*["mouse_move -100 -100"] * 20, pausa=0.05)
                px, py = int(x) + 150, int(y) + 200
                pasos = []
                while px > 0 or py > 0:
                    dx, dy = min(px, 100), min(py, 100)
                    pasos.append(f"mouse_move {dx} {dy}")
                    px, py = px - dx, py - dy
                mandar(*pasos, pausa=0.05)
                time.sleep(0.3)
                mandar("mouse_move 0 0 -1", "mouse_move 0 0 -1", "mouse_move 0 0 1", "mouse_move 0 0 1", pausa=0.3)
                time.sleep(0.5)
                bx, by = borrar[-1]
                clic_en(int(x) + int(bx), int(y) + int(by))
            elif avisos_paso == 7:
                time.sleep(0.4)
                if captura_avisos:
                    mandar("screendump " + captura_avisos[:-4] + ".vacio.ppm")
                    time.sleep(0.5)
                clic_en(int(ca.group(1)), int(ca.group(2)))
            elif avisos_paso == 8:
                # El cursor, abajo a la derecha, donde los clics de siempre (que siguen, hacia abajo a la derecha) no dan
                # en nada: desde la campana volverían a abrir el historial y el panel.
                ir_a(790, 590)
                avisos_paso = 99
                continue
            avisos_paso += 1
            reintento_avisos = time.time() + 10
            continue
        # Con una sesión bloqueada, el teclado es de la pantalla de bloqueo: lo del selector espera (si no, sus Intro
        # de reintento caían allí como contraseñas vacías).
        def ultimo(t):
            return texto.rfind(t)
        # Bloqueada desde cualquiera de sus avisos hasta que se desbloquea o entra alguien. El bloqueo que pide `pide` ("se
        # bloquea la sesión de ...") no cuenta hasta que se cumple: guardian lo deja pendiente hasta acabar sus 40 s de
        # preguntas, y contarlo antes dejaba el selector un minuto sin teclas, encima del menú de Ajustes.
        bloqueo_desde = max(ultimo("[guardian] sesión bloqueada"), ultimo("pantalla de bloqueo pintada"),
                            ultimo("pantalla de entrada pintada"))
        bloqueado_ahora = bloqueo_desde > max(ultimo("[guardian] desbloqueado"), ultimo("[guardian] entra "))
        # S10: el Workbench, al final (tras la barra), con la sesión de Ana delante y sin bloquear. Doble clic en el icono
        # de Sistema (abre su ventana), en DeCarla (no se abre: es de Carla) y en Programas (otra ventana); doble clic en
        # hola (lo lanza `sesiones`) y botón derecho en hola (su Información, que no se puede cambiar: Cerrar). Luego el
        # icono de Home, Privado, el botón Lista, un clic en nota.txt e Información: en su campo, "nota de Ana", la casilla
        # Archivado y Guardar. Los sitios los dicen el compositor (dónde salió cada ventana: la n-ésima "ventana de
        # workbench", y la capa) y el Workbench (dentro de cada una). Con CAPTURA_WORKBENCH=fichero.ppm: el escritorio
        # con las ventanas de Sistema y Programas (fichero.ppm), Privado como lista (fichero.lista.ppm), la Información
        # (fichero.info.ppm) y la
        # Información con el comentario escrito (fichero.comentario.ppm). Al final, Cerrar en cada ventana.
        if wb == 0 and barra == 11 and not bloqueado_ahora and "[workbench] icono de Home en (" in texto and "[compositor] capa de workbench en (" in texto:
            wb = 1
            reintento = 0
        if 1 <= wb <= 17:
            import re
            captura_wb = os.environ.get("CAPTURA_WORKBENCH")
            def ultima_wb(patron):
                t = re.findall(patron, texto)
                return t[-1] if t else None
            def ir_wb(x, y):
                mandar(*["mouse_move -100 -100"] * 12, pausa=0.05)
                pasos = []
                while x > 0 or y > 0:
                    dx, dy = min(x, 100), min(y, 100)
                    pasos.append(f"mouse_move {dx} {dy}")
                    x, y = x - dx, y - dy
                mandar(*pasos, pausa=0.05)
                time.sleep(0.3)
            def doble():
                mandar("mouse_button 1", "mouse_button 0", "mouse_button 1", "mouse_button 0", pausa=0.08)
            def clic():
                mandar("mouse_button 1", "mouse_button 0", pausa=0.08)
            def capa_wb():
                c = ultima_wb(r"\[compositor\] capa de workbench en \((\d+), (\d+)\)")
                return (int(c[0]), int(c[1])) if c else (0, 40)
            def icono_wb(nombre):
                c = ultima_wb(r"\[workbench\] (?:en pantalla: \d+x\d+, )?icono de " + nombre + r" en \((\d+), (\d+)\)")
                cx, cy = capa_wb()
                return (cx + int(c[0]), cy + int(c[1])) if c else None
            def origen(n):
                # La n-ésima ventana del Workbench que coloca el compositor.
                t = re.findall(r"\[compositor\] ventana de workbench en \((\d+), (\d+)\)", texto)
                return (int(t[n - 1][0]), int(t[n - 1][1])) if len(t) >= n else None
            def en_ventana(clave, que):
                # Dónde está `que` (una entrada o un botón) en la ventana de `clave`, en la pantalla.
                n = wb_ventana.get(clave)
                o = origen(n) if n else None
                if not o:
                    return None
                g = ultima_wb(r"\[workbench\] ventana " + str(n) + r"[^\n]*?(?:: |; |, )" + re.escape(que) + r" en \((\d+), (\d+)\)")
                return (o[0] + int(g[0]), o[1] + int(g[1])) if g else None
            def ventana_de(ruta):
                g = ultima_wb(r"\[workbench\] ventana (\d+) de " + re.escape(ruta) + r" en pantalla")
                return int(g) if g else None
            espera = {1: ventana_de("Sistema") and origen(ventana_de("Sistema")),
                      2: "[workbench] no abre Sistema/DeCarla" in texto,
                      3: ventana_de("Sistema/Programas") and origen(ventana_de("Sistema/Programas")),
                      4: "[workbench] lanza Programas/hola" in texto,
                      5: ventana_de("Información de Sistema/Programas/hola") and origen(ventana_de("Información de Sistema/Programas/hola")),
                      6: "[workbench] cierra la ventana " + str(ventana_de("Información de Sistema/Programas/hola")) in texto,
                      7: ventana_de("Home") and origen(ventana_de("Home")),
                      8: ventana_de("Home/Privado") and origen(ventana_de("Home/Privado")),
                      9: "[workbench] Home/Privado como lista" in texto,
                      10: ventana_de("Información de Home/Privado/nota.txt") and origen(ventana_de("Información de Home/Privado/nota.txt")),
                      11: True,
                      12: "[workbench] guarda Home/Privado/nota.txt" in texto,
                      13: "[workbench] cierra la ventana " + str(wb_ventana.get("Home/Privado")) in texto,
                      14: "[workbench] cierra la ventana " + str(wb_ventana.get("Home")) in texto,
                      15: "[workbench] cierra la ventana " + str(wb_ventana.get("Sistema/Programas")) in texto,
                      16: "[workbench] sin ventanas abiertas" in texto}
            if wb > 1 and not espera[wb - 1]:
                if time.time() > reintento:
                    # No llegó: se deja (las líneas que falten lo dirán).
                    wb = 99
                time.sleep(0.25)
                continue
            time.sleep(0.6)
            for clave in ("Sistema", "Sistema/Programas", "Información de Sistema/Programas/hola", "Home", "Home/Privado", "Información de Home/Privado/nota.txt"):
                if ventana_de(clave):
                    wb_ventana[clave] = ventana_de(clave)
            sitio = None
            if wb == 1:
                sitio = icono_wb("Sistema")
                if sitio:
                    ir_wb(*sitio)
                    doble()
            elif wb == 2:
                sitio = en_ventana("Sistema", "DeCarla")
                if sitio:
                    ir_wb(*sitio)
                    doble()
            elif wb == 3:
                sitio = en_ventana("Sistema", "Programas")
                if sitio:
                    ir_wb(*sitio)
                    doble()
            elif wb == 4:
                if captura_wb:
                    mandar("screendump " + captura_wb)
                    time.sleep(0.5)
                sitio = en_ventana("Sistema/Programas", "hola")
                if sitio:
                    ir_wb(*sitio)
                    doble()
            elif wb == 5:
                sitio = en_ventana("Sistema/Programas", "hola")
                if sitio:
                    ir_wb(*sitio)
                    mandar("mouse_button 2", "mouse_button 0", pausa=0.08)
            elif wb == 6:
                sitio = en_ventana("Información de Sistema/Programas/hola", "Cerrar")
                if sitio:
                    ir_wb(*sitio)
                    clic()
            elif wb == 7:
                sitio = icono_wb("Home")
                if sitio:
                    ir_wb(*sitio)
                    doble()
            elif wb == 8:
                sitio = en_ventana("Home", "Privado")
                if sitio:
                    ir_wb(*sitio)
                    doble()
            elif wb == 9:
                sitio = en_ventana("Home/Privado", "Lista o Iconos")
                if sitio:
                    ir_wb(*sitio)
                    clic()
            elif wb == 10:
                if captura_wb:
                    mandar("screendump " + captura_wb[:-4] + ".lista.ppm")
                    time.sleep(0.5)
                sitio = en_ventana("Home/Privado", "nota.txt")
                boton = en_ventana("Home/Privado", "Información")
                if sitio and boton:
                    ir_wb(*sitio)
                    clic()
                    time.sleep(0.5)
                    ir_wb(*boton)
                    clic()
            elif wb == 11:
                if captura_wb:
                    mandar("screendump " + captura_wb[:-4] + ".info.ppm")
                    time.sleep(0.5)
                sitio = en_ventana("Información de Home/Privado/nota.txt", "campo")
                if sitio:
                    ir_wb(*sitio)
                    clic()
                    time.sleep(0.5)
                    teclear("nota de Ana")
                    time.sleep(0.5)
                    archivado = en_ventana("Información de Home/Privado/nota.txt", "Archivado")
                    if archivado:
                        ir_wb(*archivado)
                        clic()
            elif wb == 12:
                if captura_wb:
                    time.sleep(0.5)
                    mandar("screendump " + captura_wb[:-4] + ".comentario.ppm")
                    time.sleep(0.5)
                sitio = en_ventana("Información de Home/Privado/nota.txt", "Guardar")
                if sitio:
                    ir_wb(*sitio)
                    clic()
            elif 13 <= wb <= 16:
                # Al final, se cierran todas (la captura de `captura_ventanas.py` mira lo de debajo).
                clave = ("Home/Privado", "Home", "Sistema/Programas", "Sistema")[wb - 13]
                sitio = en_ventana(clave, "Cerrar")
                if sitio:
                    ir_wb(*sitio)
                    clic()
            elif wb == 17:
                sitio = True
            if sitio is None:
                # No se sabe dónde: se deja.
                wb = 99
                continue
            wb += 1
            reintento = time.time() + 15
            continue
        # Si el selector ya se rindió (un minuto sin elegir: con el PC cargado, la sesión siguió bloqueada todo ese rato),
        # el guion no le espera más: si no, sus pasos hacían `continue` para siempre y no llegaba ni a Super+L. Sus líneas
        # faltarán y lo dirán.
        if selector in (1, 2, 17, 18) and "[selector] el usuario no elige nada" in texto:
            selector, espera_carpeta = 3, time.time() + 15
        if selector in (4, 5) and "[selector] el usuario no elige carpeta" in texto:
            selector = 6
        if bloqueado_ahora and selector in (1, 2, 4, 5, 17, 18):
            reintento = max(reintento, time.time() + 4)
        # U5: la ventana del selector. Mientras está, nada de lo de siempre (un clic podría caer en Cancelar). Primero un
        # Intro sin nada elegido, que no debe abrir nada; luego flecha abajo hasta Privado (el tercer cajón) e Intro para
        # entrar; dentro, Fin (el último, nota.txt; el primero es "../") e Intro.
        if not bloqueado_ahora and selector == 0 and "[selector] ventana de abrir en pantalla" in texto:
            selector = 1
            time.sleep(0.5)
            # Antes, un clic en su cabecera, a la derecha (donde no hay nada que pulsar y Ajustes, que puede estar encima,
            # no llega: el selector mide 460 de ancho), para que tenga el teclado: si el paso de Ajustes pulsó en Ajustes
            # justo después de salir el selector, se lo quitó. Con más programas en el arranque (los avisos), Ajustes y
            # el selector salen más juntos.
            import re
            vs = re.search(r"\[compositor\] ventana de selector en \((\d+), (\d+)\)", texto)
            if vs:
                ir_a(int(vs.group(1)) + 440, int(vs.group(2)) + 20)
                time.sleep(0.3)
                mandar("mouse_button 1", "mouse_button 0", pausa=0.08)
                time.sleep(0.5)
            mandar("sendkey ret")
            time.sleep(0.5)
            # Tres abajo: los cajones van primero, y `Fondos` (C3) y `Prefs` (S10c) van antes que `Privado`.
            mandar("sendkey down")
            time.sleep(0.3)
            mandar("sendkey down")
            time.sleep(0.3)
            mandar("sendkey down")
            time.sleep(0.3)
            mandar("sendkey ret")
            reintento = time.time() + 4
        if not bloqueado_ahora and selector == 1:
            if "[selector] entra en C/Privado" not in texto:
                # Si una pregunta de `guardian` (que toma el teclado) o la ventana del centro de control (sale en (168, 180),
                # encima) se llevó las teclas, otra vez: antes, un clic arriba a la izquierda del selector, que el centro
                # no tapa (empieza 32 más a la derecha), para devolverle el teclado; Inicio (el primero) y tres abajo
                # hasta Privado (van antes `Fondos` y `Prefs`).
                if time.time() > reintento:
                    reintento = time.time() + 4
                    import re
                    vs = re.search(r"\[compositor\] ventana de selector en \((\d+), (\d+)\)", texto)
                    if vs:
                        ir_a(int(vs.group(1)) + 20, int(vs.group(2)) + 30)
                        time.sleep(0.3)
                        mandar("mouse_button 1", "mouse_button 0", pausa=0.08)
                        time.sleep(0.3)
                    mandar("sendkey home")
                    time.sleep(0.3)
                    mandar("sendkey down", "sendkey down", "sendkey down", pausa=0.3)
                    time.sleep(0.3)
                    mandar("sendkey ret")
                time.sleep(0.25)
                continue
            # Dentro, Retroceso sube a C; y otra vez abajo, abajo e Intro para volver a entrar.
            if not bloqueado_ahora and selector == 1 and "[selector] sube a C," not in texto:
                time.sleep(0.5)
                mandar("sendkey backspace")
                selector = 17
                reintento = time.time() + 4
        if not bloqueado_ahora and selector == 17:
            if "[selector] sube a C," not in texto and time.time() < reintento:
                time.sleep(0.25)
                continue
            time.sleep(0.5)
            mandar("sendkey down", "sendkey down", "sendkey down", "sendkey ret", pausa=0.3)
            selector = 18
            reintento = time.time() + 4
        if not bloqueado_ahora and selector == 18:
            if texto.count("[selector] entra en C/Privado") < 2 and time.time() < reintento:
                time.sleep(0.25)
                continue
            selector = 2
            time.sleep(0.5)
            # Dentro, con el ratón: doble clic en la segunda fila (nota.txt; la primera es "../"). Dónde: lo dicen el
            # compositor (la ventana) y el selector (su lista). Si no se sabe, con el teclado: Fin e Intro.
            import re
            v = re.search(r"\[compositor\] ventana de selector en \((\d+), (\d+)\)", texto)
            l = re.search(r"\[selector\] lista en \((\d+), (\d+)\)", texto)
            if v and l:
                x, y = int(v.group(1)) + int(l.group(1)) + 60, int(v.group(2)) + int(l.group(2)) + 56 + 28
                mandar(*["mouse_move -100 -100"] * 12, pausa=0.05)
                pasos = []
                while x > 0 or y > 0:
                    dx, dy = min(x, 100), min(y, 100)
                    pasos.append(f"mouse_move {dx} {dy}")
                    x, y = x - dx, y - dy
                mandar(*pasos, pausa=0.05)
                time.sleep(0.3)
                # Un clic solo marca la fila; con la captura, la ventana así.
                mandar("mouse_button 1", "mouse_button 0", pausa=0.08)
                if captura_selector:
                    time.sleep(0.7)
                    mandar("screendump " + captura_selector)
                time.sleep(0.8)
                mandar("mouse_button 1", "mouse_button 0", "mouse_button 1", "mouse_button 0", pausa=0.08)
            else:
                mandar("sendkey end")
                time.sleep(0.3)
                mandar("sendkey ret")
            reintento = time.time() + 4
        if not bloqueado_ahora and selector == 2:
            if "[selector] el usuario " not in texto:
                if time.time() > reintento:
                    reintento = time.time() + 4
                    mandar("sendkey end")
                    time.sleep(0.3)
                    mandar("sendkey ret")
                time.sleep(0.25)
                continue
            selector = 3
            espera_carpeta = time.time() + 15
        # U5: la carpeta para guardar (el hostil pide "carpeta"): abajo tres veces (Privado, tras Fondos y Prefs), Intro para entrar y, ya
        # dentro, clic en "Elegir esta carpeta" (los botones del kit no toman el teclado).
        # El hostil la pide justo después de lo de antes: hasta que sale, nada de lo de siempre (un clic caería en Cancelar).
        if not bloqueado_ahora and selector == 3 and "[selector] ventana de elegir carpeta en pantalla" not in texto and time.time() < espera_carpeta:
            time.sleep(0.25)
            continue
        if not bloqueado_ahora and selector == 3 and "[selector] ventana de elegir carpeta en pantalla" in texto:
            selector = 4
            # Las veces que ya entró en C/Privado (el flujo de antes también entra): hace falta una más.
            entradas_antes = texto.count("[selector] entra en C/Privado")
            time.sleep(0.5)
            mandar("sendkey down", "sendkey down", "sendkey down", pausa=0.3)
            time.sleep(0.3)
            mandar("sendkey ret")
            reintento = time.time() + 4
        if not bloqueado_ahora and selector == 4:
            if texto.count("[selector] entra en C/Privado") <= entradas_antes:
                if time.time() > reintento:
                    reintento = time.time() + 4
                    mandar("sendkey ret")
                time.sleep(0.25)
                continue
            selector = 5
            import re
            v = re.findall(r"\[compositor\] ventana de selector en \((\d+), (\d+)\)", texto)
            b = re.search(r"\[selector\] botón Elegir en \((\d+), (\d+)\)", texto)
            if v and b:
                x, y = int(v[-1][0]) + int(b.group(1)), int(v[-1][1]) + int(b.group(2))
                time.sleep(0.5)
                if captura_selector:
                    mandar("screendump " + captura_selector + ".carpeta.ppm")
                    time.sleep(0.5)
                mandar(*["mouse_move -100 -100"] * 12, pausa=0.05)
                pasos = []
                while x > 0 or y > 0:
                    dx, dy = min(x, 100), min(y, 100)
                    pasos.append(f"mouse_move {dx} {dy}")
                    x, y = x - dx, y - dy
                mandar(*pasos, pausa=0.05)
                time.sleep(0.3)
                mandar("mouse_button 1", "mouse_button 0", pausa=0.08)
            reintento = time.time() + 4
        if not bloqueado_ahora and selector == 5:
            if "[selector] el usuario elige la carpeta" not in texto and "[selector] el usuario no elige carpeta" not in texto:
                time.sleep(0.25)
                if time.time() < reintento:
                    continue
            selector = 6
        if bloqueo == 0 and "[guardian] pantalla de bloqueo pintada" in texto:
            bloqueo = 1
            time.sleep(0.5)
            teclear("mala")
            # La segunda, cuando `guardian` ya dijo que la primera era mala (no a ciegas: se perdían teclas).
            for _ in range(40):
                time.sleep(0.25)
                if "contraseña mala en la pantalla de bloqueo" in open(serie, errors="replace").read():
                    break
            time.sleep(0.3)
            teclear("clave de Carla")
        if bloqueo == 1:
            if "[guardian] desbloqueado" not in texto:
                time.sleep(0.5)
                continue
            bloqueo = 2
        # Después, la pantalla de entrada (U3): el nombre y la contraseña de Ana.
        if bloqueo == 2 and "[guardian] pantalla de entrada pintada" in texto:
            bloqueo = 3
            time.sleep(0.5)
            # Primero alguien que no existe; cuando `guardian` dice que no, Ana con su contraseña.
            antes = open(serie, errors="replace").read().count("contraseña mala en la pantalla")
            teclear("Nadie")
            time.sleep(0.5)
            teclear("x")
            for _ in range(40):
                time.sleep(0.25)
                if open(serie, errors="replace").read().count("contraseña mala en la pantalla") > antes:
                    break
            time.sleep(0.3)
            teclear("Ana")
            time.sleep(0.5)
            teclear("clave de Ana")
        if bloqueo == 3:
            if "[guardian] entra " not in texto:
                time.sleep(0.5)
                continue
            bloqueo = 4
        # U6: el cambio rápido de usuario. Tras entrar Ana, `guardian` bloquea su sesión: Esc, "Carla" y su contraseña
        # (entra Carla con Ana detrás). Luego bloquea la de Carla: Esc, "Ana" y la suya (vuelve Ana). Mientras se teclea
        # y hasta que entra, nada de lo de siempre.
        if cambio in (0, 1) and bloqueo == 4 and texto.count("pantalla de bloqueo pintada") >= 2 + cambio:
            quien, clave = (("Carla", "clave de Carla"), ("Ana", "clave de Ana"))[cambio]
            captura_cambio = os.environ.get("CAPTURA_CAMBIO") if cambio == 0 else None
            time.sleep(0.5)
            if captura_cambio:
                mandar("screendump " + captura_cambio)
                time.sleep(0.5)
            mandar("sendkey esc")
            for _ in range(40):
                time.sleep(0.25)
                if open(serie, errors="replace").read().count("[guardian] cambio de usuario: se pide el nombre") > cambio:
                    break
            time.sleep(0.3)
            teclear(quien)
            time.sleep(0.8)
            if captura_cambio:
                mandar("screendump " + captura_cambio[:-4] + ".nombre.ppm")
                time.sleep(0.5)
            teclear(clave)
            cambio += 1
        if cambio in (1, 2) and texto.count("[guardian] cambio de usuario: entra ") < cambio:
            time.sleep(0.25)
            continue
        # Y el bloqueo por tiempo (U3): 3 s más de teclas tras pedir `guardian` el aviso, y luego nada hasta que bloquea;
        # entonces, una contraseña mala y la de Ana.
        if bloqueo == 4 and "[guardian] esperando " in texto:
            bloqueo, quieto = 5, time.time() + 3
        if bloqueo == 5 and time.time() >= quieto:
            # La cuarta: antes, la de Carla y las dos del cambio de usuario (U6).
            if texto.count("pantalla de bloqueo pintada") < 4:
                time.sleep(0.25)
                continue
            bloqueo = 6
            time.sleep(0.5)
            antes = texto.count("contraseña mala en la pantalla de bloqueo")
            teclear("mala")
            for _ in range(40):
                time.sleep(0.25)
                if open(serie, errors="replace").read().count("contraseña mala en la pantalla de bloqueo") > antes:
                    break
            time.sleep(0.3)
            teclear("clave de Ana")
        if bloqueo == 6:
            if texto.count("[guardian] desbloqueado") < 2:
                time.sleep(0.5)
                continue
            bloqueo = 7
        # U3: bloquear con la tecla. `guardian` espera un aviso de 60 s; Super+L lo da ya. Con la pantalla de bloqueo,
        # una mala y la de Ana (y nada de las teclas de siempre, que irían a ella).
        if bloqueo == 7 and "[guardian] esperando 60000 ms" in texto:
            bloqueo = 8
            time.sleep(1)
            mandar("sendkey meta_l-l")
        if bloqueo == 8:
            if texto.count("pantalla de bloqueo pintada") < 5:
                time.sleep(0.25)
                continue
            bloqueo = 9
            time.sleep(0.5)
            antes = texto.count("contraseña mala en la pantalla de bloqueo")
            teclear("mala")
            for _ in range(40):
                time.sleep(0.25)
                if open(serie, errors="replace").read().count("contraseña mala en la pantalla de bloqueo") > antes:
                    break
            time.sleep(0.3)
            teclear("clave de Ana")
        if bloqueo == 9:
            if texto.count("[guardian] desbloqueado") < 3:
                time.sleep(0.5)
                continue
            bloqueo = 10
        # H5: la ventana Amiga de la caja (`caja/pruebas68k/ventana.c`). Cuando la caja dice que está en pantalla y el
        # compositor dónde, con CAPTURA_AMIGA=fichero.ppm la pantalla así (sin preguntas encima), y después un clic en su
        # gadget de cerrar, que Intuition dibuja dentro del búfer, arriba a la izquierda (20 x 11: se pulsa en (10, 5)):
        # la ventana recibe IDCMP_CLOSEWINDOW de verdad. Después el cursor vuelve a donde lo tenían los pasos de siempre. Nunca con una
        # pregunta de `guardian` en pantalla, y antes, a que se conteste la de `pide` (sale mientras la ventana está
        # abierta): el clic le quitaría el teclado y su "s" no llegaría. Si no se contesta en 30 s, sin esperarla (la
        # ventana se cierra sola a los 40). Tampoco con el menú de Ajustes abriéndose. Sin esperar dentro de la vuelta.
        pregunta_abierta = texto.count("[guardian] pregunta en pantalla: ") > texto.count("El usuario dice ")
        libre_para_amiga = not bloqueado_ahora and menu != 1 and not pregunta_abierta
        if amiga == 0 and "[caja-amiga] ventana Amiga en pantalla" in texto and not pregunta_abierta:
            import re
            v_amiga = re.search(r"\[compositor\] ventana de caja-amiga en \((\d+), (\d+)\)", texto)
            if v_amiga:
                amiga, amiga_desde = 1, time.time()
                # La captura, enseguida: luego sale Ajustes encima.
                if os.environ.get("CAPTURA_AMIGA"):
                    time.sleep(0.3)
                    mandar("screendump " + os.environ["CAPTURA_AMIGA"])
        elif amiga == 1 and libre_para_amiga and (
                "¿pide puede usar eco? El usuario dice" in texto or time.time() - amiga_desde > 30):
            import re
            amiga = 2
            ir_a(int(v_amiga.group(1)) + 10, int(v_amiga.group(2)) + 5)
            mandar("mouse_button 1", "mouse_button 0", pausa=0.1)
            w = re.search(r"\[compositor\] ventana de ajustes en \((\d+), (\d+)\)", texto)
            if menu == 2:
                ir_a(790, 590)
            elif ajustes and w:
                ir_a(int(w.group(1)) + 196, int(w.group(2)) + 196)
            else:
                ir_a(10 * paso, 5 * paso)
        # La ventana del programa de Dash (`caja/pruebas68k/dash/caja_ventana.dash`), después de la de ventana.exe y de
        # hola.exe: cuando dice que está dibujada, la captura (CAPTURA_DASH=fichero.ppm) y un clic en su gadget de cerrar,
        # que el programa mira con window_check_close() cada décima de segundo. Dónde está: lo último que dice el
        # compositor de una ventana de la caja desde que acabó hola.exe.
        if amiga == 2 and dash_amiga == 0 and libre_para_amiga:
            import re
            corte = texto.find("[caja-amiga] Dash hola.exe acabó")
            if corte >= 0 and "[caja-amiga] dash: ventana abierta y dibujada" in texto[corte:]:
                v_dash = list(re.finditer(r"\[compositor\] ventana de caja-amiga en \((\d+), (\d+)\)", texto[corte:]))
                if v_dash:
                    dash_amiga = 1
                    x, y = int(v_dash[-1].group(1)), int(v_dash[-1].group(2))
                    if os.environ.get("CAPTURA_DASH"):
                        time.sleep(0.5)
                        mandar("screendump " + os.environ["CAPTURA_DASH"])
                        time.sleep(0.5)
                    ir_a(x + 10, y + 5)
                    mandar("mouse_button 1", "mouse_button 0", pausa=0.1)
                    w = re.search(r"\[compositor\] ventana de ajustes en \((\d+), (\d+)\)", texto)
                    if menu == 2:
                        ir_a(790, 590)
                    elif ajustes and w:
                        ir_a(int(w.group(1)) + 196, int(w.group(2)) + 196)
                    else:
                        ir_a(10 * paso, 5 * paso)
        # Mientras el selector espera teclas, no: el clic en Ajustes le quitaría el teclado y sus teclas irían a
        # Ajustes (con el PC lento, Ajustes sale después del selector).
        if not ajustes and "[ajustes] ventana del kit en pantalla" in texto and selector not in (1, 2, 4, 5, 17, 18):
            ajustes = True
            # El clic, a (296, 26) de la esquina de Ajustes (en (400, 130) con la ventana en (104, 104)): a la derecha de su
            # título, donde no hay nada que pulsar (el foco sigue en su campo; a (196, 196) está su Cancelar). Después, el
            # cursor vuelve a (196, 196) de su esquina, como antes. Dónde está la ventana lo dice el compositor (con la
            # barra de la pantalla, S10, la cascada empieza más abajo).
            import re
            v = re.search(r"\[compositor\] ventana de ajustes en \((\d+), (\d+)\)", texto)
            vx, vy = (int(v.group(1)), int(v.group(2))) if v else (104, 104)
            def ir_a(x, y):
                mandar(*["mouse_move -100 -100"] * 12, pausa=0.05)
                pasos = []
                while x > 0 or y > 0:
                    dx, dy = min(x, 100), min(y, 100)
                    pasos.append(f"mouse_move {dx} {dy}")
                    x, y = x - dx, y - dy
                mandar(*pasos, pausa=0.05)
            ir_a(vx + 296, vy + 26)
            mandar("mouse_button 1", "mouse_button 0", pausa=0.1)
            ir_a(vx + 196, vy + 196)
            time.sleep(0.5)
            mandar("sendkey meta_l-up")
            time.sleep(2)
            mandar("sendkey meta_l-down")
            time.sleep(1)
        # El desplegable de Ajustes, cuando su campo ya tiene su "a" (antes, un clic en él se llevaría las "a"): el cursor
        # a la esquina y al desplegable (donde lo dicen el compositor y Ajustes), un clic abre el menú; con
        # CAPTURA_MENU=fichero.ppm, la pantalla así; un clic en la segunda opción la elige. El cursor va después abajo a la
        # derecha: desde (300, 300), los clics de siempre (+10, +5 cada medio segundo) caían en el desplegable, lo abrían
        # y lo cerraban, y la captura final podía salir con el menú abierto.
        # Sin el centro de control encima (sale en (200, 200) y taparía el desplegable): antes de que se anuncie, o ya
        # cerrado. Tampoco con la ventana del selector esperando teclas: también lo taparía.
        sin_centro = "[guardian] el centro de control abrirá" not in texto or "[centro] cerrado" in texto
        # Sin esperar dentro de la vuelta (antes se esperaba hasta 10 s aquí, y con reintentos 30: el guion no atendía
        # nada más y, con el PC lento, Super+L llegaba tarde): menu 1 = clic dado, esperando el menú; 2 = elegida.
        if ajustes and menu == 0 and sin_centro and not bloqueado_ahora and not selector_espera and "[ajustes] el campo dice: DashOSa" in texto:
            import re
            v = re.search(r"\[compositor\] ventana de ajustes en \((\d+), (\d+)\)", texto)
            d = re.search(r"\[ajustes\] desplegable en \((\d+), (\d+)\)", texto)
            if v and d:
                menu = 1
                menu_desde = time.time()
                menus_antes = texto.count("[ajustes] menú abierto; la segunda opción en (")
                ir_a(int(v.group(1)) + int(d.group(1)), int(v.group(2)) + int(d.group(2)))
                mandar("mouse_button 1", "mouse_button 0", pausa=0.1)
                ir_a(790, 590)
        elif menu == 1 and not bloqueado_ahora:
            import re
            abiertos = re.findall(r"\[ajustes\] menú abierto; la segunda opción en \((\d+), (\d+)\)", texto)
            if len(abiertos) > menus_antes:
                v = re.search(r"\[compositor\] ventana de ajustes en \((\d+), (\d+)\)", texto)
                o = abiertos[-1]
                if os.environ.get("CAPTURA_MENU"):
                    mandar("screendump " + os.environ["CAPTURA_MENU"])
                ir_a(int(v.group(1)) + int(o[0]), int(v.group(2)) + int(o[1]))
                mandar("mouse_button 1", "mouse_button 0", pausa=0.1)
                ir_a(790, 590)
                menu = 2
            elif time.time() - menu_desde > 6 and menu_intentos < 3:
                # No se abrió (algo lo tapaba, o el PC va lento): otra vez, hasta tres.
                menu_intentos += 1
                menu = 0
            if menu == 1:
                # Con el menú abierto o por abrirse, nada de lo de siempre: un clic fuera lo cierra sin elegir.
                time.sleep(0.25)
                continue
        paso += 1
        ordenes = ["mouse_move 10 5"]
        if paso >= 15:
            ordenes += ["mouse_button 1", "mouse_button 0"]
        ordenes.append("sendkey a")
        mandar(*ordenes, pausa=0.05)
    except OSError:
        pass
    time.sleep(0.5)
PY
    TECLAS_PID=$!
fi
# Multimonitor: con MULTI=prefijo, `tools/guion_multimonitor.py` (las teclas y el ratón de `make arranque-virt-2`, sin
# el guion de arriba).
if [ -n "${MULTI:-}" ]; then
    python3 -I "$(dirname "$0")/guion_multimonitor.py" "$MONITOR" "$SEGUNDOS" "$SALIDA" "$MULTI" >/dev/null 2>&1 &
    MULTI_PID=$!
fi
# La terminal de la caja Linux (H9): con CAJA_LINUX=prefijo, `tools/guion_caja_linux.py` teclea en ella (y la captura
# en prefijo.ppm y prefijo.png). CAJA_MODO=arch: el guion de Arch Linux ARM (systemd, red por DHCP, pacman).
if [ -n "${CAJA_LINUX:-}" ]; then
    python3 -I "$(dirname "$0")/guion_caja_linux.py" "$MONITOR" "$SEGUNDOS" "$SALIDA" "$CAJA_LINUX" "${CAJA_MODO:-busybox}" >/dev/null 2>&1 &
    CAJA_PID=$!
fi
# Se para en cuanto han salido todas las líneas esperadas, en cualquier orden (o al acabar el tiempo); el orden se
# mira después. Antes se paraba en la última del fichero, y con procesos que acaban a su ritmo (W8, K7) la última en
# el fichero no siempre es la última en salir. Con CAPTURA=fichero.ppm, antes guarda la pantalla (U4).
LINEAS=$(mktemp)
grep -v '^#' "$ESPERADOS" | grep -v '^$' | sed 's/^~//' > "$LINEAS"
todas() {
    local texto
    texto=$(sed -e 's/\x1b\[[0-9;]*[A-Za-z]//g' -e 's/\r//g' "$SALIDA")
    while IFS= read -r l; do
        grep -qF -- "$l" <<< "$texto" || return 1
    done < "$LINEAS"
}
while kill -0 "$QEMU" 2>/dev/null; do
    if todas; then
        sleep 0.5
        if [ -n "${CAPTURA:-}" ]; then
            # Un segundo para que el compositor pinte lo último (tras desbloquear, U3, vuelve a componer todo).
            sleep 1
            python3 -I -c 'import socket,sys,time; s=socket.socket(socket.AF_UNIX); s.connect(sys.argv[1]); s.sendall(("screendump "+sys.argv[2]+"\n").encode()); time.sleep(1)' "$MONITOR" "$CAPTURA"
        fi
        kill "$QEMU" 2>/dev/null
        break
    fi
    sleep 0.5
done
wait "$QEMU" 2>/dev/null
[ -n "${TECLAS_PID:-}" ] && kill "$TECLAS_PID" 2>/dev/null
[ -n "${MULTI_PID:-}" ] && kill "$MULTI_PID" 2>/dev/null
[ -n "${CAJA_PID:-}" ] && kill "$CAJA_PID" 2>/dev/null
# Lo que escribe EDK2 lleva secuencias de escape de terminal: fuera, para comparar el texto.
sed -e 's/\x1b\[[0-9;]*[A-Za-z]//g' -e 's/\r//g' "$SALIDA" > "$SALIDA.txt"

FALLO=0
DESDE=1
while IFS= read -r ESPERADO; do
    [ -z "$ESPERADO" ] && continue
    case "$ESPERADO" in \#*) continue;; esac
    case "$ESPERADO" in
        \~*)
            if ! grep -qF -- "${ESPERADO#\~}" "$SALIDA.txt"; then
                echo "FALTA: ${ESPERADO#\~}"; FALLO=1; break
            fi
            continue;;
    esac
    N=$(tail -n +"$DESDE" "$SALIDA.txt" | grep -nF -m1 -- "$ESPERADO" | cut -d: -f1)
    if [ -z "$N" ]; then
        echo "FALTA: $ESPERADO"
        FALLO=1
        break
    fi
    DESDE=$((DESDE + N))
done < "$ESPERADOS"

if [ "$FALLO" = 0 ]; then
    echo "BIEN: $(basename "$ESPERADOS")"
else
    echo "--- salida del puerto serie (final) ---"
    tail -40 "$SALIDA.txt"
fi
[ -n "${GUARDAR:-}" ] && cp "$SALIDA.txt" "$GUARDAR"
rm -rf "$DISCO" "$SALIDA" "$SALIDA.txt" "$MONITOR" "$LINEAS"
exit $FALLO
