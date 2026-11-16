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

- ~~`RAM:` es un cajón del disco~~. Ya no: es un volumen en memoria por sesión (abajo, "`RAM:` en memoria").
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
3. ~~`RAM:` en disco~~: hecha en memoria (abajo, "`RAM:` en memoria", con sus propias decisiones pendientes).

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

### Paso 5: el explorador, por nombres

El explorador pide su tabla de nombres al prepararse y resuelve por ella los caminos escritos con un nombre (Ctrl+L):
`LIBS:`, `ENVARC:Sys`, `T:`, `Programas:hola` llevan a la primera capa donde existen, como en el Amiga
(`App::lugar_por_nombre`, con `dashos::asignaciones::mirar`). Un nombre que no está en la tabla (`Amiga:`, o
"Home/Privado" sin dos puntos) sigue por lo de siempre. Lo que intenta salir del nombre no vale; lo que lleva a una raíz
que no tiene, tampoco; y un nombre no da nada que el servicio no deje: `SYS:DeCarla` se resuelve, pero listarlo da -12.

Pruebas en QEMU: al prepararse, "[explorador] nombres: C:grande.txt → SYS:C/grande.txt; LIBS: → SYS:Libs; ENVARC:Sys →
SYS:Prefs/Env-Archive/Sys; T: → RAM:T; Programas:hola → SYS:Programas/hola; Home:../DeCarla y SYS:/Libs no valen;
Nada:x y Amiga: no están en la tabla; SYS:DeCarla se resuelve y listarlo da -12". Y con el ratón y el teclado (el guion), la segunda
ventana va a `Home:Viaje` escribiendo el camino con su nombre, y la primera a `SYS:Programas`.

## Lo que queda por pasar a nombres

| Qué | Cómo está |
|---|---|
| Workbench por dentro | Enseña los volúmenes de su tabla, pero abre y lista por raíces numeradas |
| Barra del explorador | `SYS:`, `RAM:` y `Work:` fijos (coinciden con la tabla), no leídos de ella |
| `selector`, `centro`, `fondo`, `panel` | Por su raíz 0 y caminos fijos (`C/Prefs`, `C/Fondos`): esperan a `ENVARC:` y `Home:` de verdad |
| `LANZAR` de `sesiones` | Camino del disco (`Programas/hola`), no `C:hola` ni `Programas:hola` |
| `cambio`, `pide`, `hostil` | Siguen probando también `NOMBRES` (10), la tabla vieja de 5 nombres sin capas; quitarla cuando nada la use |
| Caja Amiga | Su propio dos; lee sus ajustes de `SYS:C/Prefs` (por eso `C` guarda aún lo del Home provisional) |

## Lo que falta del todo

- La capa de paquetes (`Paquetes:*/C`…): espera a S8.
- `Work:` como disco aparte (hoy, un cajón del disco). `RAM:` ya está en memoria.
- `Home:` del escritorio: hoy `SYS:C`; el de lo que lanza `sesiones` sí es el volumen cifrado.
- `User-Startup` y `WBStartup` por usuario. `ENV:` ya se copia de `ENVARC:` al entrar.
- Perfiles en las fichas: la tabla del perfil encerrado está hecha y probada en el PC, pero ninguna ficha es aún
  "aplicación" o "suelto".
- Los `.info` de los cajones de `SYS:` y su `Disk.info`.
- Una orden `Assign` nativa (no hay Shell nativo): hoy solo se asigna por la API.

`make prueba` bien; `make arranque-virt` bien a la tercera (las dos primeras, la prueba de `SYS:DeCarla` esperaba que
mirarlo fallara: el servicio deja abrir y mirar un cajón ajeno y niega listarlo; ahora se prueba listando).

## `RAM:` en memoria (rama `ram-volumen`, desde `revision` 41e5b81)

Encargo: la tercera decisión de asignaciones. Que `RAM:` sea un volumen en memoria de verdad, uno por sesión de usuario,
y sobre él `ENV:` (copiado de `ENVARC:` al entrar) y `T:`.

### Diseño

En tres frases: al entrar un usuario, `sesiones` crea el grupo de recursos de su sesión y, cobrado a él, un objeto de
memoria disperso (sin páginas) que entrega al servicio de archivos; `archivos` monta encima un DashFS propio cuyo
"disco" pide una página la primera vez que se escribe un bloque y la devuelve al sistema cuando DashFS lo descarta
(TRIM); al salir, `sesiones` manda quitarlo y la memoria vuelve entera.

Las piezas:

