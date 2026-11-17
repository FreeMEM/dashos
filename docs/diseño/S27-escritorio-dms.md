# S27. Un escritorio "como DMS"

Lo pidió el usuario el 2026-11-13: "Quisiera conseguir un escritorio 'DMS like'. Sería brutal."

DMS es DankMaterialShell, que en la 2.0 pasó a llamarse Dux Shell y en la 2.1 (2026-11-04) Dux Modular Shell. Es un
escritorio de Material 3 hecho con Quickshell, sobre todo para el compositor niri. La orden sigue siendo `dms`.

**Sala limpia.** De DMS solo se toman ideas. Se leyó su README, su documentación (duxlabs.dev/docs, que es el repositorio
`dux-docs`), las notas de versión de su blog y sus capturas. No se leyó código de DMS, Quickshell, niri ni matugen. Todo
se hace con el kit de DashOS (Material 3) sobre los servicios de DashOS. Las medidas de DMS que salen aquí son a ojo,
sacadas de sus capturas a 1920 × 1080.

Sigue a S10 (el escritorio), S10b (escritorios de terceros y la política de ventanas), S10c (el centro de control) y S23
(las pantallas del Amiga). Bitácora: `docs/bitacoras/escritorio-dms.md`.

## 0. Decidido el 2026-11-16: portar DMS, no solo inspirarse

El usuario: "vamos a crear un sistema de escritorio configurable como el de DMS de Niri. De hecho si te lo puedes
fusilar, estaría bien". DMS (DankMaterialShell, ahora Dux Modular Shell) pasó de GPL-3.0 a MIT el 12/11/2025, así que
su código se puede leer y portar a Rust y al kit (QML no se usa tal cual), con su aviso de copyright MIT en
`docs/licencias/` y en las cabeceras de lo portado. Deja de valer la "sala limpia" de DMS de la primera oleada (no la
de Linux, BSD, AROS, UAE ni QEMU, que sigue). Copia de su código, fuera de git: `~/recuperar/geosian-7d/dms-fuente/`
(la versión que usa el usuario). El modelo de configuración también se toma de DMS (`settings.json`: `barConfigs` con
`leftWidgets`/`centerWidgets`/`rightWidgets`, `dockConfigs`, `controlCenterWidgets`, `desktopWidgetInstances`...),
llevado a ficheros de texto de `ENVARC:` (S36, regla 1).

## 1. Qué es DMS, pieza por pieza

DMS tiene dos estilos: "Expressive" (el de serie: píldoras, y los controles se deforman al pulsarlos) y "Classic" (el
mismo radio en todo). También tiene cuatro densidades, de "Spacious" a "Dense".

### La barra

- Una tira flotante y translúcida, a unos 6 píxeles de los bordes, con los extremos redondos. Se ve el fondo detrás.
- Dentro, cada cosa es una píldora propia, algo más clara que la tira, con 4 a 6 píxeles entre una y otra.
- **A la izquierda**: el botón del lanzador (el logo) y los espacios de trabajo. El espacio que se ve es una píldora
  alargada del color de acento; los demás, puntos grises. Hay más estilos (línea, tarjeta, con números, con los iconos
  de los programas de cada espacio), un mínimo de espacios que se enseñan, y se pueden renombrar y ordenar arrastrando.
- **En medio**: la música (título, artista y los botones), el reloj con la fecha ("9:36 PM · Mon 4") y el tiempo. Un
  clic en el reloj abre el "Dash" (abajo).
- **A la derecha**: la bandeja del sistema, el uso de CPU y de memoria, la campana de los avisos (con un punto si hay) y
  una píldora de estado con la red, el bluetooth y el volumen en el color de acento, que abre el centro de control.
- Se puede poner en cualquier borde, hasta cuatro barras, ocultarla sola, o cambiarla por un "marco" alrededor del
  monitor o por una "isla" (una píldora compacta que se abre con un muelle).

### El centro de control

- Un panel de unos 600 de ancho bajo la barra. Arriba, el usuario (avatar, nombre, tiempo encendido) y botones redondos:
  editar, ajustes, bloquear y apagar.
