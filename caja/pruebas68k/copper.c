/*
    B2 (docs/CHIPS_LOOP.md, fase 2 adelantada): el Copper visto desde un programa 68k. El 68k no puede leer el Copper,
    pero sí lo que hace: una lista propia en chip RAM que, con WAIT y MOVE a INTREQ (SET de SOFT o COPER), avisa en
    varias líneas. El 68k, con el sistema parado, espera cada bit, apunta en qué línea lo vio y lo borra. Cada paso
    dice "copper: <qué>: bien" o "MAL"; al final, las líneas medidas y los fallos, que son también lo que devuelve.

    Los cuadros, en orden (la lista se pone en COP1LC sin COPJMP1: el Copper la empieza al empezar el cuadro):
    - A, lista 1: SOFT en la línea 40; en la 80, un SKIP que se cumple (salta un MOVE de COPER) y otro que no (SOFT);
      COPER en la 120; WAIT $FFDF y después WAIT a la línea 30, que tras la 255 es la 286: SOFT; COP2LC a la lista 2 y
      COPJMP2 (el MOVE de COPER que sigue no se hace); en la lista 2, COPER en la línea 300 (44 en 8 bits).
    - B: SOFT otra vez en la línea 40 (el Copper volvió a COP1LC). El 68k pone en COP1LC la lista 3.
    - C, lista 3, CDANG a 0: en la línea 40, un MOVE a BLTAFWM ($044) y después SOFT. $044 está prohibido sin CDANG:
      el Copper se para y SOFT no llega en todo el cuadro. El 68k pone CDANG y en COP1LC la lista 4.
    - D, lista 4, CDANG a 1: lo mismo, y ahora SOFT llega en la línea 40.

    Lo que depende del tiempo va con margen: el bit se ve en su línea o en la siguiente. Como chips.c, el mismo binario
    vale para el oráculo (FS-UAE con el Kickstart) y para la caja, aunque la caja aún no da chip RAM a los chips: la
    misma lista se prueba con los Chips en caja/chips/tests/copper.rs.

    Con el sistema parado (Disable) y el Blitter pedido (OwnBlitter: la lista escribe en BLTAFWM). Al acabar, pone
    COP1LC y COP2LC como los tiene graphics.library (copinit y LOFlist), CDANG a 0, espera dos cuadros para que el
    Copper vuelva a la lista del sistema y borra SOFT y COPER antes de Enable.
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

/* Los registros (Hardware Reference Manual). */
#define CUSTOM16(d) (*(volatile UWORD *)(0xDFF000 + (d)))
#define CUSTOM32(d) (*(volatile ULONG *)(0xDFF000 + (d)))
#define VPOSR_VHPOSR (*(volatile ULONG *)0xDFF004)
#define INTREQR 0x01E
#define COPCON 0x02E
#define BLTAFWM 0x044
#define COP1LC 0x080
#define COP2LCH 0x084
#define COP2LCL 0x086
#define COPJMP2 0x08A
#define INTREQ 0x09C
#define INTF_SETCLR 0x8000
#define INTF_COPER 0x0010
#define INTF_SOFT 0x0004
#define CDANG 0x0002

/* Lo más que se espera a algo (en lecturas): bastante más que un cuadro en cualquier motor. */
#define LIMITE 2000000
/* La línea en que el 68k mira que SOFT no llegó en el cuadro C. */
#define LINEA_PARADO 200
#define NO_LLEGA 0xFFFF

static void di(const char *f, LONG a, LONG b, LONG c, LONG d)
{
    LONG v[4] = { a, b, c, d };
    VPrintf((CONST_STRPTR)f, v);
}

static LONG fallos;

static void mira(int bien, const char *que)
{
    if (!bien) fallos++;
    di("copper: %s: %s\n", (LONG)que, (LONG)(bien ? "bien" : "MAL"), 0, 0);
}

/* La línea del haz (9 bits), leída hasta que dos lecturas coinciden (ver chips.c: el acceso largo son dos). */
static UWORD linea(void)
{
    UWORD a = (UWORD)((VPOSR_VHPOSR >> 8) & 0x1FF), b;
    while ((b = (UWORD)((VPOSR_VHPOSR >> 8) & 0x1FF)) != a) a = b;
    return a;
}

