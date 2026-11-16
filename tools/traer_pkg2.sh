#!/usr/bin/env bash
# Trae `pkg2`, el código común del formato de paquetes 2 (versiones, requisitos, manifiesto, resolutor y comprobador de
# planes; MPL-2.0, `no_std` y sin dependencias), del proyecto Hangar a `third_party/pkg2`, con su especificación y sus
# vectores en `third_party/docs/formato` (las pruebas de pkg2 los buscan en `../docs/formato/vectores`).
#
# Como Emu68: fuera de git (`.gitignore`). Se trae tal cual: aquí no se cambia. Lo que le falte para DashOS se apunta en
# `docs/bitacoras/paq.md`, "Para la sesión de Hangar".
#
#   tools/traer_pkg2.sh [ORIGEN]      ORIGEN: la carpeta `servicio` de Hangar (por omisión ~/Proyectos/hangar/servicio)
set -eu
ORIGEN=${1:-${HANGAR_SERVICIO:-$HOME/Proyectos/hangar/servicio}}
AQUI=$(cd "$(dirname "$0")/.." && pwd)
DESTINO=$AQUI/third_party
if [ ! -f "$ORIGEN/pkg2/Cargo.toml" ] || [ ! -d "$ORIGEN/docs/formato/vectores" ]; then
    echo "No está pkg2 en $ORIGEN (hace falta pkg2/ y docs/formato/vectores/)" >&2
    exit 1
fi
mkdir -p "$DESTINO/docs"
rm -rf "$DESTINO/pkg2" "$DESTINO/docs/formato"
# Sin lo compilado (target/) ni las cachés de Python de la referencia.
mkdir -p "$DESTINO/pkg2"
cp -r "$ORIGEN/pkg2/Cargo.toml" "$ORIGEN/pkg2/src" "$ORIGEN/pkg2/tests" "$DESTINO/pkg2/"
[ -f "$ORIGEN/pkg2/Cargo.lock" ] && cp "$ORIGEN/pkg2/Cargo.lock" "$DESTINO/pkg2/"
cp -r "$ORIGEN/docs/formato" "$DESTINO/docs/formato"
find "$DESTINO/docs/formato" -name __pycache__ -type d -prune -exec rm -rf {} +
# La huella de lo traído, para saber de qué versión de Hangar sale lo que se compila.
( cd "$DESTINO" && find pkg2 docs/formato -type f | LC_ALL=C sort | xargs sha256sum | sha256sum | cut -c1-16 ) > "$DESTINO/pkg2/HUELLA"
echo "pkg2 traído de $ORIGEN a $DESTINO/pkg2 (huella $(cat "$DESTINO/pkg2/HUELLA"))"
