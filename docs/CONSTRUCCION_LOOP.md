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
- [ ] C3 **Excepciones.** Tabla de vectores de EL1, volcado de registros y pánico legible. Prueba: un `brk` provocado.
- [ ] C4 **Árbol de dispositivos.** Leer el DTB que deja el firmware en `x0`: memoria, puerto serie, GIC, temporizador
  y `dma-ranges` (S3). Prueba: imprime el mapa.
- [ ] C5 **Memoria física.** Asignador de páginas con zonas (primer GB, primeros 3 GB, resto) sacadas de C4. Prueba.
- [ ] C6 **MMU.** Mitad alta del kernel en `TTBR1`, periféricos como dispositivo, páginas de 4 KB. Prueba.
- [ ] C7 **Montón del kernel.** `alloc` sobre C5. Prueba.
- [ ] C8 **Temporizador y GIC-400.** Interrupción periódica. Prueba: cuenta tics.
- [ ] C9 **Cuatro núcleos.** Despertar 1 a 3 con la tabla de espera de la Pi (como Emu68 y QEMU). Prueba: cada núcleo
  se presenta.
- [ ] C10 **Hilos y planificador.** Cambio de contexto, una cola por núcleo, clase justa sencilla (S2). Prueba: hilos
  que se turnan.
- [ ] C11 **Procesos en EL0.** Espacio de direcciones propio con ASID, llamada al sistema por `svc`. Prueba: un
  proceso de usuario escribe por una llamada.
- [ ] C12 **Capacidades y canales.** Tabla de llaves por proceso, canales con mensajes cortos y paso de llaves (S1).
  Prueba: dos procesos se pasan mensajes.
- [ ] C13 **Objetos de memoria.** Crear, proyectar y compartir (S3). Prueba: dos procesos comparten una página.
- [ ] C14 **Clase de plazo.** Presupuesto y periodo (S2). Prueba: un hilo de plazo cumple con los cuatro núcleos
  cargados.
- [ ] C15 **Buzón de la VideoCore y pantalla del firmware.** Controlador del buzón y la pantalla (fase A de S9) como
  base del vídeo. Prueba: pinta un degradado y lo comprueba leyendo la memoria de la pantalla.

### Análisis de vídeo y GPU (agente analista)

- [~] V1 **HVS**: lista de visualización, planos, formatos, escalado, FIFOs y su reparto entre salidas en la BCM2711
  (`drivers/gpu/drm/vc4/vc4_hvs.c`, `vc4_plane.c`, `vc4_crtc.c`, `vc4_regs.h`).
- [ ] V2 **Pixel valves y HDMI de la BCM2711**: modos, relojes, PHY, las dos salidas, interrupción de sincronía
  vertical, audio por HDMI (`vc4_hdmi.c`, `vc4_hdmi_phy.c`, `vc4_hdmi_regs.h`, `vc4_crtc.c`).
- [ ] V3 **V3D, lado del kernel**: memoria de la GPU y su MMU, envío de trabajos (binner, render, TFU, CSD),
  interrupciones, reinicio y relojes (`drivers/gpu/drm/v3d/`).
- [ ] V4 **Mesa v3d y v3dv frente al kernel**: qué llamadas de DRM usa Mesa con v3d y vc4, qué estructuras se pasan y
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

