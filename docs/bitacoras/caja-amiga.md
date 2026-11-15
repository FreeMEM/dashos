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

## 2026-11-13: la caja, fuera de los 4 GB del 68k (decisión 1)

Rama `caja-amiga-4gb`, desde `revision` (267a072). Cierra el agujero 1 de S28: el ejecutable nativo de la caja estaba
en `$7000_0000`, al alcance del 68k (podía leer y escribir su `.data` y `.bss`: el estado del 68k, la tabla de la caché
del JIT, el estado de exec, dos e intuition).

### Qué se hizo

- **Parche 0014 de Emu68** (`patches/emu68/0014-direcciones-de-64-bits.patch`): las tres direcciones de funciones de C
  que el JIT cargaba con 32 bits (`M68K_Exception` en `EMIT_Exception`, y `Load96bit`/`Store96bit` en `get_Load96` y
  `get_Save96` de `M68k_LINEF.c`) van con 64 bits (`movz` y tres `movk`), solo con `DASHOS_HOST`. Las demás llamadas
  del código traducido (`dashos_native_call`, las matemáticas, `kprintf`...) ya eran de 64 bits; se buscaron todas las
  que convierten un puntero a 32 bits y no hay más que alcancen el código traducido. Los 14 parches se aplican limpios
  sobre Emu68 af63580 y dan el mismo árbol que el de `third_party/Emu68`.
- **La caja es un PIE estático** (`ET_DYN`), sin tocar el kernel: el cargador ya pone los PIE en una base al azar entre
  128 y 256 GB y su pila entre 256 y 384 GB (C29, pensado para esto). `caja/enlace.ld` la enlaza en 0 (con otra
  dirección, ld la marca `ET_EXEC` aunque sea `-pie`), con `.data.rel.ro` y `.got` entre los datos y `.rela.dyn` entre
  los de solo lectura. `caja/entrada.S` aplica sus `R_AARCH64_RELATIVE` antes de nada (13.962; otro tipo de
  reubicación acaba la caja con 70, y el Makefile no deja enlazar si sale alguna). C con `-fpie`, Rust (`pegamento/`)
  con `relocation-model=pie`. Los objetos dependen del Makefile (las opciones cambiaron).
- **Arreglo de un fallo de antes, que el PIE destapó**: `main_loop_anidado` y `arranca` (caja.c) llaman a `MainLoop`
  desde un asm con x29 en la lista de lo que se pisa, pero GCC no lo guarda (es el puntero de marco) y `MainLoop` deja
  en x29 la A7 del 68k. Con `relocation-model=pie`, Rust tenía en x29 un puntero a `CAJA` a través de `ejecutar_68k`:
  Ctrl-C a un programa parado en `Wait` (el ejemplo de GadTools de Dash, cuando el clic del guion no llega a "Cancel")
  rompía la caja (lectura en `entregar_excepciones` con FAR `0xa5fc0021100002f6`). Forzado a 1,2 s, falla siempre con
  el PIE sin el arreglo y nunca con la caja de antes. Ahora el asm guarda x29.

### Qué ve el 68k

Los 4 GB bajos: la página 0 y su memoria (4 MB en `$0010_0000`). Nada de la caja. Lo comprueba la caja tras
`memoria.exe` (`caja_fuera_del_68k`): el ejecutable (de `__caja_imagen` a `__caja_fin`), la pila del kernel, la de
ahora, el estado del 68k, el código traducido, los montones, las pilas y la página 0 para escribir, todo por encima de
4 GB; y le pregunta al kernel por los 4 GB bajos con la llamada 42 (`esperar`, que da `E_ARGUMENTO` donde no hay memoria
que leer), una página de cada 64 KB: 65.471 trozos, ninguno con algo, tampoco `$7000_0000` (medio segundo en QEMU;
página a página eran 8 s, demasiado para la prueba). `memoria.exe` pregunta antes a exec (`TypeOfMem($70000000)` = 0) y
no lee: leer ahí ahora terminaría la caja.

```
│   [caja-amiga] malware: memoria: el ejecutable nativo de la caja, antes en $70000000: nada (TypeOfMem 0; no lee)
│   [caja-amiga] malware: memoria: la caja, toda por encima de 4 GB (0 direcciones suyas por debajo); en los 4 GB del 68k, el kernel no ve más que la página 0 y la memoria del 68k (0 de 65471 trozos de 64 KB con algo de más; en $70000000, nada; 513 ms): bien
```

### Coste (QEMU TCG, PC cargado: carga 6-9)

| Prueba | Antes | Después |
|---|---|---|
| Suma de 1 a 100.000 (`hola`) | 11 ms | 11 y 12 ms |
| `saludo.exe` (7 llamadas nativas) | 6 ms | 7 y 6 ms |
| Hunk hostil (30.216 cargas) | 168 ms | 179 ms |
| Dash `texto.exe` / `texto_gcc.exe` (62 / 6.862 llamadas) | 246 / 249 ms | 248 / 250 ms |
| Dash `test_interp_comillas` / `test_lista_listas_literal` | 14 / 22 ms | 15 / 22 ms |
| Dash `ctrl_c`, `test_ctrl_c_for`, `test_trap_*` | 706, 19, 10, 13 ms | 705, 18, 9, 13 ms |

Las llamadas nativas, iguales. Dentro del ruido: la carga de 64 bits son dos instrucciones más solo donde el código
traducido entra en una excepción o en la coma flotante de 96 bits. El ejecutable lleva 335 KB más de tabla de
reubicaciones (`.rela.dyn`).

### Pruebas

- `make -C kernel prueba`: "BIEN: cargador hunk (0 fallos)" y "BIEN: esperado.txt".
- `make -C kernel arranque-virt`: las cinco BIEN (en esta vuelta, `test_gadtools` acabó por el Ctrl-C de los 6 s, el
  caso que rompía la caja antes del arreglo de x29). Antes, una vuelta con el PIE sin el arreglo falló por eso, y otra
  por "el desplegable dice: Destello" de Ajustes con la comprobación página a página (8 s más de caja, que se cruzaban
  con el guion); con una de cada 64 KB, bien.
- Ctrl-C forzado a 1,2 s en `test_gadtools` (cambio de prueba, no queda): con el PIE sin el arreglo, la caja muere
  siempre; con la caja de antes y con el arreglo, acaba con 5 y las cinco BIEN.
- `arranque-virt-vm`, `-linux`, `-2`, `-diag` (la caja de la Pi 5), `-escritorio` y `-usb`: BIEN. `make tarjeta-pi5`
  sale. `make -C jit-host run`: BIEN (también con el parche 0014). No se pasaron `-arch`, `-gpu` ni `-wayland`: no se
  tocó nada común (solo `caja/` y lo que espera la prueba de la caja).

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
4. **En el tema clásico, el clic del guion en la tercera opción del mx no llega** (ni `GADGETDOWN`): ya pasaba a 1x
   (se ve el mx en "Verde" en `caja-gadtools-menu-clasico-ondas-fhd.png` de la rama `gadtools-material`); con el tema
   Material sí llega. No es de la escala; sin mirar.

### Capturas (`capturas-agente/`, fuera de git)

Con `CAJA_INTERACTIVA=1 DASHOS_PANTALLA=1920x1080 TIEMPO_VIRT=480` y `DASHOS_PREFS="fondo=ondas.png;apariencia=claro
fondo;cajas=general dashos"` (u `original`), sin línea de escala: la automática, 2x.

- `caja-gadtools-escala-material-fhd.png` y `caja-gadtools-escala-clasico-fhd.png`: la ventana de GadTools a 2x (820x384
  en la pantalla), entera, antes de los clics.
- `caja-gadtools-menu-escala-material-fhd.png` y `caja-gadtools-menu-escala-clasico-fhd.png`: con el menú Proyecto
  abierto con el botón derecho y "Salir" señalado, tras los clics del guion (ciclo en "Dos", "Amiga 4000" en la lista,
  "Aceptado"; con Material, el mx en "Azul"). Al soltar llega `MENUPICK` y el programa acaba con 0:

```
│   [caja-amiga] ventana Amiga en pantalla (410x192, a 2x): GadTools en DashOS
│   [compositor] ventana de caja-amiga en (40, 80) de 820x384
│   [caja-amiga] gadtools: GADGETUP del gadget 2, codigo 0
│   [caja-amiga] gadtools: GADGETUP del gadget 3, codigo 1
│   [caja-amiga] gadtools: GADGETDOWN del gadget 4, codigo 2
│   [caja-amiga] gadtools: GADGETUP del gadget 10, codigo 2
│   [caja-amiga] gadtools: GADGETUP del gadget 0, codigo 0
│   [caja-amiga] gadtools: MENUPICK menu 0 elemento 2 (2)
│   [caja-amiga] gadtools.exe acabó con 0 (cerrada por el programa), 1286 llamadas nativas, en 16344 ms, 0 avisos, 0 bloques sin soltar: bien
```

El guion de las capturas, además: no mira ni pulsa la ventana de GadTools con la sesión bloqueada (la captura salía de
la pantalla de bloqueo), y reconoce la casilla Material en (100, 21), dentro de la casilla y fuera de la marca blanca
(en (103, 25), donde pulsa, ya está la marca: con ella no la daba por vista).

### Pruebas

- `cargo test`: `wayland` (92, con `aspecto::escalas` y las hostiles nuevas: 0, 4, negativos, números enormes, "2x",
  líneas repetidas, nombres malos, demasiadas excepciones, y la basura al azar con las palabras nuevas) e intuition
  (con `escala`: el factor que se baja si no cabe, coordenadas en el borde, fuera y negativas, la copia por vecino más
  cercano a 1x, 2x y 3x, solo lo dañado, daños fuera o enormes, búferes que no cuadran como al cambiar de tamaño entre
  medias, factor 0 y enorme). clippy limpio en lo nuevo.
- `make prueba`: BIEN esperado.txt y BIEN cargador hunk.
- `make arranque-virt` (800x600: factor 1, las líneas de siempre sin cambios): las cinco BIEN, también con el guion
  final (una vuelta dio MAL en la captura del compositor, la de `ventana-a` y `ventana-b`, sin ventanas Amiga, con
  otra QEMU de otro agente a la vez; repetida, BIEN). `-vm`, `-2`, `-escritorio` y `-diag`: BIEN.

## 2026-11-13: la caja Amiga integrada (barra de DashOS y letra del kit)

Rama `amiga-integrada`, desde `revision` (86d7e2e). El usuario vio GadTools con el tema Material a 2x en Full HD y
dijo: "no me gusta cómo se ven las fuentes ni la barra de la ventana; no se ven integradas con el entorno de
escritorio; debería ser más Material Design". Con "como DashOS", la barra y la letra pasan a ser las de DashOS.
"Original" no cambia.

### La barra

- **La pide la caja al compositor** (`caja/pegamento/src/compositor.rs`): `zxdg_decoration_manager_v1`, modo del
  servidor, antes del primer commit, como una ventana de Linux que la pide. Sale la barra de siempre (`wayland/src/
  decoracion.rs`): título, maximizar, cerrar, las cuatro esquinas redondeadas y la sombra. Solo si la ventana va "como
  DashOS", tiene barra (título, `DRAGBAR`, cerrar o profundidad) y no es `BORDERLESS` ni `BACKDROP`; si el compositor no
  tiene decoraciones (`Compositor::pone_barra`), como antes.
- **Intuition no pinta barra ni bordes.** La geometría que ven los programas, coherente con 3.x: la pantalla dice
  `WBorTop`, `WBorLeft`, `WBorRight` y `WBorBottom` = 0, y una ventana con barra lleva `BorderTop` = `WBorTop` + alto de
  la letra + 1 = 9 (topaz/8) y 0 en los demás lados; una sin barra, 0 en todo. Así los que colocan sus cosas con
  `BorderTop`, con `GimmeZeroZero` (la capa empieza en (0, 9)) o con la receta de los RKM `WBorTop + ta_YSize + 1` caen
  en el mismo sitio, a 9 de arriba. Los que colocan con números fijos (como `gadtools.c`: el primer gadget en y = 20)
  quedan donde siempre: no se mueve nada, solo desaparece la barra de dentro. La franja de 9 es del color de dentro.
  `WindowLimits` y el tamaño mínimo cuentan con los bordes nuevos. La pantalla va con el aspecto de la caja (sin
  excepciones: es de todos) y cambia con el ajuste; una ventana ya abierta no cambia de bordes nunca (el programa ya
  colocó sus cosas): si se pasa a "original", sigue con la barra de DashOS y dentro clásico; si una "original" pasa a
  "como DashOS", sigue con su barra de intuition, pintada como Material.
- **Los gadgets del sistema**: cerrar de la barra → `xdg_toplevel.close` → `IDCMP_CLOSEWINDOW` (en un EasyRequest,
  como el botón de la derecha: 0). Cambiar el tamaño desde el borde de DashOS o maximizar → `xdg_toplevel.configure` con
  tamaño, que la caja **no atendía** (el gadget de tamaño pedía `xdg_toplevel.resize`, pero el tamaño nuevo no llegaba
  nunca): ahora pasa a `Evento::Tamano`, entre el factor de la escala, e intuition lo ajusta a `WindowLimits` y manda
  `NEWSIZE`, `REFRESHWINDOW` y `CHANGEWINDOW`. Profundidad: la ventana sube al pulsarla, como todas las de DashOS; la
  barra no tiene botón de "al fondo" (pendiente, con el zoom de `WA_Zoom`). Una ventana que no se puede agrandar sigue
  con el botón de maximizar: el compositor la lleva a la esquina y la caja no cambia su tamaño (el compositor no tiene
  `set_max_size`; pendiente).
- **La marca de caja (S29)** también para la caja Amiga (`wayland/src/marca.rs`, `escena.de_caja_amiga`, la pone el
  compositor por el nombre "caja-amiga" que da `inicio`): borde y pestaña azules (`00 55 aa`, el del Workbench) con
  "AMIGA" en blanco, y "Amiga · " delante del título en la barra de la pantalla. Un programa Amiga "como DashOS" se ve
  como una ventana del sistema, así que la razón de S29 vale igual. Si al usuario le parece que rompe la integración,
  es una línea en el compositor.
- La caja dice ", con la barra de DashOS" en la línea "ventana Amiga en pantalla"; el guion (`tools/prueba_uefi.sh`)
  la lee y cierra `ventana.exe` y la ventana de Dash con el botón de cerrar de la barra (40 de alto, a 20 del borde
  derecho), y reconoce la ventana de GadTools por sus esquinas de la superficie y la barra encima. Los clics de GadTools
  no cambian (sus gadgets están donde estaban).

### La letra

`caja/intuition/src/nitido.rs`. Intuition sigue pintando topaz a 1x (lo que lee el programa y lo que se ve si algo
falla). Cada texto que pintan nuestras bibliotecas en una ventana "como DashOS" (`gadtools::texto_n`) se apunta en el
lienzo como **texto nítido**: el hueco de la topaz, lo de debajo antes de las letras y lo que quedó después. Al ampliar
(`presentar`), la caja mira cada texto que toca lo dañado: si su hueco sigue igual que al pintarlo, amplía el fondo de
antes (sin la topaz) y escribe encima con Roboto del kit a la resolución de la pantalla (antialiasada); si no (el
programa pintó encima, un menú lo tapa), sale tal cual. Al cerrarse el menú vuelve lo de antes y vuelve la letra.

- **Encaje**: la letra mide 7/8 del alto del hueco (14 a 2x, 21 a 3x), centrada en vertical por la altura de las
  mayúsculas y lo que baja la g. A lo ancho, Roboto es más estrecha que topaz casi siempre, así que se alinea como el
  texto: las etiquetas a la izquierda de su gadget, contra el gadget; dentro, encima y debajo, centradas; los menús,
  listas, ciclos y textos, a la izquierda; el nivel de un deslizador, a la derecha. Si no cabe, se reduce hasta 3/4 y
  después se recorta con "…". Se escribe en un búfer del tamaño del hueco: nunca se sale. Si la topaz estaba cortada
  (`hasta`), el hueco llega hasta ese límite y Roboto, más estrecha, enseña más del texto.
- **Botones** con Roboto Medium (Material: "label large"). **Campos de texto** con Roboto Mono, una letra en cada
  celda de topaz: el cursor (que ahora se pinta antes que las letras y queda en el fondo) y los clics caen en la misma
  letra que con topaz.
- **Desactivados**: el aclarado de Material pasa por `Lienzo::filtrar`, que aclara también el fondo y el color de los
  textos que tapa, y siguen valiendo.
