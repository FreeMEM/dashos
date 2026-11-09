# Referencia del kernel de DashOS

Lo que un programa ve del kernel tal como está hoy en el código. El diseño, el porqué, está en `docs/diseño/` (S1 a
S3). Este documento dice qué hay y cómo se usa. Si no coincide con el código, manda el código y hay que corregir esto.

Estado: 2026-11-03, tras C30. Todo es **provisional**. En S11 se decidió que los programas no llamarán al kernel a
mano, sino a través de una biblioteca. Los números y el orden de los argumentos pueden cambiar.

Fuentes: `kernel/src/llamadas.rs`, `objetos.rs`, `elf.rs`, `planificador.rs`, `interrupciones.rs`, `espacio.rs`, y
la biblioteca de usuario `usuario/src/lib.rs`.

## Procesos

Un proceso tiene su espacio de direcciones, su tabla de llaves y su orden de bytes. Hoy cada proceso tiene un solo
hilo. Lo arranca el kernel desde un ELF.

Cada espacio tiene un ASID de 16 bits propio mientras vive; dos procesos vivos nunca lo comparten. Cuando sale el
último hilo, el proceso se libera en ese momento: su memoria, sus tablas y sus llaves.

### Formato del ejecutable

ELF64 para AArch64 (`e_machine` 183), en cualquiera de los dos órdenes de bytes:

- **Little-endian** (`EI_DATA` = 1): lo nativo de DashOS.
- **Big-endian** (`EI_DATA` = 2): la caja Amiga y sus bibliotecas.

El proceso corre en el orden de bytes de su ELF. El kernel pone `SCTLR_EL1.E0E` en cada cambio de hilo, así que
procesos de los dos órdenes conviven en el mismo núcleo. Las instrucciones son siempre little-endian, como manda
AArch64.

Hay dos tipos de ejecutable:

| Tipo | Dónde va | Pila |
|---|---|---|
| `ET_EXEC` | En sus direcciones. Todo debe acabar antes de 2 GB − 8 MB | Termina en `0x8000_0000` |
| `ET_DYN` (PIE estático) | Base al azar entre 128 y 256 GB, alineada a 64 KB | Tope al azar entre 256 y 384 GB |

Reglas comunes:

- Se cargan los `PT_LOAD`. Si un segmento tiene `X`, es código (lectura y ejecución). Si tiene `W`, son datos
  (lectura y escritura, sin ejecución). Si no, es de solo lectura.
- Se rechaza el ELF si:
  - un segmento tiene `W` y `X` a la vez;
  - dos segmentos comparten página;
  - algo cae en los primeros 64 KB;
  - la entrada no está en un segmento con `X`;
  - `e_phentsize` no es 56 o hay más de 64 cabeceras de programa.
- Si hay `PT_INTERP`, el ELF se rechaza. No hay enlazador dinámico.
- **El kernel no aplica reubicaciones.** Un PIE aplica sus `R_AARCH64_RELATIVE` al arrancar, y tienen que caer en un
  segmento escribible. Su base la saca de su propio contador de programa (`adr`/`adrp`).
- La pila mide el `p_memsz` del `PT_GNU_STACK`, redondeado a páginas, entre 4 KB y 8 MB. Sin `PT_GNU_STACK`, mide
  64 KB.
- Al arrancar, `SP` apunta al tope de la pila y `PC` a la entrada. Los demás registros valen cero. No hay
  argumentos, entorno ni vector auxiliar.

### Mapa de direcciones de un proceso

El espacio de usuario va de 64 KB a 512 GB (`1 << 39`). Los primeros 64 KB no se proyectan nunca. Hoy está así:

| Tramo | Uso |
|---|---|
| 0 – 2 GB | Ejecutables `ET_EXEC` (desde `0x40_0000`) y su pila (hasta `0x8000_0000`) |
| 0 – 4 GB | En la caja Amiga, la memoria del Amiga tal cual (S3). Ningún código nativo de la caja va aquí |
| desde 4 GB | Libre. El runtime de Dash pone ahí su montón, con `memoria_crear` y `memoria_proyectar` |
| 128 – 256 GB | Imagen de los PIE |
| 256 – 384 GB | Pila de los PIE |
| 384 – 512 GB | Libre |

El programa decide dónde proyecta su memoria. El kernel no le busca sitio.

### Fin de un proceso

