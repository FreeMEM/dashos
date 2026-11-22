# Unión: las ramas de DMS y de los iconos

Rama `union-dms`, copia de `fusion-fondo` (009e7dc9: `revision` 7ef8de03 más `firefox-firme`). Encargo: unir, una a
una y en este orden, `dms-barra`, `dms-avisos`, `dms-dock`, `iconos` e `iconos-cajas`, con las dos partes de cada
conflicto. Leídas antes las bitácoras de las cinco ramas y las de `cuentas-2` y `union-menus`.

## Las uniones

| Commit | Rama | Conflictos y cómo quedan |
|---|---|---|
| 0f5402c8 | `dms-barra` | `centro.rs`: Cuentas (de `cuentas-2`) y Barra; el editor de la barra entra en la columna de las dos maquetas. Catálogos: Barra de 104-105 a 124-125. |
| 11e1d7b2 | `dms-avisos` | `barra.rs`: la barra de `dms-barra` con la vista de avisos, el Dash y sus teclas, ratón y rueda. `centro.rs`: Avisos y Centro de control; dos `PUERTA_BARRA`. `hostil.rs`, `arranque_virt.txt`, `prueba_uefi.sh`. |
| 74bddee8 | `dms-dock` | `panel.rs`: el de `dms-avisos` sin el OSD (pasa al dock). `centro.rs`: Dock; sus textos de 104-127 a 126-149. `inicio.rs`, `Makefile`, `prueba_uefi.sh`. |
| f17f98e3 | `iconos` | `barra.rs` y `barra_widgets.rs`, `panel.rs`: Material Symbols donde ahora se pintan; las categorías nuevas con su símbolo. |
| 60f5f874 | `iconos-cajas` | `explorador/iconos.rs`, `pintar.rs`, `entrada.rs`, `explorador.rs`, `disco_archivos.py`, `prueba_uefi.sh`. |

### Decisiones de la unión

- **El orden de las categorías de Ajustes**: las de antes (Permisos ... Acerca de) no se mueven; detrás, Cuentas (8),
  Barra (9), Avisos (10), Centro de control (11) y Dock (12). `dms-barra` ponía Barra antes de Acerca de, pero el guion
  de `make arranque-virt` baja con las flechas hasta Acerca de dentro del plazo de 60 s de `guardian`, y `cuentas-2` vio
  que una categoría más delante no llegaba. La captura de Ajustes > Barra del guion baja ahora dos (antes subía una).
  Ninguna otra prueba llega a estas categorías por la posición: se abren por su nombre ("abrir barra", "abrir cuentas").
- **Los números de catálogo de `centro`**: Cuentas 104-123 (estaba en la base), Barra 124-125, Dock 126-149, Avisos
  200-239. Los `.cd` y `.ct` se regeneraron en orden con `tools/catalogo.py` (sin cambiar ningún texto) y pasan
  `catalogo.py todo --comprobar`.
- **La barra con los avisos**: los objetos de las barras de `dms-barra` (desde 100, cuatro por barra y monitor) chocaban
  con los de `avisos_vista` (100-107) y `dash_reloj` (110-113), y su memoria (VA+8) con la de los emergentes: las barras
  pasan a 120 y a VA+11. Los emergentes, el centro de avisos y el Dash piden su capa sin monitor, como en `dms-avisos`:
  salen en el monitor por defecto, con el ancho del primero (`ancho_principal`). El margen derecho del centro de avisos
  es el `LADO` de la barra de antes (10). El reloj de la barra abre el Dash: acción nueva `Reloj` en `barra_widgets`.
- **Dos `PUERTA_BARRA` en el centro**: la de `dms-barra` (el timbre de la barra para su configuración) se queda; la de
  `dms-avisos` (los ajustes de los avisos) pasa a `PUERTA_AVISOS_BARRA`.
- **El OSD del volumen** es del dock (como `dms-dock`); el panel de `dms-avisos` lo pierde igual que lo perdió el viejo.
- **Los iconos**: lo de `iconos` se lleva a donde ahora se pinta cada cosa. En la barra, en `barra_widgets.rs` (los
  dibujados a mano se quitan, como hizo `iconos`); en el panel de `dms-avisos`, todos sus iconos, también los de sus
  mosaicos nuevos. Hicieron falta 11 símbolos más, bajados con `tools/iconos_material.py` del mismo commit fijado de
  Google: settings, bluetooth, mic, do_not_disturb_on, chevron_left, chevron_right, undo (panel) y manage_accounts,
  toolbar, tune y dock_to_bottom (las categorías Cuentas, Barra, Centro de control y Dock; Avisos lleva notifications).
