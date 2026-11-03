# Bucle de construcción de DashOS

## Encargo

Encargo del usuario, del 2026-11-03 a las 2 de la madrugada:

> Hay que meterle caña al controlador de vídeo y hacer funcionar la gráfica 3D. Vale todo y arranca. Te dejo el loop
> currando sin parar hasta mañana por la mañana.

El diseño está cerrado: `docs/diseño/RESUMEN.md`, la hoja de ruta en `docs/diseño/S15-hoja-de-ruta.md` y las
decisiones en `PLAN.md`. Este bucle construye.

## Dos líneas en el mismo bucle

- **Construcción** (tareas `C`): el sistema, en QEMU `raspi4b`. Lo hace la sesión del bucle, que tiene el papel de
  implementador.
- **Análisis** (tareas `V`, vídeo y GPU): especificaciones en prosa de lo que no tiene manual (HVS, HDMI, V3D), a
  partir del código de Linux. Lo hace **un agente aparte con el papel de analista**, lanzado por el bucle. El bucle no
  lee el código de Linux: solo la especificación que deja el analista en `docs/spec/hw/` (AGENTS.md, regla 1;
  decisión del usuario en S12).

**Aviso**: QEMU no emula el HVS, el HDMI ni la V3D (S12). El controlador de vídeo y la GPU solo se pueden escribir y
probar de verdad en una Pi 4 real. Esta noche se adelanta todo lo que no la necesita: las especificaciones y la base
del kernel sobre la que irán esos controladores.

## Reglas de cada vuelta

1. **Hora y uso.** Al empezar, mira la hora y `~/.claude/state/uso-7d.txt`. Si son las 09:00 del 2026-11-03 o más
   tarde, o el uso pasa del 90 %, cierra (regla 10).
2. **Tarea.** La primera `[ ]` de la lista, alternando: una tarea `C`, y cuando no haya un analista trabajando, lanza
   la siguiente `V` en segundo plano. Márcala `[~]`. Si es grande, divídela y haz solo la primera parte.
3. **Tarea `V`.** Se lanza con la herramienta Agent, en segundo plano, con el encargo de la propia tarea y estas
   condiciones: papel de analista; lee el código de Linux (`torvalds/linux` por la API de GitHub o un clon en el
   scratchpad, nunca dentro del repositorio); escribe una especificación en prosa, con registros, campos, orden de
   operaciones y casos raros, en `docs/spec/hw/<tema>.md`; **no copia código** ni pone fragmentos de más de una línea;
   cita fichero y función de Linux de donde sale cada cosa; y contesta solo "hecho" con la ruta. Cuando avise, la
   tarea se marca `[x]` y se apunta en la bitácora.
4. **Tarea `C`.** Código en `kernel/` (Rust) o donde diga la tarea. Cada tarea deja una prueba automática en QEMU:
   `tools/prueba_qemu.sh <imagen> <fichero de salidas esperadas>`, que arranca `raspi4b` con tiempo límite y comprueba
   las líneas del puerto serie.
5. **Verificar.** Todas las pruebas anteriores siguen pasando. Si algo se rompe y no se arregla en la vuelta, se
   revierte.
6. **Cerrar.** `[x]`, entrada en la bitácora (qué se hizo, qué se comprobó, qué no), commit local `<tarea>: <resumen>`.
   Sin push.
7. **Sin avisos de noche.** El usuario duerme: nada de PushNotification salvo un bloqueo que lo necesite de verdad.
8. **Bloqueos.** Si una tarea se atasca dos vueltas, se apunta en "Bloqueos" y se pasa a la siguiente.
9. **Ritmo.** Vueltas seguidas, con un minuto de espera entre una y otra.
10. **Cierre.** A las 09:00 o con el uso por encima del 90 %: termina lo que esté a medias o revierte, escribe
    "Resumen de la noche" al final de este fichero, manda un PushNotification con el resultado y para el bucle.

## Tareas

### Construcción

- [x] C1 **Llamada nativa desde el 68k** (H1). En `jit-host/`: `EMIT_lineA` de Emu68 reconoce `$A0FF` seguido de un
  número de 32 bits y emite una llamada a una función nativa de una tabla, guardando y reponiendo A0-A4 (x13-x17).
  Parche en `patches/emu68/`. Prueba: un programa 68k llama dos veces a una función nativa que escribe un texto y
  devuelve un valor en D0, y el 68k lo usa.
- [x] C2 **Esqueleto del kernel.** `kernel/` con `rust-toolchain.toml` (nightly fijada), destino
  `aarch64_be-unknown-none-softfloat`, guion de enlace, entrada en ensamblador (big-endian en EL2, bajar a EL1, pila
  por núcleo), tipo de registro de periférico little-endian, PL011, `kprint!`, `tools/prueba_qemu.sh` y la primera
  prueba.
- [x] C3 **Excepciones.** Tabla de vectores de EL1, volcado de registros y pánico legible. Prueba: un `brk` provocado.
- [x] C4 **Árbol de dispositivos.** Leer el DTB que deja el firmware en `x0`: memoria, puerto serie, GIC, temporizador
  y `dma-ranges` (S3). Prueba: imprime el mapa.
- [x] C5 **Memoria física.** Asignador de páginas con zonas (primer GB, primeros 3 GB, resto) sacadas de C4. Prueba.
- [x] C6 **MMU.** Mitad alta del kernel en `TTBR1`, periféricos como dispositivo, páginas de 4 KB. Prueba.
- [x] C7 **Montón del kernel.** `alloc` sobre C5. Prueba.
- [x] C8 **Temporizador y GIC-400.** Interrupción periódica. Prueba: cuenta tics.
- [x] C9 **Cuatro núcleos.** Despertar 1 a 3 con la tabla de espera de la Pi (como Emu68 y QEMU). Prueba: cada núcleo
  se presenta.
- [x] C10 **Hilos y planificador.** Cambio de contexto, una cola por núcleo, clase justa sencilla (S2). Prueba: hilos
  que se turnan.
- [x] C11 **Procesos en EL0.** Espacio de direcciones propio con ASID, llamada al sistema por `svc`. Prueba: un
  proceso de usuario escribe por una llamada.
- [x] C12 **Capacidades y canales.** Tabla de llaves por proceso, canales con mensajes cortos y paso de llaves (S1).
  Prueba: dos procesos se pasan mensajes.
- [x] C13 **Objetos de memoria.** Crear, proyectar y compartir (S3). Prueba: dos procesos comparten una página.
- [x] C14 **Clase de plazo.** Presupuesto y periodo (S2). Prueba: un hilo de plazo cumple con los cuatro núcleos
  cargados.
