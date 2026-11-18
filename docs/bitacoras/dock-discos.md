# Bitácora: los discos del escritorio y el dock como el de DMS (rama `dock-discos`)

Rama `dock-discos`, copia de `fusion-fondo` (7baf481c: `revision` más la pantalla de bloqueo). Petición del usuario
(2026-11-17): "El docker de la derecha es horrible, estudia el de DMS para que tenga las mismas funcionalidades y si
puedes hacer cosas chulas de animación". Aclaró que era la **columna de discos** de la derecha (los iconos de volúmenes
del Workbench, S10). Primero pidió pasarlos al dock; luego corrigió: "No es necesario que pasen al dock, puede ser algo
independiente, pero más estético". Quedan así tres encargos: (1) los discos, una pieza propia y bonita, con
animaciones y sin gastar en reposo, configurable; (2) el dock con todas las funciones del de DMS; (3) animaciones en el
dock. Prioridad: los discos.

## Diseño en tres frases

Los discos son un panel flotante translúcido (como los widgets de escritorio de DMS), con una tarjeta por volumen: su
icono de la colección, su nombre, lo libre y una barra del espacio usado; lo pinta el Workbench (que ya tenía las raíces
y la Información) con lo que calcula `discos_modelo.rs`, sin nada de DashOS y probado en el PC. Las animaciones van con
los muelles y curvas de DMS (`muelle.rs`) y solo mientras algo se mueve: aparecer, montar y desmontar, el ratón encima,
la onda del clic y la barra que se llena. Los ajustes son texto en `ENV:Escritorio/discos` (vigilado: se aplican en
marcha), con filas en Ajustes > Dock; lo nuevo del dock (lupa, rebote, indicador, arrastrar, cuerpo) va en
`dock_vivo.rs`, también sin nada de DashOS.

## 1. Los discos del escritorio

### Lo que había (la columna) y dónde queda

| La columna (S10) | Ahora |
|---|---|
| Una capa de abajo de 104 de ancho y todo el alto, pegada a la derecha, transparente salvo los iconos | Una capa de abajo del tamaño del panel (248 × 300 con cuatro discos), arriba a la derecha y a 12 de los bordes, bajo la barra. Menos que mezclar en cada composición (antes 104 × alto entero) |
| SYS:, Home:, RAM:, Work: de la tabla de nombres; Home: y RAM: solo con la sesión de Ana | Igual (los mismos volúmenes y las mismas reglas); al entrar Ana, Home: y RAM: entran con la animación de montar, y al salir, salen |
| Clic: elegir. Doble clic: abrir en el explorador (o en la ventana de cajón si no está) | Igual, con la onda del clic |
| Botón derecho: la Información del volumen | Un menú (Abrir, Información) junto a la tarjeta; Información abre la misma ventana del Workbench |
| El ratón encima: una capa de la superficie | La capa de estado de Material (12 %) y el filo del acento, animados, y el icono crece un 10 % |
| Las líneas de la consola para las pruebas ("en pantalla: …, icono de SYS en (x, y)", "icono de Home en …") | Las mismas, con el centro de cada tarjeta: el guion y `tools/guion_iconos.py` siguen pulsando ahí |

Nada de lo que hacía la columna se pierde. La Información de Work: y RAM: decía "Por SYS: solo se puede leer" (solo se
podía cambiar por Home:); ahora se cambia por cualquier raíz de leer y escribir (Home:, Work:, RAM:), y solo SYS: es de
leer.

### El panel (`usuario/src/discos_modelo.rs`, `usuario/src/bin/workbench.rs`)

- Inspirado en `Modules/BuiltinDesktopPlugins/SystemMonitorWidget.qml` (el fondo con `transparency` 0,8, las filas de
  discos con su barra de 4 de alto en el acento, que pasa a aviso con más del 75 % y a error con más del 90 %) y en
  `Modules/ControlCenter/Details/DiskUsageDetail.qml` (icono, nombre, "usado / total", porcentaje). No se porta código
  de ahí: solo el aspecto.
