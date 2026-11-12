# Bitácora: DashOS en la Raspberry Pi 5

Rama `pi5` (desde `revision`). Para el préstamo de una Pi 5 con SSD el 2026-11-13: tenerlo todo listo para que el
usuario grabe una tarjeta, enchufe y haga una foto. La hoja para él: `docs/PI5_PRIMER_ARRANQUE.md`.

## 2026-11-12: primer arranque, sin cable serie

### El camino de arranque: el firmware de la Pi, sin UEFI

Dos caminos posibles:

- **(a) El firmware de la Pi 5 carga el kernel directamente** (`kernel_2712.img`, `config.txt`, el árbol de
  dispositivos del firmware). Elegido.
- (b) Un UEFI de terceros para la Pi 5 (el EDK2 de la comunidad, `rpi5-uefi`) y el cargador UEFI de DashOS.

Por qué (a):

- No depende de nada de fuera más que del firmware oficial, que ya está en la EEPROM de la Pi. En la tarjeta solo van
  `config.txt`, el kernel y los `.dtb` oficiales (bajados por el guion, fuera de git).
- Es el mismo camino que la Pi 4: el kernel ya arranca así (`enlace.ld`, 0x80000, el árbol en x0, EL2). Lo nuevo son
  las direcciones de la BCM2712.
- Con (b), el cargador UEFI pasa el control a la imagen de `virt`, enlazada para la RAM de QEMU (1 GB) y con su mapa de
  aparatos: haría falta una tercera variante del kernel igualmente, más un binario de terceros (el proyecto
  `rpi5-uefi` está parado) y su ACPI a medias. A cambio daría GOP y USB del RP1 por el firmware, que es lo que interesa
  para el escritorio: queda como opción para más adelante.

### Lo que dice el árbol oficial de la Pi 5

De `bcm2712-rpi-5-b.dtb` (github.com/raspberrypi/firmware, decompilado con dtc; es un dato, no código):

- el bus `soc@107c000000`: `ranges = <0 0x10 0 0x80000000>`, así que los periféricos están entre 0x10_0000_0000 y
  0x10_8000_0000 (GB 64 y 65);
- el buzón de la VideoCore en 0x10_7C01_3880 (`brcm,bcm2835-mbox`), y el nodo `firmware` con `dma-ranges` vacío: la
  VideoCore ve la RAM con su dirección física, sin el desplazamiento de la Pi 4;
- el UART10 (PL011, el del conector de depuración) en 0x10_7D00_1000, con un reloj de 9,216 MHz;
- el GIC-400 en 0x10_7FFF_9000 (distribuidor), 0x10_7FFF_A000 (CPU), 0x10_7FFF_C000 y 0x10_7FFF_E000 (virtual);
- el temporizador, en las PPI de siempre (la 14 para el físico no seguro);
- PSCI por `smc`, y los núcleos con `reg` 0x000, 0x100, 0x200 y 0x300: el Cortex-A76 marca MT en MPIDR y el número
  del núcleo va en Aff1, no en Aff0;
- `atf@0`: de 0 a 0x80000 es del firmware de seguridad (no-map);
- un nodo `fb` (`brcm,bcm2708-fb`) sin dirección: la pantalla se pide al firmware por el buzón; no hay
  `simple-framebuffer` en el árbol estático (el firmware podría añadirlo; si lo hace, se usa ese).

### Lo hecho

- **`pi5`** (característica del kernel, implica `diagnostico`): `placa.rs` con la ventana de periféricos de 64 a 66 GB y
  el UART10; las tablas del arranque (`arranque.s`) y el mapa del kernel (`mapa.rs`) proyectan esos dos GB como
  dispositivo fuera del mapa lineal; el UART se deja como lo dejó el firmware (o a 115200 con su reloj). `make pi5`
  saca `target/kernel_2712.img` (1,7 MB).
