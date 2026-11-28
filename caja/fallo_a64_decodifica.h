/*
    DashOS, K6 (docs/CHIPS_LOOP.md): la decodificación de las cargas y los guardados de A64 de un fallo de datos (qué
    registros, cuántos bytes, qué dirección, si actualiza la base), sin nada de los chips ni del sistema. La usan el
    manejador de los chips (fallo_a64.h, motor_emu68.c), el trampolín (K7) y la caja de Linux (su manejador de SIGSEGV
    rellena un marco_fallo desde el ucontext).
*/
#ifndef DASHOS_FALLO_A64_DECODIFICA_H
#define DASHOS_FALLO_A64_DECODIFICA_H

#include <stdint.h>

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
    /* Campo a campo: una estructura literal puede ser una llamada a memset, que en la caja usa SIMD (K7). */
    a->escribe = 0; a->tam = 0; a->pares = 1; a->rt = i & 31; a->rt2 = 0; a->rn = (i >> 5) & 31; a->signo = 0;
    a->x = 0; a->dir = 0; a->actualiza = 0; a->base_nueva = 0; a->estado = -1;
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

#endif
