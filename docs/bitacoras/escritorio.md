# Bitácora: la barra de la pantalla y el lanzador (S10)

Rama `escritorio`, sacada de `revision`. Diseño en `docs/diseño/S10-escritorio.md` ("La barra de la pantalla",
"Lanzador, notificaciones y Commodities", "Quién hace qué", "Aspecto") y `docs/diseño/S10c-centro-de-control.md`.

## 2026-11-12: primera versión

### La barra

- Programa nuevo, `usuario/src/bin/barra.rs`. Lo lanza `inicio` (solo en virt, tras el panel), con los canales
  "ventanas", "sesiones" y "archivos".
- Es una capa de arriba a todo lo ancho: 40 de alto, con zona exclusiva de 40. No pide teclado.
- A la izquierda, el botón del lanzador. En medio, el título de la ventana con el teclado. A la derecha, la hora y el
  botón del panel rápido.
- La hora es la de `dashos::hora_ns`, en UTC: aún no hay zona horaria. Se repinta al cambiar el minuto.
- Material 3 con el tema del sistema, como el panel: la barra sobre el contenedor de superficie, botones de icono
  redondos y tonales, y capa de estado al pasar el ratón. El del lanzador se llena (primario) mientras está abierto.
  Los iconos (rejilla de aplicaciones, deslizadores, lupa) se dibujan con trazos dentro del programa.

### El lanzador

- Otra capa de la barra, arriba a la izquierda bajo su botón, con el teclado mientras se ve.
- Se abre con el botón o con Super sola. Se cierra con Esc, con Super o con el botón.
- Mide lo que piden sus programas al abrirse (hasta seis filas). Al buscar no cambia de tamaño.
- Lista los programas de `Programas/`: ficheros que se pueden ejecutar, por nombre. La barra solo ve ese cajón (su
  raíz en el servicio de archivos, `FICHAS` de `inicio`), sin derechos.
- Al escribir, busca: primero los que empiezan así, luego los que lo contienen. Flechas o Tab para elegir. Intro o un
  clic lanza.
- Lanzar no lo hace la barra: se lo pide a `sesiones` (orden `LANZAR`).
- Aspecto como el panel: esquinas de 28, buscador redondo en el contenedor alto, filas con la inicial en un círculo
  (contenedor primario) y lo elegido en el contenedor secundario, con "Intro" a la derecha.

### El compositor

- `dashos_tema_v1` crece (en el XML y en las tablas, regeneradas con `tools/wayland_tablas.py`):
  - Petición `panel`: abre o cierra el panel rápido, como Super+A. Solo la barra; a otro, error de protocolo.
  - Evento `lanzador`: Super sola (pulsada y soltada sin otra tecla, sin botón y sin otro modificador). Solo a la
    barra. La tecla sigue llegando a la ventana, como antes.
  - Evento `titulo`: el título de la ventana con el teclado. Solo a la barra.
- El título, decidido así para que sea seguro: es el de la ventana en la que escribe quien está delante. Si el
  teclado lo tiene una capa (el lanzador), el de la ventana de arriba. Con la sesión bloqueada, vacío. La barra nunca
  sabe el título de otra ventana. Cuando el compositor separe las sesiones (U6 paso 2), el teclado solo irá a la
  sesión de delante, y esto sigue valiendo.
- Las capas sin zona propia (0) se colocan dentro de lo que dejan las barras, y su tamaño 0 es ese hueco. Lo pide
  wlr-layer-shell. Así el panel rápido sale bajo la barra. Con zona -1 (un fondo de pantalla), la pantalla entera.
- Antes, una ventana maximizada no se movía si la barra salía después. Ahora, al cambiar el hueco, las maximizadas
  se mueven y se les pide el tamaño nuevo. Colocar en cascada y maximizar ya respetaban la zona exclusiva.
- El compositor dice dónde sale cada capa ("capa de panel en (x, y)"), como con las ventanas.
- Al cerrar una capa con el teclado (el lanzador, el panel), el teclado se perdía: nadie lo tenía hasta un clic.
  Ahora vuelve a la ventana de arriba, como cuando se va el cliente.
