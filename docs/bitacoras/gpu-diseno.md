# Bitácora: el escritorio en la GPU (rama `gpu-diseno`)

Rama `gpu-diseno`, desde `revision` (ba0c2a2e). Encargo de diseño con prototipo: el camino de la GPU para el
escritorio y los fondos animados (la herramienta de fondos va en `fondos`). A mitad, el usuario pidió "vamos a por la
del 3D": el prototipo del camino 3D de verdad. Diseño en `docs/diseño/S38-gpu-escritorio.md`.

## 2026-11-17

### Lo hecho

- **`docs/diseño/S38-gpu-escritorio.md`**: qué hace hoy el compositor y cuánto cuesta, los cuatro caminos, los fondos
  animados baratos, la seguridad y la recomendación (el escritorio por el dominio de gráficos con un servidor de
  composición propio; en QEMU, la `virtio-vga-gl` pasada al dominio en x86).
- **`medir_componer_1080`** (`wayland/src/tests.rs`, `#[ignore]`): la escena de 1920 × 1080 por CPU, seis casos.
- **`docs/diseño/pruebas/fondo-cpu/fondo_cpu.rs`**: plasma, plasma a un cuarto y ciclo de paleta en la CPU, con dos
  pruebas (el ciclo solo toca lo que cicla; el cuarto es el plasma a saltos).
- **`caja-linux/invitado/componer_gl.c`**: el servidor de composición sin protocolo. GLES 2 por EGL y GBM (o
  "surfaceless"), la escena de `medir_componer_1080` con un fondo de sombreador, comprobación de píxeles contra la
  cuenta en la CPU, cuatro modos (fondo, escritorio, subida, lectura). Solo interfaces públicas.
- **`tools/gpu_componer.py`**: `pc` (cada GPU del PC y llvmpipe), `qemu` (la caja aarch64 con
  `virtio-gpu-gl-device` y `egl-headless`; el programa entra por HTTP desde el PC; el disco con `snapshot=on`) y
  `hostil`.

### Lo medido

Las tablas, en S38 (secciones 1, 3 y "Lo medido del prototipo"). En corto, CPU por cuadro a 1920 × 1080 con el fondo
animado entero, cuatro ventanas y sombras:

- Compositor de DashOS por CPU: 3,0 ms en el PC (13,4 en aarch64 por TCG), más 2,2 (8,2) del plasma.
- Servidor con GPU: 0,03 a 0,04 ms en el PC (Radeon integrada, Arc A310); 0,38 a 1,05 ms en la caja aarch64 con virgl (dos vueltas; la segunda, con la máquina cargada).
- Las sombras son más de la mitad del cuadro por CPU. Vectorizar da 1,2 a 2 veces; en ARM no se vectoriza nada (los
  programas van con `aarch64-unknown-none-softfloat`).
- Con virgl, subir una ventana entera cuesta 2 a 3,5 ms y devolver el cuadro unos 7 ms: el cuadro no debe volver a DashOS.

### Pruebas

- `cargo test --release` de `wayland`: bien (148).
- `tools/gpu_componer.py pc`: comprobación BIEN en las dos GPU y en llvmpipe, en los cuatro modos.
- `tools/gpu_componer.py qemu`: BIEN dos veces (virgl en los cuatro modos, llvmpipe en dos).
- `tools/gpu_componer.py hostil`: 12 casos bien. Antes, `--dispositivo /dev/null` daba 0: GBM caía en la CPU sin
  decir nada. Ahora se exige un nodo de DRM y se rechaza un renderizador de CPU cuando se pidió una GPU.
- `fondo_cpu.rs`: 2 pruebas bien.
- `make prueba` y `make arranque-virt` en `kernel/`: bien a la primera.

### Pendiente

- G1 a G8 de S38. Lo siguiente con más riesgo es G5 (el dominio de x86 con la `virtio-vga-gl` pasada), en la rama
  `x86`.
- Decisiones del usuario: las seis de S38.

### Lo que NO se ha comprobado

- Nada en una Pi ni en un PC con DashOS. DashOS no ha compuesto nada en la GPU: el servidor corre fuera de DashOS.
- Venus por rutabaga.
- La memoria de un dominio con Mesa.
