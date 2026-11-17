# Bitácora: avisos, el Dash del reloj y el centro de control configurable, como DMS (S27, oleada 3)

Rama `dms-avisos`, sacada de `revision` (e60553a6). Petición del usuario (2026-11-16): "un escritorio configurable como
el de DMS de Niri. De hecho si te lo puedes fusilar, estaría bien"; y después: "si puedes darle el mismo aspecto que mi
escritorio en Shiva ya sería la rehostia". Diseño en `docs/diseño/S27-escritorio-dms.md` (sección 0: DMS es MIT desde el
12/11/2025, se porta su código) y en la sección 8, nueva.

## Lo portado de DMS

Código de DMS v2.1.2 (la versión de shiva) en `~/recuperar/geosian-7d/dms-fuente/`, fuera de git. Lo leyeron tres
agentes, que sacaron las medidas, los radios, las curvas y el comportamiento (con fichero y línea) de:
`Services/NotificationService.qml`, `Modules/Notifications/**`, `Modules/ControlCenter/**`, `Common/GridLayout.js`,
`Common/Theme.qml`, `Common/settings/SettingsSpec.js`, `Modules/DDash/**` y `DCommon/Widgets/DMonthGrid.qml`.

El aviso MIT de DMS: `docs/licencias/DankMaterialShell-MIT.txt`, el `LICENSE` del repositorio oficial
(`github.com/AvengeMedia/DankMaterialShell`, etiqueta `v2.1.2`; el de `master` y el de `main` son iguales, misma huella
sha256 4cee9628…). Nada más se bajó. Lo portado lleva el aviso en su cabecera.

Lo de serie sale de los ajustes de DMS del usuario en shiva (`~/recuperar/geosian-7d/aspecto-shiva/`):
`notificationPopupPosition = -1` (arriba en el centro), `notificationAnimationDuration = 400`, su
`controlCenterWidgets` (14 mosaicos), `radiusStrength = 38` (radios × 0,76) y `niriLayoutRadiusOverride = 3` (el radio
de las ventanas emergentes de DMS en niri).

## Lo hecho

### Avisos (servicio y protocolo)

- `usuario/src/protocolo/avisos.rs`: el modelo de DMS. `AVISAR_CON`: hasta 4 acciones (identificador de 1 a 16 bytes
  `a-z A-Z 0-9 _ - .`, sin repetir; etiqueta de 1 a 24) y el número de un aviso propio que sustituye. La acción
  `default` es la del clic en el cuerpo (no lleva botón). Un aviso es "actual" hasta que se descarta y sigue en el
  historial hasta que se quita o caduca (7 días de serie). "No molestar" es estado de la sesión, con plazo o sin él
  (`NoMolestar`, en el reloj que no salta). La cabecera de las respuestas lleva una versión: si cambia, la barra vuelve
  a pedir los ajustes y el "no molestar".
- `usuario/src/bin/avisos.rs`: al pulsar una acción, se la manda al programa por la llave que dio al avisar (una por
  programa, la última) y el aviso deja de ser actual. Solo la barra pulsa, descarta y lee; "no molestar", la barra y
  el panel; los ajustes, la barra. El servicio entero va en memoria estática: con las acciones, el historial pasa de
  20 KB y no cabía en la pila (la primera vuelta murió así: excepción de datos en la pila, `FAR 0x7ffeff90`).
- `avisos_cliente::avisar_con` y `oir_accion`.

### Los emergentes y el centro de avisos (`usuario/src/avisos_vista.rs`, lo incluye la barra)

Medidas de `NotificationMetrics` y `NotificationCard`: tarjeta de 392 (400 con su margen de 4), relleno 12, la inicial
del programa en un círculo de 36 (contenedor secundario), "programa · hora" en 12 (ahora, hace N min, hh:mm, o el día y
la hora), título en 14 medium en una línea, texto en 12 en dos, botones de texto de 40 en píldora a 4 uno de otro, más
"Borrar" (en DMS, "Clear": lo quita también del historial). Los urgentes: franja de 2 en el primario a la izquierda y no
caducan. Sombra de nivel 2. Radio 3 (el de las ventanas de su niri).

- Hasta 4 a la vez; el quinto echa al más viejo que no tenga el ratón encima. Una sola capa (`avisos-emergentes`) con
  la pila; el más nuevo junto al borde.
