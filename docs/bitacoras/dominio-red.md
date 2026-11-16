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

## 2026-11-16: hecho, con la e1000 por PCI (paso intermedio) y TCP

### Qué funciona

`make arranque-virt-dominio`: DashOS arranca en QEMU `virt` con `virtualization=on` y sin virtio-net-pci; su única
tarjeta es una e1000 de QEMU. El servicio `dominio` arranca un Linux 7.3.4 mínimo (el del paso 1, con `paso.config`)
en una máquina de 24 MB y una vCPU, le presta la e1000 y el Linux la lleva con su controlador (`e1000`) y devuelve a
DashOS una virtio-net. `virtio-red` (el conductor de siempre, con el transporte nuevo) la monta, el servicio de red
toma dirección por DHCP (10.0.2.15, de la red de usuario de QEMU) y `dominio-prueba` hace un eco TCP con 10.0.2.100:7
(el eco del PC por la red de QEMU, con `restrict=on`: nada sale del PC) y el eco UDP del vecino de prueba
(10.0.2.53:4007). Después relanza el dominio en modos hostiles y la red sigue. Nada va a Internet.

### Cómo va

```
e1000 (QEMU) ──registros en la etapa 2, INTx reenviada, DMA en su zona──▶ Linux del dominio (controlador e1000)
                                                                              │ socket de paquetes
                                                                              ▼
                                                               /init (dominio_red.c): el aparato virtio-net
                                                                  │ colas en la ventana   ▲ canal (UIO): avisos
                                                                  ▼                       │
virtio-red (DashOS) ◀── la ventana es su memoria ──▶ servicio `dominio` ◀── mensajes (virtio_msg) ──▶ virtio-red
     │ tramas (protocolo::red)
     ▼
servicio de red (smoltcp, DHCP, TCP) ──▶ dominio-prueba
```

- **Kernel de DashOS** (`base/hiper.rs`, `maquinas.rs`, `llamadas.rs`): dos operaciones nuevas de `maquina_estado`,
  con números altos: 40 presta al invitado los registros de una llave de registros en su misma dirección física
  (memoria de dispositivo, sin ejecutar) y 41 pone un objeto de memoria DMA en su dirección de bus. Comprueban que no
  pisan nada de lo que ya ve el invitado y que la llave no es revocable (revocarla no quitaría nada de la etapa 2).
- **El crate `caja-linux/vmm`**, módulo `dominio` (sin `unsafe`, con pruebas en el PC): el canal del dominio (una
  página de registros y una interrupción: estado, funciones, ventana y colas que pidió el conductor; avisos en los dos
  sentidos; la oferta del aparato y el latido), la comprobación de la oferta (`validar_oferta_red`), el PCIe del
  dominio (un ECAM con un solo aparato: la cabecera de verdad sin capacidades, la BAR 0 fija, el mando, la INTx) y los
  nodos del árbol (zona de DMA como `restricted-dma-pool`, la ventana reservada, el canal, el PCIe). `arbol` gana
  `generar_dtb_con` para añadir nodos.
- **El transporte** (`usuario/src/protocolo/virtio_msg.rs`, con pruebas): HOLA, INFO, FUNCIONES, MEMORIA, COLA,
  ESTADO y AVISO del conductor; USADOS y APARATO del servicio. Cada mensaje lleva la generación del aparato: lo de una
  generación vieja se rechaza.
- **El servicio `dominio`** (`usuario/src/bin/dominio.rs`): carga el Linux, corre su vCPU, emula el PL011 (cada línea
  de la consola al registro común con "linux:" delante, con tope), el canal y el PCIe, reenvía la INTx de la e1000
  (sube la línea al llegar la interrupción de DashOS; al terminarla el dominio, la baja y la vuelve a habilitar),
  contesta al conductor y pone su memoria como ventana. Un vigilante: sin latido 3 s, o sin oferta en 30 s, el dominio
  está colgado. Al acabar una generación (apagado, reinicio por pánico, fallo, colgado, orden de la prueba): apaga el
  maestro de bus de la e1000 (deja de escribir en la memoria) antes de soltar nada, dice APARATO falso al conductor,
  deja una línea con el motivo y las cuentas, y relanza. Más de 5 caídas en un minuto: se deja apagado.
- **`virtio-red`**: con la llave "dominio-red" usa el transporte por mensajes. Monta el aparato de cero en cada
  generación (memoria nueva, colas nuevas); las tramas del servicio de red y su canal duran lo que el programa. El
  bucle de mover tramas es el de siempre, para los dos transportes; ahora además vuelve a escribir cada descriptor
  antes de dárselo al aparato (en la ventana, el dominio puede cambiarlos).
- **El /init del dominio** (`caja-linux/dominios/dominio_red.c`, sin biblioteca C): espera la e1000 y `/dev/uio0`,
  levanta la tarjeta, ofrece la red con su MAC, dice "listo" con su reloj y su memoria, y pasa tramas entre el socket
  de paquetes y las colas. Comprueba direcciones, largos y cadenas de lo que lee en la ventana.
- **La e1000 y sus puertos**: el controlador de Linux de la 82540EM hace el reinicio por la BAR de puertos (la 1);
  sin ella, su primera escritura cayó fuera y el kernel del dominio entró en pánico (el vigilante lo relanzó 6 veces y
  lo dejó apagado, como debe). Esa BAR es otra puerta a los mismos registros (IOADDR e IODATA, documentado en el manual
  de la familia 8254x), así que el servicio la emula sobre la BAR 0 (`PuertosE1000`). Es propio de esta tarjeta y de
  este paso.
- **Las imágenes** van fuera de git: `tools/dominio_red.py preparar` compila el kernel (tinyconfig, `comun.config`,
  `arm64.config` y `paso.config`: e1000, `DMA_RESTRICTED_POOL`, UIO y `/dev/mem`) y el initramfs en
  `~/recuperar/geosian-7d/dominio-red/`. En la imagen de DashOS van juntos en un objeto ("dominio-linux"): un programa
  nace con 8 llaves como mucho.

### Lo comprobado antes de escribir el servicio

En QEMU solo, con un árbol de `virt` cambiado a mano (`tools/dominio_red.py` hace lo mismo): `uio_pdrv_genirq` con
`of_id=dashos,canal-dominio` en la línea del kernel engancha el nodo y da `/dev/uio0`; `/dev/mem` proyecta una zona de
`reserved-memory` dentro de la RAM y los accesos desalineados funcionan (memoria normal); el `memory-region` en el nodo
del PCIe hace que la e1000 use la `restricted-dma-pool` ("assigned reserved memory node").
