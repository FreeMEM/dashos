# Bitácora: caja Amiga (H4, primer paso)

Rama `caja-amiga`, sacada de `revision` (da0adf0). Hito H4 de `docs/diseño/S15-hoja-de-ruta.md`; diseño en
`docs/diseño/S4-caja-amiga.md` (§1, la línea A) y `docs/diseño/S3-memoria.md`.

## 2026-11-12: un programa 68k escribe en la consola de DashOS

### Qué funciona, de punta a punta (`make -C kernel arranque-virt`)

`inicio` lanza `caja-amiga`, un proceso de usuario big-endian con el JIT de Emu68 dentro. La caja ejecuta cuatro
programas 68k seguidos (`caja/pruebas68k/`):

1. `hola`: suma de 1 a 100.000 y la escribe por una dos.library nativa: `PutStr`, `VPrintf`, `Output` y `Write`,
   con sus LVO de verdad (-948, -954, -60, -48). Cada entrada de la tabla de saltos es `$A0FF <número> RTS`, y el JIT
   la convierte en una llamada a `dashos_native_call` (parche 0002, que ya existía y no hubo que tocar). La caja
   comprueba la suma que deja el 68k en D0.
2. `lineaa` (hostil): `$A0FF 999`, una función que no existe: la caja la rechaza (D0 = -1) y el programa sigue.
   Después `$A123`, una línea A que no es una llamada: excepción 10 del 68k; el programa acaba y la caja sigue.
3. `bucle` (hostil): `bra.s` sin fin. Un hilo vigilante de la caja (`hilo_crear`) duerme 300 ms y levanta la
   interrupción de nivel 7 del 68k (`INTF.ARM_err`); el bucle traducido mira `INT64` en cada vuelta, vuelve a
   `MainLoop`, que salta al vector 31, y su trampa acaba el programa. Mientras, el resto del sistema sigue (en la
   salida se ven 30 líneas de otros servicios entre medias).
4. `mala` (hostil): escribe en `$DEAD0000`. El kernel termina la caja (clase 0x24) y nada más.

Salida (QEMU virt):

```
│   [caja-amiga] JIT de Emu68 en un proceso de usuario big-endian (H4, primer paso)
│   [caja-amiga] hola desde un programa 68k, por la dos.library nativa
│   [caja-amiga] suma de 1 a 100000 en el 68k: 705082704
│   [caja-amiga] Write() en Output(): bien
│   [caja-amiga] el 68k dio 705082704 (esperado 705082704) con 4 llamadas nativas, 11 unidades traducidas, en 5 ms: bien
│   [caja-amiga] función nativa 999: no existe; rechazada (D0 = -1)
│   [caja-amiga] funcion 999 rechazada; el programa 68k sigue
│   [caja-amiga] línea A desconocida: excepción 10 del 68k, 1 rechazada, 1 llamada buena: bien
│   [caja-amiga] bucle sin fin: cortado por el vigilante a los 300 ms tras 17978404 vueltas (vector 31): bien
│   [caja-amiga] el 68k escribe en $DEAD0000, fuera de su memoria: la caja tiene que morir
Proceso: excepción de clase 0x24 en 0x1100003a0c (FAR 0xdead0000); se termina
│   [inicio] caja-amiga acabó: clase 2, código 0
```

Medidas (QEMU TCG, no valen para la Pi): la suma (300.000 instrucciones 68k, 11 bloques traducidos) en 5 ms; el
bucle, unos 18 millones de vueltas en 300 ms. El ejecutable mide 1,1 MB (406 KB con LZ4 dentro del kernel).

### Lo que se hizo

- **Kernel.** `memoria_proyectar` acepta el permiso 4 (ejecutar): lectura y ejecución, nunca escritura. 2 y 4 juntos
  dan `E_ARGUMENTO`; la memoria de DMA no se ejecuta. El mismo objeto se proyecta para escribir en una dirección y para
  ejecutar en otra (S3). Al proyectar para ejecutar, `ic ialluis` (lo que quedara de otro uso de esas páginas).
  `SCTLR_EL1.UCT` y `UCI` (en `mapa::wxn`, en cada núcleo): EL0 lee `CTR_EL0` y hace `dc cvau`/`dc civac`/`ic ivau`,
  como en Linux; el JIT de Emu68 los usa. Documentado en `docs/referencia/kernel.md`.
- **Prueba hostil del kernel** (`malo`, paso 37, en `make prueba`): 6, 7 y 0xff se rechazan; escribir por una
  dirección y ejecutar por otra funciona, también tras reescribir el código; se puede ejecutar con una llave de solo
  leer; el kernel no escribe por el proceso en la dirección de ejecutar.
- **`caja/`** (C, MPL, ficheros propios): `caja.c` (el anfitrión: memoria, montones TLSF, la dos.library nativa, los
  vectores y el vigilante), `entrada.S`, `enlace.ld`, `programas.S` (los .bin dentro) y `Makefile`, con las mismas
  opciones de Emu68 que `jit-host/`. Usa `jit-host/emu68_context.c` tal cual. Sin libc ni libgcc.
- **Arranque.** `make -C kernel usuario` compila la caja si está `third_party/Emu68`; `kernel/build.rs` la toma de
  `caja/build/`. En `inicio`, la última del todo (solo virt), sin llaves (así `inicio` la espera) y con 32 MB de cupo.

### Mapa de la caja

| Dirección | Qué |
|---|---|
| `$0010_0000` – `$001F_FFFF` | Memoria del 68k (1 MB): vectores (VBR), trampas, la tabla de la dos.library, los programas, la pila |
| `$7000_0000` | El ejecutable (ET_EXEC) |
| 4 GB | El código traducido, para escribir (4 MB) |
| 4 GB + 64 GB | El mismo, para ejecutar (bit 36: lo pone Emu68) |
| 8 GB | Montón general de Emu68 (4 MB) |
| 12 GB | Pilas nativas (la del JIT, 1 MB, y la del vigilante); `entrada.S` cambia de pila antes de nada |

### Lo que falta

- exec nativo (tareas, señales, puertos, memoria con dueño), `OpenLibrary`, y dos.library sobre el sistema de
  archivos (S4, S4b). Hoy A6 = DOSBase al empezar, y la dirección 4 no existe (el kernel nunca proyecta los primeros
  64 KB: para ExecBase en 4 hará falta una excepción para la caja).
- Cargar ejecutables hunk (hoy, binarios planos con su dirección fija, de `jit-host/pruebas68k/hunk2bin.py`).
- El oráculo: el mismo binario en FS-UAE con el Kickstart real (la salida de `hola` es de RawDoFmt, `%ld`).
- Varias tareas 68k: hoy un solo estado (`__m68k_state`) y una caché de traducciones global.
- La caja en la imagen de la Pi (`make arranque`): hoy solo en virt.
- `llamar_68k` (de lo nativo al 68k), `SetFunction`.

### Decisiones provisionales (para el usuario)

1. **El ejecutable de la caja va por debajo de 4 GB** (en `$7000_0000`), contra "ningún código nativo de la caja va
   aquí" del mapa de `kernel.md`. Emu68 guarda en el código traducido las direcciones de sus funciones de C con 32
   bits (`EMIT_LoadImmediate` con un `uint32_t`, en `M68k_Exception.c` y `M68k_LINEF.c`). Llevarlo arriba pide un
   parche de Emu68 que cargue 64 bits. Mientras, el 68k puede escribir en los datos de la caja (su `.data` y `.bss`,
   2 MB, con el estado del 68k y la tabla de la caché del JIT): no sale de la caja, pero puede romperla. Las pilas,
   el montón y el código traducido ya están por encima de 4 GB.
2. **Número de función de 16 bits**, como dejó el parche 0002 (`$A0FF <n> RTS`, 6 bytes, la entrada entera), no de 32
   como dice S4 §1. Con 32 bits no cabe el `RTS` y el JIT tendría que emitir el retorno.
3. **Una sola caja para las cuatro pruebas**; la última la mata. Separarlas pide tres ejecutables (406 KB cada uno en
   el kernel) o pasarle un modo por un canal (como `trampas`).
4. **El vigilante usa la NMI del 68k** (nivel 7, `ARM_err`) para cortar un programa: no hace falta tocar Emu68. Para
   exec nativo será el mismo camino (las interrupciones de la caja).
5. **EL0 puede limpiar cachés por dirección** (UCI) en todos los procesos, no solo en la caja. Es lo de Linux; el
   riesgo conocido es usarlo para medir la caché (canales laterales), que ya se puede con lecturas temporizadas.

### Pruebas

- `make -C kernel prueba`: "BIEN: esperado.txt", con la línea nueva de `malo`.
- `make -C kernel arranque-virt`: las cinco BIEN, con las líneas de la caja en `pruebas/arranque_virt.txt`. De
  cuatro vueltas, dos fallaron en líneas del guion de Ajustes que se teclean ("el compositor pide 800x560", "el campo
  dice: DashOSa"), con el PC cargado (carga media 10-23, otras sesiones con QEMU; el bucle de la caja dio 1,6 millones
  de vueltas en vez de 18). La caja acaba unos 8 s antes de que salga Ajustes; las otras dos vueltas, todo BIEN.
- `make -C jit-host run`: las dos BIEN de siempre.

## 2026-11-12: ejecutables hunk, la página 0 con ExecBase y un programa de m68k-amigaos-gcc

Rama `caja-amiga-hunk`, desde `caja-amiga` (269806a).

### Qué funciona (`make -C kernel arranque-virt`)

Tras los tres programas de antes, la caja:

1. **Proyecta la página 0**: un objeto de una página, escrito por una dirección por encima de 4 GB y proyectado de
   solo lectura en la dirección 0 con la llamada 49 (45 antes de juntarla con la caja Linux) y el permiso de página 0 (su llave 1, que le da `inicio`). En la
   dirección 4, ExecBase. La caja comprueba que la lee, que pedirla otra vez falla y que en `$1000` o en 0 no puede
   proyectar nada por la vía normal.
2. **Carga y ejecuta `saludo.exe`**, un ejecutable hunk de verdad hecho con `m68k-amigaos-gcc` (la cadena de Dash,
   `/opt/amiga`): tres hunks (código, datos y BSS), 21 reubicaciones entre ellos, símbolos y depuración (`-g`). El
   programa toma ExecBase de la dirección 4, abre la dos.library con `OpenLibrary` (la exec mínima y provisional),
   hace una criba en el BSS, suma una tabla del hunk de datos y escribe con `PutStr`, `VPrintf` y `Write(Output())`.
   Entra como desde el CLI: A0 los argumentos, D0 su largo, A6 = ExecBase. La caja comprueba D0 = 0, las 7
   llamadas nativas y que la dos.library quedó cerrada.
3. **Hostil del cargador, en la caja**: el mismo fichero cortado en cada byte (9216 cortes) y estropeado al azar
   (20.000 mutaciones de palabras): ninguno rompe la caja y ninguno pierde memoria.
4. **Hostil de la página 0**: `mala` ahora escribe en la dirección 4. El kernel termina la caja (FAR 0x4).

Salida (QEMU virt; las líneas de antes, iguales):

```
│   [caja-amiga] página 0: ExecBase en la dirección 4 ($106000), solo de lectura; otra vez o en los primeros 64 KB, rechazada: bien
...
│   [caja-amiga] saludo.exe: 3 hunks (código 612, datos 32, BSS 1008 bytes) en $150008, 21 reubicaciones
│   [caja-amiga] hunk: hola desde un ejecutable de AmigaOS (m68k-amigaos-gcc)
│   [caja-amiga] hunk: 168 primos por debajo de 1000, el ultimo 997 (tres hunks)
│   [caja-amiga] hunk: tabla 31415926, dos.library abierta (v36 o mas: 1)
│   [caja-amiga] hunk: Write() en Output(): bien
│   [caja-amiga] saludo.exe acabó con 0 (vector -1), 7 llamadas nativas, en 22 ms: bien
│   [caja-amiga] hunk hostil: 9216 cortes y 20000 mutaciones de saludo.exe: 10590 con error, 18626 cargan, 0 pierden memoria: bien
│   [caja-amiga] el 68k escribe en la dirección 4 (ExecBase), en la página 0 de solo lectura: la caja tiene que morir
Proceso: excepción de clase 0x24 en 0x110000598c (FAR 0x4); se termina
│   [inicio] caja-amiga acabó: clase 2, código 0
```

