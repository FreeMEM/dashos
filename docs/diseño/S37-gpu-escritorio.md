# S37. El escritorio en la GPU y los fondos animados

Del 2026-11-17. Lo pidió el usuario: "una herramienta espectacular para cambiar de fondo, poner fondos animados con un
bajo consumo de CPU, que si lo calcula la GPU sería lo suyo. Supongo que el escritorio podría ser renderizado por la
GPU si tiene la máquina, para no cargar la CPU". La herramienta de fondos va en la rama `fondos`; aquí, el camino de la
GPU y lo que cuesta cada cosa. Rama `gpu-diseno`. Sala limpia: Mesa y virglrenderer se ejecutan, no se leen.

> Ojo con el número: ya hay un `S37-iconos.md`. Este se llama así porque lo pidió el encargo; renumerarlo es cosa del
> usuario.

## 1. Qué hace hoy el compositor y cuánto cuesta

Todo por CPU (`wayland/src/escena.rs`, `Escena::componer`; `usuario/src/bin/compositor.rs`):

- **Componer por daños.** Solo se recompone lo dañado. Por cada fila dañada, tramos entre los bordes de las capas; en
  cada tramo se empieza por la capa opaca más alta (lo de debajo no se pinta).
- **Mezclar**: ARGB premultiplicado, sin saltos (`mezclar`), para que el compilador junte píxeles.
- **Sombras**: cada ventana echa la suya, calculada píxel a píxel en cada cuadro con una raíz entera (`alfa_sombra`).
- **Desenfocar**: solo el fondo de la pantalla de bloqueo, una vez, reducido a un octavo (`usuario/src/desenfoque.rs`).
- **Copiar a la pantalla**: lo dañado, por filas (x86-gpu), al GOP o a la virtio-gpu (que además lo transfiere).
- **Sin SIMD en ARM**: los programas de DashOS se compilan para `aarch64-unknown-none-softfloat`. El kernel guarda
  los registros de NEON por hilo (C28), pero el compilador nunca los usa. En x86 sí sale SSE2.

El compositor ya dice cada 10 s cuántos cuadros compuso y cuánto tardó (E8). Para tener números comparables se midió
la misma escena fuera de DashOS (`medir_componer_1080` en `wayland/src/tests.rs`): 1920 × 1080, un fondo a pantalla
entera y cuatro ventanas (dos XRGB, una ARGB opaca con esquinas, una ARGB a medias), con sombras.

| Caso (1920 × 1080) | PC, Ryzen 9 9955HX (SSE2) | PC sin vectorizar | PC con AVX-512 | aarch64 en QEMU (TCG) |
|---|---|---|---|---|
| Pantalla entera, 4 ventanas, sombras | 3,0 ms | 3,6 ms | 1,5 ms | 13,4 ms |
| Lo mismo sin sombras | 1,3 ms | 1,9 ms | 0,6 ms | 8,2 ms |
| Solo el fondo, entero | 0,65 ms | 0,95 ms | 0,17 ms | 3,4 ms |
| Arrastrar una ventana de 800 × 600 | 1,2 ms | 1,3 ms | 0,6 ms | 4,0 ms |
| Cuatro zonas del fondo de 256 × 256 | 0,13 ms | 0,16 ms | 0,07 ms | 0,68 ms |
| Una letra | 1 µs | 1 µs | 1 µs | 21 µs |
| Copiar 8 MB (lo mínimo de tocar la pantalla entera) | 0,15 ms | — | 0,12 ms | 0,5 ms |

Lo que se lee:

- **Un fondo que cambia entero cada cuadro obliga a recomponer la pantalla entera**: 3 ms por cuadro en un núcleo
  rápido (18 % de un núcleo a 60 cuadros), sin contar lo que cueste calcular el fondo.
- **Las sombras son más de la mitad** de un cuadro entero (3,0 frente a 1,3 ms): se recalculan píxel a píxel en cada
  cuadro aunque la ventana no cambie.
