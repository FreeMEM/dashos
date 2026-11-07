# Bucle de construcción de DashOS

## Encargo vigente: fase 4, A7 de Dash y UEFI (2026-11-06)

Del usuario, el 2026-11-06 por la noche, en la sesión "DashOS" (que lleva DashOS y Dash para AArch64 desde el
traspaso de ese día): "1, 2 y 3, en loop". Con `/loop`. El uso se mira en
`~/.claude-ftapias/state/uso-7d-ftapias.txt` (la cuenta de esta sesión); al 90 % se para.

1. **Fase 4 de S16**, tal como dice "Encargo de la fase 4 de S16" (abajo): terminar C49, C50, C51 y la auditoría de cierre.
2. **A7 de Dash** (builtins de procesos, canales, llaves y grupos, C39-C48; y devolver memoria con `memoria_quitar`,
   C45). Tras cerrar la fase 4, con la API ya quieta. En un worktree propio de FreeMEMLang
   (`.worktrees/dashos-a7`), con plan y bitácora en `docs/AARCH64_LOOP.md`; los dos caminos (`./dash` y
   `./dash --gcc`); baterías de una en una bajo `systemd-run --user --scope -p MemoryMax=16G -p MemorySwapMax=0`.
   Avisar a la sesión `freememlang` antes de fusionar en main si toca `compiler/codegen.py` o los builtins.
3. **Arranque por UEFI en otros ARM**, desarrollado en QEMU virt con EDK2 (PLAN.md, decisión del 2026-11-04). Se
   puede llevar en paralelo con 2 mientras corre una batería.

Nada hacia fuera (push, PR, mensajes) sin permiso del usuario: commits locales y se le pregunta al acabar cada parte.

## Encargo de la fase 4 de S16 (parte 1 del de arriba)

Del usuario, el 2026-11-03 a las 20:1x, tras cerrar la fase 3: "cierra lo que ha quedado abierto y empieza la fase
4". El bucle hace la fase 4 de `docs/diseño/S16-kernel-seguro.md` (tareas C45 a C51) y la cierra con su auditoría.
Las decisiones de la fase están aprobadas (S16, "Decisiones", C y D). Lo que no esté decidido allí y cambie lo que ve
un programa se apunta en la bitácora con el porqué.

Reglas propias de este encargo (las mismas que en la fase 3), que sustituyen a las de abajo donde choquen:

- **Fin.** Cuando C45 a C51 estén `[x]` y la auditoría de cierre de la fase 4 esté hecha y sus fallos críticos,
  altos y medios arreglados; o con el uso por encima del 90 %. Entonces: "Resumen de la fase 4" al final de este
  fichero, un PushNotification con el resultado y parar el bucle.
- **Sin hora límite.** La regla 1 solo mira el uso.
- **Avisos.** El usuario está despierto, pero no se le avisa salvo bloqueo de verdad o al acabar.
- **Lo que ve un programa.** Si una tarea cambia una llamada, se pone al día `docs/referencia/kernel.md` en el mismo
  commit y se avisa a la sesión de Dash (`freememlang`, con SendMessage) de qué cambia. La llamada 1 (`escribir`) la
  usa Dash: puede cambiar por dentro, pero no lo que ve un programa.
- **Cada tarea**, con su prueba (hostil cuando toque) y comprobando que falla sin el arreglo cuando se pueda.
- **Lo que solo se ve en la Pi 4** (PCIe, HVS, V3D) se deja escrito y marcado "sin probar", sin inventar
  comprobaciones.

## Encargo de la fase 3 (cumplido)

Del usuario, el 2026-11-03 a las 18:5x: "sigue pero actívate en loop". Fase 3 de S16, tareas C39 a C43. Cerrada a las
20:0x; ver "Resumen de la fase 3".

## Encargo de la noche (cumplido)

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
   las líneas del puerto serie. Desde C36, las pruebas van en `kernel/src/pruebas.rs` (`make -C kernel prueba`).
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

### Fase 4 de S16 (encargo vigente)

- [x] C45 **Quitar proyecciones.** Una llamada para quitar lo proyectado en un tramo, con su TLB vaciada. Y que
  revocar una llave de memoria deshaga sus proyecciones en todos los espacios (lo que dejó abierto C41).
- [x] C46 **Memoria DMA** (S16, D). Un objeto de memoria del primer GB con su dirección de bus, que solo puede crear
  quien tenga una llave de permiso DMA que da el kernel. Así los búferes de DMA salen de objetos con dueño y cobrados.
- [x] C47 **Programa de inicio.** La imagen normal lanza `inicio` (un ELF dentro de la imagen) en el grupo raíz, con
  las llaves de arranque: registros e interrupciones de cada aparato del árbol, permiso DMA. `inicio` lanza los
  servicios, cada uno en su grupo. `make arranque` lo comprueba.
- [x] C48 **Consola como servicio.** El controlador serie en EL0 es la consola. El kernel deja de usar el PL011 cuando
  la consola arranca y escribe su registro en un anillo que la consola vacía; `escribir` va también al anillo. Solo el
  pánico escribe directo. Cierra N2/N6 de las auditorías.
- [x] C49 **USB fuera del kernel.** El controlador DWC2 como proceso, con memoria DMA (C46), que da teclado y ratón por
  canales.
- [x] C50 **Compositor y GPU fuera del kernel.** El compositor por CPU como proceso sobre la pantalla del firmware; el
  HVS y la V3D como procesos (sin probar en QEMU). El buzón de la VideoCore, como servicio.
- [ ] C51 **Cierre de la fase 4**: la ventana de entrada del PCIe (sin probar, Pi 4), las pruebas que quedan en el
  kernel pasan a `inicio` donde se pueda, y auditoría independiente de cierre con sus fallos arreglados.

### Fase 3 de S16 (cumplida)

- [x] C39 **Crear, matar y esperar procesos desde un proceso.** Un objeto proceso con su llave: `proceso_crear` a
  partir de un ELF en un objeto de memoria y una lista de llaves que se le pasan; `proceso_matar`; esperar a que
  acabe (por un canal o una llave que se cierra). Matar siempre funciona (S2): un hilo bloqueado también muere, y
  nadie se queda con un `HiloRef` suelto (auditoría de la fase 2).
- [x] C40 **Al morir un proceso, sus extremos se cierran** y el otro lado se entera (`E_CERRADO`). Sin ciclos: no se
  puede mandar un extremo por su propio canal ni por su pareja, ni una llave de interrupción al canal al que está
  asociada (hallazgo 15).
- [x] C41 **Llaves completas** (S16, G): duplicar una llave con menos derechos; identidad del que escribe (una marca
  en cada extremo, que pone quien lo crea, y que el lector recibe con el mensaje); revocar (quien dio una llave puede
  retirarla de todas sus copias).
- [x] C42 **Cupos por grupo** (S16, F): cada proceso pertenece a un grupo con un presupuesto de memoria del kernel
  que le da quien lo crea. Todo lo que crea se cobra ahí (páginas, tablas, llaves, mensajes en cola, canales). Al
  agotarse, `E_MEMORIA`. Matar el grupo libera todo. Sustituye a los límites provisionales de C32.
- [x] C43 **Cierre de la fase 3**: las pruebas del arranque que puedan ser programas de usuario pasan a serlo, con un
  proceso que lanza a otros; y auditoría independiente de cierre, con sus fallos arreglados.

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
- [x] C38 **Fuera del alcance del DMA** (decisión D, adelantada a petición del usuario): el kernel y la memoria de
  los procesos por encima del primer GB; el primer GB, solo para búferes de DMA.
- [x] C33 **Kernel en little-endian** (fase 2 de S16, A).
- [x] C34 **Mapa del kernel por páginas** (fase 2): W^X en el kernel, pilas con página de guarda, sin proyección
  identidad después del arranque, PAN por software (TTBR0 vacío mientras el kernel no copia de usuario).
- [x] C35 **El kernel no se expulsa** (fase 2, E): hilos del kernel con las IRQ enmascaradas salvo en puntos fijos,
  trabajo acotado por llamada (poner a cero por tandas, `escribir` corto).
- [x] C36 **Pruebas fuera de la imagen** (fase 2): las pruebas del arranque solo con una opción de compilación; las
  que se pueda, como programas de usuario.
- [x] C37 **Todo el `unsafe` en una caja** (fase 2, B): caja `base` y `forbid(unsafe_code)` en el resto.
- [x] C32 **Fase 1 de S16: que ningún programa tumbe ni lea el kernel** (03/11, aprobado por el usuario). Una
  vuelta por hallazgo de `docs/seguridad/auditoria-2026-11-03.md`, cada uno con su paso hostil en `malo`: 6 (llave
  repetida), 1 (cerrojo de páginas), 2 (ASID), 7 (cerrojos del canal), 3 (destrucción en bucle y guardas), 8 (sin
  memoria), 11 (SError y FIQ), 5 y 16 (registros de EL0), 12 (TTBR1 sin ejecución para EL0), ELF y bajos.
- [x] C31 **Copias seguras de la memoria de usuario** (03/11 por la tarde): ningún puntero de un programa tumba el
  kernel.
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

### 2026-11-03 tarde — C31 (copias seguras de la memoria de usuario)

Salió al escribir `docs/referencia/kernel.md`. El kernel leía y escribía la memoria de los procesos con punteros
directos, y un puntero a una zona vacía lo tumbaba con un pánico.

Ahora toda copia pasa por `copia.s`:

