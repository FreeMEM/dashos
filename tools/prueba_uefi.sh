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
# debe contar, y después no. S28: a `actualiza.exe` de la caja Amiga, "e" (solo esta vez) y luego "s" (siempre).
# Cuando `guardian` pone la pantalla de bloqueo (U3), se dejan de mandar esas teclas y se teclea una contraseña mala
# y la buena, con Intro tras cada una; al desbloquear, sigue lo de antes. Igual con la pantalla de entrada y, tras
# ella, con el bloqueo por tiempo: 3 s después de que `guardian` pida el aviso, nada hasta que bloquea.
# U6, el cambio rápido de usuario: tras entrar Ana, en las dos pantallas de bloqueo siguientes, Esc y otra persona
# (Carla, y luego otra vez Ana) con su contraseña. Con CAPTURA_CAMBIO=fichero.ppm, la primera de esas dos pantallas
# (fichero.ppm) y la de "Cambiar de usuario" con el nombre ya puesto (fichero.nombre.ppm).
# U6, paso 2: con CAPTURA_U6=prefijo, la pantalla con la ventana de `cambio` de Ana delante (prefijo.ana.ppm) y la de la
# sesión de Carla, con Ana detrás (prefijo.carla.ppm): ahí no tiene que estar (lo mira `tools/captura_cambio.py`).
# Con REINICIO=fichero (make arranque-virt): cuando ya salió todo lo esperado, el guion pulsa Reiniciar en el menú de
# apagado del panel (decisión del usuario del 2026-11-13) y se espera a que la máquina vuelva a arrancar (otro "DashOS en
# marcha") y a las líneas de ese fichero, en orden. La señal para el guion es un fichero junto a la salida.
export REINICIO_SENAL="${REINICIO:+$SALIDA.reiniciar}"
if [ -n "${TECLAS:-}" ]; then
    ESPERADOS_GUION="$ESPERADOS" python3 -I - "$MONITOR" "$SEGUNDOS" "$SALIDA" "${CAPTURA_PREGUNTA:-}" "${DIAG_REGISTROS:-}" "${CAPTURA_SELECTOR:-}" "${CAPTURA_CENTRO:-}" <<'PY' >/dev/null 2>"${GUION_ERRORES:-/dev/null}" &
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

def cerrar_amiga(x, y, m):
    # Dónde se pulsa para cerrar una ventana Amiga en (x, y) de la pantalla. Con la barra de DashOS (la caja dice
    # "con la barra de DashOS"), su botón de cerrar: encima de la ventana (40 de alto), a 4 + 16 del borde derecho
    # (`wayland/src/decoracion.rs`). Si no, el gadget de cerrar de intuition, arriba a la izquierda (20 x 11).
    if m and m.group(3):
        return (x + int(m.group(1)) * int(m.group(2) or 1) - 4 - 16, y - 20)
    return (x + 10, y + 5)

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
ex = 0
ex_mov = {}
bloqueo_capturado = False
cambio = 0
captura_u6_hecha = 0
amiga = 0
amiga_desde = 0
v_amiga = None
dash_amiga = 0
dash_nativo = 0
dash_desde = 0
dash_intentos = 0
dash_entradas = 0
dash_fila_n = 2
dash_subidas = 0
gt_amiga = 0
gt_dash = 0
ra_capturas = 0
gt_mirado = 0
gt_movida = 0
req_dash = 0
# El selector de ficheros desde un programa del disco (`Programas/guarda`): lo último de la prueba. Sus líneas esperadas
# (las que llevan alguna de estas marcas) y las demás, que tienen que haber salido antes de empezar.
MARCAS_GUARDA = ("[guarda]", "para guarda", "Programas/guarda", "[selector] guarda ", "el usuario guarda ", "informe.txt", "el nombre del campo",
                 # S10, Commodities: su atajo se pulsa tras guarda (abajo), así que guarda no las espera.
                 "[commodity-prueba]", "commodity Prueba se va")
# S10, Commodities: cuántas veces se pulsó Ctrl+Alt+K (una por cada "pulsa" de la prueba, hasta 6).
commodity_pulsadas = 0
guarda_paso = 0
guarda_desde = 0
guarda_mirado = 0
guarda_abiertos = 0
guarda_suyas, guarda_otras = [], []
if os.environ.get("ESPERADOS_GUION"):
    try:
        for l in open(os.environ["ESPERADOS_GUION"], errors="replace").read().splitlines():
            if l and not l.startswith("#"):
                l = l[1:] if l.startswith("~") else l
                (guarda_suyas if any(m in l for m in MARCAS_GUARDA) else guarda_otras).append(l)
    except OSError:
        pass
reinicio = 0
reinicio_hasta = 0
reinicio_abiertos = 0
reinicio_menus = 0

