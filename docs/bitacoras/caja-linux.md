# Bitácora de la caja Linux (H9)

Rama `caja-linux`, sacada de `revision` (da0adf0). El hito H9 de `docs/diseño/S15-hoja-de-ruta.md`: la caja Linux es
una máquina virtual con Arch Linux ARM dentro (S7, PLAN.md). Este es su primer paso: que DashOS pueda ejecutar una
máquina virtual.

## 2026-11-12: el hipervisor y la primera máquina virtual

### Diseño elegido

La Cortex-A72 es ARMv8.0, sin VHE. El kernel sigue entero en EL1, como siempre; en EL2 solo queda un trozo pequeño
que cambia de mundo entre el kernel y un invitado (como KVM en esas CPUs, sin su código: sala limpia, con la
especificación de ARM). La lógica de alto nivel va en un servicio de usuario, `caja-linux`.

- **Arranque** (`kernel/src/base/arranque.s`). Si el firmware entra en EL2 (la Pi, QEMU `raspi4b`, QEMU `virt` con
  `virtualization=on`), antes de bajar a EL1 deja en VBAR_EL2 una tabla que solo sabe una orden, "instalar", y pone el
  VMID 0 para el kernel. A Rust le dice en x1 si arrancó en EL2. Si entra en EL1, todo sigue como antes.
- **EL2** (`kernel/src/base/hiper.s`, unas 300 líneas de ensamblador). Tres órdenes por `hvc #0` desde el kernel:
  instalar (la tabla de verdad, su traducción y su pila), correr (guarda el kernel, carga el invitado, `eret`) y
  olvidar (vacía de las TLB un VMID que se suelta). Cualquier excepción que llega de abajo con un invitado en marcha
  es una salida: guarda el invitado, repone el kernel y vuelve tras su `hvc`. EL2 tiene la MMU encendida con una
  traducción propia: la RAM en identidad, en bloques de 2 MB (así ve con caché lo que escribe el kernel). Se instala
  tras el traslado de la imagen (C38), en cada núcleo.
- **El mundo** de una vCPU es una página: x0 a x30, PC, PSTATE y 26 registros de EL1 (con el temporizador virtual)
  del invitado; lo mismo del kernel mientras corre el invitado; la configuración (VTTBR, HCR, MDCR, CPTR, CNTHCTL,
  VTCR) y la salida (ESR, FAR, HPFAR, tipo).
- **La etapa 2** (`kernel/src/base/hiper.rs`): tablas de 4 KB desde el nivel 1, 4 GB de direcciones del invitado,
  sobre las páginas de un objeto de memoria. Un VMID de 8 bits por máquina (hasta 255).
- **Lo que se atrapa en EL2** mientras corre el invitado: las interrupciones físicas (IMO, FMO, AMO: el tic del reloj
  saca al invitado cada 10 ms como mucho, así que nunca se queda con el núcleo), SMC (si no, un invitado apagaría la
  máquina por PSCI), WFI, las cachés por set/way, los registros propios de la CPU, la depuración y los contadores de
  rendimiento, la coma flotante y el temporizador físico del kernel.
- **Llaves** (`kernel/src/maquinas.rs`, `llamadas.rs`). Un objeto nuevo, el permiso de máquinas virtuales, que el
  kernel da a `inicio` y `inicio` a `caja-linux`. Con él, `maquina_crear` (llamada 45) da una llave de máquina virtual
  sobre un objeto de memoria del cupo de quien la pide; la vCPU y las tablas también se cobran al grupo.
  `maquina_correr` (46) entra en el invitado y vuelve con el motivo ya interpretado: HVC, lectura o escritura en una
  dirección que no es su memoria (para emular aparatos), WFI, interrumpido o fallo. Error nuevo: `E_NO_HAY` (−10),
  sin hipervisor. Detalles en `docs/referencia/kernel.md`.
- **El servicio** (`usuario/src/bin/caja_linux.rs`) emula un PL011 en 0x09000000 para el invitado y decide qué hacer
  con cada salida. Los invitados de prueba son unas instrucciones AArch64 con su fuente en
  `usuario/invitados/pruebas.s`, que el propio servicio ensambla (`global_asm!`) y copia en la RAM del invitado.

### Qué funciona

