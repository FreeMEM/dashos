# Bitácora: un escritorio como DMS (S27)

Rama `escritorio-dms`, sacada de `revision`. Petición del usuario (2026-11-13): "Quisiera conseguir un escritorio 'DMS
like'. Sería brutal." Estudio y plan por oleadas en `docs/diseño/S27-escritorio-dms.md`.

Sala limpia: de DMS solo el README, su documentación, sus notas de versión y sus capturas (las miró un agente aparte, que
las describió en prosa). Nada de código de DMS, Quickshell, niri ni matugen.

## 2026-11-13: estudio y primera oleada

### Antes

- `DASHOS_PANTALLA=1920x1080 make arranque-virt` con `CAPTURA_CENTRO`, `CAPTURA_BARRA`, `CAPTURA_MENU`,
  `CAPTURA_AVISOS` y `CAPTURA_BLOQUEO`: capturas en `capturas-agente/antes/` (fuera de git). A 1920 × 1080 la prueba
  no puede dar BIEN (espera 800x600), y el guion no llega a todos los pasos (los avisos no salieron): sirve solo
  para ver.
- La barra de antes: una tira opaca de lado a lado, tres botones redondos, la hora y el título en medio. El lanzador,
  una hoja de 360 pegada a su botón.

### Lo hecho

**Compositor** (`wayland/src/escena.rs`, `usuario/src/bin/compositor.rs`), en piezas pequeñas:

- Espacios de trabajo del 1 al 9. `Xdg::espacio` guarda el de cada ventana (el que se veía al salir). `fuera(v)` junta
  lo que no se ve (sesión de detrás, U6, u otro espacio) y lo usan `raices`, `ventana_de_arriba`, `dar_teclado` y el
  cambio de sesión. Las subsuperficies miran el espacio de su raíz (`fuera_raiz`).
- Los avisos de `frame` de las ventanas de otro espacio salen a ritmo bajo, por el mismo camino que los de U6.
- Super+1..9 cambia; Super+Mayúsculas+1..9 lleva la ventana con el teclado (sin ir con ella). Al cambiar a un espacio
  con ventanas y sin nadie con el teclado, se le da a la de arriba (faltaba: la prueba lo pilló).
- Teclas del volumen: HID 0x7F, 0x80 y 0x81 en `teclas.rs` (la prueba contra `input-event-codes.h` las incluye). Si el
  panel está suscrito, van a él (evento `volumen`) y no a la ventana.
- `dashos_tema_v1` versión 3: petición `espacio` (solo la barra), eventos `espacios` (solo a la barra, al suscribirse y
  al cambiar) y `volumen` (solo al panel). Tablas regeneradas con `tools/wayland_tablas.py`. Todos los globales del tema
  pasan a la 3, así el hostil puede probar que pedir `espacio` sin ser la barra es un error de protocolo (con la 2 ya
  lo sería, pero por la versión, no por el permiso).
- Referencia al día: `docs/referencia/servicios.md`.

**Barra** (`usuario/src/bin/barra.rs`):

- La capa sigue midiendo 40 con zona exclusiva 40: las ventanas, las capas y las pruebas no se mueven. Lo que se ve es
  una tira de 32 a 6 de los lados y 4 de arriba, en el contenedor de superficie al 88 %, y píldoras de 24 en el
  contenedor alto: lanzador, espacios, título, reloj con fecha, campana y estado (red y sonido, abre el panel).
- `disponer()` calcula dónde va todo; lo usan el pintado, el ratón y las líneas para las pruebas.
- La fecha, con `usuario/src/reloj.rs` (días civiles; sigue en UTC).
- El lanzador, en el centro de lo que deja la barra (capa sin anclas), de 600 de ancho, con hasta 7 filas de 56 y un
  pie con las teclas y cuántos casan. La lista va en `usuario/src/lanzador_lista.rs`. Lista hasta 1000 programas.
- Sin colores a mano: todo son papeles del tema del kit (primario, contenedores, contorno, error), así sigue el color
  del fondo (C3) y lo que traiga `aspecto-cajas`.

**Panel rápido** (`usuario/src/bin/panel.rs`): enlaza la 3. Con el evento `volumen`, sube o baja 5 % o pone el
silencio (`usuario/src/volumen_osd.rs`), lo manda al servicio de sonido y enseña el OSD: una capa de encima de todo,
abajo en el centro, 320 × 56, contenedor alto, altavoz, pista y valor. Dura 1,5 s tras la última tecla; una ráfaga solo
repinta la misma capa y al cerrarse dice cuántas pulsaciones juntó. Su memoria se pide al abrirlo y se suelta al
cerrarlo. El mosaico del volumen dice "Silencio" con el silencio puesto.

### Pruebas