while time.time() - inicio < total:
    try:
        texto = open(serie, errors="replace").read()
        # Con ESTADO_GUION=fichero: el estado del guion cada 2 s (para ver en qué paso se queda).
        if os.environ.get("ESTADO_GUION") and time.time() - globals().get("estado_dicho", 0) > 2:
            estado_dicho = time.time()
            with open(os.environ["ESTADO_GUION"], "a") as f:
                f.write(f"{time.time() - inicio:.0f} s: paso {paso} ajustes {ajustes} menu {menu} selector {selector} preguntas {preguntas} centro {centro} bloqueo {bloqueo} cambio {cambio} panel {panel} barra {barra} avisos {avisos_paso} caja {amiga}/{dash_amiga}/{gt_amiga}/{gt_dash}/{req_dash}\n")
        # Con REINICIO: cuando ya salió todo lo esperado (la señal), Super+A, el botón de apagado del panel y Reiniciar.
        # Con otras sesiones abiertas (Carla sigue detrás de Ana), `guardian` lo pregunta: S, pasado su segundo. Lo demás
        # del guion ya no se hace (sus clics caerían en el panel).
        senal = os.environ.get("REINICIO_SENAL")
        if senal and os.path.exists(senal):
            import re
            def clic_panel(x, y):
                # Los sitios, como en el paso del panel: desde la esquina de arriba a la derecha, bajo la barra.
                capa = re.findall(r"\[compositor\] capa de panel en \((\d+), (\d+)\)", texto)
                y += int(capa[-1][1]) - 8 if capa else 0
                ir_a(800 - x, y)
                time.sleep(0.3)
                mandar("mouse_button 1", "mouse_button 0", pausa=0.08)
            if reinicio == 0:
                reinicio_abiertos = texto.count("[panel] botón de apagado a (")
                mandar("sendkey meta_l-a")
                reinicio, reinicio_hasta = 1, time.time() + 8
            elif reinicio == 1:
                b = re.findall(r"\[panel\] botón de apagado a \((\d+), (\d+)\)", texto)
                if len(b) > reinicio_abiertos:
                    time.sleep(0.5)
                    reinicio_menus = texto.count("[panel] menú de apagado: ")
                    clic_panel(int(b[-1][0]), int(b[-1][1]))
                    reinicio, reinicio_hasta = 2, time.time() + 8
                elif time.time() > reinicio_hasta:
                    # No se abrió (o ya estaba abierto y Super+A lo cerró): otra vez.
                    reinicio = 0
            elif reinicio == 2:
                m = re.findall(r"\[panel\] menú de apagado: .*reiniciar \((\d+), (\d+)\)", texto)
                if len(m) > reinicio_menus:
                    time.sleep(0.5)
                    clic_panel(int(m[-1][0]), int(m[-1][1]))
                    reinicio = 3
                elif time.time() > reinicio_hasta:
                    mandar("sendkey meta_l-a")
                    time.sleep(1)
                    reinicio = 0
            elif reinicio == 3 and "[guardian] confirmación en pantalla: ¿reiniciar?" in texto:
                # Sí pide Mayúsculas y S, como los permisos.
                time.sleep(1.3)
                mandar("sendkey shift-s")
                reinicio = 4
            time.sleep(0.25)
            continue
        # "Guardar como" desde un programa del disco: cuando ya salió todo lo demás que se espera (así no se cruza con
        # los otros pasos ni con el selector del hostil o de dash-ventana), el lanzador de la barra (1), "guarda" (2) e
        # Intro (3): `sesiones` lo lanza como Ana, con la llave del selector. Pide guardar "informe.txt": un clic en
        # "Guardar" (4). Pide otra vez: clic en el campo, Fin, se borra el nombre e Intro (5: vacío no vale); se teclea
        # otra vez e Intro (6: ya existe, se pregunta) y un clic en "Sustituir" (7). Y pide abrir con un patrón: un
        # clic en la ventana, Fin e Intro (8). Cada paso, como mucho 40 s.
        if guarda_paso == 0 and guarda_suyas and time.time() - guarda_mirado > 2:
            import re
            guarda_mirado = time.time()
            limpio = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", texto).replace("\r", "")
            guarda_otras = [l for l in guarda_otras if l not in limpio]
            if not guarda_otras:
                guarda_paso, guarda_desde = 1, time.time()
        if 1 <= guarda_paso <= 8:
            import re
            if time.time() - guarda_desde > 40:
                guarda_paso = 99                 # (no siguió: faltarán sus líneas)
                continue
            def clic_guarda(x, y):
                ir_a(x, y)
                time.sleep(0.3)
                mandar("mouse_button 1", "mouse_button 0", pausa=0.1)
                ir_a(790, 590)
            def en_selector(patron):
                # El último sitio que dijo el selector (dentro de su ventana), ya en la pantalla.
                v = re.findall(r"\[compositor\] ventana de selector en \((\d+), (\d+)\)", texto)
                s = re.findall(patron, texto)
                return (int(v[-1][0]) + int(s[-1][0]), int(v[-1][1]) + int(s[-1][1])) if v and s else None
            con_nombre = texto.count("[selector] campo Nombre en (") - texto.count("), con 0 letras")
            hecho = False
            if guarda_paso == 1:
                bb = re.search(r"\[barra\] botones: lanzador en \((\d+), (\d+)\)", texto)
                if bb:
                    guarda_abiertos = texto.count("[barra] lanzador abierto")
                    clic_guarda(int(bb.group(1)), int(bb.group(2)))
                    hecho = True
            elif guarda_paso == 2 and texto.count("[barra] lanzador abierto") > guarda_abiertos:
                time.sleep(0.5)
                mandar(*["sendkey " + c for c in "guarda"], pausa=0.3)
                hecho = True
            elif guarda_paso == 3 and '[barra] busca "guarda": guarda' in texto:
                time.sleep(0.5)
                mandar("sendkey ret")
                hecho = True
            elif guarda_paso == 4 and con_nombre >= 1:
                time.sleep(1)
                sitio = en_selector(r"\[selector\] botón Guardar en \((\d+), (\d+)\)")
                if sitio:
                    clic_guarda(*sitio)
                    hecho = True
            elif guarda_paso == 5 and con_nombre >= 2 and "[guarda] guardar: " in texto:
                time.sleep(1)
                sitio = en_selector(r"\[selector\] campo Nombre en \((\d+), (\d+)\)")
                if sitio:
                    clic_guarda(*sitio)
                    time.sleep(0.5)
                    mandar("sendkey end", *["sendkey backspace"] * 11, "sendkey ret", pausa=0.2)
                    hecho = True
            elif guarda_paso == 6 and "[selector] el nombre del campo no vale" in texto:
                time.sleep(0.5)
                mandar(*["sendkey " + ("dot" if c == "." else c) for c in "informe.txt"], "sendkey ret", pausa=0.2)
                hecho = True
            elif guarda_paso == 7 and "[selector] botón Sustituir en (" in texto:
                time.sleep(0.7)
                sitio = en_selector(r"\[selector\] botón Sustituir en \((\d+), (\d+)\)")
                if sitio:
                    clic_guarda(*sitio)
                    hecho = True
            elif guarda_paso == 8 and "[selector] ventana de abrir en pantalla, para guarda" in texto:
                time.sleep(1)
                v = re.findall(r"\[compositor\] ventana de selector en \((\d+), (\d+)\)", texto)
                if v:
                    # Un clic en el título de la ventana (no en la lista), para que tenga el teclado.
                    clic_guarda(int(v[-1][0]) + 230, int(v[-1][1]) + 36)
                    time.sleep(0.5)
                    mandar("sendkey end", "sendkey ret", pausa=0.4)
                    hecho = True
            if hecho:
                guarda_paso, guarda_desde = guarda_paso + 1, time.time()
            time.sleep(0.25)
            continue
        # S10, Commodities: acabado guarda (o sin él), cada vez que `commodity-prueba` pide su atajo, Ctrl+Alt+K. Si el
        # atajo funciona no llega a ninguna ventana; si no, a la que tenga el teclado (y la prueba lo dirá).
        if (guarda_paso >= 9 or not guarda_suyas) and commodity_pulsadas < 6 and texto.count("[commodity-prueba] pulsa Ctrl+Alt+K") > commodity_pulsadas:
            commodity_pulsadas += 1
            time.sleep(0.5)
            mandar("sendkey ctrl-alt-k")
        # Los pasos de las capturas de la caja Amiga (GadTools, el ejemplo de Dash y el requester), al principio de la
        # vuelta: los pasos de más abajo esperan con "continue" y no se llegaría a ellos.
        libre_caja = texto.count("[guardian] pregunta en pantalla: ") <= texto.count("El usuario dice ")
        # Ni con la sesión bloqueada: la captura saldría de la pantalla de bloqueo (lo mismo que `bloqueado_ahora`, abajo).
        bloqueo_caja = max(texto.rfind("[guardian] sesión bloqueada"), texto.rfind("pantalla de bloqueo pintada"),
                           texto.rfind("pantalla de entrada pintada"))
        libre_caja = libre_caja and bloqueo_caja <= max(texto.rfind("[guardian] desbloqueado"), texto.rfind("[guardian] entra "))
        # Las ventanas de Dash en DashOS (kit-c): el programa de Dash `dash-ventana` (`kit-c/pruebas/ventana.dash`) dice
        # cuándo está en pantalla y dónde están sus botones, y el compositor dónde está la ventana. Con CAPTURA_DASHOS=
        # fichero.ppm, la pantalla así (tras un clic en su margen, que la trae delante); luego un clic en su botón
        # "Añadir" (1). El programa pregunta si guardar (mui_request): con la captura (fichero-pregunta.ppm), un clic en
        # "Guardar" (2); guarda en su cajón y pide al selector un fichero: doble clic en Privado y en nota.txt (3 y 4); y
        # una carpeta: "Elegir esta carpeta", C (5). Luego acaba (6). Cuando `guardian` acabó su prueba (sus bloqueos se
        # llevarían los clics y las teclas), tras el selector del hostil y el desplegable de Ajustes, sin el centro de
        # control, sin preguntas ni bloqueo. Al principio de la vuelta: mientras dura, los pasos de abajo esperan (con
        # "continue"), y cada paso, como mucho 30 s.
        import re
        centro_fuera = "[guardian] el centro de control abrirá" not in texto or "[centro] cerrado" in texto
        # Si el clic no llegó (una pregunta de `guardian` salió encima mientras), otra vez, hasta tres.
        if dash_nativo == 1 and "[dash-ventana] pulsado" not in texto and time.time() - dash_desde > 8 \
                and dash_intentos < 3:
            dash_nativo = 0
        # Tampoco con la ventana Amiga de H5 esperando su clic: delante, la de Dash la taparía.
        amiga_fuera = amiga >= 2 or "[caja-amiga] ventana Amiga en pantalla" not in texto
        if dash_nativo == 0 and menu == 2 and centro_fuera and amiga_fuera and libre_caja and \
                "[guardian] prueba acabada" in texto and "[hostil] selector:" in texto and \
                selector not in (1, 2, 4, 5, 17, 18):
            d = re.search(r'\[dash-ui\] ventana "Dash (?:en|on) DashOS" en pantalla \((\d+)x\d+\).*?"Añadir" en \((\d+), (\d+)\)', texto)
            v = re.search(r"\[compositor\] ventana de dash-ventana en \((\d+), (\d+)\)", texto)
            if d and v:
                dash_nativo, dash_desde, dash_intentos = 1, time.time(), dash_intentos + 1
                vx, vy = int(v.group(1)), int(v.group(2))
                # Delante primero: un clic en su margen de arriba a la derecha (sale detrás de Ajustes y de otras).
                ir_a(vx + int(d.group(1)) - 12, vy + 10)
                mandar("mouse_button 1", "mouse_button 0", pausa=0.1)
                time.sleep(0.5)
                if os.environ.get("CAPTURA_DASHOS"):
                    ir_a(790, 590)
                    time.sleep(0.5)
                    mandar("screendump " + os.environ["CAPTURA_DASHOS"])
                    time.sleep(0.5)
                ir_a(vx + int(d.group(2)), vy + int(d.group(3)))
                mandar("mouse_button 1", "mouse_button 0", pausa=0.1)
                ir_a(790, 590)
                continue
        if dash_nativo in (1, 2, 3, 31, 4, 5) and time.time() - dash_desde > 30:
            dash_nativo = 9                      # (no siguió: faltarán sus líneas)
        if dash_nativo == 1 and "[dash-ventana] pulsado" in texto:
            # La pregunta: su ventana es la última de dash-ventana que pone el compositor.
            dp = re.search(r'\[dash-ui\] ventana "Guardar" en pantalla \(\d+x\d+\); "Guardar" en \((\d+), (\d+)\)', texto)
            vs = re.findall(r"\[compositor\] ventana de dash-ventana en \((\d+), (\d+)\)", texto)
            if dp and len(vs) >= 2:
                time.sleep(0.5)
                if os.environ.get("CAPTURA_DASHOS"):
                    mandar("screendump " + os.environ["CAPTURA_DASHOS"].replace(".ppm", "-pregunta.ppm"))
                    time.sleep(0.5)
                ir_a(int(vs[-1][0]) + int(dp.group(1)), int(vs[-1][1]) + int(dp.group(2)))
                mandar("mouse_button 1", "mouse_button 0", pausa=0.1)
                ir_a(790, 590)
                dash_nativo, dash_desde = 2, time.time()
            time.sleep(0.25)
            continue
        # El selector, para dash-ventana: dónde está (el compositor) y su lista (él). Doble clic en la fila de Privado y,
        # dentro, en la 2 (nota.txt; la primera es "../"). Las filas miden 56. Privado es la tercera (tras Fondos y
        # Prefs) si en C no hay más cajones: si entra en otro, Retroceso y la fila siguiente.
        def dash_fila(fila):
            vs = re.findall(r"\[compositor\] ventana de selector en \((\d+), (\d+)\)", texto)
            ls = re.findall(r"\[selector\] lista en \((\d+), (\d+)\)", texto)
            if not vs or not ls:
                return False
            ir_a(int(vs[-1][0]) + int(ls[-1][0]) + 60, int(vs[-1][1]) + int(ls[-1][1]) + 28 + 56 * fila)
            time.sleep(0.3)
            mandar("mouse_button 1", "mouse_button 0", "mouse_button 1", "mouse_button 0", pausa=0.08)
            ir_a(790, 590)
            return True
        if dash_nativo == 2 and "[selector] ventana de abrir en pantalla, para dash-ventana" in texto:
            time.sleep(0.8)
            dash_entradas, dash_fila_n = texto.count("[selector] entra en C/"), 2
            if dash_fila(dash_fila_n):
                dash_nativo, dash_desde = 3, time.time()
            continue
        if dash_nativo == 3 and texto.count("[selector] entra en C/") > dash_entradas:
            time.sleep(0.8)
            if re.findall(r"\[selector\] entra en (C/[^,]+),", texto)[-1] == "C/Privado":
                if dash_fila(1):
                    dash_nativo, dash_desde = 4, time.time()
            elif dash_fila_n < 8:
                dash_subidas = texto.count("[selector] sube a C,")
                mandar("sendkey backspace")
                dash_nativo = 31
            continue
        if dash_nativo == 31 and texto.count("[selector] sube a C,") > dash_subidas:
            time.sleep(0.8)
            dash_entradas, dash_fila_n = texto.count("[selector] entra en C/"), dash_fila_n + 1
            if dash_fila(dash_fila_n):
                dash_nativo = 3
            continue
        if dash_nativo == 4 and "[selector] ventana de elegir carpeta en pantalla, para dash-ventana" in texto:
            time.sleep(0.8)
            vs = re.findall(r"\[compositor\] ventana de selector en \((\d+), (\d+)\)", texto)
            bs = re.findall(r"\[selector\] botón Elegir en \((\d+), (\d+)\)", texto)
            if vs and bs:
                ir_a(int(vs[-1][0]) + int(bs[-1][0]), int(vs[-1][1]) + int(bs[-1][1]))
                time.sleep(0.3)
                mandar("mouse_button 1", "mouse_button 0", pausa=0.08)
                ir_a(790, 590)
                dash_nativo, dash_desde = 5, time.time()
            continue
        if dash_nativo == 5 and "[dash-ventana] fin" in texto:
            dash_nativo = 6
        if dash_nativo in (2, 3, 31, 4, 5):
            # (El programa acaba y su ventana se va: hasta entonces, nada más: taparía lo que se pulse.)
            time.sleep(0.25)
            continue
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
                    m_f = list(re.finditer(r"ventana Amiga en pantalla \(\d+x\d+(?:, a (\d+)x)?(?:, con la barra de DashOS)?\)", texto[corte:]))
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
                    # (A 3 del borde: la marca de la caja Amiga, S29, ocupa los 3 píxeles de fuera.)
                    vistos = pixeles(foto, [(x + 3 * f_gt, y + 3 * f_gt), (x + 406 * f_gt, y + 3 * f_gt), (x + 3 * f_gt, y + 188 * f_gt), (x + 406 * f_gt, y + 188 * f_gt)])
                    vacio = pixeles(foto, [(x + 200 * f_gt, y + 183 * f_gt)])
                    # Con la barra de DashOS (rama amiga-integrada): las esquinas de abajo las redondea el compositor y
                    # arriba no hay barra de intuition. Se miran un poco más dentro, y la barra de DashOS encima.
                    con_barra = bool(m_f) and "con la barra de DashOS" in m_f[-1].group(0)
                    if con_barra:
                        vistos = pixeles(foto, [(x + 6 * f_gt, y + 1 * f_gt), (x + 403 * f_gt, y + 1 * f_gt), (x + 6 * f_gt, y + 186 * f_gt), (x + 403 * f_gt, y + 186 * f_gt)])
                        barra_d = pixeles(foto, [(x + 100 * f_gt, y - 30), (x + 300 * f_gt, y - 30)])
                    ancho_pantalla = 0
                    try:
                        ancho_pantalla = int(open(foto, "rb").read().split(b"\n", 3)[1].split()[0])
                    except Exception:
                        pass
                    clasica = len(vistos) == 4 and all(c in wb for c in vistos) and vacio == [(0xAA, 0xAA, 0xAA)]
                    # Con el aspecto de DashOS (S26): las dos esquinas de arriba, de la barra; las de abajo y el sitio
                    # vacío, de la superficie (otra ventana encima no da eso).
                    # Y la casilla marcada, del primario: no es la barra ni la superficie (una ventana del kit encima
                    # tiene la misma superficie). En (100, 21), dentro de la casilla y fuera de la marca blanca (en
                    # (103, 25), donde se pulsa, ya está la marca).
                    casilla = pixeles(foto, [(x + 100 * f_gt, y + 21 * f_gt)])
                    material = len(vistos) == 4 and len(vacio) == 1 and vistos[0] == vistos[1] != vistos[2] \
                        and vistos[2] == vistos[3] == vacio[0] and len(casilla) == 1 and casilla[0] not in (vistos[0], vacio[0]) \
                        and max(casilla[0]) - min(casilla[0]) > 40
                    if con_barra:
                        # Las cuatro esquinas y el sitio vacío, de la superficie; la barra de DashOS, de otro color (el
                        # contenedor) e igual en los dos sitios; la casilla, del primario.
                        material = len(vistos) == 4 and len(vacio) == 1 and len(set(vistos + vacio)) == 1 \
                            and len(barra_d) == 2 and barra_d[0] == barra_d[1] != vacio[0] \
                            and len(casilla) == 1 and casilla[0] != vacio[0] and max(casilla[0]) - min(casilla[0]) > 40
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
                        # En una pantalla grande y con la barra de DashOS, se arrastra por su barra hasta la derecha de
                        # Ajustes (en la cascada quedan una encima de la otra): así la captura con el menú las enseña a
                        # las dos, una al lado de la otra.
                        if con_barra and ancho_pantalla >= 1600:
                            w_aj = re.search(r"\[compositor\] ventana de ajustes en \((\d+), (\d+)\)", texto)
                            destino = (int(w_aj.group(1)) + 360 + 40) if w_aj else 600
                            if destino > x:
                                ir_a(x + 200 * f_gt, y - 20)
                                time.sleep(0.3)
                                mandar("mouse_button 1", pausa=0.3)
                                dx_t = destino - x
                                while dx_t > 0:
                                    mandar(f"mouse_move {min(dx_t, 50)} 0", pausa=0.08)
                                    dx_t -= 50
                                mandar("mouse_button 0", pausa=0.5)
                                x = destino
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
        # BOOPSI y ReAction (`caja/pruebas68k/boopsi.c`, solo con CAJA_INTERACTIVA=1): sus dos ventanas de ReAction, la
        # de "como DashOS" (boopsi-20) y la original (boopsi-orig-20, con la excepción de DASHOS_PREFS): una captura de
        # cada una (CAPTURA_RA.ppm y CAPTURA_RA.orig.ppm) con su sitio y su tamaño al lado (.txt: x y ancho alto
        # escala), para ponerlas lado a lado. Se cierran solas a los 20 s.
        if libre_caja and ra_capturas < 2 and os.environ.get("CAPTURA_RA"):
            import re
            nombre_ra = "boopsi-20" if ra_capturas == 0 else "boopsi-orig-20"
            corte = texto.find("[caja-amiga] " + nombre_ra + ": ")
            if corte >= 0:
                m_ra = list(re.finditer(r"ventana Amiga en pantalla \((\d+)x(\d+)(?:, a (\d+)x)?(?:, con la barra de DashOS)?\): ReAction", texto[corte:]))
                v_ra = list(re.finditer(r"\[compositor\] ventana de caja-amiga en \((\d+), (\d+)\)", texto[corte:]))
                if m_ra and v_ra:
                    # Delante de las demás: un clic dentro, en el margen del grupo (nada que pulsar ahí).
                    time.sleep(1.5)
                    f_ra = int(m_ra[-1].group(3) or 1)
                    ir_a(int(v_ra[-1].group(1)) + 6 * f_ra, int(v_ra[-1].group(2)) + 16 * f_ra)
                    time.sleep(0.3)
                    mandar("mouse_button 1", "mouse_button 0", pausa=0.15)
                    time.sleep(1.0)
                    foto = os.environ["CAPTURA_RA"] + (".orig" if ra_capturas else "") + ".ppm"
                    mandar("screendump " + foto)
                    with open(foto + ".txt", "w") as f:
                        f.write(f"{v_ra[-1].group(1)} {v_ra[-1].group(2)} {m_ra[-1].group(1)} {m_ra[-1].group(2)} {m_ra[-1].group(3) or 1}\n")
                    ra_capturas += 1
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
                    m_f = list(re.finditer(r"ventana Amiga en pantalla \(\d+x\d+(?:, a (\d+)x)?(?:, con la barra de DashOS)?\)", texto[corte:]))
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
                m_r = re.search(r"ventana Amiga en pantalla \((\d+)x(\d+)(?:, a (\d+)x)?(?:, con la barra de DashOS)?\): Dash", texto[corte:])
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
        # S36, decisión 1 (rama admin-desarrollo): la tanda de acciones de administrador del guion del Shell: `guardian`
        # enseña la lista y pide la contraseña. A las tres primeras, la de Ana; a la cuarta (la de la prueba hostil), una
        # mala.
        tandas_en = texto.count("[guardian] tanda en pantalla: ")
        if tandas_en > globals().get("tandas_vistas", 0):
            globals()["tandas_vistas"] = globals().get("tandas_vistas", 0) + 1
            time.sleep(1.3)
            # S6b, rama cuentas: la que crea una cuenta pide además su contraseña (Intro en una caja vacía pasa a la
            # siguiente). A Berta, la de Ana y la suya; a Dora, primero una mala de administrador, luego la buena de
            # Carla (que no lo es) y luego la de Ana con una corta para Dora: ninguna se crea.
            import re
            linea_t = [l for l in texto.splitlines() if "[guardian] tanda en pantalla: " in l][tandas_vistas - 1]
            nueva = re.search(r"\(y la contraseña de la cuenta nueva (\w+)\)", linea_t)
            if nueva and nueva.group(1) == "Dora":
                globals()["doras"] = globals().get("doras", 0) + 1
                if doras == 1:
                    teclear("no es esta")
                    teclear("clave de Dora")
                elif doras == 2:
                    mandar("sendkey shift-tab", "sendkey backspace", "sendkey backspace", "sendkey backspace", pausa=0.15)
                    teclear("Carla")
                    teclear("clave de Carla")
                    teclear("clave de Dora")
                else:
                    teclear("clave de Ana")
                    teclear("corta")
            elif nueva:
                if os.environ.get("CAPTURA_CUENTAS"):
                    mandar("screendump " + os.environ["CAPTURA_CUENTAS"] + ".tanda.ppm")
                    time.sleep(0.5)
                teclear("clave de Ana")
                teclear("clave de " + nueva.group(1))
            else:
                teclear("no es esta" if tandas_vistas == 4 else "clave de Ana")
            time.sleep(0.5)
            continue
        # S6b, rama cuentas: el código de recuperación de la cuenta nueva, en la ventana del sistema: Intro (con
        # CAPTURA_CUENTAS, antes, su captura).
        codigos_en = texto.count("[guardian] código de recuperación en pantalla: ")
        if codigos_en > globals().get("codigos_vistos", 0):
            globals()["codigos_vistos"] = globals().get("codigos_vistos", 0) + 1
            time.sleep(1.3)
            if os.environ.get("CAPTURA_CUENTAS"):
                mandar("screendump " + os.environ["CAPTURA_CUENTAS"] + ".codigo.ppm")
                time.sleep(0.5)
            mandar("sendkey ret")
            time.sleep(0.5)
            continue
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
            # S28: la guardia de ficheros de la caja Amiga. A `actualiza.exe` (un programa honrado que actualiza
            # otro), "E" (solo esta vez) la primera vez y "S" (siempre) la segunda; a los virus de laboratorio, "N". Con
            # CAPTURA_PREGUNTA_CAJA=fichero.ppm, la primera de actualiza.
            caja_actualiza = " quiere cambiar " in linea and "actualiza.exe" in linea
            if caja_actualiza:
                globals()["actualiza_vistas"] = globals().get("actualiza_vistas", 0) + 1
                if actualiza_vistas == 1 and os.environ.get("CAPTURA_PREGUNTA_CAJA"):
                    time.sleep(0.3)
                    mandar("screendump " + os.environ["CAPTURA_PREGUNTA_CAJA"])
            # Las de la caja, sin la "s" a destiempo: con el guion lento (los movimientos del ratón de otros pasos tardan),
            # llegaba pasado el primer segundo y valía como "siempre". Y la tecla, solo si la pregunta sigue en pantalla:
            # si no, se la llevaría la ventana de debajo (Ajustes, que espera su texto).
            def sigue_preguntando():
                t = open(serie, errors="replace").read()
                return t.count("[guardian] pregunta en pantalla: ") > t.count("El usuario dice ")
            if "¿pide " in linea:
                time.sleep(1.3)
                mandar("sendkey shift-s")
            elif "pregunta en pantalla: cambiar permiso: " in linea:
                # S6b, S36: la orden `Permiso` del Shell: a `pide`, que sí; a otro, que no. La primera vez, antes, la D
                # (los detalles: el nombre interno del permiso).
                time.sleep(1.3)
                globals()["cambios_vistos"] = globals().get("cambios_vistos", 0) + 1
                if cambios_vistos == 1 and sigue_preguntando():
                    mandar("sendkey d")
                    time.sleep(0.5)
                if sigue_preguntando():
                    # S6b, S36: y a `espera`, el modo de desarrollo (dar y quitar).
                    mandar("sendkey " + ("shift-s" if " a pide " in linea or " a espera " in linea else "n"))
            elif " quiere cambiar " in linea:
                time.sleep(1.3)
                if sigue_preguntando():
                    mandar("sendkey " + ("n" if not caja_actualiza else "e" if actualiza_vistas == 1 else "shift-s"))
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
            # Con todo lo que hace falta para el clic ya leído: el centro dice dónde están la lista y el botón, y el
            # compositor, dónde está la ventana, cada uno por su lado. Si se seguía con solo lo primero, a veces la línea
            # del compositor aún no había llegado y se quitaba con Supr en vez de con el botón (fallaba la prueba).
            if "[centro] Permisos en pantalla" not in texto or "[centro] botón Quitar en (" not in texto \
                    or "[compositor] ventana de centro en (" not in texto:
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
            centro = 19
            if captura_centro:
                time.sleep(0.8)
                mandar("screendump " + captura_centro + ".fondo-atardecer.ppm")
            time.sleep(0.5)
            # Decisión del usuario del 2026-11-13: la fila "Ampliar ventanas Amiga", de automático a 1x (en 800 x 600
            # la automática ya es 1x: la caja Amiga se ve igual). Por su nombre, como los fondos.
            elegir_fila("Ampliar ventanas Amiga")
            reintento = time.time() + 15
        if not selector_espera and centro == 19:
            if "[centro] Ampliar ventanas Amiga: 1x" not in texto and time.time() < reintento:
                time.sleep(0.25)
                continue
            # Decisión del usuario del 2026-11-13: la memoria de la caja Amiga, de 256 MB (de serie) a 512.
            time.sleep(0.5)
            elegir_fila("Memoria de la caja Amiga")
            reintento = time.time() + 15
            centro = 20
        if not selector_espera and centro == 20:
            if "[centro] Memoria de la caja Amiga: 512 MB" not in texto and time.time() < reintento:
                time.sleep(0.25)
                continue
            centro = 7
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
        # El explorador de archivos, al final (tras la barra), con la sesión de Ana delante y sin bloquear: el doble clic
        # en el icono de SYS: del Workbench lo abre; luego DeCarla (no se lista), Atrás, Programas, hola (lo lanza
        # `sesiones`), sus Propiedades (por SYS: no se cambian), la carpeta personal, Privado como lista, las
        # Propiedades de nota.txt (comentario y Archivado), buscar escribiendo, carpeta nueva (y un nombre que no vale),
        # copiar y pegar, cambiar el nombre (F2), la papelera y restaurar, las miniaturas de Fondos, una pestaña, otra
        # ventana con el camino escrito (Ctrl+L), moverla por su barra y arrastrar un fichero a la primera. Los sitios
        # los dicen el compositor (dónde salió cada ventana) y el explorador (dónde quedó cada cosa dentro). Con
        # CAPTURA_EXPLORADOR=prefijo: prefijo.ppm (Fondos con miniaturas), .lista, .info, .copia y .dos.
        if ex == 0 and barra == 11 and not bloqueado_ahora and "[workbench] icono de Home en (" in texto and "[compositor] capa de workbench en (" in texto and "[explorador] listo" in texto:
            ex = 1
            reintento = time.time() + 20
        if 1 <= ex < 90:
            import re
            captura_ex = os.environ.get("CAPTURA_EXPLORADOR")
            def ultima(patron):
                t = re.findall(patron, texto)
                return t[-1] if t else None
            def ir_ex(x, y):
                mandar(*["mouse_move -100 -100"] * 22, pausa=0.02)
                pasos = []
                while x > 0 or y > 0:
                    dx, dy = min(x, 100), min(y, 100)
                    pasos.append(f"mouse_move {dx} {dy}")
                    x, y = x - dx, y - dy
                mandar(*pasos, pausa=0.03)
                time.sleep(0.3)
            def clic_ex():
                mandar("mouse_button 1", "mouse_button 0", pausa=0.08)
            def doble_ex():
                mandar("mouse_button 1", "mouse_button 0", "mouse_button 1", "mouse_button 0", pausa=0.08)
            def escribir_ex(t):
                # Como `teclear`, sin el Intro del final.
                mandar(*[("sendkey spc" if c == " " else "sendkey shift-" + c.lower() if c.isupper() else "sendkey shift-7" if c == "/" else "sendkey shift-dot" if c == ":" else "sendkey " + c) for c in t], pausa=0.15)
            def tecla_ex(*t):
                mandar(*["sendkey " + x for x in t], pausa=0.25)
            def capa_ex():
                c = ultima(r"\[compositor\] capa de workbench en \((\d+), (\d+)\)")
                return (int(c[0]), int(c[1])) if c else (0, 40)
            def icono_ex(nombre):
                c = ultima(r"\[workbench\] (?:en pantalla: \d+x\d+, )?icono de " + nombre + r" en \((\d+), (\d+)\)")
                cx, cy = capa_ex()
                return (cx + int(c[0]), cy + int(c[1])) if c else None
            def origen_ex(n):
                t = re.findall(r"\[compositor\] ventana de explorador en \((\d+), (\d+)\)", texto)
                if len(t) < n:
                    return None
                dx, dy = ex_mov.get(n, (0, 0))
                return (int(t[n - 1][0]) + dx, int(t[n - 1][1]) + dy)
            def en_ex(n, que, linea=r"(?:rejilla|lista)"):
                o = origen_ex(n)
                g = ultima(r"\[explorador\] ventana " + str(n) + r", " + linea + r": (?:[^\n]*?; )?" + re.escape(que) + r" en \((\d+), (\d+)\)")
                return (o[0] + int(g[0]), o[1] + int(g[1])) if o and g else None
            def en_menu(n, que):
                return en_ex(n, que, "menú")
            def cuenta(t):
                return texto.count(t)
            espera = {1: "[explorador] ventana 1 en SYS:," in texto and origen_ex(1),
                      2: "[explorador] no abre SYS:DeCarla: -12" in texto,
                      3: cuenta("[explorador] ventana 1 en SYS:,") >= 2,
                      4: "[explorador] ventana 1 en SYS:Programas," in texto,
                      5: "[explorador] lanza Programas/hola" in texto,
                      6: "[explorador] ventana 1, menú:" in texto,
                      7: "[explorador] ventana 1, propiedades:" in texto,
                      8: "[explorador] ventana 1: diálogo cancelado" in texto,
                      9: "[explorador] ventana 1 en Home:," in texto,
                      10: "[explorador] ventana 1 en Home:Privado," in texto,
                      11: "[explorador] ventana 1 como lista" in texto,
                      12: "[explorador] ventana 1: por Tamano" in texto,
                      13: "[explorador] Propiedades de Home:Privado/nota.txt" in texto and cuenta("[explorador] ventana 1, propiedades:") >= 2,
                      14: "[explorador] guarda Home:Privado/nota.txt" in texto,
                      15: cuenta("[explorador] ventana 1 en Home:,") >= 2,
                      16: "[explorador] búsqueda de \"nota\" acabada" in texto,
                      17: cuenta("[explorador] ventana 1 en Home:,") >= 3,
                      18: "[explorador] crea la carpeta Home:Viaje: 0" in texto,
                      19: "[explorador] ventana 1: el nombre \"..\" no vale" in texto,
                      20: "[explorador] copia 2 elementos" in texto,
                      21: "[explorador] ventana 1 en Home:Viaje," in texto,
                      22: "[explorador] acabado: Copiar 2 elementos a Viaje" in texto,
                      23: "[explorador] cambia el nombre de Home:Viaje/grande.txt a \"texto grande.txt\": 0" in texto,
                      24: "[explorador] acabado: Mover \"texto grande.txt\" a la papelera" in texto,
                      25: "[explorador] ventana 1 en Papelera," in texto,
                      26: cuenta("[explorador] ventana 1, menú:") >= 2,
                      27: "[explorador] acabado: Restaurar" in texto,
                      28: "[explorador] ventana 1 en Home:Fondos," in texto,
                      29: "[explorador] miniatura de Home:Fondos/" in texto,
                      30: "[explorador] ventana 1: pestaña nueva (2 pestañas)" in texto,
                      31: "[explorador] ventana 1: cierra una pestaña (1 quedan)" in texto,
                      32: cuenta("[explorador] ventana 1 en Home:Privado,") >= 2,
                      33: "[explorador] ventana 2 en pantalla" in texto and origen_ex(2),
                      34: "[explorador] ventana 2: camino escrito \"Home/../DeCarla\": no vale" in texto,
                      35: "[explorador] ventana 2 en Home:Viaje," in texto,
                      36: True,
                      37: "[explorador] acabado: Mover \"datos.bin\" a Privado" in texto,
                      38: "[explorador] cierra la ventana 2" in texto,
                      39: "[explorador] sin ventanas abiertas" in texto}
            if ex > 1 and not espera.get(ex - 1, True):
                if time.time() > reintento:
                    # No llegó: se deja (las líneas que falten lo dirán).
                    ex = 99
                time.sleep(0.25)
                continue
            time.sleep(0.6)
            sitio = None
            if ex == 1:
                sitio = icono_ex("SYS")
                if sitio:
                    ir_ex(*sitio)
                    doble_ex()
            elif ex in (2, 5, 10):
                sitio = en_ex(1, {2: "DeCarla", 5: "hola", 10: "Privado"}[ex])
                if sitio:
                    ir_ex(*sitio)
                    doble_ex()
            elif ex == 4:
                # Paso 2 de las asignaciones: SYS: tiene ya todos sus cajones y Programas no cabe a la vista; se va por su
                # nombre, como en el Amiga (Ctrl+L y "SYS:Programas").
                sitio = True
                tecla_ex("ctrl-l")
                time.sleep(0.4)
                tecla_ex("ctrl-a")
                escribir_ex("SYS:Programas")
                tecla_ex("ret")
            elif ex == 3:
                sitio = en_ex(1, "Atrás")
                if sitio:
                    ir_ex(*sitio)
                    clic_ex()
            elif ex == 6:
                sitio = en_ex(1, "hola")
                if sitio:
                    ir_ex(*sitio)
                    mandar("mouse_button 2", "mouse_button 0", pausa=0.08)
            elif ex == 7:
                sitio = en_menu(1, "Propiedades")
                if sitio:
                    ir_ex(*sitio)
                    clic_ex()
            elif ex == 8:
                sitio = en_ex(1, "Cerrar", "propiedades")
                if sitio:
                    ir_ex(*sitio)
                    clic_ex()
            elif ex == 9:
                sitio = en_ex(1, "lugar Carpeta personal")
                if sitio:
                    ir_ex(*sitio)
                    clic_ex()
            elif ex == 11:
                sitio = en_ex(1, "Vista")
                if sitio:
                    ir_ex(*sitio)
                    clic_ex()
            elif ex == 12:
                if captura_ex:
                    mandar("screendump " + captura_ex + ".lista.ppm")
                    time.sleep(0.5)
                sitio = en_ex(1, "columna Tamano")
                if sitio:
                    ir_ex(*sitio)
                    clic_ex()
            elif ex == 13:
                # Otra vez por nombre (la columna Nombre), y Ctrl+I en nota.txt.
                nombre = en_ex(1, "columna Nombre")
                if nombre:
                    ir_ex(*nombre)
                    clic_ex()
                    time.sleep(0.6)
                sitio = en_ex(1, "nota.txt")
                if sitio:
                    ir_ex(*sitio)
                    clic_ex()
                    time.sleep(0.4)
                    tecla_ex("ctrl-i")
            elif ex == 14:
                sitio = en_ex(1, "campo", "propiedades")
                if sitio:
                    ir_ex(*sitio)
                    clic_ex()
                    time.sleep(0.4)
                    escribir_ex("nota de Ana")
                    time.sleep(0.4)
                    archivado = en_ex(1, "Archivado", "propiedades")
                    if archivado:
                        ir_ex(*archivado)
                        clic_ex()
                    time.sleep(0.4)
                    if captura_ex:
                        mandar("screendump " + captura_ex + ".info.ppm")
                        time.sleep(0.5)
                    guardar = en_ex(1, "Guardar", "propiedades")
                    if guardar:
                        ir_ex(*guardar)
                        clic_ex()
            elif ex == 15:
                sitio = True
                tecla_ex("backspace")
            elif ex == 16:
                sitio = True
                escribir_ex("nota")
            elif ex == 17:
                sitio = True
                tecla_ex("esc")
            elif ex == 18:
                sitio = True
                tecla_ex("ctrl-shift-n")
                time.sleep(0.6)
                tecla_ex("ctrl-a")
                escribir_ex("Viaje")
                tecla_ex("ret")
            elif ex == 19:
                sitio = True
                tecla_ex("ctrl-shift-n")
                time.sleep(0.6)
                tecla_ex("ctrl-a", "dot", "dot", "ret")
                time.sleep(0.6)
                tecla_ex("esc")
            elif ex == 20:
                sitio = en_ex(1, "datos.bin")
                if sitio:
                    ir_ex(*sitio)
                    clic_ex()
                    time.sleep(0.4)
                    # Ctrl+abajo mueve sin elegir, Ctrl+espacio elige: datos.bin y grande.txt (frio.bin va en medio).
                    tecla_ex("ctrl-down", "ctrl-down", "ctrl-spc", "ctrl-c")
            elif ex == 21:
                sitio = en_ex(1, "Viaje")
                if sitio:
                    ir_ex(*sitio)
                    doble_ex()
            elif ex == 22:
                sitio = True
                tecla_ex("ctrl-v")
                if captura_ex:
                    time.sleep(0.2)
                    mandar("screendump " + captura_ex + ".copia.ppm")
            elif ex == 23:
                sitio = en_ex(1, "grande.txt")
                if sitio:
                    ir_ex(*sitio)
                    clic_ex()
                    time.sleep(0.4)
                    tecla_ex("f2")
                    time.sleep(0.6)
                    escribir_ex("texto grande")
                    tecla_ex("ret")
            elif ex == 24:
                sitio = True
                tecla_ex("delete")
            elif ex == 25:
                if captura_ex:
                    time.sleep(0.3)
                    mandar("screendump " + captura_ex + ".papelera.ppm")
                    time.sleep(0.5)
                sitio = en_ex(1, "lugar Papelera")
                if sitio:
                    ir_ex(*sitio)
                    clic_ex()
            elif ex == 26:
                sitio = en_ex(1, "texto grande.txt")
                if sitio:
                    ir_ex(*sitio)
                    mandar("mouse_button 2", "mouse_button 0", pausa=0.08)
            elif ex == 27:
                sitio = en_menu(1, "Restaurar")
                if sitio:
                    ir_ex(*sitio)
                    clic_ex()
            elif ex == 28:
                sitio = True
                tecla_ex("ctrl-l")
                time.sleep(0.4)
                tecla_ex("ctrl-a")
                escribir_ex("Home/Fondos")
                tecla_ex("ret", "ctrl-1")
            elif ex == 29:
                # Las miniaturas, una a una: se espera a la primera y un poco más.
                sitio = True
                time.sleep(4)
                if captura_ex:
                    mandar("screendump " + captura_ex + ".ppm")
                    time.sleep(0.5)
                tecla_ex("ctrl-t")
            elif ex == 30:
                sitio = True
                # Plan de iconos, parte 3: en la pestaña nueva, los `.info` de la caja Amiga (uno de cada formato y uno
                # roto) y el volumen con su `Disk.info`. Con CAPTURA_EXPLORADOR: .amiga y .amiga-raiz.
                tecla_ex("ctrl-l")
                time.sleep(0.4)
                tecla_ex("ctrl-a")
                escribir_ex("Amiga:Iconos")
                tecla_ex("ret")
                fin_iconos = time.time() + 10
                while time.time() < fin_iconos and "[explorador] icono de Amiga:Iconos/Roto.info" not in open(serie, errors="replace").read():
                    time.sleep(0.3)
                time.sleep(0.6)
                if captura_ex:
                    mandar("screendump " + captura_ex + ".amiga.ppm")
                    time.sleep(0.5)
                tecla_ex("ctrl-l")
                time.sleep(0.4)
                tecla_ex("ctrl-a")
                escribir_ex("Amiga:")
                tecla_ex("ret")
                time.sleep(2)
                if captura_ex:
                    mandar("screendump " + captura_ex + ".amiga-raiz.ppm")
                    time.sleep(0.5)
                tecla_ex("ctrl-w")
            elif ex == 31:
                sitio = True
                tecla_ex("ctrl-l")
                time.sleep(0.4)
                tecla_ex("ctrl-a")
                escribir_ex("Home/Privado")
                tecla_ex("ret")
            elif ex == 32:
                sitio = True
                tecla_ex("ctrl-n")
            elif ex == 33:
                sitio = True
                tecla_ex("ctrl-l")
                time.sleep(0.4)
                tecla_ex("ctrl-a")
                escribir_ex("Home/")
                tecla_ex("dot", "dot")
                escribir_ex("/DeCarla")
                tecla_ex("ret")
            elif ex == 34:
                sitio = True
                tecla_ex("ctrl-l")
                time.sleep(0.4)
                tecla_ex("ctrl-a")
                # Paso 2 de las asignaciones: con su nombre, como en el Amiga (lo resuelve su tabla de nombres).
                escribir_ex("Home:Viaje")
                tecla_ex("ret")
            elif ex == 35:
                # La segunda ventana, por su barra (el título "Archivos"), 300 a la derecha.
                o = origen_ex(2)
                if o:
                    sitio = True
                    ir_ex(o[0] + 120, o[1] + 26)
                    mandar("mouse_button 1")
                    time.sleep(0.6)
                    mandar(*["mouse_move 30 0"] * 10, pausa=0.05)
                    time.sleep(0.3)
                    mandar("mouse_button 0")
                    # Dónde acabó de verdad (lo dice el compositor al soltar): si empezó a arrastrar tarde, se movió
                    # menos de 300, y suponer 300 dejaba el arrastre siguiente encima de la segunda ventana ("aquí no").
                    ex_mov[2] = (300, 0)
                    fin_mov = time.time() + 5
                    while time.time() < fin_mov:
                        texto = open(serie, errors="replace").read()
                        m = re.findall(r"\[compositor\] arrastre: [^\n]*; acaba en \((-?\d+), (-?\d+)\)", texto)
                        t2 = re.findall(r"\[compositor\] ventana de explorador en \((\d+), (\d+)\)", texto)
                        if m and len(t2) >= 2:
                            ex_mov[2] = (int(m[-1][0]) - int(t2[1][0]), int(m[-1][1]) - int(t2[1][1]))
                            break
                        time.sleep(0.2)
            elif ex == 36:
                # Arrastrar datos.bin de la segunda ventana a la carpeta de la primera (Privado), por la parte que se ve.
                desde = en_ex(2, "datos.bin")
                o1 = origen_ex(1)
                if desde and o1:
                    sitio = True
                    ir_ex(*desde)
                    mandar("mouse_button 1")
                    time.sleep(0.3)
                    x, y = desde
                    hx, hy = o1[0] + 290, o1[1] + 330
                    # Lejos del borde de la segunda ventana (su sombra y el borde de redimensionar se llevan el soltar):
                    # a 30 px de ella como poco, sin salir de la zona de ficheros de la primera (desde +240).
                    o2 = origen_ex(2)
                    if o2 and hx > o2[0] - 30:
                        hx = max(o1[0] + 240, o2[0] - 30)
                    pasos = []
                    while (x, y) != (hx, hy):
                        dx = max(-25, min(25, hx - x))
                        dy = max(-25, min(25, hy - y))
                        pasos.append(f"mouse_move {dx} {dy}")
                        x, y = x + dx, y + dy
                    mandar(*pasos, pausa=0.05)
                    time.sleep(0.4)
                    if captura_ex:
                        mandar("screendump " + captura_ex + ".arrastre.ppm")
                        time.sleep(0.5)
                    mandar("mouse_button 0")
            elif ex == 37:
                sitio = True
                if captura_ex:
                    time.sleep(0.5)
                    mandar("screendump " + captura_ex + ".dos.ppm")
                    time.sleep(0.5)
                tecla_ex("ctrl-w")
            elif ex == 38:
                o1 = origen_ex(1)
                if o1:
                    sitio = True
                    ir_ex(o1[0] + 560, o1[1] + 380)
                    clic_ex()
                    time.sleep(0.4)
                    tecla_ex("ctrl-w")
            elif ex == 39:
                sitio = True
            if sitio is None:
                # No se sabe dónde: se deja.
                ex = 99
                continue
            ex += 1
            reintento = time.time() + 20
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
            ir_a(*cerrar_amiga(int(v_amiga.group(1)), int(v_amiga.group(2)),
                               re.search(r"ventana Amiga en pantalla \((\d+)x\d+(?:, a (\d+)x)?(, con la barra de DashOS)?\)", texto)))
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
                    ir_a(*cerrar_amiga(x, y, list(re.finditer(r"ventana Amiga en pantalla \((\d+)x\d+(?:, a (\d+)x)?(, con la barra de DashOS)?\)", texto[corte:]))[-1]))
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
        # S28: ni con una pregunta de `guardian` en pantalla (las de la caja Amiga salen a esta hora) ni con la sesión
        # bloqueada: se llevarían el teclado, también Super+Arriba. Si sale una pregunta mientras, se repite.
        # Con el sitio que le dio el compositor ya dicho (con más programas al arrancar, como el explorador, esa línea
        # puede salir después que la de Ajustes, y el clic iba al sitio de siempre: encima de su campo).
        if not ajustes and "[ajustes] ventana del kit en pantalla" in texto and "[compositor] ventana de ajustes en (" in texto and selector not in (1, 2, 4, 5, 17, 18) and libre_caja and not bloqueado_ahora:
            ajustes = True
            preguntas_antes = texto.count("[guardian] pregunta en pantalla: ")
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
            if open(serie, errors="replace").read().count("[guardian] pregunta en pantalla: ") > preguntas_antes:
                ajustes = False
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
# El Shell nativo en pantalla: con SHELL_CAPTURAS=prefijo, `tools/guion_shell.py` entra, abre el Shell desde el
# lanzador, teclea en él y lo captura en cada momento (`make arranque-virt-shell`).
if [ -n "${SHELL_CAPTURAS:-}" ]; then
    python3 -I "$(dirname "$0")/guion_shell.py" "$MONITOR" "$SEGUNDOS" "$SALIDA" "$SHELL_CAPTURAS" >/dev/null 2>&1 &
    SHELL_PID=$!