- **La imagen en 0x80000**: `config.txt` lo pide (`kernel_address=0x80000`). Si el firmware la deja más arriba (lo de
  siempre para 64 bits es 0x200000), el arranque se copia solo a 0x80000: el bucle de la copia va al final de la imagen
  (sección `.reubicar`), así que el destino nunca llega a pisarlo.
- **El número de núcleo** (`arranque.s`, `vectores.s`, `nucleos.rs`): Aff1 si MPIDR marca MT, Aff0 si no; y PSCI
  enciende los núcleos con esa misma afinidad. Antes, en la Pi 5 los cuatro se habrían creído el núcleo 0. En QEMU
  (Cortex-A72 y A76 sin MT) no cambia nada.
- **El orden de los colores** del buzón: se pide el orden 0, el mismo que el compositor de la Pi 4 (el píxel queda
  0x00RRGGBB en little-endian).
- **La consola en pantalla y el diagnóstico** (característica `diagnostico`, `pantalla.rs`, `diagnostico.rs`,
  `base/pantalla.rs`): la pantalla del árbol (`simple-framebuffer`) o, si no hay, la que se pide al firmware por el
  buzón (tamaño del monitor, 32 bits, alineada a 4 KB). Arriba, un panel con letra grande; abajo, todo el registro del
  kernel y de los programas. Letra Roboto Mono en mapa de bits de 12 x 24 (`tools/fuente_consola.py`, OFL), estirada
  a 18 x 36 en una pantalla de 1920. Colores que no cambian si el firmware cambia rojo y azul (verde, magenta, grises).
  Cada línea del panel se pinta en cuanto se sabe: placa y revisión, temperatura, CPU (MIDR), `ID_AA64MMFR0_EL1`
  (BigEnd, BigEndEL0, TGran4), la prueba de verdad del big-endian en EL0 (el programa `hola` compilado en big-endian,
  con E0E, en el núcleo 1: acaba con 5 si leyó 0x11 primero de 0x11223344), memoria, núcleos y la caja Amiga (sus
  líneas importantes suben solas al panel). El pánico también sale en el panel.
- **Los programas**: con `diagnostico`, `inicio` lanza solo la consola y la caja Amiga (sin escritorio, USB ni disco).
  La caja corre el JIT, exec y dos nativas y `saludo.exe`; `ventana.exe` y `hostil.exe` necesitan el compositor y
  salen mal, como se espera (la caja acaba con 12 = 10 + 2 fallos).
- **El hipervisor** no se instala en la Pi 5 por ahora (no hay caja Linux en esta imagen; una cosa menos que pueda
  parar el primer arranque). La línea de la CPU dice igualmente si entró en EL2.
- **Un fallo de antes**, que destapó el diagnóstico: `Cupo::raiz()` creaba el grupo raíz con el repartidor de páginas
  tomado; si el montón necesitaba una página nueva para ese `Arc`, el núcleo se quedaba esperándose a sí mismo. Ahora
  lee los bytes antes. Hasta hoy no se veía porque la primera llamada caía siempre con sitio en el montón.
- **La tarjeta**: `tools/preparar_pi5.py` (`make tarjeta-pi5`) hace `~/.cache/dashos-pi5/dashos-pi5.img`, 65 MB: MBR,
  una FAT32 de 64 MB con `config.txt`, `kernel_2712.img`, `bcm2712-rpi-5-b.dtb`, `bcm2712d0-rpi-5-b.dtb` y `LEEME.txt`.
  Sin root (mkfs.fat, mtools, sfdisk). Los `.dtb` se bajan a `~/.cache/dashos-pi5/firmware-stable`.
- `config.txt`: `kernel=kernel_2712.img`, `kernel_address=0x80000`, `os_check=0` (si no, el firmware busca un árbol
  de Linux compatible), `enable_uart=1` y `uart_2ndstage=1` (el UART de depuración, con el registro del firmware),
  `disable_fw_kms_setup=1` y `hdmi_force_hotplug=1`, todo bajo `[pi5]`.

### Pruebas

