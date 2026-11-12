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
   solo lectura en la dirección 0 con la llamada 45 y el permiso de página 0 (su llave 1, que le da `inicio`). En la
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
  demás) e `inicio` se lo da solo a la caja. La llamada 45 (`pagina_cero`) proyecta con él un objeto de memoria de una
  página en la dirección 0, de solo lectura y sin ejecutar (`Espacio::proyectar_pagina_cero`); el resto de los
  primeros 64 KB sigue cerrado, y `para_copiar` sigue sin dejar al kernel copiar desde ni hacia la página 0. Una llave
  revocable de memoria no vale (la página 0 no se apunta en intermediarios). `inicio` espera a la caja aunque ahora
  lleve una llave (`SE_ESPERAN`). Documentado en `docs/referencia/kernel.md`.
- **Pruebas del kernel** (`make prueba`): `prueba_pagina_cero` (H4): un objeto de dos páginas no vale, la vía normal
  sigue cerrada en los 16 primeros marcos, la página queda de solo lectura y sin ejecutar para EL0, otra vez no, el
  resto de los 64 KB vacío, `para_copiar(4)` falla y se puede quitar. `malo`, paso 38: sin el permiso, la llamada 45
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
