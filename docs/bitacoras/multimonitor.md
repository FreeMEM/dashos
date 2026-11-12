# Bitácora: multimonitor, primera versión

Rama `multimonitor`, sacada de `revision`. Petición del usuario: "debemos soportar multimonitor". Diseño en
`docs/diseño/S10-escritorio.md` ("Varios monitores") y `docs/diseño/S9-graficos.md` ("Pantallas y monitores").
Probado en QEMU `virt` con una virtio-gpu de dos cabezas. La Pi 4, con sus dos HDMI por el HVS, queda para la placa.

## 2026-11-12: primera versión

### El controlador de virtio-gpu

- Programa nuevo, `usuario/src/bin/virtio_gpu.rs`, en su proceso. Escrito con la especificación virtio 1.2: transporte
  PCI moderno (4.1), cola partida (2.7) y el aparato de GPU (5.7), solo 2D.
- `inicio` lo lanza si hay una virtio-gpu en el PCIe (fabricante 0x1af4, aparato 0x1050), con su permiso DMA, sus
  registros, su configuración del PCIe, su interrupción si la hay y el canal "ventanas". En el kernel solo cambian las
  líneas de `inicio` (buscar el aparato y la ficha del programa, al final de la lista para no mover las marcas).
- Lee las cabezas (`GET_DISPLAY_INFO`). A cada una: un recurso 2D en B8G8R8X8 (el XRGB del compositor), su memoria DMA
  en trozos de 4 MB, la pone en su salida y la deja en negro.
- Una cabeza sin monitor se usa igual, a 1024 × 768. Con QEMU sin ventana (`-display none`) solo la 0 dice tener
  monitor: así se prueban dos.
- Da cada cabeza al compositor por "ventanas", con su memoria (protocolo nuevo, `usuario/src/protocolo/pantallas.rs`).
  El compositor escribe ahí lo que compone y le pide por un canal propio que vacíe lo que cambió. El controlador manda
  `TRANSFER_TO_HOST_2D` y `RESOURCE_FLUSH` por cada rectángulo, todos los de un mensaje de una vez en la cola.
- El compositor no toca la DMA ni el aparato: solo escribe píxeles en la memoria que le da el controlador.
- Lo que manda el compositor no se cree: cabeza y rectángulos se recortan. Lo que devuelve el aparato tampoco.
- Sin virtio-gpu, nada cambia: el compositor sigue con la pantalla de UEFI.

### El compositor y la escena

- Las salidas van en un espacio común, una al lado de otra, de izquierda a derecha y arriba del todo. El búfer propio
  mide lo justo para todas.
- `Escena::poner_salidas`: la primera es la principal. Se quedan las que tienen tamaño, no se pisan y caben (hasta 8).
- Cada daño se parte por salidas. El hueco entre salidas de distinto alto no se compone. Cada trozo se copia a su
  salida; a las del controlador, por filas enteras.
- El cursor pasa de una a otra por donde se tocan. Si se sale de todas, se queda en el borde de la suya; en diagonal
  hacia una esquina donde se tocan dos, sigue por la otra.
- Un `wl_output` por salida: global 6 la principal (el de siempre), 11, 12 y 13 las demás. Nombre DASHOS-1,
  DASHOS-2..., y su sitio en `geometry`. Sin xdg-output: con escala 1 no hace falta.
- Si las salidas cambian con clientes dentro (el controlador llega después que ellos): `global` y `global_remove` a sus
  registros, y lo nuevo a sus `wl_output`. La barra recibe otro configure si cambia su ancho. Para eso la conexión
  recuerda sus registros (`Conexion::registros`, con tope).
- Las ventanas nuevas salen en la salida del cursor, en cascada; si no caben, se arriman para no salirse.
- Maximizar llena la salida de la ventana (la que más tapa), menos su barra.
- Super+Mayúsculas+Izquierda/Derecha llevan la ventana con el teclado a la salida de al lado. Guarda su sitio
  respecto a lo que queda para ventanas. Maximizada, sigue maximizada con el tamaño de la nueva.
- Arrastrar: Super y el botón izquierdo mueven la ventana, también de una salida a otra. El clic no llega a la ventana.
  No había forma de mover ventanas con el ratón; esta es la primera.
