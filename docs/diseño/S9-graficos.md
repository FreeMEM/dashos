# S9. Gráficos

## El problema

DashOS tiene que pintar el escritorio en una o dos pantallas, con fluidez aunque el sistema esté cargado (meta de
S0: 60 cuadros por segundo con los cuatro núcleos al 100 %). Las ventanas llegan de tres sitios: aplicaciones
nativas, la caja Amiga (S4) y la caja Linux, que habla Wayland (S7).

La Pi 4 tiene tres piezas de vídeo:

- **El HVS** (Hardware Video Scaler). Compone por hardware varias capas (planos), las escala, convierte formatos de
  color y manda el resultado a la salida. Hay un solo HVS con varias salidas.
- **Las "pixel valves"** y los codificadores HDMI. Hay dos salidas HDMI.
- **La GPU V3D 4.2**, para dibujar en 3D. Con Mesa, en Linux, tiene OpenGL ES 3.1 y Vulkan conformes.

## Lo que se comprobó

- **La pantalla del firmware funciona desde el primer momento.** Emu68 la pide por el buzón de la VideoCore y la
  recibe en QEMU `raspi4b` (registro de arranque de la fase 1: "Display size is 640x480").
- **Los controladores de Linux para estas piezas son medianos.** En el árbol de Linux, `drivers/gpu/drm/v3d` suma
  unos 200 KB de C, y `drivers/gpu/drm/vc4` (pantalla, HVS, HDMI y más) unos 770 KB. Medido con la API de GitHub
  sobre `torvalds/linux`, sin contar líneas.
- **No hay manual público de la V3D 4.2.** Broadcom nunca lo publicó. Todo lo que se sabe está en el código de Mesa y
  de Linux.
- **Mesa usa dos controladores del kernel en la Pi 4**: v3d para dibujar y vc4 para la pantalla, y comparte los
  búferes entre los dos.

## Las opciones

### A. Solo la pantalla del firmware y dibujo por CPU

- Gana: funciona ya, con una llamada al buzón. Cero controladores.
- Cuesta: sin sincronía vertical fiable ni dos pantallas bien controladas. Todo el compuesto lo hace la CPU.

### B. Controlador propio del HVS y la salida HDMI, con compuesto por CPU

- Gana: control de la sincronía vertical, de los modos de pantalla y de las dos salidas. El HVS puede componer
  algunas ventanas por hardware, sin copiar sus píxeles: el cursor, un vídeo, la ventana a pantalla completa.
- Cuesta: escribir el equivalente a una parte de vc4, sin manual, leyendo el código de Linux.

### C. Además, la GPU V3D con Mesa

- Gana: OpenGL ES y Vulkan para las aplicaciones y para el propio compositor.
- Cuesta: portar Mesa (con relibc, S7) y escribir el lado del kernel de v3d: gestión de memoria de la GPU, envío de
  trabajos y sus interrupciones. Unos 200 KB de C en Linux. Sin manual.

## Recomendación

**Por fases: A para arrancar, B para la primera versión, C después.**

1. **Fase A**: la pantalla del firmware y un compositor que dibuja por CPU. Basta para las primeras pruebas, la caja
   Amiga y el escritorio básico.
2. **Fase B**: controlador propio de pantalla (HVS, pixel valves y HDMI) como servicio de DashOS (S1, S12).
   - Sincronía vertical con la interrupción de la salida, y cambio de búfer en el momento justo.
   - Las dos salidas HDMI, cada una con su modo.
   - Planos del HVS para el cursor, el vídeo y las ventanas a pantalla completa.
3. **Fase C**: la GPU, con Mesa portado y un servicio de kernel para v3d. Los dos servicios comparten búferes como
   en Linux.

### El compositor

- **Es un proceso con presupuesto de la clase de plazo** (S2): tiene su tiempo reservado en cada cuadro.
- **Dibujo por daños**: solo se recompone lo que cambia. Mover el ratón o escribir una letra no repinta la pantalla
  entera. Así el compuesto por CPU es viable a 1080p.
- **Cada ventana es un objeto de memoria compartido** entre la aplicación y el compositor (S3). Sin copias.
- **Sin parpadeos ni desgarros**: triple búfer y cambio en la sincronía vertical (fase B).
- **Escalado** para pantallas de mucha densidad, también fraccional (125 %, 150 %).

