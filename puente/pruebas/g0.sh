#!/usr/bin/env bash
# G0 (docs/GRAFICOS_LOOP.md): el compositor de DashOS en el PC con un cliente de libwayland de verdad. El cliente pinta
# 10 cuadros; la captura tiene que tener su ventana (azul 0x3366cc) en la cascada (40, 40) con la banda blanca del
# último cuadro en la fila 90, y el fondo fuera.
set -eu
cd "$(dirname "$0")/.."
C=target/c
mkdir -p "$C"
X=/usr/share/wayland-protocols/stable/xdg-shell/xdg-shell.xml
wayland-scanner client-header "$X" "$C/xdg-shell-client-protocol.h"
wayland-scanner private-code "$X" "$C/xdg-shell-protocol.c"
cc -O2 -Wall -I"$C" -o "$C/cliente_shm" pruebas/cliente_shm.c "$C/xdg-shell-protocol.c" -lwayland-client
cargo build --release --offline --quiet
T=$(mktemp -d)
SOCK=${XDG_RUNTIME_DIR:-/tmp}/dashos-g0-$$
timeout 30 target/release/compositor-pc "$SOCK" --captura "$T/captura.ppm" --salir-sin-clientes > "$T/compositor.txt" 2>&1 &
COMP=$!
for _ in $(seq 50); do [ -S "$SOCK" ] && break; sleep 0.1; done
WAYLAND_DISPLAY="$SOCK" timeout 20 "$C/cliente_shm"
wait "$COMP"
python3 - "$T/captura.ppm" <<'PY'
import sys
d = open(sys.argv[1], "rb").read()
px = d[d.index(b"255\n") + 4:]
p = lambda x, y: px[(y * 800 + x) * 3:(y * 800 + x) * 3 + 3].hex()
bien = p(45, 45) == "3366cc" and p(45, 135) == "ffffff" and p(45, 125) == "3366cc" and p(5, 5) == "204060"
print("G0: libwayland con el compositor de DashOS:", "bien" if bien else f"MAL ({p(45, 45)} {p(45, 135)} {p(5, 5)})")
sys.exit(0 if bien else 1)
PY
rm -rf "$T"
