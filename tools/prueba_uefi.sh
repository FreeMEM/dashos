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
timeout "$SEGUNDOS" qemu-system-aarch64 -monitor unix:"$MONITOR",server,nowait -M virt,gic-version=${GIC:-2},acpi=${ACPI:-off}${MAQUINA:+,$MAQUINA} -cpu cortex-a72 -smp 4 -m 2G \
    -bios "$FIRMWARE" -drive file=fat:rw:"$DISCO",format=raw,if=virtio \
    -display none -serial file:"$SALIDA" -net none ${QEMU_EXTRA:-} >/dev/null 2>&1 &
QEMU=$!
# Con TECLAS=1, una "a" y un movimiento del ratón (10, 5) cada medio segundo desde el segundo 3 (U6: el xHCI). Desde
# el paso 15 también un clic (W8): el cursor del compositor, que empieza en (0, 0), ya está en (150, 75), sobre la
# ventana de prueba de arriba, que se lleva el teclado sin cambiar el orden de las ventanas.
# Ajustes sale cuando ya se está tecleando en otra ventana, así que no se lleva el teclado al salir (S8,
# docs/REVISION_LOOP.md). Cuando la salida serie dice que está en pantalla, el cursor va a la esquina y de ahí a
# (300, 300), donde solo está ella; un clic le da el teclado, y Super+Arriba la maximiza y Super+Abajo la devuelve.
# Cuando `guardian` pregunta por un permiso (U4), se contesta: a `pide`, sí; a otro, una "s" antes de tiempo, que no
# debe contar, y después no.
# Cuando `guardian` pone la pantalla de bloqueo (U3), se dejan de mandar esas teclas y se teclea una contraseña mala
# y la buena, con Intro tras cada una; al desbloquear, sigue lo de antes. Igual con la pantalla de entrada y, tras
# ella, con el bloqueo por tiempo: 3 s después de que `guardian` pida el aviso, nada hasta que bloquea.
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

preguntas = 0
selector = 0
entradas_antes = 0
espera_carpeta = 0
centro = 0
panel = 0
bloqueo_capturado = False