- **A 1x, topaz**: el hueco mide 8 de alto y Roboto a ese tamaño (unos 6 de mayúscula) se lee peor que topaz, que está
  dibujada para eso. Los textos se apuntan igual, pero no se usan.
- **Lo que pinta el programa con `Text()` sigue en topaz.** Pendiente: una opción por programa para sus `Text()` (con
  el mismo mecanismo: el hueco es el de `Text()`, pero cambia la anchura que el programa mide con `TextLength`).
- Topes: 512 textos por ventana (al pasar, fuera los que ya no valen y después los más viejos; uno en el mismo hueco
  sustituye al de antes, así los menús, que se repintan a cada movimiento, no acumulan), 256 letras por texto. Lo que
  la letra no tiene sale como "?" (la caché de letras del kit no crece sin fin). Al cambiar de tema, se olvidan y se
  apuntan otra vez al repintar.
- Coste en la caja: el ejecutable pasa de 2,5 a 3,1 MB (frente al de `caja-amiga-escala`: Roboto, Roboto Medium y
  Roboto Mono son 185 KB; el resto, el lector de TTF y el rasterizador del kit con sus reubicaciones); cada letra se rasteriza una vez
  por tamaño (sin FPU, con la coma flotante por software del kit) y se guarda. Un texto que vale se compara con su copia
  cada vez que se manda lo que lo toca.

### Los controles a la resolución de la pantalla (no hecho)

Los controles Material (píldoras, aros, casillas) siguen pintándose a 1x y ampliándose: a 2x se ven los escalones de 2
píxeles en las curvas. Hacerlos nítidos pide pintar el gadget otra vez a `f` veces (el tema Material ya trabaja en
octavos de píxel con `forma` y `trazo`, así que se podría con un lienzo a escala) y guardarlo como un parche con la
misma comprobación que los textos; pero intuition tendría que saber el factor de cada ventana y el pintor, pintar en
dos lienzos. Apuntado para otra vuelta.

### Pruebas

- `cargo test` de `caja/intuition` (78): `nitido` (encaje, reducir, elipsis, huecos de 0 y negativos, textos enormes,
  letras que no hay, componer solo lo que vale, instantáneas que no cuadran, destinos cortos, celdas con más letras que
  celdas, control y Latin-1), `tests/barra_dashos.rs` (bordes y `WBor*`, sin gadgets del sistema dentro, cerrar,
  `NEWSIZE` dentro de `WindowLimits` con tamaños absurdos, `GimmeZeroZero`, ventanas sin barra, `BORDERLESS` y
  `BACKDROP`, el programa con la excepción "original", un compositor sin barra, cambiar de tema con la ventana abierta
  ida y vuelta, textos que dejan de valer al pintar encima y vuelven al repintar, la etiqueta de fuera repintada sin
  borrar, etiquetas enormes y gadgets pegados al borde, 300 repintados y cambios de tamaño con el tope de textos, el
  EasyRequest cerrado desde la barra) y `tests/letra_kit.rs` (con Roboto de verdad a 2x y 3x: antialiasada, dentro de
  su hueco, y fuera de los huecos igual que la ampliación de siempre). `wayland` (95, con `marca_de_la_caja_amiga`).
- `make prueba`: BIEN cargador hunk y BIEN esperado.txt. `make arranque-virt` (800x600, a 1x, con la barra de DashOS:
  `ventana.exe` se cierra con el botón de la barra y da `IDCMP_CLOSEWINDOW`; lo mismo la de Dash): las cinco BIEN.
  `-vm`, `-2`, `-escritorio` y `-diag`: BIEN. Todo otra vez tras fusionar `fusion-fondo` (esquinas a 8,
  `RADIO_MARCO`, que es lo que usa la barra de las ventanas Amiga: la caja no lleva ningún radio propio).
- Un fallo que salió en la primera captura y no en las pruebas: las etiquetas de fuera de un gadget (a la izquierda,
  encima) se repintan sin borrar y su texto nítido tomaba como fondo la topaz de antes (se veían las dos letras). Ahora
  el fondo de un texto que sigue valiendo en el mismo hueco se conserva; con su prueba.

### Capturas (`capturas-agente/`, fuera de git)

Con `CAJA_INTERACTIVA=1 DASHOS_PANTALLA=1920x1080 TIEMPO_VIRT=480` y `DASHOS_PREFS="fondo=ondas.png;apariencia=claro
fondo;cajas=general dashos"` (u `original`), a 2x, tras fusionar `fusion-fondo` (esquinas a 8):

- `antes-caja-gadtools-menu-material-fhd.png`: la de antes (rama `caja-amiga-escala`): barra de intuition pintada como
  Material y topaz ampliada.
- `caja-gadtools-menu-integrada-fhd.png`: la misma ventana ahora, con el menú Proyecto abierto y "Salir" señalado, al
  lado de la ventana del centro (nativa). En pantallas grandes, el guion la arrastra por su barra a la derecha de
  Ajustes antes de los clics (en la cascada salían una encima de la otra). `caja-gadtools-integrada-fhd.png`: antes de
  los clics.
- `caja-gadtools-menu-original-fhd.png`: con "original" (3.x, topaz, su barra de intuition).
- `comparacion-original-dashos-fhd.png`: las dos ventanas con el menú abierto, lado a lado, con su rótulo.

Lo que se ve y queda por hacer:

- Con el menú abierto, las etiquetas que el menú tapa en parte ("Ciclo" y el texto del ciclo, debajo del menú) salen en
  topaz hasta que se cierra: su hueco ya no es el de antes. Se podría dar por bueno el hueco si lo que cambió es solo
  lo que tapa el menú (intuition sabe dónde está), pero no está hecho.
- "Nivel" sale en topaz y cortado: el nivel del deslizador ("25") se pinta encima de la etiqueta, como en 3.1; el
  hueco cambió.
- La marca azul de la caja sale en las dos (va por caja, no por aspecto).
- Los controles, a 1x ampliados (escalones de 2 en las curvas): ver arriba.
- El guion mira las esquinas de la ventana de GadTools a 3 del borde (la marca ocupa los 3 de fuera; con el clásico, la
  primera vuelta no la reconocía y no pulsaba nada).

## 2026-11-13: BOOPSI y ReAction en la caja Amiga

Rama `caja-amiga-reaction` (desde `fusion-fondo`, 77d8e47). intuition tiene BOOPSI nativo y, encima, las clases de
ReAction de 3.2, con las dos apariencias por el rasgo `Tema` (las mismas reglas que GadTools). Escrito desde el RKRM
(Libraries, "BOOPSI"), los autodocs de intuition, amiga.lib y de cada clase de ReAction del NDK 3.2 y sus includes; sin
leer código de ninguna implementación.

### Cómo va (`caja/intuition/src/boopsi/`)

- **Las clases son `struct IClass` de verdad** en la memoria del 68k, con su gancho despachador: `DoMethod` de
  amiga.lib salta al `h_Entry` de la clase sin pasar por intuition (y `DoSuperMethod`, al de la superclase). Las
  nativas comparten un `h_Entry`: 6 bytes de 68k con `$A0FF n; RTS` (n = 0xB00, `Numeros::boopsi`) que entran en el
  despachador nativo con la clase en A0. Una clase de usuario (`MakeClass`) lleva su despachador en 68k; los objetos
  son los de siempre: `struct _Object` delante, `OCLASS(o)` en `o - 4`, `INST_DATA` por `cl_InstOffset`, un gadget es
  su `struct Gadget` (ExtGadget) y una imagen su `struct Image`. La memoria es de la tarea (`AllocMem`): lo que un
  programa no dispone cuenta como suyo sin soltar.
- **Llamar a 68k desde lo nativo** (un `OM_NEW` a una clase de usuario, el `OM_UPDATE` de una interconexión a uno de
  sus objetos, el gancho de edición de un campo): el rasgo `Puente` (`partes()` y `ejecutar_68k`). Nada de BOOPSI es un
  método de `CajaGrafica` con `&mut self`: todo recibe el puente, pide `partes()` cuando lo necesita y suelta la
  referencia antes de ejecutar 68k (lo asegura el comprobador de préstamos: las dos cosas piden `&mut` al puente). En
  la caja, `PuenteCaja` (pegamento) pide `caja()` en cada `partes()` y ejecuta con `caja_ejecutar_68k`, con la pila de
  la llamada nativa en curso. El 68k de dentro puede volver a intuition (`DoSuperMethodA`): es otra llamada nativa,
  con su propio puente.
- **Lo que llega por eventos** (un clic en un gadget BOOPSI, repintar uno tras `RefreshGList` o el cambio de tema,
  colocar los relativos tras `NEWSIZE`) no se puede hacer en `CajaGrafica::atender`, que tiene `&mut self`: va a una
  cola (`Pendiente`) y la caja la atiende después con el puente (`boopsi::atender`): al final de cada llamada nativa
  que atiende al compositor, en `Wait` (`Espera::Boopsi`: se atiende y la espera se repite) y en `Delay`/`WaitTOF`.
  Mientras haya un gadget BOOPSI activo o un evento suyo en la cola, el ratón y el teclado de esa ventana van a la
  cola (si no, un clic y su soltar en la misma vuelta se separaban).
- **La entrada**: un clic sobre un gadget BOOPSI da `GM_HITTEST` y `GM_GOACTIVE` (con su `InputEvent` y el ratón
  relativo al gadget); mientras devuelva `GMR_MEACTIVE`, `GM_HANDLEINPUT` con cada evento; al acabar, `GM_GOINACTIVE` y,
  con `GMR_VERIFY`, `IDCMP_GADGETUP` con el código de terminación. `GMR_REUSE` pasa el clic a lo de debajo. Un hijo de
  un layout o un grupo que acaba deja su dirección (`hoja`) y el `GADGETUP` lleva la del hijo: window.class lo
  traduce a `WMHI_GADGETUP | GA_ID`.
- **Lo malo**: una clase o un objeto que no lo son, un método desconocido, etiquetas en círculo, un almacén que no se
  puede escribir: aviso y 0. Un despachador 68k que no vuelve (el programa acaba dentro): `Corte68k::Acabado` deshace
  todo hasta arriba y la caja corta el programa. Uno que se llama a sí mismo por intuition sin fin: tope de 48 métodos
  anidados (`MAX_NIVELES`); en 68k puro, el tope de 16 niveles de la caja (sigue con 0). Interconexiones en círculo:
  cada objeto, mientras notifica, no atiende más notificaciones (lo de `ICM_SETLOOP`/`ICM_CHECKLOOP`). Un árbol de
  layouts muy hondo se dispone sin recursión.

### Clases de sistema (`sistema.rs`)

rootclass (OM_NEW, OM_DISPOSE, OM_ADDTAIL, OM_REMOVE), icclass y modelclass (ICA_TARGET, ICA_MAP con
`TAG_IGNORE`, ICTARGET_IDCMP con la copia de las etiquetas en el `IAddress` de `IDCMP_IDCMPUPDATE`, que se suelta al
contestar; los miembros de modelclass reciben `OM_UPDATE` y se disponen con él), imageclass (la `struct Image`,
IM_DRAW/DRAWFRAME/HITTEST/HITFRAME/ERASE/FRAMEBOX/DOMAINFRAME), frameiclass (con el tema si dibuja en una ventana
nuestra: tarjeta con contorno "como DashOS"), sysiclass (flechas, casilla, mx, cerrar, profundidad, tamaño),
fillrectclass, itexticlass, gadgetclass (todos los `GA_*` de posición, relativos, activación, texto, imagen,
`GA_Previous`), propgclass (mando, página, arrastre, `PGA_Top`/`Total`/`Visible` y los pots), strgclass (su
`StringInfo` y búfer, `STRINGA_TextVal`/`LongVal`, edición con el teclado, y `STRINGA_EditHook`: el gancho en 68k con su
`SGWork`, `SGH_KEY`), buttongclass y frbuttonclass (con `GA_ToggleSelect`), groupgclass (miembros colocados respecto al
grupo, el grupo crece, reenvía render y entrada).

Lo que dijo el oráculo (FS-UAE con el Kickstart 3.2.3 y el Workbench 3.2, `caja/pruebas68k/boopsi.c`, sin cerrojo):
propgclass **no notifica** lo que le llega por `OM_SET`, `OM_UPDATE` ni `SetGadgetAttrs` (solo lo que mueve el
usuario): así se hizo. icclass y modelclass sí pasan lo que les llega. En 3.2, `NewObject(NULL, "layout.gadget")` (y
checkbox, string, label, slider) da NULL: esas clases no están en la lista pública con su nombre; `button.gadget` sí.
Aquí todas son públicas (más permisivo: lo que va en 3.2 va aquí); los programas de 3.2 usan `XXX_GetClass()`.

### ReAction (`reaction.rs`)

Las bibliotecas `window.class`, `layout.gadget`, `button.gadget`, `checkbox.gadget`, `chooser.gadget`,
`integer.gadget`, `string.gadget`, `slider.gadget`, `scroller.gadget`, `listbrowser.gadget`, `radiobutton.gadget`,
`space.gadget`, `getfile.gadget`, `label.image`, `bevel.image` y `glyph.image` (versión 47), con sus funciones: los
`GetClass`, los nodos de chooser, listbrowser y radiobutton (`Alloc/Free/Set/GetXNodeAttrsA`, `FreeListBrowserList`,
`ListBrowserClearAll`, `ListBrowserSelectAll`) y `RethinkLayout` y `LayoutLimits` de layout. exec abre
`gadgets/layout.gadget` por el final del camino (lo que sería `LIBS:Gadgets/` con ramlib).

- **window.class**: `WA_*` guardadas, `WINDOW_Layout`, `WINDOW_Position` (centrada), `WINDOW_MenuStrip`,
  `WINDOW_SigMask`, `WINDOW_Window`; `WM_OPEN` abre del tamaño pedido y, como mínimo, del que pide el layout
  (`GM_DOMAIN`), con `WindowLimits` a ese mínimo; el layout va como gadget de la ventana en todo lo de dentro, con
  `GFLG_RELSPECIAL`; `WM_HANDLEINPUT` traduce los mensajes (`WMHI_CLOSEWINDOW`, `GADGETUP | GA_ID` con el código,
  `MENUPICK`, `NEWSIZE` recolocando, `RAWKEY`, `VANILLAKEY`, `ACTIVE`, `INACTIVE`, `IDCMPUPDATE` con `GA_ID`) hasta
  `WMHI_LASTMSG`; `WM_CLOSE`, `WM_ICONIFY` (cierra), `WM_RETHINK`; `OM_DISPOSE` cierra y dispone el layout.
- **layout.gadget**: `LAYOUT_AddChild`/`AddImage` con los `CHILD_*` que los siguen (pesos, mínimos, máximos,
  `CHILD_Label`, `CHILD_NoDispose`, `CHILD_ReplaceObject`), `LAYOUT_RemoveChild`, orientación, `SpaceOuter`,
  `SpaceInner`, márgenes y espacio, alineación, `EvenSize`, `BevelStyle` y `Label` (el título del grupo). El diseño:
  el mínimo de cada hijo (`GM_DOMAIN` / `IM_DOMAINFRAME`, recursivo), lo que sobra repartido por pesos sin pasar de
  los máximos, el otro eje entero (o su mínimo, centrado, con peso 0), la columna de etiquetas en vertical. Con la
  ventana abierta, añadir un hijo la recoloca entera. La entrada va al hijo de debajo (con su `GM_HITTEST`).
- **Los controles se pintan como los de GadTools** (`pintar.rs`): botón, casilla (de su tamaño, a la izquierda, con su
  texto), chooser (un ciclo), integer y string (campo), slider y scroller (deslizador), listbrowser (listview),
  radiobutton (mx), getfile (texto y botón). Así salen las dos apariencias y la letra nítida sin nada propio, en los
  mismos rectángulos. Las imágenes: label (texto con la tinta del tema, sin el `_`), bevel (grupo, campo, botón,
  raya), glyph (flechas, marca).

Diferencias sabidas: chooser cambia de opción con cada clic (como un ciclo de GadTools) en vez de abrir su menú;
getfile no abre el requester (no hay asl.library); listbrowser con varias columnas las junta en una; space no llama a
`SPACE_RenderHook`; `LAYOUT_FixedHoriz`, `ShrinkWrap` y las páginas (`PAGE_GetClass`), sin hacer.

### De paso: dos fallos de GadTools "como DashOS" (vistos en la captura Full HD de GadTools)

