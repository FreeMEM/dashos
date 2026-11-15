# Asignaciones con nombre en lo nativo (paso 2)

Rama `asignaciones`, desde `revision` (eace2e1). Encargo: el paso 2 de `docs/REVISION_LOOP.md`. Que los programas
nativos vean volúmenes y asignaciones con nombre (`SYS:`, `Home:`, `RAM:`, `Work:`, `C:`, `LIBS:`, `ENV:`,
`ENVARC:`, `PROGDIR:`…), por proceso, heredadas del que lanza y en capas. Y que el disco de pruebas tenga la forma de
`SYS:` de S4b.

Diseño de partida: S4b ("La idea central", "Asignaciones del sistema, por capas", "Cada usuario, su mundo") y S6b
("Capa 2": "los nombres son cosa del cliente: al nacer recibe una tabla de nombre a raíz y subcamino, y la biblioteca
resuelve"). La caja Amiga ya tiene su dos con lo mismo (`caja/dos/src/espacio.rs`): de ahí sale la semántica, no el
código.

## Diseño

### Dónde vive el espacio de nombres

En tres sitios, cada uno con su papel:

- **`sesiones` guarda la tabla de cada proceso** que conoce (su ficha). Es la copia que manda: de ella sale la del
  hijo al lanzar. Sin código nuevo en el kernel, como pide S4b ("el espacio de nombres por proceso lo da el
  lanzador").
- **El programa la pide** (orden nueva `ASIGNACIONES`, por páginas) y la resuelve él, con la biblioteca
  (`dashos::asignaciones`). Resolver no cuesta viajes a `sesiones`: solo al servicio de archivos.
- **El servicio de archivos no sabe de nombres.** Sigue viendo asas y raíces (huecos 0 a 15). Es quien decide qué se
  alcanza. La tabla de nombres solo traduce: no abre nada.

### Qué hay en la tabla

Texto, una línea por nombre, como el `Assign LIST` del Amiga:

```
*SYS=@1
*Home=@2
*RAM=@3
*Work=@4
PROGDIR=@0
C=Home:C,SYS:C
ENV=RAM:Env
```

- **Volumen** (`*` delante): un nombre que lleva a una raíz del servicio de archivos (`@n`, el hueco n) y, si hace
  falta, un subcamino (`@1/Programas`). Los pone solo `sesiones`, según las raíces que da a ese proceso.
- **Asignación**: un nombre que lleva a una o más rutas con nombre (`Home:C,SYS:C`), que se prueban en orden.
  `PROGDIR` es la única asignación con raíz directa (`@0`), y la pone `sesiones`.
- Nombres de hasta 30 bytes, sin `:`, `/`, `=`, `,`, `*`, `@`, espacios ni bytes de control. No distinguen mayúsculas
  (`libs:` es `LIBS:`), como en el Amiga, pero se guardan como se escribieron.
- La tabla entera, hasta 1.024 bytes; una asignación, hasta 6 capas.

### Cómo se resuelve `X:camino`

1. Se parte por el primer `:`. Si hay otro `:` en el resto, no vale. Tampoco un resto que empiece por `/` (en el
   Amiga es "subir"), con `..` o `.`, ni con partes vacías (`a//b`): no se sube por encima de un nombre.
2. Se busca `X` en la tabla. Un volumen da un sitio: su raíz y su subcamino, con el resto detrás.
3. Una asignación da los sitios de cada capa, en orden: cada capa es otra ruta con nombre, y se resuelve igual, con el
   resto detrás. Así `ENV:` lleva a `RAM:Env`, y `RAM:` a la raíz 3.
4. Una asignación que lleva a otra que lleva a la primera es un bucle: a los 8 niveles, error (`BUCLE`), como el
   `ERROR_TOO_MANY_LEVELS` de dos.
5. Como mucho 8 sitios por ruta.

Con la lista de sitios:

- **Abrir o mirar**: el primero que exista. Se pasa al siguiente si el servicio dice "no existe" o "asa mala" (una
  raíz que el proceso no tiene, o una capa que desapareció); cualquier otro error (protegido, por ejemplo) para la
  búsqueda y se devuelve, como en dos.
- **Listar**: la unión de todas las capas; un nombre que sale en dos capas vale el de la primera (la del usuario tapa
  a la del sistema).
- **Crear**: en la primera capa en la que se pueda. Se pasa a la siguiente si falta el cajón padre, si no hay permiso
  (protegido, solo lectura) o si la raíz no está. Si ya existe en una capa, se para ahí (`YA_EXISTE`): no se crea
  otro debajo de uno que se ve.

### Cómo se hereda al lanzar

- Lo que lanza `sesiones` (`LANZAR`) nace con **las asignaciones del que lo lanza**, si es del mismo usuario: lo que
  ese programa cambió con `ASIGNAR` lo ve su hijo. Si lo lanza otro (`guardian` lanza como Ana), nace con las de la
  sesión de Ana.
- Los **volúmenes** no se heredan: los pone `sesiones` según las raíces que da al hijo, que pueden ser otras que las
  del padre. Una asignación heredada que lleva a un volumen que el hijo no tiene, sencillamente no resuelve.
- `PROGDIR` es siempre el del propio programa (su raíz 0, el cajón de donde salió), o no está si no salió del disco.

### Lo que monta `sesiones` al entrar

Para un proceso de una persona (usuario con cuenta), las asignaciones del sistema de S4b con su capa encima:

| Nombre | Capas |
|---|---|
| `C:` | `Home:C`, `SYS:C` |
| `LIBS:` | `Home:Libs`, `SYS:Libs` |
| `DEVS:` | `SYS:Devs` |
| `FONTS:` | `Home:Fonts`, `SYS:Fonts` |
| `LOCALE:` | `SYS:Locale` |
| `S:` | `Home:S`, `SYS:S` |
| `ENVARC:` | `Home:Prefs/Env-Archive`, `SYS:Prefs/Env-Archive` |
| `CLASSES:` | `Home:Classes`, `SYS:Classes` |
| `REXX:` | `Home:Rexx`, `SYS:Rexx` |
| `L:` | `SYS:L` |
| `KEYMAPS:`, `PRINTERS:` | `SYS:Devs/Keymaps`, `SYS:Devs/Printers` |
| `HELP:` | `SYS:Locale/Help` |
| `ENV:`, `T:` | `RAM:Env`, `RAM:T` |
| `Programas:` | `SYS:Programas` (el `Paquetes:` de S4b, provisional) |

La capa de paquetes (`Paquetes:*/C`…) no está: espera a S8, que dirá qué paquetes hay.

### Las llaves y las raíces de S6b

- **Un nombre no da nada.** Lo que se alcanza lo deciden las raíces que el proceso tiene en el servicio de archivos;
  la tabla solo dice "`SYS:` es la raíz 1". Un programa que se escriba su propia tabla con `SYS=@1` sin tener la raíz
  1 recibe "asa mala" del servicio.
- **`ASIGNAR` no pone raíces.** Un programa puede cambiar sus asignaciones (y las de los hijos que lance), pero sus
  capas son siempre rutas con nombre (`Home:Trabajo`), nunca `@n`: los nombres con raíz directa (volúmenes y
  `PROGDIR`) los pone solo `sesiones`, según lo que da. Así un programa encerrado no llega más lejos por tener un
  nombre.
- **No se tapan volúmenes**: `ASIGNAR` no acepta el nombre de un volumen ni `PROGDIR` (dos hace lo mismo). Si no, un
  programa podría hacer que `SYS:Libs` de sus hijos fuera un cajón suyo.
- Un perfil encerrado (aplicación, suelto) no tiene la capa del usuario en `C:`, `LIBS:`, `FONTS:` ni `CLASSES:`
  (S4b, S6b regla 6): `sesiones` la quita al montarle la tabla. Hoy no hay aún perfiles en las fichas (todo lo que
  lanza `sesiones` es como el Shell); la función está y tiene su prueba.

### Lo provisional

- **`RAM:` es un cajón del disco**, `Estado/RAM/<cuenta>`, no memoria: DashOS aún no tiene disco en memoria. Lo de
  una sesión no lo ve otra (cajón 0700 de su dueño), pero no se borra al salir. Pendiente: un volumen en memoria en
  el servicio de archivos.
- **`Work:` es el cajón `Work` del disco principal**, no un disco aparte. Igual que `Estado:` y `Programas:`.
- **`Home:` del escritorio** (Workbench, explorador) sigue siendo `SYS:C`, como hasta ahora; el de lo que lanza
  `sesiones` es el volumen cifrado del usuario. La caja Amiga lee sus ajustes de `SYS:C/Prefs` (`caja/pegamento`),
  así que el contenido provisional de `C` se queda hasta que la caja los lea por `ENVARC:`.

## Decisiones pendientes del usuario

1. **Volúmenes sobre asignaciones**: una asignación no puede llamarse como un volumen. Es lo que hace dos; la otra
   opción (que la asignación tape al volumen, como un `bind` de Plan 9) es más flexible y menos segura.
2. **Los hijos heredan las asignaciones cambiadas del padre** (Plan 9) y no las de la sesión. Un programa que se
   cambie `LIBS:` se lo cambia también a lo que lance. Es lo que dice S4b; se apunta porque cambia lo que ve un
   programa lanzado desde otro.
3. **`RAM:` en disco** hasta que haya un volumen en memoria (arriba).

## Bitácora

### Paso 2: la biblioteca (`dashos::asignaciones`)

`usuario/src/asignaciones.rs`: la tabla (`Tabla`, texto de hasta 1 KB, sin montón: la usa también `sesiones`),
resolver (`Tabla::resolver` da los sitios en orden), cambiar (`Tabla::asignar`: poner, añadir detrás, delante, quitar),
heredar (`Tabla::heredar`: volúmenes del hijo, asignaciones del padre), las de la sesión (`de_sesion`, con perfil
encerrado), y sobre el servicio de archivos (rasgo `Fs`): `abrir`, `mirar`, `crear`, `guardar` y `listar` (unión de
capas). Con `sesiones`: `pedir_tabla` y `asignar_en_sesiones` (órdenes 30 y 31, aún sin servidor en este paso). Lo de
antes (`NOMBRES`, raíces numeradas) sigue igual.

Pruebas en el PC (`rustc --test src/asignaciones.rs`, en `make prueba`), 11, con un servicio falso: resolver en capas,
rutas hostiles (`:` de más, `/` delante, `..`, `.`, partes vacías, bytes de control, nombres y caminos larguísimos),
bucles (en la tabla que llega, al asignar y a 8 niveles), capas que faltan o desaparecen (la raíz se va), asignar que
no tapa volúmenes ni `PROGDIR` ni pone raíces, tablas mal formadas, heredar, encerrado, crear en la primera capa que
deja (y no en `SYS:`), listar sin repetidos, y un programa encerrado con una tabla falsa que no llega a nada.
`make prueba` bien; `make arranque-virt` bien (sin el fallo suelto del explorador esta vez).

### Paso 3: `sesiones` monta el espacio de nombres y lo heredan los hijos

- `sesiones` guarda las asignaciones de cada ficha (`[Tabla; 64]`, en la pila: 512 KB para `sesiones` en `build.rs`).
  Un alta de `inicio` de una persona con cuenta nace con las de la sesión (`de_sesion`); lo que lanza `LANZAR` hereda
  las del que lo lanza si es de su usuario, o las de la sesión si no (`guardian` lanza como Ana).
- `ASIGNACIONES` (30) da la tabla entera por páginas: volúmenes según sus raíces (lo lanzado por `sesiones` y el
  Workbench y el explorador, que se ceden las mismas), `PROGDIR:` si salió del disco, y sus asignaciones.
  `ASIGNAR` (31) cambia la tabla entera y guarda lo que no son volúmenes; queda en el registro de la consola.
- Lo que lanza nace con dos raíces más: 3 `Estado/RAM/<cuenta>` (`RAM:`) y 4 `Work` (`Work:`), las dos de leer y
  escribir (`RAIZ_*` y `CAMINO_*` en el protocolo).
- Disco de pruebas: `Estado/RAM/Ana` y `Estado/RAM/Carla` (0700, cada una de su dueña, con `T` y `Env`) y `Work`.

Pruebas en QEMU (`make arranque-virt`):

- `cambio`, lanzado por `guardian` como Ana: tabla con `SYS: Home: RAM: Work:` y `PROGDIR:`; `C:grande.txt` se lee de
  `SYS:C` (capa 1); crear en `C:` sin `Home:C` da -12 (no escribe en `SYS:`); con `Home:C` hecho, crea ahí (capa 0);
  listar `C:` une las dos capas sin repetir; `T:cambio.tmp` va a `RAM:T` (y se borra); `ASIGNAR Trabajo=Home:C` vale y
  la tabla nueva la trae; `SYS=Home:` -12, `Casa=@2` -1, un bucle -30.
- `hostil` (Carla, lanzado por `inicio` solo con su cajón): su tabla trae asignaciones y ningún volumen, y por ella no
  llega a nada; con una tabla escrita a mano (`SYS=@9`, `Home=@15`) tampoco; `ASIGNAR` rechaza tapar volúmenes (también
  uno que no tiene, `Work`), `PROGDIR`, raíces (`@1`, `@2/Libs`), caminos que suben o empiezan por `/`, `:` de más,
  nombres de 31 bytes, bucles (también uno consigo mismo al añadir), un cambio que no existe y largos que no caben en
  el mensaje; y `sesiones` sigue contestando.

Cambio en lo esperado: la línea de `ficheros` con la raíz del disco cuenta 8 entradas (con `Work/`).

`make prueba` bien; `make arranque-virt` bien (la primera vuelta falló por esa línea de `ficheros` y por dos líneas de
`Estado/RAM/Carla` que faltaban en lo esperado del disco; arreglado, la comprobación del disco repetida sobre el mismo
disco, bien); `make arranque-virt-2` y `make arranque-virt-linux` bien.

### Paso 4: el disco con la forma de `SYS:`, y los volúmenes por su nombre

- **Disco de pruebas** (y el de usar a mano, que es el mismo, `tools/qemu_a_mano.sh`): la raíz es `SYS:` y tiene los
  cajones del árbol de S4b: `Classes/{DataTypes,Gadgets,Images,MUI}`, `Devs/{DataTypes,DOSDrivers,Keymaps,Monitors,
  Printers}`, `Fonts`, `L`, `Libs`, `Locale/{Catalogs,Countries,Help,Languages}`, `Prefs/{Env-Archive/Sys,Presets}`,
  `Rexx`, `S` (con `Startup-Sequence`, `Shell-Startup` y `User-Startup` vacío), `Storage/{DOSDrivers,Keymaps,Monitors,
  Printers}`, `System`, `Tools/Commodities`, `Utilities`, `WBStartup`. Vacíos donde no hay nada. `C` ya estaba y sigue
  guardando lo del Home provisional del escritorio. Sin `.info`: el formato de iconos con PNG está por hacer.
- **Workbench**: los iconos del escritorio son los volúmenes de su tabla de nombres (`SYS`, `Home` con la sesión de
  Ana abierta, `RAM`, `Work`), con su nombre; se cede las raíces 3 y 4 (`RAM:` y `Work:`). Los caminos, como en el
  Amiga: `SYS:Programas`.
- **Explorador**: la barra lateral lleva `SYS:`, `RAM:` y `Work:` (y la carpeta personal, la papelera y la caja Amiga,
  que pasa a la raíz 5); los caminos y las migas, como en el Amiga (`Home:Privado/nota.txt`); la línea de cada ventana,
  "ventana 1 en SYS:Programas, 3 elementos…".

Cambios en las pruebas (todos en este commit):

- `tools/prueba_uefi.sh`: los textos del explorador con el camino nuevo; el icono del Workbench se busca como `SYS`;
  el paso 4 del explorador va a `SYS:Programas` escribiendo el camino (Ctrl+L), porque con todos los cajones de `SYS:`
  `Programas` ya no cabe a la vista en la rejilla; escribir `:` es Mayúsculas y punto (teclado español).
- `kernel/pruebas/arranque_virt.txt`: las mismas líneas; `ficheros` lista 21 entradas en la raíz (`ficheros.rs` tenía
  sitio para 8: ahora 32); el malware de guiones de la caja Amiga ya no da "no existe" (205) al intentar crear
  `SYS:S/Startup-Sequence` y `SYS:L/Disk-Validator`, sino "protegido" (214), porque esos cajones existen; y la guardia
  cuenta 10 negativas en vez de 8. Sigue sin conseguir nada.

`make prueba` bien; `make arranque-virt` bien a la tercera (la primera, `Programas` no cabía a la vista; la segunda,
las líneas del malware de guiones; las dos, ajustes de lo esperado, no fallos); `make arranque-virt-2` y
`make arranque-virt-linux` bien.
