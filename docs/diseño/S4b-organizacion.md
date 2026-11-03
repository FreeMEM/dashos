# S4b. Organización a lo Amiga

## El problema

El usuario ha decidido que DashOS se organiza como AmigaOS: volúmenes, cajones, asignaciones, `C:`, `S:`, `L:`,
`LIBS:`, `DEVS:`, `FONTS:`, `LOCALE:`, `ENV:`, `ENVARC:`, Prefs, `DEVS:Monitors`, dispositivos, catálogos e iconos.
También ha decidido que habrá usuarios, grupos y permisos como en Linux.

AmigaOS nunca tuvo varios usuarios. Todo era de todos: `ENVARC:` era uno solo, `LIBS:` también, y cada instalador
añadía líneas a `S:User-Startup`. Hay que conservar la forma del Amiga y meter dentro varios usuarios, paquetes que se
actualizan solos (S8) y programas de Linux, que esperan `/usr`, `/etc` y `/home`.

## La idea central: cada proceso tiene su vista de los nombres

En Plan 9, cada proceso tiene su propio espacio de nombres. Con `bind` se monta un directorio encima de otro, y se
pueden juntar varios en un directorio unión. Al buscar un fichero se recorren en orden; al crear uno, va al primero
que lo admite.

Las asignaciones del Amiga ya son casi eso. Una multiasignación (`Assign LIBS: X: ADD`) junta varios cajones bajo un
nombre, y se busca en orden. La diferencia es que en el Amiga las asignaciones son de todo el sistema, y en Plan 9 son
de cada proceso.

**DashOS toma las asignaciones del Amiga, con la semántica de Plan 9: cada proceso hereda las asignaciones de quien
lo lanza.** En un micronúcleo sale natural: el espacio de nombres es una tabla del proceso, y el lanzador decide qué
le pasa (S1).

Así, `LIBS:` o `ENVARC:` pueden ser distintos para cada usuario, sin que ningún programa lo note.

## La organización

### Volúmenes

| Volumen | Qué hay | Quién escribe |
|---|---|---|
| `SYS:` | El sistema base: `C`, `Libs`, `Devs`, `L`, `S`, `Fonts`, `Locale`, `Prefs`, `System`, `Utilities`, `Tools` | Solo las actualizaciones del sistema. De solo lectura en uso normal (S5, S14) |
| `Paquetes:` | Lo instalado con el gestor de paquetes, un cajón por paquete | Solo el gestor de paquetes (S8) |
| `Usuarios:` | Un cajón por usuario | Cada usuario el suyo |
| `RAM:` | Disco en memoria, uno por sesión | El usuario de la sesión |
| `Work:`, `DH1:`, ... | Discos de datos, como en el Amiga | Según sus permisos |

El nombre `Paquetes:` es provisional. En el Amiga los programas solían ir a `Work:` o a un cajón suelto.

### Asignaciones del sistema, por capas

Cada asignación de sistema es una multiasignación con tres capas. Se busca de arriba abajo. Lo que se escribe va a la
primera capa en la que el usuario tiene permiso.

| Asignación | Capa del usuario | Capa de paquetes | Capa del sistema |
|---|---|---|---|
| `C:` | `Home:C` | `Paquetes:*/C` | `SYS:C` |
| `LIBS:` | `Home:Libs` | `Paquetes:*/Libs` | `SYS:Libs` |
| `DEVS:` | — | `Paquetes:*/Devs` | `SYS:Devs` |
| `FONTS:` | `Home:Fonts` | `Paquetes:*/Fonts` | `SYS:Fonts` |
| `LOCALE:` | — | `Paquetes:*/Locale` | `SYS:Locale` |
| `ENVARC:` | `Home:Prefs/Env-Archive` | — | `SYS:Prefs/Env-Archive` (valores de fábrica) |
| `S:` | `Home:S` | — | `SYS:S` |

Otras asignaciones:

- **`Home:`**: el cajón del usuario, `Usuarios:<nombre>`. Es el `HOME` de Linux.
- **`ENV:`**: las preferencias en uso, en `RAM:Env`. Al empezar la sesión se copia de `ENVARC:`, como en el Amiga.
- **`T:`**: temporales, en `RAM:T`.
- **`PROGDIR:`**: el cajón del programa que corre, como en el Amiga.

Un paquete nunca edita `S:User-Startup`. Declara en sus datos qué asignaciones necesita, por ejemplo `MUI:`, y el
gestor de paquetes las crea y las quita al desinstalar (S8).

### Prefs

- Los editores de preferencias están en `SYS:Prefs`, como en el Amiga. Guardan en `ENVARC:` y usan en `ENV:`.
- **Los servicios vigilan `ENV:`** con avisos de cambio, como el `StartNotify` del Amiga, y aplican el cambio al
  momento. No hay que reiniciar nada.
- **Formatos**: las preferencias que también leen los programas 68k (idioma, fuentes, modo de pantalla, teclado,
  puntero) siguen en el formato IFF del Amiga, para que la caja las vea igual. Las que solo usa DashOS van en texto,
  en el mismo árbol de `ENVARC:`.
- Las preferencias de fábrica viven en `SYS:Prefs/Env-Archive`, y nunca se tocan. "Volver a los valores de fábrica"
  es borrar la capa del usuario.

### `DEVS:`

- **`DEVS:Monitors`**: un fichero por salida de vídeo, con su icono y sus tooltypes, como los de Picasso96. En la Pi 4
  hay dos: `HDMI-0` y `HDMI-1`. El controlador es un servicio de DashOS (S12); el fichero de monitor dice qué
  controlador usa y con qué ajustes. El modo de pantalla se elige en `Prefs/ScreenMode` (S9, S10).
