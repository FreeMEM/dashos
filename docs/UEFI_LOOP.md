# DashOS por UEFI: plan y bitácora

Encargo del usuario (2026-11-04, `PLAN.md`, "Otros ordenadores ARM"; puesto en marcha el 2026-11-06, "1, 2 y 3, en
loop"): DashOS arranca directamente por UEFI en ordenadores ARM (portátiles con Snapdragon, Mac con Apple Silicon a
través del U-Boot de Asahi, placas con UEFI), sin máquina virtual. Se desarrolla en QEMU `virt` con EDK2
(`/usr/share/edk2/aarch64/QEMU_EFI.fd`), porque no se puede comprar todo el hardware.

Rama `uefi` de `~/Projects/dashos`, en `.worktrees/uefi`. Se fusiona en `main` por partes que funcionen.

## Decisiones

- **El programa de UEFI es aparte del kernel** (`uefi/`, objetivo `aarch64-unknown-uefi`): hace lo que en la Pi hace
  su firmware (cargar el kernel y darle el árbol de dispositivos) y nada más. El kernel sigue igual para la Pi.
- **Árbol de dispositivos primero** (U1, 2026-11-06). EDK2 en `virt` solo lo da con `acpi=off`; los Mac (U-Boot)
  también dan árbol. Los portátiles con Snapdragon dan ACPI: va después (U5).
- **GICv3** en las pruebas (`gic-version=3`): el de los Snapdragon y los servidores. Los Mac tienen el AIC de Apple,
  que es otro controlador (cuando haya Mac).

## Tareas

- [x] U1 **Programa de UEFI mínimo.** EDK2 lo arranca desde `EFI/BOOT/BOOTAA64.EFI`; escribe por la consola de UEFI,
  encuentra el árbol en la tabla de configuración, sale de los servicios de arranque y escribe solo en el PL011.
- [x] U2 **Cargar el kernel** (con un núcleo de prueba; el kernel de verdad, en U3). El programa de UEFI lleva dentro (o lee del disco) la imagen del kernel, la pone en
  memoria y salta a ella con el árbol en `x0`, como el firmware de la Pi. Hace falta que el kernel no dependa de
  dónde se carga (en `virt` la RAM empieza en 0x40000000) y que respete las zonas que UEFI reserva (mapa de memoria:
  se pasan al kernel como `/memreserve/` o en un nodo del árbol).
- [x] U3 **El kernel en `virt`**, con el GICv2 de `virt`: `make -C kernel arranque-virt` llega a `inicio` con los
  cuatro núcleos (PSCI) y la consola como servicio.
- [x] U3b **GICv3** (redistribuidores por núcleo, registros del sistema `ICC_*`), para `gic-version=3` y las máquinas
  de verdad.
- [x] U3c **Las pruebas del kernel en `virt`** (`make -C kernel prueba-virt`).
- [x] U3d **Memoria que vuelve tarde con GICv3** (era del planificador, no de `virt`).
- [x] U4 **La pantalla del firmware (GOP).** El programa de UEFI pasa al kernel el búfer de pantalla de GOP
  (dirección, tamaño, paso, formato); el compositor pinta ahí cuando no hay buzón de la VideoCore. En QEMU, `ramfb`
  o `virtio-gpu`.
- [x] U5 **ACPI.** Para los portátiles: leer MADT (interrupciones y núcleos), GTDT (temporizador) y SPCR (puerto
  serie), o convertirlo a árbol en el programa de UEFI.
- [~] U6 **Controladores estándar.** Hecho: U6a (el kernel encuentra el xHCI en el PCIe), U6b (el servicio `xhci`
  lo reinicia), U6c (teclado y ratón por xHCI hasta el compositor), U6d (NVMe: reconoce el disco y lee), U6e (la
  INTx del árbol para el xHCI), U6f (el NVMe sirve el disco por un canal, con ventanas DMA compartidas) U6g (MSI-X
  por el GICv2m, también con ACPI) y U6h (MSI-X por el ITS del GICv3). Falta: hubs, el anillo de peticiones del disco,
  leer el `msi-map` y el IORT. xHCI (`qemu-xhci`) y NVMe (`nvme`), como procesos, igual que el USB de la Pi.