- **Órdenes y llamadas**: `ESCRIBIR_ENV` (91) y `R_AJUSTE_ESCRIBIR` (91, clase del registro) en `sesiones`, y
  `RAM_ESCRIBIR` (121) en `archivos`, de `dms-dock`, no chocan con nada (`cuentas-2`: 130-134 y `CAMBIAR_CLAVE` 42). Los
  canales nuevos de la barra ("barra suscribir", "avisos vigilar", "cc vigilar", "cc guardar") son textos distintos.
- **La licencia MIT de DMS**: las tres ramas traían el mismo fichero (`docs/licencias/DankMaterialShell-MIT.txt`, mismo
  texto): queda uno.
- **El tema**: `scheme-monochrome` de `dms-barra` entra tal cual; `dms-avisos` y `dms-dock` usan los papeles del tema y
  lo toman sin cambios.

## Lo que salió al probar la unión (y se arregló)

Con `firefox-firme` dentro, el guion de `make arranque-virt` llega más lejos que en cada rama por separado, y la prueba
solo dice la primera línea que falta: salieron cosas que ninguna de las cinco ramas había visto pasar.

| Commit | Qué | De dónde venía |
|---|---|---|
| d6430cc3 | El disco de pruebas se quedaba sin sitio a mitad de A15 ("[comprobador] A15 la foto: bien", dos de dos; "[archivos] queda poco sitio: se suelta el disco sujeto"). `rota.png` sale de `atardecer` (23 KB) y no de `ondas` (153 KB): +32 bloques. | La unión: 355 bloques libres de 1535 (la base, unos 406: `C/Ordenes` con el importador de DMS +57 KB, el Shell +8 KB, Exchange +8 KB y unos 16 ficheros y cajones pequeños, los `.info` y el perfil de shiva). Con el arreglo, 387, y A15 sale bien. |
| b7439571 | Al quitar `ENV:Escritorio/centro-control` (o `avisos`) y no valer lo de debajo, lo de serie, como al arrancar ("vuelven al quitar el fichero"). Si lo que no vale está en `ENV:`, lo que había. | `dms-avisos` (su prueba lo pedía y no se llegó a ver). |
| 6e6e648c | `prueba_uefi.sh`: `capa_tras` se usaba antes de su `def` (NameError: el guion paraba en los avisos). | `dms-avisos`. |
| 573828c4 | El centro de avisos no pasa del alto útil de la pantalla: en 800 × 600 con el historial lleno, "Borrar todo" quedaba en y = 736. | `dms-avisos` (solo probado a 1920 × 1080). |
| bfc7785d, 4ee91369 | `ram` y sus líneas: GUARDAR_ENV con un nombre que sale del cajón es -3 y el porqué del registro es el nuevo. | `dms-dock` (cambió `sesiones` y no se llegó a esas líneas). |
| 4269ad55 | La orden `Registro` guardaba lo visto en un array de 3 y el registro común tiene 4 ficheros (dominio-red): pánico. | La base (`fusion-fondo`). |
| 0bc9c1fa | `make arranque-virt` con 900 s (antes 720): con el dock, los avisos, el Dash y los iconos de la caja Amiga no llegaba al último paso. `TIEMPO_VIRT` lo cambia. | La unión. |

## Pruebas

- `make prueba`: BIEN, tras unir las cinco y otra vez al final, con todos los arreglos.
- `cargo test`: kit 93, wayland 139, shell 71, iconos-cajas 19, bien. Las de PC de `usuario` (barra_config 14,
  json_mini 2, cc_rejilla 6, protocolo/avisos 13, avisos_ajustes 5, dms_medidas 4, calendario 4, muelle 7,
  dock_ajustes 11, dock_modelo 29, osd_modelo 23, volumen_osd 2, explorador 25): bien. `catalogo.py todo --comprobar`:
  bien.
- `make arranque-virt-dms`, `-iconos`, `-escritorio`, `-shell`, `-menus`, `-cuentas`, `-2` y `-wayland`: BIEN, a la
  primera, en una tanda seguida.