- `salir(código)` termina el hilo, y con él el proceso.
- Una excepción del programa termina el proceso, no el sistema. Por ejemplo, un fallo de página, una instrucción
  ilegal o una alineación incorrecta. El kernel escribe en la consola la clase de la excepción, la dirección de la
  instrucción y la dirección del fallo.
- Al morir el último hilo se liberan la tabla de llaves, el espacio de direcciones y las páginas que solo usaba ese
  proceso (C18).

### FPU y SIMD

Cada hilo tiene sus propios registros de coma flotante y SIMD: `q0`–`q31`, `FPCR` y `FPSR`. La primera vez valen cero
y `FPCR` = 0, es decir, redondeo al más cercano y sin excepciones de coma flotante. El kernel los guarda solo cuando
otro hilo del mismo núcleo usa la FPU (C28). No hay SVE.

### Planificación

Los procesos de usuario van en la clase justa: reparto por tiempo virtual, con un tic de 10 ms. Un hilo que despierta
empieza como mucho medio tic por debajo del mínimo de su núcleo y, si va por delante del que corre, se adelanta al
momento (M1 y M3, `docs/MULTITAREA_LOOP.md`). La clase de plazos de S2 la usan el kernel y el sonido (llamada 28). Un
proceso corre en el núcleo donde se creó y no cambia de núcleo.

### El programa de inicio

En la imagen normal, el kernel lanza un solo proceso, `inicio` (C47), en el grupo raíz. Le da las llaves de arranque,
las únicas que da el kernel:

- La 1: el paquete de arranque, un objeto de memoria de solo lectura. Dice qué es cada una de las demás llaves y trae
  los ELF de los servicios. Su formato está en `kernel/src/inicio.rs`.
- Después, los permisos DMA (uno por aparato con DMA), el registro del sistema, la memoria de la VideoCore y, para
  cada aparato del árbol que conoce el kernel (serie, SD, USB, buzón de la VideoCore, y la V3D si contesta), sus
  registros (en páginas enteras) y su interrupción.

`inicio` lanza cada servicio en su grupo, con copias de las llaves de sus aparatos y los canales que comparte con
otros servicios, y se queda esperando. Hoy son la consola (C48), el buzón, el USB, el compositor y la GPU (C49, C50); lo que
ve un programa de ellos está en `servicios.md`. Ningún otro proceso recibe nada del kernel: todo lo
que tenga le llega desde `inicio`.

## Llamadas al sistema

Se entra con `svc #0`. El número va en `x8` y los argumentos en `x0`–`x4`. El resultado vuelve en `x0`, y en
algunas llamadas también en `x1`. Un resultado negativo es un error (ver la tabla de errores).

Las direcciones que se pasan son del propio proceso. Los tamaños van en bytes.

El kernel copia lo que lee o escribe en la memoria del proceso con accesos sin privilegio (`LDTRB`/`STTRB`, C31). La
MMU los comprueba con los permisos de EL0. Así, el kernel no puede leer por el proceso lo que el proceso no puede leer,
ni escribir donde él no puede escribir, como su código. Si una dirección no está proyectada o no tiene permiso, la
llamada devuelve `E_ARGUMENTO` y el kernel sigue.

