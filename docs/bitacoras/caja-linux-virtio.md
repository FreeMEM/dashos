# Bitácora de la caja Linux: lo que la máquina virtual da al invitado (H9)

Rama `caja-linux-virtio`, sacada de `revision` (cb104b1). Va en paralelo a la rama `caja-linux`, que hace el
hipervisor (EL2, la máquina virtual, el vCPU y las salidas al servicio de la caja). Aquí no se toca `kernel/` ni
`usuario/`: todo es un crate y herramientas del PC, para enchufarlo en el servicio.

Sala limpia: escrito con las especificaciones públicas (OASIS virtio 1.2, Arm PL011 TRM DDI 0183, Arm GICv2 IHI 0048B,
Arm PSCI DEN 0022, Devicetree 0.4, `Documentation/arch/arm64/booting.rst` y las vinculaciones de Devicetree de Linux
como especificación, y la especificación de vhost-user de la documentación de QEMU). Sin leer código de Linux, KVM,
QEMU, crosvm ni Firecracker. Las direcciones de la placa `virt` de QEMU salen de su DTB (`-M virt,dumpdtb=`).

## 2026-11-12: aparatos, DTB, carga, y Linux arrancando en QEMU con ellos

### Qué hay

**`caja-linux/vmm/`** (crate `dashos-caja-vmm`, `no_std` con `alloc`, `forbid(unsafe_code)`; compila para
`aarch64-unknown-none-softfloat` con `-Zbuild-std=core,alloc`):

| Módulo | Qué es |
|---|---|
| `memoria` | `MemoriaInvitado` (leer/escribir por dirección del invitado, con error si se sale) y `Ram<B>` sobre un `Vec` o un `&mut [u8]` |
| `mmio` | `AparatoMmio`: acceso (desplazamiento, tamaño, valor), nivel de su línea y `sondear` |
| `maquina` | `Maquina`: RAM + distribuidor del GIC + aparatos con su dirección y SPI; `mmio(vcpu, dir, tam, op)` y `sondear()` |
| `virtio/cola` | Colas partidas (§2.7) con tablas indirectas; comprueba todo lo que viene del invitado |
| `virtio/mmio` | Transporte virtio-mmio versión 2 (§4.2), con negociación, DEVICE_NEEDS_RESET y aviso de configuración |
| `virtio/consola` | virtio-console de un puerto (`hvc0`), con SIZE y EMERG_WRITE, hacia un `ExtremoConsola` |
| `virtio/bloque` | virtio-blk (IN, OUT, FLUSH, GET_ID) sobre un trait `Bloques`; `DiscoEnMemoria` |
| `virtio/red` | virtio-net de un par de colas, sin descargas, hacia un `ExtremoRed` (MAC, enlace, MTU 1500) |
| `pl011` | PL011 mínimo para `earlycon` y `ttyAMA0` |
| `gic` | Distribuidor GICv2 emulado (la parte que el hardware no virtualiza) y su interfaz con los registros de lista |
| `psci` | PSCI 1.0 por HVC: CPU_ON/OFF, AFFINITY_INFO, SYSTEM_OFF/RESET, FEATURES |
| `fdt`, `arbol` | Escritor de FDT con `alloc` (la idea de `uefi/src/fdt.rs`, con mapa de reservas) y el DTB del invitado |
| `arranque` | Cabecera de la `Image` de arm64, dónde va cada cosa, carga y registros de entrada |
| `disposicion` | El mapa provisional de la máquina (el de QEMU `virt`) y su `ConfigInvitado` |

`src/bin/caja_vmm.rs` (`caja-vmm preparar ...`): saca el DTB, el plan de carga y un trampolín para probar en QEMU.

**`caja-linux/vhost-prueba/`**: solo PC. Sirve el `Bloque` o la `Red` del crate, con su `ColaPartida`, a QEMU por
vhost-user, para que los usen los controladores de un Linux de verdad. En la red, el otro lado es un "router" de
juguete en 10.0.2.2 que contesta ARP y ping. Lleva `unsafe` (mmap, recvmsg, poll).

