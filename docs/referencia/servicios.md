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
| consola | registro, registros del puerto serie | — |
| buzon | su permiso DMA, registros del buzón, memoria de la VideoCore | "buzon" |
| usb | permiso DMA, registros e interrupción del DWC2 | "entrada" |
| xhci | su permiso DMA, registros del xHCI del PCIe, su INTx (opcional) | "entrada" |
| nvme | su permiso DMA, registros del NVMe del PCIe, su interrupción (opcional) | "disco" (es quien lee) |
| disco (prueba, solo `virt`) | — | "disco" |
| compositor | — (en la Pi); la pantalla de UEFI y sus medidas (por UEFI) | "buzon" (en la Pi), "entrada" (es quien lee) |
| gpu | permiso DMA de la GPU, registros de la V3D, del puente ASB, del bloque PM y del cprman | "buzon" |
| sonido | permiso de plazo (S3) | "sonido" (es quien lee) |
| virtio-sonido | su permiso DMA, registros y configuración del aparato, permiso de plazo (S3), su interrupción (opcional) | "sonido" |
| red (solo `virt`) | — | "red" y "red-estado" (es quien lee los dos) |
| virtio-red (solo `virt`) | su permiso DMA, registros y configuración del aparato, su interrupción (opcional) | "red" |
| red-prueba, red-gloton (pruebas, solo `virt`) | — | "red" |

En cada canal el primero que lo nombra es el servidor; los demás reciben una copia marcada (su número de programa más
1) del otro extremo, y el servidor, un mensaje sin marca por cliente con su marca (8 bytes) y su nombre (16). Solo
`inicio` escribe sin marca, así que el servidor sabe quién le pide qué (auditoría de la fase 4, M6). Las llaves de
aparatos llegan en el orden en que el servicio las pide, y cada aparato con DMA tiene su propio permiso (M5). Llegan
con la restricción "una vez" (S4): el servicio no puede pasárselas a otro programa. Un servicio al que le falta algún aparato no se lanza: el kernel lo dice al arrancar
("inicio: gpu no se lanza: falta ..."). Es lo que pasa con la GPU en QEMU.

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
kernel), 3 su INTx si el kernel la sabe (del `interrupt-map` del árbol), y después el canal de entrada del compositor. Reinicia el controlador, da dirección a lo que haya en cada puerto y,
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
(`Estado/Servicios/caja-amiga.papelera`). Sin alta no hay sesión (-12). Con la orden 101, **baja**, también sin marca (la manda
`sesiones` cuando acaba algo que lanzó): se sueltan la sesión de esa marca (bytes 8-15), sus asas y raíces, y su alta.
Las fichas provisionales están en `usuario/src/bin/inicio.rs` (`FICHAS`). Con ellas:

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
| 31 prestar | Presta las páginas de la caché del fichero (A7): hasta 4 llaves de memoria, solo de leer, de 512 KB cada una, con los trozos seguidos desde el que tiene el byte a. Se leen sin viajes y siguen al día con lo que se escriba. Dejan de valer al cerrar la asa, cuando el cliente pasa de 8 prestadas (las suyas más viejas) y al volver a una instantánea, comprobar, o quitar o cerrar un volumen. Un fichero de menos de 512 KB no se presta (-6): con leer (2). En Rust, `dashos::prestados` | asa, a desde | a el byte donde empieza el primer trozo, b tamaño del fichero, c cuántas llaves; y las llaves |
| 34 dueño | El dueño y el grupo de lo que tiene la asa, y el usuario de quien pregunta (S10: la Información del Workbench dice de quién es y si se puede cambiar) | asa | a dueño, b grupo, c el usuario del cliente |

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
`Estado/Servicios/sesiones` (usuario 1, 0700): `registro` (la cuenta y un anillo de 1.024 entradas) y `permisos` (una
línea "programa servicio" por permiso concedido). Cuentas y frases, provisionales en el código (`CUENTAS`, `ADMIN`).

`DELANTE` (21, S27): quién está delante (un i32 y 16 bytes con el nombre). Solo al panel rápido y a la barra, por el
nombre que da `inicio`; a otro, o sin nadie delante, -2 y nada más.

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
| 9 lanzar | `guardian`; la barra y el Workbench (S10) | Un programa del disco, como quien pide o como un usuario que entró; nace con su canal con `sesiones`, su llave de archivos y sus raíces. La barra y el Workbench solo lanzan de `Programas/` (un nombre, sin cajones), siempre como la persona que está delante; sin nadie delante, -2 |
| 10 nombres | cualquiera | Su tabla de nombres (`SYS:`, `Programas:`, `Home:`, `PROGDIR:`), con la raíz de cada uno |
| 11 preguntas | `guardian` | La llave por la que se le pregunta: permisos, acciones de administrador y bloquear |
| 12 respuesta, 13 retirar | `guardian` | Sí o no a un permiso (lo concedido se guarda; S28: 2 es "sí, solo esta vez", sin guardarlo); quitar uno concedido (se borra y se revocan sus llaves) |
| 14 administrar | cualquiera | Una acción de la lista cerrada ("comprobar", "matar"), con la contraseña de un administrador que pide `guardian`. Desde una sesión de detrás, -2 (U6) |
| 15 respuesta de administrador | `guardian` | Usuario y contraseña para esa acción; se comprueban con su volumen |
| 16 bloquear | uno del usuario con la sesión delante | `guardian` bloquea la sesión con su pantalla |
| 18 activa | cualquiera | Si su sesión está delante (1), detrás (0) o no tiene (-2). U6: cambio rápido de usuario |

