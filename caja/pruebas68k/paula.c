/*
    B3 (docs/CHIPS_LOOP.md): el audio de Paula visto desde un programa 68k. Pone en marcha canales de audio con DMA
    (muestras en chip RAM, volumen 0) y mide con el haz cuántas líneas pasan entre las interrupciones AUDx de cada uno
    (en INTREQR, con el sistema parado), para varios AUDxLEN y AUDxPER; también cambiando AUDxLEN a mitad, con dos
    canales a la vez y en el modo manual (AUDxDAT escrito por la CPU, sin DMA).

    Es el oráculo de caja/chips: el mismo binario en FS-UAE con el Kickstart da las líneas que copia la prueba de Rust
    (caja/chips/tests/paula.rs), que hace lo mismo en su imitación. Cada caso dice "bien" o "MAL" según el manual: entre
    dos AUDx pasan AUDxLEN x 2 x AUDxPER ciclos de color (227 por línea), y uno más de margen por cada lado; los de
    periodo por debajo del mínimo del DMA (124) solo se miden. Escribe después, con el sistema en marcha.

    Toca DMACON (los bits de audio), ADKCON (los de modulación, a 0), INTREQ (los de audio) y los registros de los
    canales; al acabar repone DMACON y ADKCON como estaban y deja INTREQ sin bits de audio y los volúmenes a 0.
*/
#include <exec/types.h>
#include <exec/memory.h>
#include <exec/execbase.h>
#include <dos/dos.h>
#include <proto/exec.h>
#include <proto/dos.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;

#define R(d) (*(volatile UWORD *)(0xDFF000 + (d)))
#define RL(d) (*(volatile ULONG *)(0xDFF000 + (d)))
#define VPOSR_VHPOSR (*(volatile ULONG *)0xDFF004)
enum {
    DMACONR = 0x002, ADKCONR = 0x010, INTREQR = 0x01E, DMACON = 0x096, INTREQ = 0x09C, ADKCON = 0x09E,
    AUD0LC = 0x0A0, AUD0LEN = 0x0A4, AUD0PER = 0x0A6, AUD0VOL = 0x0A8, AUD0DAT = 0x0AA
};
#define AUD(x, r) ((r) + 0x10 * (x))
#define SETCLR 0x8000
#define DMAEN 0x0200
#define INTF_AUD0 0x0080
#define LINEAS_CUADRO 313
/* Lo más que se espera a una interrupción, en líneas. */
#define LIMITE 6000
#define N_IRQ 5     /* interrupciones por canal: 4 intervalos */
#define TAM 2048    /* bytes de muestras */

static void di(const char *f, LONG a, LONG b, LONG c, LONG d)
{
    LONG v[4] = { a, b, c, d };
    VPrintf((CONST_STRPTR)f, v);
}

/* La línea del haz, leída hasta que dos lecturas coinciden (ver chips.c). */
static UWORD linea(void)
{
    UWORD a = (UWORD)((VPOSR_VHPOSR >> 8) & 0x1FF), b;
    while ((b = (UWORD)((VPOSR_VHPOSR >> 8) & 0x1FF)) != a) a = b;
    return a;
}

/* Las líneas que van pasando: se lleva la cuenta mirando el haz. */
static UWORD ultima;
static ULONG lineas;
static void mira_haz(void)
{
    UWORD l = linea();
    if (l != ultima) {
        lineas += l > ultima ? (ULONG)(l - ultima) : (ULONG)(l + LINEAS_CUADRO - ultima);
        ultima = l;
    }
}

struct caso {
    UWORD canales;        /* bits 0-3 */
    UWORD len[4], per[4];
    UWORD len2;           /* si no es 0: el AUDxLEN que se escribe tras la segunda interrupción */
};

/* canal, LEN, PER (y un segundo canal con los suyos, o LEN 0). */
#define UNO(c, l, p, l2) { 1 << (c), { [c] = l }, { [c] = p }, l2 },
static const struct caso casos[] = {
    UNO(0, 64, 227, 0)
    UNO(0, 100, 300, 0)
    UNO(1, 32, 454, 0)
    UNO(2, 500, 124, 0)
    UNO(3, 10, 1000, 0)
    UNO(0, 1, 2000, 0)
    UNO(0, 50, 400, 20)
    UNO(0, 40, 100, 0)
    UNO(1, 40, 60, 0)
    { 0x6, { 0, 64, 20, 0 }, { 0, 227, 500, 0 }, 0 },
};
#define N (sizeof casos / sizeof casos[0])

