# Caja PowerPC: bitácora

Plan y diseño: `docs/diseño/S17-caja-powerpc.md`. Decisiones del usuario (2026-11-07): una sola caja con dos motores
(68k y PPC); orden WarpOS/PowerUP → MorphOS → AltiVec → OS4 al final (OS4 entero, de momento solo con QEMU); la
fase 1 (el JIT PPC de Emu68 en el banco) empieza ya, antes de la caja.

Sala limpia: se usa el JIT PPC de Emu68 (`src/PPC_*.cpp`, MPL-2.0). `src/ppc/` (el ROM PPC, con código que viene de
SonnetAmiga) no se compila ni se lee: el banco lleva un `ppc_rom.h` vacío.

## F1: el JIT PPC en el banco

- [x] F1a Banco `jit-host/ppc/` (Emu68 PPC sin libc en qemu-aarch64_be), oráculo en qemu-ppc, comparación.
- [x] F1b Enteros: lo que traduce el JIT, con programas en C a -O0 y -O2 y en ensamblador.
- [x] F1c Cargas y guardados de todas las formas, `lmw` y `stmw`, reserva, `dcbz`.
- [x] F1d FPU: doble y simple (grupo 59), cargas y guardados de todas las formas, NaN de PPC, redondeo, FPSCR
  (sus bits de control).
- [x] F1e Las formas con punto de la FPU (CR1 desde las excepciones de FPSCR) y los bits de excepción de FPSCR.
- [x] F1f La llamada nativa por instrucción reservada (como la línea A del 68k, C1).
- [x] F1g Saltos condicionales: todos los BO en bc, bcl, bclr, bclrl, bcctr, bcctrl.
- [x] F1h `icbi` en el banco (código que se modifica a sí mismo), `lswi`/`lswx`/`stswi`/`stswx`, `tw`/`twi`, `sc`,
  `mftb`, `mullwo`/`divwo`/`divwuo`.
- [ ] AltiVec, después de MorphOS (orden del usuario).

## Cómo se prueba

```
make -C jit-host/ppc && make -C jit-host/ppc/pruebas && python3 jit-host/ppc/comparar_ppc.py
```

Cada `pruebas/*.bin` es un binario plano en 0x00140000 que empieza por `prueba()`. El banco (`build/jit-ppc`) y el
oráculo (`pruebas/oraculo`, con `qemu-ppc -cpu 750`) lo ejecutan igual: memoria en 0x00100000..0x001fffff, r1 =
0x001ff000, los demás GPR, CR, XER y FPSCR a 0, LR = 0, CTR = la entrada, los FPR a 0x7fffffffffffffff. El `blr` del
final salta a 0: el banco sale de su bucle y el oráculo recoge el SIGSEGV. Se comparan los 32 GPR, CR, XER, CTR, los
32 FPR, FPSCR y una suma de la memoria; si la memoria difiere, se dice en qué bytes.

