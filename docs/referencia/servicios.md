# Servicios de DashOS

Lo que ve un programa de los servicios que lanza `inicio` (`kernel/src/inicio.rs`). Las llamadas al kernel están en
`kernel.md`.

## USB: teclado y ratón (C49)

`usuario/src/bin/usb.rs`. El controlador DWC2 como proceso. Llaves al lanzarlo:

| Llave | Qué es |
|---|---|
| 1 | Permiso DMA |
| 2 | Registros del DWC2 |
| 3 | Su interrupción (sin ella, consulta girando) |
| 4 | Un extremo de canal por el que manda los eventos (sin ella, los escribe en la consola) |

Al arrancar, con el DWC2 ya reiniciado, devuelve la cuarentena de su permiso (`dma_liberar`). Espera con la
interrupción del DWC2 y un temporizador de 10 ms; entre consulta y consulta no gasta CPU. Enumera el
hub y usa los teclados y ratones en modo de arranque (HID 1.11, apéndice B).

Cada evento es un mensaje de 8 bytes:

| Byte | Tecla | Ratón | Tableta (solo `xhci`) |
|---|---|---|---|
| 0 | 1 | 2 | 3 |
| 1 | Uso HID (página 7, teclado); los modificadores son 0xE0 a 0xE7 | Botones (bit 0 izquierdo, 1 derecho, 2 central) | Botones |
| 2 | 1 pulsada, 0 soltada | Movimiento x, con signo | x absoluta, de 0 a 32767 (2 bytes, little-endian) |
| 3 | 0 | Movimiento y, con signo | |
| 4 | 0 | Rueda, con signo (0 si el ratón no la manda) | y absoluta, de 0 a 32767 (2 bytes) |
| 5 | 0 | 0 | |
| 6 | 0 | 0 | Rueda, con signo |
| 7 | 0 | 0 | 0 |

La tableta es un puntero absoluto (la `usb-tablet` de QEMU, pantallas táctiles en modo ratón): `xhci` la reconoce por
su descriptor de informe (`usuario/src/hid_informe.rs`), sin modo de arranque. El compositor lleva el cursor a ese punto
de todo el espacio de las salidas.

Si la cola del canal está llena, el evento se pierde.

En la imagen normal, la llave 4 es el canal "entrada", cuyo otro lado tiene el compositor.

## Cómo los conecta `inicio` (C50)

Cada servicio recibe primero copias de las llaves de sus aparatos y después los extremos de sus canales, como llaves
1, 2... Un canal con nombre se crea la primera vez que se nombra: el primer servicio que lo nombra recibe un extremo y
el segundo, el otro. La lista está en `kernel/src/inicio.rs` (`PROGRAMAS`):

| Servicio | Aparatos | Canales |
|---|---|---|
| consola | registro, registros del puerto serie (en x86, los puertos de COM1) | — |
| buzon | su permiso DMA, registros del buzón, memoria de la VideoCore | "buzon" |
| usb | permiso DMA, registros e interrupción del DWC2 | "entrada" |
| xhci | su permiso DMA, registros del xHCI del PCIe, su INTx (opcional) | "entrada" |
| nvme | su permiso DMA, registros del NVMe del PCIe, su interrupción (opcional) | "disco" (es quien lee) |
| ahci (x86) | su permiso DMA, registros del AHCI (ABAR), su interrupción (opcional) | "sata" si hay NVMe; si no, "disco" (es quien lee) |
| disco-sata (prueba, x86) | — | "sata" (o "disco") |
| disco (prueba, solo `virt`) | — | "disco" |
| compositor | — (en la Pi); la pantalla de UEFI y sus medidas (por UEFI, también en x86) | "buzon" (en la Pi), "entrada" (es quien lee) |
| gpu | permiso DMA de la GPU, registros de la V3D, del puente ASB, del bloque PM y del cprman | "buzon" |
| sonido | permiso de plazo (S3) | "sonido" (es quien lee) |
| virtio-sonido | su permiso DMA, registros y configuración del aparato, permiso de plazo (S3), su interrupción (opcional) | "sonido" |
| hda (x86) | su permiso DMA, registros del controlador HDA, permiso de plazo (S3), su interrupción (opcional) | "sonido" |
| red (`virt` y x86) | — | "archivos" (cliente: lee `Prefs/Red`), y "red", "red-estado" y "red-escuchar" (es quien lee los tres) |
| hora (solo `virt`) | el permiso del reloj | "red" |
| virtio-red (`virt` y x86) | su permiso DMA, registros y configuración del aparato, su interrupción (opcional) | "red" |
| intel-red (x86) | su permiso DMA, registros (BAR 0) y configuración del aparato, su interrupción (opcional) | "red" |
| rtl-red (x86) | su permiso DMA, registros (BAR 2) y configuración del aparato, el firmware de su PHY (datos), su interrupción (opcional) | "red" |
| red-prueba (prueba, `virt` y x86) | — | "red" y "red-escuchar" |
| red-gloton (prueba, `virt` y x86) | — | "red" |

En cada canal el primero que lo nombra es el servidor; los demás reciben una copia marcada (su número de programa más
1) del otro extremo, y el servidor, un mensaje sin marca por cliente con su marca (8 bytes) y su nombre (16). Solo
`inicio` escribe sin marca, así que el servidor sabe quién le pide qué (auditoría de la fase 4, M6). Las llaves de
aparatos llegan en el orden en que el servicio las pide, y cada aparato con DMA tiene su propio permiso (M5). Llegan
con la restricción "una vez" (S4): el servicio no puede pasárselas a otro programa. Un servicio al que le falta algún aparato no se lanza: el kernel lo dice al arrancar
("inicio: gpu no se lanza: falta ..."). Es lo que pasa con la GPU en QEMU.

En x86-64 (X5 y X6) la lista es otra, `PROGRAMAS` de `kernel/src/inicio_x86.rs`, con las mismas llaves para cada
servicio: `consola`, `compositor`, `xhci`, `nvme`, `disco`, `ahci`, `disco-sata`, `archivos`, `ficheros`, `sonido`,
`virtio-sonido`, `hda`, `red`, `virtio-red`, `intel-red`, `rtl-red`, `red-gloton`, `red-prueba`, `tono-a`, `tono-b`, `ventana-a`, `ventana-b`,
`ajustes` y `hola` (X5c: el AHCI, el HDA y la red). Al AHCI le toca el canal "disco" si no hay NVMe; si lo hay,
"sata", y el resto de servicios del disco siguen en el NVMe. La consola recibe el registro y, en vez de los
registros del PL011, una llave de los puertos de COM1, que pide con `puertos_permitir` y usa con `in` y `out`. El
compositor recibe la pantalla del GOP y sus medidas, como por UEFI en ARM, y nombra antes que nadie el canal de
entrada. Las interrupciones de los aparatos son MSI-X.

### La tabla de llaves: qué es cada llave

Quien lanza un programa dice qué es cada llave que le da: `inicio` y `sesiones` lanzan con `proceso_crear` y los
nombres de las llaves (llamada 12 con `CON_NOMBRES`), el kernel los guarda y el programa los lee con la llamada 56
(`dashos::llaves::Tabla::leer`, `buscar`; en C, `dash_llave` de kit-c). Un programa no cuenta sus llaves ni supone
quién lo lanzó: busca "ventanas" o "selector" por su nombre. Un nombre mide 16 bytes, relleno con ceros; el primero
es el de la llave 1. Los números de siempre no cambian: los programas que usan un número fijo siguen igual.

| Quién lanza | Llave | Nombre |
|---|---|---|
| `inicio` | Un aparato | El del aparato en el paquete de arranque ("plazo", "dma-xhci0", "registro"...) |
| `inicio` | Un canal, para quien lo sirve y para sus clientes | El del canal en `PROGRAMAS` ("ventanas", "archivos", "sesiones", "selector", "sonido"...) |
| `inicio` | La copia para escribirse a sí mismo, de quien sirve un canal | "yo:" y el canal ("yo:ventanas") |
| `inicio` | La copia sin marca del canal de archivos que recibe `sesiones` | "archivos.altas" |
| `sesiones` | 1, su canal con `sesiones` | "sesiones" |
| `sesiones` | 2, archivos | "archivos" |
| `sesiones` | 3, el compositor | "ventanas" |
| `sesiones` | 4, el selector de ficheros (solo los programas de la persona del selector) | "selector" |

Lo que lanza un programa por su cuenta con `proceso_crear` sin nombres nace sin tabla. El programa no puede cambiar
la suya; lo que diga de sus llaves solo lo cree él.

## Selector de ficheros (U5)

`usuario/src/bin/selector.rs`, protocolo en `usuario/src/protocolo/selector.rs`. La ventana de "Abrir" y "Guardar
como": el programa pide, el usuario elige en una ventana que es del selector (el programa no la ve ni la maneja), y el
programa recibe solo lo elegido. Es el camino por el que un programa encerrado llega a un fichero del usuario (S6b).
Corre como una persona (en la prueba, Ana) y enseña lo suyo desde `C`.

**Quién lo tiene.** Los programas que lanza `inicio` con el canal "selector", y los que lanza `sesiones` como la
persona del selector (llave "selector" de su tabla). Los de otra persona y los del sistema, no (hasta que haya un
selector por sesión). El selector solo atiende a quien le presentan `inicio` o `sesiones` (sin marca: 8 bytes de
marca y 16 de nombre; con el nombre vacío, que se fue): a otro, `SIN_DERECHO`. El nombre que enseña en su ventana
("quién pide") es ese, no lo que diga el programa.

**Petición** (un mensaje con una llave: por dónde contestar):

| Bytes | Qué |
|---|---|
| 0..4 | "SEL2" |
| 4..8 | Orden (u32): 1 abrir, 2 guardar, 3 carpeta, 4 cancelar (sin llave ni respuesta), 5 consulta |
| 8..10 | Largo del nombre propuesto (u16; solo al guardar) |
| 10..12 | Largo del patrón (u16) |
| 12..16 | Ceros |
| 16.. | El nombre propuesto y, detrás, el patrón (UTF-8) |

**Respuesta**: estado (i32), asa (u32), banderas (u32), largo del nombre (u32) y el nombre elegido (UTF-8).

| Orden | Qué elige el usuario | Qué llega |
|---|---|---|
| abrir | Un fichero | Su asa, solo para leer (con ella no se escribe, recorta ni cambia nada, aunque el fichero sea del mismo usuario), y su nombre |
| guardar | Un cajón y un nombre (en el campo va el propuesto) | La asa del fichero, para leer y escribir, y su nombre. Si no existía, nace vacío. Si existía, el selector pregunta ("ya existe. ¿Sustituir?", con el botón "Sustituir"; Intro no sustituye) y llega como estaba, con la bandera 1 (`EXISTIA`): quien guarda lo deja del tamaño de lo que escribe (`TAMANO`) |
| carpeta | Un cajón | El hueco de raíz (de 15 a 8, el primero libre) en que queda cedido, para leer y escribir, y su nombre |
| consulta | Nada (sin ventana) | `BIEN` y el nombre con que el selector conoce a quien pide |

- **Solo el nombre, nunca el camino.** El programa sabe cómo se llama lo elegido (para su título, o para saber el
  formato por la extensión: es parte de lo que el usuario le entrega), pero no en qué cajón está ni cómo se llaman
  los cajones del usuario. Decisión tomada con S6b ("qué gana quien explota una aplicación": su cajón y los ficheros
  que el usuario le cedió, no el resto de `Home:`); pendiente de que el usuario la confirme.
- **Un nombre es uno solo**: sin '/', ':' ni '|', sin letras de control, ni "." ni "..", y de 107 bytes como mucho.
  Vale para lo que teclea el usuario (el selector lo dice bajo el campo y no guarda) y para lo que propone el
  programa (si no vale, el campo empieza vacío). El selector crea y busca por la asa del cajón y ese nombre: no hay
  camino de texto que se pueda torcer.
- **El patrón** solo decide qué ficheros se enseñan (los cajones, siempre): alternativas separadas por ';', con '*'
  (o "#?") y '?', sin distinguir mayúsculas ("*.txt;*.md"). No es una barrera. Como mucho 64 bytes; si no, se
  enseña todo.
- **Estados**: 0 bien; -1 mal formada; -12 el selector no conoce a quien pide; -100 cancelado (el usuario canceló,
  pasó un minuto sin que pulsara nada, o quien pidió mandó "cancelar" o se fue); -101 ocupado. Los demás negativos
  son los del servicio de archivos al ceder.
