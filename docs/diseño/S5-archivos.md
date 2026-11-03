# S5. Sistema de archivos

## El problema

El encargo pide un sistema de archivos rápido y seguro. Además, las tareas anteriores le piden cosas concretas:

- **Instantáneas**, para las actualizaciones atómicas y para volver atrás (principio 3 de S0; S8 y S14).
- **Sumas de comprobación**, para detectar datos estropeados en vez de leerlos sin saberlo.
- **Dueño, grupo y permisos** como en Linux (decisión del usuario).
- **Sin distinguir mayúsculas, pero guardándolas tal como se escriben**, como el Amiga (S4b, recomendado).
- **Atributos extra**: el comentario de fichero y los bits de protección del Amiga (`S`, `P`, `A`), que la caja
  necesita (S4).
- **Cifrado** del cajón de cada usuario.
- **Compresión**, para ahorrar espacio y escrituras.
- **Cuidar la tarjeta SD**, que es el disco habitual de una Pi 4 y se gasta con las escrituras.

En un micronúcleo, el sistema de archivos es un proceso (S1). Si se cuelga, se reinicia sin tirar el sistema.

## Lo que se comprobó

- **La Pi 4 no tiene instrucciones de cifrado AES.** Broadcom no licenció las extensiones de criptografía de ARMv8
  para el BCM2711. AES por software es lento en esta CPU. Hay que elegir un cifrado pensado para CPUs sin AES, como
  Adiantum, que Google diseñó para móviles baratos y que usa ChaCha. La Pi 5 sí tiene AES.
- **RedoxFS existe y encaja.** Es el sistema de archivos de Redox: escrito en Rust, pensado para un micronúcleo,
  inspirado en ZFS, con copia al escribir, sumas de comprobación de datos y metadatos, cifrado y permisos Unix.
  Licencia MIT. Funciona también en Linux con FUSE.
- **Para leer ext4 hay una biblioteca de Rust**, `ext4-view`: solo lectura, sin `unsafe`, `no_std` con `alloc`.

Lo que no se comprobó: si RedoxFS guarda sus estructuras en el disco con un orden de bytes fijo, o con el de la CPU.
En DashOS, big-endian, eso decide si se puede leer el mismo disco en un PC. Tampoco se comprobó si tiene
instantáneas y compresión.

## Las opciones

### A. Adaptar RedoxFS

- Gana: ya hace la mitad de lo que se pide, en Rust, para un micronúcleo, con licencia MIT. Hay años de trabajo
  dentro.
- Cuesta: hay que añadir lo que falte (instantáneas, compresión, mayúsculas, atributos del Amiga) y revisar el orden
  de bytes. Su formato evoluciona con Redox; habría que separarse y mantener una rama propia.

### B. Diseñar uno propio

Copia al escribir con escritura en registro, pensado para memoria flash, como F2FS.

- Gana: hecho a la medida de DashOS y de la SD.
- Cuesta: un sistema de archivos fiable lleva años. Los fallos aquí pierden datos de la gente.

### C. Portar uno grande: ZFS, btrfs o bcachefs

- Gana: maduros y con todo lo que se pide.
- Cuesta: son de C, enormes y muy atados a Linux o a Solaris. Sus licencias (CDDL, GPL) no casan con MPL-2.0.
  Redox ya probó un ZFS de solo lectura y lo quitó, porque su diseño monolítico chocaba con el micronúcleo.

### D. ext4 o un sistema sencillo

- Gana: conocido y rápido de tener.
- Cuesta: sin instantáneas ni sumas de los datos. No cumple lo que se pide. Y su diario reescribe siempre las mismas
  zonas, lo peor para una SD.

## Recomendación

**A: partir de RedoxFS, con una rama propia para DashOS.** Si al revisarlo resulta que su formato no se puede
llevar a big-endian sin rehacerlo, se pasa a B usando RedoxFS como guía.

### Lo que hay que añadirle

- **Instantáneas** de un volumen o de un cajón. Con copia al escribir son baratas: se guarda la raíz vieja.
- **Compresión** por bloques, con LZ4 por defecto (rápida) y zstd a elegir (comprime más).
- **Búsqueda sin distinguir mayúsculas**, guardando el nombre tal como se escribió. Un volumen se puede crear
  distinguiendo mayúsculas, si se pide (S4b).
- **Atributos extra**: el comentario del Amiga, sus bits de protección y lo que pida la capa de Linux.
- **Orden de bytes fijo en el disco**, little-endian, como casi todos los formatos. Así un disco de DashOS se puede
  abrir en un PC con la versión de FUSE. El coste de dar la vuelta a los bytes al leer es pequeño frente al disco.