- Medidas: pequeño 208 de ancho, tarjetas de 52 e iconos de 32; mediano 248, 60 y 40; grande 296, 72 y 48 (sin la barra
  del espacio, 12 menos de alto). Relleno 8, cabecera 32 ("Discos" con el símbolo `hard_drive`), 4 entre tarjetas,
  radio 16 el panel y 12 las tarjetas.
- Colores: el contenedor del tema con la opacidad de los ajustes y un filo de 1 en la variante del contorno (para que
  se vea sobre cualquier fondo); las tarjetas en el contenedor alto; el elegido en el contenedor secundario.
- Iconos de la colección (`kit/iconos/dashos.iconos`): SYS: `disco-sistema`, Home: `carpeta-casa`, RAM: `disco-ram`,
  Work: y los demás `disco-duro`.
- Lo que mide cada volumen sale de `INFO` del servicio de archivos por su raíz, cada 5 s; si cambia, la barra se mueve
  hasta el valor nuevo. SYS:, Home: y Work: están en el mismo disco y dicen lo mismo.
- Caben las tarjetas que quepan en el alto que dejan las barras (como mucho 24); las que no, se dicen ("no caben").

### Animaciones (todas en `discos_modelo.rs`, con `muelle.rs`)

| Qué | Cómo |
|---|---|
| Aparecer el panel | Se desliza 24 desde su lado y se aclara, con el muelle "expresivo" de 300 ms (el de DMS, 560/37, afinado con `tunedSpring`); las tarjetas entran tras él de una en una (120 ms y 70 ms entre cada una) |
| Montar | La tarjeta crece de alto desde 0 y se aclara, con el mismo muelle (se pasa un poco y vuelve); las de debajo se apartan con ella. La capa crece de golpe al empezar (no a cada cuadro) |
| Desmontar | Lo contrario, con el muelle "de serie" (sin rebote); la capa encoge al acabar. Una que vuelve antes de irse, vuelve desde donde estaba |
| El ratón encima | La capa de estado y el filo, con el muelle "rápido" de 150 ms; el icono crece un 10 % |
| Un clic | La onda de Material desde el punto pulsado: 450 ms con OutCubic, al 12 % y apagándose en la segunda mitad |
| El espacio | La barra se llena desde 0 al aparecer y va al valor nuevo cuando cambia (muelle de 700 ms) |
| El menú | Se aclara en 75 ms con OutQuart (como el del dock de DMS) |

Nada se anima hasta que la capa está en pantalla (si no, con el sistema ocupado la entrada acababa antes del primer
cuadro). Solo se pide otro cuadro (16,7 ms) mientras algo se mueve; quieto, el Workbench despierta cada 500 ms como
antes (mira la sesión cada 1,5 s y mide los volúmenes cada 5 s).

### Ajustes (`ENV:Escritorio/discos`, `usuario/src/discos_ajustes.rs`)

`mostrar` (sí), `lado` (derecha | izquierda), `altura` (arriba | centro | abajo), `tamano` (pequeño | mediano |
grande), `opacidad` (0-100, de serie 80), `espacio` (la barra, sí), `titulo` (la cabecera, sí), `animaciones` (sí).
Lo que no vale se queda de serie y se dice, como en `dock_ajustes.rs`. El Workbench lo lee de `ENV:` (o `ENVARC:`, o
el de fábrica sin sesión) y vigila `ENV:` y `ENV:Escritorio`: un cambio de Ajustes, de la orden `Ajuste` (por ejemplo
`Ajuste Escritorio/discos.lado izquierda`) o de un editor se ve en marcha. Cambiar de lado rehace la capa (y vuelve a
entrar); esconderlos la quita.

Ajustes > Dock tiene seis filas más (Intro cambia cada una, como las demás): enseñarlos, dónde (las seis
combinaciones de lado y altura), tamaño, opacidad (100, 90, 80, 66, 50), espacio usado y animaciones. Textos de
catálogo 150-161 de `centro` (con el inglés).

### El menú

