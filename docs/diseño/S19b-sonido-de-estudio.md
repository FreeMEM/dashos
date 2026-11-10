# S19b. Sonido de estudio

Del 2026-11-10. Lo pidió el usuario: algo potente, porque la gente del Amiga lleva años limitada a Paula, y los del C64
adoran el SID y los sintetizadores. Recomendaciones aceptadas el mismo día. Amplía S19, que sigue valiendo para lo
básico (servicio propio, flujos por anillo, micrófono con permiso, caja Amiga y caja Linux). Es diseño: no hay código.

## Lo que se pide además de S19

- **Calidad de estudio**: coma flotante de 32 bits por dentro, hasta 192 kHz y muchos canales en la salida.
- **Latencia de instrumento**: períodos de 64 muestras (1,3 ms a 48 kHz) para tocar en directo, sin cortes con los
  cuatro núcleos ocupados.
- **Cualquier cosa conectada con cualquier cosa**: programas, efectos, instrumentos, entradas y salidas.
- **MIDI de primera**, también para los programas del Amiga.
- **Los chips clásicos como instrumentos**: Paula, SID, OPL y AY/YM.
- **Plugins** de terceros que no tumben el sistema.
- **Que no tire solo de CPU** (S21).

## El motor: un grafo

El servicio "sonido" de S19 pasa de mezclador a grafo, como JACK:

- **Nodos**: un flujo de un programa, un efecto, un instrumento, una entrada o una salida. Cada nodo tiene puertos de
  audio y de MIDI.
- **Conexiones libres**, que se cambian sin cortar el sonido. Un programa puede pedir su sitio en el grafo, o dejar
  que el servicio lo conecte a la salida, como en S19.
- **Un período para todo el grafo**, con orden por dependencias: cada nodo corre una vez por período, cuando sus
  entradas están listas. Los nodos independientes van en paralelo, en varios núcleos.
- **Período por salida**: 64 muestras para tocar, 256 por defecto (S19), más para ahorrar energía. Lo eligen
  `Prefs/Sonido` y el programa que lo necesite (con permiso: es la clase de plazo de S3).
- **Muestra exacta**: cada mensaje de MIDI y cada cambio de parámetro lleva su muestra dentro del período.
- **Sin copias** (S21): los nodos de un mismo proceso comparten búferes; los de otros procesos, anillos de memoria
  compartida.

## El hardware, por calidad

1. **USB Audio clase 2**: interfaces de audio y DAC. Es lo que da buen sonido en las tres Pi, y la única salida
   buena de la Pi 5 aparte de HDMI e I2S. Pide transferencias isócronas en xHCI y DWC2.
2. **I2S**: las placas de sonido (DAC, ADC y códecs comunes, que se controlan por I2C). Con DMA (S21, H1).
3. **HDMI**, con su controlador (S9).
4. **El jack** de la Pi 3 y la Pi 4 (PWM): para avisos y para empezar, no para música.

## MIDI

- Un **servicio de MIDI**: puertos de aparatos (USB-MIDI 1.0 y 2.0), puertos virtuales entre programas y conexiones,
  con marca de tiempo exacta.
- **MIDI 2.0** (paquetes UMP) por dentro; los aparatos de MIDI 1.0 se traducen.
- **La caja Amiga**: `camd.library` nativa, para que funcionen los secuenciadores de siempre.
- Permiso: usar MIDI no pide permiso; los aparatos USB de MIDI, sí, como el micrófono (S6).

## Los chips como instrumentos

Cada chip es un nodo instrumento del grafo: entra MIDI (o órdenes de registro, para los reproductores de música) y
sale audio. Cualquier programa los usa.

| Chip | De dónde | Notas |
|---|---|---|
| Paula | El de S18, ya hecho | Con sus filtros del A500 y del A1200 |
| SID (6581 y 8580) | Primero reSID, en la zona GPL; después uno propio | Ver abajo |
| OPL2 y OPL3 (AdLib, Sound Blaster) | Por decidir el emulador | Después del SID |
| AY-3-8910 e YM2149 (Spectrum, Atari ST, MSX) | Por decidir el emulador | Después del SID |

**El SID.** reSID es el emulador bueno y es GPL. Decisión: **primero reSID, en la zona GPL**, en su propio proceso, que
habla con el servicio de sonido solo por canales (la misma regla que los controladores de Linux, `AGENTS.md` regla 7).
**Después, uno propio** en sala limpia, desde la hoja de datos del 6581 y las medidas publicadas de su filtro, que
pueda ir dentro del servicio y con NEON (S21). Mientras tanto, reSID con varios SID a la vez.

**SID de verdad**: soporte para SID por USB (USBSID-Pico, SIDBlaster), como salida del nodo SID: el mismo programa
suena por emulación o por el chip, sin cambiar nada.

**Reproductores**: los ficheros de música de siempre (MOD de ProTracker, MED de OctaMED, SID del C64) se tocan con un
nodo reproductor que manda órdenes al nodo del chip. Así un SID de verdad toca los ficheros del C64.

