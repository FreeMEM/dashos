# Pixel valves y salidas HDMI de la BCM2711 (Raspberry Pi 4)

Especificación de sala limpia. La escribe el analista leyendo el controlador vc4 de Linux y los controladores que lo
rodean. Quien implemente no necesita ni debe leer ese código: todo lo necesario tiene que estar aquí o en
[`hvs.md`](hvs.md).

## 0. Origen y convenciones

- **Fuente**: `torvalds/linux`, commit `6628e7d4c6b25c8d18f1345e3a20d21829aae7e4` (1 de noviembre de 2026).
  Ficheros leídos:
  - `drivers/gpu/drm/vc4/`: `vc4_crtc.c`, `vc4_hdmi.c`, `vc4_hdmi.h`, `vc4_hdmi_phy.c`, `vc4_hdmi_regs.h`,
    `vc4_regs.h`, `vc4_kms.c`, `vc4_hvs.c` (solo la parte de relojes) y `vc4_drv.c` (solo el aviso al firmware).
  - `drivers/clk/bcm/clk-raspberrypi.c` y `drivers/clk/bcm/clk-bcm2711-dvp.c` (relojes).
  - `drivers/firmware/raspberrypi.c` y `include/soc/bcm2835/raspberrypi-firmware.h` (buzón).
  - `drivers/pmdomain/bcm/raspberrypi-power.c` (dominios de energía).
  - `drivers/i2c/busses/i2c-brcmstb.c` (controlador I2C del DDC).
  - `drivers/irqchip/irq-brcmstb-l2.c` (controlador de interrupciones del HDMI).
  - `drivers/reset/reset-simple.c` (reinicios del bloque DVP).
  - `drivers/gpu/drm/display/drm_hdmi_helper.c`, `drm_hdmi_state_helper.c`, `drm_scdc_helper.c` e
    `include/drm/display/drm_scdc.h` (cálculos y SCDC comunes a todos los HDMI).
  - `arch/arm/boot/dts/broadcom/bcm2711.dtsi` y `bcm2711-rpi.dtsi` (árbol de dispositivos de Linux).
- **Árbol de dispositivos de la placa**: `build/firmware/bcm2711-rpi-4-b.dtb` del repositorio, decompilado con
  `dtc -I dtb -O dts`. Es el árbol del firmware de Raspberry Pi. Donde difiere del de Linux se dice.
- **Generación**: Linux llama `VC4_GEN_5` (o "vc5") a la BCM2711. La BCM2835 (Pi 0 a 3) es `VC4_GEN_4` y la BCM2712
  (Pi 5) es `VC4_GEN_6`. Este documento describe solo la BCM2711.
- **Citas**: cada dato lleva entre corchetes el fichero y la función de Linux de donde sale. Por ejemplo,
  [`vc4_crtc.c`, `vc4_crtc_config_pv`]. Si sale de una tabla o cabecera, se cita la tabla o la cabecera.
- **Comprobado y deducido**: "Linux hace X" significa que se leyó en el código. "Se deduce" significa que es una
  conclusión del analista. Nada de este documento se ha probado en hardware real.
- **Notación**: desplazamientos en bytes. Un campo `[h:l]` ocupa los bits de `h` a `l` de una palabra de 32 bits,
  ambos incluidos. "Palabra" son siempre 32 bits. Los ejemplos de valores son deducidos: se calcularon a mano con las
  fórmulas de Linux.
- **Nombres**: los nombres de registros y bits son los de Linux (`PV_CONTROL`, `HDMI_VID_CTL`...). Se usan para que
  se puedan contrastar.
- **Orden de bytes**: Linux accede a todos los registros de este documento con lecturas y escrituras de 32 bits en
  little-endian (`readl`, `writel`). La única excepción es el controlador I2C, que en un núcleo big-endian usa
  accesos big-endian (sección 9.2). Para DashOS se deduce lo mismo que en `hvs.md` (sección 2.5): cada valor de 32
  bits se escribe con los bytes invertidos respecto al orden nativo de la CPU.

## 1. Visión de conjunto

La imagen sale así:

1. El HVS compone un canal y deja los píxeles en su FIFO (ver `hvs.md`).
2. Una **pixel valve** (PV) genera los tiempos de vídeo. Saca píxeles de la FIFO del HVS y se los entrega al
   codificador. [`vc4_crtc.c`, comentario "VC4 CRTC module"]
3. El **controlador HDMI** recibe los píxeles, convierte el color (CSC), mete los paquetes de datos (infoframes y
   audio) en los borrados y codifica en TMDS.
4. El **PHY** del HDMI serializa y saca las cuatro líneas TMDS por el conector. Su PLL también fabrica el reloj del
   enlace (sección 4.6).

En la Pi 4 cada conector tiene su cadena fija:

| Conector | Salida del HVS | PV | Controlador HDMI | Bus I2C del DDC |
|---|---|---|---|---|
| HDMI0 (junto a la alimentación) | 4 | PV2 | `hdmi0` | `ddc0` |
| HDMI1 | 5 | PV4 | `hdmi1` | `ddc1` |

- La salida del HVS y la PV salen de [`vc4_crtc.c`, `bcm2711_pv2_data` y `bcm2711_pv4_data`].
- El nombre "HDMI0 junto a la alimentación" es conocimiento general de la placa. No sale de Linux.
- Qué canal del HVS alimenta a cada salida no es fijo. Lo decide el mux del HVS (`hvs.md`, secciones 10.3 y 10.4).
  Con un solo monitor, su salida coge el canal 0. Con dos, el primero que se enciende coge el 0 y el segundo el 1.
- Una PV alimenta un solo codificador a la vez. [`vc4_crtc.c`, `vc4_get_crtc_encoder`]

## 2. Pixel valves

### 2.1. Cuántas hay y dónde están

La BCM2711 tiene cinco PV. Solo PV2 y PV4 sirven para HDMI. [`vc4_crtc.c`, tablas `bcm2711_pv*_data`; DTB]

| PV | Compatible | Bus | CPU (deducida) | IRQ (SPI) | INTID GIC | FIFO | Píxeles por ciclo | Salida HVS | Codificador |
|---|---|---|---|---|---|---|---|---|---|
| PV0 | `brcm,bcm2711-pixelvalve0` | `0x7E206000` | `0xFE206000` | 109 | 141 | 64 B | 1 | 0 | DSI0 o DPI |
| PV1 | `brcm,bcm2711-pixelvalve1` | `0x7E207000` | `0xFE207000` | 110 | 142 | 64 B | 1 | 3 | DSI1 o SMI |
| **PV2** | `brcm,bcm2711-pixelvalve2` | `0x7E20A000` | `0xFE20A000` | **101** | **133** | **256 B** | **2** | 4 | **HDMI0** |
| PV3 | `brcm,bcm2711-pixelvalve3` | `0x7EC12000` | `0xFEC12000` | 106 | 138 | 64 B | 1 | 1 | VEC |
| **PV4** | `brcm,bcm2711-pixelvalve4` | `0x7E216000` | `0xFE216000` | **110** | **142** | **64 B** | **2** | 5 | **HDMI1** |

- La ventana de registros mide `0x100` bytes en todas. [DTB, `reg`]
- Todas las interrupciones son de nivel alto. [DTB, tercer valor de `interrupts` a 4]
- El INTID es el número SPI más 32. Se deduce.
- La dirección para la CPU sale del `ranges` del nodo `soc`, en el modo de periféricos "bajo". Vale la misma
  advertencia que en `hvs.md`, sección 2.1.
- **PV1 y PV4 comparten la línea SPI 110.** Linux pide la interrupción como compartida. El manejador de cada PV mira
  su propio estado y devuelve "no era mía" si no hay nada. [DTB; `vc4_crtc.c`, `vc4_crtc_bind` y
  `vc4_crtc_irq_handler`]
- Los registros de la PV van con el reloj del bus (AXI). Los tiempos y la FIFO van con el reloj de la salida.
  [`vc4_crtc.c`, comentario "VC4 CRTC module"]
- La PV no tiene reloj ni reinicio propios en el árbol de dispositivos. Toma el reloj del codificador que elige
  `CLK_SELECT`. Se deduce del comentario anterior y de que los nodos de PV no tienen `clocks`.

### 2.2. Mapa de registros

Desplazamientos desde la base de la PV. [`vc4_regs.h`, bloque `PV_*`]

| Desp. | Registro | Uso |
|---|---|---|
| `0x00` | `PV_CONTROL` | Encendido, FIFO, formato y reloj |
| `0x04` | `PV_V_CONTROL` | Vídeo activo, modo continuo, entrelazado |
| `0x08` | `PV_VSYNCD_EVEN` | Retardo de la sincronía vertical del campo par |
| `0x0C` | `PV_HORZA` | Pórtico trasero y sincronía horizontales |
| `0x10` | `PV_HORZB` | Pórtico delantero horizontal y ancho activo |
| `0x14` | `PV_VERTA` | Pórtico trasero y sincronía verticales |
| `0x18` | `PV_VERTB` | Pórtico delantero vertical y alto activo |
| `0x1C` | `PV_VERTA_EVEN` | Como `PV_VERTA`, para el campo par en entrelazado |
| `0x20` | `PV_VERTB_EVEN` | Como `PV_VERTB`, para el campo par en entrelazado |
| `0x24` | `PV_INTEN` | Permiso de interrupciones (mismos bits que `PV_INTSTAT`) |
| `0x28` | `PV_INTSTAT` | Estado de interrupciones; se borra escribiendo 1 |
| `0x2C` | `PV_STAT` | Estado. Linux solo lo vuelca para depurar |
| `0x30` | `PV_HACT_ACT` | Ancho activo real. Solo para DSI |
| `0x34` | `PV_MUX_CFG` | Orden de los componentes de color (nuevo en la BCM2711) |
| `0x68` | `PV_HW_CFG1` | Citado en un comentario como posible fuente del tamaño de la FIFO. Linux no lo lee |
| `0x94` | `PV_PIPE_INIT_CTRL` | Solo en la BCM2712. No escribir en la BCM2711 |

### 2.3. Campos

**`PV_CONTROL` (`0x00`)** [`vc4_regs.h`]

| Bits | Campo | Significado |
|---|---|---|
| 0 | `EN` | Enciende la PV |
| 1 | `FIFO_CLR` | Vacía la FIFO. Solo funciona con `EN` a 0 [`vc4_crtc.c`, `vc4_crtc_pixelvalve_reset`] |
| 3:2 | `CLK_SELECT` | Qué codificador da el reloj. En PV2 y PV4 el HDMI es el valor **0** [`vc4_crtc.c`, `encoder_types` y `vc4_set_crtc_possible_masks`] |
| 5:4 | `PIXEL_REP` | Repetición de píxel menos 1. Para HDMI siempre 0 |
| 12 | `WAIT_HSTART` | Sin descripción en Linux. Se pone siempre |
| 13 | `TRIGGER_UNDERFLOW` | Sin descripción en Linux. Se pone siempre |
| 14 | `CLR_AT_START` | Sin descripción en Linux. Se pone siempre |
| 20:15 | `FIFO_LEVEL` | Los 6 bits bajos del nivel de llenado de la FIFO, en bytes |
| 23:21 | `FORMAT` | 0 = 24 bits por píxel. Los valores 1 a 4 son para DSI |
| 26:25 | `FIFO_LEVEL_HIGH` | Los bits 7:6 del nivel de llenado (`PV5_*`, nuevo en la BCM2711) |

**`PV_V_CONTROL` (`0x04`)** [`vc4_regs.h`]

| Bits | Campo | Significado |
|---|---|---|
| 0 | `VIDEN` | Vídeo activo. Arranca y para el barrido |
| 1 | `CONTINUOUS` | Barrido continuo. Linux lo pone siempre |
| 2 | `COMMAND` | Modo comando (DSI). No se usa con HDMI |
| 3 | `DSI` | Solo DSI |
| 4 | `INTERLACE` | Modo entrelazado |
| 5 | `ODD_FIRST` | El primer campo es el impar. Solo para NTSC por el VEC |
| 22:6 | `ODD_DELAY` | Retardo del campo impar, en ciclos de la PV |
| 29 | `ODD_TIMING` | Solo en la BCM2712 |

**Tiempos** [`vc4_regs.h`]

| Registro | Bits 31:16 | Bits 15:0 |
|---|---|---|
| `PV_HORZA` | `HBP`: pórtico trasero horizontal | `HSYNC`: ancho de la sincronía horizontal |
| `PV_HORZB` | `HFP`: pórtico delantero horizontal | `HACTIVE`: ancho activo |
| `PV_VERTA` y `PV_VERTA_EVEN` | `VBP`: pórtico trasero vertical | `VSYNC`: alto de la sincronía vertical |
| `PV_VERTB` y `PV_VERTB_EVEN` | `VFP`: pórtico delantero vertical | `VACTIVE`: alto activo |

- Los valores horizontales van en **ciclos de la PV**, no en píxeles. PV2 y PV4 sacan 2 píxeles por ciclo, así que
  van divididos entre 2 (sección 2.4).
- Los verticales van en líneas.

**`PV_INTEN` (`0x24`) y `PV_INTSTAT` (`0x28`)** [`vc4_regs.h`]

| Bit | Nombre | Evento (deducido del nombre) |
|---|---|---|
| 0 | `HSYNC_START` | Empieza la sincronía horizontal |
| 1 | `HBP_START` | Empieza el pórtico trasero horizontal |
| 2 | `HACT_START` | Empieza la zona activa de la línea |
| 3 | `HFP_START` | Empieza el pórtico delantero horizontal |
| 4 | `VSYNC_START` | Empieza la sincronía vertical |
| 5 | `VBP_START` | Empieza el pórtico trasero vertical |
| 6 | `VACT_START` | Empieza la zona activa del cuadro |
| 7 | **`VFP_START`** | Empieza el pórtico delantero vertical: acaba la zona activa. **Es la única que usa Linux** |
| 8 | `VFP_END` | Acaba el pórtico delantero vertical |
| 9 | `VID_IDLE` | Vídeo parado |

**`PV_STAT` (`0x2C`)**: el bit 10 indica que la FIFO de la PV se desbordó. Lo dice un comentario de Linux, no hay
nombre. [`vc4_crtc.c`, `vc4_get_fifo_full_level`]

**`PV_MUX_CFG` (`0x34`)**: el campo `RGB_PIXEL_MUX_MODE` (bits 5:2) vale 8 para "sin intercambio". Linux escribe
siempre 8 en la BCM2711, es decir el valor `0x00000020`. [`vc4_regs.h`; `vc4_crtc.c`, `vc4_crtc_config_pv`]

### 2.4. Cálculo de los valores para HDMI

