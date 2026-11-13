# Bitácora: el aspecto de los programas de las cajas (S26)

Rama `aspecto-cajas`, sacada de `revision` (d819301). Estudio en `docs/diseño/S26-aspecto-de-las-cajas.md`.

Petición del usuario (2026-11-13): "Hay que ir trabajando en tener el entorno visualmente homogéneo: que sea
configurable que las cajas Amiga, Linux, etc. muestren sus widgets con el estilo de DashOS (Material Design) o que
mantengan su estilo original (MUI, GadTools, ReAction... y en Linux el suyo)." Después, decisión suya: "aprende de cómo
se hace en DMS (DankMaterialShell sobre Niri) y lo aplicamos" (color dinámico sacado del fondo, repartido a todo).

Sala limpia: Material 3, freedesktop, documentación de GTK y Qt. Nada de código de DMS, matugen, GNOME, KDE, Adwaita,
temas de terceros, UAE, AROS ni MUI. Las letras, las del kit (Roboto y Roboto Mono, OFL).

## 2026-11-13: tema único, ajuste de las cajas, reparto y agente de Linux

### Lo hecho

- **`wayland/src/aspecto.rs`**: el ajuste "Aspecto de los programas de las cajas" (general, por caja y por programa;
  "dashos" u "original"), su texto (el de `Prefs/cajas` y el del protocolo), quién gana, y una lectura que rechaza todo
  lo que no vale (nombres con `/`, `:` o controles, líneas repetidas, cajas que no existen, más de 32 programas o de
  2048 bytes). Pruebas con basura al azar: nunca un pánico y lo que vale se escribe y se relee igual.
- **`dashos_tema_v1` versión 3**: petición y evento `cajas`. Solo el centro lo cambia; un texto que no vale es un error
  de protocolo; se avisa a los suscritos de la versión 3 solo si cambia; al enlazar llega tras `tema`. El compositor lo
  anuncia en la versión 3 (los de la 1 y la 2 siguen igual).
- **El centro de control hace de servicio del tema**: al arrancar pone en el compositor lo guardado (`Prefs/apariencia`
  y `Prefs/cajas`); la de serie es ahora la **paleta del fondo** (claro), como en Material You y DMS: pide la semilla al
  fondo de escritorio y calcula los esquemas (una vez; si estaban guardados para esa semilla, no). Sin fondo, el violeta
  de referencia. En Apariencia: "Color fijo violeta/azul/verde/naranja", "Paleta del fondo" y las dos filas nuevas,
  "Programas de las cajas: como DashOS" y "... originales", que se guardan en `Prefs/cajas` y van al compositor.
- **La caja Amiga** (`caja/pegamento/src/tema.rs` y `compositor.rs`): enlaza el tema (versión 3), recuerda los
  esquemas que llegan hechos, guarda color, modo y ajuste y, si los programas Amiga van como DashOS, saca las 12 plumas
  de la `DrawInfo` de los papeles de Material 3. Quedan en `CompositorWayland::tema.plumas` para el pintor de GadTools
  (rama `caja-amiga-gadtools`); aún no pinta con ellas. Lo dice por la consola.
- **La caja Linux** (`caja-linux/aspecto`, `dashos-aspecto`): un programa de Wayland más dentro de Arch, por el puente.
  Enlaza el tema y, cada vez que cambia, escribe el tema de GTK "DashOS" (GTK 3 y 4, claro y oscuro, hecho aquí: importa
  el de GTK por su recurso y pone encima las reglas de Material con `:not(#dashos)` para ganar a sus estados, porque GTK
  no tiene `!important`), `settings.ini`, GSettings (`keyfile`), el CSS del usuario de GTK 4 para libadwaita, foot y,
  para Qt, `/run/dashos-aspecto/linux` (que mira `/etc/profile.d/dashos-aspecto.sh` para poner
  `QT_QPA_PLATFORMTHEME=gtk3`). Las excepciones de programas de Linux, como envoltorios en `/run/dashos-aspecto/bin`.
  Con "original", Adwaita tal cual y sin los ficheros suyos (los del usuario no se tocan).
- **Imagen y prueba**: `tools/aspecto_caja_linux.py preparar` copia (con reflink) la imagen de Arch con GTK de
  `caja-linux-apps` a su caché (`~/.cache/dashos-aspecto-cajas`) y le mete con `debugfs` el agente, el puente de esta
  rama, `muestra-gtk3` y `muestra-gtk4` (`caja-linux/invitado/muestra_gtk.c`), el servicio de systemd, el perfil y las
  letras. `make arranque-virt-aspecto` arranca DashOS con los Prefs de `ASPECTO_PREFS` (`tools/disco_archivos.py`
  acepta `DASHOS_PREFS`) y el guion (modo `aspecto`) abre las muestras y foot y captura la pantalla.
- **Hostil en DashOS** (`hostil`): cambiar el aspecto de las cajas sin ser el centro, con "todo original" y con un nombre
  con `../`: error de protocolo las dos veces; por otra conexión sigue "general dashos".

### Resultado

`make arranque-virt-aspecto` en Full HD (`DASHOS_PANTALLA=1920x1080`), tres arranques con distintos Prefs:

```
[centro] al arrancar: tema claro, color Del fondo (3d6f88), programas de las cajas como DashOS; en el compositor: sí; guardado: 0
[caja-amiga] tema de DashOS: programas Amiga como DashOS, claro, color 3d6f88; plumas: fondo f6fafe, texto 171c1f, relleno 006688, claro c0c7cd, oscuro 71787d; 0 excepciones
[caja-linux] linux: aspecto: programas de Linux como DashOS, claro, acento 3d6f88 (superficie f6fafe, primario 006688); 0 excepciones; GTK 3 y 4, foot y Qt al día
[caja-linux] linux: muestra-gtk: GTK 4 en pantalla, tema DashOS-1
```

