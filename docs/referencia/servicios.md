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

Hoy, en la imagen normal, `inicio` no le da la llave 4: los eventos van a la consola. El canal llegará con el
compositor (C50).
