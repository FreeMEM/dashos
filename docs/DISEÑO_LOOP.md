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
- [x] S4 **La caja Amiga.** exec dentro de la caja, `Forbid()` y `Disable()`, varias cajas a la vez, llamadas a
  bibliotecas nativas desde el JIT, recursos con dueño para matar tareas 68k sueltas (`S2-planificador.md`), ventanas Amiga como ventanas del escritorio (la idea de Newbench), volúmenes
  DOS sobre el sistema de archivos, y qué programas 68k quedan fuera.
- [x] S4b **Organización a lo Amiga.** Petición del usuario (2026-11-03): volúmenes, cajones, asignaciones, `SYS:`,
  `C:`, `S:`, `L:`, `LIBS:`, `DEVS:`, `FONTS:`, `LOCALE:`, `ENV:` y `ENVARC:`, Prefs, `DEVS:Monitors`, dispositivos,
  catálogos e iconos, igual que en AmigaOS. Cómo conviven con usuarios y permisos de Linux (dónde vive lo de cada
  usuario), con los programas nativos, los 68k y los de Linux (que esperan `/usr`, `/etc`, `/home`), y con los
  paquetes (S8).
- [x] S5 **Sistema de archivos.** Rápido y seguro: copia al escribir, sumas de comprobación, instantáneas, cifrado y
  compresión. Diseñar uno propio, adaptar uno existente o empezar con uno sencillo. Qué otros sistemas lee y escribe:
  FAT32 para el arranque, FFS y PFS del Amiga, ext4.
- [x] S6 **Seguridad.** Usuarios, grupos, propietarios y permisos como en Linux (decisión del usuario), PID por
  proceso, y cómo se traducen a capacidades del kernel. Permisos por aplicación, aislamiento, firmas de paquetes y
  arranque verificado (lo que permite la Pi 4).
- [x] S7 **Software de Linux.** Cómo ejecutar programas de Linux. Opciones a estudiar: una capa de llamadas al
  sistema de Linux (como hacen FreeBSD y el primer WSL), una máquina virtual con Linux y sus ventanas en el
  escritorio (como ChromeOS y WSLg), o portar programas. Ojo: los programas Linux de ARM son little-endian. AArch64
  permite que cada proceso de usuario tenga su orden de bytes (`SCTLR_EL1.E0E`): comprobar que la Pi 4 lo admite y
  qué cuesta.
- [x] S8 **Paquetes y Hangar.** Un gestor de paquetes como pacman: formato, repositorios, firmas, dependencias,
  actualizaciones atómicas con instantáneas del sistema de archivos, y deltas. Tres clases de paquete: nativos de
  DashOS, programas Amiga y programas de Linux. Hangar como tienda y catálogo por encima, con programas de pago.
- [x] S9 **Gráficos.** De la pantalla del firmware a la GPU V3D de la Pi 4. Las dos salidas HDMI. Compositor,
  sincronía vertical, escalado para pantallas de mucha densidad.
- [x] S10 **Escritorio.** Ventanas flotantes y en mosaico en el mismo escritorio, escritorios virtuales, varios
  monitores, lanzador, notificaciones, bandeja. El aire del Amiga hecho moderno. Referencias que el usuario usa a
  diario: niri, mango, DMS.
- [x] S11 **Interfaz de programación.** La API nativa de DashOS para aplicaciones nuevas, frente a la API Amiga de
  la caja (decisión pendiente en `PLAN.md`). Juego de controles para la interfaz, formato de ejecutables, bibliotecas
  compartidas y cómo encaja Dash.
- [x] S12 **Controladores de la Pi 4.** Lista y orden: serie, temporizador, interrupciones, tarjeta SD, USB (VL805
  por PCIe), red GENET, Wi-Fi y Bluetooth, audio, HDMI. Cuáles se escriben, cuáles se adaptan y con qué licencia.
- [x] S13 **Red.** Pila TCP/IP, cortafuegos, DNS, TLS, y `bsdsocket.library` para los programas Amiga.
- [x] S14 **Arranque, servicios y actualizaciones.** Del firmware de la Pi al escritorio. Gestor de servicios.
  Actualizaciones A/B o por instantáneas, y volver atrás si algo falla.