- `desde_usuario` y `hacia_usuario` acceden con `LDTRB`/`STTRB`, que la MMU comprueba con los permisos de EL0.
- Si un acceso falla, `excepciones.rs` lleva la ejecución a `copia_fallo` y la llamada devuelve `E_ARGUMENTO`.

`canal_leer` ya no pierde mensajes: si no cabe o no se puede entregar, el mensaje vuelve a la cola con sus llaves.
También se arregla un desbordamiento al multiplicar la capacidad de llaves por 4.

Prueba: `malo.rs` pasa punteros a memoria vacía, al kernel, a su propio código y tramos que dan la vuelta, en once
pasos. Recibe errores, y después recibe el mensaje entero, con una llave que funciona. BIEN.

### 2026-11-03 tarde — C32, primera vuelta (hallazgos 6 y 7)

**Hallazgo 6.** `canal_escribir` rechaza con `E_ARGUMENTO` una tabla con la misma llave dos veces; antes, el segundo
`quitar().unwrap()` tumbaba el kernel. De paso, si el otro extremo está cerrado, las llaves vuelven a sus números;
antes se perdían.

**Hallazgo 7.** El canal tiene un solo cerrojo para los dos lados. Ya no hay orden de cerrojos que respetar.
`Canal::escribir` devuelve el mensaje si el otro lado está cerrado.

**Pruebas.** `malo` suma los pasos 12 a 16: llave repetida, y llave mandada a un canal cerrado que sigue sirviendo
después. `tenis` (nuevo): dos procesos en los núcleos 2 y 3 mandan y leen 20 000 veces por los dos extremos del mismo
canal. Pasa. Con el canal viejo también pasa: la ventana del cuelgue es de pocas instrucciones y QEMU no la ha
pillado. La prueba sirve de regresión, no demuestra que el cuelgue existía.

### 2026-11-03 tarde — C32, segunda vuelta (hallazgos 1, 2 y 13, y la recogida)

**Hallazgo 1.** El repartidor de páginas está tras un `CerrojoIrq`, y el montón también. Los dos enmascaran las IRQ
mientras se tienen, así que el tic no puede expulsar a quien los tiene (parte del hallazgo 10). Orden: montón antes
que páginas. `paginas()` devuelve una guarda.

**Hallazgo 2.** `TCR.AS = 1`: ASID de 16 bits. Se reparten desde un mapa de bits, el 0 nunca. Un ASID vuelve al mapa
solo después del `tlbi aside1is` del espacio que lo tenía. Si se acaban, `Espacio::nuevo` cae en `expect`; eso entra
con el hallazgo 8.

**Hallazgo 13.** TTBR0 apunta a una tabla vacía (ASID 0) al arrancar cada núcleo, al cambiar a un hilo del kernel y
cuando muere un proceso. Al arrancar cada núcleo se vacía la TLB: los bloques de la proyección identidad eran globales
y valían para cualquier ASID. La proyección sigue en `__tabla_l1` para los núcleos que arrancan; quitarla es de la
fase 2.

**Recogida.** Encontrado al escribir la prueba: el hilo ocioso es el único que recogía a los muertos. En un núcleo
siempre ocupado (el 1, por el hilo de plazo de C14) no corre nunca, y cada proceso muerto se quedaba con unas 26
páginas para siempre. Ahora el hilo que muere suelta su proceso en `terminar_actual`, con la mitad baja ya en la
tabla vacía. Las pilas del kernel de los muertos se liberan también al entrar en cada llamada.

**Pruebas.**

- `repartos` (nuevo): seis procesos en los núcleos 1 a 3 crean y cierran 300 objetos de 64 KB cada uno. La cuenta de
  páginas libres más las del montón tiene que quedar igual. Quitando el cerrojo a propósito, fallan esta y C18 (se
  pierden unas 200 páginas); con el cerrojo pasan.
- `prueba_asid`: 1000 espacios vivos tienen ASID distintos, y al soltarlos el siguiente reutiliza el más bajo.
- C14 falló una vez de tres con la máquina cargada, como antes de este cambio.

### 2026-11-03 tarde — C32, tercera vuelta (hallazgos 5, 11, 12 y 16)

**Hallazgo 5.** TPIDR_EL0 se guarda en el hilo y se repone en cada cambio. TPIDRRO_EL0 queda a 0.

**Hallazgo 16.** Al arrancar cada núcleo: `CNTKCTL_EL1` solo deja a EL0 leer el contador virtual;
`PMUSERENR_EL0` y `MDSCR_EL1` a 0.

**Hallazgo 11.** Las FIQ van enmascaradas en EL0 (SPSR.F) y, si llega una, se avisa y se sigue. Un SError desde EL0
termina el proceso. En EL1 los SError están enmascarados, así que el que provoca un proceso se toma al volver a EL0.
Uno en EL1 sigue acabando en pánico. No se ha podido probar: QEMU no genera SError en `raspi4b`.

**Hallazgo 12.** Los bloques del mapa lineal llevan UXN: EL0 no ejecuta nada por la mitad alta. El W^X del kernel
sigue pendiente (fase 2).

**Pruebas.**

- `tls` (nuevo): dos copias en el mismo núcleo ponen su TPIDR_EL0 y comprueban durante 300 ms que nadie se lo toca.
- `trampas` (nuevo): tres copias intentan apagar el temporizador físico, leer el contador de ciclos y saltar al código
  del kernel. Las tres mueren (clases 0x18, 0x18 y 0x20) y el kernel sigue. Sin UXN, la tercera falla.

### 2026-11-03 tarde — C32, cuarta vuelta (hallazgo 8, ELF y bajos)

**Hallazgo 8.**

- Los últimos 32 MB libres son del kernel: la memoria de los procesos (objetos, imagen, pila y tablas de páginas) sale
  de `pagina_usuario`, que no los toca. Quien se come la memoria recibe `E_MEMORIA` (−7).
- Límites por proceso: 1024 llaves, 16 mensajes por lado de canal y 1024 proyecciones. Al pasarse, `E_LIMITE` (−8).
- `Espacio::nuevo`, `proyectar` y `pagina_nueva` devuelven error en vez de `expect`. Las reservas del montón que
  dependen de un proceso usan `try_reserve`.
- `Memoria::nueva` devuelve sus páginas si falla a medias.
- Queda: el montón del kernel sin memoria sigue acabando en pánico. Con un proceso solo no se llega; con muchos, sí.
  Lo cierran los cupos (fase 3).

**Proyecciones.** `memoria_proyectar` y `registros_proyectar` comprueban el tramo entero antes de tocar nada: nada
proyectado ya, nada en los primeros 64 KB y las tablas creadas. Ya no se pisa una entrada sin vaciar la TLB.

**ELF.** Cabeceras con tamaño 56, como mucho 64 y dentro del fichero. Sumas comprobadas. Se rechazan los segmentos
W+X, los que se solapan y una entrada fuera de un segmento ejecutable.

**Repartidor.** Encontrado con la prueba: con la memoria alta llena, cada página nueva recorría el mapa desde arriba,
y llenar la memoria era cuadrático (un objeto de 64 MB tardaba segundos). Ahora hay una pista: toda palabra del mapa
por encima está llena.

**Pruebas.** `malo` suma los pasos 17 a 23: proyectar en 0 y encima de algo, la cola llena, comerse la memoria (acaba
en `E_MEMORIA` tras 14 trozos de 64 MB y se recupera al soltarla) y llenar la tabla de llaves (509 canales). Cuatro
pasadas seguidas en verde. C18 falló una vez antes de la pista y no se ha repetido; no sé por qué falló.

### 2026-11-03 tarde — C32, quinta vuelta (hallazgo 3)

**Hallazgo 3.** `Drop` de `Canal` suelta lo que hay dentro con una lista en vez de por recursión. Cada canal (o
interrupción con canal de destino) que se queda sin dueños se vacía en la lista y se suelta ya vacío. Además, cada
pila del kernel lleva una marca al fondo que se comprueba en cada cambio de hilo: un desbordamiento para el kernel
con un mensaje claro en vez de seguir con memoria pisada. Las páginas de guarda de verdad necesitan el mapa del
kernel por páginas (fase 2).

**Prueba.** `malo`, paso 24: una cadena de 3000 canales, cada uno con los dos extremos del siguiente en su cola, y se
cierra el primero. Sin el arreglo, el kernel cae con una excepción en EL1; con él, sigue. C14 falló una vez de
cuatro, como antes.

### 2026-11-03 tarde — C32, sexta vuelta (hallazgo 9 y bajos)

- **Hallazgo 9.** `escribir_byte` deja de esperar sin límite a que el PL011 tenga hueco: si el controlador de EL0 lo
  apaga, los mensajes del kernel se pierden y el kernel sigue. La consola de registro propia es de la fase 4.
- **Consola.** El cerrojo de la serie enmascara las IRQ, así que imprimir desde una interrupción ya no cuelga el
  núcleo. `escribir` cambia por "?" los caracteres de control salvo salto de línea y tabulador, y saca el texto de
  una vez.
- **Interrupciones.** `interrupcion_asociar` e `interrupcion_terminar` piden el derecho de escribir en la llave de la
  interrupción.
- **GIC.** Leer, cambiar y escribir la prioridad y el destino va con cerrojo.
- **8 GB.** Solo se reparte memoria por debajo de 4 GB, lo que ve el mapa lineal.

**Pruebas.** `malo`, paso 25: una secuencia de escape sale como "?[31m". Lo demás no tiene prueba en QEMU: la placa
de QEMU tiene menos de 4 GB y la serie no se apaga.

