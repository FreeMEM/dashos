# Bucle de diseño de DashOS

## Encargo

Encargo del usuario, del 2026-11-03:

> Crea un loop para el diseño del S.O. Debe ser moderno, sistema de archivos rápido y seguro, multicore, protección
> de memoria, y todas las cosas modernas que lo hagan un S.O. mejor que Linux. Que se pueda alimentar de todo lo de
> Amiga, pero también que sea sencillo aprovechar software de Linux. Con un sistema de paquetes que se puedan
> actualizar tipo pacman y un proyecto Hangar (en el que estamos trabajando). Un escritorio moderno, con ventanas
> flotantes y tiles, escritorios virtuales, multimonitor, etc.

Este bucle produce el diseño, no el sistema. Cada vuelta deja un documento de diseño en `docs/diseño/`. Las
decisiones grandes las toma el usuario: el bucle prepara las opciones y recomienda una.

Base de partida: `PLAN.md`. Ya está decidido lo siguiente:

- La primera placa es la Pi 4, y se desarrolla en QEMU `raspi4b`.
- El sistema es AArch64 big-endian.
- El kernel va en Rust, el JIT en C (el de Emu68), y las bibliotecas Amiga y las aplicaciones en Dash.
- Los programas 68k viven en una "caja Amiga", que es un proceso con la memoria compartida por dentro.

## Qué significa "mejor que Linux"

Linux tiene treinta años de controladores y millones de programas. DashOS no le gana en eso. El diseño elige dónde
puede ganar y lo dice con números que se puedan medir. Candidatos:

- **Arranque**: de encender a escritorio en pocos segundos.
- **Respuesta**: el escritorio no se traba nunca, aunque el sistema esté cargado. Es la sensación del Amiga.
- **Coherencia**: un solo escritorio, un solo juego de herramientas y una sola forma de configurar las cosas.
- **Seguridad por defecto**: cada aplicación ve solo lo que se le da.
- **Actualizaciones atómicas**: una actualización se aplica entera o no se aplica, y se puede deshacer.
- **Sencillez**: un sistema que una persona puede entender entero.

La tarea S0 fija cuáles de estos son objetivos, con qué número y cómo se miden.

## Reglas de cada vuelta

1. **Uso.** Al empezar, lee `~/.claude/state/uso-7d.txt` (porcentaje y hora). Si pasa del 90 %, apúntalo en la
   bitácora, para el bucle y avisa con PushNotification. Si la hora tiene más de una hora, avisa de que el dato
   puede no estar al día.
2. **Tarea.** Coge la primera tarea `[ ]` de la lista. Márcala `[~]` mientras trabajas. Una por vuelta. Si es
   grande, divídela en la lista y haz solo la primera parte.
3. **Investigar.** Antes de proponer, mira cómo lo resuelven otros sistemas: Linux, seL4, Fuchsia, Redox, Haiku,
   Plan 9, MorphOS, AmigaOS 4, macOS y los que vengan al caso. Usa WebSearch y las fuentes originales. Cita lo que
   uses, con enlace.
4. **Comprobar.** Cuando una propuesta dependa de un dato que se puede comprobar en shiva, compruébalo: QEMU, las
   toolchains, `jit-host/`, el código de Emu68. Di qué se comprobó y qué no.
5. **Escribir.** Un documento por tarea, `docs/diseño/<tarea>-<tema>.md`, con este orden:
   - El problema, en pocas frases.
   - Las opciones, de dos a cuatro, con lo que gana y lo que cuesta cada una.
   - La recomendación y su porqué.
   - Lo que queda abierto: las preguntas para el usuario, cada una en una línea.
   - Cómo encaja con las tareas anteriores.

   Redacción clara: una idea por frase, sujeto y verbo, casi sin incisos. Sin tablas de adorno: solo para comparar
   opciones.
6. **Decisiones.** El bucle no decide lo que es del usuario. Si una tarea necesita una decisión pendiente, sigue
   con la opción recomendada, marcada como provisional. Apunta la pregunta en "Decisiones pendientes", al final de
   este fichero. Cuando el usuario decide, se pasa a `PLAN.md` y se quita de la lista.
7. **Sala limpia.** Este bucle puede leer documentación pública de AmigaOS, MorphOS y AROS. No lee código de AROS ni
   desensamblados del Kickstart: esos los usan solo las sesiones de análisis (`AGENTS.md`, regla 1).
8. **Hangar.** Hangar es privado (`~/Proyectos/hangar`, plan en `servicio/docs/PLAN.md`). El bucle puede leerlo
   para diseñar cómo encaja con DashOS. No copia nada de Hangar en este repositorio: solo describe la integración.
9. **Sin código del sistema.** Este bucle no escribe el kernel. Sí puede hacer pruebas pequeñas para comprobar un
   dato, en `docs/diseño/pruebas/` o en el scratchpad.
10. **Cerrar.** Marca la tarea `[x]`, añade una entrada a la bitácora y haz un commit local
    `diseño <tarea>: <resumen>`. Sin push.