- [ ] U7 **La caja Amiga sin big-endian.** El modo del JIT que da la vuelta a los bytes, para las CPU sin big-endian
  en EL0 (Apple). Se prueba en `virt`.

## Herramientas

- `make -C uefi/prueba` saca el núcleo de prueba de U2 (`nucleo.bin`, en C, enlazado en 0x40200000).
- `cd uefi && cargo build --release` (con `DASHOS_NUCLEO=<imagen>` para meter otra) saca `uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi`.
- `make -C kernel prueba-virt`: la imagen de pruebas en `virt` (`pruebas/esperado_virt.txt`).
- `make -C kernel arranque-virt`: el kernel con la característica `virt`, metido en el programa de UEFI y arrancado.
- `tools/prueba_uefi.sh <programa.efi> <esperados> [segundos]`: QEMU `virt` (GICv2, o `GIC=3`; `TECLAS=1` manda una
  "a" y un movimiento del ratón cada medio segundo; `ACPI=on` para
  arrancar con ACPI y sin árbol, como un portátil; `MAQUINA=...` añade
  opciones a la máquina, como `virtualization=on` para entrar en EL2; 4 núcleos, 2 GB, `acpi=off`) con
  EDK2 y el programa como `EFI/BOOT/BOOTAA64.EFI` en un disco FAT; comprueba el puerto serie como `prueba_qemu.sh` y
  para en cuanto sale la última línea esperada.

## Bitácora

### 2026-11-06 noche — U1 (programa de UEFI mínimo)

- `uefi/src/main.rs`, escrito con la especificación de UEFI 2.10 (tabla del sistema, servicios de arranque, salida
  de texto) y la de devicetree: 7 KB. Busca el árbol por su GUID (b1b621d5-...), pide el mapa de memoria y sale de
  los servicios de arranque (repite si el mapa cambió), y escribe en el PL011 de `virt` (0x09000000, fijo por ahora).
- Con ACPI, EDK2 no da árbol: las pruebas van con `acpi=off` (decisión arriba).
- La consola de UEFI saca "?" en lugar de las letras con tilde (el terminal de EDK2 en modo PC-ANSI). No importa para
  el kernel, que escribe directo en el puerto serie.
- **Comprobado:** `tools/prueba_uefi.sh ... uefi/pruebas/u1.txt` bien, en 6 s. **No comprobado:** ninguna máquina
  real.

### 2026-11-07 madrugada — U2 (cargar el kernel)

- El cargador lleva la imagen dentro (`include_bytes!` de `DASHOS_NUCLEO`) y la pone en 0x40200000 con
  `AllocatePages` en esa dirección: el kernel no es reubicable (está enlazado para una física fija), así que si UEFI
  la tuviera ocupada no se puede seguir, y lo dice. Detrás reserva 2 MB más.
- Copia el árbol con las zonas que UEFI se queda tras salir (servicios de ejecución, ACPI, reservado, inservible,
  persistente) añadidas a su mapa de reservas, juntando las seguidas. Así el kernel no las reparte, sin cambiar su
  lectura de memoria (ya respeta `/memreserve/`). Sale de los servicios de arranque con la llave de ese mapa; si
  cambió, otra vuelta.
- Lleva la imagen y el árbol hasta el punto de coherencia (`dc civac`), apaga MMU y cachés en EL1 o EL2 y salta con
  el árbol en `x0`, como el firmware de la Pi.
- Núcleo de prueba (`uefi/prueba/nucleo.c`): con la MMU apagada escribe dónde está, dónde le llegó el árbol, su
  tamaño y cuántas reservas trae. En QEMU: árbol de 8769 bytes con 5 zonas de UEFI.
- **Comprobado:** `tools/prueba_uefi.sh ... uefi/pruebas/u2.txt`, bien. **No comprobado:** máquinas reales; un
  firmware que deje el núcleo en EL2 (en QEMU `virt` EDK2 corre en EL1; con `virtualization=on`, en EL2).
- Siguiente, U3: el kernel de verdad en `virt`. Hace falta enlazarlo en 0x40200000 (o hacerlo reubicable), sacar del
  árbol el PL011 (hoy fijo en 0xFE201000), un GICv3, PSCI para los otros núcleos y unas tablas de arranque que no den
  por hecho el mapa de la Pi (periféricos en 0xFC000000).