Las negativas (matar, servicio, entrar, lanzar, administrar, bloquear) y los procesos que mueren por un fallo quedan en
el registro.

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

El servicio "sonido" lee el canal "sonido"; el controlador de virtio-sound y los programas le escriben con copias
marcadas, y `inicio` le dice de quién es cada marca. Mensajes de palabras de 32 bits (little-endian):

| Orden | Quién | Datos | Llaves |
|-------|-------|-------|--------|
| 1 `SALIDA` | solo "virtio-sonido" | ranuras (1 a 64), cuadros por período (16 a 4096), ritmo (48.000) | la memoria de la salida |
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

## Red (S13, primera versión)

`usuario/src/bin/red.rs` es el servicio: la pila TCP/IP (smoltcp 0.14, 0BSD) en su proceso. `usuario/src/bin/virtio_red.rs`
es el controlador de virtio-net (PCI moderno; en QEMU, `disable-legacy=on`). El protocolo está en
`usuario/src/protocolo/red.rs` y el lado del programa en `usuario/src/red_cliente.rs` (`Conexion`: conectar, enviar,
recibir, esperar y cerrar; `estado`).

**El permiso.** Solo llega a la red quien tiene una llave del canal "red". Hoy la da `inicio`, por nombre, a quien la
pide en `PROGRAMAS`; más adelante la repartirá `sesiones`. El servicio sabe de quién es cada marca por `inicio` y no
atiende marcas que no conoce. Un programa solo toca sus conexiones.

**El estado, solo para leer** (decisión del usuario del 2026-11-13). El canal "red-estado" lo atiende un hilo aparte del
servicio, que solo contesta `ESTADO` (las mismas palabras de `R_ESTADO`, sin `E_SUYAS` ni `E_RECHAZOS`) con una foto que
deja el bucle principal en cada vuelta. Cualquier otra orden se tira (y sus llaves se cierran): con esa llave no se
abre nada. La tiene el panel rápido, en vez de la de "red".

**El controlador.** Llaves: 1 su permiso DMA, 2 sus registros (BAR 4), 3 su configuración del PCIe, 4 su interrupción
(INTx; opcional, sin ella mira cada 2 ms) y después el canal "red". Funciones: VERSION_1 y MAC; sin sumas por el
aparato ni TSO. Una cola de recibir de 128 búferes de 2 KB y una de enviar de 64. Manda `TARJETA` (la MAC, una memoria
con 128 ranuras de recibir y 64 de enviar, y un canal suyo) y después `TRAMAS` cuando mueve algo; el servicio le manda
`TRAMAS` cuando pone algo para enviar. Copia cada trama entre la DMA y las ranuras: el servicio no toca los
descriptores. Tira y cuenta lo corto (menos de 14 bytes), lo largo (más de 1518), lo que no cabe en las ranuras y los
descriptores que el aparato devuelve sin tenerlos.

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
| 4 `ESTADO` | | un extremo de escritura | `R_ESTADO` (4): dirección, puerta, DNS, conexiones, tramas y rechazos |

Estados: 0 bien, -1 mal formado, -2 sin dirección, -3 límite, -4 rechazada, -5 cortada, -6 no existe, -7 familia que no
se sabe (IPv6). Como mucho 8 conexiones por programa y 32 en total; la novena da -3, las de los demás siguen.

**La memoria de una conexión** la crea el servicio (cuenta en su cupo) y el programa recibe una copia que no puede
pasar. Así no puede dar una memoria revocable y quitársela al servicio con él dentro. Cabecera de palabras (cabeza y
cola del anillo de envío, cabeza y cola del de recepción, fase, "el programa duerme", "el servicio espera") y los dos
anillos desde el byte 64. Lo que mueve el servicio lo guarda él, no lo lee de la memoria. Cuando el programa duerme y
algo cambia, el servicio le manda `EVENTO`; cuando el servicio espera al programa, este le manda `AVISO`. Cada 2 s sin
mensajes, un `LATIDO`: si su canal está cerrado, el programa acabó y la conexión se corta.

**Cortafuegos.** Nada escucha: lo que llega sin pedirlo, la pila lo rechaza. Escuchar queda para después.

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
