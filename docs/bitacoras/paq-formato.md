# Bitácora: Paq sigue el formato de paquetes 2 nuevo (paq-formato)

Rama `paq-formato`, sacada de `revision` (f27af229, que ya tiene Paq). La especificación es la de Hangar,
`servicio/docs/FORMATO_PAQUETES.md` (leída entera, solo lectura); la sesión de Hangar cambió tres cosas: `Url` se repite
(el original y sus copias), la pregunta firmada `identify` y dos aclaraciones (las rutas de `Assigns` y los campos de
más de la base local). Lo de antes, en `docs/bitacoras/paq.md`. No se leyó código ajeno a DashOS ni a Hangar.

## 2026-11-17

### `Url` repetida

- **El índice** (`paq/src/indice.rs`) guarda todas las `Url` de una entrada, en orden (`Entrada::urls`; una repetida
  se probaría dos veces, sin daño).
  Una entrada sin ninguna, con una vacía, de más de 1.024 bytes o con más de 16 (`MAX_URLS`) no se usa: cuenta en
  `malas`, como una con dependencias que no se entienden (el índice sigue valiendo).
- **Al bajar** (`motor::bajar`), se prueban en orden. Cada una se lee sin pasar de su `Size` y se mira su tamaño y su
  SHA-256; la que da otra cosa se tira y se pasa a la siguiente. Nada toca el disco hasta que una cuadra: lo de antes
  (descomprimir, el tar, el manifiesto, el almacén) va igual, con la buena. Nunca se lee más de 64 MB (`MAX_PAQUETE`):
  con una `Size` mayor, lo leído es "otro tamaño".