- [x] S15 **Hoja de ruta.** Todo lo anterior ordenado en fases con hitos que se puedan enseñar. Qué hace falta para
  la primera versión que arranca en una Pi 4 real.

## Decisiones pendientes

Preguntas para el usuario que salen de las tareas. Cada una dice de qué tarea viene y qué opción se sigue mientras.

- (S0) ¿Valen las metas de `docs/diseño/S0-principios.md`? Sobre todo arranque en menos de 5 s y memoria en reposo
  por debajo de 128 MB. Mientras: esas.
- (S0) ¿La Pi 3 entra como placa soportada? Mientras: no; solo como placa de pruebas si sale barato.
- (S4) ¿Adornos de ventana Amiga por dentro y borde de DashOS por fuera, o todo Amiga? Mientras: Amiga por
  dentro, DashOS por fuera.
- (S4) ¿Cargar bibliotecas de la ROM de Kickstart del usuario mientras falten las nativas? Mientras: sí.
- (S4) ¿Amiberry empaquetado como aplicación para lo que toca el chipset? Mientras: sí.
- (S4b) ¿Nombre del volumen de paquetes: `Paquetes:`, `Programas:` o `Apps:`? Mientras: `Paquetes:`.
- (S4b) ¿Sin distinguir mayúsculas en todo el sistema, con volúmenes que sí distinguen a petición? Mientras: sí.
- (S4b) ¿Preferencias compartidas con la caja en IFF, y las solo de DashOS en texto? Mientras: sí.
- (S5) ¿Partir de RedoxFS o diseñar uno propio? Mientras: RedoxFS, con rama propia.
- (S5) ¿Cajón de cada usuario cifrado de serie con su contraseña (Adiantum en la Pi 4)? Mientras: sí.
- (S5) ¿Instantáneas automáticas del cajón del usuario cada hora? Mientras: sí.
- (S6) ¿Sin superusuario: administradores que confirman con contraseña, y `SYS:` intocable salvo actualizaciones?
  Mientras: sí.
- (S6) ¿Permisos por aplicación preguntados en el primer uso? Mientras: sí.
- (S6) ¿Arranque verificado de la Pi 4 solo como opción documentada (es irreversible)? Mientras: sí.
- (S7) ¿Orden: biblioteca POSIX nativa (relibc), después caja Linux en máquina virtual, y capa de llamadas solo si
  hace falta? Mientras: sí.
- (S7) ¿Distribución de la caja Linux: Arch Linux ARM o Debian? Mientras: Arch Linux ARM.
- (S7) ¿Caja Linux solo desde 4 GB de RAM? Mientras: sí.
- (S8) ¿Nombre de la orden de paquetes? Mientras: `Paq`.
- (S8) ¿Sistema base como imagen firmada y paquetes por encima, como Silverblue? Mientras: sí.
- (S8) ¿Hangar como tienda encima de `Paq`, con una arquitectura de DashOS en su catálogo? Mientras: sí (se decide
  en el proyecto Hangar).
- (S9) ¿Gráficos por fases: pantalla del firmware, controlador propio del HVS y HDMI, y la GPU al final? Mientras:
  sí.
- (S9) ¿Wayland como protocolo propio del compositor, sobre canales de DashOS? Mientras: sí.
- (S10) ¿Escritorios virtuales como "pantallas" del Amiga, con el gesto de arrastrar? Mientras: sí.
- (S10) ¿Menús del programa en la barra de la pantalla con el botón derecho? Mientras: sí.
- (S13) ¿smoltcp como pila de red, mejorándola si hace falta? Mientras: sí.
- (S13) ¿DNS cifrado de serie o como opción? Mientras: como opción.
- (S13) ¿`amissl.library` nativa sobre rustls después de la primera versión? Mientras: sí.
- (S14) ¿A/B del firmware para kernel e imagen de arranque, y `SYS:` por instantáneas, probados juntos con
  `tryboot`? Mientras: sí.
