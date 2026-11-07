# Caja PowerPC: bitácora

Plan y diseño: `docs/diseño/S17-caja-powerpc.md`. Decisiones del usuario (2026-11-07): una sola caja con dos motores
(68k y PPC); orden WarpOS/PowerUP → MorphOS → AltiVec → OS4 al final (OS4 entero, de momento solo con QEMU); la
fase 1 (el JIT PPC de Emu68 en el banco) empieza ya, antes de la caja.

Sala limpia: se usa el JIT PPC de Emu68 (`src/PPC_*.cpp`, MPL-2.0). `src/ppc/` (el ROM PPC, con código que viene de
SonnetAmiga) no se compila ni se lee: el banco lleva un `ppc_rom.h` vacío.

## F1: el JIT PPC en el banco

- [x] F1a Banco `jit-host/ppc/` (Emu68 PPC sin libc en qemu-aarch64_be), oráculo en qemu-ppc, comparación.
- [ ] F1b Enteros: lo que traduce el JIT, con programas en C a -O0 y -O2 y en ensamblador.
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
commit "base"; `0010` sale de su `git diff`.

## Entradas

### 2026-11-07 — F1a: el banco del JIT PPC

- `jit-host/ppc/`: `Makefile`, `host_ppc.cpp` (memoria, carga, `PPCState` como `StartupPPC`, llamada a
  `PPCMainLoop`, constructores de C++ a mano porque no hay libc), `include/ppc_rom.h` vacío, `comparar_ppc.py`.
- `pruebas/`: `prueba.ld` (binario en 0x00140000), `oraculo.c` (programa de Linux PPC estático para qemu-ppc; el
  manejador de la señal recupera r2 y r13 de glibc antes de escribir), `p1.c` (enteros, saltos, llamadas
  recursivas, tabla de saltos, divisiones, 64 bits en 32, estructuras, comparaciones) a -O0 y -O2. Las pruebas se
  compilan sin PIE: el compilador de Bootlin lo da por defecto, y con PIE `lis` sale a 0 (reubicaciones sin aplicar).
- Parche `0010-ppc-banco-y-rlwinm.patch`:
  - En el banco, `CNTPCT_EL0` no se lee desde EL0 en qemu-user: `CNTVCT_EL0` (`sys_CNTPCT_EL0_HOST`).
  - **Fallos del JIT de Emu68** que encontró `p1` a -O0 (`rlwinm r9,r9,0,24,19`):
    - Máscaras que dan la vuelta (MB > ME) en `rlwinm`, `rlwimi` y `rlwnm`: tomaba MB - ME - 1 unos, que son los ceros.
    - `rlwimi` sin rotación usaba un temporal sin valor; con la máscara entera no insertaba nada.
    - `rlwinm` sin rotación y con la máscara entera (`mr` hecho con `rlwinm`) no escribía rA.
    - Con el punto (`rlwinm.`, `rlwimi.`), las formas de desplazar y la de `rlwimi` no ponían las banderas antes
      de `setCRnLogic`; y `tst` con 32 unos no se puede codificar.
    - La máscara complementaria (`bic_immed`) rotaba módulo 64: con más de 32 sale una instrucción reservada.
- Resultado: `p1-o0` igual que qemu-ppc (registros y memoria). `p1` a -O2 no acaba: lo siguiente (F1b).
- `jit-host/comparar.py` (68k): la línea "Programa ruta" se compara sin la ruta (la referencia se sacó en el
  worktree de U7). Los cinco programas 68k, bien en los dos órdenes.