- `make arranque-virt-diag` (nueva): la imagen de `virt` con `diagnostico`, por UEFI, con la ramfb. Comprueba las
  líneas de `pruebas/arranque_virt_diag.txt` (pantalla, CPU, memoria, cuatro núcleos, el big-endian en EL0 BIEN, la
  caja con `saludo.exe` bien) y mira la captura (`tools/captura_diag.py`: panel pintado, líneas en verde, registro
  con texto). BIEN. Probada también a 1920 x 1080 (`DASHOS_PANTALLA`) y con `-cpu cortex-a76`.
- `make pi5` y `make tarjeta-pi5`: la imagen sale; la tabla de particiones y el `config.txt` se comprobaron con sfdisk
  y mtools, y el kernel de la FAT es igual al de `target/`.
- Lo de siempre, tras los cambios (el número de núcleo, el arranque y `Cupo::raiz` tocan a todas las placas):
  `make prueba` ("BIEN: esperado.txt" y "BIEN: cargador hunk"), `make arranque-virt` (las cinco BIEN),
  `arranque-virt-vm` y `arranque-virt-2`, todas BIEN.

### Lo que solo se sabe en la Pi 5

Sin probar, escrito con el árbol y la documentación pública: que el firmware cargue la imagen y salte a ella con
`kernel_address=0x80000` (y, si no, que la copia funcione); que el buzón dé la pantalla (y en qué HDMI); que el
mensaje del buzón vaya con la dirección física; que el UART10 esté encendido; que PSCI encienda los núcleos con
MPIDR 0x100...; que el GIC-400 y el temporizador vayan como en la Pi 4; y, sobre todo, lo que dirá la CPU del
big-endian en EL0.


## 2026-11-13: el escritorio en la pantalla del firmware (paso 4)

- Característica `escritorio` (con `diagnostico`): `inicio` lanza la consola, el compositor, `ventana-a`, `ventana-b`,
  Ajustes y la caja Amiga, ahora con el canal "ventanas". El compositor recibe la pantalla que tenía la consola del
  kernel (`pantalla_del_kernel` en `inicio.rs`, como la de UEFI: la memoria como objeto del firmware y sus medidas),
  y la consola deja de pintar (`pantalla::ceder`). Para que el compositor pueda proyectarla aunque caiga en RAM del
  árbol, la pantalla apartada queda también como reserva (`Paginas::apartar`).
- Sin `sesiones` ni `archivos`: nada de fondo, barra, centro ni SYS:. La caja abre su ventana Amiga (ventana.exe) en el
  escritorio; sin teclado ni ratón, se cierra sola a los 40 s.
- `make pi5-escritorio` saca `target/dashos-escritorio.img`; `make tarjeta-pi5` lo mete en la tarjeta, y se elige en
  `config.txt` (la línea `kernel=`). Lo que arranca por defecto sigue siendo el diagnóstico.
- Prueba: `make arranque-virt-escritorio` (QEMU virt, la ramfb pasa de la consola al compositor): BIEN; captura en
  `capturas-agente/pi5-escritorio-virt.png` (las dos ventanas de prueba, Ajustes y la ventana Amiga). En la Pi 5, sin
  probar.
- Tras el paso 4, otra vez `make prueba`, `arranque-virt` (las cinco), `arranque-virt-vm` y `arranque-virt-2`: BIEN.
  Y `arranque-virt-diag` y `tarjeta-pi5` de nuevo (la tarjeta ya lleva las dos imágenes).

### Lo siguiente: teclado y ratón (el RP1)

En la Pi 5, el USB (dos xHCI), la Ethernet, los GPIO y los UART de la cabecera están en el RP1, un chip aparte detrás
del PCIe (el `pcie@1000120000` del árbol, `rp1_target`; el conector PCIe de fuera, donde va el SSD, es otro, el
`pcie@1000110000`). Para el teclado y el ratón hace falta:

1. **El controlador PCIe de la BCM2712** (Broadcom, de la familia del de la Pi 4: `docs/spec/hw/pcie.md` sirve de
   punto de partida, pero no es igual). El firmware deja el enlace del RP1 entrenado (lo usa para sus UART); falta
   saber si deja también las ventanas de salida y de entrada como hacen falta, o hay que programarlas. Sus
   interrupciones van por un controlador de MSI propio (`msi-controller@...` en el árbol), no por el GIC directamente.
2. **El RP1 por dentro**: su BAR con todos los periféricos (el árbol los describe bajo `rp1`, en la ventana
   0x1F_0000_0000 de la CPU), su controlador de interrupciones propio (que pasa sus fuentes a MSI hacia la BCM2712) y
   encender sus relojes. Raspberry Pi publica el "RP1 peripherals" (documentación pública) con los registros.
3. **Los xHCI del RP1**: son DWC3 de Synopsys en modo anfitrión, que se manejan como un xHCI normal: el controlador
   `xhci` de DashOS debería valer tal cual, con sus registros y su interrupción.
4. La ventana del RP1 en el mapa del kernel (`placa.rs` y `arranque.s`: el GB 124) y la llave de sus registros para
   los servicios.

Es la parte grande: un controlador de PCIe nuevo y el de las interrupciones del RP1, sin poder probarlos en QEMU. El
otro camino es el UEFI de la comunidad (`rpi5-uefi`), que ya trae el PCIe y el USB del RP1 en marcha, con el cargador
UEFI de DashOS: haría falta un kernel para la Pi 5 que entre por UEFI (otra variante de `placa.rs`), y saber qué
deja preparado ese firmware al salir de los servicios de arranque.

### Otras cosas pendientes en la Pi 5

- El hipervisor (caja Linux): apagado en `pi5` hasta ver el primer arranque.
- La tarjeta SD (controlador `brcm,bcm2712-sdhci`) y el SSD (el NVMe del conector PCIe externo, que pide el mismo
  controlador de PCIe que el RP1).
- La GPU (V3D 7.1) y la pantalla por el HVS sin el firmware.
- La RAM por encima de 4 GB (el mapa lineal del kernel llega hasta 4 GB; lo mismo que en la Pi 4 de 8 GB).


## 2026-11-13: el teclado y el ratón por el RP1 (rama `pi5-usb`)

Para la prueba de mañana, sin Pi 5 a mano: código que se diagnostica solo. Un tercer kernel en la tarjeta,
`dashos-usb.img` (`make pi5-usb`, opciones `pi5,usb`), que se elige en `config.txt`; el diagnóstico de siempre
(`kernel_2712.img`) no cambia. El escritorio (`dashos-escritorio.img`) lleva ahora también el USB, con el teclado y el
ratón al compositor.

### Fuentes (sala limpia)

- El árbol oficial `bcm2712-rpi-5-b.dtb` (y el `d0`, igual en esto): el controlador `pcie@1000120000` (registros en
  0x10_0012_0000), su `ranges` (memoria de 32 bits: PCI 0 → CPU 0x1F_0000_0000, casi 4 GB), su `dma-ranges` (PCI
  0x10_0000_0000 → RAM 0, 64 GB; y PCI 0xFF_FFFF_F000 → el MIP de las MSI), el nodo `rp1` (`ranges`: 0xC0_4000_0000 del
  RP1 → PCI 0, 0x410000) y sus `usb@200000` y `usb@300000` (`snps,dwc3`, `dr_mode = "host"`, interrupciones 31 y 36 del
  RP1).
- "RP1 Peripherals" (Raspberry Pi, pública; `~/.cache/dashos-pi5/docs/rp1-peripherals.pdf`, fuera de git): los
  periféricos se ven por la BAR 1 y la SRAM por la BAR 2 (2.3.1); el USB son dos xHCI DWC3 v3.30b, xHCI 1.2, dos puertos
  cada uno (uno USB 2 y uno USB 3), en 0x4020_0000 y 0x4030_0000 (cap. 5); el bloque `pcie` (MSI-X) en 0x4010_8000.
