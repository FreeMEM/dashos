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
- [ ] U3 **El kernel en `virt`.** GICv3 (redistribuidores por núcleo, registros del sistema `ICC_*`), el PL011 y la
  memoria sacados del árbol, nada de direcciones fijas de la Pi (ventana de periféricos en 0xFC000000, zonas DMA),
  PSCI para arrancar los demás núcleos. Prueba: `make -C kernel prueba` en las dos máquinas.
- [ ] U4 **La pantalla del firmware (GOP).** El programa de UEFI pasa al kernel el búfer de pantalla de GOP
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
- `tools/prueba_uefi.sh <programa.efi> <esperados> [segundos]`: QEMU `virt` (GICv3, 4 núcleos, 2 GB, `acpi=off`) con
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
