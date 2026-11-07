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
- [ ] U3c **Las pruebas del kernel en `virt`** (`make -C kernel prueba` en las dos máquinas): muchas son de la Pi
  (SD, USB, buzón); separar las que valen para las dos.
- [x] U4 **La pantalla del firmware (GOP).** El programa de UEFI pasa al kernel el búfer de pantalla de GOP
  (dirección, tamaño, paso, formato); el compositor pinta ahí cuando no hay buzón de la VideoCore. En QEMU, `ramfb`
  o `virtio-gpu`.
- [ ] U5 **ACPI.** Para los portátiles: leer MADT (interrupciones y núcleos), GTDT (temporizador) y SPCR (puerto
  serie), o convertirlo a árbol en el programa de UEFI.
- [ ] U6 **Controladores estándar.** xHCI (`qemu-xhci`) y NVMe (`nvme`), como procesos, igual que el USB de la Pi.
- [ ] U7 **La caja Amiga sin big-endian.** El modo del JIT que da la vuelta a los bytes, para las CPU sin big-endian
  en EL0 (Apple). Se prueba en `virt`.

## Herramientas

- `make -C uefi/prueba` saca el núcleo de prueba de U2 (`nucleo.bin`, en C, enlazado en 0x40200000).
- `cd uefi && cargo build --release` (con `DASHOS_NUCLEO=<imagen>` para meter otra) saca `uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi`.
- `make -C kernel arranque-virt`: el kernel con la característica `virt`, metido en el programa de UEFI y arrancado.
- `tools/prueba_uefi.sh <programa.efi> <esperados> [segundos]`: QEMU `virt` (GICv2, o `GIC=3`; `MAQUINA=...` añade
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
