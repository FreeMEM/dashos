#!/usr/bin/env bash
# DashOS: arranca una imagen en QEMU raspi4b y comprueba la salida del puerto serie.
#
#   tools/prueba_qemu.sh <imagen> <fichero de esperados> [segundos]
#
# Cada línea del fichero de esperados tiene que aparecer en la salida, en ese orden (como texto, no como
# expresión regular). Las que empiezan por "~" pueden aparecer en cualquier sitio (para lo que corre en paralelo en
# varios núcleos). Las que empiezan por "#" se ignoran. Sale con 0 si todo aparece.
set -u
IMAGEN=$1
ESPERADOS=$2
SEGUNDOS=${3:-6}
RAIZ=$(cd "$(dirname "$0")/.." && pwd)
FIRMWARE=$RAIZ/build/firmware
DTB=$FIRMWARE/bcm2711-rpi-4-b.dtb

# El árbol de dispositivos de la Pi 4 sale del firmware oficial, a la misma versión que usa Emu68.
# No va en el repositorio (viene de Linux, con licencia GPL); se baja la primera vez.
if [ ! -f "$DTB" ]; then
    mkdir -p "$FIRMWARE"
    curl -sSfL -o "$DTB" https://github.com/raspberrypi/firmware/raw/0ea2874060/boot/bcm2711-rpi-4-b.dtb || {
        echo "no se pudo bajar el árbol de dispositivos"; exit 2; }
fi

# QEMU solo pone en el árbol los primeros 960 MB, aunque raspi4b tiene RAM también entre 1 GB y 0x7E00_0000 (lo
# dice "info mtree"). La Pi de verdad los da todos (el firmware rellena "memory"). Para probar lo que va por encima
# del primer GB (C38), una copia del árbol con un nodo de memoria más: 0x4000_0000 a 0x7C00_0000. No se llama
# "memory@...": QEMU borra esos nodos y pone el suyo. El kernel busca la memoria por device_type.
DTB_ORIGINAL=$DTB
DTB=$FIRMWARE/bcm2711-rpi-4-b-qemu.dtb
# Con MEMORIA_ALTA=no se usa el árbol tal cual: como una Pi de 1 GB, sin nada por encima del primer GB.
if [ "${MEMORIA_ALTA:-si}" = no ]; then
    DTB=$DTB_ORIGINAL
elif [ ! -f "$DTB" ] || [ "$DTB_ORIGINAL" -nt "$DTB" ] || [ "$0" -nt "$DTB" ]; then
    cp "$DTB_ORIGINAL" "$DTB"
    fdtput -c "$DTB" /ram-alta@40000000
    fdtput -ts "$DTB" /ram-alta@40000000 device_type memory
    fdtput -tx "$DTB" /ram-alta@40000000 reg 0 0x40000000 0x3c000000
fi

# Tarjeta SD de prueba (C21): 64 MB con un texto en el sector 0, la firma 0x55AA y otro texto en el sector 1.
SD=$RAIZ/build/sd/prueba.img
if [ ! -f "$SD" ]; then
    mkdir -p "$(dirname "$SD")"
    python3 -I -c '
import sys
img = bytearray(64 * 1024 * 1024)
a = b"DashOS: primer sector de la tarjeta SD"
b = b"segundo sector: bien"
img[0:len(a)] = a
img[510:512] = b"\x55\xaa"
img[512:512 + len(b)] = b
open(sys.argv[1], "wb").write(img)
' "$SD"
fi

SALIDA=$(mktemp)
# Entrada para el teclado y el ratón USB (C22): por el monitor de QEMU, una "a" y un movimiento del ratón cada
# medio segundo desde el segundo 2. Con CAPTURA=fichero.ppm, guarda la pantalla 1,5 segundos antes del final.
MONITOR=$SALIDA.mon
python3 -I - "$MONITOR" "$SEGUNDOS" "${CAPTURA:-}" <<'PY' >/dev/null 2>&1 &
import socket, sys, time
ruta, total, captura = sys.argv[1], float(sys.argv[2]), sys.argv[3]
inicio = time.time()
time.sleep(2)
capturada = False
while time.time() - inicio < total:
    if captura and not capturada and time.time() - inicio > total - 1.5:
        capturada = True
        try:
            s = socket.socket(socket.AF_UNIX)
            s.connect(ruta)
            s.sendall(("screendump " + captura + "\n").encode())
            time.sleep(0.3)
            s.close()
        except OSError:
            pass
    try:
        s = socket.socket(socket.AF_UNIX)
        s.connect(ruta)
        s.sendall(b"sendkey a\nmouse_move 10 5\n")
        s.close()
    except OSError:
        pass
    time.sleep(0.5)
PY
TECLAS=$!
timeout "$SEGUNDOS" qemu-system-aarch64 -M raspi4b -kernel "$IMAGEN" -dtb "$DTB" \
    -display none -serial file:"$SALIDA" -serial null \
    -drive file="$SD",if=sd,format=raw -device usb-kbd -device usb-mouse \
    -monitor unix:"$MONITOR",server,nowait ${QEMU_EXTRA:-} >/dev/null 2>&1
kill "$TECLAS" 2>/dev/null
tr -d '\r' < "$SALIDA" > "$SALIDA.txt"

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
    echo "--- salida del puerto serie ---"
    cat "$SALIDA.txt"
fi
[ -n "${GUARDAR:-}" ] && cp "$SALIDA.txt" "$GUARDAR"
rm -f "$SALIDA" "$SALIDA.txt" "$MONITOR"
exit $FALLO
