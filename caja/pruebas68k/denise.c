/*
    B4 (docs/CHIPS_LOOP.md, fase 2 adelantada): la imagen de los planos y de Denise, para compararla con una captura
    de FS-UAE. Con el sistema parado (Disable), pone en chip RAM unos planos con un dibujo conocido y una lista de
    Copper propia que los enseña, con barras de Copper, y deja cada modo quieto unos segundos (SEGUNDOS). Los modos
    están en denise_modos.h; la prueba de Rust (caja/chips/tests/denise.rs) pinta lo mismo con los Chips y lo compara.

    La lista de cada modo (copper(), la misma en la prueba de Rust):
    - FMODE a 0, BPLCON3 a $0C00 y BPLCON4 a $0011 (lo de serie de AGA: en un A1200 el sistema puede dejar otra cosa;
      en OCS no existen), SPRxCTL a 0 (sin sprites), la ventana, DDF, módulos, BPLCON0-2, los punteros y la paleta.
    - Barras: COLOR00 cambia en las líneas $20, $22... $2A (seis colores); en la $2B, 16 MOVE seguidos a COLOR00 desde
      el ciclo $40 (bandas de 8 píxeles de baja); en la $2C vuelve al de la paleta.
    - A mitad de línea, dentro de la imagen: en la línea $90, desde el ciclo $70, COLOR01 a blanco; en la $91 vuelve.
      En alta con 4 planos el Copper no tiene ciclos mientras se leen los planos: el cambio cae al final de la lectura.
    - En la línea $AC, BPLCON2 pasa a bplcon2_abajo (en el modo de campos dobles, el campo 2 pasa delante).

    Al acabar pone como estaban DMACON, INTENA y las listas del sistema (COP1LC a copinit, COP2LC a LOFlist, como
    copper.c), espera dos cuadros y Enable. Escribe "denise: N modos".
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
#define INTENAR 0x01C
#define COP1LC 0x080
#define COP2LCH 0x084
#define COP2LCL 0x086
#define DIWSTRT 0x08E
#define DIWSTOP 0x090
#define DDFSTRT 0x092
#define DDFSTOP 0x094
#define DMACON 0x096
#define INTENA 0x09A
#define BPL1PTH 0x0E0
#define BPLCON0 0x100
#define BPLCON1 0x102
#define BPLCON2 0x104
#define BPLCON3 0x106
#define BPL1MOD 0x108
#define BPL2MOD 0x10A
#define BPLCON4 0x10C
#define SPR0CTL 0x142
#define COLOR00 0x180
#define COLOR01 0x182
#define FMODE 0x1FC
#define DMAF_SETCLR 0x8000
#define DMAF_MASTER 0x0200
#define DMAF_RASTER 0x0100
#define DMAF_COPPER 0x0080
#define DMAF_SPRITE 0x0020

/* Cuántos cuadros se queda quieto cada modo (50 por segundo). */
#define CUADROS 200

static const struct modo {
    UWORD bplcon0, bplcon1, bplcon2, bplcon2_abajo, ddfstrt, ddfstop, diwstrt, diwstop;
    WORD bytes_fila, filas, bpl1mod, bpl2mod, dibujo;
} modos[] = {
#define MODO(c0, c1, c2, c2b, ds, dp, ws, wp, bf, f, m1, m2, d) { c0, c1, c2, c2b, ds, dp, ws, wp, bf, f, m1, m2, d },
#include "denise_modos.h"
#undef MODO
};
#define MODOS (sizeof(modos) / sizeof(modos[0]))

/* El color n de la paleta (n < 32), de las barras (n < 6) y de las bandas (n < 16). */
static UWORD paleta(int n) { return (UWORD)((n * 0x137 + 0x0A5) & 0xFFF); }
static const UWORD barras[6] = { 0xF00, 0xF80, 0xFF0, 0x0F0, 0x00F, 0x80F };
static UWORD banda(int n) { return (UWORD)(n * 0x111); }

/* El valor de los planos del píxel (x, y) del mapa, bit n para el plano n + 1. */
static UBYTE indice(int dibujo, int planos, int x, int y)
{
    int mascara = (1 << planos) - 1;
    switch (dibujo) {
    case 0: return (UBYTE)(((x >> 3) + (y >> 3)) & mascara);
    case 1: return (UBYTE)(((x >> 2) + (y >> 3)) & mascara);
    case 2: return (UBYTE)(((x >> 2) + y) & 63);
    case 3:
        /* HAM: cada 16 píxeles, uno de la paleta (0-15) y 15 cambios de azul, rojo o verde (1 a 15). */
        if ((x & 15) == 0) return (UBYTE)((y >> 4) & 15);
        return (UBYTE)((1 + ((x >> 4) % 3)) << 4 | (x & 15));
    case 4: {
        int i1 = (x >> 4) & 7, i2 = (y >> 4) & 7;
        return (UBYTE)((i1 & 1) | (i2 & 1) << 1 | (i1 & 2) << 1 | (i2 & 2) << 2 | (i1 & 4) << 2 | (i2 & 4) << 3);
    }
    default: return (UBYTE)(((x >> 3) + (y >> 2)) & mascara);
    }
}