- Debajo, una rejilla de dos columnas: deslizadores anchos de volumen y brillo, y mosaicos con un icono en un cuadrado
  de acento y dos líneas ("Ethernet / Connected"). Un clic en el mosaico abre su detalle; un clic en el icono lo
  enciende o lo apaga. Varias páginas, con puntos.
- Mosaicos: red, bluetooth, salida y entrada de sonido, VPN, disco, cuentagotas, captura, no molestar, pantalla, energía,
  modo noche, modo oscuro, mantener despierto, impresoras, micrófono...
- Modo de edición: quitar, cambiar el tamaño (25 a 100 %), arrastrar para ordenar, deshacer, y una lista para añadir.

### Avisos

- **El centro de avisos**: un panel de unos 500 de ancho. Arriba, no molestar, dos pestañas ("Actuales" e "Historial")
  con su número, borrar todo y cerrar. Tarjetas con el icono del programa, "programa · ahora", el título, el texto y
  sus acciones. Los de un mismo programa se agrupan y se pliegan. Se maneja con el teclado.
- **El aviso emergente**: una tarjeta de unos 400 × 160 con una barra fina que marca el tiempo que le queda. Se queda
  quieto con el ratón encima.
- Reglas, silencios por programa y modo privado (solo quién avisa).

### El lanzador

- Una ventana en el centro (Super+Espacio), de 630 a 860 de ancho. Arriba, la búsqueda en una píldora grande con borde
  de acento. Encima, categorías en píldoras (Todo, Desarrollo, Gráficos, Juegos...).
- Lista (icono de 36, nombre y descripción) o rejilla (icono de 64 y el nombre debajo), con un botón para cambiar.
- Busca programas, ficheros, ventanas abiertas, ajustes, atajos y la web, y hace cuentas. Tab enseña las acciones del
  elegido. Abajo, las teclas.
- Hay también una barra de búsqueda pequeña que baja desde arriba (Alt+Espacio).

### Lo demás

- **Dock**: una tira flotante abajo, con los programas fijados y abiertos (un punto bajo los abiertos), que agranda el
  icono bajo el ratón y se esconde con una ventana maximizada.
- **Pantalla de bloqueo**: un reloj enorme apilado (horas sobre minutos), el avatar, la contraseña en una píldora,
  chips con los avisos y una barra mínima arriba. Se compone con piezas que se mueven.
- **Avisos en pantalla (OSD)**: el del volumen es una caja oscura abajo en el centro, de unos 200 × 40, con el altavoz y
  una pista del color de acento. Hay OSD de brillo, micrófono, música, mayúsculas, distribución del teclado y cambio de
  espacio.
- **El "Dash"** (un clic en el reloj): pestañas (resumen, música, tiempo, fondos...). El resumen son tarjetas: reloj,
  tiempo, usuario, calendario del mes, CPU, temperatura y memoria, y el reproductor.
- **Monitor de procesos** (Ctrl+Alt+Supr), **historial del portapapeles** (Super+V, con buscador y fijados), **menú de
  apagado** (seis mosaicos con su tecla: reiniciar, salir, apagar, bloquear, suspender, hibernar; se mantiene pulsado
  para confirmar) y **Ajustes** (Super+Coma: barra lateral de categorías con buscador).
- **Color dinámico**: la paleta sale del fondo y tiñe todo, también los programas de GTK, Qt y las terminales. Claro u
  oscuro automático por la hora.

### Con niri

- niri pone las ventanas en **columnas que se desplazan a lo ancho** (lo que no cabe sigue a la derecha) y los
  **espacios de trabajo uno debajo de otro**, por monitor. Siempre hay un espacio vacío al final.
- La barra de DMS enseña los espacios de niri; las columnas no tienen indicador propio.
- La vista general de niri enseña el fondo (o el fondo difuminado), y en ella el lanzador busca en cuanto se escribe.

### Atajos de DMS

Super+Espacio lanzador, Super+V portapapeles, Super+M el Dash, Super+Coma ajustes, Super+Ctrl+N avisos, Super+Y fondos,
Ctrl+Alt+Supr procesos, Super+Alt+L bloquear, teclas del volumen y del brillo (también con la pantalla bloqueada).