- **Una petición por programa**, y cuatro esperando como mucho: las demás, ocupado en el acto. Las ventanas salen de
  una en una, por orden. Quien pidió puede retirar su petición ("cancelar"); si acaba, `sesiones` lo dice y su
  ventana se cierra.

Las peticiones de antes siguen valiendo: el texto "abrir" (o cualquier otro) y "carpeta", con respuesta de 4 bytes
(la asa, o el hueco 3; 0 si no). Sin nombre ni patrón, y un minuto desde que sale la ventana. "raiz" (ceder `C`
entero para leer) es solo para la prueba del `hostil` de `inicio`: a otro, 0.

## Buzón de la VideoCore (C50)

`usuario/src/bin/buzon.rs`. Llaves: 1 su permiso DMA, 2 los registros del buzón (la página entera; el buzón está en
el desplazamiento 0x880), 3 la memoria de la VideoCore, 4 el canal de peticiones.

Cada petición es un mensaje de propiedades del firmware tal cual, de hasta 256 bytes: palabras de 32 bits en
little-endian con el tamaño, el código 0, las etiquetas y la etiqueta 0 de fin. Lleva una llave: un extremo de un canal
nuevo del cliente, por el que vuelve la respuesta, con el mismo tamaño y el código 0x80000000. Así varios clientes
comparten el canal de peticiones. Sin esa llave no se contesta; las llaves de más se sueltan, y un mensaje que no vale
no para el servicio (M6). Cada cliente, según su nombre en el directorio, puede pedir unas etiquetas u otras. Al
reservar la pantalla, la respuesta lleva además una llave de la memoria de la VideoCore recortada a ella (M4). Si el mensaje está mal formado o lleva una etiqueta que no está en la
lista, vuelve con el código 0x80000001 y el firmware no lo ve. Etiquetas permitidas:

| Etiqueta | Para | Quién |
|---|---|---|
| 0x00030002, 0x00030004 | Frecuencia de un reloj y su máximo | Todos |
| 0x00040001, 0x00040008 | Reservar la pantalla y su paso de línea | "compositor" |
| 0x00048003 a 0x00048006 | Tamaño físico y virtual, bits por píxel y orden de colores | "compositor" |
| 0x00048009 | Desplazamiento virtual (cambiar de búfer) | "compositor" |
| 0x00038001, 0x00038002 | Encender un reloj y ponerle la frecuencia: solo el de la V3D (reloj 5) | "gpu" |

## Compositor (C50)

`usuario/src/bin/compositor.rs`. Llaves: 1 el canal del buzón, 2 el canal de entrada. La pantalla le llega del buzón,
como una llave que solo cubre su memoria.
Pide al firmware una pantalla de 640 × 480 a 32 bits con dos búferes, pinta el fondo y una ventana, y repinta solo lo
que cambia. Cada movimiento del ratón mueve la ventana. Provisional: el de verdad hablará Wayland (S9).

`dashos_tema_v1` (protocolo propio, `wayland/protocolos/dashos-tema-v1.xml`), además del tema: Super+A al panel rápido
(evento `panel`); para la barra (S10), la petición `panel` (como Super+A; a otro, error de protocolo), Super sola
(evento `lanzador`) y el título de la ventana con el teclado (evento `titulo`, vacío con la sesión bloqueada). Las capas
sin zona exclusiva propia se colocan dentro de lo que dejan las barras; las ventanas maximizadas se reajustan si la barra
sale o se va.

Versión 4 (S27, `docs/diseño/S27-escritorio-dms.md`): **espacios de trabajo** del 1 al 9. Cada ventana es del espacio que
se veía al salir; las de los otros no se pintan, no reciben teclado ni ratón y sus avisos de `frame` salen a ritmo bajo,
como las de una sesión de detrás (U6). Super+1..9 cambia de espacio y Super+Mayúsculas+1..9 lleva allí la ventana con el
teclado (sin ir con ella); esas teclas no llegan a la ventana. A la barra, el evento `espacios` (el que se ve y los que
tienen ventanas, un bit por espacio) al suscribirse y al cambiar; la petición `espacio` (solo la barra; a otro, error de
protocolo; fuera de 1 a 9, nada) cambia de espacio. **Teclas del volumen** (HID 0x7F, 0x80 y 0x81): si el panel rápido
está suscrito, le llegan como el evento `volumen` (1, -1 o 0 para el silencio) y no a la ventana; si no, a la ventana.
Bloqueada la sesión, nada de esto. Los programas normales ven también la versión 3, pero solo la barra y el panel
reciben sus eventos.

Versión 5 (S27, oleada 2): petición `estado` (red: 0 sin servicio, 1 sin dirección, 2 conectada; nivel del volumen; si
hay silencio), solo del panel rápido; a otro, error de protocolo. El compositor la recorta (red hasta 2, nivel hasta
100), la guarda y se la da a la barra con el evento `estado`, al suscribirse y cada vez que cambia.

**Multimonitor.** Un `wl_output` por pantalla, cada uno con su global (6 la principal; 11, 12 y 13 las demás), su
nombre (DASHOS-1, DASHOS-2...) y su sitio en el espacio común (`geometry`; las pantallas van una al lado de otra, de
izquierda a derecha). Sin xdg-output: la escala es 1 y el tamaño lógico es el del modo. Si las pantallas cambian con
clientes dentro (llega el controlador gráfico), sus registros reciben `global` y `global_remove` y sus `wl_output` lo
nuevo con su `done`. Una capa o una pantalla de bloqueo van en la pantalla de su `wl_output`; sin él, o con uno que no
lo es, en la principal. Una pantalla sin pantalla de bloqueo se queda en negro mientras dura el bloqueo. Las ventanas
nuevas salen en la pantalla del cursor; Super+Mayúsculas+Izquierda/Derecha llevan la ventana con el teclado a la de al
lado; maximizar llena la de la ventana.

## Controlador de virtio-gpu (multimonitor)

`usuario/src/bin/virtio_gpu.rs`. Llaves: 1 su permiso DMA, 2 sus registros (BAR 4), 3 su espacio de configuración del
PCIe, 4 su interrupción si la hay, y después el canal "ventanas". Virtio 1.2 moderno, solo 2D. Hasta 4 cabezas; la que
no tiene monitor (QEMU sin ventana) se usa a 1024 × 768. Cada cabeza va al compositor con su memoria por el canal
"ventanas" (`usuario/src/protocolo/pantallas.rs`: `CABEZA`), y el compositor le pide por un canal suyo que vacíe lo que
cambió (`VACIAR`). El compositor solo acepta cabezas de quien `inicio` llama "virtio-gpu".

## GPU (C50)

`usuario/src/bin/gpu.rs`, con la biblioteca `dashos::graficos` (HVS, V3D, listas y búferes). Llaves: 1 el permiso DMA
de la GPU (desplazamiento 0: la V3D ve direcciones físicas), 2 los registros de la V3D (hub y núcleo 0), 3 el puente
ASB, 4 el bloque PM, 5 el cprman, 6 el canal del buzón. Enciende la V3D, prepara su MMU (tabla de 4 MB en memoria DMA)
y hace el primer trabajo: borrar 64 × 64 píxeles y comprobarlo. Después espera; el servidor de verdad atenderá a Mesa
(S9). **Sin probar**: QEMU no emula la V3D. El bloque PM también tiene el perro guardián: quien tiene esa llave puede
reiniciar la máquina, como puede leer toda la memoria con la GPU; es un servicio de confianza (S16, D).

El controlador del HVS está en la biblioteca, sin servicio propio: lo usará el compositor en la Pi 4 para componer con
planos por hardware.

## xHCI (U6)

`usuario/src/bin/xhci.rs`. Llaves: 1 su permiso DMA, 2 sus registros (la BAR 0 del aparato del PCIe, que busca el
kernel), 3 su interrupción si el kernel la sabe (MSI-X, o la INTx del `interrupt-map` del árbol), y después el canal
de entrada del compositor. Reinicia el controlador, da dirección a lo que haya en cada puerto y,
a los teclados y ratones de arranque, les lee los informes y los manda como eventos, con el mismo formato que `usb`.
Hoy sin hubs; sin interrupción (con ACPI), mira los eventos cada 10 ms.

El código está en `usuario/src/xhci_comun.rs`: `xhci`, `xhci0` y `xhci1` son el mismo controlador con otro nombre en el
registro. En la Pi 5 (opción `usb` del kernel), `xhci0` y `xhci1` llevan los dos xHCI del RP1: llave 1 su permiso DMA
(bus = física + 0x10_0000_0000, el `dma-ranges` del PCIe del RP1), 2 sus registros (64 KB), sin interrupción, y el canal
"ventanas" si hay compositor. Sin canal, escribe cada tecla y cada movimiento del ratón en el registro. Pide los búferes
de trabajo que diga el controlador (scratchpad, hasta 32) y, con PPC, da corriente a los puertos.

## NVMe (U6d)

`usuario/src/bin/nvme.rs`. Llaves: 1 su permiso DMA, 2 sus registros (la BAR 0, que busca el kernel por la clase
0x010802), 3 su interrupción si el kernel la sabe, y después el canal "disco". Al arrancar reconoce el disco (modelo, sectores), lee los dos primeros sectores y la tabla de particiones.

**Tramos y tabla de particiones** (A13, A14). Solo abren el disco los programas con tramo, por el nombre que les da
`inicio`, y cada uno ve su tramo como su disco entero: su sector 0 es el primero del tramo, los sectores de la
respuesta son los del tramo y no puede ni nombrar lo de fuera (-2, fuera del disco). `archivos` tiene la partición de
DashOS: en un MBR, la entrada del tipo 0x7F; en una GPT, la del tipo d0a18dbf-bbf4-4b0e-8627-ce5006b50017. `disco`, la
prueba en bruto, el primer MB sin llegar a la primera partición (con GPT, nada). `caja-linux` (H9), la partición de
la caja Linux, solo en GPT, del tipo d0a18dbf-bbf4-4b0e-8627-ce5006b50018: el disco de su invitado (Arch Linux ARM).
Nunca una partición de Linux cualquiera. Dos de la caja Linux son un error, como dos de DashOS. El lector
(`usuario/src/particiones.rs`, con sus pruebas en `archivos/herramientas`) no adivina: con entradas fuera del disco,
pisadas, dos de DashOS o un CRC de la GPT que no casa, no da tramo a nadie. La copia de la GPT del final no se mira.

Protocolo del canal "disco" (U6f). Petición, 24 bytes little-endian, con una llave: el extremo de escritura de un
canal del cliente (con derecho a transferir), por el que vuelve la respuesta.

| Bytes | Campo |
|---|---|
| 0-3 | orden: 0 abrir, 1 leer, 2 escribir, 5 vaciar la caché del disco, 6 descartar |
| 4-7 | sectores |
| 8-15 | primer sector |
| 16-19 | desplazamiento en la ventana (múltiplo de 4) |

Respuesta, 24 bytes: estado (0 bien; -1 mal formado o sin marca, -2 fuera del disco, -3 fuera de la ventana, -4 error
del disco, -5 sin sitio, -6 sin tramo), en 8-15 los sectores de su tramo, en 16-19 el tamaño del sector y en 20-23 el de la ventana.
Al abrir lleva además la llave de la **ventana**: 64 KB de memoria DMA (sin caché) del cliente, compartida con el
servicio, donde el disco lee y escribe directamente. Cada cliente se reconoce por la marca de su llave (la pone
`inicio`): un mensaje sin marca se rechaza, y abrir otra vez da la misma ventana. Hasta 4 clientes. Una transferencia
puede ocupar la ventana entera (128 KB desde A14: el servicio de archivos usa dos mitades, una para leer y otra para
leer por adelantado), con una lista de páginas; si el controlador admite menos (MDTS), dos páginas (8
KB desde un principio de página). La ventana lleva derecho a transferir, para que
el cliente pueda dársela a otro proceso suyo.

**Vaciar** (orden 5, FS2): sin sectores ni ventana. Cuando contesta, todo lo escrito antes está guardado en el disco,
también lo que estuviera en su caché. Un sistema de archivos lo pide antes y después de escribir su cabecera: sin esto,
un disco con caché puede guardar la cabecera antes que los bloques a los que apunta. Vale también en el anillo.