`make arranque-virt-vm` (la imagen de `virt` en QEMU con `virtualization=on`, sin disco ni USB), con sus líneas en
`kernel/pruebas/arranque_virt_vm.txt`:

- El kernel arranca en EL2, instala el trozo de EL2 en los 4 núcleos y baja a EL1.
- Un invitado escribe "Hola desde el invitado, por el UART emulado" por el PL011 emulado, esperando sitio en su
  registro de estado, y acaba con HVC: la consola dice `[caja-linux] el invitado dice: ...`.
- Hostiles, cada uno en su máquina, que el servicio para cerrando su llave; el sistema sigue:
  - leer fuera de su memoria (0x70000000): el servicio lo ve como lectura de un aparato que no emula;
  - escribir en 0x40200000, donde en la máquina de verdad está el kernel de DashOS: para el invitado no existe;
  - un bucle sin fin con las interrupciones enmascaradas: el reloj lo saca unas 11 veces en 100 ms;
  - PSCI SYSTEM_OFF por SMC: atrapado (clase 0x17), la máquina no se apaga;
  - escribir en el temporizador físico (el tic del kernel): atrapado (clase 0x18).
  Tras un fallo, la máquina queda muerta: correrla otra vez da `E_CERRADO`.
- Tras los hostiles, una máquina nueva funciona.
- `sin-maquina`, sin el permiso, prueba las 1024 llaves posibles: ninguna crea una máquina. Con una llave de memoria
  por permiso, `E_TIPO`.
- Sin `virtualization=on` (`make arranque-virt`), el servicio dice que no hay hipervisor y el resto sigue igual.

### Medidas

QEMU `virt`, TCG, 4 núcleos, con el PC cargado por otras sesiones. Son números de emulador: en la Pi serán otros.

| Medida | Valor |
|---|---|
| Ida y vuelta servicio → invitado (HVC) → servicio, con la llamada al sistema, 2000 vueltas | 30 µs de media con el PC poco cargado; 51 a 121 µs con carga de 9 a 17 |
| De ellas, dentro del `hvc` del kernel (EL2, cambio de mundo y el invitado) | 18 µs; 30 a 66 µs con carga (siempre algo más de la mitad) |
| Linux hasta su línea 40 por la consola (Image de 36 MB, 64 MB de RAM) | 0,5 s desde la primera entrada en el invitado |

En TCG cada `msr`/`mrs` de un registro del sistema es caro; el cambio de mundo guarda y carga unos 60. Falta medirlo
en la Pi.

### Qué falta

- Interrupciones para el invitado: ni GIC virtual ni temporizador virtual entregado (por eso Linux se para en
  `time_init`). Si el invitado arma su
  temporizador virtual, su interrupción sale al anfitrión una y otra vez (el servicio lo ve como "interrumpido").
- La coma flotante del invitado: hoy se atrapa y mata la máquina. Hay que guardar la del hilo que lo corre (o la del
  dueño perezoso de la FPU en ese núcleo, C28) y dar al invitado la suya.
- Varias vCPU por máquina.
- Con GICv3, el invitado puede tocar los registros del sistema de la interfaz de CPU (los virtuales, con IMO/FMO);
  falta atrapar ICC_SRE_EL1 (ICC_SRE_EL2.Enable a 0 mientras corre).
- La memoria del invitado se lleva a la RAM al crear la máquina: lo que escriba el servicio después, con el invitado
  sin MMU, puede no verlo en la Pi (QEMU no tiene cachés).
- virtio (consola, disco, red, vsock, virtio-fs, virtio-gpu), el globo, Arch Linux ARM, el puente de Wayland.
- Probarlo en la Pi 4.

### Segundo paso: un kernel de Linux de verdad

`make arranque-virt-linux` (LINUX=Image; por defecto `~/.cache/dashos-linux/virt/linux`, el kernel 6.12 de Debian
para arm64 que ya usan otras pruebas; no va en git). La Image entra en el paquete de arranque con la opción `linux`
del kernel y llega a `caja-linux` como datos (su llave 2). El servicio:

- crea 64 MB de RAM en cuatro objetos de 16 MB (el tope de `memoria_crear`); la llamada nueva `maquina_memoria` (47)
  añade cada uno a lo que ve el invitado;
