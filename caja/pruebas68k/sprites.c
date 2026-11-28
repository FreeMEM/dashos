/*
    B6 (docs/CHIPS_LOOP.md, fase 2 adelantada): los sprites y los choques, para compararlos con FS-UAE. Con el sistema
    parado (Disable), pone en chip RAM unos planos y unas listas de sprites con un dibujo conocido y una lista de
    Copper propia que los enseña. Las escenas están en sprites_escenas.h; la prueba de Rust
    (caja/chips/tests/sprites_oraculo.rs) hace lo mismo con los Chips y lo compara con la captura de cada escena y con
    CLXDAT.

    La lista de cada escena (copper(), la misma en la prueba de Rust):
    - Los ocho SPRxPT (lo primero: tienen que estar antes de la línea 25), FMODE a 0, BPLCON3 a $0C00 y BPLCON4 a $0011
      (lo de serie de AGA: sprites de baja y sus colores en 16-31), CLXCON, la ventana, DDF, módulos a 0, BPLCON0-2,
      los punteros de los planos y la paleta.
    - En el grupo 1, lo del Copper con los sprites 1 y 5 (ver sprites_escenas.h).
    - En la línea $AC, BPLCON2 pasa a bplcon2_abajo.

    Cada escena: se pone su lista, se espera a que pasen dos cuadros enteros, se lee CLXDAT (lo borra) al empezar el
    cuadro, se espera otro y se vuelve a leer: los choques de un cuadro entero. Después se queda quieta los segundos
    que diga. Al acabar pone como estaban DMACON, INTENA y las listas del sistema (como denise.c) y escribe una línea
    por escena: "escena N clxdat XXXX".
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

#define CUSTOM16(d) (*(volatile UWORD *)(0xDFF000 + (d)))
#define CUSTOM32(d) (*(volatile ULONG *)(0xDFF000 + (d)))
#define VPOSR_VHPOSR (*(volatile ULONG *)0xDFF004)
#define DMACONR 0x002
#define CLXDAT 0x00E
#define INTENAR 0x01C
#define COP1LC 0x080
#define COP2LCH 0x084
#define COP2LCL 0x086
#define DIWSTRT 0x08E
#define DIWSTOP 0x090
#define DDFSTRT 0x092
#define DDFSTOP 0x094
#define DMACON 0x096
#define CLXCON 0x098
#define INTENA 0x09A
#define BPL1PTH 0x0E0
#define SPR0PTH 0x120
#define SPR0POS 0x140
#define BPLCON0 0x100
#define BPLCON1 0x102
#define BPLCON2 0x104
#define BPLCON3 0x106
#define BPL1MOD 0x108
#define BPL2MOD 0x10A
#define BPLCON4 0x10C
#define COLOR00 0x180
#define FMODE 0x1FC
#define DMAF_SETCLR 0x8000
#define DMAF_MASTER 0x0200
#define DMAF_RASTER 0x0100
#define DMAF_COPPER 0x0080
#define DMAF_SPRITE 0x0020

static const struct escena {
    UWORD bplcon0, bplcon2, bplcon2_abajo, ddfstrt, ddfstop, clxcon;
    WORD dibujo, grupo, segundos;
} escenas[] = {
#define ESCENA(c0, c2, c2b, ds, dp, clx, d, g, s) { c0, c2, c2b, ds, dp, clx, d, g, s },
#define SPRITE(g, c, x, y, a, d, at)
#include "sprites_escenas.h"
#undef ESCENA
#undef SPRITE
};
#define ESCENAS (sizeof(escenas) / sizeof(escenas[0]))

static const struct sprite {
    WORD grupo, canal, hstart, vstart, alto, dibujo, attach;
} sprites[] = {
#define ESCENA(c0, c2, c2b, ds, dp, clx, d, g, s)
#define SPRITE(g, c, x, y, a, d, at) { g, c, x, y, a, d, at },
#include "sprites_escenas.h"
#undef ESCENA
#undef SPRITE
};
#define NSPRITES (sizeof(sprites) / sizeof(sprites[0]))

/* El color n de la paleta (como denise.c). */
static UWORD paleta(int n) { return (UWORD)((n * 0x137 + 0x0A5) & 0xFFF); }