## 2. Qué tiene ya DashOS y qué falta

Capturas de antes (1920 × 1080, `DASHOS_PANTALLA=1920x1080 make arranque-virt`), fuera de git, en
`capturas-agente/antes/`: `barra.png` (el lanzador), `barra.busca.png`, `barra.panel.png` (el panel rápido),
`centro*.png` (el centro de control), `menu.png`, `bloqueo.png`, `u6.ana.png`, `amiga.png`, `dash.png`. Las de después
de la primera oleada, en `capturas-agente/despues/`.

| Pieza de DMS | DashOS antes de S27 | Falta |
|---|---|---|
| Barra | Una tira opaca de lado a lado (S10): lanzador, título en medio, hora, campana y panel | Forma de DMS (tira flotante y píldoras), espacios, fecha, indicadores |
| Espacios de trabajo | No había | Todo |
| Lanzador | Una hoja de 360 bajo su botón, con la búsqueda y la inicial de cada programa | En el centro, más grande, categorías, rejilla, iconos de verdad, ficheros y órdenes |
| Centro de control | Panel rápido (C2: modo oscuro, bloquear, volumen con − y +, modo de edición) y Ajustes (C1, C3) | Rejilla de dos columnas, deslizadores, red, brillo, no molestar, usuario, apagar |
| Avisos | Servicio, toast, campana con punto e historial con "Borrar todo" | Pestañas, agrupar, acciones, no molestar, barra de tiempo |
| OSD | No había | Volumen (hecho en S27), brillo, mayúsculas, espacio |
| Bloqueo | `guardian`: pantalla de bloqueo y de entrada, cambio de usuario | Reloj grande, avatar, campo con contorno, avisos en el bloqueo |
| Dock | No había | Todo |
| Calendario y tiempo | No había (la hora es UTC: no hay zona horaria) | Zona horaria, calendario, tiempo (con red) |
| Monitor de procesos | No había (Trasfondo es de la caja Amiga) | Todo |
| Portapapeles | El compositor tiene los objetos, pero no copia ni pega | Copiar y pegar, y el historial |
| Menú de apagado | No había | Todo (con `sesiones`) |
| Ajustes | Centro de control con barra lateral y búsqueda (C1) | Más categorías |
| Color dinámico | Hecho en C3 (semilla del fondo, esquemas), y lo sigue la rama `aspecto-cajas` | Que lo tomen las cajas |
| Columnas de niri | No había: ventanas flotantes en cascada | Todo |

## 3. Las columnas de niri en el compositor

### Qué se propone

Una **disposición por espacio de trabajo**, elegida por el usuario: **flotante** (la de ahora, la clásica del Workbench)
o **columnas** (como niri). Es lo que S10b llama "E3b" y lo que dibujan los bocetos 1 y 4 del lienzo
"Escritorio de DashOS".

En columnas:

- Cada ventana nueva abre una columna a la derecha de la que tiene el teclado. Las columnas tienen anchos por pasos
  (1/3, 1/2, 2/3 y entera, con Super+R) y ocupan todo el alto que deja la barra.
- Lo que no cabe sigue a la derecha, fuera del monitor. Super+Izquierda y Super+Derecha pasan de columna, y la tira
  se desplaza para que la del teclado se vea entera (con un muelle, sin saltos).
- Dos ventanas pueden ir en una misma columna, una sobre otra (Super+Coma la mete en la de al lado; Super+Punto la saca).
- Los diálogos, los selectores y las ventanas de tamaño fijo nacen flotando encima.
- La barra enseña, junto a los espacios, cuántas columnas hay y cuál se ve (el boceto 1 lo dibuja abajo).

### Cómo encaja con lo que hay

- **Por dentro**, el compositor ya separa la escena de quien atiende. Hace falta separar también "decidir dónde va cada
  ventana" de "dibujar" (S10b, sección 5). La disposición en columnas es una función que, con la lista de ventanas del
  espacio, el área útil y el desplazamiento, da el sitio y el tamaño de cada una. Se prueba en el PC sin pintar nada.
