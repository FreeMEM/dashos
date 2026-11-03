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
  (lectura y escritura, sin ejecución). Si no, es de solo lectura. Dos segmentos no deben compartir página.
- Si hay `PT_INTERP`, el ELF se rechaza. No hay enlazador dinámico.
- **El kernel no aplica reubicaciones.** Un PIE aplica sus `R_AARCH64_RELATIVE` al arrancar, y tienen que caer en un
  segmento escribible. Su base la saca de su propio contador de programa (`adr`/`adrp`).
- La pila mide el `p_memsz` del `PT_GNU_STACK`, redondeado a páginas, entre 4 KB y 8 MB. Sin `PT_GNU_STACK`, mide
  64 KB.
- Al arrancar, `SP` apunta al tope de la pila y `PC` a la entrada. Los demás registros valen cero. No hay
  argumentos, entorno ni vector auxiliar.

### Mapa de direcciones de un proceso

El espacio de usuario va de 0 a 512 GB (`1 << 39`). Hoy está así:

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

Los procesos de usuario van en la clase justa: reparto por tiempo virtual, con un tic de 10 ms. La clase de plazos de
S2 existe, pero hoy solo la usa el kernel. Un proceso corre en el núcleo donde se creó y no cambia de núcleo.

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
| 1 | `escribir` | `x0` texto, `x1` longitud (se corta a 4096) | Bytes escritos |
| 2 | `salir` | `x0` código | No vuelve |
| 3 | `canal_crear` | — | `x0` y `x1`: las llaves de los dos extremos |
| 4 | `canal_escribir` | `x0` llave, `x1` datos, `x2` longitud, `x3` tabla de llaves, `x4` cuántas | 0 |
| 5 | `canal_leer` | `x0` llave, `x1` búfer, `x2` capacidad, `x3` tabla de llaves, `x4` capacidad de la tabla | `x0` bytes, `x1` llaves recibidas |
| 6 | `cerrar` | `x0` llave | 0 |
| 7 | `memoria_crear` | `x0` tamaño (hasta 64 MB) | Llave de un objeto de memoria |
| 8 | `memoria_proyectar` | `x0` llave, `x1` dirección (alineada a 4 KB), `x2` permisos: 1 leer, 2 escribir | 0 |
| 9 | `registros_proyectar` | `x0` llave de registros, `x1` dirección (alineada a 4 KB) | 0 |
| 10 | `interrupcion_asociar` | `x0` llave de interrupción, `x1` llave de un extremo de canal | 0 |
| 11 | `interrupcion_terminar` | `x0` llave de interrupción | 0 |

### Detalles

- **1, `escribir`.** Va a la consola del sistema, que hoy es el puerto serie. El texto se interpreta como UTF-8, y
  cada byte que no lo sea sale como `?`. Es provisional: la consola será un servicio.
- **4, `canal_escribir`.**
  - Hasta 256 bytes de datos y 8 llaves por mensaje.
  - Las llaves enviadas salen de la tabla de quien escribe y entran en la de quien lee, con los mismos derechos.
  - Cada llave enviada necesita el derecho de transferir. Un extremo no puede enviarse por sí mismo.
  - La llave por la que se escribe necesita el derecho de escribir.
  - Nunca bloquea: el mensaje se queda en la cola del otro extremo.
- **5, `canal_leer`.**
  - Si la cola está vacía, se bloquea hasta que llegue un mensaje o se cierre el otro extremo.
  - Las llaves recibidas toman números nuevos en la tabla de quien lee.
  - Si el mensaje no cabe en el búfer o en la tabla de llaves, devuelve `E_TAMANO`. Si no se puede escribir donde
    dice el proceso, devuelve `E_ARGUMENTO`. En los dos casos el mensaje vuelve al principio de la cola, con sus
    llaves.
- **3, 4 y 5, tablas de llaves.** Son tablas de números de 32 bits, en el orden de bytes del proceso.
- **6, `cerrar`.** Quita la llave de la tabla. Si es un extremo de canal, lo cierra: quien esté leyendo al otro lado
  se despierta con `E_CERRADO` cuando la cola queda vacía.
