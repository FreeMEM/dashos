# Bitácora: prestar un aparato USB a un dominio (S35, paso 2 con USB)

Rama `dominio-usb`, sacada de `revision` (5470528a, con el dominio de red por PCI ya unido). Sigue a
`docs/bitacoras/dominio-red.md`: allí el dominio llevaba una e1000 pasada por PCI sin IOMMU (paso intermedio, solo en
QEMU). Aquí el aparato es USB: lo lleva el xHCI de DashOS y se presta al dominio. Así el préstamo no necesita IOMMU y
vale en todas las máquinas.

Sala limpia: la documentación de Linux leída como especificación (`Documentation/usb/usbip_protocol.rst`, los Kconfig
de `drivers/usb/usbip` y `drivers/net/usb`), la especificación de xHCI 1.2 de Intel y la de USB 2.0 (cap. 9). El código
de Linux se ejecuta (el kernel del dominio), no se copia ni se lee. El kernel y el initramfs del dominio van fuera de
git, en `~/recuperar/geosian-7d/dominio-usb/` (`tools/dominio_red.py preparar usb`).

## 2026-11-16: cómo llega un aparato USB al dominio

### Lo mirado

| Camino | Cómo va | Qué pide |
|---|---|---|
| **USB/IP con `vhci-hcd`** (el plan de la bitácora anterior) | El dominio tiene un controlador anfitrión virtual de serie (`vhci-hcd`); se le engancha un aparato dándole un socket por sysfs (`attach`), y por el socket habla USB/IP: `CMD_SUBMIT` y `CMD_UNLINK` (las URB de Linux) y sus respuestas. El servidor lo pone DashOS | Un socket de flujo en el dominio que llegue a DashOS |
| Emular un xHCI en el dominio | El servicio pone en el dominio un xHCI entero (registros, anillos, contextos) y pasa sus transferencias al de verdad | Escribir y probar un xHCI emulado: miles de líneas, y una superficie de ataque grande (todo lo que el dominio escribe en esos registros y anillos) |
| virtio-usb | No existe en la especificación de virtio ni en Linux | — |

**Elegido: USB/IP con `vhci-hcd`.** Linux sin tocar, un protocolo pequeño y documentado, y lo que llega del dominio son
mensajes que se comprueban uno a uno (no registros ni anillos).

Y una mejora sobre el plan: **sin vsock**. El plan decía "USB/IP sobre vsock"; vsock pide emular en el dominio un
aparato virtio-vsock (el transporte virtio-mmio y el protocolo de paquetes de vsock) y otro controlador en su kernel. En
su lugar, `vhci-hcd` acepta cualquier socket de flujo: el `/init` del dominio crea un **par de sockets locales**
(`socketpair(AF_UNIX, SOCK_STREAM)`), da uno a `vhci-hcd` y pasa los bytes del otro a un **tubo**: una memoria
compartida con el servicio del dominio (dos anillos de bytes de 128 KB), con el canal del dominio (el aparato propio
de la bitácora anterior) para los avisos. Es lo mismo que ya hacían la ventana y el canal para virtio: nada nuevo en el
kernel de DashOS ni en el de Linux.

### Cómo va

```
usb-net (QEMU) ── xHCI de QEMU ──▶ xhci (DashOS): enumera, configura los puntos, hace las transferencias
                                     ▲ protocolo::usb_prestado (TOMAR, CONTROL, TRANSFERIR, CANCELAR / HECHO, QUITADO)
                                     │ los datos, en búferes DMA que presta el xHCI con el aparato (16 de 16 KB)
                                     ▼
                              servicio `dominio`: servidor de USB/IP (caja_vmm::dominio::usbip), lo comprueba todo
                                     ▲ el tubo (memoria compartida) y el canal (avisos)
                                     ▼
              /init del dominio: par de sockets ◀──▶ vhci-hcd ──▶ cdc_ether (Linux) ──▶ socket de paquetes
                                                                             │ virtio-net por la ventana (como antes)
                                                                             ▼
                                                           virtio-red (DashOS) ──▶ servicio de red
```