- **Los espacios de trabajo**, hechos en S27, son la base: cada espacio tiene su disposición y su desplazamiento.
- **Los tres nombres** (decidido por el usuario el 2026-11-16, S36): **monitor** es el aparato; **espacio de
  trabajo**, lo de todos los días, del 1 al 9; **pantalla**, solo la pantalla propia de un programa de Amiga, que se
  ve como un espacio más. Las teclas que mandan son las de aquí: Super+1..9 y Super+arriba y abajo.
- **Multimonitor**: cada monitor tiene sus espacios (como niri). Hoy los espacios son de todo el escritorio; pasar a
  "por monitor" es un cambio pequeño en `Escena` (el espacio que se ve, por salida).
- **Las pantallas del Amiga (S23)**: cada pantalla propia de un programa 68k es un espacio más, a pantalla completa y
  siempre flotante (su mapa de bits es el fondo, y sus ventanas van encima, como en el Amiga). Amiga izquierda + M y N
  quedan como atajo del Amiga para lo mismo que Super+abajo y Super+arriba: pasar al espacio siguiente y al anterior,
  sea una pantalla del Amiga o no. El gesto de arrastrar una pantalla hacia abajo se conserva (S23). La de Workbench es
  el escritorio: sus ventanas siguen la disposición del espacio en que se abren.
- **El Workbench de DashOS** (los iconos del fondo) es una capa de fondo: se ve en todos los espacios y no entra en las
  columnas. Sus ventanas de cajones son ventanas normales. En columnas, un cajón abierto es una columna más; quien quiera
  el Workbench de siempre usa el espacio en flotante.

### Preguntas para el usuario

1. **¿Columnas de serie o flotante de serie?** Recomiendo **flotante de serie** en el primer espacio (es lo que espera
   quien viene del Amiga y lo que hace el Workbench) y columnas en un clic por espacio. Quien quiera todo en columnas,
   lo marca una vez en Ajustes.
2. **¿Espacios en vertical (niri) o en horizontal?** Recomiendo **vertical**, como niri: arriba y abajo cambian de
   espacio, y los lados recorren las columnas. Así las dos cosas no se pisan.

## 4. El plan, por oleadas

Primero lo que más se ve. Cada oleada se cierra con sus capturas a 1920 × 1080 y con las pruebas de siempre.

| Oleada | Qué | Por qué en este orden |
|---|---|---|
| **1 (hecha)** | La barra como la de DMS: tira flotante translúcida con píldoras, el lanzador, los espacios de trabajo (píldora alargada en el que se ve), el título, el reloj con la fecha, la campana y la píldora de estado (red y sonido) que abre el panel. Espacios de trabajo en el compositor (Super+1..9, Super+Mayús+1..9, clic en la barra). Lanzador en el centro, más grande. Teclas del volumen y OSD del volumen | Es lo que se ve siempre: la barra es la cara de DMS |
| 2 | Centro de control nuevo: rejilla de dos columnas con deslizadores de volumen (y brillo cuando haya), mosaicos de icono y dos líneas, cabecera con el usuario y botones de bloquear y apagar. La píldora de estado con el estado de verdad (red conectada o no, volumen, silencio). Menú de apagado | Es lo segundo que más se usa, y el estado real hace creíble la barra |
| 3 | Avisos al estilo de DMS: centro con pestañas y grupos, no molestar, la barra de tiempo en el emergente, acciones. El "Dash" del reloj con el calendario del mes (con zona horaria) | Completa la parte derecha de la barra |
| 4 | Columnas de niri en el compositor (sección 3), por espacio, con el indicador de columnas | Es lo más largo y toca el compositor: mejor con la barra ya hecha |
| 5 | Lanzador completo: rejilla, categorías, iconos de los programas (con S8, paquetes), ventanas abiertas, cuentas, órdenes. Historial del portapapeles (cuando el compositor copie y pegue) | Depende de los paquetes y del portapapeles |
| 6 | Pantalla de bloqueo al estilo de DMS (reloj grande, avatar, campo con contorno de Material 3, avisos según la privacidad). Dock. Monitor de procesos | Se ven menos a menudo |
| 7 | Animaciones con muelle (abrir, cerrar, desplazar columnas, cambiar de espacio), OSD de espacio y de mayúsculas, densidades | Pulido final |