static int planos(const struct escena *e) { return (e->bplcon0 >> 12) & 7; }

/* Los bytes de una fila de cada plano: lo que lee el DMA en una línea. */
static int bytes_fila(const struct escena *e)
{
    int unidades = (e->ddfstop - e->ddfstrt) / 8 + 1;
    return unidades * ((e->bplcon0 & 0x8000) ? 4 : 2);
}

/* El valor de los planos del píxel (x, y) del mapa, bit n para el plano n + 1 (dibujos 0, 1, 3 y 4 como denise.c). */
static UBYTE indice(int dibujo, int planos, int x, int y)
{
    int mascara = (1 << planos) - 1;
    switch (dibujo) {
    case 1: return (UBYTE)(((x >> 2) + (y >> 3)) & mascara);
    case 3:
        if ((x & 15) == 0) return (UBYTE)((y >> 4) & 15);
        return (UBYTE)((1 + ((x >> 4) % 3)) << 4 | (x & 15));
    case 4: {
        int i1 = (x >> 4) & 7, i2 = (y >> 4) & 7;
        return (UBYTE)((i1 & 1) | (i2 & 1) << 1 | (i1 & 2) << 1 | (i2 & 2) << 2 | (i1 & 4) << 2 | (i2 & 4) << 3);
    }
    case 6: return (UBYTE)((x >> 5) & mascara);
    default: return (UBYTE)(((x >> 3) + (y >> 3)) & mascara);
    }
}

static void dibujar(const struct escena *e, UBYTE *mapa)
{
    int n = planos(e), bf = bytes_fila(e);
    LONG tam = (LONG)bf * 256;
    for (int y = 0; y < 256; y++)
        for (int b = 0; b < bf; b++) {
            UBYTE v[6] = { 0, 0, 0, 0, 0, 0 };
            for (int k = 0; k < 8; k++) {
                UBYTE i = indice(e->dibujo, n, b * 8 + k, y);
                for (int p = 0; p < n; p++)
                    if (i & (1 << p)) v[p] |= (UBYTE)(0x80 >> k);
            }
            for (int p = 0; p < n; p++) mapa[p * tam + (LONG)y * bf + b] = v[p];
        }
}

/* Las palabras DATA y DATB de la línea y de un sprite con el dibujo d. */
static void datos(int d, int y, UWORD *a, UWORD *b)
{
    switch (d) {
    case 0: *a = 0xFFFF; *b = 0x0000; break;
    case 1: *a = 0x0000; *b = 0xFFFF; break;
    case 2: *a = 0xFFFF; *b = 0xFFFF; break;
    case 3: {
        int r = y & 7;
        *a = (UWORD)((0xF0F0 << r | 0xF0F0 >> (16 - r)) & 0xFFFF);
        *b = 0xFF00;
        break;
    }
    case 4: *a = 0xAAAA; *b = 0xCCCC; break;
    default: *a = 0xF0F0; *b = 0xFF00; break;
    }
}

/* Las palabras de control de un sprite. */
static UWORD pos(const struct sprite *s) { return (UWORD)((s->vstart & 0xFF) << 8 | ((s->hstart >> 1) & 0xFF)); }
static UWORD ctl(const struct sprite *s)
{
    int vstop = s->vstart + s->alto;
    return (UWORD)((vstop & 0xFF) << 8 | (s->attach ? 0x80 : 0) | ((s->vstart >> 8) & 1) << 2 | ((vstop >> 8) & 1) << 1 |
                   (s->hstart & 1));
}

