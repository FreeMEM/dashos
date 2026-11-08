# S19. El sonido

Del 2026-11-09. Lo pidió el usuario ese día. Es análisis y diseño: no hay código. Orden decidido por el usuario: el
sonido se programa después del compositor que habla Wayland.

## El problema

Hasta hoy el sonido solo aparecía de pasada:

- S2 pone el sonido en la clase de plazos, con el compositor y la entrada: un corte se oye.
- S6 dice que el servicio de sonido corre como un usuario propio.
- S10 pone el sonido en la bandeja de la barra y en el centro de control.
- S15 pone el sonido en H6, la Pi 4 de verdad.
- S18 necesita dónde mandar lo que genera Paula, y lo marca como dependencia que falta.

No hay servidor, ni API, ni controladores. Este documento resuelve: qué servidor, cómo hablan con él los programas
nativos, los de la caja Amiga y los de la caja Linux, qué hardware y en qué orden, cómo se protege el micrófono, cómo
se prueba y en qué fases se hace.

## Lo que se pide

- **Sin cortes** con los cuatro núcleos al 100 % (S0). Es la medida que manda.
- **Latencia baja**: 10 ms o menos de un programa al altavoz para juegos, emulación y música; el escritorio puede
  vivir con 20 a 40 ms y gastar menos energía.
- **Muchos programas a la vez**, cada uno con su volumen, y elegir por dónde sale (HDMI, auriculares, USB).
- **Los programas del Amiga suenan** igual que los nativos: los que usan el sistema (`audio.device`, AHI) y los que
  tocan Paula (S18).
- **La caja Linux suena** sin saber que está en DashOS.
- **Matar siempre funciona** (S2): matar un programa a mitad de un sonido corta su sonido y nada más.
- **El micrófono no se escucha a escondidas** (S6).

## Las opciones

### El servidor

**A. Un servidor propio y pequeño (recomendada).** Un proceso, "sonido", en la clase de plazos. Cada programa tiene
uno o varios flujos; el servidor los mezcla en un grafo sencillo (flujos → mezcla por salida → controlador) y entrega
bloques al controlador al ritmo del reloj del hardware. Es lo que hace PipeWire, sin todo lo que PipeWire trae para
Linux (sesiones, módulos cargables, compatibilidad con PulseAudio y JACK). Con los anillos de memoria compartida de
S1 y la clase de plazos de S2, el núcleo del servidor cabe en unas pocas miles de líneas de Rust.

- Gana: se entiende entero (S0), encaja con las llaves y los anillos de DashOS, y su latencia la decide la clase de
  plazos, no reglas que adivinan.
- Cuesta: hay que escribir el remuestreo y la mezcla, y probar mucho los cortes.

**B. Portar PipeWire.** Es MIT y es el estándar en Linux.

- Gana: hecho y probado; los programas de Linux portados hablarían con él directamente.
- Cuesta: es grande, depende de cosas de Linux (eventfd, epoll, memfd, dbus para sesiones) y su planificación
  supone el kernel de Linux. Portarlo es más trabajo que escribir A, y no se entendería entero.

**C. Sin servidor**: cada programa abre el dispositivo, como OSS en Unix o Paula en el Amiga. Solo suena uno a la
vez, o la mezcla la hace el controlador. Descartada: no da volumen por programa, ni varias salidas, ni protege el
micrófono.

### Cómo hablan los programas con él

- **Anillo de muestras por flujo**, en memoria compartida (S1): el programa escribe, el servidor lee. Un canal de
  mensajes para abrir, cerrar, cambiar el volumen y saber la latencia. El servidor despierta al programa cuando el
  anillo baja de un umbral, con un temporizador de la clase de plazos; el programa no tiene que estar en ella si
  llena el anillo con margen.
- **Formatos**: el programa elige entre enteros de 16 y 32 bits y coma flotante de 32, a 44,1 o 48 kHz, mono o
  estéreo (más canales después). El servidor mezcla siempre en coma flotante a la frecuencia de la salida y remuestrea
  lo que haga falta.
- **El reloj manda**: la frecuencia real del hardware nunca es la nominal. Cada flujo se reajusta un poco a ese reloj,
  como ya prevé S18 para Paula.

## Recomendación

A, con este diseño:

