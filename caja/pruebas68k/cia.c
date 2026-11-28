/*
    B5 (docs/CHIPS_LOOP.md): las dudas de los CIA, medidas desde un programa 68k. No dice "bien" ni "MAL": escribe lo
    que ve, una línea por medida ("cia: <qué>: <valores>"), y la prueba de Rust (caja/chips/tests/cia.rs) repite lo
    mismo en su imitación y compara con lo que dio FS-UAE con el Kickstart 3.1.

    Lo que mide, con el sistema parado (Disable) y escribiendo después:
    1. Los registros como los deja el Kickstart (puertos, direcciones, control) y DMACONR.
    2. Lo que se lee en el registro $B.
    3. Los espejos en $BFxxxx: escribe y lee SDR (el registro serie, que sin SPMODE solo se guarda) por direcciones
       con A12 y A13 a 0 o a 1, pares e impares, con bits sueltos en A1-A7, y con accesos de 2 bytes.
    4. CRA bit 7 y CRB bit 7 tras escribirlos a 1; el bit 4 (LOAD) leído.
    5. El bit 14 de INTREQ.
    6. INTREQ por nivel o por flanco: con la línea /IRQ del CIA-B abajo (el temporizador A con su máscara), borra
       EXTER en INTREQ sin leer el ICR y mira si vuelve a subir.
    7. Dónde cae el pulso del TOD del CIA-A (VSYNC) respecto a VERTB, y el del CIA-B (HSYNC) dentro de la línea.
    8. La alarma del TOD: si salta al escribir la alarma igual al TOD, al escribir el TOD igual a la alarma, al
       volver a escribir la misma, y al contar; y que escribir el byte alto lo para hasta el bajo (el del medio no).
    9. El retraso del temporizador A del CIA-B al arrancar (START; LOAD y START; el byte alto en modo una vez), en
       marcha, al pararlo y con LOAD en marcha: lee el contador seis veces seguidas (en ensamblador, para que cada
       lectura tarde lo mismo), ocho veces cada caso, en fases distintas del reloj E.
    10. El registro serie del CIA-B en salida (SPMODE, reloj del temporizador A): cuánto tarda el bit SP del ICR.
        Y en entrada: en el Amiga, PA0 y PA1 del CIA-B van a SP y CNT; con ellos en salida se meten 8 bits a mano.

    Los temporizadores son del CIA-B, y los pide antes a ciab.resource (AddICRVector, con una interrupción que no hace
    nada): si el sistema ya los tiene, esa medida no se hace y lo dice. La alarma, la de un CIA que la tenga libre; con
    el Kickstart 3.1 las dos están ocupadas, y entonces usa la del CIA-B sin pedirla y al acabar la pone poco después
    del TOD, para que su dueño reciba una alarma (antes de tiempo) en vez de ninguna. Leer el ICR borra también los
    bits que el sistema tuviera pendientes en ese CIA. Al acabar repone CRA, CRB, DDRA, PRA y SDR como estaban, y el
    TOD del CIA-B con lo que ha pasado (las líneas, aproximadas).

    Con el A1200 va tal cual (68020); para el A500 hay que compilarlo con -m68000 (el Makefile no lo hace).
*/
#include <exec/types.h>
#include <exec/execbase.h>
#include <exec/interrupts.h>
#include <hardware/cia.h>
#include <resources/cia.h>
#include <dos/dos.h>
#include <proto/exec.h>
#include <proto/dos.h>
#include <proto/cia.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;

#define B8(d) (*(volatile UBYTE *)(d))
#define W16(d) (*(volatile UWORD *)(d))
#define CIAA(r) B8(0xBFE001 + 0x100 * (r))
#define CIAB(r) B8(0xBFD000 + 0x100 * (r))
enum { PRA = 0, PRB, DDRA, DDRB, TALO, TAHI, TBLO, TBHI, TODLO, TODMID, TODHI, REGB, SDR, ICR, CRA, CRB };
#define DMACONR 0xDFF002
#define VHPOSR 0xDFF006
#define INTENAR 0xDFF01C
#define INTREQR 0xDFF01E
#define INTREQ 0xDFF09C
#define SETCLR 0x8000
#define F_EXTER 0x2000
#define F_VERTB 0x0020
#define I_TA 0x01
#define I_TB 0x02
#define I_ALRM 0x04
#define I_SP 0x08
#define C_START 0x01
#define C_RUNMODE 0x08
#define C_LOAD 0x10
#define C_SPMODE 0x40
#define C_ALARM 0x80
#define LIMITE 400000