**`tools/caja_linux_invitado.py`**: `preparar` baja Linux 6.18 (LTS de kernel.org) y lo compila con `LLVM=1`
(defconfig + virtio, unos 5 minutos con 32 núcleos), compila busybox 1.37 estático con la toolchain musl de Bootlin y
hace el initramfs (cpio escrito en Python, sin root). Todo en `~/.cache/dashos-caja-linux/`. `probar` y
`probar-vhost`, abajo.

### Pruebas

- `cargo test` en `caja-linux/vmm` (34 pruebas, también en modo debug, con las comprobaciones de desbordamiento):
  - unitarias de memoria, GIC (nivel, flanco, SGI entre CPU, accesos raros), PSCI, DTB (contenido y `dtc` sin avisos,
    GICv2 y GICv3) y carga (plan, RAM sin alinear, más de 32 GiB, cosas que no caben);
  - `tests/invitado_falso.rs`: un controlador de mentira que negocia, monta colas y manda peticiones a la consola, el
    disco y la red, con su interrupción en el GIC. Casos hostiles: descriptores fuera de la RAM, cadenas en bucle (en la
    tabla y en una indirecta), indirectas anidadas, vacías o de largo raro, "siguiente" y cabezas fuera de la tabla,
    índice disponible que salta, lectura tras escritura, direcciones que desbordan, anillos fuera de la RAM, colas de
    tamaño imposible o desalineadas, largos de 4 GiB, avisos de colas que no existen. En todos: DEVICE_NEEDS_RESET (o
    IOERR para una petición de disco mala) y el aparato vuelve a funcionar tras reiniciarlo. Y `a_lo_loco`: 800 000
    accesos al azar a registros, memoria de las colas, avisos y sondeos, sin pánico.
- `tools/caja_linux_invitado.py probar`: Linux 6.18 arranca en `qemu-system-aarch64 -M virt` (Cortex-A72, 2 CPU,
  GICv2) **con el DTB y la carga del crate**, sin `-kernel` ni `-dtb`: QEMU solo copia la Image, el initrd, el DTB y un
  trampolín de seis instrucciones (x0 = DTB, x1..x3 = 0, salto) a las direcciones del plan, y empieza en el trampolín
  en EL1 con la MMU apagada. Salida:

  ```
  Machine model: DashOS caja Linux
  earlycon: pl11 at MMIO 0x0000000009000000 (options '')
  psci: PSCIv1.1 detected in firmware.
  arch_timer: cp15 timer running at 62.50MHz (virt).
  Freeing initrd memory: 740K
  caja-linux: init en marcha
  caja-linux: kernel 6.18.63 aarch64
  caja-linux: cpus 2
  caja-linux:   virtio0 id=0x0003 virtio_console
  caja-linux:   virtio1 id=0x0002 virtio_blk
  caja-linux: consola hvc0
  caja-linux: vda 2048 sectores, primeros bytes: DISCO-CAJA-LINUX
  caja-linux: irq  11:        354        348 GIC-0  27 Level     arch_timer
  caja-linux: irq  14:        132          0 GIC-0  79 Level     virtio0
  caja-linux: irq  15:          2          0 GIC-0  78 Level     virtio1
  caja-linux: fin de la prueba
  ```

  Los aparatos que ve el invitado en esta prueba son los de QEMU: se prueban el DTB y la carga.
- `tools/caja_linux_invitado.py probar-vhost`: lo mismo, pero el disco y la red son el `Bloque` y la `Red` del crate,
  servidos por vhost-user (dos procesos de `vhost-prueba`). Linux hace `md5sum` de los 8 MiB del disco y copia el
  primer MiB a los 4 MiB con `dd conv=fsync`; las sumas cuadran con las del fichero y la copia está en el fichero al
  acabar. Después configura `eth0` y hace `ping -c 3 10.0.2.2`: `3 packets transmitted, 3 packets received`. Así la
  cola partida (con tablas indirectas negociadas), las peticiones IN, OUT y FLUSH y la cabecera de red de virtio 1.x
  están probadas con los controladores de Linux. En las dos pruebas QEMU da virtio-mmio moderno
  (`-global virtio-mmio.force-legacy=false`), el mismo transporte que el del crate.

