# S10. Escritorio

## El problema

El encargo pide un escritorio moderno: ventanas flotantes y en mosaico, escritorios virtuales, varios monitores,
lanzador, notificaciones y bandeja. Y que tenga el aire del Amiga, hecho moderno.

El usuario trabaja a diario con niri y con mango, y con DMS como barra y lanzador. Son las mejores referencias de lo
que le gusta usar.

Lo que ya está decidido o recomendado:

- El compositor habla Wayland y tiene presupuesto de plazo (S2, S9).
- Las ventanas Amiga son ventanas del escritorio, y la pantalla de Workbench es el escritorio (S4).
- `DEVS:Monitors` y `Prefs/ScreenMode` describen los monitores (S4b).
- La organización es la de AmigaOS: volúmenes, cajones, iconos `.info` y Prefs (S4b).

## Lo que hacen las referencias

- **niri**: mosaico con desplazamiento. Las ventanas van en columnas sobre una tira infinita hacia la derecha; abrir
  una ventana nueva nunca encoge las demás. Cada monitor tiene su tira y sus escritorios, que se crean y se borran
  solos, siempre con uno vacío abajo. Al desconectar un monitor, sus escritorios pasan a otro, y vuelven al
  reconectarlo. Ventanas flotantes desde la versión 25.01. Columnas con pestañas. No es un escritorio completo:
  necesita barra y lanzador aparte.
- **mango**: basado en dwl. Muchas disposiciones de mosaico (maestro y pila, desplazamiento, rejilla, monóculo...).
  Etiquetas en vez de escritorios, cada una con su disposición. Cualquier ventana puede pasar a flotante. Vista
  general de todas las ventanas.
- **DMS**: la barra, el lanzador, las notificaciones y los ajustes, como un proceso aparte del compositor.
- **Workbench del Amiga**: iconos en el escritorio, cajones como ventanas, menús en la barra de la pantalla con el
  botón derecho, pantallas que se arrastran hacia abajo para ver la de detrás, y Commodities (Exchange) para atajos
  y utilidades de fondo.

## Recomendación

### Pantallas: escritorios virtuales a lo Amiga

En el Amiga, cada programa podía abrir su propia pantalla, y se pasaba de una a otra arrastrándolas hacia abajo o con
Amiga izquierda + M. **En DashOS, los escritorios virtuales son "pantallas"**, con ese mismo gesto:

- Cada monitor tiene sus propias pantallas, como en niri. Se crean y se borran solas, siempre con una vacía.
- Se cambia de pantalla con el teclado (Amiga + M / N, o Super + flechas), con un gesto del panel táctil, o
  arrastrando la barra de la pantalla hacia abajo, como en el Amiga. Al arrastrar se ve la de detrás.
- Una pantalla Amiga propia (`OpenScreen` de un programa 68k, S4) es una pantalla más de esta lista.
- Al desconectar un monitor, sus pantallas pasan a otro y vuelven al reconectarlo, como en niri.

### Mosaico y flotantes a la vez, de forma natural

Petición del usuario (2026-11-03): que se puedan usar mosaicos y ventanas flotantes, pero de forma natural. En niri y
mango, flotar es un modo aparte: una tecla cambia la ventana de capa. La propuesta es que no haga falta pensar en
modos:

- **Arrastrar saca una ventana del mosaico.** Si coges una ventana por su barra y la sueltas lejos de la tira, se
  queda flotando donde la sueltas. Si la arrastras de vuelta a la tira, se coloca en la columna que marca una guía.
- **Los bordes ayudan.** Llevar una ventana flotante al borde de la pantalla la encaja a media pantalla o a un
  cuarto, como en GNOME o Windows.
- **Cada ventana nace donde le toca.** Los diálogos, las ventanas pequeñas y las de tamaño fijo nacen flotando; las
  normales, en el mosaico. Cada pantalla dice si las nuevas llegan al mosaico o flotando; la pantalla de Workbench,
  flotando.
- **Se recuerda.** Si una aplicación se dejó flotando, la próxima vez vuelve a flotar en el mismo sitio.
- **El teclado también vale**: una tecla manda la ventana al mosaico o la saca, para quien no use el ratón.

### Cada pantalla elige su disposición

- **Flotante**: como el Workbench clásico.
- **Mosaico con desplazamiento**: como niri. Es la disposición de mosaico por defecto.
- **Maestro y pila**, **rejilla** y **monóculo**: como mango, para quien las prefiera.
- **Cualquier ventana puede flotar** encima de una pantalla en mosaico. Los diálogos y las ventanas pequeñas flotan
  solas.
- **Columnas con pestañas**, como niri.
- **Vista general** de todas las ventanas y pantallas, con una tecla.

Se configura en `Prefs/Escritorio`, con aplicación inmediata (S4b), y por teclado se cambia al momento.

### Workbench

- Los iconos de volúmenes, cajones y programas están en el fondo de la pantalla de Workbench, como en el Amiga.
- Los cajones se abren como ventanas, con iconos o como lista.
- Los programas dejan iconos de aplicación (AppIcons) donde soltar ficheros, como en el Amiga.
- "Información" de un icono enseña y cambia sus tooltypes, su comentario, sus permisos (S6) y su dueño.

### La barra de la pantalla

Arriba de cada pantalla, como en el Amiga, y con lo de DMS a la derecha:

- **A la izquierda**: el título de la pantalla y, con el botón derecho, **los menús del programa activo**, como en el
  Amiga. Los programas Wayland de Linux con menú propio lo siguen teniendo dentro de su ventana.