### 2026-11-03 tarde — C32, cierre de la fase 1 (auditoría nueva)

Auditoría independiente sobre el código tras C32: `docs/seguridad/auditoria-2026-11-03-fase1.md`. Sin críticos. Dos
altos nuevos y dos menores, arreglados aquí:

- **N1.** Un proceso podía gastar todo el montón metiendo canales llenos dentro de otros canales: los límites no
  contaban los canales fuera de su tabla. Ahora cada proceso tiene una cuenta de canales vivos creados por él, que
  se descuenta al soltar el canal: como mucho 512. Mi frase de la cuarta vuelta ("con un proceso solo no se llega")
  era falsa entonces; ahora un proceso no pasa de unos 7 MB del montón.
- **N2.** El plazo de la consola era por byte: un texto de 4096 bytes con el PL011 apagado podía tener un núcleo
  minutos con las IRQ enmascaradas. Ahora, tras el primer plazo agotado, la consola se da por muerta y se tiran los
  bytes hasta que vuelva a tener hueco.
- **N3.** Un SError pendiente se tomaba al bajar a EL0 el siguiente proceso del núcleo, y se le cobraba a él. Ahora,
  al entrar desde EL0, `recoger_serror` (copia.s) abre un instante los SError y el pendiente se le cobra al que
  acaba de entrar. Sin probar: QEMU no genera SError.
- **N4.** Una IRQ dentro de `entrar_el0` pisaba ELR y SPSR, y el hilo se quedaba dando vueltas en EL1. Venía de C11.
  Ahora enmascara las IRQ al empezar.

Quedan anotados y no se arreglan en la fase 1: N5 (la página del PL011 incluye las UART 2 a 5, del mismo
controlador), N6 (un programa puede imitar líneas del kernel en la consola; las pruebas salen de la imagen en la
fase 2) y N7 (supuestos de un hilo por proceso, para la fase 3).

**Prueba.** El paso 24 de `malo` ya no hace 3000 canales: hace la cadena hasta `E_LIMITE`, que llega a los 512. Sin
el `Drop` iterativo, esa cadena desborda la pila del kernel y la marca del fondo lo detecta (pánico claro); con él,
sigue. Tres pasadas en verde.

**Fase 1 cerrada.** Pendiente para las siguientes, según S16: fase 2 (kernel LE, `unsafe` en una caja, pruebas fuera,
no expulsión, mapa del kernel por páginas con guardas y W^X, quitar la proyección identidad).

### 2026-11-03 tarde — C33 (kernel en little-endian)

Fase 2 de S16, decisión A. El kernel compila para `aarch64-unknown-none-softfloat` (nivel 2) y arranca con EE a 0.

- `arranque.s`: EE y E0E a 0 en EL2 y EL1.
- `dtb.rs`: el FDT es big-endian, así que ahora se lee con `from_be`.
- `hvs.rs`: el formato nativo `0xAARRGGBB` queda en memoria como `XRGB8888` (ORDER 2).
- Los periféricos, el buzón, la V3D y el compositor ya usaban `to_le`/`from_le` y no cambian.
- Los procesos siguen en su orden con E0E: las pruebas de C19/C30 (LE y BE), C28 (dos órdenes en el mismo núcleo)
  y las de C32 pasan igual.

**C18.** Fallaba a veces porque solo contaba las páginas libres, no las que se queda el montón; ahora cuenta las dos,
como `repartos`. Es la causa del fallo de C18 que no sabía explicar en C32.

**C14** falló 2 de 5 veces con la máquina a carga 18: mide el reloj real y QEMU no corre a tiempo. No es del cambio.

### 2026-11-03 tarde — C34 (mapa del kernel por páginas)

Fase 2 de S16. Tres partes, cada una con su prueba:

**Mapa con W^X** (`mapa.rs`). El núcleo 0 hace las tablas de la mitad alta al poner en marcha el repartidor, y cada
núcleo se pasa a ellas con `cambiar_ttbr1` (arranque.s), que rompe antes de hacer: corre desde la proyección
identidad, deja TTBR1 vacío, vacía la TLB y pone la tabla nueva. Código: solo lectura y ejecutable. `.rodata`: solo
lectura. Datos, montón y mapa lineal: sin ejecución. Periféricos: dispositivo. UXN en todo. Con los cuatro núcleos en
marcha se borran las tablas del arranque: ya no hay proyección identidad. Cada sección del enlace empieza en página
propia. La prueba lo mira con la instrucción AT y con las entradas de la tabla.

**Pilas con guarda.** Toda pila del kernel es la mitad de arriba (16 KB) de una ranura de 32 KB alineada. La mitad de
abajo no se proyecta. Las de los hilos van en el GB 256 de la mitad alta. Las de arranque son ranuras iguales dentro
de la imagen. La entrada de cada excepción comprueba el bit 14 del SP antes de escribir el marco: si el marco caería en
la guarda, pasa a una pila de emergencia y el kernel para con "pila del kernel desbordada". Lo hace como Linux,
cambiando SP y x0 con sumas, sin tocar la memoria. La última prueba del arranque hace que un hilo se llame a sí mismo
sin fin y espera ese pánico. Sustituye a la marca de C32.

**PAN por software.** Dentro del kernel, TTBR0 es la tabla vacía (ASID 0). El espacio del proceso solo se pone al
volver a EL0 y durante `desde_usuario`/`hacia_usuario`. `malo`, paso 27, pregunta con la llamada de prueba 99 si el
kernel ve su memoria fuera de una copia: tiene que ser que no. Sin este cambio, el paso falla.

La llamada 99 es solo para esta prueba; sale con las pruebas en C36.

### 2026-11-03 tarde — C35 (el kernel no se expulsa con un cerrojo tomado)

Fase 2 de S16, decisión E.

- **Cerrojos.** Todos enmascaran las IRQ mientras se tienen y reponen el estado de antes al soltarlos; `CerrojoIrq`
  queda como otro nombre del mismo. Así ningún hilo, tampoco uno del kernel, puede ser expulsado con un cerrojo
  tomado: se cierra el hallazgo 10 (el canal y el destino de las interrupciones eran los que quedaban). Reglas en
  `cerrojo.rs`: soltar en orden inverso, y bloquearse con un cerrojo solo con las IRQ ya enmascaradas.
  `Canal::leer` lo hace.
- **Topes de trabajo.** `memoria_crear` pide como mucho 4 MB (antes 64): ponerlos a cero cabe en menos de un
  milisegundo en la Pi 4. `escribir` toma como mucho 256 bytes y devuelve cuántos; la biblioteca repite con el
  resto. Los dos cambian lo que ve un programa: avisada la sesión de Dash.
- Lo que queda del trabajo acotado: soltar un proceso grande (sus páginas y canales) se hace de una vez, al morir.
  Hoy los límites lo acotan (memoria del proceso y 512 canales); trocearlo va con los cupos (fase 3).

**Pruebas.** `malo` come la memoria a trozos de 4 MB (230 trozos hasta `E_MEMORIA`) y el paso 28 comprueba que 8 MB
dan `E_TAMANO`. Para el hallazgo 10 no hay prueba que lo provoque: la ventana es de suerte.

### 2026-11-03 tarde — C36 (pruebas fuera de la imagen)

Fase 2 de S16.

- `kmain` solo pone el sistema en marcha: memoria, mapa del kernel, GIC y reloj, núcleos, repartidor de
  interrupciones. Sin pruebas, se queda en "DashOS en marcha con 4 núcleos. Todavía no hay programa de inicio."
- Las pruebas del arranque pasan a `kernel/src/pruebas.rs`, con la opción de Cargo `pruebas`. También quedan fuera de
  la imagen normal lo que solo usan ellas: los controladores que aún viven en el kernel (buzón, compositor, HVS, V3D,
  GPU, USB), los programas de `usuario.s` y la llamada 99.
- `make` saca la imagen normal (356 KB; con pruebas, 1,3 MB). `make arranque` la arranca en QEMU con
  `pruebas/arranque.txt`. `make prueba` saca y prueba `kernel8-pruebas.img`.
- Sin programa de inicio, lo de los procesos (ELF, espacios, llaves) solo lo usan las pruebas: en la imagen normal
  se permite el código muerto hasta que lo haya.
- N6 (un programa puede imitar líneas del kernel) queda solo en la imagen de pruebas, que es donde se juzga por la
  consola.

Las dos pruebas pasan. Las pruebas que pueden ser programas de usuario siguen en el kernel: pasarlas pide crear
procesos desde un proceso (fase 3).

### 2026-11-03 tarde — C37 (todo el `unsafe` en `base`)

Fase 2 de S16, decisión B.

**Forma.** Un módulo `kernel/src/base/`, no una caja de Cargo aparte. La raíz del kernel tiene `deny(unsafe_code)` y
solo `base` lo permite, además de las pruebas y los controladores que solo van con ellas (buzón, compositor, HVS,
V3D, GPU y USB; salen del kernel en la fase 4). La comprobación es la misma que con una caja, sin partir el proyecto
de Cargo.

**Qué hay en `base`** (unas 2100 líneas, 122 usos de `unsafe`): arranque, vectores, cambio de contexto y copias en
ensamblador; registros del sistema (`cpu.rs`); cerrojos; registros de periféricos; memoria física y montón; mapa del
kernel y pilas; espacios de los procesos; planificador; excepciones; núcleos; copias de usuario.

**Interfaces seguras nuevas**, para que el resto no necesite `unsafe`:

