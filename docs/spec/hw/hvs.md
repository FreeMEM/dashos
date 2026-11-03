# HVS de la BCM2711 (Raspberry Pi 4)

Especificación de sala limpia. La escribe el analista leyendo el controlador vc4 de Linux. Quien implemente no
necesita ni debe leer ese código: todo lo necesario tiene que estar aquí.

## 0. Origen y convenciones

- **Fuente**: `torvalds/linux`, commit `6628e7d4c6b25c8d18f1345e3a20d21829aae7e4` (1 de noviembre de 2026), carpeta
  `drivers/gpu/drm/vc4/`. También `include/soc/bcm2835/raspberrypi-firmware.h`, `drivers/clk/bcm/clk-raspberrypi.c`
  y `arch/arm/boot/dts/broadcom/bcm2711*.dtsi`.
- **Árbol de dispositivos**: `build/emu68-rpi/firmware/bcm2711-rpi-4-b.dtb` del repositorio, decompilado con `dtc`.
  Es el árbol del firmware de Raspberry Pi, no el de Linux, pero coinciden en lo que importa aquí.
- **Generación**: Linux llama "HVS5" o `VC4_GEN_5` al HVS de la BCM2711. La BCM2835 (Pi 0 a 3) es `VC4_GEN_4`. La
  BCM2712 (Pi 5) es `VC4_GEN_6_C` o `VC4_GEN_6_D`. Este documento describe solo la BCM2711. Las diferencias con las
  otras dos se señalan cuando ayudan a no equivocarse.
- **Citas**: cada dato lleva entre corchetes el fichero y la función de Linux de donde sale. Por ejemplo,
  [`vc4_hvs.c`, `vc4_hvs_hw_init`]. Si un dato sale de una cabecera, se cita la cabecera.
- **Comprobado y deducido**: "Linux hace X" significa que se leyó en el código. "Se deduce" significa que es una
  conclusión del analista, no escrita como tal en Linux. Nada de este documento se ha probado en hardware real.
- **Notación**: los desplazamientos son en bytes desde la base del HVS. Un campo `[h:l]` ocupa los bits de `h` a `l`
  de una palabra de 32 bits, ambos incluidos. "Palabra" son siempre 32 bits.
- **Nombres**: los nombres de registros y bits son los de Linux (`SCALER_DISPCTRL`, `SCALER5_CTL0_UNITY`...). Son
  hechos del hardware y se usan para que se puedan contrastar.

## 1. Qué es el HVS

- El HVS (Hardware Video Scaler) lee píxeles de la memoria, los convierte, los escala y los compone por capas.
- El resultado va a una FIFO de salida. Cada FIFO la consume una "pixel valve" (PV), que genera las señales de
  sincronía y entrega los píxeles al codificador (HDMI, DSI, DPI, VEC).
- Hay un solo HVS. Tiene tres canales de salida, cada uno con su FIFO. En este documento "canal" y "FIFO" son lo
  mismo.
- El HVS no tiene planos fijos. Lo que se compone en cada canal lo dice una **lista de visualización** (display
  list, "dlist"): una secuencia de entradas, una por plano, en una memoria interna del HVS.
- Funciona con el reloj del sistema ("core"), mucho más rápido que el reloj de píxel. [`vc4_hvs.c`, comentario
  inicial del módulo]

## 2. Dónde está y qué necesita

### 2.1. Direcciones

| Dato | Valor | Fuente |
|---|---|---|
| Compatible | `brcm,bcm2711-hvs` | DTB, nodo `hvs@7e400000`; [`vc4_hvs.c`, `vc4_hvs_dt_match`] |
| Dirección de bus (VideoCore) | `0x7E400000` | DTB, `reg` |
| Dirección física para la CPU | `0xFE400000` | Se deduce del `ranges` del nodo `soc`: el bus `0x7E000000` se ve en `0x0_FE000000` |
| Tamaño de la ventana | `0x8000` (32 KB) | DTB, `reg` |
| Registros | `0x0000` a `0x3FFF` | [`vc4_regs.h`]; ver la sección 3 |
| Memoria de listas | `0x4000` a `0x7FFF` (16 KB, 4096 palabras) | [`vc4_regs.h`, `SCALER5_DLIST_START`, `SCALER_DLIST_SIZE`]; [`vc4_hvs.c`, `vc4_hvs_bind`] |

El `ranges` del DTB corresponde al modo de periféricos "bajo" que usa el firmware por defecto. Con el modo de 35 bits
completo la dirección física sería otra. No se ha comprobado qué modo deja Emu68.

### 2.2. Interrupción

| Dato | Valor | Fuente |
|---|---|---|
| Línea | GIC SPI 97, nivel alto | DTB (`interrupts = <0 0x61 4>`); `bcm2711.dtsi` |
| INTID en el GIC-400 | 129 (SPI + 32) | Se deduce |
| Controlador | GIC-400 en `0x40041000` (CPU `0xFF841000`) | DTB, nodo `gicv2` y `ranges` |

Linux pide la interrupción del HVS solo en la BCM2835 y la BCM2711. En la BCM2712 no la usa. [`vc4_hvs.c`,
`vc4_hvs_bind`]

### 2.3. Relojes

- El nodo del HVS pide un solo reloj: el reloj número 4 del firmware, `core`. [DTB, `clocks = <&firmware_clocks
  4>`; `bcm2711-rpi.dtsi`; `raspberrypi-firmware.h`, `RPI_FIRMWARE_CORE_CLK_ID`]
- Linux obtiene dos manejadores de reloj, `core_clk` y `disp_clk`, pero en la BCM2711 los dos son el mismo reloj.
  Los pide sin nombre y el nodo solo tiene uno. [`vc4_hvs.c`, `vc4_hvs_bind`]
- **El reloj lo gobierna el firmware de la VideoCore.** No hay registro del HVS para encenderlo. Se maneja con el
  buzón de propiedades (el mismo canal que ya usa Emu68):

| Operación | Etiqueta del buzón | Uso en Linux |
|---|---|---|
| Leer el máximo | `0x00030004` (`GET_MAX_CLOCK_RATE`), reloj 4 | Al arrancar, para decidir límites. [`vc4_hvs.c`, `vc4_hvs_bind`] |
| Encender | `0x00038001` (`SET_CLOCK_STATE`), bit 0 a 1 | `clk_prepare_enable`. [`clk-raspberrypi.c`, `raspberrypi_fw_prepare`] |
| Fijar frecuencia | `0x00038002` (`SET_CLOCK_RATE`) | Según la carga. [`vc4_kms.c`, `vc4_atomic_commit_tail`] |
| Leer frecuencia | `0x00030002` (`GET_CLOCK_RATE`) | Solo para depurar. |

- El reloj `core` también mueve el bus de todo lo demás. Linux nunca lo apaga. Lo comparte con la cámara (Unicam).
  [`clk-raspberrypi.c`, tabla de variantes, entrada de `CORE_CLK_ID`]
- En la práctica el firmware ya lo deja encendido al arrancar. Se deduce que DashOS no necesita encenderlo, solo
  subirlo cuando la carga lo pida (sección 11).
- Antes de tocar el HVS, Linux le dice al firmware que deje de manejar la pantalla. Usa la etiqueta `0x00030066`
  (`NOTIFY_DISPLAY_DONE`) sin datos. [`vc4_drv.c`, `vc4_drm_bind`] Ver la sección 12.1.
- No hay dominio de energía propio para el HVS en el DTB. Las PV y los HDMI sí tienen relojes y reinicios propios,
  pero quedan fuera de este documento.

### 2.4. Qué memoria alcanza

- El HVS lee los píxeles por DMA con direcciones de bus de 32 bits. En la BCM2711 el puntero de una entrada de plano
  es de 32 bits y no hay bits altos. [`vc4_plane.c`, `vc4_plane_mode_set`, palabras de puntero; los bits altos solo
  existen en la BCM2712, `SCALER6_PTR0_UPPER_ADDR`]
- El `dma-ranges` del nodo `soc` dice: la dirección de bus `0xC0000000 + f` corresponde a la física `f`, para `f`
  menor que `0x40000000`. [DTB, `soc/dma-ranges`]
- **Se deduce**: el HVS solo ve el primer GB de RAM, y el puntero que se escribe es la dirección física más
  `0xC0000000`. Linux no calcula esto a mano: usa la dirección DMA del búfer, que el núcleo deriva de ese
  `dma-ranges`.
- Linux pone los búferes de pantalla en la zona CMA. En este DTB la zona CMA está por debajo de `0x30000000`. [DTB,
  `reserved-memory/linux,cma`]
- El HVS no es coherente con las cachés de la CPU, se deduce del uso de memoria DMA en Linux. Los píxeles tienen que
  estar en memoria no cacheable o limpiarse de la caché antes de mostrarse.

### 2.5. Orden de bytes

- Linux accede a los registros y a la memoria de listas con escrituras y lecturas de 32 bits en orden
  little-endian (`writel`, `readl`). [`vc4_drv.h`, `HVS_READ` y `HVS_WRITE`]
- La memoria de listas también se escribe palabra a palabra. Linux no usa copias por bloques: "necesita escrituras de
  32 bits". [`vc4_plane.c`, `vc4_plane_write_dlist`]
- **Para DashOS (big-endian)**: se deduce que cada valor de 32 bits que este documento da para un registro o para una
  palabra de lista se tiene que escribir con los bytes invertidos respecto al orden nativo de la CPU. Lo mismo al
  leer.
- El orden de bytes de los píxeles en RAM se trata en la sección 6.3.

## 3. Registros

### 3.1. Mapa general

Solo están los registros que Linux usa o nombra para la BCM2711. Los canales 0, 1 y 2 tienen un bloque de cuatro
registros cada uno, separados `0x10`. [`vc4_regs.h`; `vc4_hvs.c`, tabla `vc4_hvs_regs`]

