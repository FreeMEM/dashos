# DashOS en la Raspberry Pi 5: el primer arranque

Para el préstamo de una Pi 5 (2026-11-13). DashOS arranca sin Linux y escribe en la pantalla lo que ve de la máquina.
Con una foto de esa pantalla basta. No hace falta cable serie.

**Nada de esto cambia la Pi.** La EEPROM (el firmware de arranque de la Pi 5) no se toca: ni DashOS ni la tarjeta la
escriben. Para dejarla como estaba, se quita la tarjeta y se enciende.

## Qué grabar

La imagen: `~/.cache/dashos-pi5/dashos-pi5.img` (65 MB). Si no está, o tras cambiar algo:

```bash
make -C kernel tarjeta-pi5
```

Lleva una partición FAT con `config.txt`, el kernel (`kernel_2712.img`), los árboles de dispositivos oficiales de la
Pi 5 (bajados de github.com/raspberrypi/firmware) y un `LEEME.txt`.

## Dónde: en una microSD

Una microSD cualquiera (se borra entera). Es lo más seguro: no toca el SSD de la Pi.

```bash
lsblk                                   # buscar la tarjeta (por el tamaño): /dev/sdX o /dev/mmcblkN
sudo dd if=$HOME/.cache/dashos-pi5/dashos-pi5.img of=/dev/sdX bs=4M conv=fsync status=progress
```

También vale Raspberry Pi Imager ("Use custom" y la imagen) o balenaEtcher.

### ¿Y el SSD?

**No grabar la imagen en el SSD prestado.** Se borraría lo que tenga (seguramente el sistema de su dueño).

El orden de arranque de la Pi 5 está en su EEPROM (`BOOT_ORDER`, se lee de derecha a izquierda: 1 es la tarjeta, 6 el
NVMe, 4 el USB). Lo normal es que pruebe la tarjeta antes que el NVMe (por ejemplo `0xf461`). Entonces, con la microSD
puesta, arranca DashOS aunque el SSD esté conectado, y al quitarla vuelve a arrancar lo del SSD.

Si con la tarjeta puesta arranca igualmente el sistema del SSD, es que su EEPROM pone el NVMe antes. Para verlo sin
cambiar nada, desde ese sistema: `rpi-eeprom-config` (solo lee). Opciones, de más a menos segura:

1. Desconectar el SSD (el cable plano del HAT, con la Pi apagada) mientras se prueba DashOS.
2. Pedir permiso al dueño para cambiar el orden con `sudo raspi-config` (Advanced Options, Boot Order) y apuntar el
   valor de antes para devolverlo. Es la única opción que toca la EEPROM: que la decida el dueño.

Un SSD de sobra, que se pueda borrar, también vale: la misma imagen, grabada igual (con un adaptador USB a NVMe en el
PC). Arranca si la EEPROM tiene el 6 en `BOOT_ORDER`.

## Qué conectar

1. La microSD en la ranura de la Pi.
2. El monitor en el **HDMI 0**: el micro-HDMI que está junto a la toma de corriente USB-C. Es donde el firmware pone
   su pantalla; con el otro puede no salir nada.
3. Encender el monitor antes que la Pi.
4. La corriente (el cargador oficial de 27 W, mejor).

Teclado y ratón no hacen falta: DashOS todavía no los lee en la Pi 5 (van por el chip RP1, aún sin controlador).

## Qué foto hacer

Unos 15 segundos después de encender, la pantalla queda quieta. Una foto de la pantalla entera, que se lean las líneas
de arriba. Arriba, sobre fondo gris, el panel:

- el modelo de la placa y su revisión, la temperatura;
- la CPU (`MIDR_EL1`, debería decir Cortex-A76) y en qué nivel entró (EL2);
- `ID_AA64MMFR0_EL1` con `BigEnd` y `BigEndEL0`: si la CPU dice que admite big-endian en EL0;
- **"Big-endian en EL0, de verdad"**: un programa big-endian de prueba ha corrido; BIEN si leyó los bytes como el 68k;
- la memoria;
- los núcleos en marcha (4 de 4);
- **la caja Amiga**: el JIT de Emu68 corriendo `saludo.exe`, un ejecutable de AmigaOS. Lo importante es "saludo.exe
  BIEN". En esta imagen no hay escritorio: `ventana.exe` y `hostil.exe` fallan, y eso es lo esperado.

Lo verde fue bien; lo magenta, mal. Abajo, en letra más pequeña, sale todo lo demás, como por el puerto serie. Si
algo se para, la foto dice hasta dónde llegó: también vale.

Si se puede, una segunda foto unos segundos más tarde, por si cambió algo.

## Si la pantalla se queda negra

- **Antes de DashOS**, la Pi 5 enseña su pantalla de diagnóstico (versión de la EEPROM, orden de arranque, errores).
  Si se queda ahí, una foto de esa pantalla: dice por qué no arranca.
- **El LED verde** (ACT): sus parpadeos son códigos. Siete cortos: no encuentra el kernel. Contar largos y cortos y
  apuntarlo. La tabla, en la documentación de Raspberry Pi ("LED warning flash codes").
- **Otro monitor u otro cable**, en el HDMI 0. Si el monitor no responde a tiempo, DashOS pide 1280 x 720.
- **El puerto serie**, si hay un Raspberry Pi Debug Probe o un adaptador USB a serie de 3,3 V con el cable JST de 3
  patillas: el conector UART pequeño, entre los dos micro-HDMI. 115200 baudios, 8N1. Sale el registro del firmware
  (`uart_2ndstage=1` en `config.txt`) y después todo lo de DashOS. Con `picocom -b 115200 /dev/ttyACM0` (o `ttyUSB0`),
  guardar el texto.
- Probar a quitar la línea `hdmi_force_hotplug=1` de `config.txt` (la tarjeta se puede abrir en cualquier PC).

## Lo que no está comprobado

Nada de esto se ha probado en una Pi 5 de verdad: QEMU no la emula. En QEMU (máquina `virt`) se probó la misma consola
y el mismo diagnóstico, con otra pantalla (`make -C kernel arranque-virt-diag`). Lo propio de la Pi 5 (pedir la
pantalla al firmware por el buzón, las direcciones de la BCM2712, el puerto serie de depuración, encender los núcleos
con el MPIDR del Cortex-A76, cargar la imagen en 0x80000) está escrito con el árbol de dispositivos oficial y la
documentación pública, pero sin probar. Detalle en `docs/bitacoras/pi5.md`.
