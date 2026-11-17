# Bitácora: la pantalla de bloqueo y el monitor de procesos como los de DMS (S27, oleada 6)

Rama `dms-bloqueo`, sacada de `revision` (8a4a71a6: la barra, los avisos, el dock y los iconos de DMS ya unidos).
Diseño: `docs/diseño/S27-escritorio-dms.md` (sección 0: DMS es MIT desde el 12/11/2025, se porta su código con su aviso,
ya en `docs/licencias/`). Leídas antes las bitácoras `dms-barra`, `dms-avisos`, `dms-dock`, `union-dms`, `cuentas-2` y
`trasfondo`. Código de DMS v2.1.2 en `~/recuperar/geosian-7d/dms-fuente/` (fuera de git).

## Diseño en tres frases

`guardian` sigue siendo quien pinta la pantalla de bloqueo y la de entrada (la única que ve lo que se teclea), ahora
con la disposición de DMS (`bloqueo_vista.rs`): el reloj grande y la fecha arriba, el avatar y el campo con contorno
en medio, los avisos debajo y el estado arriba a la derecha, sobre el fondo del usuario desenfocado. Lo de alrededor
le llega de quien ya lo tiene, sin darle más poderes que leer: el fondo pequeño y desenfocado de `fondo` (una llave solo
para leer), la zona horaria del centro, la red y el volumen del compositor (como a la barra) y los avisos de su
servicio a través de `sesiones` (solo cuántos, o también de quién, según el ajuste; nunca el texto). El monitor de
procesos es el desplegable de la barra rehecho como el de DMS (`procesos_lista.rs` y `procesos_vista.rs`), con lo que
ya da `sesiones` (`MONITOR`), y cerrar lo ajeno pasa por la tanda de administrador.

## Lo portado de DMS (licencia MIT, aviso en `docs/licencias/DankMaterialShell-MIT.txt`)

| DashOS | De DMS |
|---|---|
| `usuario/src/bloqueo_vista.rs` | `WidgetEditLayer.qml` (`stockRect`: dónde va cada pieza), `LockMetrics.qml`, `DClockWidget.qml` y `DClockFace.qml` (el reloj "expressive", 240 × 120 por 1,75), `LockAuthWidget.qml` (avatar de 56, campo de 312 × 56), `LockNotificationsWidget.qml`, `SharedSettingsSpec.js` (`lockWidgetDefaults`) |
| `usuario/src/desenfoque.rs` | `LockScreenBackground.qml` y `Theme.qml` (`lockScreenBlur` 0,8 de 32, velo negro al 40 %) |
| `usuario/src/procesos_lista.rs` | `ProcessListPopout.qml`, `ProcessesView.qml` (ordenar), `ProcessFilterChips.qml`, `ProcessSort.js` |
| `usuario/src/procesos_vista.rs` | `ProcessListPopout.qml` (644 × 602), `ProcessSystemHeader.qml`, `ProcessSummary.qml` y `PerformanceCard.qml`, `ProcessRow.qml` (filas de 36), `ProcessColumns.js` |
| `usuario/src/bin/guardian.rs` (`pintar`) | lo visible de lo de arriba, con el kit |

## Lo hecho

### 1. La pantalla de bloqueo (y la de entrada)

- **Disposición de DMS** (lo de serie de `lockWidgetDefaults`): el reloj arriba en el centro (a 45 del borde, caja de
  420 × 210, horas y minutos en una línea en el primario, Roboto Medium de unos 155), la fecha larga debajo ("Martes, 17
  de noviembre", el formato `dddd, MMMM d` de shiva), la fila del avatar (la inicial en un círculo del contenedor
  primario) y el campo (contenedor alto, radio XL con la fuerza de shiva, contorno de 2 del primario como el foco de
  Material 3, en rojo tras un error; el candado en su cuadrado del contenedor primario; un punto por letra; la flecha
  para entrar cuando hay algo escrito) en el centro, el error o "Bloq Mayús está activado" debajo, los avisos debajo, y
  la red y el volumen arriba a la derecha. En una pantalla baja el reloj se encoge (en 800 × 600, 240 × 120).
- **Lo que DMS no tiene y aquí sigue**: el nombre de quien entra y su frase encima del campo (S6b: así se sabe que es
  el sistema), y las teclas al pie (Esc, Tab).
- **El fondo**: `fondo` hace, cada vez que pone una imagen, una copia a un octavo de la pantalla y desenfocada (dos
  pasadas de caja de radio 2: 240 × 135 a 1920, 14 ms), y se la da solo a `guardian` ("bloqueo", por el nombre que da
  `inicio`), con una llave solo para leer que no puede pasar a nadie. `guardian` la agranda con interpolación bilineal
  y le pone el velo de DMS. Sin fondo (al arrancar, antes de que nadie entre, `fondo` aún no tiene imagen), el color de
  la superficie del tema, como antes. Sobre el fondo, el texto en blanco y el reloj en el primario del esquema oscuro
  (se lee con cualquier tema).
- **La hora**: en la zona horaria del sistema (`SYS:Prefs/zona`), que le dice el centro de control (pedido "zona" por su
  canal: `guardian` no lee ficheros); cada minuto se repinta el reloj. Sin respuesta, UTC y se vuelve a preguntar.
- **Red y volumen**: el compositor le pasa el `estado` del panel como a la barra (`ven_estado`, por el nombre
  `guardian`; versión 5 de `dashos_tema_v1`). Sin batería: DashOS no tiene quien la mida.