[`vc4_crtc.c`, `vc4_crtc_config_pv`, `vc4_get_fifo_full_level` y `vc4_crtc_get_fifo_full_level_bits`]

Notación del modo (como en cualquier tabla de tiempos de vídeo): `hdisplay`, `hsync_start`, `hsync_end`, `htotal`,
y lo mismo en vertical. Con `ppc` = 2 (píxeles por ciclo de PV2 y PV4):

- `HBP` = (`htotal` − `hsync_end`) / `ppc`
- `HSYNC` = (`hsync_end` − `hsync_start`) / `ppc`
- `HFP` = (`hsync_start` − `hdisplay`) / `ppc`
- `HACTIVE` = `hdisplay` / `ppc`
- `VBP` = `vtotal` − `vsync_end`
- `VSYNC` = `vsync_end` − `vsync_start`
- `VFP` = `vsync_start` − `vdisplay`
- `VACTIVE` = `vdisplay`

Notas:

- La repetición de píxel de los modos "doble reloj" (480i, 576i) no se aplica en la PV cuando la salida es HDMI. La
  hace el controlador HDMI (sección 6.4). [`vc4_crtc.c`, `vc4_crtc_config_pv`, cálculo de `pixel_rep`]
- Como se divide entre 2, **todos los tiempos horizontales tienen que ser pares**. Linux rechaza los modos con
  `hdisplay`, `hsync_start`, `hsync_end` o `htotal` impares. Solo en los modos doble reloj intenta arreglarlo:
  resta 1 a `hsync_start` si el pórtico delantero es impar, y resta 1 a `hsync_end` si la sincronía es impar.
  [`vc4_hdmi.c`, `vc4_hdmi_encoder_mode_valid` y `vc4_hdmi_encoder_atomic_check`]
- En entrelazado, los valores verticales son los de un campo. Linux usa los tiempos "por campo" que calcula el núcleo
  DRM (`crtc_vdisplay` y compañía). Se deduce que son la mitad de los del cuadro.

**Entrelazado (HDMI)**:

- `PV_VERTA_EVEN` y `PV_VERTB_EVEN` llevan `VBP`, `VSYNC`, `VFP` y `VACTIVE` tal cual.
- `PV_VERTA` y `PV_VERTB` llevan lo mismo, pero con `VBP` + 1.
- `PV_V_CONTROL` = `CONTINUOUS` + `INTERLACE` + `ODD_DELAY` = `htotal` / (2 · `ppc`).
- `PV_VSYNCD_EVEN` = 0.
- El ajuste de `ODD_FIRST` y el `ODD_DELAY` + 1 son solo para el VEC.

**Progresivo**: `PV_V_CONTROL` = `CONTINUOUS` (valor `0x00000002`) y `PV_VSYNCD_EVEN` = 0.

**Nivel de llenado de la FIFO** (en bytes). La PV pide píxeles al HVS mientras la FIFO tenga menos bytes que este
nivel. La latencia de la petición es de 6 píxeles.

- PV2 (HDMI0): profundidad − 3 · 6 = 256 − 18 = **238**.
- PV4 (HDMI1): **32**, fijo. Linux dice que con la fórmula general no funciona.
- `FIFO_LEVEL` = nivel mod 64. `FIFO_LEVEL_HIGH` = nivel / 64 (división entera).

**`PV_CONTROL`** para HDMI, antes de encender: `FIFO_CLR` + nivel + `FORMAT` 0 + `PIXEL_REP` 0 + `CLR_AT_START` +
`TRIGGER_UNDERFLOW` + `WAIT_HSTART` + `CLK_SELECT` 0.

**Ejemplo deducido: 1920 × 1080 a 60 Hz** (CTA-861 VIC 16: `htotal` 2200, `hsync_start` 2008, `hsync_end` 2052,
`vtotal` 1125, `vsync_start` 1084, `vsync_end` 1089, reloj 148,5 MHz, sincronías positivas).

| Registro | HDMI0 (PV2) | HDMI1 (PV4) |
|---|---|---|
| `PV_HORZA` | `0x004A0016` (74, 22) | igual |
| `PV_HORZB` | `0x002C03C0` (44, 960) | igual |
| `PV_VERTA` | `0x00240005` (36, 5) | igual |
| `PV_VERTB` | `0x00040438` (4, 1080) | igual |
| `PV_V_CONTROL` | `0x00000002` | igual |
| `PV_MUX_CFG` | `0x00000020` | igual |
| `PV_CONTROL` (antes de `EN`) | `0x06177002` | `0x00107002` |
| `PV_CONTROL` (con `EN`) | `0x06177003` | `0x00107003` |

### 2.5. Secuencias de la PV

**Reiniciar la PV** [`vc4_crtc.c`, `vc4_crtc_pixelvalve_reset`]

1. Leer `PV_CONTROL`, quitar `EN` y escribir.
2. Leer `PV_CONTROL`, poner `FIFO_CLR` y escribir. La FIFO solo se vacía con la PV apagada.

**Programar un modo** [`vc4_crtc.c`, `vc4_crtc_config_pv`]

1. Reiniciar la PV.
2. Escribir `PV_HORZA` y `PV_HORZB`.
3. En entrelazado: escribir `PV_VERTA_EVEN`, `PV_VERTB_EVEN`, `PV_V_CONTROL` y `PV_VSYNCD_EVEN`. En progresivo:
   escribir `PV_V_CONTROL` y `PV_VSYNCD_EVEN`.
4. Escribir `PV_VERTA` y `PV_VERTB`.
5. Escribir `PV_MUX_CFG` = `0x00000020`.
6. Escribir `PV_CONTROL` con el valor de la sección 2.4. **Esta escritura deja `EN` a 0.**

**Arrancar** [`vc4_crtc.c`, `vc4_crtc_atomic_enable`]

1. Programar el modo (arriba). Antes, el controlador HDMI ya tiene relojes, PHY y tiempos (sección 7, pasos 8 a 13).
2. Poner `EN` en `PV_CONTROL` (lectura, modificación y escritura).
3. Configurar el color del HDMI y su FIFO (sección 7, pasos 16 y 17).
4. Poner `VIDEN` en `PV_V_CONTROL` (lectura, modificación y escritura).
5. Terminar el HDMI (sección 7, pasos 19 en adelante).

**Parar** [`vc4_crtc.c`, `vc4_crtc_disable`]

1. Quitar `VIDEN` en `PV_V_CONTROL`. Esperar hasta 1 ms a que se lea a 0. Si no baja, Linux solo avisa.
2. **Esperar 20 ms con espera activa.** Sin esta espera, en la BCM2711 se queda un píxel atascado en una FIFO entre la
   PV y el HDMI que no se puede vaciar. Toda la imagen sale desplazada un píxel a la derecha: el último píxel de cada
   línea aparece como el primero de la siguiente. Linux dice que el fallo sale en menos de 10 cambios de modo si se
   quita la espera, y que el tiempo es delicado.
3. Apagar el vídeo del HDMI (sección 8, paso 4).
4. Reiniciar la PV (`EN` a 0 y después `FIFO_CLR`).
5. Parar el canal del HVS (`hvs.md`, sección 12.6).
6. Apagar el PHY y los relojes del HDMI (sección 8, paso 7).

**Al arrancar el sistema, si el firmware dejó una salida encendida** [`vc4_crtc.c`, `vc4_crtc_disable_at_boot`]

- Linux solo lo mira en PV2 y PV4.
- Si `PV_CONTROL.EN` y `PV_V_CONTROL.VIDEN` están a 1, busca el canal del HVS en el mux de la salida. Si lo
  encuentra, comprueba que `CLK_SELECT` vale 0 (HDMI) y apaga con la secuencia "Parar".
- `hvs.md` (lo que no está claro, punto 16) explica por qué ese apagado puede no llegar a hacerse en la BCM2711.

### 2.6. Interrupción de sincronía vertical

- **De dónde sale**: de la PV, con el evento `VFP_START`: empieza el pórtico delantero vertical, es decir, acaba la
  zona activa del cuadro. El HVS no da esta interrupción en la BCM2711. [`vc4_crtc.c`, `vc4_enable_vblank` y
  `vc4_crtc_irq_handler`; `hvs.md`, sección 13.1]
- **Al iniciar**: escribir `PV_INTEN` = 0 y `PV_INTSTAT` = `0x80` (borra un `VFP_START` pendiente). Después pedir la
  línea en el GIC. [`vc4_crtc.c`, `vc4_crtc_bind`]
- **Habilitar**: `PV_INTEN` = `0x80` (solo `VFP_START`). Linux lo hace antes de encender el canal del HVS y la PV.
  [`vc4_crtc.c`, `vc4_crtc_atomic_enable` y `vc4_enable_vblank`]
- **Deshabilitar**: `PV_INTEN` = 0. Linux lo hace antes de apagar la PV. [`vc4_crtc.c`, `vc4_crtc_atomic_disable` y
  `vc4_disable_vblank`]
- **Reconocer**: leer `PV_INTSTAT`. Si el bit 7 está a 1, escribir `0x80` en `PV_INTSTAT`. Después hacer el trabajo
  del cuadro. Si el bit no está, la interrupción era de la otra PV de la línea compartida.
  [`vc4_crtc.c`, `vc4_crtc_irq_handler`]
- **Qué hace Linux en cada sincronía**: guarda la hora, avisa al núcleo DRM y comprueba el cambio de lista del HVS.
  [`vc4_crtc.c`, `vc4_crtc_handle_vblank` y `vc4_crtc_handle_page_flip`]

**Relación con el cambio de lista del HVS** (detalle en `hvs.md`, secciones 12.4 y 13.3):

1. Se escribe la lista nueva en un bloque libre y su índice en `SCALER_DISPLISTx`, en cualquier momento del cuadro.
2. El HVS toma ese índice al empezar el cuadro siguiente, dentro del borrado vertical.
3. En cada `VFP_START`, se lee `SCALER_DISPLACTx`. Si ya vale el índice nuevo, el cambio está hecho: se avisa al
   cliente, se liberan los búferes viejos y se reactiva el aviso de vaciado del canal.
4. Si no, se espera al siguiente `VFP_START`.

Así la imagen cambia entera entre dos cuadros, sin desgarros.

## 3. Bloques del HDMI

### 3.1. Direcciones

Cada controlador HDMI tiene varias ventanas de registros. Los nombres son los de `reg-names` del árbol de
dispositivos. [DTB y `bcm2711.dtsi`, nodos `hdmi@7ef00700` y `hdmi@7ef05700`; `vc4_hdmi.c`,
`vc5_hdmi_init_resources`]

| Bloque | Qué es | HDMI0 (bus) | HDMI1 (bus) | Tamaño |
|---|---|---|---|---|
| `hdmi` | Núcleo del controlador: tiempos, paquetes, audio, hotplug, scrambler | `0x7EF00700` | `0x7EF05700` | `0x300` |
| `dvp` | Interfaz de vídeo del HDMI: parada del reloj, cruce de canales | `0x7EF00300` | `0x7EF05300` | `0x200` |
| `phy` | PHY TMDS y su PLL | `0x7EF00F00` | `0x7EF05F00` | `0x80` |
| `rm` | "Rate manager" del PLL del PHY | `0x7EF00F80` | `0x7EF05F80` | `0x80` |
| `packet` | RAM de paquetes (infoframes) | `0x7EF01B00` | `0x7EF06B00` | `0x200` |
| `metadata` | Sin uso en Linux | `0x7EF01F00` | `0x7EF06F00` | `0x400` |
| `csc` | Conversión de color | `0x7EF00200` | `0x7EF00280` | `0x80` |
| `cec` | CEC | `0x7EF04300` | `0x7EF09300` | `0x100` |
| `hd` | Bloque **compartido** por los dos HDMI: control de vídeo y audio (MAI) | `0x7EF20000` | `0x7EF20000` | `0x100` |
| `intr2` | Controlador de interrupciones compartido (sección 5). Solo en el DTB del firmware | `0x7EF00100` | `0x7EF00100` | `0x30` |

Bloques compartidos fuera del nodo HDMI:

| Bloque | Qué es | Bus | Tamaño | Fuente |
|---|---|---|---|---|
| DVP global | Puertas del reloj de audio y reinicios de los dos HDMI | `0x7EF00000` | `0x10` | Nodo `clock@7ef00000`, `brcm,brcm2711-dvp` |
| `aon_intr` | Interrupciones de HPD y CEC de los dos HDMI | `0x7EF00100` | `0x30` | Nodo `interrupt-controller@7ef00100` |
| `ddc0`: BSC | I2C del DDC de HDMI0 | `0x7EF04500` | `0x100` | Nodo `i2c@7ef04500` |
| `ddc0`: auto-i2c | Máquina automática de I2C de HDMI0 | `0x7EF00B00` | `0x300` | Mismo nodo |
| `ddc1`: BSC | I2C del DDC de HDMI1 | `0x7EF09500` | `0x100` | Nodo `i2c@7ef09500` |
| `ddc1`: auto-i2c | Máquina automática de I2C de HDMI1 | `0x7EF05B00` | `0x300` | Mismo nodo |

- La dirección para la CPU es la de bus con `0x7E` cambiado por `0xFE` (por ejemplo, `0xFEF00700`). Se deduce del
  `ranges` del nodo `soc`, con la advertencia de `hvs.md`, sección 2.1.
- El bloque `hd` es el mismo para los dos HDMI. Cada uno usa sus propios desplazamientos dentro de él, salvo
  `HDMI_DVP_CTL`, que está en el mismo sitio para los dos (sección 3.2).
- Linux no mapea el bloque `metadata` ni el `intr2`. El `intr2` solo aparece en el DTB del firmware. En el de Linux el
  controlador de interrupciones es un nodo aparte.
- Todos los nodos de vídeo vienen con `status = "disabled"` en el DTB del firmware. Se deduce que el firmware los
  habilita con la capa `vc4-kms-v3d` cuando se le pide.

### 3.2. Registros de cada bloque

Desplazamientos en bytes desde la base del bloque. Los de HDMI0 y HDMI1 coinciden salvo donde se indica.
[`vc4_hdmi_regs.h`, tablas `vc5_hdmi_hdmi0_fields` y `vc5_hdmi_hdmi1_fields`]

**Bloque `hdmi` (núcleo)**