while time.time() - inicio < total:
    try:
        texto = open(serie, errors="replace").read()
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
        # S10c: el centro de control. `guardian` avisa 3 s antes de pedirle que abra Permisos: se deja de teclear, para que
        # su ventana se lleve el teclado (S8: 2 s sin teclas). Dentro, flecha abajo (pide · eco) y Supr para quitarlo;
        # luego Tab (a la lista de categorías) y flecha abajo (Seguridad), y Esc para cerrar. Con CAPTURA_CENTRO=fichero.ppm,
        # la ventana con la fila marcada, y en fichero.ppm.seg.ppm, la de Seguridad.
        if centro == 0 and "[guardian] el centro de control abrirá Permisos" in texto:
            centro = 1
        if centro == 1:
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
        if centro == 2:
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
            mandar("sendkey tab")
            time.sleep(0.3)
            mandar("sendkey down")
            reintento = time.time() + 4
        if centro == 5:
            if "[centro] Seguridad, " not in texto:
                if time.time() > reintento:
                    reintento = time.time() + 4
                    mandar("sendkey tab")
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
        if centro == 6:
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
        if centro == 14:
            if "[centro] Comprobar el disco: " not in texto and time.time() < reintento:
                time.sleep(0.25)
                continue
            centro = 11
            if captura_centro:
                time.sleep(0.8)
                mandar("screendump " + captura_centro + ".discos.ppm")
            time.sleep(0.5)
            mandar("sendkey tab", "sendkey down", pausa=0.3)
            reintento = time.time() + 4
        # Pantallas; y otra abajo, Apariencia.
        if centro == 11:
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
        if centro == 8:
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
        if centro == 9:
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
        if centro == 10:
            if "[centro] Apariencia: oscuro, color Azul" not in texto and time.time() < reintento:
                time.sleep(0.25)
                continue
            centro = 7
            time.sleep(0.5)
            mandar("sendkey tab", "sendkey down", pausa=0.3)
            reintento = time.time() + 4
        if centro == 7:
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
        if centro == 12:
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
        if centro == 13:
            if "[centro] vuelve a " not in texto:
                time.sleep(0.25)
                if time.time() < reintento:
                    continue
            centro = 3
            time.sleep(0.5)
            mandar("sendkey esc")
            reintento = time.time() + 4
        if centro == 3:
            if "[centro] cerrado" not in texto:
                if time.time() > reintento:
                    reintento = time.time() + 4
                    mandar("sendkey esc")
                time.sleep(0.25)
                continue
            centro = 4
        # C2: el panel rápido. Cuando `guardian` acaba sus bloqueos (con la sesión bloqueada, Super+A no hace nada): Super+A lo abre; un clic en el mosaico apaga el modo oscuro y
        # otro lo vuelve a encender (lo pone el centro, que lo guarda); "−" y "+" del volumen; Super+A lo cierra. La pantalla mide 800 de ancho.
        # Con CAPTURA_CENTRO, también fichero.panel.ppm (abierto, en oscuro) y fichero.panel-claro.ppm.
        if panel == 0 and "[guardian] bloqueo con la tecla: " in texto:
            panel = 1
            time.sleep(0.5)
            mandar("sendkey meta_l-a")
            reintento = time.time() + 6
        if panel in (1, 2, 3, 4, 5, 6):
            import re
            def ir_panel(x, y):
                mandar(*["mouse_move -100 -100"] * 12, pausa=0.05)
                pasos = []
                while x > 0 or y > 0:
                    dx, dy = min(x, 100), min(y, 100)
                    pasos.append(f"mouse_move {dx} {dy}")
                    x, y = x - dx, y - dy
                mandar(*pasos, pausa=0.05)
                time.sleep(0.3)
                mandar("mouse_button 1", "mouse_button 0", pausa=0.08)
            m = re.search(r"\[panel\] abierto; mosaico a \((\d+), (\d+)\)", texto)
            vol = re.search(r"\[panel\] volumen: menos a \((\d+), (\d+)\), más a \((\d+), (\d+)\)", texto)
            espera = {1: m is not None and vol is not None, 2: "[centro] modo claro (lo pide el panel)" in texto,
                      3: "[centro] modo oscuro (lo pide el panel)" in texto,
                      4: "[sonido] volumen general: 90 % (lo pide panel)" in texto,
                      5: "[sonido] volumen general: 100 % (lo pide panel)" in texto, 6: "[panel] cerrado" in texto}[panel]
            if not espera:
                if time.time() > reintento:
                    panel = 5  # no llegó: las líneas que falten lo dirán
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
            elif panel == 5:
                time.sleep(0.5)
                mandar("sendkey meta_l-a")
            panel += 1
            reintento = time.time() + 6
            # Sin lo de siempre (un clic de más daría otra vuelta al modo) hasta acabar.
            continue
        # U5: la ventana del selector. Mientras está, nada de lo de siempre (un clic podría caer en Cancelar). Primero un
        # Intro sin nada elegido, que no debe abrir nada; luego flecha abajo (el primer cajón, Privado) e Intro para
        # entrar; dentro, Fin (el último, nota.txt; el primero es "../") e Intro.
        if selector == 0 and "[selector] ventana de abrir en pantalla" in texto:
            selector = 1
            time.sleep(0.5)
            mandar("sendkey ret")
            time.sleep(0.5)
            # Dos abajo: los cajones van primero, y `Prefs` (S10c) va antes que `Privado`.
            mandar("sendkey down")
            time.sleep(0.3)
            mandar("sendkey down")
            time.sleep(0.3)
            mandar("sendkey ret")
            reintento = time.time() + 4
        if selector == 1:
            if "[selector] entra en C/Privado" not in texto:
                # Si una pregunta de `guardian` (que toma el teclado) se llevó las teclas, otra vez (Inicio: el primero).
                if time.time() > reintento:
                    reintento = time.time() + 4
                    mandar("sendkey home")
                    time.sleep(0.3)
                    mandar("sendkey down")
                    time.sleep(0.3)
                    mandar("sendkey ret")
                time.sleep(0.25)
                continue
            # Dentro, Retroceso sube a C; y otra vez abajo, abajo e Intro para volver a entrar.
            if selector == 1 and "[selector] sube a C," not in texto:
                time.sleep(0.5)
                mandar("sendkey backspace")
                selector = 17
                reintento = time.time() + 4
        if selector == 17:
            if "[selector] sube a C," not in texto and time.time() < reintento:
                time.sleep(0.25)
                continue
            time.sleep(0.5)
            mandar("sendkey down", "sendkey down", "sendkey ret", pausa=0.3)
            selector = 18
            reintento = time.time() + 4
        if selector == 18:
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
        if selector == 2:
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
        # U5: la carpeta para guardar (el hostil pide "carpeta"): abajo dos veces (Privado), Intro para entrar y, ya
        # dentro, clic en "Elegir esta carpeta" (los botones del kit no toman el teclado).
        # El hostil la pide justo después de lo de antes: hasta que sale, nada de lo de siempre (un clic caería en Cancelar).
        if selector == 3 and "[selector] ventana de elegir carpeta en pantalla" not in texto and time.time() < espera_carpeta:
            time.sleep(0.25)
            continue
        if selector == 3 and "[selector] ventana de elegir carpeta en pantalla" in texto:
            selector = 4
            # Las veces que ya entró en C/Privado (el flujo de antes también entra): hace falta una más.
            entradas_antes = texto.count("[selector] entra en C/Privado")
            time.sleep(0.5)
            mandar("sendkey down")
            time.sleep(0.3)
            mandar("sendkey down")
            time.sleep(0.3)
            mandar("sendkey ret")
            reintento = time.time() + 4
        if selector == 4:
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
        if selector == 5:
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
        # Y el bloqueo por tiempo (U3): 3 s más de teclas tras pedir `guardian` el aviso, y luego nada hasta que bloquea;
        # entonces, una contraseña mala y la de Ana.
        if bloqueo == 4 and "[guardian] esperando " in texto:
            bloqueo, quieto = 5, time.time() + 3
        if bloqueo == 5 and time.time() >= quieto:
            if texto.count("pantalla de bloqueo pintada") < 2:
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
            if texto.count("pantalla de bloqueo pintada") < 3:
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
        if not ajustes and "[ajustes] ventana del kit en pantalla" in texto:
            ajustes = True
            mandar(*["mouse_move -100 -100"] * 12, pausa=0.05)
            mandar(*["mouse_move 100 100"] * 3, pausa=0.05)
            mandar("mouse_button 1", "mouse_button 0", pausa=0.1)
            time.sleep(0.5)
            mandar("sendkey meta_l-up")
            time.sleep(2)
            mandar("sendkey meta_l-down")
            time.sleep(1)
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
