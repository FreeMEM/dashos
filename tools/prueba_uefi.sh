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
# Cuando `guardian` pone la pantalla de bloqueo (U3), se dejan de mandar esas teclas y se teclea una contraseña mala
# y la buena, con Intro tras cada una; al desbloquear, sigue lo de antes. Igual con la pantalla de entrada y, tras
# ella, con el bloqueo por tiempo: 3 s después de que `guardian` pida el aviso, nada hasta que bloquea.
if [ -n "${TECLAS:-}" ]; then
    python3 -I - "$MONITOR" "$SEGUNDOS" "$SALIDA" <<'PY' >/dev/null 2>&1 &
import socket, sys, time
ruta, total, serie = sys.argv[1], float(sys.argv[2]), sys.argv[3]
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

while time.time() - inicio < total:
    try:
        texto = open(serie, errors="replace").read()
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
