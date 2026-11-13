# S26. El aspecto de los programas de las cajas

Del 2026-11-13, a pedido del usuario: "Hay que ir trabajando en tener el entorno visualmente homogéneo: que sea
configurable que las cajas Amiga, Linux, etc. muestren sus widgets con el estilo de DashOS (Material Design) o que
mantengan su estilo original (MUI, GadTools, ReAction... y en Linux el suyo)." Y después: "aprende de cómo se hace en
DMS (DankMaterialShell sobre Niri) y lo aplicamos": color dinámico al estilo de Material You, sacado del fondo de
pantalla y repartido a todo.

Rama `aspecto-cajas`, sacada de `revision` (d819301). Sala limpia: la especificación de Material 3 (m3.material.io,
color dinámico, HCT, paletas tonales, esquema "tonal spot"), las de freedesktop (portal de ajustes, `color-scheme`,
`accent-color`), la documentación de GTK (nodos de CSS, `settings.ini`, GSettings) y de Qt (temas de la plataforma). De
DMS y matugen, solo ideas, de su documentación; nada de su código, ni de GNOME, KDE, Adwaita, temas de terceros, UAE,
AROS ni MUI.

## Dónde estamos

- **El kit** (`kit/`) pinta con Material 3: `Tema` con los 13 papeles del esquema y sus derivados (contenedores),
  claro y oscuro, desde cualquier color (HCT sobre CAM16, `kit/src/color.rs`). La semilla de una imagen
  (`kit/src/semilla.rs`: muestras, cuantización de Wu, puntuación por matiz y croma) está hecha desde la descripción
  pública del método de Material (C3). Letra: Roboto y Roboto Mono (OFL), en `kit/fuentes`.
- **El tema del sistema** lo pone el centro de control (Apariencia: modo y color) y lo reparte el compositor por
  `dashos_tema_v1` (color, modo y, desde C3, los esquemas ya hechos de un color que no está en la tabla).
- **La caja Amiga**: intuition, graphics y GadTools son nuestros, en Rust (`caja/intuition`); las ventanas Amiga son
  ventanas del compositor con su borde de Intuition dentro. GadTools lo está haciendo otro agente (rama
  `caja-amiga-gadtools`) con un solo punto de pintado (`Tema` con `Plumas`, las de la `DrawInfo`) y el aspecto clásico.
- **La caja Linux**: Arch Linux ARM en una máquina virtual; sus programas salen por el puente de Wayland (vsock) como
  ventanas de DashOS. GTK, Qt y Firefox se pintan solos. La barra de DashOS para quien la pide (`xdg-decoration`) está
  en la rama `caja-linux-apps`, aún sin fusionar.

## Cómo lo hace DMS (ideas)

DMS es un escritorio para niri hecho con Quickshell. Lo que hace con el color, según su documentación:

1. Una herramienta (matugen) saca del fondo de pantalla la paleta de Material You (claro y oscuro).
2. Con ella **rellena plantillas**, una por pieza: el `gtk.css` de GTK 3 y GTK 4 (con los colores con nombre de
   libadwaita), el esquema de qt6ct o Kvantum, los colores de foot, kitty y otras terminales, los bordes y el foco de
   niri, el tema de Firefox...
3. El modo oscuro va por el **portal de ajustes** de freedesktop (`color-scheme`), que leen GTK 4, libadwaita, Qt,
   Firefox y Electron.
4. La misma letra, iconos y cursor para todo; los bordes de las ventanas los pone el compositor (`prefer-no-csd`).
5. Al cambiar el fondo se regenera todo y se avisa a los programas para que recarguen.

DashOS hace lo mismo con piezas propias: la paleta la calcula el kit (desde la especificación), el centro de control
hace de servicio del tema, el compositor la reparte, y en cada caja un agente rellena las plantillas de esa caja.

## Lo que se propone

### 1. Un tema único de DashOS, que sale del fondo

El tema es uno y lo tienen todos: el escritorio de DashOS, sus programas y las cajas.

