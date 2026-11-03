# S14. Arranque, servicios y actualizaciones

## El problema

Hay que llevar la Pi 4 del encendido al escritorio en el menor tiempo posible (meta de S0: del kernel al escritorio
en menos de 1 segundo), arrancar los servicios en orden, y aplicar actualizaciones sin riesgo de dejar la placa sin
arrancar (principio 3 de S0).

Lo que ya está decidido o recomendado:

- El firmware de la Pi gasta unos 3,5 segundos antes de cargar el kernel (S0).
- El firmware solo lee FAT, así que el kernel vive en una partición FAT32 (S5).
- `SYS:` es una imagen firmada, en una instantánea de solo lectura. Una actualización escribe otra al lado (S5, S8).
- El kernel es un micronúcleo: todo lo demás son procesos que alguien tiene que arrancar y a los que tiene que dar
  sus capacidades (S1).
- Hay un vigilante (watchdog) en la Pi 4 (S12).

## Lo que se comprobó

- **QEMU entra en EL2 y deja el árbol de dispositivos en la memoria**, como el firmware real. La prueba de Rust de
  S1 arranca así.
- **El firmware de la Pi 4 tiene un mecanismo para actualizaciones A/B**, documentado por Raspberry Pi:
  - `autoboot.txt`, en la primera partición FAT, dice qué partición se arranca. Tiene una sección `[all]` (la de
    siempre) y otra `[tryboot]` (la de prueba).
  - Con `tryboot_a_b=1`, el cambio se hace por partición entera.
  - Se reinicia con `reboot "0 tryboot"`: el siguiente arranque, y solo ese, usa la partición de prueba.
  - Si el sistema nuevo funciona, se reescribe `autoboot.txt` para que sea la de siempre. Es un sector, así que la
    escritura es atómica.
  - Si falla, el indicador de prueba se borra solo en el siguiente reinicio, y se vuelve a la buena.
  - Ubuntu añade el vigilante: si el kernel nuevo no arranca, el vigilante reinicia la placa y vuelve a la buena.
  - Las EEPROM desde la versión del 10 de marzo de 2025 tienen además la condición `[boot_partition=N]` en
    `config.txt`.

## Recomendación

### La tarjeta

| Partición | Formato | Qué lleva |
|---|---|---|
| 1. Arranque | FAT32, pequeña | `autoboot.txt` y el firmware de la Pi |
| 2. Ranura A | FAT32 | `config.txt`, el kernel y la imagen de arranque, versión A |
| 3. Ranura B | FAT32 | Lo mismo, versión B |
| 4. Sistema | El de S5 | `SYS:`, `Paquetes:`, `Usuarios:`, con sus instantáneas |

Las ranuras A y B son pequeñas: solo el kernel y lo mínimo para montar el sistema. Lo grande vive en la partición 4,
donde las instantáneas hacen el trabajo.

### Del encendido al escritorio

1. **Firmware de la Pi**: lee `autoboot.txt`, elige la ranura, carga el kernel y la imagen de arranque (con
   `initramfs ... followkernel`) y salta al kernel en EL2 con el árbol de dispositivos.
2. **Kernel**: pone el big-endian, prepara EL2 para el hipervisor (S7), baja a EL1, enciende la MMU y los núcleos y
   lanza el primer proceso. Meta: menos de 100 ms.
3. **El primer proceso, el gestor de arranque**: recibe todas las capacidades y las reparte (S1, S6). Lee la imagen
   de arranque, que trae lo mínimo: el controlador de la SD, el sistema de archivos y el gestor de servicios.
4. **Monta el sistema**: comprueba la firma de la instantánea de `SYS:` que dice la ranura (S6), la monta y arma el
   espacio de nombres del sistema (S4b).
5. **Gestor de servicios**: arranca el resto en paralelo, según sus dependencias.
6. **Escritorio**: el compositor y la pantalla de entrada salen en cuanto hay pantalla y entrada. La red y lo demás
   siguen llegando por detrás.

### El gestor de servicios

- **Declarativo**: cada servicio es un fichero corto con su programa, su usuario (S6), sus capacidades, sus
  dependencias y su clase de planificador (S2). Viven en `SYS:Servicios` y en `Paquetes:*/Servicios`.
- **En paralelo**, como launchd o systemd. Lo que no hace falta para el escritorio no lo retrasa.
- **Bajo demanda**: la caja Linux, la impresión y similares no arrancan hasta que alguien los usa (S7).
- **Vigilancia**: si un servicio se cae, se reinicia, con espera creciente. Si se cae muchas veces seguidas, se
  avisa en el escritorio.
