/*
    DashOS, K7 (docs/CHIPS_LOOP.md): la instrucción reescrita. Tras el primer fallo de un sitio del código traducido en
    los chips (K6, fallo_a64.h), esa carga o ese guardado pasa a ser un `b` a un trampolín propio, que hace el acceso
    sin pasar por el kernel. Lo usa motor_emu68.c; aparte, se prueba en Linux contra la CPU de verdad (pruebas_fallo/,
    con qemu-user, en big-endian y en little-endian).

    El trampolín de un sitio (64 bytes, uno por sitio, en una zona al final del código traducido, a menos de 128 MB de
    todo él):

         0  str  x30, [sp, #-16]!      x30 (LR) puede estar vivo en el código traducido: se guarda en la pila
         4  ldr  x30, 32               la rutina común (trampolin_a64.c), lejos: por registro
         8  blr  x30
        12  ldr  x30, [sp], #16        vuelta de los chips (la común deja x30 final en la pila)
        16  b    sitio + 4
        20  ldr  x30, [sp], #16        vuelta de la RAM (la común vuelve 8 bytes más allá)
        24  <la instrucción original>  el acceso normal, con los registros como estaban
        28  b    sitio + 4
        32  .quad la rutina común      en el orden de la caja
        40  .quad su descripción       (struct trampolin)
        48  udf, udf, udf, udf

    El sitio salta con `b` y el trampolín vuelve con `b`: x30 solo se toca dentro, guardado. La rutina común guarda
    x0-x30, SP y NZCV en un marco como el de K5, llama a caja_trampolin_c y lo repone todo (con lo que cambió del
    acceso). caja_trampolin_c, y lo que llama, no tocan los registros de la FPU y SIMD (trampolin_a64.c se compila con
    -mgeneral-regs-only y caja/chips es Rust softfloat): por eso no se guardan.
*/
#ifndef DASHOS_TRAMPOLIN_A64_H
#define DASHOS_TRAMPOLIN_A64_H

#include <stdint.h>
#include "fallo_a64.h"

#define TRAMPOLIN_BYTES   64u
#define TRAMPOLINES       1024u                                /* 64 KB */
#define TRAMPOLINES_ZONA  (TRAMPOLIN_BYTES * TRAMPOLINES)
/* Un sitio reescrito que va a la RAM tantas veces seguidas vuelve a su instrucción original (deja de pagar el rodeo):
   si vuelve a tocar los chips, falla otra vez y se reescribe otra vez. */
#define TRAMPOLIN_RAM_SEGUIDAS 1024u

struct trampolin {
    uint64_t sitio;            /* la instrucción reescrita, en la proyección de ejecutar */
    uint32_t ins;              /* la original */
    uint32_t ram_seguidas;
};

/* Lo que se cuenta, para las pruebas y las medidas. */
extern uint64_t chips_tramp_accesos;   /* accesos a los chips por un trampolín */
extern uint64_t chips_tramp_ram;       /* accesos a otra cosa (el camino normal) */
extern uint32_t chips_reescritos;      /* sitios reescritos */
extern uint32_t chips_deshechos;       /* sitios devueltos a su instrucción: muchos accesos a la RAM seguidos */
/* 0: no se reescribe nada (los accesos siguen por el fallo). De serie, 1. */
extern int chips_reescribe;

/* La zona de los trampolines: `rw` para escribir y `rx` para ejecutar, la misma memoria (pueden ser iguales). El
   código traducido tiene la misma distancia entre sus dos proyecciones. */
void trampolines_iniciar(uintptr_t rw, uintptr_t rx);
/* Tras atender el fallo de `ins` en `sitio` (proyección de ejecutar) con el acceso `a`: lo reescribe, si se puede.
   1 si quedó reescrito. No se reescriben las exclusivas (un monitor exclusivo no sobrevive al rodeo) ni las de base SP
   (el trampolín usa la pila). Si la zona está llena, deshace todo y empieza de nuevo. */
int trampolin_poner(uint64_t sitio, uint32_t ins, const struct acceso *a);
/* Devuelve a su instrucción todos los sitios que siguen saltando a su trampolín, y vacía la zona. Un sitio que ya no
   lo hace (su unidad se soltó y la memoria es de otra) no se toca. Con el 68k parado, o desde el fallo. */
void trampolines_deshacer(void);
/* ¿Está `pc` (proyección de ejecutar) en la zona de los trampolines? */
int en_trampolines(uint64_t pc);
/* K8: tras una escritura en los chips, a su hora: los pone al día y vuelve a armar el plazo del contador hacia atrás
   (motor_emu68.c; sin tocar la FPU ni SIMD). */
void chips_rearmar(uint64_t hora);

#endif