Botón derecho en una tarjeta: Abrir (`arrow_forward`) e Información (`info`), en una capa de arriba junto al panel (del
lado de dentro de la pantalla, a la altura de la tarjeta). Se cierra con Esc, al elegir, con un clic en los discos, y
con el ratón 1,5 s fuera de los discos y del menú (de un clic en otra ventana no llega nada: esa es la forma de saberlo).
El teclado, a petición: uno exclusivo no dejaría escribir en otra ventana con el menú abierto.

## 2. El dock: las funciones del de DMS

Estudiado `Modules/Dock/*`, `Modules/SurfaceWidgets/*`, `Common/settings/DockConfig.js`, `Services/TrashService.qml`
y los ajustes de DMS v2.1.2. "Hecha" quiere decir que está en DashOS (antes de esta rama o en ella).

| Función de DMS | Estado |
|---|---|
| Fijados (`dockPins`), fijar y quitar desde el menú | Hecha (dms-dock) |
| Abiertos, agrupados por programa o no (`groupByApp`), fijados aparte (`separatePinnedAndRunningApps`) | Hecha |
| Indicadores `circle` y `line` (y ninguno), el activo tres veces más largo en el primario | Hecha; **ahora animado** (abajo) |
| Menú contextual: ventanas del grupo con su cerrar, fijar o quitar, cerrar la ventana o todas | Hecha |
| Menú: minimizar o restaurar una ventana | No aplica: DashOS no tiene minimizar |
| Menú: acciones del `.desktop`, "Launch on dGPU", "Edit widgets", saltos a Ajustes | No aplica (no hay acciones de escritorio, ni gráfica dedicada, ni widgets en el dock) |
| Autoocultar con 250 ms de gracia, aparecer y esconderse con el muelle "fast" de 75 ms | Hecha |
| `smartAutoHide` (esconderse solo si una ventana lo tapa) | Pendiente: el compositor no dice si una ventana tapa el borde; se lee |
| Posición (los cuatro bordes), modo compacto o barra (`taskbar`) y su alineación | Hecha |
| Por monitor (`screenPreferences`, `showOnLastDisplay`) | Hecha (un dock por monitor elegido) |
| Varios docks (uno por borde y monitor, `resolveEdge`) | Pendiente: un dock; varios piden otro fichero por dock y su sitio en Ajustes |
| `isolateDisplays` (solo las ventanas de su monitor) | Pendiente: el modelo lo admite, falta la clave |
| Tamaño de los iconos, relleno, separación, margen, hueco de la zona exclusiva | Hecha |
| Transparencia, borde (color, opacidad, grosor), radio | Hecha (`followInterfaceStyle` no: no hay estilo común de la interfaz aún) |
| Lanzador (`launcherEnabled`) | Hecha; el logo (`launcherLogoMode` "os", personalizado…): pendiente, se pinta el de "apps" |
| Límite de visibles y desbordado con su insignia (`maxVisibleApps`, `showOverflowBadge`) | Hecha |
| Clic central: cerrar o abrir otra (`middleClickAction`) | Hecha |
| Clic izquierdo: lanza, trae, pasa a la siguiente del grupo | Hecha; ahora al soltar, como DMS |
| Burbuja con el nombre; sale en el acto al pasar a otro icono y espera 250 ms solo al aparecer | Hecha; **corregida** (antes esperaba 250 ms en cada icono) |
| Lupa al pasar el ratón (`magnification`, `magnificationScale`, `magnificationProfile`: parabólica, coseno, gauss) | **Hecha en esta rama** (abajo); de serie apagada, como shiva |
| La lupa que aparta a los vecinos (`magnificationExpand`) | **Hecha en esta rama** (`aumento_expandir`) |
| Arrastrar los fijados para ordenarlos (pulsar 500 ms y mover 5 px) | **Hecha en esta rama**; el orden nuevo se escribe en `ENV:` y se guarda |
| Papelera (`showTrash`): icono vacío o lleno, abrir, vaciar con pregunta | Pendiente: el dock tendría que ver `Home:.Papelera` (la raíz de Home:, que no tiene) o que el explorador le diga cuántas hay y le deje abrirla (hoy el canal del explorador solo atiende al Workbench). Shiva la tiene apagada |
| Widgets en el dock y modo edición, `order`, `widgetExpansion` | No aplica aún: DashOS no tiene widgets de barra que se puedan poner en el dock |
| OSD (volumen, brillo, Bloq Mayús, espacio de trabajo) | Hecha (dms-dock; el brillo, sin quien lo lleve) |
| Pantalla completa (`showOnFullscreen`), `openOnOverview`, `useOverlayLayer`, `editOnRightClick` | No aplica: DashOS no tiene pantalla completa, vista general ni edición del dock |
| Colorear el activo (`appsDockColorizeActive`), solo el espacio actual (`runningAppsCurrentWorkspace`) | Pendiente (no están en shiva) |
| Teclado (flechas, Intro) e IPC (`dms ipc dock …`) | Pendiente: el dock no pide el teclado; no hay IPC del escritorio |
| Desenfoque detrás del dock | Pendiente: el compositor no desenfoca detrás de una capa |
| Iconos de los programas | Pendiente (de antes): la letra en un cuadrado, como DMS sin icono |

