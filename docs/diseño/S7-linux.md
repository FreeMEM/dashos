# S7. Software de Linux

> **Nota del 2026-11-03 por la tarde.** El usuario decidió que lo nativo sea little-endian y solo la caja Amiga
> big-endian. Donde este documento dice big-endian para lo nativo, manda `PLAN.md`.

## El problema

El encargo pide que sea sencillo aprovechar software de Linux. Hay miles de programas que nadie va a reescribir para
DashOS: navegadores, ofimática, editores, herramientas de desarrollo.

Tres hechos marcan el diseño:

- **Los programas de Linux para ARM de 64 bits son little-endian**, y DashOS es big-endian.
- **Linux no es solo llamadas al sistema.** Un programa de escritorio espera también `/proc`, `/sys`, `/dev`, D-Bus,
  Wayland o X11, fuentes, temas y cientos de bibliotecas.
- **El principio 6 de S0**: Linux es un invitado, no manda en el diseño.

## Lo que se comprobó

- **La CPU admite procesos de usuario con su propio orden de bytes.** La prueba de S1 se amplió: el bit
  `SCTLR_EL1.E0E` se puede poner a 1 y se lee a 1 en QEMU `raspi4b`. Un kernel big-endian puede ejecutar programas
  de usuario little-endian. No se ha ejecutado aún un programa entero así.
- **QEMU y el firmware de la Pi 4 entran en EL2**, el nivel del hipervisor (S1). La Cortex-A72 tiene virtualización.
  Es ARMv8.0, sin VHE, así que un hipervisor necesita un trozo pequeño en EL2 y el resto en EL1, como KVM en las CPUs
  de esa época.
- **La lección de Microsoft.** WSL1 traducía llamadas de Linux a llamadas de Windows. Siempre faltaba alguna y algún
  programa fallaba, así que WSL2 pasó a ser una máquina virtual con un kernel de Linux de verdad. A cambio se perdió
  algo de integración con los ficheros.

## Las opciones

### A. Una capa de llamadas al sistema de Linux

Como WSL1, el Linuxulator de FreeBSD o Asterinas. Los programas de Linux corren directamente sobre el kernel de
DashOS, como procesos de usuario little-endian. Una capa traduce sus llamadas al sistema.

- Gana: es la mejor integración. Cada programa es un proceso más, con sus permisos (S6), sus ventanas y sus ficheros
  de verdad. No gasta memoria en un segundo kernel.
- Cuesta: hay que escribir cientos de llamadas al sistema. Asterinas lleva más de 200 y aún no ejecuta una
  distribución entera. Cada estructura que pasa por memoria (`stat`, `sockaddr`, `iovec`...) hay que darle la vuelta
  de little a big-endian. Y siempre quedará algo sin hacer, como le pasó a WSL1.

### B. Una máquina virtual con Linux: la "caja Linux"

Como ChromeOS con Crostini o Windows con WSL2. Un kernel de Linux de verdad corre en una máquina virtual. Las ventanas
de sus programas salen en el escritorio de DashOS.

- Gana: compatible con todo, porque es Linux de verdad. El orden de bytes da igual: el invitado pone su CPU en
  little-endian por su cuenta. Solo los dispositivos virtio, que son little-endian por norma, necesitan dar la vuelta
  a los bytes en el lado de DashOS.
- Cuesta: memoria. Un Linux mínimo con sus servicios gasta cientos de megas, mucho para una Pi 4 de 1 o 2 GB. Hay que
  escribir el hipervisor y los dispositivos virtio. Sin GPU en la máquina virtual: los programas dibujan por CPU.

### C. Portar programas a DashOS

Con una biblioteca de C POSIX nativa, se recompilan los programas para DashOS.

- Gana: programas nativos de verdad, en big-endian, con todo integrado.
- Cuesta: programa a programa. Los grandes, como Firefox, son proyectos enormes. Sirve para herramientas y
  bibliotecas, no para "todo Linux".

## Recomendación

**Las tres, en este orden, cada una para lo suyo.**

### Primero C: una biblioteca POSIX nativa

DashOS la necesita de todas formas. Muchas bibliotecas que usarán los propios servicios y aplicaciones están en C:
zlib, libpng, FreeType, HarfBuzz, SQLite, Lua.

- **Partir de relibc**, la biblioteca C de Redox. Está escrita en Rust, es casi POSIX, y está pensada para portar
  programas de C, C++ y Rust a un sistema que no es Linux. Licencia MIT.
- Con ella se portan las herramientas de línea de órdenes que DashOS quiera tener de serie.

### Después B: la caja Linux para lo grande

Para navegadores, ofimática y todo lo demás, una máquina virtual. Es el camino que eligieron Google y Microsoft
después de probar el otro.

- **Hipervisor en el kernel de DashOS**: un trozo pequeño en EL2 y la gestión en un proceso, como crosvm en ChromeOS.
- **Dispositivos virtio**: disco, red, consola, `vsock` para hablar con DashOS, `virtio-fs` para compartir ficheros y
  `virtio-gpu` en modo de dibujo por CPU (hecho en 2D; la GPU de verdad para el invitado, en S25: la V3D mediada en
  la Pi 4).
- **Ventanas**: dentro de la máquina virtual corre un compositor intermediario, como Sommelier en ChromeOS. Reenvía
  cada ventana de Wayland al compositor de DashOS, que la muestra como una ventana más, con mosaico y escritorios
  virtuales (S10). Para programas de X11, XWayland dentro de la caja.
- **Ficheros**: `Home:` se comparte con la caja Linux, con los permisos del usuario (S6). Dentro se ve como
  `/home/<usuario>` (S4b).
