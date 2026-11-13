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

## 2026-11-12: el permiso por `sesiones`, la memoria calculada, la Pi 4, una terminal y la red (rama `caja-linux-3`)

Rama `caja-linux-3`, sacada de `revision` con la caja hasta busybox ya fusionada (b33354d). Sala limpia como antes:
especificaciones (ECMA-48 y el VT100 para la terminal; virtio 1.2 para la red); nada de código de Linux, KVM, QEMU,
crosvm, Firecracker ni Xen.

### Las decisiones del usuario, hechas

1. **El permiso lo reparte `sesiones`.** `inicio` solo da el permiso de máquinas virtuales a `sesiones` (su llave 2;
   las demás corren un puesto: archivos 3, comprobar 4, eco 5, su canal 6, su copia 7 y la de archivos 8, justo las 8
   que caben). En `REPARTE` es "maquinas", un permiso del kernel como el de plazo. `SIN_PREGUNTAR` dice lo que se da
   sin preguntar a un programa del sistema (por el nombre que da `inicio`, no el de un fichero del disco): "maquinas" a
   `caja-linux`. Se apunta como concedido desde el final de la tabla (lo que concede el usuario va delante, y la
   prueba del centro de control quita la primera fila), no se guarda en el disco (es de serie) y sale en el centro de
   control; si alguien lo quita, la próxima vez se pregunta (hasta reiniciar: lo quitado aún no se guarda). Toda
   concesión de "maquinas" (sin preguntar o porque el usuario dijo que sí) va al registro de seguridad, no solo las
   negativas. `caja-linux` lo pide al arrancar (hasta 60 s: `sesiones` puede tardar); `sin-maquina` lo pide también y
   no lo recibe (con `guardian`, se le pregunta a quien está delante; sin él, -2 en el acto). Solo se pide si hay
   hipervisor: en `make arranque-virt` (sin EL2) nada cambia en el centro ni en las preguntas.
   - `sesiones` no atendía a nadie sin disco: esperaba sin plazo al servicio de archivos, que sin disco no contesta.
     Ahora espera 15 s (con disco contesta en menos de uno) y sigue sin archivos. Por eso, en `arranque-virt-vm` y
     `arranque-virt-linux` (sin disco) ya hay `sesiones`.
2. **La memoria, calculada.** El kernel da a `inicio` un objeto "sistema" (la RAM que conoce y si hay hipervisor).
   `dashos::maquina::memoria_invitado`: 512 MB con 4 GB o más, 256 MB con 2 GB o más, 128 MB con menos (con 128 MB de
   holgura en cada umbral: el kernel no cuenta lo del firmware). El cupo de la caja es eso más 32 MB
   (`MARGEN_SERVICIO`) si tiene la Image de Linux; sin ella, 32 MB. Si no cabe, la mitad para el invitado, hasta 64 MB.
   La caja lee el mismo objeto y pide esa RAM en trozos de 16 MB (hasta 32). Con `MEMORIA=3G`: `[inicio] caja-linux:
   3072 MB de RAM en la máquina; el invitado, hasta 256 MB`.
3. **La Pi 4.** `caja-linux` va en la imagen de la Pi 4 (no en la de la Pi 3): llaves "sistema" y el permiso, que da
   `inicio` (en la Pi aún no hay `sesiones`, que es solo de `virt`: excepción a la decisión 1 hasta que lo haya). Si no
   hay hipervisor, lo dice y acaba; si lo hay, corre un invitado que saluda y acaba. Nada más (ni los hostiles ni
   Linux). En QEMU `raspi4b`, `make prueba` sigue igual.

### La terminal

`caja-linux` tiene ahora el canal "ventanas". Con la Image de Linux, antes de arrancarlo abre una ventana de terminal
en un hilo suyo (`hilos::crear`): un cliente de Wayland con la llave de "ventanas" que le toca (`ventana_cliente::
usar_llave`; era siempre la 1), 80x25 letras de Roboto Mono a 13 px (añadida al kit, `Fuente::mono`, OFL) en una
ventana de 656x416. Lo que escribe Linux por hvc0 pasa por un `Tubo` (cola con cerrojo y tope de 256 KB; lo que no cabe
se pierde y se cuenta) a una `Terminal` (`usuario/src/terminal.rs`, un VT100 sencillo: UTF-8, ajuste de línea, CSI de
mover, borrar, insertar y quitar, colores SGR, DSR 6; acotada en todo y con pruebas en el PC, también con 200 000 bytes
al azar). Pinta solo las filas que cambian, una vez por cuadro (espera el `frame` del compositor). Las teclas van por
el `Teclado` del kit (mapa español) y Control+letra; salen como las de un VT100 (Intro = CR, retroceso = DEL, flechas
CSI) a otro `Tubo` hacia virtio-console. Para que la vCPU las vea: el hilo marca `aviso` y cambia `timbre`; la vCPU, en
cada salida por el reloj y en cada WFI, llama a `Maquina::sondear` si hay aviso, y en un WFI duerme en `timbre`
(`hilos::esperar`) en vez de `dormir_us`, así una tecla la despierta en el acto.

El `/init` (`tools/caja_linux_invitado.py`), con `caja.terminal` en la línea de órdenes, deja tras la prueba una shell
con terminal de control en hvc0 (`(setsid cttyhack sh)`) y apaga al salir de ella. Hace también `busybox --install` y
`hostname caja`.

Al registro de DashOS sigue yendo cada línea de la consola (sin las secuencias de escape), pero con tope: 400 seguidas y
100 por segundo; si se pasa, calla hasta que Linux lleva medio segundo sin líneas y dice cuántas calló.

`make arranque-virt-linux` lleva ahora teclado y ratón USB y la red de QEMU, y `tools/guion_caja_linux.py` (con
`CAJA_LINUX=prefijo` en `prueba_uefi.sh`): clic en la ventana, `uname -a`, `echo hola desde DashOS`, captura,
`seq 1 30000` (la inundación), `echo sigue viva`, `ping -c 2 10.0.2.2` y `poweroff -f`. La captura queda en
`target/caja-terminal.png` y en `capturas-agente/` (fuera de git). En esta imagen (`linux`) no van `guardian` ni las
pruebas que lo usan (pide, entrada, hostil y presa, selector): sus preguntas y su pantalla de entrada se llevarían el
teclado.

### La red del invitado

Un puente de tramas en el servicio de red: la orden `PUERTO` (solo de "caja-linux") crea una memoria de tramas como la
de la tarjeta (64 ranuras para el invitado, 64 de él) y le da una copia a la caja. Lo que llega por la tarjeta para la
MAC del invitado va solo al puerto; lo de difusión, al puerto y a smoltcp. Lo que pone el invitado sale por la tarjeta
si lleva su MAC de origen (si no, se tira y se cuenta). El servicio guarda lo que mueve él y no se fía de lo que dice
la caja. Avisos por `TRAMAS_PUERTO` en los dos sentidos; un hilo de la caja espera los del servicio y despierta a la
vCPU. Si la caja se va, el puerto se suelta. En la caja, la virtio-net del crate (tercer transporte) con una MAC local
al azar (02:da:50:...). Linux sale con 10.0.2.16 (DashOS tiene la 15 por DHCP) y QEMU (slirp) contesta el ping a
10.0.2.2: `caja-linux: ping 3 packets transmitted, 3 packets received, 0% packet loss`.

### Resultado de `make arranque-virt-linux`

```
[inicio] caja-linux: 3072 MB de RAM en la máquina; el invitado, hasta 256 MB
[sesiones] caja-linux es del sistema: maquinas sin preguntar
[sesiones] sin-maquina pide maquinas: -2
[sin-maquina] pedir el permiso a sesiones: -2 (negado)
[caja-linux] terminal: ventana en pantalla (80x25 letras, 656x416 px)
[red] puerto para la caja Linux con MAC 02-da-50-38-57-fc: un puente de tramas hacia la tarjeta
[caja-linux] linux: caja-linux: eth0 02:da:50:38:57:fc mtu 1500
[caja-linux] linux: caja-linux: ping 3 packets transmitted, 3 packets received, 0% packet loss
[caja-linux] linux: caja:~# uname -a
[caja-linux] linux: Linux caja 6.18.63 #2 SMP Thu Nov 12 18:56:07 CET 2026 aarch64 GNU/Linux
[caja-linux] linux: hola desde DashOS
[caja-linux] linux: (29563 líneas sin sacar al registro: demasiadas seguidas)
[caja-linux] linux: sigue viva
[caja-linux] linux: 2 packets transmitted, 2 packets received, 0% packet loss
[caja-linux] linux: se para la máquina a los 143280 ms: Linux apagó la máquina por PSCI
[caja-linux] red: 13 tramas del invitado a la red, 7 de la red al invitado; 0 perdidas sin sitio, 0 malas del servicio
[caja-linux] terminal: 0 bytes de la consola no cupieron en la ventana; 0 teclas no cupieron hacia Linux
```

La inundación (`seq 1 30000`, 170 KB) no tumba a la caja ni al compositor: la ventana la enseña entera, el registro
calla y la shell contesta después.

En `make arranque-virt-vm` (con `guardian`), `sin-maquina` sale en una pregunta a quien está delante; nadie contesta,
la pregunta caduca y se le niega: `[guardian] ¿sin-maquina puede usar maquinas? El usuario dice no`.

En la imagen normal de la Pi en QEMU `raspi4b` (que entra en EL2): `[caja-linux] el invitado dice: Hola desde el
invitado, por el UART emulado` y `[caja-linux] hipervisor en marcha: un invitado saluda y acaba: bien`. La imagen de
pruebas (`make prueba`) no lanza `inicio`: no cambia.

### Pruebas

- `make prueba` (BIEN: esperado.txt, con las pruebas de `usuario/src/terminal.rs` en el PC), `make arranque-virt`,
  `make arranque-virt-vm`, `make arranque-virt-linux` y `make arranque-virt-2`: BIEN.
- Hostiles: `sin-maquina` pide el permiso a `sesiones` y no lo recibe, y no crea máquinas con ninguna de sus 1024
  llaves; la inundación de la consola (arriba); la terminal con 200 000 bytes al azar (en el PC); el puerto de red no se
  fía de los índices de la caja y tira las tramas con otra MAC de origen.

### Qué falta

- Varias vCPU (PSCI CPU_ON, una vCPU por hilo): no empezado. Hace falta en el kernel una vCPU por objeto (con su
  VMPIDR, su página de mundo y su estado del GIC virtual) y sacar a una vCPU que corre desde otro núcleo.
- Lo quitado de `SIN_PREGUNTAR` en el centro de control no se guarda: al reiniciar se da otra vez sin preguntar.
- Retirar el permiso revoca la llave, pero las máquinas ya creadas siguen en marcha (el kernel solo mira el permiso al
  crear).
- En la Pi no hay `sesiones`: `inicio` da el permiso directamente a la caja.
- La terminal no cambia de tamaño, no tiene historial hacia atrás, ni ratón, ni copiar y pegar; las letras que no tiene
  Roboto Mono salen como "?". Cerrar la ventana no hace nada.
- La red: sin prueba de un invitado que inunde la red o falsee su MAC (el servicio lo tira y lo cuenta, pero no se ha
  probado desde Linux); sin DHCP en el invitado (dirección fija 10.0.2.16); el invitado no llega a DashOS mismo (solo
  a la tarjeta).
- `make arranque` (la imagen normal en `raspi4b`) ya no casaba con `pruebas/arranque.txt` antes de esta rama (las
  llaves de `inicio` cambiaron con el plazo y el hipervisor).

## 2026-11-12: Arch Linux ARM en la caja (rama `caja-linux-arch`)

Rama `caja-linux-arch`, sacada de `fusion-fondo` (be30e2b), con todo lo de las cajas. El usuario preguntó si los paquetes
de Arch Linux iban a funcionar: la caja de verdad lleva un Arch Linux ARM completo, no busybox. Sala limpia como antes:
especificaciones (Arm PL031 TRM DDI 0224, la GPT de UEFI, el README de systemd con lo que pide al kernel); nada de código
de Linux, KVM, QEMU, crosvm, Firecracker ni Xen. Kernels, imágenes y rootfs, fuera de git (`~/.cache/dashos-caja-linux`).

### Resultado

`make arranque-virt-arch`: DashOS arranca, el servicio de disco da a la caja la partición de la caja Linux del NVMe y
la caja arranca dentro un Linux 6.18 que monta esa partición como raíz y ejecuta el systemd 261 de Arch Linux ARM.
getty entra como root en hvc0 (la ventana de terminal del escritorio), la red sale por DHCP y `pacman -Sy` baja el
índice de paquetes de los servidores de Arch Linux ARM. El guion lo apaga con `poweroff`: systemd para todo y apaga la
máquina por PSCI. De la primera instrucción del invitado a `poweroff`, 49 s (con QEMU sin aceleración); root entra a
los 16 s.

