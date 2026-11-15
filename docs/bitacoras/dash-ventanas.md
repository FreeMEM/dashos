# Ventanas de Dash en DashOS (kit-c)

Rama `dash-ventanas`. Los programas de Dash con ventana (las funciones `mui_*`) en DashOS nativo, con el kit (Material
3). La bitácora completa y las decisiones, en Dash: `~/Projects/FreeMEMLang/docs/DASHOS_VENTANAS_LOOP.md` (rama
`dashos-ventanas` de Dash).

## Qué hay aquí

- `kit-c/`: el kit con ABI de C (`staticlib`, `libkit_c.a`; cabecera `kit-c/include/kit_c.h`). Usa `kit`, `wayland`, el
  cliente de `usuario/src/ventana_cliente.rs` y el crate `dashos`. Su montón es el `malloc` del programa de Dash.
  Objetivo `aarch64-unknown-none` (con FPU) y código PIC.
- `make -C kernel kit-c` (lo pide `usuario`, así que lo hacen `make`, `make prueba` y `make arranque-virt`):
  `build/libkit_c.a` con el `memcpy` de `dashos` débil, y, si hay Dash en `DASH_DIR` (de serie
  `~/Projects/FreeMEMLang`), la prueba `build/dash-ventana` (`kit-c/pruebas/ventana.dash`) y su catálogo
  `build/DashVentana.catalog` (`ventana_es.ct`, UTF-8). Sin Dash, o con uno que no sepa de kit-c, la imagen va sin la
  prueba (`build.rs` pone `cfg(dash_ventana)` solo si está el ELF) y sus líneas faltan.
- `dash-ventana` lo lanza `inicio` (solo en `virt`, al final de `PROGRAMAS`) con "ventanas", "archivos", "sesiones"
  y "selector": llaves 1 ventanas, 2 archivos, 3 sesiones (no la usa) y 4 selector (`inicio` da los canales en el
  orden de su lista, no en el del programa). Ficha de Ana encerrada en `Estado/Servicios/dash-ventana`: lee
  `Locale/Catalogs/español/DashVentana.catalog` de ahí (lo pone `tools/disco_archivos.py`; en la raíz del disco
  cambiaría lo que listan otras pruebas).
- DashOS no dice qué es cada llave: el runtime de Dash lo sabe por cuántas trae el programa (3, lo de `sesiones`: 1
  su canal, 2 archivos, 3 ventanas; 2 o 4, lo de `inicio`, con el compositor en la 1 y el selector en la 4) y se lo
  dice a kit-c (`dash_ui_usar_llave`). Si `sesiones` reparte algún día el selector, o cambia cuántas da, hay que
  tocar `runtime/dashos/plat.c` de Dash.
- Los ficheros de Dash (`file_*`, `dir_*`) van por el servicio "archivos" en la llave 2, desde la raíz del cliente
  (`runtime/dashos/dos.c` de Dash): `SYS:` y `PROGDIR:` son la raíz, `LOCALE:` su `Locale/`. Provisional hasta que lea
  las asignaciones (`dashos::asignaciones`, ya en `revision`). La ventana de la sesión se proyecta para escribir.
- El selector (`file_open_dialog`, `dialog_file_at`): "abrir" da una asa para leer (el programa la ve como
  `Elegido:`, sin nombre: el selector no lo dice); "carpeta", la raíz 3 para leer y escribir (`Carpeta:`). Una segunda
  carpeta no se puede ceder (la raíz 3 ya está).
- La prueba (`tools/prueba_uefi.sh`, al principio de la vuelta), cuando `guardian` acabó su prueba (sus bloqueos se
  llevarían los clics), tras el selector del hostil y el desplegable de Ajustes, sin centro, preguntas ni bloqueo: un
  clic en el margen de la ventana para traerla delante, una captura (`target/dash-nativo.ppm`), un clic en "Añadir";
  en la pregunta de `mui_request`, otra captura (`target/dash-nativo-pregunta.ppm`) y un clic en "Guardar". El
  programa guarda la lista en su cajón, la mueve a un cajón nuevo, lo lista, la lee y lo borra todo (el disco queda
  igual). Luego el selector: doble clic en Privado y en nota.txt (el programa la lee: 18 bytes) y, para guardar,
  "Elegir esta carpeta" en C (escribe, lee y borra `dash-lista.txt`). Cada paso, como mucho 30 s; mientras, los pasos
  de abajo esperan. `kernel/pruebas/arranque_virt.txt` espera sus líneas.
- La salida de `Print` en DashOS sale por líneas (el runtime de Dash la guardaba hasta 512 bytes o el final).

## Pendiente

- Los menús van en una fila dentro de la ventana: S10 los quiere en la barra de la pantalla con el botón derecho
  (falta el protocolo).
- `sesiones` no reparte el selector: un programa de Dash lanzado desde el disco no puede abrir el diálogo de ficheros.
  Para el editor de catálogos de `SYS:Utilities` hace falta que lo dé (o que el programa lo pida por su canal).
- El selector solo devuelve la asa: el programa no sabe el nombre de lo elegido (para el título de la ventana, por
  ejemplo). Y no tiene campo de nombre para guardar.
- Las asignaciones (`ENV:`, `RAM:`, `Work:`, `LOCALE:` de verdad) en el runtime de Dash.
- La fase de reinicio y la prueba U6 (`captura_cambio.py`): ver la bitácora abajo.

## Bitácora

- 2026-11-15: `make arranque-virt` (con el reinicio) y `make prueba`, bien, con la prueba entera de `dash-ventana`:
  ventana, "Añadir", la pregunta de `mui_request` ("Guardar" da 1), los ficheros en su cajón y el selector (abrir
  nota.txt, guardar en C). Capturas en `target/dash-nativo.ppm` y `target/dash-nativo-pregunta.ppm`. De cinco vueltas:
  la primera falló en `captura_cambio.py` (U6: la captura de Ana salió con la sesión bloqueada, una carrera del guion
  que no es de esta rama) y la cuarta se quedó parada en el paso del centro de control (Permisos en pantalla y nada
  más; `dash-ventana` aún no había empezado), sin repetirse en la quinta. En C, el selector tenía un cajón más que
  cuando lo usa el hostil: el guion busca Privado por su nombre (si entra en otro, Retroceso y la fila siguiente).