- **Kernel: memoria dispersa** (llamadas 57 y 58, `docs/referencia/kernel.md`). `memoria_dispersa(tamaño, grupo)` da
  un objeto de memoria sin ninguna página puesta, cobrado al grupo que se diga (con escribir sobre su llave, como para
  crear un proceso en él). `memoria_paginas(llave, poner o soltar, primera, cuántas)` pone páginas nuevas a cero
  (cobradas a ese grupo; se para donde no cabe en su tope) o las suelta (fuera de las proyecciones, TLB vaciadas,
  devueltas al sistema). Reutiliza lo que ya había para el globo de las máquinas (páginas soltadas de un objeto), con
  la misma condición: solo con un objeto que no tiene nadie más. Vacío cuesta su lista (8 bytes por página), cobrada
  también: un objeto de 1 GB vacío no sale gratis. Tocar un hueco es un fallo del programa.
- **`usuario/src/disco_ram.rs`**: el disco de memoria como `dashfs::Disk`, sin saber de dónde salen las páginas (un
  rasgo; en DashOS, el objeto disperso; en las pruebas del PC, memoria normal con tope). Un mapa de bits dice qué
  bloques tienen página; un bloque sin escribir se lee como ceros. Un lote se escribe entero o nada: primero se piden
  todas las páginas y, si falta una, se devuelven las recién pedidas y se da "lleno". El anillo de cabeceras de DashFS
  (256 bloques, 1 MB) no ocupa: el disco no se abre nunca por segunda vez, así que solo guarda la última, en una página.
- **DashFS**: `FileSystem::create_plain`, crear sin `std` (sin cifrar, con el UUID que da quien llama) y ya en modo
  diferido, para que lo que suelta al crearse también se descarte. Nada más: el TRIM ya lo mandaba (A14).
- **`archivos`**: su `Fs` es ahora un "mundo": el DashFS del disco y un DashFS en memoria por sesión. Los volúmenes en
  memoria llevan un bit propio en su número (`VOL_RAM | sesión`), así que asas, raíces, lo recordado y la caché de
  páginas los distinguen sin cambiar. En ellos cada transacción se cierra en el acto (`durable`): ahí DashFS descarta lo
  soltado y la memoria vuelve. Cuatro órdenes sin marca, de `sesiones` (`docs/referencia/servicios.md`): crear, quitar,
  dar la raíz a un cliente y copiar un árbol del disco. Quien crea o escribe recibe "lleno" cuando quedan menos de
  64 KB además de lo que escribe: siempre se puede borrar. Sin instantáneas ni papelera en `RAM:`.
- **`sesiones`**: al entrar (sesión nueva), `grupo_crear`, `memoria_dispersa` en ese grupo, crear el volumen, `T`,
  `Env` con `SYS:Prefs/Env-Archive` y, encima, `Home:Prefs/Env-Archive` (lo del usuario pisa lo de fábrica), y la raíz
  de `RAM:` para el escritorio (Workbench y explorador nacen antes de que entre nadie). Lo que lanza nace con la raíz 3
  en el volumen de su sesión. Al salir, quitar el volumen y matar el grupo. Si no se puede crear, el usuario entra
  igual, sin `RAM:`, y se dice.
- **Escritorio**: Workbench y explorador ya no se ceden `Estado/RAM/Ana`. El icono de `RAM:` del Workbench sale con la
  sesión de Ana abierta, como el de `Home:`. El disco de pruebas ya no tiene `Estado/RAM`.

### Por qué dentro de `archivos` y no un servicio aparte

El encargo dejaba elegir. Un servicio de archivos por sesión (otro proceso con el mismo protocolo) aísla por proceso y
cobra todo a la sesión sin tocar el kernel, pero cada programa tendría dos servicios de archivos: dos llaves, asas que
hay que saber de cuál son, y la tabla de nombres dejaría de ser "nombre a raíz" (S6b, capa 2). Habría que cambiar
todos los clientes (la biblioteca de asignaciones, el explorador, el Workbench, la caja Amiga, el runtime de Dash), no
se podría ceder una asa de `RAM:` como las demás, y cada sesión cargaría con otro proceso y sus cachés. Dentro de
`archivos`, `RAM:` es una raíz más y ningún programa cambia. Lo que se pierde, que la memoria se cobre sola a la
sesión, se recupera con el objeto disperso cobrado a su grupo: los datos de `RAM:` se cobran a la sesión; a `archivos`,
solo sus tablas y lo que DashFS guarda en su montón.

### Pruebas

- **Kernel** (`make prueba`, `grupos` e `hijo`): un objeto de 4 MB en un grupo de 1 MB nace igual y solo cobra su lista;
  cada página se cobra al ponerla y vuelve al soltarla; puesta otra vez llega a cero; el tope del grupo para el
  llenado (252 páginas) y una más da `E_MEMORIA`; tramos fuera, 0 o 257 páginas, otra operación, una llave que no es
  memoria o sin escribir, un grupo sin escribir: rechazado; con una copia de la llave viajando en un mensaje, ni poner
  ni soltar; un hijo que lee un hueco muere él solo (clase 2); al final el grupo queda a cero.