| Desp. | Registro | Sección |
|---|---|---|
| `0x074` | `HDMI_FIFO_CTL` | 6.6 |
| `0x09C` | `HDMI_MAI_CHANNEL_MAP` | 11 |
| `0x0A0` | `HDMI_MAI_CONFIG` | 11 |
| `0x0B8` | `HDMI_AUDIO_PACKET_CONFIG` | 11 |
| `0x0BC` | `HDMI_RAM_PACKET_CONFIG` | 10.2 |
| `0x0C4` | `HDMI_RAM_PACKET_STATUS` | 10.2 |
| `0x0C8` | `HDMI_CRP_CFG` | 11 |
| `0x0CC` | `HDMI_CTS_0` | 11 |
| `0x0D0` | `HDMI_CTS_1` | 11 |
| `0x0E0` | `HDMI_SCHEDULER_CONTROL` | 6.5 |
| `0x0E4` | `HDMI_HORZA` | 6.4 |
| `0x0E8` | `HDMI_HORZB` | 6.4 |
| `0x0EC` | `HDMI_VERTA0` | 6.4 |
| `0x0F0` | `HDMI_VERTB0` | 6.4 |
| `0x0F4` | `HDMI_VERTA1` | 6.4 |
| `0x0F8` | `HDMI_VERTB1` | 6.4 |
| `0x100` | `HDMI_MISC_CONTROL` | 6.4 |
| `0x134` a `0x158` | `HDMI_FORMAT_DET_1` a `_10` | Solo para depurar |
| `0x170` | `HDMI_DEEP_COLOR_CONFIG_1` | 6.4 |
| `0x178` | `HDMI_GCP_CONFIG` | 6.4 |
| `0x17C` | `HDMI_GCP_WORD_1` | 6.4 |
| `0x1A8` | `HDMI_HOTPLUG` | 9.1 |
| `0x1C4` | `HDMI_SCRAMBLER_CTL` | 12 |

**Bloque `hd` (compartido)**: los desplazamientos sí cambian entre HDMI0 y HDMI1.

| Registro | HDMI0 | HDMI1 | Sección |
|---|---|---|---|
| `HDMI_DVP_CTL` | `0x00` | `0x00` (el mismo registro) | 4.5 |
| `HDMI_MAI_CTL` | `0x10` | `0x30` | 11 |
| `HDMI_MAI_THR` | `0x14` | `0x34` | 11 |
| `HDMI_MAI_FMT` | `0x18` | `0x38` | 11 |
| `HDMI_MAI_DATA` | `0x1C` | `0x3C` | 11 |
| `HDMI_MAI_SMP` | `0x20` | `0x40` | 11 |
| `HDMI_VID_CTL` | `0x44` | `0x48` | 6.6 |
| `HDMI_FRAME_COUNT` | `0x60` | `0x64` | Solo para depurar |

**Bloque `dvp` (de cada HDMI)**

| Desp. | Registro | Sección |
|---|---|---|
| `0x0BC` | `HDMI_CLOCK_STOP` | 4.5 |
| `0x0EC` | `HDMI_VEC_INTERFACE_CFG` | 10.1 |
| `0x0F0` | `HDMI_VEC_INTERFACE_XBAR` | 10.1 |

**Bloque `phy`**

| Desp. | Registro |
|---|---|
| `0x000` | `HDMI_TX_PHY_RESET_CTL` |
| `0x004` | `HDMI_TX_PHY_POWERDOWN_CTL` |
| `0x008` | `HDMI_TX_PHY_CTL_0` |
| `0x00C` | `HDMI_TX_PHY_CTL_1` |
| `0x010` | `HDMI_TX_PHY_CTL_2` |
| `0x014` | `HDMI_TX_PHY_CTL_3` |
| `0x01C` | `HDMI_TX_PHY_PLL_CTL_0` |
| `0x020` | `HDMI_TX_PHY_PLL_CTL_1` |
| `0x028` | `HDMI_TX_PHY_CLK_DIV` |
| `0x034` | `HDMI_TX_PHY_PLL_CFG` |
| `0x044` | `HDMI_TX_PHY_TMDS_CLK_WORD_SEL` |
| `0x04C` | `HDMI_TX_PHY_CHANNEL_SWAP` |
| `0x050` | `HDMI_TX_PHY_PLL_CALIBRATION_CONFIG_1` |
| `0x054` | `HDMI_TX_PHY_PLL_CALIBRATION_CONFIG_2` |
| `0x05C` | `HDMI_TX_PHY_PLL_CALIBRATION_CONFIG_4` |

**Bloque `rm`**

| Desp. | Registro |
|---|---|
| `0x000` | `HDMI_RM_CONTROL` |
| `0x018` | `HDMI_RM_OFFSET` |
| `0x01C` | `HDMI_RM_FORMAT` |

**Bloque `packet`**: la RAM empieza en el desplazamiento 0 (`HDMI_RAM_PACKET_START`). Ver la sección 10.2.

**Bloque `csc`**

| Desp. | Registro |
|---|---|
| `0x000` | `HDMI_CSC_CTL` |
| `0x004` | `HDMI_CSC_12_11` |
| `0x008` | `HDMI_CSC_14_13` |
| `0x00C` | `HDMI_CSC_22_21` |
| `0x010` | `HDMI_CSC_24_23` |
| `0x014` | `HDMI_CSC_32_31` |
| `0x018` | `HDMI_CSC_34_33` |
| `0x02C` | `HDMI_CSC_CHANNEL_CTL` |

**Bloque `cec`**: `HDMI_CEC_CNTRL_1` a `_5` en `0x010` a `0x020`, `HDMI_CEC_TX_DATA_1` a `_4` en `0x028` a `0x034`,
`HDMI_CEC_RX_DATA_1` a `_4` en `0x038` a `0x044`. Ver la sección 13.

Registros que existen en la BCM2835 y **no** en la BCM2711: `HDMI_M_CTL`, `HDMI_SW_RESET_CONTROL`,
`HDMI_HOTPLUG_INT`, `HDMI_CEC_CPU_*`, `HDMI_TX_PHY_CTL_0` en el núcleo y el campo `ORDER` de `HDMI_CSC_CTL`.
[`vc4_hdmi_regs.h`, `vc4_hdmi_fields`]

## 4. Relojes, reinicios y energía

### 4.1. Resumen

[`bcm2711-rpi.dtsi`, nodos `&hdmi0` y `&hdmi1`; DTB; `vc4_hdmi.c`, `vc5_hdmi_init_resources`]

| Nombre en Linux | Qué mueve | Origen | Cómo se pide |
|---|---|---|---|
| `hdmi` ("HSM") | La máquina de estados del HDMI y el acceso a sus registros | Firmware, reloj **13** (`M2MC`) | Buzón |
| `bvb` | El bus de píxeles hacia el HDMI | Firmware, reloj **14** (`PIXEL_BVB`) | Buzón |
| `audio` | El reloj de muestreo del audio (MAI) | Puerta en el DVP global; padre fijo de 108 MHz | Registro |
| `cec` | El reloj de bit del CEC | Reloj fijo de 27 MHz | Nada que hacer |
| Reloj de píxel | El enlace TMDS y, se deduce, la PV | El PLL del PHY del propio HDMI | Registros del PHY |
| `core` | El HVS | Firmware, reloj 4 | Ver `hvs.md`, secciones 2.3 y 11 |

- Los relojes `hdmi` y `bvb` son **los mismos para los dos HDMI**. Linux los gobierna pidiendo un mínimo por cada
  salida. El reloj resultante es el mayor de los mínimos pedidos. [`clk-raspberrypi.c`, comentario de
  `RPI_FIRMWARE_M2MC_CLK_ID` y `raspberrypi_fw_dumb_determine_rate`]
- Los números de reloj del firmware son: 1 EMMC, 2 UART, 3 ARM, 4 CORE, 5 V3D, 6 H264, 7 ISP, 8 SDRAM, 9 PIXEL,
  10 PWM, 11 HEVC, 12 EMMC2, 13 M2MC, 14 PIXEL_BVB, 15 VEC, 16 DISP. [`raspberrypi-firmware.h`]
- El reloj 9 del firmware (`PIXEL`) existe, pero el HDMI de la BCM2711 no lo usa.

### 4.2. Mensajes del buzón

El firmware maneja los relojes `hdmi` y `bvb`. Linux usa el canal 8 (propiedades) del buzón de la VideoCore.
[`raspberrypi.c`, `rpi_firmware_property_list`; `clk-raspberrypi.c`]

- Formato del mensaje: tamaño total, código 0 (petición), las etiquetas y una palabra 0 al final. El firmware deja
  `0x80000000` en el código si todo fue bien y `0x80000001` si hubo error. [`raspberrypi.c`;
  `raspberrypi-firmware.h`, `RPI_FIRMWARE_STATUS_*`]
- Las etiquetas de reloj llevan tres palabras de datos: número de reloj, valor y una tercera a 0 ("sin turbo").
  [`clk-raspberrypi.c`, `raspberrypi_firmware_prop`]

| Etiqueta | Nombre | Datos de ida | Datos de vuelta | Uso |
|---|---|---|---|---|
| `0x00030001` | `GET_CLOCK_STATE` | reloj | bit 0: encendido | Comprobar |
| `0x00030002` | `GET_CLOCK_RATE` | reloj | Hz | Leer la frecuencia actual |
| `0x00030004` | `GET_MAX_CLOCK_RATE` | reloj | Hz | Límite superior |
| `0x00030007` | `GET_MIN_CLOCK_RATE` | reloj | Hz | Límite inferior |
| `0x00038001` | `SET_CLOCK_STATE` | reloj, estado | estado | Bit 0 = encender. Bit 1 = "esperar", Linux no lo usa |
| `0x00038002` | `SET_CLOCK_RATE` | reloj, Hz, 0 | Hz concedidos | Fijar frecuencia |
| `0x00030066` | `NOTIFY_DISPLAY_DONE` | nada | nada | Quitar la pantalla al firmware (`hvs.md`, 12.1) |
| `0x00030020` | `GET_EDID_BLOCK` | (no leído) | (no leído) | Existe en la cabecera. vc4 no la usa |

Comportamiento de Linux con estos relojes [`clk-raspberrypi.c`]:

- **Al registrar el reloj**: lee el mínimo y el máximo del firmware.
- **Reloj 13 (`hdmi`)**: Linux impone un mínimo propio de **120 MHz**. Si la frecuencia actual es menor, la sube a
  120 MHz en el arranque. El motivo: si la Pi arranca sin monitor, el firmware no inicia este reloj y lo deja a 0.
  **Con el reloj a 0, cualquier acceso a un registro del HDMI bloquea el bus y la CPU se cuelga sin aviso.**
  [`clk-raspberrypi.c`, variante de `RPI_FIRMWARE_M2MC_CLK_ID`; `vc4_hdmi.c`, `vc4_hdmi_runtime_resume`]
- **Encender**: `SET_CLOCK_STATE` con el bit 0 a 1. No cambia la frecuencia.
- **Apagar**: primero `SET_CLOCK_RATE` al mínimo y después `SET_CLOCK_STATE` a 0. Linux explica que el firmware actual
  no apaga de verdad el reloj con `SET_CLOCK_STATE`. Bajarlo al mínimo es lo único que ahorra energía.
  [`clk-raspberrypi.c`, `raspberrypi_fw_unprepare`]
- **Fijar frecuencia**: Linux siempre pide el mínimo que necesitan los consumidores, nunca más.

### 4.3. Frecuencias que pide el HDMI

[`vc4_hdmi.c`, `vc4_hdmi_encoder_pre_crtc_configure`]

Sea `T` la frecuencia de caracteres TMDS de la salida (sección 6.1).

- **`hdmi` (HSM)**: mínimo = máx(120 MHz, (`T` / 100) · 101). El HSM tiene que ser un poco más rápido que el
  reloj de píxel. Linux deja un margen del 1 %. Cita al firmware: "infinitesimalmente más rápido", comprobado en
  simulación.
- **`bvb`**:
  - `T` > 297 MHz: 300 MHz.
  - 148,5 MHz < `T` ≤ 297 MHz: 150 MHz.
  - `T` ≤ 148,5 MHz: 75 MHz.
- Como los dos relojes son compartidos, se deduce que DashOS tiene que pedir al firmware el mayor de los valores que
  necesiten las dos salidas encendidas.

| Modo | `T` | HSM | BVB |
|---|---|---|---|
| 640 × 480 a 60 Hz | 25,175 MHz | 120 MHz | 75 MHz |
| 1280 × 720 a 60 Hz | 74,25 MHz | 120 MHz | 75 MHz |
| 1920 × 1080 a 60 Hz | 148,5 MHz | 149,985 MHz | 75 MHz |
| 3840 × 2160 a 30 Hz | 297 MHz | 299,97 MHz | 150 MHz |
| 3840 × 2160 a 60 Hz | 594 MHz | 599,94 MHz | 300 MHz |

Valores deducidos con las fórmulas.

### 4.4. Reloj de audio y reinicios: el DVP global

El bloque DVP global (`0x7EF00000`, 16 bytes) tiene dos registros útiles. [`clk-bcm2711-dvp.c`, `clk_dvp_probe`]

| Desp. | Registro | Bits |
|---|---|---|
| `0x04` | `DVP_HT_RPI_SW_INIT` | Seis líneas de reinicio. Bit 0: HDMI0. Bit 1: HDMI1. Bit a 1 = reinicio activo |
| `0x08` | `DVP_HT_RPI_MISC_CONFIG` | Bit 3: puerta del reloj de audio de HDMI0. Bit 4: de HDMI1. **Bit a 1 = reloj parado** |

- El reloj de audio de cada HDMI es el reloj fijo de 108 MHz con esa puerta. [DTB, `clk-108M`]
- La polaridad del reinicio sale de `reset-simple.c` (`reset_simple_update`, sin `active_low`).
- **Linux no llega a pulsar el reinicio.** Pide un "reset" de la línea, pero el controlador `reset-simple` no tiene
  tiempo de reinicio configurado para este bloque y devuelve "no soportado". El código del HDMI ignora el error.
  [`vc4_hdmi.c`, `vc5_hdmi_reset`; `reset-simple.c`, `reset_simple_reset`; `clk-bcm2711-dvp.c`]

### 4.5. Reinicio del HDMI

[`vc4_hdmi.c`, `vc5_hdmi_reset`]

1. (Intento de reinicio por el DVP global, sin efecto en la práctica: ver 4.4.)
2. Escribir `HDMI_DVP_CTL` = 0 en el bloque `hd`.
3. Poner a 1 el bit 1 (`VC4_DVP_HT_CLOCK_STOP_PIXEL`) de `HDMI_CLOCK_STOP` en el bloque `dvp`. Para el reloj de
   píxel hacia el HDMI.

El bit se vuelve a poner a 0 al final de la escritura de tiempos (sección 6.4). `HDMI_DVP_CTL` está en el mismo
sitio para HDMI0 y HDMI1. Linux lo escribe a 0 al reiniciar cualquiera de los dos.

### 4.6. Reloj de píxel

- En la BCM2711 Linux no pide ningún reloj de píxel al firmware ni a otro bloque. El manejador del reloj de píxel
  queda vacío y las llamadas que lo fijan no hacen nada. [`vc4_hdmi.c`, `vc5_hdmi_init_resources` no lo pide; solo
  `vc4_hdmi_init_resources`, que es la de la BCM2835]