El Emu68 del worktree es un clon aparte (`third_party/Emu68`, no va a git) en af63580 con los parches 0001-0009 como
commit "base" y `0010-ppc-f1.patch` como commit "0010" (F1a-F1g, solo toca el JIT PPC, salvo `csel64` en A64.h, que
no usaba nadie; es lo que se mandó a Emu68 en el PR #354 y no se cambia). `0011-ppc-f1h.patch` es el `git diff`
desde el commit "0010" (F1h y F1e). El banco se compila contra ese árbol:

```
make -C jit-host/ppc EMU68=$PWD/third_party/Emu68
```

El banco se compila con `PPC_FPSCR_EXCEPTIONS=1` (FPSCR exacto tras cada operación de la FPU; F1e). Con
`make -C jit-host/ppc EMU68=... FPSCR=0` sale en `build-rapido/` el camino rápido de Emu68 (el valor por defecto de
`config.h`), que se prueba con `JIT=build-rapido/jit-ppc python3 jit-host/ppc/comparar_ppc.py`: ahí p2, p2-o0, t3 y
t8 difieren de qemu-ppc a propósito (solo en FPSCR) y las demás tienen que dar bien, t9 incluida. El banco PPC se compila con `PPC_SO_PROPAGATION=1`
(SO de XER copiado a CR, como en un PPC de verdad; Emu68 lo deja a 0 por velocidad).

## Entradas

### 2026-11-07 — F1a: el banco del JIT PPC

- `jit-host/ppc/`: `Makefile`, `host_ppc.cpp` (memoria, carga, `PPCState` como `StartupPPC`, llamada a
  `PPCMainLoop`, constructores de C++ a mano porque no hay libc), `include/ppc_rom.h` vacío, `comparar_ppc.py`.
- `pruebas/`: `prueba.ld` (binario en 0x00140000), `oraculo.c` (programa de Linux PPC estático para qemu-ppc; el
  manejador de la señal recupera r2 y r13 de glibc antes de escribir), `p1.c` (enteros, saltos, llamadas
  recursivas, tabla de saltos, divisiones, 64 bits en 32, estructuras, comparaciones) a -O0 y -O2. Las pruebas se
  compilan sin PIE: el compilador de Bootlin lo da por defecto, y con PIE `lis` sale a 0 (reubicaciones sin aplicar).
- Parche 0010 (hoy `0010-ppc-banco-enteros-y-fpu.patch`):
  - En el banco, `CNTPCT_EL0` no se lee desde EL0 en qemu-user: `CNTVCT_EL0` (`sys_CNTPCT_EL0_HOST`).
  - **Fallos del JIT de Emu68** que encontró `p1` a -O0 (`rlwinm r9,r9,0,24,19`):
    - Máscaras que dan la vuelta (MB > ME) en `rlwinm`, `rlwimi` y `rlwnm`: tomaba MB - ME - 1 unos, que son los ceros.
    - `rlwimi` sin rotación usaba un temporal sin valor; con la máscara entera no insertaba nada.
    - `rlwinm` sin rotación y con la máscara entera (`mr` hecho con `rlwinm`) no escribía rA.
    - Con el punto (`rlwinm.`, `rlwimi.`), las formas de desplazar y la de `rlwimi` no ponían las banderas antes
      de `setCRnLogic`; y `tst` con 32 unos no se puede codificar.
- Resultado: `p1-o0` igual que qemu-ppc (registros y memoria). `p1` a -O2 no acaba: lo siguiente (F1b).
- `jit-host/comparar.py` (68k): la línea "Programa ruta" se compara sin la ruta (la referencia se sacó en el
  worktree de U7). Los cinco programas 68k, bien en los dos órdenes.

### 2026-11-07 — F1b: enteros

- `pruebas/gen_t1.py a|b` genera `t1a.s` y `t1b.s` (con un `.map` de qué es cada registro): cada instrucción de
  enteros con 10 valores de borde por operando (0, 1, -1, 0x7fffffff, 0x80000000...), con CA a 0 y a 1 en las que
  lo leen, con y sin punto, con y sin OE. Tras cada una se guardan rD, CR y XER en una tabla; `comparar_ppc.py`
  dice qué instrucción y con qué entradas difiere. t1a: suma, resta, multiplicación, neg (4160 casos); t1b:
  división (sin los casos que PPC deja sin definir), lógicas, desplazamientos (también de 32 a 64), cntlzw, extsb,
  extsh, inmediatos, comparaciones en varios campos, 47 combinaciones de rlwinm/rlwimi/rlwnm, operaciones de CR,
  mcrf, mtcrf, mcrxr (8872 casos).
- `p1` a -O2 colgaba: `addze` sin traducir y Emu68 se queda en `while(1)` tras "UNIMPLEMENTED". En el banco ahora
  para con "sin traducir: opcode en dirección" (`dashos_ppc_unimplemented`).
- **Fallos del JIT de Emu68** arreglados en el parche 0010 (todos los encontró el oráculo):
  - Faltaban `subfe` (solo estaba el atajo `subfe rX,rX,rX`), `addze`, `addme`, `subfze` y `subfme`: hechos con
    `adcs`/`sbcs` (`xoCarry`).
  - Las formas con OE (`addo`, `subfo`, `nego`, `addeo`...) no se despachaban (el bit OE cambia el código
    secundario). Ahora van a la misma función. `mullwo`, `divwo` y `divwuo` siguen sin hacer.
  - OV y SO: con OE, SO se ponía siempre; tiene que quedarse como estaba si no hay desbordamiento.
  - CR0 de las aritméticas con punto: con desbordamiento, LT salía de N != V; ahora se compara el resultado con 0.
  - `srawi`/`sraw`: CA con fuentes positivas (la condición estaba al revés).
  - `slw`, `srw`, `sraw`: el desplazamiento es de 6 bits (de 32 a 63, todo fuera o todo signo); tomaba 5.
  - `crandc`, `crnand`, `crnor`: negaban (`neg`) en vez de invertir los bits; `crnor d,a,a` con d distinto de a
    invertía d en vez de copiar a invertido.
  - `mtcrf`: con máscara parcial usaba un temporal sin valor en vez de rS; con máscara 0 borraba CR.
- Resultado: `p1-o0`, `p1`, `t1a` y `t1b` iguales en el JIT y en qemu-ppc (registros, CR, XER y memoria). El banco
  del 68k sigue igual (los cinco programas, en los dos órdenes, mismo código que la referencia).
- Sin probar aún: `lwarx`/`stwcx.`, `lwbrx`/`stwbrx`, `dcbz`, `tw`/`twi`, `sc`, `mftb`, `mfspr`/`mtspr`, las
  cargas y guardados con actualización de todos los tamaños. Faltan del todo: `lmw`, `stmw`, `lswi`, `lswx`,
  `stswi`, `stswx`, `mullwo`, `divwo`, `divwuo`.

### 2026-11-07 — F1c: cargas y guardados

- `pruebas/gen_t2.py` (t2, 1033 casos): una zona de 64 KB con un patrón; `lbz/lhz/lha/lwz` y `stb/sth/stw` en las
  formas D, X, con actualización y con rA = 0, con desplazamientos de borde (0, ±4, impares, 255, -256, 256, -257,
  0x3ffc, 0x4000, 0x7fff, -0x8000...) e índices negativos; `lhbrx`, `lwbrx`, `sthbrx`, `stwbrx`; `lmw` y `stmw`
  desde r5, r14, r20, r29 y r31 con desplazamientos (también impares); `lwarx`/`stwcx.` con y sin reserva; `dcbz`.
  La tabla usa r2 de puntero (lmw pisa hasta r31).
- El banco recoge los fallos de memoria (SIGSEGV, SIGBUS, SIGILL) y dice la instrucción PPC (w18) y la dirección.
- **Fallos del JIT de Emu68** (parche 0010):
  - Desplazamientos negativos fuera del rango de `ldur` (-257 y menos) e índices negativos en las formas X: la
    dirección subía 4 GB (SXTX de un registro escrito en 32 bits). Ahora SXTW, exacto mientras la memoria del PPC
    esté por debajo de 2 GB, como en la caja de DashOS.
  - `lmw` y `stmw` no estaban ("need to be interpreted"): traducidos, con el asignador de registros.
  - Las operaciones de caché (`dcbz`, `dcbst`, `dcbf`, `dcbi`, `dcbt`, `dcbtst`, `dcba`) leían rA y rB de los campos
    equivocados: tomaban rA como dirección entera.
  - `dcbz` borraba el bloque de ARM (`dc zva`, 64 bytes); el de los PPC del Amiga es de 32: ahora cuatro `stp` de
    ceros desde la dirección redondeada a 32.
- Resultado: las cinco pruebas iguales en el JIT y en qemu-ppc.
- Pendiente de esta parte: `icbi` borra la caché de traducciones con direcciones altas del kernel de Emu68 (en el
  banco fallaría; hace falta para código que se modifica a sí mismo), `lswi`/`lswx`/`stswi`/`stswx`, `tw`/`twi`,
  `sc`, `mftb`, `mfspr`/`mtspr` de usuario (LR, CTR, XER).

### 2026-11-07 — F1d: la FPU

- `pruebas/p2.c` (double y float como los genera gcc, a -O2 y -O0: las cuatro operaciones, multiplicar y sumar,
  conversiones, comparaciones, raíz por Newton, polinomios) y `pruebas/gen_t3.py` (t3, 6052 casos): cada operación
  con valores de borde (±0, ±1, 0.1, 1e300, -1e-300, el menor denormal, el mayor doble, ±infinito, NaN silencioso,
  con señal y negativo con datos), las de tres operandos con 7 valores, fsel, fneg, fabs, fnabs, frsp, fctiw,
  fctiwz, fmr, fcmpu, fcmpo, y fctiw, frsp, fadd y fadds con los cuatro modos de redondeo (mtfsfi), y mffs.
  Registros de 12 bytes: frD y CR. FPSCR no se compara entero: el JIT no lleva sus bits de excepción; mffs, solo
  los de control. El banco y el oráculo escriben FPSCR al final, que da lo mismo en todas las pruebas.
- **Faltaba**, ahora en el parche 0010: todo el grupo 59 (fadds, fsubs, fmuls, fdivs, fsqrts, fmadds, fmsubs,
  fnmadds, fnmsubs: la operación en doble y el resultado redondeado a simple), lfsu, lfdu, stfsu, stfdu y las ocho
  formas X (lfsx ... stfdux), frsp, fneg, fabs, fnabs, fsel, fmsub, fnmadd, fnmsub, fctiw, fcmpo, mffs, mtfsf,
  mtfsfi, mtfsb0 y mtfsb1 (estos llevan RN de FPSCR al modo de redondeo de ARM).
- **Fallos del JIT de Emu68**:
  - `FPR::allocate()` sacaba el temporal de los registros enteros: un "temporal" de coma flotante podía ser v9,
    que es f1 de PPC (stfs pisaba f1).
  - `lfs` con rA = 0 cargaba un doble; `stfs` con rA = 0 guardaba un registro entero.
  - `fcmpu` con un NaN daba LT en vez de FU.
  - NaN: ARM prefiere el NaN con señal y cambia el signo del NaN del sumando en FNMADD, FNMSUB y FMSUB; PPC toma
    el primero de frA, frB, frC, silenciado (`ppcNaN`, solo en el camino del NaN). fnmadd y fnmsub niegan el
    resultado, también un cero, pero no un NaN.
  - `csel64` de A64.h ponía Rm en el bit 6 en vez del 16 (no lo usaba nadie).
- Lo que se toma de qemu porque PPC no lo define: la palabra alta de fctiw/fctiwz (el signo; 0 con un NaN) y de
  mffs (0).
- Resultado: las ocho pruebas iguales en el JIT y en qemu-ppc; el banco del 68k, igual que la referencia.
- Pendiente: las formas con punto de la FPU y los bits de excepción de FPSCR (FX, VX..., XX, FI, FR, FPRF), fres,
  frsqrte, mcrfs, el redondeo de las simples con doble redondeo en casos raros (no ha salido en las pruebas).

### 2026-11-07 — F1f: la llamada nativa

- El opcode primario 1 (ilegal en todos los PowerPC) con un número de función en sus 26 bits bajos
  (`Emit::native`, con `DASHOS_NATIVE_CALLS`): el bloque acaba con el PC en la instrucción siguiente,
  `NATIVE_INDEX` y `INT.NATIVE` en `PPCState`. El bucle (`PPCMainLoop`), en la comprobación de interrupciones de
  cada vuelta, guarda el contexto, llama a `dashos_ppc_native(ctx, n)`, lo vuelve a cargar y sigue. Una entrada de
  biblioteca es esa instrucción y `blr`, como la línea A del 68k seguida de RTS (parche 0002).
- `pruebas/p3.s`: tres entradas llamadas con `bl`, 20 veces por `bctrl` en un bucle, y una sin función. El banco
  hace las funciones en `host_ppc.cpp`; el oráculo, en su manejador de SIGILL (qemu-ppc da SIGILL con el opcode
  1), que cambia los registros del contexto y salta la instrucción. Igual en los dos: 24 llamadas, resultados 42,
  142, 36, 2^20 y -1.
- Las nueve pruebas, bien; el banco del 68k, igual.

### 2026-11-07 — F1g: saltos

- `pruebas/gen_t4.py` (t4, 2112 casos): cada BO que admite el ensamblador (con y sin restar CTR, condición a 1, a
  0 o ninguna, con y sin el bit y) con BI 0, 2, 5 y 31, tres CR y cuatro CTR (0, 1, 2, 0xffffffff), en bc, bcl,
  bclr, bclrl, bcctr y bcctrl. Registros: si saltó, CTR y LR.
- **Fallos del JIT de Emu68** (parche 0010, ahora `0010-ppc-f1.patch`):
  - `bcctr`/`bcctrl` condicional no se traducía (solo "salta siempre"): ahora PC = condición ? CTR : siguiente.
  - `bclrl` condicional que no salta no ponía LR (PPC lo pone salte o no).
- Las diez pruebas iguales en el JIT y en qemu-ppc; el banco del 68k, igual que la referencia.

## Dónde se queda F1 (2026-11-07)

Probado contra qemu-ppc: enteros (todas las formas, con CA, OE y punto), cargas y guardados (todas las formas,
lmw/stmw, reserva, dcbz), FPU de doble y simple (con NaN, infinitos, denormales y los cuatro redondeos), saltos
condicionales de todas las formas y la llamada nativa. 26 fallos o faltas del JIT de Emu68 arreglados en
`patches/emu68/0010-ppc-f1.patch`, que se podría mandar a Emu68 (con permiso del usuario). Falta (F1h): icbi en el
banco, las instrucciones de cadenas, tw/twi, sc, mftb, mullwo/divwo/divwuo, las formas con punto de la FPU y los bits
de excepción de FPSCR. Después de F1 viene WarpOS/PowerUP (F2 en `S17-caja-powerpc.md`), que necesita la caja.

### 2026-11-07 — Los arreglos del JIT PPC, a Emu68 (con permiso del usuario)

- PR https://github.com/michalsc/Emu68/pull/354 desde el fork FreeMEM/Emu68, rama `ppc-jit-fixes` (un commit sobre
  af63580). Es el parche 0010 sin lo de DashOS (llamada nativa, contador y parada del banco, `PPC_SO_PROPAGATION`
  con `#ifndef`) y con los comentarios en inglés. Comprobado: sin comentarios, el código solo difiere de 0010 en
  eso; compila sin las definiciones de DashOS. No se ha compilado el Emu68 entero para la Pi.