### 2026-11-07 madrugada — U3 (el kernel en `virt`, con GICv2)

- **Característica `virt`** del kernel: se elige al compilar, porque el kernel está enlazado para una física fija
  (la Pi en 0x80000, `virt` en 0x40200000: `enlace_virt.ld`, que elige `build.rs`). Lo que cambia de placa está en
  `kernel/src/base/placa.rs`: la ventana de periféricos (Pi: 0xFC000000 a 4 GB; `virt`: el primer GB entero) y el
  PL011 de antes de leer el árbol. `mapa.rs`, `espacio.rs`, `mmio.rs` y el árbol la usan en vez de 0xFC000000.
- **Tablas de arranque** (`arranque.s`, `.if PLACA_VIRT`): en `virt`, el primer GB de dispositivo y los otros tres
  normales, en bloques de 1 GB.
- **Mapa del kernel** (`mapa.rs`): solo los bloques de 2 MB que tocan la imagen van en páginas; antes, todo lo que
  había por debajo de su final (en `virt` habría sido el primer GB entero en páginas de 4 KB). En la Pi, igual.
- **PSCI** (`nucleos.rs`): si el árbol trae `/psci`, los núcleos 1 a 3 se encienden con CPU_ON por `hvc` o `smc`
  según su `method`; si no, la tabla de espera de la Pi.
- **GIC:** el GICv2 de `virt` ("arm,cortex-a15-gic") se maneja como el GIC-400 de la Pi.
- **`inicio`** no sondea la V3D donde no es zona de aparatos (en `virt` caía en un `assert`). Sin buzón, USB,
  pantalla ni GPU, se lanzan la consola y `hola`.
- **Comprobado:** `make -C kernel arranque-virt` bien; en la Pi, `make -C kernel prueba` (C14 falla a ratos por la
  carga de la máquina, media de 5) y `make -C kernel arranque`, bien. **No comprobado:** GICv3, ACPI, máquinas de
  verdad; las pruebas del kernel en `virt`.

### 2026-11-07 madrugada — U3b (GICv3)

- `kernel/src/gic.rs` maneja las dos versiones y elige por el árbol ("arm,gic-v3" si no hay GICv2). GICv3: el
  distribuidor con afinidades (ARE) y las interrupciones de aparatos en el grupo 1, dirigidas con `IROUTER`; un
  redistribuidor por núcleo, que se busca por su afinidad en `GICR_TYPER` y se despierta (`GICR_WAKER`), con las
  privadas y las de software; la interfaz de CPU por registros del sistema (`ICC_SRE_EL1`, `PMR`, `BPR1`,
  `IGRPEN1`, `IAR1`, `EOIR1`, `SGI1R`), con envoltorios en `base/cpu.rs` (el `asm`, solo en `base`).
- Si el núcleo entra en EL2 con un GICv3, `arranque.s` pone `ICC_SRE_EL2` para que EL1 pueda usar esos registros.
- Escrito con la arquitectura pública de ARM (GICv3/v4, IHI 0069). Supone una sola seguridad (`GICD_CTLR.DS` = 1,
  como QEMU sin EL3) y solo la afinidad 0 (hasta 16 núcleos en un grupo): suficiente para `virt`, no para máquinas
  con varios grupos de núcleos.
- **Comprobado:** `arranque_virt.txt` bien con GICv2 y GICv3, entrando en EL1 y en EL2 (`virtualization=on`).
  **No comprobado:** máquinas de verdad; firmware con EL3 (seguridad doble).

### 2026-11-07 madrugada — U4 (la pantalla de UEFI)

- El cargador busca el GOP (`LocateProtocol`) y, si tiene búfer lineal de 32 bits, añade al árbol un nodo
  `simple-framebuffer` (el de Linux: `reg`, `width`, `height`, `stride`, `format`), al final de la raíz. En QEMU `virt`
  solo `ramfb` da GOP con este EDK2 (`bochs-display` y `virtio-gpu-pci`, no): 800×600 en RAM, en una zona que UEFI se
  reserva.
- El kernel lee ese nodo y da a `inicio` la pantalla como memoria del firmware y sus medidas en un objeto de memoria
  (tipo nuevo de aparato, 6 "datos"). La memoria del firmware vale ahora también dentro de una zona reservada del
  árbol (`Paginas::dentro_de_una_reserva`), nunca en RAM que reparta el kernel.