- **El xHCI** (`usuario/src/xhci_comun.rs`) ofrece por "usb-dominio" los aparatos que no usa él (ni teclados, ni
  ratones, ni tabletas, ni concentradores: esos no se prestan nunca). Al prestado le hace las transferencias que le
  piden, varias a la vez (hasta 16, 4 por punto) y cancelables (Stop Endpoint, los TRB a "no hacer nada", Set TR
  Dequeue: xHCI 4.6.9). Lo de la pila USB lo sigue haciendo él: `SET_ADDRESS` no llega al aparato, `SET_CONFIGURATION`
  y `SET_INTERFACE` configuran los puntos en el controlador (Configure Endpoint con los del descriptor, que lee él del
  aparato), `CLEAR_FEATURE(ENDPOINT_HALT)` reinicia el punto también aquí. Un punto atascado (STALL) se desatasca
  solo. Al devolverlo (o si el dominio se va) desactiva su ranura, reinicia el puerto y lo vuelve a configurar como
  nuevo, con otro número. Mira los puertos raíz en marcha: lo que se desenchufa (si estaba prestado, el dominio lo
  pierde) y lo que se enchufa.
- **El servicio del dominio** (`usuario/src/bin/dominio.rs`) toma el aparato que diga su tabla
  (`caja-linux/dominios/aparatos.txt`: qué aparato va a qué dominio, por su identificador o su clase) y arranca el
  dominio cuando lo tiene (a demanda). Es el servidor de USB/IP: lee del tubo, comprueba y pasa al xHCI; escribe las
  respuestas. Al acabar cada generación, el aparato vuelve al xHCI.
- **El `/init` del dominio** (`caja-linux/dominios/dominio_red.c`) engancha el aparato en `vhci-hcd` cuando el canal
  dice que hay uno (y lo suelta, cerrando su socket, cuando DashOS se lo quita), pasa bytes entre el socket y el tubo, y
  espera la tarjeta que salga en `/sys/class/net`. Lo demás (la virtio-net hacia DashOS) es lo de antes.
- **El kernel del dominio**: `usb.config` en lugar de `paso.config`: sin PCI, con USB, `vhci-hcd`, `usbnet`,
  `cdc_ether` y `rndis_host`.

### Lo que no se fía de nadie

- **El servidor de USB/IP** (`caja_vmm::dominio::usbip::Servidor`, con pruebas en el PC): un aparato que no es el
  prestado (`devid`) da `ENODEV`; un punto que no existe, un largo de más de 16 KB, isócronas, una preparación que no
  cuadra con el largo o un `seqnum` repetido dan `EINVAL`, y no salen de aquí. Lo que rompe el flujo (una orden que no
  existe, datos de salida que no caben, índices del tubo imposibles) da el dominio por roto: se relanza. Lo que tiene en
  marcha está acotado (12 de entrada, 3 de salida y 1 de control en el xHCI; 256 peticiones y 256 KB en espera); con
  eso lleno, no lee más del tubo (el dominio solo se frena a sí mismo).
- **El xHCI** comprueba lo que le pide el servicio como si viniera de fuera: el aparato tiene que estar prestado, el
  punto configurado y en su dirección, la ranura y el largo dentro, una sola de control a la vez; una configuración o
  una alternativa que no existe en el descriptor del aparato da `NO_VALE`.

### Lo pendiente del dominio (de la bitácora anterior)

