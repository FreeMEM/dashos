/*
    B1 (docs/CHIPS_LOOP.md): el Blitter visto desde un programa 68k. Hace en chip RAM los blits de blitter_casos.h
    (copia, relleno y líneas), cada uno sobre datos de partida conocidos, y escribe por la consola una suma (CRC32)
    de los 4096 bytes tras cada uno y su BZERO. Al final, un blit con el DMA del Blitter apagado: no se hace hasta
    encenderlo.

    Es el oráculo de caja/chips: el mismo binario en FS-UAE con el Kickstart da las cifras que copia la prueba de
    Rust (caja/chips/tests/blitter.rs), que hace los mismos blits en su imitación. Toca los registros directamente,
    con el Blitter pedido (OwnBlitter) y el sistema parado (Disable) mientras mide; escribe después.

    Duda: BBUSY puede no verse a 1 justo tras escribir BLTSIZE (el manual pide leer DMACONR una vez antes de
    esperar). Aquí se lee una vez y se espera; cuántas veces se vio a 1, se dice como medida, no como fallo.
*/
#include <exec/types.h>
#include <exec/memory.h>
#include <exec/execbase.h>
#include <graphics/gfxbase.h>
#include <dos/dos.h>
#include <proto/exec.h>
#include <proto/dos.h>
#include <proto/graphics.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;
struct GfxBase *GfxBase;

#define R(d) (*(volatile UWORD *)(0xDFF000 + (d)))
#define RL(d) (*(volatile ULONG *)(0xDFF000 + (d)))
enum {
    DMACONR = 0x002, INTREQR = 0x01E, BLTCON0 = 0x040, BLTCON1 = 0x042, BLTAFWM = 0x044, BLTALWM = 0x046,
    BLTCPT = 0x048, BLTBPT = 0x04C, BLTAPT = 0x050, BLTAPTL = 0x052, BLTDPT = 0x054, BLTSIZE = 0x058,
    BLTCMOD = 0x060, BLTBMOD = 0x062, BLTAMOD = 0x064, BLTDMOD = 0x066, BLTCDAT = 0x070, BLTBDAT = 0x072,
    BLTADAT = 0x074, DMACON = 0x096, INTREQ = 0x09C
};
#define BBUSY 0x4000
#define BZERO 0x2000
#define BLTEN 0x0040
#define SETCLR 0x8000
#define INTF_BLIT 0x0040

#define TAM 4096
#define FILA 64     /* bytes por fila del mapa de las líneas */

struct caso {
    WORD linea, relleno;
    LONG semilla;
    /* copia */
    UWORD con0, con1, afwm, alwm;
    LONG aoff, boff, coff, doff;
    WORD amod, bmod, cmod, dmod;
    UWORD adat, bdat, cdat, size;
    /* línea */
    WORD x1, y1, x2, y2;
    UWORD minterm, textura, inicio, sing;
};

#define COPIA(r, s, c0, c1, fw, lw, ao, bo, co, dof, am, bm, cm, dm, ad, bd, cd, sz) \
    { 0, r, s, c0, c1, fw, lw, ao, bo, co, dof, am, bm, cm, dm, ad, bd, cd, sz, 0, 0, 0, 0, 0, 0, 0, 0 },
#define LINEA(r, s, xa, ya, xb, yb, mt, tx, it, sg) \
    { 1, r, s, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, xa, ya, xb, yb, mt, tx, it, sg },
static const struct caso casos[] = {
#include "blitter_casos.h"
};
#define N (sizeof casos / sizeof casos[0])

static ULONG sumas[N + 2];
static UWORD ceros[N + 2];

static void di(const char *f, LONG a, LONG b, LONG c, LONG d)
{
    LONG v[4] = { a, b, c, d };
    VPrintf((CONST_STRPTR)f, v);
}

/* El generador: el de siempre de C (x * 1103515245 + 12345), la palabra alta. */
static ULONG x;
static UWORD azar(void)
{
    x = x * 1103515245UL + 12345UL;
    return (UWORD)(x >> 16);
}

static void llenar(UWORD *m, WORD relleno, LONG semilla)
{
    x = (ULONG)semilla;
    for (int i = 0; i < TAM / 2; i++) {
        if (relleno == 0) m[i] = 0;
        else if (relleno == 1) m[i] = azar();
        else {
            UWORD a = azar(), b = azar();
            m[i] = a & b & azar();
        }
    }
}

static ULONG crc32(const UBYTE *p, ULONG n)
{
    ULONG c = 0xFFFFFFFFUL;
    while (n--) {
        c ^= *p++;
        for (int k = 0; k < 8; k++) c = (c >> 1) ^ (0xEDB88320UL & -(c & 1));
    }
    return ~c;
}

static ULONG vistos_ocupado, vistos_blit;

/* Espera al Blitter (la primera lectura, la que pide el manual) y apunta si estaba ocupado. */
static void espera(void)
{
    if (R(DMACONR) & BBUSY) vistos_ocupado++;
    while (R(DMACONR) & BBUSY) {}
    if (R(INTREQR) & INTF_BLIT) vistos_blit++;
}