- **Vectorizar da de 1,2 a 2 veces**; con AVX-512, el doble. En ARM hoy no hay nada de eso.
- **Por daños, lo normal es casi gratis**: una letra, un microsegundo; cuatro zonas animadas del fondo, 0,13 ms.
- La Pi no se midió (no hay placa a mano). La columna de TCG no es una Pi: es la CPU del PC emulando un Cortex-A72.

## 2. Caminos para usar la GPU

### (a) virtio-gpu 3D en QEMU (virgl, Venus)

QEMU da al invitado una virtio-gpu cuyo 3D hace la GPU del PC. Comprobado el 17/11 en shiva: QEMU 11.1 trae
`virtio-gpu-gl` (virgl), `virtio-vga-gl`, `virtio-gpu-rutabaga` (Venus y gfxstream) y `egl-headless`, para aarch64 y
x86-64, y el Mesa de Arch Linux ARM de la caja trae el controlador virgl (no el de Venus).

Por la virtio-gpu 3D no viajan llamadas de GL: viajan **flujos de órdenes de Gallium** (virgl, con los sombreadores
en TGSI) o **Vulkan serializado** (Venus). Quien los escribe es Mesa. Para que DashOS hablara virgl o Venus por sí
mismo:

- **virgl desde DashOS**: un codificador de ese flujo, el estado de Gallium y sombreadores en TGSI escritos a mano.
  El flujo no tiene especificación pública: solo está en el código de Mesa y virglrenderer, así que haría falta que
  unos analistas lo escribieran antes (como `docs/spec/hw/v3d.md`). Unas 2.000 a 3.000 líneas, más la especificación.
  Y **solo sirve en QEMU**: ningún PC ni ninguna Pi habla virgl.
- **Venus desde DashOS**: es portar Mesa (X14 de S34): grande y pide la capa POSIX.

No se recomienda ninguno de los dos para el escritorio. La virtio-gpu 3D sí sirve, y mucho, **como la GPU del
dominio de gráficos en QEMU** (b): el dominio lleva Mesa y habla virgl sin que DashOS lo entienda.

**virgl o rutabaga (Venus)**: virgl. Funciona ya con el Mesa del invitado (medido abajo) y da GL ES 3.2, que es lo que
pide componer. Venus hará falta cuando el servidor use Vulkan; no se comprobó si el rutabaga de este QEMU trae Venus
ni hay Venus en el Mesa de la caja.

### (b) La GPU de verdad, por el dominio de gráficos (S34, S35, X12)

Un Linux mínimo con el controlador de la GPU y Mesa. **DashOS no le manda GL ni Vulkan: le manda la escena**, y el
dominio compone con la GPU y saca el cuadro a la pantalla. Así:

```
clientes Wayland ──wl_shm / dmabuf──► compositor de DashOS ──escena por cuadro──► servidor de composición
 (memoria compartida)                  (quién está dónde,       (capas, recortes,     (dominio de gráficos:
                                        daños, foco, entrada)    sombras, efecto       Mesa, GLES/Vulkan, KMS)
                                                                 del fondo, hora)  ──► pantalla
```

- **El compositor no cambia de papel**: sigue decidiendo qué se ve y dónde (Wayland, foco, bloqueo, marcas de las
  cajas, S29). Solo cambia quién pinta. El "pintor" pasa a ser una interfaz con dos lados: el de CPU de hoy y el
  remoto.
- **Los búferes de los clientes no se copian por un canal**: son objetos de memoria de DashOS proyectados en el
  dominio (X13). El servidor sube a la GPU solo lo dañado o, mejor, importa la memoria como textura sin copia.
- **El cuadro no vuelve a DashOS**: lo saca el dominio por KMS. Devolverlo cuesta (ver "lectura" abajo).
- **DashOS no porta Mesa para esto**. Mesa en DashOS (X14) sigue haciendo falta para los programas nativos con 3D, no
  para el escritorio.