- **Los campos de texto salían espaciados** ("h o l a", "4 2"): con `Estilo::Celdas` cada letra de Roboto Mono iba
  centrada en su celda de topaz (8 x f), y el avance de Mono a 7/8 del alto es casi la mitad. Ahora van seguidas, con
  su avance (`nitido::escribir`). El cursor ya no puede ir en la celda de topaz del fondo: intuition lo pinta de 1 de
  ancho después de las letras (a 1x, en su celda, como antes) y el texto nítido lo lleva (`TextoNitido::cursor`, con
  `Lienzo::cursor_del_ultimo`, que copia otra vez lo de después) y lo pinta delante de su letra. Queda: un clic en el
  campo pone el cursor por celdas de topaz, así que en textos largos cae unas letras más allá de donde se ve.
- **La etiqueta de la izquierda de un deslizador con nivel** ("Nivel" con `GTSL_LevelFormat`) salía en topaz: el campo
  del nivel la pisaba (como en 3.1, "Niv25") y su hueco ya no era el de antes. Con el tema de DashOS va a la izquierda
  del campo del nivel (`etiqueta_desde`); el clásico sigue como 3.1. Es lo único que se mueve respecto al clásico (la
  prueba `mismo_rectangulo_que_el_clasico` lo admite solo para ese caso).
- Prueba: `tests/letra_kit.rs`, `campo_de_texto_seguido_y_etiqueta_del_deslizador_nitida`: con Roboto de verdad a 2x y
  3x, mide lo que ocupa la tinta de "hola" (no más que 4 avances de Mono, menos de 3 celdas de topaz), el cursor delante
  de la "l" y la etiqueta y el nivel del deslizador valiendo los dos, sin pisarse.

### De paso: la barra de intuition aplastada en el aspecto original

Aviso del coordinador: con "cajas=general original" a 2x, la barra de intuition de la ventana de GadTools salía
aplastada (título cortado por arriba, gadget de profundidad pequeño), como si el `BorderTop` 9 y los `WBor*` 0 de "como
DashOS" valieran también ahí. El oráculo (FS-UAE, Kickstart y Workbench 3.1, un programa que lee la pantalla y abre una
ventana con todos los gadgets): `WBorTop` 2, `WBorLeft` 4, `WBorRight` 4, `WBorBottom` 2, letra de la pantalla
topaz/9 y `BorderTop` 12 (= 2 + 9 + 1), 4, 18 y 2; los gadgets de la barra, de 24 x 12 (cerrar 20 x 12). La caja, con
topaz/8, ya hacía la misma cuenta: `WBorTop` 2 y `BorderTop` 11 en el original, y 0 y 9 "como DashOS". La prueba
`border_top_en_los_dos_aspectos` (`tests/barra_dashos.rs`) lo comprueba en los dos aspectos (los bordes, el título dentro
de la barra, el gadget de profundidad hasta la última fila de la barra y el hueco de la marca).

**Lo que lo aplastaba era la marca del compositor** (`capturas-agente/caja-gadtools-dash-original-antes-fhd.png`): la
tira azul de arriba (3 píxeles de la pantalla, por dentro de la ventana) tapaba la primera fila y media de la barra a 2x
(el relieve y lo de arriba del título) y parte de los gadgets. Ahora, en una ventana de la caja Amiga con barra de
intuition (la caja dijo su hueco, ver abajo), el compositor no pinta esa tira: la barra de intuition ya hace de borde de
arriba, y la pestaña va dentro de ella. Las tiras de los lados y de abajo siguen. Con su prueba en
`wayland/src/tests_s29.rs` (y la tira, cuando se quita o vuelve, se daña para que se repinte).

Queda apuntado un caso que sí descuadra en intuition: un programa con la excepción "original" en una caja "como DashOS"
ve `WBorTop` 0 (la pantalla es de todos) y su ventana lleva `BorderTop` 11; si coloca sus gadgets con `WBorTop +
ta_YSize + 1` (9), pisan 2 filas de la barra.

### De paso: la pestaña "68K" a la derecha (aspecto original)

Encargo del usuario: con el aspecto original, la pestaña "68K" del compositor (por dentro, arriba a la izquierda;
desde 130bfcb una pastilla del kit que cuelga del borde) tapaba el gadget de cerrar de la ventana Amiga. Ahora va
arriba a la derecha, dentro de la barra de intuition, justo a la izquierda del gadget de profundidad. Fusionado
130bfcb antes de tocar `wayland/`.

- **Protocolo**: `dashos_tema_v1` pasa a la versión 6 con la petición `marca(superficie, derecha, alto)`: desde la
  derecha de la ventana, dónde empiezan los gadgets de su barra, y el alto de la barra, en píxeles de la superficie. Solo
  vale para una superficie del mismo cliente (otra, o una que no existe, no hace nada); se guarda en la superficie
  (`Superficie::hueco_marca`) y se va con ella.
- **La caja**: intuition dice el hueco al abrir una ventana sin la barra de DashOS que tenga barra
  (`NuevaVentana::hueco_marca`: el ancho del gadget de profundidad y `BorderTop`, a 1x); el pegamento lo manda con el
  factor de la ventana (y otra vez si cambia la escala).
- **El compositor** (`marca::sitio_pestana`): la sigue pintando él y solo usa el dato para colocarla: a `derecha` + 6 del
  borde derecho, siempre dentro de la ventana; sin dato o con uno absurdo (negativo, más ancho que la ventana), arriba a
  la derecha con un margen fijo (56). "Como DashOS" no cambia (la pastilla va en la barra de DashOS), ni las ventanas de
  Linux sin barra (a la izquierda, como antes).
- Prueba: `wayland/src/tests_s29.rs`, `marca_de_la_caja_amiga`: a la derecha sin dato, junto a los gadgets con el dato
  por el protocolo, y huecos negativos, enormes, `i32::MIN`/`MAX`, del ancho de la ventana y de otra superficie: la
  pestaña sigue entera en su ventana; una ventana más estrecha que la pestaña, a la izquierda dentro. En intuition, el
  hueco que se pide en los dos aspectos (`border_top_en_los_dos_aspectos`).

### Programas que corren

- **`caja/pruebas68k/boopsi.c`** (m68k-amigaos-gcc, sin amiga.lib): la clase "contador" (subclase de gadgetclass con su
  despachador en 68k, `DoSuperMethodA` por `CallHookPkt`, un atributo y un método propios y `GM_RENDER` que pinta con
  graphics), interconexión (propgclass, strgclass, icclass, modelclass), y una ventana de window.class con su layout
  (etiqueta, campo, casilla, deslizador y botón por los `GetClass`). 43 comprobaciones: **0 fallos en FS-UAE con el
  Kickstart 3.2.3 y el Workbench 3.2** (el oráculo, sin cerrojo) **y 0 en la caja** (`boopsi.exe acabó con 0, 136
  llamadas nativas, 7 despachadores 68k, 0 avisos, 0 bloques sin soltar`).
- **`tests/test_doc_reaction.dash`** de FreeMEMLang (con `./dash`, directo a hunk): "ventana 1", "nombre: Amiga, sonido:
  1", "fin", con 0 y sin bloques sin soltar (lo mismo que espera FreeMEMLang en el emulador).
- **`examples/test_reaction.dash`**: abre su ventana de ReAction Demo (300x200) y espera; la prueba la cierra como con
  el botón de la barra a los 4 s y acaba con 0. Ctrl-C no vale para esto: Dash solo lo mira en sus bucles y el programa
  está en `Wait` con la máscara de su ventana (igual que en un Amiga).
- Los tres, en `make arranque-virt` (líneas nuevas en `kernel/pruebas/arranque_virt.txt`).

### Capturas (`capturas-agente/`, fuera de git)

Con `CAJA_INTERACTIVA=1 DASHOS_PANTALLA=1920x1080 TIEMPO_VIRT=750` (750: con 480 el guion acababa antes de llegar a
ReAction) y `DASHOS_PREFS="fondo=ondas.png;apariencia=claro fondo;cajas=general original\nprograma amiga dashos
boopsi-20"`: la caja corre boopsi.exe dos veces en ese modo, "boopsi-20" (como DashOS, por la excepción) y
"boopsi-orig-20" (original), cada una 20 s; el guion (`tools/prueba_uefi.sh`, `CAPTURA_RA`) pone cada ventana delante
con un clic dentro y la captura, con su sitio en un `.txt`. BOOPSI va ahora antes que GadTools en la prueba de la caja:
con GadTools esperando 90 s, la sesión se bloqueaba antes.

- `comparacion-reaction-fhd.png`: la ventana de ReAction de boopsi.c, original y como DashOS, lado a lado con su rótulo
  (a 2x). `caja-reaction-original-fhd.png` y `caja-reaction-dashos-fhd.png`: las pantallas enteras.
- `caja-gadtools-original-fhd.png`, `caja-gadtools-menu-original-fhd.png` (original) y `caja-gadtools-dashos-fhd.png`,
  `caja-gadtools-menu-dashos-fhd.png` (de serie): GadTools en los dos aspectos, con la pestaña "68K" a la derecha en el
  original y, como DashOS, los campos de texto seguidos y "Nivel" nítido.
- `comparacion-gadtools-fhd.png`: la ventana de GadTools en los dos aspectos, lado a lado con su rótulo.
- `caja-gadtools-dash-original-antes-fhd.png` y `caja-gadtools-dash-original-fhd.png`: el ejemplo de GadTools de Dash
  en el original antes y después de quitar la tira de arriba de la marca (antes, la tira azul tapaba la fila de luz de
  la barra y el título quedaba pegado arriba, como cortado).
- El oráculo de FS-UAE no dio captura útil de la ventana de ReAction: `emu_run.py --screenshot` saca la pantalla de
  delante, y era la de la consola de AmigaDOS.

### Pruebas

- `cargo test`: intuition 98 (nuevas: `tests/boopsi.rs` 13, `tests/reaction.rs` 5, `letra_kit` 1, `barra_dashos` 1;
  con hostiles: clases y objetos que no existen, métodos desconocidos, etiquetas en círculo, almacenes y mensajes
  ilegibles, despachadores 68k que no vuelven, que se llaman sin fin por intuition o en 68k puro, `h_Entry` nulo o
  impar, círculos de interconexión de dos y de tres, `ICA_TARGET` a la basura, 3000 miembros de tamaños absurdos,
  layouts con hijos que no son objetos, 2000 hijos, 100 niveles de layouts, ventanas de 100000 x 0xFFFFFFFF, la tarea
  que muere con un clic en la cola), exec 113 (`open_library_con_camino`), wayland 96. Las pruebas de BOOPSI usan un
  puente de prueba cuyos "despachadores 68k" son funciones de Rust que hacen lo del programa (`tests/comun/puente.rs`).
  clippy limpio en lo nuevo.
- `make prueba`: BIEN cargador hunk y BIEN esperado.txt. (En dos vueltas anteriores, G1, libwayland por el puente de la
  caja Linux, dio MAL con los mismos colores; nada de lo suyo cambió en esta rama, y en la última vuelta pasó.)
- `make arranque-virt` (las seis BIEN, con boopsi.exe, test_doc_reaction y test_reaction), `-vm`, `-2`, `-escritorio`
  y `-diag`: BIEN, tras lo último (la marca y el orden de BOOPSI antes de GadTools).
- El oráculo: boopsi.c en FS-UAE con 3.2 (0 fallos) y un programa de bordes con 3.1 (los números de arriba).

### Lo que falta

- chooser.gadget abre su menú en 3.2; aquí cambia de opción con cada clic. getfile.gadget no abre el requester (no hay
  asl.library). listbrowser con columnas las junta en una. space.gadget no llama a `SPACE_RenderHook` (el lienzo de
  `ra_add_canvas` de Dash). Sin páginas (`PAGE_GetClass`), `LAYOUT_ShrinkWrap`, `FixedHoriz`/`Vert`, el foco con Tab
  (`GMR_NEXTACTIVE` se devuelve pero no activa el siguiente) ni las teclas de los gadgets (`GA_ActivateKey`).
- Los gadgets BOOPSI no reciben `IECLASS_TIMER` (los que repiten mientras se pulsa, como las flechas de un scroller).
- Un clic en un campo de texto "como DashOS" coloca el cursor por celdas de topaz: en textos largos cae unas letras más
  allá de donde se ve (ahora la letra va seguida).
- La excepción "original" en una caja "como DashOS": la pantalla dice `WBorTop` 0 y la ventana lleva `BorderTop` 11.
- El oráculo visual de ReAction en FS-UAE (la captura sale de la pantalla de la consola).

## 2026-11-13: bibliotecas de disco (ramlib) y el AmigaOS del usuario

Rama `caja-amiga-libs`, desde `fusion-fondo` (d28660d). La meta: ver Tachyon 68k (MUI 5) dentro de la caja. Plan del
coordinador, por pasos: ramlib, el AmigaOS del usuario como capa, apuntar lo que falta, Tachyon. Diseño: S4, apartado 6
y recomendación 6 (cargar del AmigaOS del usuario lo que aún no es nativo, nunca lo que toca el chipset).

### Paso 1: ramlib

- **exec** (`caja/exec/src/ramlib.rs`): un `OpenLibrary` u `OpenDevice` de algo que no está en memoria pide a la caja
  que lo cargue (`poner_ramlib`: un número nativo para cargar y otro para soltar; `Accion::Llamar` con su
  continuación). Con la lista de segmentos, exec busca el `RomTag` (un `RTC_MATCHWORD` cuyo `rt_MatchTag` se apunta a
  sí mismo, entero dentro de su segmento), lo inicia (`InitResident`: con `RTF_AUTOINIT`, `MakeLibrary` con su tabla y
  después `AddLibrary` o `AddDevice`; sin él, su `rt_Init` con D0 = 0, A0 = la lista y A6 = SysBase) y abre la
  biblioteca. Al cerrarla el último, su `Expunge`; si devuelve la lista de segmentos, se suelta. `InitResident` y
  `FindResident` (0: no hay ROM) también para los programas. Lo nativo siempre gana: solo se pide lo que no está en la
  lista, y un `RomTag` que se llame como una que ya está (una nativa) no se inicia. Una que se abre a sí misma desde su
  `Init` no se vuelve a cargar. Una que miente con su versión (más baja que la pedida) se echa en seguida. Los
  dispositivos de disco llevan su cuenta de aperturas en su `Open`, como en el Amiga (los nativos, exec).
- **dos** (`caja/dos/src/hunk.rs`): el cargador hunk en Rust, para `LoadSeg` (antes daba `ERROR_NOT_IMPLEMENTED`: el
  cargador estaba en C, solo para los programas de prueba de la caja) y para ramlib: CODE, DATA, BSS, RELOC32,
  RELOC32SHORT, DREL32, SYMBOL, DEBUG, NAME, END; todo comprobado; si falla, suelta lo que reservó.
  `Dos::cargar_de_disco`: lee un fichero (con las asignaciones de la tarea) y lo carga sin apuntarlo como `LoadSeg`
  suyo.
- **La caja** (`caja/pegamento/src/ramlib.rs`): busca en `LIBS:`, `Classes:`, `PROGDIR:Libs/` y `PROGDIR:Classes/`
  (los dispositivos en `DEVS:` y `PROGDIR:Devs/`; un nombre con `:`, tal cual), carga en memoria del sistema (la
  biblioteca sigue cuando acaba la tarea que la abrió, hasta que se echa) y el JIT olvida lo traducido. Nunca de
  disco: exec, graphics, layers, intuition y dos. `LIBS:`, `DEVS:`, `Classes:` y `FONTS:` se asignan al arrancar a
  los cajones de `Caja:` que haya (en el disco de pruebas, `Caja:Libs`).
- **Prueba de la caja** (`caja/pruebas68k/ramlib.c`, con `prueba_lib.c`: una biblioteca de disco hecha a mano, con su
  `RomTag`, `RTF_AUTOINIT`, Open/Close/Expunge que se echa sola y `Suma`): abre `prueba.library` de `Caja:Libs`, la
  usa (`Suma(40, 2)` = 42), la abre otra vez (la misma), la cierra (se echa) y la abre por su camino entero. Lo malo:
  versión más nueva de la que hay, RomTag falso (`falsa.library`), versión que miente (`miente.library`), un fichero
  que no es ejecutable (`rota.library`), una que no existe, `LIBS://../S/Startup-Sequence`, `layers.library` (nunca de
  disco), graphics (la nativa) y, al final, nada suyo en memoria: 0 fallos, 0 avisos, 0 bloques sin soltar y los
  bloques del sistema como antes.