```
[nvme] particiones: GPT; DashOS, sectores 2048 a 16383
[nvme] la caja Linux: sectores 16384 a 8386559 (4087 MB)
[archivos] RedoxFS abierto: 7168 KB, 4084 KB libres
[caja-linux] arch: la partición de la caja Linux, 4087 MB, como disco del invitado
[caja-linux] linux: Image de 7288 KB en 0x40000000, initramfs de 0 KB, árbol de 2058 bytes en 0x4fe00000, 256 MB de RAM, PL011, PL031, virtio-console, virtio-blk de 4185088 KB y virtio-net por el servicio de red; arranca
[caja-linux] linux: [    0.701815] rtc-pl031 9010000.rtc: setting system clock to 2026-11-12T18:44:21 UTC (1794509061)
[caja-linux] linux: [    0.872244] EXT4-fs (vda): mounted filesystem a60c4dff-... r/w with ordered data mode.
[caja-linux] linux: [    0.902033] Run /sbin/init as init process
[caja-linux] linux: [    1.915844] systemd[1]: systemd 261.2-1-arch running in system mode (+PAM +AUDIT ...)
[caja-linux] linux: Welcome to Arch Linux ARM!
[caja-linux] linux: Arch Linux ARM 6.18.63-caja (hvc0)
[caja-linux] linux: caja login: root (automatic login)
[caja-linux] linux: [root@caja ~]# uname -a
[caja-linux] linux: Linux caja 6.18.63-caja #2 SMP Thu Nov 12 19:34:13 CET 2026 aarch64 GNU/Linux
[caja-linux] linux: [root@caja ~]# systemctl is-system-running --wait
[caja-linux] linux: running
[caja-linux] linux: [root@caja ~]# ip -4 addr show eth0
[caja-linux] linux:     inet 10.0.2.16/24 metric 1024 brd 10.0.2.255 scope global dynamic eth0
[caja-linux] linux: [root@caja ~]# date
[caja-linux] linux: Thu Nov 12 18:54:43 UTC 2026
[caja-linux] linux: [root@caja ~]# pacman -Sy --noconfirm
[caja-linux] linux: :: Synchronizing package databases...
[caja-linux] linux: [root@caja ~]# ls /var/lib/pacman/sync
[caja-linux] linux: alarm.db  aur.db  core.db  extra.db
[caja-linux] linux: se para la máquina a los 48885 ms: Linux apagó la máquina por PSCI
[caja-linux] linux: 42988 salidas: 29618 a aparatos (0 abortos), 7 PSCI, 3175 WFI (25326 ms dormido), 10184 por el reloj, 4 registros; 2963 interrupciones inyectadas
[caja-linux] red: 485 tramas del invitado a la red, 8038 de la red al invitado; 0 perdidas sin sitio, 0 malas del servicio
[caja-linux] arch: disco: 2679 lecturas (98303 KB), 1399 escrituras (71940 KB), 52 vaciados, 0 errores
[caja-linux] arch: 495 líneas por la consola; systemd de Arch Linux ARM: sí, root entra en hvc0: sí, su shell: sí: bien
```

La captura de la terminal queda en `capturas-agente/caja-linux-arch.png` (fuera de git).

### Lo elegido y por qué

- **La imagen.** El rootfs oficial, `ArchLinuxARM-aarch64-latest.tar.gz` (790 MB, del 2026-08-05), de
  os.archlinuxarm.org. Se comprueba su MD5 y su firma: la hizo la clave de compilación de Arch Linux ARM, cuya huella
  (68B3537F39A313B3E574D06777193F152BDBE6A6) es la que publica archlinuxarm.org; la clave pública sale del propio
  rootfs, así que lo que cuenta es que la huella case. `tools/caja_linux_invitado.py preparar-arch` lo baja, lo
  comprueba y hace el disco sin root: reescribe el tar con Python (`tarfile`, conserva dueños, permisos y los atributos
  `security.capability`) y `mke2fs -d fichero.tar` hace el ext4 desde él (e2fsprogs 1.47.1 o más; el `mke2fs` del SDK de
  Android, si va antes en el PATH, no sabe leer un tar: se usa el de `/usr/bin`). Lo cambiado en el rootfs:
  - `/etc/hostname` (caja) y `/etc/fstab` (la raíz, sin fsck);
  - getty entra solo como root en hvc0 (`serial-getty@hvc0.service.d/autologin.conf`). La contraseña de root sigue
    siendo la de serie de Arch Linux ARM ("root");
  - apagados (máscara a `/dev/null`): `systemd-timesyncd` (iría a servidores de NTP de fuera; la hora la da el PL031),
    `systemd-firstboot` (pediría idioma y contraseña por la consola), `sshd` y `sshdgenkeys` (genera sus claves en el
    primer arranque, y nadie entra por SSH a la caja);
  - `/etc/.updated` y `/var/.updated` con la fecha de `/usr`: sin ldconfig, hwdb ni catálogo en el primer arranque (el
    rootfs ya los trae hechos).