- (S14) ¿Menú de arranque temprano a lo Amiga, y con qué tecla? Mientras: sí, barra espaciadora o los dos botones.
- (S14) ¿Pantalla de entrada siempre (el cajón va cifrado)? Mientras: sí.
- (S15) ¿Este orden, con la primera versión en H8 y la caja Linux en la 0.2? Mientras: sí.
- (S15) ¿Arrancar ya la línea de análisis de la sala limpia, empezando por exec? Mientras: sí, cuando el usuario lo diga.
- (S15) ¿Avisar ya a las sesiones de Dash del backend ARM? Mientras: no se ha avisado.

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

### 2026-11-03 — Requisitos del usuario: PID, permisos de Linux y organización de AmigaOS

Cada proceso con su PID. Usuarios, grupos, propietarios y permisos como en Linux (contesta la pregunta de S0 sobre
varias cuentas: sí). Y toda la organización de AmigaOS igual: cajones, asignaciones, Prefs, `DEVS:Monitors`,
`LOCALE:`, dispositivos. Pasa a `PLAN.md`; tarea nueva S4b para la organización, y S6 ampliada.

### 2026-11-03 — Decisiones del usuario sobre S3

Aceptadas las tres recomendaciones: sin intercambio a disco, sin prometer de más y Enforcer siempre activo.

### 2026-11-03 — S4 (la caja Amiga)

`docs/diseño/S4-caja-amiga.md`. Comprobado: Emu68 no tiene llamada a código nativo, y la línea A del 68k se traduce
en una sola función (`EMIT_lineA`), el sitio natural para añadirla. MorphOS hace lo mismo: interfaz 68k en sus
bibliotecas nativas y emulación solo si alguien parchea la tabla. Recomendación: línea A en la tabla de saltos;
una caja usa un núcleo a la vez con un testigo (corrige S2: repartir una caja entre núcleos no es seguro); ventanas
Amiga como ventanas del escritorio; dos.library sobre el sistema de archivos; varias cajas; módulos del Kickstart del
usuario como atajo; Amiberry empaquetado para lo que toca el chipset. Las bibliotecas de la caja necesitan punteros
de 32 bits: razón de fondo para escribirlas en Dash.

### 2026-11-03 — S4b (organización a lo Amiga)

`docs/diseño/S4b-organizacion.md`. La idea central: las asignaciones del Amiga con la semántica de Plan 9, por
proceso. Cada asignación de sistema es una multiasignación en tres capas (usuario, paquetes, sistema), así que
`ENVARC:` o `LIBS:` son distintos por usuario sin que los programas lo noten. `SYS:` de solo lectura; los paquetes
declaran sus asignaciones en vez de editar `S:User-Startup`. `DEVS:Monitors` con un fichero por salida HDMI.
Catálogos en UTF-8, con Latin-1 para la caja. Vista POSIX para Linux con `/Volumes`, como macOS.

### 2026-11-03 — S5 (sistema de archivos)

`docs/diseño/S5-archivos.md`. Recomendación: partir de RedoxFS (Rust, micronúcleo, copia al escribir, sumas de
comprobación, cifrado, MIT) y añadirle instantáneas, compresión, búsqueda sin mayúsculas y atributos del Amiga, con
el disco en little-endian. Hallazgo: la Pi 4 no tiene AES por hardware; el cifrado tiene que ser Adiantum (ChaCha).
FAT32 obligatorio para el arranque; FFS, SFS y PFS3 para discos Amiga; ext4 de lectura con `ext4-view`. Pendiente
de comprobar: el orden de bytes del formato de RedoxFS.

### 2026-11-03 — S6 (seguridad)

`docs/diseño/S6-seguridad.md`. Dos capas: quién (usuarios, grupos y permisos de Linux, decisión del usuario) y qué
programa (permisos por aplicación, como Android y macOS). Las dos van sobre capacidades: cada canal lleva la
identidad de quien lo abrió. Sin superusuario: administradores que confirman cada acción. El selector de ficheros
entrega una llave para el fichero elegido y nada más. Paquetes firmados con Ed25519. Comprobado: el arranque
verificado de la Pi 4 es irreversible (OTP) y solo completo en B1/C0; se recomienda como opción, no de serie.