- escribe un árbol de dispositivos mínimo propio (memoria, una CPU, `bootargs` con `earlycon=pl011,0x09000000`; sin
  GIC ni temporizador), pone la Image en RAM + 2 MB y la vCPU empieza ahí con x0 = el árbol (`maquina_crear` lleva
  ahora el x0 del invitado en x4);
- atiende los registros del sistema que se atrapan (depuración y contadores de rendimiento, que Linux toca al
  arrancar) como de solo cero: motivo nuevo 7, `REGISTRO`, con la codificación del registro. El hostil del
  temporizador físico sale ahora por ahí, y el servicio para la máquina.

La vCPU se ve como el núcleo 0 (VMPIDR_EL2, que EL2 guarda y repone: el kernel lee de ahí su número de núcleo).

Resultado: Linux escribe sus primeras líneas por el PL011 emulado (las 40 primeras, en medio segundo):
`Booting Linux on physical CPU 0x0000000000 [0x410fd083]`, `Linux version 6.12.107+deb13-arm64`, `Machine model:
DashOS, caja Linux`, la consola temprana, las zonas de memoria, las erratas de la Cortex-A72, la línea de órdenes...
Sigue hasta `time_init`, unas 68 líneas, y ahí se para con `Kernel panic - not syncing: Unable to initialise
architected timer`: el árbol no tiene temporizador ni GIC (no hay interrupciones para el invitado). Con `panic=-1`
intenta reiniciar, no puede (sin PSCI) y se queda parado; a los 10 s el servicio para la máquina. Lo esperado está en
`kernel/pruebas/arranque_virt_linux.txt`.

### Lo que le falta a esta interfaz para el crate `caja-linux/vmm` (rama `caja-linux-virtio`)

El crate trae virtio-mmio (consola, disco, red), el PL011, el distribuidor GICv2 emulado, PSCI, el DTB y la carga de la
Image. Para enchufarlo en `caja-linux`:

1. **Escribir registros del invitado.** Para contestar a PSCI (x0 tras el HVC) y arrancar otra vCPU hace falta una
   llamada que lea y escriba x0..x30, PC y PSTATE de una vCPU parada. Hoy solo se escribe el registro de una lectura de
   aparato o de registro pendiente.
2. ~~Extensión de signo en las lecturas de aparato~~: hecha (SSE y SF).
3. **Inyectar un aborto externo** (`SalidaMmio::SinAparato`): no hay forma; solo leer 0 y seguir.
4. **Varias vCPU** por máquina, con su VMPIDR cada una (hoy una, siempre el núcleo 0), y que cada una pueda correr en
   su hilo del servicio.
5. **Interrupciones**: el GIC virtual del GIC-400 (GICH) guardado y repuesto en EL2 con cada cambio de mundo, la GICV
   proyectada en la etapa 2 en la IPA del GICC del invitado, los LR escritos desde el servicio (o desde el kernel con
   lo que diga el servicio), la interrupción de mantenimiento y el temporizador virtual (PPI 27, con HW = 1). Nada de
   eso está hecho: el invitado no recibe ninguna interrupción.
6. **Sacar a una vCPU que corre** (por una interrupción de otro núcleo) cuando llega algo por un canal (teclas, red),
   y un WFI que espere de verdad hasta que haya algo para la vCPU, en vez de volver al servicio.
7. Una barrera `dmb ish` en la biblioteca `dashos` para `MemoriaInvitado::barrera`.
8. CNTVOFF_EL2 está a 0 para todas (vale); CNTFRQ_EL0 ya lo pone el firmware.

### Decisiones para el usuario

1. El permiso de máquinas virtuales lo da hoy `inicio` a `caja-linux`. ¿Lo reparte `sesiones` como permiso de una
   aplicación (S6), o es solo del sistema?
2. La caja Linux solo está en la imagen de `virt`. ¿Va también en la de la Pi?
3. El hipervisor de EL2 se instala siempre que el firmware entra en EL2. ¿Se quiere poder apagarlo (que el kernel
   cierre EL2 al arrancar si no hay caja Linux)?

## 2026-11-12: Linux hasta busybox (rama `caja-linux-2`)