| n | Llamada | Argumentos | Devuelve |
|---|---|---|---|
| 1 | `escribir` | `x0` texto, `x1` longitud | Bytes tomados: como mucho 256; hay que repetir con el resto |
| 2 | `salir` | `x0` código | No vuelve |
| 3 | `canal_crear` | — | `x0` y `x1`: las llaves de los dos extremos |
| 4 | `canal_escribir` | `x0` llave, `x1` datos, `x2` longitud, `x3` tabla de llaves, `x4` cuántas | 0 |
| 5 | `canal_leer` | `x0` llave, `x1` búfer, `x2` capacidad, `x3` tabla de llaves, `x4` capacidad de la tabla | `x0` bytes, `x1` llaves recibidas |
| 6 | `cerrar` | `x0` llave | 0 |
| 7 | `memoria_crear` | `x0` tamaño (hasta 4 MB) | Llave de un objeto de memoria |
| 8 | `memoria_proyectar` | `x0` llave, `x1` dirección (alineada a 4 KB), `x2` permisos: 1 leer, 2 escribir | 0 |
| 9 | `registros_proyectar` | `x0` llave de registros, `x1` dirección (alineada a 4 KB) | 0 |
| 10 | `interrupcion_asociar` | `x0` llave de interrupción, `x1` llave de un extremo de canal | 0 |
| 11 | `interrupcion_terminar` | `x0` llave de interrupción | 0 |
| 12 | `proceso_crear` | `x0` llave de memoria con el ELF, `x1` tamaño del ELF, `x2` tabla de llaves, `x3` cuántas, `x4` llave de grupo o 0 | Llave de proceso |
| 13 | `proceso_matar` | `x0` llave de proceso | 0 |
| 14 | `proceso_esperar` | `x0` llave de proceso | `x0` clase, `x1` código |
| 15 | `llave_duplicar` | `x0` llave, `x1` derechos, `x2` marca | Llave nueva |
| 16 | `llave_revocable` | `x0` llave | `x0` copia revocable, `x1` revocador |
| 17 | `revocar` | `x0` revocador | 0 |
| 18 | `grupo_crear` | `x0` presupuesto en bytes | Llave de grupo |
| 19 | `grupo_matar` | `x0` llave de grupo | 0 |
| 20 | `grupo_uso` | `x0` llave de grupo | `x0` límite, `x1` usado |
| 21 | `memoria_quitar` | `x0` dirección donde empieza una proyección | 0 |
| 23 | `registro_leer` | `x0` llave del registro, `x1` búfer, `x2` capacidad | Bytes leídos |
| 22 | `memoria_dma` | `x0` permiso DMA, `x1` tamaño | `x0` llave de memoria, `x1` dirección de bus |
| 25 | `registros_recortar` | `x0` llave de registros o de memoria del firmware, `x1` desde, `x2` tamaño (páginas enteras) | Llave nueva |
| 26 | `dma_liberar` | `x0` permiso DMA | Páginas devueltas |
| 27 | `memoria_tamano` | `x0` llave de memoria (con derecho de leer) | Bytes que mide (páginas enteras) |
| 24 | `temporizador_crear` | `x0` llave de un extremo de canal, `x1` primer plazo en µs, `x2` periodo en µs (0: una vez) | Llave del temporizador |
| 29 | `medir` | `x0` 0: núcleo de quien llama; 1, 2, 3: interrupciones, cambios de hilo o µs ociosos del núcleo `x1` | El número |

### Detalles

- **1, `escribir`, y la consola** (C48). Lo escrito va al registro del sistema, un anillo de 64 KB del kernel, y de
  ahí a la consola. Cada línea de un programa empieza por "│ ", así que nunca se confunde con una del kernel. Si el
  anillo está lleno, lo nuevo se pierde. Antes: va a la consola del sistema, que hoy es el puerto serie. El texto se interpreta como UTF-8, y
  cada byte que no lo sea sale como `?`. Los caracteres de control también, salvo el salto de línea y el tabulador.
  Es provisional: la consola será un servicio.
- **4, `canal_escribir`.**
  - Hasta 256 bytes de datos y 8 llaves por mensaje.
  - Las llaves enviadas salen de la tabla de quien escribe y entran en la de quien lee, con los mismos derechos.
  - Cada llave enviada necesita el derecho de transferir.
  - Sin ciclos (C40): no se puede mandar un extremo de este mismo canal (ni el propio ni su pareja), ni una llave de
    interrupción o de temporizador que avise a este canal. Da `E_ARGUMENTO`.
  - La llave por la que se escribe necesita el derecho de escribir.
  - Nunca bloquea: el mensaje se queda en la cola del otro extremo.
  - Una misma llave dos veces en la tabla devuelve `E_ARGUMENTO`, y no se manda nada.
  - Si el otro extremo está cerrado, devuelve `E_CERRADO` y las llaves vuelven a sus números de antes.
- **5, `canal_leer`.**
  - Si la cola está vacía, se bloquea hasta que llegue un mensaje o se cierre el otro extremo.
  - Las llaves recibidas toman números nuevos en la tabla de quien lee.
  - Si el mensaje no cabe en el búfer o en la tabla de llaves, devuelve `E_TAMANO`. Si no se puede escribir donde
    dice el proceso, devuelve `E_ARGUMENTO`. En los dos casos el mensaje vuelve al principio de la cola, con sus
    llaves.
- **3, 4 y 5, tablas de llaves.** Son tablas de números de 32 bits, en el orden de bytes del proceso.
- **6, `cerrar`.** Quita la llave de la tabla. Si era la última llave de un extremo de canal, ese lado se cierra:
  quien esté leyendo al otro lado se despierta con `E_CERRADO` cuando la cola queda vacía, y los mensajes que
  esperaban al lado cerrado se sueltan con sus llaves.
