# Bitácora: un dominio de red (S35, paso 2)

Rama `dominio-red`, sacada de `revision` (2cffd5c5). El paso 2 de S35: DashOS es el hipervisor, arranca una máquina
virtual pequeña con el Linux mínimo medido en el paso 1, le presta una tarjeta de red y el Linux se la devuelve a
DashOS como un aparato virtio-net, que DashOS lleva con su controlador de siempre y su servicio de red.

Sala limpia: especificación virtio 1.2 (OASIS), la del transporte virtio-msg (borrador de Linaro) y la de vhost-user
(documentación de QEMU) leídas como descripción; la documentación del kernel (Kconfig, `Documentation/`, las
vinculaciones de Devicetree) como especificación. Sin leer código de Linux, QEMU, KVM, Xen ni crosvm. El kernel y la
imagen del dominio se construyen fuera de git, en `~/recuperar/geosian-7d/dominio-red/`.

## 2026-11-16: cómo sirve un invitado un aparato virtio a su anfitrión

### Lo que hay escrito

En virtio el aparato lo pone siempre "el otro lado" del conductor. Lo normal es que sea el hipervisor (QEMU, la caja
Linux de DashOS); aquí el que pone el aparato es un invitado y el que conduce es el anfitrión. Cuatro formas
documentadas de hacerlo:

| Mecanismo | Cómo va | Qué pide |
|---|---|---|
| **vhost-user** (QEMU, `docs/interop/vhost-user`) | El aparato vive en otro proceso. El hipervisor emula los registros del transporte y le pasa al proceso, por mensajes, la tabla de memoria (`SET_MEM_TABLE`), cada cola (`SET_VRING_NUM/ADDR/BASE`) y dos avisos (`KICK`, `CALL`). El proceso lee las colas directamente en la memoria del conductor | Que el aparato pueda ver la memoria del conductor |
| **virtio-vhost-user** (propuesta a OASIS, de vhost-pci) | Lo mismo, pero el proceso del aparato es una máquina virtual: un aparato PCI le enseña la memoria del conductor en una BAR y le pasa los mensajes de vhost-user | Un controlador de ese aparato en el invitado (no está en Linux) |
| **Dominios de controladores de Xen con virtio por "grants"** (`xen,grant-dma`) | El conductor solo concede las páginas de sus colas y búferes; el aparato, en otro dominio, solo puede proyectar esas | Xen en los dos lados |
| **virtio-msg** (Linaro, borrador para virtio 1.4) | Un transporte por mensajes: `GET_DEVICE_INFO`, `GET/SET_FEATURES`, `GET/SET_CONFIG`, `SET_DEVICE_STATUS`, `SET_VQUEUE`, `EVENT_AVAIL`, `EVENT_USED`. Las colas siguen en memoria compartida; cada "bus" dice cómo se comparte y qué es una dirección | Un bus que lleve los mensajes y comparta la memoria |

Y una más, la de pKVM y las máquinas confidenciales: el conductor solo usa para DMA una zona compartida
(`restricted-dma-pool`, swiotlb), y el otro lado solo ve esa zona.

### Lo elegido

**Un transporte por mensajes como virtio-msg, por un canal de DashOS, con la memoria de las colas compartida como
un "grant": un solo objeto de memoria del conductor, que el hipervisor pone en la etapa 2 del dominio en una
dirección fija.** El dominio ve esa ventana y nada más de DashOS.

- **El plano de control lo contesta el servicio del dominio**, que es de DashOS y de fiar, como hace QEMU con
  vhost-user: el conductor (`virtio-red`) le manda por su canal "info", "funciones", "cola", "estado" y "memoria"; el
  servicio contesta con lo que ofreció el aparato (tipo, funciones, MAC), comprobado: solo virtio-net, solo
  funciones conocidas, una MAC de unidifusión.
- **El plano de datos lo lleva un programa dentro del dominio** (su `/init`, sin biblioteca C), como un "backend" de
  vhost-user: lee las colas en la ventana, pasa las tramas entre la tarjeta (un socket de paquetes) y los búferes, y
  devuelve los usados. El servicio del dominio no copia ni mira ninguna trama.