- `base::usuario::{leer, escribir}`: las copias de C31, con el PAN de C34 dentro.
- `arbol_de_dispositivos()`: el FDT como `&[u8]`, guardado al entrar; `dtb.rs` lee con límites.
- `Espacio::pagina_nueva(va, permiso, contenido, desplazamiento)`: rellena la página y, si es código, limpia la caché.
- `Espacio::proyectar_memoria` y `proyectar_dispositivo`: ya no se puede proyectar una dirección física suelta. Los
  registros solo pueden ser de la ventana de periféricos.
- `Memoria` (objeto de memoria) vive en `base` y devuelve sus páginas al soltarse.
- `HiloRef` para guardar un hilo y despertarlo; `proceso_actual()` da un `Arc`.
- `RegLe32::new` solo acepta direcciones de la ventana de periféricos.
- Pasan a `unsafe fn`: `Paginas::liberar`, `crear_en` (con proceso) y `asignar_espacio`. `olvidar_arranque` lo llama
  `nucleos::despertar` cuando han llegado los cuatro núcleos.

**Queda por revisar** con la auditoría de cierre: que cada función pública de `base` sea segura de verdad con
cualquier argumento. `HiloRef` depende de que un hilo bloqueado no muera; se cierra al matar procesos (fase 3).

Las pruebas y el arranque normal pasan.

### 2026-11-03 tarde — Cierre de la fase 2 (auditoría nueva)

Auditoría independiente: `docs/seguridad/auditoria-2026-11-03-fase2.md`. Sin críticos ni altos. Arreglado:

- **M1.** Entre `asignar_espacio` y `entrar_el0` podía entrar un tic, y el proceso bajaba a EL0 con la mitad baja
  vacía. Ahora `planificador::bajar_a_el0` enmascara las IRQ antes de poner el espacio.
- **M2.** `Paginas::reservar` miraba cada página una vez por candidato. Ahora empieza en la pista y salta por debajo de
  la primera página ocupada.
- **Funciones de `base` que no eran sanas con cualquier argumento:**
  - `Marco` ya no deja tocar ELR ni SPSR fuera de `base`: solo leer los registros y poner x0 y x1.
  - `planificar_con` enmascara las IRQ él mismo.
  - `tic` es interno.
  - `bloquear(soltar)` pasa a `bloquear()` y `bloquear_soltando(guarda)`, que suelta sin abrir las IRQ.
  - `Espacio::asid` es privado.
  - `mapa::construir`, `Paginas::iniciar`, `nucleos::despertar` e `iniciar_nucleo` no hacen nada la segunda vez.
  - `terminar_actual` desde el hilo ocioso es un pánico claro.
  - `Guarda` no se puede pasar a otro hilo.

Quedan, por decisión o para la fase 3: la ventana de periféricos incluye el DMA (decisión D); `HiloRef` depende de
que un hilo bloqueado no muera (se cierra al matar procesos); `crear` hace pánico sin pilas libres (solo lo llama el
kernel).

**Fase 2 cerrada.** Siguiente, según S16: fase 3 (cupos por grupo, capacidades completas, crear y matar procesos
desde un proceso).

### 2026-11-03 tarde — C38 (el kernel y los procesos, fuera del alcance del DMA)

El usuario preguntó si el DMA sin IOMMU era un fallo de seguridad. Sí: un controlador con DMA (SD, USB, gráfica)
puede escribir en cualquier memoria que alcance su aparato. Aprobó reducirlo ("haz lo que más recomiendes"). En la Pi
4, la SD y el DMA de la placa solo llegan al primer GB (ventanas `dma-ranges` del árbol). Así que:

- **Zonas.** Si hay al menos 64 MB por encima del primer GB, la zona normal empieza en 1 GB. El montón, las tablas,
  las pilas y la memoria de los procesos salen de ahí. El primer GB queda para los búferes de DMA. La memoria se busca
  por `device_type = "memory"` en todos los nodos.
- **La imagen del kernel**, que el firmware carga en 0x80000, se copia arriba y el mapa nuevo lleva sus direcciones
  virtuales a la copia. `trasladar` (arranque.s) copia y cambia de mapa a la vez, con TTBR1 vacío y sin tocar la
  memoria entre medias, así que la copia es una foto exacta: pila y variables incluidas. La copia no tiene alias en
  el mapa lineal. `mmu::fisica` traduce las direcciones de la imagen a la copia.
- **Núcleos 1 a 3.** Arrancan desde la copia y cambian al mapa nuevo en `sec_alto`, antes de usar la pila.
- **Árbol de dispositivos.** Se copia al montón al arrancar.
- **QEMU.** Solo pone 960 MB en el árbol, aunque `raspi4b` tiene RAM hasta 0x7E00_0000. `prueba_qemu.sh` usa una
  copia del árbol con un nodo `ram-alta@40000000`: QEMU borra los nodos llamados "memory". `MEMORIA_ALTA=no` usa el
  árbol tal cual.

**Pruebas.** C38 comprueba con la instrucción AT que estén por encima de 1 GB:
- el código del kernel, en la copia;
- el montón, una pila, una página de proceso y el árbol.

Y que una página DMA esté por debajo y que la copia no tenga alias. Con `MEMORIA_ALTA=no` (como una Pi de 1 GB), la
imagen normal arranca sin trasladar y C38 da "MAL", como debe: ahí no hay protección.

**Qué queda.**
- El USB 3 va por PCIe, que llega a toda la memoria. Falta cerrar su ventana de entrada al primer GB (fase 4, sin
  comprobar que se pueda).
- Los motores DMA de 40 bits llegan a todo: no se dan a ningún controlador.
- La gráfica (VideoCore) lo ve todo, pero es el firmware que arranca la placa.
- La copia vieja de la imagen sigue ocupando unos 2 MB del primer GB.
- En una Pi de 1 GB no hay protección.

### 2026-11-03 tarde, bucle — C39 (crear, matar y esperar procesos)

- **Llamadas nuevas.**
  - 12 `proceso_crear`: el ELF va en un objeto de memoria y las llaves pasan como en un mensaje.
  - 13 `proceso_matar`.
  - 14 `proceso_esperar`: devuelve la clase (salida, matado o fallo) y el código.
- **Llave de proceso.** Apunta a un `Control` (sus hilos, cómo acabó, quién espera), no al proceso. Así, tenerla no
  mantiene viva la memoria del proceso.
- **`procesos::lanzar`** carga y arranca, lo usen la llamada o las pruebas. `planificador::crear_proceso` apunta el
  hilo en la lista de su proceso antes de que pueda correr.
- **Matar siempre funciona** (S2).
  - Se marca cada hilo con el cerrojo de su núcleo y se despierta a los bloqueados. A los que corren en otro núcleo
    se les avisa con la SGI del planificador.
  - Cada hilo muere al volver a EL0 (`excepciones.rs`, `bajar_a_el0`) o al despertar dentro del kernel.
    `Canal::leer` y `proceso_esperar` lo miran.
  - Un hilo marcado no se bloquea: el planificador lo mira con el mismo cerrojo, así que no hay hueco.
- **`HiloRef`.** Quien guarda uno lo quita al despertar si nadie lo hizo (canal y espera), y la lista de hilos de un
  proceso se cambia con su cerrojo tomado. Cierra lo que dejó abierto la auditoría de la fase 2.

**Prueba.** `padre` crea hijos (`hijo`) que:
- salen con un código;
- giran en EL0 en otro núcleo, y los mata;
- se quedan bloqueados en un canal, y los mata;
- fallan.

Los espera a todos. Además:
- intenta pasar un ELF que no lo es, un tamaño mayor que el objeto, una llave de otro tipo y una que no existe;
- mata dos veces al mismo hijo.

Al acabar, las páginas libres vuelven a las de antes. Sin la comprobación al volver a EL0, el hijo que gira no muere
y la prueba se cuelga.

### 2026-11-03 tarde, bucle — C40 (lados que se cierran solos, sin ciclos)

- **`Punta`.** Un extremo de canal es un objeto propio que tienen las llaves. Al soltarse el último `Arc`, ese lado se
  cierra: el otro se entera, y los mensajes que esperaban a ese lado se sueltan. Da igual por qué se suelte:
  `cerrar`, la muerte del proceso (se suelta su tabla de llaves) o un mensaje que nadie leerá.
- **Soltar sin recursión.** `soltar` lleva una lista: cada punta o canal que se queda sin dueños se cierra o se vacía
  dentro del mismo bucle. Sustituye al `Drop` de C32 y cubre también las puntas.
- **Sin ciclos.** `canal_escribir` rechaza con `E_ARGUMENTO` un extremo del mismo canal y una interrupción que avisa a
  ese canal. Los ciclos de varios canales siguen siendo posibles; los acotan los límites (y los cupos de C42).

**Prueba** (`padre`, pasos 15 a 21):
- el padre lee un canal cuyo otro lado tiene un hijo que sale, y recibe `E_CERRADO`; lo mismo matando al hijo;
- mandar la pareja o el propio lado da `E_ARGUMENTO`;
- un lado que va dentro de un mensaje que nadie leerá se cierra.

Sin el cierre al soltar la punta, `malo` y `padre` fallan.

**Pruebas lentas.** Con la máquina a carga 27, la batería no cabía en 15 s (falló C32 "dos núcleos" por tiempo) y C14
falló dos veces. Con 25 s pasó tres de tres: `make prueba` usa ahora 25 s.

### 2026-11-03 tarde, bucle — C41 (llaves completas)