### El servicio "sonido"

- Un proceso propio, con su usuario (S6), en la clase de plazos con un presupuesto pequeño (S2). Período por
  defecto: 256 muestras a 48 kHz (5,3 ms), dos o tres períodos en el anillo del controlador. El escritorio sin
  juegos puede subir el período para gastar menos.
- Los controladores son otros procesos (S12): el servicio les habla por un anillo de bloques ya mezclados. Si un
  controlador cae, el servicio sigue y pasa a otra salida.
- **Volumen por programa y por salida**, y silenciar. La barra (S10) lo enseña con los programas que suenan.
- **Matar**: un flujo es un objeto con dueño (S2). Muere el programa y el servicio ve cerrarse el anillo y lo quita en
  el siguiente período, con un fundido de unos milisegundos para que no chasque.

### La caja Amiga

- **`audio.device` nativo**: los cuatro canales de Paula, con sus prioridades y su reparto (`ADCMD_ALLOCATE`), y sus
  órdenes (`CMD_WRITE`, `ADCMD_PERVOL`, `ADCMD_FINISH`...). Cada canal es un flujo del servicio, a la frecuencia que
  sale de su período (3.546.895 / período en PAL), panoramizado como en el Amiga (0 y 3 a la izquierda, 1 y 2 a la
  derecha), y remuestreado por el servicio.
- **AHI nativo** (`ahi.device` y la biblioteca), desde su documentación de programador (`ahidev.pdf`), como Zune se
  hizo desde la de MUI. AHI es LGPL en el dispositivo y GPL en sus programas; los controladores pueden tener cualquier
  licencia. No hace falta copiar su código: su API está documentada. Un solo "modo" de AHI: el servicio, en estéreo
  de 16 bits a 48 kHz, y los modos que piden los programas viejos se aceptan y se remuestrean.
- **Paula imitada** (S18): la caja de chips genera sus muestras en hora virtual y las manda por un flujo, con el
  reajuste de reloj que S18 ya describe.
- **Dash** ya tiene el módulo `lib/amiga/audio` (Paula a mano). En DashOS esos builtins van a `audio.device` nativo,
  y se añaden unos de flujo (abrir, escribir, volumen) para programas nativos.

### La caja Linux

La máquina virtual de Linux (S7) ve un dispositivo virtio-sound. Su lado en DashOS es un cliente más del servicio:
cada flujo de virtio-sound es un flujo nuestro. Dentro, Linux usa su PipeWire de siempre, que habla con virtio-sound
por ALSA. No hay puente de protocolos que mantener.

### Los controladores, por orden

1. **virtio-sound en QEMU `virt`.** Es el primero porque deja pruebas automáticas: QEMU puede escribir lo que suena a
   un WAV (`-audiodev wav`), que la prueba compara con lo esperado. El QEMU de este equipo (11.1.3) trae
   `virtio-sound-device`.
2. **La salida de auriculares de la Pi 4.** Es PWM: el bloque PWM1 en los GPIO 40 y 41, alimentado por DMA. Es el
   hardware más sencillo de la placa y hay ejemplos sin sistema operativo. Su calidad es limitada (es un PWM, no un
   DAC), y vale para empezar en la placa.
3. **HDMI.** Va con el controlador de HDMI de H6 (S9): el audio viaja por el mismo bloque HDMI, con sus paquetes de
   audio. Falta estudiar el camino (registros y DMA) en la documentación y en el comportamiento del hardware; el
   controlador de Linux (`vc4_hdmi`) es GPL y no se copia.
4. **USB Audio** (clases 1 y 2): tarjetas y auriculares USB, que es lo que más se usa en la Pi para buen sonido. Pide
   transferencias isócronas en el controlador xHCI, que hoy no las tiene.
5. **Placas I2S** (las "HAT" de sonido), por el bloque PCM/I2S.
6. **Bluetooth** (A2DP), mucho después: pide la pila de Bluetooth entera.

### El micrófono

- Grabar pide permiso por programa, como en un móvil (S6). Sin permiso, el programa no recibe el anillo de entrada.
- **Un aviso que no se puede tapar**: mientras un programa graba, el compositor (no el escritorio, que es de terceros
  en S10b) dibuja una marca en la pantalla y la barra dice quién graba.