/* Las listas de los sprites del grupo, una cada TAM_CANAL bytes desde zona. */
#define TAM_CANAL 1024
static void listas(int grupo, UWORD *zona)
{
    for (int c = 0; c < 8; c++) {
        UWORD *w = zona + c * (TAM_CANAL / 2);
        for (unsigned i = 0; i < NSPRITES; i++) {
            const struct sprite *s = &sprites[i];
            if (s->grupo != grupo || s->canal != c) continue;
            *w++ = pos(s);
            *w++ = ctl(s);
            for (int y = 0; y < s->alto; y++) datos(s->dibujo, y, w, w + 1), w += 2;
        }
        *w++ = 0;
        *w++ = 0;
    }
}

/* La lista del Copper, palabra a palabra. */
static UWORD *pc;
static void mov(UWORD reg, UWORD v) { *pc++ = reg; *pc++ = v; }
static void wait(UWORD v, UWORD h) { *pc++ = (UWORD)((v & 0xFF) << 8 | (h & 0xFE) | 1); *pc++ = 0xFFFE; }

static void copper(const struct escena *e, UWORD *lista, ULONG mapa, ULONG zona)
{
    int n = planos(e);
    ULONG tam = (ULONG)bytes_fila(e) * 256;
    pc = lista;
    for (int c = 0; c < 8; c++) {
        ULONG d = zona + c * TAM_CANAL;
        mov((UWORD)(SPR0PTH + 4 * c), (UWORD)(d >> 16));
        mov((UWORD)(SPR0PTH + 4 * c + 2), (UWORD)d);
    }
    mov(FMODE, 0);
    mov(BPLCON3, 0x0C00);
    mov(BPLCON4, 0x0011);
    mov(CLXCON, e->clxcon);
    mov(DIWSTRT, 0x2C81);
    mov(DIWSTOP, 0x2CC1);
    mov(DDFSTRT, e->ddfstrt);
    mov(DDFSTOP, e->ddfstop);
    mov(BPL1MOD, 0);
    mov(BPL2MOD, 0);
    mov(BPLCON0, e->bplcon0);
    mov(BPLCON1, 0);
    mov(BPLCON2, e->bplcon2);
    for (int p = 0; p < n; p++) {
        ULONG d = mapa + p * tam;
        mov((UWORD)(BPL1PTH + 4 * p), (UWORD)(d >> 16));
        mov((UWORD)(BPL1PTH + 4 * p + 2), (UWORD)d);
    }
    for (int i = 0; i < 32; i++) mov((UWORD)(COLOR00 + 2 * i), paleta(i));
    if (e->grupo == 1) {
        for (int l = 0x80; l < 0x88; l++) {
            wait((UWORD)l, 0x20);
            mov(SPR0POS + 8 * 1, 0x7E50);
            wait((UWORD)l, 0x70);
            mov(SPR0POS + 8 * 1, 0x7E80);
        }
    }
    wait(0xAC, 0x07);
    mov(BPLCON2, e->bplcon2_abajo);
    if (e->grupo == 1) {
        wait(0xB0, 0x10);
        mov(SPR0POS + 8 * 5, 0x0088);
        mov(SPR0POS + 8 * 5 + 2, 0x0000);
        mov(SPR0POS + 8 * 5 + 6, 0x0FF0);
        mov(SPR0POS + 8 * 5 + 4, 0x3C3C);
        wait(0xC0, 0x10);
        mov(SPR0POS + 8 * 5 + 2, 0x0000);
    }
    *pc++ = 0xFFFF;
    *pc++ = 0xFFFE;
}

/* La línea del haz (9 bits), leída hasta que dos lecturas coinciden (ver chips.c). */
static UWORD linea(void)
{
    UWORD a = (UWORD)((VPOSR_VHPOSR >> 8) & 0x1FF), b;
    while ((b = (UWORD)((VPOSR_VHPOSR >> 8) & 0x1FF)) != a) a = b;
    return a;
}