Las mutaciones que cargan son las que tocan bytes de código, datos o depuración: el fichero sigue bien formado.

### Lo que se hizo

- **Cargador hunk** (`caja/hunk.c`, `hunk.h`): escrito de la documentación pública del formato (The AmigaDOS Manual,
  ficheros de carga; `dos/doshunks.h` del NDK). HUNK_HEADER (sin bibliotecas residentes; primer hunk 0 y último
  `cuantos - 1`: sin overlays), CODE, DATA, BSS, RELOC32, RELOC32SHORT (y DREL32, que en un fichero de carga es lo
  mismo), SYMBOL, DEBUG, NAME, END y los bloques desconocidos con la marca de "se puede saltar" (bit 29). Las
  banderas CHIP y FAST (y la palabra extra de atributos) se leen y se pasan a quien reserva, que las ignora. Cada
  hunk va con sus 8 bytes delante (tamaño y BPTR al siguiente): la lista de segmentos de LoadSeg. Sin libc ni
  llamadas al sistema: la reserva y la vista de la memoria del 68k las da quien carga (`struct hunk_ops`), así que
  el mismo código se prueba en el PC y en la caja. Errores (todos devuelven lo reservado): fichero corto, no es
  hunk, bibliotecas residentes, número de hunks 0 o mayor que la tabla (256 en la caja), overlay, tamaños que no
  cuadran (datos más largos que el hunk, hunks de 4 GB), sin memoria, reubicación a un hunk que no existe o fuera del
  hunk, bloques fuera de orden, hunks de más o de menos, bloque desconocido.
  Dos tolerancias, por lo que sacan los enlazadores: un HUNK_END sin contenido delante se ignora (el `ld` de
  `m68k-amigaos-gcc` con `-g` pone tras el último hunk un bloque de depuración y símbolos acabado en HUNK_END), y el
  último hunk sin HUNK_END al final del fichero cuenta como acabado.
- **Pruebas en el PC** (`caja/pruebas_hunk/`, con ASan y UBSan; las lanza `make -C kernel prueba`): `saludo.exe`
  (hunks, entrada, reubicaciones a datos y BSS, lista de segmentos, BSS a 0), un fichero a mano con lo que gcc no
  saca (RELOC32SHORT con su relleno, DREL32, NAME, un bloque que se salta, atributos), 39 ficheros hostiles cada uno
  con su error, los 9216 cortes y 200.000 mutaciones al azar.
- **Kernel: la página 0** (S3). Un objeto nuevo, el permiso de página 0 (`Objeto::PermisoPaginaCero`): solo lo crea
  el kernel, va entre las llaves de arranque de `inicio` (la última, `pagina0`, tipo 8: no cambia el número de las
  demás) e `inicio` se lo da solo a la caja. La llamada 49 (`pagina_cero`; era la 45 antes de juntarla con la caja Linux) proyecta con él un objeto de memoria de una
  página en la dirección 0, de solo lectura y sin ejecutar (`Espacio::proyectar_pagina_cero`); el resto de los
  primeros 64 KB sigue cerrado, y `para_copiar` sigue sin dejar al kernel copiar desde ni hacia la página 0. Una llave
  revocable de memoria no vale (la página 0 no se apunta en intermediarios). `inicio` espera a la caja aunque ahora
  lleve una llave (`SE_ESPERAN`). Documentado en `docs/referencia/kernel.md`.
- **Pruebas del kernel** (`make prueba`): `prueba_pagina_cero` (H4): un objeto de dos páginas no vale, la vía normal
  sigue cerrada en los 16 primeros marcos, la página queda de solo lectura y sin ejecutar para EL0, otra vez no, el
  resto de los 64 KB vacío, `para_copiar(4)` falla y se puede quitar. `malo`, paso 38: sin el permiso, la llamada 49
  da `E_LLAVE` o `E_TIPO` (con una llave que no existe, con una de memoria o con un canal en su lugar), y por
  `memoria_proyectar` ni 0, ni `$1000`, ni `$8000`, ni `$F000`, tampoco de solo lectura ni para ejecutar.
- **exec mínima y provisional** (`caja/exec_minima.c`): ExecBase en `$106000` (cabecera de biblioteca, versión 40,
  AttnFlags de 68010 a 68040 y FPU), `OpenLibrary` (solo "dos.library" hasta la versión 40) y `CloseLibrary`. La
  dos.library tiene ahora su cabecera (versión 40). Números de función nativa de exec desde `$100` (`N_EXEC`).
- **`caja/caja.h`**: el mapa de la memoria del 68k y el acceso a ella, compartidos.
- **Un fallo que salió al partir `ejecuta`**: con la ejecución en una función aparte (`arranca`), gcc dedujo (análisis
  entre funciones) que `arranca` no cambia `llamadas_nativas`, que es `static`, y no la volvía a leer: la cuenta de
  `lineaa` salía 0. El asm de `arranca` llama a `MainLoop`, y el código traducido a `dashos_native_call`, que sí la
  cambia; el clobber "memory" no bastó. Los contadores son ahora `volatile` y `arranca` lleva `noipa`.
- **Parches 0012 y 0013 de Emu68** (de la rama `caja-amiga-u7`, por encargo): dos fallos del giro de bytes (en
  big-endian no cambian nada) y el indirecto en memoria con base An/PC y desplazamiento negativo, que sumaba en 64
  bits y salía de los 4 GB (falla también en big-endian). Aplicados al clon y la caja recompilada.

### Mapa de la caja (lo nuevo)

| Dirección | Qué |
|---|---|
| `$0000_0000` – `$0000_0FFF` | La página 0, de solo lectura: ExecBase en 4 |
| `$0000_1000` – `$0000_FFFF` | Nada: el kernel no lo proyecta nunca |
| `$0010_6000` | ExecBase (exec mínima); su tabla de saltos, por debajo |
| `$0015_0000` – `$001D_FFFF` | Los hunks de los ejecutables (reparto provisional, sin AllocMem) |
| 14 GB (`$3_8000_0000`) | La página 0, para escribirla |

### El oráculo

Pendiente. Tres intentos (5, 25 y 40 minutos de espera) se quedaron esperando el cerrojo del emulador
(`/run/user/1000/dash-emu-red.lock`), que tenían otras sesiones con tandas largas. La orden, para cuando esté libre
(salida esperada: las cuatro líneas "hunk:" de la caja, iguales):

```
cp caja/pruebas68k/saludo.exe /tmp/saludo
cd ~/Projects/FreeMEMLang && flock /run/user/1000/dash-emu-red.lock \
    env DASH_KICKSTART="$HOME/Descargas/Telegram Desktop/kick40068.A1200" python3 tools/emu_run.py /tmp/saludo
```

### Lo que falta y decisiones pendientes

1. **El Enforcer.** Hoy cualquier acceso del 68k a la página 0 que no sea leer, o a `$1000`-`$FFFF`, mata la caja
   (el kernel termina el proceso, clase 0x24). Para avisar y seguir hace falta que el kernel entregue los fallos de
   un hilo a su proceso en vez de matarlo: un canal de excepciones por proceso (como los de Zircon) donde el kernel
   deje el hilo parado y mande ELR, FAR y ESR; un hilo de la caja decide (saltar la instrucción del 68k, dar un
   valor, o matar el programa) y el kernel lo reanuda con los registros cambiados. Es un cambio del kernel con su
   propio diseño (quién puede pedirlo, qué registros se pueden tocar, qué pasa si el hilo que decide también falla),
   no un parche: se deja para un paso aparte. Otra vía, sin kernel: que Emu68 compruebe las direcciones bajas en cada
   acceso; cuesta en cada instrucción de memoria.
2. **Sin PAN en la A72**, un puntero nulo del kernel mientras corre la caja lee la página 0 en vez de fallar (no
   puede escribirla). Solo la caja la tiene. Decisión del usuario si basta.
3. **La exec de verdad** (crate `caja/exec` de `caja-amiga-exec`) sustituye a `exec_minima.c`. Para enchufarla:
   enlazar Rust (`aarch64_be-unknown-none-softfloat`, `staticlib`) en el ejecutable de C de la caja; un tipo que
   implemente su `Memoria` sobre los 4 GB del 68k, con la página 0 leída y escrita por su alias de escritura
   (`$3_8000_0000`); `Exec::arrancar` en lugar de `exec_minima_preparar` (escribe la base en 4 por ese alias);
   `dashos_native_call` le pasa los números de exec; y el reparto de los hunks (`hunks_reservar`) pasa a ser
   `AllocMem` con los atributos que ya da el cargador.
4. **`LoadSeg` y `UnLoadSeg`** como funciones de la dos.library (hoy el cargador solo lo usa la caja) piden la
   dos.library sobre el sistema de archivos.
5. **Los textos de la caja y del oráculo**: los programas escriben ASCII; con Latin-1 haría falta pasarlo a UTF-8 en
   la consola.

### Pruebas

- `make -C kernel prueba`: "BIEN: esperado.txt", con las líneas nuevas (`H4: la página 0 ...: bien` y `[malo] página
  0: ...`), y antes el cargador en el PC: "BIEN: cargador hunk (0 fallos)".
- `make -C kernel arranque-virt`: las cinco BIEN, con las líneas nuevas de la caja. La primera vuelta falló en la
  línea de `lineaa` (el fallo de `volatile` de arriba); arreglado, todo BIEN a la primera.
- `make -C caja/pruebas_hunk`: el cargador en el PC, con ASan y UBSan (también dentro de `make prueba`).

## 2026-11-12: exec, dos, graphics e intuition enchufadas; una ventana Amiga en el escritorio

Rama `caja-amiga-union`, desde `fusion-fondo` (`cc4c8ca`, con `revision` `c84e08a` encima).

### Qué funciona, de punta a punta (`make -C kernel arranque-virt`)

La caja ya no tiene bibliotecas en C: exec (`caja/exec`), dos (`caja/dos`), graphics e intuition (`caja/intuition`)
son los crates de Rust, enchufados por `caja/pegamento/`. Cada programa 68k corre como un proceso de exec
(`struct Process`, con su pila de `AllocMem`, la consola en `pr_CIS` y `pr_COS`, y el testigo de exec); al acabar, la
caja suelta lo que dejara (ficheros, cerrojos, ventanas, memoria). Los programas de antes salen igual (`hola`,
`lineaa`, `bucle`, `saludo.exe`, el cargador hostil y `mala`), y dos nuevos:

1. **`ventana.exe`** (`caja/pruebas68k/ventana.c`, de `m68k-amigaos-gcc`): abre dos, intuition y graphics con
   `OpenLibrary`; lee `SYS:LEEME.txt` por dos.library, que va sobre el servicio de archivos de DashOS; abre una
   ventana con `OpenWindowTags` (título, gadgets de cerrar, arrastre y profundidad, `IDCMP_CLOSEWINDOW`); dibuja fuera
   de ella (nada sale), y dentro cuatro rectángulos de colores (plumas de `ObtainBestPen`), un marco y dos diagonales
   con `Move`/`Draw` y dos líneas de texto con `Text` (topaz/8); y espera mirando su `UserPort` con `GetMsg` y
   `Delay(5)`, como mucho 40 s. La ventana es una ventana del compositor (un `xdg_toplevel` con su búfer `wl_shm`):
   sale en el escritorio, en la cascada, con su barra de Intuition dentro. El guion de la prueba pulsa su gadget de
   cerrar con el ratón USB de QEMU (por el monitor, como los demás clics): llega `IDCMP_CLOSEWINDOW` de verdad.
2. **`hostil.exe`** (`caja/pruebas68k/hostil.c`): 61 llamadas malas. `OpenWindow` y `OpenWindowTagList` con punteros
   a 0, a la página 0, fuera de la memoria del 68k y al final de los 4 GB; etiquetas en círculo (`TAG_MORE` a sí
   mismas) y una pantalla que no existe; `SetAPen`, `RectFill`, `Move`, `Draw`, `WritePixel` y `Text` con `RastPort`
   malos; `CloseWindow` de lo que no es una ventana; `Open` con un nombre fuera de la memoria, con `/..` y para
   escribir en `SYS:` (de solo lectura); `Read` de un fichero que no es y hacia la página 0 o fuera; cerrar dos veces;
   `FreeMem` de lo que no es suyo. Y deja sin soltar memoria, un fichero y un cerrojo. Ninguna funciona, la caja lo
   apunta (57 avisos del Enforcer; se escriben los 6 primeros), suelta lo que quedó y el compositor sigue contestando
   (un `wl_display.sync`).

