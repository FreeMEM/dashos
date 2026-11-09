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
| M1 | Despertar justo: el dormilón no acapara (lag acotado, como EEVDF) | pendiente |
| M2 | Temporizadores de alta resolución (el vencimiento programa el reloj) | pendiente |
| M3 | Despertar en el mismo núcleo replanifica si el despertado va antes | pendiente |
| M4 | Equilibrado entre núcleos (robo de trabajo, migración con la FPU) y el núcleo 0 para programas | pendiente |
| M5 | Sin tic en núcleos ociosos | pendiente |
| M6 | Paso directo en canales (el cliente cede su núcleo al servidor que espera) | pendiente |
| M7 | Préstamo de tiempo del cliente al servidor (seL4 MCS) | pendiente |
| M8 | Pesos: foco del compositor y procesos de fondo (con permiso) | pendiente |
| M9 | Lo que salga de medir | abierto |
| L | Las mismas pruebas en C en Raspberry Pi OS, en el mismo QEMU `raspi4b` (pedido del usuario) | pendiente |
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