- El poder de escritorio se parte en dos. `guardian` sigue con todo. El panel y la barra solo tienen las capas: sin
  el bloqueo de la sesión ni los avisos de inactividad. Con el bloqueo, un programa podría enseñar una pantalla de
  bloqueo falsa.

### `sesiones`

- La barra puede lanzar, pero solo `Programas/<nombre>` (un nombre, sin cajones ni subir) y solo como la persona que
  está delante. Sin nadie delante, -2. Nunca como el sistema ni como otra persona.
- Fallo que abría esto, arreglado: `sesiones` reconocía a `guardian`, `entrada` y `centro` por el nombre de su ficha.
  Lo lanzado desde el disco se llama como su fichero. Con la barra, un `Programas/guardian` puesto ahí por cualquiera
  habría tenido los poderes de `guardian` (entrar, contestar preguntas...). Ahora solo cuenta el nombre que da
  `inicio` (marca por debajo de la de los lanzados). Lo comprueba `usuario/src/lanzables.rs`, compartido con la barra.
- Las pruebas de PC de `lanzables.rs`: `rustc --edition 2024 --test usuario/src/lanzables.rs` (2 pruebas). No están
  en `make prueba`: tocaría `kernel/Makefile`.

### Pruebas

- Escena (`cargo test --lib` en `wayland`), tres nuevas:
  - `la_barra_aparta_ventanas_maximizadas_y_capas`: la barra llega con una ventana maximizada y la baja; el panel sale
    bajo la barra; tamaño 0 sin zona y con -1; maximizar con la barra; al irse la barra, la ventana vuelve arriba.
  - `el_teclado_vuelve_al_cerrar_una_capa`: el lanzador se lleva el teclado y al cerrarlo vuelve a la ventana.
  - `la_barra_panel_lanzador_y_titulo`: el panel lo pide la barra y no otro; Super sola sí y con otra tecla, clic o
    Mayúsculas no; el título solo a la barra, una vez por cambio, y vacío con la sesión bloqueada.
- Virt (`tools/prueba_uefi.sh`, tras el panel): clic en el botón del lanzador, "ho" (queda hola), Intro (lo lanza
  `sesiones`), Super sola y Esc, y el botón del panel lo abre y lo cierra.
- Hostil: un programa cualquiera no ve las capas, enlazarlas da error, y pedir el panel también.
- Cambios en el guion por la barra: la cascada de ventanas empieza 40 más abajo. El clic de Ajustes va ahora según
  dónde dice el compositor que está, y los del panel suman dónde salió su capa. Maximizar pide 800x560.
- `tools/captura_ventanas.py`: arriba ya no se ve el fondo. Mira el fondo en (5, 60), bajo la sombra de la barra, y
  que la barra esté.
- Capturas (fuera de git): el lanzador con la búsqueda y la barra con el panel rápido abierto debajo.

Resultado: `make prueba` BIEN; `make arranque-virt`, las cuatro BIEN (a la tercera: la primera falló por el orden de
las llaves de la barra, la segunda por la comprobación de la captura).

Lecciones:

- Las llaves de los canales van en el orden en que se nombra cada canal por primera vez en la lista de `inicio`, no
  en el de la lista del programa. La barra pide "sesiones" antes que "archivos", pero le llegan al revés.

Pendiente:

- Zona horaria: la hora es UTC.
- Cerrar el lanzador con un clic fuera (hace falta un popup con agarre, `xdg_popup`).
- La barra no sabe si el panel está abierto: su botón no cambia de aspecto.
- Menús del programa activo con el botón derecho, bandeja, notificaciones (S10).
- Programas de la caja Amiga y de Linux en el lanzador (S7, S8): de momento, solo `Programas/`.
- Lo lanzado desde la barra aún no tiene llave del compositor: no abre ventanas. Es U6 paso 2.
- Las pruebas de PC de `lanzables.rs` no están en `make prueba` (tocaría `kernel/Makefile`).