- El compositor, si su llave 1 se proyecta como pantalla, pinta en ella con un solo búfer (GOP no tiene
  desplazamiento virtual), y da la vuelta a R y B si el formato lo pide; si no, sigue con el buzón, como en la Pi.
- `tools/prueba_uefi.sh` guarda la pantalla con `CAPTURA=fichero.ppm` y espera a la última línea esperada, lleve "~"
  o no. `make -C kernel arranque-virt` va con `-device ramfb`.
- **Comprobado:** `arranque_virt.txt` con GICv2 y GICv3; la captura tiene el fondo, el tablero y la ventana con los
  colores que pinta el compositor. En la Pi, `make -C kernel prueba` y `arranque`, bien. **No comprobado:** máquinas
  de verdad (otro formato de píxel, pantallas que no son de 32 bits).

### 2026-11-07 madrugada — U3c (las pruebas del kernel en `virt`)

- `make -C kernel prueba-virt`: la imagen de pruebas con la característica `virt`, dentro del programa de UEFI, en
  QEMU `virt` con `ramfb`. `pruebas/esperado_virt.txt` tiene los veredictos que valen en las dos placas; las pruebas
  de la Pi sin aparato aquí (buzón, USB, SD con tarjeta, DMA de la SD) se saltan solas.
- **Zona de DMA de `virt`:** el primer GB de RAM (1 a 2 GB, `placa::LIMITE_DMA`), porque allí la RAM empieza en
  1 GB. C5 y C38 miran ese límite y no 1 GB fijo; C5 ya no libera lo que no obtuvo (caía con la zona vacía).
- **Comprobado:** `prueba-virt` bien con GICv2 (cuatro de cuatro); en la Pi, `prueba` y `arranque` bien.
- **U3d, abierto: con GICv3, a veces C43 da MAL.** Tras `lanzador` faltan 64 a 77 páginas (unos 300 KB). No es una
  pérdida para siempre: vuelven en vueltas posteriores de `lanzador` (repitiendo la prueba de `grupos` diez veces en
  un arranque, los saldos van +64, -64, +68, -68...). En un arranque ocioso no vuelven en 10 s. Solo con GICv3 y con
  `grupos` (que mata grupos a la vez mientras se crean procesos); con GICv2, nunca en ocho tandas. Los tics llegan a
  los cuatro núcleos (100 por segundo cada uno). Sospecha sin comprobar: algo que solo se suelta con actividad en un
  núcleo concreto (un hilo muerto que no se recoge, o un objeto retenido en una cola), que la temporización del
  GICv3 destapa. Falta encontrarlo: no está claro que sea solo de `virt`.

### 2026-11-07 madrugada — U3d (hilos muertos sin recoger)

- **Causa** (con contadores puestos un momento en el kernel): en el núcleo 1 quedaban de 16 a 30 hilos muertos sin
  recoger, 4 páginas de pila cada uno. Ese núcleo nunca quedaba ocioso (lo ocupan los hilos del kernel que giran de
  C10), y los muertos solo se soltaban desde el hilo ocioso de su núcleo o en una llamada al sistema hecha en él. Con
  GICv3 cambia en qué núcleo mueren los hilos de `grupos`; por eso salía ahí. Afecta a cualquier núcleo siempre
  ocupado sin llamadas al sistema (cálculo puro): no es de `virt`.
- **Arreglo** (`base/planificador.rs`): tras cada cambio de hilo, el que vuelve pasa los muertos de su núcleo a una
  lista global enlazada (`sig_muerto`, sin reservar memoria, que vale en la interrupción) y avisa al repartidor del
  núcleo 0, que los suelta (`recoger_global`). El repartidor mira la lista con su cerrojo tomado antes de dormir, y
  quien apunta avisa tomándolo: no se pierde el aviso. La pila se suelta invalidando la TLB en todos los núcleos.
- **Comprobado:** `prueba-virt` cuatro de cuatro con GICv3 y dos de dos con GICv2, sin páginas perdidas; en la Pi,
  `prueba` y `arranque`, bien.

### 2026-11-07 madrugada — U5 (ACPI)