- **Registro**: lo que dicen los servicios va a un registro en memoria, que se guarda en disco con calma.

### Lo que se conserva del Amiga

- **`S:Startup-Sequence`** existe, pero para la caja Amiga: la caja lo ejecuta al arrancar, con sus `Assign` y sus
  programas 68k, como en un Amiga (S4).
- **`WBStartup`**: un cajón del usuario con lo que se lanza al entrar en la sesión, como en el Workbench.
- **El menú de arranque temprano.** En el Amiga, con los dos botones del ratón pulsados al encender, salía el Early
  Startup Control. DashOS tiene el suyo, con una tecla al arrancar:
  - Arrancar con la instantánea anterior de `SYS:` o de `Paquetes:`.
  - Modo seguro: sin paquetes de terceros ni `WBStartup`.
  - Una consola de emergencia.

### Actualizaciones

**Del kernel y la imagen de arranque**, con el A/B del firmware:

1. Se escribe la versión nueva en la ranura que no está en uso.
2. Se reinicia con `tryboot`.
3. El sistema nuevo arranca. Si en un tiempo fijo llega a estar sano (escritorio y servicios básicos en marcha), se
   cambia `autoboot.txt` y queda como la de siempre.
4. Si no llega, el vigilante reinicia y el firmware vuelve solo a la ranura buena.

**De `SYS:`**, con instantáneas:

1. Se baja la imagen nueva por trozos (S8) y se escribe como instantánea nueva.
2. La ranura de prueba apunta a esa instantánea. Así el kernel y `SYS:` se prueban juntos, con el mismo `tryboot`.
3. Si todo va bien, la instantánea vieja se guarda un tiempo, para poder volver desde el menú de arranque.

**De los paquetes**, sin reiniciar (S8).

### Apagar

Por orden inverso a las dependencias, con tiempo para que cada servicio guarde. El sistema de archivos escribe lo
pendiente y se cierra limpio. Si algo no para, se le mata (S2).

## Lo que queda abierto

1. ¿Tarjeta con A/B para el kernel y la imagen de arranque, y `SYS:` por instantáneas, probados juntos con
   `tryboot`? Recomendado: sí.
2. ¿Menú de arranque temprano con una tecla, al estilo del Early Startup Control del Amiga? Recomendado: sí. ¿Qué
   tecla? Propuesta: mantener pulsada la barra espaciadora, o los dos botones del ratón como en el Amiga.
3. ¿Entrada automática para un único usuario, o pantalla de entrada siempre? Recomendado: pantalla de entrada, porque
   el cajón del usuario va cifrado con su contraseña (S5).

## Cómo encaja

- Con S0: la meta de menos de 1 segundo depende de arrancar en paralelo y de sacar el escritorio pronto.
- Con S1: el gestor de arranque reparte las capacidades.
- Con S2: cada servicio declara su clase; matar al apagar.
- Con S4 y S4b: `S:Startup-Sequence` para la caja, `WBStartup` para la sesión, espacio de nombres del sistema.
- Con S5: `SYS:` y `Paquetes:` por instantáneas; desmontaje limpio.
- Con S6: firma de `SYS:` al arrancar; cada servicio con su usuario.
- Con S7: EL2 para el hipervisor; la caja Linux bajo demanda.
- Con S8: la imagen de `SYS:` y los paquetes.
- Con S12: vigilante, y los controladores mínimos en la imagen de arranque.

## Fuentes

- `autoboot.txt` y `tryboot`: [documentación de config.txt](https://raspberrypi.com/documentation/computers/config_txt.html),
  [cambio en la documentación](https://github.com/raspberrypi/documentation/pull/2701/files),
  [A/B en Ubuntu para la Pi](https://ubuntu.com/hardware/docs/boards/explanations/piboot-ab/),
  [RAUC en la Pi 5, de Bootlin](https://bootlin.com/?p=19769) y
  [Thistle](https://docs.thistle.tech/update/bootloaders/integration).
- `initramfs ... followkernel` y `arm_64bit`: [guía de Ubuntu](https://ubuntu.com/hardware/docs/boards/how-to/special_hardware/rpi-config-txt/).
- Entrada al kernel con el árbol en `x0`: [TF-A en la Pi 4](https://coral.googlesource.com/tf-a-mtk/+/refs/heads/master/docs/plat/rpi4.rst).

Lo que no se comprobó: `tryboot` en una Pi 4 real, y si el tiempo de 1 segundo del kernel al escritorio es posible.
