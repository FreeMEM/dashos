# Controlador PCIe de la BCM2711 (Raspberry Pi 4): ventana de entrada

Especificación de sala limpia. La escribe el analista leyendo el controlador `pcie-brcmstb` de Linux. Quien implemente
no necesita ni debe leer ese código: todo lo necesario tiene que estar aquí.

El objetivo es uno: **cerrar la ventana de entrada** del PCIe. Es decir, que el VL805 (el xHCI de USB 3 de la placa)
solo pueda hacer DMA a una zona baja de la memoria (el primer GB, por ejemplo) y no a toda la RAM, donde vive el
kernel. La Pi 4 no tiene IOMMU: esta ventana es el único freno de hardware que hay. Contexto en
`docs/diseño/S16-kernel-seguro.md` (apartado D) y `docs/diseño/S3-memoria.md`.

Este documento no cubre el xHCI ni la enumeración PCI en general. Cubre lo justo del arranque del controlador para
poder reprogramar la ventana sin romper el USB.

## 0. Origen y convenciones

- **Fuente principal**: `torvalds/linux`, commit `d48723711bae9b8cdc56719c448ba42fef28cbee` (6 de noviembre de
  2026). Ficheros: `drivers/pci/controller/pcie-brcmstb.c`, `drivers/reset/reset-raspberrypi.c`,
  `drivers/usb/host/xhci-pci.c`, `drivers/usb/host/xhci-mem.c`, `drivers/usb/host/pci-quirks.c`,
  `include/soc/bcm2835/raspberrypi-firmware.h`, `drivers/pci/pci.h`,
  `arch/arm/boot/dts/broadcom/bcm2711.dtsi`, `arch/arm/boot/dts/broadcom/bcm2711-rpi-4-b.dts` y
  `Documentation/devicetree/bindings/pci/brcm,stb-pcie.yaml`.
- **Mensajes de commit** de Linux que explican el porqué. Se citan por su identificador corto (por ejemplo
  `52ded9e4f07b`).
- **Fuentes de contraste**: el kernel de Raspberry Pi (`raspberrypi/linux`, rama `rpi-6.18.y`, commit `31a3488538`)
  y U-Boot (`u-boot/u-boot`, commit `ece349ade2`, `drivers/pci/pcie_brcmstb.c`). Solo se usan para confirmar o para
  señalar diferencias. Se dice siempre cuándo un dato sale de ahí.
- **Árbol de dispositivos**: `build/firmware/bcm2711-rpi-4-b.dtb` del repositorio, decompilado con `dtc`. Es el del
  firmware de Raspberry Pi. Donde difiere del de Linux, se dice.
- **Citas**: cada dato lleva entre corchetes el fichero y la función de Linux. Por ejemplo,
  [`pcie-brcmstb.c`, `brcm_pcie_setup`].
- **Comprobado y deducido**: "Linux hace X" significa que se leyó en el código. "Se deduce" es una conclusión del
  analista, no escrita en Linux. Nada de este documento se ha probado en una Pi 4 real.
- **Notación**: los desplazamientos son en bytes desde la base del controlador (sección 2). Un campo `[h:l]` ocupa
  los bits de `h` a `l` de una palabra de 32 bits, ambos incluidos. Los registros son de 32 bits.
- **Nombres**: los de Linux sin el prefijo `PCIE_`. `MISC_RC_BAR2_CONFIG_LO` es `PCIE_MISC_RC_BAR2_CONFIG_LO`.
- **Direcciones**: "física" es la dirección física de la CPU. "De bus" es la dirección que ve el VL805 en el PCIe.
  "SCB" es el bus interno de memoria del chip (Linux lo llama así; en la Pi 4 coincide con la física para la RAM,
  sección 2.2).

## 1. Qué es

- El bloque PCIe de la BCM2711 es el controlador de Broadcom de sus decodificadores (familia "STB"). En la Pi 4 hace
  de **complejo raíz** (RC) con un solo puerto y una sola línea (x1).
- Del puerto cuelga un solo aparato: el **VL805** de VIA (fabricante `0x1106`, aparato `0x3483`), un xHCI de USB 3.
  [`xhci-pci.c`, `PCI_DEVICE_ID_VIA_VL805`; `pci-quirks.c`, `quirk_usb_early_handoff`]
- El tráfico tiene dos sentidos, cada uno con sus ventanas:
  - **Salida** (CPU → PCIe): la CPU lee y escribe los registros del VL805. Lo traducen las ventanas
    `MISC_CPU_2_PCIE_MEM_WIN*`.
  - **Entrada** (PCIe → memoria): el VL805 hace DMA. Lo traducen las ventanas `MISC_RC_BAR1..3`. **Esto es lo que
    hay que cerrar.**
- Los registros llamados "BAR" del controlador **no son BAR del PCI**. El nombre viene de cuando el bloque era un
  aparato final. En modo RC, cada "BAR" es una ventana de entrada. [`pcie-brcmstb.c`, comentario antes de
  `PCIE_BRCM_MAX_INBOUND_WINS`]

## 2. Dónde está

### 2.1. Nodo del árbol

| Dato | Valor | Fuente |
|---|---|---|
| Nodo | `/scb/pcie@7d500000` | DTB |
| Compatible | `brcm,bcm2711-pcie` | DTB; [`pcie-brcmstb.c`, `brcm_pcie_match`] |
| `reg` | `0x0_7D500000`, tamaño `0x9310` | DTB |
| Dirección física (modo de periféricos bajo) | `0xFD500000` a `0xFD50930F` | se deduce del `ranges` de `scb` (2.2) |
| Interrupciones | SPI 147 "pcie", SPI 148 "msi" (nivel alto) | DTB, `interrupts` e `interrupt-names` |
| INTA a INTD del VL805 | SPI 143, 144, 145, 146 | DTB, `interrupt-map` |
| `ranges` (salida) | PCI `0xC0000000` → física `0x6_00000000`, 1 GB, memoria de 32 bits no prefetch | DTB del firmware |
| `dma-ranges` (entrada) | PCI `0x0` → SCB `0x0`, 3 GB (`0xC0000000`) | DTB |
| Otros | `msi-controller`, `msi-parent` = él mismo, `brcm,enable-ssc` | DTB |
| Hijo | `pci@0,0` (puerto raíz) → `usb@0,0` con `resets = <&reset 0>` | DTB; `bcm2711-rpi-4-b.dts` |

