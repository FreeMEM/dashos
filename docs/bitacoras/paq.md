# Bitácora: Paq, el gestor de paquetes (paq)

Rama `paq`, sacada de `revision` (66e0f990). Diseño: S8 (con lo decidido el 15 y el 16/11), S6 (firmas), S6b (tanda de
administrador, `guardian`), S4b (asignaciones en capas), S5 (instantáneas). Papel: implementa. El código común del
formato es `pkg2`, de Hangar (MPL-2.0), traído con `tools/traer_pkg2.sh` a `third_party/pkg2` (fuera de git, como
Emu68) y sin tocar. No se leyó código ajeno a DashOS ni a Hangar.

## 2026-11-16 y 17

### Las piezas

- **`tools/traer_pkg2.sh`**: copia `servicio/pkg2` (sin `target/`) a `third_party/pkg2` y `servicio/docs/formato` (la
  especificación, los vectores y la referencia en Python) a `third_party/docs/formato`, que es donde las pruebas de pkg2
  buscan sus vectores (`../docs/formato/vectores`). Apunta una huella de lo traído en `third_party/pkg2/HUELLA`. Las
  pruebas propias de pkg2 pasan desde ahí (`cargo test` en `third_party/pkg2`).
- **`paq/`, la biblioteca** (`no_std`, MPL-2.0). Sin disco: huellas (CRC-32, SHA-256), firmas Ed25519 con etiqueta
  (`verify_strict`), gzip y tar estrictos, el índice `PKG-INDEX 2`, la lista de repositorios y las claves, la ficha de
  cada versión (`PKG-DB 2` con `Sum:`), las generaciones y los cambios de un plan en una línea. Con disco (rasgo
  `Disco`): el motor (`motor.rs`). Crates nuevos, todos `no_std`: `ed25519-dalek` y `curve25519-dalek` (BSD-3-Clause,
  sin las tablas precalculadas), `sha2` (MIT/Apache-2.0), `miniz_oxide` (MIT/Zlib/Apache-2.0, solo descomprimir).
- **La orden `Paq`** (`ordenes/src/paq.rs`, programa suelto `SYS:C/Paq`, no dentro de `C/Ordenes`):
  `Instalar`, `Quitar`, `Buscar`, `Info`, `Actualizar`, `Generaciones`, `Volver [n]`, `Limpiar [TODO]`, `Repos`,
  `Claves`; con `HOME`, en `Home:Paquetes`. Alias en inglés: `install`/`in`, `remove`/`rm`, `search`/`se`, `info`,
  `upgrade`/`up`, `generations`/`gen`, `rollback`/`back`, `clean`, `repos`, `keys`.
- **El servicio `paq`** (`ordenes/src/bin/paq.rs`, lo lanza `inicio`): el único que escribe en `Paquetes:` (usuario
  2, dueño de `Paquetes`; raíz 0 `Paquetes` para escribir, raíz 1 el disco para leer). Su canal solo lo tiene
  `sesiones` (una copia sin marca que le da `inicio`), y solo atiende mensajes sin marca.
- **`sesiones`** (cambios pequeños, lo demás en `usuario/src/sesiones_paq.rs`): las acciones de Paq de una tanda
  (`instalar`, `actualizar`, `volver`, `quitar`, `limpiar`, en `usuario/src/tanda.rs`) van juntas al servicio `paq` y
  se espera su respuesta en el hueco del comprobador; y las asignaciones de los paquetes de todos
  (`Paquetes:Paq/Asignaciones`) van en la tabla de nombres de cada proceso.
- `Paquetes=SYS:Paquetes` en las asignaciones de la sesión (`asignaciones::DE_SESION`).

### Cómo es