- En el texto se le deja a Michal decidir lo de SXTW (exacto con la memoria PPC por debajo de 2 GB) o la forma
  exacta con una instrucción más.
- Worktree del clon de Emu68 en `.worktrees/emu68-upstream`. Si lo acepta, el parche 0010 se parte en lo de Emu68
  y lo de DashOS.

### 2026-11-07 — F1h (1): mullwo, divwo, divwuo

- Rama `ppc-f1h` (worktree `.worktrees/ppc-f1h`, con su propio clon de Emu68). Lo nuevo del JIT va en
  `patches/emu68/0011-ppc-f1h.patch`; el 0010 queda como se mandó a Emu68.
- `pruebas/gen_t5.py` (t5, 4074 casos): `mullwo`, `divwo` y `divwuo`, con y sin punto, con 15 valores por operando
  (también la división entre 0 y 0x80000000 / -1), con XER de entrada a 0, con SO y con CA; y con rD igual a rA o rB.
- **Faltaba**, ahora en el 0011: las tres no se despachaban (el bit OE cambia el código secundario).
- **Fallos del JIT de Emu68**:
  - `divwuo`/`divwo` ponían SO aunque no hubiera desbordamiento (SO se ponía antes de elegir); `mullwo` igual, y
    además comparaba el producto en 32 bits (`cmp` en vez de `cmp64`), así que nunca veía el desbordamiento.
  - `divwo` no veía 0x80000000 / -1 (había un TODO).