- Si el firmware no da árbol (los portátiles con Snapdragon dan solo ACPI), el cargador lo hace desde ACPI
  (`uefi/src/acpi.rs`, con la especificación ACPI 6.5): la memoria del mapa de UEFI (todo salvo lo reservado, lo
  inservible y la entrada y salida proyectadas), el GIC de la MADT (GICD y su versión, GICR o la interfaz de CPU de
  GICC), el temporizador de la GTDT, el PL011 de la SPCR (tipos 3 y 0x0E) y PSCI de la FADT (`hvc` o `smc`). El
  kernel no cambia: lee el árbol de siempre. `uefi/src/fdt.rs` escribe un árbol desde cero, sin montón.
- `tools/prueba_uefi.sh` arranca con ACPI con `ACPI=on`.
- **Comprobado:** con `ACPI=on`, `arranque_virt.txt` con GICv2 y GICv3, y `esperado_virt.txt` con GICv3, bien; con
  árbol, igual que antes. **No comprobado:** un portátil de verdad. Su ACPI trae más (varios grupos de núcleos, el
  SMMU, el DSDT con los aparatos) y su puerto serie no es siempre un PL011; el kernel aún escribe al principio en el
  PL011 de `virt` (`placa::SERIE`).

### 2026-11-07 madrugada — U6a y U6b (xHCI)

- **Kernel** (`inicio.rs`, `xhci_pci`): en el PCIe genérico de ECAM ("pci-host-ecam-generic") busca en el bus 0 un
  aparato de clase 0x0C0330, mide su BAR 0, enciende la memoria y el bus master, y da a `inicio` sus registros y un
  permiso DMA propio ("dma-xhci"). Solo si el ECAM y la BAR caen en la ventana de aparatos: en `virt` hace falta
  `highmem=off` (si no, el ECAM está por encima de 256 GB). Con la especificación de PCI.
- **Servicio `xhci`** (`usuario/src/bin/xhci.rs`, con la especificación xHCI 1.2): para el controlador, lo reinicia,
  devuelve la cuarentena de su permiso y dice qué puertos tienen algo. En QEMU: versión 1.00, 8 puertos, el teclado y
  el ratón en los puertos 5 y 6.
- **`inicio`:** el compositor nombra "entrada" antes que los controladores USB, para ser quien lee (el servidor del
  canal); el USB de la Pi y el xHCI le escriben con copias marcadas.
- **Fallo encontrado de paso:** el compositor leía la entrada con un búfer de 16 bytes; el directorio de `inicio` (24)
  daba `E_TAMANO`, se quedaba el primero de la cola y el compositor ya no leía nada. Ahora lee con 256 y no mira las
  llaves si la lectura falla.
- **Comprobado:** `arranque-virt` (con `qemu-xhci`, teclado y ratón) bien; en la Pi, `prueba` y `arranque` (dos
  veces), bien.

### 2026-11-07 madrugada — U6c (teclado y ratón por xHCI)

- `usuario/src/bin/xhci.rs`, con la especificación xHCI 1.2: anillo de órdenes y de eventos (un segmento),
  ranuras, contextos de 32 o 64 bytes según `HCCPARAMS1.CSZ`, reinicio de cada puerto con algo, "pedir ranura" y
  "dar dirección", transferencias de control por el punto 0 (preparación, datos, estado), descriptores de aparato y
  de configuración, la interfaz HID de arranque, SET_CONFIGURATION, SET_PROTOCOL y SET_IDLE, "configurar punto" con
  su intervalo, y un TRB normal por informe. Mira los eventos en cada tic de un temporizador de 10 ms (aún sin
  interrupción). Toda su memoria en un objeto DMA de 128 KB.
- Los informes pasan a eventos con `dashos::hid::Entrada`, que ahora comparten `usb` (el DWC2 de la Pi) y `xhci`.
- El árbol hecho desde ACPI (U5) lleva también el PCIe genérico, de la MCFG: sin él, con ACPI no aparecía el xHCI.
- `tools/prueba_uefi.sh` manda teclas y ratón con `TECLAS=1`; `make -C kernel arranque-virt` lo usa.
- **Comprobado:** en `virt` con árbol y con ACPI, con GICv2 y GICv3: el teclado y el ratón se enumeran y el ratón
  mueve la ventana del compositor. En la Pi, `prueba` y `arranque`, bien. **No comprobado:** el VL805 de la Pi 4
  (además necesita su firmware por el buzón, `docs/spec/hw/pcie.md` §8), hubs, aparatos de USB 3 y máquinas reales.