- Escena (`cd wayland && cargo test --lib`): `espacios_de_trabajo_y_volumen`, nueva. Con hostiles: números 0, 10 y
  `u32::MAX`, la barra de la versión 1 pidiendo la 3, un programa cualquiera pidiendo `espacio`, 50 pulsaciones del
  volumen seguidas, y todo con la sesión bloqueada.
- PC (`make prueba` las corre): `volumen_osd.rs` (ráfagas de 10 000 teclas y pasos raros; el OSD junta 500 pulsaciones
  en una), `lanzador_lista.rs` (5000 programas con nombres de 120 eñes, letras de control, 20 000 flechas) y `reloj.rs`
  (fechas conocidas, bisiestos, 10 000 valores al azar y `u64::MAX`).
- Hostil (virt): cambiar de espacio sin ser la barra, con 2, 0 y `u32::MAX`: error de protocolo. Línea nueva en
  `arranque_virt.txt`.
- Virt (`tools/prueba_uefi.sh`, tras el panel en el paso de la barra): Super+2, un clic en el 1 de la barra, y seis
  teclas de bajar el volumen y una de subir seguidas: un solo OSD que junta las 7. Con `CAPTURA_BARRA`, también
  `.espacio2.ppm` y `.osd.ppm`. El final del paso de la barra pasa de 8 a 11 (los avisos y el Workbench lo esperan).
- `tools/captura_ventanas.py` mira la barra en (8, 20), dentro de la tira (en (5, 5) ahora se ve el fondo).
- En el guion, ir a la esquina antes de un clic de la barra o de los avisos mueve 2000 y no 1200: a 1920 de ancho no
  llegaba.

### Capturas (fuera de git, `capturas-agente/`)

- `antes/`: la tanda de 1920 × 1080 sin tocar nada.
- `despues/`: la misma tanda con la primera oleada. Ahora el guion llega a todos los pasos también a 1920 (avisos
  incluidos): `barra.png` (lanzador), `barra.busca.png`, `barra.panel.png`, `barra.espacio2.png` (en el 2, la ventana
  del 1 no se ve y su punto sale ocupado), `barra.osd.png`, `avisos*.png` y las de siempre.
- `v1/`: las de `make arranque-virt` a 800 × 600 (la prueba de verdad), con el OSD y el espacio 2.
- Comparadas: `comp-barra.png` (arriba de la pantalla, antes y después, con el panel abierto) y `comp-lanzador.png`.
- Las de DMS que miró el agente de las ideas, para comparar: en el directorio de trabajo de la sesión (no van al repo).

Comparado con DMS: misma forma (tira flotante, píldoras, el espacio que se ve alargado y en el acento, reloj con fecha
en medio, estado en el acento a la derecha, OSD abajo en el centro). Distinto a propósito: Material 3 de verdad en los
papeles (DMS usa grises fijos con algo del acento), la barra algo más alta (32 frente a unos 28: las píldoras de 24 dejan
el texto de 14 de Material 3), y el OSD en el contenedor alto en vez de casi negro.

### Resultado

- `cd wayland && cargo test --lib`: 68, bien. Pruebas de PC nuevas: bien.
- `make prueba`: BIEN (esperado.txt y cargador hunk).
- `make arranque-virt`: las cinco BIEN a la primera (arranque, disco, captura, sonido y la ventana de Ana).
- `-vm`, `-linux`, `-2` (las dos BIEN), `-diag`, `-escritorio`, `-usb`, `-wayland` y `-gpu`: BIEN.
- `-arch`: la primera vez, MAL. Linux no recibió el `poweroff` final. Justo antes, la ventana del programa de Dash de la
  caja Amiga salió y se llevó el teclado (nadie tecleaba desde hacía rato), y al cerrarse a los 40 s el teclado quedó
  sin dueño ("título: (ninguno)"): el compositor solo devuelve el teclado a la ventana de arriba cuando se va una
  *capa* que lo tenía, no cuando se va una ventana. Eso ya era así en `revision`; es una carrera de tiempos con la
  máquina cargada (otra sesión corría a la vez su prueba de Arch). Se repitió y falló igual, dos veces más: la ventana
  de Dash sale siempre en ese punto de la prueba.
- Arreglo (en el compositor, pequeño): al cerrarse la ventana que tiene el teclado, o al irse su cliente, el teclado
  pasa a la ventana de arriba que queda, como ya pasaba al cerrarse una capa. Antes se quedaba sin dueño hasta un clic,
  y lo que se tecleaba se perdía (le pasaría a cualquiera que cierre una ventana y siga escribiendo). Pruebas de la
  escena: `el_teclado_vuelve_al_cerrar_una_ventana` (nueva) y `asiento_puntero_y_teclado` (al irse el cliente, el
  teclado pasa a la otra ventana; antes esperaba que no lo tuviera nadie).

### Resultado tras el arreglo del teclado