- **Con la GPU que haya**: Intel, AMD, Nvidia (S34) en el PC; en QEMU, la virtio-gpu 3D de (a). El servidor es el
  mismo programa.

**En QEMU**: en x86 (`q35` con IOMMU), la `virtio-vga-gl` se pasa al dominio con X11, que ya existe y está probado
con IOMMU. Es el mismo camino que en un PC con una Intel. En ARM (`virt`) DashOS no tiene SMMU: pasarla habría que
hacerlo reenviando las órdenes y copiando la memoria de los recursos, solo para QEMU. No se recomienda: la parte de
ARM del servidor ya se prueba en el invitado de la caja (abajo).

**En la Pi 5** (V3D 7.1, sin manual): S34 dice "la Pi, con controladores propios, sin dominio". Hay otra opción: el
mismo servidor en un dominio con la V3D mediada de S25 (camino C: la MMU de la V3D con una tabla sombra de DashOS,
que ya está escrita y probada en el PC). Así la Pi no espera a que DashOS tenga Mesa. Es una decisión del usuario
(abajo). En cualquier caso, en la Pi lo primero es el HVS (c).

### (c) Un camino propio mínimo, sin GPU de 3D

- **Planos del HVS (Pi 3, 4 y 5; G7 de `GRAFICOS_LOOP.md`)**: el chip de la pantalla junta varias capas al sacarlas,
  sin coste. El fondo en un plano y las ventanas en otros: un fondo animado ya no obliga a recomponer las ventanas,
  solo a pintar el fondo. Es lo que más rinde en la Pi y no depende de la V3D. Se prueba solo en la placa.
- **El DMA de la placa** para copiar a la pantalla y rellenar (S21, H1): ahorra la copia (0,15 ms en el PC, más en la
  Pi). No mezcla con alfa.
- **Planos en el PC**: los da el dominio por KMS (b); DashOS solo no los tiene.

### (d) Sin GPU

- **Sombras en caché**: hoy son más de la mitad del cuadro entero. Con la sombra de cada medida guardada (cuatro
  esquinas y cuatro bordes, como un 9-slice), es copiar o mezclar una máscara. Pequeño.
- **NEON en el compositor** (ARM): `mezclar`, rellenar y copiar con NEON, en funciones marcadas para NEON dentro del
  objetivo sin coma flotante. El kernel ya guarda esos registros. Medio. Hay que medirlo en la Pi.
- **Daños mínimos** (ya está) y **un fondo que no daña la pantalla entera** (siguiente punto).

## 3. Fondos animados con poco gasto

**Con GPU**: el fondo animado es un **efecto del servidor**, un sombreador con parámetros y la hora, no un cliente
que pinta píxeles. No pasa memoria de un sitio a otro y no despierta a nadie. Medido abajo: 0,02 a 0,05 ms de CPU por
cuadro con una GPU de verdad, frente a los 2,2 ms que cuesta un plasma parecido en la CPU (y los 3 de recomponer).

**Sin GPU** (el programa `fondo` de la CPU, rama `fondos`), medido con `docs/diseño/pruebas/fondo-cpu/fondo_cpu.rs`:

| Técnica, 1920 × 1080 | PC (SSE2) | aarch64 en QEMU (TCG) | A 10 cuadros por segundo, en el PC |
|---|---|---|---|
| Plasma por píxel, pantalla entera | 2,2 ms | 8,2 ms | 2,2 % de un núcleo, más recomponer |
| Plasma a un cuarto de lado, ampliado | 0,24 ms | 1,4 ms | 0,2 % |
| Ciclo de paleta (el 23 % de la pantalla cicla) | 0,40 ms | 5,5 ms | 0,4 % |

Y lo que cuesta componerlo (tabla de la sección 1): la pantalla entera, 0,65 a 3 ms; por zonas, una décima.

