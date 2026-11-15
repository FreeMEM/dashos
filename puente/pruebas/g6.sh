#!/usr/bin/env bash
# G6 (docs/GRAFICOS_LOOP.md): linux-dmabuf con un programa de verdad. alacritty pinta con la tarjeta gráfica (Mesa, EGL)
# en búferes de dmabuf lineales; su fondo verde tiene que salir en la captura, y el búfer tiene que haber llegado por
# zwp_linux_buffer_params_v1 (no por wl_shm). Dos veces: directo al compositor, que lee el dmabuf con la CPU, y por el
# puente de la caja Linux, que lo copia a una memoria del otro lado como un pool de wl_shm.
set -eu
cd "$(dirname "$0")/.."
command -v alacritty >/dev/null || { echo "G6: sin alacritty en este PC, no se prueba"; exit 0; }
[ -e /dev/dri/renderD128 ] || { echo "G6: sin /dev/dri/renderD128, no se prueba"; exit 0; }
cargo build --release --offline --quiet
T=$(mktemp -d)
R=${XDG_RUNTIME_DIR:-/tmp}/dashos-g6-$$
# alacritty contra el socket $1; deja en $T/$2.ppm la captura y dice "verde N" (N búferes de dmabuf) o lo que salió.
alacritty_en() {
    WAYLAND_DISPLAY="$1" WAYLAND_DEBUG=1 timeout 15 alacritty -o 'colors.primary.background="#00ff00"' -e sleep 3 > "$T/$2.txt" 2>&1 || echo "alacritty acabó con $?" >> "$T/$2.txt"
    wait "$COMP" 2>/dev/null || true
    python3 - "$T/$2.ppm" "$(grep -c create_immed "$T/$2.txt" || true)" <<'PY'
import sys
d = open(sys.argv[1], "rb").read()
px = d[d.index(b"255\n") + 4:]
r, g, b = px[(300 * 800 + 400) * 3:(300 * 800 + 400) * 3 + 3]
verde = g > 0xb0 and r < 0x40 and b < 0x40
print(("verde" if verde else "%02x%02x%02x" % (r, g, b)) + " " + sys.argv[2])
PY
}
timeout 30 target/release/compositor-pc "$R" --captura "$T/directo.ppm" --captura-tras 2500 --salir-sin-clientes > "$T/comp1.txt" 2>&1 &
COMP=$!
for _ in $(seq 50); do [ -S "$R" ] && break; sleep 0.1; done
DIRECTO=$(alacritty_en "$R" directo)
timeout 30 target/release/compositor-pc "$R-directo" --transporte "$R-transporte" --captura "$T/puente.ppm" --captura-tras 3000 --salir-sin-clientes > "$T/comp2.txt" 2>&1 &
COMP=$!
for _ in $(seq 50); do [ -S "$R-transporte" ] && break; sleep 0.1; done
timeout 30 target/release/puente "$R-wayland" "$R-transporte" > "$T/puente.txt" 2>&1 &
for _ in $(seq 50); do [ -S "$R-wayland" ] && break; sleep 0.1; done
PUENTE=$(alacritty_en "$R-wayland" puente)
if [ "${DIRECTO%% *}" = verde ] && [ "${DIRECTO#* }" -gt 0 ] && [ "${PUENTE%% *}" = verde ] && [ "${PUENTE#* }" -gt 0 ]; then
    echo "G6: alacritty pinta con la tarjeta en búferes de dmabuf y se ve, directo y por el puente: bien"
    grep "por cuadro" "$T/puente.txt" || true
else
    echo "G6: MAL (directo: $DIRECTO; puente: $PUENTE)"; grep -v "^\[" "$T/directo.txt" "$T/puente.txt" | head; cat "$T/comp1.txt" "$T/comp2.txt" "$T/puente.txt"; exit 1
fi
rm -rf "$T"