- **Su registro** (S35, "Los dominios se dejan mirar"): un fichero por dominio en `Estado:Registros/`, `dominio-red`,
  con lo que dice su consola (cada línea del PL011: el kernel con `quiet` y su `/init`) y una línea por cada reinicio
  ("--- la generación N acaba: motivo"). Lo escribe `sesiones`, como los otros tres, rotando igual (64 KB y `.1`). Una
  clase nueva, "consola", que solo puede apuntar el servicio de un dominio (`registro::DOMINIOS`) y que va a su fichero,
  con su propio ritmo (ráfaga de 200 líneas y después una cada 50 ms: un arranque son decenas de líneas seguidas) para
  no comerse el de `sistema`. El servicio las manda desde un hilo propio: la vCPU no espera a `sesiones`. La orden
  `Registro` lo lee sin cambios (`Registro dominio-red`), porque recorre `registro::FICHEROS`. Las caídas siguen
  también en `sistema`.
- **El aviso a la tercera caída seguida** (S35, "Memoria"): por el servicio de avisos de siempre, con la llave
  "avisos" que le da `inicio` (como a la barra; así el servicio de avisos sabe su nombre: con una llave pedida a
  `sesiones` contestaba `PROTEGIDO`, porque no sabía quién era). "Seguidas": sin una generación
  que aguante sana un minuto entre medias; un relanzamiento pedido corta la racha. Texto: "La red está fallando — El
  controlador de la red se ha caído tres veces seguidas y se reinicia solo. El detalle, en el registro dominio-red."
  Queda además una línea en `sistema`.
- **Si `virtio-red` se cae**, `inicio` lo relanza; el servicio del dominio le dice cada 2 s si hay aparato, y cuando
  la escritura falla porque su canal se cerró, le vuelve a dar su `HOLA` por "dominio-red": el relanzado contesta,
  monta el aparato y la red vuelve.

### Lo que se retira: la e1000 por PCI

Con el USB, el paso intermedio (la e1000 pasada sin IOMMU, solo en QEMU) ya no hace falta: el servicio, `inicio` y la
prueba dejan de usarlo, y la imagen de prueba ya no presta ningún aparato PCI. Lo que vale para el paso 4 (PCI con
IOMMU) se queda: las operaciones 40 y 41 de `maquina_estado` en el kernel y `caja_vmm::dominio::pci` (con sus pruebas).
El servicio con la e1000 está en la historia (`revision` 5470528a). `tools/dominio_red.py` sigue sabiendo construir
esa variante (`preparar pci`) para medir en QEMU solo.

**Con USB, el préstamo no necesita IOMMU**: el dominio no ve ningún registro del aparato ni hace DMA; solo pide
transferencias, que comprueban el servicio y el xHCI. Vale en la Raspberry Pi y en cualquier PC. La imagen de usar
podría llevar este camino (no se ha hecho: ver "Decisiones pendientes").

### Lo que hubo que arreglar por el camino

- **`attach` de `vhci-hcd` solo se escribe**: abrirlo para leer y escribir da `EACCES`; el `/init` decía "sin
  vhci-hcd". Comprobado en QEMU solo, con un `/init` de diagnóstico que engancha un par de sockets a mano: `vhci-hcd`
  acepta un socket local (`AF_UNIX`), y lo primero que manda es `GET_DESCRIPTOR` del aparato (64 bytes), después el de
  18 y el calificador; el `SET_ADDRESS` lo hace él solo ("SetAddress Request (2) to port 0"), sin mandarlo.

### Decisiones pendientes del usuario

1. **La tabla de aparatos** (`caja-linux/dominios/aparatos.txt`, qué aparato va a qué dominio): va dentro de la imagen
   del sistema, junto al servicio (`include_str!`), y solo la cambia quien construye la imagen. S35 deja abierto dónde
   vive y quién puede cambiar una línea. Lo elegido es lo más cerrado: nadie la cambia en marcha. ¿Debería poder
   añadirse una línea en la capa del usuario (como la imagen de dominio propia de S35, solo en el modo de desarrollo)?
2. **Qué se presta**: todo lo que no sea teclado, ratón, tableta o concentrador se ofrece al dominio que lo pida, y el
   servicio solo toma lo de su tabla. Un aparato que no está en ninguna tabla se queda en DashOS sin controlador.