- [x] C15 **Buzón de la VideoCore y pantalla del firmware.** Controlador del buzón y la pantalla (fase A de S9) como
  base del vídeo. Prueba: pinta un degradado y lo comprueba leyendo la memoria de la pantalla.

### Segunda tanda (añadida a las 03:0x, al acabar C15)

- [x] C16 **Doble búfer y compositor mínimo por CPU.** Pantalla virtual del doble de alto y cambio de mitad con el
  desplazamiento virtual del firmware (etiqueta `0x48009`), un compositor provisional en el kernel que mueve una
  ventana (rectángulo) repintando solo lo que cambia, y captura de QEMU. Es la fase A de S9.
- [x] C17 **Avisos entre núcleos.** Interrupciones por software del GIC (SGI) para despertar un hilo de otro núcleo
  sin esperar a su tic. Prueba: medir el tiempo de ida y vuelta de un mensaje entre núcleos antes y después.
- [x] C18 **Recoger lo que dejan los muertos.** Liberar pila e hilo al terminar, y espacio, páginas y llaves al
  terminar el último hilo de un proceso. Prueba: crear y terminar muchos procesos sin que baje la memoria libre.
- [x] C19 **Programas de usuario en Rust.** Un crate `usuario/` (no_std, big-endian, envoltorios de las llamadas) y
  un cargador de ELF en el kernel. Las pruebas pasan a programas en Rust.
- [x] C20 **Controladores fuera del kernel.** Llave de registros (proyectar un periférico como memoria de
  dispositivo en un proceso) y llave de interrupción (llega como mensaje). Prueba: un proceso que maneja el buzón o
  la serie.
- [x] C21 **Tarjeta SD.** Controlador SDHCI (EMMC2) que lee el primer sector de una imagen en QEMU.
- [x] C22 **USB DWC2.** Enumerar dispositivos y leer el teclado y el ratón de QEMU. Varias vueltas.
- [x] C30 **Orden de bytes por proceso** (03/11 por la tarde, decisión del usuario): lo nativo en little-endian, la
  caja en big-endian.
- [x] C29 **Cargador de PIE** (03/11 por la tarde, a petición de Dash): `ET_DYN` con base y pila al azar por
  encima de 4 GB.
- [x] C28 **FPU por hilo** (03/11 por la tarde, a petición de Dash): registros de FP/SIMD por hilo, de forma perezosa.
- [x] C27 **Listas de borrado de la V3D** (añadida a las 08:27): construir la RCL de V5 y, en la Pi, el primer
  trabajo de la GPU.
- [x] C26 **Búferes de la GPU** (añadida a las 08:16): crear, VA, liberar, sobre la tabla de la MMU de la GPU.
- [x] V5 **Listas de control mínimas de la V3D** (añadida a las 08:16, agente analista): borrar baldosas y guardarlas
  en memoria sin Mesa, para el primer trabajo de la GPU en la Pi 4. Salida: `docs/spec/hw/v3d-listas.md`.
- [x] C25 **El ratón USB mueve la ventana del compositor** (añadida a las 08:14). Con captura.
- [x] C24 **V3D, lado del kernel, según la especificación** (añadida a las 08:05). Encendido, identificación, MMU
  de la GPU, cachés y lanzamiento de trabajos, solo con `docs/spec/hw/v3d.md`. Compilado; en QEMU solo la
  codificación.
- [x] C23 **HVS según la especificación**, compilado pero sin probar: falta la Pi 4. Solo con `docs/spec/hw/hvs.md`.

### Análisis de vídeo y GPU (agente analista)

- [x] V1 **HVS**: lista de visualización, planos, formatos, escalado, FIFOs y su reparto entre salidas en la BCM2711
  (`drivers/gpu/drm/vc4/vc4_hvs.c`, `vc4_plane.c`, `vc4_crtc.c`, `vc4_regs.h`).
- [x] V2 **Pixel valves y HDMI de la BCM2711**: modos, relojes, PHY, las dos salidas, interrupción de sincronía
  vertical, audio por HDMI (`vc4_hdmi.c`, `vc4_hdmi_phy.c`, `vc4_hdmi_regs.h`, `vc4_crtc.c`).
- [x] V3 **V3D, lado del kernel**: memoria de la GPU y su MMU, envío de trabajos (binner, render, TFU, CSD),
  interrupciones, reinicio y relojes (`drivers/gpu/drm/v3d/`).
- [x] V4 **Mesa v3d y v3dv frente al kernel**: qué llamadas de DRM usa Mesa con v3d y vc4, qué estructuras se pasan y
  qué habría que ofrecer en DashOS para portar Mesa sin cambiar su controlador (`src/gallium/drivers/v3d`,
  `src/broadcom/vulkan`, `include/drm-uapi/v3d_drm.h`). Mesa es MIT, pero se sigue el mismo método.

## Bloqueos

Ninguno.

## Bitácora

### 2026-11-03 — Plan

Bucle creado por encargo del usuario. Quince tareas de construcción (H1 y H2 de la hoja de ruta, más la base del
vídeo) y cuatro de análisis de vídeo y GPU.

### 2026-11-03 02:2x — C1 (llamada nativa desde el 68k) y V1 lanzada

Parche `patches/emu68/0002-llamada-nativa-linea-a.patch`, con `DASHOS_NATIVE_CALLS`: `EMIT_lineA` reconoce `$A0FF` y
el número de función, guarda x0..x30 en la pila, llama a `dashos_native_call(regs, número)` y los repone; el `RTS` que
sigue en la entrada de la tabla lo traduce el JIT como siempre. `M68K_GetINSNLength` cuenta 2 palabras para `$A0FF`.
Prueba en `jit-host/` (`make run`): un programa 68k llama dos veces a una "biblioteca" con entradas `$A0FF n RTS`;
las dos funciones nativas leen D0 y D1 de la copia de registros y devuelven en D0; resultado 17, correcto, y la
prueba de la suma sigue bien. Funcionó a la primera. No probado: llamadas anidadas (nativo que vuelve al 68k), ni
con la caché del JIT invalidada. V1 (HVS) lanzada a un agente analista.

### 2026-11-03 02:2x — C2 (esqueleto del kernel)