/* Espera a que empiecen n cuadros (la línea vuelve atrás). */
static void espera_cuadros(int n)
{
    UWORD antes = linea(), l;
    while (n > 0) {
        l = linea();
        if (l < antes) n--;
        antes = l;
    }
}

#define TAM_MAPA 90000
#define TAM_LISTA 2048
#define TAM_ZONA (8 * TAM_CANAL)

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    if (!DOSBase) return 20;
    GfxBase = (struct GfxBase *)OpenLibrary((CONST_STRPTR)"graphics.library", 36);
    if (!GfxBase) return 20;
    UBYTE *mapa = AllocMem(TAM_MAPA, MEMF_CHIP | MEMF_CLEAR);
    UWORD *lista = AllocMem(2 * TAM_LISTA, MEMF_CHIP | MEMF_CLEAR);
    UWORD *zona = AllocMem(2 * TAM_ZONA, MEMF_CHIP | MEMF_CLEAR);
    if (!mapa || !lista || !zona) return 20;
    static UWORD clx[ESCENAS];

    OwnBlitter();
    WaitBlit();
    UWORD dmacon = CUSTOM16(DMACONR) & 0x07FF;
    UWORD intena = CUSTOM16(INTENAR) & 0x3FFF;
    Disable();
    CUSTOM16(INTENA) = 0x7FFF;

    for (unsigned i = 0; i < ESCENAS; i++) {
        /* La lista y los sprites de cada escena van en la otra mitad: los de la anterior siguen enteros hasta el
           cambio. */
        const struct escena *e = &escenas[i];
        UWORD *l = lista + (i & 1) * (TAM_LISTA / 2);
        UWORD *z = zona + (i & 1) * (TAM_ZONA / 2);
        listas(e->grupo, z);
        copper(e, l, (ULONG)mapa, (ULONG)z);
        espera_cuadros(1);
        CUSTOM16(DMACON) = DMAF_RASTER | DMAF_SPRITE;
        dibujar(e, mapa);
        espera_cuadros(1);
        CUSTOM32(COP1LC) = (ULONG)l;
        CUSTOM16(DMACON) = DMAF_SETCLR | DMAF_MASTER | DMAF_RASTER | DMAF_COPPER | DMAF_SPRITE;
        espera_cuadros(2);
        (void)CUSTOM16(CLXDAT);
        espera_cuadros(1);
        clx[i] = CUSTOM16(CLXDAT);
        espera_cuadros(e->segundos ? 50 * e->segundos : 4);
    }

    /* El sistema, otra vez. */
    CUSTOM16(DMACON) = DMAF_RASTER | DMAF_SPRITE;
    CUSTOM32(COP1LC) = (ULONG)GfxBase->copinit;
    if (GfxBase->LOFlist) {
        CUSTOM16(COP2LCH) = (UWORD)((ULONG)GfxBase->LOFlist >> 16);
        CUSTOM16(COP2LCL) = (UWORD)(ULONG)GfxBase->LOFlist;
    }
    CUSTOM16(DMACON) = (UWORD)(~dmacon & 0x07FF);
    CUSTOM16(DMACON) = (UWORD)(DMAF_SETCLR | dmacon);
    espera_cuadros(2);
    CUSTOM16(INTENA) = (UWORD)(0x8000 | intena);
    Enable();
    DisownBlitter();

    for (unsigned i = 0; i < ESCENAS; i++) {
        LONG a[2] = { (LONG)(i + 1), clx[i] };
        VPrintf((CONST_STRPTR)"escena %ld clxdat %04lx\n", a);
    }
    FreeMem(zona, 2 * TAM_ZONA);
    FreeMem(lista, 2 * TAM_LISTA);
    FreeMem(mapa, TAM_MAPA);
    CloseLibrary((struct Library *)GfxBase);
    CloseLibrary((struct Library *)DOSBase);
    return 0;
}