Rama `caja-linux-2`, sacada de `caja-linux` (f0f681f) con `revision` fusionada (la caja Amiga, el montón, el kit y el
crate `caja-linux/vmm` de la rama `caja-linux-virtio`). Sala limpia: especificaciones de Arm (GICv2 IHI 0048B, cap. 5,
las extensiones de virtualización; el temporizador genérico y el Arm ARM para la entrada a una excepción; PSCI DEN
0022) y el protocolo de arranque de arm64 como especificación. Sin código de Linux, KVM, QEMU, crosvm, Firecracker
ni Xen.

### Resultado

`make arranque-virt-linux`: un Linux 6.18 pequeño (tinyconfig y lo justo, 5 MB) con un initramfs de busybox
arranca dentro de DashOS con 256 MB de RAM, una vCPU, el PL011 (consola temprana), virtio-console (`hvc0`, su
consola) y un disco virtio-blk de 1 MB en memoria del servicio. Llega a `/init` en 0,73 s de su reloj, busybox lee el
disco (`DISCO-CAJA-LINUX`), escribe `ESCRITO-POR-LINUX` en el sector 8 (el servicio lo comprueba al acabar), enseña
sus interrupciones y apaga la máquina con `poweroff -f` (PSCI SYSTEM_OFF). Todo en 1,5 s:

```
[caja-linux] linux: [    0.000000] psci: PSCIv1.0 detected in firmware.
[caja-linux] linux: [    0.000000] arch_timer: cp15 timer running at 62.50MHz (virt).
[caja-linux] linux: [    0.731342] Run /init as init process
[caja-linux] linux: caja-linux: BusyBox v1.37.0 (2026-11-12 17:20:49 CET) multi-call binary.
[caja-linux] linux: caja-linux:   virtio0 id=0x0003 virtio_console
[caja-linux] linux: caja-linux:   virtio1 id=0x0002 virtio_blk
[caja-linux] linux: caja-linux: consola hvc0
[caja-linux] linux: caja-linux: vda 2048 sectores, primeros bytes: DISCO-CAJA-LINUX
[caja-linux] linux: caja-linux: escrito en vda: ESCRITO-POR-LINUX
[caja-linux] linux: caja-linux: irq  11:        171 GIC-0  27 Level     arch_timer
[caja-linux] linux: caja-linux: irq  14:         32 GIC-0  48 Level     virtio0
[caja-linux] linux: caja-linux: irq  15:          6 GIC-0  49 Level     virtio1
[caja-linux] linux: se para la máquina a los 1539 ms: Linux apagó la máquina por PSCI
[caja-linux] linux: 19291 salidas: 18978 a aparatos (0 abortos), 7 PSCI, 0 WFI (0 ms dormido), 302 por el reloj, 4 registros; 48 interrupciones inyectadas
```

Casi todas las salidas son de la consola temprana: cada carácter por el PL011 es una escritura y una lectura de FR.

### Lo que se hizo en el kernel

- **El GIC virtual** (`base/hiper.rs`). `main.rs` lee del árbol las regiones 3 y 4 del GICv2 (GICH y GICV, las dos
  del kernel) y la interrupción de mantenimiento (en QEMU `virt`, GICH 0x08030000, GICV 0x08040000, PPI 9 = 25).
  Cada núcleo apaga su GICH y habilita las PPI 27 y 25 (`gic_nucleo`, desde `gic::iniciar_cpu`). En cada vuelta al
  invitado, con las interrupciones del kernel enmascaradas (el hilo no cambia de núcleo), se cargan GICH_VMCR,
  GICH_APR y los registros de lista y se pone GICH_HCR.En; al salir se guardan y En vuelve a 0. No hace falta EL2
  para eso: con el kernel en marcha, HCR_EL2.IMO = 0 y una interrupción virtual no le llega.
- **La GICV en la etapa 2**: dos páginas de aparato (Device-nGnRE, sin ejecutar) donde el invitado espera su GICC
  (`maquina_estado`, operación 3). El distribuidor sale al servicio como cualquier aparato.