fi
# Plan de iconos, parte 3: con ICONOS_CAPTURAS=prefijo, `tools/guion_iconos.py` (`make arranque-virt-iconos`).
if [ -n "${ICONOS_CAPTURAS:-}" ]; then
    python3 -I "$(dirname "$0")/guion_iconos.py" "$MONITOR" "$SEGUNDOS" "$SALIDA" "$ICONOS_CAPTURAS" >/dev/null 2>&1 &
    ICONOS_PID=$!
fi
# S10, los menús: con MENUS_CAPTURAS=prefijo, `tools/guion_menus.py` (`make arranque-virt-menus`).
if [ -n "${MENUS_CAPTURAS:-}" ]; then
    python3 -I "$(dirname "$0")/guion_menus.py" "$MONITOR" "$SEGUNDOS" "$SALIDA" "$MENUS_CAPTURAS" >/dev/null 2>&1 &
    MENUS_PID=$!
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
# Con REINICIO, lo que tiene que salir tras pulsar Reiniciar, y otro arranque del kernel.
reiniciada() {
    local texto
    texto=$(sed -e 's/\x1b\[[0-9;]*[A-Za-z]//g' -e 's/\r//g' "$SALIDA")
    [ "$(grep -cF -- "DashOS en marcha con" <<< "$texto")" -ge 2 ] || return 1
    grep -v '^#' "$REINICIO" | grep -v '^$' | sed 's/^~//' | while IFS= read -r l; do
        grep -qF -- "$l" <<< "$texto" || exit 1
    done
}
CAPTURADA=
while kill -0 "$QEMU" 2>/dev/null; do
    # Con REINICIO, tras la captura se da la señal al guion y se espera al segundo arranque (`todas` ya no se mira).
    if [ -n "$CAPTURADA" ] && [ -n "${REINICIO:-}" ]; then
        [ -e "$REINICIO_SENAL" ] || touch "$REINICIO_SENAL"
        if reiniciada; then
            kill "$QEMU" 2>/dev/null
            break
        fi
    elif todas; then
        sleep 0.5
        if [ -n "${CAPTURA:-}" ]; then
            # Un segundo para que el compositor pinte lo último (tras desbloquear, U3, vuelve a componer todo).
            sleep 1
            python3 -I -c 'import socket,sys,time; s=socket.socket(socket.AF_UNIX); s.connect(sys.argv[1]); s.sendall(("screendump "+sys.argv[2]+"\n").encode()); time.sleep(1)' "$MONITOR" "$CAPTURA"
        fi
        CAPTURADA=1
        if [ -z "${REINICIO:-}" ]; then
            kill "$QEMU" 2>/dev/null
            break
        fi
    fi
    sleep 0.5