- La documentación de `config.txt`: `pciex4_reset=0` hace que el firmware no reinicie el PCIe del RP1 y deje su
  configuración al sistema que arranca. Es lo que usa DashOS: no hay que levantar el enlace (eso solo está documentado
  en código de Linux).
- `docs/spec/hw/pcie.md` (el controlador de la 2711) para los registros MISC_*, `EXT_CFG_*` y `RGR1_SW_INIT_1`.
  **Supuesto**: que la 2712 los tiene en los mismos sitios (el `reg` mide 0x9310 en las dos).
- La norma PCI/PCIe (cabecera, capacidades, estado del enlace, orden), la de xHCI 1.2, y la referencia pública de
  registros del DWC3 de la Zynq UltraScale+ (UG1087) para GSNPSID (0xC120) y GCTL (0xC110), que solo se leen.
- Huecos que solo se cerrarían con código de Linux, y que la Pi dirá: dónde están los MISC_* en la 2712, el valor de
  `EXT_CFG_INDEX` que deja el firmware, el número de aparato del RP1 (el fabricante, 0x1DE4, es de la lista pública de
  la PCI-SIG), si el firmware deja la ventana de entrada abierta y el bus maestro puesto, la corriente de los puertos
  (sus patillas VBUS van por el banco 1 de GPIO del RP1, "reservado" en la hoja: no se toca) y lo que pida el DWC3 más
  allá de la norma xHCI.

### Lo hecho

- **El mapa**: la ventana del RP1 (el GB 124, 0x1F_0000_0000) entra en los periféricos de la Pi 5 (`placa.rs`: ahora
  una lista de zonas, `es_periferico` mira las dos; `mapa.rs` proyecta como dispositivo solo los GB de esas zonas). Las
  llaves de registros ya no pueden cruzar el hueco de en medio (`proyectar_dispositivo` mira el principio y el final).
- **`kernel/src/rp1.rs`**, sin dependencias, con pruebas en el PC (`make prueba` las corre, `rustc --test`): mira, sin
  tocar nada, el controlador (revisión, `MISC_PCIE_STATUS`: modo RC y enlace), el puerto raíz (su número, su bus
  secundario, Gen y anchura del enlace por la capacidad PCIe), la ventana de salida 0 y la de entrada 2 contra el árbol.
  Si el enlace está arriba y `EXT_CFG_INDEX` tiene forma de índice, escribe en él el bus del RP1 y lee su cabecera: si
  es de Raspberry Pi, enciende memoria y bus maestro en su orden si faltan (las únicas dos escrituras). Después lee del
  RP1 por la ventana de salida (la primera palabra de `sysinfo`) y los registros de capacidad de los dos xHCI, sus
  puertos (PORTSC) y el núcleo DWC3. Si la BAR 1 no está donde dice el árbol, usa la de verdad. El modelo falso de las
  pruebas: el caso bueno, enlace caído (no escribe nada), controlador que no contesta, índice raro y otro aparato (no se
  toca), ventana de entrada cerrada (lo dice y sigue) y la BAR 1 en otro sitio.
- **El panel** (`pantalla.rs`, con `usb`: 21 líneas): en lugar de la caja Amiga, cinco líneas (PCIe, ventanas, RP1,
  USB0, USB1), "Teclado y ratón" con la última tecla o movimiento, y debajo lo que importa de `xhci0`/`xhci1`.
  Cada línea del RP1 cabe en las 104 columnas de una pantalla de 1920 (lo comprueba la prueba).
- **Las llaves** (`inicio.rs`, `diagnostico::usb_pi5`): de cada xHCI que contesta, 64 KB de registros y un permiso DMA
  con el desplazamiento del `dma-ranges` (bus = física + 64 GB). Sin interrupciones: se sondea cada 10 ms (las MSI del
  RP1 y su MIP, para más adelante: el MIP solo está descrito en código de Linux).