- **Los avisos**, por un aparato propio y pequeño que el servicio emula en el dominio (el "canal del dominio": una
  página de registros y una interrupción). DashOS → dominio: el conductor avisa al servicio, el servicio sube la
  interrupción. Dominio → DashOS: el programa escribe en un registro (una salida al servicio), y el servicio avisa al
  conductor por su canal. En el Linux del dominio, el aparato se lleva con UIO (`uio_pdrv_genirq`, de serie en Linux:
  la interrupción se espera con `read`) y la ventana se lee con `/dev/mem`.
- **El conductor es el de siempre** (`virtio-red`): las colas, los búferes y lo que hace con las tramas no cambian.
  Cambia el transporte: en vez de los registros de virtio-pci, mensajes; en vez de memoria DMA, un objeto de memoria
  normal; y la "dirección de bus" de sus colas es la de la ventana en el dominio.

Por qué así:

1. **Encaja con DashOS.** DashOS ya habla por canales y mensajes; un transporte por mensajes es lo natural (es lo
   que virtio-msg propone para sistemas así). El hipervisor ya sabe poner un objeto de memoria en la etapa 2 de una
   máquina (`maquina_memoria`): para este lado no hace falta nada nuevo en el kernel.
2. **Linux sin tocar.** Ni un módulo ni un parche: UIO, `/dev/mem` y un socket de paquetes son de serie. Lo propio es
   un programa de usuario.
3. **Lo mínimo para el dominio (capa 4).** Ve su tarjeta, la ventana y su canal; nunca la memoria de DashOS. Lo que
   la etapa 2 no le da, para él no existe.
4. **DashOS no se fía (capa 5).** Lo que llega del dominio son índices y largos en la cola de usados (el conductor ya
   los comprueba: ver el comentario de `virtio_red.rs`), escrituras en su canal (el servicio las comprueba) y su
   oferta (comprobada). El dominio puede escribir lo que quiera en la ventana, pero la ventana solo tiene colas y
   búferes de red.
5. **Sin copias en el servicio.** El dominio escribe la trama directamente en el búfer del conductor.

Lo descartado:

- **El dominio como conductor** (el hipervisor le da una virtio-net de las de la caja Linux y el dominio hace de
  puente). Funciona y es poco código, pero no invierte nada: DashOS no usaría su controlador virtio-net, sino un
  puente propio en el servicio.
- **vhost-net dentro del Linux del dominio** (el aparato virtio-net del propio kernel, el que usa KVM). Tentador,
  pero pide que la memoria del conductor sea memoria de usuario normal para el kernel del dominio y sus ioctl
  cambian con las versiones; el programa propio son unos cientos de líneas que se entienden enteras.
- **virtio-vhost-user**: necesita un controlador en el invitado que Linux no tiene.

### Cómo llega la tarjeta al dominio: PCI en QEMU, paso intermedio

El diseño pide USB primero: DashOS lleva el xHCI y presta un aparato. Para este encargo es demasiado largo: el xHCI de
DashOS solo sabe de teclados y ratones (sin puntos de transferencia masiva, sin varias transferencias a la vez, sin
cancelarlas), y faltaría además el camino hasta el dominio (USB/IP por vsock, abajo). **Se empieza con una tarjeta PCI
emulada por QEMU (Intel e1000) que DashOS pasa al dominio sin IOMMU, solo en QEMU y solo en la imagen de esta
prueba.** Es un paso intermedio y no vale fuera de QEMU:

- **Sin IOMMU, un dominio malicioso puede programar la tarjeta para que escriba en cualquier sitio de la memoria
  física.** El dominio usa para la DMA de la tarjeta una zona propia (`restricted-dma-pool`) que el hipervisor le
  pone en la misma dirección que tiene en la máquina, pero eso solo ordena a un dominio que se porta bien: no protege
  de uno malo. Por eso S35 dice "PCI solo con IOMMU" (paso 4), y por eso esto no sale de la imagen de prueba.
- Lo que sí vale para después: el plano de datos y de control por virtio (todo lo de arriba), el registro, los
  reinicios y la medida. Con IOMMU, la zona de DMA desaparece: la IOMMU traduce las direcciones del dominio.
- **El USB, siguiente paso** (plan): el xHCI de DashOS aprende a configurar los puntos de un aparato prestado y a
  llevar transferencias de control, masivas y de interrupción (con cancelación); el servicio del dominio habla USB/IP
  (protocolo documentado en `Documentation/usb/usbip_protocol.rst`) por un vsock con el dominio, y el Linux del dominio
  lo engancha con su `vhci-hcd` de serie. Así el dominio no ve ningún registro de DashOS y vale en todas las máquinas.