### 2026-11-07 — U6d (NVMe)

- `usuario/src/bin/nvme.rs`, con NVM Express Base 2.0: apagar, colas de administración (16 entradas), encender,
  Identify del controlador y del espacio de nombres 1, una cola de E/S y una lectura de los dos primeros sectores.
  Mira las terminadas por su bit de fase, girando (solo al arrancar). Memoria: un objeto DMA de 64 KB.
- El kernel busca en el PCIe por clase (`aparato_pci`): el xHCI (0x0C0330) y el NVMe (0x010802), cada uno con sus
  registros y su permiso DMA. `inicio` lanza `nvme` si lo hay.
- `make -C kernel arranque-virt` crea `target/nvme.img` (1 MB, con un texto y la firma 0x55AA en el sector 0) y se
  lo pone a QEMU como disco NVMe.
- **Comprobado:** en `virt` con árbol y con ACPI (GICv2 y GICv3): modelo "QEMU NVMe Ctrl", 2048 sectores de 512, y
  los dos sectores con su texto y su firma. En la Pi, `prueba` y `arranque`, bien (allí no hay NVMe y no se lanza).
  **No comprobado:** discos reales, sectores de 4 KB, más de un espacio de nombres.
- Siguiente: que `nvme` sirva bloques por un canal (leer y escribir), para un sistema de ficheros; y las
  interrupciones del PCIe (INTx por el `interrupt-map` del árbol, o MSI por el GICv2m/ITS).

### 2026-11-07 — U6e (la interrupción del xHCI)

- El kernel saca la INTx de cada aparato del PCIe: su patilla (byte 0x3D de la configuración) y el `interrupt-map`
  del nodo del PCIe (con `interrupt-map-mask` y las celdas del padre por su phandle). Permite la INTx en el registro
  de mando (bit 10 a 0). En QEMU `virt`, el xHCI (0:1) da la 36 y el NVMe (0:2) la 37.
- Aparatos opcionales en `PROGRAMAS` (`OPCIONAL`): un servicio se lanza aunque falten; sus llaves siguientes corren
  un puesto. El xHCI lleva su interrupción así: con ACPI no hay `interrupt-map` (va en el AML) y sigue con el
  temporizador de 10 ms.
- `xhci.rs`: si tiene llave 4, la 3 es su interrupción. Enciende IMAN.IE y USBCMD.INTE; en cada vuelta borra
  USBSTS.EINT e IMAN.IP, mira los eventos y vuelve a permitir la interrupción.
- **Comprobado:** con árbol, GICv2 y GICv3: "avisos por su interrupción" y el ratón mueve la ventana sin temporizador.
  Con ACPI (GICv2 y GICv3): "cada 10 ms" y lo mismo. Pi: `prueba` y `arranque`; `prueba-virt`. **No comprobado:** dos
  aparatos con la misma INTx (el segundo se quedaría sin llave: `Interrupcion::nueva` no comparte).

### 2026-11-07 — U6f (el servicio de disco)

- `nvme` sirve el disco por el canal "disco" (protocolo en `docs/referencia/servicios.md`): órdenes de 24 bytes por
  el canal, con una llave de respuesta; los datos, en una ventana de 64 KB de memoria DMA por cliente, compartida, en
  la que el NVMe lee y escribe directamente (PRP1 y PRP2). Clientes por la marca que les pone `inicio`; sin marca, se
  rechaza. Comprueba sector, tamaño y desplazamiento antes de mandar nada al disco.
- `usuario/src/bin/disco.rs`, cliente de prueba (solo en `virt`): abre, lee los dos primeros sectores, escribe el 2 y
  lo relee, lee 4 KB que cruzan página (PRP2) y comprueba tres rechazos.
- `dashos::dormir_us` en la biblioteca (el buzón lo usa en vez de su función propia).
- Fallo encontrado: para mandar una llave en un mensaje hace falta su derecho de transferir; la ventana y la copia
  de la llave de respuesta lo llevan. Si la respuesta con la ventana no se puede mandar, va sin ella y con -5.