/* Espera al principio de un cuadro (la línea vuelve atrás). 0 si no llega. */
static int espera_cuadro(void)
{
    ULONG n = 0;
    UWORD antes = linea(), l;
    while ((l = linea()) >= antes && ++n < LIMITE) antes = l;
    return n < LIMITE;
}

/* Espera a que suba `bit` en INTREQR, lo borra y dice en qué línea lo vio (NO_LLEGA si no llega). En `otro`, si
   estaba también el otro bit de los dos (SOFT o COPER): un MOVE que no tocaba. */
static UWORD espera(UWORD bit, int *otro)
{
    ULONG n = 0;
    UWORD r;
    while (!((r = CUSTOM16(INTREQR)) & bit) && ++n < LIMITE) {}
    if (!(r & bit)) return NO_LLEGA;
    UWORD l = linea();
    *otro = (r & (INTF_SOFT | INTF_COPER) & ~bit) != 0;
    CUSTOM16(INTREQ) = bit;
    return l;
}

/* La lista, palabra a palabra. */
static UWORD *p;
static void mov(UWORD reg, UWORD v) { *p++ = reg; *p++ = v; }
static void wait(UWORD v, UWORD h) { *p++ = (UWORD)((v & 0xFF) << 8 | (h & 0xFE) | 1); *p++ = 0xFFFE; }
static void skip(UWORD v, UWORD h) { *p++ = (UWORD)((v & 0xFF) << 8 | (h & 0xFE) | 1); *p++ = 0xFFFF; }
static void fin(void) { *p++ = 0xFFFF; *p++ = 0xFFFE; }

#define PALABRAS 128