- Decisión: en los casos que PPC deja sin definir, rD sale como en qemu-ppc (el divisor pasa a ser 1: rD = rA), pero
  solo en las formas con OE, que ya calculan el desbordamiento. `divw`/`divwu` sin OE siguen con la división de ARM
  tal cual (rD = 0 entre 0), para no añadir instrucciones a la división normal; t1b sigue sin probar esos casos.
- Las once pruebas, bien.

### 2026-11-07 — F1h (2): lswi, lswx, stswi, stswx; mullw como dirección

- `pruebas/gen_t6.py` (t6, 705 casos de 132 bytes): `lswi`/`stswi` con rD 3, 13, 14, 25 y 30 (con la vuelta de r31
  a r0) y de 1 a 32 bytes; `lswx`/`stswx` con rD 3, 14 y 30 y la cuenta de XER de 0 a 108 (127 en stswx), también
  con rA = 0; direcciones sin alinear. Cargas: antes, cada GPR con un centinela, y después todos los GPR y XER a la
  tabla; guardados: escriben en una zona llena de 0xee. rA, rB y el puntero de la tabla quedan siempre fuera de los
  registros que se cargan (lo contrario es una forma no válida: qemu da SIGILL; el JIT no lo comprueba).
- **Faltaban** las cuatro. Cómo van (0011, `PPC_LoadStore.cpp`):
  - `lswi`/`stswi`: la cuenta está en la instrucción, así que se desenrolla como `lmw`/`stmw`: una palabra por
    registro y, en el último, de 1 a 3 bytes con `ldrh`/`ldrb` (los bytes bajos a 0). Sin bucles ni saltos.
  - `lswx`/`stswx`: la cuenta (XER[25-31]) y el registro al que va cada byte solo se saben al ejecutar. Se vuelcan
    todos los GPR (y CR y XER, que van detrás) a `PPCState.GPR` (7 `stp` y 5 `st1`), un bucle copia los bytes entre
    la memoria y ese vector (el byte i a GPR[rD] + i, módulo 128; lswx pone a 0 lo que queda del último registro) y
    lswx los vuelve a cargar. `PPCState.GPR` no tiene datos vivos mientras corre el código traducido: el bucle
    guarda el contexto antes de leerlo. Con cuenta 0 no se hace nada (como qemu). Son unas 40 instrucciones de ARM
    y unos 6 ciclos por byte; son instrucciones raras (gcc no las saca para el 603e/604e/G3/G4 sin `-mstring`).