- La frecuencia TMDS solo se usa para programar el PLL del PHY (sección 6.3).
- **Se deduce**: el PLL del PHY genera el reloj del enlace a partir del oscilador de 54 MHz, y la PV y el HDMI toman
  de ahí su reloj de píxel.

### 4.7. Dominios de energía

- En la BCM2711, Linux no pide ningún dominio de energía para el HDMI, las PV ni el HVS. Los nodos del árbol no
  tienen `power-domains`. [`bcm2711.dtsi`; DTB]
- En la BCM2835 el HDMI sí usa el dominio 5 (`RPI_POWER_DOMAIN_HDMI`) del firmware. Se enciende con la etiqueta
  `0x00038030` (`SET_DOMAIN_STATE`) y dos palabras: dominio y estado. [`raspberrypi-power.c`,
  `rpi_firmware_set_power`; `bcm2835-rpi-*.dts`]
- No se sabe si en la BCM2711 hace falta encender ese dominio cuando el firmware no inició el HDMI. Linux no lo hace.

### 4.8. Orden de los relojes

Al encender una salida, Linux sigue este orden [`vc4_hdmi.c`, `vc4_hdmi_runtime_resume` y
`vc4_hdmi_encoder_pre_crtc_configure`]:

1. Encender el HSM (reloj 13).
2. Leer su frecuencia. **Si es 0, parar: no tocar ningún registro del HDMI.**
3. Abrir la puerta del reloj de audio.
4. Reiniciar el HDMI (4.5).
5. Subir el HSM al mínimo de la sección 4.3.
6. Subir y encender el BVB (reloj 14).
7. Programar el PHY.

Al apagar, el orden es el inverso: PHY, BVB, y al final audio y HSM. [`vc4_hdmi.c`,
`vc4_hdmi_encoder_post_crtc_powerdown` y `vc4_hdmi_runtime_suspend`]

Si el firmware dejó el vídeo encendido (`HDMI_VID_CTL.ENABLE` a 1), Linux enciende el HSM y el BVB al arrancar para
que la cuenta de usos cuadre con el apagado posterior. [`vc4_hdmi.c`, `vc4_hdmi_bind`]

## 5. Interrupciones del HDMI

El HDMI no tiene línea propia en el GIC. Todas sus interrupciones pasan por un controlador de segundo nivel
compartido, `aon_intr`. [DTB; `bcm2711.dtsi`; `vc4_hdmi.h`, comentario de `external_irq_controller`]

| Dato | Valor | Fuente |
|---|---|---|
| Compatible | `brcm,bcm2711-l2-intc`, `brcm,l2-intc` | DTB |
| Dirección | `0x7EF00100` (CPU `0xFEF00100`), `0x30` bytes | DTB |
| Línea en el GIC | SPI 96, INTID 128 | DTB |
| Tipo | Flanco de subida en el DTB del firmware. Nivel alto en el de Linux | DTB (`<0 0x60 1>`); `bcm2711.dtsi` |

**Registros** (variante "de flanco" de Linux) [`irq-brcmstb-l2.c`, `l2_edge_intc_init`]

| Desp. | Registro | Uso |
|---|---|---|
| `0x00` | `CPU_STATUS` | Eventos pendientes |
| `0x08` | `CPU_CLEAR` | Escribir 1 borra el evento |
| `0x0C` | `CPU_MASK_STATUS` | 1 = enmascarado |
| `0x10` | `CPU_MASK_SET` | Escribir 1 enmascara |
| `0x14` | `CPU_MASK_CLEAR` | Escribir 1 desenmascara |

El desplazamiento `0x04` no lo usa Linux. Se supone que es un registro para provocar eventos por software.

**Bits** [DTB y `bcm2711.dtsi`, `interrupts` de los nodos HDMI]

| Evento | HDMI0 | HDMI1 |
|---|---|---|
| `cec-tx` | 0 | 8 |
| `cec-rx` | 1 | 7 |
| `cec-low` | 2 | 6 |
| `wakeup` | 3 | 9 |
| `hpd-connected` | 4 | 10 |
| `hpd-removed` | 5 | 11 |

**Uso** [`irq-brcmstb-l2.c`, `brcmstb_l2_intc_probe` y `brcmstb_l2_intc_irq_handle`]

1. Al iniciar: escribir `0xFFFFFFFF` en `CPU_MASK_SET` y en `CPU_CLEAR`.
2. Para habilitar un evento: escribir su bit en `CPU_MASK_CLEAR`.
3. En la interrupción del GIC: pendientes = `CPU_STATUS` y no `CPU_MASK_STATUS`. Para cada bit pendiente, escribirlo
   en `CPU_CLEAR` y atender el evento. Linux reconoce antes de atender, como en cualquier interrupción de flanco.

## 6. Valores para un modo

### 6.1. Frecuencia de caracteres TMDS

[`drm_hdmi_helper.c`, `drm_hdmi_compute_mode_clock`]

Sea `P` el reloj de píxel del modo y `bpc` los bits por componente.

- RGB o YCbCr 4:4:4: `T` = `P` · `bpc` / 8 (redondeado al entero más cercano).
- YCbCr 4:2:2: `T` = `P`, sea cual sea `bpc` (hasta 12).
- YCbCr 4:2:0: `T` = `P` / 2 · `bpc` / 8. El HDMI de la BCM2711 no lo ofrece en Linux.
- Modos doble reloj: se multiplica por 2.

Con 8 bits por componente en RGB, `T` = `P`. Es lo que se recomienda para DashOS.

Linux ofrece RGB, YCbCr 4:2:2 y YCbCr 4:4:4, de 8 a 12 bits. [`vc4_hdmi.c`, `vc4_hdmi_connector_init`]

### 6.2. Límites de los modos

[`vc4_hdmi.c`, `vc4_hdmi_connector_clock_valid`, `vc4_hdmi_encoder_atomic_check` y variantes `bcm2711_hdmi*_variant`;
`vc4_hvs.c`, `vc4_hvs_bind`]

| Regla | Valor |
|---|---|
| `T` máximo en HDMI0 | 600 MHz |
| `T` máximo en HDMI1 | 340 MHz |
| `T` > 340 MHz (HDMI 2.0) | Solo si el máximo del reloj `core` que da el firmware es ≥ 550 MHz |
| Más de 3840 de ancho, 2160 o más de alto y 50 Hz o más | Solo si el máximo del reloj `core` es ≥ 600 MHz |
| Tiempos horizontales impares | Rechazados (sección 2.4) |
| Doble barrido | Rechazado [`vc4_crtc.c`, `vc4_crtc_mode_valid`] |
| Coexistencia con wifi | Si `T` · 10 cae entre 2,400 y 2,422 GHz, Linux baja el reloj de píxel a 238,56 MHz |

- La regla del wifi solo se aplica si el nodo HDMI tiene `wifi-2.4ghz-coexistence`. En la Pi 4 lo tiene. Afecta, por
  ejemplo, a 2560 × 1440 a 60 Hz (241,5 MHz). [DTB; `vc4_hdmi.c`, `vc4_hdmi_encoder_atomic_check`]
- El máximo del reloj `core` lo sube el firmware con la opción `hdmi_enable_4kp60` de `config.txt`. Lo dice un aviso
  de Linux. [`vc4_hdmi.c`, `vc4_hdmi_connector_get_modes`]

### 6.3. PHY y PLL

[`vc4_hdmi_phy.c`, `vc5_hdmi_phy_init`, `phy_get_vco_freq`, `phy_get_rm_offset`, `phy_get_vco_gain`,
`phy_get_cp_current`, `phy_get_settings` y tabla `vc5_hdmi_phy_settings`]

**Fórmulas** (frecuencia de referencia `F` = 54 MHz):

- **Divisor del VCO** `d`: el menor entero ≥ 1 tal que `T` · 10 · `d` ≥ 3 GHz.
- **Frecuencia del VCO**: `V` = `T` · 10 · `d`.
- **`VCO_SEL`** = 1 si `V` > 4,5 GHz; si no, 0.
- **Desplazamiento del RM** (formato 9.22): `R` = ⌊ ⌊ `V` · 2 · 2²² / `F` ⌋ / 4 ⌋. Equivale a `V` / 108 MHz con 22 bits
  de fracción.
- **Corriente de la bomba de carga** `ICP` = `0x1C` si `V` < 3,7 GHz; si no, `0x18`.
- **Ganancia del VCO**: `0xF` si `V` < 3,35 GHz; `0xC` si < 3,7 GHz; `0x6` si < 4,05 GHz; `0x5` si < 4,8 GHz;
  `0x7` si < 5,2 GHz; `0x2` en otro caso.
- **`TMDS_CLK_WORD_SEL`** = 3 si `T` ≥ 340 MHz; si no, 0. Se deduce que el 3 pone el reloj TMDS a la cuarta parte de
  la frecuencia de caracteres, como pide HDMI 2.0 por encima de 340 MHz.

**Ajustes de las líneas** según `T` (preénfasis, corriente principal, `RES_SEL`, `TERM_RES_SEL`):

| Rango de `T` | Líneas de datos | Línea de reloj |
|---|---|---|
| hasta 50 MHz | 0, `0x0A`, `0x12`, 0 | 0, `0x0A`, `0x18`, 0 |
| 50 a 75 MHz | 0, `0x09`, `0x12`, 0 | 0, `0x0C`, `0x18`, 3 |
| 75 a 165 MHz | 0, `0x09`, `0x12`, 0 | 0, `0x0C`, `0x18`, 3 |
| 165 a 250 MHz | 0, `0x0F`, `0x12`, 1 | 0, `0x0C`, `0x18`, 3 |
| 250 a 340 MHz | 2, `0x0D`, `0x12`, 1 | 0, `0x0C`, `0x18`, `0xF` |
| 340 a 450 MHz | 0, `0x1B`, `0x12`, `0xF` | 0, `0x0A`, `0x12`, `0xF` |
| 450 a 600 MHz | 0, `0x1C`, `0x12`, `0xF` | 0, `0x0B`, `0x13`, `0xF` |

- Cada rango incluye su límite superior y empieza 1 Hz por encima del anterior. Por encima de 600 MHz se usa la última
  fila.

**Orden de las líneas.** Las dos salidas cablean distinto el PHY. Linux describe cada salida con una tabla de cuatro
posiciones: para cada salida física del PHY (TX0, TX1, TX2, TXCK) dice qué señal lleva. [`vc4_hdmi.c`,
`phy_lane_mapping`; `vc4_hdmi.h`]

| Salida del PHY | HDMI0 lleva | HDMI1 lleva |
|---|---|---|
| TX0 | Datos 0 | Datos 1 |
| TX1 | Datos 1 | Datos 0 |
| TX2 | Datos 2 | **Reloj** |
| TXCK | **Reloj** | Datos 2 |

- Cada salida del PHY recibe los ajustes de la señal que lleva. En HDMI1, la salida TX2 recibe los ajustes de reloj y
  TXCK los de datos.
- `HDMI_TX_PHY_CHANNEL_SWAP` lleva el número de señal (0, 1, 2, 3 = reloj) de cada salida del PHY.

**Campos de los registros del PHY y del RM** [`vc4_hdmi_phy.c`, definiciones al principio del fichero]

| Registro | Campos |
|---|---|
| `TX_PHY_RESET_CTL` | 0 `TX_0_RESET`; 1 `TX_1_RESET`; 2 `TX_2_RESET`; 3 `TX_CK_RESET`; 4 `PLLDIV_RESETB`; 5 `PLL_RESETB`. Los bits 4 y 5 son activos a 0 |
| `TX_PHY_POWERDOWN_CTL` | 4 `RNDGEN_PWRDN`. Linux también escribe el bit 10, sin nombre |
| `TX_PHY_CTL_0` | Para TX0, TX1, TX2 y TXCK: preénfasis (3 bits) y corriente principal (5 bits). TX2: 31:29 y 28:24. TX1: 23:21 y 20:16. TX0: 15:13 y 12:8. TXCK: 7:5 y 4:0 |
| `TX_PHY_CTL_1` | `RES_SEL`: TX2 19:15; TX1 14:10; TX0 9:5; TXCK 4:0 |
| `TX_PHY_CTL_2` | 19:16 `VCO_GAIN`; `TERM_RES_SEL`: TX2 15:12; TX1 11:8; TX0 7:4; TXCK 3:0 |
| `TX_PHY_CTL_3` | 19:17 `RP`; 16:12 `RZ`; 11:10 `CP1`; 9:8 `CP`; 7:6 `CZ`; 5:0 `ICP` |
| `TX_PHY_PLL_CTL_0` | 13 `MASH11_MODE`; 12 `VC_RANGE_EN`; 11 `EMULATE_VC_LOW`; 10 `EMULATE_VC_HIGH`; 9 `VCO_SEL`; 8 `VCO_FB_DIV2`; 7 `VCO_POST_DIV2`; 6 `VCO_CONT_EN`; 5 `ENA_VCO_CLK` |
| `TX_PHY_PLL_CTL_1` | 27:16 `CPP`; 15:14 `FREQ_DOUBLER_DELAY`; 13 `FREQ_DOUBLER_ENABLE`; 12:11 `POST_RST_SEL` |
| `TX_PHY_CLK_DIV` | 15:8 `VCO`: el divisor `d` |
| `TX_PHY_PLL_CFG` | 3:0 `PDIV` |
| `TX_PHY_CHANNEL_SWAP` | 13:12 `TXCK_OUT_SEL`; 9:8 `TX2_OUT_SEL`; 5:4 `TX1_OUT_SEL`; 1:0 `TX0_OUT_SEL` |
| `TX_PHY_PLL_CALIBRATION_CONFIG_1` | 27:0 `MIN_LIMIT` |
| `TX_PHY_PLL_CALIBRATION_CONFIG_2` | 27:0 `MAX_LIMIT` |
| `TX_PHY_PLL_CALIBRATION_CONFIG_4` | 31:16 `STABLE_THRESHOLD`; 15:0 `HOLD_THRESHOLD` |
| `RM_CONTROL` | 19 `EN_FREEZE_COUNTERS`; 17 `EN_LOAD_INTEGRATOR`; 4 `FREE_RUN` |
| `RM_OFFSET` | 31 `ONLY`; 30:0 `OFFSET` |
| `RM_FORMAT` | 25:24 `SHIFT` |

**Secuencia de programación** [`vc4_hdmi_phy.c`, `vc5_hdmi_phy_init` y `vc5_hdmi_reset_phy`]

"OR" significa leer, poner esos bits a 1 sin tocar los demás y escribir.

1. `TX_PHY_RESET_CTL` = `0x0F` (las cuatro salidas en reinicio y el PLL también, porque sus bits activos a 0 quedan a
   0).