- **Al morir un proceso** (C40) se sueltan todas sus llaves, como si las cerrara una a una: sus extremos de canal se
  cierran y el otro lado se entera. Lo mismo pasa con un extremo que va dentro de un mensaje que ya nadie leerá.
- **7, `memoria_crear`.** Páginas nuevas de 4 KB, todas a cero. La llave tiene todos los derechos. Los últimos
  32 MB libres son del kernel: al llegar ahí devuelve `E_MEMORIA`.
- **8, `memoria_proyectar`.**
  - Proyecta el objeto entero a partir de la dirección. Si alguna página del tramo ya tiene algo, no proyecta nada y
    devuelve `E_ARGUMENTO`.
  - Permiso 3 es lectura y escritura, y nunca se ejecuta.
  - El mismo objeto proyectado en dos procesos es memoria compartida.
  - Sus páginas vuelven al sistema cuando no queda ninguna llave ni ninguna proyección.
- **9, `registros_proyectar`.**
  - Para controladores. La llave de registros solo la da el kernel, al lanzar el controlador.
  - Pide los derechos de leer y escribir.
  - Se proyecta como memoria de dispositivo: sin caché, sin reordenar y nunca se ejecuta.
  - También proyecta una llave de memoria del firmware (C50): la de la VideoCore, donde está la pantalla. Esa va como
    memoria normal sin caché. El kernel comprueba que no toca la RAM que reparte.
  - Los registros de los periféricos de la Pi son little-endian. Un proceso big-endian tiene que darles la vuelta.
- **10, `interrupcion_asociar`.**
  - Solo interrupciones de aparato (SPI, de la 32 a la 255, y con GICv3 las LPI de la 8192 a la 8255, que son las
    MSI que pasan por el ITS), y cada una con un único dueño. Las LPI se enmascaran en el kernel, no en el GIC: una
    que llega enmascarada se avisa al terminar la anterior. El mensaje es el número, 4 bytes.
  - El extremo de canal necesita el derecho de escribir.
  - Cuando llega la interrupción, el kernel la enmascara y escribe en el canal un mensaje de 4 bytes con su número,
    **siempre en little-endian**.
- **11, `interrupcion_terminar`.** La vuelve a habilitar cuando el controlador ya ha atendido el aparato.
- **10 y 11.** Las dos piden el derecho de escribir en la llave de la interrupción.
- **5, `canal_leer`, la marca** (C41). En `x2` devuelve la marca de la llave por la que se escribió el mensaje, o 0.
- **La marca 0** es "sin marca" y también la de lo que escribe el kernel (las interrupciones). Quien recibe una llave
  sin marca puede marcarla como quiera: un servidor que quiera saber quién le escribe solo debe dar llaves marcadas.
- **Varios lectores.** Con copias de una llave, varios hilos pueden leer el mismo lado: cada mensaje lo recibe uno, y
  al cerrarse el otro lado se despiertan todos.
- **15, `llave_duplicar`** (C41). Una copia del mismo objeto con los derechos pedidos que ya tenga la llave, nunca
  más. Si la llave no tenía marca y `x2` no es 0, la copia lleva esa marca; con `x2` a 0 conserva la suya. Marcar con
  otra una llave ya marcada da `E_DERECHOS`. Un lado de canal sigue abierto mientras quede alguna copia.
- **16, `llave_revocable`.** Una copia que pasa por un intermediario, y el revocador de ese intermediario. Las copias
  que se hagan de la revocable también pasan por él. Cada intermediario cuesta 256 bytes del cupo. Como mucho hay 4
  intermediarios uno dentro de otro, contando los de los revocadores; el quinto da `E_LIMITE`.
- **Grupos y cupos** (C42). Cada proceso es de un grupo con un presupuesto en bytes. Todo lo que crea se cobra ahí:
  - un objeto de memoria, por sus páginas;
  - su espacio, por sus tablas y sus páginas de imagen y pila;
  - cada proceso, 64 KB fijos (hilo, pila del kernel y tabla de llaves);
  - cada canal, unos 26 KB fijos (sus dos colas llenas).

  Al agotarse, la llamada da `E_MEMORIA`. Lo cobrado vuelve al grupo cuando se suelta el objeto, aunque lo tenga otro
  proceso. Los procesos que lanza el kernel son del grupo raíz, cuyo límite es la memoria libre para procesos al
  arrancar, menos la reserva del kernel.