- **Fallo del JIT de Emu68**: `mullw` usaba `smull` (producto de 64 bits) y dejaba la mitad alta del registro de ARM
  con bits: usado luego como dirección (`lwz 5,0(5)`), la dirección se salía de los 4 GB (en el banco, SIGSEGV).
  Ahora `mul` de 32 bits; `mullwo` hace `smull` en un temporal para ver el desbordamiento y copia la palabra baja.
  Prueba nueva en t5 (el producto 0x10000 * 0x10010, de parte baja 0x00100000).
- Las doce pruebas, bien.

### 2026-11-07 — F1h (3): tw, twi y sc, por los vectores de excepción

- Decisión: el banco no para en un trap ni en `sc` (como con lo que no se traduce), sino que deja que el JIT haga lo
  que hace en Emu68: SRR0, SRR1 y MSR, y saltar al vector alto (0xfff00700, 0xfff00c00). `host_ppc.cpp` pone en esos
  vectores una llamada nativa con el tipo y `rfi`, así que se prueban también la entrada, `rfi` y la vuelta. El
  manejador hace lo mismo que el oráculo con su señal:
  - trap (0x700 con el bit de trap en SRR1): r20 += 1, r21 = SRR0 (la dirección del trap) y SRR0 += 4. El oráculo
    recoge SIGTRAP y hace lo mismo con el contexto de la señal.
  - `sc` (0xc00): qemu-ppc ejecuta la llamada de Linux de r0, y no hay manera de cogerla antes; la prueba usa una que
    no existe (r0 = 0x7777) y el banco responde como Linux: r3 = 38 (ENOSYS) y SO de CR0 a 1.
  - Otra excepción de programa para el banco con "excepción" y código 7.
