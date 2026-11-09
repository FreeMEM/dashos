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
if [ -n "${TECLAS:-}" ]; then
    python3 -I - "$MONITOR" "$SEGUNDOS" <<'PY' >/dev/null 2>&1 &
import socket, sys, time
ruta, total = sys.argv[1], float(sys.argv[2])
inicio = time.time()
time.sleep(3)
paso = 0
while time.time() - inicio < total:
    try:
        s = socket.socket(socket.AF_UNIX)
        s.connect(ruta)
        s.sendall(b"mouse_move 10 5\n")
        paso += 1
        if paso >= 15:
            s.sendall(b"mouse_button 1\n")
            time.sleep(0.1)
            s.sendall(b"mouse_button 0\n")
        s.sendall(b"sendkey a\n")
        s.close()
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