**Descartar** (orden 6, A14; TRIM): los sectores que se dan ya no guardan nada que importe y el disco puede soltarlos
(en NVMe, Dataset Management con "deallocate"). Sin ventana; las mismas comprobaciones que leer y escribir (dentro del
disco, que es su tramo, al menos un sector). Es un aviso: si el disco no sabe descartar, contesta bien sin hacer nada.
Vale también en el anillo, por orden con lo demás: un descarte seguido de una escritura del mismo sector deja lo
escrito. El servicio de archivos descarta lo que suelta cada guardado, en el envío del guardado siguiente.

**Anillo** (U6l, S1): con la orden 3 (tras abrir, con una llave de respuesta propia) el servicio da una página de
memoria compartida y se queda con esa llave para avisar. Cabecera de palabras de 32 bits: 0 cabeza de envíos (la
mueve el servicio), 4 cola de envíos (el cliente), 8 cabeza de terminadas (el cliente), 12 cola de terminadas (el
servicio), 16 "el servicio duerme", 20 "el cliente espera". Envíos de 32 bytes desde el byte 64 (orden 1 o 2,
sectores, primer sector de 64 bits, desplazamiento en la ventana, etiqueta); terminadas de 16 desde el 2048
(etiqueta, estado). 32 de cada. El cliente pone envíos, mueve la cola y, solo si el servicio duerme, le manda la
orden 4 (24 bytes, sin llave, sin respuesta). El servicio, si el cliente espera, le manda 4 bytes por su canal. El
servicio copia cada envío antes de comprobarlo. Hasta 16 sectores por envío.

Desde A15 el servicio manda al disco a la vez hasta 8 envíos seguidos del anillo (lecturas, escrituras y descartes) y
pone sus terminadas en el orden del anillo cuando acaba el lote. Lo que se pisa va detrás: el mismo sector, si uno
escribe o descarta, o el mismo sitio de la ventana, si uno lee. Vaciar va solo, con nada en vuelo. Para el cliente,
todo sale como si fuera de uno en uno.

Pendiente: varias colas de E/S del NVMe (una por núcleo).

## AHCI, discos SATA (X5c, x86)

`usuario/src/bin/ahci.rs` (Serial ATA AHCI 1.3.1). Llaves: 1 su permiso DMA, 2 sus registros (la BAR 5, ABAR), 3 su
interrupción (MSI) si el kernel la sabe, y después su canal: "disco" si no hay NVMe, "sata" si lo hay. Reinicia el
HBA (con el traspaso del firmware si lo tiene, CAP2.BOH), prepara cada puerto con un disco ATA (firma 0x101; ni
lectores ATAPI ni multiplicadores de puertos) y le pide IDENTIFY: hace falta LBA de 48 bits; sectores lógicos de 512 a
4096 bytes. Cada disco es un número, por orden de puerto (hasta 8). Órdenes sin cola (READ y WRITE DMA EXT, FLUSH
CACHE EXT, DATA SET MANAGEMENT con TRIM si el disco lo admite), hasta 8 en vuelo en ranuras distintas, cada una con
una entrada de PRD. Si el disco da un error, el puerto se recupera (parar, borrar errores, CLO o COMRESET) y lo que
estaba en vuelo vuelve con -4.

Sirve el mismo protocolo del canal "disco" que el NVMe (arriba), con el código común de `usuario/src/disco_servidor.rs`.
Lo que cambia: **al abrir (orden 0), los bytes 8-15 dicen qué disco** (0 el primero; el NVMe no mira ese campo). Abrir
otro disco cambia el disco del cliente, con la misma ventana; uno que no existe da -2. Cada disco tiene su tabla de
particiones, y los tramos por nombre son los del NVMe en ese disco, más `disco-sata` (la prueba), que ve lo mismo que
`disco`. El plazo de cada orden es de 10 s (un disco de verdad puede tener que arrancar el motor).