static void di(const char *f, LONG a, LONG b, LONG c, LONG d)
{
    LONG v[4] = { a, b, c, d };
    VPrintf((CONST_STRPTR)f, v);
}

/* La interrupción que se da a ciab.resource para quedarse con un bit: no hace nada. */
static void nada(void) {}
static struct Interrupt irq_ta, irq_tb, irq_alrm, irq_alrm_a;

static int pedir(struct Library *res, WORD bit, struct Interrupt *i)
{
    i->is_Node.ln_Type = NT_INTERRUPT;
    i->is_Node.ln_Name = (char *)"cia.exe";
    i->is_Code = (VOID (*)())nada;
    return AddICRVector(res, bit, i) == NULL;
}

static UWORD vh(void) { return W16(VHPOSR); }

/* Espera a que el haz pase por la línea `l` (0-255, sin V8). */
static void espera_linea(UBYTE l)
{
    ULONG n = 0;
    while ((vh() >> 8) == l && ++n < LIMITE) {}
    while ((vh() >> 8) != l && ++n < LIMITE) {}
}

/* Un registro del CIA que empieza en `c` (el registro 0). */
#define REG(c, r) ((c)[0x100 * (r)])

static ULONG tod(volatile UBYTE *c)
{
    ULONG h = REG(c, TODHI), m = REG(c, TODMID), l = REG(c, TODLO);
    return h << 16 | m << 8 | l;
}

static void tod_poner(volatile UBYTE *c, ULONG v)
{
    REG(c, TODHI) = (UBYTE)(v >> 16);
    REG(c, TODMID) = (UBYTE)(v >> 8);
    REG(c, TODLO) = (UBYTE)v;
}

static void alarma(volatile UBYTE *c, ULONG v, UBYTE crb)
{
    REG(c, CRB) = crb | C_ALARM;
    tod_poner(c, v);
    REG(c, CRB) = crb;
}

/* Espera `n` pulsos del TOD de `c`: líneas en el CIA-B, cuadros en el CIA-A (por VERTB). */
static void espera_tod(volatile UBYTE *c, int n)
{
    for (int i = 0; i < n; i++) {
        ULONG k = 0;
        if (c == &CIAB(0)) {
            UBYTE l = (UBYTE)(W16(VHPOSR) >> 8);
            while ((UBYTE)(W16(VHPOSR) >> 8) == l && ++k < LIMITE) {}
        } else {
            W16(INTREQ) = F_VERTB;
            while (!(W16(INTREQR) & F_VERTB) && ++k < LIMITE) {}
        }
    }
}

/* Escribe `v` en `cr` y lee seis veces seguidas `lo` en `o`, cada lectura con la misma instrucción. */
static void seis_lecturas(volatile UBYTE *cr, UBYTE v, volatile UBYTE *lo, UBYTE *o)
{
    __asm__ volatile(
        "move.b %1,(%2)\n\t"
        "move.b (%3),(%0)+\n\t"
        "move.b (%3),(%0)+\n\t"
        "move.b (%3),(%0)+\n\t"
        "move.b (%3),(%0)+\n\t"
        "move.b (%3),(%0)+\n\t"
        "move.b (%3),(%0)+\n\t"
        : "+a"(o)
        : "d"(v), "a"(cr), "a"(lo)
        : "memory");
}