2. `TX_PHY_POWERDOWN_CTL` = `0x400`.
3. `TX_PHY_POWERDOWN_CTL` = `0x10` (todo encendido salvo el generador aleatorio).
4. `TX_PHY_RESET_CTL`: poner a 0 los bits 0 a 3 (lectura, modificación y escritura).
5. `RM_CONTROL`: OR `0x000A0010` (`EN_FREEZE_COUNTERS`, `EN_LOAD_INTEGRATOR`, `FREE_RUN`).
6. `TX_PHY_PLL_CALIBRATION_CONFIG_1`: poner a 0 los bits 27:0.
7. `TX_PHY_PLL_CALIBRATION_CONFIG_2`: poner a 0 los bits 27:0.
8. `RM_OFFSET` = `R` + `ONLY` (bit 31).
9. `TX_PHY_CLK_DIV` = `d` en los bits 15:8.
10. `TX_PHY_PLL_CALIBRATION_CONFIG_4` = `0x0E14E147` (estable `0xE14`, retención `0xE147`).
11. `TX_PHY_PLL_CTL_0` = `ENA_VCO_CLK` + `VCO_CONT_EN` + `MASH11_MODE` + `VCO_SEL`. Vale `0x00002060` u `0x00002260`.
12. `TX_PHY_PLL_CTL_1`: OR `0x008A7800` (`FREQ_DOUBLER_ENABLE`, `POST_RST_SEL` 3, `FREQ_DOUBLER_DELAY` 1,
    `CPP` `0x8A`).
13. `RM_FORMAT`: OR `0x02000000` (`SHIFT` 2).
14. `TX_PHY_PLL_CFG`: OR `0x1` (`PDIV` 1).
15. `TX_PHY_TMDS_CLK_WORD_SEL` = 0 o 3.
16. `TX_PHY_CTL_3` = `ICP` + `CP` 1 + `CP1` 1 + `CZ` 3 + `RP` 4 + `RZ` 6. Vale `0x000865D8` u `0x000865DC`.
17. `TX_PHY_CTL_0` = preénfasis y corrientes de las cuatro salidas.
18. `TX_PHY_CTL_1`: OR con los cuatro `RES_SEL`.
19. `TX_PHY_CTL_2` = los cuatro `TERM_RES_SEL` + `VCO_GAIN`.
20. `TX_PHY_CHANNEL_SWAP` = el orden de las líneas.
21. `TX_PHY_RESET_CTL`: poner a 0 los bits 4 y 5.
22. `TX_PHY_RESET_CTL`: poner a 1 los bits 4 y 5. El PLL sale del reinicio.

Linux no espera a que el PLL se enganche ni lee ningún bit de estado del PHY.

**Apagar el PHY**: `TX_PHY_RESET_CTL` = `0x0F` y `TX_PHY_POWERDOWN_CTL` = `0x400`. [`vc4_hdmi_phy.c`,
`vc5_hdmi_phy_disable`]

**Valores del PLL** (deducidos con las fórmulas)

| `T` | `d` | `V` | `VCO_SEL` | `RM_OFFSET` | `ICP` | `VCO_GAIN` | `WORD_SEL` |
|---|---|---|---|---|---|---|---|
| 25,175 MHz | 12 | 3,021 GHz | 0 | `0x86FE38E3` | `0x1C` | `0xF` | 0 |
| 74,25 MHz | 5 | 3,7125 GHz | 0 | `0x88980000` | `0x18` | `0x6` | 0 |
| 148,5 MHz | 3 | 4,455 GHz | 0 | `0x8A500000` | `0x18` | `0x5` | 0 |
| 297 MHz | 2 | 5,94 GHz | 1 | `0x8DC00000` | `0x18` | `0x2` | 0 |
| 594 MHz | 1 | 5,94 GHz | 1 | `0x8DC00000` | `0x18` | `0x2` | 3 |

**Ejemplo deducido: 1920 × 1080 a 60 Hz** (`T` = 148,5 MHz, fila "75 a 165 MHz")

| Registro | HDMI0 | HDMI1 |
|---|---|---|
| `TX_PHY_CLK_DIV` | `0x00000300` | igual |
| `TX_PHY_PLL_CTL_0` | `0x00002060` | igual |
| `TX_PHY_CTL_3` | `0x000865D8` | igual |
| `TX_PHY_CTL_0` | `0x0909090C` | `0x0C090909` |
| `TX_PHY_CTL_1` (bits que se ponen) | `0x00094A58` | `0x000C4A52` |
| `TX_PHY_CTL_2` | `0x00050003` | `0x00053000` |
| `TX_PHY_CHANNEL_SWAP` | `0x00003210` | `0x00002301` |
| `RM_OFFSET` | `0x8A500000` | igual |
| `TX_PHY_TMDS_CLK_WORD_SEL` | 0 | igual |

### 6.4. Tiempos del controlador HDMI

[`vc4_hdmi.c`, `vc5_hdmi_set_timings` y definiciones `VC5_HDMI_*` al principio del fichero; `vc4_regs.h`,
`VC4_HDMI_VERTB_VBP`]

Sea `rep` la repetición de píxel: 2 en modos doble reloj, 1 en los demás. Los valores horizontales van en píxeles
(no se dividen entre 2, al contrario que en la PV).

| Registro | Campos |
|---|---|
| `HDMI_HORZA` | 28:16 `HFP` = (`hsync_start` − `hdisplay`) · `rep`; 15 `VPOS` = sincronía vertical positiva; 14 `HPOS` = sincronía horizontal positiva; 13:0 `HAP` = `hdisplay` · `rep` |
| `HDMI_HORZB` | 26:16 `HBP` = (`htotal` − `hsync_end`) · `rep`; 10:0 `HSP` = (`hsync_end` − `hsync_start`) · `rep` |
| `HDMI_VERTA0` y `HDMI_VERTA1` (mismo valor) | 28:24 `VSP` = `vsync_end` − `vsync_start`; 22:16 `VFP` = `vsync_start` − `vdisplay`; 12:0 `VAL` = `vdisplay` |
| `HDMI_VERTB0` (campo par) | 29:16 `VSPO` = 0; 8:0 `VBP` = `vtotal` − `vsync_end` |
| `HDMI_VERTB1` (campo impar) | 29:16 `VSPO` = `htotal` / 2 si `rep` = 1, `htotal` si `rep` = 2; 8:0 `VBP` = `vtotal` − `vsync_end` + (1 si es entrelazado) |
| `HDMI_MISC_CONTROL` | 3:0 `PIXEL_REP` = `rep` − 1 (lectura, modificación y escritura) |
| `HDMI_DEEP_COLOR_CONFIG_1` | 10:8 `INIT_PACK_PHASE` = 2; 3:0 `COLOR_DEPTH` = `g` (lectura, modificación y escritura) |
| `HDMI_GCP_WORD_1` | 15:8 = `g`; 7:0 = `0x10` (`CLEAR_AVMUTE`, bit 4) (lectura, modificación y escritura) |
| `HDMI_GCP_CONFIG` | 31 `GCP_ENABLE` a 1 (lectura, modificación y escritura) |

- `g` (profundidad de color en el paquete GCP) = 0 para 8 bits, 5 para 10 bits, 6 para 12 bits. En YCbCr 4:2:2
  siempre 0.
- El bit 4 de `HDMI_GCP_WORD_1` es "quitar el silencio de audio y vídeo" (AVMUTE). El bit 0 sería "poner". Linux
  siempre pone el 4.
- Los anchos de campo limitan los modos: `VSP` hasta 31 líneas, `VFP` hasta 127, `VBP` hasta 511, `HBP` y `HSP` hasta
  2047 píxeles. Se deduce de las máscaras.

Al terminar, Linux escribe `HDMI_CLOCK_STOP` = 0 (bloque `dvp`): vuelve a dejar pasar el reloj de píxel.

**Ejemplo deducido: 1920 × 1080 a 60 Hz, 8 bits, sincronías positivas**

| Registro | Valor |
|---|---|
| `HDMI_HORZA` | `0x0058C780` |
| `HDMI_HORZB` | `0x0094002C` |
| `HDMI_VERTA0` y `HDMI_VERTA1` | `0x05040438` |
| `HDMI_VERTB0` | `0x00000024` |
| `HDMI_VERTB1` | `0x044C0024` |
| `HDMI_MISC_CONTROL`, bits 3:0 | 0 |
| `HDMI_DEEP_COLOR_CONFIG_1`, bits 10:8 y 3:0 | 2 y 0 |
| `HDMI_GCP_WORD_1`, bits 15:0 | `0x0010` |

### 6.5. `HDMI_SCHEDULER_CONTROL`

[`vc4_regs.h`; `vc4_hdmi.c`, `vc4_hdmi_encoder_pre_crtc_configure` y `vc4_hdmi_encoder_post_crtc_enable`]

| Bit | Campo | Uso |
|---|---|---|
| 0 | `MODE_HDMI` | 1 = HDMI (con paquetes de datos). 0 = DVI |
| 1 | `HDMI_ACTIVE` | Estado: el modo HDMI está activo |
| 3 | `VERT_ALWAYS_KEEPOUT` | Linux no lo usa |
| 5 | `IGNORE_VSYNC_PREDICTS` | Linux lo pone siempre |
| 15 | `MANUAL_FORMAT` | Linux lo pone siempre |

### 6.6. `HDMI_VID_CTL` y `HDMI_FIFO_CTL`

**`HDMI_VID_CTL`** (bloque `hd`) [`vc4_regs.h`]

| Bit | Campo | Uso en Linux |
|---|---|---|
| 31 | `ENABLE` | Vídeo del HDMI encendido |
| 30 | `UNDERFLOW_ENABLE` | Se pone al encender |
| 29 | `FRAME_COUNTER_RESET` | Se pone al encender |
| 28 | `VSYNC_LOW` | Sincronía vertical negativa |
| 27 | `HSYNC_LOW` | Sincronía horizontal negativa |
| 24 | `CLRSYNC` | Linux no lo usa |
| 23 | `CLRRGB` | Se pone al encender y al apagar |
| 18 | `BLANKPIX` | Saca píxeles en negro. Se quita al encender y se pone al apagar |
| 16 | `BLANK_INSERT_EN` | Se pone al encender |

**`HDMI_FIFO_CTL`** (bloque `hdmi`) [`vc4_regs.h`; `vc4_hdmi.c`, `vc4_hdmi_recenter_fifo`]

| Bit | Campo |
|---|---|
| 0 | `MASTER_SLAVE_N` |
| 1 | `USE_FULL` |
| 2 | `CAPTURE_PTR` |
| 3 | `INV_CLK_XFR` |
| 4 | `USE_PLL_LOCK` |
| 5 | `FIFO_RESET` |
| 6 | `RECENTER` |
| 7 | `ON_VB` |
| 13 | `USE_EMPTY` |
| 14 | `RECENTER_DONE` (estado) |

Solo los bits que cubre la máscara `0xEFFF` se pueden escribir. [`vc4_regs.h`, `VC4_HDMI_FIFO_VALID_WRITE_MASK`]

**Recentrar la FIFO** [`vc4_hdmi.c`, `vc4_hdmi_recenter_fifo`]

1. Leer `HDMI_FIFO_CTL` y quedarse con los bits de la máscara `0xEFFF`. Llamarlo `f`.
2. Escribir `f` sin `RECENTER`. Escribir `f` con `RECENTER`.
3. Esperar entre 1 y 1,1 ms.
4. Repetir el paso 2.
5. Esperar hasta 1 ms a que `RECENTER_DONE` se lea a 1. Si no, Linux solo avisa.

## 7. Encender una salida HDMI

Secuencia completa para encender HDMI0 o HDMI1 con un modo, por ejemplo 1920 × 1080 a 60 Hz. Mezcla lo que hace
Linux en el HVS (`hvs.md`), la PV y el HDMI, en el orden de Linux. [`vc4_kms.c`, `vc4_atomic_commit_tail`;
`vc4_crtc.c`, `vc4_crtc_atomic_enable`; `vc4_hdmi.c`, `vc4_hdmi_encoder_pre_crtc_configure`,
`vc4_hdmi_encoder_pre_crtc_enable` y `vc4_hdmi_encoder_post_crtc_enable`]

**Una vez, al arrancar** (deducido del orden de Linux en el arranque)

1. Pedir `NOTIFY_DISPLAY_DONE` al firmware (`hvs.md`, 12.1).
2. Asegurar el HSM: `SET_CLOCK_STATE` del reloj 13 a encendido. Leer su frecuencia. Si es menor de 120 MHz, fijar
   120 MHz. **No tocar ningún registro del HDMI antes de esto.**
3. Abrir la puerta del reloj de audio: poner a 0 el bit 3 (HDMI0) o el bit 4 (HDMI1) de `0x7EF00008`.
4. Reiniciar el HDMI (4.5).
5. Escribir `HDMI_RAM_PACKET_CONFIG` = `0x00010000` (RAM de paquetes encendida, ningún paquete activo).
   [`vc4_hdmi.c`, `vc4_hdmi_runtime_resume`]
6. Iniciar `aon_intr` y habilitar los bits de HPD (sección 5).
7. Iniciar el I2C del DDC (sección 9.2).
8. En cada PV: `PV_INTEN` = 0 y `PV_INTSTAT` = `0x80`.
9. Si el firmware dejó la salida encendida, apagarla (secciones 2.5 y 8).

**Para cada encendido**

1. Detectar el monitor y leer el EDID (sección 9). Decidir si el monitor es HDMI o DVI y si admite SCDC.
2. Validar el modo (6.2) y calcular `T` (6.1).
3. Subir el reloj `core` lo necesario (`hvs.md`, sección 11).
4. Elegir canal del HVS y escribir el mux de la salida (`hvs.md`, 10.3 y 10.4).
5. Escribir la lista del HVS y `DISPBKGNDx.FILL` (`hvs.md`, 12.3, paso 3).
6. Habilitar `VFP_START` en la PV: `PV_INTEN` = `0x80`.
7. Iniciar el canal del HVS: `DISPLISTx`, reinicio, `DISPCTRLx` y `DISPBKGNDx` (`hvs.md`, 12.3, pasos 5 a 8).
8. Pedir el HSM mínimo de 4.3. Fijarlo con `SET_CLOCK_RATE` del reloj 13, teniendo en cuenta la otra salida.
9. Pedir el BVB mínimo de 4.3 y encenderlo (reloj 14).
10. Programar el PHY (6.3).
11. `HDMI_SCHEDULER_CONTROL`: OR `0x00008020` (`MANUAL_FORMAT` e `IGNORE_VSYNC_PREDICTS`).
12. Escribir los tiempos del HDMI (6.4). El último paso deja `HDMI_CLOCK_STOP` = 0.
13. Programar la PV sin encenderla (2.5, "Programar un modo").
14. Encender la PV: poner `EN` en `PV_CONTROL`.
15. Configurar el color: CSC y cruce de canales (10.1).
16. `HDMI_FIFO_CTL` = `0x00000001` (`MASTER_SLAVE_N`).
17. Poner `VIDEN` en `PV_V_CONTROL`.
18. `HDMI_VID_CTL` (lectura, modificación y escritura): quitar `VSYNC_LOW` y `HSYNC_LOW`. Poner `ENABLE`,
    `CLRRGB`, `UNDERFLOW_ENABLE`, `FRAME_COUNTER_RESET` y `BLANK_INSERT_EN` (OR `0xE0810000`). Poner `VSYNC_LOW` si
    la sincronía vertical es negativa y `HSYNC_LOW` si la horizontal es negativa.