## 3. Las animaciones del dock (`usuario/src/dock_vivo.rs`)

| Qué | Cómo (de DMS salvo lo dicho) |
|---|---|
| Lupa | Escala = 1 + (M − 1)·f(d/R)·p; M = `escala_aumento` (1,05 a 2), R = 1,8 / 2,2 / 2,5 iconos (parabólica, gauss, coseno), f = 1 − u², ½(1 + cos πu) o (e^(−3u²) − e^(−3))/(1 − e^(−3)) en enteros; p entra en 50 ms con OutCubic y sale en 75 ms con InCubic. Crece desde el borde de la pantalla. Sin apartar, los vecinos se solapan; con `aumento_expandir`, cada uno se ensancha y aparta a los demás, y el cuerpo crece hasta su tope. La capa deja ese sitio de antemano (no cambia de tamaño a cada cuadro) |
| Rebote al abrir un programa | Propio (DMS no lo tiene; es el de macOS): el icono bota lo que deja el hueco de la capa para el saltito (el 35 % del icono), 560 ms cada bote, hasta que sale su ventana (acaba el bote que lleva) o 5 botes. Lo que sube y lo que crece con la lupa se recortan para no salirse de la capa (en la primera vuelta de capturas el icono subía de más y no se veía) |
| Indicador | El del activo se alarga o acorta en 75 ms con OutCubic (DMS); el punto de una ventana nueva crece desde nada con el muelle expresivo (propio: DMS lo pone sin animar) |
| Reordenar | El arrastrado sigue al ratón con el fondo y el filo del primario; los demás van a su hueco en 250 ms con `expressiveDefaultSpatial` (0,38, 1,21, 0,22, 1: se pasa y vuelve) |
| El cuerpo | Cambia de largo con el muelle "fast" de 75 ms cuando entra o sale un elemento (DMS) |
| El menú | Se aclara en 75 ms con OutQuart (DMS) |

Las curvas `EntradaCubica` y `ExpresivaEspacial` se añaden a `muelle.rs`. El dock solo pide otro cuadro si algo de
esto se mueve (`vivo`): con un icono crecido y quieto, o el ratón quieto sobre la lupa, no se repinta (lo de
tecla-pegada).

## Pruebas

- PC (`make prueba` las corre): `discos_modelo.rs` 26 (la primera entrada y las barras, montar con rebote y desmontar,
  volver antes de irse, poner otra vez mientras entran, el ratón y la onda, sin animaciones, lados y alturas, **mil
  volúmenes y pantallas de 0 a 100.000 de alto**, **10.000 ráfagas de montar y desmontar al azar con el ratón y clics
  por medio y pasos de reloj absurdos**, ajustes en marcha), `discos_ajustes.rs` (de serie, ida y vuelta, lo que no
  vale, **2.000 ficheros de bytes al azar**), `dock_vivo.rs` 27 (e^-x y cos πx en enteros, los tres perfiles, las
  escalas de DMS, apartar con tope, la lupa entra y sale, el rebote, el indicador, arrastrar, **destinos absurdos y mil
  fijados arrastrados al azar: siempre una permutación**), `muelle.rs` (las dos curvas nuevas), `dock_ajustes.rs` y
  `dock_modelo.rs` (con `aumento_expandir` y el sitio de la lupa).
