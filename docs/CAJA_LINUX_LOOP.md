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
    `0018-pagina0-en-linux.patch`, con `DASHOS_PAGINA0`. Las que llegan por un registro (`move.l (a0),d0` con A0 = 4,
    o un puntero nulo) dan SIGSEGV, y el manejador las imita sobre la copia con `decodifica()` de `caja/fallo_a64.h`.
    Una escritura en la página 0, o un acceso a lo demás de los primeros 64 KB, es un fallo del programa, como en
    DashOS.
  - **x86 (RelevoJIT):** sabe sumar una base (`[base + dirección]`, `relevo_config.memory`), pero no hace falta: en
    Linux se queda como en DashOS, con base 0 (`RELEVO_OPT_BASE_ZERO`) y las direcciones del 68k como direcciones del
    proceso. La página 0 ya es una página "del anfitrión" en `motor_relevo.c` (cada acceso llama a `pagina_leer`):
    en Linux, `pagina_leer` lee la copia de $10000 en vez de la dirección 0. Es una línea en `motor_relevo.c`, para
    después de K8. Sin cambios en el JIT.
  - **Las bibliotecas nativas** pasan todas por `MemoriaCaja::sitio` (`pegamento/src/memoria.rs`), que ya lee y
    escribe la página 0 por su proyección de escribir (por encima de 4 GB). No cambian.
  - Así, en las dos arquitecturas, el C de la caja sigue usando las direcciones del 68k como punteros.
  - **El programa de Linux no puede vivir en los 4 GB bajos** (los usa el 68k): se enlaza por encima, como la caja de
    DashOS, y lo que pida al sistema (montones, pilas, el código traducido) va también arriba, con `MAP_FIXED_NOREPLACE`.

## Tareas

- [x] L1 **El escollo de la página 0** (2026-11-28). Probado en `jit-host` (`make ORDEN=le PAGINA0=1 run`, también en
  big-endian): ver la bitácora.
- [x] L2 **La capa de plataforma en la caja de DashOS** (2026-11-28). L2a, el C de `caja.c` (`plataforma.h`,
  `plataforma_dashos.c`); L2b, el trait `Plataforma` del pegamento y la opción `dashos`; L2c, los motores (tras K8).
  Solo `plataforma_dashos.c` llama al kernel de DashOS.
- [x] L3 **La caja como programa de Linux** (2026-11-28): `make -C caja linux` (AArch64) y `make -C caja linux-x86`;
  `caja-amiga [-v] programa [argumentos]`.
- [x] L4 **Ficheros de Linux** (2026-11-28): `Linux:` (la raíz, de arranque; el programa empieza en el directorio
  actual), `Caja:` y `SYS:` (el cajón de la caja). Pruebas: `caja/pruebas_linux/prueba.sh`.
- [~] L5 **Ventanas y sonido.** L5a, las ventanas por Wayland (2026-11-28), hecho; el teclado, comprobado (con
  ydotool); el ratón, no. El sonido, en espera: la caja de DashOS tampoco saca el de los programas 68k todavía (Paula da
  muestras y `audio.device` es un programa aparte, sin enganchar; S19 fase 2). Cuando lo tenga, en Linux irá por lo
  que haya (PipeWire o PulseAudio por su socket, sin libc).
- [~] L6 **Lo que falta para usarla de verdad.** Hecho: `Input()` desde la entrada estándar, Ctrl-C (SIGINT), las
  rutas de Linux en los argumentos, `$CAJA_AMIGAOS` y los segmentos a 64 KB (para Asahi; sin probar allí). Falta: medir
  en un ARM de verdad y probar en un Linux de AArch64 de verdad; el ratón, sin comprobar.

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
- RelevoJIT y la caja de x86: nadie los lleva (dashos-x86 está cerrada). jefe-dash da vía libre con condiciones:
  `caja-x86`, `arranque-virt` y `arranque-virt CAJA_ORDEN=le` siguen pasando y `motor_relevo.c` no se toca hasta K8.
  L2 empieza cuando union-chips (K6, que cambia `caja.c`) llegue a main; avisa jefe-dash.