### Cómo cuidar la SD

- La copia al escribir nunca reescribe un bloque en su sitio. Es mejor para la flash que el diario de ext4.
- Las escrituras se agrupan y se mandan juntas cada pocos segundos, salvo que un programa pida `fsync`.
- Se avisa a la tarjeta de los bloques libres (borrado o TRIM), para que su controlador reparta el desgaste.
- Los bloques se alinean con el tamaño de borrado de la tarjeta, que suele ser de 4 MB.
- `T:` y `ENV:` van en `RAM:` (S4b), así que lo que más se reescribe no toca la tarjeta.
- Recomendación de uso: para mucho trabajo, un SSD por USB. La Pi 4 también arranca de USB.

### Cifrado

- **El cajón de cada usuario, cifrado con su contraseña.** La clave se abre al entrar en la sesión.
- **Adiantum (ChaCha) en la Pi 4**, porque no tiene AES. En la Pi 5, AES con su hardware.
- Cifrar el disco entero es una opción aparte, no la de serie.

### Otros sistemas de archivos

Cada uno es un proceso aparte, y se monta desde `DEVS:DOSDrivers` (S4b).

| Sistema | Para qué | Cómo |
|---|---|---|
| FAT32 | La partición de arranque: el firmware de la Pi solo lee FAT | Lectura y escritura |
| exFAT | Memorias USB y tarjetas de cámara | Lectura y escritura |
| FFS, SFS y PFS3 del Amiga | Discos y ficheros HDF de Amiga | Lectura y escritura, desde especificaciones públicas |
| ext4 | Intercambiar discos con Linux | Lectura, con `ext4-view`; escritura más adelante |
| ISO 9660 | Imágenes de CD | Lectura |

### Volúmenes y actualizaciones

- **`SYS:` es una instantánea de solo lectura.** Una actualización escribe una instantánea nueva al lado y, al
  reiniciar, el arranque usa la nueva. Si falla, vuelve a la anterior (S14).
- **`Paquetes:`** igual: cada transacción del gestor de paquetes es una instantánea (S8).
- **`Usuarios:`** con instantáneas periódicas a elegir, como "Time Machine" en macOS.

## Lo que queda abierto

1. ¿Partir de RedoxFS (recomendado) o diseñar uno propio desde el principio?
2. ¿Cifrar el cajón de cada usuario de serie, con su contraseña? Recomendado: sí.
3. ¿Instantáneas periódicas automáticas del cajón del usuario? Recomendado: sí, cada hora, guardando las de los
   últimos días.

## Cómo encaja

- Con S0: principio 3 (todo o nada) gracias a las instantáneas.
- Con S1: el sistema de archivos es un proceso; si se cae, se reinicia.
- Con S3: sirve los ficheros proyectados en memoria.
- Con S4 y S4b: guarda los atributos del Amiga, ignora mayúsculas y sirve los volúmenes y asignaciones.
- S6 usa dueño, grupo y permisos.
- S8 y S14 usan las instantáneas para instalar y actualizar.
- S12 necesita el controlador de la tarjeta SD con borrado (TRIM) y el de USB.

## Fuentes

- RedoxFS: [libro de Redox](https://doc.redox-os.org/book/redoxfs.html),
  [README](https://gitlab.redox-os.org/ashton/redoxfs/-/blame/50b4ebfcd55166bb2c9ec4da4626b2a6560c3f20/README.md) y
  [docs.rs](https://docs.rs/crate/redoxfs/^0.5.0).
- `ext4-view`: [lib.rs](https://lib.rs/crates/ext4-view) y [docs.rs](https://docs.rs/crate/ext4-view/latest).
- Sin AES en la Pi 4: [foro de OpenWrt](https://forum.openwrt.org/t/raspberry-pi-4-released/39444?page=2),
  [parche de Yocto](https://patchwork.yoctoproject.org/comment/233) y
  [CNX Software](https://www.cnx-software.com/2020/01/29/raspberry-pi-4-benchmarked-with-32-bit-and-64-bit-debian-os/).
- Sistemas de archivos y desgaste de la SD (foros, sin medidas rigurosas):
  [FlightAware](https://discussions.flightaware.com/t/raspberrypi-sd-cards-wear-and-f2fs-filesytem-experience/29508),
  [lista de Devuan](https://lists.dyne.org/lurker/message/20191108.192830.73cbf0b1.da.html) y
  [Home Assistant](https://community.home-assistant.io/t/home-assistant-os-release-8/422300?page=7).

Las cifras de desgaste que circulan en foros no tienen método detrás, y no se usan aquí.
