# S25. La tarjeta gráfica para los programas de la caja Linux

Del 2026-11-12, a pedido del usuario ("usar la tarjeta gráfica"). Rama `caja-linux-gpu`, sacada de `revision` (0ce1f49).
Sala limpia: especificación de virtio 1.2 (§5.7, GPU), los XML de Wayland y lo que DashOS ya sabe de la V3D
(`docs/spec/hw/v3d.md`, escrito por analistas). Nada de código de Linux, Mesa, QEMU, crosvm ni virglrenderer.

## Dónde estamos

- **La caja Linux** (S7, H9) arranca Arch Linux ARM con 4 vCPU. Sus programas de Wayland salen como ventanas de DashOS:
  el puente de Linux copia lo dañado de cada búfer por vsock y la caja lo escribe en la memoria del compositor.
- **El invitado no tiene GPU.** Mesa dibuja con llvmpipe (OpenGL) o lavapipe (Vulkan), con la CPU del invitado.
- **DashOS no tiene GL ni Vulkan.** Mesa no está portado (S9, fase C; G8 de `docs/GRAFICOS_LOOP.md`).
- **El servicio `gpu` de DashOS** (`usuario/src/bin/gpu.rs`) enciende la V3D de la Pi 4, prepara su MMU y hace un
  borrado de 64 × 64 píxeles con una lista de control propia. Nunca se ha probado: falta una Pi 4. No atiende a nadie.
- **virtio-gpu en DashOS** existe como *controlador* (`usuario/src/bin/virtio_gpu.rs`): DashOS invitado de QEMU, en
  2D, para el multimonitor. La caja no tenía un *aparato* virtio-gpu para su invitado.
- **QEMU no emula la V3D** (ni `raspi4b` ni `virt`). Ni la `virt` de las pruebas tiene GPU 3D para DashOS.

## Los caminos

| Camino | Qué corre en el invitado | Qué pide a DashOS | Comparte la GPU con DashOS | Se prueba en QEMU |
|---|---|---|---|---|
| A. virtio-gpu, contexto nativo de v3d | Mesa v3d/v3dv con un "backend" de virtio-gpu (no existe para v3d) | Atender la API de trabajos de v3d (crear búferes, mandar listas, TFU, cómputo, esperar) sobre virtio-gpu con `CONTEXT_INIT` y recursos *blob* | Sí, por trabajos | Solo la parte de virtio-gpu |
| B. virtio-gpu con Venus (Vulkan) o virgl (GL) | Mesa venus o virgl, tal cual | Mesa portado a DashOS (v3dv o v3d) y un intérprete de Venus o de virgl | Sí | Con una GPU del PC detrás, no con la de la Pi |
| C. La V3D mediada (paso directo con trampas) | Linux con su controlador v3d y Mesa v3d/v3dv, **tal cual** | Dar los registros de la V3D atrapados, una tabla de páginas sombra, la interrupción y el reinicio | Por turnos (al principio, la caja la tiene sola) | La lógica (tabla sombra, registros) con pruebas en el PC; la GPU, no |
| D. Paso directo sin más | Igual que C | Proyectar los registros en la etapa 2 | No | No |
| E. Solo virtio-gpu 2D | llvmpipe/lavapipe, como hoy | El aparato 2D (recursos, transferencias, pantalla) | — | Sí, entero |

### A. Contexto nativo (virtio-gpu, `CONTEXT_INIT`)

El invitado manda por virtio-gpu las mismas órdenes que mandaría al controlador v3d de Linux; DashOS las valida y las
pasa a la V3D. Es lo que hace ChromeOS con las GPU de Qualcomm, AMD e Intel.

- A favor: casi sin coste por cuadro (las listas de control no se traducen), la GPU se comparte por trabajos con
  DashOS y con otras cajas, y el lado de DashOS es justo el servicio de v3d que pide la fase C de S9 (el que atenderá a
  Mesa en DashOS). La validación es barata: todo lo que hace la V3D pasa por su MMU (`v3d.md`, 5 y 6), así que basta
  con que la tabla de cada contexto solo apunte a memoria de ese contexto.
- En contra: **Mesa no tiene ese "backend" para v3d** (lo tiene para otras GPU). Habría que escribirlo dentro de Mesa,
  y la sala limpia no deja leer Mesa a quien implementa. Es trabajo de Mesa, no de DashOS.

