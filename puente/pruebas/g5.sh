#!/usr/bin/env bash
# G5 (docs/GRAFICOS_LOOP.md): bloqueo de la sesión con un programa de verdad. swaylock (verde, sin indicador) bloquea;
# una ventana que se abre después no se ve ni completa cuadros; si swaylock muere, la pantalla sigue tapada (negro).
# Sin el poder de escritorio, swaylock no encuentra ext_session_lock_manager_v1 y acaba con error.
set -eu
cd "$(dirname "$0")/.."
C=target/c
command -v swaylock >/dev/null || { echo "G5: sin swaylock en este PC, no se prueba"; exit 0; }
[ -x "$C/cliente_shm" ] || pruebas/g0.sh >/dev/null
cargo build --release --offline --quiet
T=$(mktemp -d)
R=${XDG_RUNTIME_DIR:-/tmp}/dashos-g5-$$
pixeles() {
python3 - "$1" <<'PY'
import sys
d = open(sys.argv[1], "rb").read()
px = d[d.index(b"255\n") + 4:]
p = lambda x, y: px[(y * 800 + x) * 3:(y * 800 + x) * 3 + 3].hex()
print(p(45, 45), p(5, 5), p(795, 595))
PY
}
timeout 40 target/release/compositor-pc "$R" --escritorio --captura "$T/c%d.ppm" --salir-sin-clientes > "$T/comp.txt" 2>&1 &
COMP=$!
for _ in $(seq 50); do [ -S "$R" ] && break; sleep 0.1; done
WAYLAND_DISPLAY="$R" swaylock -u -c 00ff00 > "$T/swaylock.txt" 2>&1 &
LOCK=$!
for _ in $(seq 30); do grep -q "cliente 1" "$T/comp.txt" 2>/dev/null && break; sleep 0.1; done
sleep 1
# Una ventana con la sesión bloqueada: no se ve, y sus cuadros no llegan (timeout la corta).
WAYLAND_DISPLAY="$R" timeout 3 "$C/cliente_shm" > "$T/cliente.txt" 2>&1 && CLI=0 || CLI=$?
# Muere swaylock: sigue bloqueado.
kill -9 "$LOCK" 2>/dev/null || true; wait "$LOCK" 2>/dev/null || true
wait "$COMP" 2>/dev/null || true
UNO=$(pixeles "$T/c1.ppm" 2>/dev/null || echo "sin captura")
DOS=$(pixeles "$T/c2.ppm" 2>/dev/null || echo "sin captura")
# Sin el poder de escritorio, swaylock no ve el bloqueo y acaba con error.
timeout 30 target/release/compositor-pc "$R" --salir-sin-clientes > "$T/comp2.txt" 2>&1 &
COMP=$!
for _ in $(seq 50); do [ -S "$R" ] && break; sleep 0.1; done
WAYLAND_DISPLAY="$R" timeout 5 swaylock -u -c 00ff00 > "$T/swaylock-sin.txt" 2>&1 && SIN=0 || SIN=$?
wait "$COMP" 2>/dev/null || true
if [ "$UNO" = "00ff00 00ff00 00ff00" ] && [ "$DOS" = "000000 000000 000000" ] && [ "$CLI" = 124 ] && [ "$SIN" != 0 ] && [ "$SIN" != 124 ]; then
    echo "G5: swaylock tapa la pantalla, la ventana nueva no se ve ni pinta, si muere sigue tapada, y sin el poder no bloquea: bien"
else
    echo "G5: MAL (con swaylock: $UNO; sin él: $DOS; cliente $CLI; sin poder $SIN)"; cat "$T/swaylock.txt" "$T/swaylock-sin.txt" "$T/comp.txt"; exit 1
fi
rm -rf "$T"