- **PC** (`archivos/herramientas`, 6 pruebas con DashFS de verdad sobre el disco de memoria): recién hecho ocupa menos
  de 12 páginas; mil guardados no gastan el megabyte del anillo; llenar da "lleno", lo escrito sigue entero, lleno del
  todo se puede borrar y la memoria vuelve; sin memoria a medias (el disco dice 16 MB y hay 1) falla limpio 50 veces
  sin perder una página; reescribir no crece y recortar devuelve; y lo que no vale (fuera del disco, medio bloque,
  números que dan la vuelta, descartar lo que no está). En todas, lo puesto es justo lo que DashFS dice usar.
- **QEMU** (`make arranque-virt`, programa nuevo `Programas/ram`, que `guardian` lanza como Ana y como Carla):
  - Ana: `ENV:Sys/idioma` trae lo de `ENVARC:`; cambiado en `ENV:`, `ENVARC:` sigue igual. Llena `RAM:` con datos que
    no se comprimen hasta que da -11 (lleno); escribir en otro fichero, -11; lo de antes se lee y el disco sigue. La
    memoria libre de la máquina baja lo escrito y, al borrar, vuelve.
  - Carla, con la sesión de Ana abierta: su `RAM:` tiene 2 entradas (`T` y `Env`); el secreto que Ana dejó en la suya
    no está, 0 caminos llegan a él por sus 16 raíces, pedir el volumen por nombre da -12 y las órdenes de `sesiones`
    con la llave de un programa, -1.
  - Carla sale y entra (`guardian`, sin la pantalla): la huella de `T:` y el cambio de `ENV:` no están; `ENV:` trae
    ahora su capa (la que guardó en `Home:Prefs/Env-Archive`) encima de la de fábrica; cambiar `ENV:` no toca
    `ENVARC:` ni `Home:`.

Cambios en lo esperado (`kernel/pruebas/arranque_virt.txt`, con sus programas, en el mismo commit): las líneas de
raíces del Workbench y del explorador (ya sin `RAM:`; "la pone sesiones al entrar"); en los nombres del explorador,
`T:` al arrancar "no existe en ninguna de sus capas" (aún no ha entrado nadie); el Workbench dice los iconos de `RAM` y
`Work` otra vez al abrirse la sesión; y las líneas nuevas de `sesiones`, `ram` y `guardian`. `esperado.txt`: la línea
de memoria dispersa de `grupos`. El guion `tools/prueba_uefi.sh` no cambia.

### Medido (QEMU `virt`, 2 GB, con otras pruebas corriendo en la máquina)

| Qué | Antes (cajón del disco) | Ahora (en memoria) |
|---|---|---|
| Entrar en la sesión (`sesiones`, de recibir la orden a contestar; el volumen ya se abrió una vez) | 0,7 a 1,2 ms | 10 a 15 ms (25 la más lenta): unos 5 de crear el volumen y de 4,5 a 9 de `T` y `ENV:` |
| Memoria en reposo por sesión abierta | 0 (y cuatro cajones en el disco) | 188 KB cobrados al grupo de la sesión: 60 KB de páginas (15) y 128 KB de la lista del objeto de 64 MB; 200 KB con una capa de `ENVARC:` del usuario |
| Memoria de `archivos` (lo que le cobra el kernel, con las sesiones de la prueba abiertas) | 36,6 MB | 36,8 MB |
| Salir | 2,2 a 2,4 ms | sin medir aparte; el grupo de la sesión pasa de 188 o 196 KB a 0 |

Llenar y vaciar (Ana, tope de 64 MB): 65.216 KB escritos en 588 ms (unos 110 MB/s en QEMU) hasta "lleno"; quedan 216 KB
libres en `RAM:`. La memoria libre de la máquina pasa de 934.004 KB a 870.632 (baja 63.372) y, tras borrar el fichero,
a 935.852 (vuelven 65.220). Libre en `RAM:`: 65.468 KB antes, 216 llena, 65.436 después.

La entrada es unas diez veces más lenta que antes, en milisegundos: el volumen se crea entero (DashFS nuevo, unas
cuantas transacciones) y `ENV:` son tres órdenes con sus viajes. No se ha afinado. En una máquina de verdad, abrir el
volumen cifrado del usuario (Argon2 con su coste normal, no el de prueba) tarda mucho más que esto.

### Pendiente del usuario