**Dónde vive cada cosa** (los dos niveles, igual): `<raíz>/<Cajón>` es lo que se ve (`Paquetes:Saludo`); en
`<raíz>/Paq/` va lo de Paq, sin icono (ordenado, no escondido): `Almacen/<nombre>/<hueco>.pkg` (la ficha), `.readme`
(el manifiesto tal cual) y `<hueco>/` (el cajón cuando no está en uso), `Generaciones/<n>`, `Asignaciones`, `Series` y
`Cerrojo`. `<raíz>` es `Paquetes:` (lo de todos) o `Home:Paquetes` (lo de cada uno). La lista de repositorios y las
claves, en `SYS:Estado/Paq/Repositorios` y `SYS:Estado/Paq/Claves/<nombre>` (S8: `Estado:`, que aún no es volumen).

**La generación manda.** Cada generación es un fichero de texto que se escribe entero de una vez (`GUARDAR` del
servicio de archivos: crear y escribir en una transacción) y no se cambia nunca. Manda la de número más alto que cuadra
con su `Sum:`; una dañada no cuenta (se usa la de antes y se avisa). Volver es escribir otra nueva igual a una de antes
("Volver a la generación 1"), así que la lista solo crece hasta que se limpia.

**El cambio de vista.** El sistema de archivos no tiene enlaces ni vistas por proceso de un cajón suelto, y una tabla
de nombres con una línea por paquete no cabe (1 KB, 6 capas). Así que cada versión tiene un solo cajón, que está o en el
almacén o a la vista, y se pasa de un sitio a otro cambiando de nombre (atómico, sin pisar). Una versión cuyo cajón
falta en el almacén es la que está a la vista: no hace falta otro fichero que lo diga. Tras escribir la generación,
Paq mueve lo que sobra al almacén y pone lo que toca; si se corta a medias, al empezar otra vez (el servicio al
arrancar, la orden antes de cada transacción) se termina, hacia la generación que manda. Lo que se ve es siempre una
generación entera: la de antes (si el corte fue antes de escribir la nueva) o la nueva. Lo que dejó a medias una
transacción (un `.nuevo` a medio desempaquetar, un cajón sin ficha, un manifiesto suelto) se barre. Un programa abierto
sigue con su cajón: se mueve entero, no se pisa.

**Una transacción**: el cerrojo; poner en orden; leer y comprobar los índices; el plan (con `pkg2::resolutor::resolver`
para instalar y actualizar, a mano para quitar con los huérfanos, y la diferencia con la tabla de antes para volver) y
`pkg2::plan::comprobar` siempre (necesita, choques, mismo cajón o asignación, retenidos, bajar de versión, cabe); bajar
cada paquete que falte, comprobar su tamaño y su SHA-256 contra el índice, descomprimir con tope, leer el tar con todas
sus reglas, leer el manifiesto con `pkg2::formato::leer` y mirar que es el del índice, y dejarlo en el almacén; todo eso
sin tocar nada de lo que se ve. Luego la generación, la vista, las asignaciones, la serie vista de cada índice, y la
limpieza sola (las tres últimas y las de 30 días).

**Un plan, una contraseña.** Para lo de todos, la orden hace el plan (solo lee) y lo pide como tanda: una acción por
cambio, con las palabras de `paq/src/cambio.rs` (`instalar saludo 1.0-1 prueba explicit`...). `guardian` enseña la
lista entera ("Instalar para todos saludo 1.0 (de prueba)", "... letras 1.0 (de prueba; lo necesita otro)") y pide la
contraseña una vez; `sesiones` pasa al servicio las líneas confirmadas y el servicio hace solo esas: si lo instalado
cambió entre medias, no hace nada ("pídelo otra vez"). El servicio lo vuelve a comprobar todo (firmas, huellas, plan):
lo que diga la orden no le vale de nada a la seguridad. Lo de cada usuario (`HOME`) lo hace la orden misma, con sus
permisos, sin administrador.

**El contenedor** de un paquete de DashOS (el formato lo deja a DashOS): `tar` ustar con gzip. Se admiten solo
ficheros y cajones; un enlace, un aparato o una cabecera extendida tiran el paquete, como una ruta con `:`, `..`, `.`,
partes vacías, barra delante o al revés, bytes de control, o dos rutas que solo cambian en mayúsculas. Todo va dentro
de su cajón (`Install: drawer X`), salvo el manifiesto en la raíz. Las asignaciones (`Assigns`) solo a su cajón y con
un nombre que no sea del sistema (`LIBS:`, `C:`, `SYS:`...). `Startup` solo `Run <su programa>` con `autostart` (se
apunta; aún no se lanza).