- **18, `grupo_crear`.** Un subgrupo del propio con ese presupuesto, más 512 bytes del grupo en sí, que salen del
  propio: si no cabe, `E_MEMORIA`. Se devuelve al propio cuando ya no queda nada cobrado al subgrupo. Como mucho hay 7
  niveles bajo el raíz. Un grupo matado no admite procesos ni subgrupos nuevos.
- **12 con `x4`.** Con una llave de grupo con derecho de escribir, el proceso nuevo es de ese grupo; con 0, del de
  quien lo crea.
- **19, `grupo_matar`.** Pide el derecho de escribir. Mata a todos los procesos del grupo y de sus subgrupos, y cierra
  los dos lados de todos los canales que crearon (C44), aunque tengan llaves en otros grupos: quien los use después
  recibe `E_CERRADO`. Así se suelta todo lo que el grupo dejó, también los ciclos entre canales.
- **20, `grupo_uso`.** Pide el derecho de leer. Devuelve el presupuesto y lo usado.
- **17, `revocar`.** Pide el derecho de escribir en el revocador. Desde ese momento, la copia revocable y las suyas
  dan `E_LLAVE`, estén donde estén. El objeto se suelta: si era lo último que tenía un lado de canal, el lado se
  cierra. Si es memoria, se quitan las proyecciones que se hicieron con la copia revocable o con sus copias, en todos
  los procesos (C45); las hechas con la llave original siguen.
- **23, `registro_leer`** (C48). Para la consola: pide la llave del registro, que solo da el kernel a `inicio`. Saca
  hasta 4096 bytes del registro, esperando si no hay nada. La primera vez, el kernel deja el puerto serie: desde ahí
  solo escribe en el registro, salvo el pánico, que va directo.
- **22, `memoria_dma`** (C46). Pide una llave de permiso DMA con derecho de escribir; solo la da el kernel, a los
  controladores que la necesitan. Da un objeto de memoria de hasta 4 MB de páginas seguidas de la zona del permiso
  (el primer GB, el que alcanza el DMA; la de la GPU, de la zona normal), a cero, cobrado al cupo, y en `x1` la
  dirección de bus de su primera página, la que hay que dar al aparato. Se proyecta sin caché, también para leer: el
  aparato y el proceso ven lo mismo sin limpiar cachés. Al soltarse, va a la cuarentena del permiso (ver 26). Con otra
  llave, `E_TIPO`; con más de 4 MB, `E_TAMANO`.
- **21, `memoria_quitar`** (C45). Quita la proyección de un objeto de memoria que empieza en esa dirección: el tramo
  entero que se proyectó. Desde ese momento, leer o escribir ahí es un fallo. Si no empieza ninguna ahí,
  `E_ARGUMENTO`. Los registros de aparatos no se quitan así.
- **24, `temporizador_crear`** (C49). Pasado el primer plazo, y después cada periodo si no es 0, llega un mensaje al
  otro lado del canal: 8 bytes, un número de 64 bits en little-endian con las veces que ha vencido desde el último
  aviso que se pudo entregar (casi siempre 1; más si la cola del canal estaba llena o el aviso llegó tarde). Así se
  distingue de una interrupción, que llega en 4 bytes. Pide el derecho de escribir en el canal (`E_DERECHOS`); otra
  llave que no sea de canal da `E_TIPO`; un plazo o un periodo de más de un día, `E_ARGUMENTO`. Cuesta 4 KB del cupo
  del grupo. Resolución: la del contador de la CPU (M2); el periodo más corto, 100 µs. Nunca avisa antes de tiempo.
  Los avisos llevan la marca de la llave del canal con que se creó (C41); si esa llave era revocable y se revoca, el
  temporizador se para para siempre. Si la cola está llena, el aviso se cuenta para el siguiente; uno de una vez se
  reintenta en cada tic hasta que cabe. Se para al soltarse su última llave (`cerrar`, o al morir el proceso). Para
  esperar un rato sin gastar CPU: un temporizador de una vez y leer el canal. Lo mismo vale para `interrupcion_asociar`:
  sus avisos llevan la marca de la llave del canal y dejan de llegar si se revoca.