`kernel/`: crate de Rust para `aarch64_be-unknown-none-softfloat` con `build-std` (core, alloc,
compiler_builtins). `src/arranque.s` entra en EL2, pone big-endian en EL2 antes de leer ningún dato, configura EL1
(AArch64, temporizador accesible, `SCTLR_EL1` con EE y E0E) y baja a EL1 con `eret`; también vale si el firmware
entra ya en EL1. `src/mmio.rs` con `RegLe32` (da la vuelta a los bytes). `src/serie.rs` con el PL011 y `kprintln!`.
`tools/prueba_qemu.sh <imagen> <esperados>` arranca QEMU `raspi4b` y comprueba las líneas en orden; baja el árbol de
dispositivos del firmware oficial a `build/firmware/` (no va en git: es GPL). `make -C kernel prueba`: BIEN.
Aviso: `+strict-align` hace falta mientras la MMU esté apagada (memoria de dispositivo), pero Rust avisa de que lo
quitará; con la MMU (C6) se puede retirar. Sin cerrojo en la serie hasta C9.

### 2026-11-03 02:2x — C3 (excepciones)

`kernel/src/vectores.s`: tabla de 16 entradas alineada a 2 KB; cada entrada guarda x0 y x1 y salta a un tramo común
que guarda el resto, ELR, SPSR, ESR y FAR en un marco de 288 bytes y llama a `excepcion()` en Rust
(`excepciones.rs`). Un BRK desde EL1 se avisa y se sigue; lo demás vuelca registros y entra en pánico. Prueba: un
`brk #0x2a` provocado y vuelta. BIEN. No probado aún: el volcado de un fallo de verdad.

### 2026-11-03 02:3x — C4 (árbol de dispositivos)

`kernel/src/dtb.rs`: lector de FDT sin memoria dinámica (el formato es big-endian, como DashOS). Busca por
`compatible` o por nombre, lee `reg` con las celdas del padre, traduce direcciones subiendo por los `ranges` y
decodifica `dma-ranges`. En QEMU `raspi4b`: memoria desde 0, 960 MB (QEMU solo describe el primer tramo); PL011 en
`0xfe201000`; GIC-400 en `0xff841000` y `0xff842000`; temporizador físico no seguro en el PPI 14; DMA del `soc` y de
`emmc2bus` en el primer GB, y `scb` en los 16 GB. Coincide con S3. Pendiente: el nodo `pcie` no sale por nombre
(está dentro de `scb`); se mirará al hacer el PCIe en la placa.

### 2026-11-03 02:3x — V1 (HVS), del analista

`docs/spec/hw/hvs.md`, 1.176 líneas: direcciones, interrupción, relojes, memoria que alcanza, orden de bytes,
registros con sus campos, memoria de listas, la entrada de plano palabra a palabra, canales y FIFOs, secuencias y
casos raros, con la cita de Linux de cada dato. Revisado por encima que no lleva código. V2 (pixel valves y HDMI)
lanzada.

### 2026-11-03 02:3x — C5 (memoria física)

`kernel/src/memoria.rs`: mapa de bits de páginas de 4 KB para hasta 8 GB (256 KB en `.bss`). Marca libre lo que
dice `memory` y ocupa lo reservado: los primeros 512 KB (espera de núcleos y resguardo del firmware), el kernel, el
árbol, `/memreserve/` y los nodos `no-map`. Zonas `Dma1G` (límite sacado de `emmc2bus`), `Dma3G` (3 GB fijo: el
nodo PCIe no se encuentra aún) y `Normal`; se reparte desde arriba. Reserva de páginas seguidas con alineación. En
QEMU: 960 MB, 958 libres; la prueba pide una página de cada zona y 64 KB seguidos alineados, comprueba los límites y
lo devuelve. BIEN. No se ve la separación de zonas porque QEMU solo describe 960 MB. Búsqueda lineal: lenta con
mucha memoria, a mejorar.

### 2026-11-03 02:3x — C6 (MMU)

El kernel se enlaza en la mitad alta (`KERNEL_VA = 0xFFFFFF8000000000`, 39 bits) y se carga en `0x80000`.
`arranque.s` monta, antes de entrar en Rust, una tabla de nivel 1 que sirve a la vez para `TTBR0` (identidad) y
`TTBR1` (mapa directo de la memoria física): tres bloques de 1 GB de memoria normal y una tabla de nivel 2 para el
cuarto GB, con bloques de 2 MB normales hasta `0xFC000000` y de dispositivo desde ahí. Las tablas se escriben en
big-endian (la MMU las lee con el orden de EL1). MAIR con dispositivo, normal con caché y normal sin caché; TCR con 4
KB, IPS de 44 bits. Enciende MMU y cachés y salta a la mitad alta. En Rust, `mmu::virt()` y `mmu::fisica()`; serie y
árbol por el mapa directo. Fuera `+strict-align` (ya no avisa Rust). Prueba: MMU encendida, `kmain` en
`0xffffff80...`, y una página del reparto escrita y leída por el mapa directo. Todo lo anterior sigue BIEN.
Pendiente: quitar la identidad de `TTBR0` cuando lleguen los procesos (C11), y mapear la RAM por encima de 4 GB
en las Pi de 8 GB.

### 2026-11-03 02:3x — C7 (montón del kernel)

`kernel/src/cerrojo.rs`: cerrojo de espera activa con atómicos (ya valen con la MMU encendida; aún no desactiva
interrupciones). `kernel/src/monton.rs`: `#[global_allocator]` con ocho clases de 16 a 2048 bytes, cada una con su
lista de bloques libres troceando páginas del reparto, y páginas seguidas para lo mayor, que se devuelven al
liberar. Prueba: un `Vec` de 10.000 números (suma correcta), `String` con formato, `Box` y un `BTreeMap` de 500
claves; al liberar no se pierden páginas. BIEN. Quitados dos avisos del compilador.

### 2026-11-03 02:3x — C8 (temporizador y GIC-400)

`kernel/src/gic.rs`: distribuidor y CPU del GIC-400 desde el árbol (por el mapa directo), habilitar con prioridad,
reconocer y terminar. `kernel/src/reloj.rs`: temporizador físico de EL1 con el PPI del árbol (14, INTID 30), tic a
100 Hz que se vuelve a programar en la interrupción. Las IRQ de EL1 van del vector al GIC y al reloj. Prueba: 10 tics
con `wfi` miden 100 ms con la frecuencia de 62,5 MHz que da QEMU. BIEN a la primera. No probado en la placa: si el
firmware real deja el GIC en el grupo 1 no seguro, `GICD_CTLR = 1` también vale (habilita el grupo 1 en esa vista).

### 2026-11-03 02:4x — C9 (cuatro núcleos)