- **`DEVS:DOSDrivers`**: listas de montaje, como en el Amiga. Cada una describe un volumen y qué sistema de archivos
  lo sirve (S5).
- **Dispositivos de exec** (`serial.device`, `timer.device`, `input.device`, `audio.device`...): para la caja son
  ficheros en `DEVS:`, como siempre. Por debajo, cada uno habla con un servicio de DashOS (S12).
- **`DEVS:DataTypes`**: se conservan los DataTypes del Amiga. Las clases nativas de DashOS (PNG, JPEG, WebP, SVG,
  FLAC, Opus...) sirven a los programas nativos y a los 68k a la vez.
- **`DEVS:Keymaps`** y **`DEVS:Printers`**, como en el Amiga.

### `LOCALE:`

- `LOCALE:Catalogs/<idioma>/<programa>.catalog`, `LOCALE:Languages`, `LOCALE:Countries` y `LOCALE:Help`, como en el
  Amiga.
- **Los catálogos del Amiga son de 8 bits (Latin-1).** DashOS usa UTF-8. Los catálogos nativos llevan UTF-8, y la
  caja se los da en Latin-1 a los programas 68k cuando hace falta.
- Dash ya genera catálogos (`tools/catalogo.py` de Dash), así que los programas en Dash funcionan igual en el Amiga y
  en DashOS.
- Para los programas de Linux, el idioma de `Prefs/Locale` se traduce a `LANG` y compañía.

### Iconos y cajones

- Un cajón es un directorio. Workbench lo muestra con su icono `.info`, como en el Amiga.
- **Iconos `.info`** con imagen PNG dentro, como los de AmigaOS 3.5 en adelante. Los tooltypes siguen sirviendo para
  configurar programas.
- "Mostrar todos los ficheros" enseña los que no tienen icono, como en el Amiga.

### Rutas

- **La forma nativa es la del Amiga**: `SYS:Prefs/Env-Archive`. Los dos puntos marcan el volumen o la asignación, y
  una barra al principio sube un cajón (`/` es "el de arriba", no la raíz).
- **Los programas de Linux** ven una vista POSIX, montada por la capa de S7:
  - `/home/<usuario>` es `Home:`.
  - `/tmp` es `T:`.
  - `/usr`, `/etc`, `/lib` son los del entorno Linux que decida S7.
  - Los volúmenes de DashOS aparecen en `/Volumes/<nombre>`, como en macOS, para poder abrir ficheros de cualquier
    sitio.
- **Mayúsculas y minúsculas**: en el Amiga, `LIBS:` y `libs:` son lo mismo, pero el nombre se guarda tal como se
  escribió. Linux distingue unas de otras. macOS demuestra que casi todo el software de Unix funciona sin
  distinguirlas. Recomendación: sin distinguir, como el Amiga, en todo el sistema; y volúmenes que sí distinguen
  para quien lo necesite, por ejemplo para trabajar con repositorios git (S5).

### Usuarios y permisos encima de todo esto

- Cada fichero tiene dueño, grupo y permisos de lectura, escritura y ejecución, como en Linux (decisión del usuario).
  El sistema de archivos los guarda (S5). Los bits de protección del Amiga (`RWED`, `S`, `P`, `A`) se derivan de
  ellos para la caja.
- `SYS:` es del administrador. `Usuarios:<nombre>` es de ese usuario.
- **Instalar paquetes del sistema** necesita permiso de administrador. **Instalar en `Home:`** no lo necesita: cada
  usuario puede tener sus programas en su capa (S8).
- El gestor de arranque monta el espacio de nombres de cada sesión con las capas de ese usuario (S14).

## Lo que queda abierto

1. ¿Nombre del volumen de paquetes? Propuestas: `Paquetes:`, `Programas:` o `Apps:`.
2. ¿Sin distinguir mayúsculas en todo el sistema, como el Amiga y macOS, con volúmenes que sí distinguen para quien
   los pida? Recomendado: sí.
3. ¿Preferencias compartidas con la caja en IFF del Amiga, y las solo de DashOS en texto? Recomendado: sí.

## Cómo encaja

- Con S1: el espacio de nombres por proceso lo da el lanzador, sin nada nuevo en el kernel.
- Con S4: la caja ve estas mismas asignaciones, con las capas de su usuario.
- S5 guarda dueños y permisos, decide cómo se ignoran mayúsculas y monta `SYS:` de solo lectura.
- S6 detalla usuarios, grupos y administrador.
- S7 monta la vista POSIX para los programas de Linux.
- S8 instala en `Paquetes:` y en `Home:`, y crea las asignaciones que declara cada paquete.
- S9 y S10 usan `DEVS:Monitors` y `Prefs/ScreenMode`.
- S12 pone un servicio detrás de cada dispositivo de `DEVS:`.
- S14 monta el espacio de nombres al abrir la sesión.

## Fuentes

- Asignaciones del Amiga (`ADD`, `PATH`, `DEFER`): [documentación de AROS](https://aros.sourceforge.io/documentation/users/shell/assign.php)
  y [la guía de AssignManager](https://discmaster.textfiles.com/file/934/LIGHT-ROM%201%20(Amiga%20Library%20Services)(1994).iso/ffdisks/d913.lha/AssignManager/AssignManager.guide/AssignManager.pdf).
  Es documentación de usuario, no código de AROS.
- Espacios de nombres de Plan 9: [bind(2)](https://man.aiju.de/2/bind) y
  [bind(1)](https://src.rcs.uwaterloo.ca/xref/plan9/sys/man/1/bind).

Lo que no se comprobó: cuánto software de Linux falla sin distinguir mayúsculas. Lo de macOS es experiencia conocida,
no una medida.