| Desp. | Nombre | Para qué | Uso en Linux (BCM2711) |
|---|---|---|---|
| `0x000` | `SCALER_DISPCTRL` | Control global, interrupciones, mux de la salida 3 | Sí |
| `0x004` | `SCALER_DISPSTAT` | Estado global e interrupciones | Sí |
| `0x008` | `SCALER_DISPID` | Identificación | Solo depuración |
| `0x00C` | `SCALER_DISPECTRL` | Mux de la salida 2 (bit 31) | Sí |
| `0x010` | `SCALER_DISPPROF` | Perfilador | No |
| `0x014` | `SCALER_DISPDITHER` | Tramado; mux de la salida 5 (bits 31:30) | Sí, solo el mux |
| `0x018` | `SCALER_DISPEOLN` | Línea N para la interrupción EOLN; mux de la salida 4 (bits 31:30) | Sí, solo el mux |
| `0x020` | `SCALER_DISPLIST0` | Inicio de la lista del canal 0 | Sí |
| `0x024` | `SCALER_DISPLIST1` | Inicio de la lista del canal 1 | Sí |
| `0x028` | `SCALER_DISPLIST2` | Inicio de la lista del canal 2 | Sí |
| `0x02C` | `SCALER_DISPLSTAT` | Estado de las listas | No |
| `0x030` | `SCALER_DISPLACT0` | Lista activa del canal 0 (lectura) | Sí |
| `0x034` | `SCALER_DISPLACT1` | Lista activa del canal 1 | Sí |
| `0x038` | `SCALER_DISPLACT2` | Lista activa del canal 2 | Sí |
| `0x040` | `SCALER_DISPCTRL0` | Control del canal 0 | Sí |
| `0x044` | `SCALER_DISPBKGND0` | Fondo del canal 0 | Sí |
| `0x048` | `SCALER_DISPSTAT0` | Estado del canal 0 | Sí |
| `0x04C` | `SCALER_DISPBASE0` | Zona del COB del canal 0 | Sí |
| `0x050` a `0x05C` | `DISPCTRL1`, `DISPBKGND1`, `DISPSTAT1`, `DISPBASE1` | Canal 1 | Sí |
| `0x060` a `0x06C` | `DISPCTRL2`, `DISPBKGND2`, `DISPSTAT2`, `DISPBASE2` | Canal 2 | Sí |
| `0x070` | `SCALER_DISPALPHA2` | Sin descripción en Linux | No |
| `0x078` | `SCALER_GAMADDR` | Dirección de la tabla de gamma | No en la BCM2711 |
| `0x080` | `SCALER_OLEDOFFS` | Matriz de color de salida: desplazamientos y FIFO | Escribe 0 (desactivada) |
| `0x084` a `0x08C` | `SCALER_OLEDCOEF0..2` | Matriz de color de salida: coeficientes | No en la BCM2711 |
| `0x0C0`, `0x0C9`, `0x0D0` | `SCALER_DISPSLAVE0..2` | Salida por registro en vez de FIFO | No |
| `0x0E0` | `SCALER_GAMDATA` | Datos de la tabla de gamma | No en la BCM2711 |
| `0x4000` | Memoria de listas | 4096 palabras | Sí |

`DISPSLAVE1` aparece en Linux en `0x0C9`, que no está alineado a 4. Probablemente es una errata de la cabecera y
vale `0x0C8`. No importa: Linux no lo usa. [`vc4_regs.h`]

### 3.2. `SCALER_DISPCTRL` (`0x000`): control global

Para la BCM2711 algunos bits están en otra posición que en la BCM2835. Linux usa las macros `SCALER5_*` para ellos.
[`vc4_regs.h`]

| Bits | Nombre | Significado |
|---|---|---|
| 31 | `ENABLE` | Activa el HVS. Linux lo llama "puerta de reloj global". |
| 29:28 | `PANIC2` | Umbral de "pánico" del bus AXI para el canal 2. |
| 27:26 | `PANIC1` | Ídem, canal 1. |
| 25:24 | `PANIC0` | Ídem, canal 0. |
| 19:18 | `DSP3_MUX` | Qué FIFO alimenta la salida 3 (PV1). 0 a 2: esa FIFO. 3: ninguna. |
| 9 + 4·c | `DSPEISLUR(c)` | Para el canal `c`: las líneas cortas y el vaciado (underrun) cuentan para la interrupción. |
| 8 + 4·c | `DSPEIEOLN(c)` | Para el canal `c`: el fin de la línea N cuenta para la interrupción. |
| 7 + 4·c | `DSPEIEOF(c)` | Para el canal `c`: el fin de cuadro cuenta para la interrupción. |
| 6 + 4·c | `DSPEIVST(c)` | Para el canal `c`: el inicio de cuadro (VSTART) cuenta, se deduce del nombre. |
| 5 | `SLVEIRQ` | Interrupción por error de esclavo AXI. |
| 4 | `DMAEIRQ` | Interrupción por error de DMA. |
| 3, 2, 1 | `DISPEIRQ(2)`, `(1)`, `(0)` | Permite que el canal genere interrupción. |
| 0 | `SCLEIRQ` | Interrupción del perfilador. |

Con `c` de 0 a 2: el canal 0 usa los bits 6 a 9, el canal 1 los bits 10 a 13 y el canal 2 los bits 14 a 17.
[`vc4_regs.h`, `SCALER5_DISPCTRL_*`]

Valores que deja Linux al arrancar [`vc4_hvs.c`, `vc4_hvs_hw_init`]:

- `ENABLE` a 1.
- `DSP3_MUX` a 3 (salida 3 desconectada).
- `DISPEIRQ(0)`, `(1)` y `(2)` a 1.
- `DMAEIRQ`, `SLVEIRQ`, `SCLEIRQ` y todos los `DSPEI*` de los tres canales a 0. El de vaciado se activa después, por
  canal (sección 13).
- Los tres `PANIC` a 2. El comentario de Linux dice que la BCM2711 "entra en pánico con menos de una línea en la
  FIFO". No explica la codificación del valor.
- El resto de bits se conserva: Linux lee, modifica y escribe.

### 3.3. `SCALER_DISPSTAT` (`0x004`): estado e interrupciones

| Bits | Nombre | Significado |
|---|---|---|
| 13 + 8·c | `COBLOW(c)` | El COB del canal `c` está bajo. Sin más explicación en Linux. |
| 12 + 8·c | `EOLN(c)` | El canal terminó de componer la línea N de `DISPEOLN`. |
| 11 + 8·c | `ESFRAME(c)` | Llegó VSTART con píxeles pendientes en la línea (cuadro corto). |
| 10 + 8·c | `ESLINE(c)` | Llegó HSTART con píxeles pendientes en la línea (línea corta). |
| 9 + 8·c | `EUFLOW(c)` | La PV leyó de la FIFO vacía (vaciado, underrun). |
| 8 + 8·c | `EOF(c)` | El canal pasó de RUN a EOF (fin de cuadro). |
| 15:14 | `RESP` | Respuesta AXI: 0 OKAY, 1 EXOKAY, 2 SLVERR, 3 DECERR. |
| 7 | `DMA_ERROR` | Error de identificador de DMA en AXI. |
| 6 | `IRQSLVRD` | Error de decodificación en lectura como esclavo. |
| 5 | `IRQSLVWR` | Error de decodificación en escritura como esclavo. |
| 4 | `IRQDMA` | Resumen de errores de DMA. |
| 3, 2, 1 | `IRQDISP(2)`, `(1)`, `(0)` | Hay un evento activo y permitido en ese canal. |
| 0 | `IRQSCL` | Al leer: interrupción del perfilador. Al escribir un 1: borra todas las interrupciones. |

El canal 0 usa los bits 8 a 13, el canal 1 los 16 a 21 y el canal 2 los 24 a 29. Los bits de evento se borran
escribiendo un 1. Linux borra los seis bits de los tres canales de una vez. [`vc4_regs.h`, `SCALER_DISPSTAT_*`;
`vc4_hvs.c`, `vc4_hvs_irq_handler`]

Estos bits son los de la BCM2835. Linux los usa igual en la BCM2711 y no define variantes `SCALER5_`. Se deduce que no
cambiaron.

### 3.4. Registros de mux de salida

Ver la sección 10.3 para el significado. Los tres campos comparten registro con otras funciones. Hay que leer,
modificar y escribir. [`vc4_kms.c`, `vc5_hvs_pv_muxing_commit`]

| Registro | Bits | Campo | Salida |
|---|---|---|---|
| `SCALER_DISPECTRL` (`0x00C`) | 31 | `DSP2_MUX` | 2 (TXP) |
| `SCALER_DISPCTRL` (`0x000`) | 19:18 | `DSP3_MUX` | 3 (PV1) |
| `SCALER_DISPEOLN` (`0x018`) | 31:30 | `DSP4_MUX` | 4 (PV2, HDMI0) |
| `SCALER_DISPDITHER` (`0x014`) | 31:30 | `DSP5_MUX` | 5 (PV4, HDMI1) |

### 3.5. `SCALER_DISPLISTx` (`0x020 + 4·c`) y `SCALER_DISPLACTx` (`0x030 + 4·c`)

- `DISPLISTx`: índice, **en palabras**, de la primera palabra de la lista del canal dentro de la memoria de listas. No
  es una dirección en bytes. [`vc4_hvs.c`, `vc4_hvs_install_dlist` y `vc4_hvs_debugfs_dlist`]
- `DISPLACTx`: índice de la lista que el hardware está usando de verdad. Es de lectura. Linux lo compara con el
  índice que escribió en `DISPLISTx` para saber si el cambio ya se aplicó. [`vc4_crtc.c`, `vc4_crtc_handle_page_flip`]

### 3.6. `SCALER_DISPCTRLx` (`0x040 + 0x10·c`): control del canal

Posiciones de la BCM2711 [`vc4_regs.h`, `SCALER5_DISPCTRLX_*`]:

| Bits | Nombre | Significado |
|---|---|---|
| 31 | `ENABLE` | Canal activo. |
| 30 | `RESET` | Reinicia el canal. |
| 28:16 | `WIDTH` | Ancho de salida en píxeles. Linux escribe el ancho activo del modo, sin restar 1. |
| 15 | `ONESHOT` | Genera un solo cuadro al ver VSTART y se para. Linux lo usa solo con el TXP. |
| 14:13 | `ONECTX` | Procesa un contexto de la lista por turno en vez de una línea entera. Linux no lo usa. |
| 12:0 | `HEIGHT` | Alto de salida en líneas. Linux escribe el alto activo del modo, sin restar 1. |

[`vc4_hvs.c`, `vc4_hvs_init_channel`]

En la BCM2835 el ancho estaba en `[23:12]` y el alto en `[11:0]`. Allí existían además `FIFO32` (bit 27) y `FIFOREG`
(bit 26). En la BCM2711 esas posiciones son parte de `WIDTH`.

### 3.7. `SCALER_DISPBKGNDx` (`0x044 + 0x10·c`): fondo del canal

| Bits | Nombre | Significado |
|---|---|---|
| 31 | `BCK2BCK` (BCM2711) | Sin descripción. Linux lo pone a 0. En la BCM2835 este bit era `AUTOHS`. |
| 30 | `INTERLACE` | Salida entrelazada. |
| 29 | `GAMMA` | Aplica la tabla de gamma. Linux lo pone a 0 en la BCM2711. |
| 28:25 | `TESTMODE` | Modo de prueba. Linux no lo toca. |
| 24 | `FILL` | Rellena cada línea con el color de `[23:0]` antes de componer. |
| 23:0 | Color | RGB del fondo. Linux no lo escribe nunca: conserva lo que hubiera. |

- Linux pone `FILL` siempre en la BCM2711, aunque los planos cubran toda la pantalla. El comentario dice que así el
  HVS puede ir con un reloj `core` más bajo. [`vc4_hvs.c`, `vc4_hvs_atomic_flush`]
- En la BCM2835 el relleno cuesta ciclos y Linux lo evita cuando puede. [`vc4_regs.h`, comentario de `FILL`]

### 3.8. `SCALER_DISPSTATx` (`0x048 + 0x10·c`): estado del canal

| Bits | Nombre | Significado |
|---|---|---|
| 31:30 | `MODE` | 0 DISABLED, 1 INIT, 2 RUN, 3 EOF. |
| 29 | `FULL` | La FIFO del canal está llena. |
| 28 | `EMPTY` | La FIFO del canal está vacía. |
| 11:0 | `LINE` | Línea que el HVS está componiendo. |