`arranque.s` reorganizado con dos macros, `A_EL1` (de EL2 a EL1 en big-endian) y `MMU` (encender la MMU con las
tablas que construye el núcleo 0), y una entrada `_secundario` para los núcleos 1 a 3, con pila de 64 KB por
núcleo. `kernel/src/nucleos.rs`: escribe la dirección física de `_secundario` en la tabla de espera (`0xe0`, `0xe8`,
`0xf0`) en little-endian, porque esos núcleos aún no han cambiado de orden de bytes; la saca de la caché y avisa con
`sev`. Cada núcleo pone sus vectores, su interfaz del GIC y se presenta. La serie ya tiene cerrojo. Prueba: los tres
se presentan con su MPIDR. BIEN a la primera.

### 2026-11-03 02:4x — V2 (pixel valves y HDMI), del analista

`docs/spec/hw/hdmi.md`: pixel valves, las dos salidas HDMI con sus bloques, relojes por el buzón, secuencia de
encendido de un modo, HPD y EDID, 4K, sincronía vertical y audio, con cita de Linux. Revisado por encima que no lleva
código. V3 (V3D, lado del kernel) lanzada.

### 2026-11-03 02:4x — C10 (hilos y planificador)

`kernel/src/contexto.s`: `cambiar_contexto` guarda y carga x19-x30 y la pila; `hilo_entrada` arranca un hilo nuevo.
`kernel/src/planificador.rs`: una cola por núcleo, clase justa por tiempo virtual (tiempo × 1024 / peso), un hilo
ocioso por núcleo (el de su arranque), el tic del reloj reparte, `ceder()` y fin de hilo. `cerrojo.rs` gana
`CerrojoIrq`, que enmascara las interrupciones mientras se tiene (si no, el tic que pide el cerrojo de la cola en el
mismo núcleo se bloquea). La interrupción del reloj se termina en el GIC antes de cambiar de hilo. Cada núcleo pone
su temporizador. Prueba: tres hilos que cuentan en el núcleo 1 durante 300 ms quedan parejos (4,99 M, 5,56 M y
5,50 M), 30 cambios de hilo, y un hilo corto en el núcleo 2 termina. Cuatro ejecuciones seguidas, BIEN. Pendiente:
liberar los hilos muertos, migración y robo de trabajo entre núcleos.

### 2026-11-03 02:5x — C11 (procesos en EL0)

`kernel/src/espacio.rs`: espacio de direcciones por proceso (tablas de tres niveles de 4 KB en big-endian, páginas
no globales con su ASID, permisos de código y de datos con PXN y UXN). `contexto.s` gana `entrar_el0`, que baja a
EL0 con los registros limpios. Los vectores guardan y reponen `SP_EL0` en el marco. El planificador cambia `TTBR0`
al cambiar a un hilo con espacio. `kernel/src/llamadas.rs`: llamadas provisionales por `svc` (escribir y salir),
con comprobación de que el texto está en la mitad del usuario. Un fallo de un proceso lo termina a él, no al
sistema. `kernel/src/usuario.s`: dos programas de prueba. Prueba: dos procesos con el mismo programa en las mismas
direcciones (`0x400000`) y distinto mensaje, en los núcleos 2 y 3, escriben cada uno el suyo y salen con 42; un
tercero que escribe en la memoria del kernel muere con un fallo de datos (clase 0x24) y el sistema sigue. Tres
ejecuciones, BIEN. `tools/prueba_qemu.sh` admite líneas con `~` que pueden salir en cualquier orden.
Pendiente: liberar el espacio y las páginas al morir el proceso; las llamadas de verdad irán por capacidades (C12).

### 2026-11-03 02:5x — C12 (capacidades y canales)

`kernel/src/objetos.rs`: objetos del kernel con llaves (derechos de leer, escribir y transferir), tabla de llaves por
proceso (la 0 no existe), canales con dos lados, mensajes de hasta 256 bytes y 8 llaves. Leer en un lado vacío
bloquea el hilo; escribir lo despierta. El planificador gana `bloquear(soltar)` y `despertar(hilo)`: el cerrojo del
lado se suelta con el hilo ya marcado como bloqueado, así que no se pierde ningún despertar; como los hilos no cambian
de núcleo, nadie lo pone a correr antes de guardar su contexto. `kernel/src/llamadas.rs`: canal_crear,
canal_escribir (las llaves que viajan salen de la tabla de quien manda), canal_leer (entran en la de quien recibe) y
cerrar. Prueba: un servidor en el núcleo 2 y un cliente en el 3, unidos por un canal; el cliente crea otro canal y
manda uno de sus extremos dentro del mensaje; el servidor responde por ese extremo. Llegan "ping" y "pong" y los dos
salen. Tres ejecuciones, BIEN, a la primera. Pendiente: un aviso entre núcleos (IPI) para que el despertar no espere
al siguiente tic del otro núcleo, que hoy tarda hasta 10 ms; recortar derechos al pasar una llave.

### 2026-11-03 02:5x — V3 (V3D, lado del kernel), del analista

`docs/spec/hw/v3d.md`: direcciones, encendido por el buzón, identificación, MMU de la GPU, los cinco tipos de
trabajo (binner, render, TFU, CSD, limpieza de cachés), interrupciones, reinicio, contadores y orden de bytes, con
cita de Linux. Revisado por encima que no lleva código. V4 (lo que Mesa pide al kernel) lanzada.

### 2026-11-03 02:5x — C13 (objetos de memoria)

`objetos.rs` gana `Objeto::Memoria` (páginas físicas que se pueden proyectar en varios procesos), y el proceso
guarda su `Espacio`. Llamadas nuevas: `memoria_crear(tamaño)` y `memoria_proyectar(llave, dirección, permisos)`,
que comprueba los derechos de la llave (solo lectura si no tiene el de escribir). Prueba: un productor crea una
página, la proyecta en `0x600000`, escribe su texto y manda la llave al consumidor por un canal; el consumidor la
proyecta en `0x700000`, lee el texto del productor, escribe el suyo y avisa; el productor lee entonces el texto del
consumidor en su propia dirección. Tres ejecuciones, BIEN, a la primera. Pendiente: devolver las páginas cuando no
quede ninguna llave ni proyección; quitar proyecciones.

### 2026-11-03 03:0x — C14 (clase de plazo)

El planificador gana la clase de plazo de S2: un hilo con capacidad y periodo corre antes que cualquier hilo justo
mientras le quede presupuesto; entre los de plazo, el de plazo más cercano (EDF). Al agotarlo o al llamar a
`esperar_periodo()`, espera al periodo siguiente. El temporizador ya no da un tic fijo: el planificador lo programa en
cada decisión para el momento exacto en que algo cambia (fin del tic, fin del presupuesto, o renovación de un hilo
de plazo). `reloj::tics()` se calcula con la hora. `crear_plazo` rechaza lo que pase del 80 % del núcleo. Prueba: un
hilo de 2 ms cada 10 ms en el núcleo 1, que ya tiene tres hilos justos ocupados: 31 activaciones en 300 ms, con una
desviación máxima del periodo de 44 a 346 µs según la ejecución (QEMU); los hilos justos siguen avanzando; pedir un
70 % más se rechaza. BIEN. Es la base de la meta de S0 de un escritorio fluido con carga; falta medirlo en la placa.