### B. Venus o virgl

El invitado serializa las llamadas de Vulkan (Venus) o de GL (virgl) y DashOS las ejecuta con su propio controlador.

- A favor: el invitado ya trae Mesa venus y virgl. Venus va bien en GPU grandes.
- En contra: hace falta **Mesa portado a DashOS** (v3dv para Venus, v3d para virgl) y un intérprete de esos protocolos
  (otro proyecto grande, sin poder leer virglrenderer). Serializar cuesta CPU, y la de la Pi 4 es justa. virgl además
  está de salida y la V3D solo llega a GL ES 3.1. Sin la fase C de S9 no hay nada que hacer aquí.

### C. La V3D mediada

El invitado ve una V3D 4.2 como la de la Pi, con su controlador de Linux de siempre (`CONFIG_DRM_V3D`) y el Mesa de Arch.
Cada acceso a los registros de la V3D sale a DashOS (como hoy con virtio-mmio), y DashOS lo hace por él en la de
verdad, salvo lo que es peligroso:

- **La MMU de la V3D.** La V3D ve la RAM física entera sin desplazamiento y la BCM2711 no tiene IOMMU delante
  (`v3d.md`, 2.6). Si el invitado programara la MMU, o la apagara, leería y escribiría toda la RAM de la Pi. Así que
  DashOS no le deja: atrapa `MMU_CTL`, `MMU_PT_PA_BASE`, `MMU_ILLEGAL_ADDR` y `MMUC_CONTROL`, y le da a la V3D una
  **tabla sombra** suya. La tabla del invitado vive en la RAM del invitado (su dirección es una IPA); cuando el
  invitado vacía la TLB (Linux lo hace siempre tras cambiar entradas, `v3d.md`, 5.6), DashOS lee esa tabla, traduce
  cada página (IPA → física) y solo copia las que caen en la RAM de esa caja. Las demás, como no válidas.
- **Encender, apagar, reiniciar y el reloj**: los hace el servicio `gpu` de DashOS. El invitado ve un reinicio
  emulado que acaba en el de verdad.
- **La interrupción** (SPI 74): la recibe DashOS y la inyecta en el GIC virtual de la caja.
- **Los plazos**: si un trabajo del invitado no acaba (bucles en una lista, `v3d.md`, 10.1), DashOS reinicia la GPU y
  se lo dice al invitado como un reinicio. El invitado no puede dejar la GPU colgada para DashOS. Cada reinicio por
  plazo deja una línea en `Estado:Registros/` (S6), con la caja y la hora.

A favor: **el invitado no necesita nada nuevo** (el Linux y el Mesa de Arch), corre a velocidad de la GPU (las
listas no se tocan, la MMU confina), y casi todo lo que pide a DashOS sirve después para A. En contra: la GPU es de la
caja mientras la usa (DashOS no la usa hoy para nada; repartirla por trabajos es un paso después, atrapando también los
registros que lanzan trabajos), y la tabla sombra cuesta un repaso de la tabla del invitado en cada vaciado (hay que
limitarlo a las páginas cambiadas, con protección de escritura en la etapa 2).

### D. Paso directo sin trampas

Nunca: el invitado apagaría la MMU de la V3D o le daría direcciones físicas de DashOS (sin IOMMU, nada lo para).

### E. virtio-gpu 2D

No acelera el dibujo: Mesa sigue usando la CPU. Pero es la base de A (las mismas colas, recursos y pantallas), da al
invitado una pantalla de verdad (`/dev/dri/card0`: un programa de KMS, o un escritorio de Linux entero en una ventana de
DashOS) y un camino para los búferes de `linux-dmabuf` que no pasa por vsock. Y es lo único que se prueba entero en
QEMU.

## Seguridad: qué hay que validar

El invitado no es de fiar. Con cualquier camino:

- **Memoria**: toda dirección que mande el invitado (colas, descriptores, el respaldo de un recurso, la tabla de la
  V3D) se comprueba contra *su* RAM; nada fuera, nada que desborde. Con la V3D, la tabla que ve el hardware es siempre
  la sombra, nunca la del invitado (que puede cambiarla mientras se lee: se lee una vez y se usa la copia).
