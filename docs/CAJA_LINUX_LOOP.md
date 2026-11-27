# La caja Amiga en Linux: plan y bitácora

Encargo del usuario (2026-11-28, por jefe-dash): la caja Amiga de DashOS como programa de Linux, en x86-64 y AArch64.
Es el "WINE de AmigaOS": ejecutar en Linux programas 68k de otros sin emular el Amiga entero. Las bibliotecas son
las nativas de la caja (exec, dos, intuition...) y el 68k corre en el JIT: Emu68 en ARM, RelevoJIT en x86. En Linux,
siempre little-endian.

No confundir con `caja-linux/`, que es lo contrario: la máquina virtual de Linux dentro de DashOS.

Rama `amiga-en-linux`, worktree `.worktrees/amiga-en-linux`, desde `origin/main` (4d7cea5b). Emu68 en un clon
propio, `third_party/Emu68` del worktree, con los parches de `patches/emu68/`.

## Decisiones

- **Capa de plataforma** (acordado con las sesiones de DashOS): una interfaz en C y un trait de Rust, con dos
  versiones, `dashos` y `linux`. Cubre memoria (crear, proyectar, W^X; en Linux, memfd y dos mmap), hilos, reloj,
  consola, ficheros, ventanas, sonido y fallos (en Linux, `sigaction` de SIGSEGV y `ucontext`, que rellenan el
  `marco_fallo` de `caja/fallo_a64.h`).
- **La página 0 del 68k en Linux** (L1, 2026-11-28). Linux no deja proyectar nada por debajo de `vm.mmap_min_addr`
  (64 KB de serie) sin ser root. La caja de DashOS ya pone la RAM del 68k desde $100000; en los primeros 4 GB solo
  tiene, además, la página 0 (4 KB de solo lectura, ExecBase en la 4). Así que no hace falta que el JIT sume una base
  a cada acceso:
  - **ARM (Emu68):** las direcciones del 68k siguen siendo direcciones del proceso. La página 0 va a una copia de
    solo lectura en $10000 (por encima de `mmap_min_addr`, y en la caja no hay nada entre $1000 y $100000). El JIT
    desvía allí, al traducir, las lecturas con dirección absoluta (`move.l 4.w,a6`): parche
    `0017-pagina0-en-linux.patch`, con `DASHOS_PAGINA0`. Las que llegan por un registro (`move.l (a0),d0` con A0 = 4,
    o un puntero nulo) dan SIGSEGV, y el manejador las imita sobre la copia con `decodifica()` de `caja/fallo_a64.h`.
    Una escritura en la página 0, o un acceso a lo demás de los primeros 64 KB, es un fallo del programa, como en
    DashOS.
  - **x86 (RelevoJIT):** ya suma la base por diseño (`[base + dirección]`, `relevo_config.memory`). En DashOS la caja
    usa base 0 (`RELEVO_OPT_BASE_ZERO`); en Linux, la base es una reserva de 4 GB en cualquier sitio y la página 0
    puede ir en `base + 0`. Sin cambios en el JIT.
  - **Las bibliotecas nativas** pasan todas por `MemoriaCaja::sitio` (`pegamento/src/memoria.rs`): ahí se suma la
    base (x86) y se manda la página 0 a su copia (ARM).
  - **El programa de Linux no puede vivir en los 4 GB bajos** (los usa el 68k): se enlaza por encima, como la caja de
    DashOS, y lo que pida al sistema (montones, pilas, el código traducido) va también arriba, con `MAP_FIXED_NOREPLACE`.

## Tareas

- [x] L1 **El escollo de la página 0** (2026-11-28). Probado en `jit-host` (`make ORDEN=le PAGINA0=1 run`, también en
  big-endian): ver la bitácora.
- [ ] L2 **La capa de plataforma en la caja de DashOS.** Sacar de `caja.c` lo de DashOS (las llamadas al kernel con
  `svc #0`, la memoria, los fallos) a `plataforma.h` + `plataforma_dashos.c`, y lo del crate `dashos` de `pegamento/`
  (archivos, compositor, aviso, preguntador, multitarea) a un trait. Cambio mecánico: `caja-x86`, `arranque-virt` y
  `arranque-virt CAJA_ORDEN=le` tienen que seguir pasando. Antes de tocar `caja.c`, `pegamento/` o `caja/Makefile`,
  avisar a dashos-2f (chips y motor de ARM) y a quien lleve RelevoJIT. No tocar `caja/chips/`, `motor.h`,
  `motor_emu68.c` ni `motor_relevo.c` hasta que dashos-2f cierre K8.