Pendiente de L1: `FALLO_INC` apunta a `caja/`, pero `caja/fallo_a64.h` aún no está en main (llega con union-chips,
de dashos-2f). Hasta entonces, `make PAGINA0=1 FALLO_INC=<dir con fallo_a64.h y un motor.h>`.

### 2026-11-28: L2a, la capa de plataforma en C

Rama traída a `origin/main` con union-chips (786eb010). `jit-host` con `PAGINA0=1` compila ya con el
`caja/fallo_a64.h` de main y P0 sigue bien.

- `caja/plataforma.h`: la interfaz (consola, reloj, dormir, memoria, hilos, la página 0 y `plat_hay_memoria` para la
  comprobación de S28). `caja/plataforma_dashos.c`: `caja_sc` y todo lo que llamaba al kernel desde `caja.c`, sin
  cambiar el orden de las llamadas. `caja.c` ya no tiene ninguna llamada al kernel; `motor_emu68.c` y
  `motor_relevo.c` siguen con `caja_sc` (declarada en `caja.h`) hasta K8.
- Compila en ARM (be y le) y en x86. Pruebas: `caja-x86` BIEN; `arranque-virt CAJA_ORDEN=le` BIEN entero (155
  "bien" de la caja, el sonido sin chasquidos); `arranque-virt` (be) BIEN salvo el sonido (89 chasquidos, con el PC
  a carga 4-6, como en REVISION_LOOP del 12/11); la caja, igual: 155 "bien", ningún "MAL".
- Ojo para la próxima vez: no editar el árbol mientras corre una prueba que compila desde él. La primera referencia
  de `arranque-virt` sobre main se mezcló con el cambio a medias y no vale.
- dashos-2f ha separado el decodificador en `caja/fallo_a64_decodifica.h` (rama chips-blitter, d2383bb9, sin subir).
  Cuando llegue a main, `jit-host/pagina0.c` lo incluye a él y deja de definir los `caja_chip_*` de mentira. K7 toca
  también `caja.c` (la medida tras chips.exe), `programas.S` y `caja/Makefile` (`trampolin_a64.o`): habrá choque
  pequeño con L2a al unir. En Linux, el marco de la señal guarda y repone los registros q, así que el manejador de
  SIGSEGV puede usar la FPU.

### 2026-11-28: L2b, L2c, L3, L4 y L5a

