# S12. Controladores de la Pi 4

## El problema

Hay que decir qué controladores necesita DashOS en la Pi 4, en qué orden, de dónde sale la información para
escribirlos y con qué licencia.

Lo que ya está decidido o recomendado:

- Cada controlador es un proceso (S1). Si falla, se reinicia.
- Los periféricos son little-endian y la CPU big-endian: cada acceso a un registro da la vuelta a los bytes, con un
  tipo propio en la base del kernel (S1).
- La memoria para DMA se pide por zonas: la SD y el DMA clásico solo alcanzan el primer GB, y el USB los primeros
  3 GB (S3).
- La licencia de DashOS es MPL-2.0. No se puede copiar código GPL.

## Lo que se comprobó

**Qué emula QEMU `raspi4b`**, mirado con `info qtree` en QEMU 11.1:

- Sí: puerto serie PL011 y el auxiliar, GIC, buzón y propiedades de la VideoCore, pantalla del firmware, dos
  controladores SDHCI y el `sdhost` con tarjeta SD, el USB DWC2, GPIO, DMA, I2C, SPI, temporizador del sistema,
  relojes (CPRMAN) y gestión de energía.
- No: el PCIe con el USB VL805, la red GENET, la salida HDMI con el HVS, la GPU V3D y el Wi-Fi. Además hay 11
  dispositivos sin implementar.

**El USB DWC2 de QEMU acepta teclado, ratón y disco USB.** Con `-device usb-kbd -device usb-mouse -device
usb-storage`, `info usb` enseña los tres colgados de un concentrador. En la Pi 4 real, el DWC2 es el del conector
USB-C. Un controlador DWC2 da teclado y ratón en QEMU desde el principio, y sirve en la placa para el USB-C.

**El USB de los conectores normales de la Pi 4 (VL805) tiene una trampa.** En las placas nuevas, el VL805 no tiene
memoria propia y pierde su firmware en cada reinicio del PCIe. La VideoCore se lo vuelve a cargar si se le pide por el
buzón con la etiqueta `0x00030058` ("notify xHCI reset"), justo después de configurar el PCIe y antes de arrancar el
xHCI. Así lo hacen Linux y FreeBSD.

## De dónde sale la información

| Fuente | Licencia | Uso |
|---|---|---|
| Hoja de datos "BCM2711 ARM Peripherals" de Raspberry Pi | Documento público | Base para lo que cubre |
| Normas: SDHCI, xHCI, USB, PCIe, virtio, HID | Públicas | Controladores genéricos |
| FreeBSD y NetBSD | BSD, ISC | Referencia y, si encaja, código adaptado |
| Emu68 | MPL-2.0 | Buzón, pantalla del firmware, arranque de núcleos |
| Tutoriales de SO en Rust de Andre Richter | MIT o Apache-2.0 | Serie, temporizador, MMU, excepciones |
| Linux | GPL-2.0 | Solo para entender lo que no está documentado; no se copia |

Para lo que no está documentado (HVS, HDMI, V3D, Wi-Fi), la única fuente fiable es el código de Linux. Se propone el
mismo método que con la ROM del Amiga (AGENTS.md, regla 1): una sesión de análisis lo lee y escribe una
especificación en prosa, y otra sesión escribe el controlador sin ver Linux.

## La lista, en orden

| Fase | Controlador | En QEMU | Notas |
|---|---|---|---|
| Kernel | Serie PL011 | Sí | Primera salida de texto |
| Kernel | Temporizador genérico de ARM | Sí | Planificador (S2) |
| Kernel | GIC-400 | Sí | Interrupciones; la Pi 3 usa otro controlador (S0) |
| Kernel | Buzón de la VideoCore | Sí | Pantalla, relojes, temperatura, VL805 |
| Kernel | Vigilante y reinicio | Sí | Gestión de energía |
| Kernel | Números aleatorios (RNG200) | Sin comprobar | Claves y direcciones aleatorias (S3, S6) |
| Base | Tarjeta SD (EMMC2, SDHCI) | Sí | Para arrancar del disco; DMA en el primer GB |
| Base | Pantalla del firmware | Sí | Gráficos, fase A (S9) |
| Base | USB DWC2 | Sí | Teclado, ratón y disco en QEMU; USB-C en la placa |
| Base | GPIO | Sí | |
| Placa | PCIe y USB VL805 (xHCI) | No | Los cuatro USB de la placa; firmware por el buzón |
| Placa | Red GENET y su PHY | No | Ethernet |
| Placa | HVS y HDMI | No | Gráficos, fase B (S9); dos salidas |
| Placa | Sonido por HDMI y analógico | No | |
| Después | Wi-Fi CYW43455 (SDIO) | No | Firmware cerrado de Infineon, que se puede redistribuir; protocolo complejo |
| Después | Bluetooth (por la UART) | No | |
| Después | GPU V3D | No | Gráficos, fase C (S9) |
| Caja Linux | virtio: disco, red, consola, vsock, fs, gpu | — | Lado de DashOS (S7) |