- **Tamaños**: recursos, transferencias y listas, con topes y con aritmética comprobada (un 65536 × 65536 × 4 desborda
  32 bits). La memoria que gasta DashOS por el invitado, con un presupuesto por caja.
- **Números**: ids de recurso repetidos, que no existen, el 0; pantallas que no existen.
- **Tiempo**: ningún trabajo de la GPU dura para siempre (plazo y reinicio); ninguna orden hace trabajar a DashOS sin
  tope (las colas se vacían como mucho de su tamaño por aviso).
- **Estado**: un fallo del invitado lo deja a él sin GPU (DEVICE_NEEDS_RESET, o la V3D reiniciada), nunca a DashOS.
- **Rastro**: lo que se niega o se reinicia se apunta en `Estado:Registros/` (S6), con la caja, el motivo y la hora:
  cada reinicio de la GPU por plazo y cada entrada de la tabla del invitado que la sombra rechaza (lo repetido se
  resume). Se lee con la orden `Registro`. Si se repite, aviso en llano al usuario.
- **Lo que se ve**: lo que el invitado muestra va a una ventana suya, con su título; nunca a una capa o al bloqueo.

## Recomendación

**C para la Pi 4, con E debajo para la pantalla y los búferes, y A como meta cuando DashOS tenga su servicio de v3d.**

1. **Ya (QEMU): virtio-gpu 2D en la caja (E).** Hecho en esta rama: el aparato, sus pruebas y hostiles, y la pantalla
   del invitado como ventana de DashOS. Es la pieza de pantalla que C necesita (la V3D no tiene pantalla: dibuja en
   búferes que otro muestra) y la base de A.
2. **Ya (PC): la tabla sombra de la V3D**, como lógica pura con sus pruebas hostiles: es lo único de C que es
   peligroso y se puede probar sin una Pi. Hecho en esta rama (`caja-linux/vmm/src/v3d.rs`).
3. **En una Pi 4: C entera.** El invitado usa la V3D con el Mesa de Arch; sus búferes salen por `linux-dmabuf` (el
   puente ya los acepta, G6) hacia el compositor.
4. **Después: A**, si alguien escribe el "backend" de v3d en Mesa (fuera de la sala limpia de DashOS) y DashOS ya
   reparte la GPU entre sus programas y las cajas.

B no se recomienda: necesita Mesa portado a DashOS para dar a la caja lo que C da sin portarlo.

Para depurar, un ajuste de texto en el fichero de la caja en `ENVARC:` (S7) fuerza el dibujo por CPU (llvmpipe)
aunque haya GPU. Así se ve si un fallo es de la GPU. El nombre del ajuste, por diseñar al hacerlo.

### Lo que C pide al kernel de DashOS

1. **Registros de un aparato atrapados para un invitado**: hoy toda dirección que no es RAM sale al servicio
   (`maquina_correr`, motivo de aparato), así que basta con que el servicio de la caja tenga la llave de los registros
   de la V3D (o hable con el servicio `gpu`, que la tiene). Más adelante, para no salir en cada acceso: proyectar en la
   etapa 2 las páginas sin peligro (las del núcleo 0, `0xFEC04000`) como memoria de aparato (Device-nGnRE), con una
   llamada nueva tipo `maquina_mmio(vm, llave de registros, ipa)` que solo acepte llaves de registros.
2. **Saber qué página física hay detrás de cada página de la RAM del invitado**, para la tabla sombra: una llamada que
   dé las direcciones físicas de un objeto de memoria, solo a quien tenga el permiso DMA de la V3D.
3. **Que esas páginas no se muevan ni se suelten** mientras la V3D las tenga en su tabla (hoy no se mueven; al cerrar
   la caja hay que parar la V3D antes de soltar su RAM).
4. **Protección de escritura de la etapa 2 en un rango** (con aviso de qué página se escribió), para repasar solo las
   páginas de la tabla del invitado que cambiaron.
5. **La interrupción de la V3D** entregada al servicio que inyecta en la caja (ya existe el camino de llaves de
   interrupción y la inyección en el GIC virtual).