Salida (QEMU virt; lo de antes, igual):

```
│   [caja-amiga] exec, dos, graphics e intuition nativas (Rust): ExecBase $102338, dos $200404, graphics $200e90, intuition $2023dc
│   [caja-amiga] SYS: sobre el servicio de archivos (la raíz de su ficha, de solo lectura); compositor: conectada por Wayland, Workbench de 800x600
│   [caja-amiga] página 0: ExecBase en la dirección 4 ($102338), solo de lectura; otra vez o en los primeros 64 KB, rechazada: bien
...
│   [caja-amiga] saludo.exe acabó con 0 (vector -1), 7 llamadas nativas, en 6 ms, sin memoria perdida: bien
│   [caja-amiga] ventana.exe: 2 hunks (código 2760, datos 80, BSS 0 bytes) en $202960, 64 reubicaciones
│   [caja-amiga] ventana: SYS:LEEME.txt, leido por dos.library: "DashOS lee RedoxFS" (18 bytes)
│   [caja-amiga] ventana: bordes 4, 11, 4 y 2 (izquierda, arriba, derecha, abajo)
│   [compositor] ventana de caja-amiga en (104, 144)
│   [caja-amiga] ventana Amiga en pantalla (328x163): Ventana Amiga: 68k en DashOS
│   [caja-amiga] ventana: pantalla 800x600, su fuente de 8 puntos, WBorTop 2
│   [caja-amiga] ventana: abierta (328x163, dentro 320x150), dibujada; espera IDCMP_CLOSEWINDOW
│   [caja-amiga] ventana: IDCMP_CLOSEWINDOW recibido tras unos 23700 ms
│   [caja-amiga] ventana.exe acabó con 0 (cerrada con su gadget), 538 llamadas nativas, en 23780 ms, 0 avisos, sin memoria perdida: bien
│   [caja-amiga] hostil.exe: 2 hunks (código 1468, datos 44, BSS 0 bytes) en $202960, 75 reubicaciones
│   [caja-amiga] Enforcer: OpenWindow (graphics/intuition): Exec(PaginaCero { dir: 4 })
...
│   [caja-amiga] hostil: 61 llamadas malas, 0 funcionaron; el programa sigue vivo
│   [caja-amiga] hostil.exe acabó con 0 tras 71 llamadas nativas: 57 avisos del Enforcer, 1 bloques sin soltar (los soltó la caja), el compositor contesta con 0 ventanas de la caja abiertas: bien
```

La captura de la ventana (con `CAPTURA_AMIGA`, en `kernel/target/amiga.ppm`): la ventana Amiga en (104, 144), con su
barra de título azul de Intuition, el gadget de cerrar a la izquierda y el de profundidad a la derecha, los cuatro
rectángulos (rojo, verde, azul y amarillo), el marco con sus diagonales y los textos "Hola desde el 68k" y "DashOS lee
RedoxFS" en blanco sobre negro. El guion espera 23 s a pulsar porque antes tiene que contestarse la pregunta de
permisos de `pide` (ver abajo).

### Lo que se hizo

- **`caja/pegamento/`** (Rust, `staticlib` para `aarch64_be-unknown-none-softfloat`, nightly con `build-std`). Por qué
  así y no la caja entera en Rust: el C que rodea al JIT (`MainLoop`, la entrada, el vigilante, los vectores) tiene que
  compilarse con los registros del 68k reservados (`-ffixed-x12` a `x29`, `v19` a `v26`), y rustc no tiene opción para
  eso. Rust solo se llama desde `caja_main` (antes y después de cada `MainLoop`) y desde `dashos_native_call`, que el
  JIT llama tras guardar x0..x30 y que recarga todos al volver (parche 0002); con `softfloat`, Rust no toca los
  registros de coma flotante. Lo que tiene:
  - `memoria.rs`: los rasgos `Memoria` de exec y de dos sobre la RAM del 68k y la página 0 (por su proyección de
    escribir, por encima de 4 GB). Lo de fuera da `None`: las bibliotecas lo apuntan, nunca lo tocan. El `unsafe` (los
    `from_raw_parts`) va comentado: mientras corre una función nativa, el 68k está parado.
  - `archivos.rs`: `SistemaArchivos` (dos) sobre el canal "archivos": sesión, ventana de 64 KB proyectada en 16 GB, una
    orden por método, todo en little-endian. Con `solo_lectura`, lo que escribe no llega al servicio (`SoloLectura`):
    `SYS:` es de solo lectura porque lo dice la caja, no el servicio (la ficha de `inicio` dejaría escribir, y dos solo
    mira el volumen para `Info`).
  - `compositor.rs`: `Compositor` (intuition) sobre Wayland, por el canal "ventanas", con el crate `wayland` (que ya
    escribe en little-endian) y `dashos` (las llamadas al sistema; compila en big-endian). Una ventana es un
    `xdg_toplevel` con un búfer `wl_shm` XRGB8888 en su franja de 64 MB (desde 20 GB); `presentar` copia lo dañado
    dando la vuelta a los bytes de cada punto y manda `damage` y `commit`. Ratón (`enter`, `motion`, `button`), teclado
    (`enter`, `leave`, `key`; el mapa de teclas se suelta), `ping`, `configure` y `close` pasan a `Evento`. El tamaño
    del Workbench es el modo del primer `wl_output` (como mucho 1024 x 768: su lienzo va al montón de Rust).
  - `lib.rs`: el estado (`Exec`, `Dos`, `CajaGrafica`), el montón (`GlobalAlloc` sobre un TLSF propio de 8 MB en
    10 GB, aparte del de Emu68), el reparto de los números nativos (exec 1-255, dos `0x100 + entrada`, graphics desde
    `0x400`, intuition desde `0x600`: no chocan), `Accion::Llamar` con destino nativo (el `Open` de las bibliotecas), y
    el planificador para una tarea: `Bloquear` (Wait, WaitPort) espera atendiendo al compositor hasta que la tarea
    vuelve a estar lista y repite la llamada (como mucho 60 s; después se corta el programa), `Dormir` (Delay) duerme
    atendiendo al compositor, `Ceder` le devuelve el testigo, `Terminar` corta el programa (la NMI del vigilante). Lo
    dibujado va al compositor en cuanto el programa llama a exec o a dos (un `Delay`, un `GetMsg`).
- **`caja/caja.c`**: sin `exec_minima.c` ni la dos.library en C. `dashos_native_call` pasa todo a `caja_llamada`.
  ExecBase la monta exec (`Exec::arrancar`) en la zona del sistema `$102000-$110000`; la página 0 se proyecta en 0
  después. Los hunks se cargan con `AllocMem` de exec; el cargador hostil va con un reparto rápido dentro de un bloque
  de `AllocMem` (con `AllocMem` en cada una de sus 30.000 cargas tardaba) y 1.000 cargas más con `AllocMem` de verdad,
  sin perder ningún bloque. Antes de cada programa, `jit_olvidar`: suelta lo traducido (un programa nuevo va donde
  estaba otro y Emu68 ejecutaba lo del de antes: `ventana.exe` corría el código de `saludo.exe`).
- **exec**: `Config::sistema` y `Config::caja` (la zona del sistema donde diga la caja; la memoria del 68k no empieza
  en 0, el kernel no proyecta nada por debajo de 64 KB salvo la página 0). Una prueba nueva con una memoria así.
- **`inicio`**: la caja lleva los canales "ventanas" (llave 2) y "archivos" (llave 3) y una ficha (Ana, 1000, la raíz
  del disco), con 32 MB. Va la última de la lista, tras la caja Linux y `sin-maquina`: `inicio` la espera
  (`SE_ESPERAN`), y con su ventana abierta (hasta 40 s si nadie la cierra) retrasaba lo de detrás; en
  `arranque-virt-vm`, `sin-maquina` llegaba cuando `guardian` ya no contestaba preguntas.
- **El guion** (`tools/prueba_uefi.sh`): cuando la caja dice que su ventana está en pantalla y el compositor dónde, y
  ya se ha contestado la pregunta de permisos de `pide` (o pasados 30 s), sin ninguna pregunta en pantalla: la captura
  (`CAPTURA_AMIGA`) y un clic en el gadget de cerrar (en (10, 5) de la ventana). Después el cursor vuelve a donde lo
  dejaban los pasos de siempre. Al principio pulsaba enseguida: si la pregunta de `pide` salía en ese momento, el clic
  le quitaba el teclado, su "s" no llegaba y `pide` se quedaba sin permiso.
- **`tools/guion_caja_linux.py`** (`arranque-virt-linux`): al repetir una orden, otro clic en la terminal antes. La
  ventana de la caja Amiga sale después de la terminal y se lleva el teclado (como toda ventana nueva).
- **`tools/captura_ventanas.py`**: Ajustes ya no es la tercera de la cascada (la ventana Amiga sale antes y ocupa un
  sitio): sus comprobaciones van donde dice el compositor que está.

### Mapa de la caja (lo que cambia)

| Dirección | Qué |
|---|---|
| `$0010_0000` – `$0010_1FFF` | Vectores, trampas, `COMUN` y `MARCA` |
| `$0010_2000` – `$0010_FFFF` | La zona del sistema de exec: ExecBase, su tabla de saltos, trampolines |
| `$0011_0000` – `$0014_FFFF` | Los programas planos de prueba, cada uno en su dirección |
| `$0015_0000` – `$001F_FFFF` | Memoria "chip" de exec (`AllocMem`) |
| `$0020_0000` – `$004F_FFFF` | Memoria "fast" de exec |
| 10 GB | El montón de Rust (8 MB) |
| 16 GB | La ventana del servicio de archivos |
| 20 GB | Los búferes de las ventanas (64 MB por ventana) |

### El oráculo

`saludo.exe` y `ventana.exe` en FS-UAE con el Kickstart 3.1 del usuario (`kick40068.A1200`) y el Workbench 3.1 de
`DASH_EMU_SYS`, por `~/Projects/FreeMEMLang/tools/emu_run.py`, con el cerrojo del emulador:

```
=== saludo: RC=0
  [caja-amiga] hunk: hola desde un ejecutable de AmigaOS (m68k-amigaos-gcc)
  [caja-amiga] hunk: 168 primos por debajo de 1000, el ultimo 997 (tres hunks)
  [caja-amiga] hunk: tabla 31415926, dos.library abierta (v36 o mas: 1)
  [caja-amiga] hunk: Write() en Output(): bien
=== ventana: RC=5
  [caja-amiga] ventana: SYS:LEEME.txt no se abre (IoErr 205)
  [caja-amiga] ventana: SYS:LEEME.txt, leido por dos.library: "" (0 bytes)
  [caja-amiga] ventana: bordes 4, 12, 4 y 2 (izquierda, arriba, derecha, abajo)
  [caja-amiga] ventana: pantalla 640x256, su fuente de 9 puntos, WBorTop 2
  [caja-amiga] ventana: abierta (328x164, dentro 320x150), dibujada; espera IDCMP_CLOSEWINDOW
  [caja-amiga] ventana: nadie la cerro en 15 s; se cierra sola
```

(En el Amiga, `Write` sale antes que el final de la línea de `VPrintf`, que va por el búfer de la consola: es el
relevo de `emu_run`, no el programa.)

- `saludo.exe`: las cuatro líneas, iguales.
- `ventana.exe` (una versión con 15 s de espera; nadie pulsa en el emulador): igual en lo que es de la interfaz.
  `SYS:LEEME.txt` no existe en el Amiga (205, `ERROR_OBJECT_NOT_FOUND`, el que daría la caja). La ventana mide 328x164
  allí y 328x163 aquí, pero por la fuente de la pantalla: el Workbench del emulador usa topaz/9 y la caja topaz/8. La
  barra de título es `WBorTop` (2) + la altura de la fuente + 1 en los dos: 12 allí, 11 aquí. Los lados (4) y abajo
  (2), iguales. La pantalla, 640x256 allí (PAL) y la del escritorio aquí (800x600).