Rama traída a `origin/main` con la fase 1 de los chips (ddc95f84, PR #17). El parche de la página 0 pasa a
`0018-pagina0-en-linux.patch` (el 0017 es el plazo de los chips); los 18 se aplican sobre el clon. `jit-host/pagina0.c`
usa `caja/fallo_a64_decodifica.h` y su regreso de la señal va en ensamblador suelto (GCC ignora `naked` en AArch64).

- **L2b.** `pegamento/src/plataforma.rs`: el trait `Plataforma` (reloj, hora, dormir, consola, salida del programa,
  final), con `Dashos` (el crate de siempre) y `Linux` (las `plat_*` de C). La opción `dashos` (por defecto) trae el
  crate `dashos` y sus servicios; sin ella, los módulos de `src/linux/`.
- **L2c.** `motor_emu68.c` pide a la plataforma el código traducido (`plat_codigo`, `plat_codigo_wx`) y el manejador
  de fallos (`plat_fallos`); la entrada en ensamblador de los fallos de DashOS pasa a `plataforma_dashos.c`.
  `motor_relevo.c`: `plat_dormir`, y `pagina_leer` lee la página 0 por `PAGINA0_RW` (ahora en `caja.h`).
- **L3.** `plataforma_linux.c` (mmap con `MAP_FIXED_NOREPLACE`, memfd, clone y futex, SIGSEGV con el `marco_fallo`
  desde el `ucontext` y el ESR de `__reserved`), `caja_linux.c` (incluye `caja.c` tal cual; sus pruebas del arranque
  las quita `--gc-sections`), `entrada_linux.S`, `programas_linux.S`. Lo que escribe el programa, a la salida estándar;
  lo que dice la caja, a la de errores con `-v`.
- **L4.** `pegamento/src/linux/archivos.rs` y `sys.rs`: `SistemaArchivos` con descriptores (openat, pread, pwrite,
  getdents64, statx...). No es una jaula: los enlaces simbólicos llevan adonde apunten, como en cualquier programa del
  usuario. Linux no guarda los bits ni los comentarios del Amiga.
- **L5a.** El compositor de la caja ya hablaba Wayland estándar: su transporte pasa a `transporte.rs` (DashOS, el
  canal de siempre) y `linux/transporte.rs` (el socket, con SCM_RIGHTS, y los búferes en memfd).

Comprobado: `prueba.sh` 12 BIEN (saludo.exe y cajones.exe, en x86-64 nativo y en AArch64 con qemu-aarch64);
texto.exe, interp.exe y listas.exe de Dash dan su salida; ventana.exe abre y dibuja su ventana en niri, en las dos
cajas. Las cajas de DashOS (be, le y x86) compilan en cada paso. `arranque-virt` (be) BIEN con L3; la tanda entera
con L5a, abajo. No comprobado: ratón y teclado en las ventanas de Linux; el coste del fallo de la página 0 en un ARM de
verdad; la caja de AArch64 en un Linux de verdad (solo qemu-user).

Para las capturas de ventanas sin tocar el portapapeles: niri no da la posición de las ventanas en mosaico, así que se
pone flotante la de la caja (`niri msg action move-window-to-floating --id N` y `move-floating-window -x 200 -y 200`) y
`grim -g` captura solo su rectángulo.

### 2026-11-28: L6, y programas de AmigaOS 3.9

- **L6a.** `Read(Input())` lee la entrada estándar (de UTF-8 a Latin-1); Ctrl-C llega como `SIGBREAKF_CTRL_C` (al
  tercero sin atender, la caja acaba con 130). **L6b.** Un argumento que es una ruta absoluta de Linux y existe pasa a
  `Linux:...`, entre comillas si lleva espacios; lo que el programa escribe y ya es UTF-8 sale tal cual. **L6c.**
  `$CAJA_AMIGAOS=<carpeta>` monta un AmigaOS del usuario como en DashOS (`montar_amigaos_en`). Los segmentos de la caja
  de AArch64 van a 64 KB y el memfd de la página 0 mide 64 KB (Asahi: sin probar).
- `prueba.sh`: 24 BIEN, 0 MAL, en las dos cajas. A mano: `teclas.exe` en niri con ydotool (con `wtype` no vale: sube
  su propio mapa de teclado, y la caja traduce los códigos estándar sin mirarlo, como en DashOS). Antes de teclear se
  mira que el foco sea de la caja.
- **Programas de otros: los comandos de `C:` de un AmigaOS 3.9** (el del usuario, que ya usa la demo de DashOS),
  con `CAJA_AMIGAOS` o un `SYS:` con enlaces a sus Libs. Van: Version (carga `version.library` de disco), Avail, Type,
  Assign, Info, MakeDir, Copy y Protect. No van, por huecos de la caja que son también de DashOS (no de Linux):
  - `dos.library`: `MatchFirst("")` da 205 (en AmigaOS es el cajón actual) y `MatchNext` no está: Dir, List, Search,
    Delete y Rename (los de patrones). `DateToStr` no está: Date.
  - `exec.library`: `Supervisor` y `CacheControl` no están: Cpu (en x86 cae después en la dirección $F; en ARM acaba).
  - `icon.library`: la rechaza ramlib (su `Init` llama a lo privado de exec; hace falta una nativa): Calculator.
  - Status y Which acaban con 20 (sin mirar).
- `arranque-virt` (be) con todo hasta L5a: la caja, 158 "bien" y ningún "MAL"; solo faltó el intermitente de
  "[ram] Ana: avisos hostiles" (el del servicio de archivos, de `X86_LOOP.md`), que no es de la caja.

### 2026-11-28: los huecos de dos y exec, e icon.library (pedido por jefe-dash: nadie lleva dos ni exec)

Todo comprobado contra el oráculo (FS-UAE con el Kickstart 40.68, `tools/emu_run.py` de Dash): pruebas nuevas en
`pruebas68k/` (`dosasl.c`, `patron.c`, `formato.c`, `supervisor.c`, `icono.c`), cuya salida guarda
`pruebas_linux/*.esperado` y compara `prueba.sh` (36 BIEN en las dos cajas). La caja de Linux es la forma más rápida
de probar las bibliotecas: segundos, frente a los 15 minutos de `arranque-virt`.

- dos: `MatchFirst("")` (el cajón actual, con el nombre vacío) y `APF_DODIR`/`APF_DIDDIR` (entrar en los cajones:
  dentro sale todo, case o no); `DateToStr` (los cuatro formatos y `DTF_SUBST`); `ParsePattern` con el patrón
  analizado de AmigaOS (las marcas `P_*`: Search lo arma a mano y Rename usa el búfer como nombre); `GetDeviceProc`
  mínimo (Delete). exec y dos: `%x` en mayúsculas en `RawDoFmt` y `VPrintf`.
- exec: `Supervisor` (la función en modo supervisor hasta su `RTE`: `caja_ejecutar_68k` con el modo 2, un marco de
  formato 0) y `CacheControl` (un CACR de mentira).
- icon.library nativa V40 (`pegamento/src/icono.rs`). La guardia de ficheros (S28) pregunta en Linux por `/dev/tty`;
  sin terminal, niega; `CAJA_AMIGA_PROTEGER=no` la quita.
- La caja de Linux, en hora local: `/etc/localtime` (TZif y su regla POSIX), sin libc; igual que `localtime_r`.
- De AmigaOS 3.9 van ya Dir, List, Date, Search, Rename, Delete ALL, Cpu ("68040 68882") y la Calculator. Falta: los
  signos × y ÷ en la letra topaz de la caja (salen como cuadrados).

### 2026-11-28: cae el programa, no la caja; y Supervisor()

Pedido por jefe-dash, en este orden; la decisión, también en `docs/diseño/S28-malware-amiga.md`.

1. **Cae el programa, no la caja.** Con `Supervisor()` dado, `residente.c` (S28) tumbaba la caja entera en
   `arranque-virt` y `caja-x86`: su código vuelve con RTS y el 68k salta fuera de su memoria. Ahora un programa que sale
   de su memoria cae solo él: en ARM, el parche 0019 de Emu68 (no traducir un PC fuera de la memoria del 68k: la caja lo
   lleva a `PARADA68K` y lo corta) y el manejador de fallos siempre puesto (los accesos del código traducido: la carga da
   0, el guardado no se hace, se corta); en x86, RelevoJIT ya daba un error de bus, y `pagina_leer` y `pagina_escribir`
   ya no salen de la página 0. Prueba hostil `pruebas68k/perdido.c` (salta, lee, escribe, pila, cero) en la prueba 9 de
   `caja.c` y en `prueba.sh`. No toca el kernel.
2. **Supervisor().** Dado. El corte entra por el vector 31, que `residente.c` cambia: antes de cortar, la caja vuelve a
   poner sus vectores y su VBR (ARM). `residente.c` espera ahora que su código de Supervisor corra una vez, en
   supervisor dentro de la caja (no es un logro), y que caiga el programa. Cpu de AmigaOS 3.9 va en las dos cajas.

Lo que se vio por el camino: el `DateToStr`, el `ParsePattern` y lo demás de dos y exec pasaron `arranque-virt` BE con la
caja entera bien (158 "bien"); falló solo una línea del escritorio ("[sesiones] barra lanza Programas/guarda"), con otra
sesión corriendo QEMU a la vez.

Pendiente con dashos-2f (2026-11-28): `third_party/Emu68` pasa a ser un submódulo (rama `dashos` de la bifurcación, con
0016-0018). Cuando llegue a main: en el clon del submódulo, encima de esa rama, un commit "DashOS 0019-pc-fuera-de-memoria"
con el contenido del parche; en esta rama, el puntero del submódulo a ese commit y fuera `patches/emu68/0019`. Subir el
commit a la bifurcación, con el sí del usuario (por jefe-dash). El worktree tiene `third_party/Emu68` como clon suelto:
al traer main hay que cambiarlo (AGENTS.md lo explica).