3. **La imagen de usar** podría llevar ya este camino (vale sin IOMMU y en la Pi): no se ha hecho; faltan las capas 1 a
   3 de S35 y medir el dominio en hardware de verdad.
4. **El aviso** dice "La red está fallando" en llano, con el detalle en `Registro dominio-red`; urgencia normal (no
   urgente). "Tres seguidas": sin una generación que aguante un minuto sana entre medias.
5. **`/dev/mem` sin `STRICT_DEVMEM`** en el dominio (como antes): ahora el `/init` lee por ahí también el tubo.
- **La interrupción del canal, perdida al arrancar**: el préstamo ya está puesto cuando arranca el dominio, así que el
  canal avisa desde el principio. UIO atiende esa interrupción al sondear el aparato, antes de que el `/init` abra
  `/dev/uio0`: la deshabilita y la cuenta, pero quien abre después no la ve (solo ve las que llegan desde que abre). Se
  quedaba deshabilitada y el `/init` no se enteraba de nada más (ni de las colas del conductor: ni una trama). Ahora el
  `/init` la habilita al abrir; si el canal sigue avisando, vuelve a llegar.
- **Un conductor relanzado no podía montar el aparato**: el servicio le contestaba que la ventana no valía, porque la
  del conductor muerto seguía puesta en la etapa 2 del dominio (el hipervisor no tiene cómo quitar un objeto de memoria
  de una máquina en marcha). Ahora, si llega otro conductor cuando el de antes ya tenía su ventana puesta, el dominio se
  relanza (`Fin::Conductor`, no cuenta como caída): cuesta un arranque del dominio (1 a 3 s, con el aparato devuelto y
  prestado otra vez). Quitar memoria de la etapa 2 en marcha queda para cuando haga falta.
- **La prueba de la basura en USB/IP** esperaba la caída por el protocolo después de la orden de relanzar; llega antes
  (se rompe nada más arrancar, antes de estar listo). Ahora mira los motivos que vio por el camino.
- **El servicio de red no aceptaba la tarjeta de un conductor relanzado**: un segundo `TARJETA` se tiraba si ya había
  tarjeta, así que tras relanzar `virtio-red` (con dominio o con PCI) la red se quedaba muda. Ahora, si es la misma MAC
  y viene del controlador, cambia la memoria de las tramas y el aviso y la pila sigue (dirección y conexiones). Cambio
  pequeño en `usuario/src/bin/red.rs` (`relevar_tarjeta`).
- **Desenchufar a media transferencia**: una conexión TCP aguanta 10 s sin respuesta (el plazo del servicio de red).
  Con 4 s desenchufado, más lo que tarda en volver, a veces se pasaba; la prueba lo deja 2 s.

### Medidas

QEMU `virt` con TCG, 4 núcleos, el PC muy cargado por otras sesiones (carga 40 a 50). Números de emulador. "Antes":
`make arranque-virt-dominio` de `revision` (la e1000 por PCI), corrido hoy en esta rama antes de tocar nada.
"Después": el adaptador USB (usb-net de QEMU, plena velocidad), en las pruebas 4 y 6 de este paso (17 arranques).