- **Pruebas de cargo**: `exec/tests/ramlib.rs` (con una "caja" que hace lo que haría la de verdad: cargar, abrir,
  cerrar y echar; lo nativo gana; RomTag falso, sin RomTag, tabla de saltos fuera de la memoria, versión que miente,
  cerrar dos veces, nombres de 300 letras, vacíos y con caminos raros, la que se abre a sí misma, un dispositivo) y
  `dos/tests/hunk.rs` (ejecutables y bibliotecas de verdad de `caja/pruebas68k`, el RomTag reubicado; cortados por
  todas partes, 6000 con bytes cambiados al azar, a mano: tamaños enormes, reubicaciones fuera, overlays, sin memoria a
  mitad; nada sin soltar).
- `make prueba` (BIEN cargador hunk, BIEN esperado.txt) y `make arranque-virt` (las seis BIEN, con la línea nueva de
  ramlib.exe): bien.

### Paso 2: el AmigaOS del usuario como capa

- **Dónde**: `SYS:C/Prefs/amigaos` (una línea: una carpeta del disco de DashOS, sin `:` ni `..`). La caja la asigna
  como `AmigaOS:` (de solo leer: `SYS:` lo es) y pone sus `Libs`, `Classes`, `Devs`, `Fonts`, `Locale`, `MUI` y
  `MUI/Libs` detrás de `LIBS:`, `Classes:`, `DEVS:`... (`AssignAdd`): lo nativo y lo de `Caja:` va antes. DashOS no
  trae ningún AmigaOS ni ROM. Para probar, `tools/disco_archivos.py` copia uno al disco de pruebas con
  `DASHOS_AMIGAOS=carpeta` (el del usuario, de su equipo: nunca va a git). Un HDF todavía no: hace falta leer su
  sistema de archivos (FFS o PFS3), pendiente.
- **Nunca de disco** (`pegamento/src/ramlib.rs`, `RECHAZADAS`): exec, graphics, layers, intuition y dos (las de la caja
  son nativas, las de un AmigaOS hablan con el chipset), lowlevel, las 680x0 y mmu, y los dispositivos del hardware
  (audio, trackdisk, gameport, keyboard, input, serial, parallel, scsi, carddisk, printer). Fallan limpias: 0 o
  `IOERR_OPENFAIL`.
- **Aún no** (`SIN_SOPORTE`): icon, workbench y asl del AmigaOS 3.2 llaman en su `Init` a `execPrivate17`, una
  función privada de exec sin documentar. Una privada de exec ahora acaba el programa que la llama (antes devolvía 0 y
  la biblioteca saltaba a la página cero: la caja se colgaba); estas tres ni se cargan, para que quien las abre
  siga sin ellas. Hace falta que sean nativas.
- **Arreglo de exec**: `InitResident` con `RTF_AUTOINIT` pone el nodo desde el `RomTag` (tipo, prioridad, nombre,
  versión, id) antes de su tabla de `InitStruct`. Muchas bibliotecas de verdad no traen tabla y su `Init` solo pone la
  revisión: sin nombre, ningún `OpenLibrary` las encontraba y ramlib las cargaba otra vez, 16 veces. Y tras cargarla,
  exec la abre por su base, no por su nombre.
- **Prueba** (`caja/pruebas68k/capa.c`, paso 8b3 de la caja): abre 16 bibliotecas del AmigaOS del usuario; lowlevel y
  audio.device tienen que fallar. Sin AmigaOS, lo dice y acaba (así va en `make arranque-virt`). Con el 3.2 del
  usuario (`sys32mui`): abren iffparse 45.3, locale 47.26, mathieeedoubbas 47.2, mathieeedoubtrans 47.2 y diskfont
  47.1; no abren mathieeesingtrans y mathtrans (piden mathieeesingbas y mathffp, que en el Amiga están en la ROM),
  commodities (pide keymap, en la ROM), datatypes (pide layers), icon, workbench y asl (privadas de exec),
  muimaster (pide keymap) y las dos clases. Ninguna cuelga ni deja nada sin soltar. (Con el paso 3 abren también
  commodities 45.2 y rexxsyslib 47.2.)
- **Lo que falta del paso**: el HDF; un aviso `CierreDeMas` al cerrar locale y diskfont recién cargadas (abren y se
  usan; su cuenta de aperturas llega a 0 antes de tiempo, sin averiguar aún por qué); con el AmigaOS puesto, el
  runtime de Dash abre su `clipboard.device`, que pide `GetDeviceProc` (no hecho en dos): un aviso en los dos
  programas de ReAction.

### Paso 3: apuntar lo que falta y rellenar lo más pedido

- **El registro** (`caja/pegamento/src/faltan.rs`): cada función nativa que se llama y no está hecha (el aviso
  `NoImplementada` de exec, dos, graphics, intuition, gadtools, layers, keymap, utility o timer), y cada biblioteca o
  dispositivo que no hay (ni nativo ni en disco, o rechazado), con su LVO y quién llama: la biblioteca de disco cuyo
  código tiene la dirección de vuelta (`mathieeesingtrans.library (+$974)`) o la tarea. La primera vez de cada una
  sale en la consola (`falta: ...`); al acabar cada tarea, un resumen con las más pedidas (`faltan N funciones
  distintas...`).
- **Programas del usuario** (paso 8b4 de la caja): `tools/disco_archivos.py` con `DASHOS_PROGRAMAS_AMIGA=a:b:c`
  los copia al disco de pruebas y los apunta en `SYS:C/Prefs/programas-amiga`; la caja los ejecuta y a los 6 s cierra
  sus ventanas (si no acaban en 10 s más, el vigilante los corta). No cuentan como fallo.
- **Lo más pedido**, con el 3.2 del usuario, las demos de MUI 5 (Class1, Balancing, Aboutbox) y `capa.exe`:
  - keymap.library nativa (la pedían muimaster y commodities; en el Amiga está en la ROM): `MapRawKey` (Latin-1 con
    la distribución estadounidense de `teclado.rs`, y las secuencias CSI de cursores, F1-F10 y Help), `MapANSI`,
    `AskKeyMapDefault` (una `struct KeyMap` de verdad, con sus tablas) y `SetKeyMapDefault`.
  - layers.library nativa (la pedía datatypes; muimaster la abre): cerrojos que no esperan (una tarea 68k a la vez),
    `InstallClipRegion` (guarda la región; graphics aún no recorta con ella), `NewLayerInfo`/`DisposeLayerInfo`,
    `BeginUpdate`, `LayerOccluded`. Crear y mover capas sueltas y los ganchos, `NoImplementada`.
  - exec: `OpenResource`, `AddResource`, `RemResource` (mathieeedoubtrans; la caja no tiene recursos propios: 0),
    `Alert` (commodities: un aviso; con `AT_DeadEnd`, acaba la tarea) y `RawDoFmt` (rexxsyslib), con el formato del
    autodoc; con `PutChProc` a 0 o el `stuffChar` del autodoc se escribe directo, con otra función de 68k se la
    llama por carácter.
  - dos: `GetDeviceProc` (lo pide `clipboard.device` del 3.2, que abre el runtime de Dash): 0 con
    `ERROR_DEVICE_NOT_MOUNTED` (la caja no tiene procesos manejadores); `FreeDeviceProc`, nada.
  - Pruebas de cargo: `intuition/tests/capas.rs` (lo que hacen, lo que no, y punteros nulos, búferes cortos o
    fuera de la memoria), `exec/tests/formato.rs` (los tres caminos de `RawDoFmt`, destinos y formatos malos, el corte
    en 64 KB), `exec/tests/ramlib.rs` (recursos y `Alert`), `dos/tests/cerrojos.rs` (`GetDeviceProc`).
- **El oráculo** (FS-UAE con el 3.2 del usuario y su MUI, sin cerrojo): Class1 abre su ventana y dibuja sus líneas.
- **Lo que para a MUI** (y a Tachyon, que es MUI 5): con keymap y layers, muimaster se carga y empieza su `Init`, y
  la caja entera cae: el traductor de Emu68 lee un puntero nulo (`EMIT_LoadFromEffectiveAddress`, FAR 0) al traducir
  un bloque de muimaster que hace un `longjmp`: `movea.l $90,a1`, `btst #4,297(a1)`, `fmovem.x 64(a0),fp0-fp7`,
  `movem.l 4(a0),d1-d7`, `movem.l 36(a0),a1-a7`, `move.l (a0),(a7)`, `movea.l 32(a0),a0`, `rts` (localizado con una
  traza temporal del JIT, ya quitada). En el banco del JIT (`jit-host`, con qemu-aarch64_be) ese mismo bloque se
  traduce y va bien (con `$90` cambiada por una dirección del banco, cuya memoria empieza en 1 MB; FMOVEM.X d16(An)
  carga FP0 bien): lo que cae depende de algo de la caja (¿la página cero, `$90`?, ¿otra configuración del JIT?).
  Lo siguiente: una traza por instrucción del JIT en la caja, en ese bloque, y el arreglo en el parche de Emu68.
  Hasta entonces, un programa que abra muimaster tumba la caja (no el sistema: DashOS sigue).
- **Lo que sigue faltando** (lo dice el registro): mathieeesingbas y mathffp (en la ROM del Amiga; las piden
  mathieeesingtrans y mathtrans), `FindNamedObject` y `AllocNamedObjectA` de utility (datatypes; el registro aún no
  sabe los LVO de utility: sale 0), `CreateNewProc` de dos (texteditor.gadget, que abre igual), icon, workbench y
  asl nativas (las de 3.2 usan una privada de exec), los ganchos de layers (`DoHookClipRects`, que MUI usa para el
  fondo) y que graphics recorte con la región de la capa. Y el aviso `CierreDeMas` de locale, diskfont y commodities
  recién cargadas.

### Paso 4: Tachyon 68k

