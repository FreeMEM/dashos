# DashOS

[English](README.en.md)

DashOS es un sistema operativo nuevo para ordenadores ARM, empezando por la Raspberry Pi 4. Es moderno por dentro y
Amiga por fuera: kernel propio en Rust, memoria protegida y todos los núcleos de la CPU, con la organización, las
ideas y los programas de AmigaOS.

Los programas 68k de AmigaOS 3.x se ejecutan sin emular un Amiga entero. Un JIT (un traductor que convierte el código
68k en código ARM mientras se ejecuta) corre su código, y sus llamadas al sistema van a bibliotecas nativas. Es la
idea de Petunia en AmigaOS 4 y de Trance en MorphOS. Además, DashOS imitará los chips del Amiga para que también
funcionen los juegos y demos que los tocan directamente.

El proyecto está en una fase temprana. Hay un kernel que arranca y pasa sus pruebas en QEMU, pero todavía no se puede
usar como sistema: no hay escritorio ni caja Amiga. Abajo está el detalle de lo hecho y lo que falta.

## Por qué

El hardware original del Amiga se estropea cada vez más y no hay repuestos. Las placas aceleradoras como PiStorm
alargan su vida, pero dependen de una placa base original: dentro de veinte años no tendrán sentido. Los emuladores
como UAE imitan la máquina entera y dejan al Amiga dentro de una ventana de otro sistema.

DashOS va por otro camino: un sistema completo, en hardware que se puede comprar hoy, donde los programas Amiga son
ciudadanos de primera y conviven con programas nativos nuevos.

## Para quién