/* Las líneas (de la cuenta) a cada interrupción de cada canal; la 0, al encender el DMA. */
static ULONG en[N][4][N_IRQ + 1];
static int agotado[N];

/* Modo manual: canal, AUDxPER y cuántas palabras. */
struct manual { UWORD canal, per, palabras; };
static const struct manual manuales[] = { { 0, 1000, 20 }, { 3, 300, 20 } };
#define NM (sizeof manuales / sizeof manuales[0])
static ULONG en_manual[NM][2];   /* las líneas a la primera interrupción y a la última */
static int manual_agotado[NM];

static LONG fallos;

static void mira(int bien, const char *que, LONG n)
{
    if (!bien) fallos++;
    di("paula: %s %ld: %s\n", (LONG)que, n, (LONG)(bien ? "bien" : "MAL"), 0);
}

/* Si `n` líneas caben en `cc` ciclos de color, con una de margen por cada lado. */
static int cabe(ULONG n, ULONG cc)
{
    ULONG lo = cc / 227, hi = (cc + 226) / 227;
    return n + 1 >= lo && n <= hi + 1;
}

/* Para los canales y espera a que se queden quietos. Sin DMA, un canal que sonaba acaba su palabra y, si su AUDx
   está a 0, suena otra (el modo manual): se pone AUDx a 1 para que se pare al acabarla, se esperan dos palabras de
   `per` y se borra. */
static void para_canales(UWORD mascara, UWORD per)
{
    R(DMACON) = mascara;
    R(INTREQ) = SETCLR | mascara << 7;
    ultima = linea();
    lineas = 0;
    ULONG hasta = 4 * (ULONG)per / 227 + 3;
    while (lineas < hasta) mira_haz();
    R(INTREQ) = mascara << 7;
    for (int x = 0; x < 4; x++)
        if (mascara & (1 << x)) R(AUD(x, AUD0VOL)) = 0;
}

static UWORD per_max(const struct caso *c)
{
    UWORD p = 0;
    for (int x = 0; x < 4; x++)
        if (c->per[x] > p) p = c->per[x];
    return p;
}

static void haz_caso(int i, UBYTE *muestras)
{
    const struct caso *c = &casos[i];
    UWORD m = c->canales;
    for (int x = 0; x < 4; x++) {
        if (!(m & (1 << x))) continue;
        RL(AUD(x, AUD0LC)) = (ULONG)muestras;
        R(AUD(x, AUD0LEN)) = c->len[x];
        R(AUD(x, AUD0PER)) = c->per[x];
        R(AUD(x, AUD0VOL)) = 0;
    }
    /* Espera al principio de una línea, para empezar siempre igual. */
    ultima = linea();
    while (linea() == ultima) {}
    ultima = linea();
    lineas = 0;
    R(DMACON) = SETCLR | DMAEN | m;
    int vistas[4] = { 0, 0, 0, 0 };
    for (int x = 0; x < 4; x++) en[i][x][0] = 0;
    for (;;) {
        int faltan = 0;
        for (int x = 0; x < 4; x++)
            if ((m & (1 << x)) && vistas[x] < N_IRQ) faltan = 1;
        if (!faltan) break;
        if (lineas > LIMITE) {
            agotado[i] = 1;
            break;
        }
        UWORD rq = R(INTREQR);
        mira_haz();
        for (int x = 0; x < 4; x++) {
            if (!(m & (1 << x)) || !(rq & (INTF_AUD0 << x)) || vistas[x] >= N_IRQ) continue;
            R(INTREQ) = INTF_AUD0 << x;
            en[i][x][++vistas[x]] = lineas;
            if (vistas[x] == 2 && c->len2) R(AUD(x, AUD0LEN)) = c->len2;
        }
    }
    para_canales(m, per_max(c));
}