### El protocolo de las ventanas: Wayland

**Recomendación: el compositor habla Wayland como protocolo propio**, con las extensiones que DashOS necesite. Por
debajo, el transporte son los canales de DashOS en vez de sockets de Unix, y los descriptores de fichero que pasa
Wayland son capacidades.

- **Gana**: los kits de interfaz que ya hablan Wayland (SDL, GTK, Qt) se portan sin reescribir su parte de
  ventanas. La caja Linux (S7) se conecta con un intermediario como Sommelier, sin traducir de un protocolo a otro.
- **Encaja con el principio 4 de S0**: un solo protocolo para todas las ventanas, vengan de donde vengan.
- **La caja Amiga** también es un cliente Wayland: su Intuition nativa abre una superficie por ventana o pantalla
  Amiga (S4).
- **Ojo con el orden de bytes**: Wayland usa el orden de la máquina. Dentro de DashOS todo es big-endian y no hay
  problema. Las ventanas de la caja Linux, que es little-endian, pasan por el intermediario, que da la vuelta a los
  bytes de los mensajes.

La alternativa sería un protocolo propio de DashOS y un traductor de Wayland aparte, como Exo en ChromeOS. Da más
libertad, pero duplica el trabajo de todos los kits de interfaz.

### Pantallas y monitores

- **`DEVS:Monitors`** tiene un fichero por salida: `HDMI-0` y `HDMI-1` (S4b).
- **`Prefs/ScreenMode`** elige resolución, frecuencia y escala de cada monitor.
- **Conectar y desconectar en caliente**: el controlador avisa y el escritorio recoloca las ventanas (S10).

## Lo que queda abierto

1. ¿Por fases: pantalla del firmware, después controlador propio del HVS y HDMI, y la GPU al final? Recomendado: sí.
2. ¿Wayland como protocolo propio del compositor? Recomendado: sí.

## Cómo encaja

- Con S0: principio 1 y la meta de 60 cuadros con carga.
- Con S2: el compositor tiene presupuesto de plazo.
- Con S3: ventanas como objetos de memoria compartidos; los búferes de pantalla en la zona de memoria que alcance el
  HVS (comprobar en el árbol de dispositivos).
- Con S4: la caja Amiga es un cliente Wayland.
- Con S4b: `DEVS:Monitors` y `Prefs/ScreenMode`.
- Con S7: la caja Linux se conecta con un intermediario de Wayland.
- S10 diseña el escritorio encima de este compositor.
- S11: el kit de interfaz nativo dibuja en superficies Wayland.
- S12: controladores de HVS, HDMI y V3D.

## Fuentes

- Controlador vc4 de Linux, HVS y planos: [documentación del kernel](https://docs.kernel.org/gpu/vc4.html) y
  [binding del árbol de dispositivos](https://git.pengutronix.de/cgit/lst/linux/tree/Documentation/devicetree/bindings/display/brcm,bcm-vc4.txt).
- V3D en Mesa: [documentación de v3d](https://android.googlesource.com/platform/external/mesa3d/+/f45d77038bb/docs/drivers/v3d.rst),
  [Vulkan conforme (GamingOnLinux)](https://gamingonlinux.com/2020/11/vulkan-for-the-raspberry-pi-4-with-v3dv-is-now-conformant-and-official),
  [Igalia en hackster.io](https://hackster.io/news/igalia-confirms-raspberry-pi-4-400-compute-module-4-vulkan-graphics-driver-conformance-milestone-dea503c6412c),
  [cambio de Mesa sobre V3D 4.2 y 7.1](https://gitlab.neroreflex.duckdns.org/NeroReflex/mesa/commit/b509446c2c97b1e923e7cdffd745bd956ce1f5fd.diff)
  y [charla de Igalia](https://www.slideshare.net/slideshow/overview-of-the-open-source-vulkan-driver-for-raspberry-pi-4/251353718).
- Tamaños de `drivers/gpu/drm/v3d` y `vc4`: API de GitHub sobre `torvalds/linux`, 2026-11-03.

Lo que no se comprobó: si el compuesto por CPU llega a 60 cuadros en una Pi 4 real a 1080p, y cómo reparte el HVS
sus salidas entre los dos HDMI de la Pi 4.