done
wait "$QEMU" 2>/dev/null
[ -n "${TECLAS_PID:-}" ] && kill "$TECLAS_PID" 2>/dev/null
[ -n "${MULTI_PID:-}" ] && kill "$MULTI_PID" 2>/dev/null
[ -n "${CAJA_PID:-}" ] && kill "$CAJA_PID" 2>/dev/null
[ -n "${SHELL_PID:-}" ] && kill "$SHELL_PID" 2>/dev/null
[ -n "${MENUS_PID:-}" ] && kill "$MENUS_PID" 2>/dev/null
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

# Con REINICIO: sus líneas, en orden, tras la última de los esperados; y el segundo arranque del kernel, después.
if [ "$FALLO" = 0 ] && [ -n "${REINICIO:-}" ]; then
    while IFS= read -r ESPERADO; do
        [ -z "$ESPERADO" ] && continue
        case "$ESPERADO" in \#*) continue;; esac
        ESPERADO=${ESPERADO#\~}
        N=$(tail -n +"$DESDE" "$SALIDA.txt" | grep -nF -m1 -- "$ESPERADO" | cut -d: -f1)
        if [ -z "$N" ]; then
            echo "FALTA (reinicio): $ESPERADO"
            FALLO=1
            break
        fi
        DESDE=$((DESDE + N))
    done < "$REINICIO"
    if [ "$FALLO" = 0 ] && ! tail -n +"$DESDE" "$SALIDA.txt" | grep -qF "DashOS en marcha con"; then
        echo "FALTA (reinicio): el segundo arranque"
        FALLO=1
    fi
    [ "$FALLO" = 0 ] && echo "BIEN: $(basename "$REINICIO")"
fi
if [ "$FALLO" = 0 ]; then
    echo "BIEN: $(basename "$ESPERADOS")"
else
    echo "--- salida del puerto serie (final) ---"
    tail -40 "$SALIDA.txt"
fi
[ -n "${GUARDAR:-}" ] && cp "$SALIDA.txt" "$GUARDAR"
rm -rf "$DISCO" "$SALIDA" "$SALIDA.txt" "$MONITOR" "$LINEAS" "$SALIDA.reiniciar"
exit $FALLO
