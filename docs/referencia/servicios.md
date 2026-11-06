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

Espera con la interrupción del DWC2 y un temporizador de 10 ms; entre consulta y consulta no gasta CPU. Enumera el
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
| buzon | permiso DMA, registros del buzón | "buzon" |
| usb | permiso DMA, registros e interrupción del DWC2 | "entrada" |
| compositor | memoria de la VideoCore | "buzon", "entrada" |

## Buzón de la VideoCore (C50)

`usuario/src/bin/buzon.rs`. Llaves: 1 el permiso DMA, 2 los registros del buzón (la página entera; el buzón está en
el desplazamiento 0x880), 3 el canal de peticiones.

Cada petición es un mensaje de propiedades del firmware tal cual, de hasta 256 bytes: palabras de 32 bits en
little-endian con el tamaño, el código 0, las etiquetas y la etiqueta 0 de fin. La respuesta vuelve por el mismo canal
con el mismo tamaño y el código 0x80000000. Si el mensaje está mal formado o lleva una etiqueta que no está en la
lista, vuelve con el código 0x80000001 y el firmware no lo ve. Etiquetas permitidas:

| Etiqueta | Para |
|---|---|
| 0x00010006 | Memoria de la VideoCore: base y tamaño |
| 0x00030002, 0x00030004 | Frecuencia de un reloj y su máximo |
| 0x00040001, 0x00040008 | Reservar la pantalla y su paso de línea |
| 0x00048003 a 0x00048006 | Tamaño físico y virtual, bits por píxel y orden de colores |
| 0x00048009 | Desplazamiento virtual (cambiar de búfer) |

Hoy atiende a un solo cliente, el compositor. Las etiquetas de la V3D llegarán con ella.

## Compositor (C50)

`usuario/src/bin/compositor.rs`. Llaves: 1 la memoria de la VideoCore, 2 el canal del buzón, 3 el canal de entrada.
Pide al firmware una pantalla de 640 × 480 a 32 bits con dos búferes, pinta el fondo y una ventana, y repinta solo lo
que cambia. Cada movimiento del ratón mueve la ventana. Provisional: el de verdad hablará Wayland (S9).