### Para la sesión de Hangar (pkg2 no se ha tocado)

1. **Falta leer el índice.** El encargo dice que pkg2 trae "índice" y "firmas", y no los trae. Paq los lee con su
   código (`paq/src/indice.rs`, con `leer_lista`, `numero`, `version_valida` y `nombre_valido` de pkg2). Propuesta:
   `pkg2::indice::leer(texto) -> Result<Indice, String>` con la cabecera (`Repository`, `Architecture`, `Serial`,
   `Expires`) y una `Entrada` que sea la de `resolutor::Entrada` más `Url`, `Size`, `Sha256`, `Installed-Size`,
   `Install`, `Short`, `Permissions`, `Assigns`, `Declared` y `Security`; una entrada con algo que no se entiende, fuera
   y contada (no tira el índice). Con sus vectores (`vectores/indices/`).
2. **Falta comprobar firmas.** Con Ed25519 dentro, pkg2 dejaría de ser "sin dependencias". Propuesta sin dependencias:
   `pkg2::firma::comprobar(etiqueta, datos, sig_texto, claves, verificar)`, que lee las líneas `ed25519 <id> <firma>`,
   calcula el id (necesita SHA-256: o un `sha256` propio pequeño, o que lo pase quien llama), arma `etiqueta + 0 +
   datos` y llama a `verificar(pública, mensaje, firma) -> bool`, que pone quien la usa (`ed25519-dalek` en el servicio
   y en Paq). Paq lo hace hoy en `paq/src/firma.rs` (con `verify_strict`); sus casos de prueba sirven de vectores.
3. **El resolutor no quita.** No hay `Orden::Quitar` ni los huérfanos de la sección 3 ("Orphans"). Paq lo hace en
   `motor::plan_quitar` (quita lo pedido y las dependencias que ya no alcanza nada pedido por `Requires` o `Recommends`)
   y lo comprueba con `plan::comprobar`. Propuesta: `Orden::Quitar(Vec<String>)` que dé `- x` también para los
   huérfanos, con vectores.
4. **Las líneas del plan no se pueden leer de vuelta sin ambigüedad.** `+ x 1.0-2 repo #Id (motivo)` lleva el Id, pero
   `^ x 1.0 -> 1.0-2 repo` no, y `1.0-2` puede ser la versión `1.0` con release 2 o la versión `1.0-2`. Paq busca la
   entrada que, escrita igual, da ese texto. Propuesta: el `#Id` también en las `^`, o que `resolver` devuelva pasos con
   el índice de la entrada (`Vec<Paso { que, entrada: usize, motivo }>`) además del texto de los vectores.
5. **`tests/formato.rs` no está.** `formato.rs` dice que lo comprueban los vectores de `docs/formato/vectores` en
   `tests/formato.rs`, pero en `pkg2/tests/` solo están `azar.rs`, `comprobador.rs` y `resolutor.rs`: los vectores de
   versiones, requisitos y manifiestos no se pasan en Rust (solo en la referencia de Python o en el servicio).
6. **Las rutas de `Assigns`.** La especificación dice "relativas a donde se instaló" y el ejemplo es
   `MONITOR: Monitor` con `Install: drawer Monitor`: Paq lo lee como `Run` (desde el destino, así que empiezan por el
   cajón) y rechaza una que no lleve a su cajón. Conviene decirlo así en la sección 2 (o al revés, y Paq lo cambia).
7. **El contenedor de DashOS**: Paq usa `tar` ustar con gzip (la especificación lo deja a DashOS). Cuando Hangar
   publique `aarch64-dashos`, su `repositorio.py` y el servicio tienen que hacerlos así (o decidir otro y avisar).