- Capas: van en la salida de su `wl_output`; sin él, o con uno que no lo es, en la principal. Cada salida tiene su
  zona para ventanas (la barra solo aparta las de la suya).
- Pantalla de bloqueo: una por salida, del tamaño de la suya y en su sitio. Dos para la misma salida es error. Una
  salida sin pantalla de bloqueo queda en negro. `guardian` pide solo la principal: la otra, en negro.
- La sombra de una ventana que está en una sola salida no pasa a la de al lado. La barra manchaba la esquina de la
  segunda: se vio en la primera captura.
- Una ventana que se queda fuera de todas las salidas (se fue un monitor) pasa a la principal.
- Coordenadas absurdas de los clientes (márgenes, daños, movimientos del ratón) ya no pueden desbordar: `Rect::cruce`
  en 64 bits, sumas con tope y márgenes recortados.

### El cargador de UEFI

- La pantalla del firmware se reserva siempre en el árbol. Con virtio-gpu, la de UEFI puede salir de memoria de los
  servicios de arranque, que queda libre al salir. Sin reservarla, el kernel podría dar esa memoria a otro mientras el
  compositor aún pinta en ella.

### Los guiones de virt

- `tools/guion_multimonitor.py`, nuevo: las teclas y el ratón de `make arranque-virt-2`. Va solo, sin el guion de
  siempre (TECLAS). Primero lo metí en ese guion y falló cuatro veces de cuatro maneras: sus pasos dependen de las
  preguntas, del selector, del centro y de los bloqueos, y con el PC cargado se cruzan. Contesta las preguntas de
  `guardian` (si no, cada una espera 30 s y el centro sale tarde).
- `tools/prueba_uefi.sh` lo lanza con `MULTI=prefijo`.
- Dos arreglos en el guion de siempre, de algo que no era mío, tras repetir: `make arranque-virt` fallaba igual en
  `revision` sin tocar (el desplegable de Ajustes y Super+L).
  - El bloqueo que pide `pide` ("se bloquea la sesión de Carla") contaba como bloqueo desde que se pedía, pero
    `guardian` no bloquea hasta que se cierra el centro de control. Con el PC cargado, `pide` lo pedía antes de abrirse
    el centro: el selector esperaba al bloqueo, el centro al selector y el bloqueo al centro, hasta que el selector
    caducaba al minuto; sus Intro iban a Ajustes. Ahora ese bloqueo cuenta desde que `guardian` dice que el centro se
    cerró.
  - Si el clic del paso de Ajustes llega mientras sale el selector, le quita el teclado y el selector no recibe nada.
    Al reintentar, un clic a la derecha del título del selector se lo devuelve.

### El centro de control

- Pantallas enseña todas: con una, como antes; con varias, una línea por pantalla con su tamaño y dónde está dicho en
  palabras (a la izquierda, a la derecha), y la frecuencia y la escala. Un dibujo de los monitores, como el de GNOME,
  queda para cuando se puedan mover desde ahí. Enlaza los `wl_output` que hay al abrir y los que llegan con la ventana
  abierta.

### Pruebas

- PC (`cd wayland && cargo test --lib`), `wayland/src/tests_multimonitor.rs`: dos salidas de distinto alto; el cursor
  cruza y no se pierde en el hueco; una ventana en cada una; mover con el teclado; maximizar en la segunda con su
  barra; un `wl_output` por salida; las salidas cambian con clientes dentro; bloqueo en dos salidas; la sombra no pasa
  de salida; arrastrar a la otra; y hostiles (capa con una salida que no existe, márgenes y daños absurdos, pantalla de
  bloqueo con una salida falsa, enlazar una salida que no existe, salidas absurdas).
- virt: `make arranque-virt-2`. La imagen de `arranque-virt` con una virtio-gpu de dos cabezas (`max_outputs=2`, la 0
  a 800x600 como la ramfb). `tools/guion_multimonitor.py` captura cada cabeza (`screendump fichero vgpu N`), lleva el
  cursor a la segunda, pasa Ajustes allí con Super+Mayúsculas+Derecha, la maximiza (1024x768, sin barra), la devuelve
  y la trae, y baja el centro de control a Pantallas. `tools/captura_multimonitor.py` mira las capturas. Para cuando
  el centro enseña las dos pantallas.

