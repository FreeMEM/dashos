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

### Lo que hubo que arreglar por el camino

- **La BAR de puertos de la e1000** (arriba): sin ella, pánico al reiniciar la tarjeta.
- **Ocho llaves como mucho por programa**: el kernel y el initramfs del dominio van en un objeto, y el permiso de
  máquinas lo pide el servicio a `sesiones` (como la caja Linux, que es lo que decidió el usuario el 2026-11-12):
  `SIN_PREGUNTAR` lleva ahora también ("dominio", "maquinas"). Con la misma llave de `sesiones` apunta en el registro
  común.
- **4 MB de más en el dominio**: con la zona de DMA (1 a 2 GB), la RAM (en 2 GB) y la ventana (en 2,5 GB) en tres
  secciones de 128 MB de la memoria de Linux, su tabla de páginas era el triple (2 MB por sección). Ahora la RAM y la
  ventana van junto a la zona de DMA, en su misma sección (`dominio::colocar`): "reserved" baja de 14 184 KB a
  10 080 KB, lo mismo que en QEMU solo (10 144 KB).
- **El HOLA del servicio llega tarde**: `inicio` lanza el servicio del dominio el último, minutos después en QEMU sin
  aceleración; la prueba espera hasta 15 minutos, de segundo en segundo. Y por los canales de servidor llega también lo
  que dice `inicio` de sus clientes: se lee con un búfer de 256 bytes y se ignora.
- **Las llaves por nombre** (`dashos::llaves::buscar`): los canales llegan en el orden en que se nombran por primera vez
  en la lista de `inicio`, no en el de cada programa.

### Pruebas