### 2026-11-03 — S7 (software de Linux)

`docs/diseño/S7-linux.md`. Comprobado en QEMU: `SCTLR_EL1.E0E` se puede activar, así que un kernel big-endian puede
tener procesos little-endian (prueba ampliada en `pruebas/rust-be/`). Recomendación en tres pasos: relibc como
biblioteca POSIX nativa; una caja Linux en máquina virtual, como Crostini y WSL2, con las ventanas reenviadas al
compositor; y la capa de llamadas al sistema (con Asterinas de referencia) solo más adelante. Lección de WSL1: una
capa de traducción nunca está completa. Consecuencia para S9: el compositor tiene que aceptar ventanas de Wayland.

### 2026-11-03 — S8 (paquetes y Hangar)

`docs/diseño/S8-paquetes.md`. Dos niveles como Fedora Silverblue: `SYS:` como imagen firmada que entra al reiniciar,
y paquetes por encima en `Paquetes:` o `Home:`, con cada transacción en una instantánea. Un gestor, `Paq`, con una
orden que lo pone todo al día, como `pacman -Syu`. Tres mundos: nativos y Amiga con `Paq` (los Amiga con su
`Installer` dentro de la caja), Linux con el gestor de su distribución. Hangar es la tienda y `Paq` el motor; el
cliente de Hangar para el Amiga funcionaría en la caja desde el primer día. Hangar solo se leyó, no se tocó.

### 2026-11-03 — S9 (gráficos)

`docs/diseño/S9-graficos.md`. Por fases: pantalla del firmware y compuesto por CPU con daños; después controlador
propio del HVS y HDMI (sincronía vertical, dos salidas, planos para cursor y vídeo); la GPU V3D con Mesa al final.
Wayland como protocolo propio del compositor, sobre canales de DashOS: los kits de interfaz se portan sin reescribir,
y la caja Linux y la caja Amiga son clientes Wayland. Medido con la API de GitHub: v3d son unos 200 KB de C en Linux
y vc4 unos 770 KB. No hay manual público de la V3D.

### 2026-11-03 — S10 (escritorio)

`docs/diseño/S10-escritorio.md`. Los escritorios virtuales son "pantallas" del Amiga, por monitor y dinámicas como en
niri, con el gesto de arrastrar hacia abajo. Cada pantalla elige disposición: flotante, mosaico con desplazamiento
(niri) o las de mango; cualquier ventana puede flotar. Workbench con iconos y cajones, barra de pantalla con los
menús del programa al botón derecho y la bandeja de DMS a la derecha, lanzador por teclado, Commodities y puerto
ARexx. Compositor y escritorio son procesos separados, como niri y DMS.

### 2026-11-03 — S11 (interfaz de programación)

`docs/diseño/S11-api.md`. API nativa "Amiga moderna": bibliotecas por nombre y versión, puertos sobre canales,
rutas y asignaciones, catálogos, IFF, DataTypes, ARexx e iconos, pero de 64 bits, con capacidades y sin memoria
compartida por defecto; POSIX con relibc para portar. Al kernel solo por una biblioteca, como el vDSO de Fuchsia; sin
el paso de interfaces de AmigaOS 4. ELF big-endian, y las `.library` nativas son ELF con versión. Kit de interfaz con
el modelo de MUI (como Zune), para que `.dashui` valga en los dos sistemas. Consecuencia: un programa Dash debería
compilar para AmigaOS 3.x y para DashOS sin cambios.

### 2026-11-03 — S12 (controladores de la Pi 4)

