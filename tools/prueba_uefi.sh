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
timeout "$SEGUNDOS" qemu-system-aarch64 -monitor unix:"$MONITOR",server,nowait -M virt,gic-version=${GIC:-2},acpi=off${MAQUINA:+,$MAQUINA} -cpu cortex-a72 -smp 4 -m 2G \
    -bios "$FIRMWARE" -drive file=fat:rw:"$DISCO",format=raw,if=virtio \
    -display none -serial file:"$SALIDA" -net none ${QEMU_EXTRA:-} >/dev/null 2>&1 &
QEMU=$!
# Se para en cuanto sale la última línea esperada (o al acabar el tiempo). Con CAPTURA=fichero.ppm, antes guarda la
# pantalla (U4).
FIN=$(grep -v '^#' "$ESPERADOS" | grep -v '^$' | tail -1 | sed 's/^~//')
while kill -0 "$QEMU" 2>/dev/null; do
    if [ -n "$FIN" ] && tr -d '\r' < "$SALIDA" | grep -qF -- "$FIN"; then
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
rm -rf "$DISCO" "$SALIDA" "$SALIDA.txt" "$MONITOR"
exit $FALLO