- Las capturas del emulador (`--screenshot`) salen antes de que corran los programas: no hay foto de la ventana en el
  Amiga. Queda para otra vuelta (con `--autoclose` no vale: es para programas de Dash).

### Lo que falta y decisiones pendientes

1. **La fuente del Workbench**: topaz/8 en la caja, topaz/9 en el Workbench 3.1 del oráculo (lo pone su Prefs). ¿La
   caja debería leer la fuente de las preferencias (`ENVARC:Sys/font.prefs`) cuando dos las tenga?
2. **Una sola tarea 68k a la vez**: el planificador de exec ya decide, pero la caja tiene un solo estado del 68k y un
   solo hilo para él. Varias tareas piden un hilo (y un `M68KState`) por tarea, o cambiar de contexto en el hilo.
3. **El JIT y el código cambiado**: `SetFunction` y `MakeFunctions` apuntan qué olvidar (`tomar_codigo_cambiado`),
   pero desde una llamada nativa no se puede soltar lo traducido (se está ejecutando): hace falta lo del CINV de
   Emu68 (copiar el epílogo) o marcar y olvidar al volver a `MainLoop`. Hoy se olvida todo entre programa y programa.
4. **Ejecutar 68k desde una función nativa** (`Accion::Llamar` con `Codigo68k`: una función parcheada que exec llama
   por la tabla): da 0 y un aviso. Hace falta una tarea 68k anidada.
5. **`LoadSeg`** por dos (la caja carga los hunks ella; `cargar_segmentos` del rasgo `Caja` sigue sin hacer) y
   lanzar programas desde el 68k.
6. **`SYS:` de solo lectura por la caja**: dos no mira `solo_lectura` del volumen al escribir; lo para el adaptador.
   ¿Debería hacerlo dos (`caja/dos`), volumen a volumen?
7. **La ventana espera a la pregunta de `pide`**: el clic del guion y las preguntas de `guardian` se pelean por el
   teclado, y el guion espera a que se conteste (en `arranque-virt`, la ventana dura unos 25 s). En las pruebas sin
   guion de ratón (`-vm`, `-linux`, `-2`) se cierra sola a los 40 s; como la caja va la última, no retrasa a nadie.
8. **La cascada**: la ventana Amiga ocupa un sitio en la cascada del compositor y lo de después sale un sitio más
   abajo. Ajustes, en (136, 176) en vez de (104, 144).
9. Lo de antes sigue: el Enforcer de verdad (avisar y seguir en vez de morir), el oráculo de cada función.

### Pruebas

- `make -C kernel prueba`: "BIEN: cargador hunk (0 fallos)" y "BIEN: esperado.txt".
- `make -C kernel arranque-virt`: las cinco BIEN (tras juntar `revision` 946a07d). Antes, vueltas con fallos que
  salieron de esto y se arreglaron: el JIT con lo traducido del programa de antes; el clic del guion que dejaba sin
  teclado la pregunta de `pide` (y con ella Ajustes, más tarde); la capa del Workbench, a pantalla entera por el
  nuevo ritmo del arranque (el arreglo del compositor de arriba). Una vuelta falló en `red-gloton` ("dan -2" en vez
  de "-3"), sin relación con la caja; repetida, BIEN.
- `make -C kernel arranque-virt-vm`, `arranque-virt-linux` y `arranque-virt-2`: BIEN. `-linux` falló una vez (la
  ventana Amiga se llevaba el teclado de la terminal) hasta el arreglo del guion; `-vm`, mientras la caja iba delante
  de la caja Linux.
- `cargo test` en `caja/exec` (96), `caja/dos` (53), `caja/intuition` (47) y `wayland` (61): todas bien.


## 2026-11-12: programas de Dash en la caja

Rama `caja-amiga-dash`, desde `revision` (fec71bf).

### Qué funciona (`make -C kernel arranque-virt`)

Tras los programas de antes, la caja ejecuta seis programas de Dash, el lenguaje de FreeMEMLang, compilados con
`~/Projects/FreeMEMLang/dash` al hacer la caja:

1. **`texto.exe`** (`caja/pruebas68k/dash/caja_texto.dash`, por el camino directo de `dash`): cuentas (multiplicar,
   dividir y el resto van por el runtime de Dash en un 68000), textos (unir, trozos, buscar, cambiar, mayúsculas,
   hexadecimal), listas (`push`, `pop`, `sort` con una función), un diccionario, structs con métodos y 50 objetos en
   una lista, y el reloj (`time_ms`, que va por timer.device y `ReadEClock`, alrededor de un `time_delay(10)`).
2. **`texto_gcc.exe`**: el mismo con `dash --gcc` (por C, con m68k-amigaos-gcc y libnix). libnix abre
   utility.library al arrancar y hace las multiplicaciones y divisiones de 32 bits con `SMult32` y `UDivMod32`: unas
   6.800 llamadas nativas frente a 60 del directo.
3. **`hola.exe`** (`examples/hola.dash` de FreeMEMLang, tal cual): una ventana con `OpenWindowTags`, `Delay` de 5 s y
   texto.
4. **`ventana.exe`** (`caja_ventana.dash`): abre su ventana, dibuja con las plumas del Workbench (`RectFill`,
   `Move`/`Draw`, `WritePixel`, `Text`) y espera en un `EventLoop` con `On Close` y `On Timer 100` (timer.device, como
   mucho 40 s). El guion de la prueba captura la pantalla (`CAPTURA_DASH`) y pulsa su gadget de cerrar.
5. **`test_interp_comillas`** y **`test_lista_listas_literal`**: dos pruebas de `tests/` de FreeMEMLang, tal cual.
   Su salida es la de sus líneas `# expect-out` (las que FreeMEMLang comprueba en FS-UAE).

Cada uno acaba con 0, sin avisos del Enforcer y sin dejar memoria. Lo que escriben sale con "dash: " delante (la caja
pone un prefijo a cada línea de la consola mientras corren).

```
│   [caja-amiga] utility.library y timer.device nativos (Rust): $... y $...
...
│   [caja-amiga] texto.exe: 3 hunks (código 17864, datos 852, BSS 1500 bytes) en $202c48, 717 reubicaciones
│   [caja-amiga] dash: hola desde Dash
│   [caja-amiga] dash: suma de cuadrados 333833500, fib(20) 6765, 14285 y 5, -3
│   [caja-amiga] dash: hola, DashOS (12 letras), trozo [DashOS], busca 6
│   [caja-amiga] dash: a-b-c AMIGA ff
│   [caja-amiga] dash: lista 5, suma 25, ultima 7, palabras uno tres cinco
│   [caja-amiga] dash: dicc 3: 2 3 false
│   [caja-amiga] dash: punto (13, 3), 50 puntos, el ultimo (50, 100)
│   [caja-amiga] dash: reloj: time_delay(10) tarda unos 200 ms
│   [caja-amiga] dash: adios desde Dash
│   [caja-amiga] Dash texto.exe acabó con 0 (vector -1), 62 llamadas nativas, en 249 ms, 0 avisos, 0 bloques sin soltar: bien
│   [caja-amiga] texto_gcc.exe: 3 hunks (código 15672, datos 392, BSS 920 bytes) en $202c48, 503 reubicaciones
│   (las mismas nueve líneas)
│   [caja-amiga] Dash texto_gcc.exe acabó con 0 (vector -1), 6862 llamadas nativas, en 252 ms, 0 avisos, 0 bloques sin soltar: bien
│   [caja-amiga] dash: Starting Dash...
│   [caja-amiga] dash: Window 'Hola Dash' opened.
│   [caja-amiga] ventana Amiga en pantalla (400x150): Hola Dash
│   [compositor] ventana de caja-amiga en (232, 272)
│   [caja-amiga] dash: Closing window and exiting.
│   [caja-amiga] Dash hola.exe acabó con 0 (vector -1), 25 llamadas nativas, en 5016 ms, 0 avisos, 0 bloques sin soltar: bien
│   [caja-amiga] dash: ventana abierta y dibujada; espera a que la cierren
│   [caja-amiga] ventana Amiga en pantalla (360x170): Dash en DashOS
│   [compositor] ventana de caja-amiga en (264, 304)
│   [caja-amiga] dash: cerrada con su gadget
│   [caja-amiga] Dash ventana.exe acabó con 0 (vector -1), 238 llamadas nativas, en 3044 ms, 0 avisos, 0 bloques sin soltar: bien
```

### Lo que pedía Dash y se hizo

Primero se ejecutó tal cual: `hola.exe` ya funcionaba; `texto.exe` se quedaba sin memoria (`CreatePool` no estaba: el
runtime de Dash saca textos, listas y structs de un charco de exec) y `texto_gcc.exe` no arrancaba (libnix no
encontraba utility.library). Lo que se añadió:

- **El proceso, como desde el Shell** (`caja/pegamento/`): cada programa lleva una CLI (`pr_CLI`): sin ella, el
  runtime de Dash (y libnix) creen que vienen del Workbench y esperan su mensaje. `cli_Module` es su lista de
  segmentos (el runtime de Dash la usa para decir en qué hunk falló), `cli_CommandName` su nombre y la entrada y
  salida, la consola. La CLI y sus tres BSTR van en un bloque suyo, que la caja suelta con lo demás.
- **exec** (`caja/exec`):
  - Charcos (`charcos.rs`): `CreatePool`, `DeletePool`, `AllocPooled`, `FreePooled`. Lo que ve el 68k es lo de exec
    40: la cabecera empieza con una `MinList` de charcos, cada charco un `MemHeader` con su lista de trozos libres y 8
    bytes sin usar detrás (el runtime de Dash escribe un 0 ahí, por un fallo del `FreePooled` de la ROM 3.1). Por
    debajo, una tabla nativa con cada trozo y su tamaño: soltar lo que no es del charco, con otro tamaño o dos veces da
    un aviso y no toca nada; un `MemHeader` estropeado (límites cambiados, lista que apunta fuera) se nota antes de
    repartir. Los bloques son de quien creó el charco: si muere, se van con lo suyo.
  - Semáforos (`semaforos.rs`): `InitSemaphore`, `ObtainSemaphore`, `ObtainSemaphoreShared`, `AttemptSemaphore`,
    `AttemptSemaphoreShared`, `ReleaseSemaphore`, `AddSemaphore`, `RemSemaphore`, `FindSemaphore`. El de otra tarea
    hace esperar con `SIGF_SINGLE` y repetir; la cola de espera es nativa. Si el dueño murió con él cogido, quien lo
    pide se lo queda (con un aviso).
  - `SetExcept` (apunta las señales en `tc_SigExcept`; ver abajo) y `StackSwap` (la pila de Main de Dash, como el
    `swapstack` de libnix): la dirección de vuelta pasa a la pila nueva; un `stk_Pointer` fuera de sus límites o de la
    memoria no cambia nada.
  - Dispositivos (`dispositivos.rs`): `OpenDevice`, `CloseDevice`, `DoIO`, `SendIO`, `CheckIO`, `WaitIO`, `AbortIO`,
    `AddDevice`, `CreateIORequest`, `DeleteIORequest`, y `crear_dispositivo_nativo`. El `Open` y el `BeginIO` de un
    dispositivo nativo se llaman con `Accion::Llamar`, como el `Open` de una biblioteca. Si `DoIO` tiene que esperar,
    la tarea se bloquea y la caja repite la llamada: exec apunta qué peticiones ya salieron para no mandarlas dos
    veces. Los dispositivos van también en una tabla nativa: así no se recorre `DeviceList` en cada petición.
  - **Listas en círculo**: el fuzzer de `tests/hostil.rs` pasó de 1 s a 160 s con las funciones nuevas (una semilla
    dejaba una lista del sistema en círculo, y cada `FindName` o `Enqueue` daba un millón de vueltas antes de
    rendirse). `FindName`, `Enqueue` y los recorridos llevan ahora el detector de Brent: una lista que vuelve sobre sí
    misma se nota en unos pasos más que su largo. El fuzzer, 0,4 s.