11. **Avisar.** Al cerrar cada tarea, manda al usuario una notificación con PushNotification: la tarea, la
    recomendación en una frase y las preguntas que deja.
12. **Fin.** Cuando no queden tareas `[ ]`, escribe `docs/diseño/RESUMEN.md` con el sistema entero en pocas páginas,
    avisa y para el bucle.

## Tareas

- [x] S0 **Principios y metas.** En qué será DashOS mejor que Linux, con números medibles. En qué no compite. Qué
  hardware cubre y cuál no.
- [x] S1 **Arquitectura del kernel.** Micronúcleo, monolítico o híbrido. Capacidades (como seL4) o permisos
  clásicos. Comunicación entre procesos. Controladores dentro o fuera del kernel. Qué se aprovecha del ecosistema
  Rust para kernels.
- [x] S2 **Núcleos y planificador.** Reparto entre los cuatro núcleos de la Pi 4, prioridades, tiempo real suave
  para audio y escritorio, ahorro de energía.
- [x] S3 **Memoria.** Espacios de direcciones, protección, memoria compartida, el hueco de 4 GB para la caja Amiga,
  memoria comprimida en vez de intercambio, y cómo se reparte con poca RAM (la Pi 4 tiene de 1 a 8 GB).
- [ ] S4 **La caja Amiga.** exec dentro de la caja, `Forbid()` y `Disable()`, varias cajas a la vez, llamadas a
  bibliotecas nativas desde el JIT, recursos con dueño para matar tareas 68k sueltas (`S2-planificador.md`), ventanas Amiga como ventanas del escritorio (la idea de Newbench), volúmenes
  DOS sobre el sistema de archivos, y qué programas 68k quedan fuera.
- [ ] S5 **Sistema de archivos.** Rápido y seguro: copia al escribir, sumas de comprobación, instantáneas, cifrado y
  compresión. Diseñar uno propio, adaptar uno existente o empezar con uno sencillo. Qué otros sistemas lee y escribe:
  FAT32 para el arranque, FFS y PFS del Amiga, ext4.
- [ ] S6 **Seguridad.** Permisos por aplicación, aislamiento, firmas de paquetes, arranque verificado (lo que
  permite la Pi 4) y cuentas de usuario.
- [ ] S7 **Software de Linux.** Cómo ejecutar programas de Linux. Opciones a estudiar: una capa de llamadas al
  sistema de Linux (como hacen FreeBSD y el primer WSL), una máquina virtual con Linux y sus ventanas en el
  escritorio (como ChromeOS y WSLg), o portar programas. Ojo: los programas Linux de ARM son little-endian. AArch64
  permite que cada proceso de usuario tenga su orden de bytes (`SCTLR_EL1.E0E`): comprobar que la Pi 4 lo admite y
  qué cuesta.
- [ ] S8 **Paquetes y Hangar.** Un gestor de paquetes como pacman: formato, repositorios, firmas, dependencias,
  actualizaciones atómicas con instantáneas del sistema de archivos, y deltas. Tres clases de paquete: nativos de
  DashOS, programas Amiga y programas de Linux. Hangar como tienda y catálogo por encima, con programas de pago.
- [ ] S9 **Gráficos.** De la pantalla del firmware a la GPU V3D de la Pi 4. Las dos salidas HDMI. Compositor,
  sincronía vertical, escalado para pantallas de mucha densidad.
- [ ] S10 **Escritorio.** Ventanas flotantes y en mosaico en el mismo escritorio, escritorios virtuales, varios
  monitores, lanzador, notificaciones, bandeja. El aire del Amiga hecho moderno. Referencias que el usuario usa a
  diario: niri, mango, DMS.
- [ ] S11 **Interfaz de programación.** La API nativa de DashOS para aplicaciones nuevas, frente a la API Amiga de
  la caja (decisión pendiente en `PLAN.md`). Juego de controles para la interfaz, formato de ejecutables, bibliotecas
  compartidas y cómo encaja Dash.
- [ ] S12 **Controladores de la Pi 4.** Lista y orden: serie, temporizador, interrupciones, tarjeta SD, USB (VL805
  por PCIe), red GENET, Wi-Fi y Bluetooth, audio, HDMI. Cuáles se escriben, cuáles se adaptan y con qué licencia.
- [ ] S13 **Red.** Pila TCP/IP, cortafuegos, DNS, TLS, y `bsdsocket.library` para los programas Amiga.
- [ ] S14 **Arranque, servicios y actualizaciones.** Del firmware de la Pi al escritorio. Gestor de servicios.
  Actualizaciones A/B o por instantáneas, y volver atrás si algo falla.
- [ ] S15 **Hoja de ruta.** Todo lo anterior ordenado en fases con hitos que se puedan enseñar. Qué hace falta para
  la primera versión que arranca en una Pi 4 real.

## Decisiones pendientes

