# S21. Cada trabajo a su chip

Del 2026-11-10. Lo pidió el usuario: "algo brillante y súper óptimo que no tire solo de CPU". Recomendaciones
aceptadas el mismo día. Es diseño: no hay código.

## El problema

Hoy todo lo hace la CPU: cada programa pinta sus ventanas, el compositor las junta, el sonido se mezcla y se copia,
y el disco y la red copian bloques. Funciona, y en QEMU no hay otra cosa. Pero una Raspberry Pi tiene, además de la
CPU, motores de DMA, un compositor de pantalla por hardware (el HVS), una GPU con procesadores de cálculo (los QPU) y
decodificadores de vídeo. Usarlos libera la CPU, gasta menos energía y permite cosas que la CPU sola no alcanza.

## El principio

Cada trabajo va al chip que mejor lo hace, sin copias por el camino, y la CPU queda para lo que nadie más puede
hacer. Tres reglas:

1. **Mover datos no es trabajo de la CPU.** Sonido a su salida, disco, red y pantalla van por DMA. La CPU prepara la
   lista y se va.
2. **Sin copias.** Un búfer se pinta, se mezcla o se lee una vez, en memoria compartida (anillos de S1, `dmabuf` de
   Wayland), y el chip que lo usa lo lee de ahí.
3. **Medido.** Cada camino nuevo se compara con el de la CPU en la misma prueba, con el uso de CPU al lado, y va al
   panel de medidas. Si el chip no gana, se queda la CPU.

## Qué hay en cada placa

De memoria; se comprueba en la documentación de cada placa antes de empezar su fase.

| Trabajo | Pi 3 (BCM2837) | Pi 4 (BCM2711) | Pi 5 (BCM2712) |
|---|---|---|---|
| CPU | 4 × Cortex-A53, NEON, sin AES por hardware | 4 × Cortex-A72, NEON, sin AES por hardware | 4 × Cortex-A76, NEON, con AES por hardware |
| Mover datos | DMA de la BCM2835 | DMA, con canales de 40 bits | DMA del chip y DMA propio del RP1 (la E/S) |
| Juntar ventanas | HVS de la VideoCore IV | HVS (`docs/spec/hw/hvs.md`) | HVS nuevo |
| Pintar y 3D | VideoCore IV (V3D 2.1): manual público de Broadcom (2014) | V3D 4.2: sin manual; Mesa (`docs/spec/hw/v3d.md`) | V3D 7.1 (VideoCore VII): sin manual; Mesa |
| Cálculo en la GPU (QPU) | Sí, documentado; hay FFT hecha para ella | Sí, por Mesa (Vulkan) o a mano | Sí, por Mesa (Vulkan) |
| Vídeo | H.264 por hardware | HEVC por hardware (hasta 4K); H.264 por el firmware | HEVC por hardware; H.264 por CPU (no tiene) |
| Sonido de serie | Jack (PWM), HDMI, I2S | Jack (PWM), HDMI, I2S | HDMI e I2S; sin jack |
| USB para audio | USB 2 (DWC2, compartido con la red) | USB 3 (VL805 por PCIe) y USB 2 | USB 3 (RP1) |

Lo que sale de la tabla:

- **El buen sonido en las tres es USB o I2S.** El jack de la Pi 3 y la Pi 4 es un PWM; la Pi 5 ni lo tiene.
- **La Pi 3 es la mejor para empezar con la GPU**, aunque sea la más lenta: es la única con manual público. Lo que se
  aprenda de los QPU allí sirve de modelo para las otras.
- **El vídeo cambia de una a otra**: la Pi 5 decodifica H.264 con la CPU.

## Quién hace qué

| Trabajo | Quién | Por qué | Reserva |
|---|---|---|---|
| Mover datos | DMA | La CPU no copia | CPU |
| Juntar ventanas | HVS, una ventana por plano | Monta la imagen al mandarla a la pantalla | GPU, y si no, CPU solo lo dañado |
| Pintar ventanas, efectos y 3D | GPU (V3D) | Para eso está | CPU (`wl_shm`) |
| Vídeo | Decodificador de la placa | Un 4K no cabe en la CPU | CPU |
| Sonido pesado: convolución, efectos espectrales, cientos de voces, analizadores | GPU (QPU), en bloques grandes | Cálculo en paralelo | CPU con NEON |
| Mezcla, filtros, remuestreo, Paula, SID | CPU con NEON | Cada muestra depende de la anterior: no se reparte en la GPU, pero NEON hace varias voces por instrucción | — |
| TLS | CPU: AES en la Pi 5, ChaCha20 en la 3 y la 4 (S13) | Lo que tenga cada CPU | — |
| Chips de verdad | Hardware externo: SID por USB, sintetizadores por MIDI (S19b) | El sonido original | Emulación |