19. `HDMI_VID_CTL`: quitar `BLANKPIX`.
20. Si el monitor es HDMI: poner `MODE_HDMI` en `HDMI_SCHEDULER_CONTROL` y esperar hasta 1000 ms a que
    `HDMI_ACTIVE` se lea a 1. Si es DVI: quitar `MODE_HDMI` y esperar hasta 1000 ms a que `HDMI_ACTIVE` se lea a 0.
    Si no llega, Linux solo avisa.
21. Si el monitor es HDMI: escribir los paquetes de información (10.2). Como mínimo el AVI.
22. Recentrar la FIFO del HDMI (6.6).
23. Si `T` > 340 MHz y el monitor admite scrambling: activarlo (sección 12).
24. Esperar al primer `VFP_START`. Comprobar `DISPLACTx` y activar el aviso de vaciado del canal (`hvs.md`, 12.3,
    paso 12).
25. Bajar el reloj `core` al valor necesario (`hvs.md`, sección 11).

Notas:

- En Linux, los pasos 3 a 5 y 25 los hace el código común del cambio de estado. Los pasos 6 a 24 los hace el
  encendido de la PV, que llama a los del HDMI en los momentos indicados.
- Linux no comprueba el enganche del PLL ni espera entre el PHY y la PV.
- Si una salida ya estaba encendida con otro modo, primero se apaga entera (sección 8). Linux no cambia de modo en
  caliente.

## 8. Apagar una salida HDMI

[`vc4_crtc.c`, `vc4_crtc_atomic_disable` y `vc4_crtc_disable`; `vc4_hdmi.c`, `vc4_hdmi_encoder_post_crtc_disable`,
`vc4_hdmi_encoder_post_crtc_powerdown` y `vc4_hdmi_runtime_suspend`]

1. Desactivar la interrupción de la PV: `PV_INTEN` = 0.
2. Quitar `VIDEN` en `PV_V_CONTROL` y esperar hasta 1 ms a que se lea a 0.
3. Esperar 20 ms (2.5).
4. Apagar el vídeo del HDMI:
   1. `HDMI_RAM_PACKET_CONFIG` = `0x00010000` (para todos los paquetes).
   2. `HDMI_VID_CTL`: poner `CLRRGB`.
   3. Esperar 1 ms.
   4. `HDMI_VID_CTL`: quitar `ENABLE`.
   5. Si el scrambling estaba activo, desactivarlo (sección 12).
5. Reiniciar la PV: quitar `EN` y poner `FIFO_CLR`.
6. Parar el canal del HVS (`hvs.md`, 12.6, pasos 6 y 7).
7. Apagar el lado del HDMI:
   1. `HDMI_VID_CTL`: poner `BLANKPIX`.
   2. Apagar el PHY (6.3).
   3. Soltar el BVB (y el reloj de píxel, que en la BCM2711 no hace nada).
8. Si ya nadie usa el HDMI (Linux lo hace cuando no queda ningún uso): `HDMI_RAM_PACKET_CONFIG` = 0, cerrar la puerta
   del reloj de audio y soltar el HSM.
9. Desconectar el mux de la salida y liberar el canal (`hvs.md`, 12.6, paso 8). Bajar el reloj `core`.

Se deduce que **DashOS debería dejar el HSM encendido** mientras quiera detectar monitores, porque leer
`HDMI_HOTPLUG` necesita el HSM (sección 9.1). Linux lo enciende y lo apaga alrededor de cada detección.

## 9. Detección de conexión y EDID

### 9.1. HPD

- **Registro**: `HDMI_HOTPLUG` (bloque `hdmi`, `0x1A8`). Bit 0 `CONNECTED` = hay un monitor.
  [`vc4_hdmi.c`, `vc5_hdmi_hp_detect`; `vc4_regs.h`]
- La Pi 4 no tiene un GPIO de HPD en el árbol de dispositivos. Linux usaría el GPIO si lo hubiera. [`vc4_hdmi.c`,
  `vc4_hdmi_bind`; DTB]
- Leer el registro necesita el HSM encendido. Linux lo enciende antes de leer y lo suelta después.
  [`vc4_hdmi.c`, `vc4_hdmi_connector_detect_ctx`]
- **Interrupciones**: `hpd-connected` y `hpd-removed` del `aon_intr` (sección 5). Linux usa las dos con el mismo
  manejador: vuelve a leer `HDMI_HOTPLUG` y, si hay monitor, relee el EDID. No espera ningún tiempo de rebote.
  [`vc4_hdmi.c`, `vc4_hdmi_hotplug_init` y `vc4_hdmi_hpd_irq_thread`]
- Al reconectar un monitor con el scrambling activo, Linux lee el registro SCDC del monitor. Si el scrambling no
  coincide con lo esperado, apaga y vuelve a encender la salida entera. [`vc4_hdmi.c`, `vc4_hdmi_reset_link`]

### 9.2. Controlador I2C del DDC

Cada HDMI tiene su propio controlador I2C (Broadcom "BSC"): `ddc0` para HDMI0 y `ddc1` para HDMI1. No son los I2C de
uso general de la Pi. [DTB, propiedad `ddc` de los nodos HDMI; `i2c-brcmstb.c`]

| Dato | `ddc0` | `ddc1` |
|---|---|---|
| Compatible | `brcm,bcm2711-hdmi-i2c` | igual |
| BSC | `0x7EF04500` | `0x7EF09500` |
| auto-i2c | `0x7EF00B00` | `0x7EF05B00` |
| Velocidad | 97 500 Hz (`clock-frequency`) | igual |
| Interrupción | Ninguna: Linux trabaja por sondeo | igual |

**Registros del BSC** [`i2c-brcmstb.c`, estructura `bsc_regs` y definiciones `BSC_*`]

| Desp. | Registro | Campos |
|---|---|---|
| `0x00` | `CHIP_ADDRESS` | Dirección de 8 bits: dirección I2C de 7 bits desplazada 1 a la izquierda, más 1 si es lectura |
| `0x04` a `0x20` | `DATA_IN0` a `DATA_IN7` | Datos que se envían. 4 bytes por registro; el primero en los bits 7:0 |
| `0x24` | `CNT_REG` | 5:0 número de bytes de la transferencia (hasta 32) |
| `0x28` | `CTL_REG` | 1:0 `DTF` (0 escribir, 1 leer, 2 leer y escribir, 3 escribir y leer); 5:4 `SCL_SEL`; 6 `INT_EN`; 7 `DIV_CLK` |
| `0x2C` | `IIC_ENABLE` | 0 `ENABLE` (arranca); 1 `INTRP` (terminada); 2 `NOACK`; 4 `NOSTOP`; 5 `NOSTART`; 6 `RESTART` |
| `0x30` a `0x4C` | `DATA_OUT0` a `DATA_OUT7` | Datos recibidos. 4 bytes por registro; el primero en los bits 7:0 |
| `0x50` | `CTLHI_REG` | 0 `WAIT_DIS`; 1 `IGNORE_ACK`; 6 `DATAREG_SIZE` (1 = 4 bytes por registro); 7 `INPUT_SWITCHING_LEVEL` |
| `0x54` | `SCL_PARAM` | Reservado |

**Velocidades** (`SCL_SEL` y `DIV_CLK`): `SCL_SEL` 0 = 375 kHz, 1 = 390 kHz, 2 = 187,5 kHz, 3 = 200 kHz. Con
`DIV_CLK` a 1 se dividen entre 4. Los 97 500 Hz de la Pi 4 son `SCL_SEL` 1 con `DIV_CLK` 1: `CTL_REG` = `0x90`.
[`i2c-brcmstb.c`, tabla `bsc_clk`]

**Iniciar** [`i2c-brcmstb.c`, `bcm2711_release_bsc`, `brcmstb_i2c_probe` y `brcmstb_i2c_set_bsc_reg_defaults`]

1. Liberar el BSC de la máquina automática: escribir `0x2` (`RELEASE_BSC`, bit 1) en el desplazamiento `0x26C` del
   bloque auto-i2c (`0x7EF00D6C` para `ddc0`, `0x7EF05D6C` para `ddc1`).
2. `IIC_ENABLE` = 0.
3. `CTL_REG` = 0 (interrupción apagada).
4. `CTLHI_REG` = `0x40` (registros de datos de 4 bytes).
5. `CTL_REG` = `0x90` (97 500 Hz).

**Una transferencia de hasta 32 bytes** [`i2c-brcmstb.c`, `brcmstb_i2c_xfer_bsc_data` y `brcmstb_send_i2c_cmd`]

1. Escribir `CTLHI_REG` (con `IGNORE_ACK` solo si se quiere ignorar el NACK).
2. Preparar `CTL_REG`: `DTF` = 0 para escribir o 1 para leer.
3. `CNT_REG` = número de bytes.
4. Si es escritura, llenar `DATA_IN0` en adelante.
5. Esperar hasta 100 ms a que `IIC_ENABLE.INTRP` esté a 0.
6. Poner `INT_EN` en `CTL_REG` y escribirlo. Linux lo hace aunque trabaje por sondeo.
7. Escribir `IIC_ENABLE` con los bits de condición (`RESTART`, `NOSTART`, `NOSTOP`) y `ENABLE`.
8. Esperar hasta 100 ms a que `IIC_ENABLE.INTRP` se lea a 1.
9. Si `IIC_ENABLE.NOACK` está a 1, el dispositivo no respondió.
10. Escribir `CNT_REG` = 0 e `IIC_ENABLE` = 0.
11. Si es lectura, leer `DATA_OUT0` en adelante.

Siempre que no se ponga `NOSTART`, el controlador envía la condición de inicio y la dirección de `CHIP_ADDRESS`. Se
deduce de que Linux solo escribe la dirección una vez por mensaje.

**Mensajes más largos y varios mensajes seguidos** [`i2c-brcmstb.c`, `brcmstb_i2c_xfer`]

- Al empezar cada mensaje, Linux escribe `CHIP_ADDRESS`.
- Un mensaje de más de 32 bytes se parte en trozos de 32.
  - Primer trozo: sin `NOSTART` (envía inicio y dirección) y con `NOSTOP`.
  - Trozos intermedios: `NOSTART` y `NOSTOP`.
  - Último trozo del último mensaje: solo `NOSTART` (termina con parada).
- Si después viene otro mensaje, el último trozo lleva `RESTART` y `NOSTOP`, y el siguiente mensaje empieza con su
  propio inicio y dirección.

**Leer un bloque de EDID** (protocolo E-DDC de VESA; no sale de vc4, sino del estándar y del uso que hace el núcleo
DRM de este controlador):

- Dirección de 7 bits `0x50` (bytes `0xA0` para escribir y `0xA1` para leer).
- Bloque `b` de 128 bytes: si `b` ≥ 2, escribir antes el segmento `b` / 2 en la dirección `0x30`. Después escribir el
  desplazamiento (`b` mod 2) · 128 en `0x50` y leer 128 bytes de `0x50`.
- Con este controlador, la lectura de 128 bytes son cuatro trozos de 32.
- El byte 126 del bloque 0 dice cuántos bloques de extensión siguen.
- Cada bloque suma 0 módulo 256.
- La extensión CTA-861 dice si el monitor es HDMI (bloque de datos del fabricante HDMI) y si admite SCDC y scrambling
  (bloque del HDMI Forum). Linux usa estos datos en `vc4_hdmi_supports_scrambling`.

## 10. Color e información de vídeo

### 10.1. CSC y cruce de canales

[`vc4_hdmi.c`, `vc5_hdmi_csc_setup`, `vc5_hdmi_set_csc_coeffs` y tablas `vc5_hdmi_csc_*`; `vc4_regs.h`]

**Campos**

| Registro | Campos |
|---|---|
| `HDMI_CSC_CTL` | 1:0 `MODE` (3 = matriz propia); 2 `ENABLE`; 3 `USE_RNG_SUPPRESSION`; 5:4 `FILTER_MODE_444_TO_422` (3 = estándar); 6 `USE_444_TO_422` |
| `HDMI_CSC_CHANNEL_CTL` | 7:6 `OUTPUT_REMAP` (2 = estilo antiguo, para 4:2:2) |
| `HDMI_VEC_INTERFACE_CFG` | 3:2 `SEL_422` (2 = 4:2:2 antiguo) |
| `HDMI_VEC_INTERFACE_XBAR` | Cruce de componentes. Sin campos documentados |
| `HDMI_CSC_12_11` | 31:16 coeficiente fila 1, columna 2; 15:0 fila 1, columna 1 |
| `HDMI_CSC_14_13` | 31:16 desplazamiento de la fila 1; 15:0 fila 1, columna 3 |
| `HDMI_CSC_22_21` y `_24_23` | Lo mismo para la fila 2 |
| `HDMI_CSC_32_31` y `_34_33` | Lo mismo para la fila 3 |

- Los coeficientes son de 16 bits con signo en formato 2.13: `0x2000` vale 1,0.
- Los desplazamientos son de 16 bits con signo en formato 9.6: `0x0400` vale 16.

**Salida RGB** (la recomendada):

- `HDMI_VEC_INTERFACE_CFG` = 0.
- `HDMI_VEC_INTERFACE_XBAR` = `0x00354021`. (Para YCbCr Linux escribe `0x00543210`.)
- `HDMI_CSC_CHANNEL_CTL` = 0.
- `HDMI_CSC_CTL` = `0x00000007` (`ENABLE` y `MODE` 3).
- Coeficientes, según el rango:

| Registro | Rango completo (0 a 255) | Rango limitado (16 a 235) |
|---|---|---|
| `HDMI_CSC_12_11` | `0x00002000` | `0x00001B80` |
| `HDMI_CSC_14_13` | `0x00000000` | `0x04000000` |
| `HDMI_CSC_22_21` | `0x20000000` | `0x1B800000` |
| `HDMI_CSC_24_23` | `0x00000000` | `0x04000000` |
| `HDMI_CSC_32_31` | `0x00000000` | `0x00000000` |
| `HDMI_CSC_34_33` | `0x00002000` | `0x04001B80` |