- `pruebas/gen_t7.py` (t7, 1829 casos): `tw` con los 32 TO y 8 pares (con y sin signo), `twi` con los 32 TO, 7
  valores y 7 inmediatos (cortos, largos y negativos, que Emu68 compara de tres maneras), y `sc` con 5 CR. Tras cada
  trap: r20, r21 y r25, que se cambia en el mismo bloque justo antes; tras `sc`, r3, CR y r20 (+1 en la siguiente).
- **Fallo del JIT de Emu68**: `tw`/`twi` condicionales emitían la salida por la excepción (que vacía los registros
  y carga el puntero al contexto) sin guardar el estado del asignador, y el camino sin trap seguía como si los
  registros sucios ya estuvieran guardados y el puntero cargado: con un registro sucio, el banco caía (SIGSEGV en la
  dirección 0). Ahora `save()`/`restore()` alrededor, como `SupervisorCheck`.
- **Mejora**: SRR1 lleva la causa de la excepción de programa (0x00020000 trap, 0x00040000 instrucción privilegiada,
  como en el 603e/750); `emitException` tiene un argumento para eso. `rfi` borra de MSR los bits 17-23 (POW y esas
  causas) en vez de solo POW, para que la causa no pase a MSR.
- Las trece pruebas, bien.

### 2026-11-07 — F1h (4): mftb

- El contador de tiempo no puede coincidir: el JIT lee `CNTPCT_EL0` de ARM (en el banco `CNTVCT_EL0`, que da
  qemu-aarch64_be) y qemu-ppc, el reloj de la máquina, a otra frecuencia. Decisión: no se comparan valores, sino lo
  que tiene que cumplir el contador, y la prueba deja a 0 los registros que lo tuvieron.
- `pruebas/p4.s`: lectura de 64 bits con `mftbu`/`mftb`/`mftbu` (otra vez si TBU cambia); TB avanza a lo largo de
  un bucle de 196608 vueltas; 1000 lecturas seguidas nunca menores que la anterior; 1000 lecturas de TBL con
  `mfspr 268` que no bajan sin que cambie TBU; `mfspr 269` igual que `mftbu`. Cuatro palabras de resultado.
- **Faltaba**: `mfspr rD,268/269` (TBL, TBU), que es como codifican `mftb` los ensambladores nuevos (para POWER4 y
  después) y que qemu-ppc acepta en el 750: ahora va a la misma función que `mftb`.
- Las catorce pruebas, bien.

### 2026-11-07 — F1h (5): icbi y el código que se modifica a sí mismo

