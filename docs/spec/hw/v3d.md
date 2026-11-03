# GPU V3D 4.2 de la BCM2711 (Raspberry Pi 4)

Especificación de sala limpia del lado del kernel. La escribe el analista leyendo el controlador v3d de Linux. Quien
implemente no necesita ni debe leer ese código: todo lo necesario tiene que estar aquí.

Este documento cubre el hardware y lo que hace el kernel con él. Lo que pide Mesa (formato de las listas de control,
de los sombreadores y de las texturas) queda para otro documento.

## 0. Origen y convenciones

- **Fuente**: `torvalds/linux`, commit `6628e7d4c6b25c8d18f1345e3a20d21829aae7e4` (1 de noviembre de 2026). Carpeta
  `drivers/gpu/drm/v3d/` completa y `include/uapi/drm/v3d_drm.h`. Para la energía y los relojes, también
  `drivers/pmdomain/bcm/bcm2835-power.c`, `drivers/mfd/bcm2835-pm.c`, `drivers/clk/bcm/clk-raspberrypi.c`,
  `drivers/clk/bcm/clk-bcm2835.c`, `include/soc/bcm2835/raspberrypi-firmware.h`,
  `include/dt-bindings/soc/bcm2835-pm.h` y `arch/arm/boot/dts/broadcom/bcm2711*.dtsi`. Para el reinicio,
  `drivers/gpu/drm/scheduler/sched_main.c`.
- **Árbol de dispositivos**: `build/firmware/bcm2711-rpi-4-b.dtb` del repositorio, decompilado con `dtc`. Es el árbol
  del firmware de Raspberry Pi. Coincide con el de Linux en lo que importa aquí, salvo donde se dice.
- **Generación**: Linux llama `V3D_GEN_42` a la V3D de la BCM2711. La de la BCM2712 (Pi 5) es `V3D_GEN_71`. Las
  versiones 3.3 y 4.1 son de chips de decodificadores de Broadcom (BCM7268, BCM7278) y Linux las da por obsoletas.
  Este documento describe solo la 4.2. Las diferencias con la 7.1 van en la sección 16.
- **Citas**: cada dato lleva entre corchetes el fichero y la función de Linux de donde sale. Por ejemplo,
  [`v3d_mmu.c`, `v3d_mmu_set_page_table`]. Si un dato sale de una cabecera, se cita la cabecera.
- **Comprobado y deducido**: "Linux hace X" significa que se leyó en el código. "Se deduce" significa que es una
  conclusión del analista, no escrita como tal en Linux. Nada de este documento se ha probado en hardware real.
- **Notación**: los desplazamientos son en bytes desde la base del bloque que se indica (hub o núcleo). Un campo
  `[h:l]` ocupa los bits de `h` a `l` de una palabra de 32 bits, ambos incluidos. "Palabra" son siempre 32 bits.
- **Nombres**: los nombres de registros y bits son los de Linux (`V3D_MMU_CTL`, `V3D_INT_FRDONE`...). Se usan para
  que se puedan contrastar. En las tablas se quita el prefijo `V3D_` cuando no hay duda.
- **"VA"** es una dirección virtual de la GPU, de 32 bits. **"PA"** es una dirección física de la RAM.

## 1. Qué es la V3D 4.2

- Es una GPU de **dibujo por baldosas** (tiles). Cada cuadro se dibuja en dos fases:
  - **Binning**: el "binner" ejecuta una lista de control (la BCL), corre los sombreadores de coordenadas y decide en
    qué baldosas cae cada primitiva. Escribe, para cada baldosa, una lista con lo que hay que dibujar en ella.
  - **Render**: ejecuta otra lista de control (la RCL), que recorre las baldosas, dibuja cada una en una memoria
    interna y la escribe en la RAM.
- Además tiene dos unidades independientes:
  - **TFU** (texture formatting unit): convierte imágenes a los formatos de textura de la GPU y genera mipmaps.
  - **CSD** (compute shader dispatch): lanza sombreadores de cómputo sin pasar por las listas de control.
- Los programas de la GPU corren en los **QPU**, agrupados en "slices". Cada slice tiene sus cachés y sus TMU
  (unidades de texturas y de memoria).
- Todo acceso de la GPU a la RAM pasa por su **MMU**, con direcciones virtuales de 32 bits.
  [`v3d_drv.c`, comentario inicial; `v3d_mmu.c`, comentario inicial]
- Los registros se reparten en dos bloques:
  - **Hub**: lo compartido por todos los núcleos. Identificación global, interrupciones del hub, MMU y TFU.
  - **Núcleo 0** ("core0"): el núcleo de dibujo. Identificación del núcleo, cachés, interrupciones del núcleo,
    listas de control (CLE), binner (PTB), contadores, GMP y CSD.
- La BCM2711 tiene un solo núcleo. Linux no sabe manejar más de uno y avisa si el hardware dice otra cosa.
  [`v3d_drv.c`, `v3d_platform_drm_probe`]
