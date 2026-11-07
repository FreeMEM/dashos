# Caja PowerPC: bitácora

Plan y diseño: `docs/diseño/S17-caja-powerpc.md`. Decisiones del usuario (2026-11-07): una sola caja con dos motores
(68k y PPC); orden WarpOS/PowerUP → MorphOS → AltiVec → OS4 al final (OS4 entero, de momento solo con QEMU); la
fase 1 (el JIT PPC de Emu68 en el banco) empieza ya, antes de la caja.

Sala limpia: se usa el JIT PPC de Emu68 (`src/PPC_*.cpp`, MPL-2.0). `src/ppc/` (el ROM PPC, con código que viene de
SonnetAmiga) no se compila ni se lee: el banco lleva un `ppc_rom.h` vacío.

## F1: el JIT PPC en el banco

- [x] F1a Banco `jit-host/ppc/` (Emu68 PPC sin libc en qemu-aarch64_be), oráculo en qemu-ppc, comparación.
- [x] F1b Enteros: lo que traduce el JIT, con programas en C a -O0 y -O2 y en ensamblador.
- [ ] F1c `lmw` y `stmw`.
- [ ] F1d FPU de doble precisión (lo que hay) y las formas con actualización (`lfsu`, `stfdu`...).
- [ ] F1e FPU de precisión simple (grupo 59).
- [ ] F1f La llamada nativa por instrucción reservada (como la línea A del 68k, C1).
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
commit "base"; `0010` sale de su `git diff`. El banco PPC se compila con `PPC_SO_PROPAGATION=1`
(SO de XER copiado a CR, como en un PPC de verdad; Emu68 lo deja a 0 por velocidad).

## Entradas

### 2026-11-07 — F1a: el banco del JIT PPC

- `jit-host/ppc/`: `Makefile`, `host_ppc.cpp` (memoria, carga, `PPCState` como `StartupPPC`, llamada a
  `PPCMainLoop`, constructores de C++ a mano porque no hay libc), `include/ppc_rom.h` vacío, `comparar_ppc.py`.
- `pruebas/`: `prueba.ld` (binario en 0x00140000), `oraculo.c` (programa de Linux PPC estático para qemu-ppc; el
  manejador de la señal recupera r2 y r13 de glibc antes de escribir), `p1.c` (enteros, saltos, llamadas
  recursivas, tabla de saltos, divisiones, 64 bits en 32, estructuras, comparaciones) a -O0 y -O2. Las pruebas se
  compilan sin PIE: el compilador de Bootlin lo da por defecto, y con PIE `lis` sale a 0 (reubicaciones sin aplicar).
- Parche `0010-ppc-banco-y-enteros.patch` (entonces `-rlwinm`):
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