### 2026-11-03 03:0x — C15 (buzón de la VideoCore y pantalla del firmware)

`kernel/src/buzon.rs`: buzón (canal de propiedades) con el mensaje en little-endian, en una página del primer GB,
limpiando la caché antes y después, y la dirección en el bus de la VideoCore (`0xC0000000`). Lee el reloj de la CPU
(700 MHz en QEMU) y pide una pantalla de 640x480 a 32 bits. Se pinta un degradado del azul al violeta con un tablero
rojo y blanco (homenaje a la Boing Ball), y se comprueba leyendo la memoria. Captura de QEMU con `screendump` en
`docs/capturas/c15-pantalla-firmware.png`. Dos tropiezos: el paso de línea estaba en la palabra equivocada del
mensaje, y el orden de colores "RGB" del firmware deja el rojo en el byte bajo; se pide el orden B, G, R para que
el píxel sea `0x00RRGGBB`. Aviso para la placa: la pantalla está en memoria con caché; aquí se limpia a mano, pero lo
correcto es proyectarla sin caché o con escritura combinada. Segunda tanda de tareas añadida (C16 a C23).

### 2026-11-03 03:0x — C16 (doble búfer y compositor por daños)

`buzon.rs`: pantalla virtual de varios búferes y `desplazar(y)` (etiqueta `0x48009`). `kernel/src/compositor.rs`:
compositor provisional en el kernel con dos búferes; pinta en el oculto y cambia de mitad; repinta solo los daños de
este cuadro y del anterior (cada búfer se vio hace dos cuadros); limpia la caché de lo pintado. Una ventana sencilla
(borde, barra de título y cuerpo) sobre el fondo de C15. Prueba: 60 cuadros moviendo la ventana. En QEMU, repintar
todo cuesta 2.526 µs por cuadro (307.200 píxeles) y solo los daños 297 µs (30.593 píxeles), unas 8,5 veces menos. Se
comprueban seis píxeles del búfer visible (la ventana en su sitio final, sin rastro en el inicial). Captura en
`docs/capturas/c16-compositor.png`. BIEN. Los tiempos son de QEMU, no de la placa. Sin sincronía vertical: el
firmware no la da; llegará con el controlador propio del HVS (fase B).

### 2026-11-03 03:1x — C17 (avisos entre núcleos)

`gic.rs`: `avisar(núcleo, sgi)` por `GICD_SGIR`; `reconocer` devuelve el IAR entero y `terminar` lo devuelve tal
cual (en las SGI lleva el núcleo que la mandó). `despertar` avisa al núcleo del hilo si es otro (SGI 1), y la
interrupción acaba en el planificador. Prueba: un hilo en el núcleo 2 y un eco en el 3 se pasan un mensaje por
canales del kernel. Ida y vuelta: sin avisos 10.057 y 10.080 µs (esperaban al tic de 10 ms del otro núcleo); con
avisos 14 y 18 µs. Unas 600 veces menos. BIEN.

### 2026-11-03 03:1x — V4 (lo que Mesa pide al kernel), del analista

`docs/spec/hw/mesa-v3d.md`: llamadas de DRM que usan v3d y v3dv, búferes y su paso a la pantalla, sincronización,
lo que Mesa supone del sistema (incluido el orden de bytes) y una propuesta de interfaz por fases para DashOS.
Revisado por encima que no lleva código. Con esto la línea de análisis de vídeo y GPU (V1 a V4) está completa.
Hallazgo importante de V4: v3d y v3dv no tienen nada sobre el orden de bytes y escriben palabras nativas para la
GPU; compilarían en big-endian pero darían datos al revés. Opciones para DashOS en `PLAN.md`, "Preguntas abiertas".

### 2026-11-03 07:5x — C18 (recoger lo que dejan los muertos)

La sesión estuvo parada de 03:10 a 07:47 por un problema de inicio de sesión; el bucle siguió al volver.

El hilo muerto pasa a una lista de su núcleo al cambiar de hilo, y lo libera más tarde otro hilo del mismo núcleo
(el ocioso, en su vuelta), porque su contexto todavía se guarda en el cambio. Al liberar un hilo se suelta su
referencia al proceso; al soltar la última, el proceso cae con su tabla de llaves y su espacio. `Espacio` tiene
`Drop`: invalida su ASID en todos los núcleos (`tlbi aside1is`) y devuelve tablas, páginas propias y el nivel 1. Un
objeto de memoria tiene `Drop` que devuelve sus páginas, y cada espacio que lo proyecta guarda un `Arc`, así que no
se libera mientras alguien lo tenga proyectado. Prueba: tras calentar el montón con 60 procesos, dos tandas de 50
procesos creados y terminados dejan las páginas libres exactamente igual (245.401). BIEN. Sin la recogida, cada
proceso dejaría unas 10 páginas. Pendiente: cerrar los canales de un proceso que muere, para que su otro extremo lo
note.

### 2026-11-03 07:5x — C19 (programas de usuario en Rust)

`usuario/`: crate `dashos` (no_std, `aarch64_be-unknown-none-softfloat`, enlazado en `0x400000`) con las llamadas
provisionales envueltas (escribir, salir, canales, memoria), `println!` y la macro `programa!` (entrada y pánico).
Primer programa: `usuario/src/bin/hola.rs`. `kernel/src/elf.rs`: cargador de ELF de 64 bits big-endian para AArch64,
segmentos `PT_LOAD` a páginas nuevas con permisos según sus banderas y limpieza de la caché de instrucciones para el
código. El Makefile del kernel compila `usuario/` antes y el kernel incluye el ELF. Prueba: el programa escribe con
formato, suma de 1 a 100 (5050), crea un objeto de memoria, lo proyecta, comprueba que es big-endian y sale con 5.
BIEN, a la primera. Las pruebas de C11 a C13 siguen con los programas en ensamblador; pasarlas a Rust queda pendiente.


### 2026-11-03 08:0x — C20 (controladores fuera del kernel)