/* Los avisos de los cuadros A y B: qué bit y en qué línea. */
static const struct { UWORD bit; UWORD linea; const char *que; } avisos[] = {
    { INTF_SOFT, 40, "WAIT a la linea 40 y MOVE a INTREQ" },
    { INTF_SOFT, 80, "SKIP: el que se cumple salta el MOVE y el que no, no" },
    { INTF_COPER, 120, "WAIT a la linea 120 (COPER)" },
    { INTF_SOFT, 286, "WAIT $FFDF y despues a la linea 30: la 286" },
    { INTF_COPER, 300, "COPJMP2 salta a COP2LC (linea 300, 44 en 8 bits)" },
    { INTF_SOFT, 40, "al empezar el cuadro, el Copper vuelve a COP1LC" },
};
#define AVISOS (sizeof(avisos) / sizeof(avisos[0]))

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    if (!DOSBase) return 20;
    GfxBase = (struct GfxBase *)OpenLibrary((CONST_STRPTR)"graphics.library", 36);
    if (!GfxBase) return 20;
    UWORD *mem = AllocMem(PALABRAS * 2, MEMF_CHIP | MEMF_CLEAR);
    if (!mem) return 20;

    /* Las cuatro listas. */
    UWORD *l1 = mem, *l2 = mem + 48, *l3 = mem + 64, *l4 = mem + 80;
    p = l1;
    wait(40, 0x07);
    mov(INTREQ, INTF_SETCLR | INTF_SOFT);
    wait(80, 0x07);
    skip(70, 0x07);
    mov(INTREQ, INTF_SETCLR | INTF_COPER);
    skip(90, 0x07);
    mov(INTREQ, INTF_SETCLR | INTF_SOFT);
    wait(120, 0x07);
    mov(INTREQ, INTF_SETCLR | INTF_COPER);
    wait(255, 0xDE);
    wait(30, 0x07);
    mov(INTREQ, INTF_SETCLR | INTF_SOFT);
    mov(COP2LCH, (UWORD)((ULONG)l2 >> 16));
    mov(COP2LCL, (UWORD)(ULONG)l2);
    mov(COPJMP2, 0);
    mov(INTREQ, INTF_SETCLR | INTF_COPER);
    fin();
    p = l2;
    wait(300, 0x07);
    mov(INTREQ, INTF_SETCLR | INTF_COPER);
    fin();
    p = l3;
    wait(40, 0x07);
    mov(BLTAFWM, 0xFFFF);
    mov(INTREQ, INTF_SETCLR | INTF_SOFT);
    fin();
    p = l4;
    wait(40, 0x07);
    mov(BLTAFWM, 0xFFFF);
    mov(INTREQ, INTF_SETCLR | INTF_SOFT);
    fin();

    OwnBlitter();
    WaitBlit();
    Disable();

    /* Cuadros A y B. */
    CUSTOM16(INTREQ) = INTF_SOFT | INTF_COPER;
    CUSTOM32(COP1LC) = (ULONG)l1;
    UWORD vistos[AVISOS];
    int otros[AVISOS];
    for (unsigned i = 0; i < AVISOS; i++) {
        otros[i] = 0;
        vistos[i] = espera(avisos[i].bit, &otros[i]);
    }

    /* Cuadro C: lista 3, sin CDANG. */
    CUSTOM32(COP1LC) = (ULONG)l3;
    int c_llega = espera_cuadro();
    CUSTOM16(INTREQ) = INTF_SOFT | INTF_COPER;
    ULONG n = 0;
    while (linea() < LINEA_PARADO && ++n < LIMITE) {}
    c_llega = c_llega && n < LIMITE;
    UWORD sin_cdang = CUSTOM16(INTREQR) & INTF_SOFT;

    /* Cuadro D: lista 4, con CDANG. */
    CUSTOM16(COPCON) = CDANG;
    CUSTOM32(COP1LC) = (ULONG)l4;
    int otro_d = 0;
    UWORD con_cdang = espera(INTF_SOFT, &otro_d);

    /* La lista del sistema, otra vez. */
    CUSTOM16(COPCON) = 0;
    CUSTOM32(COP1LC) = (ULONG)GfxBase->copinit;
    if (GfxBase->LOFlist) {
        CUSTOM16(COP2LCH) = (UWORD)((ULONG)GfxBase->LOFlist >> 16);
        CUSTOM16(COP2LCL) = (UWORD)(ULONG)GfxBase->LOFlist;
    }
    espera_cuadro();
    espera_cuadro();
    CUSTOM16(INTREQ) = INTF_SOFT | INTF_COPER;

    Enable();
    DisownBlitter();

    for (unsigned i = 0; i < AVISOS; i++) {
        int bien = vistos[i] != NO_LLEGA && vistos[i] >= avisos[i].linea && vistos[i] <= avisos[i].linea + 1 &&
                   !otros[i];
        mira(bien, avisos[i].que);
        if (!bien)
            di("copper: %s en la linea %ld (se esperaba %ld)%s\n",
               (LONG)(avisos[i].bit == INTF_SOFT ? "SOFT" : "COPER"), vistos[i] == NO_LLEGA ? -1 : (LONG)vistos[i],
               avisos[i].linea, (LONG)(otros[i] ? ", con el otro bit" : ""));
    }
    mira(c_llega && !sin_cdang, "MOVE a $044 sin CDANG: el Copper se para hasta el cuadro siguiente");
    int d_bien = con_cdang != NO_LLEGA && con_cdang >= 40 && con_cdang <= 41 && !otro_d;
    mira(d_bien, "MOVE a $044 con CDANG: se hace");
    if (!d_bien) di("copper: con CDANG, SOFT en la linea %ld\n", con_cdang == NO_LLEGA ? -1 : (LONG)con_cdang, 0, 0, 0);
    di("copper: medida: lineas %ld %ld %ld %ld", vistos[0], vistos[1], vistos[2], vistos[3]);
    di(" %ld %ld; con CDANG %ld\n", vistos[4], vistos[5], con_cdang, 0);
    di("copper: %ld fallos\n", fallos, 0, 0, 0);

    FreeMem(mem, PALABRAS * 2);
    CloseLibrary((struct Library *)GfxBase);
    CloseLibrary((struct Library *)DOSBase);
    return fallos;
}