El color dinámico ya está (C3) y lo lleva a las cajas la rama `aspecto-cajas`: todo lo de S27 usa los papeles del tema
del kit, sin colores a mano, así que cambia solo con el fondo.

## 5. Lo hecho en la primera oleada

Detalle en `docs/bitacoras/escritorio-dms.md`.

- **Barra** (`usuario/src/bin/barra.rs`): sigue siendo una capa de 40 con su zona exclusiva (las ventanas no se mueven),
  pero lo que se ve es una tira de 32 a 6 de los lados y 4 de arriba, en el contenedor de superficie al 88 %, con las
  esquinas redondas. Dentro, píldoras de 24 en el contenedor alto:
  - el lanzador (los nueve puntos en el primario; abierto, la píldora es del primario);
  - los espacios: el que se ve, una píldora de 24 × 8 en el primario; los que tienen ventanas, puntos en el color del
    texto; los vacíos, en el del contorno; un clic en uno lo pide al compositor;
  - el título de la ventana con el teclado (como antes, solo el de la ventana en la que se escribe);
  - en medio, la hora en medium y la fecha ("vie 13 nov") tras un punto, en el color suave;
  - a la derecha, la campana (con el punto de error) y la píldora de estado (red y sonido en el primario), que abre el
    panel rápido como Super+A.
- **Espacios de trabajo** (compositor, `wayland/src/escena.rs`): del 1 al 9, como lo descrito en la sección 3 sin las
  columnas. `dashos_tema_v1` pasa a la versión 3: evento `espacios` para la barra y petición `espacio` solo de la barra.
  Falta (S0: lo del ratón, también por guion): la misma petición por el canal de órdenes del compositor y por ARexx
  (S10b, sección 3), con el poder "Espacios de trabajo".
- **Lanzador**: en el centro de lo que deja la barra, 600 de ancho, buscador de 56, hasta 7 filas de 56 con el icono
  (la inicial en un cuadrado redondeado del contenedor primario), el nombre y de dónde sale, y un pie con las teclas y
  cuántos casan. La lista (buscar, elegir, desplazar) va aparte, en `usuario/src/lanzador_lista.rs`, con sus pruebas.
  Lista hasta 1000 programas (antes, 200).
- **OSD del volumen**: las teclas del volumen van del compositor al panel rápido (evento `volumen`), que sube o baja 5 %
  o pone el silencio, lo manda al servicio de sonido y enseña una píldora abajo en el centro (320 × 56, contenedor
  alto, altavoz, pista y valor) durante 1,5 s tras la última tecla. Una ráfaga repinta la misma capa.

## 6. Lo hecho en la segunda oleada

El usuario aceptó las siete recomendaciones de arriba (2026-11-13). Detalle en la bitácora.

- **Centro de control** (el panel rápido, `usuario/src/bin/panel.rs`), de 440 de ancho:
  - Cabecera: la inicial de quien está delante en un círculo del contenedor primario, su nombre (de `sesiones`, la
    petición nueva `DELANTE`, solo para el panel y la barra), "Encendido hace N min", Editar y Bloquear (botón de icono
    tonal).
  - Deslizadores en filas redondas del contenedor alto: el volumen (el altavoz pone o quita el silencio, un clic en la
    pista pone ese nivel, y los botones − y + de 10 en 10) y el brillo, apagado y con "Sin control aquí" (no hay aún
    quien lleve el brillo).
  - Rejilla de dos columnas: la red (lo que dice el servicio de red: "Conectada · 10.0.2.15", "Sin dirección" o "Sin
    red"), el modo oscuro y bloquear. Cada mosaico con su icono en un cuadrado redondeado (en el primario si está
    encendido), el nombre y el estado. El modo de edición no cambia.
  - Esos textos son los de hoy y tienen jerga. Por S0 (mensajes en llano, el detalle a un clic) tienen que quedar
    así: "Este monitor no deja cambiar el brillo" en vez de "Sin control aquí"; "Conectada" o "Con cable, pero sin
    Internet" en vez de la dirección y de "Sin dirección", con la dirección en el detalle del mosaico.
