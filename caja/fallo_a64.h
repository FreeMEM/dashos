/*
    DashOS, K6 (docs/CHIPS_LOOP.md): las cargas y los guardados de A64 que fallan en los chips del Amiga
    (decodificados con fallo_a64_decodifica.h), imitados con caja_chip_leer y caja_chip_escribir (motor.h). La usa el
    manejador de fallos de motor_emu68.c; aparte, para probarla en Linux contra la CPU de verdad (pruebas_fallo/, con
    qemu-user, en big-endian y en little-endian).
*/
#ifndef DASHOS_FALLO_A64_H
#define DASHOS_FALLO_A64_H

#include <stdint.h>
#include "motor.h"
#include "fallo_a64_decodifica.h"

#define CHIPS_CIA    0x00BF0000u    /* los CIA: CIA-A en $BFE001, CIA-B en $BFD000 */
#define CHIPS_CUSTOM 0x00DF0000u    /* los registros de los chips: $DFF000-$DFF1FF */
#define CHIPS_ZONA   0x00010000u    /* cada zona, 64 KB */

/* Un trozo de memoria tal como lo vería la instrucción: los bytes en el orden del 68k, leídos o escritos en el orden
   de la caja. Así la carga da al registro lo mismo que daría sobre la memoria del 68k (en big-endian, el valor; en
   little-endian, los bytes girados, que el REV que sigue en el código traducido endereza), y el guardado saca del
   registro los bytes que habría escrito. */
typedef union { uint8_t b[8]; uint16_t h; uint32_t w; uint64_t d; } trozo;

static inline uint64_t trozo_cargar(const trozo *t, unsigned tam)
{
    return tam == 1 ? t->b[0] : tam == 2 ? t->h : tam == 4 ? t->w : t->d;
}

static inline void trozo_guardar(trozo *t, unsigned tam, uint64_t v)
{
    if (tam == 1) t->b[0] = (uint8_t)v;
    else if (tam == 2) t->h = (uint16_t)v;
    else if (tam == 4) t->w = (uint32_t)v;
    else t->d = v;
}

/* De los chips a los bytes del 68k (y al revés), de 4 en 4 como mucho: un registro de 8 bytes son dos accesos largos,
   el de la dirección menor primero. */
static inline void chips_leer(uint32_t dir, trozo *t, unsigned tam, uint64_t hora)
{
    for (unsigned hecho = 0; hecho < tam; hecho += 4) {
        unsigned n = tam - hecho < 4 ? tam - hecho : 4;
        uint32_t v = caja_chip_leer(dir + hecho, n, hora);
        for (unsigned k = 0; k < n; k++) t->b[hecho + k] = (uint8_t)(v >> (8 * (n - 1 - k)));
    }
}

static inline void chips_escribir(uint32_t dir, const trozo *t, unsigned tam, uint64_t hora)
{
    for (unsigned hecho = 0; hecho < tam; hecho += 4) {
        unsigned n = tam - hecho < 4 ? tam - hecho : 4;
        uint32_t v = 0;
        for (unsigned k = 0; k < n; k++) v = v << 8 | t->b[hecho + k];
        caja_chip_escribir(dir + hecho, v, n, hora);
    }
}

/* ¿Cae [dir, dir + n) entero en una de las dos zonas? Un acceso que cruza de una zona a lo de fuera (otra zona, RAM o
   nada) no se parte: no es de los chips, y acaba como sin ellos. */
static inline int en_zona(uint64_t dir, unsigned n)
{
    uint64_t fin = dir + n;
    return (dir >= CHIPS_CIA && fin <= CHIPS_CIA + CHIPS_ZONA) || (dir >= CHIPS_CUSTOM && fin <= CHIPS_CUSTOM + CHIPS_ZONA);
}

/* Atiende la instrucción `ins` del marco con los chips, a la hora `hora`: pone los registros y PC + 4 en el marco.
   1 si la atendió; 0 si no sabe decodificarla; -1 si lo que toca no cae entero en una zona de los chips. */
static inline int chips_atiende(marco_fallo *m, uint32_t ins, uint64_t hora, struct acceso *a)
{
    if (!decodifica(ins, m, a)) return 0;
    unsigned largo = a->tam * a->pares;
    if (!en_zona(a->dir, largo) || m->far < a->dir || m->far >= a->dir + largo) return -1;
    unsigned regs[2] = { a->rt, a->rt2 };
    uint64_t valores[2] = { 0, 0 };
    for (int k = 0; k < a->pares; k++) {
        trozo t = { .d = 0 };
        uint32_t dir = (uint32_t)a->dir + (uint32_t)k * a->tam;
        if (a->escribe) {
            trozo_guardar(&t, a->tam, reg_x(m, regs[k]));
            chips_escribir(dir, &t, a->tam, hora);
        } else {
            chips_leer(dir, &t, a->tam, hora);
            uint64_t v = trozo_cargar(&t, a->tam);
            if (a->signo) v = (uint64_t)extiende(v, 8 * a->tam);
            valores[k] = a->x ? v : (uint32_t)v;
        }
    }
    if (a->actualiza) {
        if (a->rn == 31) m->sp = a->base_nueva;
        else m->x[a->rn] = a->base_nueva;
    }
    if (!a->escribe)
        for (int k = 0; k < a->pares; k++)
            if (regs[k] != 31) m->x[regs[k]] = valores[k];
    if (a->estado >= 0 && a->estado != 31) m->x[a->estado] = 0;
    m->pc += 4;
    return 1;
}

#endif
