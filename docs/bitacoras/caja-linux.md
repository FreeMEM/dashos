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
| Ida y vuelta servicio → invitado (HVC) → servicio, con la llamada al sistema, 2000 vueltas | 30 µs de media |
| De ellas, dentro del `hvc` del kernel (EL2, cambio de mundo y el invitado) | 18 µs de media |

En TCG cada `msr`/`mrs` de un registro del sistema es caro; el cambio de mundo guarda y carga unos 60. Falta medirlo
en la Pi.

### Qué falta

- Interrupciones para el invitado: ni GIC virtual ni temporizador virtual entregado. Si el invitado arma su
  temporizador virtual, su interrupción sale al anfitrión una y otra vez (el servicio lo ve como "interrumpido").
- La coma flotante del invitado: hoy se atrapa y mata la máquina. Hay que guardar la del hilo que lo corre (o la del
  dueño perezoso de la FPU en ese núcleo, C28) y dar al invitado la suya.
- Varias vCPU por máquina, y varios objetos de memoria por máquina (hoy uno, de hasta 16 MB).
- Con GICv3, el invitado puede tocar los registros del sistema de la interfaz de CPU (los virtuales, con IMO/FMO);
  falta atrapar ICC_SRE_EL1 (ICC_SRE_EL2.Enable a 0 mientras corre).
- La memoria del invitado se lleva a la RAM al crear la máquina: lo que escriba el servicio después, con el invitado
  sin MMU, puede no verlo en la Pi (QEMU no tiene cachés).
- virtio (consola, disco, red, vsock, virtio-fs, virtio-gpu), el globo, Arch Linux ARM, el puente de Wayland.
- Probarlo en la Pi 4.

### Decisiones para el usuario

1. El permiso de máquinas virtuales lo da hoy `inicio` a `caja-linux`. ¿Lo reparte `sesiones` como permiso de una
   aplicación (S6), o es solo del sistema?
2. La caja Linux solo está en la imagen de `virt`. ¿Va también en la de la Pi?
3. El hipervisor de EL2 se instala siempre que el firmware entra en EL2. ¿Se quiere poder apagarlo (que el kernel
   cierre EL2 al arrancar si no hay caja Linux)?