Las fases "Kernel" y "Base" se pueden hacer enteras en QEMU. **A partir de la fase "Placa" hace falta una Pi 4
real.** Es la respuesta práctica a la pregunta de S0 sobre comprar la placa: no corre prisa hasta terminar la fase
Base.

Para tener red en QEMU antes de la placa, se puede probar un adaptador de red USB emulado (`usb-net`) en el DWC2,
con un controlador CDC Ethernet. No se ha comprobado.

### Cómo es un controlador

- Un proceso con las capacidades justas: la memoria de sus registros, su interrupción y memoria para DMA en su zona.
  Nada más (S6).
- Lo escribe en Rust, con el tipo de registro que da la vuelta a los bytes.
- Habla con sus clientes por canales y anillos en memoria compartida (S1).
- Si se cuelga, el gestor de servicios lo reinicia, y sus clientes reintentan.

## Lo que queda abierto

1. ¿Método para lo que no está documentado: una sesión lee Linux y escribe una especificación, y otra escribe el
   controlador sin ver Linux? Recomendado: sí. Es el mismo método de la sala limpia.
2. ¿Aceptar el firmware cerrado del Wi-Fi de Infineon, que se puede redistribuir, como hacen todas las
   distribuciones de la Pi? Recomendado: sí. Sin él no hay Wi-Fi.
3. ¿Comprar la Pi 4 al terminar la fase Base en QEMU, y no antes? Recomendado: sí. Sustituye a la pregunta de S0.

## Cómo encaja

- Con S0: la Pi 3 tendría que añadir su controlador de interrupciones; la compra de la Pi 4 se concreta aquí.
- Con S1: cada controlador es un proceso; registros con vuelta de bytes.
- Con S2: el buzón da la frecuencia y la temperatura.
- Con S3: memoria DMA por zonas.
- Con S5: la SD necesita el borrado (TRIM) para cuidar la tarjeta.
- Con S6: capacidades justas para cada controlador; números aleatorios para las claves.
- Con S7: virtio para la caja Linux.
- Con S9: pantalla del firmware, HVS y HDMI, y V3D, por fases.
- S13: la red GENET (y la USB en QEMU) y el Wi-Fi.
- S14: el orden de arranque de los controladores.

## Fuentes

- QEMU `raspi4b`: `info qtree` e `info usb` en QEMU 11.1.3, en shiva, el 2026-11-03.
- Firmware del VL805: [parches de Linux de 2020](https://lkml.iu.edu/2002.2/03529.html),
  [parche de Linaro](https://patches.linaro.org/patch/238770),
  [parche de Ubuntu](https://lists.ubuntu.com/archives/kernel-team/2020-July/111981.html),
  [revisión de FreeBSD](https://reviews.freebsd.org/D25261) y
  [`bcm2838_xhci.c` de FreeBSD](https://src.rcs.uwaterloo.ca/xref/freebsd-src/sys/arm/broadcom/bcm2835/bcm2838_xhci.c)
  (licencia ISC).
- Tutoriales de Rust: [rust-raspberrypi-OS-tutorials](https://github.com/TaiseiIto/rust-raspberrypi-OS-tutorials)
  y [su README](https://git.blob42.xyz/Archives/rust-raspberrypi-OS-tutorials/blame/commit/70989b963c18331db7ee2c2a1e29e554e43bdc7f/README.md).
- Buzón de Emu68: `src/raspi/support_rpi.c`.

Lo que no se comprobó: el RNG200 en QEMU, la red USB en QEMU, y el estado de GENET en FreeBSD y NetBSD.