- `make prueba`: BIEN (esperado.txt y cargador hunk); 66 baterías de `cargo test`, todas bien.
- `make arranque-virt`: tres vueltas. La 1.ª, BIEN entera con el reinicio (la comprobación del disco no la vi: el
  registro lo pisó otra sesión que escribe en la misma carpeta temporal). La 2.ª (ya con los commits) se paró en
  "[ram] Ana: avisos hostiles": llegaron 16 de los 63 timbres en los 300 ms que espera la prueba (con otras tres QEMU
  de otras sesiones en marcha); no toca nada de lo de aquí (vigila otro fichero, en otra sesión). La 3.ª, BIEN entera:
  las líneas, el reinicio, el disco leído en Linux, la captura, el sonido y U6.
- `make arranque-virt-2`, `-escritorio`, `-dms` y `-shell`: BIEN a la primera.
- Tras recortar el rebote a la capa (último cambio del dock): `make prueba`, `make arranque-virt` (entera, con el disco,
  la captura, el sonido y U6) y `make arranque-virt-dms`: BIEN a la primera.
- `make capturas-discos` (con `DMS_SHIVA=1` y el fondo del usuario): cuatro vueltas, que sacaron cuatro fallos ya
  arreglados: medir los volúmenes antes de saber el alto de la pantalla ("1 no caben"); el menú se cerraba al pasar el
  ratón por la esquina (el guion); la animación de entrada acababa antes del primer cuadro con el sistema ocupado (ahora
  nada se mueve hasta que la capa está en pantalla); y las tarjetas que entraban de una en una se quedaban sin entrar si
  se volvían a poner los volúmenes a medias (al entrar la sesión: habría dejado SYS: y Work: invisibles). La última
  prueba de PC de arriba es la de ese fallo.

## Capturas (fuera de git, `~/recuperar/geosian-7d/dock-discos-capturas/`)

A 1920 × 1080 con la imagen de usar, el perfil de shiva (tema oscuro, su barra) y su fondo ("Lofi - Chill Room2", en PNG
de 128 colores, solo en el disco de la prueba): `shiva.1-discos` (quietos), `2-encima`, `3-onda`, `4-elegido`,
`5-menu`, `6-informacion` (la de Work:), `7-explorador` (doble clic en SYS:), `8-izquierda` (a la izquierda, abajo,
grande), `9-compacto` (en el centro, pequeño, sin la barra), y del dock `10-dock-lupa` y `11-dock-lupa-entre` (la
lupa sobre un icono y entre dos) y `12-dock-menu`. GIF: `discos-aparecer.gif` (el panel entra y las tarjetas tras él),
`discos-entrar-sesion.gif` (al entrar Ana; la pantalla de entrada tapa casi toda la animación), `dock-lupa.gif` (el
ratón de lado a lado por el dock) y `dock-rebote.gif` (el Shell fijado, abierto desde el dock, bota hasta que sale su
ventana). Los GIF son tiras de `screendump` de QEMU cada 50-70 ms: van a saltos, no a 60 cuadros por segundo.
El guion de las capturas es `tools/guion_discos.py` (`make capturas-discos`, con `DMS_SHIVA=1`, `DASHOS_PANTALLA`,
`DASHOS_FONDO_EXTRA` y `DASHOS_PREFS` como `arranque-virt-dms`). El fondo no siempre sale (el programa `fondo`
arranca antes que el servicio de archivos: lo apuntó `union-dms`); en las vueltas en que no salió, el fondo es azul.

## Medido

