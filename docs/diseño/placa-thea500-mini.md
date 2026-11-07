# El THEA500 Mini como placa de DashOS

Propuesta del usuario (2026-11-07): el THEA500 Mini de Retro Games como placa candidata. Ya está a la venta, y es
la misma familia que el THEA1200 (placa candidata desde el 2026-11-05), así que sirve para adelantar ese trabajo.

## Qué lleva

Datos de la comunidad, no del fabricante (sin comprobar en una máquina):

| Pieza | Dato | Fuente |
|---|---|---|
| Chip | Allwinner H6 (SUN50IW6P1): cuatro Cortex-A53, GIC-400 (GICv2) | [hilo 1220][h1220] |
| Memoria | 512 MB DDR3 | [hilo 1220][h1220], [hilo 971][h971] |
| Almacenamiento | 256 MB de NAND en bruto (Kioxia TC58NVG1S3HT); sin eMMC ni tarjeta | [hilo 1220][h1220] |
| USB | tres puertos; el de al lado del HDMI es OTG (FEL) | [hilo 971][h971], [PCUAE][fel] |
| Vídeo | HDMI, 720p | [hilo 971][h971] |
| GPU | Mali-T720 (sin confirmar) | [hilo 971][h971] |
| Energía | sin PMIC de verdad ("dummy"); USB-C 5 V | [hilo 971][h971] |
| Sistema | U-Boot de Allwinner y Linux 3.10.65 de 64 bits con espacio de usuario de 32 | [hilo 1220][h1220] |

## Cómo se arrancaría DashOS sin tocar su NAND

- **FEL**: el botón pequeño de la placa (o la tecla "2" por la consola serie al encender) deja el H6 esperando
  por USB. Con `sunxi-fel` se cargan en memoria U-Boot, TF-A, el núcleo y el árbol, y se arranca. PCUAE ya lo hace
  así ([PCUAE][fel]). Nada se escribe en la NAND: si algo va mal, se apaga y vuelve su sistema.
- **Consola serie**: pads soldados detrás del USB de la derecha, a 115200 baudios ([hilo 1220][h1220]). Hace falta un
  adaptador USB-serie de 3,3 V.
- **UEFI**: U-Boot y TF-A de mainline soportan el H6 (Orange Pi 3, Pine H64). U-Boot da UEFI (`bootefi`), así que
  el cargador UEFI de DashOS (fase U) vale tal cual, con el árbol del H6. Falta el ajuste de su DDR3 para el SPL de
  U-Boot: se saca del U-Boot de la máquina o del paquete de PCUAE.
- No se sabe si el arranque está firmado; PCUAE carga su propio U-Boot por FEL sin quejas, que es buena señal.

## Qué le falta a DashOS

| Pieza | Estado |
|---|---|
| CPU, MMU, temporizador, GICv2 | Hecho (Pi 4 y QEMU `virt`) |
| Big-endian en EL0 (caja Amiga) | El A53 lo admite: la caja va sin dar la vuelta a los bytes (U7 no hace falta) |
| Consola serie | Nueva: la UART del H6 es una 16550 (DesignWare), no la PL011 de la Pi. Poca cosa |
| USB | El H6 tiene USB 3 (DWC3, xHCI) y USB 2 (EHCI/OHCI). Si los puertos van por el xHCI, el controlador de U6 sirve; si van por EHCI, controlador nuevo |
| Pantalla | Con suerte, el marco que deja U-Boot (GOP o `simple-framebuffer`). Si no, DE3 + HDMI de DesignWare: controlador nuevo, grande |
| Disco | Solo una memoria USB (la NAND en bruto, mejor no tocarla) |
| 512 MB | Llegan: la meta es menos de 128 MB en reposo |

## Riesgos

- Escribir en la NAND puede dejarla sin arrancar; el plan no la toca.
- QEMU no tiene una máquina H6: todo se prueba en la máquina de verdad.
- La GPU (Mali-T720, Midgard) no tiene Vulkan en Mesa (panvk empieza en Bifrost).

## Primer paso propuesto

Con una máquina abierta y un adaptador serie: arrancar por FEL el U-Boot de mainline con la DDR3 de la máquina,
ver su consola, y lanzar el cargador UEFI de DashOS con una UART 16550. Si llega a "DashOS en marcha", lo demás
son controladores.

[h1220]: https://thec64community.online/thread/1220/thea500-mini-hacking-development
[h971]: https://thec64community.online/thread/971/thea500-mini-specs
[fel]: https://projectcarouselusb.eu/thea500-mini-fell-rescue-boot/