### Cómo se enchufa al servicio de la caja

Lo que el servicio necesita del hipervisor y lo que hace con el crate:

1. **Memoria del invitado.** El servicio tiene la RAM del invitado mapeada (la misma que da la etapa 2) y la envuelve
   en `Ram { base: disposicion::RAM, datos: &mut [u8] }`. Hay que poner en `MemoriaInvitado::barrera` un `dmb ish`
   (por una función segura de la biblioteca `dashos`): hoy `Ram` no la pone, porque en el PC no hace falta.
2. **Carga.** `arranque::leer_cabecera`, `planificar(cab, largo, (RAM, tam), largo_initrd)`, el DTB con
   `arbol::generar_dtb(&disposicion::config(...))` (con `initrd` del plan y una `semilla` del generador de DashOS),
   `arranque::cargar` y `arranque::registros(&plan)`: el vCPU 0 empieza con ELR_EL2 = `pc`, SPSR_EL2 = 0x3c5 (EL1h,
   DAIF enmascarado), x0 = DTB y x1..x3 = 0. El protocolo pide además: MMU y caché de datos de EL1 apagadas
   (SCTLR_EL1 conocido), la Image limpia hasta el punto de coherencia, CNTFRQ_EL0 programado, CNTVOFF_EL2 igual en
   todos los vCPU y CNTHCTL_EL2.EL1PCTEN = 1. VMPIDR_EL2 de cada vCPU = número de vCPU en Aff0 (el `reg` del DTB).
3. **Salidas de MMIO.** Un aborto de datos de EL1 (ESR_EL2.EC = 0x24) en una IPA sin RAM: si ISV = 1, la IPA sale de
   HPFAR_EL2 (página) y FAR_EL2[11:0]; tamaño = 1 << SAS, registro = SRT, escritura = WnR. Se llama a
   `maquina.mmio(vcpu, ipa, tam, Leer | Escribir(xt))`; si lee, el valor va a Xt (con signo si SSE; SF dice 32 o 64
   bits) y ELR_EL2 avanza 4. `SalidaMmio::SinAparato` (o ISV = 0, que Linux no usa para MMIO): inyectar un aborto
   externo, o leer 0 y seguir. El distribuidor del GIC (0x0800_0000) va por la misma vía.
4. **HVC.** ESR_EL2.EC = 0x16: `psci.atender(vcpu, [x0, x1, x2, x3])` y hacer lo que diga `AccionPsci`
   (responder en x0, arrancar otro vCPU en `entrada` con x0 = `contexto` y el mismo estado que el 0, parar este,
   esperar interrupción, apagar o reiniciar la máquina). Tras arrancar uno, `psci.encendida(cpu)`.
5. **Interrupciones** (GIC-400 de la Pi 4, GICv2 con extensiones de virtualización; en la Pi 4 en modo de periféricos
   bajos: GICD 0xff84_1000, GICC 0xff84_2000, GICH 0xff84_4000, GICV 0xff84_6000, mantenimiento en la PPI 9):
   - la GICV (8 KiB) se mapea en la etapa 2 en la IPA del GICC del invitado (`disposicion::GICC`). El GICD del
     invitado no se mapea: sus accesos salen y los atiende `maquina.gic` (distribuidor emulado);
   - antes de volver a un vCPU: para cada LR libre (GICH_ELRSR0), `maquina.gic.pendientes(vcpu)` en orden, se escribe
     el LR (estado pendiente = 01 en [29:28], prioridad[7:3] en [27:23], Grp1 en [30], ID virtual en [9:0], para una
     SGI la CPU de origen en [12:10]) y se llama a `maquina.gic.cargada(vcpu, &e)`. Con el bit EOI ([19]) puesto, el
     GIC da la interrupción de mantenimiento cuando el invitado la termina; entonces (o al salir, viendo el LR vacío)
     `maquina.gic.terminada(vcpu, intid)`. Las de los aparatos son por nivel: si la línea sigue alta, vuelven a salir;
   - el temporizador virtual del invitado (CNTV, PPI 27) no pasa por el distribuidor emulado: al saltar mientras corre
     el invitado llega al anfitrión como PPI 27 física, y el hipervisor la pone en un LR con HW = 1 y el ID físico 27
     (así el EOI del invitado desactiva la física), o la enmascara y la inyecta sin HW;
   - un vCPU en WFI se despierta cuando `maquina.gic.hay_para(vcpu)`.