| Qué | De dónde | Cómo viaja |
|---|---|---|
| La semilla: **"Paleta del fondo"** (de serie) o **"Color fijo"** (uno de los acentos) | El centro, con `kit::semilla` del fondo que pinta `fondo` (reducido a unas 16 000 muestras) | `dashos_tema_v1.tema` (color y modo) |
| Los esquemas claro y oscuro (13 papeles cada uno) | Los calcula el centro una vez por fondo (HCT, paletas tonales, "tonal spot") y los guarda en `Prefs/apariencia` | `dashos_tema_v1.esquema` (así nadie más los calcula: sin FPU cuestan medio segundo) |
| Modo claro u oscuro | Centro o panel rápido | `dashos_tema_v1.tema` |
| Letra (Roboto, Roboto Mono), tamaños (14 y 16 px), radios (botones redondos, 12 en ventanas) | Los del kit, fijos hoy | Constantes del kit y del agente; cuando C3 tenga "letras", un evento más (versión 4) |
| El aspecto de los programas de las cajas | Centro, `Prefs/cajas` | `dashos_tema_v1.cajas` (versión 3, nuevo) |

**El servicio del tema es el centro de control**, no un servicio nuevo: ya calcula la paleta, ya guarda la apariencia y
es el único al que el compositor deja cambiarla. Lo que le faltaba: ponerla **al arrancar** (antes el tema del
compositor era el de serie hasta abrir Ajustes) y que la de serie sea la paleta del fondo. Al cambiar de fondo desde
el centro, con la paleta del fondo elegida, la paleta sale del nuevo y llega a todos. El compositor guarda lo último y
se lo dice a cada programa al enlazar y cada vez que cambia: el reparto es el de siempre, con un evento más.

### 2. El ajuste "Aspecto de los programas de las cajas"

En Apariencia, dos filas: **"Programas de las cajas: como DashOS"** y **"Programas de las cajas: originales"**. Debajo
de eso hay excepciones, por caja y por programa, en `Prefs/cajas` (el mismo texto que viaja por el protocolo,
`wayland/src/aspecto.rs`):

```
general dashos
caja amiga original
programa linux original gimp
programa amiga dashos Directory Opus
```

Gana la excepción del programa, después la de la caja, después la general. Las excepciones se ponen a mano en el
fichero por ahora; en la interfaz, la idea es ponerlas donde S23 pone la "compatibilidad por programa": en las
propiedades del programa (Información en el Workbench, o la ficha del programa de Linux), con una casilla "Aspecto
original". El centro solo dice cuántas hay.

### 3. La caja Amiga: qué se puede y qué no

La caja pinta con nuestras bibliotecas, así que lo que pintan ellas se puede cambiar. Lo que pinta el programa, no.

| Pieza | ¿Cambia con "como DashOS"? | Por qué y cómo |
|---|---|---|
| Bordes de ventana, barra de título, gadgets de cerrar, profundidad y tamaño (intuition, `sysiclass`) | Sí | Los pinta `caja/intuition`. Mismo tamaño (`BorderLeft`, `BorderTop`...): los programas colocan sus cosas contando con ellos |
| Gadgets y menús de GadTools | Sí, con límites | Los pinta el `Tema` de GadTools. Un pintor Material: botones con esquinas, casillas e interruptores de Material, menús como los del kit. **Sin cambiar el tamaño**: el programa los coloca con píxeles, así que nada de radios grandes ni márgenes de Material; dentro de su rectángulo |
| Colores de la `DrawInfo` (`dri_Pens`) | Sí | `caja/pegamento/src/tema.rs` saca las 12 plumas de los papeles de Material (fondo y texto de la superficie, relleno del primario, relieve de los contornos). Los programas que dibujan con las plumas de la `DrawInfo` se tiñen solos |
| La paleta de la pantalla (colores 0 a 7) | **No** | Los iconos (MagicWB) y los programas que pintan con plumas fijas cuentan con esos colores. La pantalla de Workbench es de 32 bits: las plumas de DashOS son colores nuevos (`ObtainPen`) y la `DrawInfo` apunta a ellos; la paleta de siempre no se toca |
| BOOPSI de ReAction (window.class, layout.gadget...) | Sí, cuando sean nuestras | Clases nativas pintadas con el mismo `Tema`. Las de terceros, no |
| MUI | Sí, si `muimaster.library` es la nativa sobre el kit (S11: el kit sigue el modelo de MUI); con la MUI de terceros, no | Con la MUI de 68k, MUI pinta con sus propias imágenes y su configuración (MUI prefs); como mucho se le da una configuración con colores de DashOS |
| Lo que pinta el programa con graphics (`RectFill`, `Text`, `BltBitMap`...) | No | Es suyo. Un juego, un programa de dibujo o un `Text` con colores fijos se ven igual |
| La letra | Opcional, aparte | Los programas Amiga colocan el texto contando con la altura de topaz/8 (o la de `font.prefs`). Una letra de DashOS (Roboto en mapa de bits, unos 13 px) movería sus controles. Recomendado: topaz de serie y la de DashOS solo por programa |