- **No arranca todavía**, por dos cosas, las dos fuera de lo que faltaba de las bibliotecas:
  - **La memoria del 68k de la caja es de 4 MB** (`M68K_RAM_LEN`, de 1 a 5 MB). Tachyon (`build/dashweb`, 68020)
    ocupa 5 MB solo de código y gasta 29-39 MB al correr (su README): la caja dice "no carga: sin memoria para los
    hunks". Agrandarla es una decisión del sistema: `memoria_crear` reserva entera la memoria del objeto y la cobra
    al cupo del proceso; 64 MB por caja pesan en la máquina de pruebas. Propuesta: que la caja la pida según el
    programa (o crezca a trozos), o un tope mayor en un ajuste.
  - **MUI 5 tumba la caja** (paso 3): hasta arreglar el traductor, muimaster no se carga (`TUMBAN_EL_JIT` en
    `pegamento/src/ramlib.rs`): quien la abre recibe 0 y acaba solo (las demos de MUI acaban con 20, "no se pudo
    abrir muimaster"), en vez de tumbar la caja. Tachyon es MUI 5, así que tampoco pintaría su ventana.
  - bsdsocket no hace falta para lo anterior: Tachyon arranca sin red; no se llegó a ver.
- **De paso, un arreglo de fondo**: lo que reserva el código de una biblioteca de disco (su `Init`, sus funciones) era
  de la tarea que la llamaba, y la caja lo soltaba al acabar ella, con la biblioteca aún en memoria. Ahora exec
  reserva para el sistema mientras corre código de una biblioteca cargada por ramlib (la caja lo mira en cada llamada
  nativa por la dirección de vuelta: `memoria_del_sistema`; prueba en `exec/tests/ramlib.rs`). Con eso, `capa.exe`
  vuelve a acabar con 0 bloques sin soltar. Con todo: 8 de las 16 bibliotecas de su prueba abren del 3.2 del usuario
  (iffparse, locale, mathieeedoubbas, mathieeedoubtrans, diskfont, commodities, rexxsyslib y texteditor.gadget).

## 2026-11-13 (noche): hacia Tachyon. Memoria según el programa y MUI 5

Rama `caja-amiga-libs`, con `revision` (404ea53) unida: `archivos/dashfs`, la tableta USB y `make virt-uso`.

### La memoria del 68k crece según el programa

Decisión del usuario: la caja pide la memoria según lo que necesite el programa, con un tope en un ajuste (de serie,
256 MB).

- **Cómo crece** (`caja_ampliar_ram`, `caja/caja.c`): el 68k ve sus direcciones como las del proceso, desde 1 MB.
  Empieza con 4 MB. Cuando un `AllocMem` (o `AllocVec`, `CreatePool`, los hunks de un programa) no cabe, exec pide más
  (`Memoria::ampliar`): la caja proyecta objetos de memoria nuevos justo detrás de lo que hay. `memoria_crear`
  reserva entera la memoria de un objeto y la cobra al cupo, así que se crece a trozos: lo que haga falta y al menos
  una cuarta parte de lo que ya hay, en objetos de 16 MB como mucho (el máximo del kernel). Exec sube el `MH_UPPER` de
  la región de arriba y mete lo nuevo en su lista de libres, pegado al trozo libre de su final: cabe un bloque más
  grande que cada trozo (un hunk de 5 MB). La chip no crece. Lo crecido no se devuelve: exec lo vuelve a repartir.
- **El tope**: `memoria amiga <MB>` en `Prefs/cajas` (de 8 a 2048; sin línea, 256), una fila nueva del centro de
  control ("Memoria de la caja Amiga", que pasa por 64, 128, 256, 512 y 1024 MB). El compositor lo reparte como el
  resto del ajuste; la caja lo mira cada vez que atiende al compositor. Un `AllocMem` que no cabría ni con todo lo que
  queda no hace crecer nada (un programa que pide 2 GB no se lleva la caja hasta el tope).
- **El cupo** (propuesta del coordinador): el de la caja Amiga en `inicio` es 32 MB más una cuarta parte de lo que
  queda libre para grupos al lanzarla, hasta 256 MB (`margen_caja_amiga`: la mayor que cabe se busca a mitades con
  grupos de prueba que se matan en seguida). Un grupo saca su presupuesto del de quien lo crea al nacer: con todo el
  margen que cupiera, los servicios de después (`espia`) ya no se podían lanzar en `virt` de 2 GB. Un tope mayor en el
  ajuste no pasa del cupo: `memoria_crear` da `E_MEMORIA` y la caja se queda donde llegó.
- `AvailMem` cuenta, en lo que no es chip, lo que queda por crecer: un programa que mira antes de pedir no se asusta.
- **Pruebas**: `exec/tests/crecer.rs` (crece lo justo, la chip no, lo soltado se reutiliza, `TypeOfMem` de lo nuevo;
  hostil: pedir 256 KB sin fin para en el tope, tamaños absurdos dan 0) y `glotona.exe` en la caja (paso 8b3b): con un
  tope de prueba de 48 MB pide 1 MB tras otro hasta que no le dan más, sin pasar del tope ni del cupo y sin colgar
  nada; soltado todo, la mitad de lo que tuvo, de una vez, cabe sin crecer. Con 32 + 32 MB de cupo llegaba a 33 MB
  (lo paraba el cupo, no el tope).

### MUI 5: no era el JIT

- **La traza por instrucción** (temporal, ya quitada: cada instrucción que traduce el JIT, con su dirección y sus
  palabras, desde que ramlib carga muimaster) enseñó que el bloque del `longjmp` se traduce entero dos veces y que la
  caída llega después. La dirección de la caída (`0x110000c154`) no es del traductor sino del **código traducido**
  (la caché del JIT); FAR 0: el 68k escribe en la dirección 0. Ese bloque es el `exit()` de la biblioteca de C de
  muimaster: hace `longjmp` a un `jmp_buf` que en una biblioteca nadie ha llenado (ceros), carga A7 = 0 y escribe ahí.
  En la caja, escribir en la página 0 acaba con la caja entera (S28, como la prueba de malware de la dirección 4).
  En el banco del JIT el mismo bloque, llamado dos veces como en muimaster, va bien. **No hace falta parche de Emu68.**
- **Por qué salía**: lo dice la propia muimaster por `Output()`, "mathieeesingbas.library could not be opened": la
  coma flotante simple IEEE, que en el Amiga está en la ROM. Ahora es nativa (`pegamento/src/mates.rs`, del autodoc:
  las 12 funciones; `IEEESPCmp` e `IEEESPTst` dejan los códigos de condición con un `TST.L D0` al volver, con
  `crear_biblioteca_nativa_con_cc` de exec). Con ella muimaster 21.227 abre.
- **Las clases de MUI**: MUI 5 no usa `MakeClass` ni la `rootclass` de intuition: arma sus `struct IClass` a mano,
  con su propia clase raíz (`cl_Super` = 0, despachador en 68k) que hace los objetos ella sola (la clase en la palabra
  de antes del objeto, `OCLASS`). Intuition no comprueba lo que le dan; la caja comprobaba que la clase o el objeto
  fueran suyos ("MUI: cannot create an object"). Ahora los **adopta** (`adoptar_clase`, `clase_de_objeto` en
  `intuition/src/boopsi/mod.rs`): una clase que se puede leer entera, con un `h_Entry` par y legible, y cuyas
  superclases (como mucho 32, sin dar vueltas) llegan a una conocida o a `cl_Super` = 0, pasa a ser de usuario y del
  sistema; un objeto cuya palabra de antes es una clase así se despacha por ella (`SetAttrsA`, `GetAttr`, `DoMethodA`,
  `DisposeObject`). Basura, círculos o despachadores fuera de la memoria siguen dando 0 con aviso
  (`tests/boopsi.rs`: `clase_hecha_a_mano`, `raiz_propia`). Además, lo que crea código de una biblioteca de disco
  (clases con `MakeClass`, objetos) es del sistema, como su memoria (`Exec::dueno_de_lo_nuevo`).
- **Resultado**: la demo Class1 de MUI abre su ventana ("A Simple Custom Class", 144 x 133) y acaba con 0 al cerrarla;
  Tachyon (`build/dashweb`, 68020, MUI 5) carga sus 5 MB de hunks (la memoria crece), arranca sin red y abre su
  ventana ("Tachyon", 800 x 171). El oráculo FS-UAE 3.2 da la misma ventana de Class1 con sus líneas.
- **Lo que pidieron y ya está**: graphics `TextExtent` (Tachyon, por cada texto) y `SetRPAttrsA` (fuente, plumas y
  modo), intuition `LockIBase`/`UnlockIBase` (con una tarea 68k a la vez, nada que esperar) y `ReportMouse`.
- **Lo que piden y falta** (el registro): `CreateNewProc` de dos (muimaster; la caja aún no tiene varias tareas 68k a
  la vez), `GetDisplayInfoData` de graphics, `WeighTAMatch` (diskfont), `FindNamedObject` y `AllocNamedObjectA` de
  utility (datatypes), cybergraphics, icon, workbench, screennotify e input.device (las abre muimaster y sigue sin
  ellas), y `CierreDeMas` al cerrar bibliotecas de disco recién cargadas (sin averiguar).
- El registro de lo que falta dice ahora la primera vez de cada cosa **en cada tarea** (antes, solo la primera de todas:
  lo que pedía muimaster ya lo había pedido otro antes y no salía) y los `OpenLibrary` que dan 0 por versión.
- **Capturas** (`capturas-agente/`, fuera de git; llevan la demo de MUI del SDK del usuario: no publicar), en Full HD
  con `CAJA_INTERACTIVA=1 DASHOS_PANTALLA=1920x1080` y un vigía que pide `screendump` al monitor de QEMU cuando la
  caja dice que la ventana está en pantalla:
  - `caja-tachyon-fhd.png`: la ventana de Tachyon a lo ancho, con su barra de estado ("Ready: 4649 bytes in 641 ms":
    carga su página de inicio, sin red), el marco de la página y la barra de desplazamiento. Faltan la barra de
    botones y el texto de la página.
  - `caja-mui-class1-fhd.png`: la ventana de Class1 abierta, pero vacía: el oráculo FS-UAE dibuja ahí sus líneas.
    Lo que MUI pinta dentro (su fondo con `DoHookClipRects`, su recorte con `InstallClipRegion`, lo que pide de
    `GetDisplayInfoData`) es lo siguiente que mirar.
- **Pruebas de la tanda**: `make prueba` BIEN. `make arranque-virt`: lo de la caja, bien (glotona, en la máquina de 2 GB
  sin margen de cupo, llega a 6 MB y para); fallan, cada vez una distinta, dos líneas del guion del explorador
  ("suelta en la ventana 1: mueve a Home/Privado", "no abre Sistema/DeCarla: -12"), que esta rama no toca: sin
  averiguar si es el explorador recién unido de `revision` o la máquina cargada (había otras QEMU a la vez).

### Arreglo: `make arranque-virt` sobre `revision` unida

- "MAL: el disco tras la prueba" era `C/Prefs/cajas` de 48 bytes en vez de 30: el guion pulsa ahora también la fila
  "Memoria de la caja Amiga" (`memoria amiga 512`). El árbol esperado de `tools/disco_archivos.py` lo dice ya.
  "[archivos] queda poco sitio: se suelta el disco sujeto" sale también en `revision` sola (y en prueba1 y prueba3):
  es la prueba A15 de `ficheros`, que sujeta el disco sin soltarlo hasta que `archivos` lo suelta ("A15 sujetar sin
  soltar: bien"). "no abre Sistema/DeCarla: -12" es una línea esperada (lo ajeno del explorador).
- Con el margen nuevo, en `virt` de 2 GB la caja tiene 40 MB de cupo (8 para su 68k) y `espia` se lanza; glotona llega
  a 15 MB. `make arranque-virt` dos vueltas seguidas: todo BIEN.

## 2026-11-14: MUI que pinte (rama `caja-amiga-mui`, desde `revision` b8805be) — a medias

- **Memoria para probar Tachyon**: `MEMORIA=3G make arranque-virt` (la variable de `tools/prueba_uefi.sh` ya existía;
  `virt` va con `highmem=off`, así que 4 GB no caben). Con 3 GB, la regla de la cuarta parte da a la caja 288 MB de
  cupo (256 para su 68k), y Tachyon carga.
- **Hecho**: una ventana de refresco simple nace por pintar (su capa con `LAYERREFRESH` e `IDCMP_REFRESHWINDOW` al
  abrirla o cuando `ModifyIDCMP` lo pide luego: MUI abre sin IDCMP y pone su puerto después); `GetDisplayInfoData`
  (`DTAG_DISP`, `DIMS`, `MNTR`, `NAME`) y `WeighTAMatch`. Con sus pruebas en `intuition/tests/dibujo.rs`.
- **Lo que se ve** (una traza temporal, ya quitada, de las llamadas de graphics, intuition y layers de cada tarea):
  Class1 recibe el `IDCMP_REFRESHWINDOW` y hace `BeginRefresh`/`EndRefresh` sin dibujar nada en medio; no llama a
  `Move`, `Draw`, `RectFill`, ni a las regiones, ni a los ganchos de layers (solo `LockLayerInfo`/`UnlockLayerInfo` y
  `MapRawKey`). MUI decide no pintar antes de llegar a graphics: no es el recorte ni el gancho de relleno. Lo siguiente
  es averiguar qué mira MUI para dar sus objetos por visibles (la `Layer` de la ventana, su `DamageList` vacía, la
  `Screen` y su `ViewPort`/`ColorMap`, o el proceso que no puede crear con `CreateNewProc`), con el oráculo de FS-UAE.
- **Sin hacer**: las regiones de graphics (`NewRegion`, `OrRectRegion`...: no están; Tachyon llama a `NewRegion`),
  los ganchos de layers, cybergraphics, los objetos con nombre de utility, icon y workbench.

## 2026-11-14 (tarde): MUI pinta y Tachyon enseña su página (rama `caja-amiga-mui`, con `revision` 16940f0 unida)

- **Unión**: `revision` (16940f0) unida sin conflictos (4c32357). Emu68 de este worktree con los parches 0001-0014
  aplicados: su `git diff` es el mismo que el de `.worktrees/revision` (el `--reverse --check` de cada parche suelto
  falla en algunos porque los de después tocan las mismas líneas).
- **Cómo se miró**: `DASHOS_AMIGAOS=~/.cache/dash-emu/sys32mui DASHOS_PROGRAMAS_AMIGA=<Class1>:<dashweb>` con
  `make arranque-virt` (con Tachyon, `MEMORIA=3G`), un guion fuera de git que corta QEMU (por PID) cuando acaba el
  último programa y saca un `screendump` por el monitor (unos 4 minutos por vuelta), y una traza temporal en
  `pegamento` (cada llamada nativa de la tarea del programa, con registros, quién la hace y lo que devuelve; ya
  quitada). Las capturas se quedan en el equipo: no se publican (llevan MUI y Tachyon del usuario). FS-UAE no hizo
  falta: no lo he ejecutado en esta tanda.

### Por qué MUI no pintaba

- MUI no dibuja en una capa sin `ClipRect` (`layer->ClipRect` a NULL: la da por tapada entera). Comprobado: con la
  `ClipRect` y sin ella (lo demás igual), Class1 hace 96 `Draw` o ninguno. Ahora la capa de cada ventana lleva una
  `ClipRect` que la cubre, su `Layer_Info` (la de la pantalla) y una `DamageList` de verdad (una región): todo por
  pintar al abrir una ventana de refresco simple o al cambiarla de tamaño; `EndRefresh(TRUE)` la vacía.
- **Resultado** (captura mirada a mano): la ventana de Class1 sale con su marco de MUI y el abanico de líneas de la
  clase propia. Class1 acaba con 0, 2 avisos (antes 21).

### Tachyon

- Con `MEMORIA=3G`, Tachyon (768 x 540) pinta su página de inicio: el texto, el cuadro de búsqueda con su botón y las
  ocho tarjetas con sus bordes redondeados. **Falta**: los iconos de la barra de botones (salen cuadrados vacíos) y
  el logo (un rectángulo vacío); son imágenes y datatypes aún no arranca (abajo).
- **Lo que pidió y ya está**: las regiones de graphics (`regiones.rs`: las diez, en la memoria del 68k con la
  disposición de `graphics/regions.h`, en bandas); `CreateUpfrontLayer` y compañía, `DeleteLayer` y `WhichLayer`
  (pinta su página en una capa de un mapa fuera de pantalla y la copia a la ventana); `InstallClipRegion` recorta ya
  todo lo que se pinta (la usa Tachyon en esa capa y MUI en la ventana); `BltMaskBitMapRastPort` (imágenes de MUI);
  el relleno de áreas (`areas.rs`: `InitArea`, `InitTmpRas`, `AreaMove`, `AreaDraw`, `AreaEllipse`, `AreaEnd`);
  `DoubleClick` (500 ms, el de DashOS).
- **Un fallo de fondo que salió**: el montón de Rust de la caja era de 8 MB fijos y ahí viven los píxeles de las
  ventanas y de los `BitMap` nativos. Con la capa de Tachyon funcionando, pintar su mapa de 740 x 516 (1,5 MB) acabó
  la memoria y la caja entera cayó (pánico de Rust). Ahora el montón crece como la memoria del 68k (objetos nuevos
  justo detrás, hasta 512 MB, lo que deje el cupo), un lienzo sin memoria no pinta en vez de caerse y `AllocBitMap`
  nativo reserva sus píxeles al crearse (sin memoria da 0). Prueba hostil en `glotona.c`: un `AllocBitMap` de 64 MB
  de una vez no cabe (en 2 y en 3 GB; no he averiguado si es el cupo o la memoria del kernel), 16 de 2 MB sí, y
  soltados otro cabe; la caja sigue.

### Arreglos sueltos

- **`CierreDeMas` al cerrar locale y diskfont** (pendiente desde el paso 1 de ramlib): las de 3.2 llevan su cuenta de
  aperturas en un campo suyo, no en `lib_OpenCnt` (volcado de las primeras palabras de su `Open`: `addq.w #1,$52(a6)`
  en locale, `addq.w #1,$22(a6)` en diskfont), y exec no llamaba a su `Close` porque veía `lib_OpenCnt` a 0. Ahora
  `CloseLibrary` solo mira `lib_OpenCnt` en las nativas; en las demás, cerrar de más es cerrar lo que la tarea no
  abrió. Y lo que abre el código de una biblioteca de disco no se apunta a la tarea en la que corre (como su
  memoria): matar esa tarea no lo cierra. Class1 y Tachyon ya no dan ninguno.
- **`WeighTAMatch`** no compara nombres: diskfont de 3.2 le pasa la `TextFont` desplazada 16 bytes (así
  `tf_YSize`, `tf_Style` y `tf_Flags` caen donde los de un `TextAttr`) y lo que cae en `ta_Name` es basura (52, el
  `mn_Length`). Desde V39 es privada y sin autodoc.
- **Menús**: un `SubItem` en círculo hacía recursión sin fin en `LayoutMenusA` (desbordaba la pila; salió en la prueba
  al azar de `hostil.rs`). Intuition solo tiene un nivel de subelementos.
- **utility**: los objetos con nombre (`nombres.rs`: `AllocNamedObjectA`, `AddNamedObject`, `FindNamedObject`,
  `ReleaseNamedObject`, `RemNamedObject`, `AttemptRemNamedObject`, `FreeNamedObject`, `NamedObjectName`), que pedía
  datatypes. Ya no faltan; datatypes sigue sin arrancar porque ahora pide `CreateNewProc`.

### Pruebas

- De cargo: `tests/regiones.rs` (contra un modelo de puntos: 3000 pares al azar con las cuatro operaciones; bandas;
  por la biblioteca; hostil: rectángulos degenerados, el plano entero de WORD, negativos, nulos, fuera de la memoria,
  listas en círculo, de un nodo que se apunta a sí mismo, puestas a mano, más largas que el tope, resultados de más de
  16384 rectángulos, sin memoria), `capa_con_dano` y `doble_clic` (`tests/ventanas.rs`), `capas_sueltas` y
  `region_de_recorte` (`tests/capas.rs`, con una región de 16000 rectángulos), `copia_con_plantilla` y
  `relleno_de_areas` (`tests/dibujo.rs`), `biblioteca_con_cuenta_propia` (`exec/tests/bibliotecas.rs`),
  `objetos_con_nombre` y su hostil (`utility/tests/utility.rs`), desplazamientos en `tests/estructuras.rs`.
- Antes de cada commit: las pruebas de cargo de exec, intuition, dos, utility y timer, `make prueba` y
  `make arranque-virt`: bien las tres veces (sin el AmigaOS del usuario, como siempre). Con él, `arranque-virt` falla
  siempre en la línea `[nvme] disco de 16384 sectores` (el disco crece con el AmigaOS): esperado, no es un fallo.

### Lo que queda (por lo que bloquea)

- **`CreateNewProc`**: lo piden muimaster (2 veces) y ahora datatypes en su `Init` (sin él, datatypes no abre y no
  hay imágenes: los iconos de Tachyon y su logo). Hace falta que la caja lleve varias tareas 68k a la vez: es lo
  grande que sigue.
- Bibliotecas que no hay: cybergraphics (muimaster, Tachyon), picasso96api (Tachyon), icon, workbench y
  screennotify (muimaster; icon y workbench de 3.2 usan una privada de exec), input.device (commodities).
- El recorte de `BeginRefresh` a la `DamageList`: hoy la `DamageList` siempre es la ventana entera, así que no cambia
  nada, pero no se recorta con ella.
- Los ganchos de relleno de 68k de las capas sueltas (se llaman como "sin relleno") y mover u ordenar capas sueltas.

## 2026-11-14 (noche): varias tareas 68k a la vez (rama `caja-amiga-procesos`, desde `revision` 124c383)

Lo piden muimaster (dos `CreateNewProc`) y datatypes (uno en su `Init`): sin ellos datatypes no abre y Tachyon no
enseña imágenes.

### Diseño

- **Un solo hilo de Emu68 por caja, con un planificador propio dentro.** Las tareas 68k no son hilos del kernel: se
  cambian de una a otra dentro de una llamada nativa. Cuando la tarea A espera (`Wait`, `WaitPort`, un semáforo, un
  `DoIO`), cede (una de más prioridad quedó lista) o acaba, la caja guarda sus registros, elige otra con
  `elegir_siguiente` de exec y vuelve al JIT con los registros de la otra. El código que sigue a la llamada nativa es
  siempre el `RTS` de la entrada (`$A0FF n RTS`), y ese `RTS` saca la vuelta de la pila de la tarea nueva: así sigue
  donde se quedó. Para retomar una tarea, la caja deja 4 bytes debajo de su pila:
  - si su llamada acabó (cedió por prioridad o por turno): nada; vuelve a su código con sus registros;
  - si se bloqueó: la dirección de un trampolín de exec (`$A0FF 201 RTS`, `lvo::REPETIR`), que repite su llamada con
    los registros de entonces (exec ya trata la segunda vez como la repetición de la espera);
  - si es nueva: su PC de arranque.
  Los registros de coma flotante (FP0-FP7 en d8-d15, FPCR y FPSR en v19) se guardan y se cambian con ella.
- **Por qué no un hilo por tarea** (lo que proponía S4 §2): el JIT, su caché y su estado son de un hilo; con varios,
  una tarea parada dentro de una llamada nativa tiene su vuelta en un bloque traducido que otra puede soltar. Además,
  miles de tareas serían miles de hilos y pilas del kernel. Con el testigo no se ganaba paralelismo. Lo que se pierde:
  que una biblioteca nativa suelte el testigo durante un trabajo largo (no lo hacía ninguna). **Decisión del usuario
  pendiente**: cambia S4 §2; provisional, esto.
- **Cuándo se cambia**: solo dentro de una llamada nativa hecha en modo de usuario (las de las tareas). En modo
  supervisor (un `tc_TrapCode`, la interrupción de la caja) no se cambia: la tarea espera como hasta ahora. El turno
  (`Quantum`, 4 cuadros = 80 ms) se mira en cada llamada nativa: si acabó y hay otra lista de su prioridad, cambia.
  Un bucle que no llama a nada no se interrumpe (pendiente: quitar el testigo con la interrupción de la caja).
- **Prioridades, `Forbid`/`Permit` y `Disable`/`Enable`**: las de exec, que ya estaban (`TaskReady` por prioridad,
  turnos entre iguales). Con `Forbid` la tarea no cede ni por prioridad ni por turno; si espera dentro, el `Forbid` se
  rompe mientras y vuelve con ella. `Disable` además aplaza la interrupción de la caja.
- **Llamadas anidadas** (68k desde una función nativa: el `Init` de una biblioteca de disco, un despachador BOOPSI):
  cada nivel es un `MainLoop` dentro de otro en la pila del hilo, y es de la tarea que lo abrió. Otra tarea puede
  correr dentro (el `Init` de datatypes crea su proceso y lo espera), pero un nivel solo acaba con la vuelta de su
  dueña cuando está arriba del todo. Si una tarea llega a la vuelta de un nivel que no es el de arriba, se aparca
  (espera sin señales) hasta que se cierren los de encima.
- **Fin de una tarea y limpieza**: una tarea que no es la del programa acaba volviendo de su función (al código de
  fin de exec) o con `RemTask`: dos cierra lo suyo, intuition sus ventanas, timer sus esperas, exec suelta su
  `tc_MemEntry` (la estructura y la pila de `CreateTask`/`CreateNewProc`) y lo demás que reservó pasa a quien la creó
  (como en AmigaOS, no se suelta: puede haberlo dado a otra tarea; se soltará cuando acabe el programa). Cuando acaba
  el programa, mueren con él las tareas que creó él o sus tareas; las que creó código de una biblioteca de disco son
  del sistema y siguen (como la de datatypes).
- **Memoria**: la estructura y la pila de cada tarea salen de `AllocMem` como todo, con el tope del ajuste y el cupo
  del grupo; sin sitio, `CreateNewProc` da 0. El aviso de memoria baja llama a los manejadores en la siguiente llamada
  a exec de cualquier tarea. Por tarea, la caja guarda además unos 200 bytes en su montón de Rust.

### Hecho (primera tanda)

- **exec** (`caja/exec/src/cambio.rs`): el contexto de cada tarea parada (`Contexto`: registros, `Modo` y coma
  flotante) en su `InfoTarea`; `guardar_contexto`, `reanudar` (deja en la pila lo que hará el `RTS`), el trampolín
  `$A0FF 201 RTS` (`lvo::REPETIR`) y `tomar_repeticion`; quién creó cada tarea (`creadora`, de `AddTask`: la tarea en
  marcha, o 0 si llama código de una biblioteca de disco) y `descendientes`; `acabar_hija` (`RemTask` y lo demás a la
  creadora); `poner_en_marcha` y `sin_tarea_en_marcha` (la tarea de un programa empieza ya, sin mirar prioridades;
  entre programas no corre ninguna); `aparcar`/`desaparcar` (esperar sin señales). Una tarea que ya corrió y no tiene
  contexto guardado no se puede retomar (no vuelve a empezar en su PC).
- **La caja** (`pegamento/src/multitarea.rs`, `lib.rs`, `caja.c`): cambia de tarea en `Wait`/`WaitPort`/cualquier
  bloqueo, al ceder por prioridad, al acabar el turno (80 ms, en cada llamada nativa), en `Delay`/`WaitTOF` (la tarea
  duerme aparcada hasta su hora) y al acabar una tarea (vuelve de su función o `RemTask(0)`). Con una sola tarea, todo
  sigue como antes (`esperar`, `dormir` en el sitio). Sin nadie listo, la caja espera atendiendo al compositor, a
  timer.device y a las dormidas; los gadgets BOOPSI se atienden con la tarea de la llamada en curso, como antes.
  Ctrl-C de fuera va a la tarea del programa. `RemTask` de otra la quita (si es la del programa, el programa acaba; si
  tiene abierta una llamada anidada suya, no se quita y se avisa).
- **Llamadas anidadas**: la vuelta (`sistema.s`) acaba ahora en `RTS` en vez de `jmp 0.w`: para cerrar el nivel, la
  caja deja un 0 encima de la pila; si la que llega no es la dueña del nivel de arriba, se aparca y corre otra; al
  cerrarse un nivel, la dueña del de debajo se desaparca. caja.c apunta la dueña de cada nivel.
- **La coma flotante**: `caja_fp_leer`/`caja_fp_poner` (caja.c, sin marco): d8-d15 son callee-saved y nadie entre el
  JIT y Rust los guarda (Rust es softfloat; `dashos_native_call` no los toca: mirado en el desensamblado), así que lo
  que se pone llega al JIT.
- **Una tarea que cae** (excepción sin `tc_TrapCode`, en modo de usuario) y no es la del programa: `trampa_defecto`
  sigue en `caida` ($101680), que llama a la función nativa `$FF02`. La caja la quita, carga la siguiente y deja
  encima del marco de la excepción (mirando su formato, para que la pila de supervisor quede como antes) uno de
  formato 0 que vuelve en modo de usuario a un `RTS` suelto, con su pila en USP: `RTE`. Si cae la del programa, como
  antes: el programa acaba por esa excepción.
- **dos** (`caja/dos/src/procesos.rs`): las etiquetas de `CreateNewProc` (`NP_*` de `dos/dostags.h`) leídas y
  comprobadas (tope de 4096 etiquetas con sus `TAG_MORE`, `TAG_SKIP` acotado, punteros, entradas pares y en memoria,
  pila de 1 KB a 64 MB, nombres de 255 como mucho); `traspasar_tarea` (ficheros, cerrojos y variables locales de una
  tarea que acaba pasan a su creadora); `abrir_nil`, `duplicar_cerrojo`, `cerrar_bptr`, `soltar_bptr`, `dar_fichero`,
  `dar_cerrojo`.
- **`CreateNewProc` y `CreateProc`** (`pegamento/src/procesos.rs`): la `Process` con su puerto, su pila, sus
  argumentos (A0/D0 al empezar), NIL: de serie como entrada y salida, copia del cajón y del de su programa, `NP_Cli`;
  todo en un `MemList` de `tc_MemEntry` (exec lo suelta al acabar). Al acabar se cierra lo que pidió
  (`NP_CloseInput`...), se sueltan sus cerrojos y, con `NP_FreeSeglist`, su lista de segmentos. Lo que crea una
  biblioteca de disco es del sistema (sus `FileHandle` y `FileLock` también) y sigue tras el programa.
- **Arreglo de la prueba**: el Ctrl-C de las pruebas de Dash con plazo (`manda_ctrl_c`) llegaba al programa
  siguiente si el suyo acababa antes (salió con test_reaction tras test_gadtools: "***Break"). Ahora solo se manda
  si su programa sigue, y cada programa empieza sin Ctrl-C ni cierre pendientes.

### Segunda tanda: el turno por reloj, PROGDIR: y lo que se vio con datatypes y Tachyon

- **El turno por reloj**: con más de una tarea, un hilo de la caja levanta la interrupción de nivel 6 cada 20 ms. En
  ella (modo supervisor, con el marco de la interrupción encima), si la tarea estaba en modo de usuario, acabó su
  turno (80 ms), no hay `Forbid` ni `Disable` y hay otra lista de su prioridad, se le quita el testigo: su CCR y su PC
  van a su pila (los saca un `RTR` suelto, $10169C, al retomarla) y el marco se rehace para volver en modo de usuario a
  la siguiente (como en `caida`). Una tarea que no llama a nada ya no se queda con la caja (`procesos-a`, prueba 7b).
- **`PROGDIR:` de los programas de disco**: la caja pone `pr_HomeDir` (un cerrojo del cajón de su ruta) a los programas
  que carga de disco (`caja_dos::Dos::cerrojo_de_ruta`). Tachyon ya encuentra su cajón (escribe allí sus ajustes).
- `tools/disco_archivos.py`: un cajón en `DASHOS_PROGRAMAS_AMIGA` se copia entero a `Amiga/Programas` sin ejecutarlo
  (los iconos de Tachyon: `DASHOS_PROGRAMAS_AMIGA=.../dashweb:.../iconos`).
- **Medidas** (QEMU `virt`, máquina con otras QEMU a la vez): sin cambios fuera del ruido en lo de antes (la suma de
  1 a 100.000, 11 → 12 ms; texto_gcc, 6862 llamadas nativas, 249 → 253 ms; boopsi 142 → 145; ramlib 18 → 16;
  glotona 256 → 246). Multitarea: 1000 idas y vueltas con Wait/Signal más 6 tareas más en `procesos-a`, 2223 cambios
  de tarea; `procesos-b`, 4000 tareas creadas, llegadas a su `Wait` y acabadas con Ctrl-C (8241 cambios) en 1,2 s con
  sus `Delay`. La medida limpia es `procesos-e`: 10.000 idas y vueltas con Wait y Signal, 20.002 cambios de tarea en
  125 ms, **6,3 µs por cambio** (con su `Wait`, su `Signal` y la repetición del `Wait`).

### datatypes y Tachyon (con el AmigaOS 3.2 del usuario, `MEMORIA=3G`; capturas solo en el equipo, sin publicar)

- **datatypes.library 47.3 abre**: su `Init` crea su proceso ("datatypes.library", prioridad 0, espera) y vuelve. El
  proceso es del sistema y sigue tras el programa. muimaster abre como antes.
- **Tachyon** arranca, pinta su página y acaba con 0 sin avisos (30 s, con los turnos y sus tareas: `dash_task`), pero
  los iconos de la barra y el logo siguen vacíos. Con sus iconos en el disco y `PROGDIR:` bien, no llega a pedir nada a
  datatypes que se vea en el registro.
- **Por qué no carga imágenes**: una prueba a mano (en `capa.c`, quitada: tarda 20 s) con `NewDTObject` de un ILBM del
  AmigaOS del usuario da 0 con `IoErr` 212 (`ERROR_OBJECT_WRONG_TYPE`): datatypes no tiene registrado ningún tipo.
  Eso lo hace `AddDataTypes REFRESH` (C: del usuario). Lanzado como proceso (`CreateNewProc` con `NP_Seglist`,
  `NP_Arguments` y `NP_Cli`: funciona), pide **`MatchFirst`/`MatchNext` de dos, que no están**, y no acaba. Es lo
  siguiente: `MatchFirst`/`MatchNext`/`MatchEnd` (AnchorPath), y que la caja lance `AddDataTypes REFRESH` al montar el
  AmigaOS del usuario (o que lo haga su Startup-Sequence). Ojo entonces: lo que reserve `AddDataTypes` (un programa,
  no una biblioteca) es suyo y pasaría a quien lo lanzó; si datatypes se queda con ello, hay que darlo al sistema.
- Para que MUI y Tachyon pinten imágenes hará falta además cybergraphics (o picasso96api): las piden y no están.

### Decisiones pendientes del usuario

- **Un hilo de JIT por caja con planificador propio** en vez de un hilo del kernel por tarea (S4 §2).
- **Las tareas que crea un programa mueren con él** (en AmigaOS seguirían, con su código ya soltado); las de las
  bibliotecas de disco siguen.
- **Lo que deja una hija al acabar pasa a su creadora** (memoria, ficheros, cerrojos, bibliotecas) en vez de soltarse.
- **Una tarea que cae** (excepción sin `tc_TrapCode`) se quita y el programa sigue; si es la del programa, el programa
  acaba (como antes). En AmigaOS sería un requester de "Software Failure".
- No hay tope de tareas propio: las para el tope de memoria del 68k y el cupo (4000 en la prueba, sin problema).

### Lo que queda

- `MatchFirst`/`MatchNext` en dos y `AddDataTypes REFRESH` (arriba); cybergraphics.
- `NP_ExitCode` (se avisa y no se llama), `NP_NotifyOnDeath` y `NP_Synchronous` (tampoco en dos 40).
- Una tarea que acaba o cae dentro de una llamada anidada suya acaba el programa entero; `RemTask` de una tarea con una
  llamada anidada abierta se niega (con aviso).
- Entre un programa y otro no corre ninguna tarea: las del sistema que queden listas esperan al siguiente programa.
- En modo supervisor (un `tc_TrapCode` que espera) no se cambia de tarea: espera en el sitio, como antes.
- La prueba del oráculo (FS-UAE) de todo esto: no se ha hecho.

## 2026-11-14 (noche, 2): MatchFirst/MatchNext/MatchEnd y AddDataTypes al arrancar

- dos: `MatchFirst`, `MatchNext`, `MatchEnd` (`caja/dos/src/busqueda.rs`): comodines en el último trozo, sin
  distinguir mayúsculas, asignaciones múltiples (salta lo que no es cajón), `an_Lock` como BPTR de verdad,
  `APF_ITSWILD` y `APF_DirChanged`, `ap_Buf` con `ERROR_BUFFER_OVERFLOW` si no cabe, tope de 256 búsquedas, limpieza al
  acabar la tarea. Pruebas en `caja/dos/tests/busqueda.rs` (7, con hostil). Falta: comodines fuera del último trozo y
  `APF_DODIR` (se avisan como función que falta), `ap_BreakBits`.
- caja.c 8b3a: con `C:AddDataTypes` (AmigaOS del usuario), `AddDataTypes REFRESH QUIET` con vigilante de 20 s; lo que
  no suelta pasa al sistema (`caja_memoria_al_sistema`) para que datatypes no se quede con memoria libre.
- `cargo test` de dos bien; `make prueba` bien; `make arranque-virt` bien salvo el fallo suelto conocido "suelta en la
  ventana 1" del explorador. SIN PROBAR con el AmigaOS del usuario (`DASHOS_AMIGAOS=~/.cache/dash-emu/sys32mui
  MEMORIA=3G make arranque-virt`): si registra los tipos y si Tachyon carga imágenes.

## 2026-11-15: cybergraphics.library y Picasso96API.library nativas (rama `caja-amiga-cgx`, desde `revision` ef7ab8c)

Con el AmigaOS 3.2 del usuario, `picture.datatype` y muimaster abrían `cybergraphics.library` y no estaba; Tachyon
abre además `Picasso96API.library`. Escritas desde el autodoc público de cybergraphics (`cybergraphics.doc`, el `.fd`
y `cybergraphx/cybergraphics.h` del SDK de `/opt/amiga`), el autodoc de Picasso96API y las constantes, estructuras y
desplazamientos de su interfaz (la traducción a Pascal de sus includes en Free Pascal; no hay includes de Picasso96 en
el equipo). No se ha leído código de CyberGraphX, Picasso96, AROS, UAE ni Amiberry.

### Diseño

- **Los mapas de cybergraphics son los nativos de la caja** (`Lienzo`): ARGB de 32 bits, opacos, en el montón de Rust,
  fuera de la memoria del 68k. Ya lo eran los de las ventanas, la pantalla y `AllocBitMap` de más de 8 planos o con
  amigo nativo; ahora también `AllocBitMap` con `BMF_SPECIALFMT` y un formato que no sea `PIXFMT_LUT8` (y
  `p96AllocBitMap` con un formato de color verdadero). Para cybergraphics siempre son `PIXFMT_ARGB32`, 4 bytes por
  píxel, 32 bits de profundidad (`GetBitMapAttr` ya decía 32), **pida el programa el formato que pida** (RGB15, RGB16,
  RGB24...): el autodoc obliga a mirar `LBMI_PIXFMT` antes de tocar los píxeles. Para Picasso96, `RGBFB_A8R8G8B8`.
  `AllocBitMap` con `BMF_DISPLAYABLE` y profundidad > 8 ya daba uno así.
- **Los planares** (los del programa, `AllocBitMap` de hasta 8 planos sin amigo nativo) no son de cybergraphics
  (`CYBRMATTR_ISCYBERGFX` 0, `LockBitMap` 0), pero las funciones de píxeles también pintan en ellos: el color va a la
  pluma más cercana de la paleta del Workbench (con caché por color); `RECTFMT_LUT8` y `FillPixelArray` (el azul es la
  pluma, dice el autodoc), tal cual.
- **Un solo modo de pantalla**: el de la caja (`MODO_CAJA`, `0x29000`, el que ya daba `GetVPModeID`), del tamaño del
  escritorio, en ARGB32. `IsCyberModeID` dice que sí solo a ese; `BestCModeIDTagList` y `p96BestModeIDTagList` lo dan si
  la profundidad cabe (y, en Picasso96, si los formatos permitidos incluyen A8R8G8B8); si no, `INVALID_ID`.
  `AllocCModeListTagList` da una lista con ese nodo (los filtros de tamaño solo si se dan: el escritorio puede pasar del
  máximo de serie, 1600 x 1200).
- **Con el compositor**: nada nuevo. Lo que se pinta en un lienzo apunta su daño y la caja lo manda como cualquier
  dibujo de graphics; las funciones de píxeles recortan como graphics (al mapa, a la capa de la ventana y a su región de
  recorte), en las coordenadas del `RastPort`.
- **`LockBitMapTagList`** (acceso directo): los píxeles no están en la memoria del 68k, así que se copian a un búfer suyo
  en ella (memoria del sistema), en big-endian A R G B, y la dirección del búfer es a la vez `LBMI_BASEADDRESS` y el asa.
  `UnLockBitMap` copia todo de vuelta al lienzo (la memoria es "lineal", así que el programa puede escribir fuera de los
  `UBMI_UPDATERECTS`) y suelta el búfer; con `UBMI_REALLYUNLOCK` a FALSE copia esos rectángulos (o todo) y sigue cogido.
  Lo que se pinte con otras funciones mientras está cogido se pierde al soltarlo (el autodoc lo prohíbe). Un mapa lo
  coge una tarea (la misma puede volver a cogerlo: cuenta); otra recibe 0, porque no puede esperar. `FreeBitMap` de un
  mapa cogido suelta el búfer sin copiar; al acabar la tarea, se copia y se suelta. `p96LockBitMap` es el mismo cerrojo
  con su `RenderInfo` (filas de 32 KB como mucho: `BytesPerRow` es WORD).
- Los tamaños son UWORD, como dice el autodoc (los programas pasan basura en la palabra de arriba: muimaster lo hace);
  las coordenadas de destino, WORD con signo. Lo que cae fuera no se lee del origen: un rectángulo de 65535 x 65535 sobre
  un mapa de 16 x 16 lee 16 filas.
- **Alfa**: la caja no lleva (los lienzos son opacos). `ReadRGBPixel` y `ReadPixelArray` dan alfa 0 (lo que dice el
  autodoc de las tarjetas sin alfa); al escribir, el alfa se ignora. `RECTFMT_RAW` (5, de V43; lo usa la biblioteca de
  Dash) es el formato del propio mapa: ARGB en los nativos, una pluma por byte en los planares.

### Hecho

- **cybergraphics.library 41** (`caja/intuition/src/cgx.rs`, números nativos desde `0x0E00`): `IsCyberModeID`,
  `BestCModeIDTagList`, `GetCyberIDAttr`, `AllocCModeListTagList`/`FreeCModeList`, `GetCyberMapAttr`,
  `WritePixelArray` (RGB, RGBA, ARGB, LUT8, GREY8 y RAW), `WriteLUTPixelArray` (`CTABFMT_XRGB8`), `ReadPixelArray`,
  `ScalePixelArray` (al más cercano), `FillPixelArray`, `InvertPixelArray`, `MovePixelArray` (con `ClipBlit`),
  `WriteRGBPixel`, `ReadRGBPixel`, `ExtractColor`, `LockBitMapTagList`, `UnLockBitMap`, `UnLockBitMapTagList`,
  `CVideoCtrlTagList` (nada que hacer). `CModeRequestTagList` y `DoCDrawMethodTagList` fallan limpias (0, apuntadas en
  lo que falta). Detrás de la tabla, 8 entradas de reserva (`cgfx (LVO -204)`...): una llamada de una versión que no
  tenemos cae en lo que falta en vez de fuera de la tabla.
- **Picasso96API.library 2** (`p96.rs`, desde `0x0E40`): `p96AllocBitMap`, `p96FreeBitMap`, `p96GetBitMapAttr`,
  `p96LockBitMap`/`p96UnlockBitMap`, `p96BestModeIDTagList`, `p96GetModeIDAttr`, `p96WritePixelArray` y
  `p96ReadPixelArray` (con `RenderInfo`, en todos los formatos RGB de 15, 16, 24 y 32 bits y CLUT), `p96WritePixel`,
  `p96ReadPixel`, `p96RectFill`, `p96EncodeColor`. Lo demás (pantallas propias, PIP, pedidor y listas de modos, datos
  de las placas, `p96Write/ReadTrueColorData`), aviso de que falta y 0. Nombre exacto `Picasso96API.library` (exec
  busca por nombre distinguiendo mayúsculas, como AmigaOS).
- **graphics**: `ReadPixelLine8`, `WritePixelLine8`, `ReadPixelArray8` y `WritePixelArray8` (las pidió
  picture.datatype 300 veces con los iconos de Tachyon), sobre lo mismo; `AllocBitMap` con `BMF_SPECIALFMT`;
  `GetVPModeID` da `MODO_CAJA`. `Lienzo::escribir_fila`: una fila de una vez, con un solo apunte de daño.
- **Quién usa qué** (traza temporal en `pegamento`, ya quitada, con el AmigaOS del usuario y Tachyon):
  - picture.datatype: `GetCyberMapAttr` (`ISCYBERGFX`, `DEPTH`), `BestCModeIDTagList`, `WritePixelArray` y
    `ReadPixelLine8` de graphics. En dtprueba (sin pantalla) solo mira `ISCYBERGFX`.
  - muimaster: `FillPixelArray` (fondos; más de 256 llamadas), y `ReadPixelArray` + `WritePixelArray` en ARGB de
    16 x 16 (mezcla ella el alfa de sus imágenes). Pasa basura en la palabra alta de los UWORD.
  - Tachyon (biblioteca de Dash): `p96ReadPixelArray` + `p96WritePixelArray` de 1 x 1 en R8G8B8 (sus mezclas), y
    `p96GetBitMapAttr`. Ni `LockBitMap` ni `ScalePixelArray`.
- **Prueba 68k** `caja/pruebas68k/cgx.c` (8b3e en caja.c, con sus líneas en `arranque_virt.txt`): un mapa de 32 bits de
  cybergraphics, píxeles, `LockBitMapTags`, Picasso96 con `RenderInfo` y `p96LockBitMap`, lo hostil (fuera, negativo,
  enorme, formato 99, planar) y un mapa que se suelta cogido; mide.
- **Pruebas de cargo** `caja/intuition/tests/cgx.rs` (9): modos y listas; atributos (nativo, planar, nulo, fuera,
  `BMF_SPECIALFMT`, amigo); escribir y leer en todos los formatos; planares (pluma más cercana, LUT8, el azul,
  `ExtractColor`); hostil (negativos, todo fuera, 65535 x 65535, tamaño 0, formatos raros, nulos, origen fuera de la
  memoria, 2000 llamadas al azar); recorte a una capa y a su región; acceso directo (campos, copia de vuelta, cerrojo
  doble, `UBMI_REALLYUNLOCK`/`UBMI_UPDATERECTS`, planar, nulo, etiqueta fuera, soltar de más, `RectList` en círculo,
  `FreeBitMap` cogido, fin de tarea cogido); Picasso96 (formatos, `RenderInfo`, cerrojo, modo, hostil); las de 8 bits
  de graphics.

### Con el AmigaOS del usuario (`sys32mui`, `MEMORIA=3G`, Tachyon y sus iconos; capturas solo en el equipo)

- dtprueba ya no pide cybergraphics ("falta: cybergraphics.library OpenLibrary" ya no sale; muimaster tampoco).
- **Tachyon** (captura `capturas-agente/caja-tachyon-cgx-fhd.png`, mirada a mano, no se publica): los iconos de la barra
  (atrás, adelante, recargar, inicio, estrella, historial) ya salen, en color, por picture.datatype; antes eran
  cuadrados vacíos. El logo de la página de inicio sigue siendo un rectángulo vacío: no sale ningún aviso ni nada que
  falte de RTG mientras tanto; sin averiguar de dónde lo saca (no parece cosa de cybergraphics).
- 0 avisos en Tachyon; Picasso96API abre (antes faltaba).
- MUI sola (una demo con imágenes) no la he probado aparte: lo de MUI que se ve es lo de dentro de Tachyon.

### Medidas

- Una llamada de un punto (`WritePixelArray` de 1 x 1, lo que hace Tachyon por cada mezcla) armaba la paleta entera
  (256 colores) aunque no hiciera falta: en el equipo (x86, `--release`), 100.000 llamadas pasan de 35,7 ms a 5,9 ms
  (de 357 a 59 ns cada una). Ahora solo se arma con plumas o en un planar.
- En el equipo: 10 `WritePixelArray` de 640 x 480 en RGB, 4,2 ms; 10 `LockBitMap` + `UnLockBitMap` de 640 x 480, 1 ms.
- En la caja (QEMU `virt`, cgx.exe, reloj de `DateStamp`: de 20 en 20 ms, así que es grueso): 10 `WritePixelArray` de
  640 x 480 en RGB, 20 ms; 10 cerrojos de 640 x 480 (copia de ida y vuelta de 1,2 MB), 40-60 ms.
- Tachyon: 6239 ms antes y 6269 ms después en la prueba (6 s de ventana más carga): sin cambio fuera del ruido.

### Pruebas de la tanda

- `cargo test` de exec, intuition, dos, utility y timer: bien. `make prueba`: bien. `make arranque-virt` (sin el
  AmigaOS del usuario): todo BIEN a la primera. Con el AmigaOS del usuario, solo falla la línea esperada del disco
  (`[nvme] disco de 16384 sectores`), como siempre.
- Un desliz: para parar QEMU usé una vez `pkill -0 -f` con patrón (la señal 0 no hace nada, solo mira); lo demás, por
  PID.

### Decisiones para el usuario

- **Un solo formato** (ARGB32) para todos los mapas de color verdadero, aunque el programa pida RGB16 o RGB24
  (`BMF_SPECIALFMT`, `p96AllocBitMap`): honrado con lo que dicen los atributos, pero gasta el doble en un mapa de 16 bits.
- **Un solo modo de pantalla**, el de la caja, y `MODO_CAJA` sigue siendo `0x29000` (un ID de PAL alta resolución del
  chipset): un programa que mire el monitor del ID para saber si es RTG se puede confundir. Cambiarlo a un ID propio
  tocaría `GetVPModeID`, `GetDisplayInfoData` y las pantallas.
- **`LockBitMap` copia** (ida y vuelta) en vez de dar los píxeles: lo correcto mientras los lienzos vivan fuera de la
  memoria del 68k; lo rápido sería poner los lienzos nativos en la memoria del 68k (cambio grande en `Lienzo`).
- **Alfa 0** al leer y alfa ignorado al escribir (la caja no lleva alfa en los lienzos).

### Lo que queda

- `DoCDrawMethodTagList` (llamaría a un gancho 68k con el mapa cogido: anidada), `CModeRequestTagList` y
  `p96RequestModeIDTagList` (pedidor de modos), `p96OpenScreenTagList`, las listas de modos de Picasso96,
  `p96Write/ReadTrueColorData`, y las de CyberGraphX V43+ (`WritePixelArrayAlpha`...): nadie las ha pedido.
- El logo de Tachyon. Una demo de MUI con imágenes, aparte. El oráculo (FS-UAE con RTG) no lo he ejecutado.

## 2026-11-15 (2): formatos de verdad, LockBitMap sin copia y modos RTG (misma rama)

El usuario no quiere dejar las tres decisiones de la tanda anterior como estaban: un solo formato (ARGB32), un solo
modo con el ID `0x29000` del chipset y `LockBitMap` que copia. Por orden: 3 (dónde viven los píxeles), 1 y 2.

### Paso 3 y paso 1: los mapas RTG en la memoria del 68k, cada uno en su formato

- **Mapas chunky** (`caja/intuition/src/chunky.rs`, `Mapa::Chunky` en graphics, `Sup::Chunky` en el pintor): los de
  `AllocBitMap` de color verdadero (más de 8 planos, `BMF_SPECIALFMT` con un `PIXFMT_*` que no sea LUT8, o un amigo de
  color verdadero) y los de `p96AllocBitMap` con un `RGBFormat` de color verdadero guardan sus píxeles en la memoria
  del 68k (`AllocMem` de exec: memoria del sistema, con su tope, el cupo y el aviso de memoria baja), en el formato
  pedido: los 13 de `cybergraphics.h` (RGB15, BGR15, RGB15PC, BGR15PC, RGB16, BGR16, RGB16PC, BGR16PC, RGB24, BGR24,
  ARGB32, BGRA32, RGBA32). Sin formato, el de su amigo; sin amigo, el de serie de su profundidad (hasta 15, RGB15; 16,
  RGB16; hasta 24, RGB24; más, ARGB32). Filas alineadas a 4 bytes (los de 24 bits de ancho impar llevan relleno, que
  nada toca). Nacen del color de la pluma 0 (como un planar a cero). Un `PIXFMT_*` que no existe con
  `BMF_SPECIALFMT`: 0 y un aviso. A8B8G8R8 de Picasso96 (sin `PIXFMT_*`) se guarda en ARGB32.
- graphics entero pinta en ellos (rectángulos, líneas, texto, copias, áreas): el valor crudo es un color ARGB, como en
  los lienzos, y se pasa al formato al escribir. `BltBitMap` entre formatos convierte (queda lo que guarda el más
  pequeño).
- **`LockBitMapTagList` de un chunky da la dirección de sus píxeles, sin copiar** (el asa es esa dirección; la misma
  que `CYBRMATTR_DISPADR` y `P96BMA_MEMORY`), con su `LBMI_BYTESPERROW`, `LBMI_PIXFMT` y `LBMI_BYTESPERPIX` de verdad.
  Lo que el programa escribe está en el mapa al momento. `p96LockBitMap` igual, con su `RenderInfo`.
- **El alfa**: los formatos de 32 bits lo guardan. `WritePixelArray` con `RECTFMT_ARGB`/`RGBA` (y los de Picasso96 con
  alfa) lo escriben; `ReadPixelArray` y `ReadRGBPixel` lo devuelven. Los lienzos siguen sin alfa (0 al leer).
- **Los lienzos** (ventanas y pantalla de Workbench: superficies del compositor, en el montón de Rust) siguen ahí: llevan
  el texto nítido de DashOS, los marcos y el tema, que se pintan con el lienzo, y sin `unsafe` (fuera de
  `kernel/src/base/`) no se pueden dejar en la memoria del 68k. Ahora llevan el formato en que los ve un programa
  (`Lienzo::formato`, el del modo de su pantalla); `LockBitMap` de uno de ellos sigue copiando, en ese formato, y la
  vuelta marca el daño (los `UBMI_UPDATERECTS`, o todo) para el compositor.
- Las funciones de pixel array convierten desde y hacia cualquier formato del programa: `RECTFMT_RGB`, `RGBA`, `ARGB`,
  `LUT8`, `GREY8`, `RAW` (el del mapa, byte a byte) y los `RGBFB_*` de Picasso96.

### Paso 2: modos de pantalla RTG

- **Modos de la caja** (`caja/intuition/src/modos.rs`): un monitor propio, `0x5DA01000` (no es ninguno del chipset de
  `graphics/modeid.h`); el `ModeID` lleva el tamaño en los bits 4-11 (el 0, el del escritorio; luego 640 x 480,
  800 x 600, 1024 x 768, 1280 x 720, 1280 x 1024, 1600 x 1200 y 1920 x 1080) y la profundidad en los 0-3 (8, 15, 16, 24
  y 32 bits: LUT8, RGB15, RGB16, RGB24, ARGB32). Un tamaño que no cabe en el escritorio no está, ni el de la lista que
  es igual al del escritorio. La pantalla de Workbench va en el del escritorio a 32 bits (`0x5DA01004`), y ese es el
  que da `GetVPModeID`.
- **Base de datos de pantallas** (graphics): `FindDisplayInfo` (el asa es el propio ID; 0 para los del chipset),
  `NextDisplayInfo` (todos los de la caja, en orden), `ModeNotAvailable` (0, o `DI_AVAIL_NOMONITOR` para los del
  chipset), `GetDisplayInfoData` con `DTAG_DISP` (bits por color del modo, `DIPF_IS_FOREIGN`), `DTAG_DIMS` (su
  profundidad, su tamaño), `DTAG_MNTR` y `DTAG_NAME` ("DashOS: 800x600 16 bits"); con un ID que no es de la caja,
  lo de antes. `BestModeIDA` sobre los de la caja (profundidad, tamaño deseado o nominal, el de un `ViewPort` o
  `SourceID`, `MonitorID`, bits por color, `DIPF_*`).
- **Elegir**: `BestModeIDA`, `BestCModeIDTagList` y `p96BestModeIDTagList` (con `FormatsAllowed`/`Forbidden`) dan el de
  la profundidad más pequeña que llegue y, de esa, el tamaño más pequeño que llegue (sin tamaño, el del escritorio;
  si ninguno llega, el más grande). `IsCyberModeID`, `GetCyberIDAttr`, `p96GetModeIDAttr` y `AllocCModeListTagList`
  (todos los que pasan los filtros, con los de serie del autodoc) sobre lo mismo. Los del chipset siguen siendo del
  chipset: `IsCyberModeID` dice que no.
- **Pantallas propias** (`pantallas.rs`): `OpenScreen`/`OpenScreenTagList` con un `SA_DisplayID` de la caja da una
  `struct Screen` nueva del tamaño del modo (o `SA_Width`/`SA_Height`), con su `ColorMap` (la paleta del Workbench de
  partida), su `DrawInfo` (con su profundidad) y su `BitMap` chunky en el formato del modo, en la memoria del 68k
  (`LockBitMap` sin copia; los amigos, en ese formato), o de 8 planos en los de 8 bits. Va delante en la lista de
  `IntuitionBase`. Sus ventanas (`WA_CustomScreen`, `nw_Screen`) son ventanas del compositor como las del Workbench,
  con su paleta y su lienzo visto en el formato del modo; `ViewPortAddress` y `GetScreenDrawInfo` dan los suyos.
  `CloseScreen` la suelta si no le quedan ventanas (si no, FALSE, como en 3.x); si su tarea acaba sin cerrarla, se
  cierra. Sin modo de la caja (uno del chipset o ninguno), la de Workbench, como antes. Un ID del monitor de la caja
  que no está: 0 y `OSERR_UNKNOWNMODE` en `SA_ErrorCode`.

### Pruebas de cargo nuevas

- `tests/formatos.rs`: ida y vuelta por cybergraphics (RGB, RGBA, ARGB) y por Picasso96 (los 12 `RGBFB_*` de color
  verdadero) en mapas de los 13 formatos, contra un modelo de bits por canal escrito en la prueba (no con las funciones
  de la biblioteca: comprobado que falla si se estropea un formato); los bytes de un rojo con alfa en cada formato
  leídos por `LockBitMap` (lo de `cybergraphics.h`) y `RECTFMT_RAW`; `BltBitMap` entre los 169 pares; hostil
  (`PIXFMT_*` que no existen, `RGBFormat` que no existe o YUV, 24 bits de ancho impar con su relleno intacto, filas que
  no se salen).
- `tests/modos.rs`: la base de datos (los 30 modos de un escritorio de 1920 x 1080, sus datos, los del chipset fuera),
  elegir modo (profundidad, tamaño, bits por color, monitor, `SourceID`, `ViewPort`, `DIPF_*`, cybergraphics y
  Picasso96), una pantalla propia de 16 bits con su ventana (`CloseScreen` con ella abierta dice que no; cerrada, todo
  suelto), una de 8 bits, una con su tamaño y otra por `NewScreen` y su extensión, el fin de tarea, y lo hostil (IDs
  del chipset, IDs de la caja que no están con `OSERR_UNKNOWNMODE`, `NewScreen` fuera de la memoria).
- `tests/cgx.rs` al día: `acceso_directo` (sin copia: la dirección es la de `CYBRMATTR_DISPADR`, lo escrito se ve antes
  de soltar) y `acceso_directo_a_un_lienzo` (la copia, con `UBMI_UPDATERECTS`). `tests/dibujo.rs`: `pixel_de` en vez del
  lienzo para los mapas de `AllocBitMap`.
- `cgx.c` (68k) dice además "formatos (RGB16 de dos bytes, RGB24 de ancho impar con relleno)" y "modos RTG (Workbench
  de la caja, pantalla propia de 16 bits)".
- Un fallo que salió en `arranque-virt`: `GetVisualInfoA` de gadtools solo aceptaba el Workbench; un programa de Dash
  abría su pantalla con el modo del Workbench (ahora una pantalla propia) y sus gadgets fallaban. Ahora acepta las
  propias. `caja/Makefile`: `programas.o` no dependía de `ils.exe`, `dtprueba.exe` ni `cgx.exe` (un `cgx.exe` nuevo no
  entraba en la imagen).

### Medidas

| Qué | Antes | Después |
|---|---|---|
| `LockBitMap` + `UnLockBitMap` de 640 x 480 (caja, cgx.exe, 10 veces) | 40-60 ms | 0 ms (menos de un tic de 20 ms) |
| `WritePixelArray` de 640 x 480 RGB (caja, 10 veces) | 20 ms | 20 ms |
| Lo mismo en el equipo (x86, `--release`, 10 veces) | 4,2 ms (lienzo) | 4,3 ms (chunky; 9,8 ms antes de escribir las filas de 24 y 32 bits sin convertir punto a punto) |
| `RectFill` de 640 x 480 en el equipo (10 veces) | 14,7 ms (lienzo) | 11,8 ms (chunky) |
| `BltBitMap` de 640 x 480 a un lienzo, en el equipo (10 veces) | 19,6 ms (de lienzo) | 22,0 ms (de chunky, convirtiendo) |
| Tachyon (programa del usuario, prueba de 6 s) | 6239-6269 ms | 6268 ms |

### Con el AmigaOS del usuario (`sys32mui`, `MEMORIA=3G`, Tachyon y sus iconos)

- `make arranque-virt` con él: solo la línea esperada del disco. dtprueba abre la ILBM sin pedir cybergraphics;
  Tachyon acaba con 0, 0 avisos, 0 bloques sin soltar.
- Captura (`capturas-agente/caja-tachyon-rtg-fhd.png`, solo en el equipo, no se publica): igual que la de la tanda
  anterior: la página, los iconos de la barra en color (por picture.datatype) y el logo vacío. La página de Tachyon se
  pinta ahora en un mapa chunky (su mapa fuera de pantalla es amigo de la ventana) y se copia a la ventana.
- **El logo**: es un PNG en un `data:` de la página de inicio (`LOGO_INICIO` en `src/dashweb.dash` de Tachyon), que
  Tachyon decodifica él solo. La traza de la tanda anterior no vio ninguna llamada de cybergraphics ni de Picasso96 para
  él (solo mezclas de 1 x 1) y ya salía vacío antes de que hubiera cybergraphics: no llega a pintarse, y no es cosa de
  RTG. Sin averiguar más; queda apuntado.

### Pruebas de la tanda

- `cargo test` de exec, intuition, dos, utility y timer antes de cada commit: bien. `make prueba`: bien las tres veces.
  `make arranque-virt` sin el AmigaOS del usuario: en cada commit, todo bien en una vuelta; en las de en medio salieron
  los dos fallos sueltos conocidos del explorador ("suelta en la ventana 1", "no abre Sistema/DeCarla: -12") y el de
  `GetVisualInfoA` (arreglado).

### Lo que queda

- Los lienzos de las ventanas y del Workbench siguen fuera de la memoria del 68k: su `LockBitMap` copia (ida y vuelta
  en el formato de su pantalla). Dejarlos en la memoria del 68k pide que todo lo que pinta intuition (marcos, tema,
  texto nítido de DashOS, menús, gadgets) pase por la memoria del 68k, o `unsafe` fuera de `kernel/src/base/`.
- Las pantallas propias: sus ventanas se pintan con las plumas y los colores del Workbench (la paleta de la pantalla
  empieza siendo la suya; cambiarla no cambia el marco de intuition). `SA_Colors`, `SA_Pens`, `SA_PubName` (pantallas
  públicas propias), `SA_BitMap` (mapa del programa) y `SA_LikeWorkbench` no se miran. Una pantalla propia no tiene
  superficie propia en el compositor (lo que se pinta en su mapa no se ve; sus ventanas sí).
- Los de 8 bits son planares de 8 planos (no LUT8 chunky): `ISCYBERGFX` 0 en su mapa, aunque su modo sí es de la caja.
- El logo de Tachyon (arriba). `DoCDrawMethodTagList`, los pedidores de modos y lo demás que nadie pide, como antes.

## 2026-11-15 (3): lo pendiente de cybergraphics (misma rama, tras unir a `revision` hasta 9c3a0d8)

Lo que quedaba de la tanda anterior, con el mismo criterio: como debe funcionar.

### El logo de Tachyon: faltaba `T:`

- Una traza temporal (ya quitada) de las llamadas nativas de las tareas de Tachyon mientras pinta su página: el logo es
  un PNG en un `data:` que Tachyon escribe en `T:dashweb_img0` para decodificarlo (`base64_to_file`), y
  `Open("T:dashweb_img0", MODE_NEWFILE)` daba 0: **la caja no asignaba `T:` ni `ENV:`** (en un Amiga, en `RAM:`). No era
  una instrucción mal traducida, ni una función que faltara o devolviera otra cosa, ni la memoria.
- Ahora `T:` y `ENV:` van a `Caja:T` y `Caja:Env` (el cajón de escribir de la caja), creando el cajón si falta (dos:
  `asignar_creando`, con su prueba; un nombre malo no crea nada). El árbol esperado del disco lleva los dos cajones.
  `RAM:` sigue sin existir en la caja: `T:` no se vacía al arrancar (Tachyon borra lo suyo).

### Paso 3: las ventanas y el Workbench, en la memoria del 68k

- **El lienzo** (`lienzo.rs`) guarda ahora sus píxeles como un chunky en la memoria del 68k (en el formato del modo de su
  pantalla); la pantalla de Workbench, al pintarla por primera vez (no gasta si nadie la pinta). Se pinta con una
  `Vista` (el lienzo y un préstamo de la memoria a la vez: `trozo_mut` de `Memoria`), detrás del rasgo `Pinta`, que es lo
  que reciben ahora el tema, los marcos, los menús, GadTools y BOOPSI. Sin `unsafe` nuevo: las funciones de pintar que no
  tenían `ex` lo reciben (`dibujar_marco`, `refresh_window_frame`, `presentar`, `fondo_en`...).
- **Al presentar**, lo cambiado se pasa de la memoria del 68k a una copia en ARGB que lee el compositor (no cambia su
  interfaz); en los de 8 bits, por la paleta.
- **`LockBitMap` de una ventana o de la pantalla** da ya la dirección de sus píxeles, sin copia; `UnLockBitMap` apunta
  como cambiado lo que dice `UBMI_UPDATERECTS` (o el mapa entero) para que el compositor lo presente.
- **Medido** (en el equipo, x86 `--release`; `tests/medida_ventana.rs`, temporal, en una ventana de 600 x 400):

| Qué | Antes (lienzo en el montón de Rust) | Después (en la memoria del 68k) |
|---|---|---|
| 20 vueltas de `RectFill` + 30 `Text` + presentar | 37,9 ms | 61,2 ms al principio; 23,0 ms con los rellenos fila a fila y las filas de 32 bits sin conversión punto a punto |
| 20 `RefreshWindowFrame` + presentar | 1,9 ms | 9,8 ms al principio; 1,8 ms con lo mismo |

  En la caja (QEMU, `make arranque-virt`): `boopsi.exe` 143 ms antes y después, `texto_gcc` 261 → 251 ms, cgx.exe igual;
  `LockBitMap` de 640 x 480, 0 ms (menos de un tic). Memoria: cada ventana gasta su mapa en la memoria del 68k y su copia
  en ARGB en el montón de Rust (el doble que antes); la de Workbench, solo si se pinta.

### Paso 2: 8 bits como en una tarjeta (LUT8)

- `Formato::Lut8` (`PIXFMT_LUT8`, `RGBFB_CLUT`): una pluma por byte. `AllocBitMap` con `BMF_SPECIALFMT` LUT8,
  `p96AllocBitMap` con CLUT y los amigos de un mapa LUT8 son chunky de 8 bits (sin formato, hasta 8 planos siguen siendo
  planares, como los del chipset).
- graphics escribe plumas exactas (también dos plumas del mismo color), COMPLEMENT invierte los bits de la máscara, las
  copias pasan por la paleta. cybergraphics y Picasso96 convierten: los colores, a la pluma más cercana; las plumas, a su
  color.
- **Los modos RTG de 8 bits** son LUT8: el mapa de la pantalla es chunky de 8 bits y sus ventanas son lienzos de plumas
  que se presentan con la paleta de su pantalla: `SetRGB32` cambia lo que se ve sin repintar, como una tarjeta de 256
  colores (`tests/lut8.rs`).

### Paso 1: pantallas propias completas

- **Paleta y plumas**: `SA_Colors` (`ColorSpec`), `SA_Colors32` (como `LoadRGB32`), `SA_Pens` (las de la `DrawInfo`),
  `SA_FullPalette` (la paleta de partida ya es entera); se leen antes de reservar nada (lo que no se puede leer es un
  fallo, sin dejar nada a medias). Sus ventanas toman la paleta y las plumas de su pantalla: el marco, GadTools, los
  menús, los requesters, BOOPSI (la `GadgetInfo` lleva su pantalla y su `DrawInfo`) y ReAction (`WA_CustomScreen` en
  `window.class`).
- **`SA_LikeWorkbench`**: sin `SA_DisplayID`, el modo y el tamaño del Workbench.
- **Públicas** (`SA_PubName`): nacen en modo privado, como en 3.x; `PubScreenStatus(s, 0)` las abre y `PSNF_PRIVATE` las
  cierra (no con visitantes: ventanas de otros programas o `LockPubScreen` sin soltar); `LockPubScreen` las encuentra
  por su nombre (sin distinguir mayúsculas), `UnlockPubScreen` resta un visitante y con el último manda la señal de
  `SA_PubSig` a `SA_PubTask` (o a la que la abrió); `WA_PubScreenName` abre la ventana en ella; un nombre repetido da
  `OSERR_PUBNOTUNIQUE`; `CloseScreen` con visitantes, FALSE.
- **`SA_BitMap`** (y `CUSTOMBITMAP` en la `NewScreen`): el `RastPort` de la pantalla pinta en el mapa del programa
  (copiado en el `BitMap` de la `Screen`, como hace intuition).
- **Se ve**: cada pantalla propia tiene una superficie del compositor de su tamaño, sin borde, que presenta su mapa: el
  lienzo con lo que ha cambiado; el mapa del programa, entero cada 5 presentaciones (se puede escribir sin que lo
  sepamos).
- Pruebas: `tests/pantallas.rs` (paleta y plumas con una ventana, como el Workbench, públicas con señal, lo que se ve,
  mapa del programa, hostil).

### Comprobado

- `cargo test` de intuition, dos y pegamento; `make prueba` y `make arranque-virt` en verde tras cada paso.
- Con el AmigaOS del usuario y Tachyon (`MEMORIA=3G`, `TIEMPO_VIRT=480`): falla solo la línea del disco NVMe de 16384
  sectores, como siempre. Tachyon acaba con 0, 0 avisos, 0 bloques sin soltar, en 6302 ms (antes, 6239 a 6356 ms);
  41180 llamadas nativas (antes 39129: ahora escribe y lee el logo).
- Captura a 1920 x 1080 (local, sin publicar): **el logo de Tachyon se ve**, en color real.
- La caja lo dice al arrancar: "temporales: T: Caja:T, ENV: Caja:Env".

### Pendiente

- El mapa de una pantalla con `SA_BitMap` se presenta entero cada 5 vueltas: no hay forma de saber cuándo lo escribe el
  programa sin pasar por graphics.
- Las superficies de las pantallas propias no siguen el orden de `ScreenToFront`/`ScreenToBack` respecto a otras
  pantallas: van donde las pone el compositor.
- Sin `LockPubScreenList`/`UnlockPubScreenList`, `NextPubScreen` ni `SetDefaultPubScreen`.
- `SA_Colors` solo cambia la paleta; los modos de 15/16/24/32 bits no tienen plumas que reservar (como en una tarjeta).
- Cada ventana gasta el doble de memoria (mapa en el 68k y copia en ARGB): si pesa, presentar directamente desde el
  chunky cuando el formato ya es ARGB.
- Tachyon intenta crear "Tachyon.cookies" junto al programa (volumen de solo lectura) y la guardia lo niega; es cosa
  de Tachyon (debería ir a `ENVARC:` o `PROGDIR:` escribible), no de la caja.