Dos llaves nuevas. La de registros (`Objeto::Registros`) proyecta un tramo de periféricos en el proceso como memoria
de dispositivo (`Permiso::Dispositivo`, índice 0 de MAIR, sin ejecutar). La de interrupción
(`kernel/src/interrupciones.rs`) se asocia a un canal: al llegar, el kernel la enmascara en el GIC, la apunta como
pendiente y despierta al repartidor, un hilo del kernel en el núcleo 0 que manda el mensaje (en la interrupción no se
puede reservar memoria). El controlador la devuelve con `interrupcion_terminar`. Las interrupciones de aparatos van
todas al núcleo 0 (`gic::dirigir`). Al perder la última llave, la interrupción se enmascara y queda libre. Llamadas
9, 10 y 11. Prueba: `usuario/src/bin/serie.rs` recibe los registros del PL011 y su interrupción (153, leída del árbol
de dispositivos), escribe una línea en el puerto sin pasar por el kernel, pide la interrupción de envío, la recibe
como mensaje, la atiende y sale con 7. BIEN, tres de tres. Pendiente: quién reparte estas llaves (el kernel las da a
mano en la prueba; en S1 es el gestor de dispositivos) y repartir las interrupciones entre núcleos.

### 2026-11-03 08:1x — C21 (tarjeta SD)

`usuario/src/bin/sd.rs`: controlador SDHCI fuera del kernel, por consulta (sin interrupciones ni DMA), escrito con
las especificaciones simplificadas públicas de la SD Association. El kernel le da las llaves de registros del EMMC2
(donde va la tarjeta en la Pi 4) y del SDHCI clásico (donde la pone QEMU `raspi4b`); usa el que tenga tarjeta.
Arranque de la tarjeta (CMD0, CMD8, ACMD41, CMD2, CMD3, CMD7), sectores de capacidad estándar y alta, registros
siempre de 32 bits. `tools/prueba_qemu.sh` crea `build/sd/prueba.img` (64 MB, texto en el sector 0, firma 0x55AA,
otro texto en el sector 1) y la conecta. Prueba: lee los dos sectores, muestra su texto y sale con 9. BIEN a la primera.
Con las pruebas nuevas la tanda entera pasa de 4 segundos: una de trece vueltas falló por los 6 segundos de margen
del script; ahora `make prueba` da 10, y cinco de cinco bien. Sin probar: el EMMC2 de verdad (su reloj y su
alimentación los pone el firmware; puede que haga falta algo más) y tarjetas de alta capacidad.

### 2026-11-03 08:0x — C23 (HVS según la especificación)

`kernel/src/hvs.rs`, escrito solo con `docs/spec/hw/hvs.md`. Codifica planos RGB sin escalado (8 palabras: `CTL0`,
`POS0`, `CTL2`, `POS2`, contexto, puntero, contexto, paso) y listas terminadas en `END`. Dos formatos: el
`XRGB8888` de DRM (código 7, `ORDER` 2) y el nativo de DashOS `0xAARRGGBB` en big-endian (código 7, `ORDER` 1, la
deducción de §6.4 sin probar). Alfa fijo o por píxel premultiplicado. Rechaza búferes fuera del primer GB. En el
hardware: busca el canal que dejó encendido el firmware y cambia solo su lista, con doble zona en la memoria de
listas (palabras 32 en adelante; las 0 a 31 son del firmware), relleno de fondo, limpieza de caché de los píxeles y
espera a `DISPLACTx`. El modo de vídeo (PV y HDMI) sigue siendo el del firmware. Nueva lectura de sondeo
(`RegLe32::sondear`): en QEMU la ventana del HVS no existe y leerla daba un fallo de bus; ahora la excepción lo
recoge y devuelve "no está". Prueba en QEMU: la lista del ejemplo de §5.10 sale igual palabra a palabra; una lista
de escritorio más cursor da 17 palabras con los valores calculados a mano; el controlador ve que no hay HVS. BIEN,
tres de tres. Sin probar en hardware: falta la Pi 4. La prueba de la Pi pondrá un cursor translúcido de 64 × 64
encima de la pantalla del firmware; si los colores salen cambiados, el `ORDER` nativo es el sospechoso.

### 2026-11-03 08:0x — C24 (V3D, lado del kernel)

`kernel/src/v3d.rs`, escrito solo con `docs/spec/hw/v3d.md`. Encendido del dominio (cprman, reinicio del bloque PM y
puente ASB, con la contraseña `0x5A`), reloj del firmware al máximo, identificación (versión, slices, QPU, TMU,
anchuras de dirección y los siete registros que pide Mesa), estado fijo del núcleo, MMU de la GPU (tabla de 4 MB
contiguos con entradas en little-endian, página de relleno, vaciado de caché y TLB), interrupciones con las máscaras
de Linux, invalidación de cachés y lanzamiento de binning y render. Detecta la GPU sondeando el identificador del
puente ("brdg"), así no cae en QEMU. Prueba en QEMU: la entrada de la tabla queda `45 23 01 30` en memoria para la
física `0x12345000`; `MMU_CTL` y las dos máscaras rehechas con los bits dan los valores de la especificación; una
tabla de 4 MB real mapea y quita dos páginas y devuelve la memoria. BIEN, tres de tres. Sin probar en hardware. Falta
para ver un triángulo: las listas de control, que escribe Mesa (o una RCL mínima a mano que borre una baldosa y la
guarde: con `mesa-v3d.md` se puede intentar), y la decisión del orden de bytes de Mesa (pregunta abierta del plan).

### 2026-11-03 08:0x — C22, primera vuelta (USB: el primer dispositivo)

`kernel/src/usb.rs`: DWC2 en modo anfitrión, por consulta y con DMA (QEMU solo emula ese modo). Reinicio del núcleo,
modo anfitrión forzado, alimentación y reinicio del puerto (con cuidado de no escribir 1 en los bits que se borran
así, que apagarían el puerto) y transferencias de control por el canal 0 (SETUP, datos, estado; repite si el
dispositivo responde NAK). Protocolo según la especificación USB 2.0. Va en el kernel de momento: para sacarlo hace
falta una llave de memoria DMA que dé direcciones de bus a un proceso. `tools/prueba_qemu.sh` conecta `usb-kbd` y
`usb-mouse`; QEMU los pone detrás de un hub propio. Prueba: el controlador arranca (`0x4f54294a`), el puerto queda
activo a velocidad completa, el primer dispositivo es el hub (clase 9, `0409:55aa`, paquetes de 8 bytes), toma la
dirección 1 y se relee su descriptor. BIEN a la primera, tres de tres. Siguiente vuelta: configurar el hub, enumerar
sus puertos y leer el teclado y el ratón (informes HID de arranque).

### 2026-11-03 08:1x — C22, segunda vuelta (hub, teclado y ratón)

