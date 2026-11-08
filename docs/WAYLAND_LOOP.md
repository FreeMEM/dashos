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
| W2 | El transporte: un canal "ventanas" que da a cada cliente su canal propio de ida y vuelta, con su marca (S10b: se sabe quién es por la marca, no por lo que diga) | |
| W3 | El servidor de objetos: `wl_display`, `wl_registry`, `wl_callback`, errores de protocolo que cortan la conexión | |
| W4 | Memoria compartida: `wl_shm`, `wl_shm_pool` (la llave es un objeto de memoria que el compositor proyecta solo para leer), `wl_buffer` | |
| W5 | Superficies: `wl_compositor`, `wl_surface` (attach, damage, frame, commit con estado pendiente y aplicado) y la composición por CPU con daños | |
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