- Hay otros bloques que Linux conoce pero que **no existen o no se usan en la 4.2**:
  - **GCA**: solo en versiones anteriores a la 4.1. [`v3d_regs.h`, comentario "GCA registers only exist in V3D <
    41"; `v3d_drv.c`, `v3d_platform_drm_probe`]
  - **Bridge** (`V3D_TOP_GR_BRIDGE_*`): solo se usa si el árbol no da un control de reinicio. La Pi 4 sí lo da
    (sección 2.4), así que no se usa. [`v3d_drv.c`, `v3d_platform_drm_probe`; `v3d_gem.c`, `v3d_reset_v3d`]
  - **SMS**: solo en la 7.1. [`v3d_drv.c`, `v3d_platform_drm_probe`]

Nombres de las unidades internas que aparecen en los registros (de los nombres de Linux; el significado de las
siglas es deducido salvo donde se cita):

| Sigla | Unidad |
|---|---|
| CLE | Ejecutor de listas de control. Tiene dos hilos: 0 para binning (CT0) y 1 para render (CT1). |
| PTB | Binner de primitivas por baldosas. |
| PSE | Montaje de primitivas. |
| TLB | Memoria de baldosa (tile buffer), no la TLB de la MMU. |
| L2T | Caché de nivel 2 de las TMU. |
| TMU | Unidad de texturas y de acceso a memoria desde los sombreadores. |
| VCD, VDW, VPM | Lectura de vértices, escritura de vértices y memoria de vértices. |
| GMP | Protección de memoria por zonas. Linux no la configura (sección 5.8). |

## 2. Dónde está y qué necesita

### 2.1. Direcciones

| Dato | Valor | Fuente |
|---|---|---|
| Compatible | `brcm,2711-v3d` | DTB, nodo `v3dbus/v3d@7ec04000`; [`v3d_drv.c`, `v3d_of_match`] |
| Hub, dirección de bus | `0x7EC00000`, 16 KB (`0x4000`) | DTB, `reg` con nombre `hub` |
| Núcleo 0, dirección de bus | `0x7EC04000`, 16 KB (`0x4000`) | DTB, `reg` con nombre `core0` |
| Hub, dirección física para la CPU | `0xFEC00000` | Se deduce del `ranges` de `v3dbus`: el bus `0x7C500000` se ve en `0x0_FC500000` |
| Núcleo 0, dirección física para la CPU | `0xFEC04000` | Ídem |
| GCA, bridge, SMS | No hay en la BCM2711 | Sección 1 |

- En el árbol del firmware el nodo está dentro de un bus propio, `v3dbus`. En el de Linux está en el bus `scb`. Las
  dos traducciones dan la misma dirección física. [DTB; `bcm2711.dtsi`, nodo `v3d: gpu@7ec00000`]
- El nodo está con `status = "disabled"` en el DTB. El firmware lo activa cuando se carga el overlay de KMS. Para
  DashOS no importa: el controlador no depende de ese estado.
- Igual que en el HVS, la dirección física vale con el modo de periféricos "bajo" que usa el firmware por defecto. No
  se ha comprobado qué modo deja Emu68.

### 2.2. Interrupción

| Dato | Valor | Fuente |
|---|---|---|
| Línea | GIC SPI 74, nivel alto | DTB (`interrupts = <0 0x4a 4>`); `bcm2711.dtsi` |
| INTID en el GIC-400 | 106 (SPI + 32) | Se deduce |
| Número de líneas | **Una sola** para hub y núcleo | DTB; [`v3d_irq.c`, `v3d_irq_init`] |

- Linux busca una segunda interrupción. Si no la hay, entiende que el hub y el núcleo comparten línea. Es el caso de
  la 4.2: el comentario dice que la 4.2 "une las interrupciones del hub y del núcleo". [`v3d_irq.c`, `v3d_irq_init`
  y `v3d_irq`]
- El tratamiento está en la sección 9.

### 2.3. Reloj

- El nodo de la V3D pide un reloj: el número 5 del firmware, `V3D`. [DTB, `clocks = <&firmware_clocks 5>`;
  `bcm2711-rpi.dtsi`; `raspberrypi-firmware.h`, `RPI_FIRMWARE_V3D_CLK_ID`]
- Lo maneja el firmware de la VideoCore por el buzón de propiedades. Linux lo trata así [`clk-raspberrypi.c`]:

| Momento | Etiqueta | Datos | Función de Linux |
|---|---|---|---|
| Al registrar el reloj | `0x00030007` `GET_MIN_CLOCK_RATE` | id 5 | `raspberrypi_clk_register` |
| Al registrar el reloj | `0x00030004` `GET_MAX_CLOCK_RATE` | id 5 | `raspberrypi_clk_register` |
| Encender | `0x00038001` `SET_CLOCK_STATE` | id 5, estado con el bit 0 a 1 | `raspberrypi_fw_prepare` |
| Justo después de encender | `0x00038002` `SET_CLOCK_RATE` | id 5, el máximo leído | `raspberrypi_fw_prepare` |
| Apagar, primero | `0x00038002` `SET_CLOCK_RATE` | id 5, el mínimo leído | `raspberrypi_fw_unprepare` |
| Apagar, después | `0x00038001` `SET_CLOCK_STATE` | id 5, estado 0 | `raspberrypi_fw_unprepare` |
| Consultar | `0x00030002` `GET_CLOCK_RATE`, `0x00030001` `GET_CLOCK_STATE` | id 5 | `raspberrypi_fw_get_rate`, `raspberrypi_fw_is_prepared` |

- Cada mensaje lleva tres palabras: el id del reloj, el valor y un tercer campo (`disable_turbo`) que Linux deja a 0.
  El firmware devuelve el valor en la segunda palabra. [`clk-raspberrypi.c`, `raspberrypi_clock_property`]
- El reloj V3D está marcado como "maximizar": cada vez que se enciende, Linux lo pone a su máximo.
  [`clk-raspberrypi.c`, tabla de variantes, entrada de `V3D_CLK_ID`]
- El comentario de Linux dice que `SET_CLOCK_STATE` a 0 **no apaga de verdad** el reloj con el firmware actual. Por
  eso baja antes la frecuencia al mínimo, para ahorrar. [`clk-raspberrypi.c`, `raspberrypi_fw_unprepare`]
- La frecuencia se puede medir con un contador de ciclos (sección 12). [`v3d_debugfs.c`, `v3d_measure_clock`]

### 2.4. Dominio de energía y reinicio

En la Pi 4, Linux **no** usa el buzón para la energía de la V3D. Usa el bloque PM del SoC y un puente AXI asíncrono
(ASB). [DTB, `power-domains = <&pm 1>` y `resets = <&pm 0>`; `bcm2835-pm.h`, `BCM2835_POWER_DOMAIN_GRAFX_V3D` = 1 y
`BCM2835_RESET_V3D` = 0]

Registros implicados:

| Bloque | Dirección de bus | Física (CPU) | Fuente |
|---|---|---|---|
| PM | `0x7E100000`, `0x114` bytes | `0xFE100000` | DTB, nodo `watchdog@7e100000`, `reg` `pm` |
| ASB general | `0x7E00A000`, `0x24` bytes | `0xFE00A000` | Ídem, `reg` `asb` |
| ASB de RPiVid (el de la V3D en la 2711) | `0x7EC11000`, `0x20` bytes | `0xFEC11000` | Ídem, `reg` `rpivid_asb` |
| cprman (reloj `v3d` del bloque PM) | `0x7E101000` | `0xFE101000` | DTB, nodo `cprman@7e101000` |

Las direcciones físicas se deducen del `ranges` del nodo `soc`.

| Registro | Desp. | Bits que usa Linux | Fuente |
|---|---|---|---|
| `PM_GRAFX` (bloque PM) | `0x10C` | 6 `V3DRSTN`: 1 = V3D fuera de reinicio | [`bcm2835-power.c`] |
| `ASB_V3D_S_CTRL` (ASB de RPiVid) | `0x08` | 0 `REQ_STOP`, 1 `ACK` | Ídem |
| `ASB_V3D_M_CTRL` (ASB de RPiVid) | `0x0C` | Ídem | Ídem |
| `ASB_AXI_BRDG_ID` (los dos ASB) | `0x20` | Lectura: debe valer `0x62726467` ("brdg") | [`bcm2835-power.c`, `bcm2835_power_probe`] |
| `CM_V3DCTL` (cprman) | `0x038` | 4 `ENABLE`, 6 `GATE`, 7 `BUSY` | [`clk-bcm2835.c`, `bcm2835_clock_on` y `bcm2835_clock_off`] |

- **Contraseña**: toda escritura en el bloque PM y en los ASB lleva `0x5A` en los bits 31:24. Linux hace la OR de
  `0x5A000000` con el valor. Lo mismo en cprman. [`bcm2835-power.c`, `PM_WRITE` y `bcm2835_asb_control`;
  `clk-bcm2835.c`, `CM_PASSWORD`]
- **En la ASB de la V3D se usa la de RPiVid**: si el bloque PM tiene esa ventana, Linux dirige ahí los dos registros
  de la V3D. La 2711 la tiene. [`bcm2835-power.c`, `bcm2835_asb_control`]
- **Activar un lado del puente**: leer, quitar `REQ_STOP`, escribir con contraseña y esperar a que `ACK` sea 0.
  **Desactivar**: poner `REQ_STOP` y esperar a que `ACK` sea 1. Plazo de 100 µs. [`bcm2835-power.c`,
  `bcm2835_asb_control`]
- **El interruptor de alimentación no se toca en la 2711.** El dominio padre (`GRAFX`) usa `POWUP`, `POWOK`,
  aislamientos y reparación de memoria en la BCM2835. En la 2711 Linux se lo salta: el código dice "no lo
  ejecutamos en la BCM2711". [`bcm2835-power.c`, `bcm2835_power_power_on` y `bcm2835_power_power_off`]
- Se deduce que en la Pi 4 "encender el dominio de la V3D" se reduce a: reloj, reinicio y puente AXI.

**Encender el dominio V3D** [`bcm2835-power.c`, `bcm2835_asb_power_on`]:

1. Encender el reloj `v3d` de cprman (`CM_V3DCTL`: poner `ENABLE` y `GATE`).
2. Esperar 1 µs. El comentario dice que son "32 ciclos para que el reinicio se propague".
3. Apagar ese reloj (`CM_V3DCTL`: quitar `ENABLE` y esperar a que `BUSY` sea 0).
4. `PM_GRAFX`: poner `V3DRSTN` (sacar la V3D del reinicio).
5. Encender otra vez el reloj `v3d` de cprman.
6. Activar el lado maestro del puente (`ASB_V3D_M_CTRL`).
7. Activar el lado esclavo (`ASB_V3D_S_CTRL`).

**Apagar el dominio V3D** [`bcm2835-power.c`, `bcm2835_asb_power_off`]:

1. Desactivar el lado esclavo del puente. Si falla, se aborta.
2. Desactivar el lado maestro. Si falla, se reactiva el esclavo y se aborta.
3. Apagar el reloj `v3d` de cprman.
4. `PM_GRAFX`: quitar `V3DRSTN` (meter la V3D en reinicio).

**Reiniciar la V3D** es apagar el dominio y volver a encenderlo con las dos secuencias anteriores. El comentario dice
que no se puede reiniciar con el dominio apagado. [`bcm2835-power.c`, `bcm2835_reset_reset`]

Sobre el buzón para la energía:

- El DTB tiene también un proveedor de dominios por firmware (`raspberrypi,bcm2835-power`). Su dominio 10 es `V3D` y
  se maneja con `SET_DOMAIN_STATE` (`0x00038030`) y dos palabras: dominio y estado. [`raspberrypi-power.c`,
  `rpi_firmware_set_power`; `raspberrypi-power.h`, `RPI_POWER_DOMAIN_V3D`]
- Linux **no** lo usa para la V3D de la Pi 4: el nodo apunta al bloque PM. No se sabe si funcionaría.

### 2.5. Orden de encendido completo (lo que hace Linux)

Se deduce del orden de la gestión de energía de Linux: el dominio se enciende antes de llamar a la función de
reanudar del controlador, y se apaga después de la de suspender.

1. Encender el dominio V3D (2.4).
2. Encender el reloj del firmware y ponerlo al máximo (2.3). [`v3d_power.c`, `v3d_power_resume`]
3. Iniciar el estado fijo del núcleo (sección 8.3). [`v3d_gem.c`, `v3d_init_hw_state`]
4. Dar la tabla de páginas a la MMU y limpiar sus cachés (sección 5.4). [`v3d_mmu.c`, `v3d_mmu_set_page_table`]
5. Activar las interrupciones (sección 9.4). [`v3d_irq.c`, `v3d_irq_enable`]
6. Si había contadores de rendimiento activos, volver a programarlos. [`v3d_perfmon.c`, `v3d_perfmon_resume`]

Al arrancar, Linux además [`v3d_drv.c`, `v3d_platform_drm_probe`]:

- Reserva la página de relleno de la MMU (5.5) y la tabla de páginas (5.1) antes de encender nada.
- Lee la identificación (sección 3) con la GPU ya encendida.
- No reinicia la GPU. Si el firmware la dejó a medias, Linux no lo trata.
- Deja que la GPU se apague sola tras 50 ms sin trabajo, y la enciende al llegar trabajo (sección 11).

### 2.6. Qué memoria alcanza

- La MMU traduce VA de 32 bits a PA. En su salida, la V3D ve la RAM **sin desplazamiento**: el `dma-ranges` de
  `v3dbus` dice que la dirección `0` del bus es la física `0`, con un tamaño de `0x4_0000_0000` (16 GB). [DTB,
  `v3dbus/dma-ranges = <0 0 0 4 0>`; las celdas son 1 de hijo, 2 de padre y 2 de tamaño]
- Se deduce que **la V3D alcanza toda la RAM de la Pi 4**, hasta 8 GB. Es distinto del HVS, que solo ve el primer GB
  y suma `0xC0000000` (ver `hvs.md`, 2.4).
- Linux fija su máscara de DMA en `30 + PA_WIDTH` bits, con `PA_WIDTH` leído de `V3D_MMU_DEBUG_INFO` (sección 3).
  [`v3d_drv.c`, `v3d_platform_drm_probe`]
- La entrada de la tabla de páginas tiene 24 bits de número de página. Con páginas de 4 KB eso da 64 GB. Linux se
  para con un error fatal si una página física no cabe. [`v3d_mmu.c`, `v3d_mmu_insert_ptes`]
- Los búferes de la GPU **no tienen que ser contiguos** en memoria física. La MMU los junta. [`v3d_bo.c`, comentario
  inicial]
- Si un búfer de la GPU lo tiene que mostrar el HVS, sí tiene que estar en el primer GB y cumplir lo que pida el HVS.
  En Linux, esos búferes los reserva el controlador de pantalla y la V3D los importa. [`v3d_bo.c`, comentario
  inicial y `v3d_prime_import_sg_table`]

### 2.7. Coherencia con las cachés de la CPU

- Linux mapea para la CPU toda la memoria de la GPU como **combinada en escritura, sin caché** (`map_wc`, `vmap` con
  `pgprot_writecombine`, `dma_alloc_wc`). Lo hace con los búferes, la tabla de páginas y la página de relleno.
  [`v3d_bo.c`, `v3d_create_object` y `v3d_get_bo_vaddr`; `v3d_gem.c`, `v3d_gem_init`; `v3d_drv.c`,
  `v3d_platform_drm_probe`]
- Se deduce que la V3D **no es coherente** con las cachés de la CPU. En DashOS, la memoria que lea o escriba la GPU
  tiene que estar mapeada sin caché (Normal no cacheable), o hay que limpiar e invalidar la caché de la CPU a mano.

## 3. Identificación

Registros de identificación [`v3d_regs.h`; `v3d_drv.c`, `v3d_platform_drm_probe` y `v3d_get_param_ioctl`;
`v3d_debugfs.c`, `v3d_v3d_debugfs_ident`]:

| Bloque | Desp. | Nombre | Campos |
|---|---|---|---|
| Hub | `0x004` | `HUB_UIFCFG` | Sin campos en Linux. Se pasa tal cual a Mesa. |
| Hub | `0x008` | `HUB_IDENT0` | Sin campos en Linux. |
| Hub | `0x00C` | `HUB_IDENT1` | 19 `WITH_MSO`; 18 `WITH_TSY`; 17 `WITH_TFU`; 16 `WITH_L3C`; 15:12 `NHOSTS`; 11:8 `NCORES`; 7:4 `REV`; 3:0 `TVER` |
| Hub | `0x010` | `HUB_IDENT2` | 8 `WITH_MMU`; 7:0 `L3C_NKB` (KB de caché L3) |
| Hub | `0x014` | `HUB_IDENT3` | 15:8 `IPREV`; 7:0 `IPIDX` |
| Hub | `0x1238` | `MMU_DEBUG_INFO` | 11:8 `PA_WIDTH`; 7:4 `VA_WIDTH`; 3:0 `VERSION` |
| Núcleo | `0x000` | `CTL_IDENT0` | 31:24 `VER` |
| Núcleo | `0x004` | `CTL_IDENT1` | 31:28 `VPM_SIZE` (en KB, dice el comentario); 23:16 `NSEM`; 15:12 `NTMU`; 11:8 `QUPS`; 7:4 `NSLC`; 3:0 `REV` |
| Núcleo | `0x008` | `CTL_IDENT2` | 28 `BCG_INT` |

Cómo lo interpreta Linux:

- **Versión**: `TVER × 10 + REV` de `HUB_IDENT1`. En la BCM2711 debe dar 42. Linux avisa si no coincide con lo que
  dice el árbol. [`v3d_drv.c`, `v3d_platform_drm_probe`]
- **Revisión**: `IPREV` de `HUB_IDENT3`. La versión completa se escribe `TVER.REV.IPREV.IPIDX`.
  [`v3d_debugfs.c`, `v3d_v3d_debugfs_ident`]
- **Núcleos**: `NCORES`. Debe ser 1.
- **Anchura de direcciones**: VA = `30 + VA_WIDTH` bits; PA = `30 + PA_WIDTH` bits. Se deduce que en la 4.2 `VA_WIDTH`
  vale 2 (VA de 32 bits), porque la tabla cubre 4 GB. No se ha leído en una placa.
- **Slices, QPU y TMU**: slices = `NSLC`; QPU totales = `NSLC × QUPS`; TMU totales = `NSLC × NTMU`.
  [`v3d_debugfs.c`, `v3d_v3d_debugfs_ident`]
- Se sabe por fuentes públicas que la V3D de la Pi 4 tiene 8 QPU en 2 slices. Linux no lo dice; hay que leerlo de
  `CTL_IDENT1`.
- Mesa lee estos siete registros tal cual: `HUB_UIFCFG`, `HUB_IDENT1`, `HUB_IDENT2`, `HUB_IDENT3`, `CTL_IDENT0`,
  `CTL_IDENT1` y `CTL_IDENT2`. Hay que poder dárselos. [`v3d_drv.c`, `v3d_get_param_ioctl`]

## 4. Mapa de registros

Solo están los que Linux usa o nombra para la 4.2. Todos son de 32 bits.

### 4.1. Hub (base `0xFEC00000`)

| Desp. | Nombre | Para qué | Uso en Linux (4.2) |
|---|---|---|---|
| `0x000` | `HUB_AXICFG` | 3:0 `MAX_LEN`: longitud máxima de ráfaga AXI | Solo en el reinicio por bridge, que la Pi 4 no usa |
| `0x004` a `0x014` | `HUB_UIFCFG`, `HUB_IDENT0..3` | Identificación | Lectura (sección 3) |
| `0x050` | `HUB_INT_STS` | Estado de interrupciones del hub | Sí |
| `0x054` | `HUB_INT_SET` | Forzar interrupciones (se deduce) | No |
| `0x058` | `HUB_INT_CLR` | Borrar: escribir 1 | Sí |
| `0x05C` | `HUB_INT_MSK_STS` | Estado de la máscara | Solo depuración |
| `0x060` | `HUB_INT_MSK_SET` | Enmascarar: escribir 1 | Sí |
| `0x064` | `HUB_INT_MSK_CLR` | Desenmascarar: escribir 1 | Sí |
| `0x400` | `TFU_CS` | Control y estado de la TFU | No (solo nombrado) |
| `0x404` | `TFU_SU` | Configuración de la TFU | No |
| `0x408` | `TFU_ICFG` | Configuración de entrada; **escribirlo lanza el trabajo** | Sí |
| `0x40C` | `TFU_IIA` | Dirección de la imagen de entrada | Sí |
| `0x410` | `TFU_ICA` | Dirección de croma de entrada | Sí |
| `0x414` | `TFU_IIS` | Paso de la imagen de entrada | Sí |
| `0x418` | `TFU_IUA` | Dirección del plano U de entrada | Sí |
| `0x41C` | `TFU_IOA` | Dirección de la imagen de salida | Sí |
| `0x420` | `TFU_IOS` | Tamaño de la imagen de salida | Sí |
| `0x424` a `0x430` | `TFU_COEF0..3` | Coeficientes YUV | Sí |
| `0x434` | `TFU_CRC` | CRC (solo 4.x) | No |
| `0x1000` | `MMUC_CONTROL` | Control de la caché de la MMU | Sí |
| `0x1200` | `MMU_CTL` | Control y estado de la MMU | Sí |
| `0x1204` | `MMU_PT_PA_BASE` | Base física de la tabla, en páginas de 4 KB | Sí |
| `0x1208` | `MMU_HIT` | Contador de aciertos de la TLB | No |
| `0x120C` | `MMU_MISSES` | Contador de fallos | No |
| `0x1210` | `MMU_STALLS` | Contador de esperas | No |
| `0x1214` | `MMU_ADDR_CAP` | Tope de direcciones: 31 `ENABLE`; 11:0 `MPAGE` | No |
| `0x1218` | `MMU_SHOOT_DOWN` | Invalidar una página: 29 `SHOOTING`; 28 `SHOOT`; 27:0 `PAGE` | No |
| `0x121C`, `0x1220` | `MMU_BYPASS_START`, `MMU_BYPASS_END` | Zona sin traducir | No |
| `0x122C` | `MMU_VIO_ID` | Identificador AXI del acceso que falló | Sí, al fallar |
| `0x1230` | `MMU_ILLEGAL_ADDR` | Página de relleno: 31 `ENABLE` | Sí |
| `0x1234` | `MMU_VIO_ADDR` | Dirección que falló | Sí, al fallar |
| `0x1238` | `MMU_DEBUG_INFO` | Anchuras de direcciones | Sí, al arrancar |

[`v3d_regs.h`; `v3d_debugfs.c`, `v3d_hub_reg_defs`]

### 4.2. Núcleo 0 (base `0xFEC04000`)

| Desp. | Nombre | Para qué | Uso en Linux (4.2) |
|---|---|---|---|
| `0x000` a `0x008` | `CTL_IDENT0..2` | Identificación | Lectura |
| `0x018` | `CTL_MISCCFG` | 0 `OVRTMUOUT`; 3:1 `QRMAXCNT` | Solo antes de la 4.1 |
| `0x020` | `CTL_L2CACTL` | Caché L2C | Solo antes de la 3.3 |
| `0x024` | `CTL_SLCACTL` | Invalidar cachés de los slices | Sí |
| `0x030` | `CTL_L2TCACTL` | Operaciones de la caché L2T | Sí |
| `0x034` | `CTL_L2TFLSTA` | Inicio del rango de la operación L2T | Sí, al iniciar |
| `0x038` | `CTL_L2TFLEND` | Fin del rango | Sí, al iniciar |
| `0x050` a `0x064` | `CTL_INT_STS`, `_SET`, `_CLR`, `_MSK_STS`, `_MSK_SET`, `_MSK_CLR` | Interrupciones del núcleo, igual que en el hub | Sí |
| `0x100`, `0x104` | `CLE_CT0CS`, `CLE_CT1CS` | Control y estado de los hilos 0 y 1 | Solo depuración |
| `0x108`, `0x10C` | `CLE_CT0EA`, `CLE_CT1EA` | Dirección de fin actual | Solo depuración |
| `0x110`, `0x114` | `CLE_CT0CA`, `CLE_CT1CA` | Dirección actual | Sí, para detectar cuelgues |
| `0x118`, `0x11C` | `CLE_CT0RA`, `CLE_CT1RA` | Dirección de retorno | Sí, para detectar cuelgues |
| `0x120` a `0x158` | `CT0LC`, `CT1LC`, `CT0PC`, `CT1PC`, `PCS`, `BFC`, `RFC`, `TFBC`, `TFIT`, `CT1CFG`, `CT1TILECT`, `CT1TSKIP`, `CT1PTCT`, `CT0SYNC`, `CT1SYNC` | Sin descripción en Linux | No |
| `0x15C` | `CLE_CT0QTS` | Estado de baldosas del binner: dirección + 1 `ENABLE` | Sí |
| `0x160` | `CLE_CT0QBA` | Inicio de la BCL | Sí |
| `0x164` | `CLE_CT1QBA` | Inicio de la RCL | Sí |
| `0x168` | `CLE_CT0QEA` | Fin de la BCL; **escribirlo lanza el binning** | Sí |
| `0x16C` | `CLE_CT1QEA` | Fin de la RCL; **escribirlo lanza el render** | Sí |
| `0x170` | `CLE_CT0QMA` | Memoria de baldosas del binner: dirección | Sí |
| `0x174` | `CLE_CT0QMS` | Memoria de baldosas del binner: tamaño | Sí |
| `0x178` | `CLE_CT1QCFG` | 7 `ETFILT`; 6 `ETPROC`; 1 `ETSFLUSH`; 0 `MCDIS` | No |
| `0x300` | `PTB_BPCA` | Dirección actual del depósito del binner | Solo depuración |
| `0x304` | `PTB_BPCS` | Tamaño actual del depósito | Solo depuración |
| `0x308` | `PTB_BPOA` | Memoria de desbordamiento: dirección | Sí |
| `0x30C` | `PTB_BPOS` | Memoria de desbordamiento: tamaño | Sí |
| `0x310` | `PTB_BXCF` | 1 `RWORDERDISA`; 0 `CLIPDISA` | No |
| `0x650` | `PCTR_0_EN` | Contadores activos (máscara) | Sí |
| `0x654` | `PCTR_0_CLR` | Poner contadores a 0 (máscara) | Sí |
| `0x658` | `PCTR_0_OVERFLOW` | Desbordamiento de contadores | Sí |
| `0x660` a `0x67C` | `PCTR_0_SRC_0_3` ... `SRC_28_31` | Qué cuenta cada contador | Sí |
| `0x680` a `0x6FC` | `PCTR_0_PCTR0..31` | Valores | Sí |
| `0x800` | `GMP_STATUS` | Estado del GMP | Sí, al suspender |
| `0x804` | `GMP_CFG` | Configuración del GMP | Sí, al suspender |
| `0x808` | `GMP_VIO_ADDR` | Dirección de la violación | Solo depuración |
| `0x80C` a `0x820` | `GMP_VIO_TYPE`, `GMP_TABLE_ADDR`, `GMP_CLEAR_LOAD`, `GMP_PRESERVE_LOAD`, `GMP_VALID_LINES` | GMP | No |
| `0x900` | `CSD_STATUS` | Estado del CSD | Solo depuración |
| `0x904` a `0x91C` | `CSD_QUEUED_CFG0..6` | Trabajo de cómputo en cola; **escribir `CFG0` lo lanza** | Sí |
| `0x920` a `0x938` | `CSD_CURRENT_CFG0..6` | Trabajo en curso (lectura) | `CFG4`, para detectar cuelgues |
| `0x93C`, `0x940` | `CSD_CURRENT_ID0`, `ID1` | Grupo en curso | No |
| `0xF04` a `0xF10` | `ERR_FDBGO`, `ERR_FDBGB`, `ERR_FDBGR`, `ERR_FDBGS` | Depuración del pipeline | Solo depuración |
| `0xF20` | `ERR_STAT` | Errores del núcleo | Se imprime al reiniciar |

[`v3d_regs.h`; `v3d_debugfs.c`, `v3d_core_reg_defs` y `v3d_csd_reg_defs`]

Los registros de la TFU, el CSD, el GMP y los contadores cambian de sitio en la 7.1 (sección 16). Linux usa las
macros con versión (`V3D_TFU_CS(ver)`...). Los desplazamientos de esta tabla son los de la 4.2.

## 5. La MMU

### 5.1. Modelo

- Un solo nivel de tabla. Cubre los 4 GB de VA con páginas de 4 KB. [`v3d_mmu.c`, comentario inicial]
- La tabla tiene 1 048 576 entradas de 4 bytes: **4 MB físicamente contiguos**. [`v3d_gem.c`, `v3d_gem_init`]
- La entrada número `n` traduce la VA `n × 4096`.
- Linux usa **una sola tabla para todos los procesos**. Cambiar de tabla es caro y 4 MB contiguos son escasos.
  Todos los búferes de todos los clientes viven en el mismo espacio de 4 GB. [`v3d_mmu.c`, comentario inicial]
- No hay aislamiento entre clientes. El comentario dice que habría que usar el GMP para eso, con granularidad de 128
  KB, y que no está hecho. [`v3d_mmu.c`, comentario inicial; `v3d_drv.c`, comentario sobre `DRM_AUTH`]
- La tabla se reserva a cero, sin caché para la CPU. [`v3d_gem.c`, `v3d_gem_init`]

### 5.2. Formato de la entrada

| Bits | Nombre | Significado |
|---|---|---|
| 31 | `SUPERPAGE` | La entrada forma parte de una superpágina de 1 MB |
| 30 | `BIGPAGE` | La entrada forma parte de una página grande de 64 KB |
| 29 | `WRITEABLE` | Se permite escribir |
| 28 | `VALID` | Entrada válida |
| 27:24 | — | Linux los deja a 0 |
| 23:0 | Número de página física | PA desplazada 12 bits a la derecha |

[`v3d_mmu.c`, macros `V3D_PTE_*` y `v3d_mmu_insert_ptes`]

- Linux pone siempre `WRITEABLE` y `VALID` juntos. Nunca crea entradas de solo lectura.
- Una entrada a 0 es una página no válida. Linux escribe 0 al quitar un búfer. [`v3d_mmu.c`, `v3d_mmu_remove_ptes`]
- **Páginas grandes**: no son un formato aparte. Son 16 entradas seguidas (64 KB) o 256 entradas seguidas (1 MB),
  todas con el bit `BIGPAGE` o `SUPERPAGE`. Cada entrada lleva su propio número de página física, que sube de uno en
  uno. El comentario dice que **todas** las entradas del bloque tienen que llevar el bit. [`v3d_mmu.c`, comentario
  de `V3D_PTE_SUPERPAGE` y `v3d_mmu_insert_ptes`]
- Condiciones para usar una página grande: el trozo físico es contiguo y mide al menos ese tamaño; la VA y la PA
  están alineadas a ese tamaño. Si no, se usan entradas de 4 KB. [`v3d_mmu.c`, `v3d_mmu_is_aligned` y
  `v3d_mmu_insert_ptes`]
- Se deduce que las páginas grandes solo mejoran el rendimiento de la TLB. Con entradas de 4 KB todo funciona igual.
  Linux solo las usa si el sistema tiene páginas enormes transparentes. [`v3d_bo.c`, `v3d_bo_create_finish`;
  `v3d_gem.c`, `v3d_huge_mnt_init`]

### 5.3. Registros

**`MMU_CTL` (hub `0x1200`)** [`v3d_regs.h`]

| Bits | Nombre | Significado |
|---|---|---|
| 27 | `CAP_EXCEEDED` | Estado: acceso por encima del tope |
| 26 | `CAP_EXCEEDED_ABORT` | Abortar el acceso en ese caso |
| 25 | `CAP_EXCEEDED_INT` | Interrumpir en ese caso |
| 24 | `CAP_EXCEEDED_EXCEPTION` | Sin descripción |
| 20 | `PT_INVALID` | Estado: acceso a una entrada no válida |
| 19 | `PT_INVALID_ABORT` | Abortar el acceso |
| 18 | `PT_INVALID_INT` | Interrumpir |
| 17 | `PT_INVALID_EXCEPTION` | Sin descripción |
| 16 | `PT_INVALID_ENABLE` | Comprobar la validez de las entradas |
| 12 | `WRITE_VIOLATION` | Estado: escritura en una página sin `WRITEABLE` |
| 11 | `WRITE_VIOLATION_ABORT` | Abortar |
| 10 | `WRITE_VIOLATION_INT` | Interrumpir |
| 9 | `WRITE_VIOLATION_EXCEPTION` | Sin descripción |
| 7 | `TLB_CLEARING` | Lectura: el borrado de la TLB está en marcha |
| 3 | `TLB_STATS_CLEAR` | Poner a 0 los contadores de la TLB |
| 2 | `TLB_CLEAR` | Borrar la TLB |
| 1 | `TLB_STATS_ENABLE` | Activar los contadores |
| 0 | `ENABLE` | Activar la MMU |

**`MMUC_CONTROL` (hub `0x1000`)**: la caché de la MMU (se deduce que guarda entradas de la tabla).

| Bits | Nombre | Significado |
|---|---|---|
| 3 | `CLEAR` | Sin uso en Linux |
| 2 | `FLUSHING` | Lectura: el vaciado está en marcha |
| 1 | `FLUSH` | Vaciar |
| 0 | `ENABLE` | Activar |

**`MMU_PT_PA_BASE` (hub `0x1204`)**: la PA de la tabla desplazada 12 bits a la derecha. La tabla tiene que estar
alineada a 4 KB. [`v3d_mmu.c`, `v3d_mmu_set_page_table`]

**`MMU_ILLEGAL_ADDR` (hub `0x1230`)**: PA de la página de relleno desplazada 12 bits, más el bit 31 `ENABLE`.

### 5.4. Dar la tabla a la MMU

[`v3d_mmu.c`, `v3d_mmu_set_page_table`]

1. `MMU_PT_PA_BASE` = PA de la tabla >> 12.
2. `MMU_CTL` = `ENABLE` | `PT_INVALID_ENABLE` | `PT_INVALID_ABORT` | `PT_INVALID_INT` | `WRITE_VIOLATION_ABORT` |
   `WRITE_VIOLATION_INT` | `CAP_EXCEEDED_ABORT` | `CAP_EXCEEDED_INT`. Valor: `0x060D0C01`. Los bits
   `*_EXCEPTION` quedan a 0.
3. `MMU_ILLEGAL_ADDR` = (PA de la página de relleno >> 12) | bit 31.
4. `MMUC_CONTROL` = `ENABLE` (valor 1).
5. Vaciar la caché de la MMU y la TLB (5.6).

Linux hace esto al encender la GPU y después de cada reinicio. [`v3d_power.c`, `v3d_power_resume`; `v3d_gem.c`,
`v3d_reset`]

### 5.5. Página de relleno

- Es una página de 4 KB a cero, sin caché para la CPU. [`v3d_drv.c`, `v3d_platform_drm_probe`]
- El comentario dice: "cuando una lectura o escritura no es válida en la MMU, se redirige aquí". [`v3d_drv.h`,
  campo `mmu_scratch`]
- Linux activa a la vez el aborto, la interrupción y esta página. No se sabe qué manda si están las tres cosas.
  Se deduce que la página evita que un acceso malo llegue a la dirección física 0.

### 5.6. Vaciar la caché de la MMU y la TLB

[`v3d_mmu.c`, `v3d_mmu_flush_all_locked`]

1. `MMUC_CONTROL` = `FLUSH` | `ENABLE` (valor 3).
2. Esperar a que `MMUC_CONTROL.FLUSHING` sea 0. Plazo de 100 ms.
3. `MMU_CTL`: leer, poner `TLB_CLEAR` y escribir.
4. Esperar a que `MMU_CTL.TLB_CLEARING` sea 0. Plazo de 100 ms.

Cuándo lo hace Linux:

- Después de escribir las entradas de un búfer nuevo. [`v3d_mmu.c`, `v3d_mmu_insert_ptes`]
- Después de borrar las de un búfer liberado. [`v3d_mmu.c`, `v3d_mmu_remove_ptes`]
- Antes de dar memoria de desbordamiento al binner (7.3). [`v3d_irq.c`, `v3d_overflow_mem_work`]
- Si la GPU está apagada, no lo hace: al encenderla se vacía todo igualmente. [`v3d_mmu.c`, `v3d_mmu_flush_all`]

Linux no usa `MMU_SHOOT_DOWN` para invalidar una sola página. Siempre vacía todo.

### 5.7. Errores de la MMU

- Llegan como interrupción del hub: bits `MMU_WRV` (escritura prohibida), `MMU_PTI` (entrada no válida) y `MMU_CAP`
  (tope superado). Ver la sección 9.
- Al tratarlos, Linux [`v3d_irq.c`, `v3d_hub_irq`]:
  1. Lee `MMU_VIO_ID` y `MMU_VIO_ADDR`.
  2. Escribe en `MMU_CTL` el mismo valor que lee. Se deduce que los bits de estado 27, 20 y 12 se borran escribiendo
     un 1.
  3. Calcula la VA que falló: `MMU_VIO_ADDR` desplazado a la izquierda `VA_WIDTH_total − 32` bits. Con VA de 32 bits
     no hay desplazamiento.
  4. Solo lo apunta en el registro de depuración. **No para el trabajo ni reinicia.** Si el trabajo se queda
     colgado, lo detecta el plazo (sección 10).
- Los 8 bits bajos de `MMU_VIO_ID` dicen qué unidad hizo el acceso (tabla de la 4.1 y la 4.2) [`v3d_irq.c`,
  `v3d41_axi_ids`]:

| `VIO_ID & 0xFF` | Unidad |
|---|---|
| `0x00` a `0x1F` | L2T |
| `0x20` | PTB |
| `0x40` | PSE |
| `0x60` a `0x7F` | TLB |
| `0x80` a `0x87` | CLE |
| `0xA0` | TFU |
| `0xC0` a `0xDF` | MMU |
| `0xE0` | GMP |

### 5.8. GMP

- Linux no configura el GMP en la 4.2. El comentario dice que está en modo "siempre permitido" y que no debería dar
  violaciones. [`v3d_irq.c`, `v3d_irq`]
- Si llega la interrupción `GMPV`, solo la apunta.
- Solo se usa para parar los accesos a memoria antes de apagar (sección 11).

`GMP_STATUS` (núcleo `0x800`): 31 `GMPRST`; 30:24 `WR_COUNT`; 22:16 `RD_COUNT`; 5 `WR_ACTIVE`; 4 `RD_ACTIVE`; 3
`CFG_BUSY`; 2 `CNTOVF`; 1 `INVPROT`; 0 `VIO`.

`GMP_CFG` (núcleo `0x804`): 3 `LBURSTEN`; 1 `STOP_REQ`; 0 `PROT_ENABLE`. [`v3d_regs.h`]

## 6. Espacio de direcciones y búferes

- Linux reparte la VA en páginas de 4 KB. **La página 0 nunca se da.** El comentario dice que varias unidades
  tratan la dirección 0 como especial; por ejemplo, en los contadores de oclusión 0 significa "desactivado".
  [`v3d_gem.c`, `v3d_gem_init`; `v3d_drm.h`, `drm_v3d_create_bo`]
- El tamaño de un búfer se redondea a 4 KB. Un tamaño 0 es un error. [`v3d_bo.c`, `v3d_create_bo_ioctl` y
  `v3d_create_object`]
- Alineación de la VA: 4 KB. Con páginas enormes activas: 1 MB si el búfer mide 1 MB o más, 64 KB si mide 64 KB o
  más. [`v3d_bo.c`, `v3d_bo_create_finish`]
- Un búfer tiene **la misma VA toda su vida**. Se mapea en la MMU al crearlo y se quita al liberarlo.
  [`v3d_bo.c`, `v3d_bo_create_finish` y `v3d_free_object`]
- La VA es la que Mesa escribe en las listas de control, en los registros de la TFU y del CSD, y en los
  sombreadores. El kernel **no valida** nada de eso: el comentario dice que no hace falta porque todo pasa por la
  MMU. [`v3d_submit.c`, comentarios de `v3d_submit_tfu_ioctl` y `v3d_submit_csd_ioctl`]

## 7. Trabajos

### 7.1. Modelo general

Linux tiene seis colas. Cinco son de la GPU y una es de la CPU. [`v3d_drm.h`, `enum v3d_queue`; `v3d_sched.c`]

| Cola | Unidad | Se lanza con | Termina con |
|---|---|---|---|
| `BIN` | Binner (CLE hilo 0) | `CLE_CT0QEA` | Interrupción del núcleo `FLDONE` |
| `RENDER` | Render (CLE hilo 1) | `CLE_CT1QEA` | Interrupción del núcleo `FRDONE` |
| `TFU` | TFU | `TFU_ICFG` | Interrupción del hub `TFUC` |
| `CSD` | CSD | `CSD_QUEUED_CFG0` | Interrupción del núcleo `CSDDONE` |
| `CACHE_CLEAN` | Cachés L2T y TMU | Escrituras en `L2TCACTL` | Síncrono: la CPU espera |
| `CPU` | Ninguna | — | Lo hace la CPU (7.8) |

- **Un trabajo a la vez por cola.** Linux no llena las colas internas del hardware (las de CT0 y CT1). El comentario
  lo explica por latencia. [`v3d_sched.c`, comentario inicial; `v3d_queue_sched_init`, límite de 1]
- **Las colas sí corren a la vez entre ellas.** Puede haber a la vez un binning, un render, una TFU y un cómputo.
- Cuando llega la interrupción de fin, Linux da por terminado el trabajo activo de esa cola. No comprueba nada más.
  [`v3d_irq.c`, `v3d_irq_signal_fence`]
- **Dependencias**: el kernel las impone con vallas, no con hardware. El render espera al binning del mismo envío.
  El comentario dice que el hardware tiene semáforos para eso, pero Linux no los usa. [`v3d_sched.c`, comentario
  inicial; `v3d_submit.c`, `v3d_submit_jobs`]
- **Cadenas por envío**: cada trabajo de un envío espera al anterior del mismo envío. [`v3d_submit.c`,
  `v3d_submit_jobs`]
  - Envío de listas: binning (opcional) → render → limpieza de cachés (opcional, con la bandera `FLUSH_CACHE`).
  - Envío de cómputo: cómputo → limpieza de cachés (siempre).
  - Envío de TFU: solo la TFU.
- **Orden por cliente**: los trabajos de un mismo cliente en una misma cola salen en orden. Entre clientes se reparte
  por orden de llegada. [`v3d_sched.c`, comentario inicial]
- **Antes** de lanzar binning, render y cómputo, Linux invalida las cachés (sección 8.2). Antes de la TFU, no.

### 7.2. Binning

[`v3d_sched.c`, `v3d_bin_job_run`]

Lo que da el cliente: inicio y fin de la BCL (`bcl_start`, `bcl_end`), memoria de baldosas (`qma`, `qms`) y estado de
baldosas (`qts`). Todo son VA. Si `bcl_start` es igual a `bcl_end`, no hay binning: solo render. [`v3d_drm.h`,
`drm_v3d_submit_cl`; `v3d_submit.c`, `v3d_submit_cl_ioctl`]

Secuencia:

1. Marcar el trabajo como activo en la cola `BIN`.
2. `PTB_BPOS` = 0. Así no se reutiliza memoria de desbordamiento de un binning anterior. Linux lo hace bajo el mismo
   cerrojo que el tratamiento del desbordamiento (7.3).
3. Invalidar las cachés (8.2).
4. Arrancar los contadores de rendimiento, si el trabajo lleva (sección 12).
5. Si `qma` no es 0: `CLE_CT0QMA` = `qma` y `CLE_CT0QMS` = `qms`.
6. Si `qts` no es 0: `CLE_CT0QTS` = `qts` | `CT0QTS_ENABLE` (bit 1).
7. `CLE_CT0QBA` = `bcl_start`.
8. `CLE_CT0QEA` = `bcl_end`. **Esta escritura lanza el binning.** El comentario lo dice así.
   `bcl_end` es el primer byte después de la BCL.

Fin: interrupción `FLDONE` del núcleo.

- El comentario de la cabecera dice que `qma` es opcional en la 3.3 pero obligatorio desde la 4.1. [`v3d_drm.h`,
  campo `qma`]
- Se deduce que `qts` tiene que tener libre el bit 1 (y seguramente estar alineado), porque Linux le suma el bit de
  activar con una OR.

### 7.3. Memoria del binner y desbordamiento

- El binner escribe las listas por baldosa en la memoria de baldosas (`qma`/`qms`). La reserva el cliente.
- Si se le acaba, pide más con la interrupción `OUTOMEM`. Linux le da más así [`v3d_irq.c`, `v3d_irq` y
  `v3d_overflow_mem_work`]:
  1. En la interrupción, borra el bit y aplaza el trabajo a un hilo, porque reservar memoria puede dormir.
  2. Reserva un búfer de **256 KB** y lo mapea en la MMU.
  3. Si ya no hay binning activo, lo suelta y termina. Es una carrera conocida: el hardware avisa **antes** de
     quedarse sin memoria del todo, y el binning puede terminar justo.
  4. Si hay binning activo, ata el búfer al **render** de ese mismo envío. Se libera cuando termine el render, porque
     el render lee lo que el binner escribió ahí.
  5. Vacía la caché de la MMU y la TLB (5.6).
  6. `PTB_BPOA` = VA del búfer. `PTB_BPOS` = tamaño en bytes (262 144).
- `OUTOMEM` se señala **por flanco**. No se repite hasta que se da más memoria. [`v3d_irq.c`, comentario en `v3d_irq`]
- Desde la 4.1, `FLDONE` **no llega** mientras haya un estado de falta de memoria sin resolver. El binning se queda
  parado esperando. [`v3d_irq.c`, comentario en `v3d_irq`]
- Si el binning se cuelga esperando memoria, lo detecta el plazo (sección 10).
- Si la reserva falla, Linux solo lo apunta. El binning se quedará parado.

### 7.4. Render

[`v3d_sched.c`, `v3d_render_job_run`]

1. Marcar el trabajo como activo en la cola `RENDER`.
2. Invalidar las cachés (8.2). El comentario explica por qué hace falta aquí además de en el binning: si el orden
   es binning 0, binning 1, render 0, render 1, la invalidación del binning 1 llega antes de que el render 0 escriba
   la textura que lee el render 1.
3. Arrancar los contadores, si lleva.
4. `CLE_CT1QBA` = `rcl_start`.
5. `CLE_CT1QEA` = `rcl_end`. **Esta escritura lanza el render.**

Fin: interrupción `FRDONE` del núcleo.

- Linux no escribe `CLE_CT1QCFG`. Hay un comentario pendiente ("XXX: Set the QCFG").
- La cabecera dice que al llegar `FRDONE` las escrituras de la memoria de baldosa (TLB) ya están en la RAM. Las
  escrituras de los sombreadores por la TMU **no**: para eso está la limpieza de cachés. [`v3d_drm.h`, comentario de
  `drm_v3d_submit_cl`]
- Un render sin binning sirve para copias y borrados que recorren un conjunto fijo de baldosas. [`v3d_drm.h`, campo
  `rcl_start`]

### 7.5. TFU

[`v3d_sched.c`, `v3d_tfu_job_run`; `v3d_drm.h`, `drm_v3d_submit_tfu`]

El cliente da los valores de los registros ya calculados: `icfg`, `iia`, `iis`, `ica`, `iua`, `ioa`, `ios` y
`coef[0..3]`. También hasta 4 búferes: el primero es el de salida.

1. Marcar el trabajo como activo en la cola `TFU`.
2. Escribir en este orden: `TFU_IIA`, `TFU_IIS`, `TFU_ICA`, `TFU_IUA`, `TFU_IOA`, `TFU_IOS`, `TFU_COEF0`.
3. Si `coef[0]` tiene el bit 31 (`USECOEF`): escribir también `TFU_COEF1`, `TFU_COEF2` y `TFU_COEF3`. Si no, la TFU
   usa sus coeficientes por defecto.
4. `TFU_ICFG` = `icfg` | `ICFG_IOC` (bit 0, "interrumpir al terminar"). **Esta escritura lanza la conversión.**

Fin: interrupción `TFUC` del hub.

- Los registros de la TFU están en el **hub**, no en el núcleo.
- Linux no invalida cachés antes de la TFU, y la TFU no lleva contadores de rendimiento.
- El comentario dice que la TFU no pasa por el GMP. [`v3d_drv.c`, comentario sobre `DRM_AUTH`]
- El significado de los campos de `ICFG`, `IIS`, `IOS` y demás no está en el kernel. Lo calcula Mesa.
- `TFU_CS` (`0x400`): 31 `TFURST` ("para el trabajo actual y vacía la cola de entrada"); 23:16 `CVTCT` (contador de
  conversiones); 13:8 `NFREE` (huecos libres en la cola); 0 `BUSY`. Linux no lo usa.
- `TFU_SU` (`0x404`): 13:8 `FINTTHR` (interrumpir cuando haya tantos huecos libres; 0 = no); 4 `CRCCHAIN`; 3 `CRC`
  (no escribe, solo calcula el CRC de la imagen); 1:0 `THROTTLE`. Linux no lo usa.
- Se deduce que el bit `TFUF` del hub (bit 0) es la interrupción de `FINTTHR`. Linux no la activa.

### 7.6. CSD (cómputo)

[`v3d_sched.c`, `v3d_csd_job_run`; `v3d_regs.h`; `v3d_drm.h`, `drm_v3d_submit_csd`]

El cliente da `cfg[0..6]`, los valores de `CSD_QUEUED_CFG0..6`.

| Registro | Desp. | Campos |
|---|---|---|
| `CFG0` | `0x904` | 31:16 `NUM_WGS_X`; 15:0 `WG_X_OFFSET` |
| `CFG1` | `0x908` | 31:16 `NUM_WGS_Y`; 15:0 `WG_Y_OFFSET` |
| `CFG2` | `0x90C` | 31:16 `NUM_WGS_Z`; 15:0 `WG_Z_OFFSET` |
| `CFG3` | `0x910` | 26 `OVERLAP_WITH_PREV`; 25:20 `MAX_SG_ID`; 19:12 `BATCHES_PER_SG_M1`; 11:8 `WGS_PER_SG`; 7:0 `WG_SIZE` |
| `CFG4` | `0x914` | Número de lotes menos 1 |
| `CFG5` | `0x918` | Dirección del sombreador con bits de modo, "como en un registro de sombreador" |
| `CFG6` | `0x91C` | Dirección de los uniformes, alineada a 4 bytes |

Secuencia:

1. Si alguno de `NUM_WGS_X`, `NUM_WGS_Y` o `NUM_WGS_Z` es 0, **no se lanza nada** y el trabajo se da por terminado.
   El comentario dice que el hardware entiende 0 como 65 536.
2. Marcar el trabajo como activo en la cola `CSD`.
3. Invalidar las cachés (8.2).
4. Arrancar los contadores, si lleva.
5. Escribir `CFG1` a `CFG6`, en ese orden.
6. Escribir `CFG0`. **Esta escritura lanza el cómputo.**

Fin: interrupción `CSDDONE` del núcleo (bit 7 en la 4.2).

- Linux añade siempre una limpieza de cachés después del cómputo (7.7). [`v3d_submit.c`,
  `v3d_setup_csd_jobs_and_bos`]
- Número de lotes, según el cálculo que hace Linux en el cómputo indirecto: `ceil(WG_SIZE / 16) × X × Y × Z`. En
  la 4.2, `CFG4` lleva ese número **menos 1**. [`v3d_sched.c`, `v3d_rewrite_csd_job_wg_counts_from_indirect`]
- `CSD_STATUS` (`0x900`): 11:4 `NUM_COMPLETED`; 3:2 `NUM_ACTIVE`; 1 `HAVE_CURRENT_DISPATCH`; 0
  `HAVE_QUEUED_DISPATCH`. Linux solo lo vuelca para depurar.
- Se deduce de los nombres que el CSD tiene un trabajo "en curso" y otro "en cola". Linux solo usa uno a la vez.
- `CSD_CURRENT_CFG4` (`0x930`) baja a medida que avanza: Linux lo usa para ver si hay progreso (sección 10).
- El campo `coef[4]` de la estructura del envío no lo usa el kernel.

### 7.7. Limpieza de cachés

[`v3d_gem.c`, `v3d_clean_caches`; `v3d_sched.c`, `v3d_cache_clean_job_run`]

Es síncrona: la CPU escribe y espera. Escribe en la memoria las líneas sucias de la L2T y de la TMU.

1. Esperar a que `L2TCACTL.L2TFLS` sea 0. Es el arreglo de la errata **GFXH-1897**: no escribir en `L2TCACTL`
   mientras haya una operación pendiente. Plazo de 100 ms.
2. `L2TCACTL` = `TMUWCF` (bit 8; valor `0x100`). Se deduce: vaciar el combinador de escrituras de la TMU.
3. Esperar a que `TMUWCF` sea 0. Plazo de 100 ms.
4. `L2TCACTL` = `L2TFLS` | modo `CLEAN` (valor `0x5`).
5. Esperar a que `L2TFLS` sea 0. Plazo de 100 ms.

Si vence un plazo, Linux solo lo apunta y sigue. Los pasos 4 y 5 los hace con un cerrojo que impide invalidaciones de
la L2T a la vez (8.2), para que la espera no se confunda.

### 7.8. Trabajos de la CPU

No tocan la GPU, pero Mesa (v3dv) los usa. Los ejecuta el kernel cuando se cumplen sus dependencias. [`v3d_sched.c`,
`cpu_job_function`; `v3d_drm.h`]

| Tipo | Qué hace |
|---|---|
| Cómputo indirecto | Lee tres contadores de grupos (X, Y, Z, de 32 bits) de un búfer. Rellena `CFG0..2` y `CFG4` del cómputo siguiente. Si se piden, escribe esos contadores también en el búfer de uniformes, en los índices dados (`0xFFFFFFFF` = no escribir). |
| Marca de tiempo | Escribe en un búfer un valor de 64 bits con el reloj del sistema en ns (el primero) y 0 en los demás. |
| Borrar marcas de tiempo | Escribe 0 de 64 bits. |
| Copiar marcas de tiempo | Copia valores a otro búfer, de 32 o 64 bits, con un bit opcional de "disponible". |
| Borrar y copiar consultas de rendimiento | Igual, con los valores de los contadores (sección 12). |

Todo lo que estos trabajos leen o escriben en búferes de la GPU está en **little-endian** (sección 13).

## 8. Cachés

### 8.1. Cuáles hay

| Caché | Qué guarda | Cómo se trata en la 4.2 | Fuente |
|---|---|---|---|
| Slices: `ICC` | Instrucciones de los QPU (se deduce) | Invalidar antes de cada trabajo | [`v3d_gem.c`, `v3d_invalidate_slices`] |
| Slices: `UCC` | Uniformes (se deduce) | Ídem | Ídem |
| Slices: `TDCCS`, `TVCCS` | Cachés de texturas de los slices (se deduce) | Ídem | Ídem |
| L1T | Caché de nivel 1 de las TMU | La cabecera dice que se vacía antes de cada lista. Se deduce que va con los slices o con la L2T. | [`v3d_drm.h`] |
| L2T | Caché de nivel 2 de las TMU | Invalidar antes de cada trabajo; limpiar tras escrituras de la TMU | [`v3d_gem.c`] |
| Combinador de escrituras de la TMU | Escrituras de los sombreadores | Vaciar en la limpieza (`TMUWCF`) | [`v3d_gem.c`, `v3d_clean_caches`] |
| VCD | Lectura de vértices | **No la toca el kernel.** La tienen que vaciar las propias listas. | [`v3d_drm.h`] |
| Caché de la MMU y TLB | Entradas de la tabla | 5.6 | [`v3d_mmu.c`] |
| L2C | Uniformes e instrucciones en la 3.2 | No existe en la 4.2 | [`v3d_gem.c`, `v3d_invalidate_l2c`] |
| GCA / L3 | Caché del hub antes de la 4.1 | No existe en la 4.2 | [`v3d_gem.c`, `v3d_flush_l3`] |

### 8.2. Invalidar antes de un trabajo

[`v3d_gem.c`, `v3d_invalidate_caches`]

El comentario dice que se invalida "de fuera hacia dentro". Así, si otra lista trae de vuelta una línea a una caché
externa, no queda un dato viejo en una interna.

1. GCA / L3: nada en la 4.2.
2. L2C: nada en la 4.2.
3. **L2T**: `L2TCACTL` = `L2TFLS` | modo `FLUSH` (valor `0x1`). **No se espera.** El comentario dice que el hardware
   retiene los accesos a la L2T hasta que termina. Se hace con el cerrojo de la limpieza (7.7).
4. **Slices**: `SLCACTL` = 0xF en los cuatro campos (valor `0x0F0F0F0F`).

`CTL_L2TCACTL` (núcleo `0x030`) [`v3d_regs.h`]:

| Bits | Nombre | Significado |
|---|---|---|
| 8 | `TMUWCF` | Vaciar el combinador de escrituras de la TMU; se lee 1 mientras dura |
| 2:1 | `FLM` | Modo: 0 `FLUSH` "invalida líneas"; 1 `CLEAR` "quita líneas sin escribir las sucias"; 2 `CLEAN` "escribe las sucias y las marca limpias, sin invalidar" |
| 0 | `L2TFLS` | Lanzar la operación; se lee 1 mientras dura |

La operación cubre el rango de `L2TFLSTA` a `L2TFLEND`.

`CTL_SLCACTL` (núcleo `0x024`): 27:24 `TVCCS`; 19:16 `TDCCS`; 11:8 `UCC`; 3:0 `ICC`. Se deduce que cada campo tiene un
bit por slice.

### 8.3. Estado fijo del núcleo

[`v3d_gem.c`, `v3d_init_core`]

- `L2TFLSTA` = 0 y `L2TFLEND` = `0xFFFFFFFF`. El comentario dice que siempre se quiere operar sobre la L2T entera.
- `MISCCFG.OVRTMUOUT` solo se pone antes de la 4.1. En la 4.2 no se escribe `MISCCFG`.
- Linux repite esto al encender y tras cada reinicio.

### 8.4. Orden entre trabajos

- Binning, render y cómputo empiezan con todas las cachés de lectura invalidadas (8.2).
- Al terminar un render, sus escrituras por la memoria de baldosa ya están en la RAM (7.4).
- Las escrituras de los sombreadores por la TMU (cómputo, o render con almacenamiento en búferes) se quedan en la L2T
  hasta una limpieza (7.7). Por eso el cómputo lleva siempre limpieza, y el render la lleva si el cliente la pide.
- La TFU no invalida nada al empezar. Se deduce que la TFU lee de la RAM sin pasar por la L2T. Para leer algo
  escrito por los sombreadores, antes tiene que haber una limpieza.
- Linux vigila con un cerrojo que una invalidación de la L2T no se cruce con una limpieza en marcha.

## 9. Interrupciones

### 9.1. Registros

Hub y núcleo tienen el mismo juego de seis registros (hub en `0x050`, núcleo en `0x050` de su bloque):

| Desp. | Nombre | Uso |
|---|---|---|
| `+0x00` | `INT_STS` | Lectura: eventos pendientes |
| `+0x04` | `INT_SET` | Forzar eventos (se deduce; Linux no lo usa) |
| `+0x08` | `INT_CLR` | Escribir 1 borra el evento |
| `+0x0C` | `INT_MSK_STS` | Lectura: bits enmascarados |
| `+0x10` | `INT_MSK_SET` | Escribir 1 enmascara (desactiva) |
| `+0x14` | `INT_MSK_CLR` | Escribir 1 desenmascara (activa) |

[`v3d_regs.h`; `v3d_irq.c`, `v3d_irq_enable` y `v3d_irq_disable`]

### 9.2. Bits del núcleo (4.2)

| Bit | Nombre | Significado | Linux lo activa |
|---|---|---|---|
| 27:16 | `QPU` | Una interrupción por QPU (se deduce) | No |
| 7 | `CSDDONE` | Fin del cómputo | Sí |
| 6 | `PCTR` | Contadores de rendimiento (se deduce) | No |
| 5 | `GMPV` | Violación del GMP | Sí, solo para apuntarla |
| 4 | `TRFB` | Sin descripción | No |
| 3 | `SPILLUSE` | Sin descripción | No |
| 2 | `OUTOMEM` | El binner necesita memoria (7.3) | Sí |
| 1 | `FLDONE` | Fin del binning | Sí |
| 0 | `FRDONE` | Fin del render | Sí |

Máscara que usa Linux: `0x000000A7`. [`v3d_irq.c`, `V3D_CORE_IRQS`]

### 9.3. Bits del hub (4.2)

| Bit | Nombre | Significado | Linux lo activa |
|---|---|---|---|
| 5 | `MMU_WRV` | Escritura en página protegida | Sí |
| 4 | `MMU_PTI` | Entrada de tabla no válida | Sí |
| 3 | `MMU_CAP` | Tope de direcciones superado | Sí |
| 2 | `MSO` | Sin descripción | No |
| 1 | `TFUC` | Fin de la TFU | Sí |
| 0 | `TFUF` | Sin descripción en Linux (ver 7.5) | No |

Máscara que usa Linux: `0x0000003A`. [`v3d_irq.c`, `V3D_HUB_IRQS`]

### 9.4. Activar y desactivar

**Activar** [`v3d_irq.c`, `v3d_irq_enable`]:

1. Núcleo: `INT_CLR` = máscara del núcleo. Borra lo que haya quedado pendiente.
2. Hub: `INT_CLR` = máscara del hub.
3. Núcleo: `INT_MSK_SET` = complemento de la máscara (enmascara todo lo demás). Después `INT_MSK_CLR` = máscara.
4. Hub: igual, con su máscara.

**Desactivar** [`v3d_irq.c`, `v3d_irq_disable`]:

1. Núcleo y hub: `INT_MSK_SET` = `0xFFFFFFFF`.
2. Esperar a que termine cualquier tratamiento de interrupción en curso.
3. Núcleo y hub: `INT_CLR` = sus máscaras.
4. Cancelar el tratamiento pendiente del desbordamiento (7.3) y esperar a que acabe.

### 9.5. Tratamiento

[`v3d_irq.c`, `v3d_irq` y `v3d_hub_irq`]

Con una sola línea, Linux hace esto en cada interrupción:

1. Leer `INT_STS` del núcleo y escribir ese mismo valor en `INT_CLR` del núcleo.
2. `OUTOMEM`: lanzar el tratamiento del desbordamiento (7.3).
3. `FLDONE`: terminar el trabajo activo de `BIN`.
4. `FRDONE`: terminar el trabajo activo de `RENDER`.
5. `CSDDONE`: terminar el trabajo activo de `CSD`.
6. `GMPV`: apuntar el error.
7. **Solo si no había nada del núcleo**, mirar el hub:
   1. Leer `INT_STS` del hub y escribir ese valor en `INT_CLR` del hub.
   2. `TFUC`: terminar el trabajo activo de `TFU`.
   3. Errores de la MMU: 5.7.

Al terminar un trabajo, Linux para y lee sus contadores de rendimiento si los llevaba, y marca la cola como libre.

Notas para DashOS (deducidas):

- Como la línea es por nivel, si había eventos del núcleo y del hub a la vez, el hub se atiende en la siguiente
  entrada de la interrupción. Es más sencillo mirar siempre los dos bloques en cada entrada.
- Linux supone que siempre hay un trabajo activo cuando llega un fin. No se protege contra un fin sin trabajo.
  Conviene comprobarlo.

## 10. Cuelgues y reinicio

### 10.1. Cómo detecta Linux un cuelgue

- Cada cola de la GPU tiene un plazo de **500 ms** por trabajo. [`v3d_sched.c`, `v3d_queue_sched_init`]
- Al vencer, Linux mira si hubo progreso [`v3d_sched.c`, `v3d_cl_job_timedout`, `v3d_csd_job_timedout` y
  `v3d_tfu_job_timedout`]:

| Cola | Qué mira | Si cambió |
|---|---|---|
| `BIN` | `CLE_CT0CA` y `CLE_CT0RA` | Guarda los valores y da otro plazo |
| `RENDER` | `CLE_CT1CA` y `CLE_CT1RA` | Ídem |
| `CSD` | `CSD_CURRENT_CFG4` (lotes que quedan) | Ídem |
| `TFU` | Nada | Reinicia siempre |
| `CACHE_CLEAN`, `CPU` | No tienen plazo | — |

- Los valores guardados empiezan a 0. Se deduce que en la práctica el primer plazo casi nunca reinicia, y un trabajo
  parado se detecta tras **1 s**.
- El comentario reconoce que una lista con un bucle infinito no se detectaría así. Lo da por improbable.

### 10.2. Reinicio

[`v3d_sched.c`, `v3d_gpu_reset_for_timeout`; `v3d_gem.c`, `v3d_reset`]

El reinicio es **global**: afecta a todas las colas.

1. Parar todas las colas.
2. Marcar el trabajo que venció como culpable.
3. Parar sus contadores de rendimiento sin leerlos.
4. Apuntar `ERR_STAT` del núcleo (`0xF20`) en el registro de depuración.
5. Desactivar las interrupciones (9.4).
6. GCA y SMS: nada en la 4.2.
7. Reiniciar la V3D por el bloque PM (2.4: apagar y encender el dominio).
8. Volver a poner el estado fijo del núcleo (8.3).
9. Dar otra vez la tabla a la MMU y vaciarla (5.4). **La tabla en memoria se conserva:** los búferes siguen mapeados.
10. Activar las interrupciones (9.4).
11. Volver a programar los contadores globales, si los hay.
12. Subir los contadores de reinicios: el global y el del cliente culpable. Mesa los consulta.
13. Volver a lanzar desde el principio **todos** los trabajos pendientes de todas las colas.
14. El trabajo culpable, y los que vengan detrás de él del mismo cliente en esa cola, se cancelan con error en vez de
    ejecutarse. [`sched_main.c`, `drm_sched_resubmit_jobs`, con límite de culpas 0]

`ERR_STAT` (núcleo `0xF20`), bits con nombre: 15 `L2CARE`; 14 `VCMBE`; 13 `VCMRE`; 12 `VCDI`; 11 `VCDE`; 10 `VDWE`;
9 `VPMEAS`; 8 `VPMEFNA`; 7 `VPMEWNA`; 6 `VPMERNA`; 5 `VPMERR`; 4 `VPMEWR`; 3 `VPAERRGL`; 2 `VPAEBRGL`; 1 `VPAERGS`; 0
`VPAEABB`. Linux no los explica. [`v3d_regs.h`]

Antes del reinicio hay un paso de "dejar quieto el bus" (`v3d_idle_axi`), pero Linux lo tiene desactivado en el
reinicio. El comentario dice que solo hace falta para apagar con seguridad. [`v3d_gem.c`, `v3d_reset`]

## 11. Apagar y encender en marcha

Linux apaga la GPU cuando lleva 50 ms sin trabajo, y la enciende al llegar un trabajo. [`v3d_drv.c`,
`v3d_platform_drm_probe`; `v3d_submit.c`, `v3d_submit_add_job`]

**Apagar** [`v3d_power.c`, `v3d_power_suspend`]:

1. Si hay contadores activos, leerlos y pararlos.
2. Desactivar las interrupciones (9.4).
3. Limpiar las cachés (7.7).
4. Parar los accesos al bus: `GMP_CFG` = `STOP_REQ` (bit 1). Esperar a que en `GMP_STATUS` valgan 0 `RD_COUNT`,
   `WR_COUNT` y `CFG_BUSY`. Plazo de 100 ms; si vence, solo se apunta. [`v3d_gem.c`, `v3d_idle_axi`]
5. GCA y SMS: nada en la 4.2.
6. Apagar el reloj del firmware (2.3).
7. Apagar el dominio (2.4). Esto mete la V3D en reinicio.

**Encender**: sección 2.5.

- Se deduce que el `STOP_REQ` del paso 4 se borra con el reinicio del paso 7, porque Linux no lo quita al encender.
  En la 4.2 no hay otro camino que lo quite.
- Para DashOS, apagar en marcha es opcional. Se deduce que basta con encender una vez y no apagar.

## 12. Contadores de rendimiento

[`v3d_perfmon.c`; `v3d_regs.h`; `v3d_drm.h`]

- Hay **32 contadores** de hardware. Cada uno cuenta un evento de una lista de **87** para la 4.2.
  [`v3d_perfmon.c`, `v3d_v42_performance_counters`; `v3d_drm.h`, `DRM_V3D_MAX_PERF_COUNTERS`]
- La lista de eventos y su orden están en la cabecera pública, en el enum `V3D_PERFCNT_*`. La cabecera lo da por
  obsoleto, pero su orden coincide con la tabla de la 4.2. El evento 32 es el contador de ciclos.
  [`v3d_drm.h`; `v3d_regs.h`, `V3D_PCTR_CYCLE_COUNT`]
- Registros del núcleo:
  - `PCTR_0_SRC_X` (`0x660 + 4·x`, `x` de 0 a 7): elige el evento de cuatro contadores. El contador `4x` va en los
    bits 6:0, el `4x+1` en 14:8, el `4x+2` en 22:16 y el `4x+3` en 30:24. Son 7 bits por evento en la 4.2.
  - `PCTR_0_EN` (`0x650`): un bit por contador; 1 = cuenta.
  - `PCTR_0_CLR` (`0x654`): escribir 1 pone ese contador a 0.
  - `PCTR_0_OVERFLOW` (`0x658`): Linux escribe la máscara al empezar. Se deduce que borra el desbordamiento.
  - `PCTR_0_PCTRi` (`0x680 + 4·i`): valor del contador `i`, 32 bits.

**Arrancar** con `n` contadores [`v3d_perfmon.c`, `v3d_perfmon_hw_start`]:

1. Escribir los registros `SRC` necesarios. Los huecos sobrantes del último van a 0.
2. `PCTR_0_EN` = máscara de los `n` bits bajos.
3. `PCTR_0_CLR` = la misma máscara.
4. `PCTR_0_OVERFLOW` = la misma máscara.

**Leer**: sumar cada `PCTRi` a un acumulador de 64 bits y escribir la máscara en `PCTR_0_CLR`. **Parar**: leer si hace
falta y escribir `PCTR_0_EN` = 0. [`v3d_perfmon.c`, `v3d_perfmon_hw_capture` y `v3d_perfmon_hw_stop`]

- Linux arranca los contadores justo antes de lanzar el trabajo y los lee en la interrupción de fin.
- Mientras un trabajo con contadores propios esté en marcha, Linux no deja correr ningún otro trabajo en ninguna cola.
  Así no se mezclan los eventos. Con un contador "global", no serializa. [`v3d_submit.c`,
  `v3d_serialize_for_perfmon`]
- **Medir la frecuencia de la GPU**: `SRC_0_3` = 32 (ciclos) en el contador 0, `CLR` = 1, `EN` = 1, esperar 1 s y
  leer `PCTR0`. [`v3d_debugfs.c`, `v3d_measure_clock`]

## 13. Orden de bytes

### 13.1. Registros

- Linux accede a todos los registros (hub, núcleo, bloque PM, ASB, cprman) con lecturas y escrituras de 32 bits en
  little-endian (`readl`, `writel`). [`v3d_drv.h`, macros `V3D_READ`, `V3D_WRITE`, `V3D_CORE_READ` y
  `V3D_CORE_WRITE`]
- **Para DashOS (big-endian)**: se deduce que cada valor de este documento se escribe en un registro con los bytes
  invertidos respecto al orden nativo de la CPU. Lo mismo al leer. Es la misma regla que en `hvs.md`, 2.5.

### 13.2. Memoria

La GPU lee y escribe la memoria en little-endian. No se conoce ningún bit para cambiarlo (ver "Lo que no está claro").
Todo lo siguiente tiene que estar en little-endian en la RAM:

| Estructura | Quién la escribe | Tamaño de los campos | Nota |
|---|---|---|---|
| Tabla de páginas de la MMU | Kernel | Palabras de 32 bits | Sección 5.2. Linux escribe palabras nativas en una CPU little-endian. |
| Listas de control (BCL, RCL) | Mesa | Paquetes de bytes con campos de 8 a 32 bits | Formato fuera de este documento |
| Código de los QPU | Mesa | Instrucciones de 64 bits | Ídem |
| Uniformes | Mesa (y el kernel en el cómputo indirecto, 7.8) | 32 bits | Ídem |
| Registros de sombreador, de atributos, estado de texturas y muestreadores | Mesa | Varios | Ídem |
| Datos de vértices e índices | Aplicación, vía Mesa | 8, 16 y 32 bits, coma flotante | La GPU los interpreta en little-endian |
| Texturas y destinos de dibujo | Aplicación, TFU, GPU | Según el formato | Ídem |
| Memoria de baldosas, estado de baldosas y desbordamiento | La propia GPU | — | La CPU no la interpreta; solo la reserva |
| Contadores de oclusión | GPU | 32 bits | Mesa los lee |
| Contadores de grupos del cómputo indirecto | Mesa o la GPU | 32 bits | El kernel los lee (7.8) |
| Marcas de tiempo y resultados de consultas | Kernel (7.8) | 32 o 64 bits | El kernel los escribe |

- Las estructuras del kernel (tabla de páginas, trabajos de la CPU) son responsabilidad del controlador de DashOS.
  Tiene que escribirlas con los bytes invertidos.
- **Aviso para portar Mesa**: Mesa escribe las listas, el código y los datos con el orden de la máquina. En una CPU
  big-endian todo eso saldría al revés. Se deduce que el port de Mesa tiene que convertir todo lo que escribe en
  memoria de la GPU. Es trabajo grande y queda fuera de este documento.
- Los valores que el cliente da para los registros de la TFU y del CSD son números, no memoria. Se escriben con la
  regla de 13.1.

## 14. Límites y casos que trata Linux

| Tema | Dato | Fuente |
|---|---|---|
| Espacio de VA | 4 GB, una tabla para todos | [`v3d_mmu.c`, comentario inicial] |
| Tabla de páginas | 4 MB contiguos, alineada a 4 KB, sin caché | [`v3d_gem.c`, `v3d_gem_init`] |
| Página física máxima | 24 bits de número de página (64 GB) | [`v3d_mmu.c`, `v3d_mmu_insert_ptes`] |
| VA 0 | Nunca se usa | [`v3d_gem.c`, `v3d_gem_init`] |
| Tamaño de búfer | Múltiplo de 4 KB, mayor que 0 | [`v3d_bo.c`] |
| Desbordamiento del binner | Bloques de 256 KB, sin límite de número | [`v3d_irq.c`, `v3d_overflow_mem_work`] |
| `OUTOMEM` | Por flanco; puede llegar antes de que falte memoria de verdad | [`v3d_irq.c`] |
| `FLDONE` | No llega mientras haya falta de memoria sin resolver | [`v3d_irq.c`] |
| Cómputo con 0 grupos | No se lanza; el hardware lo entendería como 65 536 | [`v3d_sched.c`, `v3d_csd_job_run`] |
| `CFG4` | Lotes menos 1 en la 4.2 | [`v3d_sched.c`, `v3d_rewrite_csd_job_wg_counts_from_indirect`] |
| `CFG6` | Uniformes alineados a 4 bytes | [`v3d_regs.h`] |
| Errata GFXH-1897 | Esperar a que `L2TFLS` sea 0 antes de escribir `L2TCACTL` en la limpieza | [`v3d_gem.c`, `v3d_clean_caches`] |
| Errata GFXH-1383 | Escritura perdida en el desplazamiento 0 tras reiniciar por bridge. No aplica a la Pi 4. | [`v3d_gem.c`, `v3d_reset_by_bridge`] |
| Plazos de espera de registros | 100 ms (MMU, L2T, TMU, GMP) | [`v3d_mmu.c`; `v3d_gem.c`] |
| Plazo de un trabajo | 500 ms, prorrogable si hay progreso | [`v3d_sched.c`] |
| Plazo del puente ASB | 100 µs | [`bcm2835-power.c`, `bcm2835_asb_control`] |
| Pulso de reinicio | 1 µs de reloj antes de soltar el reinicio | [`bcm2835-power.c`, `bcm2835_asb_power_on`] |
| Apagado por inactividad | 50 ms | [`v3d_drv.c`, `v3d_platform_drm_probe`] |
| Núcleos | 1; Linux no soporta más | [`v3d_drv.c`, `v3d_platform_drm_probe`] |
| Contadores por trabajo | Hasta 32 | [`v3d_drm.h`] |
| Búferes por TFU | Hasta 4; el primero es la salida | [`v3d_drm.h`, `drm_v3d_submit_tfu`] |
| Validación de listas | Ninguna: todo pasa por la MMU | [`v3d_submit.c`] |
| Errores de la MMU | Solo se apuntan; el trabajo sigue o se cuelga | [`v3d_irq.c`, `v3d_hub_irq`] |

## 15. Lo que espera Mesa del kernel (resumen)

Solo como referencia. El estudio de Mesa lo hace otra tarea. [`v3d_drm.h`; `v3d_drv.c`, `v3d_get_param_ioctl`]

- **Parámetros**: los siete registros de identificación de la sección 3, sin tocar. Además: hay TFU (sí), hay CSD (sí
  en la 4.2), hay limpieza de cachés (sí), hay contadores (sí), hay varias vallas por envío (sí), hay cola de CPU
  (sí), número de eventos de contador (87), si hay páginas grandes, y los dos contadores de reinicios.
- **Búferes**: crear (devuelve la VA, nunca 0), mapear para la CPU, consultar la VA y esperar a que la GPU acabe.
- **Envíos**: listas (`bcl_start`, `bcl_end`, `rcl_start`, `rcl_end`, `qma`, `qms`, `qts`, banderas), TFU (sus diez
  valores de registro), cómputo (`cfg[0..6]`) y trabajos de CPU. Cada uno con vallas de entrada y de salida.

## 16. Diferencias con la 7.1 (Pi 5)

| Tema | 4.2 (BCM2711) | 7.1 (BCM2712) |
|---|---|---|
| Líneas de interrupción | Una para hub y núcleo | Dos |
| `CSDDONE` | Bit 7 del núcleo | Bit 6 |
| `PCTR` | Bit 6 del núcleo | Bit 5 |
| `GMPV` | Bit 5 del núcleo | Bit 6 del hub |
| Registros de la TFU | Hub `0x400` | Hub `0x700`, con un registro `IOC` más |
| `COEF1..3` de la TFU | Solo con `USECOEF` | Siempre |
| Registros del CSD | Núcleo `0x904` | Núcleo `0x930`, con un `CFG7` (Linux le escribe 0) |
| `CFG4` del cómputo | Lotes menos 1 | Igual hasta la revisión 7.1.5; desde la 7.1.6, lotes sin restar |
| GMP | Núcleo `0x800` | Hub `0x600` |
| Selector de evento | 7 bits | 8 bits |
| Evento "ciclos" | 32 | 0 |
| `MMUC_CONTROL.CLEAR` | Bit 3 | Bit 11 |
| Energía y reinicio | Bloque PM y ASB | Bloque SMS propio |
| Errata GFXH-1897 | Se aplica | No |
| Tabla de `MMU_VIO_ID` | 5.7 | Otra |

[`v3d_regs.h`; `v3d_irq.c`; `v3d_sched.c`; `v3d_gem.c`; `v3d_power.c`; `bcm2835-power.c`]

## Lo que no está claro

1. **Orden de bytes de la GPU.** No se conoce ningún bit para que la V3D lea en big-endian. `HUB_UIFCFG` y
   `HUB_AXICFG` no tienen campos documentados en Linux. Se da por hecho que todo es little-endian.
2. **Página de relleno y aborto a la vez.** Linux activa los dos para las entradas no válidas. No se sabe cuál manda
   ni qué ve el sombreador.
3. **Bits de estado de `MMU_CTL`.** Que se borren escribiendo 1 se deduce de que Linux reescribe el valor leído.
4. **Bits 27:24 de la entrada de tabla.** Linux los deja a 0. No se sabe si significan algo.
5. **Valores reales de `VA_WIDTH` y `PA_WIDTH`** en la Pi 4. No se han leído en una placa.
6. **Número de QPU, slices y TMU.** Se espera 8 QPU en 2 slices. Hay que leerlo de `CTL_IDENT1`.
7. **El reloj `v3d` de cprman.** Linux lo enciende y apaga desde el bloque PM, y a la vez el firmware gobierna el
   reloj V3D por el buzón. No se sabe si son el mismo reloj ni si tocar `CM_V3DCTL` estorba al firmware.
8. **Dominio de energía por firmware** (`SET_DOMAIN_STATE`, dominio 10). Existe en el DTB, pero Linux no lo usa en la
   Pi 4. No se sabe si serviría en lugar del bloque PM.
9. **Estado en que deja la GPU el firmware.** Linux no reinicia al arrancar. No se sabe si la V3D está ya fuera de
   reinicio, con el puente activo y con el reloj encendido. Se deduce que lo seguro es hacer el ciclo completo de
   2.4 al empezar.
10. **`OUTOMEM` sin trabajo activo.** Linux suelta la memoria. No se sabe si el hardware puede quedarse esperando en
    algún caso.
11. **Cuánta memoria de desbordamiento hace falta.** Linux da 256 KB cada vez, sin tope. No se sabe si conviene dar
    más de una vez.
12. **`CLE_CT1QCFG`**: Linux no lo escribe. No se sabe qué valor tiene tras el reinicio.
13. **Colas internas de CT0 y CT1 y semáforos de hardware.** Existen, según los comentarios, pero Linux no los usa.
    No se sabe cómo funcionan.
14. **`TFUF`, `MSO`, `TRFB`, `SPILLUSE`, `QPU` y `PCTR`**: interrupciones sin descripción o sin uso en Linux.
15. **Registros CLE sin descripción** (`CT0LC` a `CT1SYNC`) y bits de `ERR_STAT`: solo hay nombres.
16. **L1T y VCD.** La cabecera dice que la L1T se vacía antes de cada lista, pero no hay operación propia. Se deduce
    que va con los slices o la L2T. La VCD queda a cargo de las listas.
17. **TFU y la L2T.** Se deduce que la TFU no pasa por la L2T porque Linux no invalida antes. No está escrito.
18. **`STOP_REQ` del GMP tras encender.** Se deduce que el reinicio lo borra. No está escrito.
19. **QEMU `raspi4b`.** No se ha comprobado si emula la V3D. Lo más probable es que no, y que esto solo se pueda
    probar en una Pi 4 real.
20. **Modo de direcciones de periféricos.** Las direcciones físicas valen con el modo bajo. No se ha comprobado qué
    modo deja Emu68.
21. **Mesa en big-endian.** Ver 13.2. El tamaño del trabajo de conversión no se ha estudiado aquí.