static void copia(UBYTE *m, const struct caso *c)
{
    R(BLTCON0) = c->con0;
    R(BLTCON1) = c->con1;
    R(BLTAFWM) = c->afwm;
    R(BLTALWM) = c->alwm;
    if (c->coff >= 0) RL(BLTCPT) = (ULONG)(m + c->coff);
    if (c->boff >= 0) RL(BLTBPT) = (ULONG)(m + c->boff);
    if (c->aoff >= 0) RL(BLTAPT) = (ULONG)(m + c->aoff);
    if (c->doff >= 0) RL(BLTDPT) = (ULONG)(m + c->doff);
    R(BLTAMOD) = (UWORD)c->amod;
    R(BLTBMOD) = (UWORD)c->bmod;
    R(BLTCMOD) = (UWORD)c->cmod;
    R(BLTDMOD) = (UWORD)c->dmod;
    R(BLTCDAT) = c->cdat;
    R(BLTBDAT) = c->bdat;
    R(BLTADAT) = c->adat;
    R(BLTSIZE) = c->size;
}

/* Una línea como dice el manual: el octante por SUD (el paso de siempre es horizontal), SUL y AUL; el acumulador
   4 * dmin - 2 * dmax en BLTAPTL; BLTAMOD 4 * (dmin - dmax), BLTBMOD 4 * dmin; C y D en el primer punto. */
static void linea(UBYTE *m, const struct caso *c)
{
    WORD dx = c->x2 - c->x1, dy = c->y2 - c->y1;
    WORD ax = dx < 0 ? -dx : dx, ay = dy < 0 ? -dy : dy;
    WORD dmax, dmin;
    UWORD oct;
    if (ax >= ay) {
        dmax = ax; dmin = ay;
        oct = 0x10 | (dx < 0 ? 0x04 : 0) | (dy < 0 ? 0x08 : 0);
    } else {
        dmax = ay; dmin = ax;
        oct = (dy < 0 ? 0x04 : 0) | (dx < 0 ? 0x08 : 0);
    }
    WORD acc = 4 * dmin - 2 * dmax;
    UBYTE *p = m + c->y1 * FILA + (c->x1 >> 4) * 2;
    R(BLTCON0) = (UWORD)((c->x1 & 15) << 12) | 0x0B00 | c->minterm;
    R(BLTCON1) = (UWORD)(c->inicio << 12) | oct | (acc < 0 ? 0x40 : 0) | (c->sing ? 0x02 : 0) | 0x01;
    R(BLTAFWM) = 0xFFFF;
    R(BLTALWM) = 0xFFFF;
    RL(BLTCPT) = (ULONG)p;
    RL(BLTDPT) = (ULONG)p;
    R(BLTAPTL) = (UWORD)acc;
    R(BLTAMOD) = (UWORD)(4 * (dmin - dmax));
    R(BLTBMOD) = (UWORD)(4 * dmin);
    R(BLTCMOD) = FILA;
    R(BLTDMOD) = FILA;
    R(BLTADAT) = 0x8000;
    R(BLTBDAT) = c->textura;
    R(BLTSIZE) = (UWORD)(((dmax + 1) << 6) | 2);
}

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    if (!DOSBase) return 20;
    GfxBase = (struct GfxBase *)OpenLibrary((CONST_STRPTR)"graphics.library", 36);
    UBYTE *m = AllocMem(TAM, MEMF_CHIP | MEMF_CLEAR);
    if (!GfxBase || !m) {
        di("blitter: sin graphics.library o sin chip RAM\n", 0, 0, 0, 0);
        return 20;
    }

    OwnBlitter();
    WaitBlit();
    Disable();
    for (ULONG i = 0; i < N; i++) {
        const struct caso *c = &casos[i];
        if (c->relleno >= 0) llenar((UWORD *)m, c->relleno, c->semilla);
        R(INTREQ) = INTF_BLIT;
        if (c->linea) linea(m, c);
        else copia(m, c);
        espera();
        ceros[i] = (R(DMACONR) & BZERO) != 0;
        sumas[i] = crc32(m, TAM);
    }

    /* Sin DMA: BLTSIZE con BLTEN apagado no hace nada (con BBUSY a 1) hasta encenderlo. El caso 1 otra vez. */
    llenar((UWORD *)m, 1, 1);
    R(DMACON) = BLTEN;
    copia(m, &casos[0]);
    for (volatile int k = 0; k < 2000; k++) {}
    UWORD ocupado_sin_dma = (R(DMACONR) & BBUSY) != 0;
    sumas[N] = crc32(m, TAM);
    R(DMACON) = SETCLR | BLTEN;
    espera();
    sumas[N + 1] = crc32(m, TAM);
    ceros[N + 1] = (R(DMACONR) & BZERO) != 0;
    Enable();
    DisownBlitter();

    for (ULONG i = 0; i < N; i++)
        di("blitter: caso %ld: crc %08lx bzero %ld\n", (LONG)(i + 1), (LONG)sumas[i], ceros[i], 0);
    di("blitter: sin dma: bbusy %ld, crc antes %08lx, despues %08lx bzero %ld\n", ocupado_sin_dma, (LONG)sumas[N],
       (LONG)sumas[N + 1], ceros[N + 1]);
    di("blitter: medida: bbusy visto en %ld de %ld, BLIT en INTREQR en %ld\n", (LONG)vistos_ocupado,
       (LONG)(N + 1), (LONG)vistos_blit, 0);

    FreeMem(m, TAM);
    CloseLibrary((struct Library *)GfxBase);
    CloseLibrary((struct Library *)DOSBase);
    return 0;
}