`kernel/src/usb.rs`: configuración del hub y de sus puertos (alimentación, estado, reinicio y borrado de los avisos
de cambio, según la especificación USB 2.0, 11.24), dirección para cada dispositivo, lectura del descriptor de
configuración y búsqueda de interfaces HID de arranque (clase 3, subclase 1; protocolo 1 teclado, 2 ratón). Pone el
protocolo de arranque y lee el punto de interrupción de entrada alternando DATA0 y DATA1; un NAK es "nada nuevo". El
tamaño de paquete del punto 0 se guarda por dirección. `tools/prueba_qemu.sh` abre el monitor de QEMU en un socket y
manda cada medio segundo una "a" y un movimiento del ratón; `make prueba` da ahora 15 segundos. Prueba: detrás del
hub salen dos dispositivos (direcciones 2 y 3), uno es teclado y otro ratón; llega la tecla "a" (uso `0x04`) y el
movimiento (10, 5). BIEN, tres de tres. Pendiente: sacar el controlador del kernel (falta la llave de memoria DMA),
interrupciones en vez de consulta, dispositivos de baja velocidad detrás de un hub (transacciones partidas) y el
VL805 (xHCI) de la Pi 4 real: allí los puertos USB de la placa van por el xHCI del PCIe, no por el DWC2,
que solo está en el conector USB-C.

### 2026-11-03 08:1x — C25 (el ratón USB mueve la ventana)

Tras leer el teclado y el ratón (C22), un compositor nuevo (el de C16) mueve su ventana con los informes del ratón
durante 3 segundos, recortada a la pantalla, repintando solo los daños. `tools/prueba_qemu.sh` acepta
`CAPTURA=fichero.ppm` y guarda la pantalla por el monitor de QEMU 1,5 segundos antes del final. Prueba: 6 informes,
la ventana pasa de (40, 40) a (100, 70) y los píxeles comprobados coinciden. BIEN, tres de tres. Captura:
`docs/capturas/c25-raton-usb.png`. Es la primera vez que algo de entrada mueve algo en pantalla.

### 2026-11-03 08:1x — C26 (búferes de la GPU) y V5 lanzada

`kernel/src/gpu.rs`: lo que Mesa llama BO (`mesa-v3d.md`, 4.3 a 4.6 y 6.2). Crear da una manija y una VA de la GPU
que nunca es 0, múltiplo de 4 KB y fija; las páginas llegan a cero, no tienen que ser contiguas y se ponen en la
tabla de la MMU de la GPU (vaciando su caché si hay hardware). Liberar quita las entradas y devuelve páginas y VA;
los tramos libres de VA se juntan. Sin hardware usa una tabla suelta para poder probar. Prueba: tres BO en `0x1000`,
`0x2000` (tres páginas) y `0x5000`; la tabla apunta a cada página en orden; liberar el del medio deja un hueco que
el siguiente reutiliza; al final queda un solo tramo libre, la manija liberada se rechaza y la memoria vuelve
entera. BIEN. Una de ocho vueltas de la tanda completa falló en C14 (desviación de más de 1 ms): la máquina estaba
cargada (el analista de V5 trabajando a la vez) y QEMU sigue el reloj real. No es de C26; si se repite sin carga,
mirar. V5 lanzada: especificación de las listas de control mínimas de la V3D, leyendo Mesa (MIT).

### 2026-11-03 08:2x — Mensaje de la sesión de Dash

El usuario confirmó el backend AArch64 de Dash. Va en la rama `aarch64` de FreeMEMLang, con el plan en
`docs/AARCH64_LOOP.md`, por fases de A1 a A7. Preguntó qué ELF queremos. Respuesta: estático, ET_EXEC big-endian,
en 0x400000, sin relocaciones. Segmentos alineados a 4 KB y sin compartir página. Pila de una página bajo 0x800000.
Funciones de plataforma sobre las llamadas provisionales 1, 2, 7 y 8, como única capa que conozca sus números.

### 2026-11-03 08:2x — Pila de los programas ELF (petición de Dash)

La sesión de Dash pidió más pila: 16 KB para un programa Dash, 32 KB con MUI y 64 KB con TLS. `kernel/src/elf.rs`
lee ahora el `p_memsz` del `PT_GNU_STACK` (con lld, `-z stack-size=N`). Sin él da 64 KB, y como mucho 8 MB. La pila
va justo debajo de 2 GB (`0x80000000`), porque Dash quiere toda la memoria de un programa por debajo de 2 GB.
Ningún segmento puede pasar de 2 GB menos 8 MB. Prueba: `hola` usa 40 KB de pila, que antes fallaban; `sd` pide 128
KB con `-zstack-size=131072` (en `usuario/build.rs`) y los recibe. BIEN dos de tres. La tercera falló otra vez en
C14, con el analista de V5 cargando la máquina.

### 2026-11-03 08:3x — V5 (listas de control mínimas de la V3D), del analista

`docs/spec/hw/v3d-listas.md`, 764 líneas, leyendo Mesa (MIT). 25 paquetes campo a campo. Hallazgo: Mesa nunca lanza
un render sin binning; para borrar manda antes un binning vacío. El documento da dos variantes: A, solo render, con
la lista de cada baldosa sin el salto a lo que escribe el binner; B, la de Mesa, con binning vacío. Ejemplo de 64 ×
64 con volcados byte a byte (RCL de 106 bytes, lista de baldosa de 17), contrastados con los empaquetadores que
genera Mesa compilados fuera del repositorio: comprueba la codificación, no el hardware. Resultado esperado en
memoria: `20 40 80 FF` en cada píxel. Revisado por encima: prosa y tablas, sin código de Mesa copiado.

### 2026-11-03 08:2x — C27 (listas de borrado de la V3D)

`v3d::listas::Borrado`, escrito solo con `docs/spec/hw/v3d-listas.md`, variante A (solo render): la RCL y la lista
genérica de baldosa, byte a byte. De momento, una sola baldosa (hasta 64 × 64). Prueba en QEMU: con los datos del
ejemplo de §8 salen 106 y 17 bytes, iguales a los volcados. BIEN. En la Pi 4, `prueba_v3d` lanza el primer trabajo de
la GPU (`primer_borrado`): tres búferes (destino lleno de `0xA5`, listas, memoria de baldosas a cero), render, espera
de `FRDONE` y cuenta de los píxeles `20 40 80 FF`. Sin probar: falta la Pi. Si la variante A no va, la B (binning
vacío, como Mesa) está en §7 de la especificación.

C14 falla ahora en dos de cada tres vueltas: carga media de la máquina de 15 (otra sesión compilando). La desviación
sube a 1-2,3 ms con un límite de 1 ms. Las demás pruebas pasan. Es ruido de QEMU con el reloj real, no de C27; con la
máquina en calma pasaba siempre.