- **utility.library** (crate nuevo, `caja/utility`): `SMult32`, `UMult32`, `SDivMod32`, `UDivMod32`, `SMult64`,
  `UMult64`; etiquetas (`NextTagItem` con `TAG_MORE`, `TAG_SKIP` y `TAG_IGNORE`, `FindTagItem`, `GetTagData`,
  `TagInArray`, `PackBoolTags`, `FilterTagItems`, `ApplyTagChanges`, `AllocateTagItems`, `CloneTagItems`,
  `FreeTagItems`, `RefreshTagItemClones`); `Stricmp`, `Strnicmp`, `ToUpper`, `ToLower` (Latin-1); `Amiga2Date`,
  `Date2Amiga`, `CheckDate`; `GetUniqueID`. Una lista de etiquetas en círculo, una división entre 0 o un
  `FreeTagItems` de lo que no dio dan un aviso. Sin hacer: `CallHookPkt` (ejecutar 68k desde lo nativo), los objetos
  con nombre, `MapTags`, `FilterTagChanges`, `PackStructureTags` y `UnpackStructureTags`.
- **timer.device** (crate nuevo, `caja/timer`): las cinco unidades sobre el reloj de DashOS; `TR_ADDREQUEST`,
  `TR_GETSYSTIME`, `TR_SETSYSTIME`, `AbortIO`; `AddTime`, `SubTime`, `CmpTime`, `ReadEClock` (709.379 Hz) y
  `GetSysTime`. Las esperas van en una tabla nativa; la caja devuelve las vencidas a su puerto en cada llamada nativa
  y mientras la tarea espera (cada 5 ms). Mientras hay una espera de timer.device, el plazo de 60 s de la caja para
  una tarea que no despierta cuenta desde que venza.
- **La consola con prefijo** (`caja_consola_prefijo`): mientras corre un programa de Dash, cada línea suya sale con
  "  [caja-amiga] dash: " delante.
- **El build** (`caja/Makefile`): con `~/Projects/FreeMEMLang/dash` (o `DASH=...`), los cuatro ejecutables se compilan
  en `caja/build/dash/` (fuera de git). Sin `dash`, o si no compila, un fichero vacío y un aviso: la caja dice que no
  está y sigue. Las fuentes propias, en `caja/pruebas68k/dash/`, se llaman `caja_*`: `dash --gcc` deja su C en
  `build/` de FreeMEMLang con el nombre de la fuente (y así no pisa el de nadie).
- **El guion** (`tools/prueba_uefi.sh`): tras la ventana de `ventana.exe`, cuando el programa de Dash dice que la suya
  está dibujada, la captura (`CAPTURA_DASH`, `target/dash.ppm`) y un clic en su gadget de cerrar.
- **`WFLG_GIMMEZEROZERO`** (intuition): las ventanas de Dash lo piden y dibujan en su `RPort` desde (0, 0). La caja
  aceptaba la marca pero dibujaba desde la esquina de la ventana: el marco del programa salía encima de la barra de
  título (se vio comparando la captura con la del oráculo). Ahora la capa del `RPort` de una ventana así es la de
  dentro: su (0, 0) es la esquina de dentro de los bordes (`graphics`: un origen por capa, que el pintor, `ReadPixel`
  y `ClipBlit` suman) y no se puede pintar fuera; al cambiar de tamaño, la capa sigue a los bordes. Prueba nueva en
  `intuition/tests/ventanas.rs`. Sin `BorderRPort` (para pintar en los bordes de una ventana así) todavía.

Pruebas nuevas de `cargo test`: `exec/tests/charcos.rs` (12: charcos, semáforos, `SetExcept`, `StackSwap`, con sus
hostiles), la lista en círculo en `exec/tests/listas.rs`, `utility/tests/utility.rs` (7) y `timer/tests/timer.rs` (7:
abrir y cerrar, `SendIO` y vuelta al puerto, `WaitIO` y `DoIO` que bloquean sin mandar dos veces, `AbortIO`, las
unidades, las funciones y una hostil: punteros malos a todo, mandar dos veces, soltar la petición mientras espera,
tiempos absurdos, la tarea que muere, `DeleteIORequest` dos veces o con otro tamaño). El fuzzer de exec prueba también
las funciones nuevas (todas las de la tabla, con registros al azar).

### El oráculo

Los cuatro ejecutables, los mismos ficheros, en FS-UAE con el Kickstart 3.1 del usuario (`kick40068.A1200`) y el
Workbench 3.1 de `DASH_EMU_SYS`, por `~/Projects/FreeMEMLang/tools/emu_run.py` con el cerrojo del emulador (la orden,
en el apartado de antes). La salida de `texto`, `texto_gcc`, `hola` y `ventana` es la misma, línea a línea, que la de
la caja (sin el "dash: "); `ventana`, con "nadie la cerro en 40 s" (allí nadie pulsa). La captura del emulador a los
30 s (`--screenshot`) sí pilla la ventana de Dash: misma disposición que en la caja, con topaz/9 en vez de topaz/8 (el
texto largo se corta allí por la derecha). Con esa comparación salió el fallo de `WFLG_GIMMEZEROZERO`.

### Lo que falta y decisiones pendientes

1. **`tc_TrapCode`**: el runtime de Dash pone su manejador de excepciones de la CPU en la tarea (dirección impar,
   instrucción ilegal...) para dar un mensaje y limpiar. En la caja, una excepción del 68k sigue acabando el programa
   por la trampa de la caja, sin pasar por él: el programa acaba sin su mensaje (la caja suelta lo suyo igual). Los
   errores que detecta el runtime solo (un campo de `nil`, dividir entre 0) sí salen bien: se vio "Error: campo de nil"
   antes de que hubiera charcos.
2. **`SetExcept` no lanza nada**: apunta las señales, pero la caja no llama a `tc_ExceptCode` (ejecutar 68k desde lo
   nativo). Ctrl-C en los bucles de Dash no corta el programa (la caja no manda Ctrl-C a nadie todavía).
3. **GadTools, ReAction, MUI, asl, diskfont, layers**: no hay. Los programas de Dash que los piden no abren la
   biblioteca y acaban con su error (el runtime lo comprueba). Es lo siguiente para más programas de Dash (GadTools
   primero: `On Gadget`).
4. **`BorderRPort`** de las ventanas `WFLG_GIMMEZEROZERO`: 0. Pintar en sus bordes no se puede.
5. **timer.device** mide todas las unidades con el reloj de DashOS y despierta con 5 ms de margen (la vuelta de la
   caja mientras espera). Para vídeo o sonido con el EClock puede quedarse corto.
6. **La espera de semáforos** entre tareas está probada con `cargo test`, no con dos tareas 68k (la caja corre una).
7. **El build depende de FreeMEMLang**: sin `~/Projects/FreeMEMLang/dash` la caja se compila igual (avisa), pero las
   líneas de Dash de `arranque_virt.txt` faltan y la prueba falla. ¿Deberían ir esas líneas aparte, en un fichero que
   solo se mire si está `dash`? De momento, en el de siempre (la opción más pequeña).
8. **`dash --gcc` escribe en `build/` de FreeMEMLang** (`caja_texto.c` y su `.lock`: es donde lo deja siempre, y está
   en su `.gitignore`). No se toca nada más de ese repositorio.
9. **El prefijo "dash: "** es de la caja, no del programa: si un programa escribe media línea y otro programa
   escribiera después, el prefijo sale donde empieza cada línea de la consola (hoy hay un solo programa a la vez).

### Pruebas

- `cargo test` en `caja/exec` (110, con las 13 de `charcos.rs` y la de la lista en círculo), `caja/utility` (7),
  `caja/timer` (7), `caja/intuition` (48, con la de `WFLG_GIMMEZEROZERO`) y `caja/dos` (53): todas bien. clippy
  limpio en los cuatro primeros.
- `make -C kernel prueba`: "BIEN: cargador hunk (0 fallos)" y "BIEN: esperado.txt".
- `make -C kernel arranque-virt`: las cinco BIEN en tres vueltas seguidas (con Dash, con `WFLG_GIMMEZEROZERO` y con
  las dos pruebas de FreeMEMLang). Una vuelta anterior salió con un error de sintaxis del guion: lo había editado
  mientras corría (bash lee el guion a trozos).
- `arranque-virt-vm`, `-linux` y `-2`: BIEN. En esas la prueba acaba antes de que la caja llegue a los programas de
  Dash (va la última y la ventana de `ventana.exe` espera 40 s sin el clic del guion): no los retrasan.