### Medido

Las líneas "[compositor] en 10 s" de una vuelta entera. El PC, cargado por otras sesiones (media de 10 a 54): las cifras
dan el orden, no la décima.

| Vuelta | Cuadros | Componiendo | Píxeles | Por mil píxeles | Quieto (≈1 Mpx en 10 s) |
|---|---|---|---|---|---|
| `arranque-virt` en `revision`, sin tocar (media 54) | 528 | 9272 ms | 72 Mpx | 129 µs | 90 a 106 ms |
| `arranque-virt` en la rama, una salida (media 20 a 40) | 798 | 5194 ms | 73 Mpx | 71 µs | (acabó antes) |
| `arranque-virt-2`, dos salidas, con el guion de siempre | 442 | 2234 ms | 50 Mpx | 45 µs | 22 ms |
| `arranque-virt-2`, con su guion (94 s) | 224 | 718 ms | 30 Mpx | 24 µs | (acabó antes) |

- Con una salida, componer no cuesta más; sale algo menos (la copia a la pantalla va por filas, sin índices por píxel),
  pero la carga del PC era distinta.
- Por virtio-gpu, copiar a la memoria de la cabeza es una copia de memoria por fila (es memoria normal sin caché), y
  sale a la mitad. El paso a dos salidas repinta todo una vez (1,4 Mpx).
- El hueco entre salidas de distinto alto (800 × 168 aquí) no se compone nunca.

### Decisiones que debe confirmar el usuario

- Una cabeza sin monitor se usa igual, a 1024 × 768. Así se prueba con QEMU sin ventana; con un monitor de verdad
  quizá sea mejor dejarla apagada.
- Las salidas van en fila, de izquierda a derecha y arriba, por orden de cabeza; la principal es la primera. Moverlas
  y elegir la principal, desde el centro, más adelante.
- Las ventanas nuevas salen en la salida del cursor (en la prueba, el centro de control salió en la segunda).
- La cascada se arrima para no salirse: también con una sola salida cambia algo (una ventana que no cabía ya no se
  sale por abajo).
- Super y arrastrar mueve ventanas: es la primera forma de moverlas con el ratón.
- Con la sesión bloqueada, la salida sin pantalla de bloqueo, en negro (`guardian` solo pide la principal).
- El cargador reserva siempre la pantalla de UEFI (ver arriba).
- Hallazgo, sin tocar: un clic en una ventana quita el teclado a una capa con teclado exclusivo, como la pregunta de
  permisos de `guardian`; wlr-layer-shell dice que no debe. Lo arreglé y lo quité: el guion de siempre clica Ajustes
  con preguntas en pantalla y cuenta con ello. Decidir si se arregla (y se cambia el guion).

### Pendiente

- La Pi 4: las dos HDMI por el HVS, con su controlador, en la placa.
- Una barra por monitor (hoy, la barra, el panel y el lanzador van en la principal).
- ~~La pantalla de bloqueo en cada salida~~: hecho en revision (2026-11-12): las demás, del color de la superficie del
  tema. Falta la de una salida que aparezca con la sesión ya bloqueada.
- Orden, sitio y salida principal desde el centro; escala por monitor (S9); xdg-output cuando haya escalas.
- Avisos de cambio de monitores del aparato (`VIRTIO_GPU_EVENT_DISPLAY`) y EDID: el controlador no los mira.
- `wl_surface.enter`/`leave` de las salidas: no se mandan.
- Un búfer por cabeza: el compositor no espera a que el aparato copie (puede haber desgarro). El cursor, por software.
- El controlador no usa su interrupción: espera a la cola girando y a ratos de 100 µs.
- Si el controlador se cae, las pantallas se quedan congeladas: el compositor no vuelve a la de UEFI. La ramfb se queda
  con el último cuadro de antes.
- Arrastrar entre monitores, solo probado en el PC: con el monitor de QEMU (HMP) no se puede tener Super pulsada
  mientras se mueve el ratón (haría falta QMP).