- **El kernel.** El de Arch (`linux-aarch64`, 7.1.6) es una Image de 42 MB que necesita sus módulos en un initramfs de
  15 MB; la del defconfig (74 MB) ya no cabía en el paquete de arranque de DashOS. Así que es el propio: `Image-arch`,
  tinyconfig con lo de `Image-caja` y lo que pide systemd en su README (cgroups v2 con memoria, pids, cpu y BPF;
  devtmpfs, inotify, signalfd, timerfd, epoll, fhandle, autofs, espacios de nombres, seccomp, ACL y xattr de tmpfs y
  ext4), lo que piden glibc y pacman (flock, AIO, io_uring, rseq, membarrier, SysV IPC) e IPv6, PL031 y Landlock: pacman 7
  baja los paquetes en un sandbox con Landlock y sin él falla ("restricting filesystem access failed because Landlock
  is not supported by the kernel"). 7 MB, sin módulos. `tools/caja_linux_invitado.py probar-arch` lo arranca con el
  disco en `qemu-system-aarch64 -M virt` con el DTB y la carga del crate (como `probar`): así se ajustó, en minutos,
  antes de llevarlo a DashOS.
- **El disco grande.** Una partición del NVMe de DashOS, no un segundo disco: es como quedaría un DashOS instalado con
  su caja Linux, y el servicio de disco ya sabe dar tramos (A13, A14). `dashos::particiones` conoce un tipo nuevo de
  GPT, el de la caja Linux (d0a18dbf-bbf4-4b0e-8627-ce5006b50018, el de DashOS con el último byte en 0x18), y el
  servicio de disco se lo da a "caja-linux" como si fuera su disco entero. Nunca una partición de Linux cualquiera: en un
  PC con otro sistema, la caja no debe ver el Linux de verdad del usuario. Dos de la caja son un error, como dos de
  DashOS (con su prueba en `archivos/herramientas`). En la caja, `DiscoNvme` hace de `Bloques` del virtio-blk del crate:
  abre su tramo, proyecta la ventana de 128 KB y pide lecturas y escrituras de 64 KB como mucho (y "vaciar" en cada
  FLUSH), con la vCPU parada mientras tanto. El crate copia ahora en trozos de 64 KB (antes 4 KB: cada trozo es una
  petición al servicio). El disco es de 4 GB: la partición de DashOS (MB 1 a 8, con el RedoxFS de las pruebas, que
  `poner-dashos` copia antes de cada prueba) y la de la caja (el resto, 4087 MB). QEMU lo usa con `snapshot=on`: nada de
  lo que escriben DashOS o Arch se queda.
- **La hora.** Un PL031 en el crate (`caja-linux/vmm/src/pl031.rs`, con pruebas): cuenta la hora de DashOS
  (`hora_ns`, la del PL031 de `virt`) más un desfase que cambia si el invitado carga el contador; la alarma, con su
  interrupción. Va en el árbol (`rtc@9010000`, SPI 2) en los dos invitados.
- **La red.** DHCP en el invitado: `systemd-networkd` de Arch ya lo pide en `eth*`. Funciona por el puente de tramas
  sin cambios: la petición sale por la tarjeta con la MAC del invitado y el servidor DHCP de QEMU contesta a esa MAC,
  que el servicio de red manda al puerto. Le da la 10.0.2.16 (DashOS tiene la 15) y el DNS de QEMU. `pacman -Sy` baja
  core, extra, alarm y aur (unos 21 MB) por HTTP: 8038 tramas hacia el invitado, ninguna perdida.
- **La memoria.** 256 MB para el invitado, con `MEMORIA=3G` en QEMU. Con `-m 4G` (y `highmem` encendido sin ECAM ni
  MMIO altos) DashOS sigue viendo 3072 MB: su mapa lineal acaba en los 4 GB de direcciones físicas y la RAM de `virt`
  empieza en el GB 1, así que nunca llega a los "4 GB o más" de la decisión del usuario (512 MB). Arch con systemd va
  bien con 256 MB.

### Lo que hubo que arreglar por el camino

- **El orden de las llaves de la caja.** `inicio` da los canales en el orden en que se nombran por primera vez en su
  lista, no en el de la lista de cada programa. "disco" lo nombra antes el servicio de disco, así que va antes que
  "sesiones": la caja mandaba la petición del permiso al servicio de disco y `sesiones` no la veía (el primer intento
  acabó en "sesiones no le da el permiso (-5)"). `llaves()` lo sabe ahora: ventanas, disco, sesiones, red.
- **Sin partición de DashOS** (el primer disco solo tenía la de la caja), `archivos` acaba en el acto, `inicio` lo
  relanza tres veces y `sesiones`, que lo espera cada vez, no contesta en un minuto: tampoco llegaba el permiso. Con la
  partición de DashOS en el disco, todo va como en `arranque-virt`.
- **Las cadenas de control.** systemd manda a la consola OSC 3008 (contexto de cada sesión) y una DCS XTGETTCAP
  (`ESC P + q ...`). La terminal (`usuario/src/terminal.rs`) y el registro de la caja se saltan ahora OSC, DCS, SOS, PM y
  APC enteras (con prueba). Antes salía "+q6E616D65" en la pantalla.
- **El clic del guion.** En esta imagen, Ajustes y las ventanas de prueba salen encima de la terminal; un clic en su
  centro daba el teclado a otra. El guion de Arch hace clic abajo a la izquierda de la ventana.

### Pruebas

- `make arranque-virt-arch` (nueva): BIEN, con `kernel/pruebas/arranque_virt_arch.txt` (las particiones, la partición
  de la caja abierta, el PL031, ext4 montado, systemd, getty, uname, `running`, la dirección por DHCP, la hora,
  `pacman -Sy` y el apagado por PSCI).
- `cargo test` en `caja-linux/vmm`: 5 pruebas nuevas del PL031 y el `rtc@9010000` del árbol. `archivos/herramientas`:
  `gpt_caja_linux` (la partición de la caja sola, con la de DashOS, una de Linux cualquiera que no cuenta, y dos).
  `usuario/src/terminal.rs`: `cadenas_de_control`.
- `make prueba` (BIEN: esperado.txt y BIEN: cargador hunk), `make arranque-virt`, `make arranque-virt-vm`,
  `make arranque-virt-linux` y `make arranque-virt-2`: BIEN como estaban, seguidos y con el PC cargado por otras
  sesiones.

### Qué falta

- **Instalar paquetes**: solo se probó `pacman -Sy` (el índice). Para `pacman -S` hace falta antes `pacman-key --init` y
  `pacman-key --populate archlinuxarm` (las claves de los paquetes), sin probar en la caja. Y `pacman -Syu` actualizaría
  `linux-aarch64`, que aquí no arranca: habría que ponerlo en `IgnorePkg` o quitarlo del disco.
- **512 MB para el invitado**: con QEMU `virt` no se llega (arriba). Hace falta que el kernel de DashOS use la RAM por
  encima de los 4 GB de direcciones físicas.
- **Que lo escrito se quede**: la prueba usa el disco con `snapshot=on`. Falta una prueba de dos arranques seguidos en
  los que Arch vea lo que escribió en el primero.
- **El disco va de una en una**: cada petición del invitado para la vCPU hasta que el servicio de disco contesta. Con
  varias vCPU (no hechas) haría falta un hilo de E/S y el anillo del servicio de disco (U6l), que ya existe.
- **La entrada como root sin contraseña** en la terminal de la caja: decisión para el usuario (¿un usuario normal con
  `sudo`, o pedir contraseña?).
- La Pi 4: no tiene servicio de disco NVMe; ni probado.

### Decisiones para el usuario

1. ¿La terminal de la caja entra sola como root (como ahora), como un usuario normal, o pide contraseña?
2. ¿Se quita `linux-aarch64` (y sus 57 MB de Image, initramfs y módulos) del disco de la caja, ya que la caja usa su
   propio kernel?

## 2026-11-12: varias vCPU (rama `caja-linux-vcpu`)

Rama `caja-linux-vcpu`, sacada de `fusion-fondo` (be30e2b), con todo lo de las cajas. Sala limpia: especificaciones de
Arm (GICv2 IHI 0048B, cap. 4 y 5; PSCI DEN 0022, CPU_ON, CPU_OFF y AFFINITY_INFO; el Arm ARM para HCR_EL2.FB, las
TLB y VMPIDR_EL2). Nada de código de Linux, KVM, QEMU, crosvm, Firecracker ni Xen.

### Resultado

`make arranque-virt-linux`: el invitado tiene 4 vCPU, Linux las enciende por PSCI y las usa todas.

```
[caja-linux] linux: [    0.137281] CPU1: Booted secondary processor 0x0000000001 [0x410fd083]
[caja-linux] linux: [    0.142137] CPU2: Booted secondary processor 0x0000000002 [0x410fd083]
[caja-linux] linux: [    0.145041] CPU3: Booted secondary processor 0x0000000003 [0x410fd083]
[caja-linux] linux: [    0.145854] smp: Brought up 1 node, 4 CPUs
[caja-linux] linux: caja-linux: cpus 4, nproc 4, en línea 0-3
[caja-linux] linux: caja-linux: paralelo 4 trabajos: seguidos 1.18 s, a la vez 0.35 s
[caja-linux] linux: caja-linux: irq IPI0:        29         29         26         32       Rescheduling interrupts
[caja-linux] linux: caja-linux: irq IPI1:       631        120         88        624       Function call interrupts
[caja-linux] linux: se para la máquina a los 145703 ms: Linux apagó la máquina por PSCI
[caja-linux] linux: 134901 salidas: 99081 a aparatos (0 abortos), 10 PSCI, 17056 WFI (563047 ms dormido), 18738 por el reloj u otra vCPU, 16 registros; 14682 interrupciones inyectadas, 12207 patadas entre vCPU
[caja-linux] linux: hilos de las 4 vCPU unidos al acabar: 4
```

Los 563 s dormidos en 146 s de máquina son las 4 vCPU paradas en WFI casi todo el rato (la shell espera teclas): los
hilos duermen en el kernel, no giran.

### El kernel

- **La vCPU, objeto aparte** (`base/hiper.rs`). Lo que era una `Vcpu` con todo se parte en dos:
  - `Espacio`: la etapa 2 (sus tablas sobre los objetos de memoria), el VMID, la GICV proyectada y lo cobrado. Lo
    comparten todas las vCPU de una máquina (`Arc`) y se suelta con la última (entonces OLVIDAR y se libera el VMID).
    `anadir` y `proyectar_gicv` van con su cerrojo y valen con las vCPU en marcha: solo se añaden entradas que no
    había (una tabla nueva se enlaza ya a cero, tras una barrera).
  - `Vcpu`: su mundo (una página), su MPIDR (Aff0 = su número, de 0 a 7: lo que direcciona el GICv2), su GIC virtual
    (LR, VMCR, APR), su temporizador virtual (en el mundo) y su FPU. `reiniciar` la deja como recién encendida por
    CPU_ON: en su entrada, con x0 = el contexto, EL1h con DAIF enmascarado, la MMU apagada, los registros de EL1 a cero
    y sin FPU propia.
- **Dos vCPU de la misma máquina en el mismo núcleo** comparten etiquetas en la TLB (mismo VMID). Cada núcleo apunta la
  última vCPU que corrió en él; si ahora corre otra de la misma máquina, EL2 hace `tlbi vmalls12e1` local antes de
  entrar (una palabra nueva en el mundo, `C_LIMPIAR`). Lo que invalida el invitado ya va a todos los núcleos
  (HCR_EL2.FB).
- **Llaves** (`maquinas.rs`). Cada llave de máquina es una vCPU: `maquina_crear` da la 0 y `maquina_estado` 4 da las
  demás (número, entrada, x0), con la memoria y la etapa 2 compartidas. `Comun` (la parte de la máquina) lleva si está
  muerta y la lista de vCPU vivas para sacarlas a todas. Un fallo de una vCPU mata la máquina entera.
- **Sacar a una vCPU que corre en otro núcleo** (`Patada`). Patear (`maquina_estado` 6) pone "sal en cuanto puedas" y
  mira en qué núcleo está dentro del invitado; si está, le manda la SGI 2 (`gic::SGI_VCPU`, nueva: el kernel solo la
  termina), que la saca por IMO. Si duerme en un WFI, la despierta. La vCPU, antes de entrar, apunta su núcleo y luego
  mira la petición (con orden total, `SeqCst`): o ve la petición y no entra, o el que patea ve su núcleo; y como las
  interrupciones del kernel están enmascaradas desde ahí hasta el `eret`, una SGI que llegue en medio queda pendiente y
  la saca nada más entrar. Parar (`maquina_estado` 7) marca la máquina muerta y patea a todas.
- **Un WFI que espera de verdad**: con `ESPERAR_WFI` en el bloque de interrupciones, un WFI sin nada pendiente en los
  registros de lista duerme el hilo en el kernel (`bloquear_soltando_hasta`, sin el cerrojo de la vCPU) hasta el plazo
  de su temporizador virtual (si su distribuidor lo deja pasar), una patada o 100 ms. Si venció el temporizador, entra
  otra vez sin pasar por el servicio (lo inyecta el kernel con su bit HW). El informe crece a 13 palabras: lo que
  durmió.

### El servicio

- `Caja` es ahora de todas las vCPU: la máquina del crate (aparatos y distribuidor), PSCI, las llaves, las cuentas y
  cómo acabó, dentro de un cerrojo (`Compartida`, con el `hilos::Cerrojo` de futex). Cada vCPU corre en su hilo
  (`Sistema::correr_vcpu`; la 0 en el del servicio), con lo suyo aparte (`VcpuCaja`: su bloque de interrupciones y qué
  hay en cada registro de lista).
- PSCI: CPU_ON apunta la entrada y el contexto y despierta el hilo de esa vCPU, que la reinicia y la corre; CPU_OFF
  devuelve sus registros de lista al distribuidor y el hilo se queda esperando otro CPU_ON; AFFINITY_INFO, del crate.
  Una CPU_ON a una vCPU que no existe da INVALID_PARAMETERS; dos veces, ALREADY_ON u ON_PENDING. Si se apagan todas, la
  máquina acaba.
- Las SGI entre vCPU y las SPI dirigidas: tras cada escritura en un aparato o en el distribuidor (GICD_SGIR,
  ITARGETSR...) y tras sondear los aparatos, se mira qué otras vCPU tienen algo nuevo y se patean (una vez: hasta que
  llenan sus registros de lista no se las vuelve a patear). Las teclas y las tramas de la red patean a la vCPU 0, que
  sondea los aparatos.
- El árbol de dispositivos con 4 CPU (`enable-method = "psci"`) y el distribuidor para 4.
- Al acabar (SYSTEM_OFF, un fallo, el plazo), se para la máquina, los hilos de las vCPU salen y se unen
  (`hilos::unir`): ninguno se queda colgando, y sus pilas vuelven al montón.

### Pruebas nuevas

En `arranque-virt-vm` (y `arranque-virt-linux`), `usuario/invitados/smp.s`, dos vCPU sin sistema:

- **PSCI y SGI**: CPU_ON a una vCPU que no existe (rechazada), a la 1 y otra vez a la 1 (rechazada), AFFINITY_INFO,
  y cien idas y vueltas de SGI (la 0 manda la SGI 1, la 1 contesta con la 2).
- **Hostil, tormenta de SGI**: la 1 manda la SGI 3 a la 0 sin parar mientras la 0 trabaja; la 0 acaba con HVC y la
  máquina se para con la tormenta dentro. Los dos hilos se unen y la vCPU 1 ya no entra (`E_CERRADO`).
- **Hostil, una vCPU en bucle**: la 1 en un bucle sin fin con todo enmascarado; la 0 trabaja lo mismo que sola y apaga
  la máquina por SYSTEM_OFF con la 1 dentro.
- `maquina_vcpu` con un número que ya está o fuera de rango: `E_ARGUMENTO`.

```
[caja-linux] varias vCPU: PSCI (CPU_ON a una que no existe y dos veces a la misma, rechazados; AFFINITY_INFO), 100 idas y vueltas de SGI entre dos vCPU: 212 µs de media
[caja-linux] hostil (tormenta de SGI): 6841 recibidas mientras trabaja (el trabajo, 3946 µs sola y 440593 µs con la tormenta); 364 patadas; se para con la tormenta dentro, 2 hilos unidos: bien
[caja-linux] hostil (una vCPU en bucle): la otra trabaja (3992 µs sola, 4133 µs con ella en bucle) y apaga la máquina con ella dentro; 2 hilos unidos: bien
```

En Linux, el `/init` (`tools/caja_linux_invitado.py`) dice ahora `nproc` y las CPU en línea, y con `caja.paralelo` en
la línea de órdenes (la caja lo pone) corre un bucle de la shell (20 000 vueltas) cuatro veces seguidas y cuatro a la
vez, con lo que tarda cada tanda por el reloj del invitado. `arranque_virt_linux.txt` solo pide `cpus 4`, que vale
también con el initramfs de antes; para ver el trabajo en paralelo hay que rehacerlo
(`DASHOS_CAJA_LINUX_CACHE=... tools/caja_linux_invitado.py`, función `initramfs`; yo usé
`~/.cache/dashos-caja-linux-vcpu`, para no pisar el de la rama de Arch).

### Medidas

QEMU `virt`, TCG, 4 núcleos, con el PC compartido con otras sesiones. Orientativas: en la Pi serán otras.

| Medida | Valor |
|---|---|
| Ida y vuelta de una SGI entre dos vCPU (la 0 a la 1 y la 1 a la 0), 100 veces | 209 a 242 µs con el PC tranquilo (carga 3 a 4, cinco tandas); 1,2 ms en una tanda; 5,2 y 6,3 ms con carga 6 a 7 |
| De una patada a que el hilo de la vCPU dormida en un WFI vuelve a correr (medido en el kernel, una tanda) | 10 µs de media, 48 µs como mucho, 200 patadas |
| Linux: el mismo trabajo 4 veces seguidas y 4 a la vez, con 4 vCPU | 1,18 s y 0,35 s (3,4 veces); 1,2 s y 0,33 s (3,6 veces) |
| La 0 trabaja sola o con la 1 en un bucle sin fin con todo enmascarado | 3,9 a 4,2 ms en los dos casos |
| La 0 trabaja sola o bajo una tormenta de SGI de la 1 | 4 ms sola; 0,44 a 0,97 s con la tormenta (unos 65 µs por SGI recibida) |
| Linux, hasta apagar (con la terminal y el guion) | unos 146 s, como con una vCPU; los 4 hilos, casi todo el rato dormidos en WFI |

Cada SGI cuesta varias salidas: la escritura en GICD_SGIR de quien la manda, la patada a la otra, su vuelta para
llenar los registros de lista y la interrupción de mantenimiento cuando la termina. Por eso una tormenta frena mucho a
la vCPU que la recibe, aunque no la para ni a ella ni al sistema.

### Pruebas

- `make prueba` (BIEN: esperado.txt y el cargador hunk), `make arranque-virt`, `make arranque-virt-2`,
  `make arranque-virt-vm` y `make arranque-virt-linux` (con el initramfs compartido de siempre y con el nuevo): BIEN.

### Qué falta

- Una tormenta de SGI frena a la vCPU que la recibe de 100 a 240 veces (unos 65 µs por SGI). Se podría quitar la interrupción
  de mantenimiento de las SGI (mirar sus LR solo en la siguiente salida) o entregar varias fuentes de una vez.
- El servicio lleva todo el distribuidor y los aparatos bajo un solo cerrojo: con muchas salidas a aparatos desde
  varias vCPU a la vez, se esperan unas a otras. Para la consola temprana y virtio no se ha notado.
- CPU_SUSPEND contesta en el acto (no espera como un WFI); Linux no lo usa con este árbol (sin estados de reposo).
- `maquina_memoria` y la GICV con vCPU corriendo están hechas para no necesitar vaciar las TLB, pero no hay una prueba
  que añada memoria con el invitado en marcha.
- La FPU de cada vCPU (528 bytes) sigue sin cobrarse al grupo; ahora es una por vCPU.
- La Pi 4: nada de esto se ha probado en hardware (la SGI 2 y el vaciado de la TLB son los mismos en el GIC-400).
- El número de vCPU del invitado es fijo (4); habría que sacarlo de los núcleos de la máquina (como la memoria) y
  dejar que lo elija el usuario.

## 2026-11-12: ventanas de Linux en el escritorio de DashOS (rama `caja-linux-wayland`)

Rama `caja-linux-wayland`, sacada de `revision` (1e899c4), con `revision` fusionada después (las 4 vCPU, 69fb1ac). El
hito H9 pedía también "ventanas por un intermediario de Wayland": que un programa gráfico de Linux salga como ventana
del escritorio de DashOS. Sala limpia: la especificación de virtio 1.2 (§5.10, vsock) y los XML de Wayland; nada de
código de Linux, QEMU, crosvm (ni su sommelier ni virtio-wl), Firecracker ni Xen.

### Resultado

`make arranque-virt-wayland`: Arch Linux ARM arranca en la caja como en `arranque-virt-arch`; el puente de Wayland
(un servicio de systemd) conecta con DashOS por vsock; un programa de Linux con libwayland (`ventana_wayland`) abre su
ventana, que sale en el escritorio de DashOS con lo que pinta, y le llegan un clic y una tecla. Después, foot (una
terminal de Wayland de verdad, de los paquetes de Arch Linux ARM) abre la suya y lo que se teclea en ella llega a su
shell.

```
[caja-linux] wayland: vsock en el puerto 5000 para el puente de Wayland de Linux (CID 3)
[caja-linux] linux: [  OK  ] Started Puente de Wayland de DashO…en el escritorio de DashOS (vsock).
[caja-linux] wayland: el puente de Linux conecta por vsock (transporte 1)
[caja-linux] linux: ventana_wayland: conectada a /run/wayland-0
[compositor] ventana de caja-linux (un programa de su puente de Wayland) en (168, 208)
[caja-linux] linux: ventana_wayland: mapa de teclado xkb de 73092 bytes
[caja-linux] linux: ventana_wayland: primer cuadro en pantalla a los 50 ms
[caja-linux] linux: ventana_wayland: tengo el teclado
[caja-linux] linux: ventana_wayland: clic del botón 272 en (160, 120)
[caja-linux] linux: ventana_wayland: tecla 30 pulsada
[caja-linux] linux: ventana_wayland: en marcha: 353 cuadros en 5009 ms, 70.4 por segundo
[caja-linux] linux: ventana_wayland: al acabar: 201 cuadros en 2679 ms, 75.0 por segundo
[caja-linux] linux: ventana_wayland: 2 teclas y 1 clics; adiós
[caja-linux] linux: foot: lista
[compositor] ventana de caja-linux (un programa de su puente de Wayland) en (107, 132)
[caja-linux] linux: foot: leido hola
[caja-linux] linux: se para la máquina a los 62485 ms: Linux apagó la máquina por PSCI
[caja-linux] wayland: 6 transportes (3 cortados por tramas malas), 36 clientes de Linux, 38 KB de Wayland al compositor y 19 KB al invitado, 170846 KB de búferes copiados, 33 memorias, 346 cosas malas tiradas, 2 clientes cerrados
```

Capturas (fuera de git): `capturas-agente/caja-linux-wayland.png` (la ventana de `ventana_wayland` encima de la
terminal de la caja, con el cuadrado amarillo del clic y el color cambiado por la tecla; la barra de DashOS dice su
título, "Hola desde Linux") y `capturas-agente/caja-linux-wayland-foot.png` (foot).

### Cómo va

```
programa de Linux ──libwayland──▶ puente (Arch, /run/wayland-0) ──vsock──▶ aparato vsock (caja) ──▶ puente_caja
      ▲  memfd de wl_shm                 │ copia lo dañado                                   │ subconexiones
      └──────────── eventos ◀────────────┴──────────────── canal de respuesta ◀──────── compositor de DashOS
```

- **El transporte: virtio-vsock** (`caja-linux/vmm/src/virtio/vsock.rs`). Es lo que la norma tiene para hablar con el
  anfitrión sin red, Linux lo trae (`CONFIG_VIRTIO_VSOCKETS`, añadido a `Image-arch`) y el puente lo usa como un socket
  de flujo cualquiera. El aparato: tres colas, cabecera de 44 bytes, solo flujo, solo conecta el invitado (CID 3) a
  DashOS (CID 2), crédito como dice la norma (256 KB por conexión; el crédito liberado se anuncia cuando es mucho o
  cuando el invitado ya casi no tiene, no tras cada paquete) y hasta 64 conexiones. Lo que el invitado manda sin
  crédito, con un CID falso, un tipo que no es de flujo, un largo mayor que lo que trae o que 64 KB, o una orden que no
  existe, corta su conexión (RST) o se tira y se cuenta; el aparato nunca pasa a DEVICE_NEEDS_RESET por eso. Va bajo el
  cerrojo de la caja, como los demás aparatos: lo pueden tocar las 4 vCPU.
- **Por qué copiar y no compartir la memoria.** Los búferes de `wl_shm` son `memfd` del programa, páginas sueltas de
  la RAM del invitado, y el compositor lee un pool como un objeto de memoria de DashOS seguido. Para no copiar habría
  que darle al compositor una vista de páginas sueltas de la RAM del invitado (un objeto de memoria hecho de trozos de
  otro), que el kernel no tiene, y fiarse de un invitado que puede cambiar esas páginas mientras se componen. Así que
  el puente de Linux (`puente/src/bin/puente.rs`, el de G1) sigue copiando en cada `commit` solo las filas dañadas
  desde la última vez que mandó ese búfer, y la caja las escribe directamente en el objeto de memoria del pool, al
  vuelo (sin juntar la trama entera): del anillo de vsock a la memoria del compositor hay una copia.
- **El lado de DashOS** (`wayland/src/puente.rs`, sin `std`, con sus pruebas en el PC; y `usuario/src/puente_caja.rs`,
  lo del sistema). Cada conexión de vsock al puerto 5000 es un `Puente`. Sus clientes de Wayland son **subconexiones**
  del compositor: trozos nuevos (`canal::TROZO_SUB_CONECTAR`, `_DATOS`, `_ADIOS`) con el número del cliente delante,
  por la llave "ventanas" de la caja. El compositor solo las admite de "caja-linux" (por el nombre que da `inicio`),
  hasta 32 por marca, y cada una es un cliente aparte (su ventana, su teclado, sus errores) **sin ningún poder**: ve
  los globales de un programa cualquiera, nunca las capas ni el bloqueo. Las respuestas vuelven todas por un canal de
  la caja con el número delante; un hilo de la caja las lee y despierta a la vCPU 0 para que el aparato las pase al
  invitado. Cada memoria del puente de Linux es un objeto de memoria de DashOS (hasta 16 MB cada uno, 256 y 24 MB en
  total por transporte; `MARGEN_SERVICIO` sube a 56 MB) del que el compositor recibe una copia de solo lectura; las que
  da el compositor (el mapa de teclado) se copian al invitado como memorias nuevas, y el puente de Linux las convierte
  en `memfd`.
- **Dentro de Arch** (`tools/caja_linux_invitado.py preparar-arch`): el puente, compilado en el PC para
  `aarch64-unknown-linux-musl` (estático, 800 KB), en `/usr/local/bin`, con `dashos-puente.service` (systemd lo
  arranca y lo relanza) y `WAYLAND_DISPLAY=/run/wayland-0` en `/etc/profile.d`. `ventana_wayland`
  (`caja-linux/invitado/ventana_wayland.c`) se compila en el PC con clang contra el libwayland del paquete de Arch.
  Los paquetes (wayland, foot, ttf-dejavu y lo que piden: 14) se bajan de los servidores de Arch Linux ARM en el PC,
  con su suma (la de la base del repositorio) y su firma comprobadas con el anillo de claves que trae el rootfs, y se
  meten en la imagen con su entrada en la base de pacman (`pacman -Q` los ve). Más rápido que `pacman -S` en QEMU sin
  aceleración, y sin necesitar `pacman-key --init` en la prueba.

### Lo que hubo que arreglar por el camino

- **Un objeto de memoria de DashOS no crece.** Un pool de `wl_shm` puede crecer (`wl_shm_pool.resize`; lo hace
  libwayland-cursor con los cursores, y foot). Si cabe en las páginas de su memoria, pasa tal cual. Si no, el puente de
  Linux no manda el `resize`: crea al otro lado una memoria nueva con todo lo del pool y un `wl_shm.create_pool` sobre
  ella con un número de objeto propio (desde 0xfeffffff hacia abajo; el programa cuenta desde 1), y los búferes que el
  programa cree después en ese pool van al nuevo. Los de antes siguen en el viejo, con su memoria; al destruir el pool
  se destruyen también sus sucesores. `ventana_wayland` empieza con un pool de 4 KB que crece (lo prueba siempre).
  Al destruir un sucesor, el programa puede recibir un `delete_id` de un número que no conoce; en las pruebas no ha
  dado problemas, pero el puente aún no lo filtra.
- **Memorias que no se soltaban.** El puente de G1 no soltaba nunca la memoria de un pool de `wl_shm` (con el tope de
  memoria de la caja, se habría llenado). Ahora la suelta cuando el pool está destruido y no le quedan búferes (un
  búfer vive más que su pool), cuando el programa se va y, para las copias de otros descriptores, en cuanto el mensaje
  ha salido.
- **El portapapeles.** foot no arranca sin `wl_data_device_manager` ("no clipboard available"). El compositor lo
  anuncia ahora (v3, número 14) con sus objetos, pero sin guardar selecciones ni mandar ofertas: copiar y pegar entre
  programas sigue pendiente.
- **Los pools enormes de foot.** foot desplaza el texto con un pool de 512 MB; el compositor no admite pools de más de
  64 MB ni el puente memorias de más de 16 MB, así que el cliente moría con "tamaño de pool malo". En la imagen va
  `/etc/xdg/foot/foot.ini` con `max-shm-pool-size-mb=0` (un búfer por cuadro, como cualquier programa).
- **El guion** no sabía teclear "_" ni "&" con el teclado español.

### Pruebas

- `make arranque-virt-wayland` (nueva), con `kernel/pruebas/arranque_virt_wayland.txt`.
- Hostiles:
  - `cargo test` en `caja-linux/vmm` (ahora dentro de `make prueba`): `vsock_conexion`, `vsock_extremo_lento` y
    `vsock_hostil` (CID falso, tipo de paquete, cabecera corta, largos falsos o absurdos, datos sin conexión, pasarse
    del crédito, órdenes que no existen, conexiones repetidas y de más, búferes donde no cabe la cabecera; una conexión
    buena funciona después), y vsock en `a_lo_loco`.
  - `cargo test` en `wayland` (`puente::pruebas`): tramas a trozos de cualquier tamaño, compositor lleno, y hostiles:
    memorias que no existen, escrituras fuera o que desbordan, tamaños de más, demasiadas memorias y clientes, números
    de memoria que no existen en un mensaje (cierra ese cliente), basura del compositor y del transporte al azar.
  - En DashOS, `puente --hostil vsock:5000` desde Arch: escrituras a memorias que no existen y fuera, memorias de 16
    EB y 1 TB, 300 memorias de 1 MB (pasan 24), un cliente con números de memoria falsos, uno repetido, 90 clientes
    (pasan 32), Wayland roto para el compositor (que lo corta: "error de protocolo de caja-linux: mensaje mal
    formado"), y tres conexiones con tramas que no son tramas (tipo, 4 GB, de memoria sin número), que la caja corta.
    Después el programa de verdad funciona.
- Tras fusionar `revision` (fec71bf), seguidos: `make prueba` (BIEN: esperado.txt y BIEN: cargador hunk),
  `arranque-virt-wayland`, `arranque-virt`, `-vm`, `-linux`, `-arch` y `-2`: BIEN, a la primera.

### Medidas

QEMU `virt`, TCG, 4 vCPU, con el PC cargado por otras sesiones.

| Medida | Valor |
|---|---|
| `ventana_wayland` (320 × 240, la ventana entera dañada en cada cuadro: 300 KB por cuadro por vsock) | 65 a 75 cuadros por segundo, unos 20 MB/s de búferes |
| Del arranque del programa a su primer cuadro en pantalla | 50 ms |
| Lo copiado en la prueba entera (`ventana_wayland` 554 cuadros y foot) | 167 MB de búferes, 38 KB de Wayland hacia el compositor y 19 KB hacia Linux |
| `make arranque-virt-wayland` entera, de la primera instrucción de Linux a `poweroff` | 62 s |

Lo que limita los cuadros por segundo en QEMU es la copia (de la memoria del programa al socket de vsock en Linux, y
del anillo a la memoria del compositor), las salidas de la vCPU por cada aviso de vsock y componer por CPU la ventana
entera: no se ha medido cada parte. En la Pi será otro número.

### Qué falta

- ~~**Sin copia**~~: hecho en la rama `caja-linux-sincopia` (sección siguiente).
- **El portapapeles de verdad** (selección, ofertas, `wl_data_offer`) y la selección primaria; arrastrar y soltar.
- **Pools de más de 64 MB** (el tope del compositor) o memorias de más de 16 MB (el de `memoria_crear`): foot va con
  `max-shm-pool-size-mb=0`; un programa que pida un pool así recibe un error de protocolo.
- **Decoraciones**: foot se pinta las suyas (CSD); `ventana_wayland` sale sin borde. Falta `xdg-decoration` para que
  el compositor ponga las de DashOS.
- **GPU**: los programas de Mesa pintan por software (no hay `linux-dmabuf` en el compositor de DashOS ni virtio-gpu
  en la caja).
- **Más programas de verdad**: no se ha probado GTK4 ni Firefox (mucho para QEMU sin aceleración y 256 MB).
- `pacman-key --init` y `--populate` no van en la imagen: los paquetes se meten ya instalados desde el PC. Para instalar
  con `pacman -S` dentro de la caja hay que hacerlo a mano la primera vez.
- Una vez, al acabar la prueba (con Linux ya apagado), el compositor soltó a caja-linux por no leer sus eventos; tras
  fusionar `revision` (fec71bf) no ha vuelto a pasar. Sin mirar.

## 2026-11-12: búferes de Linux sin copia (rama `caja-linux-sincopia`)

Rama `caja-linux-sincopia`, sacada de `revision` (0ce1f49). El compositor lee los búferes de `wl_shm` de los
programas de Linux en la RAM del invitado, sin copiarlos. Sala limpia: la documentación de `/proc/self/pagemap`,
`mlock(2)` y `fallocate(2)` (páginas de manual) y los XML de Wayland; nada de código de Linux, QEMU, crosvm,
Firecracker ni Xen.

### Resultado

```
[caja-linux] linux: puente: DashOS lee los búferes sin copia
[caja-linux] linux: puente: cliente 1: primer cuadro sin copia
[caja-linux] linux: ventana_wayland: medida de 320 x 240: 746 cuadros en 8003 ms, 93.2 por segundo, 27963 KB/s de búferes
[caja-linux] linux: ventana_wayland: medida de 1280 x 720: 273 cuadros en 8025 ms, 34.0 por segundo, 122466 KB/s de búferes
[caja-linux] linux: puente: cliente 4: 273 cuadros (273 sin copia), 0 bytes copiados, 0 por cuadro; CPU del puente 1335 ms, 4893 µs por cuadro
[caja-linux] linux: puente: cliente 5: 87 cuadros (0 sin copia), 328089600 bytes copiados, 3771144 por cuadro; CPU del puente 3668 ms, 42166 µs por cuadro
[caja-linux] wayland: 7 transportes (3 cortados por tramas malas), 41 clientes de Linux, ..., 468600 KB de búferes copiados, 297 memorias (265 sin copia, vistas de la RAM del invitado; 4 vistas no hechas), ...
```

Lo copiado (468 MB) es solo lo de las dos medidas con copia forzada: `ventana_wayland` y foot van sin copia.
Captura (fuera de git): `capturas-agente/caja-linux-wayland-grande.png` (la ventana de 1280 × 720 sin copia, más
grande que la pantalla de 800 × 600).

## 2026-11-12: la tarjeta gráfica para la caja (rama `caja-linux-gpu`)

Rama `caja-linux-gpu`, sacada de `revision` (0ce1f49). El usuario pidió "usar la tarjeta gráfica" para los programas de
Linux. Primero un estudio (`docs/diseño/S25-gpu-caja-linux.md`) y después el primer paso que se prueba en QEMU. Sala
limpia: virtio 1.2 (§5.7), los XML de Wayland, las interfaces públicas de EGL, GBM y libdrm para el programa de prueba y
`docs/spec/hw/v3d.md`; nada de código de Linux, Mesa, QEMU, crosvm ni virglrenderer.

### El estudio, en corto

- **Hoy no hay GPU en ningún lado**: el invitado pinta con llvmpipe/lavapipe, DashOS no tiene Mesa, el servicio `gpu`
  (V3D) nunca se ha probado y QEMU no emula la V3D.
- **Recomendado para la Pi 4: la V3D mediada.** El invitado usa el controlador v3d de Linux y el Mesa de Arch tal
  cual; sus accesos a los registros de la V3D salen a DashOS, que le deja todo menos la MMU, el GMP y la energía. La
  V3D ve siempre una tabla de páginas sombra, hecha por DashOS con solo páginas de la RAM de esa caja (sin IOMMU en la
  BCM2711 es lo único que la confina). La meta, cuando DashOS tenga su servicio de v3d: contexto nativo por
  virtio-gpu (falta el lado de Mesa, que no existe para v3d). Venus/virgl necesitan Mesa portado a DashOS; el paso
  directo sin trampas, nunca.
- Lo que pide al kernel y al servicio `gpu`, y lo que solo se puede ver en una Pi 4: en S25.

### Resultado

`make arranque-virt-gpu` (nueva; con `ARCH_IMAGE` y `ARCH_DISCO` de `preparar-arch`): la caja tiene una virtio-gpu en
2D además de vsock. Linux la ve (`virtio_gpu`, `/dev/dri/card0` y `renderD128`). Un programa de GL de Linux pinta por
el puente de Wayland y por la pantalla de la virtio-gpu, que sale en una ventana de DashOS ("Linux (pantalla de la
caja)") mientras Linux la usa y se quita cuando la apaga.

```
[caja-linux] gpu: virtio-gpu en 2D con una pantalla de 640x480 y 8 MB para recursos
[caja-linux] linux: [    1.608179] [drm] Initialized virtio_gpu 0.1.0 for a000800.virtio on minor 0
[caja-linux] linux: cuadros_gl: wayland: GL_RENDERER llvmpipe (LLVM 23.1.4, 128 bits), GL_VERSION OpenGL ES 3.2 Mesa 26.2.6-arch1.1
[caja-linux] linux: cuadros_gl: wayland: al acabar: 189 cuadros en 20051 ms, 9.4 por segundo; adiós
[caja-linux] linux: cuadros_gl: wayland: al acabar: 191 cuadros en 15095 ms, 12.7 por segundo; adiós      (intervalo 0)
[caja-linux] linux: 118 frames in 5.0 seconds = 23.572 FPS                                                (es2gears_wayland)
[caja-linux] linux: real	0m9.459s                                                                      (vkcube, 100 cuadros)
[caja-linux] gpu: Linux muestra algo en su pantalla (640x480 desde (0, 0), formato 2)
[caja-linux] gpu: la pantalla de Linux, en una ventana de 640x480
[caja-linux] linux: cuadros_gl: kms: al acabar: 387 cuadros en 20780 ms, 18.6 por segundo; adiós
[caja-linux] gpu: Linux apagó su pantalla; se quita su ventana
[caja-linux] gpu: 1170 órdenes (0 con error, 0 malas), 2 recursos creados, 387 transferencias (464400 KB), 387 vaciados a la pantalla (18 por segundo mientras pintó), 388 cuadros en la ventana
```

Capturas (fuera de git): `capturas-agente/caja-linux-gpu-wayland.png` (`cuadros_gl` por Wayland),
`caja-linux-gpu-gears.png` (`es2gears_wayland`, con los bordes de libdecor) y `caja-linux-gpu-kms.png` (el mismo
programa por KMS, en la ventana de la pantalla de la caja).

### Cómo va

```
programa de Linux ── memfd ──▶ puente (Arch): fallocate + mlock + /proc/self/pagemap
                                    │ MEMORIA_VISTA: n.º, tamaño y la dirección física de cada página
                                    ▼
                         puente_caja: cada dirección → (trozo de 16 MB, página) de la RAM del invitado
                                    │ llamada 50, memoria_vista(trozos de la RAM, páginas)
                                    ▼
                         compositor: una memoria más, proyectada solo para leer
```

- **El kernel: la llamada 50, `memoria_vista`** (`docs/referencia/kernel.md`). Hace un objeto de memoria con las
  páginas que se le piden de otros (los orígenes, llaves de memoria con derecho de leer), en ese orden; los números
  de página cuentan seguidas las de los orígenes. La llave nueva solo lee y transfiere; la vista no se proyecta para
  escribir ni ejecutar, el kernel no escribe por ella y no vale como RAM de una máquina. Guarda los orígenes vivos
  (`Memoria::origenes`): sus páginas no vuelven al sistema hasta soltar la vista, así que nunca se ven páginas ya
  dadas a otro. Ni vistas de vistas (no hay cadenas que soltar), ni memoria DMA (va sin caché), ni llaves revocables
  (la vista seguiría viendo el objeto tras revocarla). Cobra solo su lista (256 bytes + 8 por página y por origen);
  las páginas siguen cobradas a quien creó cada origen. Hasta 64 orígenes y 4096 páginas (16 MB). Sin `unsafe` fuera
  de `kernel/src/base/`.
- **Dentro de Linux: dónde está el pool.** Los pools los crea cada programa (libwayland, foot, los toolkits) con
  `memfd_create`: el puente no decide dónde quedan. Reservar memoria que el invitado sepa seguida obligaría a cambiar
  los programas (o a copiar dentro del invitado, otra copia), así que el puente averigua las páginas: proyecta el
  pool, rellena los huecos del memfd sin cambiar su contenido (`fallocate` con `FALLOC_FL_KEEP_SIZE`), lo fija
  (`mlock`: Linux ya no lo saca ni lo mueve) y lee `/proc/self/pagemap` (el número de página física solo se ve siendo
  root; el puente es un servicio de systemd que corre como root). Las páginas no son seguidas: van como lista. Dos
  páginas en la misma física (la página de ceros) o una que no se vea: con copia. El kernel de la caja (tinyconfig)
  no tiene compactación ni swap; aun así, el puente pone `compact_unevictable_allowed` a 0 si existe y, en cada
  commit, comprueba con `pagemap` que el búfer sigue en las páginas que mandó. Si no (Linux las movió), cierra el
  programa: su ventana enseñaría lo que haya después en las viejas. Al crecer un pool, la proyección nueva se fija
  antes de quitar la vieja.
- **El protocolo** (`wayland/src/puente.rs`): `VISTAS` (DashOS → invitado, al conectar: se admiten vistas),
  `MEMORIA_VISTA` (número, tamaño, n y n direcciones físicas del invitado) y `VISTA_NO` (no se pudo: en su lugar hay
  una memoria normal del mismo tamaño, y el puente copia como antes). Sin `VISTAS` (el compositor del PC de G0/G1), el
  puente copia. Una vista no se escribe y no cuenta en el tope de 24 MB de memoria copiada (no toma memoria nueva);
  sí en el de 256 memorias por transporte.
- **El lado de DashOS** (`usuario/src/puente_caja.rs`): `abrir` recibe las llaves de la RAM del invitado (los trozos
  de 16 MB) y su dirección física. Cada dirección se comprueba: alineada, desde la base y dentro de la RAM; se pasa a
  número de página de los trozos y se pide la vista. El compositor la recibe como cualquier memoria (una copia de su
  llave de solo lectura) y no cambia nada.
- **Seguridad.** El invitado solo puede nombrar páginas de su propia RAM (los orígenes son los trozos de su máquina):
  si nombra otras, su ventana enseña lo que ya era suyo. El compositor solo puede leer. Si el invitado cambia el búfer
  mientras se compone, solo se estropea su ventana (los píxeles son bytes cualesquiera; tamaños, formato y desplazamientos
  van por el protocolo y el compositor ya los comprueba contra lo que mide la memoria). Una vista mantiene viva la
  RAM del invitado hasta que el compositor la suelta (al irse el cliente): no se ve memoria liberada y reusada.
- **Para medir**: con `/run/dashos-puente-copia`, los programas que conectan después van con copia.
  `ventana_wayland ANCHO ALTO SEGUNDOS` pinta de ese tamaño y acaba sola, con una línea "medida". El guion mide las
  cuatro combinaciones y saca con `journalctl` lo que dijo el puente, con su tiempo de CPU por cliente.

### Medidas

QEMU `virt`, TCG, 4 vCPU, la ventana entera dañada en cada cuadro, 8 s cada una, en la misma ejecución (el PC,
cargado por otras sesiones). CPU del puente: la de su proceso (usuario y sistema) mientras estaba ese cliente.

| Ventana | Con copia | Sin copia | Cambio |
|---|---|---|---|
| 320 × 240, cuadros por segundo | 61,4 (18 MB/s) | 93,2 y 94,6 (28 MB/s) | +52 % |
| 320 × 240, CPU del puente por cuadro | 7,3 ms | 3,9 y 4,2 ms | −45 % |
| 1280 × 720, cuadros por segundo | 10,8 (38 MB/s) | 34,0 (120 MB/s) | ×3,1 |
| 1280 × 720, CPU del puente por cuadro | 42,2 ms | 4,9 ms | −88 % |

Sin copia, la CPU del puente ya no depende del tamaño de la ventana: lo que queda es pasar los mensajes de Wayland y
leer `pagemap` (unas pocas páginas por commit). Lo que limita ahora en QEMU es componer por CPU y pintar el propio
programa. Antes (la bitácora anterior), 65-75 cuadros por segundo de 320 × 240 con copia.

### Pruebas

- `make prueba`: "H9: vistas de memoria con páginas de otro objeto, solo de lectura: bien" (del kernel: el orden de
  las páginas; lo cobrado; páginas fuera, lista vacía o número enorme; una vista como origen; un cupo que no llega, sin
  dejar nada cobrado; solo leer, ni escribir ni ejecutar; el origen soltado con la vista viva: las páginas libres no
  cambian y la vista las sigue viendo; al soltar la vista vuelven las 3 páginas y el cupo queda a cero) y `malo`,
  paso 39 (lo que ve un proceso: la vista lee en orden y ve lo que escribe el dueño; no se proyecta para escribir ni
  pidiendo el derecho al duplicar, ni para ejecutar; páginas fuera; una llave que no existe, un canal, una sin derecho
  de leer, una revocable y una vista de origen; tablas sin nada, de más, en memoria vacía, en el kernel o que
  desbordan; la lista más larga, 4096 páginas y 64 orígenes; vistas de 16 MB sin cerrar hasta que el cupo o la tabla
  dicen basta; y el origen soltado: la memoria que se pide después, llena de otra cosa, no cambia la vista). Su
  grupo queda a cero (lo comprueba el lanzador). BIEN: esperado.txt y BIEN: cargador hunk.
- `cargo test` en `wayland`: `puente::pruebas::vistas` (vistas buenas, fuera de la RAM, sin alinear o del kernel, que
  dan memoria normal y `VISTA_NO`; mal hechas: tamaño 0 o de más, listas que no casan, cortas o de 4 mil millones; de
  más de 256), y `a_lo_loco` con los tipos nuevos.
- `puente --hostil vsock:5000` desde Arch, tanda "vistas": direcciones 0, al final del espacio, sin alinear y más
  allá de la RAM (4 `VISTA_NO`), listas que no casan con el tamaño o enormes, 1 TB, 300 vistas de una página propia
  (pasan las que caben), escribir en una vista y un cliente que la usa. La conexión sigue y el programa de verdad
  funciona después.
- `make arranque-virt-wayland` con las líneas nuevas (sin copia, primer cuadro sin copia, las medidas): BIEN.
  Después, seguidos y a la primera: `arranque-virt` (las cinco BIEN), `-vm`, `-linux`, `-arch`, `-2` y otra vez
  `make prueba`: BIEN.

El disco de Arch de esta rama va en `~/.cache/dashos-caja-linux-sincopia` (`DASHOS_CAJA_LINUX_CACHE`, con el rootfs y
los paquetes enlazados a los de siempre), para no pisar el de otras sesiones: `make arranque-virt-wayland
ARCH_DISCO=...`.

### Qué falta

- Lo mismo para los búferes de dmabuf (G6), que siguen con copia, y para lo que va del compositor al invitado (el
  mapa de teclado, una vez).
- Probarlo en la Pi 4 (cachés de verdad: el invitado escribe con su caché y el compositor lee con la suya, las dos
  normales; debería ir, sin probar).
- Si Linux moviera páginas de un pool (con otro kernel: compactación, CMA, memoria que se quita en caliente), hoy se
  cierra el programa. Con un kernel que fije las páginas para siempre (como hacen los aparatos con DMA) no haría falta.
- Las vistas no cuentan en el tope de memoria del transporte: el coste es de su lista (unos 33 KB por vista de 16 MB,
  cobrados a caja-linux).

cuadros_gl kms ──GBM/EGL (llvmpipe)──▶ búfer "dumb" de virtio_gpu (páginas del invitado)
      │ drmModePageFlip                       │ TRANSFER_TO_HOST_2D + RESOURCE_FLUSH
      ▼                                       ▼
 controlador virtio_gpu de Linux ──virtio-mmio──▶ aparato Gpu (caja-linux/vmm) ──▶ ExtremoPantalla ──▶ ventana de DashOS
```

- **El aparato** (`caja-linux/vmm/src/virtio/gpu.rs`, sin `unsafe`): dos colas (control y cursor), una pantalla,
  recursos 2D en la memoria de la caja con un presupuesto (8 MB, `MARGEN_SERVICIO` sube de 56 a 66 MB), respaldo
  (`ATTACH_BACKING`, como mucho 16384 trozos, cada uno comprobado en la RAM del invitado), transferencias, `SET_SCANOUT`
  y `RESOURCE_FLUSH`, que pasa lo cambiado a un `ExtremoGpu`. Vallas: se contesta al acabar cada orden, con la valla
  repetida. No se ofrece 3D, EDID ni *blob*. Cualquier orden mala contesta su error y se cuenta; solo una cola mal hecha
  deja el aparato en DEVICE_NEEDS_RESET (como los demás).
- **La ventana** (`usuario/src/pantalla_caja.rs`): el vaciado convierte lo dañado a XRGB en la memoria de un búfer de
  `wl_shm` y apunta el daño; la ventana va en la misma conexión que la terminal de la caja (su hilo la atiende en cada
  vuelta: un `commit` por cada aviso de frame, con los vaciados de en medio juntos).
- **En Arch** (`tools/caja_linux_invitado.py`): `Image-arch` con DRM y `virtio_gpu` (sin consola de fbdev: la ventana
  solo sale cuando un programa usa la pantalla), `UDMABUF` y `SYNC_FILE`; los paquetes `mesa-utils`, `vulkan-swrast` y
  `vulkan-tools` (47 con lo que piden, 350 MB) y `cuadros_gl` (`caja-linux/invitado/cuadros_gl.c`: GLES2, 640 × 480,
  por Wayland o por KMS con GBM y `drmModePageFlip`).
- **La tabla sombra de la V3D** (`caja-linux/vmm/src/v3d.rs`): la lógica del camino de la Pi, probada en el PC. La
  página de la MMU del hub (con sus zonas sin traducir, `MMU_BYPASS_*`) y el GMP no se dejan tocar; el invitado ve sus
  valores y la V3D los seguros (MMU encendida, abortos, relleno de verdad). Al vaciar, la tabla del invitado se lee una
  vez y cada entrada válida se traduce a una página física de su RAM o queda no válida; sin bits de página grande.

### Lo que se vio por el camino

- **Dos conexiones directas al compositor desde un mismo programa se mezclan.** La primera versión abría la ventana de
  la pantalla con su propio `Cliente`; el compositor distingue a los clientes por la marca de la llave "ventanas", así
  que los mensajes de los dos se juntaron y mató a la terminal ("número de objeto nuevo malo"). Ahora la ventana es
  otra superficie de la conexión de la terminal.
- **Mesa por software no da búferes de dmabuf.** Con el Mesa del PC (26.2.6, el mismo que el de Arch Linux ARM),
  `es2gears_wayland` con llvmpipe usa `wl_shm` aunque el compositor anuncie `zwp_linux_dmabuf_v1`. Y con una
  virtio-gpu sin 3D, Mesa prueba virgl y zink, falla y cae en llvmpipe. Así que `linux-dmabuf` para búferes de
  virtio-gpu no lo usaría ningún programa de Mesa en QEMU: **no se hizo**. Sirve en la Pi con la V3D (Mesa v3d da
  dmabuf; el puente ya los acepta, G6), y entonces el compositor de DashOS tiene que anunciarlo a la caja.
- **Desgarro**: la ventana de la pantalla tiene un solo búfer y el aparato lo escribe mientras el compositor compone;
  a veces se ve medio cuadro (en la captura de KMS, el borde del triángulo).

### Pruebas

- `cargo test` en `caja-linux/vmm` (dentro de `make prueba`): `tests/gpu.rs` (12: camino normal, vallas, trozos con
  desplazamiento, formatos, quitar un recurso en pantalla, reinicio, cursor, y hostiles: ids 0, repetidos o que no
  existen en cada orden, recursos de 16384² y 65536², desbordes de u32, el presupuesto lleno, respaldos fuera de la
  RAM, que desbordan, de largo 0 o con más entradas de las que trae la orden, transferencias que se salen del respaldo
  o del recurso, desplazamientos de 64 bits que desbordan, órdenes cortas, de 3D o que no existen, sin sitio para la
  respuesta, y a lo loco: 20 × 400 órdenes al azar sin romper el aparato) y `v3d::pruebas` (7: la V3D siempre ve la
  sombra; entradas a la dirección física 0, fuera de la RAM, con bits desconocidos o de página grande; no puede apagar
  la MMU ni tocar las zonas sin traducir, el tope, el derribo ni la tabla del GMP; tablas fuera de su RAM; cambiar la
  tabla sin vaciar no llega a la V3D; solo se escriben las entradas que cambian; y a lo loco).
- Seguidas, sobre 49e42ab, todas BIEN a la primera: `make arranque-virt-gpu`, `make prueba` (esperado.txt y cargador
  hunk), `arranque-virt` (las cinco), `-vm`, `-linux`, `-2`, `-arch` y `-wayland`. Las tres de Arch, con la Image y
  el disco de esta rama (`preparar-arch` con `DASHOS_CAJA_LINUX_CACHE=~/.cache/dashos-caja-linux-gpu`: el kernel con
  `virtio_gpu` y Mesa en el disco); en `-wayland`, la virtio-gpu está y nadie la usa.

### Medidas

QEMU `virt`, TCG, 4 vCPU, el PC cargado por otras sesiones (carga 7 a 9). Tres vueltas de `make arranque-virt-gpu`.

| Medida | Valor |
|---|---|
| `cuadros_gl` 640 × 480 por Wayland (hoy: `wl_shm` por el puente y vsock, esperando el aviso de frame) | 9,4, 10,7 y 12,0 cuadros por segundo |
| Lo mismo sin esperar al compositor (`eglSwapInterval` 0) | 12,7 |
| `cuadros_gl` 640 × 480 por la pantalla de la virtio-gpu (KMS) | 17,3, 18,6 y 20,2 |
| `es2gears_wayland` (300 × 300, por el puente) | 18 a 24 |
| `vkcube --wsi wayland --c 100` (500 × 500, lavapipe), con su arranque | 9,4 s |
| Por cuadro de KMS | 3 órdenes, 1,2 MB transferidos |

El mismo programa va 1,5 a 2 veces más deprisa por la pantalla de la virtio-gpu que por Wayland: por KMS hay una copia
(de las páginas del invitado al recurso, hecha por DashOS) y una conversión a la ventana, sin vsock ni salidas por cada
paquete; por Wayland, llvmpipe copia a `wl_shm`, el puente al socket de vsock y la caja a la memoria del compositor.
Pinta lo mismo la CPU del invitado (llvmpipe): ninguno de los dos caminos usa una GPU, que no la hay en QEMU.

### Qué falta

- **Todo lo de la V3D, en una Pi 4** (S25, camino C): dar la V3D a la caja desde el servicio `gpu`, los registros
  atrapados con `v3d::Mediador`, las páginas físicas de la RAM del invitado (una llamada del kernel, la 51 o siguiente),
  la interrupción, el plazo y el reinicio, el nodo del árbol para el controlador v3d de Linux y `CONFIG_DRM_V3D`. Y
  medir `vkcube` y `es2gears_wayland` con la V3D.
- **Sin copia en la pantalla**: con `memoria_vista` (llamada 50, de `caja-linux-sincopia`) y recursos *blob*
  (`VIRTIO_GPU_F_RESOURCE_BLOB`), la ventana podría leer las páginas del invitado sin copiar.
- `zwp_linux_dmabuf_v1` para la caja en el compositor de DashOS (con el `main_device` del invitado): para la V3D.
- Teclado y ratón en la ventana de la pantalla (virtio-input); otro tamaño de pantalla (el aparato ya avisa con
  `EVENTO_PANTALLA`, falta que la ventana se pueda cambiar de tamaño); doble búfer contra el desgarro.
## 2026-11-13: barra de DashOS, portapapeles y programas grandes (rama `caja-linux-apps`)

Rama `caja-linux-apps`, sacada de `revision` (0ce1f49). Lo que faltaba para que los programas de Linux se usen como los
de DashOS: la barra del compositor para los que no se pintan la suya, copiar y pegar entre Linux y DashOS, y un programa
grande de verdad (GTK4, y Firefox si cabe). Sala limpia: los XML de Wayland (`xdg-decoration-unstable-v1`,
`primary-selection-unstable-v1`, `xdg-shell`); nada de Weston, wlroots, Mutter, KWin ni sommelier.

### Resultado

`make arranque-virt-wayland` (con `revision` fusionada: sin copias, virtio-gpu 2D y Pi 5):

- `ventana_wayland` pide la barra del compositor y sale con la de DashOS (título, X, esquinas redondeadas, sombra); se
  mueve arrastrando la barra y se cierra con la X (`ventana_wayland: el compositor pide cerrar`).
- **Portapapeles en los dos sentidos**: tres clics en foot y Control+Mayúsculas+C; en la terminal de la caja (nativa),
  `echo ` y Control+Mayúsculas+V: `linux: Hola desde foot, en Arch Linux ARM, en DashOS`. La primaria, con el botón
  central: `linux: primaria: Hola desde foot, ...`. Al revés: se selecciona con el ratón en la terminal, se copia y se
  pega en foot: `foot: leido pegame-en-foot`.
- **Hostiles de Linux** (`puente --cliente-hostil`): modo de decoración que no existe, dos decoraciones y un tipo que no
  es UTF-8: el compositor corta esas conexiones. Una fuente que escribe sin fin: la terminal pega 4 KB y corta (la fuente
  ve la tubería cerrada tras 64 KB, la ventana de 128 KB del tubo). Una que nunca cierra: la terminal deja de esperar a
  los 10 s (`pegado (0 bytes)`). Después, otra ventana de Linux sale y se cierra como siempre.
- **GTK4**: en el PC, el `gtk4-widget-factory` de Arch Linux ARM (GTK 4.24, aarch64, con `qemu-aarch64`) sale por el
  puente y la escena con su menú emergente (el compositor del PC y el puente de verdad). **En la caja aún no**: aborta al
  cargar su icono SVG (`Gdk-ERROR ... Loader process exited early`): glycin 4.0.5, el cargador de imágenes de
  gdk-pixbuf, falla en el kernel de la caja ("Opening source: Operation not supported" con `glycin-thumbnailer`, también
  en QEMU sin DashOS con la misma `Image-arch`). bwrap funciona; falta encontrar qué opción del kernel de Linux le falta.
- **Firefox no cabe**: 20 minutos sin ventana con 256 MB (y 768 MB de intercambio en el disco): 7 GB leídos del disco,
  la máquina pasándose páginas. En el PC (qemu-aarch64), su ventana principal sí sale por el puente. Queda fuera del
  guion (CAJA_FIREFOX=1 lo intenta).

### La barra de DashOS (`xdg-decoration`)

- `zxdg_decoration_manager_v1` (v1) para todos los clientes (`wayland/src/decoracion.rs`). El programa pide el modo;
  DashOS da el que pide, y si le da igual (`unset_mode`, o crea la decoración y no dice nada), el suyo. Contesta con
  `configure` y un `xdg_surface.configure`. Errores: modo que no existe, dos decoraciones para una ventana, decoración de
  una ventana con búfer (v1).
- La barra la pinta el compositor con el kit (Material 3): 40 px, el contenedor de superficie (alto si la ventana tiene
  el teclado), el título en Roboto Medium 14 recortado con "…", y la X de cerrar; esquinas de 12 arriba y, al componer,
  también abajo (la superficie del programa se recorta en sus esquinas). La sombra rodea las dos. Se repinta solo si
  cambia el título, el ancho, el teclado o el tema; lo que la letra no tiene sale como "?" (su caché no crece).
- Pulsar en la barra coge la ventana para moverla; en la X, `xdg_toplevel.close`. La ventana nueva se coloca en cascada
  contando la barra, y maximizada deja sitio para ella.

### El portapapeles

- `wl_data_device` de verdad y la selección primaria (`zwp_primary_selection_device_v1`), igual las dos
  (`wayland/src/portapapeles.rs`). Solo pone la selección quien tiene el teclado (a otro se le cancela la fuente); la
  oferta va a quien recibe el teclado justo antes de `enter`, y otra vez si cambia mientras lo tiene. El compositor no
  toca los datos: en DashOS el descriptor de `receive` y `send` es la llave de un extremo de canal, y la pasa de quien
  pega a quien copió. Arrastrar y soltar no está (`start_drag` cancela la fuente).
- Topes: 32 tipos por fuente (más largos de 255 bytes, repetidos o de más se ignoran), 32 ofertas vivas por programa
  (uno que no las destruye deja de recibirlas), 32 fuentes. Un `receive` de una oferta vieja o de un tipo que no se
  ofreció cierra la llave sin molestar a la fuente.
- **Por el puente**: los descriptores del portapapeles son tuberías en Linux y canales en DashOS, y van como **tubos**
  (tramas `TUBO_NUEVO`, `TUBO_DATOS`, `TUBO_FIN` y `TUBO_LEIDO`, del 12 al 15: del 8 al 10 son las vistas sin copia de
  `caja-linux-sincopia`). El lado que tiene un extremo donde hay que escribir lo anuncia; el otro crea su par, da el
  extremo de escribir a su programa con el mensaje y manda lo que lee. `TUBO_LEIDO` es una ventana de 128 KB: quien pega
  despacio frena al que copia (sin ella, 4 MB a un lector lento llenaban lo pendiente y se cortaba el tubo). Topes por
  transporte: 16 tubos, 16 MB por tubo, 256 KB pendientes, 30 s sin moverse. En Linux, `dashos_puente::tubos` (el mismo
  código en el puente y en el compositor del PC); el puente sigue además los objetos que crea el compositor en sus
  eventos (las ofertas), para saber qué descriptores llevan sus peticiones.
- **La terminal de la caja** (nativa) usa `usuario/src/portapapeles_cliente.rs`: selecciona con el ratón (al soltar va a
  la primaria), Control+Mayúsculas+C copia, Control+Mayúsculas+V pega y el botón central pega la primaria. Pega como
  mucho 4 KB y espera como mucho 10 s.

### Emergentes y lo que piden los programas grandes

`wayland/src/emergentes.rs`: `xdg_positioner` y `xdg_popup` (ancla, gravedad, desplazamiento, dar la vuelta y deslizar
si no cabe; agarre con el teclado; un clic fuera manda `popup_done`; `reposition`), `set_window_geometry` (la sombra que
se pinta GTK no cuenta para colocar ni lleva otra del compositor), `move` y `resize` a petición del programa (solo con
un botón pulsado sobre la ventana; al redimensionar, un configure con el estado "redimensionando" sin pisar uno sin
confirmar) y `set_maximized`. Y `configure_bounds` (v4) con lo que cabe en la salida.

### Lo que hubo que arreglar por el camino

- **La imagen de Arch, aparte.** GTK4 y Firefox (195 paquetes, 1,2 GB) van en otra imagen y otra caché
  (`DASHOS_CAJA_APPS=1 tools/caja_linux_invitado.py preparar-arch`: `~/.cache/dashos-caja-linux-apps`, 6 GB);
  `arranque-virt-wayland` la usa (APPS_IMAGE, APPS_DISCO). La de siempre no cambia (4 GB, sus paquetes). En esa imagen:
  GTK 4.24 necesita el glib2 2.90 del repositorio y el rootfs trae el 2.88 (Arch Linux ARM no pone versiones en sus
  dependencias): se ponen al día los paquetes del rootfs que lo nuevo necesita y son más nuevos en el repositorio
  (30, sin systemd, bash ni lo de base); sus ficheros pisan los del rootfs. Y lo que harían los ganchos de pacman, en el
  PC: esquemas de GSettings, caché de fontconfig (con `--sysroot`) y base de tipos MIME.
- **Números que chocaban** (tras fusionar `revision`): la pantalla de la caja (virtio-gpu) usa los objetos 40 a 44 y se
  quedaba con todo evento 0 de un objeto alto (las ofertas del portapapeles, que son del compositor, desde 0xFF000000):
  el portapapeles de la terminal usa del 60 al 63 y las fuentes desde 0x20000000, y la pantalla mira solo sus avisos.
  Las tramas de las vistas sin copia son del 8 al 10; los tubos, del 12 al 15.
- **El guion**: la terminal de la caja tapa casi toda la pantalla; los clics en ella van donde no hay otra ventana
  (el compositor dice ahora lo que mide cada una), foot se sube por su barra y Control+C limpia la línea antes de cada
  paso. El punto y coma, en el teclado español. foot con `LANG=C.UTF-8` (si no, su primera línea es un aviso) y más
  pequeño.
- **La caja se para a los 1500 s**: GTK va antes que los hostiles.

### Pruebas

- `cargo test` en `wayland` (79): `tests_linux.rs` (barra, portapapeles entre dos, primaria, hostiles del portapapeles,
  emergentes y sus hostiles, mover y redimensionar) y en `puente.rs` los tubos, su ventana y sus hostiles. La terminal:
  `texto_entre_celdas`.
- En el PC: `puente/pruebas/h9_portapapeles.sh` (wl-copy y wl-paste por el puente y directos: de Linux a DashOS, al
  revés, de Linux a Linux, la primaria, 4 MB a un lector lento, y los tres hostiles) y `g1.sh`.
- `make prueba`: BIEN: esperado.txt y BIEN: cargador hunk. `arranque-virt` (las cinco BIEN), `-vm`, `-linux`, `-2`:
  BIEN. `arranque-virt-arch` (con la caché de siempre, 4 GB): BIEN. `arranque-virt-wayland` (con la de los programas
  grandes): BIEN, con sus líneas nuevas (la barra, moverla y cerrarla, el portapapeles en los dos sentidos y la
  primaria, los hostiles; GTK solo hasta `gtk: empieza`). Lo apuntado por el guion: "portapapeles: de foot a DashOS,
  bien", "primaria: de foot a DashOS, bien", "portapapeles: de DashOS a foot, bien", "gtk4-widget-factory: sin ventana
  (Gdk-ERROR al cargar su icono)". Resumen de la caja: 48 clientes de Linux, 5 tubos del portapapeles (65642 bytes),
  402 cosas malas tiradas (los hostiles), 6 clientes cerrados.
- Capturas (fuera de git, `capturas-agente/`): `caja-linux-wayland.png` (la barra de DashOS en `ventana_wayland`),
  `caja-linux-wayland-portapapeles.png` (foot con lo pegado desde la terminal), `caja-linux-wayland-foot.png`, y en
  Full HD (`DASHOS_PANTALLA=1920x1080`) las mismas con `-fhd` (esa pasada sale igual salvo una línea: la fuente sin fin
  no llegó a ver su tubería cortada antes de sus 60 s; la terminal sí pegó sus 4 KB y cortó. Sin mirar más); GTK4 y Firefox de Arch Linux ARM en el PC (compositor del
  PC, puente y `qemu-aarch64`): `caja-linux-gtk4-pc-fhd.png`, `caja-linux-firefox-pc-fhd.png`.

### Medidas

QEMU `virt`, TCG, 4 vCPU, con el PC cargado por otras sesiones.

| Medida | Valor |
|---|---|
| Cuadros compuestos mientras se arrastra `ventana_wayland` (320 × 240, que pinta sin parar) por la barra de DashOS | 108 a 122 por segundo (67 a 73 antes de fusionar las vistas sin copia) |
| `ventana_wayland` con la barra, sin copia y con copia (medidas de `caja-linux-sincopia`, ahora con la barra) | 320 × 240: 115 a 128 y 74 por segundo; 1280 × 720: 41,6 y 13,4 |
| Lo mismo con foot (quieto): lo marca el ratón del guion (un movimiento cada 50 ms) | 17 por segundo |
| GTK4 hasta su primera ventana | En la caja, no sale (aborta antes); en el PC con qemu-aarch64, unos segundos |
| Firefox hasta su primera ventana | Más de 20 minutos con 256 MB: no sale |
| Redimensionar | Sin medir: lo pide GTK con `xdg_toplevel.resize` (probado en `tests_linux.rs`), y GTK no abre en la caja |

### Qué falta

- **GTK4 en la caja**: encontrar qué le falta al kernel de la caja para glycin ("Operation not supported" al abrir la
  imagen; con strace en la caja). Después, medir su arranque, su menú emergente y redimensionar.
- **Firefox**: necesita más RAM que los 256 MB del invitado con 3 GB en la máquina (con 4 GB serían 512, pero QEMU
  `virt` con `highmem=off` no pasa de 3 GB).
- Arrastrar y soltar entre programas (`start_drag` cancela la fuente), `xdg_activation`, `wp_viewporter`,
  `fractional-scale`, `text-input` y cursores del servidor (foot avisa de que no están; funciona sin ellos).
- La barra de DashOS no tiene botón de maximizar ni deja redimensionar (las ventanas de servidor no se agrandan).
- `wm_capabilities` no se manda (las pruebas de siempre esperan el configure primero).

### Después de fusionar en `fusion-fondo`: los pools de Mesa (dcf1df6)

`make arranque-virt-gpu` fallaba a veces en "frames in": `es2gears_wayland` salía y se iba al momento. No eran los
bordes: `cuadros_gl wayland` se cortaba siempre a los 5 o 6 cuadros, también en d819301 (antes de `caja-linux-apps`) y
con el disco de `revision`. Mesa con llvmpipe hace un pool por búfer y lo destruye en el acto, y libwayland repite su
número. El puente guardaba los pools por número: al crear el nuevo, el viejo (vivo por sus búferes) se perdía y esos
búferes se leían en el pool nuevo. Con copia, otro dibujo; sin copia, las páginas no casaban y el puente cerraba al
programa ("Linux movió las páginas de un búfer sin copia"). Ahora el viejo pasa a un número propio con sus búferes.
Prueba: `puente/pruebas/h9_pool_repetido.sh` (`cliente_shm` con `POOLES=1`): MAL sin el arreglo, bien con él. Con él,
`arranque-virt-gpu`: BIEN; `cuadros_gl wayland`, 386 cuadros en 20 s (19,3 por segundo; antes, 9,4 en la bitácora de
S25 y 5 cuadros en estas pruebas) y 431 en 15 s sin esperar al compositor; `es2gears_wayland`, de 94 a 140 cuadros en 5 s.

El hostil "sin-fin: le cortaron la tubería" no es fiable: en unas pasadas el programa hostil ve su tubería cerrada
(tras 64 KB) y en otras no, ni viviendo 150 s, con los mismos 64 KB pasados por el tubo en DashOS. Lo que importa sí sale
siempre: la terminal pega sus 4 KB y deja de leer, y todo sigue. Esa línea ya no es de las esperadas; el guion apunta si
llegó. Sin explicar: falta saber si DashOS corta el tubo (y manda `TUBO_FIN`) en esas pasadas.

## 2026-11-13: GTK4, Firefox y más memoria para el invitado (rama `caja-linux-grandes`)

Rama `caja-linux-grandes`, sacada de `fusion-fondo` (ff4b70f). Lo que faltaba de `caja-linux-apps`: GTK4 abortaba en la
caja y Firefox no cabía. Sala limpia: Kconfig y la documentación de zram de Linux, el README y las NEWS de glycin, las
opciones de `-M virt` de QEMU (`-M virt,help`); nada de su código.

### Resultado

- **GTK4 en la caja.** `gtk4-widget-factory` (GTK 4.24) sale como ventana de DashOS a los 12 a 16 s, con su menú (el
  botón de las tres rayas) y una caja combinada abiertos: los dos son emergentes (`xdg_popup`). Los cargadores de
  imágenes de glycin van en su caja de bwrap, como en cualquier Arch (el bwrap falso de `prueba-gtk` ya no está).
- **Firefox en la caja.** Con 512 MB y el zram del invitado, su primera ventana sale a los 36 a 44 s con una página del
  disco (`hola.html`).
- **Más memoria.** QEMU con 8 GB y `highmem=on,highmem-ecam=off,highmem-mmio=off`: DashOS ve los 8 GB y da 512 MB al
  invitado (la regla de serie). La cantidad se puede cambiar al compilar (`DASHOS_CAJA_LINUX_MB`).
- **Hostil:** `prueba-come-memoria` (un `tail /dev/zero` que crece sin fin) llena la RAM y el zram; Linux lo mata (OOM,
  código 137) a los 10 a 18 s. El invitado sigue (`echo sigue viva`), la ventana de GTK también, y DashOS no se entera.
- **La barra de DashOS:** botón de maximizar (y volver) y una franja de 6 px por fuera que redimensiona la ventana.

### GTK4: lo que le pasaba de verdad

La pista de `caja-linux-apps` ("Operation not supported" al abrir la imagen) era falsa: `glycin-thumbnailer -i` quiere
una URI; con una ruta, GIO hace un fichero que no se puede leer y da ese error sin tocar el kernel (strace: ninguna
llamada falla). Con `file:///...` carga bien en la caja.

El fallo de GTK era otro. Con `RUST_LOG=debug`, en QEMU sin DashOS y con la misma `Image-arch`: el hijo que lanza el
cargador muere con SIGSEGV justo después de escribir "Setting process memory limit", antes de ser bwrap (también la
prueba de si bwrap va, con `/usr/bin/true`). glycin limita la memoria de su cargador con `setrlimit` antes de lanzarlo, y
el límite sale de la memoria libre más el intercambio libre (sus NEWS: hasta el 80 %). Con 256 MB y sin intercambio, ese
límite (unos 120 MB) queda por debajo de lo que ya ocupa el proceso de GTK que se bifurca, y el hijo muere al seguir.
Probado en QEMU sin DashOS, 4 vCPU:

| RAM del invitado | Intercambio | El cargador de glycin |
|---|---|---|
| 256 MB | ninguno | SIGSEGV; GTK aborta ("Loader process exited early") |
| 256 MB | 512 MB en un fichero | carga |
| 256 MB | zram del tamaño de la RAM | carga, dentro de bwrap |
| 384 MB y 512 MB | ninguno | carga |
| 1 GB | ninguno | carga, dentro de bwrap |

El arreglo, en la caja: zram en `Image-arch` (`CONFIG_ZRAM`, con zstd y lz4) y `dashos-zram.service`, que pone un zram
del tamaño de la RAM como intercambio con prioridad 100, sube `vm.min_free_kbytes` a 1/32 de la RAM (con el de serie,
kswapd avisaba de que no encontraba páginas al comprimir) y pone `vm.page-cluster` a 0. Solo si el kernel tiene zram
(`ConditionPathExists`): con un `Image-arch` viejo no se ejecuta y systemd sigue en "running".

### Más memoria: por qué `highmem=off` y cómo se quitó

- QEMU `virt` con `highmem=off` no pone nada por encima de los 4 GB: la RAM empieza en el GB 1, así que no pasa de 3 GB.
  Se usaba porque con `highmem=on` el ECAM del PCIe sube por encima de 256 GB y el kernel busca el xHCI en la ventana de
  aparatos (U6). QEMU tiene opciones sueltas: `highmem-ecam=off` y `highmem-mmio=off` dejan el ECAM y las ventanas del
  PCIe abajo, y la RAM sí pasa de los 4 GB.
- Lo que no llegaba: el mapa del arranque (`arranque.s`) veía 4 GB; el mapa lineal del kernel y el reparto de páginas
  (`memoria.rs`, `mapa.rs`), también 4 GB (y la marca de páginas, 8 GB); y el cargador de UEFI pedía el árbol y la ramfb
  "en cualquier sitio": UEFI reparte desde arriba, por encima de los 4 GB.
- Ahora, en `virt`: el mapa del arranque llega al GB 16 (bloques de 1 GB normales; lo que no es RAM no se toca), el mapa
  lineal y el reparto, hasta 16 GB (4 M páginas, 512 KB de marcas). `mapa.rs` hace el mapa hasta el final de la RAM y
  saca sus tablas de arriba (por eso el del arranque llega a 16). El cargador pide el árbol y la ramfb por debajo de 4
  GB (`AllocateMaxAddress`). La pantalla y la memoria del firmware por encima de 4 GB valen si están en una zona
  reservada del árbol. En la Pi no cambia nada (sigue en 4 GB; la de 8 GB sin probar).
- `arranque-virt-wayland` va con `MEMORIA_GRANDE=8G` y `MAQUINA_GRANDE` (las dos en el Makefile): "Memoria física:
  8192 MB en total", "el invitado, hasta 512 MB". Las demás pruebas siguen con 3 GB y `highmem=off`; `arranque-virt-vm`
  con 8 GB y la máquina grande, también BIEN.
- La regla de la memoria del invitado sigue siendo la del usuario (256 MB con 2 GB, 512 con 4 GB o más). Con
  `DASHOS_CAJA_LINUX_MB=1024` al compilar el kernel va otra cantidad en el objeto "sistema" (un u64 más: 24 bytes); el
  servicio y `inicio` la usan con `memoria_invitado_con` (64 MB como poco, la mitad de la RAM como mucho). El servicio
  daba como mucho 512 MB (32 trozos de 16 MB): ahora hasta 2 GB (128; las direcciones del invitado acaban en 4 GB y su
  RAM empieza en 1 GB). Probado con 1 GB: "1024 MB de RAM, 4 vCPU" y Firefox (abajo).

### Firefox: cuánta memoria hace falta

Con el zram, 512 MB le bastan; con 256 MB, no. Medido en QEMU sin DashOS (weston sin pantalla, `WAYLAND_DEBUG` para ver
su primer búfer) y en DashOS:

| Dónde | RAM | Primera ventana | Memoria |
|---|---|---|---|
| QEMU sin DashOS | 256 MB + zram | no sale en 21 minutos; al final Firefox ya no está | |
| QEMU sin DashOS | 512 MB + zram | 25 s | Firefox 180 MB; 107 MB en el zram, que ocupan 21 MB |
| DashOS (8 GB) | 512 MB + zram | 33 a 44 s | con hola.html: RAM 366 de 492 MB, intercambio 284 de 492 (en el zram, 274 MB que ocupan 82); Firefox 174 MB |
| DashOS (8 GB), otra vez | 512 MB + zram | 31,5 s | con example.org (por la red de la caja): intercambio 427 de 492 MB (407 MB en 137); Firefox 104 MB |
| DashOS (8 GB), `DASHOS_CAJA_LINUX_MB=1024` | 1 GB + zram | 23,8 s | con hola.html: RAM 758 de 995 MB, intercambio 43 MB; Firefox 433 MB |
| Lo mismo, otra vez | 1 GB + zram | 21,8 s | con example.org: RAM 692 de 995 MB, intercambio 65 MB (58 MB en 41); Firefox 413 MB |

Con 512 MB, Firefox abre y carga una página de verdad, pero el zram queda casi lleno (87 % con example.org): una página
más pesada lo llevaría al OOM, y casi todo Firefox está en el intercambio (104 MB en la RAM, frente a 413 con 1 GB). Con
1 GB sale antes (22 a 24 s) y apenas usa el intercambio. Propuesta para el usuario: la regla de serie como está (256 MB con 2 GB, 512 con 4 GB) y
**1 GB con 8 GB o más**, para usar Firefox de verdad. Con 2 GB, Firefox no cabe (a quien lo necesite: compilar con
`DASHOS_CAJA_LINUX_MB=512`). La cantidad ya se puede cambiar al compilar; la regla nueva no se ha puesto sin su permiso.

### La barra de DashOS: maximizar y redimensionar

En `wayland/src/decoracion.rs`: `Zona::Maximizar`, el botón a la izquierda del de cerrar (un cuadrado de 10 px; lo
pintan el kit en el compositor y el pintor sencillo); maximiza con `maximizar_esta` y otra pulsación la devuelve. Y
`bordes`: una franja de `BORDE` (6 px) por fuera de la ventana con la barra la redimensiona por ese borde o esquina, con
el mismo `Redimension` que `xdg_toplevel.resize` (`empezar_redimension`, ahora compartido). Maximizada, la franja no
hace nada; si otra ventana tapa ese sitio, el clic es suyo. Prueba: `maximizar_y_redimensionar_con_la_barra`.

### Lo que hubo que arreglar por el camino

- **El guion en Full HD.** `ir_a` llevaba el cursor a la esquina con 12 pasos de 100 px: desde la derecha de una
  pantalla de 1920 no llegaba, y el clic caía 300 px a la derecha (en el botón de cerrar o de minimizar de GTK, que se
  iba). Ahora 20 pasos.
- **La geometría de las ventanas de Linux** que lee el guion traía seis números (con lo que mide), y se desempaquetaba en
  cuatro: el guion moría al ver la ventana de GTK (nunca había llegado tan lejos).
- **GTK y Firefox tapan la terminal de la caja** (con 800 x 600, GTK mide 1194 × 757 y sale en x negativa, y el guion
  no veía su ventana: ahora lee coordenadas negativas). El guion no teclea con ellos delante: `prueba-gtk-guion` abre
  GTK, deja 300 s para el menú, la caja combinada, redimensionar y mover, y lanza el hostil de la memoria con ella
  abierta. Con 800 x 600 el botón del menú queda fuera de la pantalla: se abre el del botón derecho del primer campo de
  texto (también un emergente); la caja combinada y el redimensionado, solo si se ven (Full HD). Firefox no hace caso
  al teclado ni al ratón de DashOS (ver "Qué falta"). `prueba-firefox-guion` lo hace todo solo: hola.html, su memoria a los 150 s
  (`prueba-memoria`, una línea por hvc0) y otra vez con example.org. Con `pkill -x firefox`: sin `-x`, `pkill firefox`
  mataba también a ese guion (su nombre lo lleva).
- **Las teclas `<` y `>`** del teclado español, en el guion.
- `CAJA_SOLO=gtk`, `firefox` o `grandes` (los dos): solo eso, para probarlo sin esperar a lo demás (con otras líneas
  esperadas, `ESPERADO_WAYLAND`). `CAJA_GTK_DEPURAR=1`: GTK con `WAYLAND_DEBUG` y su final en la terminal. Firefox va ya
  en el guion de siempre (`CAJA_FIREFOX=0` lo salta).
- **strace** en la imagen de los programas grandes.

### Pruebas

- `cargo test` en `wayland` (80, una nueva: `maximizar_y_redimensionar_con_la_barra`).
- `make prueba`: BIEN: esperado.txt y BIEN: cargador hunk.
- `arranque-virt` (las cinco BIEN), `-vm`, `-linux`, `-2`, `-diag`, `-escritorio`, `-usb`: BIEN, con 3 GB y
  `highmem=off` como siempre (`-vm` y `-linux` otra vez tras el cambio de los 2 GB: BIEN). `arranque-virt-vm` también con 8 GB y la máquina grande: BIEN ("Memoria física: 8192 MB
  en total").
- `arranque-virt-arch` y `-gpu`, con `Image-arch` nuevo (zram) y su disco (caché propia de 4 GB, sin los programas
  grandes): `-arch` BIEN. `-gpu`, antes de fusionar `fusion-fondo` (f965af7): faltaba "frames in" dos veces, como en
  `fusion-fondo` antes del arreglo de los pools de Mesa (dcf1df6, arriba). En QEMU sin DashOS, es2gears con el
  `Image-arch` nuevo (zram) y con el viejo da lo mismo: 60 cuadros por segundo.
- `arranque-virt-wayland` (8 GB, disco de los programas grandes rehecho): RESULTADO_WAYLAND.
- En Full HD (`DASHOS_PANTALLA=1920x1080`, `CAJA_SOLO=grandes` y `firefox`): BIEN con sus líneas (GTK, su emergente,
  el hostil de la memoria, Firefox y su memoria).
- En QEMU sin DashOS (`probar-arch` con weston sin pantalla): GTK con 256, 384, 512 MB y 1 GB, con y sin intercambio;
  Firefox con 256 y 512 MB (las tablas de arriba).
- Capturas (fuera de git, `capturas-agente/`, Full HD): `caja-linux-grandes-gtk-fhd.png` (GTK4 en DashOS),
  `-gtk-menu-fhd.png` (su menú), `-gtk-combo-fhd.png` (la caja combinada), `-gtk-grande-fhd.png` (redimensionada y
  movida), `-gtk-tras-hostil-fhd.png` (tras el OOM del invitado), `-firefox-fhd.png` (hola.html) y
  `-firefox-red-fhd.png` (example.org).

### Medidas

QEMU `virt`, TCG, 4 vCPU, 8 GB, con el PC cargado por otras sesiones; el invitado con 512 MB y zram.

| Medida | Valor |
|---|---|
| GTK4 (`gtk4-widget-factory`) hasta su primera ventana | 12,5 a 16,3 s (antes, en la caja, no salía) |
| Redimensionar GTK4 (1666 × 881, por su borde izquierdo) | 23 cuadros por segundo (24 en 1033 ms) |
| Moverla por su barra | 13 por segundo (24 en 1817 ms) |
| Firefox hasta su primera ventana | 33 a 44 s; otra vez, 31,5 s (con 1 GB: 23,8 y 21,8 s) |
| Firefox con 256 MB y zram (QEMU sin DashOS) | sin ventana en 21 minutos |
| Hostil de la memoria: hasta que Linux mata al programa | 8,5 a 17,6 s (llena 492 MB de RAM y 492 de zram) |

### Qué falta

- **Firefox no hace caso al teclado ni al ratón de DashOS** tras su primera ventana (ni Control+L, ni letras en un campo,
  ni arrastrar su barra); GTK4 y foot sí. Sin mirar: puede ser que vaya muy justo de memoria, o algo de GTK3 con el
  teclado del puente. El guion no teclea en él.
- **La regla de 1 GB con 8 GB o más**: propuesta, pendiente del usuario.
- **La Pi 4 de 8 GB** sigue sin usar lo que pasa de los 4 GB (el mapa lineal de 16 GB es solo de `virt`).
- Las tipografías CJK en la imagen (example.org las enseña como cajas).
- Lo de antes: arrastrar y soltar, `xdg_activation`, `wp_viewporter`, `fractional-scale`, `text-input`, cursores del
  servidor, `wm_capabilities`.
