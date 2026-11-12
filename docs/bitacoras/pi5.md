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