Todo otra vez, con Arch primero: `-arch` BIEN (antes, tres veces MAL), `make arranque-virt` las cinco BIEN,
`make prueba` BIEN, y `-vm`, `-linux`, `-2` (las dos), `-diag`, `-escritorio`, `-usb`, `-wayland` y `-gpu` BIEN.
`cd wayland && cargo test --lib`: 69, bien.

### Pendiente

- La píldora de estado no enseña el estado de verdad (red conectada, volumen, silencio): oleada 2.
- Los espacios son de todo el escritorio, no de cada pantalla (multimonitor): sección 3 del estudio.
- El lanzador mide lo que piden todos los programas (hasta 7 filas) aunque al buscar queden menos; DMS hace igual.
- El OSD no se repinta si cambia el tema mientras se ve (dura 1,5 s).
- La sombra del compositor va bajo toda la capa de la barra (40), no bajo la tira (de 4 a 36): no se nota en las
  capturas, pero no es exacta.
- La hora sigue en UTC.

## 2026-11-13: segunda oleada (centro de control y estado real en la barra)

El usuario aceptó las siete recomendaciones de la primera oleada. Antes, `git merge revision`: con el aspecto común
(S26), la versión 3 de `dashos_tema_v1` es la de las cajas y los espacios y el volumen pasaron a la 4. Lo nuevo va en la
5. Decisión nueva del usuario: habrá cinco temas (Claro, Oscuro, AOS4, MOS, Magic WB); no los hago, pero todo lo nuevo
usa solo papeles del tema del kit.

### Lo hecho

- **Centro de control** (`panel.rs`): 440 de ancho. Cabecera con quien está delante (inicial, nombre y "Encendido hace
  N min"), Editar y Bloquear. Deslizador del volumen (el altavoz pone el silencio, la pista pone el nivel, − y + de 10
  en 10) y del brillo, apagado ("Sin control aquí": no hay servicio de brillo). Rejilla de dos columnas: red, modo
  oscuro, bloquear, con el icono en un cuadrado redondeado (luna, candado y red dibujados con el lienzo). La edición, la
  de antes. El mosaico del modo oscuro y los botones del volumen siguen diciendo dónde están: el guion no cambia.
- **Quién está delante**: `sesiones` contesta `DELANTE` (21) solo al panel y a la barra; a otro, -2.
- **La red**: el panel pregunta `ESTADO` al servicio de red al arrancar y cada 10 s (con 300 ms de plazo). Para eso,
  `inicio` le da los canales "sesiones" y "red", y el panel pasa tras los servicios de red en la lista (el primero que
  nombra un canal es quien lo lee). Sus llaves: 1 ventanas, 2 sesiones, 3 centro, 4 sonido, 5 red.
- **La barra**: el panel da el estado (red, nivel, silencio) al compositor con la petición `estado` de la versión 5
  (solo el panel; recortado), y el compositor se lo pasa a la barra. Icono de red tachado y en el color suave sin red;
  altavoz con cruz con el silencio y ondas según el nivel.
- **Menú de apagado**: no se hizo. DashOS no tiene cómo apagar ni reiniciar (ni llamada del kernel ni servicio).
  Pregunta 8 del estudio.

### Pruebas

- Escena: `el_estado_del_panel_a_la_barra` (nueva): un programa cualquiera no lo da, el recorte, sin repetir y una
  barra que llega tarde lo recibe al suscribirse. 86, bien.
- Hostil: dar el estado sin ser el panel (error de protocolo) y `DELANTE` sin ser el panel ni la barra (-2). Dos líneas
  nuevas en `arranque_virt.txt`, más tres del panel y la barra ("red: conectada (10.0.2.15)", "delante:", "estado:").
- Volumen: `poner` (la pista) en `volumen_osd.rs`, con su prueba.
- `make arranque-virt`: las cinco BIEN a la primera. `make prueba`, `-vm`, `-linux`, `-2`, `-diag`, `-escritorio` y
  `-usb`: BIEN. `-arch`, `-wayland` y `-gpu` (con `~/.cache/dashos-caja-linux`): abajo.

### Capturas (fuera de git)

- `capturas-agente/o2-antes/` (revision, 1920 × 1080) y `o2-despues/` (esta oleada): `centro.ppm.panel.png` es el centro
  de control abierto, y la barra se ve arriba en todas.
- `capturas-agente/comp-centro-control.png`: antes y después, lado a lado.
- `capturas-agente/o2-v/panel-recorte.png`: el centro a 800 × 600.

### Pendiente

- Apagar y reiniciar (pregunta 8) y el menú de apagado.
- El estado de la red sin el permiso de red (pregunta 9).
- El brillo: no hay quien lo lleve (en la Pi, el de la pantalla oficial o DDC).
- No molestar, Bluetooth y los demás mosaicos de DMS: cuando haya servicio detrás.
- La cabecera no se repinta sola cada minuto (el "Encendido hace" se pone al día al repintar).
