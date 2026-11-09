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

| Byte | Tecla | Ratón |
|---|---|---|
| 0 | 1 | 2 |
| 1 | Uso HID (página 7, teclado); los modificadores son 0xE0 a 0xE7 | Botones (bit 0 izquierdo, 1 derecho, 2 central) |
| 2 | 1 pulsada, 0 soltada | Movimiento x, con signo |
| 3 | 0 | Movimiento y, con signo |
| 4 | 0 | Rueda, con signo (0 si el ratón no la manda) |
| 5-7 | 0 | 0 |

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

En cada canal el primero que lo nombra es el servidor; los demás reciben una copia marcada (su número de programa más
1) del otro extremo, y el servidor, un mensaje sin marca por cliente con su marca (8 bytes) y su nombre (16). Solo
`inicio` escribe sin marca, así que el servidor sabe quién le pide qué (auditoría de la fase 4, M6). Las llaves de
aparatos llegan en el orden en que el servicio las pide, y cada aparato con DMA tiene su propio permiso (M5). Un servicio al que le falta algún aparato no se lanza: el kernel lo dice al arrancar
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

## NVMe (U6d)

`usuario/src/bin/nvme.rs`. Llaves: 1 su permiso DMA, 2 sus registros (la BAR 0, que busca el kernel por la clase
0x010802), 3 su interrupción si el kernel la sabe, y después el canal "disco". Al arrancar reconoce el disco (modelo, sectores) y lee los dos primeros sectores.

Protocolo del canal "disco" (U6f). Petición, 24 bytes little-endian, con una llave: el extremo de escritura de un
canal del cliente (con derecho a transferir), por el que vuelve la respuesta.

| Bytes | Campo |
|---|---|
| 0-3 | orden: 0 abrir, 1 leer, 2 escribir, 5 vaciar la caché del disco |
| 4-7 | sectores |
| 8-15 | primer sector |
| 16-19 | desplazamiento en la ventana (múltiplo de 4) |

Respuesta, 24 bytes: estado (0 bien; -1 mal formado o sin marca, -2 fuera del disco, -3 fuera de la ventana, -4 error
del disco, -5 sin sitio), en 8-15 los sectores del disco, en 16-19 el tamaño del sector y en 20-23 el de la ventana.
Al abrir lleva además la llave de la **ventana**: 64 KB de memoria DMA (sin caché) del cliente, compartida con el
servicio, donde el disco lee y escribe directamente. Cada cliente se reconoce por la marca de su llave (la pone
`inicio`): un mensaje sin marca se rechaza, y abrir otra vez da la misma ventana. Hasta 4 clientes. Una transferencia
cabe en dos páginas de la ventana (8 KB desde un principio de página). La ventana lleva derecho a transferir, para que
el cliente pueda dársela a otro proceso suyo.

**Vaciar** (orden 5, FS2): sin sectores ni ventana. Cuando contesta, todo lo escrito antes está guardado en el disco,
también lo que estuviera en su caché. Un sistema de archivos lo pide antes y después de escribir su cabecera: sin esto,
un disco con caché puede guardar la cabecera antes que los bloques a los que apunta. Vale también en el anillo.

**Anillo** (U6l, S1): con la orden 3 (tras abrir, con una llave de respuesta propia) el servicio da una página de
memoria compartida y se queda con esa llave para avisar. Cabecera de palabras de 32 bits: 0 cabeza de envíos (la
mueve el servicio), 4 cola de envíos (el cliente), 8 cabeza de terminadas (el cliente), 12 cola de terminadas (el
servicio), 16 "el servicio duerme", 20 "el cliente espera". Envíos de 32 bytes desde el byte 64 (orden 1 o 2,
sectores, primer sector de 64 bits, desplazamiento en la ventana, etiqueta); terminadas de 16 desde el 2048
(etiqueta, estado). 32 de cada. El cliente pone envíos, mueve la cola y, solo si el servicio duerme, le manda la
orden 4 (24 bytes, sin llave, sin respuesta). El servicio, si el cliente espera, le manda 4 bytes por su canal. El
servicio copia cada envío antes de comprobarlo. Hasta 16 sectores por envío.

Pendiente: varias colas de E/S del NVMe.

## Archivos (FS1, FS3, FS7, FS8, FS9, FS10)

`usuario/src/bin/archivos.rs` sirve un RedoxFS (`archivos/redoxfs`, S5) que lee por el canal "disco": hoy, el que
empieza en el MB 1 del disco NVMe de `virt`. Llaves: 1 el canal "disco" y 2 el canal "archivos", donde lee. Cada
orden que cambia algo es una transacción de RedoxFS que queda en el disco, con la caché vaciada, antes de la respuesta
(FS4). Este protocolo es el de todos los sistemas de archivos de DashOS: los que vengan (FAT, ext4,
los del Amiga) lo cumplen igual, y `dos.library` habla con todos de la misma forma.

Cada cliente se reconoce por la marca de su llave (la pone `inicio`): un mensaje sin marca se tira. Hasta 8 clientes.
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
(un volumen con algo abierto).

Cada asa es de un volumen (FS10): lo que se busca, crea o renombra desde ella queda en el suyo. La asa 0 es la raíz
del principal; la orden 21 da la raíz de otro.