- **Pocos cuadros**: de 10 a 15 por segundo en un fondo tranquilo. El fondo pide su próximo cuadro con el aviso de
  `frame` de siempre.
- **Solo la zona que cambia**: el fondo daña lo que cambia (el agua, la cascada), no la pantalla. En un fondo hecho
  para esto, el 10 al 25 %.
- **Ciclo de paleta, como los Amiga**: la imagen son índices; cada cuadro gira una parte de la paleta. Solo se
  repintan los píxeles de los colores que giran, que son también el daño. Es el que mejor queda por coste, y el
  aspecto es de la casa.
- **Vídeo**: descodificado por el hardware (S33, H5: HEVC en la Pi 4 y 5, el dominio en el PC) y presentado como
  plano (HVS) o textura (dominio), sin pasar por la CPU. Si no hay hardware, no hay fondos de vídeo.

**Cuándo se para** (igual con GPU y sin ella; lo decide el compositor, que ya lo sabe):

- **Tapado**: el compositor ya distingue lo que no se ve y sus avisos de `frame` salen a ritmo bajo
  (`avisos_ocultos`). Con el fondo tapado entero, el fondo se queda quieto. Tapado a medias, se pinta solo lo visible.
- **Pantalla apagada o bloqueada**: quieto (el Blanker ya pone negro; el bloqueo enseña otro fondo).
- **Con batería**: a pocos cuadros o quieto, según el perfil de energía (S22). Un ajuste de texto en `ENVARC:` con
  tres valores: siempre, solo con corriente, nunca.
- **Pantalla completa** (un juego, un vídeo): quieto.
- Cada vez que se para o vuelve, nada que ver para el usuario; la orden de S21 dice quién pinta el fondo (GPU o CPU).

## 4. Seguridad y aislamiento

- **La GPU hace DMA.** En el PC, la IOMMU la encierra en la memoria del dominio (X5b, X11). Sin IOMMU no se pasa (S35).
  En la Pi no hay IOMMU delante de la V3D: la confina su propia MMU, con la tabla sombra de S25.
- **El dominio ve la pantalla entera**, también lo que pinta `guardian` (la contraseña). Es lo que ya aceptó S34 para
  el dominio de pantalla. Por eso en ese dominio no corre nada del usuario (S34: la caja Linux va aparte).
- **Qué ve cada programa**: un cliente solo pasa sus búferes al compositor, como hoy. No manda sombreadores ni
  órdenes de GPU. El servidor solo recibe del compositor de DashOS, por su canal; lo que llega se valida (medidas,
  llaves, número de capas, con topes).
- **Los sombreadores de los fondos son código que corre en la GPU y pasa por el compilador de Mesa.** Un sombreador
  malo puede colgar la GPU (un bucle) o buscar un fallo del compilador dentro del dominio de pantalla. Propuesta:
  los fondos de GPU son sombreadores de una lista firmada (en `SYS:` o en paquetes firmados) con parámetros que el
  usuario cambia (colores, velocidad), no texto libre. Y siempre un plazo por cuadro: si se pasa, el fondo vuelve a
  ser una imagen quieta y queda una línea en `Estado:Registros/`.
- **Si el dominio cae**: el compositor vuelve a pintar por CPU (S34) con el mismo pintor de hoy. Por eso el camino
  de CPU no se tira: es la reserva.
- **Con el kit y Wayland**: el kit no cambia (pinta en `wl_shm`). Los programas de la caja Linux siguen saliendo
  como ventanas por el puente (y, con GPU en la caja, por `linux-dmabuf`, S25). Un búfer de dmabuf de la caja es otra
  textura para el servidor.

## 5. Recomendación

1. **Ya, en todas las máquinas: la CPU barata.** Sombras en caché, el fondo animado por zonas y a pocos cuadros, el
   ciclo de paleta, y parar al taparse. NEON en el compositor de ARM. Pequeño y medio; es también la reserva.