- Lo mismo para grabar lo que suena (otros flujos o la salida entera): es un permiso aparte.

## Cómo se prueba

- **El oráculo es el WAV de QEMU.** Una prueba suena un tono puro, mezcla dos programas o cambia el volumen, y compara
  el WAV: frecuencia y nivel del tono, que no hay cortes (ni muestras a cero en medio), y el desfase entre dos flujos.
  Sin ventana y sin oído humano.
- **Cortes con carga**: la misma prueba con los cuatro núcleos ocupados; se cuentan los períodos que el servicio no
  entregó a tiempo. Es la medida de S0 para el sonido.
- **La caja Amiga contra un Amiga**: un programa que usa `audio.device` y otro con AHI, grabados en FS-UAE y en
  DashOS (el WAV de QEMU). Se comparan frecuencia, duración y nivel, no muestra a muestra: el filtro de Paula difiere
  (S18).
- **Latencia**: una marca de tiempo en el programa y la misma muestra en el WAV; en la placa, con un bucle de cable de
  la salida a la entrada.

## Fases

| Fase | Qué | Prueba | Tamaño |
|------|-----|--------|--------|
| 1 | Servicio con mezcla y remuestreo; controlador virtio-sound | Tono y mezcla en el WAV de QEMU; cortes con carga | 3.000 a 5.000 líneas |
| 2 | `audio.device` y AHI nativos en la caja | Programas Amiga suenan; comparados con FS-UAE | 2.000 a 3.000 |
| 3 | Auriculares de la Pi 4 (PWM con DMA) | Suena en la placa; bucle de cable | 500 a 1.000 |
| 4 | HDMI (con H6) | Suena por HDMI | 1.000 a 2.000 |
| 5 | USB Audio, con isócronas en xHCI | Una tarjeta USB suena y graba | 2.000 a 4.000 |
| 6 | Micrófono, permisos y aviso del compositor | Sin permiso no hay anillo; el aviso sale | 1.000 |
| 7 | virtio-sound para la caja Linux; Paula de S18 | Linux y un juego suenan | 1.000 a 2.000 |

Los tamaños son estimaciones, sin medir.

## Lo que queda abierto

- El período por defecto y cuánto se sube sin juegos: se decide midiendo cortes y energía en la placa.
- MIDI: el Amiga tiene `camd.library` y muchos programas de música la usan. Va en otro documento.
- Audio profesional al estilo JACK (latencias de 1 a 2 ms, rutas libres entre programas): no está en este diseño.
- Más de dos canales (5.1 por HDMI).
- Bluetooth.

## Cómo encaja

- Con S1: anillos en memoria compartida para las muestras y llaves para cada flujo.
- Con S2: el servicio y los controladores en la clase de plazos; matar corta el flujo.
- Con S4 y S18: `audio.device` y AHI nativos; Paula imitada manda sus muestras al servicio.
- Con S6: el servicio con su usuario; permisos de micrófono y de grabar lo que suena.
- Con S7: virtio-sound para la caja Linux.
- Con S10 y S10b: volumen y programas que suenan en la barra; el aviso de grabación lo dibuja el compositor.
- Con S12: los controladores son procesos aparte, recargables.
- Con S15: la fase 1 va tras el compositor que habla Wayland (decidido el 2026-11-09); las de la placa, con H6.

## Fuentes

- AHI: página del proyecto (https://arp2.sourceforge.net/ahi/), guía del programador
  (https://arp2.sourceforge.net/ahi/docs/ahidev.pdf) y licencia de los controladores
  (https://arp2.sourceforge.net/ahi/docs/COPYING.DRIVERS.txt): dispositivo LGPL, programas GPL, controladores con
  cualquier licencia.
- La salida de auriculares de la Pi 4 sin sistema operativo: PWM1 en los GPIO 40 y 41, DMA (tutorial "rpi4-osdev",
  parte 9, https://git.telodendria.io/jordan/rpi4-osdev/src/branch/master/part9-sound). No comprobado en la placa.
- virtio-sound: especificación virtio 1.2, sección del dispositivo de sonido. Que QEMU lo trae se comprobó con
  `qemu-system-aarch64 -device help` (11.1.3).
- PipeWire: licencia MIT. No se ha leído su código para este documento.
