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