- **A la derecha**: bandeja de iconos de programas, red, sonido, batería si la hay, reloj y notificaciones.

### Lanzador, notificaciones y Commodities

- **Lanzador por teclado** (Amiga derecha + espacio, o Super), como el de DMS: busca programas nativos, Amiga y Linux
  (S7, S8), ficheros, órdenes y ajustes.
- **Notificaciones** con historial. Los programas de Linux llegan por la norma de freedesktop a través de la caja
  Linux. Los Amiga, por un puente.
- **Commodities**, como en el Amiga: atajos de teclado globales y utilidades de fondo, que se ven y se paran desde
  `Exchange`.

### ARexx en el escritorio

El escritorio tiene su puerto ARexx, como Workbench desde AmigaOS 3.5: abrir cajones, mover ventanas, cambiar de
pantalla, lanzar programas. Así se automatiza con guiones, igual que con el IPC de niri y mango.

### Varios monitores

- Cada monitor, su escala y sus pantallas (S9).
- Mover una ventana a otro monitor con el teclado o arrastrando.
- La barra en cada monitor.

### Tan moderno como DMS, sin Quickshell

Petición del usuario (2026-11-03): el escritorio tiene que estar al nivel de DMS, pero sin Quickshell.

- **Lo que hay que igualar de DMS**: la barra con sus módulos, el centro de control (red, Bluetooth, sonido, brillo),
  el centro de notificaciones, el lanzador, el historial del portapapeles, los avisos en pantalla de volumen y
  brillo, el menú de apagado, la pantalla de bloqueo, los controles de música, el calendario, los indicadores del
  sistema y los colores sacados del fondo de pantalla.
- **Sin Quickshell**: el escritorio se escribe con el motor de interfaz nativo de DashOS (S11), en código compilado
  (Dash o Rust), sin QML ni un intérprete de JavaScript. Se pinta con el aspecto de Material 3.
- **Por qué importa**: en shiva, DMS sobre Quickshell se ha caído al recuperar la pantalla tras apagarla de noche, y
  hay que recompilarlo cada vez que se actualiza Qt (memoria de esta máquina). Un escritorio nativo no depende de
  otra pila que cambie por debajo.
- **La pantalla de bloqueo no es del escritorio, es del compositor.** Si el escritorio se cae, la pantalla sigue
  bloqueada y sigue aceptando la contraseña. En shiva pasó justo lo contrario: con el escritorio caído, la pantalla de
  bloqueo se quedó sin entrada. Así no puede pasar.

### Quién hace qué

Como DMS con niri: **el compositor y el escritorio son procesos distintos.**

- **El compositor** (S9) solo compone, coloca ventanas y lleva la entrada. Es pequeño, crítico y con presupuesto de
  plazo (S2).
- **El escritorio** (Workbench, barra, lanzador, notificaciones, Commodities) es otro proceso, cliente del
  compositor. Si falla, se reinicia y las ventanas siguen en su sitio.

### Aspecto

Decidido por el usuario (2026-11-03): **aspecto de Material Design** (Material 3), con el modelo de MUI por debajo
(S11).

- La estructura es la del Amiga: pantallas, barra arriba con los menús al botón derecho, Workbench con iconos y
  cajones, gadgets de cerrar y de profundidad.
- El dibujo es el de Material 3: esquinas redondeadas, superficies con tono, tipografía y espaciado de Material, y
  animaciones cortas que se pueden quitar.
- Colores sacados del fondo de pantalla, como Material You y como hace DMS con matugen. Tema claro y oscuro.
- Un tema "Amiga clásico" como opción, para quien quiera el aire de MagicWB.

## Lo que queda abierto

1. ¿Escritorios virtuales como "pantallas" del Amiga, con el gesto de arrastrar hacia abajo? Recomendado: sí.
2. ¿Disposición por defecto de una pantalla nueva: flotante (Workbench clásico) o mosaico con desplazamiento (niri)?
   Recomendado: Workbench flotante, y el resto en mosaico con desplazamiento.
3. ¿Menús del programa en la barra de la pantalla con el botón derecho, como el Amiga? Recomendado: sí.

## Cómo encaja

- Con S0: principio 1 (el compositor no se traba) y principio 4 (un solo escritorio).
- Con S2: el compositor tiene plazo; el escritorio es un proceso normal.
- Con S4: las pantallas Amiga son pantallas del escritorio, y sus ventanas, ventanas normales.
- Con S4b: Workbench muestra volúmenes, cajones e iconos `.info`; `Prefs/Escritorio`.
- Con S6: "Información" enseña dueño y permisos; el escritorio no puede leer lo que no le toca.
- Con S7 y S8: el lanzador encuentra programas de los tres mundos.
- Con S9: escala por monitor y conexión en caliente.
- S11: el kit de interfaz nativo trae los menús a la barra.

## Fuentes

- niri: [README de niri](https://github.com/xiro-codes/niri) y [niri 25.02](https://linuxiac.com/niri-25-02-wayland-compositor-released/).
- mango: [wiki de mango](https://github.com/mangowm/mango/wiki/basics:) y
  [repositorio](https://github.com/DreamMaoMao/mangowc).
- DMS y Workbench: conocidos por el uso diario del usuario (memoria de esta máquina) y por la documentación de
  AmigaOS; no se consultó nada nuevo en esta vuelta.

No se comprobó nada en código: es diseño.