8. **La base local de Paq** (`PKG-DB 2`) lleva las rutas de `F` desde la raíz del nivel (`Saludo/LEEME`, no
   `Paquetes:Saludo/LEEME`: el cajón cambia de sitio), y dos campos de más, `Drawer:` y `Manifest:` (la suma del
   manifiesto que va al lado). El motivo y `Hold` van en la generación, no en la ficha. La sección 6 dice "dónde, lo
   decide cada gestor": quizá también decir que los campos de más se ignoran.

### Pendiente del usuario

- **El contenedor**: `tar` ustar con gzip (`.tar.gz`), con el manifiesto en la raíz y todo lo demás en su cajón.
- **Dónde va lo de Paq**: `<raíz>/Paq/` (sin icono), con `Almacen`, `Generaciones`, `Asignaciones`, `Series` y
  `Cerrojo`; ningún paquete puede tener un cajón `Paq`. La lista y las claves en `SYS:Estado/Paq/` (texto; formato
  arriba, el de la sección 5 del formato sin las claves dentro: cada clave en su fichero, para un repositorio).
- **Volver crea una generación nueva** (copia de la que se pide), en lugar de mover un puntero: la lista crece y lo que
  se ve es siempre "la más alta". `Volver` sin número, a la de antes de la que está en uso.
- **Limpiar a mano** (`Paq Limpiar`) guarda la de ahora y la de antes (para poder deshacer la última); `Paq Limpiar
  TODO`, solo la de ahora. La sola (tras cada transacción), las tres últimas y las de 30 días, como dice S8.
- **Lo de cada uno no se confirma**: `Paq Instalar x HOME` enseña el plan y lo hace, sin preguntar (como
  `pacman --noconfirm`). El formato dice "se enseña y se confirma"; para lo de todos la confirmación es la de `guardian`.
- **`HOME`** como interruptor de la orden (lo de cada uno); sin él, lo de todos.
- **Los nombres de la orden**: `Instalar`, `Quitar`, `Buscar`, `Info`, `Actualizar`, `Generaciones`, `Volver`,
  `Limpiar`, `Repos`, `Claves`, y los alias en inglés (arriba). Las frases de `guardian`: "Instalar para todos saludo
  1.0 (de prueba)", "Actualizar para todos saludo de 1.0 a 1.1 (de prueba)", "Volver para todos saludo de 1.1 a 1.0",
  "Quitar para todos saludo 1.0", "Borrar las versiones viejas de los paquetes de todos (menos la de ahora y la de
  antes)".
- **Sin instantánea por transacción.** El encargo pedía instantánea si existe la orden (existe: `TOMAR`). No se toma:
  S8 (15/11) cambió "una instantánea nueva de `Paquetes:` por transacción" por el cambio de tabla, y hoy `Paquetes:`
  no es un volumen: la instantánea sería del disco entero (con los datos de todos, que no deben volver atrás) y
  sujetaría sus bloques. Cuando `Paquetes:` sea un volumen, una antes de cada transacción es un seguro barato contra un
  almacén estropeado.
- **Un cerrojo de `Home:` viejo**: si un `Paq` muere a medias, el siguiente lo da por viejo si es de otro arranque o de
  hace más de 10 minutos, y sigue (lo pone en orden antes). En `Paquetes:` no hace falta: lo quita el servicio al
  arrancar (es el único que escribe ahí), y dos tandas a la vez no llegan (la segunda da "ocupado").
- **Un repositorio cuyo índice no vale no tira los demás**: se dice ("el repositorio firmamal no se usa: ...") y se
  sigue con los otros.

### Lo que queda

- **Las capas de paquetes en `C:`, `LIBS:`, `FONTS:`...** (S4b, `Paquetes:*/C`): no están. Una tabla de nombres con
  una capa por paquete no cabe (6 capas, 1 KB) ni escala; es lo que S8 deja "por diseñar al hacerlo" (cómo se resuelven
  con la tabla sin coste al abrir programas). Hoy un paquete se usa por su cajón (`Paquetes:Saludo/...`) o por sus
  asignaciones declaradas.