[`vc4_regs.h`, `SCALER_DISPSTATX_*`]

Contadores de cuadros de la BCM2711, de 6 bits cada uno [`vc4_hvs.c`, `vc4_hvs_get_fifo_frame_count`]:

| Canal | Registro | Bits |
|---|---|---|
| 0 | `SCALER_DISPSTAT1` (`0x058`) | 25:20 |
| 1 | `SCALER_DISPSTAT1` (`0x058`) | 19:14 |
| 2 | `SCALER_DISPSTAT2` (`0x068`) | 19:14 |

Sí: el contador del canal 0 está en el registro de estado del canal 1. Así lo lee Linux.

### 3.9. `SCALER_DISPBASEx` (`0x04C + 0x10·c`): reparto del COB

El COB (composite output buffer) es la memoria de las FIFO de salida. Se reparte entre los tres canales.
[`vc4_regs.h`, `SCALER_DISPBASEX_*`; `vc4_hvs.c`, `vc4_hvs_cob_init`]

| Bits | Nombre | Significado |
|---|---|---|
| 31:16 | `TOP` | Último píxel del COB asignado al canal. |
| 15:0 | `BASE` | Primer píxel del COB asignado al canal. |

- Las dos posiciones tienen que estar alineadas a 4 píxeles. El firmware de Raspberry Pi rellena los dos bits bajos y
  Linux los ignora al leer. [`vc4_crtc.c`, `vc4_crtc_get_cob_allocation`]
- Tamaño de la zona, según Linux: `TOP` sin los dos bits bajos, menos `BASE` sin los dos bits bajos, más 4.
- El reparto que hace Linux está en la sección 10.2.

### 3.10. Matriz de color de salida (`0x080` a `0x08C`)

- `SCALER_OLEDOFFS`: bit 31 `YUVCLAMP`. Bits 25:24 `DISPFIFO`: 0 desactivada, 1 a 3 aplica a la FIFO 0 a 2. Bits
  23:16, 15:8 y 7:0: desplazamientos de rojo, verde y azul, en complemento a dos de 8 bits.
- `SCALER_OLEDCOEF0..2`: tres coeficientes de 10 bits por registro, en `[29:20]`, `[19:10]` y `[9:0]`. Formato S0.9:
  bit de signo y 9 bits de fracción, sin parte entera. `COEF0` lleva la contribución del azul a R, G y B. `COEF1`, la
  del verde. `COEF2`, la del rojo.
- Solo se puede aplicar a una FIFO a la vez.
- En la BCM2711 Linux no ofrece esta matriz a los usuarios. Aun así escribe `OLEDOFFS` con `DISPFIFO` a 0 en cada
  confirmación de estado.

[`vc4_regs.h`, `SCALER_OLED*`; `vc4_kms.c`, `vc4_ctm_commit` y `vc4_ctm_atomic_check`; `vc4_crtc.c`,
`__vc4_crtc_init`, que activa la gestión de color solo en la BCM2835]

### 3.11. Gamma

- `SCALER_GAMADDR` (`0x078`): bit 31 `AUTOINC`, bit 30 `SRAMENB`, el resto es el índice. `SCALER_GAMDATA` (`0x0E0`):
  datos.
- En la BCM2835 la tabla tiene 256 entradas de rojo, 256 de verde y 256 de azul por canal, seguidas.
- En la BCM2711 Linux no carga la tabla y pone `DISPBKGND.GAMMA` a 0. [`vc4_hvs.c`, `vc4_hvs_lut_load` y
  `vc4_hvs_init_channel`] No se sabe si la gamma funciona en la BCM2711 ni con qué formato.

## 4. La memoria de listas

- Son 4096 palabras en `0x4000` a `0x7FFF` de la ventana del HVS. Es una SRAM interna, no RAM del sistema.
  [`vc4_hvs.c`, `__vc4_hvs_alloc` y `vc4_hvs_bind`]
- Todo se direcciona en palabras: `DISPLISTx`, `DISPLACTx` y el puntero al núcleo del filtro (sección 7.4).
- **Las palabras 0 a 31 son del firmware.** Ahí está la lista que el firmware usó al arrancar. Linux no las toca para
  que la pantalla no se estropee al pasar del firmware al controlador. Gestiona solo las palabras 32 a 4095.
  [`vc4_regs.h`, `HVS_BOOTLOADER_DLIST_END`; `vc4_hvs.c`, `__vc4_hvs_alloc`]
- En esa memoria viven a la vez las listas de los canales y el núcleo del filtro de escalado.
- Linux reparte la memoria con un asignador genérico. Cada vez que cambia el estado de un canal pide un bloque nuevo
  del tamaño exacto: la suma de las entradas de sus planos, más una palabra para el final. El bloque viejo se libera
  cuando el estado viejo se destruye, es decir, después de que el hardware ya usa el nuevo. [`vc4_hvs.c`,
  `vc4_hvs_atomic_check`; `vc4_crtc.c`, `vc4_crtc_destroy_state`]
- **Se deduce**: así hay doble búfer de listas sin esfuerzo. La lista nueva se escribe en un hueco libre mientras el
  hardware lee la vieja.
- **El hardware escribe en la lista.** Algunas palabras son de "contexto": el HVS guarda ahí su estado mientras
  recorre la lista. No se puede reescribir una lista entera mientras está en uso. [`vc4_plane.c`,
  `vc4_plane_atomic_async_update`, comentario "would smash the context data"]
- Una lista es una secuencia de entradas de plano. Termina con una palabra que tiene el bit 31 (`SCALER_CTL0_END`) a
  1 y el resto a 0. [`vc4_hvs.c`, `vc4_hvs_atomic_flush`]
- El orden de las entradas es el orden de composición: la primera queda debajo de todas. Linux ordena los planos por
  su `zpos` normalizado, de menor a mayor. [`vc4_hvs.c`, `vc4_hvs_atomic_flush`]
- Una lista vacía (solo la palabra de fin) es válida. Linux la escribe cuando el canal no tiene planos.

## 5. Entrada de plano, palabra a palabra

### 5.1. Orden de las palabras

Una entrada tiene longitud variable. Las palabras van en este orden y solo aparecen las que aplican. [`vc4_plane.c`,
`vc4_plane_mode_set`, rama de la BCM2711]

| N.º | Palabra | Cuándo aparece |
|---|---|---|
| 1 | `CTL0`, control | Siempre |
| 2 | `POS0`, posición | Siempre |
| 3 | `CTL2`, alfa | Siempre (en la BCM2711; no existe en la BCM2835) |
| 4 | `POS1`, tamaño escalado | Solo si hay escalado (bit `UNITY` a 0) |
| 5 | `POS2`, tamaño de origen | Siempre |
| 6 | `POS3`, contexto | Siempre. La escribe el HVS. |
| 7 | `PTR0..PTRn-1`, punteros | Uno por cada plano de color del formato (1 para RGB, 2 o 3 para YUV) |
| 8 | Contexto de puntero | Uno por cada puntero. Lo escribe el HVS. |
| 9 | `PITCH0` | Siempre |
| 10 | `PITCH1..PITCHn-1` | Uno por cada plano de color a partir del segundo |
| 11 | `CSC0..CSC2`, conversión de color | Solo para formatos YUV |
| 12 | `LBM` | Solo si hay escalado vertical en algún canal del escalador |
| 13 | Parámetros de escalado | Solo si hay escalado; ver 5.9 |
| 14 | Punteros al núcleo del filtro (4 palabras) | Solo si algún canal usa PPF |

Linux escribe `0xC0C0C0C0` en todas las palabras de contexto. No se sabe si el valor inicial importa.

La entrada más corta, un plano RGB sin escalado, tiene 8 palabras: `CTL0`, `POS0`, `CTL2`, `POS2`, `POS3`, `PTR0`,
contexto y `PITCH0`. Se deduce del orden anterior.

### 5.2. `CTL0`: control

Posiciones de la BCM2711 [`vc4_regs.h`, `SCALER_CTL0_*` y `SCALER5_CTL0_*`]:

| Bits | Nombre | Significado |
|---|---|---|
| 31 | `END` | Fin de lista. Solo en la palabra final, que va sola. |
| 30 | `VALID` | Entrada válida. Linux siempre lo pone. |
| 29:24 | `SIZE` | Número de palabras de esta entrada, contando `CTL0`. Máximo 63. |
| 21:20 | `TILING` | Organización en memoria: 0 lineal, 1 SAND de 64 bytes, 2 SAND de 128 bytes, 3 SAND de 256 bytes o "T-tiled" de la GPU. |
| 15 | `UNITY` | Sin escalado. Con este bit no hay `POS1` ni parámetros de escalado. |
| 14:13 | `ORDER` | Orden de los componentes (sección 6.2). |
| 12 | `ALPHA_EXPAND` | Cómo se amplía el alfa de menos de 8 bits. Linux siempre lo pone. |
| 11 | `RGB_EXPAND` | Cómo se amplían los componentes de color de menos de 8 bits. Linux siempre lo pone. |
| 10:8 | `SCL1` | Modo de escalado del canal 1 del escalador (tabla 5.8). |
| 7:5 | `SCL0` | Modo de escalado del canal 0 del escalador. |
| 4:0 | `PIXEL_FORMAT` | Formato de píxel (sección 6.1). |

- Linux no explica qué hacen `ALPHA_EXPAND` y `RGB_EXPAND` exactamente. En la BCM2835 el campo equivalente tenía
  cuatro opciones (ceros, repetir el bit bajo, repetir el alto, redondear) y Linux elegía "redondear".
- Los bits 23:22 y 19:16 no se usan en la BCM2711. En la BCM2835 el 19 era la máscara de alfa, el 18:17 la clave de
  color, el 16 el volteo horizontal y el 15 el vertical. En la BCM2711 los volteos pasaron a `POS0` y el 15 es
  `UNITY`.

### 5.3. `POS0`: posición en pantalla

| Bits | Nombre | Significado |
|---|---|---|
| 31 | `VFLIP` | Volteo vertical. |
| 27:16 | `START_Y` | Línea de pantalla donde empieza el plano. |
| 15 | `HFLIP` | Volteo horizontal. |
| 13:0 | `START_X` | Columna de pantalla donde empieza el plano. |

[`vc4_regs.h`, `SCALER5_POS0_*`]

- Linux escribe `START_X` con la máscara antigua de 12 bits (`[11:0]`), aunque el campo de la BCM2711 tiene 14.
  [`vc4_plane.c`, `vc4_plane_mode_set`] Para anchos de hasta 4095 da igual.
- Las posiciones nunca son negativas. Linux recorta el plano contra la pantalla antes y ajusta el origen y el puntero.
  [`vc4_plane.c`, `vc4_plane_setup_clipping_and_scaling`, que usa el recorte genérico de DRM]
- Con volteo vertical, el puntero tiene que apuntar a la **última** línea visible del origen, no a la primera.
  [`vc4_plane.c`, `vc4_plane_mode_set`]
- Una rotación de 180 grados es volteo horizontal más vertical. Linux no ofrece rotaciones de 90 grados.
  [`vc4_plane.c`, `vc4_plane_init`]

### 5.4. `CTL2`: alfa (nueva en la BCM2711)