- **Avisos según la privacidad**: `guardian` pide `AVISOS_BLOQUEO` a `sesiones`, que se lo pasa al servicio de avisos
  sin marca (`AVISO_RESUMEN`, con la llave de `guardian`), y el servicio contesta según el ajuste `bloqueo` de
  `ENV:Escritorio/avisos` (ya existía, de serie "contador"): nada, cuántos actuales (un chip con la campana, "3 avisos
  nuevos") o también de quién (un chip por programa, como los de DMS). El título y el texto no salen del servicio con
  la sesión bloqueada; "todo" va como "nombres".
- **Bloq Mayús**: `guardian` lee los modificadores del teclado (`modifiers`) y lo dice bajo el campo.
- **Super+L en la imagen de usar**: antes, en `make virt-uso` (y en `arranque-virt-dms`), nadie bloqueaba: `guardian`
  solo escuchaba los avisos de inactividad durante su prueba. Ahora, tras entrar, un hilo recoge las preguntas de
  `sesiones` y el bucle principal espera a la vez al compositor (Super+L, el candado del panel o 30 minutos sin tocar
  nada, el `acLockTimeout` de shiva) y a esa cola, por la puerta de su conexión (como el centro y `fondo`).

### 2. El monitor de procesos (el desplegable de las píldoras de CPU y memoria)

- Como el `ProcessListPopout` de DMS, 644 × 602 (menos en una pantalla baja): la cabecera ("DashOS · encendido 2 min ·
  24 procesos"), las tarjetas de CPU y de memoria con su gráfica (el historial que ya lleva el medidor), el buscador
  (se lleva el teclado al abrirse, como en DMS) con los filtros Todos, Míos y Sistema, la cabecera de columnas que
  ordena (Nombre, CPU, Memoria, Usuario; otra vez la misma, al revés; los textos de la A a la Z y los números de más a
  menos de serie) y las filas de 36 (la inicial en su cuadrado, la pastilla 68K o LINUX de las cajas, CPU, memoria y
  usuario). Flechas para elegir, Tab cambia el filtro, la rueda desplaza, Esc cierra.
- **Lo que se ve es lo que da `sesiones`** (`MONITOR`, con la privacidad decidida el 2026-11-13): lo de la persona de
  delante con su nombre, las cajas, y lo demás del sistema y lo de otras personas en dos sumas sin nombres.
- **Cerrar**: el botón del pie (o Supr) cierra el elegido. Si es de quien está delante, `MATAR`. Si es un programa del
  escritorio o una caja del sistema (`MATAR` no los cierra), dice "Cerrar (administrador)" y pasa por la tanda de
  administrador (`matar <número>`): la ventana del sistema pide la contraseña de un administrador. Una suma no se
  cierra. Mientras la ventana del sistema pregunta, la barra sigue leyendo al compositor pero no se repinta (como
  Ajustes con las cuentas).
- Lo que DMS pone en el menú del botón derecho (terminar, forzar, el árbol) va en el pie; el enlace "Abrir el monitor
  del sistema" (Ajustes) se queda.

### Arreglo de seguridad que salió al probar: el teclado con la sesión bloqueada

La prueba hostil nueva del compositor (`bloqueado_nadie_pinta_encima_ni_oye_las_teclas`) pilló un fallo de G5: una capa
de arriba con el teclado exclusivo que se ponía **después** de bloquear se llevaba el teclado. No se veía (solo se
pintan las pantallas de bloqueo), pero oía lo que se tecleaba en la contraseña. Arreglado en la raíz: `dar_teclado` no
da el teclado a nadie que no sea una pantalla de bloqueo mientras la sesión está bloqueada (valía para las capas, los
emergentes con agarre y cualquier otro camino).

## Pruebas

(se completa al final)

## Medido

(se completa al final)

## Pendiente del usuario

1. **El fondo en la pantalla de entrada**: como DMS, el del último que lo puso (si `fondo` tiene uno); al arrancar, sin
   nadie dentro, el color del tema. Un fondo propio de la entrada, por decidir.
2. **Los avisos en el bloqueo, de serie "contador"** (cuántos, sin contenido), como pidió el encargo; DMS trae de serie
   los chips con el nombre de cada programa. "Todo" (con el contenido, el modo 3 de DMS) no se enseña: haría falta
   decidir si el texto de un aviso puede salir del servicio con la sesión bloqueada.
3. **Bloquear solo a los 30 minutos sin tocar nada** (el `acLockTimeout` de shiva), sin ajuste aún para cambiarlo.
4. **La fecha bajo el reloj**: DMS la trae apagada de serie (`lockDate` en la esquina); el encargo la pide, y va bajo el
   reloj, como la opción `showDate` del reloj de DMS.
5. **Sin animaciones** en la pantalla de bloqueo (DMS sacude el campo tras un error y anima los puntos): pendiente de la
   oleada 7 (animaciones).
6. **Cerrar lo ajeno**: solo lo que se ve con número (los programas del escritorio y las cajas del sistema). Lo de las
   sumas (el sistema y otras personas) no se puede elegir: verlo con detalle cambiaría la privacidad decidida el
   2026-11-13 (por ejemplo, "ver todo" con la contraseña de un administrador).
7. **El monitor sin menú del botón derecho**: cerrar va en el pie, con Supr. DMS tiene además forzar, reiniciar y el
   árbol de procesos, que DashOS no distingue hoy.

## Lo que queda

(se completa al final)
