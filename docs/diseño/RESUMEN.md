# DashOS en pocas páginas

Resumen del bucle de diseño del 2026-11-03 (S0 a S15). Cada apartado remite a su documento en `docs/diseño/`. Lo que
dice "decidido" lo decidió el usuario; lo demás es la recomendación, pendiente de su visto bueno (lista al final de
`docs/DISEÑO_LOOP.md`).

## Qué es

Un sistema operativo propio para la Raspberry Pi 4 (después la Pi 5 y otras placas), moderno por dentro y Amiga por
fuera. Ejecuta los programas 68k de AmigaOS 3.x sin emular un Amiga entero, como Petunia en AmigaOS 4, y aprovecha
el software de Linux en una máquina virtual. El kernel está en Rust y las aplicaciones se escriben en Dash. Lo
nativo es little-endian; solo la caja Amiga es big-endian (decidido el 03/11 por la tarde: ver `PLAN.md`).

## Principios (S0)

Ocho, por orden. Los primeros: el sistema responde siempre; es seguro por defecto; todo o nada al actualizar; un solo
sistema coherente; los programas Amiga son de primera; Linux es un invitado; se entiende entero; todo se mide.

Metas: del kernel al escritorio en menos de 1 s, menos de 128 MB en reposo, 60 cuadros por segundo con los cuatro
núcleos al 100 %. Sin medir todavía.

## El kernel (S1, S2, S3)

- **Micronúcleo pragmático con capacidades** (decidido), como QNX, seL4 o Fuchsia. Licencia MPL-2.0 (decidido).
- Un proceso nuevo no tiene nada; solo las llaves que le da quien lo lanza.
- Mensajes cortos con paso directo del cliente al servidor, y anillos en memoria compartida para los datos.
- **Planificador** (decidido): clase de plazos con presupuesto para compositor, entrada y audio; clase justa para lo
  demás; el servidor trabaja con el tiempo del cliente. Perfil de energía: responder.
- **Matar siempre funciona** (decidido): todo recurso es un objeto con dueño; cierre en dos pasos.
- **Memoria** (decidido): objetos de memoria al estilo Fuchsia, páginas de 4 KB, zonas para el DMA de la Pi 4,
  memoria comprimida con LZ4, sin intercambio a disco y sin prometer de más.
- Comprobado: Rust compila y arranca en big-endian en QEMU `raspi4b`; la CPU admite procesos little-endian.

## La caja Amiga (S4)

- Un proceso con su exec, la memoria compartida por dentro y el JIT de Emu68.
- Las bibliotecas nativas se llaman desde el 68k por una instrucción de línea A en la tabla de saltos.
- Una caja usa un núcleo a la vez, con un testigo; `Forbid()` solo para la caja. Prioridad estricta de exec
  (decidido).
- exec apunta el dueño de cada recurso, así que se puede matar una tarea 68k suelta (decidido).
- Enforcer integrado, siempre activo (decidido).
- Las ventanas Amiga son ventanas del escritorio. Lo que toca el chipset va a Amiberry empaquetado.
- Mientras falten bibliotecas nativas, se cargan las de la ROM de Kickstart del usuario.
- Comprobado: el JIT de Emu68 funciona separado del resto.

## La organización (S4b)

**La de AmigaOS** (decidido): volúmenes, cajones, asignaciones, `C:`, `LIBS:`, `DEVS:`, `ENV:`, `ENVARC:`, Prefs,
`DEVS:Monitors`, `LOCALE:`. Las asignaciones son por proceso, como en Plan 9, y en tres capas: usuario, paquetes y
sistema. `SYS:` es de solo lectura. Los programas de Linux ven `/home` y `/Volumes`.

## Ficheros (S5)

Partir de RedoxFS (Rust, copia al escribir, sumas de comprobación, cifrado), con instantáneas, compresión,
búsqueda sin mayúsculas y atributos del Amiga. Cifrado del cajón de cada usuario con Adiantum, porque la Pi 4 no
tiene AES. FAT32 para el arranque, FFS, SFS y PFS3 para discos Amiga, ext4 de lectura.

## Seguridad (S6)

- **Usuarios, grupos y permisos como en Linux, y PID por proceso** (decidido), sobre capacidades.
- Permisos por aplicación, como Android o macOS; el selector de ficheros da una llave para el fichero elegido.
- Sin superusuario: administradores que confirman cada acción.
- Paquetes firmados con Ed25519. El arranque verificado de la Pi 4 solo como opción, porque es irreversible.

## Linux (S7)

relibc como biblioteca POSIX nativa; una caja Linux en máquina virtual (Arch Linux ARM), con sus ventanas en el
escritorio, como ChromeOS y WSL2; y la capa de llamadas de Linux solo más adelante.