`disco-sata` (la prueba) abre los discos uno a uno y, solo en los de prueba (sector 0 con "DashOS: sector 0 del disco
SATA", `tools/disco_sata.py`), escribe, relee, vacía, descarta y usa su anillo. En uno de verdad solo lee.

Pendiente: NCQ (órdenes con cola), multiplicadores de puertos, lectores ATAPI, discos que se conectan en marcha, y
ahorro de energía del enlace. Sin probar en una máquina de verdad.

## Archivos (FS1, FS3, FS7, FS8, FS9, FS10)

`usuario/src/bin/archivos.rs` sirve un RedoxFS (`archivos/dashfs`, S5) que lee por el canal "disco": el de la
partición de DashOS del disco NVMe de `virt` (en los discos de prueba, desde el MB 1; A14). Llaves: 1 el canal "disco" y 2 el canal "archivos", donde lee. Cada
orden que cambia algo es una transacción de RedoxFS que queda en el disco, con la caché vaciada, antes de la respuesta
(FS4). Este protocolo es el de todos los sistemas de archivos de DashOS: los que vengan (FAT, ext4,
los del Amiga) lo cumplen igual, y `dos.library` habla con todos de la misma forma.

Cada cliente se reconoce por la marca de su llave (la pone `inicio`). Hasta 64 clientes.

**Fichas y permisos** (R6 de `docs/REVISION_LOOP.md`, U1 de `diseño/S6b-usuarios-y-permisos.md`). Antes que el
cliente llega, sin marca (solo escribe sin marca quien reparte las llaves, hoy `inicio`), su **alta**, orden 100:
derechos en la asa (bytes 4-7; 1 = administrar), su marca en a, usuario (16-19) y grupo (20-23) en b, y el cajón raíz
como nombre, desde la raíz del volumen principal. Tras el nombre pueden ir raíces de más (U2): hueco, tope (4 leer, 6
leer y escribir), largo del volumen y del camino (un byte cada uno), el volumen (vacío, el principal) y el camino; se
ponen al abrir la sesión. S28: con el hueco `HUECO_PAPELERA` (0x80), ese camino no es una raíz sino la **papelera** del
cliente: lo que borra (un fichero, no un cajón) va ahí, con su nombre o "nombre.N" si ya hay uno, en vez de borrarse. Queda
fuera de sus raíces (no la ve ni la vacía); si falta, se crea, suya y 0700. Hoy la tiene la caja Amiga
(`Estado/Servicios/caja-amiga.papelera`). S6, S36: con el bit `HUECO_SUYO` (0x40) en el hueco de una raíz, el último
cajón del camino se crea si falta (del cliente, 0755; los de antes que falten, del dueño de la raíz del volumen) y, si
ya está y no es del cliente, esa raíz no se da: así `sesiones` tiene su cajón en el `Home:Prefs` de una persona sin que
nadie se lo cambie por otro. Sin alta no hay sesión (-12). Con la orden 101, **baja**, también sin marca (la manda
`sesiones` cuando acaba algo que lanzó): se sueltan la sesión de esa marca (bytes 8-15), sus asas y raíces, y su alta.
Las fichas provisionales están en `usuario/src/bin/inicio.rs` (`FICHAS`). Con ellas:

**`RAM:` en memoria** (`bitacoras/asignaciones.md`). Cada sesión de usuario tiene un volumen en memoria: un DashFS
propio sobre un "disco" de páginas que se piden la primera vez que se escribe un bloque y vuelven al sistema cuando
DashFS lo descarta. Lo maneja `sesiones` con cuatro órdenes sin marca, como el alta, con una llave para la respuesta
(los 32 bytes de siempre):

| Orden | Mensaje | Hace |
|---|---|---|
| 102 crear | sesión (bytes 4-7: el usuario), uid y gid de su raíz (8-11 y 12-15); llaves: un objeto de memoria disperso (llamada 57 del kernel) que no tenga nadie más, y la respuesta | Un DashFS nuevo sobre ese objeto, que pasa a ser del servicio. Lo que mide el objeto es el tope del volumen; la memoria se cobra al grupo con el que se creó (el de la sesión). Su raíz es de ese usuario, 0700. En `a`, lo que mide; en `b`, lo que ocupa recién hecho. -9 si ya lo había, -6 con ocho a la vez, -12 si la memoria la tiene alguien más |
| 103 quitar | sesión | Lo quita con todo: las asas y las raíces que llevan a él dejan de valer para todos (-5) y su memoria vuelve. En `a`, lo que ocupaba. -2 si no lo había |
| 104 raíz | sesión, marca del cliente (8-15), hueco (byte 16) y tope (byte 17) | Da a ese cliente (y a su alta) la raíz del volumen en ese hueco: para el escritorio, que nace antes de que entre nadie |
| 105 copiar | sesión, uid y gid de lo copiado, largos de volumen, origen y destino (bytes 16, 17 y 18), solo crear el cajón (byte 19 a 1); los tres textos desde el byte 32 | Crea el cajón `destino` del volumen en memoria y copia en él el árbol `origen` del volumen del disco (vacío, el principal), pisando lo que se llame igual: así se monta `ENV:` con las capas de `ENVARC:`. Sin mirar permisos del origen; si no existe, no copia nada. En `a`, ficheros; en `b`, bytes. -11 si no cabe |
| 120 guardar ajuste | sesión, uid y gid (8-15), largos del volumen y del fichero (bytes 16 y 17); los dos textos desde el byte 32 | Ajustes en texto: copia `Env/<fichero>` del volumen en memoria de la sesión a `Prefs/Env-Archive/<fichero>` del volumen (el `Home:` del usuario), con los cajones que falten (0700) y el fichero 0600, suyos; lo que había se sustituye y va al disco al contestar. El fichero tiene que valer (`ajuste_valido`: no sale del cajón), ser un fichero y medir hasta 64 KB. Lo pide `sesiones` (`GUARDAR_ENV`). En `a`, los bytes. -2 si no está en `ENV:`, -1 si el nombre no vale |

En el alta, una raíz cuyo volumen empieza por el byte 0xff y sigue con la sesión (u32) es la del volumen en memoria de
esa sesión. Por sus asas todo va como en el disco, con tres diferencias: `INFO` da lo que mide y lo libre de ese
volumen; no hay instantáneas (-1); y quien crea o escribe recibe "lleno" (-11) cuando quedan menos de 64 KB además de lo
que escribe, para que siempre se pueda borrar. Lo borrado se borra de verdad (no va a la papelera).

- La asa 0 es la raíz del cliente, no la del disco. Por encima no se llega: no hay "cajón padre" ni se siguen enlaces.
- Las asas 1 a 15 son más raíces, cada una con su tope (leer, o leer y escribir). Las pone quien tiene el derecho de
  repartir con ceder (orden 30) a un hueco libre; el cliente no las cierra. Por una raíz se mira el permiso del
  fichero y además su tope, y lo que se abre desde ella hereda el tope. Las asas de buscar y crear empiezan en 16.
- Dueño, grupo y modo, como en Unix y sin paso libre para nadie (el usuario 0 es "sistema", sin privilegios): para
  pasar por un cajón al buscar hace falta su x; leer y listar, r; escribir y cambiar el tamaño, w; crear, guardar,
  borrar y renombrar, w y x en el cajón (o en los dos). En un cajón con el bit pegajoso solo borra o renombra el dueño
  de lo tocado o el del cajón. Comentar y proteger, solo el dueño. El bit setuid no da nada.
- Lo nuevo es del usuario del cliente y de su grupo (o del del cajón, si tiene setgid), con los permisos pedidos menos
  0o022. Un volumen nuevo, de quien lo hace.
- Con el derecho de administrar: comprobar (16), instantáneas (17 a 20) y volúmenes (21 con nombre, 22 a 26). Sin él,
  -12.

Un mensaje sin marca que no sea un alta se tira.
Todo va en little-endian.

Lo que lanza `sesiones` nace con sus raíces en el alta (paso 2 de las asignaciones): 0 el cajón de donde salió, 1 el
disco para leer, y si su usuario tiene cuenta, 2 su volumen, 3 su `RAM:` (el volumen en memoria de su sesión) y 4
`Work` (su `Work:`, provisional), las dos para leer y escribir.

**Petición**: 32 bytes de cabecera y, detrás, el nombre en UTF-8 (hasta 224 bytes; el mensaje entero, hasta 256).

| Bytes | Campo |
|---|---|
| 0-3 | orden |
| 4-7 | asa (0: la raíz del volumen principal) |
| 8-15 | a |
| 16-23 | b |
| 24-27 | largo del nombre |

**Respuesta**: 32 bytes por el canal de respuesta del cliente: estado (0-3), asa (4-7), a (8-15), b (16-23), c (24-31).
Estados: 0 bien, -1 mal formado, -2 no existe, -3 no es un cajón, -4 es un cajón, -5 asa mala, -6 sin sitio (de asas
o clientes), -7 error del disco, -8 solo lectura, -9 ya existe, -10 cajón no vacío, -11 disco lleno, -12 protegido
(sin permiso de escritura, o con "no borrar"; o un volumen cifrado cerrado, o una contraseña que no es), -13 en uso
(un volumen con algo abierto), -14 sin azar de calidad (para hacer llaves).

Cada asa es de un volumen (FS10): lo que se busca, crea o renombra desde ella queda en el suyo. La asa 0 es la raíz
del cliente; la orden 21 da la raíz de otro volumen.

| Orden | Qué | Pide | Devuelve |
|---|---|---|---|
| 0 sesión | Empieza; con una llave: el extremo de escritura del canal de respuesta | — | a: tamaño de la ventana; y su llave |
| 1 buscar | Abre lo que hay en `nombre` (partes separadas por "/"; vacío, el mismo) desde la asa. Con a = 1, solo sus datos, sin abrirlo (como `stat`; sin asa nueva, asa 0) | asa, a, nombre | asa nueva; a tamaño, b fecha (segundos), c modo \| tipo << 16 \| bits del Amiga << 32 |
| 2 leer | Lee del fichero al principio de la ventana | asa, a desde, b cuánto (hasta la ventana) | a bytes leídos, b tamaño del fichero |
| 3 escribir | Escribe en el fichero lo que hay al principio de la ventana | asa, a desde, b cuánto (hasta la ventana) | a bytes escritos, b tamaño del fichero |
| 4 listar | Pone en la ventana las entradas de un cajón desde la número a | asa, a desde | a cuántas, b la siguiente (o `u64::MAX` si no hay más), c bytes usados |
| 5 datos | Los datos de una asa | asa | como buscar |
| 6 cerrar | Suelta la asa | asa | — |
| 7 crear | Crea `nombre` (una parte) en el cajón de la asa y lo abre | asa, a: 1 fichero, 2 cajón; nombre | como buscar |
| 8 borrar | Borra `nombre` del cajón de la asa; un cajón, solo vacío | asa, nombre | — |
| 9 renombrar | Mueve `viejo` del cajón de la asa a `nuevo` en el cajón de la asa b. No sobrescribe; no mete un cajón en sí mismo | asa, b, nombre "viejo\0nuevo" | — |
| 10 tamaño | Recorta o alarga el fichero | asa, a tamaño nuevo | a tamaño |
| 11 vaciar | Que todo esté en el disco al contestar, como `fsync` | — | — |
| 12 info | Datos del volumen | — | a tamaño, b libre, c tamaño de bloque (bytes) |
| 13 comentario | Pone en la ventana el comentario (UTF-8, hasta 255 bytes) | asa | a su largo |
| 14 comentar | Cambia el comentario; vacío, lo quita | asa, nombre: el comentario | — |
| 15 proteger | Cambia los bits del Amiga y los permisos de Unix | asa, a bits, b permisos (hasta 0o7777) | — |
| 35 fechar | Cambia la fecha de modificación (solo el dueño) | asa, a segundos desde 1970, b nanosegundos (< 10⁹) | — |
| 16 comprobar | Comprueba el disco entero, sin cambiar nada (FS8); pone en la ventana los problemas, uno por línea, hasta llenarla | a: 1 = leer también los datos | a cuántos problemas, b bytes en la ventana, c bytes en uso |
| 17 instantáneas | Pone en la ventana las instantáneas del volumen de la asa, de la más vieja a la más nueva: fecha (8), largo del nombre (1) y el nombre; la siguiente en un múltiplo de 8 (FS9) | asa | a cuántas caben, b cuántas hay, c bytes usados |
| 18 tomar | Toma una instantánea del volumen de la asa | asa, nombre (hasta 64 bytes, sin ':' ni '/') | — |
| 19 descartar | Borra una instantánea y suelta lo que solo veía ella | asa, nombre | — |
| 20 volver | Vuelve el volumen de la asa a su instantánea más nueva, que se queda. Lo escrito después se pierde y las asas de todos los clientes en ese volumen dejan de valer, también sus raíces 1 a 15 en él, salvo las que son la raíz del volumen (S28) | asa, nombre | a el volumen |
| 21 volumen | Abre la raíz del volumen `nombre` (FS10); vacío, la raíz del cliente. Un cifrado cerrado se abre, pero no se lee | nombre | como buscar |
| 22 volúmenes | Pone en la ventana los volúmenes además del principal: número (8), marcas (1: 1 cifrado, 2 abierto), largo del nombre (1) y el nombre; el siguiente en un múltiplo de 8 | — | a cuántos caben, b cuántos hay, c bytes usados |
| 23 hacer volumen | Hace un volumen. Cifrado, con su primera contraseña en los b primeros bytes de la ventana (que se borran) y las llaves del azar del kernel (R10); sin azar de calidad, -14. Queda abierto | nombre; a: 0 sin cifrar, 1 cifrado, 2 cifrado con Argon2 barato (pruebas); b largo de la contraseña | a su número |
| 24 quitar volumen | Borra un volumen con todo lo que tiene, instantáneas incluidas; no con algo abierto | nombre | — |
| 25 abrir volumen | Abre un volumen cifrado con una de sus contraseñas, hasta que se cierre. La contraseña va en los b primeros bytes de la ventana, no en el mensaje (A13): el servicio la copia y la borra de la ventana. No se guarda | nombre, b largo de la contraseña (hasta 1.024) | — |
| 26 cerrar volumen | Olvida la llave de un volumen cifrado; no con algo abierto | nombre | — |
| 27 guardar | Crea el fichero `nombre` en el cajón de la asa con lo que hay al principio de la ventana, en una sola transacción, y en el disco al contestar. No lo deja abierto. No sobrescribe | asa, b cuánto (hasta la ventana), nombre | a bytes escritos, b tamaño |
| 28 leer por nombre | Busca `nombre` desde la asa y lo lee al principio de la ventana, sin dejar asa abierta (A3): buscar, leer y cerrar en un viaje | asa, a desde, b cuánto (hasta la ventana), nombre | a bytes leídos, b fecha, c tamaño total; asa 0 |
| 29 comprobar clave | Mira si la contraseña es la del volumen cifrado `nombre`, sin abrirlo ni tocarlo (U3: volver de un bloqueo). Solo con el derecho de administrar | b largo de la contraseña (en la ventana, como en 25), nombre | estado: 0 es la suya, -12 no |
| 40 crecer | Crece el sistema de archivos en uso hasta `a` bytes (S32): una transacción añade los bloques nuevos. Encoger no se hace en uso (es de `dashfs-resize`, sin montar). Solo con el derecho de administrar | a tamaño nuevo en bytes (0: todo el disco; si no es múltiplo de 4096, el múltiplo de antes) | a el tamaño que queda; -1 si es menos que ahora o más que el disco |
| 41 cuota | El tope del volumen `nombre` (S32; el principal no tiene) y lo que usa: lo suyo y lo que solo ven sus instantáneas. Pasado el tope, lo que lo haría crecer da -11 en ese volumen y en ningún otro; borrar y escribir encima valen siempre. Solo con el derecho de administrar | a tope en bytes (0: sin tope), b 1 para ponerlo (0: solo leer), nombre | a lo que usa, b su tope (bytes; a = 0 si aún no se ha contado) |
| 31 prestar | Presta las páginas de la caché del fichero (A7): hasta 4 llaves de memoria, solo de leer, de 512 KB cada una, con los trozos seguidos desde el que tiene el byte a. Se leen sin viajes y siguen al día con lo que se escriba. Dejan de valer al cerrar la asa, cuando el cliente pasa de 8 prestadas (las suyas más viejas) y al volver a una instantánea, comprobar, o quitar o cerrar un volumen. Un fichero de menos de 512 KB no se presta (-6): con leer (2). En Rust, `dashos::prestados` | asa, a desde | a el byte donde empieza el primer trozo, b tamaño del fichero, c cuántas llaves; y las llaves |
| 34 dueño | El dueño y el grupo de lo que tiene la asa, y el usuario de quien pregunta (S10: la Información del Workbench dice de quién es y si se puede cambiar) | asa | a dueño, b grupo, c el usuario del cliente |
| 80 vigilar | Avisos de cambio (`docs/bitacoras/ajustes-texto.md`, como `StartNotify` del Amiga): vigila lo que tiene la asa, un fichero o un cajón. Lleva una llave: la puerta por la que el servicio toca el timbre (16 bytes: `VIGI`, el número de la vigilancia u32 y cuántos cambios lleva u64). Un timbre por vigilancia hasta que se piden los avisos (82): una ráfaga es un timbre y una cuenta. Hace falta poder leerlo (un cajón, listarlo). Cuenta como cambio de un fichero escribir, recortar, comentar, proteger, fechar, renombrar o borrarlo; de un cajón, crear, borrar o renombrar dentro y cambiar un fichero de dentro abierto desde él. Si la puerta está llena, el timbre se reintenta; si está cerrada, la vigilancia se quita; si el cliente se va, se quitan todas. En Rust, `dashos::vigilar::vigilar` | asa; una llave | asa: el número de la vigilancia; a cuántas tiene el cliente, b cuántas hay en el servicio. -6 con 64; -12 si no se puede leer; -1 sin llave |
| 81 no vigilar | Deja de vigilar | asa: el número de la vigilancia | a cuántas le quedan. -2 si no es suya (o ya acabó) |
| 82 avisos | Pone en la ventana lo que cambió: 16 bytes por vigilancia con cambios (número u32; marcas u32: 1 cambio, 2 borrado, 4 fuera, 8 movido; cuántos cambios u64) y los pone a cero: el siguiente cambio vuelve a tocar el timbre. Las acabadas (borrado: lo vigilado se borró; fuera: su volumen se quitó, se cerró o volvió a una instantánea) salen una vez y se sueltan. Para ver siempre lo último: pedir los avisos y después leer | — | a cuántas entradas; b 1 si no cupieron todas |

**Cuándo está en el disco.** Cada orden que cambia algo se confirma entera o nada, pero en memoria: va al disco con
vaciar (11) o guardar (27), con las órdenes de instantáneas y volúmenes (18 a 20, 23 a 26), al juntarse 8 MB por
escribir, y si no, como mucho un segundo después. Cerrar no espera al disco (A1, como Linux). Si se va la luz antes,
el disco queda como tras el último guardado, entero: RedoxFS no reusa hasta entonces los bloques que soltó. Se puede
perder así hasta el último segundo; quien necesite más, vacía (11) o guarda (27).

**Si un guardado falla** (A2): cada cliente que tenía algo sin guardar recibe el error en su siguiente cerrar, vaciar
o guardar, y el disco pasa a solo lectura (-8 a todo lo que cambie algo) hasta que una comprobación (16) no encuentre
problemas.

Tipos: 1 fichero, 2 cajón, 3 enlace. El modo es el de Unix (permisos en los 12 bits bajos). Cada entrada de listar:
tamaño (8), fecha (8), modo (2), tipo (1), largo del nombre (1), bits del Amiga (2), largo del comentario (1), un byte
a 0, el nombre y el comentario; la siguiente empieza en un múltiplo de 8.

Los nombres no distinguen mayúsculas en las carpetas que lo tienen así (DashFS 2, FS6): hoy, todas las del disco.

**Bits del Amiga** (DashFS 3, FS7): 1 archivado, 2 puro, 4 guion, 8 residente, 16 no borrar (dueño), 32 no borrar
(grupo), 64 no borrar (otros). Leer, escribir y ejecutar son los permisos de Unix: `dos.library` los junta en los
bits de protección del Amiga. Escribir o cambiar el tamaño quita el bit de archivado, como en el Amiga. El servicio no
deja escribir ni cambiar el tamaño de un fichero sin permiso de escritura para quien lo pide, ni borrar uno con "no
borrar" (-12).

La **ventana** es memoria del servicio (64 KB) que el cliente proyecta: así el servicio sabe su tamaño y un cliente no
puede tumbarlo con una más pequeña. La sesión se puede abrir otra vez: da la misma ventana y cambia el canal de
respuesta. Si el cliente deja de escuchar, el servicio suelta sus asas y su ventana. Hasta 256 asas por cliente.

Lo borrado mientras alguien lo tiene abierto sigue leyéndose por esa asa hasta que se cierra; entonces se suelta. Si
el servicio se para antes, lo suelta al abrir el disco la vez siguiente.

Pendiente: la fecha de lo escrito (no hay reloj de la hora: va en 0), juntar escrituras (FS11), usuarios y permisos
(S6), cambiar si una carpeta distingue mayúsculas (para la caja Linux).

**Al arrancar** (A10), antes de atender a nadie, se comprueban los metadatos y, si hay algo mal, se repara en una
transacción (`archivos/dashfs/src/repair.rs`); si algo queda sin arreglar, el disco va de solo lectura.

## Sesiones (U2 a U6)

`usuario/src/bin/sesiones.rs` lleva las fichas de los procesos, los usuarios con la sesión abierta, los permisos de
cada programa y el registro de seguridad. Diseño: `diseño/S6b-usuarios-y-permisos.md`; protocolo:
`usuario/src/protocolo/sesiones.rs`. Llaves: 1 el permiso de la clase de plazo (para repartirlo), 2 su canal con
archivos, 3 `eco` sin marca (para repartirlo), 4 su canal, 5 una copia sin marca de su canal (de ella salen las llaves
con que nace lo que lanza) y 6 una copia sin marca del canal de archivos (las altas de lo que lanza). Su cajón,
`Estado/Servicios/sesiones` (usuario 1, 0700): `registro` (la cuenta y un anillo de 1.024 entradas, el de antes de S36,
que sigue leyendo el centro de control). Cuentas y frases, provisionales en el código (`CUENTAS`, `ADMIN`).

**El registro común** (S6, regla 2 de S36; rama `registro-permisos`). Texto en `Estado/Registros/` (cajón de
`sesiones`, 0700): `seguridad` (lo que decide `sesiones`: permisos dados, quitados y negados, acciones de
administrador, entradas, cierres), `sistema` (servicios que vuelven, quién topa con su tope de memoria, memoria baja,
lo que niega la guardia de la caja Amiga) y `programas` (lo que apuntan los programas). Una línea por hecho: hora (UTC),
programa, usuario, clase y el texto con su motivo; formato y limpieza en `usuario/src/registro.rs`. Solo `sesiones`
escribe, y solo al final; a los 64 KB el fichero pasa a `.1`. Lo escribe con un cliente propio de archivos (marca
aparte, raíz con `HUECO_SUYO`). Cada uno lee lo suyo y lo del sistema; todo, con la acción de administrador
"registro". Lo que un programa apunta lleva siempre su nombre y su usuario: no elige ni el fichero ni la clase (salvo
los del sistema, entre `registro::CLASES_DEL_SISTEMA`), y va con un ritmo por programa y usuario (`registro::RAFAGA`).

**Lo concedido** (S6b, regla 3 de S36). En texto: lo de cada persona en su `Home:Prefs/Permisos/concedidos` (se lee
al entrar y se escribe al cambiar; si su volumen está cerrado, cuando entre), y lo de los programas del sistema en
`Estado/Prefs/Permisos/concedidos` (`usuario/src/permisos_texto.rs`). El cajón y el fichero son de `sesiones`; el
dueño los lee. Un cajón o un fichero que no es de `sesiones` no se lee ni se escribe (queda en el registro). Lo de
antes, en `Estado/Servicios/sesiones/permisos`, se pasa a su sitio al arrancar y se borra.

`DELANTE` (21, S27): quién está delante (un i32 y 16 bytes con el nombre). Solo al panel rápido y a la barra, por el
nombre que da `inicio`; a otro, o sin nadie delante, -2 y nada más.

`MONITOR` (24, trasfondo): los procesos para el monitor del sistema, por páginas de 4 (desde la entrada que se diga, u32).
Cada entrada, 56 bytes: marca, µs de CPU desde que nació (llamada 52 sobre su grupo), memoria cobrada a su grupo,
usuario, grupo, clase (`M_PROPIO`, `M_CAJA_AMIGA`, `M_CAJA_LINUX`, `M_SISTEMA`, `M_OTROS`), cuántos y nombre.
Privacidad (decisión del usuario del 2026-11-13): con detalle, solo lo del usuario de quien pide; el fondo, la barra y
el centro miran por la persona de delante; las cajas, por su nombre; el resto del sistema y las demás personas, en dos
sumas sin marca ni nombres. La primera página, como mucho una vez cada 200 ms por programa (si no, `OCUPADO`, -3).
`MATAR` (2) desde el centro de control cierra por la persona de delante: solo lo suyo; sin nadie delante, nada. Los
programas del escritorio (fondo, centro, Workbench, explorador, selector, barra, panel y cajas, lanzados por `inicio`)
salen con el bit `M_ESCRITORIO` en la clase y `MATAR` no los cierra (-2), sea quien sea quien lo pida.

`APAGAR` (22; decisión del usuario del 2026-11-13): apagar (`MODO_APAGAR`), reiniciar (`MODO_REINICIAR`) o cerrar la
sesión de delante (`MODO_SALIR`). Solo al panel rápido, por el nombre que da `inicio`; a otro, -2 (y se apunta,
`R_APAGAR`). Apagar y reiniciar los hace `sesiones` con el permiso de apagar del kernel (llamada 51), que le manda
`inicio` sin marca (`PERMISO_APAGAR`, 23: no cabe entre sus 8 llaves; otra vez si se relanza); antes lo apunta en el
registro y lo guarda. Si hay otras sesiones abiertas además de la de delante, primero pregunta a `guardian` ("*apagar"
o "*reiniciar", contesta con `RESPUESTA`); sin `guardian`, no. Cerrar la sesión se lo pide a `guardian` ("*salir", como
`BLOQUEAR`), que hace `SALIR` y pone la pantalla de entrada. Contesta 0 (hecho o preguntado) o -2.

`inicio` da de alta cada programa (orden 1, sin marca: marca, usuario, grupo, nombre y la llave de su grupo) y pide
su aviso de fin hacia `sesiones` (la baja llega con la marca y el bit alto puesto). Los programas piden, con una llave
de respuesta:

| Orden | Quién | Qué |
|---|---|---|
| 1 procesos | cualquiera | La lista, por páginas de 7 fichas de 32 bytes (marca, usuario, grupo, nombre). Solo los de su usuario y los del sistema (usuario por debajo de 1000); `guardian`, todos (U6) |
| 2 matar | cualquiera | Mata el grupo de un proceso del mismo usuario; uno del sistema, -2 (para eso, la acción "matar"); uno de otra persona, -1, como si no existiera (U6) |
| 3 entrar, 4 salir, 5 volver | `guardian` | Abre o cierra el volumen cifrado del usuario, o comprueba su contraseña sin tocarlo. Entrar o volver con la buena pone su sesión delante; entrar en una ya abierta es volver (U6). Salir con programas suyos vivos, -13. Tras una contraseña mala, tarda 300 ms |
| 6 servicio, 7 dejar | cualquiera | La llave de un servicio o de un permiso (`eco`, `plazo`), marcada, revocable y de un solo viaje; o renunciar a ella. Sin permiso, se pregunta a `guardian`; desde una sesión de detrás, -2 sin preguntar (U6). S28: de la caja Amiga, también un permiso **sin llave** (`ejec:`, `icono:`, `guion:` o `disco:` y el programa 68k, con el programa entero y el fichero de detalle): se concede o se pregunta igual, sin llave; contesta el estado y cómo se decidió (`usuario/src/preguntas_ficheros.rs`). Una pregunta a la vez por programa |
| 8 registro | `guardian` | El registro de seguridad, por páginas |
| 9 lanzar | `guardian`; la barra y el Workbench (S10) | Un programa del disco, como quien pide o como un usuario que entró; nace con su canal con `sesiones`, su llave de archivos, la del compositor, la del selector de ficheros si es de la persona del selector, sus raíces y su tabla de llaves (ver "La tabla de llaves"). La barra y el Workbench solo lanzan de `Programas/` (un nombre, sin cajones) y el Shell nativo (`System/Shell`), siempre como la persona que está delante; sin nadie delante, -2. Si la petición trae una llave más, es la del canal de arranque del programa (`shell/src/protocolo.rs`; en su tabla de llaves, "arranque"), por donde quien lo lanza le da el cajón actual y los argumentos |
| 10 nombres | cualquiera | Su tabla de nombres (`SYS:`, `Programas:`, `Home:`, `PROGDIR:`), con la raíz de cada uno |
| 30 asignaciones | cualquiera | Su tabla de nombres entera (paso 2, `bitacoras/asignaciones.md`), por páginas de 248 bytes desde el byte que diga (u32): cuánto mide (u32), cuántos van (u32) y el texto, que lee `dashos::asignaciones::Tabla`. Volúmenes según sus raíces (lo que lanza `sesiones`, y el Workbench y el explorador, que se ceden las mismas): `SYS:` 1, y con cuenta `Home:` 2, `RAM:` 3 y `Work:` 4; `PROGDIR:` 0 si salió del disco; y sus asignaciones: las de la sesión (S4b) o las heredadas del que lo lanzó si es de su mismo usuario |
| 31 asignar | cualquiera | Cambia una de sus asignaciones (u32 cambio: 0 poner, 1 añadir detrás, 2 delante, 3 quitar; u32 largo; "Nombre=capa,capa"). Solo capas con nombre: ni raíces (`@n`), ni el nombre de un volumen o `PROGDIR`, ni caminos que suben, ni bucles (-30). Lo que lance después lo hereda. Contesta un i32 |
| 40 lanzar ayudante | un programa lanzado del disco (el Shell nativo) | Lanza otro programa **con su misma ficha** (`bitacoras/shell-nativa.md`): usuario, grupo, identidad (el nombre de la ficha: los permisos son suyos), raíces y asignaciones. El ELF lo manda quien pide, en un objeto de memoria (segunda llave, con leer): lo leyó él con sus permisos, y `sesiones` no abre nada por él. u32 lo que mide, u8 la raíz suya donde estaba (0xff, ninguna), u8 largo del nombre, u16 largo del cajón, el nombre y el cajón (hasta 80 bytes). Una tercera llave, si viene, es la cuarta del ayudante (su canal de arranque). Su `PROGDIR:` es la raíz 5, en ese cajón, con el tope de la raíz de origen. Hasta 32 procesos lanzados del disco por usuario. Contesta como lanzar |
| 11 preguntas | `guardian` | La llave por la que se le pregunta: permisos, acciones de administrador y bloquear |
| 12 respuesta, 13 retirar | `guardian` | Sí o no a un permiso (lo concedido se guarda; S28: 2 es "sí, solo esta vez", sin guardarlo); quitar uno concedido (se borra y se revocan sus llaves) |
| 14 administrar | cualquiera | Una acción de la lista cerrada ("comprobar", "matar", "registro": leer el registro de todos mientras viva quien la pidió), con la contraseña de un administrador que pide `guardian`. Desde una sesión de detrás, -2 (U6) |
| 15 respuesta de administrador | `guardian` | Usuario y contraseña para esa acción; se comprueban con su volumen |
| 16 bloquear | uno del usuario con la sesión delante | `guardian` bloquea la sesión con su pantalla |
| 18 activa | cualquiera | Si su sesión está delante (1), detrás (0) o no tiene (-2). U6: cambio rápido de usuario |
| 27 escuchar memoria | las cajas (`caja-amiga`, `caja-linux`) | aviso-cajas: con la llave de contestar y el lado de escribir de un canal que solo lee la caja. Por él le manda `sesiones` (nadie más tiene esa llave) el aviso de memoria baja del kernel: orden 28 `SOLTAR`, el nivel (1 que suelte lo que pueda; 0 normal), lo libre para procesos y lo que había al arrancar. A otro programa, -2, y el canal que da se cierra sin usar |
| 29 soltado | una caja apuntada | Lo que soltó por un aviso (u64, bytes). `sesiones` mide lo que tiene cobrado su grupo y lo apunta junto a lo que tenía al llegar el aviso. A otro, -2 |
| 90 guardar ajuste | el centro de control (`GUARDAN_AJUSTES`, por el nombre de `inicio`) | Ajustes en texto: guarda `ENV:<fichero>` en `ENVARC:` de la sesión de quien lo pide (nunca de otra: el mensaje no la nombra), por `archivos` (120). u32 largo y el nombre, que no sale del cajón (`ajuste_valido`). Contesta el estado (4 bytes): -2 si no puede, -1 sin la sesión abierta, -3 si el nombre no vale. Lo negado, al registro (`R_AJUSTE`) |
| 80 leer el registro | cualquiera | S6, S36: el fichero (u32: 0 seguridad, 1 sistema, 2 programas), la parte (u32: 0 el de ahora, 1 el `.1`) y desde qué byte (u64). Contesta el estado, cuántas veces rotó (u32), hasta qué byte llegó (u64), lo que miden las líneas (u64) y una llave de memoria solo para leer con las que ve: las de su usuario y las del sistema; todas, tras la acción "registro". La orden `Registro` |
| 81 apuntar | cualquiera | S6, S36: una línea en `programas` (el qué, el motivo y una clase que solo cuenta en los del sistema). La hora, el programa y el usuario los pone `sesiones`. Más deprisa que el ritmo, -3 (se cuenta y se dice en la siguiente) |
| 82 lo concedido, propio | cualquiera | S6b, S36: como 17, pero solo lo de su usuario. La orden `Permiso` |
| 83 dar o quitar un permiso | cualquiera, desde la sesión de delante; nunca un programa encerrado | S6b, S36: 1 dar o 0 quitar, el programa y el permiso. Lo confirma el usuario en `guardian` ("+dar" o "+quitar"; el detalle, el programa y el permiso); quitar revoca en el acto. 0, -1 (no existe o no lo tenía) o -2 (no). Rama `admin-desarrollo`: también `desarrollo`, el modo de desarrollo de una aplicación (no da llave ni se pide con 6) |
| 120 tanda | cualquiera, desde la sesión de delante | S36, decisión 1 (rama `admin-desarrollo`): una lista de acciones de administrador (`usuario/src/tanda.rs`), que es una sola acción. u32 lo que mide la lista; llaves: la de contestar y una memoria (con leer, hasta 64 KB) con la lista en texto, una acción por línea, 4096 bytes y 16 acciones como mucho. `sesiones` la copia al llegar y la lee entera: una línea que no vale, un proceso que no existe o una lista enorme, y no se pregunta ni se hace nada (-1 o -10). Si vale, pregunta a `guardian` ("*tanda"), que pide la lista en llano (121) y la contraseña una vez (15). Se hacen las de la lista, en orden, y se para en la primera que falle. Contesta 208 bytes (`tanda::Resultado`): estado, hechas, cuántas, en cuál se paró y estado y dato de cada una. Una sin contestar por programa (-3) |
| 121 lista de una tanda | `guardian` | La tanda (u32 número de la pregunta) en llano, desde el byte que diga (u32): estado, lo que mide, cuántos bytes van, cuántas acciones, el administrador que se propone (16 bytes: la persona de delante si es administradora) y el texto, una línea numerada por acción |
| 122 abrir | un programa de una persona con su sesión abierta, que no esté encerrado | S6b, S36 (rama `admin-desarrollo`): lanza un programa de `Programas/` **encerrado** (perfil "aplicación"), como con doble clic: con su identidad, raíces 0 (su cajón, `PROGDIR:`), 1 (`SYS:`, leer) y 10 (`Home:Datos/<programa>`, que se crea si falta y se ve como `Datos:`), sin `Home:`, `RAM:` ni `Work:`, y las asignaciones de la sesión sin la capa del usuario en `C:`, `LIBS:`, `FONTS:` y `CLASSES:`. Si su persona le dio `desarrollo`, además las raíces 11 a 14 (`Home:C`, `Home:Libs`, `Home:Fonts` y `Home:Classes`, solo leer) como `HomeC:`, `HomeLibs:`, `HomeFonts:` y `HomeClasses:`, delante en su asignación; el compositor marca sus ventanas y queda en el registro. Sus ayudantes (40) nacen igual de encerrados. La orden `Abrir`. Contesta como lanzar |

Las negativas (matar, servicio, entrar, lanzar, administrar, bloquear) y los procesos que mueren por un fallo quedan en
el registro de antes; eso y más (lo que se da y se quita, lo que se cierra, quién topa con su tope, lo que vuelve, la
memoria baja), en el registro común, en llano.

Quién es `guardian`, el centro o la barra lo dice el nombre que da `inicio` (S10): lo lanzado desde el disco se llama
como su fichero, y un `Programas/guardian` no tiene los poderes de `guardian`.

Cambio rápido de usuario (U6, `diseño/U6-cambio-rapido.md`): puede haber hasta 8 sesiones abiertas y una está
**delante**, la del último que entró o volvió. Las de detrás siguen en marcha con su volumen abierto, sin preguntar al
usuario, sin acciones de administrador y sin bloquear.

## Imágenes del Amiga (S20, fase 1)

`usuario/src/bin/imagen.rs` monta un ADF y lo sirve como un volumen de solo lectura por el canal "imagen", con el
protocolo de archivos de arriba. Es como un "handler" del Amiga: un proceso por imagen. Lee la imagen por el servicio
de archivos, como cualquier fichero (con una caché de 512 KB en trozos de 16 KB), y la entiende `amigados`: OFS y FFS,
normal, internacional y con caché de directorios (`DOS\0` a `DOS\5`), de 880 KB y de 1,76 MB. Llaves: 1 el canal
"archivos", 2 el canal "imagen".

Lo propio de las imágenes:

| Orden | Qué | Pide | Devuelve |
|---|---|---|---|
| 27 montar | Monta la imagen del camino `nombre` (en el volumen principal del servicio de archivos). Antes de montar, lo demás da -2; una segunda vez, -13 | nombre | a: bloques; b: 1 FFS, 2 internacional |

Diferencias con el servicio de archivos:

- **Solo lectura**: escribir, crear, borrar, renombrar, cambiar el tamaño, comentar, proteger, las instantáneas y los
  volúmenes dan -8. La capa de cambios es la fase 2.
- **Info** deja además el nombre del volumen en la ventana, en UTF-8 y acabado en 0.
- **Comprobar** lee el volumen entero (cajones y ficheros, con las sumas de cada bloque) y el mapa de bits, y pone en la
  ventana lo que falle.
- **Nombres**: en el disco van en Latin-1 y aquí en UTF-8; sin distinguir mayúsculas como el volumen (sin la variante
  internacional, solo a-z). Listar da el orden de la tabla de hash, como AmigaDOS.
- **Bits**: los permisos de Unix salen de RWED (los del dueño, y leer y ejecutar para los demás); los bits del
  Amiga, de A, P, S y H, y "no borrar" de D. Las fechas, de los días, minutos y tics desde 1978.
- **Enlaces**: los duros dan el tipo, el tamaño y los datos del original; los simbólicos, el tipo 3 y su destino al
  leerlos.
- Un bloque dañado (suma, tipo, número fuera del disco, cadena sin fin) da -7, nunca una caída.

Pendiente: los HDF (fase 3), escribir con la capa de cambios (fase 2), quién monta qué (hoy, quien tenga la llave del
canal) y un nombre de aparato (`IMG0:`) cuando haya `dos.library`.

## Fondo de escritorio (S10c, C3)

`usuario/src/bin/fondo.rs` pinta un PNG de `C/Fondos/` en la capa de fondo de layer-shell (tiene el poder de
escritorio), escalado para cubrir la pantalla (lo que sobra se recorta por igual a cada lado), y saca de él el color
semilla del tema (`kit::semilla`, el método de Material). Al arrancar pone el que dice `C/Prefs/fondo` (una línea con
el nombre; si no hay, `ondas.png`). Si la imagen no se puede leer o no se entiende, no pone nada (se ve el color liso
del compositor) o se queda la de antes. Lo que se deja: 32 MB de fichero y 16 millones de píxeles; más es "demasiado
grande", sin leerlo. En una pantalla de más de 4 MB (lo que mide un objeto de memoria), va en franjas, una capa cada
una. Llaves: 1 "ventanas", 2 "archivos", 3 su canal "fondo".

Por su canal solo le habla el centro de control (el nombre lo da `inicio`), en texto y con una llave para la respuesta:

| Pedido | Qué hace |
|---|---|
| `poner <nombre>` | Pone `Fondos/<nombre>`. No lo guarda: si sale bien, lo guarda el centro en `Prefs/fondo` |
| `semilla` | Dice el fondo de ahora |

La respuesta: estado (i32: 0 bien; -1 pedido malo o de otro; -2 no se lee el fichero; -3 no se entiende la imagen; -4
demasiado grande; -5 sin memoria; -6 sin pantalla; -7 no hay fondo), la semilla (ARGB; 0 si la imagen no tiene ningún
color que valga, como una gris), ancho, alto y los milisegundos que costó (u32 cada uno). Tras `semilla`, detrás, el
nombre del fondo que se ve.

Los colores del fondo los reparte el compositor (`dashos_tema_v1`, versión 2): el centro calcula una vez los dos
esquemas (claro y oscuro) de la semilla, los da con la petición `esquema` y pone el tema; a quien enlazó la versión 2
le llega el evento `esquema` antes de `tema`, y `kit::tema::recordar_evento_esquema` hace que `Tema::del_sistema` no
los calcule. Con la versión 1 se calculan (sin FPU, de 200 a 350 ms en QEMU), una vez por proceso y color.

La versión 3 (S26, `docs/diseño/S26-aspecto-de-las-cajas.md`) añade el aspecto de los programas de las cajas: la
petición `cajas` (solo el centro; el texto de `wayland::aspecto`, como mucho 2048 bytes; uno que no vale es un error de
protocolo) y el evento `cajas`, con el mismo texto ya comprobado, al enlazar (tras `tema`) y cada vez que cambia. Lo
escuchan la caja Amiga (sus plumas de Material) y, por el puente, el agente `dashos-aspecto` de la caja Linux. El
centro pone al arrancar la apariencia guardada y el aspecto de las cajas (`Prefs/apariencia`, `Prefs/cajas`); la de
serie es la paleta del fondo, claro, y todo como DashOS.

## Sonido (S19, A4)

El controlador de virtio-sound (`usuario/src/bin/virtio_sonido.rs`) pone en cada cola la entrada 0 de la tabla de
MSI-X (`queue_msix_vector`, virtio 1.2, 4.1.4.3): es la única que prepara el kernel. Con INTx no cuenta. En x86 (X5)
su interrupción es esa MSI-X; en ARM, la INTx.

El controlador de Intel HDA (`usuario/src/bin/hda.rs`, X5c, x86; Intel High Definition Audio 1.0a). Llaves: 1 su
permiso DMA, 2 sus registros (BAR 0), 3 el permiso de plazo, 4 su interrupción (MSI) si la hay, y el canal "sonido".
Reinicia el controlador y habla con los códecs por el CORB y el RIRB (sin interrupción: espera cada respuesta). En el
primer códec que tenga una salida elige la patilla de salida conectada que prefiera (línea, altavoz, auriculares) y
un DAC al que se llegue desde ella por mezcladores o selectores; enciende el camino (D0, la conexión elegida, los
amplificadores sin silencio a 0 dB, la salida de la patilla y EAPD) y también las demás patillas de salida que lleguen
al mismo DAC. El DAC va en estéreo de 16 bits a 48 kHz, flujo 1. Suena el primer flujo de salida del controlador con 8
búferes de 256 cuadros y una interrupción por búfer; con la salida y los tics del servicio igual que virtio-sound. Si
tras un parón hay más búferes que llenar que períodos tiene el servicio, le manda un tic y le espera hasta 2 ms antes
de poner silencio. Pendiente: la entrada (micrófono), el volumen por el amplificador, los avisos de clavija
(auriculares que se enchufan), HDMI y los códecs que necesiten arreglos propios. Sin probar en una máquina de verdad.

El servicio "sonido" lee el canal "sonido"; el controlador (virtio-sound o, desde X5c, HDA) y los programas le escriben con copias
marcadas, y `inicio` le dice de quién es cada marca. Mensajes de palabras de 32 bits (little-endian):

| Orden | Quién | Datos | Llaves |
|-------|-------|-------|--------|
| 1 `SALIDA` | solo "virtio-sonido" o "hda" (el primero que la mande; sus tics son los que cuentan) | ranuras (1 a 64), cuadros por período (16 a 4096), ritmo (48.000) | la memoria de la salida |
| 2 `TIC` | el controlador | | |
| 10 `ABRIR` | un programa | muestra (0 enteros de 16 bits, 1 de 32, 2 coma flotante), canales (1 o 2), ritmo (8.000 a 192.000) | la memoria del anillo y el extremo de escritura de un canal del programa |
| 11 `VOLUMEN` | el programa | izquierdo y derecho, como `f32` (0 a 4) | |
| 12 `CERRAR` | el programa | | |

**La salida** (del controlador): cabecera de palabras (escritos, leídos, ranuras, cuadros) y las ranuras desde el
byte 64, cada una un período en estéreo de 16 bits. El servicio mueve "escritos" y el controlador "leídos". Con cada
`TIC` (un búfer que devolvió el aparato) el servicio mezcla hasta tener 4 períodos por delante; si al controlador no
le queda ninguno, suena silencio y lo cuenta como vacío.

**El anillo** de un programa (`usuario/src/sonido_cliente.rs`): cabecera de palabras (cabeza: bytes escritos; cola:
bytes leídos; tamaño de los datos, potencia de dos) y los datos desde el byte 64. El programa mueve la cabeza y el
servicio la cola. El servicio no se fía de la cabecera: lo disponible no pasa del tamaño. Un flujo por programa.

Cuando el anillo baja de la mitad, el servicio escribe 4 bytes en el canal del programa para despertarlo (una vez
hasta que la cabeza se mueva). Si ese canal está cerrado, el programa murió: su flujo sale con un fundido de 5 ms. Al
`CERRAR`, el servicio suena lo que quede en el anillo y quita el flujo.

## Red (S13)

`usuario/src/bin/red.rs` es el servicio: la pila TCP/IP (smoltcp 0.14, 0BSD) en su proceso. `usuario/src/bin/virtio_red.rs`
es el controlador de virtio-net (PCI moderno; en QEMU, `disable-legacy=on`). El protocolo está en
`usuario/src/protocolo/red.rs` y el lado del programa en `usuario/src/red_cliente.rs` (`Conexion`: conectar, enviar,
recibir, esperar y cerrar; `estado`).

**El permiso.** Solo llega a la red quien tiene una llave del canal "red". Hoy la da `inicio`, por nombre, a quien la
pide en `PROGRAMAS`; más adelante la repartirá `sesiones`. El servicio sabe de quién es cada marca por `inicio` y no
atiende marcas que no conoce. Un programa solo toca sus conexiones.

**Escuchar es otro permiso** (`red-escuchar`, S6b): la llave del canal "red-escuchar". Lo atiende otro hilo del
servicio, que pasa al bucle principal lo que le llega (por la copia del servicio para escribirse en "red", con la marca
de quien lo mandó delante: `REENVIADO`; un programa que mande `REENVIADO` es rechazado, porque su mensaje no trae la
marca del servicio). Por ese canal valen `ESCUCHAR`, `UDP`, `AVISO`, `CERRAR` y `ESTADO`; lo demás se rechaza. Por el
canal "red", `ESCUCHAR` y `UDP` con un puerto elegido dan -13 (sin permiso). Quien tiene las dos llaves es el mismo
cliente para el servicio (la misma marca): sus topes cuentan juntos.

**El estado, solo para leer** (decisión del usuario del 2026-11-13). El canal "red-estado" lo atiende un hilo aparte del
servicio, que solo contesta `ESTADO` (las mismas palabras de `R_ESTADO`, sin `E_SUYAS` ni `E_RECHAZOS`) con una foto que
deja el bucle principal en cada vuelta. Cualquier otra orden se tira (y sus llaves se cierran): con esa llave no se
abre nada. La tiene el panel rápido, en vez de la de "red", y el fondo (el panel del monitor, trasfondo). Desde
trasfondo, `R_ESTADO` lleva también los bytes que entraron y salieron por la tarjeta (`E_ENTRAN`, `E_SALEN`: u64 en dos
palabras), también los de la caja Linux.

**El controlador.** Llaves: 1 su permiso DMA, 2 sus registros (BAR 4), 3 su configuración del PCIe, 4 su interrupción
(INTx en ARM, MSI-X en x86 desde X5c; opcional, sin ella mira cada 2 ms) y después el canal "red". Con MSI-X, cada cola
usa la entrada 0; detrás de una IOMMU acepta `VIRTIO_F_ACCESS_PLATFORM`. Funciones: VERSION_1 y MAC; sin sumas por el
aparato ni TSO. Una cola de recibir de 128 búferes de 2 KB y una de enviar de 64. Manda `TARJETA` (la MAC, una memoria
con 128 ranuras de recibir y 64 de enviar, y un canal suyo) y después `TRAMAS` cuando mueve algo; el servicio le manda
`TRAMAS` cuando pone algo para enviar. Copia cada trama entre la DMA y las ranuras: el servicio no toca los
descriptores. Tira y cuenta lo corto (menos de 14 bytes), lo largo (más de 1518), lo que no cabe en las ranuras y los
descriptores que el aparato devuelve sin tenerlos.

**El controlador de Intel** (`usuario/src/bin/intel_red.rs`, X5c, sala limpia con `docs/spec/hw/i219.md`): la MAC del
PCH con el PHY I219 (familia "Cannon Point", la de pegaso) y la tarjeta 82574L (la `e1000e` de QEMU), en un solo
programa. Las mismas llaves que virtio-red (registros de la BAR 0) y el mismo trato con el servicio, que acepta
`TARJETA` y `TRAMAS` de "virtio-red" o de "intel-red" (la primera tarjeta que se presenta). Descriptores clásicos, una
cola de recibir de 128 búferes de 2 KB y una de enviar de 64; la MAC de `RAL0`/`RAH0`; MSI-X en el 82574 (sus causas
en la entrada 0) y MSI en el I219; toda la multidifusión (filtra el servicio); sin control de flujo ni EEE. Tira lo
corto, lo largo, lo partido en varios búferes y lo que llega con error, y suma a esas cuentas lo que tira la tarjeta
misma (sus contadores). Dice en el registro cuándo sube o baja el enlace y a qué velocidad. Si el servicio se va,
para la tarjeta en orden y termina. Lo propio del I219 (ME, ULP, SMBus, NVM, erratas) está sin probar en hardware.

**El controlador de Realtek** (`usuario/src/bin/rtl_red.rs`, X5c, sala limpia con `docs/spec/hw/rtl8125.md`): la
RTL8125B (XID 0x641, la de shiva); otras versiones de la RTL8125 se dicen en el registro y no se tocan. Llaves: 1 su
permiso DMA, 2 sus registros (BAR 2), 3 su configuración, 4 el firmware del PHY (`rtl8125b-2.fw`, que el kernel lleva
dentro si el sistema donde se compiló lo tenía; tamaño 0 si no), 5 su interrupción (opcional) y después el canal
"red". El servicio acepta `TARJETA` y `TRAMAS` de "rtl-red" igual que de los otros dos. Valida el firmware entero antes
de aplicarlo (`usuario/src/rtl8125_fw.rs`) y sin él sigue, avisando. Descriptores de 16 bytes, 128 búferes de recibir
y 64 de enviar de 2 KB; la MAC de fábrica (`0x19E0`), que vuelve a dejar al parar; MSI-X en su entrada 0; toda la
multidifusión; sin control de flujo, sin EEE, sin ASPM. Rellena con ceros hasta 60 bytes al enviar y quita el CRC al
recibir; tira lo corto, lo largo, lo partido y lo que llega con error. Dice en el registro su versión, el PHY, el
firmware y el enlace (hasta 2500 Mb/s). Si el servicio se va, parada ordenada; con un error del bus, arranque en
marcha. **Sin probar en hardware** (QEMU no la emula; la prueba es pasándola con VFIO, `tools/vfio_red.sh`).

**El puerto de la caja Linux (H9).** `PUERTO` y `TRAMAS_PUERTO` (`protocolo::red`), solo de "caja-linux": un puente de
tramas para la tarjeta de su invitado (ver la caja Linux, abajo). Si la caja se va (su canal cerrado), el puerto se
suelta.
S29: el puerto pasa por el cortafuegos de la caja (`usuario/src/cortafuegos_caja.rs`): Internet sí; la red local no,
salvo DNS y ping a la puerta de enlace o al DNS y DHCP; ni multidifusión, ni ARP que se haga pasar por la puerta o
pregunte por otros aparatos; de la red local hacia el invitado, solo la puerta y el DNS. Las primeras 32 tramas
cortadas distintas se dicen en el registro ("[red] cortafuegos de la caja Linux: corta ...").

**Los programas.** Mensajes de palabras de 32 bits:

| Orden | Datos | Llaves | Respuesta |
|---|---|---|---|
| 1 `CONECTAR` | familia (4), puerto, tamaño de cada anillo (4 KB a 64 KB, potencia de dos), dirección IPv4 | un extremo de escritura para contestar | `R_CONECTAR` (1): conexión y estado; con `BIEN`, la llave de la memoria |
| 2 `AVISO` | conexión | | ninguna |
| 3 `CERRAR` | conexión | | ninguna; se manda lo que quede y se cierra |
| 4 `ESTADO` | | un extremo de escritura | `R_ESTADO` (4): dirección, puerta, DNS, conexiones, tramas y rechazos; la dirección IPv6, lo cortado por el cortafuegos, las cuentas del resolvedor y el servidor de hora |
| 5 `RESOLVER` | tipo (4: A, 6: AAAA; en la mitad alta, el puerto del servidor elegido), servidor (IPv4; 0: el del sistema), largo y nombre (hasta 240 bytes) | un extremo de escritura | `R_RESOLVER` (7): estado, cuántas direcciones (hasta 8), segundos que valen, de dónde salen (0 red, 1 caché, 2 hosts) y las direcciones (16 bytes cada una) |
| 7 `UDP` | puerto local (0: el que toque; uno elegido, solo por "red-escuchar"), tamaño de cada anillo | un extremo de escritura | `R_CONECTAR` (1) al momento, con la llave de la memoria |
| 8 `ESCUCHAR` (solo por "red-escuchar") | puerto TCP (1 a 49151), tamaño de cada anillo | un extremo de escritura | `R_ESCUCHAR` (8) al momento: conexión y estado; `R_CONECTAR` (1), con la memoria, cuando alguien entra |

`CONECTAR` admite IPv4 (familia 4) e IPv6 (familia 6, la dirección en 16 bytes).

Estados: 0 bien, -1 mal formado, -2 sin dirección, -3 límite, -4 rechazada, -5 cortada, -6 no existe, -7 familia que no
se sabe, -8 puerto ocupado, -9 el nombre no existe (o no tiene direcciones de ese tipo), -10 el servidor de nombres no
contesta, -11 el servidor de nombres falla, -12 respuesta truncada, -13 sin el permiso de escuchar. Como mucho 8
conexiones por programa (TCP, UDP y las que escuchan, juntas) y 32 en total; la novena da -3, las de los demás siguen.

**UDP.** La memoria es la de una conexión; en cada anillo van los datagramas seguidos, cada uno con una cabecera de 24
bytes: largo (u16), puerto (u16), familia (u8), tres bytes a 0 y dirección (16 bytes): a quién va o de quién viene.
Hasta 1472 bytes por datagrama. El tope de la cola de recepción es el anillo: lo que llega con él lleno se pierde y se
cuenta (palabra `PERDIDOS`); antes hay otras dos colas con su tope, las 16 del zócalo en la pila y las ranuras de la
tarjeta, que tiran sin contarlo ahí. Un zócalo abierto por "red" (sin el permiso de escuchar) solo recibe de las
direcciones y puertos a los que ha enviado en los últimos 120 s (hasta 16): lo demás se tira y se cuenta en `PERDIDOS`.
Abierto por "red-escuchar", recibe de cualquiera. Una cabecera imposible en el anillo de envío (familia que no hay,
puerto 0, más de 1472 bytes) deja el zócalo roto (`FASE` -1): ya no se sabe dónde empieza el siguiente. `Udp` en
`red_cliente`.

**Escuchar por TCP.** `ESCUCHAR` es un `CONECTAR` al revés: una conexión que espera a que alguien entre por ese puerto;
cuando entra, llega `R_CONECTAR` con la memoria y la cabecera dice quién es (`REMOTO_*`). Cada uno atiende a uno: para
varios a la vez se piden varios en el mismo puerto. Un puerto en que escucha un programa no lo puede usar otro (-8).
Mientras espera, el servicio manda latidos: si el programa acaba, deja de escuchar. `Oido` en `red_cliente`.

**Nombres.** El resolvedor va dentro del servicio (`Nombres`, con `usuario/src/dns.rs`): preguntas A y AAAA. Por este
orden: una dirección escrita se devuelve tal cual; `localhost` y `hosts` (`Prefs/Red/hosts` del disco del sistema, con
el formato de siempre; hasta 512 nombres; un nombre de `hosts` no se pregunta fuera); la caché (hasta 256 respuestas,
cada una lo que diga su TTL, con un día de tope; un "no existe" se guarda 30 s); y el servidor de nombres que dio el
DHCP, por UDP, con EDNS0 (1232 bytes). Tres intentos (esperas de 1, 2 y 4 s). Cada pregunta sale de un zócalo propio en
un puerto al azar, con un identificador al azar que cambia en cada intento; solo vale la respuesta que viene de la
dirección y el puerto del servidor, con ese identificador y la misma pregunta. Las demás se tiran y se cuentan
(`E_DNS_TIRADAS`), y se sigue esperando la buena. De la respuesta solo se usa lo que habla del nombre preguntado o del
final de su cadena de alias (hasta 8); los nombres comprimidos solo pueden apuntar hacia atrás. Hasta 16 preguntas a la
vez a los servidores; 8 en curso por programa y 64 en total (la siguiente da -3). Un programa puede elegir servidor y
puerto: esas respuestas se guardan aparte y no valen para quien pregunta al del sistema. Lo que falta: preguntar por
TCP cuando la respuesta viene truncada (da -12), DNS cifrado, servidores de nombres por IPv6, dominios de búsqueda,
nombres con letras que no son ASCII (IDN), DNSSEC y mDNS. `resolver` en `red_cliente`.

**Cortafuegos de entrada**, cerrado de serie (`usuario/src/cortafuegos.rs`). Cada trama para DashOS (o de difusión) se
mira antes de dársela a la pila: un intento de conexión TCP (SYN sin ACK) a un puerto en que nadie escucha, o un
datagrama UDP a un puerto que nadie tiene abierto, se tira sin contestar (ni RST ni ICMP) y se cuenta (`E_CORTADOS_TCP`,
`E_CORTADOS_UDP`; los seis primeros que venían a esta máquina se dicen en el registro). Abre un puerto TCP quien
escucha con el permiso; los UDP abiertos son los de los zócalos de los programas, el del DHCP y los de las preguntas de
nombres en curso. Lo demás (ARP, ICMP, segmentos de conexiones abiertas) lo mira la pila. No hay reglas configurables
todavía (`Prefs/Red`).

**IPv6.** La dirección local del enlace (fe80::/64 con la MAC) y, por SLAAC, la global del prefijo que anuncie el
encaminador, con su ruta. `CONECTAR`, UDP y escuchar valen con IPv6. Lo que falta: comprobar que la dirección no está
repetida (DAD), direcciones que no delaten la MAC (RFC 7217 o temporales), DHCPv6, los servidores de nombres que anuncia
el encaminador (RDNSS) y contestar al ping.

**Lo que se configura** está en `Prefs/Red` del disco del sistema (la raíz del servicio en el de archivos, solo lo lee):
`hosts` y `hora`, el servidor de hora: la primera línea que no está vacía ni empieza por `#`, con un nombre o una
dirección, o `ninguno` para no preguntar la hora a nadie. Sin fichero (o sin línea), `pool.ntp.org`. Si el fichero está
pero no se deja leer, o el servicio no puede leer sus ajustes (una máquina sin disco del sistema), no hay servidor de
hora: no se habla con un tercero sin saber qué dice el ajuste (`dashos::sntp::servidor`; el estado lo dice en
`E_HORA_APAGADA`). Las imágenes de prueba lo llevan apuntando al vecino de mentira (`tools/red_vecino.py`) o sin
disco, apagado. El servicio lo lee al arrancar; para que vea un cambio hay que relanzarlo.

**La hora por la red** (`usuario/src/bin/hora.rs`, con `usuario/src/sntp.rs`). `hora` es el único programa con el
permiso del reloj (llamada 39 con 1, `docs/referencia/kernel.md`); de la red tiene solo la llave de conectar. Pregunta
por SNTP (UDP, puerto 123) al servidor que el servicio de red da resuelto en su estado (`E_HORA`), pone el reloj de
golpe y vuelve a preguntar cada hora (si falla, a los 30 s y cada vez más despacio, hasta 10 minutos). La petición lleva
un número al azar en vez de la hora; solo vale la respuesta que lo devuelve, de un servidor con hora (ni estrato 0 ni
sin sincronizar) y posterior a 2024. La barra enseña la hora en la zona de `Prefs/zona` del disco del sistema (una
línea, como `Europe/Madrid`; la lee al arrancar por su raíz 1; sin ella, UTC), con `usuario/src/zona.rs`: UTC, las
zonas de Europa con la regla de la Unión Europea y unas cuantas sin cambio de hora. Las fechas y horas que escribe el
crate `idioma` (`Local`) usan la misma zona y la misma tabla (lee `SYS:Prefs/zona`); los minutos de `locale.prefs`
solo valen si no hay zona.

**La memoria de una conexión** la crea el servicio (cuenta en su cupo) y el programa recibe una copia que no puede
pasar. Así no puede dar una memoria revocable y quitársela al servicio con él dentro. Cabecera de palabras (cabeza y
cola del anillo de envío, cabeza y cola del de recepción, fase, "el programa duerme", "el servicio espera") y los dos
anillos desde el byte 64. Lo que mueve el servicio lo guarda él, no lo lee de la memoria. Cuando el programa duerme y
algo cambia, el servicio le manda `EVENTO`; cuando el servicio espera al programa, este le manda `AVISO`. Cada 2 s sin
mensajes, un `LATIDO`: si su canal está cerrado, el programa acabó y la conexión se corta.


## Avisos (S10)

El servicio de notificaciones, solo en `virt` por ahora. Canal "avisos"; el protocolo, en
`usuario/src/protocolo/avisos.rs`, y para mandar uno, `dashos::avisos_cliente::avisar(llave, urgencia, título, texto)`.

- La llave la da `inicio` (en las pruebas) o `sesiones` con el permiso "avisos" (`SERVICIO`), como la del sonido.
- `AVISAR`: urgencia (0 baja, 1 normal, 2 urgente), título (hasta 48 bytes) y texto (hasta 160). Contesta estado,
  número y el nombre con que sale. El nombre lo pone el servicio por la marca: el que da `inicio`, o el del fichero si
  lo lanzó `sesiones`. Estados: 0 bien, -1 mal hecho, -2 el servicio no sabe quién es, -3 demasiados seguidos (más de
  10 por minuto).
- Guarda 50, como mucho 8 de cada programa. Lo de una persona con su sesión detrás no sale hasta que vuelve.
- Leer, marcar como leídos y borrar: solo la barra.

## Caja Linux (H9)

El servicio `caja-linux` crea y corre máquinas virtuales (llamadas 45 a 48 de `kernel.md`). Solo hay hipervisor si el
kernel arrancó en EL2 (en QEMU, `virtualization=on`; `make arranque-virt-vm`).

**Llaves.** En `virt`: 1 "sistema" (la RAM de la máquina, si hay hipervisor y la RAM pedida para el invitado al compilar
el kernel, tres u64, 24 bytes), 2 y 3 la Image de Linux y su
initramfs si los hay (`make arranque-virt-linux`), y después los canales "ventanas" (su terminal), "disco" (la
partición de la caja Linux, el disco de Arch), "sesiones" (el permiso) y "red" (la red del invitado), en el orden en
que `inicio` crea los canales. En la Pi 4: "sistema" y el permiso, que da `inicio` (allí aún no hay
`sesiones`); solo prueba que un invitado arranca y acaba.

**El permiso** de máquinas virtuales lo reparte `sesiones` ("maquinas", decisión del usuario del 2026-11-12):
`inicio` solo se lo da a él. A la caja del sistema (el `caja-linux` de `inicio`) se lo da sin preguntar, lo apunta
como concedido (sale en el centro de control, que lo puede quitar; quitado, la próxima vez se pregunta) y en el
registro de seguridad; a cualquier otro se le pregunta a quien está delante. `sin-maquina` lo pide y no lo recibe.

**La memoria.** `inicio` le da la del invitado más 32 MB (`dashos::maquina::memoria_invitado`: 128 MB con menos de 2
GB de RAM, 256 MB con 2 GB, 512 MB con 4 GB y 1 GB con 8 GB o más); sin Image de Linux, solo los 32 MB. Si no cabe, la mitad para el
invitado, y así hasta 64 MB. Otra cantidad, al compilar el kernel: `DASHOS_CAJA_LINUX_MB=1024` (va en "sistema";
`memoria_invitado_con`: al menos 64 MB y como mucho la mitad de la RAM). En QEMU `virt`, con más de 3 GB hace falta
`highmem=on,highmem-ecam=off,highmem-mmio=off` (`MAQUINA_GRANDE` del Makefile): el kernel ve en `virt` hasta 16 GB.

**Aparatos del invitado** (crate `caja-linux/vmm`): PL011 (consola temprana), PL031 (la hora de DashOS; si el invitado
la cambia, cambia solo la suya), virtio-console (hvc0, 80x25), virtio-blk y, si el servicio de red le da un puerto,
virtio-net. El disco: con initramfs (busybox), 1 MB en memoria; sin él (Arch Linux ARM, `make arranque-virt-arch`), la
partición de la caja Linux que le da el servicio de disco, leída y escrita por su ventana de 128 KB en piezas de 64 KB,
con la vCPU parada mientras tanto. Con el canal "ventanas", hvc0 sale
en una ventana de terminal (un hilo del servicio, `usuario/src/terminal.rs` y Roboto Mono) y lo tecleado va a hvc0. Al
registro de DashOS va cada línea de la consola, sin secuencias de escape y callando si Linux escribe más de 100 líneas
por segundo seguidas (luego dice cuántas calló).

**La red del invitado.** El servicio de red le da un puerto (`PUERTO`, solo a "caja-linux"): un puente de tramas hacia
la tarjeta de verdad, con una MAC propia (02:da:50:...). Llegan al invitado las tramas para su MAC y las de difusión;
del invitado solo salen las que llevan su MAC de origen. Y pasan por el cortafuegos de la caja (S29, arriba en el servicio de red). Las ranuras las crea el servicio (64 y 64 de 2 KB). En QEMU,
el invitado es 10.0.2.16 y su puerta de enlace 10.0.2.2 (busybox, con dirección fija; Arch la pide por DHCP al
servidor de QEMU, que también contesta a la MAC del invitado, y le da la 10.0.2.16).

Los invitados de prueba de `usuario/invitados/*.s` y sus hostiles siguen corriendo antes de Linux. Bitácora:
`docs/bitacoras/caja-linux.md`.

## Caja Amiga (H4, H5)

`caja/` (C, con el JIT de Emu68, y `caja/pegamento/`, Rust): un proceso big-endian que ejecuta programas 68k con exec,
dos, graphics, intuition, utility y timer.device nativos. Corre programas de Dash (FreeMEMLang), por los dos caminos del
compilador. Solo en `virt`. Llaves, de `inicio`: 1 el permiso de página 0 (ExecBase en la
dirección 4; llamada 49 de `kernel.md`), 2 el canal "ventanas" y 3 el canal "archivos". No sirve nada: es cliente de
los dos.

- **Archivos**: su ficha en `inicio` (`FICHAS`) es la de Ana (1000), con la raíz del disco. dos.library la monta como
  `SYS:`, de solo lectura: el adaptador de la caja no manda al servicio nada que escriba (la ficha dejaría). La ventana del servicio va por encima de
  4 GB, donde el 68k no llega.
- **Ventanas**: un cliente de Wayland como los demás. Cada ventana de Intuition es un `xdg_toplevel` con un búfer
  `wl_shm` XRGB8888 del tamaño de la ventana entera; los bordes y gadgets de Intuition van dentro del búfer. El
  compositor la coloca en cascada y la nombra "caja-amiga". El tamaño de la pantalla de Workbench es el modo actual
  del primer `wl_output`.

Bitácora: `docs/bitacoras/caja-amiga.md`.