- **Integración**: los programas instalados en la caja Linux aparecen en el lanzador de DashOS, con su icono. El
  portapapeles es común.
- **Distribución**: una que ya exista, con su propio gestor de paquetes dentro (ver la pregunta 2).
- **Memoria**: la máquina virtual pide memoria al arrancar y la devuelve cuando no la usa (globo virtio). Se
  recomienda para Pi 4 de 4 y 8 GB. En las de 1 y 2 GB es posible, pero justo.
- **Arranque bajo demanda**: la caja Linux no arranca con el sistema, sino al abrir el primer programa de Linux.
  Para servidores y contenedores, se activa como servicio y arranca con el sistema, con memoria reservada, apagado
  ordenado y puertos publicados (S14, "La caja Linux como servicio", decidido el 2026-11-10).

### Más adelante, quizá A

Cuando la caja Linux funcione, se puede estudiar ejecutar algunos programas sin máquina virtual, con la capa de
llamadas y procesos little-endian. Serviría para herramientas de línea de órdenes, que usan pocas llamadas. Asterinas,
con licencia MPL-2.0 como DashOS, es la referencia para escribirla en Rust. No es necesaria para la primera versión.

## Lo que hace falta en otras partes

- **El compositor de DashOS tiene que hablar Wayland**, o tener un servidor Wayland aparte que traduzca a su
  protocolo propio, como Exo en ChromeOS. Lo decide S9.
- **El hipervisor** es trabajo del kernel (S1) y de los controladores virtio (S12).
- **Los permisos** de la caja Linux siguen S6: la caja entera es una aplicación, como la caja Amiga.

## Lo que trae la caja de serie (2026-11-10)

Decidido por el usuario:

- **Arch Linux ARM**, con pacman para la línea de órdenes, las bibliotecas y los servidores.
- **Flatpak de serie**, con Flathub, para las aplicaciones de escritorio: tienen versión para ARM, no dependen de la
  distribución y van encerradas con permisos. Lo que una aplicación de Flatpak pide (red, ficheros, cámara) se enlaza
  con los permisos de DashOS (S6b): la caja Linux es una aplicación, y lo que pase de sus permisos lo pregunta DashOS.
- **Docker y Podman** se instalan con pacman: dentro hay un kernel de Linux de verdad, con namespaces y cgroups.
- **La terminal como la del usuario**: zsh con Oh My Zsh, el tema `agnosterzak`, `zsh-autosuggestions` y
  `zsh-syntax-highlighting`, `lsd` en vez de `ls` y `fastfetch` al abrir, con una fuente con los símbolos que usa el
  tema (una Nerd Font). Terminator como terminal, con sus ventanas en el escritorio de DashOS por Wayland. Es la
  configuración por defecto de la caja; cada usuario la cambia en su `~/.zshrc`.
- Lo instalado en la caja aparece en el lanzador de DashOS y en el escaparate de Hangar (S8).

## Lo que queda abierto

1. ¿Este orden: biblioteca POSIX nativa, después caja Linux en máquina virtual, y la capa de llamadas solo si hace
   falta? Recomendado: sí.
2. ¿Qué distribución dentro de la caja Linux? **Decidido por el usuario (2026-11-10): Arch Linux ARM**, con pacman,
   como sus máquinas con CachyOS.
3. ¿La caja Linux solo en Pi 4 de 4 y 8 GB, o también en las de 2 GB con avisos? Recomendado: de 4 GB en adelante.

## Cómo encaja

- Con S0: principio 6; Linux es un invitado.
- Con S1: el hipervisor es una pieza del kernel. Asterinas es la referencia para la opción A.
- Con S3: memoria de la máquina virtual con globo. Para la opción A, la capa podría prometer de más dentro de un
  límite.
- Con S4b: `/home` es `Home:`, y los volúmenes salen en `/Volumes`.
- Con S6: permisos tipo Flatpak, la caja entera como una aplicación.
- S8: los paquetes de Linux los gestiona la distribución de dentro; Hangar puede enseñarlos en su catálogo.
- S9: el compositor tiene que aceptar ventanas de Wayland.
- S12: controladores virtio en el lado de DashOS.

## Fuentes

- Crostini, Sommelier y crosvm: [contenedores y máquinas virtuales en ChromeOS](https://chromium.org/chromium-os/developer-library/guides/containers/containers-and-vms),
  [documentación de Chromium OS](https://chromium.googlesource.com/chromiumos/docs/+/master/containers_and_vms.md),
  [resumen de OSnews](https://www.osnews.com/story/30294/linux-apps-on-chrome-os-an-overview/) y
  [charla sobre la arquitectura de Crostini](https://webapp.prod.talks.gcp.uis.cam.ac.uk/talk/index/125134/).
- WSL1 frente a WSL2: [IT Writing](https://itwriting.com/blog/?p=11543),
  [IONOS](https://www.ionos.com/digitalguide/server/know-how/wsl2-explained.md) y
  [DEV Community](https://dev.to/akionsight/the-underlying-architecture-of-wsl-and-why-is-wsl-2-better-ba0).
- relibc: [repositorio](https://github.com/redox-os/relibc) y [objetivos de Redox](https://doc.redox-os.org/book/our-goals.html).
- Asterinas: ver las fuentes de S1.
- Prueba de `E0E`: `docs/diseño/pruebas/rust-be/LEEME.md`.

Lo que no se comprobó: ningún programa little-endian ejecutado de verdad en EL0, ni una máquina virtual dentro de
DashOS. El gasto de memoria de una caja Linux es una estimación.
