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
