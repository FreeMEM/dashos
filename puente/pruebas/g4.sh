#!/usr/bin/env bash
# G4 (docs/GRAFICOS_LOOP.md): layer-shell con un programa de verdad. swaybg pinta el fondo (capa de fondo, naranja) y el
# cliente de prueba abre su ventana encima. Sin el poder de escritorio, swaybg no ve layer-shell y no pinta nada.
set -eu
cd "$(dirname "$0")/.."
C=target/c
command -v swaybg >/dev/null || { echo "G4: sin swaybg en este PC, no se prueba"; exit 0; }
[ -x "$C/cliente_shm" ] || pruebas/g0.sh >/dev/null
cargo build --release --offline --quiet
T=$(mktemp -d)
R=${XDG_RUNTIME_DIR:-/tmp}/dashos-g4-$$
pixeles() {
python3 - "$1" <<'PY'
import sys
d = open(sys.argv[1], "rb").read()
px = d[d.index(b"255\n") + 4:]
p = lambda x, y: px[(y * 800 + x) * 3:(y * 800 + x) * 3 + 3].hex()
print(p(45, 45), p(5, 5), p(795, 595))
PY
}
# Con el poder: fondo naranja y la ventana azul encima.
timeout 30 target/release/compositor-pc "$R" --escritorio --captura "$T/con.ppm" > "$T/c1.txt" 2>&1 &
COMP=$!
for _ in $(seq 50); do [ -S "$R" ] && break; sleep 0.1; done
WAYLAND_DISPLAY="$R" swaybg -c '#ff8800' > /dev/null 2>&1 &
BG=$!
sleep 0.5
WAYLAND_DISPLAY="$R" timeout 20 "$C/cliente_shm" > /dev/null
sleep 0.3
kill "$BG" 2>/dev/null || true
wait "$COMP" 2>/dev/null || true
CON=$(pixeles "$T/con.ppm")
# Sin el poder: swaybg acaba sin pintar; el fondo sigue siendo el del compositor.
timeout 30 target/release/compositor-pc "$R" --captura "$T/sin.ppm" --salir-sin-clientes > "$T/c2.txt" 2>&1 &
COMP=$!
for _ in $(seq 50); do [ -S "$R" ] && break; sleep 0.1; done
WAYLAND_DISPLAY="$R" timeout 5 swaybg -c '#ff8800' > "$T/swaybg-sin.txt" 2>&1 && SIN_RC=0 || SIN_RC=$?
wait "$COMP" 2>/dev/null || true
SIN=$(pixeles "$T/sin.ppm")
if [ "$CON" = "3366cc ff8800 ff8800" ] && [ "$SIN" != "${SIN#*204060}" ] && [ "$SIN_RC" != 0 ] && [ "$SIN_RC" != 124 ]; then
    echo "G4: swaybg pinta el fondo con el poder de escritorio, y sin él no: bien"
else
    echo "G4: MAL (con: $CON; sin: $SIN, swaybg $SIN_RC)"; cat "$T/swaybg-sin.txt"; exit 1
fi
rm -rf "$T"