6. Atributos: el invitado mapea los búferes de la GPU sin caché (`v3d.md`, 2.7); la etapa 2 da memoria normal con caché
   y la combinación queda sin caché, como debe. No hay que tocar nada, pero hay que comprobarlo en la Pi.

### Lo que C pide al servicio `gpu` de DashOS

1. Encender la V3D (ya lo hace, sin probar) y dejarla a una caja: un canal con "dámela", "suéltala" y "reiníciala".
   Y una pregunta de solo lectura para saber quién tiene la GPU y cuánto lleva gastado del presupuesto de su caja. La
   enseña una orden de Shell (la orden `Caja` de S7 lo dice para cada caja), no solo los servicios entre sí.
2. La tabla sombra (4 MB contiguos, sin caché) y su página de relleno, y programar la MMU con ellas.
3. Los registros atrapados: leer y escribir en la V3D lo que pide el invitado, salvo los de la MMU y los de energía,
   que emula.
4. El plazo de los trabajos y el reinicio (como `v3d.md`, 10), avisando al invitado.
5. Al soltarla: parar la V3D, borrar la tabla sombra y vaciar la TLB antes de que la caja suelte su RAM.

### Lo que hay que mirar en la Pi 4 (no se puede en QEMU)

- Que el controlador v3d de Linux arranca con un nodo del árbol hecho por la caja (`brcm,2711-v3d`, `hub` y `core0`,
  la interrupción) sin reloj, sin dominio de energía y con un reinicio emulado. El árbol del firmware pone `clocks`,
  `power-domains` y `resets` (`v3d.md`, 2.3 y 2.4): hay que ver qué hace Linux sin ellos o darle unos emulados.
- Que la V3D respeta la tabla sombra en todas sus unidades (CLE, TFU, CSD, PTB), y qué pasa con la página de relleno.
- Las medidas: cuadros por segundo de `vkcube` y `es2gears_wayland` con la V3D frente a llvmpipe, y cuánto cuesta
  cada salida por un registro.

## Qué se puede probar en QEMU y qué no

| Pieza | QEMU `virt` | Pi 4 |
|---|---|---|
| virtio-gpu 2D: colas, recursos, transferencias, pantalla, hostiles | Sí | Sí (lo mismo) |
| La pantalla del invitado como ventana de DashOS | Sí | Sí |
| Un programa de GL en el invitado por KMS o por Wayland | Sí, con llvmpipe | Sí, con llvmpipe; con la V3D tras C |
| La tabla sombra de la V3D (traducción, límites, hostiles) | Sí, en el PC (`cargo test`) | Sí, con la V3D de verdad |
| Atrapar los registros de la V3D, su interrupción, el reinicio | No (no hay V3D) | Sí |
| Cuadros por segundo con GPU | No | Sí |

Para probar A o B en QEMU haría falta que QEMU diera a DashOS una virtio-gpu con 3D (virgl o Venus sobre la GPU del
PC) y que DashOS pasara a esa las órdenes del invitado. Serviría para probar el reparto por contextos, pero no se
parece a la Pi (allí DashOS no tiene virtio-gpu, sino la V3D), así que no se recomienda como paso.

## Qué se hizo (primer paso)

- **virtio-gpu en 2D en la caja** (`caja-linux/vmm/src/virtio/gpu.rs`) con sus pruebas y hostiles, y la pantalla del
  invitado en una ventana de DashOS (`usuario/src/pantalla_caja.rs`). Linux la usa con su controlador `virtio_gpu`.
- **La tabla sombra de la V3D** (`caja-linux/vmm/src/v3d.rs`), con pruebas hostiles en el PC.
- **`make arranque-virt-gpu`**: el mismo programa de GL (`cuadros_gl`, llvmpipe) va a 9 a 13 cuadros por segundo por
  Wayland (`wl_shm` por el puente) y a 17 a 20 por la pantalla de la virtio-gpu, en QEMU.
- **No se hizo `linux-dmabuf` para búferes de virtio-gpu**: Mesa por software usa `wl_shm` aunque el compositor
  anuncie dmabuf (comprobado con el mismo Mesa en el PC), así que en QEMU no lo usaría nadie. Hace falta con la V3D.

Detalle, medidas y pruebas en `docs/bitacoras/caja-linux.md` (entrada "la tarjeta gráfica").