| | Antes (e1000 por PCI) | Después (USB por USB/IP) |
|---|---|---|
| Lo que mide la máquina del dominio | 27 648 KB: 24 MB de RAM, 1 MB de ventana y 2 MB de zona de DMA | 25 860 KB: 24 MB de RAM, 1 MB de ventana y 260 KB de tubo |
| Lo que ve Linux (MemTotal) | 17 960 KB | 18 088 KB |
| Usada por Linux (MemTotal − MemFree) | 3 672 a 3 688 KB | 3 352 a 3 376 KB |
| La máquina menos lo libre (como en S35) | 13 360 a 13 376 KB (13,1 MB) | 11 124 a 11 148 KB (10,9 MB) |
| Kernel del dominio (Image) | 4,1 MB | 3,9 MB (4 053 000 bytes) |
| "Listo" desde que arranca la máquina | 696 a 1 075 ms (6 arranques) | 930 a 1 577 ms (14 normales); 2 410 a 2 723 ms en 3 con más carga; 2 654 a 2 714 ms con los hostiles de USB/IP antes de enganchar |
| Reloj del dominio al decir "listo" | 570 a 880 ms | 790 a 1 320 ms (normales) |
| Eco TCP de 64 KB | 35 ms | 106 a 169 ms |
| Eco TCP de 256 KB con el dominio cayendo (colgado / muere) | 7,3 s / 7,2 s | 4,7 a 4,9 s / 1,9 a 2,4 s |

- **Memoria: 2,2 MB menos.** Sin la zona de DMA (2 MB) ni el PCIe en el dominio; Linux usa 0,3 MB menos (sin PCI ni la
  e1000; con USB, `vhci-hcd`, `usbnet` y `cdc_ether`).
- **Arranque: unos 0,3 s más.** Lo que se añade es enumerar el aparato por USB/IP: cada transferencia de control va del
  dominio al tubo, al servicio, al xHCI, al aparato de QEMU y vuelta.
- **Red: unas 3 a 5 veces más lenta en el eco corto**, y la caída de un dominio se recupera antes (el aparato vuelve
  limpio y se presta en ~1 s; la e1000 necesitaba su reinicio). El usb-net de QEMU es de plena velocidad.
- **El servicio** cuenta lo de USB/IP: una tormenta de 6 000 peticiones (2 000 de control, 2 000 de interrupción y
  2 000 cancelaciones) se atiende entera en 0,95 a 1,0 s, sin pasar de 16 en el xHCI.
- **Desenchufar a media transferencia**: el eco de 256 KB acaba en 4,8 a 7,7 s (2 o 4 s sin adaptador).
- **Volver a presentarse a un conductor relanzado**: la red vuelve a los 2,1 s.
- No medido: lo que gasta DashOS además de la máquina (la memoria del servicio y del xHCI: 64 KB de anillos y
  256 KB de búferes por préstamo), el mínimo de RAM del dominio con USB, y nada en hardware.

### Pruebas

- `make arranque-virt-dominio` (`kernel/pruebas/arranque_virt_dominio.txt`, `dominio-prueba`): bien, en la séptima
  vuelta (las anteriores sacaron los fallos de "Lo que hubo que arreglar"). Con el teclado y el ratón USB de siempre en
  el mismo xHCI (no se prestan) y el adaptador en el puerto 7.
  - normal: DHCP por el dominio, eco TCP de 64 KB y eco UDP con el vecino;
  - hostil, virtio y USB: la oferta que miente, usados ajenos, una tormenta de avisos, leer fuera (más allá de la
    ventana y del tubo, donde estuvo la RAM con la e1000 y fuera de todo: aborto y SIGBUS); por USB/IP, 4 peticiones
    para otros aparatos (las 4, `ENODEV`), 11 falsas o enormes (largo de 4 GB, `wLength` que no cuadra, 16 KB de
    descriptor —el aparato de QEMU contesta STALL y el punto se desatasca—, el punto 16, isócronas, `SET_ADDRESS`, la
    configuración 77, un punto sin configurar, `SET_DESCRIPTOR` con basura, cancelar lo que no existe, la dirección al
    revés) y la tormenta de 6 000; el servicio pide al xHCI los aparatos 1 a 12: ni el concentrador, ni el teclado, ni
    el ratón, ni los que no existen se prestan. La red sigue;
  - hostil, basura (una orden que no existe): el dominio se da por roto (motivo 9) y se relanza;
  - se desenchufa a media transferencia (`device_del` y `device_add` por el monitor de QEMU): el xHCI lo ve, el dominio
    lo suelta, vuelve como aparato nuevo, se presta otra vez y el eco de 256 KB acaba;
  - se cuelga y muere con el aparato prestado: el vigilante lo relanza, el xHCI desactiva la ranura, reinicia el puerto
    y lo deja como nuevo ("vuelve limpio como aparato N") y el eco acaba;
  - se cae el conductor (`virtio-red`, por `sesiones`): `inicio` lo relanza, el dominio se relanza para presentarle un
    aparato nuevo y la red vuelve;
  - tres caídas seguidas: el aviso ("[avisos] dominio avisa: \"La red está fallando\"");
  - los registros: `sistema` con 14 caídas y `dominio-red` con la consola de 14 arranques y 13 reinicios.
