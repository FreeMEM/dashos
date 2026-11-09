# Bucle del compositor Wayland (E1 de S10b)

Orden del usuario (2026-11-09): tras FS10, los gráficos (este compositor y después el kit de interfaz en Rust); el
sonido (S19) detrás. Diseño: S9 (Wayland como protocolo propio, sobre canales y llaves) y S10b (fase E1: `xdg-shell` y
memoria compartida, dos clientes de prueba, captura por QMP comparada con la esperada).

## Dónde está

- Copia de trabajo `.worktrees/wayland`, rama `wayland`, desde la de FS10 (`archivos-cifrado`).
- `wayland/`: la biblioteca del protocolo (no_std, se prueba en el PC con `cargo test`). Las tablas salen de los XML
  oficiales de `wayland/protocolos/` (MIT) con `tools/wayland_tablas.py`.
- El compositor provisional de C50 está en `usuario/src/bin/compositor.rs`: pinta por CPU, doble búfer y daños, una
  ventana que mueve el ratón. No habla Wayland.

## Pasos

| Paso | Qué | Estado |
|------|-----|--------|
| W1 | El formato de los mensajes: cabecera, argumentos por su firma, llaves en vez de descriptores; tablas desde los XML | hecho |
| W2 | El transporte: un canal "ventanas" que da a cada cliente su canal propio de ida y vuelta, con su marca (S10b: se sabe quién es por la marca, no por lo que diga) | trozos y juntador hechos; el reparto de llaves por `inicio`, con W3 |
| W3 | El servidor de objetos: `wl_display`, `wl_registry`, `wl_callback`, errores de protocolo que cortan la conexión | hecho (`servidor.rs`) |
| W4 | Memoria compartida: `wl_shm`, `wl_shm_pool` (la llave es un objeto de memoria que el compositor proyecta solo para leer), `wl_buffer` | hecho (`shm.rs`) |
| W5 | Superficies: `wl_compositor`, `wl_surface` (attach, damage, frame, commit con estado pendiente y aplicado) y la composición por CPU con daños | hecho (`escena.rs`) |
| W6 | Ventanas: `xdg_wm_base` (ping), `xdg_surface` (configure y su respuesta), `xdg_toplevel` (título, cerrar), colocación en cascada, foco y orden con el ratón | |
| W7 | Dos clientes de prueba en Rust (un degradado y un tablero, con animación por `frame`) y la prueba: captura por QMP en QEMU `virt`, comparada con la esperada | |
| W8 | Entrada: `wl_seat`, `wl_pointer`, `wl_keyboard` (sin keymap de xkb al principio), foco del teclado | |

## Bitácora

### 2026-11-09

- W1: `wayland/` con `Escritor` (mensajes seguidos en un búfer, llaves aparte), `leer_cabecera` y `leer_args` por la
  firma; errores de mensaje corto, tamaño, texto (sin 0, no UTF-8, nulo sin permitir), objeto nulo, llave que falta y
  bytes de más. Tablas de `wayland.xml` (sin `wl_shell`) y `xdg-shell.xml`, con los números de cada petición y evento.
  Pruebas: `get_registry` byte a byte como lo manda cualquier cliente, `global` y `bind` (el objeto nuevo sin
  interfaz en la firma), llaves y mensajes seguidos, textos, arrays y fixed, mensajes malos, tablas como en el XML.
  Sin probar todavía contra libwayland: se hará cuando hable con un cliente de verdad (la caja Linux o un puente).
- W2: el kernel no espera en varios canales a la vez y un mensaje de canal lleva hasta 256 bytes, 8 llaves y 16 en
  cola. Así que el compositor tendrá un solo canal de entrada: cada cliente le escribe por una llave marcada por
  `inicio` (la marca es la conexión y dice quién es), y le pasa al conectarse el canal por el que el compositor le
  contesta. El flujo de Wayland va en trozos de hasta 255 bytes tras un byte de tipo (datos, conectar, adiós):
  mensajes enteros juntos mientras quepan, partido solo el que no cabe, y las llaves de cada mensaje en su primer
  trozo, para que nunca lleguen después del mensaje que las usa. `Juntador` rehace los mensajes y guarda las llaves
  en orden; con más de 64 KB sin un mensaje entero, falla. Pruebas: 40 mensajes pequeños en dos trozos, un título de
  800 bytes partido, 20 `create_pool` (los corta el tope de 8 llaves), basura.
  Límite: la cola de 16 mensajes del canal de entrada es de todos los clientes y de la entrada; con la cola llena,
  quien escribe reintenta. Si se queda corta, habrá que subirla en el kernel o dar un anillo por cliente (S1).
- W3: `Conexion` lleva los objetos de un cliente (con su interfaz y versión), lo que llega a medias y lo que hay que
  mandarle. Hace lo común: `sync` (done y delete_id), `get_registry` (un `global` por global), `bind` (misma
  interfaz, versión entre 1 y la del global), objetos nuevos con la versión de quien los crea, destructores con
  `delete_id`, y los errores de protocolo (`wl_display.error`; después no se atiende nada más). Lo demás va a un
  `Servicio`. Pruebas con un servicio que apunta: registro, sync, bind en la versión 4 y una superficie que la hereda,
  destroy; y cada error: objeto que no existe, petición más nueva que el objeto, orden que no existe, bind de más
  versión o de otra interfaz, número nuevo repetido o del servidor, argumentos que no cuadran. Sin la comprobación de
  versión, la prueba falla.
- W4: toda la lógica del compositor va en la biblioteca, para probarla en el PC con memoria falsa (`Memorias`); el
  programa de DashOS será la capa fina (canales, `memoria_proyectar`, pantalla). `Shm`: pools (hasta 64 MB, solo
  crecen, se proyectan otra vez al crecer), búferes (ARGB8888 y XRGB8888; dentro del pool, línea de al menos
  ancho × 4, sin desbordar con números enormes) y sus píxeles. Destruir un pool no quita la memoria a sus búferes: se
  suelta con el último. Pruebas: un píxel pintado por el "cliente" se lee donde toca; cada error de `wl_shm`.
- W5: `Escena` lleva lo de cada cliente (su `Shm` y sus superficies) y el orden de apilado. `wl_surface`: attach,
  damage, frame y commit, que aplica el estado pendiente entero; al cambiar de búfer, el viejo se devuelve con
  `wl_buffer.release`. `componer` pinta por CPU solo lo dañado (fondo y superficies de abajo arriba; XRGB tal cual y
  ARGB premultiplicado mezclado con lo de debajo) y manda los avisos de `frame` de lo que está en pantalla con la hora.
  Al empezar, la pantalla entera está por pintar (lo encontró la prueba: el fondo salía a cero). `wl_shm` anuncia sus
  dos formatos al enlazarlo. Pruebas: dos clientes con una superficie cada uno, apiladas y solapadas (fondo, rojo,
  azul a medias sobre rojo y sobre fondo, comprobados al píxel), avisos de frame, nada que componer sin cambios,
  release al cambiar de búfer, un cliente que se va; adjuntar algo que no es un búfer corta la conexión.