- **Las asignaciones de lo de cada uno** (`Home:Paquetes/Paq/Asignaciones`): Paq las escribe, pero `sesiones` aún no
  las lee al entrar la persona (solo las de todos). Lo que haga falta: leerlas con el volumen de la persona al entrar,
  como lo concedido, con las mismas reglas (`sp::linea_valida`, con `Home:Paquetes/` en vez de `Paquetes:`).
- Un programa que ya está abierto (el Shell, por ejemplo) no ve las asignaciones nuevas hasta que se abre otra vez:
  su tabla es la de cuando nació (como dice S8 de las versiones). Lo lanzado desde él las hereda de él.
- Los repositorios por la red (`https://`), las respuestas firmadas, los deltas, el fichero `keys` con las claves raíz
  (S8, 15/11): Paq lee repositorios en un disco, con las claves de índice en `Estado:`.
- Añadir y quitar repositorios y claves (acción de administrador): `Paq Repos` y `Paq Claves` solo enseñan.
- Paquetes sueltos (un `.tar.gz` de un USB, con `PKG-FILE-2` o sin firma, el "suelto" de S6b), `Install: system` y
  `installer` (programas Amiga), variantes (`Variant:`), `Config:` (`.new`), `Hold` desde la orden, `nombre@repo`
  probado, `Recommends` que se pueden desmarcar, avisar de lo de seguridad, la comprobación diaria y bajar en segundo
  plano, "no se puede volver atrás desde esta versión", el espacio y la limpieza al llenarse el disco.
- La máquina de `aarch64-dashos` (`hypervisor`, la memoria): Paq aún no la sabe (`Maquina::default()`); un paquete que
  pide alguna de las dos no se instala.
- Los permisos que declara un paquete (`Permissions`) se enseñan en `Paq Info`, pero `sesiones` aún no los usa (S6b:
  se niega lo no declarado): sigue con su tabla `PERMISOS` y lo concedido.
- `Startup: Run <programa>` con `autostart` se acepta y se guarda; aún no se lanza al entrar.
- La generación no apunta la versión de `SYS:` con que se hizo (S8): aún no hay versión de `SYS:` que leer.

### Pruebas

- PC: `cargo test` en `paq/` (27): huellas, firmas (buena, datos cambiados, otra etiqueta, otra clave, un bit), gzip
  (bombas, cortado en cada byte, un byte cambiado, algo detrás, basura), tar (rutas que se salen, cada tipo de enlace y
  de cabecera rara, repetidas sin mayúsculas, grande, cortado, basura), el índice, las fichas, las generaciones, los
  cambios; y el motor con un disco en memoria: instalar con dependencia, actualizar, volver, quitar con huérfanos,
  limpiar; índice con firma de otra clave, índice cambiado tras firmar, índice más viejo que el último visto; paquete
  cambiado tras firmar (nada a la vista ni generación nueva); `..`, `/`, volumen, escribir en el cajón de otro, enlace
  simbólico, asignación del sistema (`LIBS:`) o a otro cajón; ciclo (se instalan los dos), dependencia imposible,
  choque, quitar lo que otro necesita; dos Paq a la vez (cerrojo); **un corte de luz en cada escritura** de una
  actualización: tras poner en orden se ve la generación de antes entera o la nueva entera, y no queda basura; una tabla
  dañada (se usa la anterior, se avisa, la siguiente no la pisa); un cajón ajeno en el sitio (no se pisa). Y el
  repositorio de verdad (`tools/repo_paq.py`, con el `tarfile` de Python y `cryptography`) leído con lo de aquí.
- PC: `usuario/src/tanda.rs` (21, con las acciones de Paq y sus frases) y `usuario/src/asignaciones.rs` (12).
  `third_party/pkg2`: sus pruebas pasan.