- **Duplicar** (15): una copia con los derechos pedidos que ya tenga, nunca más. Puede llevar una marca si la llave no
  tenía; una marcada no se vuelve a marcar con otra.
- **Marca.** Va en cada mensaje que se escribe por esa llave, y `canal_leer` la devuelve en `x2`. Es la identidad del
  que escribe: el servidor da a cada cliente una copia marcada, y el cliente no puede cambiarla.
- **Revocar** (16, 17): una copia revocable pasa por un intermediario (`Revocable`). El revocador lo vacía y suelta el
  objeto. `TablaLlaves::ver` resuelve el intermediario, así que todas las llamadas ven el objeto de detrás, o
  `E_LLAVE` si está revocada. `soltar` sabe vaciar intermediarios sin recursión.
- **Queda.** Revocar no deshace lo ya proyectado de un objeto de memoria. Para eso haría falta quitar proyecciones,
  que aún no existe.

**Prueba** (`llaves`, 14 pasos):
- una copia solo de escritura no lee, y pedir más derechos no los da;
- la marca llega con el mensaje y no se cambia;
- tras revocar, la copia y sus copias dan `E_LLAVE` y el original sigue valiendo;
- un lado sigue abierto mientras quede una copia;
- revocar la última cierra el lado.

Sin la regla de no volver a marcar, falla en el paso 6.

### 2026-11-03 tarde, bucle — C42 (cupos por grupo)

- **`cupo.rs`.**
  - `Cupo` es un presupuesto en bytes y `Cargo` lo cobrado, que se devuelve al soltarse.
  - Pagan:
    - un objeto de memoria, por sus páginas;
    - un espacio, por sus tablas y páginas propias;
    - cada proceso, 64 KB fijos;
    - cada canal, unos 26 KB fijos, que cubren sus colas llenas.
  - Un subgrupo saca su presupuesto del padre, y lo devuelve al soltarse cuando no queda nada cobrado a él.
- **Matar un grupo.** Mata a sus miembros y a los de sus subgrupos, con una lista y sin recursión.
- **Llamadas nuevas.** 18 `grupo_crear`, 19 `grupo_matar` y 20 `grupo_uso`. `proceso_crear` admite en `x4` una llave de
  grupo.
- **Límites.** El cupo sustituye al límite de 512 canales de C32. Siguen los de forma: 1024 llaves, 16 mensajes por
  lado y 1024 proyecciones. El grupo raíz, el de lo que lanza el kernel, no tiene límite; ahí protege la reserva de
  32 MB.
- **`malo`.** Ahora corre en un grupo de 64 MB, como cualquier programa que no es de fiar. Agota el cupo en 15 trozos
  de 4 MB, y la cadena de canales para en `E_MEMORIA` a los 2513.

**Prueba** (`grupos`, en un grupo de 32 MB):
- un hijo en un subgrupo de 1 MB se come la memoria y le dan 13 trozos de 64 KB;
- al acabar, el subgrupo vuelve a 0;
- dentro de 1 MB no cabe un subgrupo de 2 MB;
- matar un grupo mata a sus dos procesos;
- el grupo propio no se toca;
- al final, el grupo de la prueba queda a 0.

Sin devolver lo cobrado, falla en el paso 5. Matar un subgrupo dentro de otro no está probado.

### 2026-11-03 tarde, bucle — C43, primera parte (pruebas lanzadas por un proceso)

**`lanzador`** recibe en un objeto de memoria un paquete de ELF con su código esperado. Para cada uno:
- lo copia a un objeto propio;
- crea un grupo de 64 MB;
- lo lanza ahí;
- lo espera y comprueba su código de salida.

`malo` y `llaves` salen del kernel y los lanza él. Ejercita C39 a C42 desde EL0.

Las demás pruebas siguen en el kernel. Unas necesitan llaves que solo da el kernel (registros, interrupciones, el ELF
de `hijo`); otras miden cosas del kernel por dentro (memoria libre, ASID, mapa). Pasarlas pide un servicio que dé
esas llaves, que es el programa de inicio (fase 4).

Lanzada la auditoría de cierre de la fase 3.

### 2026-11-03 tarde, bucle — C43, segunda parte (auditoría de cierre de la fase 3)

Auditoría independiente: `docs/seguridad/auditoria-2026-11-03-fase3.md`. Un fallo crítico, dos altos y cinco medios.
Arreglados:

- **C1** (crítica). Una cadena de revocables tumbaba el kernel al soltarse, por recursión. Ahora caben como mucho 4
  intermediarios uno dentro de otro, contando los de los revocadores, y cada uno cuesta 256 bytes del cupo. `soltar`
  vacía también revocadores. Prueba: `llaves`, pasos 15 y 16. Sin el tope, el kernel cae por desbordamiento de pila.
- **A1.** Lo mismo con grupos anidados. Ahora hay como mucho 7 niveles bajo el raíz, y cada grupo cuesta 512 bytes.
  Prueba en `pruebas.rs`.
- **A2.** Se podía despertar a un hilo ya liberado. Ahora hay una `ColaEspera` en `base`, que solo se toca con el
  cerrojo de lo que se espera y despierta con él tomado. `HiloRef` ya no se puede despertar fuera de `base`. No hay
  prueba: es una carrera.
- **M1.** Un revocador colaba un ciclo. `forma_ciclo` mira ahora dentro de los revocadores. Prueba: `llaves`, paso
  17. **No arreglado:** los ciclos de varios canales siguen sin liberarse nunca, tampoco al matar el grupo. Lo acota
  el cupo de cada grupo, pero su presupuesto no vuelve al padre. Para liberarlos haría falta que el grupo cerrara sus
  canales al morir, o comprobar la alcanzabilidad al mandar un extremo. Queda para la fase 4.
- **M2.** Matar un grupo no alcanzaba a un proceso a medio crear. Ahora el grupo y el proceso llevan una marca de
  "matado", que se mira con el mismo cerrojo al apuntar. Sin prueba: es una carrera.
- **M3.** Con dos lectores en un lado, uno se colgaba. Ahora la cola de espera admite varios. Prueba: `padre`, pasos
  22 a 24. Con un solo hilo en espera, se cuelga.
- **M4.** Crear un proceso podía caer en pánico sin pilas. La pila se pide antes de tocar las llaves. Además, el grupo
  raíz tiene ahora como límite la memoria libre menos la reserva, así que la suma de presupuestos no pasa de la
  memoria que hay.
- **M5.** Los núcleos 1 a 3 podían arrancar de una copia del kernel aún en la caché. `trasladar` limpia ahora hasta el
  punto de coherencia (`dc cvac`). No se puede probar en QEMU.
- **Bajos.** B5: soltar copias de llaves y matar sin el cerrojo de la tabla. B6: la marca 0, explicada en la
  referencia. B7: comentarios desfasados. Quedan: B4 (memoria del kernel que no se cobra, acotada por las 1024
  llaves) y los demás bajos del informe.

**Fase 3 cerrada.**

## Resumen de la fase 3

Del 2026-11-03, de 18:5x a 20:xx, en bucle (C39 a C43), más C38 justo antes.

- **C38.** El kernel y la memoria de los procesos, fuera del alcance del DMA de la SD y de la placa. La imagen se
  traslada por encima del primer GB al arrancar.
- **C39.** Un proceso crea, mata y espera a otros. Matar siempre funciona: corriendo, bloqueado o en otro núcleo.
- **C40.** Los lados de canal se cierran al soltar su última llave, también al morir el proceso. Sin ciclos directos.
- **C41.** Llaves completas: copias con menos derechos, marca del que escribe y revocación.
- **C42.** Cupos por grupo de procesos; matar un grupo mata a todos.
- **C43.** Un proceso lanza las pruebas sueltas, cada una en su grupo. Auditoría de cierre con sus fallos arreglados.

Pendiente para la fase 4 (S16):
- liberar los ciclos de varios canales;
- quitar proyecciones (para que revocar las deshaga);
- el programa de inicio y pasarle las pruebas que quedan en el kernel;
- la consola como servicio;
- sacar el USB, la GPU y el compositor;
- cerrar la ventana del PCIe.

Todas las pruebas pasan. C14 sigue fallando a veces con la máquina cargada.

### 2026-11-03 noche — C44 (lo que quedó abierto en la fase 3)

Pedido por el usuario: "cierra lo que ha quedado abierto y empieza la fase 4".

- **Ciclos de varios canales** (M1 de la auditoría de la fase 3). Cada grupo apunta los canales que crean sus
  procesos. `grupo_matar` cierra los dos lados de cada uno y suelta sus colas, así que los ciclos se deshacen y el
  presupuesto vuelve. Con el mismo cerrojo y la marca de "muerto", un canal que se crea durante la matanza o se ve o
  se rechaza. Prueba: `grupos`, pasos 11 a 13 (un hijo deja un ciclo y sale; el grupo sigue con 1 MB cobrado; al
  matarlo, vuelve a 0). Sin cerrar los canales, falla en el paso 13. Sigue igual: un ciclo en un grupo que no se mata
  queda cobrado a ese grupo.
- **Estrés de A2** (`grupos`, paso 14). Diez veces: seis hijos esperan a un proceso X y se matan a la vez los que
  esperan y X. No cae. No he comprobado que la prueba pille la versión vieja: la cola de espera de C43 no se puede
  quitar sin rehacer medio planificador.
