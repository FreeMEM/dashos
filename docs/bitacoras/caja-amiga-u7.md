# U7: la caja Amiga en CPU sin big-endian (bitácora)

Encargo (2026-11-12): el JIT de Emu68 en little-endian, con la memoria del 68k en big-endian y un giro de bytes en
cada acceso. Para los ARM sin big-endian en EL0 (Apple, quizá Snapdragon X; `docs/diseño/S24-ordenadores-arm.md`).
Análisis en `docs/diseño/U7-caja-sin-big-endian.md`. Worktree `.worktrees/caja-amiga-u7`, rama `caja-amiga-u7` (sale
de `revision`, cb104b1). Commits locales.

## 2026-11-12

### De dónde se parte

El giro ya estaba hecho en el banco (`jit-host`) desde el 2026-11-07, pasos U7a a U7g (`docs/UEFI_LOOP.md`):
parches `0004-u7-base`, `0005-u7-direcciones-efectivas`, `0006-u7-instrucciones`, `0007-u7-fpu` y
`0009-u7-excepciones` (`0008` es solo del banco), con `make ORDEN=le` y `comparar.py`. Lectura del flujo de
instrucciones (`cache_read_*`), direcciones efectivas, instrucciones sueltas, FPU y marcos de excepción: hechos.
Faltaba medir el coste y probar lo delicado más a fondo. Eso es lo de hoy.

### Lo hecho

- `pruebas68k/t4.s`, programa nuevo:
  - Accesos de palabra, largo y FPU a direcciones impares (el 68020 los admite), también cruzando 8 bytes: MOVE en
    todos los modos, aritmética sobre memoria, desplazamientos de memoria, MOVEM.L y MOVEM.W, CMPM, ADDX/SUBX con
    predecremento, MULU.L, DIVS.W, CMP2.L, campos de bits, FMOVE de todos los formatos y FMOVEM.X.
  - Cargas de 16 bits con signo (en little-endian, `ldrh` + `rev16` + `sxth`) por cada camino del JIT, con valores
    negativos: MOVEA.W en todos los modos (desplazamiento negativo, impar mayor que 255, mayor que 8192, índice,
    absoluto, relativo al PC, indirecto en memoria con desplazamientos de palabra negativos leídos del flujo de
    instrucciones), ADDA/SUBA/CMPA.W, MULS/DIVS/CHK.W, MOVEM.W a registros, PACK, ASR/ASL de memoria, CMP2.W y
    FMOVE.W por los cinco caminos de la FPU.
  - MOVES en las dos direcciones y los tres tamaños, MOVES.W a un registro de dirección (con signo).
  - Indirecto en memoria: guardados, base suprimida, posindexado y preindexado, desplazamiento base negativo.
  - FPU con funciones de libm y operandos de memoria de todos los formatos: FETOX, FLOGN, FLOG10, FLOG2, FTWOTOX,
    FTENTOX, FSINH, FCOSH, FTANH, FATAN, FASIN, FACOS, FATANH, FTAN, FCOS, FSINCOS, FETOXM1, FLOGNP1, FMOD, FREM,
    FSCALE, FGETEXP, FGETMAN, FINTRZ, FINT, FABS, FNEG, FSQRT, FDIV, FSGLDIV, FSGLMUL, FMOVE.P, FMOVECR, FPIAR.
  - Los resultados de t4 están mirados a mano, además de iguales en los dos órdenes: las funciones de libm dan lo
    que deben (e^0,5 = 1,6487, ln 3, log10 1000 = 3...), las cargas con signo y MOVES también.
- `pruebas68k/t5.s`: el rastreo de banderas por un DBcc (ver el segundo fallo).
- **Fallo de U7 (1)**, en `0012-u7-arreglos.patch`: en `EMIT_StoreToEffectiveAddress`, un `else` sin llaves dejaba
  fuera el giro del puntero. Un guardado a `([bd])` (base suprimida) giraba el puntero dos veces y escribía en otra
  parte. t4 cae sin el arreglo, en little-endian.
- **Fallo de U7 (2)**, también en 0012: `M68K_GetSRMask` (decide qué banderas hace falta calcular mirando las
  instrucciones siguientes) leía el desplazamiento de un DBcc sin girarlo. En little-endian seguía un camino que no
  es: a veces calculaba banderas de más (un bucle de solo registros ejecutaba un 9,5 % más de instrucciones ARM) y a
  veces de menos, con resultados mal. t5 lo provoca: sin el arreglo, ADDX da 0 en vez de 1. En big-endian el arreglo
  no cambia nada (`BE16` no hace nada).
- **Fallo de Emu68** (no de U7, también en big-endian), en `0013-indirecto-suma-32-bits.patch`: en el indirecto en
  memoria con base An o PC, el puntero se leía con `ldr [bd, An, uxtw]`, una suma de 64 bits. Con el desplazamiento
  base negativo, `([-4,a1],d1.l*2)`, la dirección salía de los 4 GB y el banco caía con fallo de segmento. Ahora se
  suma en 32 bits, como el 68k, y se carga de ahí: una instrucción más en esos modos, en los dos órdenes. En la Pi
  no está comprobado si se notaba (depende de lo que haya proyectado por encima de 4 GB); en un proceso de DashOS,
  sí.
- Como 0013 cambia el código big-endian de esos modos, la referencia de `comparar.py` sale ahora de Emu68 con
  0001-0003, 0008 y 0013 en su versión sin U7 (`pruebas68k/ref/0013-sin-u7.patch`, el mismo cambio sobre las líneas
  sin U7). Referencias rehechas para todos los programas; frente a las de antes solo cambian los bloques con
  indirecto en memoria (`t1`, `t3`) y la ruta del programa en la primera línea.
