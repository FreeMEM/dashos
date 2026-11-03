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

SALIDA=$(mktemp)
timeout "$SEGUNDOS" qemu-system-aarch64 -M raspi4b -kernel "$IMAGEN" -dtb "$DTB" \
    -display none -serial file:"$SALIDA" -serial null ${QEMU_EXTRA:-} >/dev/null 2>&1
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
rm -f "$SALIDA" "$SALIDA.txt"
exit $FALLO