- `pruebas/p5.s`: (1) una rutina en 0x00120000 (`li 3,N; blr`) que se reescribe y se llama con `bctrl` 10 veces;
  (2) una instrucción del propio programa, dentro de un bucle, ya traducida en la vuelta anterior y cambiada antes de
  llegar a ella; (3) una rutina llamada con `bl` (que el JIT puede meter en el bloque que llama) con sus dos
  instrucciones cambiadas cada vez. Siempre `stw`, `dcbst`, `sync`, `icbi` e `isync`, como pide PPC.
- **Fallos del JIT de Emu68** (los tres los encontró p5):
  - `icbi` vacía la caché LRU de bloques con una dirección hecha de los 32 bits bajos y los bits 39-63 a 1, que solo
    vale con el núcleo de Emu68 en 0xffffff80xxxxxxxx: en el banco (y en DashOS) caía. Ahora carga la dirección
    entera de 64 bits (cuatro `mov`). La segunda parte (la tabla `alloc`) usaba `movk` de 32 bits, que borra la mitad
    alta del registro: ahora `movk` de 64.
  - Aun así se volvía a ejecutar el código viejo: el traductor lee el código PPC por la caché de instrucciones por
    software de Emu68 (`cache_read_32(ICACHE, ...)`, la de `cache.c`), y `icbi` no la tocaba; ni la comprobación del
    bloque (la huella y el CRC) ni la traducción nueva veían el cambio. `icbi` solo sube EPOCH (desde el código
    traducido no se puede llamar a C), así que el bucle (`PPCMainLoop`), en el camino lento, vacía esa caché entera
    cuando ve un EPOCH nuevo. PPC deja que `icbi` invalide más de lo pedido; el coste es el de volver a llenarla.
- Las quince pruebas, bien.

### 2026-11-07 — F1e: los bits de excepción de FPSCR y las formas con punto de la FPU

- **El oráculo daba siempre FPSCR = 0 al final**: en el contexto de la señal, FPSCR es un `double` y se convertía su
  valor a entero. Ahora se copian sus 8 bytes y se toma la palabra baja. Desde aquí el FPSCR final se compara de
  verdad en todas las pruebas (p2 y t3 acaban con FX, VX, OX, UX, ZX, XX y varios VX puestos).
- `comparar_ppc.py`: un mapa puede empezar por `#tam=16` (registros de 16 bytes); `JIT=...` elige otro banco.
- `pruebas/gen_t8.py` (t8, 11115 casos de 16 bytes: frD, CR y FPSCR con `mffs`): cada caso parte de un FPSCR
  conocido (`mtfsf 0xff`). Las aritméticas de doble y simple con y sin punto y valores que dan cada excepción
  (inexacto, desbordamiento, subdesbordamiento, división entre 0, NaN con señal, inf - inf, inf / inf, 0 / 0,
  inf * 0), multiplicar y sumar con punto, `frsp`, `fctiw`, `fctiwz`, `fcmpu`, `fcmpo`, las que no cambian FPSCR con
  punto, `mtfsf`, `mtfsfi`, `mtfsb0`, `mtfsb1` (con y sin punto), `mcrfs` y secuencias (bits pegajosos, FI y FPRF de
  la última, frD igual a una fuente). Sin estados con una excepción habilitada: qemu-ppc (linux-user) da SIGFPE.
- Cómo va (0011, `PPC_FPU.cpp`): ARM lleva sus banderas pegajosas en FPSR (IOC, DZC, OFC, UFC, IXC). Una operación
  "completa" vacía FPSR antes y después la pasa a FPSCR: OX, UX, ZX y XX (pegajosos), FI (esta fue inexacta), FX, y
  con IOC el bit VX de la causa, que se saca de los operandos solo en ese camino (VXSNAN, VXISI, VXIDI, VXZDZ, VXIMZ,
  VXVC, VXSQRT, VXCVI); FPRF de la clase del resultado; VX y FEX como resúmenes. Las formas con punto copian
  FPSCR[0-3] a CR1. `mtfsf`/`mtfsfi`/`mtfsb*` ya no escriben FEX ni VX (salen de los demás) ni el bit reservado 20;
  `mcrfs`, que faltaba, copia el campo y borra sus bits de excepción.