- El disco de prueba se crea de nuevo en cada `make arranque-virt`: así, lo escrito en una vuelta no hace pasar la
  siguiente.
- **Comprobado:** `virt` con árbol (GICv2 y GICv3) y ACPI (GICv2 y GICv3): las cuatro comprobaciones bien, y el
  sector 2 escrito en `target/nvme.img`. Pi: `prueba` y `arranque`; `prueba-virt`. **No comprobado:** varios
  clientes a la vez; un cliente que muere con la ventana (la ventana sigue del servicio, que la vuelve a dar a la
  misma marca).

### 2026-11-07 — U6g (MSI-X por el GICv2m)

- El kernel da a cada aparato del PCIe una MSI-X si hay marco GICv2m ("arm,gic-v2m-frame"; su MSI_TYPER dice qué
  SPI puede usar): la primera entrada de la tabla de MSI-X (en la BAR 0) apunta a MSI_SETSPI_NS con el número de una
  SPI, que se pone por flanco (`gic::por_flanco`, GICD_ICFGR). Si no hay marco o MSI-X, la INTx de U6e.
- El árbol hecho desde ACPI lleva el marco GICv2m de la MADT (entrada 0x0D).
- En QEMU `virt` con GICv2: xHCI en la 80 y NVMe en la 81, con árbol y con ACPI. Con GICv3 no hay GICv2m (su MSI es
  el ITS): con árbol, INTx; con ACPI, el temporizador.
- **Comprobado:** `arranque-virt` (árbol, GICv2, MSI-X), árbol con GICv3 (INTx), ACPI con GICv2 (MSI-X) y con GICv3
  (cada 10 ms): todos con el ratón moviendo la ventana. Pi: `prueba` y `arranque`; `prueba-virt`, también con ACPI y
  GICv3 (el pánico del final es la prueba C34, a propósito). **No comprobado:** tablas de MSI-X en otra BAR,
  máquinas reales.
- Siguiente en esta línea: el ITS del GICv3 (LPI), que es lo de los portátiles con Snapdragon.

### 2026-11-07 — U6h (el ITS del GICv3)

- `kernel/src/its.rs`, con la especificación del GICv3 (IHI 0069): tablas de LPI en el redistribuidor del núcleo 0
  (configuración de 8 KB y pendientes, `gic::encender_lpis`), el ITS con tablas planas de aparatos (4 páginas) y de
  colecciones, una cola de órdenes de una página, la colección 0 al núcleo 0 (MAPC, según GITS_TYPER.PTA), y por
  aparato su ITT y su suceso 0 a una LPI nueva (MAPD, MAPTI, SYNC). El DeviceID es el RID del PCIe (identidad, como
  el `msi-map` de QEMU; el IORT de ACPI no se lee).
- Con ITS, la MSI-X del aparato apunta a GITS_TRANSLATER con el suceso 0; si no, al GICv2m (U6g).
- El kernel da LPI a los procesos (las 64 primeras): se encienden en la tabla de configuración al asociarlas
  (INVALL y SYNC) y se apagan al soltarlas; mientras tanto se enmascaran en software. Fallo encontrado: encendidas
  desde el principio, la del NVMe (que su servicio no pide) llegaba sin dueño.
- `gic::numero`: con GICv3 el número de IAR es de 24 bits (antes se cortaba a 10, y la LPI 8192 parecía la 0).
- `base/memoria.rs`: `paginas_fijas` y `escribir_fisica` (memoria para tablas del GIC, fuera de la caché).
- El árbol hecho desde ACPI lleva el ITS (MADT, entrada 0x0F).
- **Comprobado:** las cuatro máquinas (árbol o ACPI, GICv2 o GICv3) con MSI-X: el xHCI en la SPI 80 o en la LPI
  8192, y el ratón mueve la ventana. Pi: `prueba` y `arranque`; `prueba-virt`. **No comprobado:** máquinas reales
  (cachés del GIC no coherentes: se limpia la caché al escribir las tablas, pero sin probar), varios núcleos
  recibiendo LPI.
- Ojo para las pruebas a mano: en zsh, `env $c` no parte `c` en palabras (hace falta `${=c}`); así salieron dos
  falsos fallos en esta tanda.