- **25, `registros_recortar`** (auditoría de la fase 4, M4). Una llave nueva del mismo tipo que solo cubre
  `[desde, desde + tamaño)` del tramo de la original, con sus mismos derechos. Los dos, múltiplos de 4 KB; fuera del
  tramo o tamaño 0, `E_ARGUMENTO`; otra llave, `E_TIPO`; una llave revocable, `E_ARGUMENTO` (la copia no se podría
  revocar). Así quien tiene un tramo grande da solo un trozo.
- **26, `dma_liberar`** (M5). La memoria DMA, al soltarse, no vuelve al sistema: queda en la cuarentena de su
  permiso, porque el aparato puede seguir escribiendo en ella (su controlador murió a medias). El controlador nuevo,
  con el aparato ya reiniciado, la devuelve con esta llamada. Mientras, sigue cobrada al grupo que la pidió. Pide el
  derecho de escribir en el permiso.
- **12, `proceso_crear`** (C39).
  - El ELF son los primeros `tamaño` bytes del objeto de memoria, que necesita el derecho de leer. Se copia al
    crear: después se puede cambiar o soltar el objeto.
  - Las llaves de la tabla (hasta 8, con derecho de transferir y sin repetir) salen de la tabla de quien crea y son
    las llaves 1, 2... del proceso nuevo. Si no se puede crear, vuelven a sus números.
  - El kernel elige el núcleo. La llave de proceso tiene todos los derechos.
  - Errores: `E_ARGUMENTO` si el ELF no vale, `E_TAMANO` si el tamaño es 0 o mayor que el objeto, `E_MEMORIA`,
    `E_LIMITE` si no cabe la llave nueva.
- **13, `proceso_matar`.** Pide el derecho de escribir. Vale para un proceso que corre, que espera o que ya acabó.
  Cada hilo muere en su siguiente vuelta a EL0 o al despertar si estaba bloqueado; la llamada no espera a eso.
- **14, `proceso_esperar`.** Pide el derecho de leer. Se bloquea hasta que acabe el último hilo del proceso.
  - Clase 0: salió con `salir`, y `x1` es el código.
  - Clase 1: lo mataron.
  - Clase 2: murió por un fallo.
  - Si ya había acabado, contesta al momento, todas las veces que se pregunte.
  - La llave de proceso no mantiene viva la memoria del proceso: se libera cuando acaba.

### Errores

| Valor | Nombre | Cuándo |
|---|---|---|
| −1 | `E_ARGUMENTO` | Dirección fuera del espacio de usuario o mal alineada; o la llamada viene de un hilo sin proceso |
| −2 | `E_LLAVE` | La llave no existe en la tabla |
| −3 | `E_DERECHOS` | Falta un derecho: leer, escribir o transferir |
| −4 | `E_CERRADO` | El otro extremo del canal está cerrado |
| −5 | `E_TAMANO` | Demasiados datos o llaves, tamaño 0 o más de 4 MB |
| −6 | `E_TIPO` | La llave es de otro tipo de objeto |
| −7 | `E_MEMORIA` | No queda memoria para procesos |
| −8 | `E_LIMITE` | Un límite: tabla de llaves llena, cola del canal llena o demasiadas proyecciones |

### Topes de trabajo

Una llamada corre entera sin que el kernel se interrumpa, así que cada una tiene un tope (desde C35):
`memoria_crear` pide como mucho 4 MB y `escribir` toma como mucho 256 bytes. Para más, se llama varias veces.

### Límites

Son límites de forma, aparte del cupo de cada grupo.

| Qué | Límite |
|---|---|
| Llaves en la tabla de un proceso | 1024 |
| Mensajes esperando en un lado de un canal | 16 |
| Proyecciones de objetos de memoria por proceso | 1024 |
| Memoria libre que solo usa el kernel | 32 MB |

Con la cola llena, `canal_escribir` devuelve `E_LIMITE` y las llaves vuelven a sus números. Una interrupción que
llega con la cola llena se pierde.

Un número de llamada desconocido devuelve `E_ARGUMENTO` y se avisa en la consola. La 99 solo existe en la imagen de pruebas (opción `pruebas`).

## Llaves y objetos

Una llave es un número de la tabla del proceso. El 0 no es nunca una llave válida. Cada llave apunta a un objeto y
lleva unos derechos:

| Derecho | Bit |
|---|---|
| Leer | 1 |
| Escribir | 2 |
| Transferir | 4 |