El rango limitado multiplica cada componente por 0,8594 y suma 16.

**Cuándo usa Linux el rango limitado** [`drm_hdmi_state_helper.c`, `hdmi_is_limited_range`]

- Nunca si el monitor es DVI.
- Siempre en salidas YCbCr.
- En RGB, si el usuario no fuerza nada: limitado en todos los modos de la norma CTA-861 salvo el VIC 1 (640 × 480).
  Por tanto, 1080p60 sale con rango limitado por defecto.
- El rango se anuncia al monitor en el paquete AVI (10.3).

**YCbCr** (resumen): Linux usa matrices BT.601, BT.709 o BT.2020, en rango completo o limitado. En 4:4:4 escribe las
filas rotadas: la fila 2 de la matriz en los registros de la fila 1, la 3 en los de la 2 y la 1 en los de la 3. En
4:2:2 añade `USE_444_TO_422`, `FILTER_MODE` 3, `USE_RNG_SUPPRESSION`, `OUTPUT_REMAP` 2 y `SEL_422` 2. Los
coeficientes están en las tablas `vc5_hdmi_csc_full_rgb_to_yuv_*` de `vc4_hdmi.c` y son los de las normas, en
formato 2.13.

### 10.2. RAM de paquetes

[`vc4_hdmi.c`, `vc4_hdmi_write_infoframe`, `vc4_hdmi_stop_packet` y `vc4_hdmi_clear_infoframe`; `vc4_regs.h`]

- La RAM de paquetes guarda paquetes de datos HDMI (infoframes). Cada uno se habilita por separado y el controlador
  lo envía en cada cuadro.
- Cada paquete ocupa **36 bytes (9 palabras)**. El paquete número `n` empieza en la base del bloque `packet` + 36 ·
  `n`. El bloque mide 512 bytes: caben 14 paquetes. Se deduce del tamaño.
- El número de paquete de un infoframe es su tipo menos `0x80`: fabricante (`0x81`) 1, AVI (`0x82`) 2, SPD (`0x83`)
  3, audio (`0x84`) 4, metadatos HDR (`0x87`) 7.
- `HDMI_RAM_PACKET_CONFIG`: el bit 16 enciende la RAM. El bit `n` activa el paquete `n`.
- `HDMI_RAM_PACKET_STATUS`: el bit `n` dice que el paquete `n` se está enviando.

**Disposición de un paquete** (deducida del bucle de escritura de Linux):

| Palabra | Bits 7:0 | 15:8 | 23:16 | 31:24 |
|---|---|---|---|---|
| 0 | HB0 (tipo) | HB1 (versión) | HB2 (longitud) | 0 |
| 1 | PB0 (suma de comprobación) | PB1 | PB2 | PB3 |
| 2 | PB4 | PB5 | PB6 | 0 |
| 3 | PB7 | PB8 | PB9 | PB10 |
| 4 | PB11 | PB12 | PB13 | 0 |
| 5 | PB14 | PB15 | PB16 | PB17 |
| 6 | PB18 | PB19 | PB20 | 0 |
| 7 | PB21 | PB22 | PB23 | PB24 |
| 8 | PB25 | PB26 | PB27 | 0 |

Es decir: una palabra de cabecera y cuatro subpaquetes de 7 bytes, cada uno repartido en 4 + 3 bytes.

**Escribir un paquete**

1. La RAM tiene que estar encendida (bit 16) y el modo HDMI activo.
2. Quitar el bit `n` de `HDMI_RAM_PACKET_CONFIG`.
3. Esperar hasta 100 ms a que el bit `n` de `HDMI_RAM_PACKET_STATUS` se lea a 0.
4. Escribir las palabras del paquete. Poner a 0 las palabras que sobren, hasta completar las 9. Linux explica que si
   no, un analizador HDMI da error de suma de comprobación.
5. Poner el bit `n` en `HDMI_RAM_PACKET_CONFIG`.
6. Esperar hasta 100 ms a que el bit `n` de `HDMI_RAM_PACKET_STATUS` se lea a 1.

**Borrar un paquete**: pasos 2 y 3.

### 10.3. Qué paquetes manda Linux

Los construye el núcleo DRM según CTA-861. Al encender en modo HDMI, Linux manda el AVI y, si tocan, SPD, el del
fabricante HDMI y el HDR. El de audio lo manda al preparar el audio. [`vc4_hdmi.c`,
`vc4_hdmi_encoder_post_crtc_enable` y `vc4_hdmi_audio_prepare`]

Contenido mínimo del AVI (del estándar CTA-861, no de vc4): tipo `0x82`, versión 2, longitud 13, suma de
comprobación, espacio de color (RGB = 0), rango de cuantificación (completo o limitado, que tiene que coincidir con
10.1) y el número VIC del modo.

## 11. Audio por HDMI

[`vc4_hdmi.c`, `vc4_hdmi_audio_startup`, `vc4_hdmi_audio_prepare`, `vc4_hdmi_audio_set_mai_clock`,
`vc4_hdmi_set_n_cts`, `vc4_hdmi_audio_shutdown`, `vc4_hdmi_audio_reset` y `vc4_hdmi_audio_init`; `vc4_regs.h`]