- Animaciones de DMS sobre `notificationAnimationDuration` (400 en shiva): entrar en ×0,875 con la deceleración
  enfatizada, salir en ×0,75 con la aceleración estándar, deslizándose todo su alto (en el centro) o todo su ancho (en
  las esquinas) y con la opacidad; la pila se recoloca con el muelle de DMS. El tiempo empieza 75 ms después de entrar,
  se para con el ratón encima y vuelve a empezar al salir.
- La barra de tiempo (si `barra_tiempo = sí`): pista de 4 en el contenedor secundario, lo que queda en el primario y el
  punto al final.
- El centro de avisos: 416 de ancho bajo la campana, con el teclado. Pestañas Actuales e Historial con su número (DMS
  no tiene pestañas: alterna dos vistas; las pidió el usuario). Los actuales agrupados por programa: plegados, el último
  con un chip "N"; desplegados, cabecera con "Descartar" y hasta 10. Estado vacío "Estás al día". Abajo, "no molestar"
  (con los plazos de `DndPresets`: 15 y 30 min, 1, 3 y 8 h, mañana a las 8:00, hasta que lo quite), "Borrar todo" y
  Ajustes. Teclado: flechas, Intro, Supr, Mayúsculas+Supr y Esc. Aparece en 250 ms con la curva expresiva.

### El Dash del reloj (`usuario/src/dash_reloj.rs`, `usuario/src/calendario.rs`)

`DashMetrics`: 6 columnas de 128 (o lo que quepa: a 800 de ancho, 768 en total), filas de 96 (se encogen si no cabe el
alto), 8 entre ellas, 12 de relleno y la barra de pestañas de 64 (solo "Resumen"). Tarjetas: el reloj (2×1, contenedor
primario), quien está delante (1×1), los avisos actuales (3×5; un clic abre el centro de avisos), el calendario (3×3) y
el sistema (3×1, CPU y memoria, con los umbrales de DMS). El tiempo y la música de DMS no tienen dato en DashOS: en su
sitio, quien está delante y el sistema. El calendario: "noviembre 2026" en 16 medium, mes anterior, hoy y siguiente, los
días desde el lunes (es_ES, como shiva), 42 casillas, hoy en el primario (con anillo de 2 si se elige otro), los de otros
meses atenuados; flechas, Re Pág, Av Pág, T y Esc. La capa ocupa todo el alto (`fullHeightSurface`): un clic fuera lo
cierra. La hora, con la zona del sistema (`Prefs/zona`, la que ya leía la barra).

### El centro de control configurable (panel)