| Orden | Qué | Pide | Devuelve |
|---|---|---|---|
| 0 sesión | Empieza; con una llave: el extremo de escritura del canal de respuesta | — | a: tamaño de la ventana; y su llave |
| 1 buscar | Abre lo que hay en `nombre` (partes separadas por "/"; vacío, el mismo) desde la asa | asa, nombre | asa nueva; a tamaño, b fecha (segundos), c modo \| tipo << 16 \| bits del Amiga << 32 |
| 2 leer | Lee del fichero al principio de la ventana | asa, a desde, b cuánto (hasta la ventana) | a bytes leídos, b tamaño del fichero |
| 3 escribir | Escribe en el fichero lo que hay al principio de la ventana | asa, a desde, b cuánto (hasta la ventana) | a bytes escritos, b tamaño del fichero |
| 4 listar | Pone en la ventana las entradas de un cajón desde la número a | asa, a desde | a cuántas, b la siguiente (o `u64::MAX` si no hay más), c bytes usados |
| 5 datos | Los datos de una asa | asa | como buscar |
| 6 cerrar | Suelta la asa | asa | — |
| 7 crear | Crea `nombre` (una parte) en el cajón de la asa y lo abre | asa, a: 1 fichero, 2 cajón; nombre | como buscar |
| 8 borrar | Borra `nombre` del cajón de la asa; un cajón, solo vacío | asa, nombre | — |
| 9 renombrar | Mueve `viejo` del cajón de la asa a `nuevo` en el cajón de la asa b. No sobrescribe; no mete un cajón en sí mismo | asa, b, nombre "viejo\0nuevo" | — |
| 10 tamaño | Recorta o alarga el fichero | asa, a tamaño nuevo | a tamaño |
| 11 vaciar | Que todo esté en el disco (hoy ya lo está al contestar cada orden) | — | — |
| 12 info | Datos del volumen | — | a tamaño, b libre, c tamaño de bloque (bytes) |
| 13 comentario | Pone en la ventana el comentario (UTF-8, hasta 255 bytes) | asa | a su largo |
| 14 comentar | Cambia el comentario; vacío, lo quita | asa, nombre: el comentario | — |
| 15 proteger | Cambia los bits del Amiga y los permisos de Unix | asa, a bits, b permisos (hasta 0o7777) | — |
| 16 comprobar | Comprueba el disco entero, sin cambiar nada (FS8); pone en la ventana los problemas, uno por línea, hasta llenarla | a: 1 = leer también los datos | a cuántos problemas, b bytes en la ventana, c bytes en uso |
| 17 instantáneas | Pone en la ventana las instantáneas del volumen de la asa, de la más vieja a la más nueva: fecha (8), largo del nombre (1) y el nombre; la siguiente en un múltiplo de 8 (FS9) | asa | a cuántas caben, b cuántas hay, c bytes usados |
| 18 tomar | Toma una instantánea del volumen de la asa | asa, nombre (hasta 64 bytes, sin ':' ni '/') | — |
| 19 descartar | Borra una instantánea y suelta lo que solo veía ella | asa, nombre | — |
| 20 volver | Vuelve el volumen de la asa a su instantánea más nueva, que se queda. Lo escrito después se pierde y las asas de todos los clientes en ese volumen dejan de valer | asa, nombre | a el volumen |
| 21 volumen | Abre la raíz del volumen `nombre` (vacío: el principal) (FS10). Un cifrado cerrado se abre, pero no se lee | nombre | como buscar |
| 22 volúmenes | Pone en la ventana los volúmenes además del principal: número (8), marcas (1: 1 cifrado, 2 abierto), largo del nombre (1) y el nombre; el siguiente en un múltiplo de 8 | — | a cuántos caben, b cuántos hay, c bytes usados |
| 23 hacer volumen | Hace un volumen sin cifrar. Los cifrados, de momento, solo con `redoxfs-volume` en Linux: DashOS no tiene aún azar de calidad para las llaves | nombre; a = 0 | a su número |
| 24 quitar volumen | Borra un volumen con todo lo que tiene, instantáneas incluidas; no con algo abierto | nombre | — |
| 25 abrir volumen | Abre un volumen cifrado con una de sus contraseñas, hasta que se cierre. La contraseña no se guarda | nombre "volumen\0contraseña" | — |
| 26 cerrar volumen | Olvida la llave de un volumen cifrado; no con algo abierto | nombre | — |

Tipos: 1 fichero, 2 cajón, 3 enlace. El modo es el de Unix (permisos en los 12 bits bajos). Cada entrada de listar:
tamaño (8), fecha (8), modo (2), tipo (1), largo del nombre (1), bits del Amiga (2), largo del comentario (1), un byte
a 0, el nombre y el comentario; la siguiente empieza en un múltiplo de 8.

Los nombres no distinguen mayúsculas en las carpetas que lo tienen así (DashFS 2, FS6): hoy, todas las del disco.

**Bits del Amiga** (DashFS 3, FS7): 1 archivado, 2 puro, 4 guion, 8 residente, 16 no borrar (dueño), 32 no borrar
(grupo), 64 no borrar (otros). Leer, escribir y ejecutar son los permisos de Unix: `dos.library` los junta en los
bits de protección del Amiga. Escribir o cambiar el tamaño quita el bit de archivado, como en el Amiga. El servicio no
deja escribir ni cambiar el tamaño de un fichero sin permiso de escritura del dueño, ni borrar uno con "no borrar"
(-12); con usuarios (S6) mirará quién lo pide.

La **ventana** es memoria del servicio (64 KB) que el cliente proyecta: así el servicio sabe su tamaño y un cliente no
puede tumbarlo con una más pequeña. La sesión se puede abrir otra vez: da la misma ventana y cambia el canal de
respuesta. Si el cliente deja de escuchar, el servicio suelta sus asas y su ventana. Hasta 256 asas por cliente.

Lo borrado mientras alguien lo tiene abierto sigue leyéndose por esa asa hasta que se cierra; entonces se suelta. Si
el servicio se para antes, lo suelta al abrir el disco la vez siguiente.

Pendiente: la fecha de lo escrito (no hay reloj de la hora: va en 0), juntar escrituras (FS11), usuarios y permisos
(S6), cambiar si una carpeta distingue mayúsculas (para la caja Linux).

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