- Los números de GIC son SPI + 32: 179 y 180 para el controlador, 175 a 178 para INTA..INTD. (Convención del GIC,
  no de Linux.)
- El `reset` del nodo `usb@0,0` apunta al nodo `raspberrypi,firmware-reset`, línea 0
  (`RASPBERRYPI_FIRMWARE_RESET_ID_USB`). Es la carga del firmware del VL805 (sección 8).
- El árbol de Linux (`bcm2711.dtsi`) da otra ventana de salida: PCI `0xF8000000` → física `0x6_00000000`, solo 64 MB.
  El firmware de Raspberry Pi da 1 GB en PCI `0xC0000000` (igual que `bcm2711-rpi-ds.dtsi` del kernel de Raspberry
  Pi). Para la ventana de entrada no importa, salvo por el solape de la sección 4.6.
- El nodo no tiene `clocks`, `resets` propios, `brcm,scb-sizes`, `max-link-speed` ni `num-lanes`. Así que en la Pi 4
  Linux no usa reloj, ni reinicios externos del puente, ni límite de generación. [DTB; `pcie-brcmstb.c`,
  `brcm_pcie_probe`]
- Configuración de Linux para este compatible: 3 ventanas de entrada, sin fase física propia (`HAS_PHY` no), sin
  informe de errores (`HAS_ERR_REPORT` no), y con el arreglo `EARLY_PERST_ASSERT` (sección 9.7).
  [`pcie-brcmstb.c`, `bcm2711_cfg`]

### 2.2. Del bus `scb` a la dirección física

- El nodo padre `scb` tiene `ranges` con la entrada `0x0_7C000000` → `0x0_FC000000`, tamaño `0x3800000`. Por eso
  `0x7D500000` del bus es `0xFD500000` físico. [DTB]