static void haz_manual(int i)
{
    const struct manual *c = &manuales[i];
    UWORD x = c->canal, bit = INTF_AUD0 << x;
    R(AUD(x, AUD0PER)) = c->per;
    R(AUD(x, AUD0VOL)) = 0;
    R(INTREQ) = bit;
    ultima = linea();
    lineas = 0;
    R(AUD(x, AUD0DAT)) = 0x1234;
    int vistas = 0;
    while (vistas <= c->palabras) {
        if (lineas > LIMITE) {
            manual_agotado[i] = 1;
            break;
        }
        UWORD rq = R(INTREQR);
        mira_haz();
        if (!(rq & bit)) continue;
        if (vistas == 0) en_manual[i][0] = lineas;
        if (vistas == c->palabras) {
            en_manual[i][1] = lineas;
            vistas++;
            break;   /* AUDx se queda a 1: el canal se para al acabar la palabra */
        }
        R(INTREQ) = bit;
        R(AUD(x, AUD0DAT)) = 0x1234;
        vistas++;
    }
    /* Que acabe: dos palabras de tiempo y algo más. */
    ULONG hasta = lineas + 4 * (ULONG)c->per / 227 + 4;
    while (lineas < hasta) mira_haz();
    R(INTREQ) = bit;
}

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    if (!DOSBase) return 20;
    UBYTE *muestras = AllocMem(TAM, MEMF_CHIP | MEMF_CLEAR);
    if (!muestras) {
        di("paula: sin chip RAM\n", 0, 0, 0, 0);
        CloseLibrary((struct Library *)DOSBase);
        return 20;
    }
    for (int i = 0; i < TAM; i++) muestras[i] = (UBYTE)(i * 7);

    Disable();
    UWORD dmacon0 = R(DMACONR) & 0x000F;
    UWORD adkcon0 = R(ADKCONR) & 0x00FF;
    R(ADKCON) = 0x00FF;   /* sin modulación */
    para_canales(0x000F, 2000);   /* por si alguno sonaba */
    for (unsigned i = 0; i < N; i++) haz_caso(i, muestras);
    for (unsigned i = 0; i < NM; i++) haz_manual(i);
    para_canales(0x000F, 2000);
    R(ADKCON) = SETCLR | adkcon0;
    R(DMACON) = SETCLR | dmacon0;
    Enable();

    for (unsigned i = 0; i < N; i++) {
        const struct caso *c = &casos[i];
        for (int x = 0; x < 4; x++) {
            if (!(c->canales & (1 << x))) continue;
            ULONG *e = en[i][x];
            ULONG len = c->len[x], per = c->per[x], len2 = c->len2 ? c->len2 : len;
            di("paula: caso %ld, canal %ld, LEN %ld, PER %ld", (LONG)i + 1, x, (LONG)len, (LONG)per);
            if (c->len2) di(", LEN %ld tras la segunda", (LONG)c->len2, 0, 0, 0);
            di(": lineas %ld %ld %ld", (LONG)e[1], (LONG)(e[2] - e[1]), (LONG)(e[3] - e[2]), 0);
            di(" %ld %ld\n", (LONG)(e[4] - e[3]), (LONG)(e[5] - e[4]), 0, 0);
            if (agotado[i]) {
                mira(0, "sin todas sus interrupciones: caso", (LONG)i + 1);
                continue;
            }
            if (per < 124) continue;   /* bajo el mínimo del DMA: solo se mide */
            /* La primera, en la línea siguiente o la otra; la segunda, una línea más LEN - 1 palabras después (con
               LEN 1, una palabra: la que recarga es la segunda); luego, LEN palabras (con el LEN nuevo desde la
               cuarta). */
            ULONG primeras = len > 1 ? len - 1 : 1;
            int bien = e[1] <= 2 && cabe(e[2] - e[1], 227 + primeras * 2 * per) && cabe(e[3] - e[2], len * 2 * per)
                       && cabe(e[4] - e[3], len2 * 2 * per) && cabe(e[5] - e[4], len2 * 2 * per);
            mira(bien, "AUDx a su hora: caso", (LONG)i + 1);
        }
    }
    for (unsigned i = 0; i < NM; i++) {
        const struct manual *c = &manuales[i];
        ULONG n = en_manual[i][1] - en_manual[i][0];
        di("paula: manual %ld, canal %ld, PER %ld: lineas %ld", (LONG)i + 1, c->canal, c->per, (LONG)en_manual[i][0]);
        di(" y %ld en %ld palabras\n", (LONG)n, c->palabras, 0, 0);
        mira(!manual_agotado[i] && en_manual[i][0] <= 1 && cabe(n, (ULONG)c->palabras * 2 * c->per),
             "modo manual: AUDx a cada palabra: caso", (LONG)i + 1);
    }
    di("paula: %ld fallos\n", fallos, 0, 0, 0);

    FreeMem(muestras, TAM);
    CloseLibrary((struct Library *)DOSBase);
    return fallos;
}
