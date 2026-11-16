# S24. DashOS en ordenadores ARM: CM5, RTX Spark, portátiles y Mac

Del 2026-11-12, a pedido del usuario ("me interesa sobre todo a ordenadores ARM, RTX Spark, CM5 y en general portátiles
ARM", "los MAC también"). Parte de lo que ya decidió el 2026-11-04 (`PLAN.md`, "Otros ordenadores ARM" y "Caja Amiga
sin big-endian"): DashOS arranca directamente en la máquina, sin otro sistema debajo; primero por UEFI con la pantalla
del firmware y los controladores estándar, y después lo propio de cada máquina. Se desarrolla en QEMU y se mide en una
o dos máquinas de verdad al final.

## Lo que ya hay

- **Cargador UEFI** propio (`uefi/`), probado en QEMU `virt` con EDK2. Usa la pantalla del firmware (GOP), y desde hoy
  también puede pedirle otro modo o programar la ramfb de QEMU (`DASHOS_PANTALLA`, hasta 1920 × 1080 probado).
- **ACPI a árbol de dispositivos** (`uefi/src/acpi.rs`, U5): para las máquinas que solo dan ACPI (los portátiles con
  Windows). Lee MADT (GIC), GTDT (temporizador), SPCR (serie), FADT (PSCI), MCFG (PCIe ECAM) e IORT (MSI).
- **Controladores estándar**: xHCI (teclado y ratón USB), NVMe, virtio (red, sonido, GPU).
- **Varias pantallas** por virtio-gpu (en QEMU) y la del firmware.
- **Lo nativo es little-endian.** Solo la caja Amiga es big-endian (decisión del 2026-11-03).
- **Desde hoy, el kernel dice al arrancar qué CPU es y si admite big-endian en EL0** y páginas de 4 KB
  (`ID_AA64MMFR0_EL1`). En QEMU `virt`: "Cortex-A72; big-endian en EL0: sí (y en EL1); páginas de 4 KB: sí". Es lo
  primero que hay que leer en cada máquina nueva. Hoy sale por la consola de arranque. Tiene que quedar también en el
  registro de arranque (`Estado:Registros/`, S6 y S14) y en la orden que describe la máquina (ver "Lo que no funciona
  en cada máquina").
- `DASHOS_PANTALLA` se da al compilar el cargador: no es un ajuste del usuario. El modo de pantalla del usuario se
  elige en Ajustes y con su orden (S9).

## Las máquinas

| | CM5 / Pi 5 | RTX Spark (N1X) | Portátiles Snapdragon X | Mac (Apple Silicon) |
|---|---|---|---|---|
| CPU | Cortex-A76 (BCM2712) | 10 Cortex-X925 + 10 Cortex-A725 (ARMv9.2) | Oryon (Qualcomm) | Apple (M1 en adelante) |
| Big-endian en EL0 | Sin comprobar (`PLAN`) | Sin comprobar (los manuales públicos no lo dicen) | **No**, según Qualcomm | **No** (`PLAN`, 2026-11-04) |
| Firmware | El de la Pi (`kernel_2712.img`) o EDK2 de la comunidad (rpi5-uefi: UEFI, GOP, USB y PCIe del RP1; ACPI a medias) | UEFI + ACPI (Windows) | UEFI + ACPI (Windows) | m1n1 + U-Boot (Asahi): UEFI y árbol de dispositivos |
| Interrupciones | GIC-400 (como la Pi 4; sin comprobar) | GIC (Arm) | GIC | **AIC de Apple** (no es GIC): controlador nuevo en `kernel/src/base/` |
| Pantalla al empezar | GOP (rpi5-uefi) o la del firmware de la Pi | GOP (la GPU de NVIDIA, sin controlador abierto) | GOP | El framebuffer que deja m1n1 |
| Teclado y ratón | USB por el RP1 (xHCI) | USB; el del portátil, sin saber | I2C-HID (Qualcomm QUP) y USB (DWC3, que en modo anfitrión es xHCI) | SPI/MTP propio de Apple; USB-C por su PHY |
| Disco | NVMe por PCIe, tarjeta SD | NVMe | NVMe (por el PCIe de Qualcomm: ¿ECAM? sin comprobar) | NVMe de Apple (ANS): no es NVMe estándar |
| Páginas | 4 KB | 4 KB | 4 KB | La CPU, 16 KB (4 KB sin comprobar); el IOMMU (DART), solo 16 KB |
| Lo caro | RP1 (Ethernet, GPIO), la pantalla HVS de la BCM2712 | La GPU (cerrada), lo del portátil | Todo lo del portátil sin documentar; Wi-Fi | Casi todo: AIC, DART, ANS, teclado, USB-C, pantalla |

Fuentes, en la conversación del 2026-11-12: Wikipedia y prensa sobre RTX Spark (N1X, anunciado el 31 de mayo de 2026,
con MediaTek; la misma familia que el GB10 de DGX Spark), la documentación de Qualcomm (Oryon sin big-endian, sin
AArch32 ni SVE), el README de `worproject/rpi5-uefi` y las notas de Asahi sobre las páginas de 16 KB del IOMMU.

## Qué significa para la caja Amiga

En Snapdragon X y en los Mac no hay big-endian en EL0: la caja Amiga necesita el modo del JIT que da la vuelta a los
bytes (U7, `docs/diseño/U7-caja-sin-big-endian.md`: unas 1100-1300 líneas de Emu68 en cuatro parches). En la CM5 y la
RTX Spark, depende de lo que diga su CPU al arrancar. El resto de DashOS (todo lo nativo) va igual en todas.

## Orden recomendado

1. **En QEMU, ya**: el camino de ACPI de punta a punta (EDK2 de `virt` también da ACPI; arrancar fiándose solo de ACPI,
   sin el árbol del firmware), con la pantalla del firmware a la resolución que dé. Y la línea de la CPU al arrancar
   (hecho).
2. **La CM5 (o una Pi 5) como primera máquina de verdad.** Es la más cercana a la Pi 4 que ya está pensada, barata, y
   con rpi5-uefi da UEFI, GOP y USB sin escribir nada: el cargador de DashOS debería llegar al escritorio con la
   pantalla del firmware y un teclado USB. Además dice si el Cortex-A76 tiene big-endian.
3. **U7, el JIT sin big-endian**, en cuanto la caja Amiga funcione en una CPU con big-endian (la rama `caja-amiga` está
   en ello). Hace falta para Snapdragon y Mac, y se prueba en QEMU.
4. **Un portátil**: Snapdragon X (hay más hecho por Linaro y Qualcomm en abierto) o uno con RTX Spark (más caro; la
   GPU sin controlador abierto se queda en la pantalla del firmware). Con lo común (UEFI, ACPI, GOP, NVMe, USB externo)
   debería arrancar; el teclado y el panel táctil del portátil, el Wi-Fi y la batería son controladores propios.
5. **Mac, al final.** Es el más distinto: otro controlador de interrupciones (AIC), IOMMU de 16 KB, disco que no es
   NVMe estándar, teclado propio. Arranca con m1n1 y U-Boot de Asahi.

## Lo que no funciona en cada máquina, dicho

En un portátil o en la RTX Spark habrá cosas que aún no van (el Wi-Fi, la batería, el teclado propio, la GPU). El
usuario no tiene que adivinarlo. Tiene que existir una lista llana en Ajustes ("En este ordenador aún no funciona:
Wi-Fi, batería") y la misma lista en una orden de Shell, que además describe la máquina (CPU, big-endian en EL0,
páginas). El nombre de la orden, por decidir al hacerla.

## Instalar sin pasos de informático

La meta es que el usuario solo grabe la imagen y arranque. Para la CM5, la imagen de DashOS llevaría dentro el firmware
UEFI (rpi5-uefi), si su licencia lo deja; no se ha comprobado. En el Mac, el instalador haría el paso de m1n1; depende
de la pregunta 3 de abajo.

## Lo que debe decidir el usuario

1. ¿La CM5 (o la Pi 5) como primera máquina ARM de verdad, antes que un portátil? Recomendado: sí.
2. ¿Qué portátil después: Snapdragon X o RTX Spark? Recomendado: Snapdragon X, por lo que hay documentado en abierto.
3. Sala limpia con Apple: ¿se puede leer la documentación de Asahi Linux (su wiki describe el hardware) aunque no su
   código? ¿Y usar m1n1 (licencia MIT) como primer cargador? Recomendado: la documentación sí, y m1n1 como programa
   aparte, sin copiar su código en DashOS.

## Lo que no se comprobó

Si el Cortex-A76, el Cortex-X925 y el Cortex-A725 admiten big-endian en EL0 (la línea nueva del arranque lo dirá en
cada máquina). Si los Mac admiten páginas de 4 KB en la CPU. El controlador de interrupciones exacto de la BCM2712. Si
el PCIe de los Snapdragon X se ve como ECAM por la MCFG.