- **Píldora de estado de la barra** con el estado de verdad: el panel pregunta a la red cada 10 s y lleva el volumen;
  cuando cambia algo, lo da al compositor (petición `estado`, `dashos_tema_v1` versión 5, solo del panel), que se lo
  pasa a la barra. Sin red, el icono de red sale tachado y en el color suave; con el silencio, el altavoz lleva una
  cruz; las ondas siguen al nivel.
- **El panel tiene ahora los canales "sesiones" y "red"**. La llave de "red" es también el permiso de red: el panel
  podría abrir conexiones. Ver la pregunta 9.
- **Menú de apagado: no.** No hay en DashOS forma de apagar ni de reiniciar (ni llamada del kernel ni servicio): hace
  falta antes una llamada al kernel con su permiso. El botón de la cabecera es Bloquear, que sí funciona.
- Todo con los papeles del tema del kit (primario, contenedores, contorno, apagado al 38 % y al 12 %): servirá con los
  cinco temas (Claro, Oscuro, AOS4, MOS, Magic WB) cuando estén.

### Preguntas de la segunda oleada

8. **Apagar y reiniciar.** Hace falta una llamada del kernel (PSCI en la Pi y en QEMU) y decidir quién puede usarla.
   Recomiendo que solo `sesiones` la tenga, y que la pida el panel con la confirmación de `guardian` si hay otras
   sesiones abiertas.
9. **El estado de la red sin el permiso de red.** Hoy, para preguntar el estado, el panel tiene la llave del canal "red",
   que también deja abrir conexiones. Recomiendo que el servicio de red sirva el estado por otro canal de solo lectura,
   y quitar al panel la llave de "red".

### Decisiones para el usuario (primera oleada, aceptadas)

3. **La barra translúcida.** Recomiendo dejarla como está (88 %: se intuye el fondo, el texto se lee igual). Más
   transparente cuesta leer con fondos claros.
4. **Cuántos espacios enseña la barra.** Ahora, como poco 3, y hasta el más alto con ventanas. niri enseña siempre uno
   vacío al final. Recomiendo como está: con 3 se entiende qué es.
5. **El lanzador.** Ahora es una lista. DMS deja elegir lista o rejilla. Recomiendo lista hasta que los programas
   tengan iconos (S8): una rejilla de iniciales no dice mucho.
6. **Paso del volumen con las teclas.** 5 % (DMS usa 3 %; los botones del panel, 10 %). Recomiendo 5 %.
7. **Atajos.** Super sola abre el lanzador (S10), Super+1..9 los espacios (y Amiga + M y N, el siguiente y el
   anterior, como atajo del Amiga). DMS usa Super+Espacio para el lanzador.
   Recomiendo dejar Super sola y añadir también Super+Espacio en la oleada 2.

## 7. Ajustes de todo DashOS al estilo de DMS (pedido del usuario, 2026-11-15)

La idea de los Ajustes de DMS (Super+Coma): una ventana con barra lateral de categorías, un buscador que encuentra
cualquier ajuste por su nombre, cambios que se ven al momento sin "Aplicar", y todo guardado en ficheros de texto.
Se toma la idea (sin leer su código), y se casa con las Prefs del Amiga:

- **Una sola cara, las Prefs de siempre por debajo.** Cada categoría de Ajustes es un editor de `SYS:Prefs` (S4b):
  guarda en `ENVARC:` (la capa del usuario, `Home:Prefs/Env-Archive`) y lo en uso va en `ENV:`; los servicios vigilan
  `ENV:` y aplican el cambio al momento. Quien prefiera el Amiga abre `SYS:Prefs/Monitores` suelto, y es lo mismo.