- **El xHCI** (`usuario/src/xhci_comun.rs`; `xhci`, `xhci0` y `xhci1` solo cambian el nombre): dice la versión, los
  puertos, las ranuras, si es de 64 bits, los búferes de trabajo que pide (y se los da: scratchpad, hasta 32 páginas;
  el de QEMU no pide ninguno, un DWC3 puede pedirlos), PPC; prueba el DMA con una orden vacía ("DMA bien" o "MAL ... HSE:
  el DMA no llega"); da corriente a los puertos si PPC; espera hasta 1,5 s a que algo se conecte (tras el reinicio un
  aparato tarda); escribe el PORTSC de cada puerto, y de cada aparato su fabricante:producto, su clase y su primera
  interfaz; si uno no se configura, en qué paso y con qué código. Vigila el HSE mientras corre. Sin compositor, cada tecla
  y cada movimiento salen en el registro.
- **La tarjeta**: `dashos-usb.img` dentro, y `pciex4_reset=0` en `config.txt` (no persiste, solo vale ese arranque).

### Pruebas

- `make arranque-virt-usb` (nueva, `pruebas/arranque_virt_usb.txt`): la imagen `virt,usb` en QEMU con el xHCI de QEMU
  como `xhci0`, un concentrador, un teclado y un ratón. El panel dice "sin PCIe de la BCM2712 en el árbol (no es una Pi
  5)" y sigue; `xhci0` dice "DMA bien", los aparatos (0409:55aa el concentrador, 0627:0001 el teclado y el ratón) y las
  teclas que manda el monitor. Captura en `capturas-agente/pi5-usb-virt.png`.
- `rp1.rs` en el PC: 8 pruebas, bien.
- Tras fusionar `revision` (7b7e5a7) en la rama: `make prueba` ("BIEN: esperado.txt" y "BIEN: cargador hunk", y las
  pruebas de `rp1.rs`), `make arranque-virt` (las cinco BIEN), `arranque-virt-vm`, `arranque-virt-2`,
  `arranque-virt-diag`, `arranque-virt-escritorio` y `arranque-virt-usb`: todas BIEN.
- `make tarjeta-pi5`: la tarjeta lleva `kernel_2712.img`, `dashos-usb.img` y `dashos-escritorio.img` (este, ahora con
  `usb`), y `pciex4_reset=0` en `config.txt`; el `dashos-usb.img` de la FAT es igual al de `target/`.
- Sin probar en una Pi 5: todo lo del RP1. Ni el escritorio con `usb` en QEMU (allí no hay RP1; la imagen `virt` del
  escritorio no lleva `usb`).

### Qué mirar en la foto de `dashos-usb.img`

1. "PCIe del RP1": si dice enlace CAÍDO, el firmware reinició el PCIe (¿se perdió `pciex4_reset=0`?) y no se sigue.
2. "Ventanas": si la de entrada no cuadra, el DMA no llegará (lo confirmará `xhci0`: "MAL ... HSE").
3. "RP1": `1de4:xxxx` y un `sysinfo` que no sea "fallo de bus". Si en su lugar sale "EXT_CFG_INDEX vale ..." o "no
   Raspberry Pi", los registros de configuración de la 2712 no están donde en la 2711: apuntar el valor.
4. "USB0" y "USB1": versión, 64 bits, scratch, y qué puerto tiene "algo".
5. Debajo: "DMA bien" de cada uno, los aparatos con fabricante:producto y "HID de arranque: 1 teclado y 1 ratón".
6. "Teclado y ratón": la última tecla.

### Lo que queda

- Las interrupciones del RP1 (sus MSI-X por el bloque `pcie` del RP1 y el MIP de la BCM2712 hacia el GIC, SPI 128 a
  191 según el árbol): hoy se sondea cada 10 ms. El MIP solo está descrito en código de Linux: hará falta una
  especificación de analista o probarlo en la Pi.
- Levantar el enlace sin el firmware (si algún día `pciex4_reset=0` no basta): también solo en código de Linux.
- La corriente de los puertos USB (VBUS) si el firmware no la deja puesta.