## Plugins: CLAP

- **CLAP** (licencia MIT) como estándar de plugins: instrumentos y efectos de terceros.
- **Cada plugin en su proceso**, con su anillo: si se cuelga, el estudio sigue sonando y el servicio lo quita del
  grafo con un fundido.
- Un plugin no tiene permisos: ni red, ni ficheros fuera de su cajón (S6b), salvo los que declare su paquete.

## API compatible con JACK

Para los programas de audio de Linux portados en nativo (Ardour, sintetizadores, secuenciadores), una biblioteca con
la API de JACK que habla con el grafo de DashOS: cada cliente de JACK es un nodo, sus puertos son puertos del grafo y
su función de proceso corre en el período del grafo. Así no hace falta la caja Linux para ellos. Es lo que hace
PipeWire con su biblioteca que imita a JACK.

- **Sala limpia**: se escribe desde la documentación pública de la API de JACK, sin copiar su código (la biblioteca de
  JACK es LGPL). Las cabeceras dicen las firmas; lo que no digan, la documentación y las pruebas con programas reales.
- **Lo que no tiene sentido aquí** (arrancar un servidor, elegir el controlador de la tarjeta) se acepta y no hace
  nada: el servidor es el servicio de sonido.
- **Permisos**: un programa portado es una aplicación más (S6b). Conectarse a otros nodos que no son suyos pide el
  mismo permiso que grabar lo que suena (S19).

## Que no tire solo de CPU (S21)

- **DMA** para mandar el audio a USB, I2S y PWM.
- **NEON** para la mezcla, los filtros, el remuestreo y los chips (varias voces por instrucción).
- **GPU (QPU)** para lo pesado en bloques grandes: reverb por convolución, efectos espectrales, síntesis de cientos de
  voces, analizadores para la pantalla. No para lo de baja latencia.
- **Chips de verdad** por USB y sintetizadores por MIDI.

## Cómo se prueba

Como en S19, el WAV de QEMU es el oráculo, más:

- **Latencia de ida y vuelta** en la placa, con un cable de la salida a la entrada de una interfaz USB.
- **Cortes con carga** a 64 muestras.
- **El SID contra un C64**: un mismo fichero SID grabado en VICE y en DashOS; se comparan frecuencia, envolventes y
  forma del filtro, no muestra a muestra.
- **Cuánto cabe**: voces de Paula, chips SID y plugins por núcleo, al panel de medidas.

## Fases

Van después de las de S19 (A1 a A6, ya hechas en parte).

| Fase | Qué | Prueba |
|---|---|---|
| E1 | El grafo: nodos, conexiones, orden por dependencias, períodos por salida | Prueba de PC del grafo; dos flujos y un efecto en el WAV de QEMU |
| E2 | USB Audio clase 2 (isócronas en xHCI) | QEMU `usb-audio`; después una interfaz de verdad en la Pi |
| E3 | MIDI: servicio, USB-MIDI, puertos virtuales, `camd.library` | QEMU con un aparato MIDI virtual; un secuenciador del Amiga |
| E4 | SID con reSID en la zona GPL, nodo instrumento y reproductor de SID | El fichero de prueba contra VICE |
| E5 | NEON en la mezcla, los filtros y Paula (S21, H3) | Voces por núcleo, antes y después |
| E6 | CLAP: plugins en su proceso | Un plugin que se cuelga a propósito no corta el sonido |
| E7 | I2S y DMA (S21, H1) | Una placa DAC en la Pi |
| E8 | SID de verdad por USB | USBSID-Pico |
| E9 | SID propio en sala limpia, OPL y AY/YM | Contra reSID y VICE |
| E10 | Efectos en la GPU (S21, H4) | Convolución: tiempo por bloque frente a NEON |
| E11 | API compatible con JACK para programas nativos portados | Un cliente de JACK de prueba en C con dos puertos y un sintetizador portado; mismo resultado en el WAV de QEMU |

## Decisiones

Aceptadas por el usuario el 2026-11-10:

1. El grafo como motor, con períodos de 64 muestras para tocar.
2. SID: primero reSID en la zona GPL, después uno propio.
3. CLAP, cada plugin en su proceso.
4. API compatible con JACK para los programas portados (E11), en sala limpia.
5. Orden: después de la revisión y de Wayland con el puente (`docs/GRAFICOS_LOOP.md`), y antes de los hilos, LKL y el
   Wi-Fi (S12b).

## Cómo encaja

- S2 y S3: la clase de plazo con su llave, para el grafo y para quien pida períodos cortos.
- S6 y S6b: permisos de micrófono, MIDI por USB y plugins.
- S12 y S12b: USB isócrono, I2S, HDMI; la zona GPL.
- S18: Paula como nodo.
- S19: lo básico, que no cambia.
- S21: qué chip hace cada parte.
