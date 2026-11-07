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
- [ ] F1e Las formas con punto de la FPU (CR1 desde las excepciones de FPSCR) y los bits de excepción de FPSCR.
- [x] F1f La llamada nativa por instrucción reservada (como la línea A del 68k, C1).
- [x] F1g Saltos condicionales: todos los BO en bc, bcl, bclr, bclrl, bcctr, bcctrl.
- [ ] F1h `icbi` en el banco (código que se modifica a sí mismo), `lswi`/`lswx`/`stswi`/`stswx`, `tw`/`twi`, `sc`,
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
``` El banco PPC se compila con `PPC_SO_PROPAGATION=1`
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
