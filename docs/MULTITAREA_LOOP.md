# Multitarea: medir y mejorar (plan y bitácora)

Encargo del usuario (2026-11-09): mejorar y optimizar la multitarea de DashOS, "tenemos que ser punteros". Midiendo,
no en teoría, y si se puede con programas en Rust, C y Dash. En loop hasta no encontrar cómo mejorarla más.

Worktree `.worktrees/multitarea`, rama `multitarea` (sale de `adf`, PR #10). Diseño de partida:
`docs/diseño/S2-planificador.md`. Código: `kernel/src/base/planificador.rs`, `kernel/src/temporizadores.rs`,
`kernel/src/reloj.rs`, `kernel/src/procesos.rs`.

## Cómo estaba (2026-11-09)

- Una cola por núcleo; los hilos no cambian de núcleo. Los procesos van por turno a los núcleos 1 a 3; el 0 solo
  lleva el arranque, el repartidor e `inicio`.
- Clase justa por tiempo virtual (peso fijo 1024 para todos) y clase de plazo (EDF con presupuesto).
- Tic fijo de 100 Hz en todos los núcleos, también los ociosos.
- Temporizadores con la resolución del tic: 10 ms, y 10 ms es también el periodo más corto.
- Al despertar, un hilo toma `max(su virtual, el mínimo de la cola)`, sin contar el que corre: si la cola está vacía,
  conserva su tiempo virtual viejo. Sospecha: un hilo que duerme mucho y luego calcula deja sin CPU al que corría.
- Despertar en el mismo núcleo no replanifica hasta el tic (salvo hilos de plazo).
- Sin préstamo de tiempo del cliente al servidor, sin prioridad por foco y sin cambio de frecuencia.

## Cómo se mide

`make medir` (en `kernel/`): imagen de raspi4b con la opción `medir`. Solo lanza `consola` y el orquestador `medir`,
que recibe un paquete con los ELF de las pruebas (Rust, C y Dash) y las corre una detrás de otra. Cada resultado es
una línea `[medir] clave=valor ...`. `tools/medir.py` repite la imagen varias veces, saca medianas a JSON y compara
con una línea base guardada (`docs/medidas/*.json`, una por paso).

Ojo: QEMU (TCG) no es una Pi. Los tiempos absolutos no valen; las comparaciones antes/después en la misma máquina,
sí, con varias vueltas. Lo que no dependa del tiempo (reparto, quién se queda sin CPU, interrupciones en núcleos
ociosos) vale tal cual.

Pruebas:

| Prueba | Qué mide | Lenguajes |
|---|---|---|
| P1 ida y vuelta | µs por ida y vuelta de un mensaje entre dos procesos, mismo núcleo y distinto | Rust, C, Dash |
| P2 reparto | N procesos de cálculo (N = 1 a 8): trabajo total y el peor contra el mejor | Rust, C, Dash |
| P3 temporizador | retraso de un temporizador de 1 ms y de 10 ms (mediana, p99, máx), sin carga y con carga | Rust |
| P4 dormilón | uno duerme 2 s y luego calcula: cuánto tiempo deja sin CPU a otro de su núcleo | Rust |
| P5 despertar | latencia de despertar con carga en el mismo núcleo | Rust |
| P6 ociosos | interrupciones por segundo en un núcleo sin trabajo | kernel |

## Pasos

| Paso | Qué | Estado |
|---|---|---|
| M0 | Banco de medidas y línea base | hecho |
| M1 | Despertar justo: el dormilón no acapara (lag acotado, como EEVDF) | hecho |
| M2 | Temporizadores de alta resolución (el vencimiento programa el reloj) | hecho |
| M3 | Despertar en el mismo núcleo replanifica si el despertado va antes | hecho (con M1) |
| M4 | Equilibrado entre núcleos (robo de trabajo, migración con la FPU) y el núcleo 0 para programas | hecho |
| M5 | Sin tic en núcleos ociosos | hecho |
| M6a | Copias de usuario sin cambiar de espacio (menos cambios de ASID por llamada) | hecho |
| M6 | Paso directo en canales (el cliente cede su núcleo al servidor que espera) | pendiente |
| M7 | Préstamo de tiempo del cliente al servidor (seL4 MCS) | pendiente |
| M8 | Pesos: foco del compositor y procesos de fondo (con permiso) | pendiente |
| M9 | Lo que salga de medir | abierto |
| L | Las mismas pruebas en C en Raspberry Pi OS, en el mismo QEMU `raspi4b` (pedido del usuario) | hecho |
| M2b | Llamada 30 "dormir": el hilo duerme en la lista de su núcleo, sin canal ni temporizador ni pasar por el 0 | hecho |
| R | Memoria: ocupada al arrancar, el kernel y lo que cuesta cada proceso, en DashOS y en Linux (pedido del usuario) | en curso |
| F | Sistema de archivos: escritura y lectura seguidas, muchos ficheros pequeños, metadatos y directorios; RedoxFS de DashOS contra ext4 de Linux en el mismo disco emulado (pedido del usuario) | pendiente |
| P3 | La Pi 3 (su controlador de interrupciones) para medir en una placa de verdad: el usuario tiene una | pendiente |
| A | Artefacto con las comparativas (pedido del usuario) | pendiente |

Cada paso: medir antes, cambiar, medir después, `make prueba` (raspi4b) y `make arranque-virt` sin fallos, apuntar
aquí los números y commit local.

## Bitácora

### 2026-11-09 — M0: banco de medidas y línea base

- Kernel: llamada 29 (núcleo de quien llama; tics, cambios de hilo y µs ociosos de cada núcleo) y la opción
  `medir` (solo `consola` y `medir`, más un objeto de memoria con el paquete de los hijos).
- `usuario/src/bin/medir.rs` (orquestador), `medir_hijo.rs` (Rust), `usuario/medir/hijo.c` (C sin biblioteca,
  clang + ld.lld) y `usuario/medir/hijo.dash` (`./dash --dashos`; hizo falta `make -C dashc dashc-a64` en
  FreeMEMLang). Mismo protocolo de texto y la misma unidad de trabajo (256 pasos de un congruencial de 32 bits).
  El de Dash mide en milisegundos y no sabe su núcleo.
- `tools/medir.py`: paquete, correr (QEMU raspi4b directo, hasta "[medir] fin"; unos 45 s por arranque) y comparar.
- Línea base, `docs/medidas/m0-base.json` (3 arranques, medianas):

| Medida | Valor |
|---|---|
| Ida y vuelta, núcleos distintos (Rust / C / Dash) | 97 / 95 / 91 µs |
| Ida y vuelta, mismo núcleo (Rust / C) | 108 / 101 µs |
| Ida y vuelta con 4 calculando, núcleos distintos / mismo | 107 / 890 µs |
| Reparto, 4 calculando: el peor contra el mejor | 0,50 (dos comparten núcleo, el 0 sin usar) |
| Reparto, 8 calculando | 0,65; trabajo total igual que con 3 (núcleo 0 sin usar) |
| Hueco más largo calculando con otro en el núcleo | 20 ms (dos tics) |
| Espera de 1 ms con temporizador: retraso mediano | 9,1 ms (sin carga y con carga) |
| Tics por segundo en un núcleo sin trabajo | 100 |
| Dormilón (2 s dormido, luego 1 s calculando) | el otro pierde 0,5 s, hueco máximo 20 ms: reparto justo |

- Lo del dormilón no se confirma: la sospecha de que un hilo que duerme acapara al despertar era falsa en esta
  prueba. Queda la de 890 µs: en el mismo núcleo con carga, el despertado no pasa delante del que calcula.
- Sospecha, sin comprobar: QEMU vacía su TLB en cada cambio de ASID en TTBR0, y DashOS cambia TTBR0 en cada
  llamada (PAN por software). Eso inflaría las idas y vueltas en QEMU respecto a una Pi. Linux en una A72 haría lo
  mismo si tiene SW_TTBR0_PAN; se mira en L.
- Iguales en Rust, C y Dash: lo que cuesta es el kernel, no el lenguaje (Dash calcula un 15 % menos por unidad).

### 2026-11-09 — M1 y M3: despertar con crédito acotado, y adelantarse al que corre

- El fallo existía: una traza en el kernel (quitada; imprimir dentro del planificador colgaba el arranque) vio
  despertar un hilo con 1 ms de tiempo virtual mientras el que corría llevaba 1.295 ms. Al despertar solo se miraba
  el mínimo de la cola, sin contar el que corre: con la cola vacía, el dormido conservaba su tiempo viejo y podía
  quedarse el núcleo más de un segundo. La prueba del dormilón de M0 no lo reprodujo (no sé por qué; queda abierto).
- Ahora cada núcleo lleva `min_virtual`, el mínimo de la clase justa entre los listos y el que corre, que nunca
  baja. Un hilo nuevo empieza ahí; uno que despierta, como mucho medio tic por debajo (el "lag" acotado de EEVDF).
- M3: si el que despierta tiene menos tiempo virtual que el que corre (contando lo gastado desde la última
  decisión), se avisa al propio núcleo y se replanifica al momento, sin esperar al tic.
- `docs/medidas/m1.json` contra `m0-base.json` (3 arranques):

| Medida | M0 | M1 |
|---|---|---|
| Ida y vuelta, mismo núcleo, con 4 calculando | 890 µs | 200 µs (-77 %) |
| Ida y vuelta, núcleos distintos, con 4 calculando | 107 µs | 122 µs (+14 %, ruido o el precio de la justicia) |
| Dormilón: hueco más largo del otro | 20 ms | 10,5 ms |
| Resto | | igual (±7 %) |

- 200 µs con carga frente a 108 sin ella: el ping, el eco y el que calcula se reparten el núcleo a partes iguales,
  que es lo justo. Bajarlo más es cosa de M6/M7 (el servidor trabaja con el tiempo del cliente).
- `make prueba` y `make arranque-virt`: bien.

### 2026-11-09 — M2: temporizadores de alta resolución

- El planificador del núcleo 0 programa su reloj también para el vencimiento más próximo (`temporizadores::proximo`,
  que no cuenta mientras el repartidor tiene algo pendiente, para no provocar una lluvia de interrupciones). Un
  temporizador nuevo que vence antes de lo programado avisa al núcleo 0. El periodo más corto baja de 10 ms a
  100 µs. El reintento de un aviso que no cupo sigue a un tic.
- `docs/medidas/m2.json` contra `m1.json`: espera de 1 ms, retraso mediano de 9.085 a 106 µs sin carga y de 9.066
  a 132 µs con carga; p99 de 9,3 ms a 0,3 ms. Lo demás, igual (±4 %; ida y vuelta en el mismo núcleo con carga
  +11 %, dentro del ruido de ese caso).
- La llamada 29 cuenta todas las interrupciones del núcleo, no solo las del reloj ("tics" en el banco).
- `make prueba` y `make arranque-virt` (con el sonido): bien. Referencia del kernel al día.

### 2026-11-09 — M4: robo de trabajo entre núcleos, el núcleo 0 para programas y holgura en los temporizadores

- Un proceso nuevo va al núcleo con menos carga (el 0 incluido; entre iguales, por turno desde el 1). Un núcleo sin
  nada de la clase justa se lleva un hilo del más cargado si este tiene dos o más; uno con dos menos que otro,
  también. En su tic, con uno menos basta (rotación: cinco hilos en cuatro núcleos se turnan). Si un núcleo se queda
  con dos o más y hay otro en el ocioso, se le avisa para que robe sin esperar a su tic.
- Solo se mueven hilos de procesos, de la clase justa, ya con su contexto guardado (`en_cambio` hasta `fin_cambio`)
  y sin sus registros en la FPU de su núcleo. Entre núcleos con trabajo, solo los que fueron expulsados calculando:
  los que despiertan (los que hablan con otros) se quedan donde están. El tiempo virtual se traslada respecto al
  mínimo de cada núcleo.
- Fallos encontrados al revisar, ya corregidos: el núcleo de un hilo se leía sin cerrojo al despertarlo o matarlo
  (ahora es atómico y `cola_de` lo comprueba con el cerrojo tomado); `actual`, `debe_morir` y `recoger` miraban el
  número de núcleo con las IRQ abiertas; tras el cambio de contexto se usaba el núcleo de antes.
- Holgura en los temporizadores (como el "timer slack" de Linux): 1/16 del intervalo, como mucho 1 ms, para que
  muchos temporizadores se junten en una interrupción. Sube el retraso mediano de 1 ms de 106 a 177 µs.
- El fallo de vez en cuando de C43 (una página menos al acabar) no era una fuga: con más hilos vivos a la vez, la
  zona de pilas crea una tabla de nivel 3 que no se devuelve. La prueba la cuenta aparte (`mapa::tablas_pilas`).
  En 8 vueltas de `make prueba`, todas bien, 3 con tabla nueva. C14 falló una vez en 11 (845 µs de desviación con
  límite de 1.000; sin M4 ya dio 595): parece ruido de QEMU, no está comprobado.
- `docs/medidas/m4.json` contra `m2.json` (3 arranques):

| Medida | M2 | M4 |
|---|---|---|
| Reparto, 4 calculando (Rust / C / Dash): el peor contra el mejor | 0,50 / 0,50 / 0,50 | 0,99 / 1,00 / 1,00 |
| Reparto, 4 calculando: trabajo total | | +15 % / +19 % / +28 % |
| Reparto, 5 calculando | 0,50 | 0,89 (+16 % de trabajo) |
| Reparto, 6 calculando | 0,98 | 0,80 (+17 % de trabajo) |
| Reparto, 8 calculando | 0,66 | 0,95 (+16 % de trabajo) |
| Ida y vuelta, mismo núcleo, sin carga (Rust / C) | 111 / 111 µs | 91 / 92 µs |
| Ida y vuelta con 4 calculando, donde los ponga el sistema | | 125 µs |
| Dormilón: tiempo perdido por el otro | 505 ms | 3 ms (ya no comparten núcleo) |
| Espera de 1 ms: retraso mediano sin / con carga | 106 / 132 µs | 177 / 196 µs (la holgura) |
| Espera de 1 ms con carga: p99 / máx | 324 / 357 µs | 299 / 988 µs |
| Ociosos | 100 tics/s | igual (eso es M5) |

- Lo que empeora: con 6 hilos en 4 núcleos el reparto baja de 0,98 a 0,80 (2, 2, 1, 1: la rotación no llega a
  igualarlos), aunque se hace un 17 % más de trabajo al usar el núcleo 0. Queda para M9.
- `make prueba` (8 vueltas) y `make arranque-virt` (con el sonido): bien.

### 2026-11-09 — M5: sin tic en los núcleos ociosos

- Cuando un núcleo pasa a su hilo ocioso, ya en su bucle de espera (`ocioso()`), el reloj se programa a 1 s en lugar
  de 10 ms (el registro del temporizador es de 32 bits). Lo que le da trabajo ya le avisa: despertar a un hilo suyo,
  un robo pendiente (`avisar_ocioso`), un temporizador en el núcleo 0 (M2), una interrupción, y ahora también un hilo
  nuevo en su cola. El hilo de arranque del núcleo 0, que es su ocioso, corre `inicio` y las pruebas esperando tics:
  hasta entrar en `ocioso()` conserva el tic (la primera versión rompió C8 por eso). Sin avisos entre núcleos
  (la medida de C17), también.
- `docs/medidas/m5.json` contra `m4.json`: interrupciones por segundo en núcleos sin trabajo de 97-101 a 0-5 (el 0
  atiende la consola y el repartidor). Lo demás, igual dentro del ruido (ida y vuelta ±5 %, reparto ±3 %; el
  temporizador con carga, p99 de 299 a 494 µs, ruido de ese caso; queda mirado en M9).
- `make prueba` (3 vueltas) y `make arranque-virt` (con el sonido): bien.

### 2026-11-09 — L: Linux de la Raspberry Pi en el mismo QEMU

- `tools/medir_linux/banco.c`: las mismas pruebas en C para Linux (fork, tuberías, `sched_setaffinity`,
  `clock_nanosleep`, `/proc/interrupts`), con la misma unidad de trabajo y el contador de la CPU. Va como `/init` de
  un initramfs (`tools/medir_linux/hacer.sh`, compilador musl de Bootlin) con el `kernel8.img` de Raspberry Pi OS
  (6.18.33-v8+, del repositorio raspberrypi/firmware; nada de eso va al repo: `~/.cache/dashos-linux`). El árbol de la
  Pi lleva el PL011 para el Bluetooth y Linux no deja consola: una copia sin ese nodo, consola `ttyAMA1`.
  `tools/medir.py linux JSON -n 3`.
- `docs/medidas/linux.json` (3 arranques) contra DashOS con M5:

| Medida | DashOS (M5) | Linux 6.18 (Raspberry Pi OS) |
|---|---|---|
| Ida y vuelta, núcleos distintos, sin carga | 91-99 µs | 33 µs |
| Ida y vuelta, mismo núcleo, sin carga | 91-95 µs | 80 µs |
| Ida y vuelta con 4 calculando | 122 µs | 119 µs |
| Reparto, 4 / 5 / 6 / 8 calculando | 0,99 / 0,89 / 0,80 / 0,93 | 0,99 / 0,89 / 0,81 / 0,93 |
| Trabajo total con 4 calculando | 17,0 M | 17,0 M |
| Espera de 1 ms, retraso mediano sin / con carga | 178 / 208 µs | 81 / 123 µs |
| Interrupciones por segundo en núcleos sin trabajo | 0-5 | 5-20 |
| Dormilón: tiempo perdido por el otro | 3 ms (núcleos distintos) | 516 ms (mismo núcleo, fijados) |

- Experimento (`--features medir,medir_asid`, no seguro, `docs/medidas/m9-asid.json`): si la tabla vacía del kernel
  conserva el ASID del proceso, la ida y vuelta baja de 91 a 26 µs (-71 %, por debajo de Linux) y la espera de 1 ms
  de 178 a 119 µs. Lo que cuesta en QEMU es que vacía su TLB entera con cada cambio de ASID, y DashOS cambia de ASID
  al entrar y salir del kernel y en cada copia de usuario (PAN por software). En una Pi de verdad cambiar de ASID no
  vacía nada: esa parte de la diferencia es del emulador. Pero hay una mejora real que también la quita: copiar sin
  cambiar de espacio (siguiente paso, M6a).

### 2026-11-09 — M6a: copias de usuario sin cambiar de espacio

- `usuario::leer` y `escribir` copian página a página por el mapa directo del kernel, con el espacio del proceso
  tomado mientras dura cada página (nadie puede quitarla entre tanto). Los permisos se miran en la tabla del proceso
  como los miraría la MMU para EL0 (`Espacio::para_copiar`: válida, con AF, de EL0 y, para escribir, no de solo
  lectura). Solo memoria normal con caché; con otra (aparatos, DMA sin caché) se sigue por LDTRB/STTRB. Además de
  quitar dos cambios de ASID por copia, copia con `memcpy` en lugar de byte a byte: en una Pi de verdad también gana
  en mensajes grandes.
- `docs/medidas/m6a.json` contra `m5.json`: ida y vuelta de 91-99 a 58 µs (-37 a -42 %, igual en Rust, C y Dash);
  con 4 calculando, de 122 a 91 µs (Linux: 119). Lo demás igual (reparto ±2 %, temporizador ±3 % en la mediana).
- `make prueba` (3 vueltas): bien. `make arranque-virt` (en otro worktree, con el sonido): bien.

### 2026-11-10 — M2b: llamada 30, dormir

- Dormir costaba seis llamadas (crear un canal y un temporizador, leer, cerrar tres llaves) y el aviso pasaba por el
  repartidor del núcleo 0 y un aviso entre núcleos. Ahora `dormir_us` (llamada 30) apunta el momento en el hilo y lo
  deja en `dormidos` de su núcleo; el planificador de ese núcleo programa su reloj para el primero y lo despierta él
  mismo, como al despertar (crédito acotado de M1). Un hilo dormido está bloqueado: no se roba. Si lo matan,
  `despertar_en` lo saca de la lista. `dashos::dormir_us` usa la llamada 30 y, si el kernel no la tiene, lo de antes.
- `docs/medidas/m2b.json` contra `m6a.json`: espera de 1 ms, retraso mediano de 172 a 35 µs sin carga (Linux: 81) y de
  202 a 46 µs con 4 calculando (Linux: 123); p99 sin carga de 432 a 171 µs (Linux: 203). Con carga, el p99 baja de
  772 a 510 µs pero sigue peor que Linux (228): pendiente para M9. Lo demás igual (±5 %).
- `make prueba` (3 vueltas) y `make arranque-virt` (con el sonido): bien.
