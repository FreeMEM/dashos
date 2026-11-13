#!/usr/bin/env bash
# S29 (docs/diseño/S29-malware-linux-y-dashos.md): el malware de laboratorio del portapapeles, en el PC. El compositor de
# DashOS en el PC (compositor-pc, con --pegar-con-entrada como en DashOS) y el puente de la caja Linux; un programa
# nativo copia una contraseña (wl-copy, directo) y un programa de Linux (wl-paste, por el puente) la pide sin que nadie
# pulse una tecla ni haga clic: se lleva el teclado al abrir su ventana, recibe la oferta, pide el texto y no le llega.
# Lo mismo un programa nativo. Sin --pegar-con-entrada (el compositor de antes), sí le llegaba: se mira también.
set -eu
cd "$(dirname "$0")/.."
command -v wl-copy >/dev/null || { echo "S29 portapapeles: sin wl-clipboard, no se prueba"; exit 0; }
cargo build --release --offline --quiet
T=$(mktemp -d)
R=${XDG_RUNTIME_DIR:-/tmp}/dashos-s29-$$
mal=0
probar() {
    local opcion=$1 esperado=$2
    timeout 60 target/release/compositor-pc "$R-directo" --transporte "$R-tr" $opcion > "$T/compositor.txt" 2>&1 &
    local C=$!
    for _ in $(seq 50); do [ -S "$R-tr" ] && break; sleep 0.1; done
    timeout 60 target/release/puente "$R-puente" "$R-tr" > "$T/puente.txt" 2>&1 &
    local P=$!
    for _ in $(seq 50); do [ -S "$R-puente" ] && break; sleep 0.1; done
    printf '%s' "contraseña-del-banco" | WAYLAND_DISPLAY="$R-directo" timeout 10 wl-copy 2>/dev/null &
    sleep 1.5
    local linux nativo
    linux=$(WAYLAND_DISPLAY="$R-puente" timeout 5 wl-paste -n 2>/dev/null || true)
    nativo=$(WAYLAND_DISPLAY="$R-directo" timeout 5 wl-paste -n 2>/dev/null || true)
    local negados
    negados=$(grep -c "pidió pegar sin tecla ni clic" "$T/compositor.txt" || true)
    kill $P $C 2>/dev/null || true
    wait $P $C 2>/dev/null || true
    rm -f "$R-directo" "$R-puente" "$R-tr"
    if [ "$esperado" = negado ]; then
        if [ -z "$linux" ] && [ -z "$nativo" ] && [ "$negados" -ge 2 ]; then
            echo "S29 portapapeles: wl-paste sin tecla ni clic, de Linux y nativo: no lee nada ($negados negados): bien"
        else
            echo "S29 portapapeles: wl-paste sin tecla ni clic: MAL (Linux \"$linux\", nativo \"$nativo\", $negados negados)"; mal=1
        fi
    else
        if [ "$linux" = "contraseña-del-banco" ]; then
            echo "S29 portapapeles: sin la comprobación (como antes), wl-paste de Linux lee la contraseña: así era"
        else
            echo "S29 portapapeles: sin la comprobación, wl-paste no lee (\"$linux\"): MAL"; mal=1
        fi
    fi
}
probar --pegar-con-entrada negado
probar "" leido
rm -rf "$T"
exit $mal
