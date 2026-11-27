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
| G1 | El puente: socket de Wayland y descriptores en un lado, trozos de canal y llaves en el otro. `wl_shm` por copia de lo dañado al hacer `commit` (como Sommelier sin memoria compartida con la máquina virtual). El transporte, una interfaz: hoy un socket al compositor del PC, en la caja un `vsock` | El mismo cliente, por el puente, y el compositor de DashOS en QEMU recibiendo lo de un cliente del PC | hecho; en DashOS desde la caja Linux por vsock (H9, `make arranque-virt-wayland`) |
| G2 | Mapa de teclado de xkb (`wl_keyboard.keymap`): texto del mapa en un objeto de memoria; el puente lo da como `memfd` | Los clientes de libwayland lo piden siempre: sin él, muchos no arrancan | hecho (mapa español; elegir en Prefs, después) |
| G3 | Subsuperficies: `wl_subcompositor`, `wl_subsurface` (posición, orden, modo síncrono y no), y `wp_viewporter` si hace falta | Pruebas de PC del árbol de superficies; los bordes de `alacritty` | hecho (sin wp_viewporter) |
| G4 | `zwlr_layer_shell_v1`: fondo, barras y avisos (capas, anclas, márgenes, zona exclusiva, teclado). Solo para clientes con el poder de escritorio (S10b: por la marca) | `swaybg` pinta el fondo; un cliente sin el poder recibe error | hecho (sin get_popup) |
| G5 | `ext_session_lock_v1`: bloqueo. Mientras dura, solo se ven y reciben entrada las superficies del bloqueo; si su cliente muere, la pantalla sigue bloqueada. Solo `guardian` (S6b) o quien tenga el poder | `swaylock`; un cliente hostil no ve ni recibe nada bloqueado | hecho (una pantalla; el relevo, por el poder) |
| G6 | `zwp_linux_dmabuf_v1` (v4, con feedback de formatos): un búfer es un objeto de memoria con formato, modificador y planos. En QEMU se lee con la CPU; en la Pi, va a un plano del HVS si cabe. En el PC, el puente pasa los dmabuf de Mesa a memoria (copia) | `alacritty` por el puente en el PC; prueba de PC de formatos y errores | hecho en el PC (en DashOS, con la caja) |
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
- G1 hecho: `puente/src/bin/puente.rs` (el lado de la caja: escucha como un compositor, sigue los objetos con las
  tablas de `wayland/`, proyecta cada pool de `wl_shm` y copia el búfer puesto en cada `commit`; otros descriptores,
  una copia; los que manda DashOS, `memfd`) y el transporte de tramas (`puente/src/lib.rs`: conectar, datos con
  llaves, adiós y memorias con número). `compositor-pc --transporte` hace de lado de DashOS. Prueba de punta a punta:
  `puente/pruebas/g1.sh`, dentro de `make prueba`: misma captura que en directo. Falta, con la caja Linux: `vsock` en
  vez del socket y el lado de DashOS con canales y objetos de memoria; y copiar solo lo dañado.
- G1, copiar solo lo dañado: el puente guarda el daño de los últimos 8 cuadros de cada superficie y, al hacer
  `commit` con un búfer, copia la unión de las filas cambiadas desde la última vez que mandó ese búfer (con doble
  búfer, el daño del programa es frente al cuadro anterior, no frente a ese búfer). El cliente de prueba gana un modo
  `DOBLE=1` (dos búferes y solo la banda vieja y la nueva dañadas) y las pruebas miran que no quede una banda vieja.
  Medida en la ventana de 200 x 150: antes 120.000 bytes por cuadro; ahora 43.200 de media en 10 cuadros (los dos
  primeros, enteros) y 24.000 en marcha (-80 %). Va al panel de medidas con las primeras de la revisión.
- G2 hecho: `wl_keyboard.keymap` en formato `xkb_v1` con el mapa español compilado de xkeyboard-config
  (`wayland/teclados/es.xkb`, 73 KB, con su aviso de licencia). `Memorias::llave_con`: el compositor de DashOS lo
  escribe una vez en un objeto de memoria y da a cada cliente una copia de solo lectura; el del PC, un `memfd`. El
  cliente de prueba lo lee con libwayland: bien en directo y por el puente (donde cruza de DashOS a Linux como
  memoria). Sin mapa (no_keymap) solo si no se puede crear la memoria.
- G3 hecho: `wl_subcompositor` y `wl_subsurface`. Cada superficie lleva su pila (ella y sus hijas, de abajo arriba) y
  cada hija su sitio respecto a la madre; componer, dañar, avisar de cuadro y buscar bajo el ratón recorren el árbol
  (`escena::arbol`). Sincronizada (por defecto): su commit se guarda hasta el de la madre; la posición siempre con el
  de la madre; `place_above` y `place_below` frente a la madre o hermanas; `set_desync` aplica lo guardado. El
  teclado va a la ventana aunque se pulse una hija. Errores `bad_surface`, `bad_parent` (también ciclos) y
  `bad_sibling`. Pruebas de PC (27 en total) y, con libwayland, un botón rojo de 20 x 20 como subsuperficie en el
  cliente de prueba: en su sitio en directo y por el puente. Falta `wp_viewporter` (escalar), que va con G6.