| Bits | Nombre | Significado |
|---|---|---|
| 31:30 | `ALPHA_MODE` | 0 alfa por píxel ("pipeline"). 1 alfa fijo. 2 fijo si el píxel no es cero. 3 fijo si el píxel supera `0x07`. |
| 29 | `ALPHA_PREMULT` | El color ya viene multiplicado por el alfa. |
| 28 | `ALPHA_MIX` | Combina el alfa por píxel con el alfa fijo del plano. |
| 25 | `ALPHA_LOC` | Sin descripción. Linux no lo usa. |
| 18:17 | `MAP_SEL` | Sin descripción. Linux no lo usa. |
| 16 | `GAMMA` | Sin descripción. Linux no lo usa. |
| 15:4 | `ALPHA` | Alfa fijo del plano, 12 bits. `0xFFF` es opaco. |

[`vc4_regs.h`, `SCALER5_CTL2_*`]

Cómo lo rellena Linux [`vc4_plane.c`, `vc4_hvs5_get_alpha_blend_mode` y `vc4_plane_mode_set`]:

| Caso | `ALPHA_MODE` | `PREMULT` |
|---|---|---|
| El formato no tiene alfa | 1 (fijo) | 0 |
| El usuario pide ignorar el alfa por píxel | 1 (fijo) | 0 |
| Alfa premultiplicado (el valor por defecto de DRM) | 0 (por píxel) | 1 |
| Alfa sin premultiplicar ("coverage") | 0 (por píxel) | 0 |

- `ALPHA` lleva siempre el alfa del plano. Linux lo tiene en 16 bits y se queda con los 12 de arriba.
- `ALPHA_MIX` solo se pone si el alfa del plano no es opaco y además el formato tiene alfa por píxel.
- Con un formato sin alfa y un alfa de plano menor que `0xFFF`, el plano entero queda translúcido. Se deduce de la
  tabla: el modo es "fijo" y el valor fijo es el del plano.

### 5.5. `POS1`: tamaño en pantalla

Solo existe si `UNITY` es 0.

| Bits | Nombre | Significado |
|---|---|---|
| 28:16 | `SCL_HEIGHT` | Alto en pantalla, en líneas. |
| 12:0 | `SCL_WIDTH` | Ancho en pantalla, en píxeles. |

[`vc4_regs.h`, `SCALER5_POS1_*`]

### 5.6. `POS2`: tamaño de origen

| Bits | Nombre | Significado |
|---|---|---|
| 28:16 | `HEIGHT` | Líneas que se leen del origen. |
| 12:0 | `WIDTH` | Píxeles por línea que se leen del origen. |

[`vc4_regs.h`, `SCALER5_POS2_*`]

- En la BCM2711 `POS2` ya no lleva el modo de alfa. En la BCM2835 sí lo llevaba, en los bits 31:28.
- Si el origen empieza o acaba en una fracción de píxel, Linux suma un píxel más por cada borde fraccionario
  (izquierdo, derecho, superior, inferior), salvo que ese borde sea el del propio búfer. [`vc4_plane.c`,
  `vc4_plane_mode_set`]
- Para YUV 4:4:4 en tres planos, Linux escribe el doble del ancho. El comentario dice que sin eso el hardware no lee
  todo el croma. [`vc4_plane.c`, `vc4_plane_mode_set`]

### 5.7. Punteros, pasos de línea y conversión de color

- **Punteros**: dirección de bus del primer píxel visible de cada plano de color. Pueden ser cualquier dirección de
  byte, sin alineación. [`vc4_plane.c`, `vc4_plane_mode_set`, comentario de "Pointer Word"]
- Para un búfer lineal, el puntero es la base del búfer más `y × paso + x × bytes_por_píxel`, donde `(x, y)` es la
  esquina del origen. En los planos de croma, `x` e `y` se dividen antes por el submuestreo horizontal y vertical.
  [`vc4_plane.c`, `vc4_plane_mode_set`, caso `DRM_FORMAT_MOD_LINEAR`]
- **`PITCH0`** para un búfer lineal: bits 15:0, paso de línea en bytes. Máximo 65535. [`vc4_regs.h`,
  `SCALER_SRC_PITCH_MASK`] Los bits de arriba a 0.
- **`PITCH1` y `PITCH2`**: igual, para el segundo y el tercer plano de color. Para los formatos SAND se repite el
  valor de `PITCH0`.
- **`CSC0..CSC2`** (solo YUV): tres palabras con la matriz de YCbCr a RGB. Los valores están en la sección 8.

### 5.8. Modos de escalado (`SCL0`, `SCL1`)

| Valor | Horizontal | Vertical |
|---|---|---|
| 0 | PPF | PPF |
| 1 | TPZ | PPF |
| 2 | PPF | TPZ |
| 3 | TPZ | TPZ |
| 4 | PPF | ninguno |
| 5 | ninguno | PPF |
| 6 | ninguno | TPZ |
| 7 | TPZ | ninguno |