Con `cpu_propia.rs` (el tiempo de CPU del grupo de cada programa; lo dicen en la consola unas pocas veces), en QEMU
(TCG, sin aceleración: todo va muchas veces más lento que en una máquina de verdad):

| | Quieto (10 s) | Animando |
|---|---|---|
| Discos (Workbench), 800 × 600 | 3,8 a 4,7 ms de CPU (0,05 %) | 27 a 40 % de un núcleo mientras dura (de 0,3 a 1,3 s cada vez) |
| Discos, 1920 × 1080 (antes de limitar los cuadros) | 4,6 ms | 40 a 59 % |
| Dock, 800 × 600 | 7,4 a 7,6 ms | 1,2 a 5,5 % (salir, esconderse, el saltito) |
| Dock, 1920 × 1080 con la lupa | 1,7 a 16 ms | 7 a 9 % moviendo el ratón por encima |

Quietos no gastan: el Workbench despierta cada 500 ms (como antes) y el dock cada segundo. Animando, los discos
pintan el panel entero en cada cuadro (unos 6 ms por cuadro en QEMU); al principio se pintaba más deprisa que la
pantalla (cada commit trae un aviso del compositor que despierta el bucle) y ahora va como mucho a un cuadro cada
16,7 ms. Lo que queda por bajar, si hace falta: pintar solo la tarjeta que cambia.

Tamaños (ELF, frente a `fusion-fondo`): `workbench` 532.832 → 655.712 bytes (+120 KB: la colección de iconos y la
letra regular), `dock` 414.128 → 430.512 (+16 KB), `centro` 857.416 → 865.608 (+8 KB). Van en la imagen del kernel: el
disco de pruebas no cambia. La capa de los discos: 248 × 300 × 4 = 290 KB con cuatro (la columna era 104 × 560 × 4 =
227 KB a 800 × 600, y 104 × 1040 × 4 = 422 KB a 1920 × 1080). El menú, 188 × 92 × 4 = 68 KB mientras se ve.

## Pendiente del usuario

1. **Dónde van los discos** de serie: arriba a la derecha, donde estaba la columna. Mediano, 80 % de opacidad, con la
   barra del espacio y la cabecera "Discos".
2. **Los ajustes de los discos en Ajustes > Dock** (seis filas al final): no hay una categoría "Escritorio". Una
   categoría nueva tras Acerca de choca con el plazo del guion (lo apuntó `union-dms`).
3. **El menú de un disco**: Abrir e Información. "Expulsar" no sale: ningún volumen de hoy se puede expulsar.
4. **El rebote al abrir** un programa desde el dock es de DashOS (de macOS), no de DMS. Se quita con las animaciones
   apagadas (`BASE_MS`) o, si se quiere, con un ajuste propio.
5. **Home: con el icono `carpeta-casa`** (la colección no tiene un disco con casa).

## Lo que no se hizo

- Volúmenes que se montan de verdad (USB, disquetes, cajas): no hay quien los monte y avise (el `imagen` del ADF
  monta para su prueba, sin avisar a nadie). El panel ya los admite (cualquier número, entrar y salir animados) y el
  icono de cada clase está en la colección (`usb`, `disquete`, `disco-amiga`…): falta el aviso del servicio.
- Arrastrar ficheros encima de un disco: el compositor no tiene arrastrar entre clientes (el explorador arrastra entre
  sus propias ventanas).
- Avisar al montar o desmontar: hoy solo pasa al entrar y salir la sesión (Home: y RAM:), que sería un aviso en cada
  entrada; se deja para cuando haya volúmenes extraíbles.

## Decisiones del usuario (2026-11-18)

- Los discos como están: panel flotante arriba a la derecha, tarjetas con icono, nombre, libre y barra ("Me gusta
  como has puesto los discos así").
- El rebote al abrir de macOS, no ("ya un poco antiguo"): de serie, el punto de abierto aparece y late suave hasta que
  sale la ventana (como GNOME/Windows 11); el rebote queda solo como opción en Ajustes > Dock, apagada de serie.