- `cargo test` en `caja-linux/vmm`: 86 pruebas (8 nuevas del servidor de USB/IP: de punta a punta, peticiones que no
  valen, lo que rompe el flujo, lo que se deja en marcha, cancelar, tormenta con la vuelta llena y 300 flujos de basura
  al azar; y las del canal y el árbol con el tubo).
- `usb_prestado` (3 pruebas en el PC: todos los mensajes, 200 000 al azar y las comprobaciones) y `registro` (la clase
  "consola" y su ritmo), en `make prueba`.
- `make arranque-virt-usb`, `make arranque-virt-linux` y `make arranque-virt-2`: bien. `make prueba`: bien a la
  segunda (la primera, "C14: clase de plazo" con la carga del PC en 94).
- `make arranque-virt`: falla en las dos vueltas solo por clics del guion perdidos con carga, el fallo conocido de
  `revision` que arregla `firefox-firme`: "[ajustes] menú abierto; la segunda opción en (" la primera y "[ajustes] el
  desplegable dice: Destello" la segunda (y detrás, en cadena, lo que el guion ya no llega a hacer: el Shell, `guarda`
  y la commodity). El teclado y el ratón por el xHCI llegan ("[ajustes] el campo dice: DashOSa"), y las otras pruebas
  con USB (`arranque-virt-usb`, `-linux`, `-2`) salen bien.

### Lo que queda

- **El ep0 de los aparatos de plena velocidad** se configura con paquetes de 8 bytes y no se corrige con el
  `bMaxPacketSize0` de verdad (Evaluate Context): ya era así; QEMU no lo nota, un aparato de verdad de 64 puede dar
  errores de babble. Hay que hacerlo antes de probar en hardware.
- **Concentradores en marcha**: lo que se enchufa o desenchufa detrás de un concentrador no se ve (solo los puertos
  raíz); un concentrador enchufado en marcha no se recorre. Un aparato detrás de un concentrador sí se presta y se
  devuelve (reiniciando el puerto del concentrador), pero no se ha probado.
- **Isócronas** (cámaras, sonido USB): no se prestan (`EINVAL`).
- **Varios aparatos por dominio**: el xHCI presta hasta dos a la vez; el servicio toma uno.
- **Quitar memoria de la etapa 2** en marcha: con ella, un conductor relanzado no obligaría a relanzar el dominio.
- **Menos salidas**: cada aviso del tubo es una salida al servicio; juntar avisos (como `EVENT_IDX` en virtio).
- Las capas 1 a 3 de S35, el dominio en `Status` y el tope de memoria medido (sigue de la bitácora anterior).

### Lo que no se ha comprobado

- Nada en hardware: ni un xHCI de verdad, ni un adaptador de verdad, ni la Pi.
- Un aparato de alta o super velocidad (el usb-net de QEMU es de plena): los puertos de super velocidad de `vhci-hcd`
  y los intervalos de alta velocidad están escritos y sin probar.
- Un aparato que hace STALL en un punto masivo (el camino de desatascar se probó en el punto 0).
- Cancelar una transferencia a medias de un punto masivo con otras detrás (lo de la tormenta son de interrupción que no
  han empezado).