[`vc4_regs.h`, `SCALER_CTL0_SCL_*`; `vc4_plane.c`, `vc4_get_scl_field`, que Linux dice que replica "una tabla de la
especificación"]

- Sin escalado en ninguna dirección, Linux escribe 0 y pone `UNITY`.
- PPF es un filtro polifásico. TPZ es un trapecio que promedia. Ver la sección 7.
- Para RGB, `SCL1` vale lo mismo que `SCL0`: los dos canales del escalador hacen lo mismo.
- Para YUV en varios planos, el **croma va en el canal 0** del escalador y la **luma en el canal 1**. `SCL0` lleva el
  modo del croma y `SCL1` el de la luma. [`vc4_plane.c`, `vc4_plane_mode_set`]

### 5.9. Palabras de escalado

Primero va la palabra `LBM`, si hay escalado vertical. Su valor es el desplazamiento, en palabras de LBM, de la zona
reservada (sección 7.5). [`vc4_plane.c`, `vc4_plane_allocate_lbm`]

Después van los parámetros de cada canal del escalador. Para YUV, primero los del croma y después los de la luma.
Para RGB, solo un juego. Dentro de un canal, en este orden y solo los que aplican [`vc4_plane.c`,
`vc4_write_scaling_parameters`]:

| Bloque | Palabras |
|---|---|
| PPF horizontal | 1: parámetros PPF |
| PPF vertical | 2: parámetros PPF y una de contexto |
| TPZ horizontal | 2: escala y recíproco |
| TPZ vertical | 3: escala, recíproco y una de contexto |

Si algún canal usa PPF en alguna dirección, al final van **cuatro** palabras iguales: el puntero al núcleo del filtro
para PPF horizontal del plano 0, PPF vertical del plano 0, PPF horizontal del plano 1 y PPF vertical del plano 1.
Linux las escribe siempre las cuatro, aunque no todas se usen. [`vc4_plane.c`, `vc4_plane_mode_set`]

Formato de la palabra de puntero al núcleo [`vc4_regs.h`, `SCALER_PPF_KERNEL_*`]:

| Bits | Nombre | Significado |
|---|---|---|
| 31 | `UNCACHED` | Sin descripción. Linux no lo usa. |
| 13:0 | `KERNEL_OFFSET` | Índice, en palabras de la memoria de listas, del núcleo del filtro. |

### 5.10. Ejemplo (deducido)

Un plano de 1920 × 1080 en `XRGB8888` según la convención de DRM (sección 6.3), sin escalado, en `(0, 0)`, con paso
de 7680 bytes y el búfer en la dirección física `0x10000000`:

| Palabra | Valor | Por qué |
|---|---|---|
| `CTL0` | `0x4800D807` | `VALID`, `SIZE` = 8, `UNITY`, `ORDER` = 2, los dos `EXPAND`, formato 7 |
| `POS0` | `0x00000000` | Posición (0, 0) |
| `CTL2` | `0x4000FFF0` | Alfa fijo, valor `0xFFF` |
| `POS2` | `0x04380780` | 1080 líneas, 1920 píxeles |
| `POS3` | `0xC0C0C0C0` | Contexto |
| `PTR0` | `0xD0000000` | `0x10000000` + `0xC0000000` |
| Contexto | `0xC0C0C0C0` | Contexto |
| `PITCH0` | `0x00001E00` | 7680 bytes |
| Fin | `0x80000000` | `END` |

Este ejemplo se ha calculado a partir de las tablas. No se ha probado.

## 6. Formatos de píxel

### 6.1. Códigos

[`vc4_regs.h`, `enum hvs_pixel_format`]

| Código | Formato | Bits por píxel |
|---|---|---|
| 0 | RGB332 | 8 |
| 1 | RGBA4444 | 16 |
| 2 | RGB555 | 16 |
| 3 | RGBA5551 | 16 |
| 4 | RGB565 | 16 |
| 5 | RGB888 | 24 |
| 6 | RGBA6666 | 24 |
| 7 | RGBA8888 | 32 |
| 8 | YCbCr 4:2:0, tres planos | 12 |
| 9 | YCbCr 4:2:0, dos planos | 12 |
| 10 | YCbCr 4:2:2, tres planos | 16 |
| 11 | YCbCr 4:2:2, dos planos | 16 |
| 12 | "H264": YCbCr 4:2:0 de dos planos en columnas SAND | 12 |
| 13 | Paleta | ? |
| 14 | YUV444 o RGB | ? |
| 15 | AYUV444 o RGB | ? |
| 16 | RGBA1010102 | 32 |
| 17 | YCbCr de 10 bits (P030, en SAND de 128 bytes) | ? |

- Los códigos 16 y 17 necesitan el campo de 5 bits. Solo existen en la BCM2711 y posteriores.
- Linux nunca usa 2, 6, 13, 14 ni 15. No se sabe cómo funcionan.

### 6.2. Orden de componentes (`ORDER`)

[`vc4_regs.h`, `HVS_PIXEL_ORDER_*`]

| Valor | Formatos con alfa (332, 4444, 555, 5551, 6666, 8888, 1010102) | Formatos 565 y 888 | Formatos YCbCr |
|---|---|---|---|
| 0 | RGBA | XBRG | XYCbCr |
| 1 | BGRA | XRBG | XYCrCb |
| 2 | ARGB | XRGB | YXCbCr |
| 3 | ABGR | XBGR | YXCrCb |

Los nombres de la tabla se leen del componente más alto al más bajo de una palabra **little-endian** (sección 6.3).
Linux lo dice así: "el bit menos significativo es el carácter de la derecha".

### 6.3. Qué formatos usa Linux en la BCM2711 y cómo están en memoria

Los formatos de DRM se definen como un número little-endian. Por ejemplo, `XRGB8888` es una palabra de 32 bits con X
en los bits 31:24, R en 23:16, G en 15:8 y B en 7:0, guardada en orden little-endian. En memoria, byte a byte, queda
B, G, R, X.

[`vc4_plane.c`, tabla `hvs_formats`, columna `pixel_order_hvs5`]

| Formato DRM | Bytes en memoria (del primero al último) | Código | `ORDER` en la BCM2711 |
|---|---|---|---|
| `XRGB8888`, `ARGB8888` | B, G, R, A | 7 | 2 (ARGB) |
| `XBGR8888`, `ABGR8888` | R, G, B, A | 7 | 3 (ABGR) |
| `RGB565` | palabra LE de 16 bits, R arriba | 4 | 2 (XRGB) |
| `BGR565` | palabra LE de 16 bits, B arriba | 4 | 3 (XBGR) |
| `XRGB1555`, `ARGB1555` | palabra LE de 16 bits | 3 | 2 (ARGB) |
| `RGB888` | B, G, R | 5 | 2 (XRGB) |
| `BGR888` | R, G, B | 5 | 3 (XBGR) |
| `XRGB2101010`, `ARGB2101010` | palabra LE de 32 bits | 16 | 2 (ARGB) |
| `XBGR2101010`, `ABGR2101010` | palabra LE de 32 bits | 16 | 3 (ABGR) |
| `RGB332` | 1 byte, R arriba | 0 | 2 (ARGB) |
| `BGR233` | 1 byte, B arriba | 0 | 3 (ABGR) |
| `XRGB4444`, `ARGB4444` | palabra LE de 16 bits | 1 | 2 (ARGB) |
| `XBGR4444`, `ABGR4444` | palabra LE de 16 bits | 1 | 3 (ABGR) |
| `RGBX4444`, `RGBA4444` | palabra LE de 16 bits | 1 | 0 (RGBA) |
| `BGRX4444`, `BGRA4444` | palabra LE de 16 bits | 1 | 1 (BGRA) |
| `YUV420`, `YVU420` | tres planos | 8 | 0 o 1 |
| `NV12`, `NV21` | dos planos | 9 | 0 o 1 |
| `YUV422`, `YVU422` | tres planos | 10 | 0 o 1 |
| `NV16`, `NV61` | dos planos | 11 | 0 o 1 |
| `YUV444`, `YVU444` | tres planos | **10** (sí, el de 4:2:2) | 0 o 1 |
| `NV12` en SAND | dos planos en columnas | 12 | 0 o 1 |
| `P030` en SAND 128 | dos planos, 10 bits | 17 | 0 |

- En los YUV, `ORDER` 0 es Cb antes que Cr y `ORDER` 1 al revés.
- Linux trata `XRGB` y `ARGB` igual en el formato. La diferencia está en `CTL2`: sin alfa, el modo es "fijo" y el byte
  X se ignora. Se deduce.
- **Diferencia con la BCM2835**: allí el `ORDER` de los formatos de 32 y 16 bits con alfa va al revés (`XRGB8888` usa
  3, `XBGR8888` usa 2). [`vc4_plane.c`, columna `pixel_order`] No hay que copiar valores de documentación de la Pi 3.
- Formatos y organizaciones que Linux acepta: `T-tiled` solo para los RGB de 32 y 16 bits. SAND 64, 128 y 256 solo
  para `NV12` y `NV21`. SAND 128 para `P030`. El resto, solo lineal. [`vc4_plane.c`, `vc4_format_mod_supported`]

### 6.4. Píxeles en una CPU big-endian (deducido)

Nada de esto está en Linux. Es lo que se deduce de la tabla anterior para DashOS.

- El HVS lee la RAM como little-endian. No hay en Linux ningún bit para cambiarlo.
- **32 bits por píxel**: si la CPU guarda una palabra nativa `0xAARRGGBB`, en memoria queda A, R, G, B. Leída como
  little-endian es B en lo alto y A en lo bajo, o sea "BGRA". Con código 7 y `ORDER` 1 (BGRA), el HVS debería leer
  bien los píxeles ARGB nativos de DashOS sin dar la vuelta a nada. Igual con `ORDER` 0 (RGBA) para palabras nativas
  `0xAABBGGRR`. Linux no usa los `ORDER` 0 y 1 con el código 7, así que esto no está comprobado.
- **24 bits por píxel** (código 5): el formato va byte a byte y el orden de la CPU no influye. Bytes B, G, R con
  `ORDER` 2. Bytes R, G, B con `ORDER` 3.
- **16 bits por píxel y 10 bits por componente**: los campos cruzan la frontera entre bytes. Ningún `ORDER` arregla
  una palabra con los bytes invertidos. Hay que guardar esos píxeles en little-endian, con los bytes dados la vuelta
  respecto al orden nativo de DashOS.
- **8 bits por píxel** (RGB332): un byte; el orden de la CPU no influye.

## 7. Escalado

### 7.1. Cuándo se escala y con qué

Linux decide por separado para cada dirección y cada plano de color. Con `orig` el tamaño de origen y `dest` el de
pantalla [`vc4_plane.c`, `vc4_get_scaling_mode`]:

- Si `dest` es igual a `orig` (en píxeles enteros): sin escalado.
- Si no, y `3 × dest ≥ 2 × orig`: PPF. Eso incluye cualquier ampliación y las reducciones hasta 1,5 veces.
- Si no: TPZ. Es decir, reducciones de más de 1,5 veces.

Casos especiales [`vc4_plane.c`, `vc4_plane_setup_clipping_and_scaling`]:

- Para YUV, el croma tiene siempre escalado PPF en las dos direcciones, aunque la proporción le salga 1:1. Sin eso el
  hardware no convierte bien (un 4:2:2 a 1:1 no se ve).
- El bit `UNITY` solo se pone si el plano 0 no se escala en ninguna dirección. Un YUV nunca es `UNITY` porque su
  croma siempre se escala.

Los tamaños de origen se manejan en coma fija 16.16 (16 bits de entero y 16 de fracción). Así lo hace DRM y así
llegan a las fórmulas siguientes.

### 7.2. Palabra PPF

| Bits | Nombre | Significado |
|---|---|---|
| 31 | `NOINTERP` | Usa solo 8 fases, sin interpolar a 64. Linux dice que hace falta para "vecino más cercano". No lo usa. |
| 30 | `AGC` | Cambia el coeficiente mayor para que los cuatro sumen la unidad. Linux siempre lo pone. |
| 24:8 | `SCALE` | Proporción origen/destino en coma fija con 16 bits de fracción. |
| 6:0 | `IPHASE` | Fase inicial, en complemento a dos de 7 bits, en 1/64 de píxel. |

[`vc4_regs.h`, `SCALER_PPF_*`]

Cómo calcula Linux los campos [`vc4_plane.c`, `vc4_write_ppf`]:

1. `SCALE` es el tamaño de origen en 16.16 dividido entre el tamaño de destino entero. El resultado ya está en 16.16.
2. Para el plano 0 (RGB o luma), Linux pone a 0 el bit más bajo de `SCALE`. Así la escala de la luma casa con la del
   croma.
3. Fase base para el plano 0: la parte fraccionaria del origen, pasada a 1/64 de píxel, menos medio píxel (32).
4. Fase base para el croma: la posición de origen en 1/64 de píxel tomando 17 bits, dividida entre 2, menos un cuarto
   de píxel (16).
5. Se calcula el error de redondeo de la escala: origen menos destino por `SCALE`, pasado a 1/64 de píxel. A la fase
   base se le suma la mitad de ese error.
6. Si la fase sale 64 o más, se deja en 63. Después se toman los 7 bits bajos. Los negativos quedan en complemento a
   dos.

### 7.3. Palabras TPZ

| Palabra | Bits | Nombre | Significado |
|---|---|---|---|
| 0 | 31 | `VERT_RECALC` | Sin descripción. Linux lo deja a 0. |
| 0 | 28:8 | `SCALE` | Proporción origen/destino en 16.16. |
| 0 | 7:0 | `IPHASE` | Fase inicial. Linux escribe 0. |
| 1 | 15:0 | `RECIP` | Recíproco de la escala. |

[`vc4_regs.h`, `SCALER_TPZ*`; `vc4_plane.c`, `vc4_write_tpz`]

- `SCALE` se calcula como en PPF, pero sin borrar el bit bajo.
- `RECIP` es `0xFFFFFFFF` dividido entre `SCALE`, con división entera. Linux cita la especificación: el recíproco
  "sería 2^32 entre la escala, pero `0xFFFFFFFF` se acerca bastante".
- Con 21 bits y 16 de fracción, la reducción máxima que cabe es menor que 32 veces. Se deduce del tamaño del campo.

### 7.4. Núcleo del filtro PPF

- El núcleo vive en la memoria de listas. Ocupa 11 palabras. Linux lo sube una sola vez al arrancar y lo comparten
  todos los planos. [`vc4_hvs.c`, `vc4_hvs_upload_linear_kernel` y `vc4_hvs_bind`]
- Como es lo primero que se reserva tras la zona del firmware, en Linux queda en las palabras 32 a 42. Se deduce del
  asignador; Linux no lo fija.
- Cada palabra lleva tres coeficientes de 9 bits con signo (complemento a dos): el primero en `[8:0]`, el segundo en
  `[17:9]` y el tercero en `[26:18]`. Los bits 31:27 a 0.
- El núcleo tiene 32 coeficientes. Linux usa uno de "fase lineal", simétrico. Se describe con 16 valores `c0..c15`:
  - Palabras 0 a 4: `(c0, c1, c2)`, `(c3, c4, c5)`, `(c6, c7, c8)`, `(c9, c10, c11)`, `(c12, c13, c14)`.
  - Palabra 5: `(c15, c15, 0)`.
  - Palabras 6 a 10: copias de las palabras 4, 3, 2, 1 y 0, en ese orden. Las palabras se copian tal cual, sin
    reordenar los coeficientes dentro de cada una.
- Según Linux, los coeficientes van del 0 al 16 subiendo, después un hueco y luego del 17 al 31 bajando, "invertidos
  dentro de cada palabra". El código no invierte nada dentro de las palabras. Ver "Lo que no está claro".
- Valores que usa Linux: el filtro de Mitchell y Netravali con B = C = 1/3, muestreado. `c0..c15` = 0, −2, −6, −8,
  −10, −8, −3, 2, 18, 50, 82, 119, 155, 187, 213, 227. Son datos de un filtro publicado y se pueden recalcular desde
  su fórmula.

### 7.5. LBM (memoria de líneas)

- El LBM guarda las líneas de origen que necesita el escalado vertical. Solo hace falta con escalado vertical en algún
  canal del escalador. [`vc4_plane.c`, `vc4_lbm_size`]
- En la BCM2711 tiene 60 K palabras. Cada palabra guarda 4 píxeles de 12 bits. [`vc4_hvs.c`, `__vc4_hvs_alloc`] En la
  BCM2835 eran 48 K palabras de 2 píxeles.
- Linux reparte el LBM entre todos los planos de todos los canales, sin reutilizar zonas. Cada plano con escalado
  vertical tiene su zona propia mientras exista.
- La palabra `LBM` de la entrada lleva el desplazamiento de la zona en palabras de LBM.

Tamaño que reserva Linux en la BCM2711, en palabras [`vc4_plane.c`, `__vc4_lbm_size` y `vc4_plane_allocate_lbm`]:

1. Píxeles por línea: el ancho de destino si la reducción horizontal es TPZ; si no, el ancho de origen.
2. Bytes: para RGB con escalado vertical TPZ, píxeles por 8. En cualquier otro caso (RGB con PPF vertical, o YUV),
   píxeles por 16.
3. Se redondea hacia arriba a múltiplo de 128 bytes.
4. Se divide entre 4.
5. Se redondea hacia arriba a múltiplo de 64 palabras.

Ejemplo deducido: un plano de 1920 píxeles de ancho con PPF vertical necesita 1920 × 16 = 30720 bytes, 7680
palabras. El LBM entero da para unos ocho planos así.

Linux avisa en un comentario de que el factor puede bajar en algunos casos (12 en vez de 16, o hasta 2 para YUV),
pero que "la tabla de la documentación está algo mal según el código del firmware". Usa los valores seguros.

## 8. Conversión de color YUV

Para los formatos YUV, la entrada lleva tres palabras con la matriz. [`vc4_regs.h`, `SCALER_CSC*`; `vc4_plane.c`,
tabla `colorspace_coeffs`]

| Norma | Rango | `CSC0` | `CSC1` | `CSC2` |
|---|---|---|---|---|
| BT.601 | limitado | `0x00F00000` | `0xE73304A8` | `0x00066604` |
| BT.709 | limitado | `0x00F00000` | `0xF27784A8` | `0x00072E1D` |
| BT.2020 | limitado | `0x00F00000` | `0xF43594A8` | `0x0006B624` |
| JFIF (BT.601) | completo | `0x00000000` | `0xEA349400` | `0x00059DC6` |
| BT.709 | completo | `0x00000000` | `0xF4388400` | `0x00064DDB` |
| BT.2020 | completo | `0x00000000` | `0xF5B6D400` | `0x0005E5E2` |

Linux usa BT.709 limitado por defecto. Con valores no válidos cae a BT.601 limitado. [`vc4_plane.c`,
`vc4_plane_init` y `vc4_plane_mode_set`]

Campos, según Linux. Los coeficientes son S2.8: 2 bits de entero y 8 de fracción. Los dos bits de entero valen 0, 1,
2 o −1 (para `00`, `01`, `10` y `11`).

| Palabra | Bits | Campo |
|---|---|---|
| `CSC0` | 31:24 | 8 bits bajos de la contribución de Cr al azul |
| `CSC0` | 23:16 | Desplazamiento con signo que se suma a Y |
| `CSC0` | 15:8 | Desplazamiento con signo para Cb (Cb' = Cb − 128 + este valor) |
| `CSC0` | 7:0 | Desplazamiento con signo para Cr |
| `CSC1` | 31:22 | Contribución de Cb al verde |
| `CSC1` | 21:12 | Contribución de Cr al verde |
| `CSC1` | 11:2 | Contribución de Y a R, G y B |
| `CSC1` | 1:0 | 2 bits altos de la contribución de Cr al azul |
| `CSC2` | 29:20 | Contribución de Cb al rojo (ver la nota) |
| `CSC2` | 19:10 | Contribución de Cr al rojo, o de Cb al azul (ver la nota) |

Nota: la cabecera de Linux da el mismo hueco `[19:10]` a dos campos. No se puede saber cuál es el bueno. Para
implementar basta con copiar las tres palabras de la tabla.

## 9. Mezcla y fondo

- Cada canal compone sus planos en el orden de la lista. El primero queda abajo.
- Debajo del primer plano está el fondo del canal, si `DISPBKGND.FILL` está a 1. El color sale de `DISPBKGND[23:0]`.
- Linux calcula si el plano de abajo necesita fondo: lo necesita si tiene alfa por píxel, si no cubre toda la
  pantalla o si su alfa de plano no es opaco. [`vc4_plane.c`, `vc4_plane_mode_set`, `needs_bg_fill`] En la BCM2711
  no usa ese cálculo: el fondo va siempre activado (sección 3.7).
- Sin fondo y con huecos sin planos, no se sabe qué sale. Se deduce que lo seguro es dejar `FILL` siempre a 1.

## 10. Canales, COB y salidas

### 10.1. Las seis salidas del HVS en la BCM2711

El HVS tiene tres canales y seis salidas. Cada salida va a un consumidor fijo. [`vc4_crtc.c`, tablas
`bcm2711_pv*_data`; `vc4_txp.c`, `bcm2835_txp_data`; `vc4_kms.c`, comentario de `vc4_pv_muxing_atomic_check`]

| Salida | Consumidor | Dirección (bus) | IRQ (SPI) | Canales posibles | Cómo se elige |
|---|---|---|---|---|---|
| 0 | PV0 (DSI0 o DPI) | `0x7E206000` | 109 | solo 0 | Fijo |
| 1 | PV3 (VEC, vídeo compuesto) | `0x7EC12000` | 106 | solo 1 | Fijo |
| 2 | TXP (escritura a memoria) | `0x7E004000` | 75 | solo 2 | `DISPECTRL[31]` |
| 3 | PV1 (DSI1 o SMI) | `0x7E207000` | 110 | 0, 1 o 2 | `DISPCTRL[19:18]` |
| 4 | **PV2 → HDMI0** | `0x7E20A000` | 101 | 0, 1 o 2 | `DISPEOLN[31:30]` |
| 5 | **PV4 → HDMI1** | `0x7E216000` | 110 | 0, 1 o 2 | `DISPDITHER[31:30]` |

- Las direcciones e interrupciones salen del DTB y de `bcm2711.dtsi`.
- **PV1 y PV4 comparten la interrupción SPI 110.** El manejador tiene que mirar las dos. Se deduce del DTB.
- PV2 (HDMI0) tiene una FIFO de 256 bytes. Las demás, de 64. PV2 y PV4 sacan 2 píxeles por ciclo; las demás, 1.
  [`vc4_crtc.c`, `bcm2711_pv2_data` y `bcm2711_pv4_data`]

### 10.2. COB: memoria de las FIFO

- En la BCM2711 el COB mide 44416 píxeles, unas 10,8 líneas de 4096. Los primeros 4096 píxeles guardan RGBA
  completo; el resto, solo RGB. Esos 4096 son para el TXP, que escribe RGBA en memoria. [`vc4_hvs.c`,
  `vc4_hvs_cob_init`]
- Linux lo reparte así al arrancar y no lo cambia más [`vc4_hvs.c`, `vc4_hvs_cob_init`]:

| Canal | `BASE` | `TOP` | Valor de `DISPBASEx` | Tamaño (según 3.9) |
|---|---|---|---|---|
| 2 | 0 | 12288 | `0x30000000` | 12292 píxeles, 3 líneas de 4096 |
| 1 | 12304 | 24592 | `0x60103010` | 12292 píxeles, 3 líneas de 4096 |
| 0 | 24608 | 44416 | `0xAD806020` | 19812 píxeles, algo más de 4 líneas de 4096 |

- Entre zonas Linux deja 16 píxeles libres.
- En la BCM2711 Linux escribe `TOP` tal cual. En la BCM2835 escribía `TOP − 1`. No se sabe cuál es la semántica
  exacta en la BCM2711 (último píxel incluido o primero excluido).
- El canal 2 se queda con la zona RGBA porque es el único que puede alimentar al TXP.
- Linux usa el tamaño del COB en líneas para estimar la posición del barrido (sección 13.3).

### 10.3. Mux de salidas

[`vc4_kms.c`, `vc5_hvs_pv_muxing_commit`; `vc4_hvs.c`, `vc4_hvs_get_fifo_from_output`]

- Salidas 3, 4 y 5: el campo de 2 bits vale el número de canal (0, 1 o 2). El valor 3 desconecta la salida.
- Salida 2: el bit vale 0 para el canal 2. Linux escribe 1 cuando el canal no es el 2, y al leer interpreta el 1 como
  "canal 0". En la práctica el TXP siempre usa el canal 2.
- Salidas 0 y 1: sin mux.
- Al arrancar, Linux conecta la salida 2 al canal 2 y desconecta las salidas 3, 4 y 5. [`vc4_hvs.c`,
  `vc4_hvs_hw_init`]
- Linux comprueba que la salida 2 y la salida 3 no usen a la vez el mismo canal. Se deduce que un canal alimenta una
  sola salida.

### 10.4. Cómo reparte Linux los canales

[`vc4_kms.c`, `vc4_pv_muxing_atomic_check`]

1. Ordena las PV por número de salida, de menor a mayor.
2. A cada PV que se enciende le da el canal libre más bajo de los que puede usar.
3. La PV conserva su canal mientras siga encendida. Solo lo suelta al apagarse. Cambiar de canal exige apagar y
   encender la PV.
4. Si no queda canal compatible, la configuración se rechaza.

Consecuencias para la Pi 4, deducidas:

- Con solo HDMI0: canal 0. Con HDMI0 y HDMI1 encendidos a la vez: HDMI0 canal 0 y HDMI1 canal 1.
- Si se enciende HDMI1 solo, coge el canal 0. Si después se enciende HDMI0, coge el 1.
- Si HDMI0 y HDMI1 tienen los canales 0 y 1, la PV0 (DPI) y la PV3 (VEC) ya no pueden encenderse: solo admiten el 0
  y el 1.
- El canal 0 tiene más COB que los otros. Si se quiere que HDMI0 tenga siempre el canal 0, basta con encenderlo
  primero. Linux no garantiza eso.

### 10.5. Límites de las salidas HDMI

[`vc4_hdmi.c`, `vc4_hdmi_connector_clock_valid` y variantes `bcm2711_hdmi0_variant`, `bcm2711_hdmi1_variant`]

- HDMI0 admite hasta 600 MHz de reloj TMDS. Por encima de 340 MHz (HDMI 2.0) solo si el máximo del reloj `core` es al
  menos 550 MHz.
- HDMI1 admite solo hasta 340 MHz (HDMI 1.4).
- 4096 × 2160 a 50 Hz o más solo si el máximo del reloj `core` es al menos 600 MHz.
- Se deduce que la diferencia entre HDMI0 y HDMI1 viene de la FIFO de la PV (256 frente a 64 bytes).

## 11. Reloj `core` y carga

En la BCM2711 Linux ajusta el reloj `core` en cada cambio de estado. [`vc4_kms.c`, `vc4_core_clock_atomic_check`,
`vc4_load_tracker_atomic_check` y `vc4_atomic_commit_tail`; `vc4_plane.c`, `vc4_plane_calc_load`; `vc4_crtc.c`,
`vc4_crtc_atomic_check`]

- **Carga de un plano**: ancho por alto en pantalla, por la frecuencia de refresco. Se divide entre 2 si el plano se
  escala y entre 4 si no. El comentario lo explica: el HVS procesa 2 píxeles por ciclo al escalar y 4 si no.
- **Carga total**: la suma de todos los planos. Se toma el 40 % si hay más de un canal en uso y el 60 % si hay uno.
- **Carga de la FIFO de un canal**: para HDMI0, el mayor de dos valores: (reloj de píxel × ancho activo / ancho
  total + 8 MHz) y el 90 % del reloj de píxel. Para las demás salidas, el reloj de píxel.
- **Frecuencia necesaria**: la mayor entre la carga total corregida y la mayor carga de FIFO.
- **Durante el cambio**: Linux pide como mínimo el mayor entre la frecuencia vieja y la nueva, limitado entre 500 MHz
  y el máximo del firmware.
- **Al terminar el cambio**: pide la frecuencia necesaria nueva, sin pasar del máximo.
- El límite de ancho de banda de memoria (2 GB/s) solo se aplica en la BCM2835.

## 12. Secuencias

Todas las escrituras son de 32 bits y en little-endian (sección 2.5).

### 12.1. Tomar el HVS al firmware

[`vc4_drv.c`, `vc4_drm_bind`; `vc4_hvs.c`, `vc4_hvs_bind`; `vc4_crtc.c`, `vc4_crtc_disable_at_boot`]

1. Pedir al firmware `NOTIFY_DISPLAY_DONE` (`0x00030066`) sin datos. Linux lo hace antes de tocar nada. Si falla, solo
   avisa y sigue.
2. Leer el máximo del reloj `core` (`0x00030004`, reloj 4) y encender el reloj (`0x00038001`). Ver la sección 2.3.
3. Iniciar el HVS (12.2).
4. Subir el núcleo del filtro a la memoria de listas (7.4).
5. Repartir el COB (10.2).
6. Activar la interrupción del HVS en el GIC.
7. Para PV2 y PV4: si están encendidas y con vídeo activo, averiguar su canal por el mux y apagarlas con la secuencia
   de 12.6.

Durante todo esto Linux no toca las palabras 0 a 31 de la memoria de listas.

### 12.2. Iniciar el HVS

[`vc4_hvs.c`, `vc4_hvs_hw_init`]

1. `DISPCTRL`: poner `ENABLE`.
2. `DISPECTRL`: `DSP2_MUX` a 0.
3. `DISPCTRL`: `DSP3_MUX` a 3.
4. `DISPEOLN`: `DSP4_MUX` a 3.
5. `DISPDITHER`: `DSP5_MUX` a 3.
6. `DISPCTRL`: poner `DISPEIRQ(0..2)`. Quitar `DMAEIRQ`, `SLVEIRQ`, `SCLEIRQ` y todos los `DSPEIEOF`, `DSPEIEOLN` y
   `DSPEISLUR`. Poner los tres `PANIC` a 2.

Cada paso es lectura, modificación y escritura.

### 12.3. Encender un canal con una PV

Orden que sigue Linux al encender una salida [`vc4_kms.c`, `vc4_atomic_commit_tail`; `vc4_hvs.c`,
`vc4_hvs_atomic_flush`, `vc4_hvs_atomic_enable` y `vc4_hvs_init_channel`; `vc4_crtc.c`, `vc4_crtc_atomic_enable` y
`vc4_crtc_config_pv`]:

1. Subir el reloj `core` lo necesario (sección 11).
2. Elegir canal (10.4) y escribir el mux de la salida (10.3).
3. Reservar un bloque en la memoria de listas y escribir la lista completa, terminada en `END`.
4. Activar la interrupción de la PV: `PV_INTEN` con `VFP_START` (apéndice A).
5. Escribir en `DISPLISTx` el índice del bloque.
6. Reiniciar el canal: escribir en `DISPCTRLx` primero 0, después solo `RESET` y después 0.
7. Escribir en `DISPCTRLx`: `ENABLE`, `WIDTH` = ancho activo, `HEIGHT` = alto activo. `ONESHOT` solo para el TXP.
   Ejemplo deducido para 1920 × 1080: `0x87800438`. El canal queda esperando el VSTART de la PV para empezar.
8. `DISPBKGNDx`: leer, quitar `BCK2BCK`, `GAMMA` e `INTERLACE`, poner `INTERLACE` si el modo es entrelazado y
   escribir.
9. `DISPBKGNDx`: poner `FILL`. Linux lo hace en el paso 3, al escribir la lista, antes de encender el canal. El orden
   entre 8 y 9 da igual porque los dos leen, modifican y escriben.
10. Configurar la PV y encenderla (apéndice A). Configurar y encender el codificador HDMI (fuera de este documento).
11. Poner `VIDEN` en la PV.
12. Esperar al primer `VFP_START` y comprobar que `DISPLACTx` vale el índice escrito. Entonces activar el aviso de
    vaciado del canal (13.2).

### 12.4. Cambiar la lista sin desgarros

[`vc4_hvs.c`, `vc4_hvs_atomic_flush`, `vc4_hvs_install_dlist` y `vc4_hvs_update_dlist`; `vc4_crtc.c`,
`vc4_crtc_handle_page_flip`; `vc4_kms.c`, `vc4_atomic_commit_tail`]

1. Desactivar el aviso de vaciado del canal (`DSPEISLUR(c)` a 0). Linux lo hace al principio de cada cambio, porque
   el cambio puede provocar vaciados falsos.
2. Escribir la lista nueva en un bloque **libre** de la memoria de listas. Nunca encima de la lista activa.
3. Escribir en `DISPLISTx` el índice del bloque nuevo. Se puede hacer en cualquier momento del cuadro.
4. El hardware no cambia de lista al momento. Toma el valor de `DISPLISTx` al empezar a componer el cuadro siguiente.
   `DISPLACTx` dice qué lista está usando.
5. En cada interrupción `VFP_START` de la PV (inicio del borrado vertical), leer `DISPLACTx`.
   - Si vale el índice nuevo: el cambio está hecho. Se puede liberar el bloque viejo y reutilizar los búferes de
     píxeles viejos. Se reactiva el aviso de vaciado (13.2).
   - Si no: esperar al siguiente `VFP_START`.

Sobre el momento exacto del cambio (deducido de los comentarios de Linux en `vc4_crtc_get_scanout_position`):

- El HVS va por delante de la PV tantas líneas como quepan en su zona del COB.
- Al llegar al final del cuadro, el HVS se para y espera la señal VSTART de la PV.
- La PV da VSTART durante el borrado vertical. Entonces el HVS empieza el cuadro nuevo y llena la FIFO con las
  primeras líneas.
- Se deduce que el cambio de lista ocurre en ese VSTART, dentro del borrado vertical. Así la imagen no se desgarra.
- Si `DISPLISTx` se escribe justo cuando el HVS lo está tomando, el cambio puede quedar para el cuadro siguiente. La
  comprobación de `DISPLACTx` cubre ese caso.

Los búferes de píxeles de la lista vieja no se pueden tocar hasta que `DISPLACTx` muestre la lista nueva. Linux espera
a ese momento para soltarlos. [`vc4_kms.c`, `vc4_atomic_commit_tail`, espera de `flip_done` antes de limpiar los
planos]

### 12.5. Cambio rápido de búfer (sin esperar al borrado)

[`vc4_plane.c`, `vc4_plane_async_set_fb`, `vc4_plane_atomic_async_check` y `vc4_plane_atomic_async_update`]

- Linux permite reescribir en la lista **activa** solo tres palabras: `POS0`, `POS2` y `PTR0`. Lo usa para mover el
  cursor y para cambios de búfer asíncronos.
- Solo vale si la entrada nueva tiene el mismo tamaño y las mismas posiciones de esas palabras, el mismo tamaño de LBM,
  y todas las demás palabras iguales (salvo la del LBM).
- El HVS empieza a leer del puntero nuevo "en cuanto la FIFO necesite rellenarse". Se deduce que el cambio puede
  verse a mitad de cuadro, con desgarro. Sirve para el cursor, no para cambiar la imagen entera.

### 12.6. Apagar un canal

[`vc4_crtc.c`, `vc4_crtc_disable` y `vc4_crtc_pixelvalve_reset`; `vc4_hvs.c`, `__vc4_hvs_stop_channel`]

1. Desactivar la interrupción de la PV.
2. Quitar `VIDEN` en la PV y esperar hasta 1 ms a que se lea a 0.
3. **Esperar 20 ms.** En la BCM2711, sin esta espera puede quedar un píxel atascado en una FIFO entre la PV y el HDMI.
   Se nota porque toda la imagen sale desplazada un píxel a la derecha. Linux dice que el fallo sale en menos de 10
   cambios de modo si se quita la espera.
4. Apagar el codificador HDMI (fuera de este documento).
5. Reiniciar la PV: quitar `EN` y después poner `FIFO_CLR`.
6. Parar el canal del HVS, solo si `DISPCTRLx.ENABLE` está a 1: escribir `RESET` y después 0.
7. Al salir, Linux espera que `DISPCTRLx.RESET` esté a 0, que `DISPSTATx.MODE` sea DISABLED, que `EMPTY` esté a 1 y
   `FULL` a 0. Si no, solo avisa.
8. Desconectar el mux de la salida (3) y liberar el canal.
9. Bajar el reloj `core` si ya no hace falta tanto.

## 13. Interrupciones

### 13.1. Del HVS

- En la BCM2711 Linux solo usa la interrupción del HVS para detectar vaciados (underrun). No la usa para el fin de
  cuadro. [`vc4_hvs.c`, `vc4_hvs_irq_handler` y `vc4_hvs_hw_init`]
- Manejador de Linux:
  1. Leer `DISPSTAT` y `DISPCTRL`.
  2. Para cada canal: si `EUFLOW(c)` está a 1 en `DISPSTAT` y `DSPEISLUR(c)` está a 1 en `DISPCTRL`, desactivar
     `DSPEISLUR(c)` y contar un vaciado. Linux comprueba el permiso a mano porque "el enmascarado no siempre se
     respeta".
  3. Escribir en `DISPSTAT` los bits 8 a 13, 16 a 21 y 24 a 29 a 1 para borrar todos los eventos de los tres canales.

### 13.2. Activar y desactivar el aviso de vaciado

[`vc4_hvs.c`, `vc4_hvs_mask_underrun` y `vc4_hvs_unmask_underrun`]

- **Desactivar**: poner `DSPEISLUR(c)` a 0 en `DISPCTRL`.
- **Activar**: escribir `EUFLOW(c)` a 1 en `DISPSTAT` para borrar lo pendiente y después poner `DSPEISLUR(c)` a 1 en
  `DISPCTRL`.
- Linux lo desactiva al empezar cada cambio de estado y lo reactiva en el primer borrado vertical en que `DISPLACTx`
  ya muestra la lista nueva. Antes de eso el HVS aún usa la lista vieja con la PV ya reconfigurada, y salen vaciados
  falsos.

### 13.3. Sincronía vertical

- El "vblank" lo da la PV, no el HVS: la interrupción `VFP_START` (inicio del pórtico delantero vertical, que es el
  final de la zona activa). Se borra escribiendo el bit a 1 en `PV_INTSTAT`. [`vc4_crtc.c`, `vc4_enable_vblank` y
  `vc4_crtc_irq_handler`]
- Para estimar la línea que se está mostrando, Linux lee `DISPSTATx.LINE` (la línea que compone el HVS) y le resta las
  líneas que caben en el COB del canal más una. Si el HVS va por una línea menor que eso, se considera que está en el
  borrado. [`vc4_crtc.c`, `vc4_crtc_get_scanout_position`]

## 14. Límites y casos que Linux trata

| Tema | Dato | Fuente |
|---|---|---|
| Resolución máxima | 7680 × 7680 para la BCM2711 (2048 en la BCM2835) | [`vc4_kms.c`, `vc4_kms_load`] |
| Ancho y alto de canal, `POS1`, `POS2` | Campos de 13 bits: hasta 8191 | [`vc4_regs.h`] |
| `POS0.START_Y` | 12 bits: hasta 4095 | [`vc4_regs.h`, `SCALER5_POS0_START_Y_MASK`] |
| `DISPSTATx.LINE` | 12 bits | [`vc4_regs.h`] |
| Paso de línea | 16 bits: hasta 65535 bytes | [`vc4_regs.h`, `SCALER_SRC_PITCH_MASK`] |
| Tamaño de entrada | `SIZE` de 6 bits: hasta 63 palabras | [`vc4_regs.h`, `SCALER_CTL0_SIZE_MASK`] |
| Palabras de lista libres | 4064 (de la 32 a la 4095), menos 11 del filtro | [`vc4_hvs.c`, `__vc4_hvs_alloc`] |
| Planos | No hay límite fijo de hardware; lo pone la memoria de listas. Linux ofrece 1 principal y 1 cursor por salida, y 16 superpuestos compartidos | [`vc4_plane.c`, `vc4_plane_create_additional_planes`] |
| Alineación de punteros | Ninguna: cualquier byte | [`vc4_plane.c`, `vc4_plane_mode_set`] |
| Alineación del COB | 4 píxeles | [`vc4_regs.h`, comentario de `DISPBASEX`] |
| Alineación del LBM | 64 palabras | [`vc4_plane.c`, `vc4_plane_allocate_lbm`] |
| Plano de tamaño 0 | Se omite de la lista | [`vc4_plane.c`, `vc4_plane_mode_set` y `vc4_plane_atomic_check`] |
| Bordes fraccionarios | Un píxel más de origen por borde | [`vc4_plane.c`, `vc4_plane_mode_set`] |
| YUV 4:4:4 | Ancho de origen doble | [`vc4_plane.c`, `vc4_plane_mode_set`] |
| Croma YUV | Siempre con PPF en las dos direcciones | [`vc4_plane.c`, `vc4_plane_setup_clipping_and_scaling`] |
| Volteo vertical | El puntero apunta a la última línea | [`vc4_plane.c`, `vc4_plane_mode_set`] |
| Modos de doble barrido | Rechazados | [`vc4_crtc.c`, `vc4_crtc_mode_valid`] |
| Una PV, un codificador | Una salida no puede alimentar dos conectores | [`vc4_hvs.c`, `vc4_hvs_atomic_check`] |
| Píxel atascado al apagar | Espera de 20 ms tras quitar `VIDEN` | [`vc4_crtc.c`, `vc4_crtc_disable`] |
| FIFO de PV4 (HDMI1) | Nivel de llenado fijo en 32; la fórmula general no funciona | [`vc4_crtc.c`, `vc4_get_fifo_full_level`] |
| Escrituras en la lista | Siempre de 32 bits | [`vc4_plane.c`, `vc4_plane_write_dlist`] |
| Reescribir la lista activa | Solo `POS0`, `POS2` y `PTR0` | [`vc4_plane.c`, `vc4_plane_atomic_async_update`] |
| Zona del firmware | Palabras 0 a 31 de la lista, intactas | [`vc4_hvs.c`, `__vc4_hvs_alloc`] |

## 15. Diferencias con las otras generaciones

| Tema | BCM2835 (`GEN_4`) | BCM2711 (`GEN_5`) | BCM2712 (`GEN_6`) |
|---|---|---|---|
| Memoria de listas | `0x2000`, 16 KB | `0x4000`, 16 KB | `0x4000`, tamaño en un registro |
| Registros de canal | `DISPCTRLx` y demás | Los mismos, otros campos | Otro mapa (`SCALER6_*`) |
| Salidas y mux | 3, casi fijas | 6, mux en cuatro registros | Distinto |
| `CTL2` en la entrada | No | Sí | Sí |
| Volteos | En `CTL0` | En `POS0` | En `POS0` y `PTR0` |
| `ORDER` de 32 bits | Al revés | Como en la tabla 6.3 | Como la BCM2711 |
| COB | 20736 píxeles | 44416 píxeles | Otro reparto |
| LBM | 48 K palabras de 2 píxeles | 60 K palabras de 4 píxeles | Otro sistema |
| Gamma y matriz de salida | Sí | Linux no las usa | No |
| Interrupción del HVS | Sí | Sí | Linux no la usa |
| Punteros | 32 bits | 32 bits | 40 bits |

[`vc4_regs.h`; `vc4_hvs.c`; `vc4_plane.c`]

## Apéndice A. Pixel valve: lo mínimo para el HVS

La PV merece su propia especificación. Aquí solo está lo que hace falta para las secuencias de este documento.
[`vc4_regs.h`, `PV_*`; `vc4_crtc.c`, `vc4_crtc_config_pv`]

| Desp. | Registro | Campos usados |
|---|---|---|
| `0x00` | `PV_CONTROL` | 0 `EN`; 1 `FIFO_CLR`; 3:2 `CLK_SELECT`; 5:4 `PIXEL_REP`; 12 `WAIT_HSTART`; 13 `TRIGGER_UNDERFLOW`; 14 `CLR_AT_START`; 20:15 `FIFO_LEVEL`; 23:21 `FORMAT`; 26:25 `FIFO_LEVEL_HIGH` |
| `0x04` | `PV_V_CONTROL` | 0 `VIDEN`; 1 `CONTINUOUS`; 4 `INTERLACE` |
| `0x08` | `PV_VSYNCD_EVEN` | Retardo de sincronía del campo par |
| `0x0C` | `PV_HORZA` | 31:16 pórtico trasero horizontal; 15:0 sincronía horizontal |
| `0x10` | `PV_HORZB` | 31:16 pórtico delantero horizontal; 15:0 ancho activo |
| `0x14` | `PV_VERTA` | 31:16 pórtico trasero vertical; 15:0 sincronía vertical |
| `0x18` | `PV_VERTB` | 31:16 pórtico delantero vertical; 15:0 alto activo |
| `0x24` | `PV_INTEN` | Permiso de interrupciones |
| `0x28` | `PV_INTSTAT` | Estado; bit 7 `VFP_START`; se borra escribiendo 1 |
| `0x34` | `PV_MUX_CFG` | 5:2: Linux escribe 8 ("sin intercambio") en la BCM2711 |

Configuración para HDMI que hace Linux:

1. Reiniciar la PV: quitar `EN` y poner `FIFO_CLR`.
2. Escribir los tiempos. Los horizontales se **dividen entre 2**, porque PV2 y PV4 sacan 2 píxeles por ciclo. Los
   verticales van tal cual.
3. `PV_V_CONTROL`: `CONTINUOUS` (más `INTERLACE` si toca).
4. `PV_MUX_CFG`: 8 en los bits 5:2.
5. `PV_CONTROL`: `FIFO_CLR`, nivel de llenado, `FORMAT` 0 (24 bits), `PIXEL_REP` 0, `CLR_AT_START`,
   `TRIGGER_UNDERFLOW`, `WAIT_HSTART` y `CLK_SELECT` 0.
6. Poner `EN`. Después se configura el codificador y al final se pone `VIDEN`.

Nivel de llenado de la FIFO de la PV, en bytes [`vc4_crtc.c`, `vc4_get_fifo_full_level` y
`vc4_crtc_get_fifo_full_level_bits`]:

- PV2 (HDMI0): profundidad menos 18, es decir 256 − 18 = 238. Los 6 bits bajos van a `FIFO_LEVEL` y el resto a
  `FIFO_LEVEL_HIGH`.
- PV4 (HDMI1): 32, siempre.

## Lo que no está claro

1. **Momento exacto en que el HVS toma `DISPLISTx`.** Se deduce que es al VSTART de la PV, dentro del borrado
   vertical. Linux no lo dice con esas palabras. Hay que medirlo.
2. **Píxeles big-endian en 32 bits.** La deducción de que `ORDER` 1 o 0 con el código 7 leen bien palabras nativas
   de DashOS sale de la regla de nombres de la tabla 6.2. Linux nunca usa esa combinación. Hay que probarlo.
3. **No hay bit de intercambio de bytes** conocido. Si existe, Linux no lo usa.
4. **Semántica de `DISPBASEx.TOP`** en la BCM2711: Linux escribe el valor sin restar 1, al revés que en la BCM2835.
5. **Codificación de `PANIC`**: el valor 2 se usa sin explicar qué significa.
6. **`BCK2BCK`**, `ALPHA_LOC`, `MAP_SEL`, `CTL2.GAMMA`, `TPZ0.VERT_RECALC`, `ONECTX`, `DISPALPHA2`, `DISPLSTAT`,
   `DISPID`: sin descripción en Linux.
7. **`ALPHA_EXPAND` y `RGB_EXPAND`**: no se sabe qué hacen exactamente con cada valor.
8. **Contenido de las palabras de contexto**: Linux escribe `0xC0C0C0C0` sin explicar si el valor inicial importa.
9. **Núcleo del filtro**: el comentario de Linux habla de coeficientes "invertidos dentro de cada palabra", pero el
   código copia las palabras sin invertir nada. Para un núcleo simétrico como el que usa Linux puede dar igual. Para
   otro núcleo no se sabe cuál es la disposición correcta.
10. **Campo `CSC2[19:10]`**: Linux le asigna dos significados distintos.
11. **Gamma en la BCM2711**: no se sabe si funciona ni con qué formato. Linux la deja apagada.
12. **Color de fondo**: Linux nunca escribe `DISPBKGNDx[23:0]`. No se sabe qué valor deja el firmware ni qué valor
    tiene tras un reinicio.
13. **`START_X`**: Linux usa una máscara de 12 bits sobre un campo de 14. No se sabe si los bits 13:12 funcionan.
14. **`DISPSTATx.LINE`** tiene 12 bits pero los modos llegan a 7680 líneas. No se sabe si en la BCM2711 el campo es
    más ancho.
15. **Contadores de cuadros**: que el del canal 0 esté en `DISPSTAT1` es lo que lee Linux. No se ha comprobado.
16. **Traspaso desde el firmware**: al iniciar el HVS, Linux desconecta las salidas 3, 4 y 5 (mux a 3). Después,
    para apagar las salidas HDMI que dejó el firmware, lee ese mismo mux para encontrar su canal, y lo encuentra
    desconectado. Parece que en la BCM2711 ese apagado no llega a hacerse. No se sabe si es un fallo de Linux o si el
    orden real de arranque es otro.
17. **Reutilizar la configuración del firmware**: se deduce que DashOS podría dejar la PV y el HDMI como los dejó el
    firmware y cambiar solo la lista del canal. Linux no lo hace. No se sabe si el firmware sigue escribiendo en el
    HVS después de `NOTIFY_DISPLAY_DONE` o sin ella.
18. **Alcance de memoria**: que el HVS solo vea el primer GB sale del `dma-ranges` del DTB, no de una prueba.
19. **Coherencia de cachés**: se deduce que el HVS no ve las cachés de la CPU. No está escrito en Linux.
20. **Modo de direcciones de periféricos**: la dirección física `0xFE400000` vale con el modo bajo. No se ha
    comprobado qué modo deja Emu68 en DashOS.
21. **QEMU `raspi4b`**: no se ha comprobado si emula el HVS. Lo más probable es que no, y que esto solo se pueda
    probar en una Pi 4 real.
22. **Formatos 2, 6, 13, 14, 15**: Linux no los usa y no se sabe cómo se configuran.
23. **Valor de `DSP2_MUX` a 1**: Linux lo interpreta como "canal 0", pero nunca lo usa en la práctica.
