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
| 0-3 | orden: 0 abrir, 1 leer, 2 escribir |
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

Pendiente: el anillo de peticiones en memoria compartida (S1), con aviso por canal solo cuando hace falta; varias
colas de E/S.