- **Categorías**: Apariencia (tema claro u oscuro y por la hora, color dinámico del fondo, acento, fuentes, iconos,
  densidad, animaciones), Fondo, Barra, Panel rápido, Dock, Lanzador, Avisos y no molestar, Espacios de trabajo y
  columnas, Atajos, Monitores (resolución, escala, varios), Sonido (salidas, entradas, volumen por programa), Red
  (cable, Wi-Fi cuando haya, cortafuegos), Bluetooth (cuando haya), Teclado y ratón, Idioma y región (S4b, catálogos),
  Fecha y hora (zona, hora por red), Energía, Usuarios (cuentas, contraseña, avatar; lo de otros, con administrador),
  Privacidad y permisos (lo concedido a cada programa, S6b), Programas (Paq y Hangar, actualizaciones, S8), Disco
  (volúmenes y espacio, S32), Caja Amiga (memoria, pantallas, ajustes del 68k) y Caja Linux (memoria, núcleos, disco).
- **El buscador** busca en los nombres y en palabras clave de cada ajuste, también en el idioma del usuario, y abre
  la categoría con el ajuste resaltado. El lanzador (Super+Espacio) también encuentra ajustes.
- **De máquina o de usuario**: lo de máquina (hora del sistema, red del sistema, usuarios, programas de todos) lleva el
  candado de administrador (S6b) y va en la capa de la máquina de `ENVARC:`, en `Estado:Prefs` (S4b): texto que
  todos leen y que se escribe con acción de administrador, desde la ventana o con la orden `Ajuste`. Lo demás es de
  cada usuario y va en su `Home:`.
- **Ficheros de texto legibles** en `ENVARC:`, y el fichero de texto es el que manda (S36, regla 1): se pueden
  copiar entre máquinas y guardar en git, y editarlos a mano vale lo mismo que usar la ventana. También para los que
  lee la caja Amiga: su `.prefs` en IFF se genera del texto (abajo, "Sincronizado con la caja Amiga"). "Volver a
  fábrica" por categoría: quitar ese fichero de la capa del usuario (S4b dice cómo se deshace).
- **Lo protegido es solo lo de seguridad**: permisos, cuentas y ajustes de la máquina pasan por `guardian`, vengan de
  la ventana o del Shell (S10c, S6b). Todo lo demás lo cambia su dueño sin preguntas, por fichero, ventana u orden.
- **Sirve recién instalado**: la zona horaria sale de serie del idioma y país elegidos al instalar (S32), sin
  preguntar otra vez; se cambia en Fecha y hora. Hoy la hora es UTC (sección 2).
- Hoy los Ajustes (C1, C3) tienen 8 secciones; faltan Red, Sonido, Teclado, Idioma, Fecha y hora y Usuarios, el
  buscador por ajuste y la casación con `SYS:Prefs`/`ENVARC:` (que depende de las asignaciones nativas, paso 2).

### Y como un Workbench: el cajón `Prefs` con sus herramientas (2026-11-15)

Ajustes y el cajón `SYS:Prefs` son dos puertas a lo mismo, no dos sistemas:

- **Un programa por categoría en `SYS:Prefs`, con su icono**: `Apariencia`, `Fondo` (el `WBPattern`), `Monitores` (el
  `ScreenMode`), `Fuentes` (el `Font`), `Idioma` (el `Locale`), `Hora` (el `Time`), `Teclado y ratón` (el `Input`),
  `Puntero` (el `Pointer`), `Sonido`, `Red`, `Barra`, `Avisos`, `Energía`, `Usuarios`, `Permisos`, `Impresoras` (el
  `Printer`), `Programas`... Se abre el cajón en el Workbench y se lanza cada uno suelto, como siempre.
- **Ajustes es el contenedor**: su barra lateral lista los programas del cajón `SYS:Prefs` (y los de `Paquetes:` y
  `Home:` que traigan su `Prefs`), y cada categoría es ese mismo editor dentro de la ventana. Mismo código, mismos
  ficheros. Un programa instalado que trae su editor de preferencias aparece solo en los dos sitios.
