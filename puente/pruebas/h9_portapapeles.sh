#!/usr/bin/env bash
# H9 (caja-linux-apps): el portapapeles por el puente de la caja Linux, en el PC. El compositor de DashOS en el PC
# (compositor-pc, que hace de DashOS con tuberías en vez de canales) y el puente; wl-copy y wl-paste (wl-clipboard) de
# un lado y del otro: de Linux (por el puente) a DashOS (directo), al revés, de Linux a Linux, la primaria y 4 MB. Y los
# hostiles de `puente --cliente-hostil`: decoraciones y tipos rotos (cortados), una fuente sin fin (se le corta al
# dejar de leer) y otra que nunca cierra; después, copiar y pegar sigue.
set -eu
cd "$(dirname "$0")/.."
command -v wl-copy >/dev/null || { echo "H9 portapapeles: sin wl-clipboard, no se prueba"; exit 0; }
cargo build --release --offline --quiet
T=$(mktemp -d)
R=${XDG_RUNTIME_DIR:-/tmp}/dashos-h9-$$
timeout 120 target/release/compositor-pc "$R-directo" --transporte "$R-tr" > "$T/compositor.txt" 2>&1 &
for _ in $(seq 50); do [ -S "$R-tr" ] && break; sleep 0.1; done
timeout 120 target/release/puente "$R-puente" "$R-tr" > "$T/puente.txt" 2>&1 &
for _ in $(seq 50); do [ -S "$R-puente" ] && break; sleep 0.1; done
mal=0
probar() {
    local nombre=$1 de=$2 a=$3 extra=$4 texto=$5
    printf '%s' "$texto" | WAYLAND_DISPLAY=$de timeout 10 wl-copy $extra &
    sleep 1.5
    local leido
    leido=$(WAYLAND_DISPLAY=$a timeout 10 wl-paste -n $extra || true)
    if [ "$leido" = "$texto" ]; then echo "H9 portapapeles: $nombre: bien"; else echo "H9 portapapeles: $nombre: MAL ($leido)"; mal=1; fi
}
probar "de Linux a DashOS" "$R-puente" "$R-directo" "" "copiado en Linux"
probar "de DashOS a Linux" "$R-directo" "$R-puente" "" "copiado en DashOS"
probar "de Linux a Linux" "$R-puente" "$R-puente" "" "de Linux a Linux"
probar "la primaria" "$R-puente" "$R-directo" "--primary" "la primaria"
head -c 3000000 /dev/urandom | base64 -w0 > "$T/grande"
probar "4 MB" "$R-puente" "$R-directo" "" "$(cat "$T/grande")"
target/release/puente --cliente-hostil "$R-puente" roto | tee "$T/roto.txt"
[ "$(grep -c "lo cortó" "$T/roto.txt")" = 3 ] || { echo "H9 portapapeles: roto: MAL"; mal=1; }
target/release/puente --cliente-hostil "$R-puente" sin-fin 8 > "$T/sinfin.txt" &
H=$!
sleep 2
WAYLAND_DISPLAY="$R-directo" timeout 5 wl-paste | head -c 100000 > /dev/null || true
sleep 1
grep -q "le cortaron la tubería" "$T/sinfin.txt" && echo "H9 portapapeles: sin fin: cortado" || { echo "H9 portapapeles: sin fin: MAL"; mal=1; }
wait $H
target/release/puente --cliente-hostil "$R-puente" nunca 6 > "$T/nunca.txt" &
H=$!
sleep 2
WAYLAND_DISPLAY="$R-directo" timeout 3 wl-paste > /dev/null || true
wait $H
probar "después de los hostiles" "$R-puente" "$R-directo" "" "sigue"
kill %1 %2 2>/dev/null || true
rm -f "$R-directo" "$R-puente" "$R-tr"
rm -rf "$T"
exit $mal
