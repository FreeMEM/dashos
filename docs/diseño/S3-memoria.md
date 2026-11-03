# S3. Memoria

## El problema

La memoria tiene que cumplir cuatro cosas a la vez:

- **Proteger**: un proceso no toca la memoria de otro (principio 2 de S0).
- **Compartir sin copiar**: la pantalla, el disco, la red y el audio mueven mucho dato entre procesos. En un
  micronúcleo (S1) eso va por memoria compartida.
- **Hacer sitio a la caja Amiga**: dentro de la caja, los programas 68k ven la memoria como un Amiga, con
  direcciones de 32 bits.
- **Caber en poca RAM**: la Pi 4 tiene modelos de 1, 2, 4 y 8 GB. La meta de S0 es menos de 128 MB en reposo.

## Lo que se comprobó

La prueba de S1 leyó el registro `ID_AA64MMFR0_EL1` en QEMU `raspi4b`: vale `0x1124`. Es el valor que da el manual
de la Cortex-A72. De él sale:

- Direcciones físicas de 44 bits.
- Identificadores de espacio de direcciones (ASID) de 16 bits. El kernel puede cambiar de proceso sin vaciar la
  caché de traducciones (TLB): hasta 65.536 procesos tienen la suya marcada.
- Páginas de 4 KB y de 64 KB. Las de 16 KB no existen en esta CPU.

El árbol de dispositivos de la Pi 4 (`bcm2711-rpi-4-b.dtb`, el que trae el firmware) dice qué memoria ve cada
periférico para el acceso directo (DMA):

| Bus | Qué cuelga de él | Memoria que alcanza |
|---|---|---|
| `soc` | periféricos antiguos y el motor DMA clásico | el primer GB |
| `emmc2bus` | la tarjeta SD | el primer GB |
| `pcie@7d500000` | el USB (chip VL805) | los primeros 3 GB |
| `scb` | red GENET y otros | toda |
| `v3dbus` | la GPU V3D | toda |

Es el fichero tal cual. El firmware puede cambiar estos valores al arrancar según la revisión del chip: las placas
con revisión C0 o posterior (las Pi 4 recientes, la CM4 y la Pi 400) alcanzan más. Hay que leerlos del árbol que
entrega el firmware, no fijarlos en el código.

También sale de la prueba que la A72 es ARMv8.0. No tiene PAN (que impide al kernel leer memoria de usuario por
error), ni PAC, ni BTI, ni MTE. Esas defensas por hardware llegan con CPUs posteriores, como la de la Pi 5. En la
Pi 4, la defensa la tiene que dar el diseño y Rust.

## Las opciones

### Cómo se reparte la memoria entre procesos

- **A. Un espacio por proceso, como Linux.** Cada proceso tiene sus tablas de páginas. Compartir es posible, pero
  como añadido.
- **B. Un solo espacio para todos.** Descartado en S1: no protege el código 68k, C ni Linux.
- **C. Un espacio por proceso, con la memoria como objeto que se pasa, como Fuchsia.** La memoria es un objeto del
  kernel (en Fuchsia, VMO) con su capacidad. Se puede proyectar en varios procesos, con derechos distintos en cada
  uno, clonar con copia al escribir, o respaldar con un fichero que sirve otro proceso.

### Qué hacer cuando se acaba la RAM

- **Intercambio a disco, como Linux tradicional.** En una tarjeta SD es lento y la desgasta.
- **Memoria comprimida, como zram en Linux o el compresor de macOS.** Las páginas que no se usan se comprimen en la
  propia RAM. Una prueba de la comunidad de Fedora (no en una Pi) da una proporción de 2,6 a 1 con LZ4 y de 3,4 a 1
  con zstd. LZ4 es bastante más rápido.
- **Matar procesos cuando se acaba, como el OOM killer de Linux.** Linux promete más memoria de la que tiene, y
  cuando se le acaba mata un proceso casi al azar.
- **No prometer de más.** Se lleva la cuenta de lo comprometido y, si no hay, la petición falla en el momento.
  Windows lo hace así. Los programas Amiga ya están acostumbrados: `AllocMem` devuelve 0 y lo comprueban.

## Recomendación

**Opción C, objetos de memoria al estilo Fuchsia, con páginas de 4 KB, memoria comprimida con LZ4 y sin prometer de
más.**

### Espacios de direcciones

- La mitad alta es del kernel (`TTBR1`). La mitad baja es de cada proceso (`TTBR0`), con su ASID.
- Páginas de 4 KB. Los programas de Linux para ARM dan por hecho 4 KB casi siempre (S7). Para memoria grande y
  contigua, como la pantalla, se usa el bit de contigüidad de las tablas, que agrupa 16 páginas en una entrada de TLB.
- Direcciones aleatorias (ASLR) en los procesos nativos.
- Nada es a la vez escribible y ejecutable. El JIT lo cumple: escribe en una dirección y ejecuta en otra (S1, prueba
  del JIT).

### Objetos de memoria

- Un objeto de memoria es un trozo de memoria con una capacidad. Quien tiene la llave lo puede proyectar en su
  espacio, con los derechos que la llave permita: leer, escribir o ejecutar.