### Con el uso de CPU al lado

Estimado, sin medir: niveles, no números. Los números de verdad salen de cada fase y van al panel de medidas, con la
misma prueba por CPU y por el chip.

| Trabajo | Pi 3 | Pi 4 | Pi 5 | CPU si lo hace ella | CPU con el chip |
|---|---|---|---|---|---|
| Mover sonido, disco y red | DMA | DMA | DMA y RP1 | Media: una copia por byte | Casi nada: preparar listas |
| Juntar ventanas | HVS | HVS | HVS | Alta con muchas ventanas o pantalla entera; un núcleo a 1080p y 60 cuadros, quizá más | Casi nada mientras quepan en planos |
| Pintar ventanas y 3D | V3D 2.1 | V3D 4.2 | V3D 7.1 | Alta: 3D en la CPU no es usable | Baja: preparar las listas de la GPU |
| Vídeo HEVC | CPU (no tiene) | Decodificador | Decodificador | Muy alta: un 4K no cabe | Casi nada |
| Vídeo H.264 | Decodificador | Firmware | CPU (no tiene) | Alta a 1080p | Casi nada |
| Efectos pesados de sonido (convolución, cientos de voces) | QPU | QPU | QPU | Alta: puede llenar un núcleo | Baja: preparar y recoger bloques |
| Mezcla, filtros, remuestreo | CPU con NEON | CPU con NEON | CPU con NEON | Baja con NEON | — |
| Paula y SID emulados | CPU con NEON | CPU con NEON | CPU con NEON | Baja por chip con NEON; la Pi 3 aguanta menos | — |
| SID de verdad | USB | USB | USB | — | Casi nada: mandar registros |
| TLS | ChaCha20 en CPU | ChaCha20 en CPU | AES por hardware | Media en descargas largas | Baja en la Pi 5 |

**Límite de la GPU en el sonido.** Darle trabajo tiene un coste fijo: con períodos de 64 muestras (1,3 ms) se lo come.
Va bien con bloques grandes y cálculo masivo (una reverb de una sala real, cientos de osciladores, un analizador de
espectro). La baja latencia se queda en la CPU.

## Fases

| Fase | Qué | Placa para probar | Medida |
|---|---|---|---|
| H1 | DMA para el sonido (PWM e I2S), el disco y la red | Pi 4, y QEMU donde se pueda | CPU por MB movido, antes y después |
| H2 | Planos del HVS en el compositor (G7 de `docs/GRAFICOS_LOOP.md`) | Pi 4 | Cuadros por segundo y CPU con 1, 3 y 8 ventanas |
| H3 | Rutas NEON en la mezcla, los filtros, el remuestreo, Paula y el SID | Las tres, y QEMU | Voces o chips por núcleo |
| H4 | Cálculo en los QPU: primero en la Pi 3 (manual público), una FFT y una convolución; después en la 4 y la 5 por Mesa | Pi 3, después Pi 4 y 5 | Tiempo por bloque frente a NEON |
| H5 | Decodificador de vídeo de cada placa | Pi 4 y Pi 5 | Vídeo 1080p y 4K: CPU y cuadros perdidos |
| H6 | GPU para pintar los programas y para componer de reserva (fase C de S9: Mesa) | Pi 4 y Pi 5 | Cuadros por segundo de `alacritty` y del kit |

QEMU no emula HVS, V3D ni decodificadores: H2, H4, H5 y H6 se prueban en placas de verdad.

## Lo que queda abierto

Nada que decidir ahora. Cada fase dirá, al medir, si el chip gana a la CPU.

## Cómo encaja

- S1: anillos de memoria compartida y llaves de memoria para pasar búferes sin copiar.
- S2: la clase de plazo para el sonido y el compositor.
- S9: los planos del HVS (fase B) y la V3D con Mesa (fase C).
- S12 y S12b: controladores de DMA, HVS, V3D y decodificadores.
- S13: TLS según la CPU.
- S19 y S19b: el sonido.
- `docs/GRAFICOS_LOOP.md`: G6 (`linux-dmabuf`) y G7 (planos).