- Con el modo de periféricos alto (`arm_peri_high=1`) la dirección sería otra. No se ha comprobado (punto 12 de "Lo
  que no está claro").
- `dma-ranges` de `scb`: SCB `0x0` → física `0x0` para 16 GB, más una entrada de periféricos. Así que, para la RAM,
  **dirección SCB = dirección física**. [DTB]
- El kernel de Raspberry Pi y Linux coinciden en el `dma-ranges` del PCIe: 3 GB desde 0. Hay un comentario en
  `bcm2711.dtsi`: "el envoltorio del bloque PCIe tiene un fallo que le impide pasar de los primeros 3 GB de memoria".

## 3. Registros que importan

Desplazamientos desde `0xFD500000`. Solo los que hacen falta para la ventana de entrada y el arranque del enlace.

| Desp. | Nombre | Uso | Fuente |
|---|---|---|---|
| `0x0000`-`0x0FFF` | Espacio de configuración del puerto raíz | Bus 0, aparato 0, función 0, acceso directo | [`brcm_pcie_map_bus`] |
| `0x00AC` | `BRCM_PCIE_CAP_REGS` | Capacidad PCI Express del puerto raíz (estándar) | [cabecera de `pcie-brcmstb.c`] |
| `0x0188` | `RC_CFG_VENDOR_VENDOR_SPECIFIC_REG1` | `[3:2]` orden de bytes de la ventana BAR2 | [`brcm_pcie_setup`] |
| `0x043C` | `RC_CFG_PRIV1_ID_VAL3` | `[23:0]` clase del puerto raíz | [`brcm_pcie_setup`] |
| `0x04DC` | `RC_CFG_PRIV1_LINK_CAPABILITY` | anchura y velocidad que se anuncian | [`brcm_pcie_setup`, `brcm_pcie_set_gen`] |
| `0x1100`, `0x1104`, `0x1108` | `RC_DL_MDIO_ADDR`, `_WR_DATA`, `_RD_DATA` | MDIO de la fase física (SSC) | [`brcm_pcie_mdio_read`, `brcm_pcie_mdio_write`] |
| `0x4008` | `MISC_MISC_CTRL` | tamaños SCB, acceso a SCB, ráfaga (3.1) | [`brcm_pcie_setup`] |
| `0x400C` + 8·n, `0x4010` + 8·n | `MISC_CPU_2_PCIE_MEM_WIN{n}_LO/HI` | dirección PCI de la ventana de salida n (0..3) | [`brcm_pcie_set_outbound_win`] |
| `0x402C`, `0x4030` | `MISC_RC_BAR1_CONFIG_LO/HI` | ventana de entrada 1 (3.2) | [`brcm_bar_reg_offset`] |
| `0x4034`, `0x4038` | `MISC_RC_BAR2_CONFIG_LO/HI` | ventana de entrada 2, la principal | ídem |
| `0x403C`, `0x4040` | `MISC_RC_BAR3_CONFIG_LO/HI` | ventana de entrada 3 | ídem |
| `0x4044`, `0x4048` | `MISC_MSI_BAR_CONFIG_LO/HI` | dirección de destino de las MSI; bit 0 de LO = activar | [`brcm_msi_set_regs`] |
| `0x404C` | `MISC_MSI_DATA_CONFIG` | patrón de datos de las MSI | ídem |
| `0x4064` | `MISC_PCIE_CTRL` | bit 0 `PCIE_L23_REQUEST` | [`brcm_pcie_enter_l23`] |
| `0x4068` | `MISC_PCIE_STATUS` | bit 7 `PCIE_PORT` (1 = RC), bit 6 `LINK_IN_L23`, bit 5 `DL_ACTIVE`, bit 4 `PHYLINKUP` | [`brcm_pcie_rc_mode`, `brcm_pcie_link_up`] |
| `0x406C` | `MISC_REVISION` | revisión del bloque (`0x0303` o más: MSI modernas) | [`brcm_pcie_probe`, `brcm_pcie_enable_msi`] |
| `0x4070` + 4·n | `MISC_CPU_2_PCIE_MEM_WIN{n}_BASE_LIMIT` | `[31:20]` límite y `[15:4]` base, en MB | [`brcm_pcie_set_outbound_win`] |
| `0x4080` + 8·n, `0x4084` + 8·n | `..._WIN{n}_BASE_HI`, `..._LIMIT_HI` | `[7:0]` bits altos de base y límite (MB) | ídem |
| `0x40AC` + 8·(n-1) | `MISC_UBUS_BAR{n}_CONFIG_REMAP` | **no se usa en la 2711** (3.3) | [`set_inbound_win_registers`] |
| `0x4204` | `MISC_HARD_PCIE_HARD_DEBUG` | bit 27 `SERDES_IDDQ` (1 = SerDes apagado), bit 21 `L1SS_ENABLE`, bit 1 `CLKREQ_DEBUG_ENABLE` | [`brcm_pcie_setup`, `brcm_config_clkreq`] |
| `0x4300` | `INTR2_CPU_BASE` | MSI antiguas (revisión < `0x0303`) | [`pcie_offsets`, `brcm_pcie_enable_msi`] |
| `0x4500` | `MSI_INTR2_BASE` | MSI modernas: estado `+0x0`, borrar `+0x8`, enmascarar `+0x10`, desenmascarar `+0x14` | ídem |
| `0x8000` | `EXT_CFG_DATA` | ventana de datos de configuración de los aparatos de abajo | [`pcie_offsets`, `brcm_pcie_map_bus`] |
| `0x9000` | `EXT_CFG_INDEX` | índice: bus `<< 20`, aparato `<< 15`, función `<< 12` | ídem |
| `0x9208` | (temporizador del RBUS) | dos registros antes de `RGR1_SW_INIT_1`; unidades de 1/216 MHz | [`brcm_extend_rbus_timeout`] |
| `0x9210` | `RGR1_SW_INIT_1` | bit 0 `PERST` (1 = PERST# activo), bit 1 `INIT` (1 = puente en reinicio) | [`brcm_pcie_perst_set_generic`, `brcm_pcie_bridge_sw_init_set_generic`] |

### 3.1. `MISC_MISC_CTRL` (`0x4008`)

| Bits | Campo | Valor de Linux en la 2711 | Significado | Fuente |
|---|---|---|---|---|
| `[31:27]` | `SCB0_SIZE` | `log2(tamaño) − 15` (4 GB → 17) | tamaño de la vista del controlador de memoria 0 | [`brcm_pcie_setup`] |
| `[26:22]` | `SCB1_SIZE` | no se toca (un solo controlador) | ídem del 1 | ídem |
| `[21:20]` | `MAX_BURST_SIZE` | 0 = 128 bytes | ráfaga máxima hacia el SCB | ídem; commit `20b1d6ba606c` |
| `[13]` | `CFG_READ_UR_MODE` | 1 | (sin descripción en Linux) | ídem |
| `[12]` | `SCB_ACCESS_EN` | 1 | (sin descripción en Linux; se deduce que deja pasar el tráfico de entrada al SCB) | ídem |
| `[10]` | `PCIE_RCB_MPS_MODE` | 1 | lecturas hasta MPS en una sola respuesta | commit `602fb860945f` |
| `[7]` | `PCIE_RCB_64B_MODE` | 1 | límite de respuesta de lectura (RCB) de 64 bytes | ídem |
| `[4:0]` | `SCB2_SIZE` | no se toca | ídem del 2 | [`brcm_pcie_setup`] |

- Linux escribe primero ráfaga, `SCB_ACCESS_EN`, `CFG_READ_UR_MODE` y los dos RCB, con leer-cambiar-escribir. Luego
  programa las ventanas. Luego, en otra escritura, los tamaños SCB. [`brcm_pcie_setup`]
- Codificación de la ráfaga en la 2711: 0 = 128, 1 = 256, 2 = 512, 3 = reservado. La 2711 **exige 128**. El mensaje
  del commit `20b1d6ba606c` trae una tabla que no cuadra con el código (punto 11 de "Lo que no está claro"). Vale lo
  del código: 0.
- `SCB_SIZE`: la memoria de cada controlador se ve por un "puerto" de tamaño potencia de dos, igual o mayor que la
  memoria del controlador. Si hay varios, sus vistas van seguidas en el espacio PCIe. La 2711 tiene uno solo.
  [`pcie-brcmstb.c`, `brcm_pcie_get_inbound_wins`; commit `52ded9e4f07b`; `brcm,stb-pcie.yaml`, `brcm,scb-sizes`]
- Codificación de `SCBx_SIZE`: `log2(tamaño) − 15`. 1 GB → 15 (`0xF`), 2 GB → 16, 4 GB → 17. [`brcm_pcie_setup`]
  U-Boot pone 1 GB → 15 como ejemplo en un comentario [U-Boot, `brcm_pcie_probe`].

### 3.2. `MISC_RC_BARn_CONFIG_LO/HI` (n = 1, 2, 3)

| Registro | Bits | Contenido | Fuente |
|---|---|---|---|
| `_LO` | `[31:5]` | bits 31..5 del desplazamiento PCI de la ventana (`pci_offset`) | [`set_inbound_win_registers`] |
| `_LO` | `[4:0]` | `SIZE`: tamaño codificado (sección 5). **0 = ventana apagada.** | ídem; [`brcm_pcie_encode_ibar_size`] |
| `_HI` | `[31:0]` | bits 63..32 del desplazamiento PCI | ídem |

- El desplazamiento está alineado a su tamaño (mínimo 4 KB), así que sus bits bajos son 0 y no chocan con `SIZE`.
  Linux escribe `LO` como "parte baja del desplazamiento, con `SIZE` metido en `[4:0]`". [`set_inbound_win_registers`]
- Linux escribe `LO` y después `HI`, con escrituras sin barrera (`relaxed`). [`set_inbound_win_registers`]
- Qué es cada ventana en la 2711 (de Linux; los nombres entre paréntesis son de U-Boot):

| Ventana | Qué hace | Linux la deja | Fuente |
|---|---|---|---|
| BAR1 (PCIe → GISB) | Por defecto **no va a la memoria sino a los registros internos del chip**. Linux dice que es una función "obsoleta, con problemas de seguridad". | apagada (`SIZE` = 0, desplazamiento 0) | [`brcm_pcie_get_inbound_wins`, comentario "STB chips beside 7712 disable the first inbound window"]; U-Boot, `brcm_pcie_probe` |
| BAR2 (PCIe → SCB) | La ventana principal. Lleva al SCB, empezando siempre en la dirección SCB 0 (3.3). | encendida, tamaño de la vista SCB | [`brcm_pcie_get_inbound_wins`] |
| BAR3 (PCIe → SCB) | "En algunos chips, la misma ventana que la 2 pero con el orden de bytes ajustable". | apagada | ídem, comentario "Disable inbound window 3" |

- **BAR1 y BAR3 tienen que quedar apagadas.** BAR1 abierta daría al VL805 acceso a los registros del chip. BAR3
  abierta sería una segunda puerta a la memoria.
- Linux maneja hasta 16 ventanas (`PCIE_BRCM_MAX_INBOUND_WINS`) y tiene registros BAR4 en adelante (`0x40D4` +
  8·(n−4)). En la 2711 solo programa las 3 primeras (`num_inbound_wins` = 3). No se sabe si BAR4 y siguientes existen
  en la 2711 (punto 7 de "Lo que no está claro").

### 3.3. `MISC_UBUS_BARn_CONFIG_REMAP` (`0x40AC` + 8·(n−1))

- Solo los programa Linux en la 7712 / 2712 (Pi 5). Allí dan la dirección de la CPU donde cae cada ventana, con el
  bit 0 `ACCESS_EN`. [`set_inbound_win_registers`]
- En la 2711 **no se usan**: el lado de la CPU de BAR2 está fijo al principio de la memoria del sistema. Linux pone
  `cpu_addr` = 0 y no escribe nada más. [`brcm_pcie_get_inbound_wins`, comentario "the BAR2 cpu_addr is hardwired
  to the start of system memory"]. U-Boot dice lo mismo: "los registros de reasignación UBUS no se usan antes de la
  2712" [U-Boot, `brcm_pcie_set_inbound_windows`].
- Consecuencia: **en la 2711 la ventana siempre empieza en la dirección física 0.** Solo se elige su tamaño y su
  desplazamiento en el lado PCI. No se puede abrir, por ejemplo, solo la zona de 512 MB a 1 GB.

## 4. Cómo programa Linux la ventana de entrada

### 4.1. Cálculo [`brcm_pcie_get_inbound_wins`]

1. Recorre los `dma-ranges` del nodo. Para cada uno toma su dirección PCI (`pcie_start`) y su tamaño. Suma los
   tamaños y se queda con la dirección PCI más baja. Si no hay `dma-ranges`, falla.
2. Si el árbol trae `brcm,scb-sizes`, ese es el tamaño de cada controlador de memoria. Si no (la Pi 4), supone uno
   solo, de tamaño = la suma redondeada hacia arriba a potencia de dos. Con 3 GB de `dma-ranges` sale **4 GB**.
3. Tamaño de BAR2 = suma de los tamaños SCB, redondeada a potencia de dos. En la Pi 4: 4 GB.
4. Desplazamiento PCI de BAR2 = la dirección PCI más baja de los `dma-ranges`. En la Pi 4: 0.
5. Comprueba tres cosas y, si no se cumplen, falla:
   - el tamaño no es 0;
   - el desplazamiento es múltiplo del tamaño;
   - el desplazamiento **no** está entre 2 GB y 4 GB (sin incluir los extremos).
6. Ventanas que resultan: BAR1 apagada, BAR2 = (4 GB, desplazamiento 0, CPU 0), BAR3 apagada.

El comentario de esa función explica las reglas del hardware:

- La ventana debe cubrir toda la memoria seguida, ser potencia de dos y empezar en una dirección PCI alineada a su
  tamaño.
- Si parte de la ventana no es memoria (3 GB de RAM piden una ventana de 4 GB), la ventana de salida puede caer dentro
  de ella. El controlador "sabe mandar abajo lo de salida y arriba todo lo demás".
- Lo mejor es que la ventana esté en los primeros 4 GB del PCI, porque hay aparatos antiguos de 32 bits. Con 4 GB de
  memoria o más no puede empezar en 0 y se pone en "una vez su tamaño".
- Linux comprueba los `dma-ranges` porque en las primeras revisiones de la 2711 "el firmware tiene que editar
  `dma-ranges` al arrancar por un fallo de la integración del PCIe, que prohíbe cualquier acceso por encima de los 3
  GB bajos".

### 4.2. Escritura [`set_inbound_win_registers`]

Para n = 1, 2, 3: `BARn_CONFIG_LO` = parte baja del desplazamiento con `SIZE` en `[4:0]`; luego `BARn_CONFIG_HI` =
parte alta. Sin más. En la 2711 no escribe `UBUS_BAR*_CONFIG_REMAP`.

### 4.3. Tamaños SCB [`brcm_pcie_setup`]

Después de las ventanas, comprueba que el bloque está en modo RC (`MISC_PCIE_STATUS` bit 7 a 1; si no, falla con
"configurado como aparato final"). Luego escribe `SCB0_SIZE` = `log2(4 GB) − 15` = 17.

### 4.4. Orden de bytes [`brcm_pcie_setup`]

Al final del montaje pone `RC_CFG_VENDOR_VENDOR_SPECIFIC_REG1` `[3:2]` = 0: tráfico de entrada de BAR2 en
little-endian.

### 4.5. Valores que deja Linux en una Pi 4

| Registro | Valor | Cálculo |
|---|---|---|
| `MISC_RC_BAR1_CONFIG_LO/HI` | `0x00000000` / `0x00000000` | apagada |
| `MISC_RC_BAR2_CONFIG_LO/HI` | `0x00000011` / `0x00000000` | 4 GB → código 17, desplazamiento 0 |
| `MISC_RC_BAR3_CONFIG_LO/HI` | `0x00000000` / `0x00000000` | apagada |
| `MISC_MISC_CTRL[31:27]` | 17 | 4 GB |

Se deduce: **en Linux la ventana de hardware abarca 4 GB** (toda la memoria SCB de 0 a 4 GB). El límite de 3 GB lo
pone el software: la capa DMA de Linux respeta `dma-ranges` y no da al VL805 direcciones por encima. El hardware no
protege nada por encima de 1 GB. Por eso DashOS tiene que reprogramar la ventana.

### 4.6. MSI y ventana de salida frente a la ventana de entrada

- **MSI**: la dirección de destino de las MSI la decodifica `MISC_MSI_BAR_CONFIG`, aparte de las ventanas. Linux la
  pone **fuera** de la ventana de entrada [`brcm_pcie_setup`, cálculo de `msi_target_addr`; `brcm_msi_set_regs`]:
  - si la ventana empieza en 4 GB o más, o si acaba por debajo de 4 GB: `0x0_FFFFFFFC`;
  - si no (el caso de la Pi 4, ventana de 0 a 4 GB): `0xF_FFFFFFFC`.
  - `MSI_BAR_CONFIG_LO` = parte baja de esa dirección con el bit 0 a 1 (activar). `_HI` = parte alta.
  - `MSI_DATA_CONFIG` = `0xFFE06540` (MSI modernas, 32 vectores) o `0xFFF86540` (antiguas, 8 vectores).
- Con una ventana de 1 GB en el desplazamiento 0, las dos direcciones de MSI quedan fuera. Se deduce que **cerrar la
  ventana no obliga a tocar las MSI**.
- **Salida**: en el árbol del firmware, la ventana de salida ocupa PCI `0xC0000000` a `0xFFFFFFFF`. Con la ventana
  de entrada de 4 GB de Linux, se solapan (Linux lo da por bueno, 4.1). Con 1 GB, no se solapan.

## 5. Codificación del tamaño

[`pcie-brcmstb.c`, `brcm_pcie_encode_ibar_size`; igual en U-Boot]

Sea `k` = log2 del tamaño, **redondeado hacia abajo**.

| `k` | Tamaño | Código `SIZE` |
|---|---|---|
| 12 a 15 | 4 KB a 32 KB | `k − 12 + 0x1C` (4 KB = `0x1C`, 8 KB = `0x1D`, 16 KB = `0x1E`, 32 KB = `0x1F`) |
| 16 a 36 | 64 KB a 64 GB | `k − 15` (64 KB = 1, ..., 1 GB = 15, 2 GB = 16, 4 GB = 17, 64 GB = 21) |
| otro | — | 0 = ventana apagada |

Valores útiles:

| Tamaño | `SIZE` | `SCB0_SIZE` |
|---|---|---|
| 256 MB | 13 (`0x0D`) | 13 |
| 512 MB | 14 (`0x0E`) | 14 |
| 1 GB | 15 (`0x0F`) | 15 |
| 2 GB | 16 (`0x10`) | 16 |
| 4 GB | 17 (`0x11`) | 17 |

- El tamaño tiene que ser potencia de dos. Si no lo es, la función se queda con la potencia de abajo (3 GB → 2 GB).
  Linux redondea hacia arriba **antes** de llamarla. [`brcm_pcie_get_inbound_wins`]. "Los primeros 3 GB" no se puede
  expresar: o 2 GB o 4 GB.
- El tope de 64 GB es reciente (commit `25a98c727015`, para la 2712). Para la 2711 no cambia nada.
- Para `SCBx_SIZE`, Linux usa `log2 − 15` sin la rama de 4 KB a 32 KB. Por debajo de 64 KB no tiene sentido.

## 6. Traducción de direcciones

- En la 2711, BAR2 lleva el rango de bus `[pci_offset, pci_offset + tamaño)` a SCB `[0, tamaño)`. Y SCB = física
  (2.2). [`brcm_pcie_get_inbound_wins`, `cpu_addr` = 0]
- Luego: **dirección de bus = dirección física + `pci_offset`**, válida solo si la física es menor que el tamaño.
- U-Boot lo explica igual con otras palabras: en la 2711 el registro guarda "inicio de bus menos inicio físico", no
  la dirección PCI cruda. [U-Boot, `brcm_pcie_set_inbound_windows`]. Con inicio físico 0 es lo mismo.
- Relación con `dma-ranges`: cada entrada es (dirección PCI, dirección SCB, tamaño). Linux saca de ahí el
  `pci_offset` (la dirección PCI más baja) y el tamaño. **Ignora la dirección SCB** y la supone 0. Para traducir en
  el día a día (fuera del controlador), la capa DMA de Linux resta el desplazamiento de la entrada: bus = física −
  (física − PCI). Con el árbol de la Pi 4, ese desplazamiento es 0. [`brcm_pcie_get_inbound_wins`, uso de
  `entry->offset`]
- Con el árbol de la Pi 4 tal cual: **bus = física**, para físicas menores que 3 GB.
- El VL805 genera direcciones de hasta 36 bits: Linux le pone una máscara DMA de 36 bits. [`xhci-pci.c`,
  `xhci_pci_quirks`, `dma_mask_bits` = 36]. Las direcciones de bus se le dan en los campos de 64 bits del xHCI, con la
  parte alta a 0 si la ventana empieza en 0.
- Se deduce: si DashOS deja `pci_offset` = 0, la traducción es la identidad y no hay que restar nada. Es lo más
  sencillo y coincide con lo que espera el firmware (sección 8).

## 7. Qué hace Linux al arrancar el controlador

Orden completo de Linux en la Pi 4. [`brcm_pcie_probe`, `brcm_pcie_setup`, `brcm_pcie_start_link`,
`brcm_pcie_add_bus`]

1. Saca el puente del reinicio: `RGR1_SW_INIT_1` bit 1 = 0 (no hay `resets` de puente en el árbol, así que escribe el
   registro). No hay reloj, ni `rescal`, ni `swinit` en la Pi 4. [`brcm_pcie_probe`]
2. **Pone el puente en reinicio**: bit 1 = 1. [`brcm_pcie_setup`]
3. **Activa PERST#** (reinicio del VL805): `RGR1_SW_INIT_1` bit 0 = 1. Solo en la 2711 (arreglo `EARLY_PERST_ASSERT`).
4. Espera de 100 a 200 µs.
5. Saca el puente del reinicio: bit 1 = 0.
6. Enciende el SerDes: `HARD_DEBUG` bit 27 = 0. Espera de 100 a 200 µs.
7. `MISC_CTRL`: ráfaga 0, `SCB_ACCESS_EN` 1, `CFG_READ_UR_MODE` 1, `RCB_MPS_MODE` 1, `RCB_64B_MODE` 1.
8. **Ventanas de entrada** (sección 4): BAR1, BAR2, BAR3.
9. Comprueba el modo RC (`MISC_PCIE_STATUS` bit 7).
10. **`SCB0_SIZE`** en `MISC_CTRL`.
11. Calcula la dirección de las MSI (4.6).
12. Capacidad del enlace (`aspm-no-l0s`, `num-lanes`: no están en la Pi 4) y clase del puerto raíz:
    `RC_CFG_PRIV1_ID_VAL3[23:0]` = `0x060400` (puente PCI a PCI; por defecto el bloque se presenta como aparato final).
13. Ventanas de salida (`MISC_CPU_2_PCIE_MEM_WIN*`), una por cada `ranges` de memoria. Máximo 4.
14. Orden de bytes de BAR2 = little-endian.
15. (Fuera de `brcm_pcie_setup`) MSI: `MSI_INTR2` desenmascara y borra los 32 bits; escribe `MSI_BAR_CONFIG` y
    `MSI_DATA_CONFIG`. [`brcm_msi_set_regs`]
16. Al añadir el bus de abajo, arranca el enlace [`brcm_pcie_start_link`]:
    - quita PERST#: bit 0 = 0;
    - espera 100 ms (`PCIE_RESET_CONFIG_WAIT_MS` en `drivers/pci/pci.h`);
    - sondea `MISC_PCIE_STATUS` cada 5 ms, hasta 100 ms más, hasta ver los bits 5 y 4 a 1; si no, "enlace caído";
    - modo CLKREQ (sin propiedad en el árbol: "default", con `L1SS_ENABLE` y el temporizador de `0x9208` a 4 s, es
      decir 864.000.000) [`brcm_config_clkreq`, `brcm_extend_rbus_timeout`];
    - SSC por MDIO, porque el árbol trae `brcm,enable-ssc` [`brcm_pcie_set_ssc`].
17. Enumera. El VL805 queda en bus 1, aparato 0, función 0.
18. El controlador xHCI, al empezar, pide el reinicio "USB" del firmware: la carga del firmware del VL805 (sección
    8). [`xhci-pci.c`, `xhci_pci_common_probe`]

Puntos clave para la ventana:

- **Linux nunca toca las ventanas con el enlace levantado.** Las programa justo tras reiniciar el puente y con PERST#
  activo, antes de soltar el VL805. Lo mismo al volver de suspensión: apaga (L23, PERST#, SerDes, puente en reinicio)
  y repite todo `brcm_pcie_setup`. [`brcm_pcie_turn_off`, `brcm_pcie_resume_noirq`]
- **Linux no se fía del estado que deja el cargador**: lo reinicia todo (commit `8eb8c2735306`: "algunas versiones
  antiguas del cargador de la 2711/RPi dejaban PERST# sin activar").
- Un acceso de configuración a los aparatos de abajo **sin enlace** provoca un aborto de la CPU. Linux no lo hace
  si los bits 5 y 4 de `MISC_PCIE_STATUS` no están a 1. [`brcm_pcie_map_bus`, comentario "An access to our HW w/o
  link-up will cause a CPU Abort"]

## 8. Carga del firmware del VL805 (`NOTIFY_XHCI_RESET`)

- Tras un reinicio PCI (PERST#), el VL805 carga su firmware de una EEPROM propia o, si la placa no la tiene, se lo da
  la VideoCore. El código de carga y el firmware están dentro del firmware de la VideoCore, sin publicar.
  [`reset-raspberrypi.c`, `rpi_reset_reset`; commit `fbbc5ff3f7f9`]
- Mensaje del buzón de propiedades:

| Dato | Valor | Fuente |
|---|---|---|
| Etiqueta | `0x00030058` `RPI_FIRMWARE_NOTIFY_XHCI_RESET` | [`raspberrypi-firmware.h`] |
| Petición | una palabra: dirección del aparato = bus `<< 20` \| aparato `<< 15` \| función `<< 12` | [`rpi_reset_reset`] |
| Valor en la Pi 4 | `0x00100000` (bus 1, aparato 0, función 0), fijo porque el PCIe va soldado | ídem |
| Después | esperar de 200 a 1000 µs "a que arranque el VL805" | ídem |

- Linux la lanza al empezar el controlador xHCI, antes de tocar el VL805, y al volver de suspensión.
  [`xhci-pci.c`, `xhci_pci_common_probe`, `xhci_pci_resume`]. El arreglo de arranque de USB de Linux se salta el VL805
  de la Pi 4: "de eso se encarga el coprocesador". [`pci-quirks.c`, `quirk_usb_early_handoff`]
- **La VideoCore espera un bus PCI ya configurado.** Lo dice la primera versión de esta rutina: "si algo fue mal con
  el PCI, todo esto es inútil, porque la VideoCore espera de nosotros un bus PCI configurado". [commit
  `fbbc5ff3f7f9`, `rpi_firmware_init_vl805`, luego retirada]. Es decir: enlace levantado y VL805 en bus 1.
- Esa primera versión leía la palabra de configuración `0x50` del VL805 como versión del firmware: 0 = sin cargar.
  Si no era 0, no pedía la carga. Después pedía la carga y volvía a leerla. [commit `fbbc5ff3f7f9`]. La versión
  actual pide la carga siempre. Se deduce que sirve para comprobar que la carga fue bien.
- **¿Toca la ventana?** Linux no lo dice. El código de la VideoCore es cerrado. No se sabe si reprograma registros del
  controlador, ni si el VL805 lee su firmware de la memoria por DMA (punto 5 de "Lo que no está claro"). Se deduce lo
  prudente:
  - programar la ventana **antes** de este mensaje y con `pci_offset` = 0, para que una dirección que use la
    VideoCore (que piensa en bus = física) siga valiendo si está por debajo del tamaño;
  - **volver a leer** BAR1, BAR2, BAR3 y `MISC_CTRL` después del mensaje, y si cambiaron, reescribirlos y avisar en
    el registro del kernel.

## 9. Casos raros y erratas

1. **Fallo de los 3 GB.** "El envoltorio del bloque PCIe tiene un fallo que le impide acceder más allá de los
   primeros 3 GB de memoria." [`bcm2711.dtsi`, comentario en `pcie0`]. En las primeras revisiones del chip el
   firmware edita `dma-ranges` al arrancar por eso. [`brcm_pcie_get_inbound_wins`, comentario]. Con una ventana de 1
   GB no afecta.
2. **BAR1 a los registros.** Si BAR1 queda abierta, el VL805 puede escribir en los registros del chip. Apagarla
   siempre (`SIZE` = 0). [`brcm_pcie_get_inbound_wins`]
3. **BAR3 alias de BAR2.** Apagarla siempre. [ídem]
4. **Tamaño no potencia de dos.** El código trunca hacia abajo (sección 5). 3 GB no existe.
5. **Alineación.** El desplazamiento PCI tiene que ser múltiplo del tamaño, y no puede estar entre 2 GB y 4 GB.
   [`brcm_pcie_get_inbound_wins`]
6. **La ventana física siempre empieza en 0** (3.3). Todo lo que haya en `[0, tamaño)` queda al alcance del VL805.
7. **PERST# suelto al arrancar.** Cargadores antiguos dejan el VL805 fuera de reinicio. Linux activa PERST# antes de
   soltar el puente, solo en la 2711. [`brcm_pcie_setup`; `bcm2711_cfg`, `EARLY_PERST_ASSERT`; commit `8eb8c2735306`]
8. **Configuración sin enlace = aborto de la CPU.** (Sección 7.)
9. **Ráfaga de 128 bytes.** Obligatoria en la 2711. [`brcm_pcie_setup`; commit `20b1d6ba606c`]
10. **El VL805 lee de más al final de un anillo.** Linux marca el VL805 con `XHCI_TRB_OVERFETCH`: "el controlador
    lee más allá del final del segmento". Linux reserva cada segmento de anillo con el doble de tamaño (y alineado
    al doble). [`xhci-pci.c`, `xhci_pci_quirks`; `xhci-mem.c`, `xhci_mem_init`]. Se deduce: **un segmento al final
    de la ventana haría que el VL805 leyera fuera de ella.** Con la ventana cerrada, esa lectura fallaría (sección
    10). Hay que dejar margen: hacer como Linux, o no poner anillos en las últimas páginas de la ventana.
11. **Máscara DMA de 36 bits** del VL805. [`xhci-pci.c`, `xhci_pci_quirks`]. Puede emitir direcciones hasta 64 GB.
    La ventana es lo que lo frena.
12. **MSI fuera de la ventana.** La dirección de las MSI no debe caer dentro de la ventana de entrada (4.6).
13. **Solape con la ventana de salida.** Linux lo admite con la ventana de 4 GB (4.1). Con una de 1 GB desaparece.
14. **Modo RC.** Si `MISC_PCIE_STATUS` bit 7 está a 0, el bloque está como aparato final y Linux se niega a seguir.
    [`brcm_pcie_setup`]
15. **Sin informe de errores en la 2711.** Los registros `OUTB_ERR_*` (`0x6000` y siguientes) y su volcado solo se
    usan en chips con `HAS_ERR_REPORT` (la 7216). Además son de errores **de salida** (CPU → PCIe), no de entrada.
    [`pcie-brcmstb.c`, `brcm_pcie_dump_err`; `bcm2711_cfg`]
16. **La interrupción "pcie"** (SPI 147) existe en el árbol, pero Linux no le pone manejador. Solo usa la "msi" (SPI
    148). [`brcm_pcie_enable_msi`, usa el índice 1]

## 10. Qué pasa con un acceso fuera de la ventana

- **Linux no lo dice.** No hay ningún registro, interrupción ni comentario sobre accesos de entrada rechazados en la
  2711.
- Se deduce de la norma PCI Express (no de Linux): una petición de memoria que llega al complejo raíz y no cae en
  ninguna ventana es una "petición no admitida" (UR).
  - Una **lectura** recibe una respuesta de error. El xHCI, según su norma, marca entonces "error del sistema
    anfitrión" (`USBSTS.HSE`, bit 2) y se para.
  - Una **escritura** se descarta. El VL805 no se entera.
  - El puerto raíz debería anotarlo en su registro "Device Status" de la capacidad PCIe (`0xAC` + `0x0A` = `0xB6`, bit
    3 "Unsupported Request Detected") y en su registro de estado PCI (`0x06`).
- No se sabe si el envoltorio de Broadcom hace eso, si lo descarta en silencio o si cuelga el bus. Hay que probarlo
  (sección 12).
- Se deduce también que lo normal no es un fallo grave: en una Pi 4 con Linux la ventana es de 4 GB y no hay nada
  configurado para avisar, así que nadie lo ha visto.

## 11. Procedimiento propuesto para DashOS

Es una propuesta del analista, no algo que haga Linux. Se basa en las secciones anteriores.

### 11.1. Valores

Para que el VL805 solo alcance el primer GB:

| Registro | Valor |
|---|---|
| `MISC_RC_BAR1_CONFIG_LO/HI` | 0 / 0 |
| `MISC_RC_BAR2_CONFIG_LO` | `0x0000000F` (desplazamiento 0, 1 GB) |
| `MISC_RC_BAR2_CONFIG_HI` | 0 |
| `MISC_RC_BAR3_CONFIG_LO/HI` | 0 / 0 |
| `MISC_MISC_CTRL[31:27]` (`SCB0_SIZE`) | 15 (1 GB) |
| Resto de `MISC_MISC_CTRL` | como Linux (3.1) |

- Traducción: bus = física, para físicas por debajo de 1 GB.
- **Mantener iguales el tamaño de BAR2 y `SCB0_SIZE`.** No se sabe qué pasa si BAR2 es mayor que la vista SCB: puede
  rechazarse, o puede **dar la vuelta** y caer en la memoria baja (punto 2 de "Lo que no está claro"). Esto último
  sería una corrupción, no una protección.
- La zona de la memoria para DMA del PCIe tiene que estar dentro de `[0, 1 GB)`. Y lo ideal es una ventana más
  pequeña (256 o 512 MB) con los búferes del USB abajo del todo. Así el VL805 no alcanza la memoria de la VideoCore
  (arriba del primer GB) ni los búferes de otros aparatos. Ver punto 6 de la sección 9.
- No se toca ni `pci_offset` ni las MSI.
- DashOS no usa el `dma-ranges` del árbol para programar la ventana. Lo lee y lo anota en el registro del kernel,
  para saber qué supuso el firmware.

### 11.2. Camino seguro: reiniciar el controlador

Recomendado. Como Linux (sección 7): puente en reinicio y PERST# activo, ventanas, `SCB0_SIZE`, resto del montaje,
quitar PERST#, esperar el enlace, enumerar, `NOTIFY_XHCI_RESET`, comprobar la palabra `0x50` del VL805, volver a leer
las ventanas.

- Ventajas: es lo único que Linux hace y está probado. Detiene cualquier DMA que dejara en marcha el cargador (por
  ejemplo tras arrancar desde USB). Y deja BAR1 apagada aunque el reinicio la vuelva a su valor por defecto (que,
  según el comentario de Linux, apunta a los registros).
- Coste: unos 200 ms (100 ms tras PERST# y hasta 100 ms de espera del enlace) y repetir el montaje del enlace (SSC,
  CLKREQ, MSI, ventanas de salida).
- Hacerlo **pronto**: antes de que el kernel o un proceso vivan por encima de 1 GB con la ventana abierta. Hasta
  entonces, con la ventana del firmware, el VL805 alcanza 3 o 4 GB.

### 11.3. Camino rápido: cambiar en caliente

Posible pero **sin respaldo en Linux**. Si se usa:

1. Hacerlo antes de arrancar el controlador xHCI de DashOS y antes de `NOTIFY_XHCI_RESET`. El VL805 no debería estar
   haciendo DMA.
2. Leer y anotar el valor que dejó el firmware en BAR1, BAR2, BAR3, `MISC_CTRL`, `RGR1_SW_INIT_1` y
   `MISC_PCIE_STATUS`.
3. Escribir BAR1_LO = 0 y BAR3_LO = 0 (y sus HI = 0).
4. Escribir BAR2_LO con el nuevo tamaño. Con desplazamiento 0, BAR2_HI no cambia: el cambio es **una sola escritura
   de 32 bits**, sin estado intermedio. (Se deduce.)
5. Después, `SCB0_SIZE`, con leer-cambiar-escribir de `MISC_CTRL`. En este orden: primero BAR2, luego SCB0, para no
   pasar nunca por "BAR2 mayor que SCB0" al encoger.
6. Leer de vuelta todos y comprobar.

Riesgos:

- Un DMA en vuelo por encima del nuevo tamaño falla a medias. Si el cargador dejó el VL805 en marcha, se parará con
  error. Da igual si luego se reinicia el VL805, pero el camino 11.2 lo evita del todo.
- No se sabe si el controlador admite el cambio con tráfico. Linux nunca lo hace.
- Si el reinicio de puente que haga el firmware (o un reinicio posterior) devuelve BAR1 a su valor por defecto, hay
  que repetir el paso 3.

## 12. Cómo probarlo

En una Pi 4 real. Se deduce que QEMU (`raspi4b`) no sirve: no consta que emule este PCIe.

1. Leer y anotar el estado que deja el firmware (punto 4 de "Lo que no está claro").
2. Cerrar la ventana a 1 GB (11.2).
3. USB normal: un teclado y un disco funcionan con búferes por debajo de 1 GB.
4. Prueba negativa: poner a propósito un anillo del xHCI (o el búfer de un comando) por encima de 1 GB. Esperado:
   `USBSTS.HSE` a 1 en el VL805, o un error visible en el estado del puerto raíz (sección 10). Y la memoria de arriba
   sin tocar (rellenarla antes con un patrón y comprobarlo después).
5. Prueba de vuelta: con BAR2 de 1 GB y `SCB0_SIZE` mayor y menor, mirar si una dirección por encima de 1 GB cae en
   la memoria baja (rellenar `[0, 1 GB)` con un patrón y ver si cambia).
6. Repetir 3 y 4 tras `NOTIFY_XHCI_RESET` y tras un reinicio del VL805.

## Lo que no está claro

1. **Acceso fuera de la ventana.** Linux no dice qué pasa: respuesta UR, descarte en silencio, cuelgue del bus o
   interrupción. La sección 10 es deducción de la norma PCIe.
2. **`SCB0_SIZE` frente al tamaño de BAR2.** No se sabe qué manda si no coinciden, ni si una dirección por encima de
   la vista SCB da la vuelta. Por eso se proponen iguales.
3. **Cambio en caliente.** Linux siempre programa las ventanas con el puente recién reiniciado y PERST# activo. No hay
   nada que diga que se puede con el enlace levantado y el VL805 activo.
4. **Estado que deja el firmware.** No se sabe con qué ventanas, `MISC_CTRL`, PERST# ni enlace llega el kernel.
   Tampoco si el cargador de la EEPROM deja el VL805 haciendo DMA tras arrancar desde USB. Solo consta que versiones
   antiguas dejaban PERST# suelto (commit `8eb8c2735306`). Que el firmware deja el PCIe iniciado viene del encargo,
   no de Linux.
5. **`NOTIFY_XHCI_RESET` por dentro.** No se sabe si la VideoCore toca registros del controlador (ventanas
   incluidas), ni si el VL805 lee su firmware de la memoria por DMA, ni en qué dirección.
6. **Valor de BAR1 tras reiniciar el puente.** El comentario de Linux da a entender que por defecto apunta a los
   registros del chip. No se sabe si el reinicio de `RGR1_SW_INIT_1` la devuelve a ese valor.
7. **BAR4 y siguientes en la 2711.** Linux no los programa en la 2711. No se sabe si existen, ni si el firmware los
   deja abiertos. Leerlos en una placa: si leen 0 o no existen, bien.
8. **`SCB_ACCESS_EN` y `CFG_READ_UR_MODE`.** Linux los pone a 1 sin explicar qué hacen. Se deduce que `SCB_ACCESS_EN`
   a 0 cortaría todo el DMA del PCIe (un freno de emergencia), pero no está comprobado.
9. **`dma-ranges` que da el firmware en cada revisión del chip.** El fichero dice 3 GB. El firmware puede editarlo al
   arrancar (S3). Para programar la ventana da igual, pero hay que leerlo en una placa.
10. **Revisión del bloque en la Pi 4** (`MISC_REVISION`). Decide entre MSI antiguas y modernas. No consta en Linux.
11. **Codificación de `MAX_BURST_SIZE`.** El comentario del código (0 = 128 en la 2711) y la tabla del commit
    `20b1d6ba606c` no cuadran. Se sigue el código.
12. **Modo de periféricos alto.** La dirección `0xFD500000` vale con el modo bajo. No se ha comprobado con cuál
    arranca DashOS.
13. **El fallo de los 3 GB en la revisión C0.** S3 dice que las placas C0 alcanzan más. Linux no distingue
    revisiones en el controlador. Con una ventana de 1 GB no importa.
14. **Qué hay en el primer GB que no debería estar al alcance del VL805.** La memoria de la VideoCore, el `armstub`
    en la dirección 0, el árbol de dispositivos que carga el firmware y los búferes de otros aparatos. Con una
    ventana de 1 GB todo eso queda expuesto. Es un límite del hardware (3.3), no de este documento.