- **7, `memoria_crear`.** Páginas nuevas de 4 KB, todas a cero. La llave tiene todos los derechos.
- **8, `memoria_proyectar`.**
  - Proyecta el objeto entero a partir de la dirección. Si ya había algo proyectado ahí, se pisa la entrada de la
    tabla sin vaciar la TLB: el proceso puede seguir viendo lo de antes un rato. No proyectéis encima de algo.
  - Permiso 3 es lectura y escritura, y nunca se ejecuta.
  - El mismo objeto proyectado en dos procesos es memoria compartida.
  - Sus páginas vuelven al sistema cuando no queda ninguna llave ni ninguna proyección.
- **9, `registros_proyectar`.**
  - Para controladores. La llave de registros solo la da el kernel, al lanzar el controlador.
  - Pide los derechos de leer y escribir.
  - Se proyecta como memoria de dispositivo: sin caché, sin reordenar y nunca se ejecuta.
  - Los registros de los periféricos de la Pi son little-endian. Un proceso big-endian tiene que darles la vuelta.
- **10, `interrupcion_asociar`.**
  - Solo interrupciones de aparato (SPI, de la 32 a la 255), y cada una con un único dueño.
  - El extremo de canal necesita el derecho de escribir.
  - Cuando llega la interrupción, el kernel la enmascara y escribe en el canal un mensaje de 4 bytes con su número,
    **siempre en little-endian**.
- **11, `interrupcion_terminar`.** La vuelve a habilitar cuando el controlador ya ha atendido el aparato.

### Errores

| Valor | Nombre | Cuándo |
|---|---|---|
| −1 | `E_ARGUMENTO` | Dirección fuera del espacio de usuario o mal alineada; o la llamada viene de un hilo sin proceso |
| −2 | `E_LLAVE` | La llave no existe en la tabla |
| −3 | `E_DERECHOS` | Falta un derecho: leer, escribir o transferir |
| −4 | `E_CERRADO` | El otro extremo del canal está cerrado |
| −5 | `E_TAMANO` | Demasiados datos o llaves, tamaño 0, más de 64 MB, o sin memoria |
| −6 | `E_TIPO` | La llave es de otro tipo de objeto |

Un número de llamada desconocido devuelve `E_ARGUMENTO` y se avisa en la consola.

## Llaves y objetos

Una llave es un número de la tabla del proceso. El 0 no es nunca una llave válida. Cada llave apunta a un objeto y
lleva unos derechos:

| Derecho | Bit |
|---|---|
| Leer | 1 |
| Escribir | 2 |
| Transferir | 4 |

Un proceso nuevo no tiene ninguna llave. Solo tiene las que le da quien lo lanza, y esas ocupan los números 1, 2,
etc. Hoy el kernel lanza los controladores con:

- La llave 1: los registros del aparato.
- La llave 2: su interrupción.

Tipos de objeto:

| Objeto | Se crea con | Para |
|---|---|---|
| Extremo de canal | `canal_crear` | Mensajes y paso de llaves |
| Memoria | `memoria_crear` | Proyectar y compartir |
| Registros | Solo el kernel | Controladores fuera del kernel |
| Interrupción | Solo el kernel | Controladores fuera del kernel |

## Biblioteca de usuario en Rust

`usuario/src/lib.rs` (crate `dashos`) envuelve las llamadas: `escribir`, `salir`, `canal_crear`, `canal_escribir`,
`canal_leer`, `memoria_crear`, `memoria_proyectar`, `registros_proyectar`, `interrupcion_asociar` e
`interrupcion_terminar`. También tiene `println!` y la macro `programa!(principal)`, que pone `_start` y el
manejador de pánico.

Los objetivos de compilación son dos:

- `aarch64-unknown-none-softfloat`: el nativo, que se usa por defecto.
- `aarch64_be-unknown-none-softfloat`: para la caja Amiga.

Los programas Dash para DashOS se compilan con `./dash --dashos` en la rama `aarch64` de FreeMEMLang.

## Limitaciones conocidas

- Proyectar encima de algo ya proyectado no vacía la TLB.
- Al morir un proceso, sus extremos de canal no se cierran. Quien espera al otro lado no se entera.
- Un proceso tiene un solo hilo, y no hay llamadas para crear hilos ni procesos, ni para esperar a que acaben.
- El azar de los PIE sale del contador del sistema y no es criptográfico. En la Pi 4 habrá que sembrarlo con el
  generador por hardware (RNG200).
- La consola de `escribir` es el puerto serie del kernel.