- La pantalla, los búferes de audio y los anillos de disco y red de S1 son objetos compartidos entre el cliente y el
  servidor, sin copiar.
- Un fichero proyectado en memoria es un objeto que llena el sistema de archivos, que es otro proceso, bajo demanda.
- Clonar con copia al escribir sirve para lanzar procesos deprisa y para las instantáneas de la caja Amiga.

### Zonas para los periféricos

El reparto de memoria física tiene zonas, sacadas del árbol de dispositivos al arrancar:

- **Primer GB**: para la tarjeta SD y el DMA clásico. Se guarda para ellos y no se gasta en otras cosas si se puede
  evitar.
- **Primeros 3 GB**: para el USB.
- **El resto**: para todo lo demás.

Cada controlador pide memoria para DMA diciendo qué periférico la usará, y el kernel elige la zona.

### La caja Amiga

- La caja es un proceso. Sus primeros 4 GB de direcciones virtuales son el "Amiga": ahí van la memoria de los
  programas 68k, las bibliotecas Amiga y sus estructuras, en big-endian. Detrás puede haber RAM física de cualquier
  zona.
- **La página 0 existe.** El Amiga guarda en la dirección 4 la base de exec, y los programas la leen. La página 0 se
  proyecta de solo lectura.
- **Un Enforcer integrado.** En el Amiga, Enforcer era una herramienta que usaba la MMU para pillar programas que
  escriben donde no deben. La caja puede hacerlo de serie: avisar de toda escritura en la página 0 y fuera de la
  memoria reservada, con la tarea y la instrucción culpables. Ayuda a depurar programas 68k, y Forja lo puede mostrar.
- El código traducido por el JIT vive en la caja, fuera de los 4 GB, con sus dos direcciones (escribir y ejecutar).
- El exec de la caja apunta el dueño de cada bloque de `AllocMem` (S2, matar tareas).

### Poca memoria

- **Sin intercambio a disco por defecto.** La tarjeta SD es lenta y se gasta. Se puede activar en un disco USB.
- **Memoria comprimida con LZ4**, en el kernel. Comprimir y descomprimir una página es un trabajo corto, sin
  pasar por otro proceso.
- **Sin prometer de más.** La memoria se reserva al pedirla, aunque las páginas se rellenen al usarlas. Si no hay, la
  petición falla en ese momento. Así no hay un OOM killer que mate procesos al azar.
- **Si aun así falta**, el sistema avisa en el escritorio y propone cerrar programas de fondo. Nunca se toca el
  compositor ni los servicios del sistema.
- **Límites por proceso**: quien lanza un proceso le puede poner un tope de memoria, igual que de CPU (S2).

## Lo que queda abierto

1. ¿Sin intercambio a disco por defecto, solo memoria comprimida? Recomendado: sí.
2. ¿Sin prometer de más memoria, de modo que la petición falle al momento, en vez de matar procesos después?
   Recomendado: sí. El coste es que algunos programas de Linux reservan mucho más de lo que usan; para ellos la capa
   de S7 puede prometer de más dentro de un límite.
3. ¿Enforcer integrado en la caja Amiga, activo siempre o solo al depurar? Recomendado: activo siempre, con avisos a
   un registro, porque cuesta poco.

## Cómo encaja

- Con S0: la meta de 128 MB en reposo depende de no cargar lo que no se usa. Los objetos de memoria y la carga bajo
  demanda ayudan. Sigue sin medir.
- Con S1: los objetos de memoria son objetos del kernel con capacidad, y llevan los anillos de comunicación.
- Con S2: los topes de memoria van junto a los de CPU, y matar libera toda la memoria de un proceso.
- S4 usa el hueco de 4 GB, la página 0 y el Enforcer.
- S5 sirve los ficheros proyectados en memoria.
- S7 decide cuánto puede prometer de más la capa de Linux.
- S12: cada controlador pide memoria DMA por zona.

## Fuentes

- `dma-ranges` de la Pi 4: leídos con `dtc` del `bcm2711-rpi-4-b.dtb` que baja Emu68 al compilar (firmware
  `0ea2874060`).
- Revisiones del chip y ventanas de PCIe: [arreglo de U-Boot](https://git01.mediatek.com/plugins/gitiles/filogic/uboot/+/038876b5813c81100ee5b0d0586416710068c937%5E%21),
  [historial de U-Boot](https://git.pikami.org/random-mirrors/smaeul-u-boot/commits/branch/a33/drivers/pci/pcie_brcmstb.c)
  y [discusión en la lista de Linux](https://lkml.iu.edu/hypermail/linux/kernel/1911.0/06777.html).
- Proporciones de LZ4 y zstd: [prueba de zram en Fedora](https://redlib.belloworld.it/r/Fedora/comments/mzun99/new_zram_tuning_benchmarks).
- Objetos de memoria de Fuchsia: documentación de Zircon (VMO y VMAR), conocida; no se consultó en esta vuelta.

Lo que no se comprobó: los `dma-ranges` que entrega el firmware en una Pi 4 real, la velocidad de LZ4 en una A72, y
el coste del Enforcer.