- **Los tres botones del Amiga, con vista al momento de DMS**: lo que se toca se ve ya (va a `ENV:`); **Guardar** lo
  escribe también en `ENVARC:` (para siempre), **Usar** lo deja solo en esta sesión, **Cancelar** vuelve a como estaba.
  En la ventana de Ajustes, cerrar es Guardar (como DMS); en el editor suelto, los tres botones. Para que cerrar no
  deje sin vuelta atrás, la ventana de Ajustes lleva a la vista un "Deshacer cambios" por categoría, que vuelve a
  como estaba al abrirla (S0: deshacer antes que preguntar).
- **Presets**: `SYS:Prefs/Presets` (de fábrica) y `Home:Prefs/Presets` (los suyos): un fichero de preferencias con su
  icono; doble clic lo aplica. Exportar e importar es copiar ese fichero.
- **Desde el Shell y los guiones**, como en el Amiga: `Idioma FROM fichero USE` o `SAVE`, para ponerlo en
  `User-Startup` o en un guion. Eso carga un fichero entero. Para un ajuste suelto hay una orden común a todas las
  categorías, **`Ajuste`**: `Ajuste LEE clave` lo enseña y `Ajuste PON clave valor` lo cambia, con `USE` (solo esta
  sesión, `ENV:`) o `SAVE` (también en `ENVARC:`). Escribe el mismo fichero de texto que la ventana, así que lo que
  se hace con el ratón se hace igual en un guion. Si el ajuste es de la máquina, pide la acción de administrador. La
  sintaxis exacta y los nombres de las claves, por diseñar al hacerlo.
- **Sincronizado con la caja Amiga**: las preferencias que también entiende AmigaOS (idioma, país, fuentes, modo de
  pantalla, teclado, puntero, paleta) las necesita la caja en `ENV:Sys/*.prefs`, en el formato IFF del Amiga (S4b).
  Son justo las que más se tocan, y un IFF no se lee ni se edita a mano. Por eso manda el fichero de texto: el
  `.prefs` en IFF se genera de él cada vez que cambia. Si el usuario abre el `Locale` o el `Font` originales de 68k
  dentro de la caja y escriben el IFF, ese cambio se lleva al texto, y DashOS lo ve al momento. Cómo se casan los
  dos sentidos sin pisarse, por diseñar al hacerlo.

## 8. La tercera oleada: avisos, el Dash del reloj y el centro de control configurable (2026-11-16)

Portado de DMS v2.1.2 (MIT; sección 0). Detalle en `docs/bitacoras/dms-avisos.md`. Lo de serie, como en el escritorio
del usuario en shiva (`notificationPopupPosition = -1`, `notificationAnimationDuration = 400`, su
`controlCenterWidgets`, `radiusStrength = 38` y el radio de 3 de su niri).

- **Avisos**: el servicio lleva el modelo de DMS (actuales e historial, acciones con la de `default` para el clic en
  el cuerpo, sustituir un aviso propio, "no molestar" como estado de la sesión con plazo). Los emergentes (arriba en el
  centro, hasta 4, entran deslizándose, la pila se recoloca con el muelle, se paran con el ratón encima, barra de tiempo
  si se pide) y el centro de avisos (pestañas Actuales e Historial con su número, grupos por programa, "no molestar"
  con los plazos de DMS, Borrar todo, Ajustes, teclado) los pinta la barra (`avisos_vista.rs`).
- **Ajustes en texto** (S36, regla 1): `ENV:Escritorio/avisos` y `ENV:Escritorio/centro-control`. Los lee el centro de
  control, que vigila `ENV:` y se los da por una puerta a la barra y al panel: cambian en caliente, también con la orden
  `Ajuste`. En Ajustes, dos categorías nuevas: Avisos y Centro de control.
- **El Dash del reloj** (`dash_reloj.rs`): el Resumen de DMS con lo que tiene dato en DashOS: el reloj, quien está
  delante, los avisos actuales, el calendario del mes (con la zona horaria del sistema, empezando el lunes) y el sistema.
- **El centro de control** (el panel): la rejilla de DMS (8 columnas de 64, cada mosaico con su ancho y su alto, filas
  pequeñas de 48, detalle bajo la fila), con su modo de edición.

Lo que DMS no tiene y DashOS sí: pestañas en el centro de avisos (las pidió el usuario) y "no molestar" con horario.