6. **Lo que viene de DashOS** (teclas de la terminal, tramas de red): al llegar por su canal, `maquina.sondear()`.
   Los extremos (`ExtremoConsola`, `ExtremoRed`, `Bloques`) son traits: el servicio los implementa con sus canales
   (la terminal de DashOS, la pila de red de S13, un fichero del servicio de archivos).

### Decisiones provisionales

- Mapa de la máquina igual al de QEMU `virt` (RAM en 1 GiB, GICD 0x0800_0000, GICC 0x0801_0000, PL011 0x0900_0000
  con SPI 1, virtio-mmio desde 0x0a00_0000 cada 0x200 con SPI 16 + i), para poder comparar con QEMU.
- Las interrupciones de virtio-mmio y del PL011 son **por nivel** en el DTB y en el distribuidor (QEMU las declara por
  flanco en el de virtio). Encaja con el bit EOI del LR: el hipervisor sabe cuándo volver a mirar la línea.
- Prioridades de 5 bits (las que caben en un LR del GIC-400). Sin extensiones de seguridad en el GIC virtual.
- PSCI 1.0 sin SMCCC 1.1: `PSCI_FEATURES(SMCCC_VERSION)` dice que no, así que Linux no pregunta por las mitigaciones
  de Spectre (ARCH_WORKAROUND_1). Para la Cortex-A72 habrá que darlas antes de usar la caja en serio.
- Consola: como mucho 64 KiB por cadena de salida. Disco: como mucho 4 MiB por petición (seg_max 64 × size_max
  64 KiB), copiados de 4 KiB en 4 KiB sin reservar memoria según lo que pida el invitado. Red: una trama que no cabe
  en su búfer se descarta y el búfer vuelve vacío.
- Un aviso atiende como mucho `tam` cadenas de la cola, para no quedarse atrapado si el invitado sigue llenando desde
  otro vCPU.
- Vendor ID de virtio "DSH\0" (0x0048_5344). Linux no lo mira.
- `image_size` = 0 (kernels anteriores a 3.17) no se admite.

### Qué falta

- Enchufarlo: el servicio de la caja (rama `caja-linux`) con los puntos de arriba, y un `Bloques` hacia el servicio
  de archivos (hoy el trait es síncrono; con el servicio en otro proceso convendrá una cola de peticiones).
- `virtio-vsock`, para el puente de Wayland (`puente/` ya habla un flujo de bytes con tramas, pensado para `vsock`) y
  para hablar con DashOS; `virtio-fs` para `Home:`; `virtio-balloon` (S7: memoria que se devuelve); `virtio-rng`.
- `virtio-gpu` 2D, si hiciera falta una consola gráfica sin Wayland.
- VIRTIO_F_EVENT_IDX (menos avisos e interrupciones), consola con varios puertos.
- GICv3 emulado (distribuidor y redistribuidores) para placas con GICv3; el DTB ya sabe describirlo.
- La consola no se ha probado con el controlador de Linux (QEMU no tiene vhost-user para ella), solo con el invitado
  de mentira. El PL011 y el transporte virtio-mmio del crate tampoco: en estas pruebas los registros los lleva QEMU.
  Eso llegará con el hipervisor de DashOS.