- [ ] L3 **Objetivo de compilación para Linux** (x86-64 y AArch64) con `plataforma_linux.c`, y un programa 68k de
  consola (`dos.library` `Write` a la salida estándar).
- [ ] L4 **Ficheros POSIX**: `dos.library` sobre los ficheros de Linux (volúmenes y asignaciones a directorios).
- [ ] L5 **Ventanas y sonido**: Wayland o SDL; el sonido por lo que haya (PipeWire, ALSA o SDL).

## Bitácora

### 2026-11-28: L1, la página 0

`jit-host` ya es Emu68 como programa de Linux (sin libc, con qemu-user). Con `PAGINA0=1` (directorio
`build-le-p0` o `build-p0`):

- `jit-host/pagina0.c`: el manejador de SIGSEGV (con `rt_sigaction` a mano, el `ucontext` de arm64 desde el byte 176)
  y la copia de la página 0: un memfd de 4 KB, de solo lectura en $10000 y de escritura en $3800000000.
- La prueba P0 de `host.c`: `move.l 4.w,d0`, `move.l $4.l,d1` y `movea.l 4.w,a6` (desviadas al traducir);
  `move.l (a0),d2`, `move.w 2(a0),d3`, `movem.l (a0),d4-d5` (un LDP) y `move.l (a1),d6` con A1 = 0 (por el
  manejador: 4 fallos). Todo bien, en little-endian y en big-endian. Con `PRUEBA=-w`, `move.l d0,4.w` acaba con
  "escritura en la página 0" y el código 3.
- Medida en qemu-user (en un PC x86, así que solo vale para comparar): 3 ns por vuelta con la lectura absoluta, 2,1 µs
  con la que falla. Las lecturas de la página 0 por registro son raras (punteros nulos, sobre todo), así que el fallo
  basta. Falta medirlo en un ARM de verdad.
- RelevoJIT: `make test` y `cargo test` de `~/Projects/RelevoJIT` (main, 42fe7e6) pasan; sus pruebas dan la memoria
  con `malloc`, en una base cualquiera, y comparan el JIT con el intérprete.
- `make comparar` (sin `PAGINA0`) sigue igual: el parche 0017 no cambia nada sin `DASHOS_PAGINA0`.

Acuerdos para L2 (2026-11-28):

- dashos-2f no toca ahora `caja.c` ni `pegamento/`. K7 (rama chips-k7) cambia `motor_emu68.c` y `fallo_a64.h`, y
  quizá una línea de `caja/Makefile`. K8 tocará `motor.h`, `motor_emu68.c`, `motor_relevo.c`, quizá
  `pegamento/src/chips.rs` y la función `chips()` de `caja.c` con su prueba 4b. **`pegamento/src/chips.rs` no se toca.**
  Al unir K7 parte `fallo_a64.h`: `caja/fallo_a64_decodifica.h` tendrá `decodifica()`, `struct acceso` y
  `marco_fallo`, sin los chips. Avisará entonces y al cerrar K8.
- dashos-ec tiene en su rama pi5-instalador (sin subir) `medir_u7` en `caja.c`, que escribe la línea "medida U7" (la
  usa la imagen de diagnóstico de la Pi 5). Al unir con la capa de plataforma, esa línea tiene que seguir saliendo;
  dashos-ec se ofrece a resolver el choque.
- RelevoJIT y la caja de x86: los llevaba dashos-x86, que no está abierta. Preguntado a jefe-dash.

Pendiente de L1: `FALLO_INC` apunta a `caja/`, pero `caja/fallo_a64.h` aún no está en main (llega con union-chips,
de dashos-2f). Hasta entonces, `make PAGINA0=1 FALLO_INC=<dir con fallo_a64.h y un motor.h>`.