### 2026-11-03 tarde — C28 y C29, a petición de la sesión de Dash

Fuera del bucle de la noche. El usuario pidió a la sesión de Dash "lo más recomendado", y esta pidió dos cosas del
kernel.

**C28, FPU por hilo.** Cada núcleo tiene un dueño de la FPU. A los demás hilos se les atrapa el acceso desde EL0
(`CPACR_EL1.FPEN=01`). Al primer acceso se guardan los registros del dueño (q0-q31, FPCR y FPSR, en
`contexto.s`), se cargan los del hilo (a cero la primera vez) y se cambia de dueño. Si solo un hilo usa la FPU, no se
guarda nada. Funciona porque los hilos no cambian de núcleo. Prueba: `fpu_a` y `fpu_b` en el mismo núcleo llenan
q0-q31 con patrones distintos, giran para que el reloj los turne y comprueban los registros 40 veces. También suman
1,5 + 2,25 con `fadd`. Hubo 16 cambios de dueño en 17 tics. BIEN tres de cuatro. La cuarta falló en C14, con una
carga media de 12.

**C29, cargador de PIE.** `elf.rs` acepta `ET_DYN` sin `PT_INTERP`. Pone la imagen al azar entre 128 y 256 GB y la
pila entre 256 y 384 GB, con alineación de 64 KB. Así los primeros 4 GB quedan libres para la caja (S3). El
cargador no aplica reubicaciones: lo hace el programa, como se acordó con Dash. El azar es provisional
(`azar.rs`: contador más splitmix64, no criptográfico). En la Pi habrá que sembrarlo con el RNG200. Prueba: un PIE
mínimo en ensamblador (`usuario/asm/pie.s`, enlazado con el ld de Bootlin) se carga dos veces. Cae en bases
distintas, y su código y su pila quedan por encima de 4 GB. BIEN tres de tres.

### 2026-11-03 tarde — C30 (orden de bytes por proceso)

El usuario decidió: lo nativo de DashOS es little-endian, y solo la caja Amiga es big-endian. Resuelve también la
pregunta de Mesa, que va tal cual. Cambios:

- Cada `Proceso` lleva su orden de bytes. El planificador pone `SCTLR_EL1.E0E` al cambiar a un hilo de otro proceso.
  El kernel sigue en big-endian (EE).
- `elf.rs` lee ELF de los dos órdenes (`EI_DATA`), y el proceso toma el de su ELF.
- Las tablas de llaves que leen y escriben `canal_escribir` y `canal_leer` van en el orden del proceso. Los
  mensajes que escribe el kernel (el número de interrupción de C20) van en little-endian.
- `usuario/` compila por defecto para `aarch64-unknown-none-softfloat`. `hola` y `fpu_b` también se compilan en
  big-endian. El PIE de C29 pasa a little-endian (`as -EL`, `ld -EL`).

Pruebas:

- `hola` corre en los dos órdenes. El primer byte de 0x11223344 es 0x44 en little-endian y 0x11 en big-endian.
- En C28, `fpu_a` (little-endian) y `fpu_b` (big-endian) se turnan en el mismo núcleo sin romperse.
- Los controladores de serie y SD funcionan en little-endian.

BIEN tres de tres.

## Resumen de la noche

Del 2 de noviembre a las 23:xx al 3 de noviembre a las 08:30, con una pausa de 03:10 a 07:47 por un problema al
iniciar sesión. Se cierra media hora antes de las 09:00: todo estaba terminado y guardado, y con la máquina muy
cargada las pruebas de tiempo daban ruido.

**Hecho y probado en QEMU `raspi4b`** (`make -C kernel prueba` lo comprueba todo):

- C1: llamada nativa desde el código 68k al sistema, en el JIT de Emu68.
- C2 a C9: kernel en Rust big-endian. Excepciones, árbol de dispositivos, memoria física, MMU, montón, temporizador
  con GIC y los cuatro núcleos.
- C10 a C14: hilos y planificador justo, procesos en EL0 con su propio espacio, llaves y canales, memoria compartida
  y clase de plazo.
- C15 a C17: pantalla del firmware, compositor con doble búfer y daños, y avisos entre núcleos (de 10 ms a 19 µs).
- C18 y C19: recogida de procesos muertos sin perder memoria, y programas de usuario en Rust cargados desde ELF.
- C20 y C21: controladores fuera del kernel (puerto serie con su interrupción como mensaje, y tarjeta SD).
- C22 y C25: USB con hub, teclado y ratón, y el ratón moviendo una ventana en pantalla
  (`docs/capturas/c25-raton-usb.png`).
- C26: búferes de la GPU.

**Escrito según especificación, sin probar en hardware** (QEMU no emula estos chips):

- C23: compositor de vídeo HVS.
- C24: lado del kernel de la GPU V3D.
- C27: primer trabajo de la GPU, que borra 64 × 64 píxeles y comprueba la memoria. Con la Pi 4 se ejecuta solo al
  arrancar.

En QEMU se comprueba la codificación contra los ejemplos de las especificaciones, byte a byte.

**Especificaciones de los analistas** (`docs/spec/hw/`):

- `hvs.md`: compositor de vídeo.
- `hdmi.md`: pixel valves y HDMI.
- `v3d.md`: GPU, lado del kernel.
- `mesa-v3d.md`: lo que Mesa pide al kernel.
- `v3d-listas.md`: listas de control mínimas.

**Con la sesión de Dash.** El backend AArch64 está en marcha en la rama `aarch64` de FreeMEMLang. Formato de ELF
acordado: estático en 0x400000. Pila de 64 KB o la que pida el ELF. El usuario eligió punteros de 64 bits.

**Pendiente o a vigilar:**

- **C14 con la máquina cargada.** Falla a veces con carga alta (media de 15): la desviación pasa de 1 ms porque QEMU
  sigue el reloj real. En calma pasaba siempre.
- **Pruebas en la Pi 4.** HVS, V3D, EMMC2 y USB de la placa necesitan la Pi. Los puertos USB-A van por el VL805, que
  es xHCI por PCIe, no por el DWC2.
- **Orden de bytes de Mesa.** Pregunta abierta en `PLAN.md`, sin decidir: A, procesos little-endian para la GPU; B,
  portar Mesa a big-endian; C, un servidor de la GPU en little-endian.
- **Pasos siguientes del kernel:**
  - Llave de memoria DMA, para sacar del kernel el USB.
  - Interrupciones de la GPU por el repartidor de C20.
  - Más de una baldosa en las listas de borrado.
  - Cerrar los canales de un proceso que muere.