- **El temporizador virtual con su bit HW**: el último registro de lista es del kernel. Antes de entrar, si quedó
  libre y el CNTV del invitado está disparado (encendido, sin enmascarar y con el contador en su plazo), se pone ahí
  el INTID 27 con HW = 1 y el mismo número físico, y la PPI 27 física de ese núcleo se marca activa (GICD_ISACTIVER)
  mientras la virtual siga en el LR: no salta en el anfitrión y el EOI del invitado la desactiva. Si el temporizador
  sigue disparado tras el EOI, la física queda pendiente, saca al invitado (IMO) y se inyecta otra vez. Con la
  prioridad y el grupo que el invitado le dio en su distribuidor (lo dice el servicio). El kernel no usa el
  temporizador virtual: lo apaga al arrancar cada núcleo por si el firmware lo dejó en marcha.
- **Los registros de lista del servicio**: un bloque que va y viene con `maquina_correr` (cuarto argumento). El kernel
  quita el bit HW (con él, el EOI del invitado desactivaría una interrupción física cualquiera del anfitrión) y los
  bits reservados, y deja a 0 un LR sin estado (con su bit EOI, el GIC daría mantenimiento sin parar). El servicio
  pone el bit EOI: cuando el invitado termina una interrupción, la de mantenimiento lo saca y el servicio vuelve a
  mirar la línea (las de virtio y el PL011 son por nivel).
- **La interrupción de mantenimiento y la PPI 27 en el anfitrión**: ya hicieron su trabajo al sacar al invitado; el
  manejador de IRQ del kernel solo las termina, sin replanificar ni avisar.
- **La coma flotante del invitado**: CPTR_EL2.TFP sigue puesto hasta la primera vez que la usa; esa salida no llega
  al servicio: la vCPU pasa a tener sus registros de FP (528 bytes, a cero) y se repite la instrucción. Desde ahí, en
  cada vuelta, `planificador::fp_soltar` guarda los del hilo dueño de la FPU de ese núcleo (C28) y lo deja sin dueño;
  se cargan los del invitado, y al salir se guardan. El dueño los vuelve a pedir por la trampa de siempre.
- **Registros y aborto externo** (llamada 48, `maquina_estado`): leer y escribir x0..x30, PC y PSTATE de la vCPU
  parada (PSTATE saneado: AArch64 en EL0t, EL1t o EL1h; si no, EL1h), y cambiar el último acceso a un aparato por un
  aborto externo síncrono de datos en el vector de EL1 del invitado, como lo haría la CPU (ESR_EL1 con clase 0x24 o
  0x25 y DFSC 0x10, FAR_EL1, ELR_EL1, SPSR_EL1, PC en VBAR_EL1 más 0x000, 0x200 o 0x400, EL1h con DAIF).
- **El informe** pasa a 12 palabras: x3 (para PSCI), el plazo del temporizador virtual y si hay una interrupción
  pendiente en los registros de lista (para saber si un WFI tiene que esperar).

### Lo que se hizo en el servicio

`caja-linux` enchufa el crate (feature `caja` de `usuario`, con el montón). Una `Caja` lleva la máquina del crate,
su PSCI y los registros de lista: antes de cada vuelta, los LR que el invitado terminó van a `Distribuidor::terminada`
y los libres se llenan con `pendientes(0)` (`gic::lr_v2`, nuevo en el crate, con `lr_libre` y `privada`, que dice
cómo tiene el invitado la PPI 27). Accesos a aparatos, por `Maquina::mmio`; sin aparato, aborto externo. HVC #0, por
`Psci::atender`, con la respuesta en x0 por la llamada 48. En un WFI sin nada pendiente, el servicio duerme hasta el
plazo del temporizador del invitado (como mucho 10 ms). La consola (PL011 y virtio-console) va al registro de DashOS
con "linux: " delante; el disco es un `Vec` del montón. La memoria del invitado se envuelve en una `Ram` con la
barrera de verdad (`dashos::barrera`, `dmb ish`).

La RAM del invitado son 256 MB en trozos de 16 MB. Con 2 GB de RAM en QEMU (el primer GB solo para DMA) no caben
los 256 + 32 MB del cupo de la caja: `inicio` le da entonces 160 MB (o 96), y el servicio usa para el invitado lo que
le quepa (como poco 128 MB). `make arranque-virt-linux` corre con 3 GB (`MEMORIA=3G`, nuevo en `prueba_uefi.sh`).