1. **El tope de `RAM:`**: la 32.ª parte de la memoria de la máquina, entre 16 y 256 MB (64 MB con 2 GB). Vacía no
   ocupa. Podría ser un ajuste, o no tener más tope que la memoria libre (como en el Amiga).
2. **El grupo de la sesión solo tiene su `RAM:`.** Hoy cada programa lanzado va en su grupo propio (64 MB), hermano
   del de la sesión, no dentro. Que la sesión entera (programas y `RAM:`) tenga un tope común pide que `grupo_crear`
   admita un padre y decidir ese tope (S4b, "Cupos").
3. **El escritorio es de Ana** (provisional, de antes): Workbench y explorador reciben la `RAM:` de Ana cuando ella
   entra. Con Carla delante y Ana detrás, siguen teniendo la de Ana. Se arregla con el escritorio por usuario.
4. **`ENV:` se copia al entrar y no se guarda al salir**: lo que un programa cambie en `ENV:` se pierde si no lo
   escribe también en `ENVARC:`, como en el Amiga.

### Lo que queda

- Si `sesiones` se cae y `inicio` la relanza, los volúmenes en memoria de las sesiones abiertas se quedan en `archivos`
  sin dueño (y las sesiones, perdidas, como hoy).
- `RAM_COPIAR` salta enlaces y ficheros de más de 4 MB, y baja 16 niveles: de sobra para `ENVARC:`.
- La caja Amiga tiene su propio `RAM:` (`caja/dos/src/memfs.rs`); no usa este.
- `User-Startup` y `WBStartup` por usuario, que ya tienen dónde dejar sus cosas (`T:`, `ENV:`).

### Unión con `union` (revision más selector-llaves, red-dns, dashfs-tamano e idiomas)

- **Llamadas renumeradas**: selector-llaves ya tenía la 56 (`llaves_nombres`). `memoria_dispersa` es la **57** y
  `memoria_paginas`, la **58**: kernel, `dashos`, la prueba de `grupos`, la referencia y esta bitácora. El modo del
  `hijo` que lee un hueco pasa del 17 (ahora, leer su tabla de llaves) al **18**.
- **Idiomas dependía del `RAM:` en disco en dos sitios**, y los dos van ahora por el de verdad:
  - El `locale.prefs` de Carla estaba en `Estado/RAM/Carla/Env/Sys` del disco. Ahora está en su capa de `ENVARC:`, en
    su volumen cifrado (`Home:Prefs/Env-Archive/Sys/locale.prefs`), y `sesiones` lo copia a su `ENV:` al entrar. Para
    ponerlo ahí al hacer el disco de pruebas hay una herramienta nueva, `archivos/herramientas/src/poner.rs` (deja un
    fichero en un volumen, cifrado o no, con su dueño): `dashfs-ar` solo llena el volumen principal.
  - El centro de control tenía la raíz 3 fija en `Estado/RAM/Ana` (`RAICES_DE_MAS` de `inicio`). Ahora `inicio` no se
    la da y `sesiones` le pone, al entrar su usuario, la raíz del `RAM:` en memoria de la sesión, solo para leer
    (`LEEN_IDIOMA`), como al Workbench y al explorador (a esos, para leer y escribir).
- La prueba `ram` de Carla ya no borra `Home:Prefs` entero al acabar: solo lo que puso ella (ahí vive su `locale.prefs`).
- `union` (d0a6c517) traía marcas de conflicto sin resolver en `usuario/src/bin/inicio.rs` (`RAICES_DE_MAS`: la raíz
  de la barra de red-dns y las del centro de idiomas): resueltas aquí, con las dos.
- **El guion** (`tools/prueba_uefi.sh`): el paso que quita un permiso en el centro de control seguía en cuanto veía
  "Permisos en pantalla", y a veces la línea del compositor con el sitio de la ventana aún no había llegado: quitaba con
  Supr y no con el botón, y la prueba fallaba ("FALTA: quita a pide el permiso de eco con el botón"). Pasó dos veces en
  esta rama, antes y después de la unión. Ahora espera a tener las tres líneas que usa. No es de `RAM:`.

Pruebas sobre la unión: `make prueba`, `arranque-virt-2` y `arranque-virt-linux`, bien a la primera. `arranque-virt`,
bien a la tercera: la primera, todas las líneas bien y mal la captura del compositor ("las ventanas no están donde
deben", sin investigar: no se repitió); la segunda, la carrera del guion de arriba; la tercera, con el guion arreglado,
todo bien. La prueba de idiomas de `cambio` (Carla en inglés, "de ENV:Sys/locale.prefs") pasa en las tres.

Sin comprobar con una línea propia: que el centro de control lee `ENV:` por su raíz nueva (va en castellano como Ana,
que no tiene `locale.prefs`, con raíz o sin ella).