- QEMU (`make arranque-virt`, el guion del Shell, `S:Prueba-Shell`, como Ana): `Paq Repos`, `Claves`, `Buscar`;
  `Instalar saludo` (tanda de 2 en `guardian`, con letras), `Execute Paquetes:Saludo/Saludo` ("Saludo 1.0"), `Info`;
  copiar el índice de la serie 2 y `Actualizar` ("Saludo 1.1"); `Volver` ("Saludo 1.0", del almacén); `Quitar saludo`
  (se lleva letras; el cajón ya no está); `Generaciones`, `Limpiar` (otra tanda), `Generaciones`. Lo de cada uno, con
  alias: `install saludo HOME`, `in circulo-a HOME` (el ciclo), `gen HOME`, `rm saludo circulo-a HOME`. Lo hostil:
  `fuera`, `absoluto`, `enlace`, `vecino`, `cambiado`, `imposible`, `ajeno` (repositorio mal firmado), `noexiste`; el
  Shell escribiendo en `Paquetes:` o borrando una generación (protegido); la tanda de `letras` con una contraseña mala
  (nada). Al arrancar, el servicio quita el cerrojo viejo y barre el cajón a medias que trae el disco. Tras la prueba,
  `disco_archivos.py --comprobar` mira `Paquetes/` entero (lo que queda tras limpiar, nada a medias, nada del intruso)
  y que el índice del repositorio es el de la serie 2.
- Cambios en lo compartido, por esto: `kernel/pruebas/arranque_virt.txt` (la raíz del disco tiene 22 entradas, con
  `Paquetes/`, y las líneas de Paq), `tools/prueba_uefi.sh` (a la tanda de "Instalar para todos letras" le teclea una
  contraseña mala) y `tools/disco_archivos.py`.

### Resultados

- `cargo test` (paq), `tanda.rs`, `asignaciones.rs`: bien.
- `make prueba`, `make arranque-virt-2`, `make arranque-virt-linux`, `make arranque-virt-shell`: bien a la primera.
- `make arranque-virt`: tres vueltas, ninguna entera, por el fallo conocido de las teclas y clics perdidos con carga
  (no es de esta rama; lo lleva `firefox-firme`). La 1.ª llegó al final del guion del Shell (todo Paq salió bien) y se
  paró en la línea de la raíz de `ficheros`, que había que poner con 22 entradas (arreglado). La 2.ª se paró antes, en
  "[dash-ventana] pulsado". La 3.ª, en "[ajustes] el desplegable dice: Destello", ya pasado todo el guion del Shell:
  las 55 líneas de Paq salen y en orden (comprobado aparte sobre su salida), y `--comprobar` no encuentra nada en
  `Paquetes/` ni en el repositorio (lo que sí señala es de lo que no llegó a pasar tras el fallo: `C/informe.txt`,
  `C/Prefs/panel`, la fecha de `hola.txt`).

### Lo medido

- `SYS:C/Paq` mide 404 KB (las firmas, gzip, tar y pkg2; `opt-level = "z"`); el servicio `paq`, 328 KB, en la imagen
  (comprimido con LZ4 como los demás). El disco de pruebas pasa a tener 1.656 KB libres al arrancar (unos 410 KB menos:
  `C/Paq` y el repositorio, 7 KB). A15 sigue bien.
- El tiempo de una transacción no se midió (en QEMU, cada tanda se ve acabar en menos de un segundo tras la contraseña,
  a ojo por el orden de las líneas, no medido).

### Lo que no se comprobó

- La ventana de `guardian` con las tandas de Paq en pantalla (solo su línea en la consola); con teclado de verdad.
- Que un programa nuevo vea `SALUDO:` (la asignación del paquete): `sesiones` la carga ("1 (y 0 que no valen)"), pero
  ningún proceso nacido después la usa en la prueba.
- Un corte de luz de verdad en QEMU a media transacción: solo el disco que trae los restos de uno (y en el PC, cortes
  en cada escritura).
- Dos `Paq` a la vez en QEMU (solo en el PC).
- Paquetes grandes (de MB), el tiempo y la memoria de descomprimirlos (el paquete entero se lee a memoria).
- En la Pi.
