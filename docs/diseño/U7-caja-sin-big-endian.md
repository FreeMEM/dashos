# U7: la caja Amiga en CPU sin big-endian (análisis)

Del 2026-11-07, para la decisión del usuario del 2026-11-04 (`PLAN.md`, "Caja Amiga sin big-endian"). Es el mapa de lo
que hay que tocar en Emu68 (`third_party/Emu68`) para un modo en que la memoria del 68k siga en big-endian y el JIT y
su código corran en little-endian, dando la vuelta a los bytes en cada acceso de 16, 32 o 64 bits. Nada hecho aún.

## Cuánto es

- Unos **350 accesos** a la memoria del 68k en el código que emite el JIT (más unos 170 de un byte, que no
  cambian). Solo unos 130 pasan por `EMIT_LoadFromEffectiveAddress` / `EMIT_StoreToEffectiveAddress` (`M68k_EA.c`), y
  de esos 70 solo calculan la dirección: unos **280 sitios** están sueltos por `M68k_LINE*.c` y `M68k_MOVE.c`.
- Unos 110 accesos al contexto (`struct M68KState`) y a la pila de ARM, mezclados con los otros, que **no** se giran.
- Estimación: **1100 a 1300 líneas tocadas** (un diff de 2000 a 3000), en cuatro parches (base, direcciones
  efectivas, instrucciones, FPU). En big-endian el código generado puede quedar idéntico byte a byte, y se comprueba
  comparando con el de ahora.

## Lo fácil

- `src/cache.c`: todas las lecturas del flujo de instrucciones pasan por `cache_read_16/32/64`; basta girar ahí.
- Las sumas CRC32 de la caché de traducciones son coherentes consigo mismas: no cambian.
- TAS y MOVEP son de bytes: no cambian.

## Lo delicado

1. **Saltos de distancia fija** dentro de listas de instrucciones (unos 240; los de memoria, sobre todo en campos de
   bits, RTE y CAS2): meter un REV los descuadra y el fallo no se ve.
2. **Guardados desde registros fijos del 68k** (MOVEM, pares de MOVE, CAS): hace falta un temporal girado, y solo hay
   8 (x4-x11).
3. **`ldrsh`**: en little-endian tiene que ser `ldrh` + `rev16` + `sxth`.
4. **FPU**: hacen falta REV de vectores (no están en `A64.h`); la precisión extendida se hace en C
   (`src/math/96bit.c`) y hay que girarla; el formato empaquetado ya va bien en LE y su `ror64` sobra.
5. **Uniones de 16 bits para cargar direcciones** (44 sitios, también el parche 0002 de la llamada nativa): en LE
   salen al revés y el fallo es un salto a basura.
6. **Marcos de excepción y vector escritos desde C** (`M68k_ExceptionEntry.c`, `ExecutionLoop.c`).
7. **MOVE16** copia memoria a memoria: no hay que girarlo.

## Un límite que no es del orden de bytes

En **macOS** el JIT no puede ir tal cual: Emu68 usa x18 como PC del 68k y macOS reserva x18; además macOS exige
`MAP_JIT` con W^X. En un Mac con Linux (Asahi) o con DashOS arrancado directamente no pasa.

## Cómo se probaría

`jit-host` compilado en little-endian (`-mlittle-endian`, otra cadena de compilación) con `qemu-aarch64`, y el
mismo programa 68k que hoy en `qemu-aarch64_be`; y en big-endian, el código generado idéntico al de ahora.