Preguntas para el usuario que salen de las tareas. Cada una dice de qué tarea viene y qué opción se sigue mientras.

- (De `PLAN.md`) Si las aplicaciones nativas nuevas usan la API Amiga dentro de la caja, una API moderna fuera, o
  las dos. Mientras: las dos, la moderna como principal.
- (S0) ¿Valen las metas de `docs/diseño/S0-principios.md`? Sobre todo arranque en menos de 5 s y memoria en reposo
  por debajo de 128 MB. Mientras: esas.
- (S0) ¿La Pi 3 entra como placa soportada? Mientras: no; solo como placa de pruebas si sale barato.
- (S0) ¿Una persona por máquina, o varias cuentas? Mientras: una persona, con cuentas separadas para servicios.
- (S3) ¿Sin intercambio a disco por defecto, solo memoria comprimida (LZ4)? Mientras: sí.
- (S3) ¿Sin prometer de más memoria (falla al pedir, no hay OOM killer)? Mientras: sí, con margen en la capa Linux.
- (S3) ¿Enforcer integrado en la caja, siempre activo? Mientras: sí, con avisos a un registro.
- (S0) ¿Comprar ya una Pi 4 para medir la referencia con Raspberry Pi OS? Mientras: las cifras de Linux son de oídas.

## Bitácora

### 2026-11-03 — Plan

Bucle creado por encargo del usuario. Dieciséis tareas, de S0 a S15.

### 2026-11-03 — S0 (principios y metas)

`docs/diseño/S0-principios.md`: ocho principios ordenados (el primero, que el sistema responda siempre) y nueve
metas con número y forma de medirlas. Lo más útil que salió: el firmware de la Pi 4 se come unos 3,5 s antes del
kernel, así que la meta propia es "del kernel al escritorio en menos de 1 s". Y la latencia de entrada de Linux con
Wayland ya es buena (diferencias de menos de 1 ms entre compositores): DashOS debe no perder ahí y ganar con el
sistema cargado. Cuatro preguntas para el usuario.

### 2026-11-03 — S1 (arquitectura del kernel)

`docs/diseño/S1-kernel.md`: recomendación de micronúcleo con capacidades, como seL4, QNX o Fuchsia, con dos ideas
del framekernel de Asterinas (el `unsafe` en una sola caja; su capa de Linux como referencia). Comprobado con una
prueba (`docs/diseño/pruebas/rust-be/`): Rust nightly compila para `aarch64_be-unknown-none-softfloat` y arranca en
QEMU `raspi4b` en big-endian; la CPU declara `BigEnd = 1`, así que admite programas de usuario little-endian (bueno
para S7). Hallazgo: cada acceso a un periférico hay que darle la vuelta, y muchas bibliotecas de Rust para
controladores dan por hecho little-endian. Rust nightly instalado en shiva (`rustup toolchain install nightly`).

### 2026-11-03 — S1 ampliada

A petición del usuario, S1 compara ocho formas de kernel (añadidos exokernel, un solo espacio de direcciones,
multikernel y unikernel) con una tabla, y separa micronúcleo puro de pragmático. El usuario aprueba el micronúcleo
y la licencia MPL-2.0 (`LICENSE`); los dos pasan a `PLAN.md`.

### 2026-11-03 — S2 (núcleos y planificador)

`docs/diseño/S2-planificador.md`: dos clases como Fuchsia, plazos con presupuesto para compositor, entrada y audio,
y reparto justo con pesos para el resto. Los servidores trabajan con el presupuesto del cliente (servidores pasivos
de seL4 MCS). Una cola por núcleo con robo de trabajo. La caja Amiga es un grupo de la clase justa con la prioridad
estricta de exec por dentro. Frecuencia por el buzón de la VideoCore, con las etiquetas que ya usa Emu68.

### 2026-11-03 — Decisiones del usuario sobre S1 y S2

Micronúcleo confirmado. Dentro de la caja, prioridad estricta de exec. Energía: responder. Requisito nuevo: matar
cualquier proceso y que las señales de terminar se cumplan siempre. Añadida la sección "Matar procesos" a
`S2-planificador.md`: en nativo lo da el micronúcleo (todo recurso es un objeto con dueño); en la caja, el exec de
DashOS apunta el dueño de cada recurso, cosa que AmigaOS no hace.

### 2026-11-03 — S3 (memoria)

`docs/diseño/S3-memoria.md`: objetos de memoria con capacidad al estilo Fuchsia, páginas de 4 KB, memoria comprimida
con LZ4, sin intercambio a disco y sin prometer de más. Comprobado del registro de la CPU: ASID de 16 bits, páginas
de 4 y 64 KB (no de 16), y que la A72 no tiene PAN, PAC, BTI ni MTE. Comprobado del árbol de dispositivos: la SD y
el DMA clásico solo alcanzan el primer GB, y el USB los primeros 3 GB; el kernel necesita zonas. Idea nueva: un
Enforcer integrado en la caja Amiga.