- **Qué vale como dirección** en un repositorio de disco: relativa a `<url>/aarch64-dashos/` (lo de siempre) o entera
  pero dentro de ahí (como la escribe `repositorio.py`, la referencia). `http://` y `https://` se saltan ("es de la
  red, y Paq aún solo lee repositorios en un disco", el mismo texto que para un repositorio de la red); cualquier otra
  cosa con `:` (`ftp:`, `file:`, `javascript:`, `HTTP://` en mayúsculas, un volumen como `SYS:Estado/...`) o que se
  sale (`..`) también ("se sale del repositorio").
- **Si ninguna vale**: con una sola dirección, lo de siempre ("cambiado 1.0 no es el paquete que dice el índice (está
  cambiado): no se toca nada", la línea que espera la prueba del Shell). Con varias, lo que dio cada una ("letras 1.0
  (malas/tamano.tar.gz: da otro tamaño; malas/suma.tar.gz: da otra suma; ...) no es el paquete que dice el índice...").
  Si ninguna se pudo ni leer (todas de la red, o no están), `Fallo::Paquete` con la lista ("el paquete letras no se
  instala: https://...: es de la red...; noesta.tar.gz: no se puede leer").

### `identify` (lo que le toca a Paq)

En `paq/src/reconocer.rs`, sin tocar la orden ni el servicio `paq` (Paq aún no tiene red; ver "Lo que queda"):

- **El inventario** de lo instalado a mano en un nivel (`Paquetes:` o `Home:Paquetes`): los programas (ELF, o hunk de
  Amiga) de los cajones que no son de ningún paquete a la vista ni el de Paq, hasta tres cajones por debajo, sin los
  iconos. De cada uno, `fichero TAB tamaño TAB nombre TAB versión`, con el nombre y la versión de su `$VER:` (como el
  cliente de Hangar: la primera cadena que se entiende, en UTF-8 o Latin-1, sin `.library` ni la `v`). Como mucho
  5.000 (lo que lee el servicio; se dice si se corta). No se mandan, y se cuentan: un fichero de más de 8 MB (no se
  puede mirar sin leerlo entero: `Disco` no da el tamaño), uno que no se puede leer y uno cuyo nombre lleva un
  tabulador o un salto de línea.
- **La pregunta**: POST a `<url>/aarch64-dashos/identify?nonce=<nonce>`; la `Query` esperada es la ruta con los
  parámetros, un espacio y el SHA-256 del cuerpo (como la arma el servicio: `api2.rs`, `format!("{uri} {sha}")`). El
  `Nonce`: un contador de 64 bits, que sube y se guarda en `<nivel>/Paq/Nonce` (se escribe aparte y se cambia de
  nombre; si se corta, se coge el más alto), y 8 bytes al azar que pone quien llama. 32 cifras hexadecimales.
- **El envío**, detrás del rasgo `Red` (`enviar(url, cuerpo, tope) -> Respuesta { cuerpo, firma }`), que pondrá quien
  tenga red. Un repositorio de disco (estático) no contesta preguntas: se dice.
- **La respuesta** (`reconocer::comprobar` y `leer_lineas`): firmada con una clave `Use: response` de ese repositorio
  (`X-Pkg-Signature: <id> <firma>`, etiqueta `PKG-RESPONSE-2`); cabecera `PKG-RESPONSE 2` con cada campo una vez;
  `Repository`, `Architecture`, `Nonce` y `Query` los de la pregunta; `Serial` no menor que la última vista del
  repositorio (no se apunta como vista: ver "Para la sesión de Hangar", 6); y luego exactamente una línea por
  fichero, en su orden, que repite su nombre, con siete campos, un estado conocido, `Id` numérico cuando lo reconoce y
  una ruta dentro del paquete que valga. Si algo no cuadra, no vale nada.
- **Lo reconocido**, por cajón (`candidatos`): qué programa del catálogo es (`Id`, nombre), la versión del disco y la
  del catálogo. No se apunta nada como instalado.

### Lo aclarado en (3): ya era así

- `Assigns` desde el destino, empezando por el cajón: Paq lo leía así (`motor::dentro_de`, `asignaciones`), con su
  prueba (asignación a otro cajón, rechazada).
- La base local admite campos de más: `Ficha::leer` solo mira los suyos; prueba nueva con `Reason`, `Hold`, `Target`,
  un campo de otro gestor y líneas `C`, `A` y `S`.
- Las rutas de `F` son las de DashOS, desde la raíz del nivel (`Saludo/LEEME`).

### `tools/traer_pkg2.sh`

Trae también la especificación en castellano (`docs/FORMATO_PAQUETES.md`, la que manda) a
`third_party/docs/FORMATO_PAQUETES.md`, y la cuenta en la huella. `pkg2` y `docs/formato` no cambiaron (comparados con
`diff -r`): la especificación nueva no está en `docs/formato/PACKAGE_FORMAT.md` (la inglesa) ni tiene vectores de
`identify`.

### Pruebas

- PC, `cargo test` en `paq/` (41, 14 nuevas). Copias: en orden hasta la buena, que va la última y entera, detrás de
  una cortada (otro tamaño), una con un bit cambiado (otra suma), otro paquete válido más grande (letras 6.6), una
  enorme (×1000), de la red, `ftp:`, que no está, un volumen, `..`, `javascript:`, `file:` y un esquema con eñes; la
  primera buena gana; todas malas (`Huella` con lo de cada una, nada a la vista, ni generación, ni almacén); ninguna
  se puede bajar (`Paquete`); mil `Url` (la entrada no se usa, las demás sí); `Size` de 1 TB (no se lee más de 64 MB).
  Índice: varias `Url`, ninguna, vacía, 16, 17, mil, demasiado larga. `identify`: cadenas `$VER:` (librería, `v`, Latin-1, cortada,
  larguísima); el inventario (lo de Paq fuera, iconos, texto, hondo, tabulador, suelto en la raíz, `Home:Paquetes`);
  5.010 programas (se cortan en 5.000) y uno de más de 8 MB; la `Query`; el contador del `Nonce` (sube, el `.nuevo` de un
  corte, un corte en cada escritura, uno estropeado); con un servicio de mentira que hace lo que `api2.rs`: la buena y
  17 trampas (otro `Nonce`, la respuesta a otra pregunta, otro repositorio, otra arquitectura, serie vieja, una línea de
  menos o de más, otro fichero, un estado raro, sin `Id`, una ruta que se sale, `Query` dos veces, un bit cambiado, la
  clave de índice en vez de la de respuestas, sin firma, una firma con un salto de línea, enorme); el repositorio de
  disco, sin clave de respuestas, sin red, y sin nada que preguntar (no se pregunta).
- QEMU: ver "Resultados".

### Lo medido: el sitio del disco de pruebas

La primera versión hacía `C:Paq` (`orden-paq`) 4 KB más grande: 414.120 → 422.312 bytes (sus secciones van por páginas
de 4 KB; `.text` creció 3,1 KB, con `bajar` metido en `aplicar`, +1,8 KB, y `.rodata` pasó de página por los textos).
Con eso, `make arranque-virt` falló en "[comprobador] A15 la foto: bien": "A15 interrumpido: el servicio
soltó el disco por falta de sitio" (4.608 KB ocupados, frente a 4.592 de `union-dms` y `tecla-pegada`, sobre la misma
base, con A15 bien). El disco de pruebas va justo (ver `union-dms`, d6430cc3). Una medida que engañó: la base compilada
en otra carpeta da 418.216, porque las rutas de los ficheros fuente (los mensajes de pánico) van en `.rodata`; se
compara con el binario de otra rama de `.worktrees/`, con una ruta igual de larga.

Arreglado sin quitar nada de lo pedido: `bajar` sin inline; los porqués como `&'static str`, cortos, y el de la red el
mismo texto que ya había; "más grande" es "otro tamaño"; el tope de 64 MB va en la lectura, sin mensaje aparte; sin
prefijo en el mensaje de "no se puede bajar". Ahora `orden-paq` mide lo mismo que en la base (414.120 bytes, con 232
bytes libres en la última página de `.text` y 4 en la de `.rodata`; en la base, 2.268 y 100) y el servicio `paq`
también (340.320). Medido con `cargo build --release` de `ordenes/` y `readelf -S`. **Ojo**: el próximo cambio que
alargue unos bytes los textos de `C:Paq` le añade 4 KB al disco de pruebas y vuelve a tirar A15; lo de fondo (darle
sitio al disco de pruebas) no es de esta rama. Visto después (ver "Resultados"): con el tamaño de la base, A15
también falló, por una carrera entre dos pruebas; así que los 4 KB quizá no eran la causa, o no la única. El arreglo se
queda: `C:Paq` no crece y no hace daño.

### Resultados

- `cargo test` en `paq/`: 41 bien. `cargo clippy`: nada nuevo (los avisos de "demasiados argumentos" de `aplicar` y
  `reconocer` son del mismo tipo que el que ya había).
- `make prueba` y `make arranque-virt-shell`: bien (tres veces cada una, la última con lo que va en los commits).
- `make arranque-virt`: ninguna vuelta entera.
  - Con `C:Paq` 4 KB más grande (las dos primeras y la tercera a medias): faltó "[comprobador] A15 la foto: bien" (A15
    interrumpido por falta de sitio, 4.608 KB ocupados) y, en la primera, la tecla pegada del explorador (la arregla
    `tecla-pegada`). De ahí el arreglo de "Lo medido".
  - Con `C:Paq` como en la base (dos vueltas, 4.592 KB ocupados, igual que `centro-vivo` y `tecla-pegada`): faltan
    "A15 la foto: bien" ("MAL": el servicio soltó el disco por falta de sitio) y la parte de `guarda` (líneas 979-1008
    y dos de `commodity-prueba`); la tecla pegada no salió. Ninguna de las dos parece de esta rama: la de `guarda`
    falta igual en `centro-vivo` (el guion no llega a lanzarlo; `tecla-pegada` ya la pasa con sus cambios del guion), y
    lo de A15 es una carrera entre dos pruebas: en mis vueltas, "[ficheros] A15 sujetar sin soltar" (que escribe hasta
    que queda poco sitio) corre antes de que el comprobador acabe sus 50 vueltas con el disco sujeto; en `centro-vivo`,
    después. Con el disco igual de lleno, solo cambia el orden, que depende de la carga (tres QEMU a la vez).
  - Todo lo de Paq salió en las cuatro (las 21 líneas de `[paq]` y `[shell] Paq:`, también "cambiado 1.0 no es el
    paquete que dice el índice (está cambiado): no se toca nada").

### Pendiente del usuario

- **16 direcciones por entrada como mucho**; una entrada con más no se usa (la especificación no pone tope).
- **Qué entra en el inventario**: solo la raíz de cada nivel (`Paquetes:`, `Home:Paquetes`), no todo `Home:`; solo
  programas (ELF y hunk), tres cajones por debajo como mucho, sin los de más de 8 MB.
- **Las copias de la red se saltan** (no se bajan), hasta que Paq tenga red.

### Lo que queda

- **Adoptar lo reconocido** (`Reason: adopted`). Propuesta: bajar la entrada por su `Id` (con sus copias y sus
  comprobaciones), mirar que los ficheros del cajón cuadran con los del paquete (CRC-32 y tamaño, como la ficha); si
  cuadran, el cajón pasa a ser el "a la vista" de esa versión (ficha y manifiesto al almacén) y la generación nueva lo
  apunta con el motivo `adopted` (falta en `generacion::Motivo`); si no, ofrecer instalar encima apartando el cajón. Con
  tanda de `guardian` para lo de todos, como lo demás.
- **La red**: bajar por `http(s)` y el `Red` de verdad; con ella, la orden (`Paq Reconocer`) y el servicio `paq`
  (lo de todos: es el único que escribe en `Paquetes:`, y el contador del `Nonce` vive ahí).
- Las demás preguntas firmadas (`package`, `updates`...) y el fichero `keys`: Paq lee el índice entero.

### Para la sesión de Hangar

1. **`identify` del servicio filtra siempre por 68k** (`reconocer_lineas` usa `FILTRO_68K`, sea cual sea `{arq}`): a
   `aarch64-dashos` contestaría con el catálogo del Amiga. Debería mirar la arquitectura pedida.
2. **El tope de `identify`**: el servicio lee 5.000 líneas y calla el resto, y quita las de fichero vacío; con más, la
   respuesta trae menos líneas que la pregunta. Conviene decirlo en la especificación (Paq no manda más de 5.000).
3. **Nombres que no se pueden mandar**: el servicio cambia tabuladores y saltos por espacios y quita los de los lados
   (`campo`), así que un nombre así vuelve distinto. Decir que no se mandan (Paq no los manda).
4. **Tope de `Url` por entrada**: la especificación no lo pone. Paq usa 16 (más: la entrada no se usa). Propuesta: un
   número en la sección 5.
5. **`Url` relativa**: la especificación no dice si vale; la referencia escribe la entera. Paq acepta las dos (la
   entera solo dentro de `<url>/<arquitectura>/`). Decirlo.
6. **La `Serial` de las respuestas** es la de ahora del repositorio (`repo::serie`), que puede ir por delante del último
   `index.gz` escrito. Paq la compara con la última vista del índice (una respuesta más vieja que el índice no vale),
   pero no la apunta: si la apuntase como vista, el índice publicado se tomaría por viejo. Un gestor que lee el índice
   entero y además pregunta tiene que llevar las dos series aparte; conviene decirlo en la sección 5.
7. **`$VER:` en los programas de DashOS**: Paq la busca también en los ELF; si los paquetes nativos no la llevan, el
   servicio solo puede reconocerlos por nombre y tamaño.