Un proceso nuevo no tiene ninguna llave. Solo tiene las que le da quien lo lanza, y esas ocupan los números 1, 2,
etc. El kernel solo lanza `inicio`, con las llaves de arranque; los controladores y demás servicios reciben las suyas
de `inicio` (ver "El programa de inicio" y `servicios.md`).

Tipos de objeto:

| Objeto | Se crea con | Para |
|---|---|---|
| Extremo de canal | `canal_crear` | Mensajes y paso de llaves |
| Memoria | `memoria_crear` | Proyectar y compartir |
| Registros | Solo el kernel | Controladores fuera del kernel |
| Memoria del firmware | Solo el kernel | La de la VideoCore, para el buzón, que da al compositor solo la pantalla (C50) |
| Interrupción | Solo el kernel | Controladores fuera del kernel |
| Proceso | `proceso_crear` | Matarlo (escribir) y esperar a que acabe (leer) |
| Revocador | `llave_revocable` | Revocar la copia (escribir) |
| Permiso DMA | Solo el kernel | Crear memoria DMA (escribir) |
| Registro | Solo el kernel | Sacar el registro del sistema (leer) |
| Grupo | `grupo_crear` | Crear procesos en él y matarlo (escribir), ver su uso (leer) |
| Temporizador | `temporizador_crear` | Avisos por un canal pasado un plazo; se para al cerrarlo |

## Biblioteca de usuario en Rust

`usuario/src/lib.rs` (crate `dashos`) envuelve las llamadas: `escribir`, `salir`, `canal_crear`, `canal_escribir`,
`canal_leer`, `memoria_crear`, `memoria_proyectar`, `registros_proyectar`, `interrupcion_asociar` e
`interrupcion_terminar`. También tiene `println!` y la macro `programa!(principal)`, que pone `_start` y el
manejador de pánico.

Los objetivos de compilación son dos:

- `aarch64-unknown-none-softfloat`: el nativo, que se usa por defecto.
- `aarch64_be-unknown-none-softfloat`: para la caja Amiga.

Los programas Dash para DashOS se compilan con `./dash --dashos` en la rama `aarch64` de FreeMEMLang.

### Registros que ve un proceso

- `TPIDR_EL0` es de cada hilo: empieza a 0 y el kernel lo guarda y lo repone. `TPIDRRO_EL0` vale 0.
- Puede leer el contador virtual (`CNTVCT_EL0`) y su frecuencia. El temporizador físico, los demás temporizadores y
  los contadores de rendimiento terminan el proceso.
- Saltar a una dirección del kernel termina el proceso.
- Las FIQ van enmascaradas. Un SError mientras corre el proceso lo termina.

## Limitaciones conocidas

- Si el montón del kernel se queda sin memoria, el kernel cae. Un proceso en un grupo con cupo no puede llevarlo
  ahí; los del grupo raíz (los que lanza el kernel) no tienen cupo.
- Una llave de registros da páginas enteras de 4 KB. La del controlador serie incluye, en la misma página, las UART 2
  a 5 de la BCM2711; son del mismo controlador. La del buzón incluye el controlador de interrupciones antiguo, el
  temporizador ARM y otros buzones de la BCM2711: falta comprobar en la Pi que no molestan con el GIC en marcha
  (auditoría de la fase 4, B8).
- `inicio` sondea la V3D al arrancar; en QEMU es un fallo de bus que se recoge. En la Pi, con el dominio de la V3D
  apagado, falta comprobar que leer no cuelga el bus (B9).
- Un temporizador puede dar un aviso más justo después de cerrar su última llave, si ya estaba saliendo (B10).
- Con la llave del canal revocable, un temporizador o una interrupción dejan de avisar también si se suelta ese
  intermediario (no solo si se revoca): no lo sujetan (segunda vuelta de la auditoría, C1).
- Un temporizador periódico que no se puede volver a programar por falta de memoria del kernel se para sin aviso.
- `dma_liberar` devuelve toda la cuarentena de un permiso de una vez; no hay forma de devolver un búfer suelto
  mientras el aparato sigue en marcha.
- El PCIe (C51): el VL805 alcanza todo el primer GB, también la memoria de la VideoCore; la llave de los registros
  del controlador PCIe no se da a ningún proceso.
- Un proceso tiene un solo hilo, y no hay llamada para crear hilos.
- El azar de los PIE sale del contador del sistema y no es criptográfico. En la Pi 4 habrá que sembrarlo con el
  generador por hardware (RNG200).