La Image del defconfig (74 MB) no cabe en el paquete de arranque: el cargador de UEFI dice "la memoria de 0x40200000
está ocupada". `tools/caja_linux_invitado.py preparar` hace ahora también `Image-caja`, un tinyconfig con lo justo
(GICv2, temporizador, PSCI, PL011, virtio-mmio, consola y disco, initramfs, red mínima), de 5 MB; el Makefile la usa
por defecto, con `INITRD` = el initramfs. El `/init` dice la versión de busybox y, con `caja.escribir` en la línea de
órdenes, escribe en el disco.

### Pruebas

- `make prueba`, `make arranque-virt` (sin virtualización: la caja dice que no hay hipervisor, como antes),
  `make arranque-virt-vm` y `make arranque-virt-linux`: BIEN. `cargo test` en `caja-linux/vmm`: 36 pruebas.
- Nuevas en `arranque-virt-vm` (`usuario/invitados/interrupciones.s`):
  - el temporizador virtual tres veces (el manejador lee GICC_IAR = 27, rearma y hace EOI), una SPI 48 que el servicio
    pone en un LR cuando el invitado toca un "timbre" (su LR queda libre al terminarla) y PSCI_VERSION contestado en
    x0: `el temporizador virtual 3 veces, la SPI 48 del servicio 1 (...: sí), PSCI 1.0 en x0, 8 salidas`;
  - hostil, el GIC al azar: 4096 escrituras con valores al azar por las dos páginas de su GICV (con lecturas de IAR y
    EOIR con números cualquiera), 4096 por su distribuidor emulado y una lectura y una escritura en la GICH del
    anfitrión (para el invitado no hay nada: dos abortos externos, que su vector cuenta);
  - hostil, una tormenta del temporizador: lo arma ya disparado y su vector vuelve sin reconocer la interrupción, que
    entra otra vez sin fin; el reloj del anfitrión lo saca (11 a 22 veces en 100 ms) y la máquina se cierra;
  - hostil, un registro de EL2 (`msr vttbr_el2`): la excepción va a su propio vector, fuera de su memoria, y la
    máquina se para por el fallo de la etapa 2.

### Medidas

QEMU `virt`, TCG, con el PC cargado (carga 20 en 32 núcleos):

| Medida | Valor |
|---|---|
| Linux, de la primera instrucción a `/init` (su reloj) | 0,73 s |
| Linux, hasta apagar la máquina (reloj del servicio) | 1,5 s, 19 291 salidas |
| Ida y vuelta servicio → invitado → servicio (HVC) | 30 a 41 µs (antes, 30), con el GIC virtual cargado y guardado |

### Qué falta

- Varias vCPU (cada una en su hilo del servicio, con su VMPIDR, CPU_ON de PSCI), y sacar a una vCPU que corre cuando
  llega algo por un canal (hoy, como mucho un tic del anfitrión).
- Entrada por la consola (teclas hacia el invitado) y un disco de verdad por el servicio de archivos.
- GICv3 (portátiles, servidores): el invitado no tiene interrupciones; falta atrapar ICC_SRE_EL1.
- Las mitigaciones de Spectre por SMCCC 1.1 (ARCH_WORKAROUND_1) para la Cortex-A72.
- El kernel no cobra al grupo los 528 bytes de la FP del invitado.
- La Pi 4: nada de esto se ha probado en hardware.

### Las decisiones del usuario (apuntadas en `PLAN.md` de `revision`) y en qué quedan aquí

1. El permiso de máquinas virtuales lo reparte `sesiones` como permiso de aplicación (S6b): **no hecho**; sigue
   dándoselo `inicio` a `caja-linux`.
2. La caja Linux también en la imagen de la Pi 4 (solo si el firmware entra en EL2), no en la de la Pi 3: **no
   hecho**; sigue solo en `virt`. El GIC virtual ya lee el GIC-400 del árbol, que es el mismo de la Pi 4.
3. El hipervisor no se apaga, pero sin invitados no debe atrapar nada: así es hoy (HCR_EL2 del anfitrión sin
   trampas; con este paso, solo las PPI 27 y 25 habilitadas, que sin invitado no saltan). **Sin cambios**.
4. La memoria de la caja, calculada (invitado + unos 32 MB; 256 MB de invitado con 2 GB, 512 con 4 GB o más): **en
   parte**: el cupo pide 256 + 32 MB, y si no caben, menos; el servicio usa lo que quepa. Falta calcularlo con la RAM
   de la máquina (y los 512 MB con 4 GB).
