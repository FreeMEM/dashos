/*
    DashOS, K6 (docs/CHIPS_LOOP.md): la decodificación de las cargas y los guardados de A64 que fallan en los chips del
    Amiga, y su imitación con caja_chip_leer y caja_chip_escribir (motor.h). La usa el manejador de fallos de
    motor_emu68.c; aparte, para probarla en Linux contra la CPU de verdad (pruebas_fallo/, con qemu-user, en big-endian
    y en little-endian).
*/
#ifndef DASHOS_FALLO_A64_H
#define DASHOS_FALLO_A64_H

#include <stdint.h>
#include "motor.h"

#define CHIPS_CIA    0x00BF0000u    /* los CIA: CIA-A en $BFE001, CIA-B en $BFD000 */
#define CHIPS_CUSTOM 0x00DF0000u    /* los registros de los chips: $DFF000-$DFF1FF */
#define CHIPS_ZONA   0x00010000u    /* cada zona, 64 KB */

/* El marco que escribe el kernel: 36 palabras de 64 bits en el orden de bytes del proceso (el de la caja), así que se
   leen tal cual en las dos cajas. */
typedef struct marco_fallo {
    uint64_t x[31];
    uint64_t sp, pc, pstate, esr, far;
} marco_fallo;
_Static_assert(sizeof(marco_fallo) == 288, "el marco de K5 son 288 bytes");

/* Una carga o un guardado de A64, decodificado. Un par (LDP/STP) son dos registros de `tam` bytes seguidos. */
struct acceso {
    int escribe;
    unsigned tam;          /* bytes de cada registro: 1, 2, 4 u 8 */
    int pares;             /* 1, o 2 en LDP/STP */
    unsigned rt, rt2, rn;  /* 31 en rt o rt2: XZR; en rn: SP */
    int signo;             /* la carga extiende el signo (LDRSB, LDRSH, LDRSW, LDPSW) */
    int x;                 /* el registro es de 64 bits (X); si no, W: arriba queda a 0 */
    uint64_t dir;          /* la primera dirección del acceso */
    int actualiza;         /* pre o post-índice: la base pasa a `base_nueva` */
    uint64_t base_nueva;
    int estado;            /* STXR y STLXR: el registro de estado (rs), que queda a 0 (escrita); si no, -1 */
};

static inline uint64_t reg_x(const marco_fallo *m, unsigned r) { return r == 31 ? 0 : m->x[r]; }
static inline uint64_t reg_base(const marco_fallo *m, unsigned r) { return r == 31 ? m->sp : m->x[r]; }
static inline int64_t extiende(uint64_t v, unsigned bits) { return (int64_t)(v << (64 - bits)) >> (64 - bits); }

/* Las formas que emite el traductor de Emu68 para la memoria del 68k (M68k_EA.c, M68k_MOVE.c, M68k_LINE*.c, con las
   de M68k_Endian.h en little-endian, que son las mismas instrucciones seguidas de un REV): LDR/STR de 1, 2, 4 y 8
   bytes con desplazamiento sin signo (LDR, LDRH, LDRB...), sin escalar (LDUR...), pre y post-índice y con registro;
   las de signo (LDRSB, LDRSH, LDRSW, a W o a X); LDP/STP de W y de X (desplazamiento, pre y post-índice; LDPSW); y
   las exclusivas de un registro de TAS y CAS (LDXR, LDAXR, STXR, STLXR, de 1, 2, 4 y 8 bytes). No las de la FPU
   (FMOVE a un registro de los chips): esas, y las demás, devuelven 0 (ARM DDI 0487, C4.1.66 a C4.1.68). */
static inline int decodifica(uint32_t i, const marco_fallo *m, struct acceso *a)
{
    *a = (struct acceso){ .pares = 1, .rt = i & 31, .rn = (i >> 5) & 31, .estado = -1 };
    uint64_t base = reg_base(m, a->rn);
    unsigned size = i >> 30;

    if ((i & 0x3f000000) == 0x39000000 || (i & 0x3f200000) == 0x38000000 || (i & 0x3f200c00) == 0x38200800) {
        /* Un registro, de enteros (V = 0): size 111 V 0x opc ... */
        unsigned opc = (i >> 22) & 3;
        a->tam = 1u << size;
        switch (opc) {
        case 0: a->escribe = 1; a->x = size == 3; break;
        case 1: a->x = size == 3; break;
        case 2: if (size == 3) return 0;                 /* PRFM */
                a->signo = 1; a->x = 1; break;           /* LDRSB, LDRSH y LDRSW a X */
        default: if (size >= 2) return 0;
                a->signo = 1; break;                     /* LDRSB y LDRSH a W */
        }
        if ((i & 0x3f000000) == 0x39000000) {            /* desplazamiento sin signo, escalado */
            a->dir = base + ((uint64_t)((i >> 10) & 0xfff) << size);
        } else if ((i & 0x00200000) == 0) {              /* imm9: sin escalar, post-índice o pre-índice */
            int64_t d = extiende((i >> 12) & 0x1ff, 9);
            switch ((i >> 10) & 3) {
            case 0: a->dir = base + d; break;
            case 1: a->dir = base; a->actualiza = 1; a->base_nueva = base + d; break;
            case 3: a->dir = base + d; a->actualiza = 1; a->base_nueva = a->dir; break;
            default: return 0;                           /* LDTR/STTR: sin privilegio */
            }
        } else {                                         /* con registro, extendido y quizá escalado */
            uint64_t r = reg_x(m, (i >> 16) & 31);
            switch ((i >> 13) & 7) {
            case 2: r = (uint32_t)r; break;              /* UXTW */
            case 3: break;                               /* LSL (UXTX) */
            case 6: r = (uint64_t)(int64_t)(int32_t)r; break;   /* SXTW */
            case 7: break;                               /* SXTX */
            default: return 0;
            }
            a->dir = base + (r << ((i & 0x1000) ? size : 0));
        }
        return 1;
    }
    if ((i & 0x3e000000) == 0x28000000) {
        /* Par de enteros: opc 101 V 0 tipo L imm7 Rt2 Rn Rt */
        unsigned opc = i >> 30, tipo = (i >> 23) & 3;
        a->escribe = !(i & 0x00400000);
        if (opc == 0) a->tam = 4;
        else if (opc == 2) { a->tam = 8; a->x = 1; }
        else if (opc == 1 && !a->escribe) { a->tam = 4; a->signo = 1; a->x = 1; }   /* LDPSW */
        else return 0;
        a->pares = 2;
        a->rt2 = (i >> 10) & 31;
        int64_t d = extiende((i >> 15) & 0x7f, 7) * (int64_t)a->tam;
        switch (tipo) {
        case 1: a->dir = base; a->actualiza = 1; a->base_nueva = base + d; break;
        case 3: a->dir = base + d; a->actualiza = 1; a->base_nueva = a->dir; break;
        default: a->dir = base + d; break;               /* 2: desplazamiento; 0: LDNP/STNP */
        }
        return 1;
    }
    if ((i & 0x3fa07c00) == 0x08007c00) {
        /* Exclusiva de un registro: size 001000 0 L 0 Rs o0 11111 Rn Rt (LDXR, LDAXR, STXR, STLXR) */
        a->tam = 1u << size;
        a->x = size == 3;
        a->dir = base;
        if (i & 0x00400000) {
            if (((i >> 16) & 31) != 31) return 0;
        } else {
            a->escribe = 1;
            a->estado = (i >> 16) & 31;
        }
        return 1;
    }
    return 0;
}

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