- `make arranque-virt-dominio` (nueva, `kernel/pruebas/arranque_virt_dominio.txt`):
  - normal: DHCP por el dominio (10.0.2.15), eco TCP de 64 KB con 10.0.2.100:7 y eco UDP con el vecino;
  - hostil, oferta: el dominio ofrece primero un aparato de disco con una MAC de grupo; el servicio la rechaza (y lo
    apunta en el registro común) y acepta la buena;
  - hostil, descriptores: el dominio devuelve usados que no son del conductor (índices fuera, repetidos, largos de 4 GB
    y de 2 bytes) y escribe 20 000 avisos seguidos; `virtio-red` los cuenta y los ignora ("9 devoluciones ajenas y 3 de
    largo imposible"), el servicio cuenta 10 000 accesos raros a su canal, y la red sigue (eco TCP y UDP);
  - hostil, fuera: el dominio lee donde DashOS tiene su kernel en la máquina (0x40200000), más allá de la ventana y en
    0xf0000000: el hipervisor le da un aborto cada vez ("el dominio toca 0x... donde no tiene nada") y el hijo que lo
    intenta muere con SIGBUS;
  - hostil, se cuelga: con un eco TCP de 256 KB en marcha, el dominio deja de atender y de latir; a los 3 s el vigilante
    lo da por colgado, apaga la tarjeta y lo relanza; el eco acaba (TCP reintenta);
  - hostil, muere: con otro eco de 256 KB en marcha, su /init acaba, su kernel entra en pánico y pide reiniciar; se
    relanza y el eco acaba;
  - el registro común (`Estado:Registros/sistema`, leído por `sesiones`) tiene una línea por cada caída o reinicio,
    con el motivo, y la de la oferta rechazada:
    `2026-11-16T14:50:42Z dominio sistema servicio el dominio de red cae (generación 3): dejó de latir (colgado)`.
  - El vigilante, además, sin querer: con la BAR de puertos sin emular, el dominio entraba en pánico al arrancar; se
    relanzó 5 veces y a la sexta caída en un minuto se dejó apagado ("el dominio cae sin parar").
- `cargo test` en `caja-linux/vmm`: 11 pruebas nuevas (canal, ofertas, PCIe, puertos, árbol, `colocar`).
- `virtio_msg`: 3 pruebas en el PC (todos los mensajes, mensajes malos y 200 000 al azar, colas fuera de la ventana),
  en `make prueba`.
- `make prueba`, `make arranque-virt`, `make arranque-virt-2` y `make arranque-virt-linux`: BIEN (con el refactor de `virtio-red`, la red de
  `arranque-virt` sigue por virtio-net-pci como antes).

### Medidas

QEMU `virt` con TCG, 4 núcleos, con el PC cargado por otras sesiones (carga 6 a 10). Números de emulador.

| | En DashOS (servicio `dominio`) | En QEMU solo (`tools/dominio_red.py qemu 27`) |
|---|---|---|
| Memoria de la máquina del dominio | 27 MB: 24 de RAM, 1 de ventana y 2 de zona de DMA | 27 MB, con las mismas zonas reservadas dentro |
| Lo que ve Linux (MemTotal) | 17 960 KB | 17 896 KB |
| Usada por Linux (MemTotal − MemFree) | 3,6 MB | 3,7 MB |
| La máquina menos lo libre (como en S35) | 13,0 MB (13 348 a 13 364 KB) | 13,2 MB (13 544 KB) |
| Mínimo para arrancar | no medido (24 MB de RAM) | 14 MB |
| "Listo" (la oferta hecha) desde que arranca la máquina | 0,68 a 0,83 s (seis arranques) | 0,58 s desde que se lanza QEMU (mediana de 5) |
| Reloj del dominio al decir "listo" | 0,55 a 0,69 s | 0,50 s |

- La memoria es la misma dentro de DashOS que en QEMU solo, y cuadra con S35 (13 MB mínimo, 12 en reposo para el
  dominio de red con una CPU: aquí, además, UIO, la zona de DMA y la ventana).
- El arranque, un 10 a 30 % más lento dentro de DashOS: en QEMU solo, los aparatos los emula QEMU en C; aquí, cada
  acceso al PL011, al PCIe emulado y al canal es una salida al servicio (unas 3 200 salidas por arranque, 2 800 de ellas
  a aparatos). Con `quiet`, la consola casi no cuenta.
- Lo que gasta DashOS además de la máquina del dominio (las tablas de la etapa 2, el montón y la pila del servicio) no
  se ha medido; el servicio tiene un tope de 40 MB.
- Red por el dominio: eco TCP de 64 KB en 28 a 40 ms; con el dominio cayendo a media transferencia, el eco de 256 KB
  tarda 7,1 s (3 s del vigilante, 0,7 de arrancar otro y lo que espera TCP para reintentar).

### Decisiones pendientes del usuario

1. **PCI sin IOMMU, solo en QEMU**: la imagen de prueba presta la e1000 a un dominio que podría hacerla escribir en
   toda la memoria. Se queda solo en `virt,dominio` y como paso intermedio; ¿vale así hasta tener el USB?
2. **`/dev/mem` sin `STRICT_DEVMEM` en el dominio**: el /init lee la ventana por `/dev/mem`, así que root en el dominio
   puede leer y escribir toda la memoria del dominio (no la de DashOS). Se eligió porque da la ventana como memoria
   normal (con caché, igual que la ve DashOS); la alternativa, UIO, la da como memoria de dispositivo y en hardware
   de verdad no sería coherente. Encaja con la capa 3 (el código del kernel del dominio, protegido desde fuera), que
   está sin hacer.
3. **El permiso de máquinas del dominio, en el centro de control**: se da sin preguntar, como a la caja Linux. Si
   alguien lo quita, al siguiente relanzamiento se pregunta; sin él, no hay red. ¿Un dominio de controladores debería
   salir entre los permisos que se pueden quitar?
4. **El registro del dominio**: cada caída va al fichero `sistema` del registro común (clase "servicio"). S35 pide
   además un fichero por dominio con su consola: hoy la consola del dominio solo sale por el registro del kernel de
   DashOS (con "linux:" delante). Falta decidir el formato.
5. **El vigilante**: 3 s sin latido es colgado; más de 5 caídas en un minuto, se deja apagado. S35 pide además un
   aviso en llano a la tercera seguida: no hecho.

### Lo que queda

- **El USB** (el plan de arriba): sin él, este paso de S35 no vale fuera de QEMU.
- Las capas 1 a 3 (paso 3). Nada de lo hecho las cierra: el kernel y el initramfs del dominio son un objeto de datos
  que el servicio solo lee (se puede comprobar su firma antes de cargarlo), sin módulos, y la etapa 2 ya distingue
  memoria y aparatos (se puede quitar la escritura o la ejecución de páginas del dominio).
- El dominio en `Status` y en el Monitor del sistema; el tope de memoria medido y la alarma (capa 6).
- Menos salidas: supresión de avisos (VIRTIO_F_EVENT_IDX) y juntar los USADOS de una ráfaga. Una tormenta de avisos
  cuesta una salida por escritura (acotado por el tiempo de CPU del dominio) y un mensaje al conductor por salida (los
  que no caben en su cola se pierden, sin daño).
- Medir lo que gasta el servicio en DashOS, y el mínimo de RAM del dominio dentro de DashOS.
- Si se cae `virtio-red` (el conductor), `inicio` lo relanza pero el servicio del dominio no le vuelve a mandar su
  `HOLA`: la red se queda sin tarjeta hasta reiniciar. Hay que volver a presentarse (o que el conductor lo pida).

### Lo que no se ha comprobado

- Nada en hardware de verdad. En QEMU no hay cachés: la coherencia entre la ventana vista por DashOS (con caché) y por
  el dominio (`/dev/mem` de RAM reservada, que Linux da con caché; no comprobado en hardware), y la de la zona de DMA
  (sin caché para DashOS, normal en la etapa 2) están sin probar.
- Una INTx compartida con otro aparato: en la prueba, la e1000 tiene la suya sola.
- Mucho tráfico seguido ni una inundación de tramas desde fuera.
- El dominio con más de una vCPU.