static void dibujar(const struct modo *m, UBYTE *mapa)
{
    int planos = (m->bplcon0 >> 12) & 7;
    LONG tam = (LONG)m->bytes_fila * m->filas;
    for (int y = 0; y < m->filas; y++)
        for (int b = 0; b < m->bytes_fila; b++) {
            UBYTE v[6] = { 0, 0, 0, 0, 0, 0 };
            for (int k = 0; k < 8; k++) {
                UBYTE i = indice(m->dibujo, planos, b * 8 + k, y);
                for (int p = 0; p < planos; p++)
                    if (i & (1 << p)) v[p] |= (UBYTE)(0x80 >> k);
            }
            for (int p = 0; p < planos; p++) mapa[p * tam + (LONG)y * m->bytes_fila + b] = v[p];
        }
}

/* La lista, palabra a palabra. */
static UWORD *pc;
static void mov(UWORD reg, UWORD v) { *pc++ = reg; *pc++ = v; }
static void wait(UWORD v, UWORD h) { *pc++ = (UWORD)((v & 0xFF) << 8 | (h & 0xFE) | 1); *pc++ = 0xFFFE; }

static void copper(const struct modo *m, UWORD *lista, ULONG mapa)
{
    int planos = (m->bplcon0 >> 12) & 7;
    ULONG tam = (ULONG)m->bytes_fila * m->filas;
    pc = lista;
    mov(FMODE, 0);
    mov(BPLCON3, 0x0C00);
    mov(BPLCON4, 0x0011);
    for (int i = 0; i < 8; i++) mov((UWORD)(SPR0CTL + 8 * i), 0);
    mov(DIWSTRT, m->diwstrt);
    mov(DIWSTOP, m->diwstop);
    mov(DDFSTRT, m->ddfstrt);
    mov(DDFSTOP, m->ddfstop);
    mov(BPL1MOD, (UWORD)m->bpl1mod);
    mov(BPL2MOD, (UWORD)m->bpl2mod);
    mov(BPLCON0, m->bplcon0);
    mov(BPLCON1, m->bplcon1);
    mov(BPLCON2, m->bplcon2);
    for (int p = 0; p < planos; p++) {
        ULONG d = mapa + p * tam;
        mov((UWORD)(BPL1PTH + 4 * p), (UWORD)(d >> 16));
        mov((UWORD)(BPL1PTH + 4 * p + 2), (UWORD)d);
    }
    for (int i = 0; i < 32; i++) mov((UWORD)(COLOR00 + 2 * i), paleta(i));
    for (int i = 0; i < 6; i++) {
        wait((UWORD)(0x20 + 2 * i), 0x07);
        mov(COLOR00, barras[i]);
    }
    wait(0x2B, 0x40);
    for (int i = 0; i < 16; i++) mov(COLOR00, banda(i));
    wait(0x2C, 0x07);
    mov(COLOR00, paleta(0));
    wait(0x90, 0x70);
    mov(COLOR01, 0xFFF);
    wait(0x91, 0x07);
    mov(COLOR01, paleta(1));
    wait(0xAC, 0x07);
    mov(BPLCON2, m->bplcon2_abajo);
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

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    if (!DOSBase) return 20;
    GfxBase = (struct GfxBase *)OpenLibrary((CONST_STRPTR)"graphics.library", 36);
    if (!GfxBase) return 20;
    UBYTE *mapa = AllocMem(TAM_MAPA, MEMF_CHIP | MEMF_CLEAR);
    UWORD *lista = AllocMem(TAM_LISTA, MEMF_CHIP | MEMF_CLEAR);
    if (!mapa || !lista) return 20;

    OwnBlitter();
    WaitBlit();
    UWORD dmacon = CUSTOM16(DMACONR) & 0x07FF;
    UWORD intena = CUSTOM16(INTENAR) & 0x3FFF;
    Disable();
    CUSTOM16(INTENA) = 0x7FFF;

    for (unsigned i = 0; i < MODOS; i++) {
        /* Se dibuja con la imagen apagada (en el borrado vertical) y la lista nueva vale desde el cuadro siguiente. */
        dibujar(&modos[i], mapa);
        copper(&modos[i], lista, (ULONG)mapa);
        espera_cuadros(1);
        CUSTOM16(DMACON) = DMAF_RASTER | DMAF_SPRITE;
        CUSTOM32(COP1LC) = (ULONG)lista;
        CUSTOM16(DMACON) = DMAF_SETCLR | DMAF_MASTER | DMAF_RASTER | DMAF_COPPER;
        espera_cuadros(CUADROS);
    }

    /* El sistema, otra vez. */
    CUSTOM16(DMACON) = DMAF_RASTER;
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

    LONG n = MODOS;
    VPrintf((CONST_STRPTR)"denise: %ld modos\n", &n);
    FreeMem(lista, TAM_LISTA);
    FreeMem(mapa, TAM_MAPA);
    CloseLibrary((struct Library *)GfxBase);
    CloseLibrary((struct Library *)DOSBase);
    return 0;
}