2. **El escritorio en la GPU por el dominio de gráficos**, con un servidor de composición propio (en C, sobre EGL y
   GLES; Vulkan después) y un protocolo de escena pequeño entre el compositor y él. DashOS no porta Mesa ni habla
   virgl para esto.
3. **En QEMU se prueba en x86**: la `virtio-vga-gl` (virgl) pasada al dominio con X11 e IOMMU. El mismo camino que una
   Intel o AMD de verdad. No rutabaga por ahora.
4. **En la Pi**: los planos del HVS primero (G7). La V3D, después, con el mismo servidor en un dominio con la V3D
   mediada (S25 C), si el usuario lo acepta.
5. **Los fondos de GPU, de una lista firmada** con parámetros, y siempre con caída a la CPU.

Pasos, de menos a más:

| Paso | Qué | Dónde | Tamaño | Se prueba |
|---|---|---|---|---|
| G1 | Sombras en caché; NEON en `mezclar`, rellenar y copiar | `wayland/`, compositor | Pequeño y medio | `medir_componer_1080`; en la Pi |
| G2 | Fondo animado por zonas, a pocos cuadros, ciclo de paleta, parar al taparse | `fondo` (rama `fondos`), escena | Pequeño | En QEMU: CPU del fondo y del compositor en reposo |
| G3 | El pintor como interfaz (CPU hoy) y el protocolo de escena | `wayland/`, compositor | Medio | El mismo cuadro por los dos pintores, píxel a píxel |
| G4 | El servidor de composición: `componer_gl` hecho servidor (protocolo, texturas desde la memoria compartida, KMS, caída) | dominio, zona GPL aparte | Medio | Hecho el prototipo; falta el protocolo |
| G5 | El dominio de gráficos de x86 con PCI, DRM, Mesa y la `virtio-vga-gl` pasada (X12 con una GPU de QEMU) | rama `x86` | Grande | Un cuadro del escritorio de DashOS compuesto en el dominio, CPU antes y después |
| G6 | Memoria compartida sin copia con el dominio (X13) e importar búferes sin copia | kernel y monitor | Medio | Subida de una ventana sin copia en la CPU |
| G7 | Planos del HVS en la Pi (G7 de `GRAFICOS_LOOP.md`) | compositor y HVS | Medio | En la Pi |
| G8 | En un PC: la integrada de pegaso o shiva | dominio | Medio | En el PC |

## Lo medido del prototipo

`caja-linux/invitado/componer_gl.c` es el servidor sin protocolo: compone con GLES 2 la misma escena que la tabla de
la sección 1 (fondo animado por un sombreador, cuatro ventanas con sus sombras, alfa premultiplicado) y comprueba
píxeles contra la misma cuenta hecha en la CPU. `tools/gpu_componer.py pc`, `qemu` y `hostil`. Cada cuadro se compone
entero y se espera a que acabe (`glFinish`). "CPU" es la del proceso, todos sus hilos. Las cifras de QEMU son de dos
vueltas; la segunda, con `make prueba` y `arranque-virt` corriendo a la vez.

| Dónde | Fondo solo | Escritorio | + subir una ventana de 800 × 600 | + devolver el cuadro |
|---|---|---|---|---|
| PC, Radeon integrada (radeonsi) | 0,05 ms CPU (0,39 ms el cuadro) | 0,04 ms (0,82) | 0,26 ms (1,41) | 0,41 ms (2,57) |
| PC, Intel Arc A310 (iris) | 0,02 ms (0,45) | 0,03 ms (0,41) | 0,11 ms (0,95) | 0,36 ms (1,53) |
| PC, llvmpipe (la CPU, 32 hilos) | 20 ms (0,9) | 39 ms (1,6) | 36 ms (2,1) | 36 ms (2,1) |
| Caja aarch64 en QEMU (TCG), virgl con la Radeon | 0,14 a 0,15 ms (0,50) | 0,38 a 1,05 ms (1,2 a 2,2) | 2,2 a 3,5 ms (8 a 10) | 6,9 a 7,5 ms (7) |
| Caja aarch64 en QEMU (TCG), llvmpipe | 2.796 ms (492) | 2.529 ms (634) | — | — |