- **ondas.png, como DashOS**: GTK 4 con la paleta azul verdosa que sale del fondo (botones redondos, interruptor,
  casillas, deslizador, pestaña y fila elegida de Material) y foot con sus colores y Roboto Mono.
- **atardecer.png, como DashOS**: lo mismo con la paleta granate del atardecer (semilla c44e68).
- **ondas.png, original**: GTK 4 con Adwaita tal cual ("tema Adwaita").

Capturas (fuera de git) en `capturas-agente/`: `aspecto-{dashos-ondas,dashos-atardecer,original-ondas}-{gtk4,foot}.png`
y las tres juntas en `aspecto-comparacion.png`. En el PC, con el compositor de DashOS (`compositor-pc`):
`aspecto-pc-gtk-dashos.png` y `aspecto-pc-gtk-original.png` (GTK 3 y GTK 4), `aspecto-pc-qt.png` (Qt 6 con el tema
de la plataforma gtk3 y sin él) y `aspecto-pc-gtk3-cambio-en-vivo.png` (GTK 3 abierto en claro y azul, pasado a oscuro
y naranja con la ventana abierta).

### Lo que hubo que arreglar por el camino

- **GTK no tiene `!important`** y el tema de debajo gana por especificidad (sus estados y `:backdrop`): con la ventana
  sin el teclado, todo volvía a Adwaita. Cada selector lleva `:not(#dashos)`, que pesa como un identificador.
- **De dónde lee GTK sus ajustes en Wayland sin portal**: GTK 3, de GSettings (por encima de `settings.ini`); GTK 4, de
  `settings.ini`. Con `GSETTINGS_BACKEND=keyfile`, GSettings es un fichero que GLib vigila: GTK 3 cambia en el acto si
  cambia el nombre del tema (por eso "DashOS-1" y "DashOS-2", uno cada vez, con el modo de ahora en los dos ficheros).
  GTK 4 no mira `settings.ini` otra vez: hace falta el portal.
- **El adiós del centro al compositor se perdía al arrancar** (la cola llena): la conexión vieja seguía y la ventana de
  Ajustes, después, recibía "número de objeto nuevo malo". Ahora se reintenta hasta 5 s (`adios_al_compositor`).
- **GTK 3 aborta en la caja** al cargar su primer icono SVG: gdk-pixbuf 2.44 carga con glycin, y glycin, sin bwrap que
  funcione en la caja, no carga (con el bwrap que dice "no va" de `caja-linux-apps`, "Loader process exited early"; con
  uno que no aísla nada, el cargador acaba sin decir nada). GTK 4 abre: pinta sus iconos sin glycin. Es lo mismo que mira
  `caja-linux-apps` (qué le falta al kernel de la caja para bwrap). GTK 3 queda probado en el PC; en DashOS, cuando
  se arregle eso. El bwrap que no aísla nada no se queda.
- **El guion**: las ventanas nuevas salen sobre la terminal, pero la de la caja Amiga puede salir encima en cualquier
  momento: un clic en la barra de la ventana de Linux antes de capturar. Las líneas de las muestras pueden salir pegadas
  al prompt de la terminal.

### Medidas

QEMU `virt`, TCG, con el PC cargado (11 QEMU de otras sesiones a la vez).

| Qué | Valor |
|---|---|
| El centro, al arrancar, con la paleta del fondo sin guardar: semilla del fondo y los dos esquemas | 141 a 186 ms (con el fondo ya en pantalla) |
| De que el agente de Arch recibe el tema a que ha escrito todo (GTK 3 y 4, GSettings, foot) | Por debajo del segundo (no medido aparte) |
| GTK 3 en el PC, del cambio de tema a la ventana repintada | Menos de los 5 s de la prueba (no medido aparte) |

### Pruebas

- `cargo test` en `wayland` (71: `aspecto::pruebas` con basura al azar, y `aspecto_de_las_cajas` con sus hostiles) y en
  `caja-linux/aspecto` (3: CSS con los colores del tema y llaves equilibradas, ajustes, GSettings, foot y envoltorios
  con nombres hostiles).
- `make prueba`: BIEN: esperado.txt y BIEN: cargador hunk.
- `make arranque-virt`: las cinco BIEN (con dos líneas nuevas: el tema al arrancar y las plumas de la caja Amiga; y la
  del `hostil`: cambiar el aspecto de las cajas sin ser el centro da error y sigue como DashOS). La primera vez falló por
  el adiós perdido (arriba); después, por el tamaño de la apariencia sin calcular en las líneas esperadas.
- `-vm`, `-linux`, `-2`, `-diag`, `-escritorio`, `-usb`: BIEN. `-arch`, `-wayland` y `-gpu` (con la caché
  `dashos-caja-linux-fusion`): BIEN.
- `make arranque-virt-aspecto` (nueva, con `tools/aspecto_caja_linux.py preparar`): BIEN en los tres casos.

### Qué falta

- El portal de ajustes en el agente (GTK 4, libadwaita, Qt 6, Firefox y Electron en el acto).
- GTK 3 en la caja (glycin y bwrap, con `caja-linux-apps`).
- El pintor Material de GadTools y de los bordes de intuition, tras fusionar `caja-amiga-gadtools`.
- Las excepciones en la interfaz (hoy, a mano en `Prefs/cajas`).
- Llevar el agente, su servicio y las letras a `preparar-arch` cuando se fusione `caja-linux-apps` (hoy los mete
  `tools/aspecto_caja_linux.py` en una copia de su imagen).
- Qt con un tema suyo (qt6ct o Kvantum); Qt en la imagen de la caja para probarlo allí (hoy, solo en el PC).