- Lo que Emu68 no hace y no es de U7 (fuera de t4, apuntado en el programa): FMOVEM.X con lista dinámica no guarda
  nada, FMOVE.P con factor k negativo no lo respeta, y con k positivo trunca en vez de redondear.
- Herramientas: `make -C jit-host run-le` y `make -C jit-host comparar` (los dos órdenes y `comparar.py`);
  `comparar.py ... t4 t5` mira solo esos programas; el banco escribe "medida: N instrucciones 68k en T us" del
  programa de fuera (sin "[" delante, para que `comparar.py` no lo mire). `jit-host/medidas/` para el coste.

### El coste

`jit-host/medidas/`: seis programas 68k en C (`banco.c`, uno por `-DK_<nombre>`) y un plugin de QEMU
(`cuenta.c`) que cuenta las instrucciones ARM ejecutadas dentro del código traducido. `make -C jit-host/medidas` y
`python3 jit-host/medidas/medir.py`. Se restan las pruebas fijas del banco.

| Programa | Instr. 68k | ARM por instr. 68k (BE) | (LE) | Instr. ARM de más |
|---|---:|---:|---:|---:|
| registros (solo registros) | 39,0 M | 1,62 | 1,62 | 0 % |
| crc (tabla de largos, bytes) | 9,9 M | 1,88 | 1,98 | 5,3 % |
| ordenar (palabras con signo) | 3,6 M | 3,19 | 3,43 | 7,2 % |
| copiar (largos y palabras) | 4,6 M | 2,45 | 2,88 | 17,6 % |
| lista (estructuras enlazadas) | 4,1 M | 3,54 | 4,17 | 18,0 % |
| fpu (Double en tablas) | 1,7 M | 2,67 | 3,24 | 21,3 % |

- En los programas de prueba, el código traducido crece un 12,5 % en conjunto (del 0,8 % de t5 al 23 % de t4, que
  es casi todo memoria).
- El tiempo en QEMU no sirve para comparar: QEMU, en un PC, da la vuelta a los bytes de cada acceso cuando emula
  big-endian, así que el banco big-endian también paga el giro. Salen diferencias del -27 % al +13 % según el
  programa. Lo que vale es la cuenta de instrucciones.
- En una CPU de verdad, cada giro es un REV (una instrucción simple, de un ciclo). En las anchas (Apple M, Oryon) se
  espera menos coste que el que dice la cuenta, salvo donde el valor cargado se usa enseguida (seguir punteros,
  `lista`): ahí el REV alarga la cadena un ciclo. **No medido en hardware.**

### Lo probado

```
$ make -C jit-host comparar
c1-o0: big-endian bien: 88 bloques iguales, 7 con solo direcciones distintas
c1-o0: little-endian bien: la misma salida
c1: big-endian bien: 92 bloques iguales, 11 con solo direcciones distintas
c1: little-endian bien: la misma salida
t1: big-endian bien: 8 bloques iguales, 5 con solo direcciones distintas
t1: little-endian bien: la misma salida
t2: big-endian bien: 7 bloques iguales, 15 con solo direcciones distintas
t2: little-endian bien: la misma salida
t3: big-endian bien: 8 bloques iguales, 31 con solo direcciones distintas
t3: little-endian bien: la misma salida
t4: big-endian bien: 8 bloques iguales, 6 con solo direcciones distintas
t4: little-endian bien: la misma salida
t5: big-endian bien: 9 bloques iguales, 2 con solo direcciones distintas
t5: little-endian bien: la misma salida
```

- "big-endian bien": el código que genera el JIT big-endian con todos los parches es el de Emu68 sin U7, salvo
  direcciones de funciones de C. "little-endian bien": mismos registros, FPU y suma de toda la memoria del 68k.
- `make -C jit-host run` y `make -C jit-host run-le`: las dos pruebas fijas (bucle y llamadas nativas) BIEN.
- Los seis programas de `medidas/` dan lo mismo en los dos órdenes.
- Los parches 0001 a 0013 se aplican limpios sobre Emu68 af63580 y dan el mismo árbol que el del worktree.
- 0012 y 0013 se aplican también sobre el Emu68 de la rama `caja-amiga` tal como está hoy (probado en una copia de
  sus ficheros, sin tocar su árbol). No chocan con su trabajo: los dos solo tocan `M68k_EA.c` y `M68k_SR.c`, y la
  rama `caja-amiga` no toca `jit-host/`.
- No se toca `kernel/` ni `usuario/`: sin `make prueba` ni `make arranque-virt`.

### Lo que falta

- Probarlo en una CPU sin big-endian de verdad (un Mac con Asahi, un portátil Snapdragon X con Linux) y medir el
  tiempo ahí. Todo lo de hoy es QEMU de usuario.
- Llevarlo a la caja de DashOS (el proceso de la caja en little-endian) cuando la rama `caja-amiga` lo tenga.
- Bajar el coste, si hace falta: un MOVE de memoria a memoria sin banderas vivas no necesita girar nada (hoy gira al
  cargar y al guardar; `copiar` bajaría bastante); los Double de la FPU, igual cuando solo se copian.
- Sin probar: interrupciones (el banco no tiene), fallos de bus y de MMU (marcos de formato 3 y 4), la excepción de
  privilegio, la parte PPC de Emu68. En macOS el JIT no va tal cual (x18 y `MAP_JIT`, ver U7).
