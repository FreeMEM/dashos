#!/bin/bash
# B4: capturas de FS-UAE de caja/pruebas68k/denise.exe, para el oráculo de la imagen (tests/denise_oraculo.rs). B6:
# también de sprites.exe (tests/sprites_oraculo.rs).
#
#   captura.sh DIR [PROGRAMA [N]]
#
# PROGRAMA es denise (lo de serie) o sprites; N, cuántas capturas (de serie, 45). Lo que escribe el programa queda en
# DIR/salida.txt. Corre el programa en FS-UAE (A1200, Kickstart 3.1 de $DASH_KICKSTART, por tools/emu_run.py de Dash, sin cambiarlo)
# dentro de un Xvfb propio, con la ventana de FS-UAE en él, y pulsa F10 cada segundo: F10 es action_screenshot
# (--set keyboard_key_f10=action_screenshot), que guarda en DIR tres PNG por pulsación: "full" (el cuadro entero de
# FS-UAE, 752 x 572: líneas 26 a 311 dobladas y posiciones horizontales $5C a $1D3, en píxeles de alta), "crop" y
# "real" (escalado a la ventana). La captura de "--screenshot" de emu_run no sirve aquí: la saca un programa Amiga
# (tools/amiga/shot.c) de la pantalla de Intuition, y denise.exe para el sistema y pone su propia lista de Copper.
# Después, huellas.py DIR [PROGRAMA] saca las huellas de cada modo o escena.
#
# Con uae_collision_level=full: el de serie de FS-UAE no ve el choque de los dos campos (bit 0 de CLXDAT).
#
# FS-UAE es compartido: va con el cerrojo /run/user/1000/dash-emu-red.lock (puede haber cola).
set -eu
salida=$(readlink -f "$1")
aqui=$(dirname "$(readlink -f "$0")")
programa="${2:-denise}"
teclas="${3:-45}"
exe="$aqui/../../../pruebas68k/$programa.exe"
mkdir -p "$salida"
export DASH_KICKSTART="${DASH_KICKSTART:-$HOME/Descargas/Telegram Desktop/kick40068.A1200}"
cat > "$salida/dentro.sh" <<EOS
#!/bin/bash
unset WAYLAND_DISPLAY
export SDL_VIDEODRIVER=x11 SDL_AUDIODRIVER=dummy ALSOFT_DRIVERS=null
python3 -I "$aqui/teclas.py" 4 1 $teclas > "$salida/teclas.txt" 2>&1 &
cd ~/Projects/FreeMEMLang && python3 tools/emu_run.py --machine a1200 --window --timeout 120 \
  --set keyboard_key_f10=action_screenshot --set screenshots_output_dir="$salida" \
  --set texture_filter=nearest --set scanlines=0 --set uae_collision_level=full "$exe" > "$salida/salida.txt" 2>&1
EOS
chmod +x "$salida/dentro.sh"
flock /run/user/1000/dash-emu-red.lock xvfb-run -a -s "-screen 0 1920x1200x24 -nolisten tcp" "$salida/dentro.sh"