`docs/diseño/S12-controladores.md`. Comprobado con `info qtree`: QEMU `raspi4b` emula serie, GIC, buzón, pantalla del
firmware, SD, USB DWC2, GPIO y DMA, pero no PCIe con el VL805, GENET, HVS y HDMI, V3D ni Wi-Fi. Comprobado con
`info usb`: el DWC2 de QEMU acepta teclado, ratón y disco USB, así que hay entrada en QEMU desde el principio.
Las fases Kernel y Base caben enteras en QEMU; la Pi 4 hace falta a partir de la fase Placa. El VL805 de las placas
nuevas pierde su firmware y la VideoCore lo recarga con la etiqueta `0x00030058`. Fuentes permitidas: hoja de datos,
normas, FreeBSD y NetBSD, Emu68 y los tutoriales de Rust; Linux solo para entender, con sala limpia.

### 2026-11-03 — S13 (red)

`docs/diseño/S13-red.md`. smoltcp como pila (comprobado: v0.14.0 tiene CUBIC y Reno y escalado de ventana; le faltan
SACK y marcas de tiempo). La red es big-endian, así que DashOS no da la vuelta a las cabeceras. TLS con rustls y
ChaCha20 primero en la Pi 4, como hacen Go y Firefox sin AES. Cortafuegos cerrado a la entrada. `bsdsocket.library`
nativa en la caja, para que Tachyon e Inferencia tengan red sin pila del Amiga; AmiSSL 68k sigue en el JIT y una
versión nativa sobre rustls queda para después.

### 2026-11-03 — Decisiones del usuario sobre S11 y S12

API nativa "Amiga moderna" con POSIX al lado; kit con el modelo de MUI, y además ReAction y GadTools (en la caja y
como caras del motor nativo) y GTK y Qt (en la caja Linux ya, en nativo más adelante). Añadido a `S11-api.md`. Las
tres de S12, sí. Todo pasa a `PLAN.md`. Sigue abierta la de S11 sobre el mismo programa Dash en los dos sistemas.

### 2026-11-03 — Decisión del usuario sobre Dash

El mismo programa Dash compila para DashOS (AArch64 big-endian) y para AmigaOS 3.x (68k). Pasa a `PLAN.md`.

### 2026-11-03 — S14 (arranque, servicios y actualizaciones)

`docs/diseño/S14-arranque.md`. Comprobado en la documentación de Raspberry Pi: el firmware tiene A/B con
`autoboot.txt`, `tryboot_a_b=1` y `reboot "0 tryboot"`, que vuelve solo a la partición buena si la nueva falla;
Ubuntu le suma el vigilante. Tarjeta con partición de arranque, ranuras A y B (kernel e imagen de arranque) y la del
sistema con instantáneas; kernel y `SYS:` se prueban juntos. Gestor de servicios declarativo, en paralelo y con
vigilancia. Del Amiga: `S:Startup-Sequence` para la caja, `WBStartup` y un menú de arranque temprano.

### 2026-11-03 — S15 (hoja de ruta) y fin del bucle

`docs/diseño/S15-hoja-de-ruta.md`: nueve hitos que se pueden enseñar, de la llamada nativa desde el 68k (H1) a la
caja Linux (H9). DashOS 0.1 son H1 a H8; la Pi 4 se compra al terminar H3. Tres líneas en paralelo: sistema, Dash y
análisis. `docs/diseño/RESUMEN.md` recoge el sistema entero. No quedan tareas: el bucle se para.

### 2026-11-03 — Después del fin: escritorio sin Quickshell y mosaico natural

Petición del usuario: escritorio al nivel de DMS sin Quickshell, y mosaico y flotantes a la vez con naturalidad.
Añadido a `S10-escritorio.md`: arrastrar saca y devuelve ventanas del mosaico, encaje en los bordes, cada ventana
nace donde le toca y se recuerda; escritorio nativo con el motor de S11; la pantalla de bloqueo en el compositor.
Pasa a `PLAN.md`.

### 2026-11-03 — Personalización del escritorio

Petición del usuario: personalización tan moderna como el resto. Añadido a `S10-escritorio.md`: colores del fondo,
fondos de vídeo, barra editable, temas como paquetes en Hangar, módulos de terceros en Dash con capacidades justas, y
Prefs con Usar, Guardar y Cancelar. Pasa a `PLAN.md` y al resumen.