- `make arranque-virt`: ocho vueltas. Las siete primeras fueron sacando lo de la tabla de arriba, una cosa cada vez.
  La octava (con todo lo de arriba menos los 900 s ya puestos) pasó todo hasta el explorador: el comprobador A15, el
  panel con su rejilla y su edición, el dock (OSD del volumen, traer, menú, fijar, esconderse, Bloq Mayús), los avisos
  (emergente y su acción, centro, historial, Borrar todo), el Dash y el explorador hasta la lista de `Home:Privado`. Ahí
  falló "[explorador] ventana 1: por Tamano, de menos a más: nota.txt": la búsqueda del explorador se llenó de "a"
  ("busca \"aaaa…\""), la familia conocida de teclas perdidas con carga que `firefox-firme` apunta como la que le queda
  en `arranque-virt` ("el explorador (búsqueda llena de "a")"). No es de una ventana que robe el teclado: es una tecla
  que se queda pulsada. En otra vuelta (la séptima) faltó "[dash-ventana] pulsado", también de esa familia, y en ella el
  guion no llegó a desbloquear la sesión a tiempo.

## La captura de shiva

`~/recuperar/geosian-7d/union-dms-capturas/` (fuera de git), a 1920 × 1080 con la imagen de usar y una copia del disco
de pruebas con el fondo del usuario (`Fuji-Dark`, el de `dms-barra`, solo en ese disco). Entra Ana, abre un Shell desde
el lanzador y teclea `Ajuste tema oscuro` (su apariencia del disco de pruebas trae a propósito un modo que no vale) y
`Ajuste DMS SYS:Prefs/Presets/DMS/shiva.json GUARDAR`; pone el fondo en Ajustes > Apariencia (abierto con el botón
derecho sobre la barra), saca el dock y abre el centro de control:

- `shiva-escritorio-barra-dock-centro.png`: la barra de shiva, el centro de control (rejilla de DMS, monocromo oscuro),
  el dock abajo (solo el lanzador: no hay ventanas abiertas) y el panel del monitor del fondo.
- `shiva-dock.png` (el dock a la vista) y `shiva-tras-ajuste-dms.png` (con el Shell, tras el `Ajuste DMS`).

El guion de la captura (`captura_shiva.py`) no va en git: se lanzó a mano contra el monitor de QEMU de una vuelta de
`tools/prueba_uefi.sh` sin guion propio.

## Pendiente del usuario

1. **El orden de las categorías de Ajustes**: las nuevas, tras Acerca de (Cuentas, Barra, Avisos, Centro de control,
   Dock). `dms-barra` puso Barra antes de Acerca de; se movió por el plazo del guion. Si se quiere otro orden, hay que
   subir el plazo de `guardian` para ese tramo o cambiar el guion.
2. **Dónde salen los avisos con varias barras**: los emergentes, el centro de avisos y el Dash salen en el monitor por
   defecto, no junto a la barra pulsada (con dos monitores, siempre en el primero). Antes de la unión, `dms-barra`
   ponía el toast y el historial junto a la barra de la que se pulsaban.
3. **Los iconos nuevos**: manage_accounts (Cuentas), toolbar (Barra), tune (Centro de control), dock_to_bottom (Dock), y
   en el panel settings, bluetooth, mic, do_not_disturb_on, undo y los chevrones. Elegidos aquí.
4. **El fondo de la captura no sale solo**: el programa `fondo` arranca antes que el servicio de archivos, no puede leer
   el fondo guardado y se queda el color liso hasta que se elige otro en Ajustes (ya pasaba antes de la unión; no se ha
   tocado).

## Lo que queda

- El dock dibuja a mano el icono del lanzador, los glifos de los OSD y los de su menú: falta pasarlos a Material Symbols,
  como se hizo con la barra y el panel (no estaba en ninguna de las dos ramas).
- `make arranque-virt` entero no ha salido bien ninguna vez: la última vuelta se paró en el explorador por la familia de
  teclas perdidas (la búsqueda llena de "a"); lo de después (el resto del explorador, `guarda`, las commodities y el
  reinicio) no se ha visto pasar en la unión.
- La comprobación de A15 del comprobador dice "MAL" en vez de "interrumpido" cuando el servicio suelta por falta de sitio
  la sujeción de otro (`SOLTADA_POR_SITIO` guarda solo el último); no se ha tocado.

## Lo que no se ha comprobado

- Los iconos nuevos del panel y de Ajustes, vistos solo en la captura del panel (no Ajustes con sus categorías nuevas).
- Dos monitores con avisos y Dash (`arranque-virt-2` no los abre).
- `make arranque-virt-linux` (no estaba en el encargo).
- En la Pi.