## Paquetes (S8)

`SYS:` como imagen firmada que entra al reiniciar, y paquetes por encima con transacciones en instantáneas. Una orden,
`Paq`, lo pone todo al día. Hangar es la tienda encima de `Paq`.

## Gráficos y escritorio (S9, S10)

- Por fases: pantalla del firmware, después controlador propio del HVS y HDMI, y la GPU V3D con Mesa al final.
- El compositor habla Wayland sobre canales de DashOS.
- **Escritorio tan moderno como DMS, nativo y sin Quickshell; mosaico y flotantes a la vez, arrastrando**
  (decidido). La pantalla de bloqueo es del compositor.
- **Personalización completa** (decidido): colores del fondo, fondos de vídeo, barra editable, temas como paquetes
  y módulos de terceros en Dash; Prefs con Usar, Guardar y Cancelar, como en el Amiga.
- Escritorios virtuales como "pantallas" del Amiga; mosaico a lo niri y mango; Workbench;
  barra con los menús al botón derecho; lanzador; Commodities; ARexx.
- **Aspecto de Material Design** (decidido), con colores del fondo de pantalla, y un tema "Amiga clásico".

## Programar (S11)

- **API nativa con los conceptos del Amiga, de 64 bits y sobre capacidades, con POSIX al lado** (decidido).
- **Motor de interfaz con el modelo de MUI, y caras de ReAction y GadTools; GTK y Qt en la caja Linux** (decidido).
- **El mismo programa Dash compila para DashOS y para AmigaOS 68k** (decidido).
- ELF little-endian, o big-endian para las bibliotecas de la caja; las `.library` nativas se abren por nombre y versión. Al kernel solo por una biblioteca.

## Hardware (S12, S13, S14)

- QEMU emula serie, interrupciones, SD, USB-C, pantalla y buzón; no el USB normal, la red, el HDMI ni la GPU.
- **Sala limpia para lo no documentado, firmware cerrado del Wi-Fi aceptado, y la Pi 4 se compra al terminar la fase
  Base** (decidido).
- Red con smoltcp y rustls (ChaCha20 primero en la Pi 4), cortafuegos cerrado a la entrada, `bsdsocket.library`
  nativa en la caja.
- Arranque con el A/B del firmware (`tryboot`), servicios declarativos en paralelo, y un menú de arranque temprano a
  lo Amiga.

## Hoja de ruta (S15)

H1 llamada nativa desde el 68k → H2 kernel mínimo → H3 base con ventana y ratón en QEMU (se compra la Pi 4) → H4 caja
Amiga → H5 caja con ventanas → H6 la placa real → H7 Dash nativo y escritorio → H8 paquetes y actualizaciones =
**DashOS 0.1**. Después, H9 la caja Linux (0.2), la GPU, el Wi-Fi y la Pi 5.

Tres líneas en paralelo: sistema, Dash y análisis de la sala limpia.

## Un kernel muy seguro (S16)

Pausa del 03/11 por la tarde, tras una auditoría del kernel (`docs/seguridad/`). Hay ocho propiedades que debe cumplir.
Siete cambios de arquitectura esperan decisión: kernel en little-endian, `unsafe` en una sola caja, sacar del kernel el
USB, la GPU y los compositores, DMA de confianza en la Pi 4, kernel no expulsable, cupos y capacidades completas. El
trabajo va en seis fases; la primera arregla los fallos que un programa hostil puede explotar hoy.

## La caja PowerPC (S17)

Programas de AmigaOS 4, MorphOS y WarpOS/PowerUP en la misma caja Amiga, con un segundo motor: el JIT PPC de Emu68
(MPL). Orden recomendado: WarpOS y PowerUP, MorphOS, AltiVec y OS4 al final; OS4 y MorphOS enteros con QEMU
empaquetado. Después de H5; pendiente de decisión (`S17-caja-powerpc.md`).

## Los chips del Amiga (S18)

Los juegos y demos que tocan el chipset también funcionan, con chips imitados por DashOS y la CPU en el JIT de
Emu68. Se empieza por la máquina entera (Kickstart del usuario), exacta a nivel de línea. Los accesos a los chips se
capturan con un fallo de página y se reescribe la instrucción traducida. El núcleo de vAmiga es MPL-2.0 y se puede
aprovechar. Aceptado el 2026-11-08 (`S18-chips.md`).

## Escritorios de terceros (S10b)

Un compositor oficial de confianza y escritorios de cualquiera encima, sin privilegios, como Noctalia o DMS sobre
Niri. Wayland estándar más una extensión propia; kit oficial en Dash. Aceptado el 2026-11-08 (`S10b-escritorios.md`).
