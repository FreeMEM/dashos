#!/bin/sh
# Las escenas de amiga-apps (tools/amiga_apps/*.txt: ProTracker, Directory Opus, LightWave, MUI...) en la caja de Linux
# x86, con el compositor de mentira (no toca el escritorio). Necesita el AmigaOS del usuario con los programas
# instalados (como make amiga-apps): CAJA_AMIGAOS=~/.cache/dash-emu/sys32mui-apps. Nada de eso va en git.
#
#   escenas.sh <carpeta de salida> [escena...]     (sin escenas: todas menos "todos")
#
# Por cada escena dice el código de salida de la caja, las ventanas que se abrieron y lo que la caja dice que falta, y
# deja en la carpeta una foto de cada ventana (<escena>-1.png...), la salida de la caja y la del compositor.
AQUI=$(cd "$(dirname "$0")" && pwd)
RAIZ=$(cd "$AQUI/../.." && pwd)
X=${CAJA_X:-$AQUI/../build-linux-x86/caja-amiga}
SALIDA=$1
shift
[ -n "$SALIDA" ] || { echo "uso: escenas.sh <carpeta de salida> [escena...]"; exit 2; }
[ -n "$CAJA_AMIGAOS" ] || { echo "falta CAJA_AMIGAOS (el AmigaOS con los programas)"; exit 2; }
mkdir -p "$SALIDA"
SALIDA=$(cd "$SALIDA" && pwd)
ESCENAS=$*
if [ -z "$ESCENAS" ]; then
    ESCENAS=$(cd "$RAIZ/tools/amiga_apps" && ls *.txt | sed 's/\.txt$//' | grep -v '^todos$')
fi
SEGUNDOS=${ESCENAS_SEGUNDOS:-30}
for e in $ESCENAS; do
    t=$(mktemp -d) || exit 1
    cp "$AQUI/../pruebas68k/lanzador.exe" "$t/$e"
    mkdir "$t/lanzador" "$t/sys"
    cp "$RAIZ/tools/amiga_apps/$e.txt" "$t/lanzador/"
    (cd "$t" && python3 "$AQUI/compositor_falso.py" wl.sock "$SALIDA/$e-0.png" "esperar:$((SEGUNDOS - 8))" "fotos:$SALIDA/$e" \
        > "$SALIDA/$e-compositor.txt" 2>&1) &
    cp_pid=$!
    sleep 0.5
    (cd "$t" && XDG_RUNTIME_DIR=. WAYLAND_DISPLAY=wl.sock CAJA_AMIGA_SYS="$t/sys" CAJA_AMIGA_PROTEGER=no \
        timeout "$SEGUNDOS" "$X" -v "$t/$e" < /dev/null > "$SALIDA/$e-caja.txt" 2>&1)
    r=$?
    sleep 1
    kill $cp_pid 2>/dev/null
    wait $cp_pid 2>/dev/null
    v=$(grep -E "^(ventana|otra ventana)" "$SALIDA/$e-compositor.txt" | sed 's/^otra //; s/^ventana //' | tr '\n' ';')
    f=$(grep -oE "falta: [^,]*" "$SALIDA/$e-caja.txt" | sort -u | sed 's/^falta: //' | tr '\n' ';')
    echo "$e | caja $r | ventanas: $v | falta: $f"
    case "$t" in /tmp/*) rm -r -- "$t" ;; esac
done