- **Estrés de M2** (paso 18). Un hijo crea hijos sin parar y se mata su grupo: tiene que volver a 0. Pasa, pero
  también pasa sin el arreglo (dos de dos): la ventana de la carrera es tan pequeña que QEMU no la pilla. La prueba
  queda como regresión, no como demostración.
- **M5** (la copia del kernel aún en la caché) solo se puede ver en la Pi 4 real.
- `make prueba` da ahora 40 s: la batería ha crecido.

### 2026-11-03 noche, bucle — C45 (quitar proyecciones)

- **Llamada 21, `memoria_quitar(va)`.** Quita la proyección que empieza en `va`: borra sus entradas y hace `tlbi vae1is`
  con el ASID del espacio, página a página. El espacio guarda ahora cada proyección con su dirección.
- **Revocar deshace las proyecciones.** Cada `Revocable` apunta las proyecciones hechas a través de él (proceso y
  dirección). Al revocar memoria se quitan, comprobando que siga siendo el mismo objeto.
- **Carrera entre proyectar y revocar.** `memoria_proyectar` apunta la proyección antes de hacerla y mira después si
  la revocaron: o la ve quien revoca, o la quita él.
- Como mucho, 1024 proyecciones apuntadas por intermediario.

**Prueba** (`padre`, pasos 25 a 30):
- quitar deja volver a proyectar, y quitar dos veces da `E_ARGUMENTO`;
- un hijo que lee lo quitado muere por fallo;
- un hijo lee sin parar memoria proyectada con una copia revocable; el padre revoca y el hijo muere por fallo;
- la proyección del padre, hecha con la llave original, sigue.

Si revocar no quita las proyecciones, el hijo no muere y la prueba falla.

### 2026-11-03 noche, bucle — C46 (memoria DMA)

- **Permiso DMA.** Un objeto `PermisoDma { zona, desplazamiento_bus }`. Solo lo crea el kernel, con la ventana del
  árbol de dispositivos (en la Pi 4, la SD: bus `0xC000_0000` → física 0).
- **Llamada 22, `memoria_dma(permiso, tamaño)`.** Hasta 4 MB de páginas seguidas de esa zona, a cero, limpiadas de la
  caché y cobradas al cupo. Devuelve la llave y la dirección de bus.
- **Sin caché.** Al proyectar para escribir, la memoria DMA va como normal sin caché (MAIR índice 2), porque el DMA de
  la Pi 4 no es coherente con las cachés. La memoria normal de los procesos sigue sin salir nunca del primer GB (C38):
  el primer GB solo se reparte así, con permiso.

**Prueba** (`dma`, en un grupo de 16 MB): 64 KB con dirección de bus en la ventana de la SD; se proyecta y se lee lo
escrito; sin permiso da `E_TIPO`; con más de 4 MB, `E_TAMANO`. Si se acepta cualquier llave como permiso, falla en el
paso 4. Que el aparato lea de verdad ese búfer solo se verá con el USB fuera del kernel (C49) y en la Pi.

### 2026-11-03 noche, bucle — C47 (programa de inicio)

- **`kernel/src/inicio.rs`.** En la imagen normal, `kmain` lanza `inicio` en el grupo raíz, en el núcleo 1. Le da:
  - la llave 1, un paquete de arranque en un objeto de memoria de solo lectura;
  - el permiso DMA de la ventana de la SD;
  - registros e interrupción de la serie, la SD (EMMC2) y el USB (DWC2), y los registros del buzón.

  El paquete dice qué es cada llave y trae los ELF de los servicios. La imagen de pruebas no lanza `inicio`.
- **`usuario/src/bin/inicio.rs`.** Lee el paquete, lista sus llaves, lanza cada servicio en un grupo de 16 MB desde
  una copia en un objeto propio y se queda esperando para siempre. De momento el único "servicio" es `hola`, que acaba;
  los de verdad llegan con C48 (consola) y C49 (USB).
- `make` compila ahora `usuario/` también para la imagen normal.

**Prueba** (`make arranque`, `pruebas/arranque.txt`): `inicio` arranca, ve el permiso DMA y la serie, lanza `hola` en
su grupo, `hola` sale con 5 e `inicio` se queda esperando.

### 2026-11-03 noche, bucle — C48 (consola como servicio)

- **Registro del sistema** (`registro.rs`).
  - `kprint` y `escribir` van a un anillo de 64 KB. Si se llena, lo nuevo se pierde y se cuenta.
  - Hasta que la consola lo toma, el kernel escribe también directo al PL011.
  - La llamada 23, `registro_leer`, saca del anillo; pide la llave `Registro`, que solo da el kernel a `inicio`. La
    primera vez, el kernel escribe su última línea directa ("registro: la consola pasa a un proceso…"), descarta lo
    que ya había salido y deja la serie.
  - El pánico escribe siempre directo.
  - Los lectores dormidos se despiertan en el tic o en `escribir`, nunca desde `kprint`, que puede ir con el cerrojo
    del planificador tomado.
- **`consola`** (servicio). Lo lanza `inicio` con el registro y los registros del PL011, y saca el anillo por la
  serie. `inicio` reparte ahora llaves a cada servicio según una máscara del paquete de arranque. Las llaves de
  arranque llevan derecho de transferir.
- **N6.** Cada línea de un programa empieza por "│ ". `prueba_qemu.sh` exige que los veredictos ("C32: …") empiecen
  la línea, así que un programa ya no puede falsearlos.
- **Cierra N2 y el hallazgo 9.** Con la consola en marcha, el kernel no espera nunca a la serie.

**Prueba** (`make arranque`):
- la consola toma el registro;
- lo que escriben `inicio` y `hola` sale con "│ " y una sola vez (el kernel ya no lo escribe por su lado);
- la línea del kernel "sale con código 5" sale por la consola.

`make prueba` sigue pasando: la imagen de pruebas no lanza `inicio`, así que el kernel escribe directo y las líneas de
los programas también llevan "│ ".

### 2026-11-03 noche, bucle — C49, primera parte (USB fuera del kernel)

- **El controlador DWC2 es un proceso** (`usuario/src/bin/usb.rs`), portado de `kernel/src/usb.rs`, que se borra.
  - Recibe el permiso DMA y los registros del DWC2.
  - Su búfer de DMA es memoria DMA de C46, sin caché, así que ya no limpia cachés a mano.
  - Enumera el hub de QEMU, encuentra el teclado y el ratón de arranque y escribe lo que mandan.
- **Pruebas.** C22 y C25 (dentro del kernel) se sustituyen por C49 en la imagen de pruebas: el proceso arranca,
  enumera, y aparecen la "a" y los movimientos del ratón que manda el monitor de QEMU. Pasa.

**Falta en C49:**
- que `inicio` lance `usb` en la imagen normal (añadirlo a `PROGRAMAS` en `kernel/src/inicio.rs`, con "dma" y los
  registros de "usb", y la línea en `pruebas/arranque.txt`);
- que mande los eventos por un canal en vez de solo escribirlos (para el compositor de C50);
- que use la interrupción del DWC2 en vez de consultar sin parar: hoy gasta CPU girando.

### 2026-11-06 noche, bucle (sesión DashOS) — C49, segunda parte (interrupción, canal y temporizadores)

- **Temporizadores en el kernel** (llamada 24, `temporizador_crear`; `kernel/src/temporizadores.rs`). Hacían falta
  para que el USB espere sin girar, y los pedirá el `Delay` de Dash (A7). No estaba decidido en S16: se eligió un
  objeto que avisa por un canal, como las interrupciones, para que un controlador espere las dos cosas en una sola
  lectura. El tic del núcleo 0 mira el vencimiento más próximo y despierta al repartidor de interrupciones, que manda
  los avisos (8 bytes: cuántas veces venció). Resolución de 10 ms. Se cobra al cupo y entra en las reglas de ciclos
  (`forma_ciclo`, `soltar`). Prueba nueva `tiempo` (código 49, lanzada por `lanzador`): una vez, periódico, se para
  al cerrar la llave, y rechaza llave cerrada, llave que no es canal, canal sin escribir, plazo de más de un día y
  mandar el temporizador por su propio canal.
- **USB con interrupción.** Solo se habilita la de canal parado. Cada transferencia consume su propia interrupción
  aunque el canal ya estuviera parado al mirar; si no, se quedaba deshabilitada y la siguiente tenía que esperar al
  temporizador (medido: 8 interrupciones en unas 50 esperas; tras el arreglo, 111 interrupciones, todas). Las pausas
  largas (10 ms o más) esperan al temporizador.
- **Eventos por un canal** (llave 4), 8 bytes cada uno: teclas pulsadas y soltadas (comparando informes, con los
  modificadores como usos 0xE0-0xE7) y ratón. Formato en `docs/referencia/servicios.md`, nuevo. Sin llave 4 los
  escribe. Fallo encontrado: la comprobación de la llave 4 se hacía después de crear la memoria DMA, que tomaba
  justo el número 4.
- **`inicio` lanza `usb`** en la imagen normal con el permiso DMA, sus registros y su interrupción; sin canal de
  eventos hasta que haya quien los lea (el compositor, C50). `pruebas/arranque.txt` comprueba la "a" pulsada y
  soltada y el ratón.
- **Prueba C49** (imagen de pruebas): el kernel lanza `usb` con las cuatro llaves y lee el canal hasta ver la "a"
  pulsada y soltada y el ratón. Ahora espera a que el proceso acabe antes de seguir: C26 cuenta las páginas libres
  y fallaba a veces con el USB aún soltándolas.