- G4 hecho: `zwlr_layer_shell_v1` (v4) y `wl_output` (v4). Capas de fondo y abajo debajo de las ventanas, arriba y
  encima por encima (`Escena::raices`); tamaño 0 = lo que dé la pantalla entre sus anclas; sitio por anclas y
  márgenes; configure al cambiar el tamaño; zona exclusiva que recorta el área de las ventanas (cascada y maximizar
  dentro de `area`); teclado exclusivo en las capas de arriba, a petición con un clic. Errores de rol, capa, tamaño,
  anclas, teclado y búfer antes de configure. Poder de escritorio: solo quien lo tiene ve el global (en DashOS, la
  lista `ESCRITORIOS` del compositor, vacía hasta que haya escritorio; `compositor-pc --escritorio` en el PC).
  Un búfer o un pool que el cliente destruye mientras se ve sigue vivo con un número privado hasta que se cambia
  (como wlroots: `swaybg` destruye el suyo tras el commit, y reusa el número del pool).
  Pruebas: 29 de PC (zona exclusiva con ventana debajo, errores) y `puente/pruebas/g4.sh`: **`swaybg` de verdad**
  pinta el fondo naranja con la ventana del cliente encima, y sin el poder no puede. Falta `get_popup` (menús de
  una barra), con `xdg_popup`.
- G5 hecho: `ext_session_lock_manager_v1`, `ext_session_lock_v1` y `ext_session_lock_surface_v1`. Al bloquear deja de
  verse todo al momento (fondo negro) y nadie tiene el teclado ni el ratón; la pantalla de bloqueo, del tamaño de la
  pantalla tras su configure, se ve sola y toma el teclado. Un segundo `lock` mientras vive el primero recibe
  `finished`. Si el bloqueador muere, sigue bloqueado en negro: solo otro con el poder puede tomar el relevo y quitarlo
  con `unlock_and_destroy` (`destroy` sin desbloquear es error). Super+Arriba/Abajo no hacen nada bloqueado. Errores
  de rol, búfer antes de configure, tamaño, configure que no se mandó y pantalla repetida. El global, solo con el
  poder de escritorio (en DashOS lo tendrá `guardian`, S6b). Pruebas: 31 de PC (bloqueo, relevo, errores) y
  `puente/pruebas/g5.sh`: **`swaylock` de verdad** tapa la pantalla de verde, una ventana que se abre detrás no se ve
  ni completa cuadros, al matar a swaylock queda negra, y sin el poder swaylock acaba con error. `compositor-pc
  --captura x%d.ppm` guarda una captura cada vez que se va un cliente.
- G6 hecho en el PC: `zwp_linux_dmabuf_v1` v4. Dos formatos (AR24 y XR24, los de `wl_shm`) solo lineales: la tabla de
  formatos va en una memoria de solo lectura (`Memorias::llave_con`, que en DashOS ya guarda varios datos fijos, no
  solo el mapa de teclado), con `main_device` el `dev_t` de la escena (`compositor-pc --dri`, por defecto
  `renderD128`; en DashOS 0 hasta V3D). Un búfer de dmabuf es un pool de un búfer, que se suelta con él; antes de
  leerlo tras el commit, `Memorias::antes_de_leer` (en Linux, `DMA_BUF_IOCTL_SYNC`: espera a la tarjeta). `create`
  da el búfer con número del servidor y `created`; un modificador no lineal, `failed` (o `invalid_wl_buffer` con
  `create_immed`). Errores de params: usado, plano, plano repetido, incompleto, formato, medidas, fuera. El puente
  trata el plano como un pool de `wl_shm` (proyecta, espera a la tarjeta, copia lo dañado) y suelta la memoria con el
  búfer. Pruebas: 33 de PC y `puente/pruebas/g6.sh`: **alacritty de verdad** (Mesa, EGL) pinta su fondo verde en
  búferes de dmabuf, directo y por el puente (590 KB por cuadro: alacritty daña la ventana entera). Pendiente: el
  global en el compositor de DashOS (cuando la caja exista: el puente tendrá que cambiar `main_device` por el
  dispositivo de la caja), `create` no inmediato por el puente (Mesa usa `create_immed`) y `wp_viewporter`.

### 2026-11-12 (rama `caja-linux-wayland`)

- G1 en DashOS: el puente corre dentro de Arch Linux ARM en la caja Linux y habla con DashOS por vsock (aparato nuevo en
  `caja-linux/vmm`). El lado de DashOS es `wayland::puente` (sin `std`) en el servicio de la caja: cada cliente de Linux
  es una subconexión del compositor (`canal::TROZO_SUB_*`, solo para "caja-linux", sin poderes) y cada memoria, un
  objeto de memoria de DashOS. `ventana_wayland` (libwayland, wl_shm) y foot salen en el escritorio con teclado y
  ratón. Detalle y medidas en `docs/bitacoras/caja-linux.md`.
- Un objeto de memoria de DashOS no crece: el puente pasa un pool que crece más allá de su memoria a un pool sucesor
  (número propio, memoria nueva) y crea ahí los búferes nuevos. Suelta las memorias de pools destruidos sin búferes.
- `wl_data_device_manager` v3 anunciado (foot no arranca sin él), sin selecciones todavía.