- **Velocidad** (`config.h`, `PPC_FPSCR_EXCEPTIONS`, como `PPC_SO_PROPAGATION`): con 1 (el banco) todas las
  operaciones son completas (unas 40-60 instrucciones de ARM más cada una). Con 0, lo que trae Emu68 por defecto,
  solo lo son las formas con punto: las demás se traducen como antes y sus OX, UX, ZX y XX se quedan en FPSR, que no
  cuesta nada y se recoge en FPSCR (con FX) al leerlo (`mffs`, `mtfs*`, `mcrfs`, una forma con punto) y al guardar el
  contexto (`PPC_ExecutionLoop.cpp`; al cargarlo, FPSR se vacía). Con 0 se pierden: los bits VX de las operaciones
  sin punto (de IOC no se sabe la causa), FI, FPRF y FPCC (solo los ponen las formas con punto; `fcmpu`/`fcmpo` no
  tienen) y, si se guarda con `stfs` un doble que no es de precisión simple, su `fcvt` puede poner XX, OX o UX de
  más (los compiladores hacen `frsp` antes). `pruebas/gen_t9.py` (t9, 390 casos) prueba ese camino con secuencias en
  las que da lo mismo que qemu-ppc (operaciones sin punto que no son inválidas y luego una con punto, `mffs`,
  `mcrfs`, `mtfsb1`, una llamada nativa en medio, y la recogida al guardar el contexto al final).
- Decisiones, donde qemu-ppc y un PPC de verdad no hacen lo mismo se sigue a qemu (es el oráculo):
  - FX se pone con cada excepción, aunque su bit ya estuviera a 1 (en el 750, solo si pasa de 0 a 1). Para hacerlo
    como el 750 bastaría comparar con el FPSCR de antes.
  - FR no se calcula nunca (qemu tampoco; haría falta el resultado exacto) y se deja como está.
  - `fcmpu`/`fcmpo` borran FI; `fcmpo` con un NaN pone también C (FPRF de NaN); `fctiw` inválida pone FPRF de NaN y
    borra FR y FI; `mtfsb1` de un bit de excepción no pone FX; inf * 0 + NaN con señal da solo VXIMZ.
  - Una excepción habilitada (VE, OE, UE, ZE, XE) solo se ve en FEX: ni excepción de programa ni el resultado que
    pide el modo habilitado. Sin probar (SIGFPE en qemu-ppc).
  - El subdesbordamiento: ARM decide si el resultado es diminuto después de redondear y PPC antes; difiere solo en
    el caso límite que redondea al menor normal. No ha salido en las pruebas.
  - `fsqrt` no está en el 750 (qemu-ppc da SIGILL): su VXSQRT queda sin probar.
- **Fallos del JIT de Emu68** (encontrados por t8 y por el oráculo arreglado):
  - Con frD igual a una fuente (`fadd 1,1,4`), el NaN de PPC (`ppcNaN`) leía la fuente ya pisada por el resultado:
    con un NaN silencioso en frA y uno con señal en frB salía el de frB. Ahora el resultado va a un temporal y luego
    a frD, solo en ese caso (una instrucción más).
  - El doble redondeo de las de precisión simple (F1d) da otro resultado con 2^24 + 1 más un valor pequeño: t8 no
    lo usa; sigue pendiente.
- Una lección del asignador de registros de Emu68: el código que solo se ejecuta a veces (detrás de un salto) no puede
  pedir temporales, porque pedirlo puede vaciar un registro PPC a su sitio y ese vaciado no se ejecutaría por el
  otro camino; y con más de unos 10 temporales a la vez se vacía el propio registro que se está usando (FPSCR). Los
  ayudantes de `PPC_FPU.cpp` reciben los temporales hechos y los piden en ámbitos cortos.
- Las diecisiete pruebas, bien (con `FPSCR=0`: todas salvo p2, p2-o0, t3 y t8, que difieren solo en FPSCR). El banco
  del 68k, igual que la referencia en los dos órdenes.

## Dónde se queda F1 tras F1h y F1e (2026-11-07)

Todo F1 hecho y probado contra qemu-ppc: las 17 pruebas en el banco (`PPC_FPSCR_EXCEPTIONS=1`) y 13 de 17 (todas
salvo p2, p2-o0, t3 y t8) con el camino rápido (`FPSCR=0`). Lo nuevo del JIT está en `patches/emu68/0011-ppc-f1h.patch` (sobre el
0010, que no cambia). Queda: el doble redondeo de las de precisión simple (F1d), `fres` y `frsqrte` (no están en el
JIT), FR, las excepciones de la FPU habilitadas (MSR[FE0,FE1] y la excepción de programa), `fsqrt` sin probar, y
mandar a Emu68 lo de 0011 que no es de DashOS (si el usuario quiere, como el PR #354).