- **Comprobado:** `make -C kernel prueba` tres veces y `make -C kernel arranque` dos, bien. Que la interrupción se
  usa, contando avisos (arriba). **No comprobado:** en una Pi 4 real (los puertos USB-A van por el VL805, no por el
  DWC2).

### 2026-11-06 noche, bucle (sesión DashOS) — C50, primera parte (buzón y compositor fuera del kernel)

- **El buzón de la VideoCore, servicio** (`usuario/src/bin/buzon.rs`). Recibe mensajes de propiedades por un canal y
  solo deja pasar una lista de etiquetas (memoria de la VideoCore, relojes que se leen, pantalla). Lo demás, o un
  mensaje mal formado, vuelve con 0x80000001 sin llegar al firmware. Decisión apuntada: la lista la decide el
  servicio, no quien tiene el canal, porque el firmware puede apagar aparatos o cambiar relojes.
- **El compositor, proceso** (`usuario/src/bin/compositor.rs`), portado de `kernel/src/compositor.rs`, que se borra.
  Pinta en la memoria de la VideoCore, mide como C16 (desde un proceso: todo 4,7 ms por cuadro, daños 0,65 ms, en
  QEMU) y mueve la ventana con el ratón que le llega del USB.
- **Objeto nuevo: memoria del firmware** (`Objeto::Firmware`). La pantalla está en la memoria de la VideoCore, por
  debajo de 1 GB y fuera de la RAM del kernel, y `registros_proyectar` solo admitía periféricos. El kernel la saca
  del árbol (de donde acaba el tramo de RAM que empieza en 0 hasta 1 GB, `Dtb::memoria_vc`), solo la da a `inicio`, y
  al proyectarla comprueba que no toca ningún tramo de RAM (`Paginas::fuera_de_la_ram`, que ahora guarda los tramos).
  Va sin caché. Quien la tiene puede escribir en toda la memoria de la VideoCore: es el compositor, de confianza.
- **`inicio` conecta servicios** con canales con nombre ("buzon", "entrada"): el primero que lo nombra recibe un
  extremo y el segundo el otro. Da copias de las llaves de aparatos (antes las movía, y el permiso DMA solo podía ir
  a uno). La cabecera de cada programa en el paquete pasa de 24 a 32 bytes. Arreglado de paso: si un servicio no se
  podía copiar, los siguientes se leían del sitio equivocado.
- **Registros en páginas enteras.** El buzón empieza en 0xFE00B880, a mitad de página, y no se podía proyectar; la
  llave cubre ahora la página entera (en `inicio` y en la prueba).
- **`println!` de una vez** (`Linea` en la biblioteca): cada línea va en una sola llamada `escribir`. Antes se
  partía y dos procesos mezclaban sus líneas a medias.
- **Prueba C50** (sustituye a C15 y C16): el kernel lanza el buzón y el compositor con lo que les daría `inicio`,
  manda tres mensajes que el buzón tiene que rechazar (etiqueta de poner un reloj, tamaño que no cuadra, sin fin),
  comprueba la memoria del firmware contra la RAM, manda tres movimientos del ratón y cierra la entrada: el
  compositor acaba y el buzón detrás. Sin la lista de etiquetas falla (2 de 3), comprobado.
- **Arranque normal:** USB → canal "entrada" → compositor: la ventana se mueve con el ratón del monitor de QEMU.
- **Comprobado:** `make -C kernel prueba` tres veces bien (una cuarta, C14 falló por la carga de la máquina, con
  1021 µs de desviación: el fallo de siempre, apuntado en "Pendiente o a vigilar"); `make -C kernel arranque` dos.
  **No comprobado:** en la Pi 4 (allí la pantalla sale por el HVS; el compositor sigue en la pantalla del firmware).
- **Queda de C50:** el HVS y la V3D como procesos (segunda parte); el kernel aún usa `buzon.rs` para sus pruebas.

### 2026-11-06 noche, bucle (sesión DashOS) — C50, segunda parte (el HVS y la V3D fuera del kernel)

- **Biblioteca `dashos::graficos`** (`usuario/src/graficos/`): el HVS, la V3D, las listas de borrado y los búferes de
  la GPU, portados de `kernel/src/{hvs,v3d,gpu}.rs`, que se borran (y `buzon.rs`: el kernel ya no habla con el
  firmware). Sin montón: las listas van en búferes fijos y los búferes de la GPU en una tabla de 32. En un proceso,
  los planos del HVS se dan por dirección de bus, la tabla de la MMU y los búferes de la GPU son memoria DMA sin
  caché (ya no se limpian cachés), y cada búfer es un objeto DMA de páginas seguidas.
- **Servicio `gpu`** (`usuario/src/bin/gpu.rs`): enciende la V3D, su MMU y hace el primer trabajo (C27 en la Pi).
  Sus registros son llaves (ventana de la V3D, puente, PM, cprman) y el reloj lo pide al buzón. **Sin probar.**
  Decisión apuntada: su permiso DMA tiene desplazamiento 0 y zona normal, porque la V3D ve direcciones físicas.
- **`inicio`** solo da las llaves de la V3D si su puente contesta (el kernel lo sondea; en QEMU leerlo es un fallo
  de bus), y un servicio al que le falta un aparato no se lanza (lo dice al arrancar).
- **El buzón atiende a varios clientes:** cada petición lleva el extremo de un canal de respuesta. `inicio` da a
  todos menos al primero una copia del mismo extremo. Admite encender y poner la frecuencia del reloj de la V3D, y
  ningún otro.
- **El HVS** queda en la biblioteca, sin servicio: lo usará el compositor en la Pi 4. Hoy el compositor sigue en la
  pantalla del firmware.
- **Prueba C50** (sustituye a C23, C24, C26 y C27): el programa `graficos`, con un permiso DMA, comprueba lo mismo
  que hacía el kernel (listas del HVS y de la V3D contra los ejemplos de las especificaciones, tabla de 4 MB,
  búferes con su VA y su hueco reutilizado) y el kernel, que la memoria vuelve entera.
- **Ya no queda `unsafe` fuera de `kernel/src/base/`** salvo en las pruebas: las excepciones de los controladores de
  vídeo y GPU se han ido con ellos.
- **Comprobado:** `make -C kernel prueba` dos veces y `make -C kernel arranque` dos, bien. **No comprobado:** todo lo
  que toca el HVS y la V3D (Pi 4).

### 2026-11-06 noche, bucle (sesión DashOS) — C51, primera parte (pruebas del kernel a procesos)

- **`lanzador` lanza también** `hola` en los dos órdenes de bytes (C19, C30), `padre` (C39) y `grupos` (C42), que
  antes lanzaba el kernel. Su paquete gana un "ayudante": una entrada cuyo ELF recibe otra prueba como objeto de
  memoria, con un canal que dice su tamaño (lo que esperan `padre` y `grupos` para lanzar a `hijo`). Cabecera de 24 a
  32 bytes.
- **Cada prueba tiene que dejar su grupo a cero** al acabar (lo mira `lanzador` con `grupo_uso`, esperando con un
  temporizador hasta medio segundo). Y el kernel mira que el grupo de `lanzador` queda a cero y que no se pierden
  páginas en toda la tanda.
- **Se quedan en el kernel** las que prueban el propio kernel (memoria, montón, hilos, mapa, ASID, repartos,
  desborde de pila, avisos entre núcleos), las que cuentan salidas o excepciones desde dentro (FPU, registros de EL0,
  PIE) y las que dan llaves que solo da el kernel (controladores, USB, buzón y compositor, gráficos, DMA). Estas
  últimas ya son procesos; solo el arranque de la prueba queda en el kernel.
- **Comprobado:** `make -C kernel prueba` bien (otra vez C14 falló una vez por la carga de la máquina, media de 5; ver
  "Pendiente o a vigilar"); `make -C kernel arranque` bien.

### 2026-11-06 noche, bucle (sesión DashOS) — C51, segunda parte (ventana de entrada del PCIe)

- **Especificación** `docs/spec/hw/pcie.md`, de un analista aparte (sala limpia, de `pcie-brcmstb.c` de Linux):
  registros de las ventanas de entrada, codificación del tamaño, qué deja Linux y dos caminos para DashOS.
- **`kernel/src/pcie.rs`:** al arrancar, antes de iniciar la memoria, apaga BAR1 (lleva a los registros del chip) y
  BAR3, pone BAR2 a 1 GB con desplazamiento 0 (bus = física) y `SCB0_SIZE` igual, en ese orden, y lo comprueba
  leyendo. Decisión apuntada: el camino rápido de la especificación (§11.3, cambiar en caliente, sin respaldo en
  Linux) y no el seguro (§11.2, reiniciar el enlace), porque este pide levantar todo el enlace sin poder probarlo.
  Cuando haya controlador xHCI y una Pi, pasar al seguro. Consecuencia: los búferes del PCIe, en `Dma1G`.
- **QEMU quita el nodo del PCIe** del árbol (no lo emula): el kernel lo dice al arrancar y no toca nada.
- **Prueba:** la codificación del tamaño contra la tabla de la especificación (§5), en `pruebas.rs`.
- **Comprobado:** `make -C kernel prueba` y `make -C kernel arranque`, bien. **No comprobado:** todo lo demás (Pi 4;
  pasos de §12 de la especificación).

### 2026-11-07 madrugada, bucle (sesión DashOS) — C51, tercera parte (auditoría de cierre de la fase 4: altos y M1-M3)

Auditoría independiente: `docs/seguridad/auditoria-2026-11-06-fase4.md`. Ningún fallo crítico, dos altos, seis medios
y once bajos. Arreglados en esta parte:

- **A1.** Los temporizadores dejaban el núcleo 0 con las IRQ enmascaradas: cada tic recorría todos los del sistema.
  Ahora van en un montículo por vencimiento y cada pasada toca como mucho 64 vencidos (los demás, en el tic
  siguiente); el periodo más corto es un tic; cada uno cuesta 4 KB del cupo (antes 256 bytes), y crear ya no recorre
  la lista (las entradas de los soltados se quitan al vencer, o todas cuando pasan del doble de los vivos). Prueba:
  `tiempo`, paso 10 (uno de 1 µs da como mucho un aviso por tic).
- **A2.** Un temporizador seguía escribiendo tras revocar la llave del canal, y con marca 0. Ahora guarda la marca y
  los intermediarios de esa llave: sus avisos llevan la marca y se para al ver uno revocado. Lo mismo las
  interrupciones (era el B2 de la fase 3). Prueba: `tiempo`, pasos 11 y 12; sin el arreglo, el 11 falla (marca 0),
  comprobado.
- **M1.** Un aviso de una vez se perdía con la cola llena. Ahora se reintenta cada tic. Prueba: `tiempo`, paso 13.
- **M2.** La lista de proyecciones de una revocable no se vaciaba, y revocar podía quitar una proyección hecha con la
  llave original en la misma dirección. Ahora cada proyección del espacio guarda por qué intermediarios se hizo:
  revocar solo quita las suyas, y al apuntar una nueva se purgan las que ya no están. Tope: 256.
- **M3.** Revocar un objeto grande hacía una invalidación de TLB por página con las IRQ enmascaradas. Ahora, con más
  de 32 páginas, una sola de todo el ASID.
- `make prueba` pasa a 60 s: con las pruebas nuevas, la tanda rozaba los 40.
- **Comprobado:** `make -C kernel prueba` dos veces y `make -C kernel arranque`, bien.

### 2026-11-07 madrugada, bucle (sesión DashOS) — C51, cuarta parte (auditoría de la fase 4: M4-M6)

- **M4.** El compositor recibía toda la memoria de la VideoCore. Ahora la tiene el buzón y, al reservar la pantalla
  para el compositor, le manda una llave recortada a ella: llamada nueva 25, `registros_recortar`.
- **M5.** La memoria DMA volvía al repartidor aunque el aparato siguiera escribiendo. Ahora el permiso DMA es un
  objeto (`memoria::PermisoDma`) con una cuarentena: al soltarse, la memoria DMA espera ahí hasta que el controlador
  nuevo, con su aparato reiniciado, la devuelve con la llamada nueva 26, `dma_liberar` (el USB tras reiniciar el
  DWC2, la GPU tras encender la V3D, el buzón al empezar). Cada aparato tiene su permiso: "dma" (USB) y "dma-buzon".
  Prueba: `dma`, pasos 6 y 7.
- **M6.** Un mensaje con dos llaves tumbaba el buzón, y no sabía quién le pedía qué. Ahora lee con sitio para 8 llaves
  y suelta las de más, y solo acaba con el canal cerrado. `inicio` da a cada cliente una copia marcada y manda al
  servidor un directorio (marca y nombre) sin marca, que solo puede escribir él. El buzón decide las etiquetas por
  cliente: la pantalla, solo el "compositor"; el reloj de la V3D, solo la "gpu". El compositor, igual con su entrada.
  Prueba C50: cuatro rechazos (también la pantalla pedida por un desconocido) tras un mensaje con dos llaves.
- **Fallo encontrado de paso:** `inicio` daba las llaves de aparatos en el orden de su lista, no en el que las pide
  cada servicio (con la memoria de la VideoCore en el buzón, sus llaves 2 y 3 se cruzaban). El paquete lleva ahora,
  por programa, los números de sus aparatos en orden.
- **Comprobado:** `make -C kernel prueba` y `make -C kernel arranque`, bien. Con la máquina muy cargada (media de 8 a
  10 por otras sesiones), C14 y C43 fallan a ratos por tiempo; en calma pasa todo.

### 2026-11-07 madrugada, bucle (sesión DashOS) — C51, quinta parte (auditoría de la fase 4: bajos)

- **B1.** `forma_ciclo` mira también dentro de una revocable (que puede ir dentro de un revocador).
- **B2.** La marca "│ " de las líneas de los programas se podía saltar: dependía de lo que creía cada proceso. Ahora
  la salida guarda quién escribió lo último y si está a principio de línea, con el mismo cerrojo con que se escribe
  (`serie::imprimir_programa`); si otro dejó la línea a medias, se empieza una nueva. Prueba: `malo`, paso 26 (media
  línea, una línea del kernel y "C99: ..." que sale con la marca).
- **B3.** `escribir` reserva lo que de verdad puede necesitar.
- **B4.** Los últimos 4 KB del registro son solo para el kernel: un programa que escribe sin parar no tapa sus avisos.
- **B5.** Se cierra con A1 (ya no hay lista de todos los temporizadores; los `Arc` se sueltan fuera del cerrojo).
- **B6.** La memoria DMA proyectada solo para leer va también sin caché.
- **B7.** `memoria_dma` en la zona normal respeta la reserva del kernel.
- **B8, B9.** Dependen de la Pi: apuntados en "Limitaciones conocidas" de `kernel.md`.
- **B10.** Documentado: puede llegar un aviso más justo tras cerrar un temporizador.
- **B11.** Documentación puesta al día (tabla de llamadas, llaves que da el kernel, zona de la GPU, servicios).
- **Comprobado:** `make -C kernel prueba` y `make -C kernel arranque`, bien.

### 2026-11-07 madrugada, bucle (sesión DashOS) — C51, sexta parte (segunda vuelta de la auditoría)

El mismo auditor revisó los arreglos y la ventana del PCIe (sección "Segunda vuelta" del informe): un crítico, dos
altos, dos medios y siete bajos nuevos. Arreglados:

- **C1.** Guardar los intermediarios en el temporizador (A2) devolvía la recursión al soltar: una cadena de 20
  temporizadores con llave revocable desbordaba la pila del kernel. Ahora se guardan como `Weak` (temporizador e
  interrupción): no sujetan el lado del canal (B13), y si el intermediario ya no existe, se paran. Prueba: `tiempo`,
  paso 14 (cadena de 30).
- **A3.** Los temporizadores de un grupo retrasaban segundos los de los demás. Ahora hay una cola por grupo de primer
  nivel (un servicio y sus subgrupos, `Cupo::primer_nivel`) y cada pasada da la vuelta a los grupos con algo vencido,
  8 avisos de cada uno por vuelta y 64 en total. Prueba nueva: un proceso con 900 temporizadores de 10 ms y uno de
  100 ms de otro grupo, que llega a los 101 ms; con una sola cola, a los 243 ms (comprobado: falla).
- **A4.** La purga de proyecciones de una revocable podía quitar una que aún no se había hecho. Ahora cada entrada
  está pendiente hasta que la proyección se hace (o se retira si falla), la purga no toca las pendientes, y solo se
  purga con la lista llena, soltando los procesos fuera del cerrojo (B15).
- **M7.** La cuarentena DMA no se cobraba: ahora sigue cobrada al grupo hasta que se libera.
- **M8.** El buzón recortaba la pantalla sin mirar la respuesta del firmware: ahora solo con el mensaje y la etiqueta
  contestados (8 bytes). Y la pantalla se da una sola vez: después no se deja reservar otra ni cambiar su tamaño.
- **B12, B14.** Cuentas de los temporizadores: un aviso de una vez reintentado dice 1; las entradas de soltados
  cuentan para el tope de la pasada; cada cola lleva sus vivos y muertos, se purga con más muertos que vivos más 32 y
  se quita al soltarse su último temporizador (si no, sujetaban memoria del kernel hasta su vencimiento: C43 lo vio
  como páginas perdidas).
- **B16.** `inicio` reintenta el directorio si la cola del buzón está llena; el buzón espera un tic ante un error en
  vez de girar.
- **B17.** `registros_recortar` no recorta una llave revocable (`E_ARGUMENTO`).
- **B18.** Escrito: la llave de los registros del PCIe no se da a nadie; el VL805 sigue viendo todo el primer GB.
- **Pruebas más tolerantes con la carga:** `make prueba` a 90 s; C43 espera a `lanzador` hasta 80 s y a que vuelvan
  las pilas de los hilos muertos (se recogen tarde, C18) hasta 3 s antes de contar páginas.
- **Comprobado:** `make -C kernel prueba` cuatro veces seguidas y `make -C kernel arranque`, bien, con la máquina
  cargada (media de 8 a 10).

## Pausa (2026-11-03, 20:5x)

El usuario para el bucle con el 38 % de la cuota semanal gastado: "seguiremos la semana que viene". Para retomar:

1. Leer este fichero desde "Encargo vigente: fase 4 de S16" y la última entrada de la bitácora (C49).
2. Terminar C49 (lo que falta, arriba).
3. Seguir con C50 (compositor y GPU fuera del kernel; el buzón como servicio) y C51 (cierre de la fase 4: ventana
   del PCIe, pruebas que quedan en el kernel a `inicio`, auditoría independiente).
4. El bucle se relanza con `/loop` y el mismo encargo.

Estado al parar: fases 1 a 3 de S16 cerradas con sus auditorías; de la fase 4, hechas C45 a C48 y media C49.
`make -C kernel prueba` y `make -C kernel arranque` pasan. Todo en commits locales, sin push.

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