- Todo el audio lo lleva el propio HDMI. Un DMA escribe las muestras en `HDMI_MAI_DATA` (bloque `hd`). Un bus interno
  (MAI) las lleva al codificador, que las mete en los borrados del vídeo. [`vc4_hdmi.c`, comentario "VC4 Falcon HDMI
  module"]
- Solo hay audio si el monitor es HDMI. Con DVI, Linux no lo ofrece. [`vc4_hdmi.c`, `vc4_hdmi_audio_can_stream`]
- Linux admite de 1 a 8 canales a 32; 44,1; 48; 88,2; 96; 176,4 y 192 kHz.
- **Formato de las muestras**: cada muestra es una palabra de 32 bits en forma de subtrama IEC 60958 completa, en
  little-endian. El preámbulo del primer cuadro de cada bloque ("B") lleva el identificador 8, el mismo que usa
  alsa-lib. [`vc4_hdmi.c`, `vc4_hdmi_audio_cpu_dai_drv` y comentario de `B_FRAME_IDENTIFIER`]

**Campos**

| Registro | Campos |
|---|---|
| `HDMI_MAI_CTL` | 0 `RESET`; 1 `ERRORF` (error de FIFO llena, 1 borra); 2 `ERRORE` (error de FIFO vacía, 1 borra); 3 `ENABLE`; 7:4 `CHNUM` (canales); 8 `PAREN`; 9 `FLUSH`; 10 `EMPTY`; 11 `FULL`; 12 `WHOLSMP`; 13 `CHALIGN`; 14 `BUSY`; 15 `DLATE` (las muestras llegan tarde, 1 borra) |
| `HDMI_MAI_THR` | 29:24 `PANICHIGH`; 21:16 `PANICLOW`; 13:8 `DREQHIGH`; 5:0 `DREQLOW` |
| `HDMI_MAI_FMT` | 23:16 formato (2 = PCM, 200 = HBR); 15:8 código de frecuencia |
| `HDMI_MAI_SMP` | 31:8 `N`; 7:0 `M`. El periodo de muestreo tiende a `N` / (`M` + 1) ciclos del reloj de audio |
| `HDMI_MAI_CONFIG` | 27 `FORMAT_REVERSE`; 26 `BIT_REVERSE`; 15:0 máscara de canales |
| `HDMI_MAI_CHANNEL_MAP` | 4 bits por canal del MAI: a qué canal HDMI va |
| `HDMI_AUDIO_PACKET_CONFIG` | 29 `ZERO_DATA_ON_SAMPLE_FLAT`; 24 `ZERO_DATA_ON_INACTIVE_CHANNELS`; 19 `FORCE_SAMPLE_PRESENT`; 18 `FORCE_B_FRAME`; 13:10 `B_FRAME_IDENTIFIER`; 9 `AUDIO_LAYOUT`; 8 `FORCE_AUDIO_LAYOUT`; 7:0 máscara de canales CEA |
| `HDMI_CRP_CFG` | 26 `USE_MAI_BUS_SYNC_FOR_CTS`; 25 `DISABLE` (no manda paquetes de reloj de audio); 24 `EXTERNAL_CTS_EN`; 19:0 `N` |
| `HDMI_CTS_0` y `HDMI_CTS_1` | CTS de 20 bits. El hardware alterna entre los dos |

Códigos de frecuencia de `HDMI_MAI_FMT`: 0 sin indicar, 1 8 kHz, 2 11,025, 3 12, 4 16, 5 22,05, 6 24, 7 32,
8 44,1, 9 48, 10 64, 11 88,2, 12 96, 13 128, 14 176,4, 15 192 kHz.

**Fórmulas**

- `HDMI_MAI_SMP`: la mejor fracción `N` / `M` que aproxima (reloj de audio) / (frecuencia de muestreo), con `N` <
  2²⁴ y `M` ≤ 256. Se escribe `N` y `M` − 1. El reloj de audio es de 108 MHz.
  - 48 kHz: 108 000 000 / 48 000 = 2250. `N` = 2250, `M` = 1. Valor `0x0008CA00`.
  - 44,1 kHz: 108 000 000 / 44 100 = 120 000 / 49. `N` = 120 000, `M` = 49. Valor `0x01D4C030`.
- **N del reloj de audio**: `n` = 128 · fs / 1000 (división entera). 48 kHz: 6144. 44,1 kHz: 5644.
- **CTS**: `cts` = `P` · `n` / (128 · fs), con `P` el reloj de píxel del modo en Hz. Para 48 kHz sale `P` / 1000:
  148 500 para 1080p60.

**Empezar** (al abrir el dispositivo de audio)

1. `HDMI_MAI_CTL` = `0x00008207` (`RESET`, `FLUSH`, `DLATE`, `ERRORE`, `ERRORF`).
2. Encender el generador aleatorio del PHY: quitar el bit 4 de `TX_PHY_POWERDOWN_CTL`. Linux no explica para qué
   sirve.

**Preparar** (con frecuencia y canales conocidos). Ejemplo para 2 canales a 48 kHz:

1. `HDMI_MAI_SMP` según la fórmula (`0x0008CA00`).
2. `HDMI_MAI_CTL` = `CHNUM` + `WHOLSMP` + `CHALIGN` + `ENABLE` (`0x00003028`).
3. `HDMI_MAI_FMT` = formato y código de frecuencia (`0x00020900`). Se usa HBR solo si el flujo no es PCM y tiene 8
   canales.
4. `HDMI_MAI_THR` = `0x10101C1C` (pánico 16 y 16, petición de DMA 28 y 28).
5. `HDMI_MAI_CONFIG` = `BIT_REVERSE` + `FORMAT_REVERSE` + máscara (`0x0C000003`).
6. `HDMI_MAI_CHANNEL_MAP` = el canal `i` va al `i`, 4 bits cada uno (`0x00000010`).
7. `HDMI_AUDIO_PACKET_CONFIG` = `ZERO_DATA_ON_SAMPLE_FLAT` + `ZERO_DATA_ON_INACTIVE_CHANNELS` +
   `B_FRAME_IDENTIFIER` 8 + máscara (`0x21002003`).
8. `HDMI_CRP_CFG` = `EXTERNAL_CTS_EN` + `n` (`0x01001800`).
9. `HDMI_CTS_0` = `HDMI_CTS_1` = `cts`.
10. Mandar el infoframe de audio (paquete 4, sección 10.2).
11. Arrancar el DMA.

**DMA**

| Dato | HDMI0 | HDMI1 |
|---|---|---|
| Destino (dirección de bus de `HDMI_MAI_DATA`) | `0x7EF2001C` | `0x7EF2003C` |
| DREQ (controlador DMA clásico, árbol de Linux) | 10 | 17 |
| Árbol del firmware | DMA de 40 bits (`dma@7e007b00`), especificador `0x41fa000a` | `0x41fa0011` |

- Escrituras de 4 bytes, ráfagas de 2. [`vc4_hdmi.c`, `vc4_hdmi_audio_init`]
- La dirección de destino es la de bus del bloque `hd` más el desplazamiento de `HDMI_MAI_DATA`. Linux la saca del
  árbol de dispositivos a propósito, porque el DMA no ve las direcciones físicas de la CPU.
- El DMA tiene que avanzar al ritmo de la petición del periférico (DREQ) en el destino. Se deduce del uso habitual de
  ese controlador. El controlador DMA queda fuera de este documento.

**Parar**

1. `HDMI_MAI_CTL` = `0x00008006` (`DLATE`, `ERRORE`, `ERRORF`).
2. Apagar el generador aleatorio: poner el bit 4 de `TX_PHY_POWERDOWN_CTL`.
3. Quitar el paquete de audio (bit 4 de `HDMI_RAM_PACKET_CONFIG`) sin esperar.
4. `HDMI_MAI_CTL` = `0x1` (`RESET`), después `0x2` (`ERRORF`) y después `0x200` (`FLUSH`).

## 12. Modos 4K y scrambling

**Qué cambia entre 4K a 30 Hz y a 60 Hz** (3840 × 2160, 8 bits RGB; valores deducidos)

| Dato | 4K a 30 Hz (VIC 95) | 4K a 60 Hz (VIC 97) |
|---|---|---|
| `T` | 297 MHz | 594 MHz |
| Salidas que lo admiten | HDMI0 y HDMI1 | Solo HDMI0 |
| Máximo del reloj `core` necesario | Sin requisito | ≥ 550 MHz (6.2) |
| HSM | 299,97 MHz | 599,94 MHz |
| BVB | 150 MHz | 300 MHz |
| `d`, `V`, `VCO_SEL` | 2; 5,94 GHz; 1 | 1; 5,94 GHz; 1 |
| `TMDS_CLK_WORD_SEL` | 0 | 3 |
| Fila de ajustes del PHY | 250 a 340 MHz | 450 a 600 MHz |
| Scrambling | No | Sí |
| Carga de FIFO del canal (HDMI0, `hvs.md` 11) | 267,3 MHz | 534,6 MHz |

- La última fila explica el requisito del reloj `core`: a 4K60 el canal pide unos 535 MHz y el reloj tiene que
  llegar. Se deduce.
- Linux ofrece 4K60 solo si el monitor admite SCDC y scrambling. Sin eso, el modo pasa la validación, pero el
  scrambling no se activa. Se deduce que el monitor no verá imagen. [`vc4_hdmi.c`, `vc4_hdmi_enable_scrambling`]

**SCDC** (registros del monitor, por el mismo bus DDC; del estándar HDMI 2.0 y de `drm_scdc.h`):

- Dirección I2C de 7 bits `0x54`.
- Registro `0x20` (`TMDS_CONFIG`): bit 1 = relación de reloj TMDS 1/40; bit 0 = scrambling activo.
- Registro `0x21` (`SCRAMBLER_STATUS`): bit 0 = el monitor detecta el scrambling.

**Activar el scrambling** (al final del encendido, paso 23 de la sección 7) [`vc4_hdmi.c`,
`vc4_hdmi_enable_scrambling` y `vc4_hdmi_scrambling_wq`; `drm_scdc_helper.c`]

1. Comprobar que el monitor admite SCDC y scrambling (EDID) y que `T` > 340 MHz.
2. En el monitor: leer `0x20`, poner el bit 1 y escribir. Esperar entre 1 y 2 ms. El estándar pide de 1 a 100 ms.
3. En el monitor: leer `0x20`, poner el bit 0 y escribir.
4. En el HDMI: poner el bit 0 (`ENABLE`) de `HDMI_SCRAMBLER_CTL`.
5. Cada segundo, leer `0x21` del monitor. Si el bit 0 está a 0, repetir los pasos 2 y 3. Linux lo hace porque
   algunos monitores pierden la configuración.

**Desactivar** (al apagar, paso 4.5 de la sección 8) [`vc4_hdmi.c`, `vc4_hdmi_disable_scrambling`]

1. Dejar de comprobar cada segundo.
2. En el HDMI: quitar el bit 0 de `HDMI_SCRAMBLER_CTL`.
3. En el monitor: quitar el bit 0 de `0x20`. Después quitar el bit 1 y esperar entre 1 y 2 ms.

Al arrancar, Linux supone que HDMI0 puede tener el scrambling activo (lo dejó el firmware) y lo desactiva en el primer
apagado. [`vc4_hdmi.c`, `vc4_hdmi_bind`]

## 13. CEC (resumen)

Fuera del alcance principal. Lo mínimo para no estorbar:

- Bloque `cec` de cada HDMI (3.1). Reloj de bit: 27 MHz / 675 = 40 kHz. El divisor 675 va en
  `HDMI_CEC_CNTRL_1[11:0]`. [`vc4_hdmi.c`, `vc4_hdmi_cec_update_clk_div`]
- Al encender el HDMI, Linux pone la dirección lógica a 15 ("sin registrar"): bits 15:12 de `HDMI_CEC_CNTRL_1` a 1.
  [`vc4_hdmi.c`, `vc4_hdmi_runtime_resume`]
- Las interrupciones de CEC llegan por `aon_intr` (sección 5).

## 14. Límites y casos raros que trata Linux

| Tema | Dato | Fuente |
|---|---|---|
| HSM a 0 sin monitor al arrancar | Cuelga el bus al tocar registros. Subirlo a ≥ 120 MHz antes | [`clk-raspberrypi.c`; `vc4_hdmi.c`, `vc4_hdmi_runtime_resume`] |
| HSM frente a reloj de píxel | HSM ≥ 1,01 · `T` | [`vc4_hdmi.c`, `vc4_hdmi_encoder_pre_crtc_configure`] |
| HSM y BVB compartidos | El mayor de lo que pidan las dos salidas | [`clk-raspberrypi.c`] |
| Apagar un reloj del firmware | El firmware no lo apaga de verdad; bajarlo al mínimo antes | [`clk-raspberrypi.c`, `raspberrypi_fw_unprepare`] |
| Píxel atascado entre PV y HDMI | Espera de 20 ms tras quitar `VIDEN` | [`vc4_crtc.c`, `vc4_crtc_disable`] |
| FIFO de PV4 | Nivel fijo de 32 bytes | [`vc4_crtc.c`, `vc4_get_fifo_full_level`] |
| Tiempos horizontales impares | No admitidos; arreglo solo en doble reloj | [`vc4_hdmi.c`, `vc4_hdmi_encoder_atomic_check`] |
| Interrupción compartida | PV1 y PV4 en SPI 110 | DTB |
| Orden de las líneas del PHY | Distinto en HDMI0 y HDMI1 | [`vc4_hdmi.c`, `phy_lane_mapping`] |
| `HDMI_DVP_CTL` compartido | El reinicio de una salida lo escribe a 0 para las dos | [`vc4_hdmi_regs.h`; `vc4_hdmi.c`, `vc5_hdmi_reset`] |
| Reinicio por el DVP global | Linux lo pide pero no se hace | [`reset-simple.c`, `reset_simple_reset`] |
| Wifi de 2,4 GHz | Modos con `T` · 10 en 2,400 a 2,422 GHz: reloj a 238,56 MHz | [`vc4_hdmi.c`, `vc4_hdmi_encoder_atomic_check`] |
| HDMI 2.0 | Solo HDMI0 y solo con `core` máximo ≥ 550 MHz | [`vc4_hdmi.c`, `vc4_hdmi_connector_clock_valid`; `vc4_hvs.c`, `vc4_hvs_bind`] |
| 4096 × 2160 a 50 Hz o más | `core` máximo ≥ 600 MHz | Ídem |
| Scrambling perdido por el monitor | Comprobación cada segundo y reescritura | [`vc4_hdmi.c`, `vc4_hdmi_scrambling_wq`] |
| Reconexión con scrambling | Apagar y encender la salida entera | [`vc4_hdmi.c`, `vc4_hdmi_reset_link`] |
| Restos en la RAM de paquetes | Poner a 0 las palabras sobrantes del paquete | [`vc4_hdmi.c`, `vc4_hdmi_write_infoframe`] |
| Salida dejada encendida por el firmware | Apagarla al arrancar; cuadrar relojes | [`vc4_crtc.c`, `vc4_crtc_disable_at_boot`; `vc4_hdmi.c`, `vc4_hdmi_bind`] |
| Underrun falso al reconfigurar | Enmascarar el aviso de vaciado hasta que el HVS use la lista nueva | [`vc4_crtc.c`, `vc4_crtc_handle_page_flip`; `hvs.md` 13.2] |

**Tiempos de espera**

| Espera | Valor | Fuente |
|---|---|---|
| `VIDEN` a 0 en la PV | hasta 1 ms | [`vc4_crtc.c`, `vc4_crtc_disable`] |
| Tras quitar `VIDEN` | 20 ms fijos | Ídem |
| `CLRRGB` antes de quitar `VID_CTL.ENABLE` | 1 ms fijo | [`vc4_hdmi.c`, `vc4_hdmi_encoder_post_crtc_disable`] |
| `HDMI_ACTIVE` | hasta 1000 ms | [`vc4_hdmi.c`, `vc4_hdmi_encoder_post_crtc_enable`] |
| Recentrado de la FIFO | 1 a 1,1 ms fijos, más hasta 1 ms | [`vc4_hdmi.c`, `vc4_hdmi_recenter_fifo`] |
| Paquete parado o arrancado | hasta 100 ms | [`vc4_hdmi.c`, `vc4_hdmi_stop_packet` y `vc4_hdmi_write_infoframe`] |
| Relación TMDS en SCDC | 1 a 2 ms fijos | [`drm_scdc_helper.c`, `drm_scdc_set_high_tmds_clock_ratio`] |
| Comprobación del scrambling | cada 1000 ms | [`vc4_hdmi.c`, `SCRAMBLING_POLLING_DELAY_MS`] |
| Cada transferencia I2C | hasta 100 ms | [`i2c-brcmstb.c`, `I2C_TIMEOUT`] |

## 15. Diferencias con las otras generaciones

| Tema | BCM2835 (`GEN_4`) | BCM2711 (`GEN_5`) | BCM2712 (`GEN_6`) |
|---|---|---|---|
| PV para HDMI | PV2 (comparte con el VEC) | PV2 y PV4 | Otras tablas |
| `CLK_SELECT` del HDMI | 1 | 0 | — |
| Píxeles por ciclo en HDMI | 1 | 2 | — |
| `FIFO_LEVEL_HIGH` y `PV_MUX_CFG` | No | Sí | Sí |
| Salidas HDMI | 1 | 2 | 2 |
| `T` máximo | 162 MHz | 600 y 340 MHz | Distinto |
| Reloj de píxel | Del CPRMAN (`PLLH_PIX`), pedido por Linux | Del PLL del PHY | Del PHY, otro PLL (8 a 12 GHz) |
| HSM | Del CPRMAN | Del firmware (reloj 13) | — |
| Reinicio del HDMI | Por registros (`HDMI_M_CTL`, `HDMI_SW_RESET_CONTROL`) | Por el DVP | Por el DVP |
| Interrupciones de HPD y CEC | Propias del HDMI | `aon_intr` compartido | — |
| HPD | GPIO o registro | Registro | Registro |
| CSC | En el bloque `hd`, con `ORDER` | Bloque propio, matriz 2.13 | Igual que la BCM2711 |
| Dominio de energía | `RPI_POWER_DOMAIN_HDMI` | Ninguno | — |
| Scrambling | No | Sí | Sí |

[`vc4_hdmi.c`, variantes `bcm2835_variant`, `bcm2711_hdmi*_variant` y `bcm2712_hdmi*_variant`; `vc4_crtc.c`, tablas
`bcm2835_pv*_data` y `bcm2711_pv*_data`; `vc4_hdmi_phy.c`]

## Lo que no está claro

1. **Origen del reloj de píxel.** Se deduce que sale del PLL del PHY porque Linux no pide otro. No está escrito en
   Linux. Tampoco se sabe cómo llega a la PV.
2. **Enganche del PLL.** Linux no espera ni comprueba que el PLL del PHY se enganche. No se sabe si hay un bit de
   estado ni cuánto tarda.
3. **Bit 10 de `TX_PHY_POWERDOWN_CTL`.** Linux lo escribe al apagar el PHY sin darle nombre. No se sabe qué apaga.
4. **Campos del PHY sin explicar**: `CPP`, `POST_RST_SEL`, `FREQ_DOUBLER_*`, `PDIV`, `CP`, `CP1`, `CZ`, `RP`, `RZ`,
   los umbrales de calibración y los bits del RM. Linux escribe valores fijos. Hay que copiarlos tal cual.
5. **`RM_FORMAT` y `TX_PHY_PLL_CFG` se escriben con OR.** Si el firmware dejó otros bits, siguen ahí. No se sabe si
   importa.
6. **Generador aleatorio del PHY.** Linux lo enciende solo con el audio. No se sabe para qué sirve.
7. **Reinicio del DVP global.** Linux lo pide pero no se hace (4.4). No se sabe si hacerlo de verdad ayuda o rompe
   algo, ni cuánto tiempo hay que mantenerlo.
8. **`HDMI_DVP_CTL` compartido.** Linux lo escribe a 0 al reiniciar cualquiera de los dos HDMI. No se sabe si eso
   afecta a la otra salida si está encendida.
9. **`HDMI_VEC_INTERFACE_XBAR`.** Se conocen los dos valores de Linux, no el significado de cada campo.
10. **Bits de `PV_CONTROL` sin explicar**: `WAIT_HSTART`, `TRIGGER_UNDERFLOW` y `CLR_AT_START`.
11. **Momento exacto de `VFP_START` respecto al VSTART del HVS.** Se sabe que `VFP_START` llega al acabar la zona
    activa. No se sabe cuántas líneas faltan hasta que el HVS toma `DISPLISTx`. Ver también `hvs.md`, punto 1.
12. **HSM y registros.** Linux dice que con el HSM a 0 el acceso "a un registro del controlador" cuelga el bus. No se
    sabe qué bloques exactamente: el núcleo seguro; el PHY, `hd`, `csc`, el BSC del DDC y `aon_intr`, no se sabe.
    Linux lee el EDID con el HSM "soltado", pero con el firmware actual el HSM no se apaga de verdad.
13. **Dominio de energía del HDMI en la BCM2711.** Linux no pide ninguno. No se sabe si el firmware lo apaga cuando
    arranca sin monitor.
14. **Tipo de la interrupción de `aon_intr`.** Flanco en el DTB del firmware, nivel en el de Linux. Linux trata los
    eventos como de flanco y los borra en `CPU_CLEAR`.
15. **Orden de bytes del BSC en un núcleo big-endian.** Linux usa accesos big-endian para este controlador cuando el
    núcleo es big-endian. Eso está pensado para chips MIPS de Broadcom donde el bus se adapta al orden de la CPU. Se
    deduce que en la BCM2711 los registros son little-endian como los demás, pero no se ha comprobado.
16. **Semántica de `RESTART` en el BSC.** Linux lo pone al final de un mensaje que va seguido de otro, pero el
    siguiente mensaje empieza sin `RESTART`. No se sabe qué hace el bit exactamente.
17. **`EXTERNAL_CTS_EN`.** El comentario de Linux dice que con ese bit el hardware calcula el CTS solo y que `N` tiene
    que ser múltiplo de 128. Linux pone el bit y además escribe `CTS_0` y `CTS_1`. A 44,1 kHz usa `N` = 5644, que no
    es múltiplo de 128 ni el valor que recomienda el estándar (6272). No se sabe qué valor usa de verdad el hardware.
18. **CTS con color profundo.** Linux calcula el CTS con el reloj de píxel, no con `T`. Con más de 8 bits por
    componente no se sabe si es correcto.
19. **Formato exacto de la subtrama IEC 60958.** Se sabe que es una palabra de 32 bits por muestra con el
    identificador de bloque 8. No se ha leído la posición de cada campo (preámbulo, audio, bits V, U, C y P). Está en
    alsa-lib, no en el kernel.
20. **Coexistencia con el wifi.** Linux cambia el reloj del modo a 238,56 MHz en la comprobación del codificador. No
    se sabe si la `T` que llega al PHY ya incluye ese cambio, porque se calcula en otra parte del núcleo DRM.
21. **Registro `0x04` de `aon_intr`.** Linux no lo usa.
22. **Bloque `metadata`.** Linux no lo usa. No se sabe para qué es.
23. **Bit 10 de `PV_STAT`.** Solo se conoce por un comentario: desbordamiento de la FIFO de la PV.
24. **`GET_EDID_BLOCK` del firmware.** Existe la etiqueta, pero vc4 no la usa y no se ha leído su formato. Podría
    servir para leer el EDID sin manejar el BSC.
25. **Escalado de la ventana de 1 % del HSM.** El firmware dice "infinitesimalmente más rápido". La documentación de la
    BCM2835 pedía un 108 %. Linux usa el 1 % en la BCM2711 sin decir si se probó a 4K60.
26. **QEMU `raspi4b`.** No se ha comprobado si emula las PV o el HDMI. Lo más probable es que no, y que todo esto
    solo se pueda probar en una Pi 4 real.
27. **Modo de direcciones de periféricos.** Las direcciones de CPU de este documento valen con el modo bajo. No se ha
    comprobado qué modo deja Emu68 en DashOS. Ver `hvs.md`, punto 20.