- Quien tiene programas, juegos o costumbres de Amiga y quiere seguir usándolos en una máquina actual.
- Quien programa para Amiga: el mismo programa en [Dash](https://github.com/FreeMEM/FreeMEMLang) compilará para
  AmigaOS 3.x (68k) y para DashOS (ARM).
- Quien quiere un sistema pequeño, que se entienda entero y que responda siempre.

## Lo que tendrá

Todo lo de esta lista está decidido en los diseños (`docs/diseño/`). Casi todo está por hacer: el estado real está en
la sección siguiente.

**Programas Amiga**

- Caja Amiga: un proceso donde viven los programas 68k y sus bibliotecas, compartiendo memoria como en un Amiga.
  Si un programa falla, cae la caja, no el sistema.
- `Forbid()` solo para la caja: no para el resto del sistema. Prioridades estrictas de exec dentro.
- Cualquier tarea 68k se puede matar: exec apunta el dueño de cada recurso.
- Enforcer integrado, siempre activo, con los avisos a un registro.
- Las ventanas de los programas Amiga son ventanas del escritorio de DashOS.
- Bibliotecas nativas de exec, dos, graphics, intuition, gadtools... Mientras falten, se usan las de la ROM de
  Kickstart del usuario.
- MUI y ReAction de 68k funcionando dentro de la caja.
- Chips propios (Paula, Blitter, Copper, Denise, Agnus, CIA) para juegos y demos que tocan el hardware, con la CPU en
  el JIT y exactitud a nivel de línea de pantalla. Velocidad de A500 o A1200 a elegir. Con el Kickstart del usuario.
- Caja PowerPC: programas de WarpOS y PowerUP, después MorphOS, y AmigaOS 4 al final.
- `bsdsocket.library` nativa (red en la caja) y, más adelante, `amissl.library` nativa.

**Organización a lo Amiga**

- Volúmenes, cajones, asignaciones (`C:`, `S:`, `LIBS:`, `DEVS:`, `FONTS:`, `LOCALE:`, `ENV:`, `ENVARC:`), Prefs,
  `DEVS:Monitors`, catálogos de idioma, iconos y Workbench.
- Asignaciones por proceso y en capas (usuario, paquetes y sistema). `SYS:` de solo lectura.
- Nombres de fichero que no distinguen mayúsculas, como en el Amiga, guardando cómo se escribieron. Los programas de
  Linux tienen sus carpetas que sí las distinguen.
- Atributos del Amiga en los ficheros: comentario y bits de protección.

**Escritorio**

- Un compositor oficial de confianza, y escritorios de cualquiera encima, sin privilegios: algo parecido a lo que
  hacen Noctalia o DMS sobre Niri en Linux, pero con permisos por programa.
- Ventanas en mosaico y flotantes a la vez: arrastrar una ventana la saca del mosaico o la devuelve.
- Escritorios virtuales como las "pantallas" del Amiga, que se arrastran.
- Menús del programa en la barra, con el botón derecho.
- Aspecto Material 3 con los colores sacados del fondo de pantalla, y un tema "Amiga clásico".
- El escritorio oficial: marco con esquinas redondeadas, barra en cuatro estilos, lanzador con categorías, ajustes
  rápidos, centro de control (música con carátula, calendario, avisos, portapapeles con imágenes), grabación de
  pantalla, selector de fondo y animaciones con desenfoque.
- Personalización completa: barra editable, fondos de vídeo, temas como paquetes, módulos de terceros en Dash.
  Prefs con Usar, Guardar y Cancelar.
- Commodities y ARexx. La pantalla de bloqueo la dibuja el compositor, no el escritorio.

**Un sistema moderno**

- Micronúcleo con capacidades (llaves): un programa nuevo no puede hacer nada salvo lo que le dan.
- Usuarios, grupos y permisos como en Linux, sin superusuario: los administradores confirman cada acción.
- Permisos por aplicación, preguntados en el primer uso.
- Matar un programa siempre funciona.
- Planificador con plazos para el escritorio, la entrada y el sonido.
- Sin intercambio a disco: memoria comprimida.
- Sistema de archivos con copia al escribir, sumas de comprobación, instantáneas automáticas y cajón de cada usuario
  cifrado con su contraseña.
- Sistema base como imagen firmada y actualizaciones A/B que vuelven solas atrás si fallan. Paquetes con la orden
  `Paq`.
- Programas de Linux en una caja con Arch Linux ARM en máquina virtual (DashOS 0.2), con sus ventanas en el
  escritorio.

**Programar**

- API nativa con los conceptos del Amiga, de 64 bits y sobre capacidades, con POSIX al lado.
- Motor de interfaz con el modelo de MUI, y las caras de ReAction y GadTools encima.
- Las aplicaciones y bibliotecas del sistema se escriben en Dash.

**Placas**

- Raspberry Pi 4 primero, la Pi 3 también, y la Pi 5 después.
- Otros ordenadores ARM que arrancan por UEFI: placas, portátiles ARM y, a la larga, Macs con Apple Silicon.
- Candidatas: THEA500 Mini y THEA1200.

Metas medibles (`docs/diseño/S0-principios.md`), aún sin medir: de encender a escritorio en menos de 5 s en una Pi 4,
menos de 128 MB de memoria en reposo, y 60 cuadros por segundo moviendo ventanas con los cuatro núcleos al 100 %.

## Estado

Todo lo hecho se ha probado en QEMU. Nada se ha probado todavía en una placa real.

### Hecho

- **Kernel** en Rust, little-endian, en QEMU `raspi4b`: excepciones, árbol de dispositivos, memoria física, MMU,
  temporizador e interrupciones, los cuatro núcleos, hilos con planificador justo y clase de plazos, procesos
  aislados, llaves y canales, memoria compartida, memoria para DMA, crear, matar y esperar procesos, cupos de memoria
  por grupo. Procesos little-endian o big-endian, cada uno con su orden.
- **Seguridad del kernel**: cuatro fases de endurecimiento, cada una cerrada con una auditoría independiente
  (`docs/seguridad/`), y pruebas hostiles para cada fallo arreglado.
- **Controladores fuera del kernel**, como procesos: consola serie, tarjeta SD, USB (teclado y ratón), buzón de la
  VideoCore y un compositor provisional que dibuja por CPU sobre la pantalla del firmware. Los lanza `inicio`, el
  primer programa.
- **Arranque por UEFI** en QEMU `virt`: con árbol de dispositivos o ACPI, GICv2 o GICv3, pantalla del firmware,
  teclado y ratón por xHCI, disco NVMe e interrupciones MSI-X.
- **JIT 68k**: el JIT de Emu68 llama a funciones nativas desde el código 68k, y corre en CPU little-endian dando la
  vuelta a los bytes (para las CPU sin big-endian, como las de Apple). Probado en un banco de pruebas en Linux.
- **JIT PowerPC**, primera fase: enteros, cargas, FPU y saltos, comparados con `qemu-ppc` en un banco. Los arreglos
  se han mandado a Emu68.
- **Diseño** de todo el sistema (S0 a S18), con las decisiones tomadas: `docs/diseño/RESUMEN.md`.
- **Dash** compila programas para DashOS (AArch64) con sus funciones propias de procesos, canales, llaves y memoria.
  Eso vive en el repositorio de Dash.

### En curso

- **Sistema de archivos DashFS**, a partir de RedoxFS: el servicio lee y escribe en el disco NVMe desde DashOS, con
  pruebas de cortes de luz. Hecho: carpetas que no distinguen mayúsculas, el comentario y los bits de protección del
  Amiga, una herramienta que comprueba y repara el disco, e instantáneas (DashFS 4: tomar, borrar y volver a una).
  Falta: el cajón cifrado de cada usuario y el cuidado de la SD.

### Falta

En el orden de la hoja de ruta (`docs/diseño/S15-hoja-de-ruta.md`):

| Hito | Qué |
|---|---|
| H3 | Compositor que habla Wayland (hoy es provisional) |
| H4 | La caja Amiga: exec y dos.library nativas, comparadas con un Amiga real |
| H5 | La caja con ventanas: graphics, layers, intuition, gadtools; MUI y ReAction de 68k |
| H6 | La Pi 4 real: arranque, red, HDMI, sonido, USB por PCIe |
| H7 | Dash nativo, motor de interfaz y escritorio; usuarios, permisos y cifrado |
| H8 | Paquetes y actualizaciones seguras: **DashOS 0.1** |
| H9 | La caja Linux: **DashOS 0.2** |

Después: GPU con Mesa, Wi-Fi y Bluetooth, la Pi 5, los chips del Amiga (S18) y la caja PowerPC (S17), que pueden
adelantarse si hay manos.

## Probar lo que hay

Hace falta:

- Rust nightly (con `rustup`, que lee los `rust-toolchain.toml`).
- El compilador cruzado `aarch64be` de [Bootlin](https://toolchains.bootlin.com/) 2026.08-1 en
  `~/.local/opt/toolchains/` (o la variable `BOOTLIN` apuntando a su `bin`). Ver `toolchains/LEEME.md`.
- `qemu-system-aarch64`, y EDK2 para AArch64 en `/usr/share/edk2/aarch64/QEMU_EFI.fd` (o la variable `EDK2`).
- Python 3.

```sh
make -C kernel prueba          # pruebas del kernel en QEMU raspi4b
make -C kernel arranque        # arranque normal en QEMU raspi4b, hasta "DashOS en marcha"
make -C kernel prueba-virt     # pruebas del kernel en QEMU virt, arrancando por UEFI
make -C kernel arranque-virt   # arranque por UEFI con pantalla, xHCI y NVMe
```

Los bancos del JIT necesitan Emu68 en `third_party/Emu68` con los parches de `patches/emu68/` (cómo, en
`toolchains/LEEME.md` y `docs/PPC_LOOP.md`), y `qemu-aarch64_be`:

```sh
make -C jit-host run           # programa 68k en el JIT, en big-endian
make -C jit-host ORDEN=le run  # lo mismo con la CPU en little-endian (necesita qemu-aarch64)
```

## Qué hay en el repositorio

| Carpeta | Qué |
|---|---|
| `kernel/` | El kernel (Rust) y sus pruebas |
| `usuario/` | Biblioteca de los programas y los servicios: `inicio`, consola, USB, SD, NVMe, xHCI, compositor... |
| `uefi/` | El programa de UEFI que carga el kernel |
| `jit-host/` | Bancos de pruebas de los JIT de Emu68 (68k y PowerPC) como programas de Linux |
| `patches/emu68/` | Nuestros cambios a Emu68 |
| `toolchains/` | Ficheros de CMake y envoltorios para los compiladores de Bootlin |
| `tools/` | Guiones para arrancar y comprobar en QEMU |
| `docs/diseño/` | El diseño, de S0 a S18, y su resumen |
| `docs/referencia/` | Llamadas del kernel y protocolos de los servicios |
| `docs/spec/` | Especificaciones de hardware escritas en sala limpia |
| `docs/seguridad/` | Auditorías del kernel |
| `docs/*_LOOP.md` | Bitácoras del trabajo |
| `PLAN.md` | El plan y todas las decisiones tomadas |

## Cómo se trabaja

- **Sala limpia.** Lo que no está documentado lo estudia una persona (o sesión) que escribe una especificación en
  prosa; otra escribe el código solo con esa especificación. No se copia código de la ROM del Amiga ni de proyectos
  con licencias incompatibles. Las reglas están en `AGENTS.md`.
- **Un oráculo.** El mismo programa 68k se ejecuta en un Amiga emulado con el Kickstart real y en DashOS, y las
  salidas se comparan.
- **Cada cambio con su prueba**, y las de seguridad con un programa hostil que intenta romper el kernel.

## Lo que nunca va al repositorio

ROMs de Kickstart, imágenes de AmigaOS, AmigaOS 4 o MorphOS, ni ningún otro software comercial. DashOS usará la ROM
que cada usuario tenga legalmente; no la distribuye.

## Licencia

El código de DashOS es MPL-2.0 (`LICENSE`), la misma licencia que Emu68. Los cambios a ficheros de Emu68 siguen
siendo MPL-2.0. RedoxFS, del que parte el sistema de archivos, es MIT.