- `arranque-virt-arch`: todo bien salvo tres líneas del tamaño del disco ("la caja Linux: sectores 16384 a 8386559
  (4087 MB)" y las dos que lo repiten): el disco de Arch de `~/.cache/dashos-caja-linux/arch/disco-arch.img`, que es
  compartido, lo agrandó a 6 GB otra rama mientras tanto (`caja-linux-apps` ya espera 6135 MB). Arch arranca, teclea y
  se apaga como siempre. No es de la caja Amiga; al juntar con esa rama, desaparece.
- La captura: `capturas-agente/caja-dash-ventana.png` (la pantalla con la ventana de Dash, a 800x600) y
  `caja-dash-ventana-detalle.png` (la ventana, al doble); la del oráculo, `oraculo-fsuae-dash-ventana.png`.

## 2026-11-13: decisiones del usuario

El usuario aceptó todas las recomendaciones ("como recomiendas"):

1. **El ejecutable de la caja, por encima de 4 GB**: parche de Emu68 que cargue las direcciones de sus funciones con
   64 bits. Pronto: hoy el 68k puede escribir los datos de la caja y usar sus permisos.
2. **El Enforcer que avisa y sigue**: canal de excepciones del kernel por proceso. Después de GadTools.
3. **Sin PAN en la A72 (Pi 4)**: se acepta; en la Pi 5 (A76) se activa.
4. **UCI en EL0 para todos los procesos**: se acepta.
5. **Varias tareas 68k**: un solo hilo que cambia de tarea, como un Amiga (Forbid y Disable como esperan los
   programas). Para usar varios núcleos, un programa por caja.
6. **Cortar un programa con la NMI de nivel 7**: se acepta.
7. **Número de función de 16 bits**: se acepta.
8. **`SYS:` de solo lectura lo controla `dos`, volumen a volumen** (ERROR_DISK_WRITE_PROTECTED).
9. **La fuente del Workbench**, de las preferencias cuando las haya; si no, topaz/8.
10. **Las cuatro pruebas en una caja**: se queda así.
11. **Las líneas de Dash en un fichero aparte**, que solo se mira si está `~/Projects/FreeMEMLang/dash`.

Preocupación del usuario: el malware de Amiga era muy destructivo (virus de arranque, de enlace, troyanos que
formateaban). Estudio y pruebas en `docs/diseño/S28-malware-amiga.md`.

## 2026-11-13: 68k desde funciones nativas, tc_TrapCode, Ctrl-C y GadTools

Rama `caja-amiga-gadtools`, desde `revision` (a77a6cd), con `revision` (d819301) juntada antes de las pruebas finales.

### Qué funciona (`make -C kernel arranque-virt`)

Tras los programas de antes, la caja ejecuta:

1. **`gadtools.exe`** (`caja/pruebas68k/gadtools.c`, de `m68k-amigaos-gcc`). En la prueba de siempre está abierta 5
   s y se cierra sola: con más tiempo, su ventana tapaba los pasos del guion de los demás (Ajustes, el centro de
   control, el bloqueo de la sesión; los clics de esos pasos caían en ella) y el guion, ocupado con ellos, no llegaba
   a pulsarla. Lo interactivo va en el modo de las capturas (`CAJA_INTERACTIVA=1`, abajo), donde espera 90 s: una ventana con un gadget de cada tipo
   de GadTools (botón, casilla, ciclo, mx, texto, número, cadena, entero, deslizador, lista y paleta), un
   `DrawBevelBoxA` y menús (`CreateMenus`, `LayoutMenus`, `SetMenuStrip`). Comprueba solo `GT_SetGadgetAttrs`,
   `GT_GetGadgetAttrs`, `ItemAddress` y `GTMENUITEM_USERDATA`, y escribe cada evento. El guion captura la ventana
   (`CAPTURA_GT`), pulsa la casilla, el ciclo, la tercera opción del mx, un elemento de la lista y "Aceptar", y elige
   Proyecto > Salir con el botón derecho (captura con el menú abierto, `CAPTURA_GT_MENU`): llegan `GADGETUP`,
   `GADGETDOWN` y `MENUPICK` con sus códigos, y el programa acaba con 0, sin avisos ni memoria perdida.
2. **`anidado.exe`** (`anidado.c`), cuatro veces (el modo, en la última letra del nombre del proceso):
   - `a`: el `Open` de una biblioteca 68k hecha con `MakeLibrary`, que exec llama desde lo nativo; un `Open` que se
     abre a sí mismo sin fin (la caja corta en 16 niveles y da 0; el programa sigue); un `Open` que vuelve con la pila
     más abajo (se acepta con un aviso: la de fuera está intacta); `tc_ExceptCode` dos veces (con `A1` =
     `tc_ExceptData`); `CallHookPkt`; un `tc_TrapCode` propio que recupera una instrucción ilegal. Acaba con 0.
   - `b`: un `tc_ExceptCode` que vuelve con la pila más arriba (ha deshecho marcos de fuera): la caja corta el
     programa (vector -2).
   - `c`: una instrucción ilegal dentro de `tc_ExceptCode`, sin `tc_TrapCode` propio: el programa acaba por el
     vector 4 dentro de la llamada anidada y se deshacen los niveles.
   - `d`: excepción dentro de excepción (un `tc_TrapCode` que provoca otra, sin fin): se acaba la pila de supervisor y
     el programa acaba por el vector 4.
3. **Ctrl-C en Dash**: `caja_ctrl_c.dash` (un `while true` que no llama a nada), por el camino directo y por
   `--gcc`: la caja manda Ctrl-C desde otro hilo a los 700 ms y el programa para con "***Break" y 5. Por el camino
   directo es todo el recorrido de Dash: la interrupción de nivel 6, la señal, `tc_ExceptCode` anidado (el de Dash
   cambia los NOP de sus bucles por `TRAP #14` y llama a `CacheClearU`), el JIT olvida lo traducido sin soltar lo que
   está corriendo, el `TRAP #14` llega al `tc_TrapCode` de Dash y este vuelve con RTE a su salida.
   `test_ctrl_c_for.dash` de FreeMEMLang (Ctrl-C por `SetSignal` a sí mismo), tal cual: igual.
4. **Excepciones del procesador en Dash**: `test_trap_illegal.dash` y `test_trap_divzero.dash` de FreeMEMLang, tal
   cual: "Error: instrucción ilegal (excepción 4)" / "Error: división por cero (excepción 5)", dónde ("en el
   programa: hunk 0 + $7e2") y 20, con su limpieza normal (sin memoria perdida). Son las salidas que FreeMEMLang
   espera en FS-UAE.
5. **`examples/test_gadtools.dash`** de FreeMEMLang, tal cual: abre su pantalla (el Workbench, ver abajo), su
   ventana con dos botones, un deslizador, una paleta, un texto y menús. El guion la captura y pulsa "Cancel"; si no
   llega a tiempo, Ctrl-C a los 6 s (acaba con 0 o con 5). En el oráculo (`oraculo-fsuae-test_gadtools.png`) los
   gadgets de una ventana `GIMMEZEROZERO` van en la capa de dentro y su etiqueta se corta en el borde: igual aquí.

Salida (QEMU virt; lo nuevo):

```
│   [caja-amiga] gadtools.library nativa (Rust): $202a08
│   [caja-amiga] gadtools: atributos (nivel 60, ciclo 2, casilla 1), entero 42, ItemAddress y UserData del menu 2: bien
│   [caja-amiga] gadtools: ventana abierta y dibujada (12 gadgets); espera clics
│   [caja-amiga] gadtools: GADGETUP del gadget 2, codigo 0
│   [caja-amiga] gadtools: GADGETUP del gadget 3, codigo 1
│   [caja-amiga] gadtools: GADGETDOWN del gadget 4, codigo 2
│   [caja-amiga] gadtools: GADGETUP del gadget 10, codigo 2
│   [caja-amiga] gadtools: GADGETUP del gadget 0, codigo 0
│   [caja-amiga] gadtools: MENUPICK menu 0 elemento 2 (2)
│   [caja-amiga] gadtools.exe acabó con 0 (cerrada por el programa), ..., 0 avisos, 0 bloques sin soltar: bien
│   [caja-amiga] anidado: Open 68k desde exec: bien (abierta 1 vez)
│   [caja-amiga] 68k en $206ce2 desde una función nativa: demasiados niveles (D0 = 0)
│   [caja-amiga] anidado: Open que se abre a si misma: 16 niveles, 15 abiertas dentro
│   [caja-amiga] 1 llamadas anidadas volvieron con la pila más abajo de lo debido (la de fuera, intacta)
│   [caja-amiga] anidado: tc_ExceptCode 2 veces, A1 = $1234
│   [caja-amiga] anidado: CallHookPkt da 1320 (esperado 1320)
│   [caja-amiga] anidado: tc_TrapCode propio recupera la instruccion ilegal: bien
│   [caja-amiga] anidado-a acabó por el vector -1 (D0 0), 38 llamadas anidadas (1 no), 2 excepciones de tarea, 0 avisos: bien
│   [caja-amiga] anidado-b acabó por el vector -2 ...: bien
│   [caja-amiga] anidado-c acabó por el vector 4 ...: bien
│   [caja-amiga] anidado-d acabó por el vector 4 ...: bien
│   [caja-amiga] dash: bucle sin fin: espera a Ctrl-C
│   [caja-amiga] dash: ***Break
│   [caja-amiga] Dash ctrl_c.exe acabó con 5 (vector -1) en 705 ms, 1 excepciones de tarea, 0 avisos, 0 bloques sin soltar: bien
│   (lo mismo con ctrl_c_gcc.exe y test_ctrl_c_for)
│   [caja-amiga] dash: Error: instrucción ilegal (excepción 4)
│   [caja-amiga] dash: en el programa: hunk 0 + $7e2
│   [caja-amiga] Dash test_trap_illegal acabó con 20 (vector -1) en 12 ms, ...: bien
│   [caja-amiga] dash: Error: división por cero (excepción 5)
│   [caja-amiga] Dash test_trap_divzero acabó con 20 (vector -1) en 12 ms, ...: bien
```

6. **`EasyRequestArgs`** (`caja/intuition/src/requester.rs`): una ventana con el título, el texto (`es_TextFormat`
   con sus argumentos, `%s`, `%ld`...) y un botón por cada parte de `es_GadgetFormat`, pintados por el tema; la tarea
   espera (`SIGF_SINGLE`) hasta el clic y la repetición de la llamada devuelve el botón (1, 2... y 0 el último). Es lo
   que usa Dash para su error cuando el programa no tiene consola (como desde el Workbench).

**El modo de las capturas** (`CAJA_INTERACTIVA=1` al hacer la caja; `make -C kernel arranque-virt` lo pasa a
`make -C caja`; hay que tocar `caja.c` para que se recompile): `gadtools.exe` espera 90 s y el guion, cuando su ventana
se ve entera (lo mira en una captura: sus esquinas y un sitio vacío, de los colores del Workbench), la captura, pulsa la
casilla, el ciclo, una opción del mx, un elemento de la lista y "Aceptar", y elige Proyecto > Salir con el botón
derecho (captura con el menú abierto); y `test_trap_illegal` corre otra vez sin consola: su error sale en un
requester, que el guion captura (`CAPTURA_REQ`) y cierra. Las capturas de esta entrada son de ese modo, con
`DASHOS_PANTALLA=1920x1080`. El guion hace los pasos de la caja al principio de cada vuelta (los de más abajo
esperan con `continue`) y solo pulsa en la ventana de GadTools cuando se ve entera (si no, un clic en su borde
izquierdo la sube); aun así, con la máquina cargada, unas vueltas lo consiguen y otras no.

### Código 68k desde una función nativa (la reentrada en el JIT)

Una función nativa corre dentro de la unidad traducida que tiene su `$A0FF`, y esa unidad sigue al volver. Para
ejecutar 68k desde ella (`caja_ejecutar_68k`, caja.c) se abre otro `MainLoop` dentro: una pila de contextos.

- **Qué se guarda.** Los x0..x30 ya los guarda el JIT al llamar a la función (parche 0002) y los recarga al volver.
  Lo que no recarga son los registros vectoriales: v8-v15 (FP0-FP7, que además el ABI pide conservar) y v19-v31 (el
  SR, FPCR/FPSR/FPIAR, la última PC, el puntero al contexto, el contador, CACR y USP/ISP/MSP). Se guardan y se
  devuelven en un solo asm alrededor de `bl MainLoop` (`main_loop_anidado`): entre medias el compilador no puede tener
  nada en ellos. El SR y las pilas del nivel de dentro salen de los registros vivos (no de `M68KState`, que está
  viejo), y los campos arquitectónicos de `M68KState` se devuelven al acabar.
- **La vuelta.** Se mete en la pila 68k la dirección `VUELTA_ANIDADA` (sistema.s): `$A0FF $FF00`, que apunta que el
  nivel volvió y comprueba la pila, y `jmp 0.w`, donde acaba ese `MainLoop`. Con la pila donde se dejó, bien; más
  abajo (el 68k dejó algo), se acepta con aviso; más arriba (deshizo marcos de fuera: un longjmp, la vuelta de otro
  nivel), se corta el programa.
- **El JIT.** La unidad que llamó se saca de la lista LRU y de la tabla mientras dura el nivel (`fijar`): ni el JIT al
  quedarse sin sitio, ni al ver su código cambiado, ni `caja_jit_olvidar` la sueltan. `caja_jit_olvidar` (con
  `CacheClearU`, `CacheClearE`, `SetFunction`, `MakeFunctions`) olvida todo lo demás desde la propia función nativa;
  la última PC pasa a "ninguna" (en ese nivel y, al volver, en los de fuera), porque su unidad puede no existir.
  Era el punto 3 de lo pendiente de la entrada anterior.
- **Tope.** 16 niveles: más da `ANIDADA_TOPE` (la función nativa sigue como si el 68k hubiera dado 0, con un aviso).
  Una pila 68k sin sitio, `ANIDADA_PILA`, igual.
- **Si el 68k de dentro no vuelve.** Si el programa acaba dentro (su RTS final a `COMUN`, una excepción sin
  `tc_TrapCode`, la NMI del vigilante), se apunta cómo acabó (vector y D0) y se deshacen todos los niveles: cada
  función nativa de fuera corta el programa (la NMI), su `MainLoop` acaba por el vector 31 y entretanto ninguna llamada
  nativa más se ejecuta. `arranca` da lo apuntado. Un programa que sigue para siempre dentro de un nivel (un longjmp
  fuera de un gancho) es como uno que sigue para siempre: lo corta el vigilante o su espera de 60 s.
- **Rust.** Ninguna referencia de `caja()` vive a través de `caja_ejecutar_68k`: `caja_llamada` la suelta, ejecuta y
  pide otra (`ejecutar_68k`, `entregar_excepciones`). Exec pide el 68k con su continuación (`Accion::Llamar` y
  `continuar`), que ya existía.

### tc_TrapCode, el modo de usuario y las excepciones de tarea

- **Las excepciones del procesador** van a `tc_TrapCode` de la tarea, como en exec: cada vector es `move.l #n,-(sp);
  jmp DESPACHO` (sistema.s, en 68k, sin llamadas nativas) y el despacho salta a `tc_TrapCode` con el número encima del
  marco. Sin tarea o sin `tc_TrapCode`, o con la pila de supervisor casi acabada (una excepción dentro de otra, sin
  fin), el programa acaba por esa excepción (`TRAMPA_DEFECTO`, que es también `ExecBase->TaskTrapCode` y el de cada
  proceso). Las interrupciones de los niveles 1-5 y 7 acaban el programa como antes; la de nivel 6 es la de la caja.
  No hizo falta la reentrada para esto: el `tc_TrapCode` de Dash vuelve con RTE al programa, todo en 68k.
- **Modo de usuario.** Los programas corren ahora en modo de usuario, con una pila de supervisor de la caja (16 KB de
  AllocMem): el manejador de Dash vuelve con RTE en modo de usuario a una pila suya, y con la misma pila para todo una
  interrupción posterior pisaba la del programa.
- **`tc_ExceptCode`** (exec: `tomar_excepcion`, `acabar_excepcion`): al final de cada llamada nativa, si la tarea ha
  recibido alguna de sus señales de `tc_SigExcept`, la caja ejecuta su `tc_ExceptCode` anidado, en modo de usuario y
  en su pila, con D0 = las señales, A1 = `tc_ExceptData` y A6 = ExecBase; las señales salen de recibidas y de
  `tc_SigExcept` mientras dura, y vuelven a `tc_SigExcept` las que devuelva en D0. Si la tarea espera en `Wait`, exec
  la saca (`despertar`), se entrega y el `Wait` se repite.
- **Ctrl-C desde fuera**: otro hilo de la caja pide Ctrl-C y levanta la interrupción de nivel 6 del 68k; su vector
  (`$A0FF $FF01; rte`) da la señal a la tarea y entrega su excepción. Así se corta un bucle que no llama a nada. En
  `Wait` y `Delay` la caja también lo mira.
- **`CallHookPkt`** no es nativa: su entrada es `move.l 8(a0),-(sp); rts` (lo de la ROM: salta a `h_Entry`).

### GadTools

`gadtools.library` 40 nativa, dentro del crate de intuition (`caja/intuition/src/gadtools.rs` y `menus.rs`), escrita
desde los autodocs, `libraries/gadtools.h`, `gadtools_lib.fd` y el RKRM. Números nativos desde `0xA00`.

- `CreateContext`, `CreateGadgetA` (botón, casilla, entero, lista, mx, número, ciclo, paleta, barra, deslizador,
  cadena y texto), `FreeGadgets`, `GT_SetGadgetAttrsA`, `GT_GetGadgetAttrsA`, `GT_GetIMsg`, `GT_ReplyIMsg`,
  `GT_FilterIMsg`, `GT_PostFilterIMsg`, `GT_RefreshWindow`, `GT_BeginRefresh`, `GT_EndRefresh`, `DrawBevelBoxA`,
  `GetVisualInfoA`, `FreeVisualInfo`, `CreateMenusA`, `FreeMenus`, `LayoutMenusA` y `LayoutMenuItemsA`.
- Cada gadget es una `struct Gadget` de verdad (con su `StringInfo` y su búfer en los de texto), de `AllocMem` de la
  tarea; su estado y su dibujo son nativos. Se dibujan en el lienzo de la ventana con el aspecto de 3.x, con las plumas
  de la pantalla. En una ventana `GIMMEZEROZERO`, los gadgets sin `GTYP_GZZGADGET` van en la capa de dentro.
- intuition: `AddGadget`, `AddGList`, `RemoveGadget`, `RemoveGList`, `RefreshGList`, `RefreshGadgets`, `OnGadget`,
  `OffGadget`, `ItemAddress`, `OnMenu`, `OffMenu`, `IntuiTextLength`, `GetDefaultPubScreen`, `WA_Gadgets` y
  `nw_FirstGadget`. Pulsar: `GADGETUP`/`GADGETDOWN` con su código (y `MOUSEMOVE` al arrastrar un deslizador), la cadena
  se edita con el teclado (Return y Tab acaban). Los gadgets de intuition que no son de GadTools se pulsan (y mandan
  sus mensajes) pero no se dibujan todavía.
- **Menús**: con el botón derecho la barra de menús sale en la barra de título de la ventana (no hay barra de
  pantalla: cada ventana Amiga es una del escritorio), el menú cae debajo y al soltar llega `MENUPICK` con
  `FULLMENUNUM` (o `MENUNULL`). Aspecto NewLook de 3.x; teclas Amiga dibujadas, `CHECKIT`/`MENUTOGGLE`, desactivados.
- **Un solo punto para el aspecto** (decisión del usuario: poder elegir entre el aspecto de DashOS y el original):
  todo lo que se ve de GadTools y de los menús (botones, casillas, ciclos, mx, cadenas, deslizadores, listas, paletas,
  etiquetas, bordes en relieve, la barra y los menús que caen, y las plumas de `DrawBevelBoxA`) se pinta por el rasgo
  `Tema` (`caja/intuition/src/tema.rs`), con los colores de `Plumas`: los de las plumas de la `DrawInfo` de la
  pantalla, leídos de la memoria del 68k. La lógica (estado, clics, mensajes) no pinta nada. Hoy hay un tema,
  `Clasico` (3.x). **Siguiente paso: el tema Material 3 de DashOS**, otro `impl Tema`, y cómo se elige (por caja, por
  programa, en Ajustes): sin hacer.
- `WaitTOF` duerme un cuadro (20 ms) atendiendo al compositor: los bucles que leen eventos sin `Wait` (los de Dash
  con `wait_vblank`) los reciben así. `GetVPModeID` da "PAL:HighRes".

### El oráculo

`gadtools.exe` en FS-UAE con el Kickstart 3.1 del usuario y el Workbench 3.1 de `DASH_EMU_SYS`, con el cerrojo:
la misma línea de atributos ("nivel 60, ciclo 2, casilla 1, entero 42, ... : bien"); allí nadie pulsa y se cierra a
los 40 s. La captura del emulador (`capturas-agente/oraculo-fsuae-gadtools.png`) sirvió para corregir el aspecto: en
un mx, las opciones van donde diga `ng_Flags` (a la izquierda) y su etiqueta no sale; el nivel del deslizador va a su
izquierda encima de la etiqueta (en 3.1 se pisan: "Niv25"); el mando es un bloque oscuro. Diferencias que quedan: la
fuente (topaz/9 allí), los botones del mx (redondeados allí) y la marca de la casilla.

### Lo que falta y decisiones pendientes

1. **`OpenScreen` da el Workbench** (lo cuenta como un `LockPubScreen`; `CloseScreen` lo suelta): cada ventana ya es
   una del escritorio y una pantalla propia no tiene dónde ir. Provisional y fácil de quitar. ¿Una pantalla propia
   debería ser otra superficie del compositor (a pantalla entera, como un juego)?
2. **Los menús salen dentro de la ventana** (en su barra de título, cortados por su tamaño). La otra opción: una
   superficie emergente del compositor (`xdg_popup`) por menú. Decisión del usuario.
3. **Los programas corren en modo de usuario** (antes, en supervisor). Lo que use instrucciones privilegiadas sin
   `Supervisor()` da ahora la excepción 8, como en el Amiga. `Supervisor()` no está.
4. **asl.library**: no se hizo (el tercer objetivo). `file_open_dialog` de Dash no abre nada.
5. **Gadgets de intuition sin GadTools**: no se dibujan (`Border`, `Image`, `IntuiText` de `GadgetRender`), ni los
   gadgets propietarios (`PROPGADGET` de intuition) se mueven; BOOPSI tampoco. Los ganchos (`GTST_EditHook`, los de
   `GTLV_CallBack`) no se llaman todavía, aunque ya se podría con `caja_ejecutar_68k`.
6. **Ctrl-C de verdad**: hoy lo manda la caja (sus pruebas). Falta de dónde: una consola de DashOS para la caja, o
   Ctrl-C en una ventana Amiga con el foco.
7. **`tests/test_gt_events.dash` de FreeMEMLang** no encaja todavía: lanza un proceso auxiliar que pulsa por
   `input.device`, y la caja corre una sola tarea 68k.
8. Lo de antes sigue: el Enforcer de verdad, varias tareas 68k, `LoadSeg` por dos.

### Capturas (`capturas-agente/`, fuera de git)

- `caja-gadtools-fhd.png`: la ventana de GadTools a 1920x1080 (con Ajustes y el centro de control del guion al lado).
- `caja-gadtools-menu-fhd.png`: la misma con el menú Proyecto abierto ("Abrir... A", la raya, "Salir Q" señalado);
  se ven ya la casilla quitada, el ciclo en "Dos" y el mx en "Azul" (los clics del guion).
- `caja-dash-test_gadtools.png`: `examples/test_gadtools.dash` de FreeMEMLang en la caja (800x600, la prueba de
  siempre; el guion pulsó después su "Cancel" y acabó con 0). Como en el oráculo: los gadgets en la capa de dentro de
  la ventana `GIMMEZEROZERO`, la etiqueta "Value:" cortada por el borde y el nivel a la izquierda.
- `oraculo-fsuae-gadtools.png` y `oraculo-fsuae-test_gadtools.png`: lo mismo en FS-UAE (Kickstart 3.1).
- El requester del error de Dash: se abre (la salida dice "ventana Amiga en pantalla (360x79): Dash" y la prueba
  `easy_request` de `cargo test` lo pulsa), pero en las vueltas del modo de capturas la captura del guion cayó con la
  sesión bloqueada (el paso de la pantalla de bloqueo del guion) o no llegó: **sin captura todavía**.

### Pruebas

- `cargo test`: exec 111, dos 53, intuition 52 (con `tests/gadtools.rs`: gadgets y eventos, menús con el botón
  derecho, `EasyRequestArgs` y una hostil; el fuzzer de `hostil.rs` llama también a gadtools), utility 7, timer 7.
  clippy limpio en exec, utility e intuition. El fuzzer encontró un fallo de antes: un `IDCMP_NEWSIZE` del compositor
  con unos límites de `WindowLimits` absurdos pedía un lienzo de 200 TB; ahora el tamaño se comprueba.
- `make -C kernel prueba`: "BIEN: cargador hunk (0 fallos)" y "BIEN: esperado.txt" (tras juntar `revision`).
- `make -C kernel arranque-virt`: las cinco BIEN, con las líneas nuevas de la caja (GadTools, los cuatro modos de
  `anidado`, Ctrl-C y las excepciones de Dash). Vueltas anteriores fallaron por cómo convivía la ventana de GadTools
  con los pasos del guion de los demás (Ajustes, el centro de control: sus clics caían en ella, o los del guion de la
  caja en ellos); por eso en la prueba de siempre está 5 s y lo interactivo va en el modo de las capturas.
- `arranque-virt-vm`, `-linux`, `-2`, `-diag`, `-escritorio`: BIEN. `-arch`, `-wayland` y `-gpu`, con el kernel y el
  disco de lo fusionado (`ARCH_IMAGE` y `ARCH_DISCO` de `~/.cache/dashos-caja-linux-fusion`): BIEN.
- El modo de las capturas (`CAJA_INTERACTIVA=1 DASHOS_PANTALLA=1920x1080 TIEMPO_VIRT=480`): sale "FALTA: pantalla del
  firmware: 800x600" (la línea esperada es la de 800x600); no es una prueba, solo para las capturas.
- El oráculo, sin cerrojo desde ahora (orden del usuario: la caja Amiga no abre servidores).

## 2026-11-13: el tema Material de DashOS en la caja Amiga (S26)

Rama `gadtools-material` (desde `fusion-fondo`). Lo que pinta la caja Amiga (GadTools, los menús, `EasyRequest`,
`DrawBevelBoxA` y los bordes de intuition) sale ya con el aspecto de DashOS si el ajuste de S26 dice "como DashOS" (lo
de serie), y con el de 3.x si dice "original".

### Cómo va

- **Los colores**: `caja/pegamento/src/tema.rs` saca de `kit::tema::Tema` (el tema del sistema, con la paleta del
  fondo, claro u oscuro) los papeles de Material 3 que hacen falta (`Tokens`, en `caja/intuition/src/compositor.rs`):
  primario, sobre-primario, superficie y su variante, contornos, contenedores de superficie (bajo, normal, alto) y el
  contenedor secundario (el acento suave). Ningún color está escrito a mano (salvo `Tokens::CLARO`, solo para las
  pruebas). Las plumas de la `DrawInfo` siguen siendo las de 3.x: el tema Material no las usa.
- **El puente**: dos funciones nuevas del rasgo `Compositor`, `tema_amiga(programa)` (los tokens si ese programa va
  como DashOS) y `version_tema()` (sube con cada evento del tema). La caja las da desde `TemaCaja`.
- **Quién elige**: cada ventana guarda el programa que la abrió (el nombre de su tarea, sin camino) y `tema_de(w)` da
  `Elegido::Material` o `Elegido::Clasico`. Las excepciones por programa del ajuste (`programa amiga original X`)
  funcionan ya. `DrawBevelBoxA` mira la ventana de la capa del `RastPort` (o, si no hay, la tarea).
- **Al cambiar el tema o el ajuste con la ventana abierta**: `atender` ve otra versión y repinta los bordes, los gadgets
  de GadTools y los requesters; lo de dentro que tenía el color de fondo de antes pasa al de ahora. Lo que el programa
  pintó con otros colores se queda; un menú abierto, hasta que se mueve.
- **El fondo de las ventanas**: con "como DashOS", una ventana nueva sale de la superficie del tema. La paleta de la
  pantalla no se toca (S26): un programa que borra con la pluma 0 pinta el gris de siempre.

### Los controles (`caja/intuition/src/tema.rs`, `Material`)

Todo en el mismo rectángulo que el clásico y con la etiqueta y el nivel en el mismo sitio (lo comprueba
`mismo_rectangulo_que_el_clasico` con los 12 tipos, los 5 lugares de la etiqueta, pulsados y desactivados). Bordes
suaves con 16 muestras por píxel, en octavos de píxel (`Forma`, `trazo`).

| Control | Material |
|---|---|
| Botón | Píldora tonal (contenedor secundario); pulsado, del primario |
| Casilla | Cuadrado de esquinas de 2 en el centro de su caja: contorno; marcada, primario con la marca |
| Mx | Botones de radio: aro (primario el elegido) con su punto |
| Ciclo | Desplegable con contorno: texto a la izquierda y triángulo a la derecha (sigue pasando de opción con el clic) |
| Cadena y entero | Campo con contorno de esquinas de 4; con el cursor, contorno de 2 del primario y cursor de barra |
| Texto y número | Con borde: relleno del contenedor alto, sin contorno |
| Deslizador | Pista de 4 (primario hasta el mando) y mando redondo del primario |
| Barra (scroller) | Pista y mando como píldoras |
| Paleta | Tarjeta con contorno, muestras redondeadas, la elegida con aro del primario |
| Lista | Tarjeta con contorno; la fila elegida en el acento suave; barra de píldoras y chevrones |
| `DrawBevelBoxA` | Tarjeta con contorno (variante), esquinas de 4; dentro no se toca |
| Menús | Barra del contenedor alto; el título señalado en píldora; el menú, contenedor bajo con contorno; el elemento señalado en el acento suave; la tecla Amiga en una tecla redondeada |
| `EasyRequest` | Ventana con bordes Material, fondo de superficie, botones tonales |
| Bordes de la ventana | Barra del mismo alto: contenedor alto (activa) o contenedor (inactiva), título a la izquierda como la barra de DashOS, X de cerrar, icono de profundidad (dos ventanas), rayas de tamaño; lados y abajo de la superficie, sin relieve |

Desactivado: todo al 38 % sobre la superficie.

### Lo que falta y decisiones pendientes

1. **Esquinas redondeadas de las ventanas Amiga**: el compositor ya les pone sombra, pero no esquinas (la caja manda
   XRGB8888: el compositor solo redondea las que llevan su barra o las ARGB con esquinas transparentes). Para tenerlas,
   la caja tendría que mandar ARGB con las esquinas transparentes (`caja/pegamento/src/compositor.rs`), o el
   compositor redondear las de la caja Amiga.
2. **El gadget de zoom**: intuition aún no tiene (`WA_Zoom`); cuando lo tenga, su icono Material va en el mismo tema.
3. **Los gadgets de intuition sin GadTools** (`Border`, `Image`, `IntuiText`, `PROPGADGET`) y BOOPSI siguen sin
   dibujarse: cuando se hagan, por el mismo `Tema`.
4. **La `DrawInfo`** sigue con las plumas de 3.x aunque el programa vaya como DashOS (S26 pedía plumas nuevas con
   `ObtainPen`). Es una por pantalla y las excepciones son por programa: queda por decidir.
5. **Siguiente paso, cinco temas para elegir** (dato del usuario): Claro, Oscuro, AOS4 (estilo AmigaOS 4), MOS
   (estilo MorphOS) y Magic WB (estilo MagicWB). `Material` sirve para Claro y Oscuro (los tokens ya salen del modo y
   de la paleta del fondo). Los otros tres son más implementaciones de `Tema` (con sus `fondo_ventana`, `marco` y
   `bisel_caja`) y una variante más de `Elegido`; la lógica de GadTools, menús y ventanas no cambia. Falta que el
   ajuste de S26 (`dashos_tema_v1.cajas`) lleve qué tema, no solo "como DashOS"/"original".
6. **Tamaño en Full HD**: ver las capturas.

### Capturas (`capturas-agente/`, fuera de git)

Con `CAJA_INTERACTIVA=1 DASHOS_PANTALLA=1920x1080 TIEMPO_VIRT=480` y `DASHOS_PREFS` (el fondo y "cajas=general dashos"
u "original"):

- `caja-gadtools-menu-material-ondas-fhd.png` y `caja-gadtools-menu-material-atardecer-fhd.png`: la ventana entera
  con el menú Proyecto abierto ("Salir" señalado). Los colores cambian con el fondo (acento 3d6f88 con las olas,
  c44e68 con el atardecer).
- `caja-gadtools-menu-clasico-ondas-fhd.png` y `caja-gadtools-clasico-ondas-fhd.png`: lo mismo con "original" (3.x).
- `caja-gadtools-material-atardecer-fhd.png`: la barra Material (X, profundidad, título) y los gadgets, con Ajustes
  tapando parte. `caja-gadtools-material-atardecer-oscuro-fhd.png`: la misma ventana repintada al pasar el guion al
  modo oscuro con ella abierta (tapada en parte por el centro de control).
- Sin captura limpia de la ventana sin menú: las ventanas del guion (Ajustes, el centro, uno maximizado) la tapan
  según cómo vaya la carga del PC.

- **A 1:1 se ve muy pequeño en Full HD**: la ventana de GadTools mide 410x192 con topaz/8 en una pantalla de
  1920x1080. Escalar las ventanas Amiga (x2) es otro paso (en el compositor o en la caja), no hecho.
- La pantalla de Workbench de la caja sigue topada a 1024x768: las ventanas Amiga salen en esa zona, arriba a la
  izquierda.
- El guion de las capturas (`tools/prueba_uefi.sh`) reconocía la ventana de GadTools por los colores de 3.x; ahora
  también por los de Material (barra, superficie y la casilla del primario: una ventana del kit encima tiene la misma
  superficie).
- El ejemplo de Dash (`test_gadtools.dash`) y el requester: sus capturas salieron tapadas por una ventana maximizada;
  no se entregan.

### Pruebas

- `cargo test` de `caja/intuition`: 7 de `tema.rs` (mismo rectángulo que el clásico para los 12 tipos y 5 lugares,
  colores de los tokens, menús, bordes, `DrawBevelBox`, círculos suaves y una hostil: rectángulos vacíos, negativos,
  enormes, campos absurdos, radios y grosores imposibles) y 3 de `tests/material.rs` (ventana con el tema y al
  cambiarlo con la ventana abierta, la excepción por programa, plumas de la `DrawInfo` fuera de la paleta y su tabla
  apuntando fuera de la memoria, `DrawBevelBoxA`); las de antes, igual. clippy limpio en lo nuevo.
- Lo hostil encontró desbordes en código compartido (`mando` y `celdas` con límites de -2^31 a 2^31-1, o un
  `GTPA_IndicatorWidth` enorme) y que el fantasma clásico recorría todo el rectángulo de un gadget enorme aunque
  cayera fuera del lienzo: arreglados (64 bits, y solo lo que cae en el lienzo).
- `make prueba`: BIEN esperado.txt y BIEN cargador hunk. `make arranque-virt`: las cinco BIEN (tras juntar
  `fusion-fondo`, que arregla la barra con `dashos_tema_v1` v4; antes, "el compositor pide 800x560" faltaba por eso).
  La línea de atributos de `gadtools.exe` se escribía en dos veces y el aviso de la ventana en pantalla caía en medio
  (con el tema Material, siempre): ahora va en una sola escritura (`caja/pruebas68k/gadtools.c` y su `.exe`).
- `arranque-virt-vm`, `-2` y `-escritorio`: BIEN.

## 2026-11-13: las ventanas Amiga a 2x y 3x en pantallas grandes

Rama `caja-amiga-escala`, desde `revision` (267a072). En Full HD, una ventana de GadTools (410x192 con topaz/8) se
veía diminuta. Ahora cada ventana Amiga se ve a un factor entero (1x, 2x, 3x) y el programa no se entera.

### Cómo va

- **Dónde se escala: en la caja, al mandar el búfer** (`caja/pegamento/src/compositor.rs`, con lo que se puede probar
  en el PC en `caja/intuition/src/escala.rs`). Intuition pinta a 1x como siempre. Al mandar una ventana al compositor,
  cada píxel va como un cuadrado de `f` x `f` (vecino más cercano: nítido, como en un monitor del Amiga), solo lo
  dañado. El ratón que llega del compositor se divide entre `f` (redondeando hacia abajo, también fuera de la ventana):
  los clics en gadgets, el arrastre de un deslizador y los menús con el botón derecho caen donde deben. El compositor
  ve una ventana normal, más grande.
- **Por qué no por el protocolo.** `wl_surface.set_buffer_scale` sirve para lo contrario (un búfer más grande que la
  ventana, para HiDPI): no amplía. `wp_viewporter` (`set_destination`) sí, pero el compositor de DashOS no lo tiene ni
  escala nada al componer: habría que meter la escala en la composición, en dónde cae el ratón, en los daños y en las
  sombras de `wayland/src/escena.rs`, que además lleva otro agente a la vez. Ampliar en la caja cuesta f² de memoria
  (410x192 a 2x: 1,2 MB) y la copia, que es poca. Si algún día el compositor tiene `wp_viewporter`, solo cambia
  `CompositorWayland`: la caja mandaría el búfer a 1x con su destino a `f` veces.
- **El tope.** El búfer ampliado tiene que caber en 4096 de lado y en su franja de 64 MB: si no, el factor de esa
  ventana baja (hasta 1x). Una ventana que cambia de tamaño se mira otra vez con el tamaño nuevo.
- **La pantalla de Workbench** mide lo de la pantalla real entre el factor de la caja (1920x1080 a 2x: 960x540), con
  el tope de siempre de 1024x768.
- **Cambiar el ajuste con la ventana abierta**: las que pasan a otro factor se piden enteras (`Evento::Exponer`) y
  salen con el búfer nuevo. La pantalla de Workbench no cambia hasta que la caja vuelve a arrancar.
- La línea de la consola dice el factor cuando no es 1: "ventana Amiga en pantalla (410x192, a 2x): ...". El guion
  (`tools/prueba_uefi.sh`) lo lee y multiplica por él sus clics en GadTools, en el ejemplo de Dash y en el requester.

### El ajuste (S26, `Prefs/cajas` y `dashos_tema_v1.cajas`)

Dos líneas nuevas en el mismo texto (`wayland/src/aspecto.rs`), que el centro ya guarda tal cual al cambiar el
aspecto:

```text
escala amiga 2
escala-programa amiga 1 Directory Opus
```

`escala <caja> <auto|1|2|3>`: la de una caja (si falta, `auto`). `escala-programa <caja> <auto|1|2|3> <nombre>`: la
de un programa (el nombre de su tarea, sin camino), por encima de la de su caja. Un 0, un 4, un negativo o cualquier
otra cosa: el texto entero no vale, como lo demás de S26.

**Decisión provisional, para el usuario**: `auto` da 3x desde 3200x1800 (4K), 2x desde 1600x900 (Full HD, 1440p) y
1x por debajo (800x600, 1024x768, 1280x1024). Así lo que el programa ve como pantalla mide al menos 800x450. ¿Vale esa
regla? ¿Hace falta 4x (8K) o un 1,5x?

### Lo que falta

1. **Ningún sitio en Ajustes para cambiarlo**: hoy se escribe a mano en `Prefs/cajas` (o con `DASHOS_PREFS`). Una fila
   en Apariencia del centro (junto a "Programas de las cajas") movería las filas de los fondos que pulsa el guion, y el
   centro lo lleva ahora el agente del escritorio: mejor con él.
2. **La caja Linux** lee el mismo ajuste pero no escala (sus programas ya son HiDPI con GTK).
3. **El factor no es fraccionario** (1,5x): con vecino más cercano saldrían píxeles de dos tamaños.