- `cc_rejilla.rs`: `controlCenterWidgets` de DMS en texto (`ENV:Escritorio/centro-control`, clave `mosaicos`: "nombre
  ancho[xalto] [pequeño] [@columna:fila]"; `columnas`), con los nombres de DMS también. Lo de serie, el de shiva (14).
  La colocación es `packCells` (primer hueco y gravedad, en medias columnas) y `shrinkRows` de DMS: con 8 columnas,
  600 × 496, como DMS.
- `panel.rs`: mosaicos (píldora, caja del icono de 48, nombre en 16 medium y estado en 14), botones de icono (apagar en
  el color de error), deslizadores (botón de 48 y pista de 32 con su asa de 44), el usuario, el detalle bajo la fila
  (red, salida, no molestar con sus plazos) y lo que DashOS aún no lleva, apagado (brillo, Bluetooth, micrófono, modo
  noche, programas abiertos). Edición como DMS: marco de 2 en el primario y una píldora con antes, después y quitar
  (DMS mueve arrastrando: aquí, botones), y la tira con Añadir (la lista de lo que falta), Deshacer (50), Descartar y
  Terminar. Al terminar o al cerrar, lo guarda el centro.

### Ajustes en texto y en caliente (S36, regla 1)

El centro de control (`centro`) ya vigilaba `ENV:Escritorio`. Ahora lee también `Escritorio/avisos` y
`Escritorio/centro-control` y se los da a la barra y al panel por una puerta que cada uno le dio ("avisos vigilar",
"cc vigilar"): cambian en marcha, se cambien en Ajustes, con la orden `Ajuste` o a mano. La barra pasa los ajustes al
servicio (lo del historial). El panel guarda lo editado con "cc guardar" (el centro lo escribe en `ENV:` y, por
`sesiones`, en `ENVARC:`). En Ajustes, dos categorías: Avisos (10 filas, Intro cambia) y Centro de control (cada
mosaico con su ancho, Quitar, Subir, Bajar, Añadir, columnas y volver a lo de serie). Cerrar es Guardar.

No se dio al centro la llave de "avisos": el primero que nombra un canal es quien lo lee, y el centro va antes que el
servicio en `inicio`. Por eso las puertas (`ventana_cliente`: `puerta_extra`, y lo que llega por ellas en `recibidos`).

## Pruebas

- PC (`make prueba` las corre): `protocolo/avisos.rs` (13: acciones de ida y vuelta, acciones falsas escritas a mano,
  sustituir solo lo propio, actuales e historial, caducar, no molestar con plazo), `avisos_ajustes.rs` (5: lo de
  shiva, valores malos, basura de bytes, horario que cruza la medianoche), `dms_medidas.rs` (4: radios de shiva,
  curvas, muelle), `cc_rejilla.rs` (6: la rejilla de shiva mide 600 × 496 como DMS, textos malos y 2000 al azar,
  bytes, editar, el detalle empuja) y `calendario.rs` (4: días conocidos, 1,6 millones de días de ida y vuelta, la
  rejilla de noviembre, meses cortos, semanas ISO). `make prueba`: BIEN.
- QEMU, hostil: `avisador` (acciones falsas rechazadas 9 de 9; un mensaje de 300 bytes no llega; 12 de 12 órdenes de
  la barra, el panel y el centro negadas; sustituir el suyo, sí), `avisa` (sustituir el de otro: -2), `hostil` (el
  centro de control y los ajustes de los avisos sin ser el panel ni la barra: -1, y por la puerta que dio no le llega
  nada). La ráfaga sigue topando con el ritmo (10 por minuto) y el historial con 8 por programa.
- `make arranque-virt-dms` (nuevo; `tools/guion_dms.py` con la imagen de usar): entra Ana, `avisa` avisa, se pulsa
  "Deshacer" en el emergente y le llega a `avisa`, el centro de avisos y su Historial, el Dash y el mes siguiente, el
  centro de control y el detalle de la red, y el modo de edición. BIEN (a 1920 × 1080, con el fondo de shiva y oscuro).
- `make arranque-virt`: tres vueltas (la 1, con el servicio de avisos cayéndose por la pila, ya arreglado). Las vueltas
  3 y 4 se paran en el mismo sitio, antes de cualquier paso mío: la ventana de Ajustes que abre `guardian` (Permisos) no
  se cierra y el guion espera ("[guardian] pide al centro abrir Permisos: 0, y se cerró: no"); la primera línea que
  falta es "[dash-ventana] pulsado". Es la familia conocida de teclas y clics perdidos con carga (la arregla
  `firefox-firme`): `dms-barra` e `iconos`, sin mis cambios, se paran igual a la misma hora. Lo de antes de ese punto
  (el servicio, el avisador, las puertas del panel y de la barra, el centro de control roto de fábrica) sale bien. Los
  pasos del guion de `arranque-virt` que tocan lo mío (panel en rejilla y su edición, el emergente y su acción, el
  centro de avisos, el Dash, `Ajuste` en caliente) NO se han visto pasar en esta prueba; los mismos pasos, sí en
  `arranque-virt-dms`.

- `make arranque-virt-escritorio`, `make arranque-virt-2` y `make arranque-virt-shell`: BIEN, a la primera.

## Medido

- Tamaño de los programas (aarch64, release; `revision` compilado el 2026-11-15, así que la diferencia no es solo de
  esta rama): barra 393 568 → 512 352 bytes, panel 336 224 → 360 800, centro 504 160 → 680 288, avisos 93 208 →
  127 888. El disco de pruebas sigue en 8 MB (A15): lo nuevo en él es el centro de control de fábrica roto (99 bytes).
- Memoria: el servicio de avisos, unos 21 KB estáticos (50 avisos con 4 acciones); la barra pide al abrir 1,7 MB para
  la capa de los emergentes (416 × 1008) y 2,8 MB para el centro de avisos (416 × 1704... tope de 4 × 240 de alto y de
  la lista de 600), y el Dash 5,3 MB (832 × 1600); se sueltan al cerrar. El panel, 3,2 MB fijos (888 × 900, de 12
  columnas). No se midió el tiempo de pintar cada cuadro de las animaciones (QEMU con la máquina cargada).

## Capturas (fuera de git)

`~/recuperar/geosian-7d/dms-avisos-capturas/`, a 1920 × 1080, modo oscuro con la paleta del fondo y el fondo de shiva
(el de `monitorWallpapers` de su Odyssey, en PNG de 256 colores para el disco de pruebas; nunca en git):
`shiva-1-emergente`, `shiva-2-emergente-encima` (con la cruz), `shiva-3-centro`, `shiva-4-historial`, `shiva-5-dash`,
`shiva-6-dash-mes`, `shiva-7-panel`, `shiva-8-panel-detalle` y `shiva-9-panel-edicion-recorte` (de la primera tanda).

Comparado con su DMS: misma forma y medidas. Distinto: la paleta. DMS en shiva usa `scheme-monochrome` de matugen
(primario blanco, superficies grises neutras); DashOS saca la paleta del fondo con su semilla (tonal), así que los
acentos salen en violeta. Eso es del tema del kit (o del perfil "shiva" de `dms-barra`), no de esta rama. Tampoco
están sus letras (Google Sans Flex no está ni en shiva).

## Decisiones pendientes del usuario

1. **Quién puede mandar avisos**: lo de antes (los de `inicio` con la llave, y los del disco con el permiso fijo
   `avisa`, por nombre de fichero). Con acciones, igual. Con paquetes (S8), el permiso del paquete.
2. **Avisos con la pantalla bloqueada**: no salen (los emergentes son capas de la barra, que no se ven con el bloqueo),
   y `guardian` aún no enseña nada. El ajuste `bloqueo` (nada, contador, nombres, todo; de serie "contador", como DMS)
   se guarda pero nadie lo usa aún.
3. **Pestañas en el centro de avisos** (las pidió el usuario; DMS alterna dos vistas con un botón).
4. **"No molestar" programado** (DMS solo tiene plazos): desde, hasta y días en `ENV:Escritorio/avisos`; en Ajustes,
   tres opciones (sin horario, 22:00 a 07:00, 23:00 a 08:00).
5. **Los urgentes no caducan** (como DMS): uno se queda arriba hasta cerrarlo.
6. **Lo que DashOS aún no lleva sale apagado** en el centro de control (brillo, Bluetooth, micrófono, modo noche,
   programas abiertos), porque están en el `controlCenterWidgets` de shiva.
7. **Mover mosaicos con botones** (antes y después) en vez de arrastrar, y el ancho, en Ajustes (DMS tiene el asa de
   cambiar el tamaño).
8. **El radio de 3** de las ventanas emergentes (el de su niri) y no el 12 de los diálogos del kit.
9. **El Dash solo con Resumen**: sin las pestañas de música, fondos y tiempo (sin dato).

## Lo que queda

- Arrastrar para mover y redimensionar en la edición; la hoja de añadir con buscador y categorías.
- El detalle de la salida de sonido con su lista de aparatos (no hay quien la dé); los de Bluetooth y brillo.
- Escala 0,96 → 1 al entrar los emergentes y al abrir los paneles (solo opacidad y desplazamiento).
- Agrupar y plegar en el historial (DMS lo enseña plano con filtros de tiempo; aquí, plano sin filtros).
- Las reglas por programa (`notificationRules`), el modo privado en el bloqueo y el sonido de los avisos.
- El centro (Ajustes) no atiende su canal con su ventana abierta: mientras, el panel no puede guardar lo editado
  ("cc guardar" no contesta) ni la barra cambiar el "no molestar" que dependa de él (el "no molestar" va al servicio
  directo, así que ese sí). Ya pasaba con "modo oscuro" del panel.
- Repetir `make arranque-virt` cuando `firefox-firme` arregle lo de las teclas y clics perdidos, para ver mis pasos ahí.

## Lo que no he comprobado

- Los pasos de `arranque-virt` que tocan lo mío (ver Pruebas): sí en `arranque-virt-dms`, no en `arranque-virt`.
- La orden `Ajuste` cambiando los avisos y el centro de control en caliente dentro de QEMU (el paso está en el guion
  del Shell de `arranque-virt`, que no se llegó a correr).
- Ajustes > Avisos y Ajustes > Centro de control con el ratón (no tienen paso en el guion).
- La barra de tiempo, el teclado del centro de avisos, el menú de "no molestar", los emergentes abajo o en las
  esquinas: solo con las pruebas de PC de su modelo, no en pantalla.
- En la Pi.