/* Lo medido, para escribirlo con el sistema en marcha. */
static UBYTE arranque_a[6], arranque_b[6], arranque_b_sdr;
static UWORD dmaconr0, intenar0;
static UBYTE reg_b[4];
static UBYTE esp[24];
static UWORD esp16[4];
static UBYTE cr_bits[6];
static UWORD intreq14[2];
static int nivel_hecho;
static UWORD nivel[3];
static UBYTE nivel_icr;
static UWORD vsync[3][4];
static UWORD hsync[8][2];
static int alarma_hecha;
static char alarma_cia;
static UBYTE alrm[6];
static ULONG tod_paro[4];
static int retraso_hecho;
#define N_RETRASO 6
static UBYTE retraso[N_RETRASO][8][6];
static int serie_hecha;
static UWORD serie_e[2];
static UBYTE serie_icr, serie_sdr, entrada_icr, entrada_sdr;

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    if (!DOSBase) return 20;
    struct Library *ciab = (struct Library *)OpenResource((CONST_STRPTR)CIABNAME);
    int ta = ciab && pedir(ciab, CIAICRB_TA, &irq_ta);
    int tb = ciab && pedir(ciab, CIAICRB_TB, &irq_tb);
    int al = ciab && pedir(ciab, CIAICRB_ALRM, &irq_alrm);
    struct Library *ciaa = (struct Library *)OpenResource((CONST_STRPTR)CIAANAME);
    int al_a = !al && ciaa && pedir(ciaa, CIAICRB_ALRM, &irq_alrm_a);

    Disable();

    /* 1. Como los deja el Kickstart. */
    for (int r = 0; r < 4; r++) {
        arranque_a[r] = CIAA(r);
        arranque_b[r] = CIAB(r);
    }
    arranque_a[4] = CIAA(CRA);
    arranque_a[5] = CIAA(CRB);
    arranque_b[4] = CIAB(CRA);
    arranque_b[5] = CIAB(CRB);
    arranque_b_sdr = CIAB(SDR);
    dmaconr0 = W16(DMACONR);
    intenar0 = W16(INTENAR);
    UBYTE cra_b = arranque_b[4] & ~C_LOAD, crb_b = arranque_b[5] & ~C_LOAD;

    /* 2. El registro $B. */
    reg_b[0] = B8(0xBFEB01);
    reg_b[1] = B8(0xBFDB00);
    reg_b[2] = B8(0xBFEB01);
    reg_b[3] = B8(0xBFDB00);

    /* 3. Espejos, con SDR (registro $C). */
    UBYTE sdr_a = CIAA(SDR), sdr_b = CIAB(SDR);
    B8(0xBFCC01) = 0x5A;    /* A12 = A13 = 0: los dos */
    esp[0] = CIAA(SDR); esp[1] = CIAB(SDR);
    B8(0xBFEC00) = 0xA5;    /* par, solo el A */
    esp[2] = CIAA(SDR); esp[3] = CIAB(SDR);
    B8(0xBFDC01) = 0x3C;    /* impar, solo el B */
    esp[4] = CIAA(SDR); esp[5] = CIAB(SDR);
    B8(0xBFFC01) = 0xC3;    /* ninguno */
    esp[6] = CIAA(SDR); esp[7] = CIAB(SDR);
    B8(0xBFEC41) = 0x66;    /* A6 a 1 */
    esp[8] = CIAA(SDR); esp[9] = CIAB(SDR);
    W16(0xBFCC00) = 0x1234; /* 2 bytes a los dos */
    esp[10] = CIAA(SDR); esp[11] = CIAB(SDR);
    CIAA(SDR) = 0x11;
    CIAB(SDR) = 0x22;
    esp[12] = B8(0xBFCC00);
    esp[13] = B8(0xBFCC01);
    esp[14] = B8(0xBFEC00);
    esp[15] = B8(0xBFDC01);
    esp[16] = B8(0xBFFC01);
    esp[17] = B8(0xBFFC00);
    esp[18] = B8(0xBFEC81);
    esp[19] = B8(0xBFDCFE);
    esp16[0] = W16(0xBFCC00);
    esp16[1] = W16(0xBFEC00);
    esp16[2] = W16(0xBFDC00);
    esp16[3] = W16(0xBFFC00);
    CIAA(SDR) = sdr_a;
    CIAB(SDR) = sdr_b;

    /* 4. CRA y CRB del CIA-B con el bit 7 a 1, y el bit 4 (LOAD) leído: se escriben sin START ni LOAD. */
    CIAB(CRA) = (cra_b & ~C_START) | 0x80;
    cr_bits[0] = CIAB(CRA);
    CIAB(CRA) = cra_b;
    cr_bits[1] = CIAB(CRA);
    CIAB(CRB) = (crb_b & ~C_START) | 0x80;
    cr_bits[2] = CIAB(CRB);
    CIAB(CRB) = crb_b;
    cr_bits[3] = CIAB(CRB);

    /* 5. El bit 14 de INTREQ. */
    W16(INTREQ) = SETCLR | 0x4000;
    intreq14[0] = W16(INTREQR);
    W16(INTREQ) = 0x4000;
    intreq14[1] = W16(INTREQR);

    /* 6. Nivel o flanco: el temporizador A del CIA-B, una vez, con su máscara (la puso AddICRVector). */
    if (ta) {
        CIAB(CRA) = 0;
        (void)CIAB(ICR);
        CIAB(TALO) = 100;
        W16(INTREQ) = F_EXTER;
        CIAB(TAHI) = 0;   /* sin START: en modo una vez, el byte alto lo pone en marcha */
        CIAB(CRA) = C_RUNMODE | C_LOAD | C_START;
        ULONG n = 0;
        while (!(W16(INTREQR) & F_EXTER) && ++n < LIMITE) {}
        nivel[0] = W16(INTREQR) & F_EXTER;
        W16(INTREQ) = F_EXTER;
        nivel[1] = W16(INTREQR) & F_EXTER;
        nivel_icr = CIAB(ICR);
        W16(INTREQ) = F_EXTER;
        nivel[2] = W16(INTREQR) & F_EXTER;
        nivel_hecho = n < LIMITE ? 1 : -1;
    }

    /* 7a. VERTB y el TOD del CIA-A: desde la línea 200, la primera lectura en que se ven, y la del haz de antes. */
    for (int k = 0; k < 3; k++) {
        espera_linea(200);
        W16(INTREQ) = F_VERTB;
        UBYTE t0 = CIAA(TODLO);
        UWORD antes = vh(), p1, p2;
        int vb = 0, td = 0;
        ULONG n = 0;
        while ((!vb || !td) && ++n < LIMITE) {
            p1 = vh();
            UWORD q = W16(INTREQR);
            UBYTE t = CIAA(TODLO);
            p2 = vh();
            if (!vb && (q & F_VERTB)) { vsync[k][0] = antes; vsync[k][1] = p2; vb = 1; }
            if (!td && t != t0) { vsync[k][2] = antes; vsync[k][3] = p2; td = 1; }
            antes = p1;
        }
    }

    /* 7b. El TOD del CIA-B, dentro de la línea. */
    for (int k = 0; k < 8; k++) {
        UBYTE t0 = CIAB(TODLO);
        UWORD antes = vh(), p1, p2;
        ULONG n = 0;
        while (++n < LIMITE) {
            p1 = vh();
            UBYTE t = CIAB(TODLO);
            p2 = vh();
            if (t != t0) { hsync[k][0] = antes; hsync[k][1] = p2; break; }
            antes = p1;
        }
        espera_linea((UBYTE)((vh() >> 8) + 3 + k));
    }

    /* 8. La alarma del TOD, en el CIA que la tenga libre. */
    ULONG tod_b0 = tod(&CIAB(0)), tod_a0 = tod(&CIAA(0));
    {
        /* Si nadie la deja libre, la del CIA-B sin pedirla: al acabar se pone poco después del TOD, para que su
           dueño reciba una alarma (antes de tiempo) en vez de ninguna. */
        volatile UBYTE *c = al || !al_a ? &CIAB(0) : &CIAA(0);
        UBYTE crb0 = REG(c, CRB) & ~C_LOAD, crb = crb0 & ~C_ALARM;  /* el sistema deja ALARM a 1 en el CIA-B */
        REG(c, CRB) = crb;
        alarma_cia = c == &CIAB(0) ? 'B' : 'A';
        alarma(c, 0xFFFFFF, crb);
        tod_poner(c, 0x004000);
        REG(c, TODHI) = 0x00;   /* parado */
        ULONG v = tod(c);
        alrm[0] = REG(c, ICR);
        alarma(c, v, crb);      /* igual al TOD parado */
        alrm[1] = REG(c, ICR);
        REG(c, CRB) = crb | C_ALARM;
        REG(c, TODLO) = (UBYTE)v; /* otra vez la misma */
        REG(c, CRB) = crb;
        alrm[2] = REG(c, ICR);
        alarma(c, 0x004080, crb);
        alrm[3] = REG(c, ICR);
        tod_poner(c, 0x004080); /* el TOD igual a la alarma */
        alrm[4] = REG(c, ICR);
        tod_poner(c, 0x00407E);
        (void)REG(c, ICR);
        espera_tod(c, 4);
        alrm[5] = REG(c, ICR);  /* contando */
        /* Escribir el byte alto lo para hasta el bajo; el del medio solo, no. */
        REG(c, TODHI) = 0x00;
        tod_paro[0] = tod(c);
        espera_tod(c, 4);
        tod_paro[1] = tod(c);
        REG(c, TODLO) = 0x00;
        espera_tod(c, 4);
        tod_paro[2] = tod(c);
        REG(c, TODMID) = 0x50;
        espera_tod(c, 4);
        tod_paro[3] = tod(c);
        alarma(c, 0xFFFFFF, crb);
        (void)REG(c, ICR);
        REG(c, CRB) = crb0;
        alarma_hecha = 1;
    }

    /* 9. El retraso al arrancar, con el temporizador A del CIA-B: cada modo ocho veces, con 0 a 7 vueltas de espera
          antes de escribir (para caer en fases distintas del reloj E). */
    if (ta) {
        volatile UBYTE *cra = &CIAB(CRA), *talo = &CIAB(TALO), *tahi = &CIAB(TAHI);
        for (int k = 0; k < N_RETRASO; k++)
            for (int i = 0; i < 8; i++) {
                UBYTE *o = retraso[k][i];
                CIAB(CRA) = 0;
                CIAB(TALO) = 0x80;
                CIAB(TAHI) = 0x10;      /* parado: carga $1080 */
                if (k == 2) CIAB(CRA) = C_RUNMODE;
                if (k >= 3) {
                    CIAB(CRA) = C_START;
                    for (int j = 0; j < 20; j++) (void)CIAB(TALO);
                }
                for (volatile int j = 0; j < i; j++) {}
                switch (k) {
                case 0: seis_lecturas(cra, C_START, talo, o); break;          /* START solo */
                case 1: seis_lecturas(cra, C_LOAD | C_START, talo, o); break; /* LOAD y START */
                case 2: seis_lecturas(tahi, 0x10, talo, o); break;            /* el byte alto en una vez */
                case 3: seis_lecturas(cra, C_START, talo, o); break;          /* en marcha: lo que baja */
                case 4: seis_lecturas(cra, 0, talo, o); break;                /* parar en marcha */
                case 5: seis_lecturas(cra, C_LOAD | C_START, talo, o); break; /* LOAD en marcha */
                }
            }
        CIAB(CRA) = 0;
        (void)CIAB(ICR);
        retraso_hecho = 1;
    }

    /* 10. El registro serie del CIA-B: en salida, con el reloj del temporizador A (latch 7: cada 8 pulsos del reloj
           E un desborde, y dos desbordes por bit); el B cuenta el reloj E para medir. */
    if (ta && tb) {
        CIAB(CRA) = 0;
        CIAB(CRB) = 0;
        (void)CIAB(ICR);
        CIAB(TALO) = 7;
        CIAB(TAHI) = 0;
        CIAB(TBLO) = 0xFF;
        CIAB(TBHI) = 0xFF;
        CIAB(CRB) = C_START;
        CIAB(CRA) = C_SPMODE | C_START;
        UBYTE h0 = CIAB(TBHI), l0 = CIAB(TBLO);
        CIAB(SDR) = 0xA5;
        UBYTE icr = 0;
        ULONG n = 0;
        while (!((icr |= CIAB(ICR)) & I_SP) && ++n < LIMITE) {}
        UBYTE h1 = CIAB(TBHI), l1 = CIAB(TBLO);
        serie_e[0] = (UWORD)(h0 << 8 | l0);
        serie_e[1] = (UWORD)(h1 << 8 | l1);
        serie_icr = icr;
        serie_sdr = CIAB(SDR);
        CIAB(CRA) = 0;
        CIAB(CRB) = 0;
        /* En entrada: PA0 es SP y PA1 es CNT (sube: entra un bit). */
        UBYTE ddra = CIAB(DDRA), pra = CIAB(PRA);
        (void)CIAB(ICR);
        CIAB(DDRA) = ddra | 0x03;
        UBYTE dato = 0xC5;
        for (int i = 7; i >= 0; i--) {
            UBYTE bit = (dato >> i) & 1;
            CIAB(PRA) = (pra & ~0x03) | bit;
            CIAB(PRA) = (pra & ~0x03) | bit | 0x02;
        }
        entrada_icr = CIAB(ICR);
        entrada_sdr = CIAB(SDR);
        CIAB(PRA) = pra;
        CIAB(DDRA) = ddra;
        serie_hecha = 1;
    }

    /* Repone el CIA-B. */
    (void)CIAB(ICR);
    CIAB(TALO) = 0xFF;
    CIAB(TAHI) = 0xFF;
    CIAB(TBLO) = 0xFF;
    CIAB(TBHI) = 0xFF;
    CIAB(CRA) = cra_b;
    CIAB(CRB) = crb_b & ~C_ALARM;
    CIAB(SDR) = arranque_b_sdr;
    if (alarma_cia == 'B') tod_poner(&CIAB(0), tod_b0 + 313 * 4);
    if (alarma_cia == 'B' && !al) alarma(&CIAB(0), tod(&CIAB(0)) + 100, crb_b & ~C_ALARM);
    CIAB(CRB) = crb_b;
    if (alarma_cia == 'A') {
        UBYTE crb_a = CIAA(CRB) & ~C_LOAD;
        CIAA(CRB) = crb_a & ~C_ALARM;
        tod_poner(&CIAA(0), tod_a0 + 12);
        CIAA(CRB) = crb_a;
    }

    Enable();

    if (ta) RemICRVector(ciab, CIAICRB_TA, &irq_ta);
    if (tb) RemICRVector(ciab, CIAICRB_TB, &irq_tb);
    if (al) RemICRVector(ciab, CIAICRB_ALRM, &irq_alrm);
    if (al_a) RemICRVector(ciaa, CIAICRB_ALRM, &irq_alrm_a);

    di("cia: recursos del CIA-B: TA %s, TB %s, ALRM %s\n", (LONG)(ta ? "libre" : "ocupado"),
       (LONG)(tb ? "libre" : "ocupado"), (LONG)(al ? "libre" : "ocupado"), 0);
    if (!al) di("cia: recursos del CIA-A: ALRM %s\n", (LONG)(al_a ? "libre" : "ocupado"), 0, 0, 0);
    di("cia: arranque A: PRA $%02lx PRB $%02lx DDRA $%02lx DDRB $%02lx\n", arranque_a[0], arranque_a[1], arranque_a[2],
       arranque_a[3]);
    di("cia: arranque A: CRA $%02lx CRB $%02lx\n", arranque_a[4], arranque_a[5], 0, 0);
    di("cia: arranque B: PRA $%02lx PRB $%02lx DDRA $%02lx DDRB $%02lx\n", arranque_b[0], arranque_b[1], arranque_b[2],
       arranque_b[3]);
    di("cia: arranque B: CRA $%02lx CRB $%02lx\n", arranque_b[4], arranque_b[5], 0, 0);
    di("cia: DMACONR $%04lx INTENAR $%04lx\n", dmaconr0, intenar0, 0, 0);
    di("cia: registro $B: A $%02lx $%02lx, B $%02lx $%02lx\n", reg_b[0], reg_b[2], reg_b[1], reg_b[3]);
    di("cia: espejos, escribir SDR (A B): $BFCC01 $%02lx $%02lx; $BFEC00 $%02lx $%02lx\n", esp[0], esp[1], esp[2],
       esp[3]);
    di("cia: espejos, escribir SDR (A B): $BFDC01 $%02lx $%02lx; $BFFC01 $%02lx $%02lx\n", esp[4], esp[5], esp[6],
       esp[7]);
    di("cia: espejos, escribir SDR (A B): $BFEC41 $%02lx $%02lx; .w $BFCC00 $%02lx $%02lx\n", esp[8], esp[9], esp[10],
       esp[11]);
    di("cia: espejos, leer (A=$11 B=$22): $BFCC00 $%02lx $BFCC01 $%02lx $BFEC00 $%02lx $BFDC01 $%02lx\n", esp[12],
       esp[13], esp[14], esp[15]);
    di("cia: espejos, leer (A=$11 B=$22): $BFFC01 $%02lx $BFFC00 $%02lx $BFEC81 $%02lx $BFDCFE $%02lx\n", esp[16],
       esp[17], esp[18], esp[19]);
    di("cia: espejos, leer .w: $BFCC00 $%04lx $BFEC00 $%04lx $BFDC00 $%04lx $BFFC00 $%04lx\n", esp16[0], esp16[1],
       esp16[2], esp16[3]);
    di("cia: CRA con bit 7: $%02lx (luego $%02lx); CRB con bit 7: $%02lx (luego $%02lx)\n", cr_bits[0], cr_bits[1],
       cr_bits[2], cr_bits[3]);
    di("cia: INTREQ bit 14: puesto $%04lx, quitado $%04lx\n", intreq14[0], intreq14[1], 0, 0);
    if (nivel_hecho)
        di("cia: nivel: EXTER $%04lx; tras borrarlo sin leer ICR $%04lx; ICR $%02lx; tras leerlo y borrar $%04lx\n",
           nivel[0], nivel[1], nivel_icr, nivel[2]);
    for (int k = 0; k < 3; k++)
        di("cia: VSYNC: VERTB entre $%04lx y $%04lx; TOD-A entre $%04lx y $%04lx\n", vsync[k][0], vsync[k][1],
           vsync[k][2], vsync[k][3]);
    for (int k = 0; k < 8; k++)
        di("cia: HSYNC: TOD-B entre $%04lx y $%04lx\n", hsync[k][0], hsync[k][1], 0, 0);
    if (alarma_hecha) {
        di("cia: alarma del CIA-%lc%s\n", alarma_cia, (LONG)(al || al_a ? "" : ", sin pedirla"), 0, 0);
        di("cia: alarma: ICR antes $%02lx; alarma igual al TOD $%02lx; otra vez $%02lx; alarma distinta $%02lx\n",
           alrm[0], alrm[1], alrm[2], alrm[3]);
        di("cia: alarma: TOD igual a la alarma $%02lx; contando $%02lx\n", alrm[4], alrm[5], 0, 0);
        di("cia: TOD: alto escrito $%06lx, 4 pulsos despues $%06lx; bajo escrito, 4 pulsos $%06lx; medio $%06lx\n",
           tod_paro[0], tod_paro[1], tod_paro[2], tod_paro[3]);
    }
    if (retraso_hecho) {
        static const char *que[N_RETRASO] = { "START", "LOAD+START", "TAHI una vez", "en marcha", "parar en marcha",
                                              "LOAD+START en marcha" };
        for (int k = 0; k < N_RETRASO; k++) {
            di("cia: retraso %s (TALO, 3 lecturas, 8 veces):", (LONG)que[k], 0, 0, 0);
            for (int i = 0; i < 8; i++)
                di(" %02lx%02lx%02lx", retraso[k][i][0], retraso[k][i][1], retraso[k][i][2], 0);
            di("\n", 0, 0, 0, 0);
        }
    }
    if (serie_hecha) {
        di("cia: serie salida: TB $%04lx -> $%04lx (%ld pulsos E); ICR $%02lx\n", serie_e[0], serie_e[1],
           (LONG)(serie_e[0] - serie_e[1]), serie_icr);
        di("cia: serie salida: SDR $%02lx; entrada por PA0/PA1: ICR $%02lx SDR $%02lx\n", serie_sdr, entrada_icr,
           entrada_sdr, 0);
    }
    di("cia: fin\n", 0, 0, 0, 0);

    CloseLibrary((struct Library *)DOSBase);
    return 0;
}
