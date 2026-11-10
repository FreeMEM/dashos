# Gráficos: Wayland completo, puente para Linux y GPU (plan y bitácora)

Encargo del usuario (2026-11-10): el puente de Wayland para la caja Linux, y planificar e implementar lo que falta:
subsuperficies, búferes de GPU (`linux-dmabuf`) y las extensiones de escritorio (`layer-shell`, bloqueo de sesión).
Pregunta suya: si pintar y componer con la CPU es lo óptimo, o lo suyo es que lo haga la tarjeta gráfica.

Se trabaja en el mismo loop que la revisión (`docs/REVISION_LOOP.md`), rama `revision`, con las mismas reglas: cada
paso con su prueba, medido si toca rendimiento, el panel de medidas al día y commit local.

## Qué hay hoy

- `wayland/`: el protocolo Wayland con los mensajes de los XML oficiales y el transporte propio de DashOS (canales y
  llaves en vez de socket y descriptores). Núcleo, `wl_shm`, `wl_surface`, `xdg-shell`, `wl_seat` con ratón y teclado
  sin mapa de xkb.
- Cada programa pinta con la CPU en memoria compartida y el compositor junta las ventanas con la CPU, solo lo dañado.
- Nunca se ha probado con un cliente de libwayland.

## Respuesta a la pregunta: quién debe pintar

La CPU no es lo óptimo; vale para QEMU, que no tiene GPU. En la Pi 4 (S9):

1. **Componer: los planos del HVS**, no la GPU. El HVS monta la imagen al vuelo, al mandarla a la pantalla, con cada
   ventana en un plano: sin copiar píxeles ni gastar memoria por cuadro. Es lo que hacen los móviles (Hardware
   Composer) y los planos de KMS. Con ventanas que no caben en planos (el HVS tiene un tope de ancho de banda y de
   planos por línea), se juntan esas con la GPU o, sin ella, con la CPU solo lo dañado.
2. **Pintar cada programa: la GPU (V3D)**, con OpenGL ES o Vulkan (Mesa), en búferes que comparte con el compositor sin
   copiarlos: eso es `linux-dmabuf`. Los programas sencillos pueden seguir con la CPU (`wl_shm`).
3. **La CPU, de reserva**: QEMU, falta de planos o de GPU.

Lo que falta para eso es hardware sin manual: el HVS tiene especificación en `docs/spec/hw/hvs.md` (fase B de S9); la
V3D necesita Mesa portado y el lado del kernel de v3d (fase C de S9). QEMU no emula ni HVS ni V3D: se prueba en la Pi.

## Orden

Primero lo que se puede probar ya con programas reales en el PC; después lo que necesita la Pi.

| Paso | Qué | Prueba | Estado |
|---|---|---|---|
| G0 | El compositor de `wayland/` en el PC, detrás de un socket de Unix (crate `puente/`, con std), que vuelca la pantalla a un PPM | Un cliente propio en C con libwayland pinta una ventana y la captura sale como debe | hecho |
| G1 | El puente: socket de Wayland y descriptores en un lado, trozos de canal y llaves en el otro. `wl_shm` por copia de lo dañado al hacer `commit` (como Sommelier sin memoria compartida con la máquina virtual). El transporte, una interfaz: hoy un socket al compositor del PC, en la caja un `vsock` | El mismo cliente, por el puente, y el compositor de DashOS en QEMU recibiendo lo de un cliente del PC | pendiente |
| G2 | Mapa de teclado de xkb (`wl_keyboard.keymap`): texto del mapa en un objeto de memoria; el puente lo da como `memfd` | Los clientes de libwayland lo piden siempre: sin él, muchos no arrancan | pendiente |
| G3 | Subsuperficies: `wl_subcompositor`, `wl_subsurface` (posición, orden, modo síncrono y no), y `wp_viewporter` si hace falta | Pruebas de PC del árbol de superficies; los bordes de `alacritty` | pendiente |
| G4 | `zwlr_layer_shell_v1`: fondo, barras y avisos (capas, anclas, márgenes, zona exclusiva, teclado). Solo para clientes con el poder de escritorio (S10b: por la marca) | `swaybg` pinta el fondo; un cliente sin el poder recibe error | pendiente |
| G5 | `ext_session_lock_v1`: bloqueo. Mientras dura, solo se ven y reciben entrada las superficies del bloqueo; si su cliente muere, la pantalla sigue bloqueada. Solo `guardian` (S6b) o quien tenga el poder | `swaylock`; un cliente hostil no ve ni recibe nada bloqueado | pendiente |
| G6 | `zwp_linux_dmabuf_v1` (v4, con feedback de formatos): un búfer es un objeto de memoria con formato, modificador y planos. En QEMU se lee con la CPU; en la Pi, va a un plano del HVS si cabe. En el PC, el puente pasa los dmabuf de Mesa a memoria (copia) | `alacritty` por el puente en el PC; prueba de PC de formatos y errores | pendiente |
| G7 | Planos del HVS en el compositor (fase B de S9): cada ventana a un plano mientras quepan; el resto, juntado por CPU en un plano más. Cursor en su plano | En la Pi real: cuadros por segundo y CPU usada, antes y después | pendiente |
| G8 | V3D (fase C de S9): Mesa con relibc y el servicio de v3d. Plan aparte | — | fuera de este loop |

Licencias: los XML de `linux-dmabuf`, `ext-session-lock` y `viewporter` (wayland-protocols) y de `wlr-layer-shell`
(wlr-protocols) son MIT, como los que ya hay en `wayland/protocolos/`. Se implementa desde los XML y su texto, sin leer
código de compositores.

## Medidas para el panel

- Tiempo de componer un cuadro con 1, 3 y 8 ventanas, por CPU (QEMU y la Pi), y con planos del HVS (Pi).
- Mensajes por segundo del compositor y latencia de un `frame` (desde el `commit` hasta el aviso).
- Por el puente: coste de la copia de `wl_shm` por cuadro.

## Bitácora

### 2026-11-10

- Plan escrito. En el PC hay `swaybg`, `swaylock`, `waybar` y `alacritty`, y libwayland 1.26 con `wayland-scanner`
  para un cliente de prueba propio.
- G0 hecho: `puente/` con `compositor-pc` (el servidor y la escena de `wayland/` tras un socket de Unix; los
  descriptores que llegan hacen de llaves, `mmap` para `wl_shm`) y `puente/pruebas/cliente_shm.c` (libwayland 1.26,
  `xdg-shell`, 10 cuadros por `frame`). Primera prueba contra libwayland: ningún error de protocolo; la captura tiene
  la ventana y la banda del último cuadro. `puente/pruebas/g0.sh`, dentro de `make prueba`.