Antes y después, con la misma escena (fondo animado, cuatro ventanas, sombras), en CPU por cuadro:

| | Antes: el compositor de DashOS por CPU | Después: el servidor con GPU |
|---|---|---|
| PC (x86-64 nativo) | 3,0 ms componiendo + 2,2 ms el plasma | 0,03 a 0,04 ms |
| aarch64 en QEMU (TCG) | 13,4 ms componiendo + 8,2 ms el plasma | 0,38 a 1,05 ms (virgl) |

Lo que se lee:

- **Con GPU, la CPU casi no hace nada** aunque el fondo cambie entero en cada cuadro: más de 100 veces menos en el
  PC, y de 20 a 55 veces menos en QEMU con virgl.
- **Subir píxeles y devolver el cuadro es lo caro**, sobre todo con virgl (2 a 3,5 y 7 ms). De ahí dos reglas del
  diseño: el cuadro lo saca el dominio (no vuelve a DashOS) y los búferes de los clientes se importan sin copia o se
  sube solo lo dañado.
- **virgl funciona ya**: el Mesa de la caja lo coge solo con la `virtio-gpu-gl-device`, sin tocar nada.
- **El sombreador en la CPU no tiene sentido**: llvmpipe gasta 20 ms de CPU por cuadro en el PC.
- Memoria del servidor de prueba en el PC: como mucho 80 MB residentes. Sin medir dentro de un dominio.

## Lo que decide el usuario

1. ¿El escritorio acelerado por el dominio de gráficos con un servidor de composición propio (y no Mesa en DashOS ni
   virgl desde DashOS)? Recomendado: sí.
2. ¿La prueba en QEMU por x86 (virtio-vga-gl pasada al dominio con IOMMU), y ARM sin 3D en QEMU? Recomendado: sí.
3. ¿En la Pi 5, la V3D en un dominio con la MMU mediada (S25 C) en vez de "sin dominio" (S34)? Recomendado: sí, pero
   después de los planos del HVS.
4. ¿Fondos de GPU solo de una lista firmada, con parámetros, o también sombreadores escritos por el usuario (en el
   modo de desarrollo)? Recomendado: lista firmada; los propios, solo en el modo de desarrollo.
5. ¿Animación del fondo con batería: quieta, a pocos cuadros, o igual? Recomendado: a pocos cuadros, con el ajuste de
   tres valores.
6. El número de este documento (ya hay un S37).

## Lo que no se comprobó

- **Nada en una Pi ni en un PC con DashOS.** Las medidas de CPU son del PC (Ryzen 9 9955HX) y de un Cortex-A72
  emulado; no de una Pi.
- **DashOS no ha compuesto aún nada en la GPU**: el prototipo es el servidor fuera de DashOS (en el PC y en el Linux
  de la caja en QEMU, con virgl). Falta el protocolo, el dominio de x86 con la GPU pasada y el pintor remoto (G3 a G6).
- Venus por rutabaga, y la `virtio-vga-gl` pasada a una máquina de DashOS con X11.
- Lo que gasta un dominio con Mesa en memoria y en arranque.
- Si los planos del HVS de la Pi 5 admiten paletas (para el ciclo de paleta sin CPU).
- Cuánto gana NEON en el compositor y las sombras en caché: estimado por las medidas del PC, sin hacer.

## Cómo encaja

S9 (fases A, B y C; el compositor), S21 (cada trabajo a su chip; H2 y H6), S25 (la GPU de la caja Linux; la V3D
mediada), S33 (vídeo), S34 (el dominio de gráficos; X12 a X15), S35 (dominios), S22 (energía), S6 (registros), S27 y
S10c (el fondo, rama `fondos`), `GRAFICOS_LOOP.md` (G7 y G8).