**Bordes de ventana.** Con "como DashOS", intuition pinta su barra y sus gadgets con el pintor Material (superficie,
título en el centro, iconos de Material para cerrar y profundidad), con el mismo tamaño de borde; el compositor pone
las esquinas redondeadas y la sombra, sin otra barra encima (dos barras serían raras). Con "original", los bordes de
3.x, como ahora, con la sombra del compositor. Esto concreta la pregunta abierta de S4 y S23 ("Amiga por dentro, borde
de DashOS por fuera").

### 4. La caja Linux: qué se puede y qué no

Dentro de Arch, un agente (`dashos-aspecto`, `caja-linux/aspecto`) se conecta al compositor como un programa de
Wayland más, por el mismo puente, enlaza `dashos_tema_v1` y, cada vez que cambia, rellena las plantillas. No hace
falta otro canal: el puente ya va por vsock y el tema es de solo lectura para Linux (cambiarlo es un error de
protocolo, con su prueba).

| Programa | Con "como DashOS" | Con "original" | Cómo |
|---|---|---|---|
| GTK 3 | Tema "DashOS": colores de Material, Roboto, botones redondos, casillas, interruptores, deslizadores, pestañas, listas y menús de Material | Adwaita tal cual | Un tema hecho aquí: importa el de GTK por su recurso (no se copia nada) y pone encima reglas con los papeles de Material. GTK no tiene `!important`: cada selector lleva `:not(#dashos)`, el peso de un identificador, para ganar a los estados del tema de debajo. Lo elige GSettings (con `GSETTINGS_BACKEND=keyfile`, un fichero) |
| GTK 4 sin libadwaita | Lo mismo | Adwaita (el "Default" de GTK) | Lo mismo; sin portal, GTK 4 lo lee de `settings.ini` |
| libadwaita (GNOME) | Colores (acento, ventanas, vistas, barras) | Los suyos | libadwaita no usa temas, pero lee `~/.config/gtk-4.0/gtk.css`: ahí van solo los colores con nombre y sus variables. Su forma (radios, tamaños) es la suya |
| Qt 5 y 6 | Acento y letra de DashOS, forma de Fusion | Lo suyo | `QT_QPA_PLATFORMTHEME=gtk3`: Qt toma los colores y la letra del tema de GTK 3. Mejor después: un esquema de qt6ct o un tema de Kvantum generado |
| foot y otras terminales | Fondo, texto, selección y cursor de DashOS; Roboto Mono | Su configuración | Una plantilla por terminal (foot hecha) |
| Firefox, Chromium, Electron | Modo oscuro y, en parte, colores de GTK | Lo suyo | Por el portal de ajustes (`color-scheme`, `accent-color`): pendiente |
| Programas que pintan solos (SDL, juegos, Blender) | No | — | Es suyo |

**Al cambiar el tema con el programa abierto.** GTK 3 cambia en el acto: GLib vigila el fichero de GSettings y el
agente pone cada vez un nombre de tema nuevo ("DashOS-1", "DashOS-2"), que es lo que hace a GTK volver a leerlo. GTK 4,
libadwaita, Qt y foot lo ven al abrirse otra vez. Para que cambien en el acto falta el **portal de ajustes**
(`org.freedesktop.portal.Settings`: `color-scheme`, `accent-color`, `gtk-theme` y la letra, con su aviso
`SettingChanged`), que es lo que escuchan GTK 4, libadwaita, Qt 6, Firefox y Electron. Se puede hacer en el mismo agente
(un servicio de D-Bus mínimo en el bus de la sesión de root), sin xdg-desktop-portal.

**Excepciones por programa.** Un envoltorio en `/run/dashos-aspecto/bin` (delante en el PATH) con el nombre del
programa, que pone `GTK_THEME` (DashOS o Adwaita) y lo de Qt y ejecuta el de verdad. Solo nombres de letras, cifras y
`._+-`. libadwaita no hace caso de `GTK_THEME` para su forma: sus excepciones solo quitan el tema de GTK.

**Bordes de ventana.** GTK dibuja los suyos (CSD) siempre en GTK 4 y con barra de cabecera en GTK 3: con "como DashOS"
su barra sale con los colores de DashOS; con "original", la de Adwaita. Los programas que piden la barra del
compositor (`xdg-decoration`: foot, Qt, Firefox con la opción) llevan la de DashOS en los dos casos (no hay otra que
darles). Una opción "preferir la barra de DashOS" (`prefer-no-csd`, como DMS) no sirve para GTK 4, que no sabe
dejarla.

### 5. Las cajas que vengan

La caja PowerPC (S17) es otra caja Amiga (OS4, MorphOS: ReAction y MUI): la misma `Caja::Ppc` del ajuste, las mismas
plumas y el mismo pintor de ReAction y MUI nativos.

## Lo hecho en este primer paso

- **El ajuste** (`wayland/src/aspecto.rs`): formato, lectura que rechaza todo lo que no vale (con pruebas y basura al
  azar) y quién gana.
- **El protocolo**: `dashos_tema_v1` versión 3, petición y evento `cajas`. Solo el centro lo cambia; un texto que no
  vale es un error de protocolo; se avisa a los suscritos de la versión 3 solo si cambia (`wayland/src/tests.rs`,
  `aspecto_de_las_cajas`, con sus hostiles).
- **El centro** (servicio del tema): al arrancar pone en el compositor la apariencia guardada y el aspecto de las cajas
  (la de serie: paleta del fondo, como DashOS), calculando la paleta del fondo si hace falta. En Apariencia, "Color fijo
  ..." y "Paleta del fondo", y las dos filas del aspecto de las cajas, que se guardan en `Prefs/cajas`.
- **La caja Amiga**: enlaza el tema (versión 3), guarda color, modo y ajuste y saca las 12 plumas de Material
  (`caja/pegamento/src/tema.rs`), donde el pintor de GadTools las podrá leer. Aún no pinta con ellas: eso, con el
  pintor Material, después de fusionar GadTools.
- **La caja Linux**: `dashos-aspecto` (Rust, estático) con las plantillas de GTK 3, GTK 4, libadwaita, GSettings, Qt y
  foot, y los envoltorios de las excepciones. `tools/aspecto_caja_linux.py preparar` lo mete, con el puente de esta
  rama, unas muestras de GTK y las letras de DashOS, en una copia de la imagen de Arch con GTK (la de
  `caja-linux-apps`). `make arranque-virt-aspecto` lo prueba entero y saca la captura.
- Probado también en el PC con el compositor de DashOS (`compositor-pc`): GTK 3, GTK 4 y Qt 6 con los dos aspectos, y
  GTK 3 cambiando de tema con la ventana abierta.

## Orden de trabajo recomendado

1. **Fusionar esto** (protocolo v3, centro, agente) y llevar el agente a `preparar-arch` cuando se fusione
   `caja-linux-apps` (que trae GTK y la barra de DashOS).
2. **El portal de ajustes en el agente** (D-Bus mínimo): GTK 4, libadwaita, Qt 6, Firefox y Electron cambian en el
   acto y toman el modo y el acento.
3. **El pintor Material de GadTools y de los bordes de intuition**, con las plumas de `tema.rs`, en cuanto se fusione
   GadTools. Con oráculo: los rectángulos de los gadgets, los mismos que en 3.x.
4. **Qt con su tema** (qt6ct o Kvantum generados) y más plantillas de terminales.
5. **Excepciones en la interfaz**: la casilla "Aspecto original" en las propiedades de cada programa.
6. **ReAction y MUI nativos** sobre el kit (S11), que ya salen con el aspecto de DashOS.
7. **Letras** (C3): tamaño y letra elegibles, y la versión 4 del protocolo; una letra de DashOS en mapa de bits para
   la caja Amiga, por programa.

## Preguntas para el usuario

1. **Lo de serie**: ¿todo "como DashOS" (recomendado: es lo que pide un entorno homogéneo), o la caja Amiga "original"
   de serie hasta que el pintor Material de GadTools esté probado con muchos programas?
2. **La paleta del fondo de serie** (recomendado, como DMS), o el violeta de referencia hasta que el usuario elija.
3. **El modo oscuro con "original"**: ¿los programas con su aspecto original siguen el modo claro u oscuro del sistema
   (recomendado, como el `color-scheme` del portal: Adwaita oscuro) o se quedan siempre en el suyo?
4. **Bordes Amiga con "como DashOS"**: ¿barra de Intuition pintada como Material, con el mismo tamaño, y la sombra y
   las esquinas del compositor (recomendado), o la barra del compositor encima y la de Intuition quitada (rompe los
   programas que cuentan con `BorderTop`)?
5. **La letra en la caja Amiga**: ¿topaz de serie y Roboto solo por programa (recomendado), o Roboto para todos?
6. **Excepciones**: ¿bastan por programa (recomendado), o también por ventana o por pantalla Amiga?
