/*
    K3 (docs/CHIPS_LOOP.md): los chips del Amiga vistos desde un programa 68k, en orden: el haz (VPOSR y VHPOSR), el
    temporizador A y el ICR del CIA-A, su TOD (cuenta cuadros) e INTREQ. Cada paso dice "chips: <qué>: bien" o "MAL",
    con el detalle; al final, una línea de medidas (las líneas que tardó el temporizador...) y los fallos, que son
    también lo que devuelve.

    El mismo binario es el del oráculo (K4): en la caja, ella enciende los chips antes de lanzarlo (motor_chips, en
    caja/motor.h) y pone "  [caja-amiga] " delante de cada línea; en un Amiga (FS-UAE con el Kickstart), los chips
    están siempre. Por eso no llama a nada de la caja, y lo que depende del tiempo se mira con margen o en orden, no
    como un número exacto. Mide con el sistema parado (Disable) y escribe después: dos.library no se puede usar con
    las interrupciones paradas.

    Duda: no sé qué temporizadores de los CIA usa el Kickstart 3.1. El A del CIA-A se toma sin pedirlo (lo correcto
    sería AddICRVector de ciaa.resource, que la caja aún no tiene) y su CRA se deja como estaba al acabar (el latch no
    se puede leer: se queda con 709). Leer el ICR borra también los otros bits del CIA-A (el del teclado, SP, y los
    del temporizador B y la alarma): si el sistema los usa, con el sistema parado se pierde ese aviso. Si en K4 algo
    de eso se nota, pedir el temporizador con ciaa.resource o usar otro.
*/
#include <exec/types.h>
#include <exec/execbase.h>
#include <dos/dos.h>
#include <proto/exec.h>
#include <proto/dos.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;

/* Los registros, por su dirección (Hardware Reference Manual y hoja de datos del 8520). */
#define CUSTOM16(d) (*(volatile UWORD *)(0xDFF000 + (d)))
#define VPOSR_VHPOSR (*(volatile ULONG *)0xDFF004)  /* VPOSR y VHPOSR en un acceso: V8 y V7-V0 juntos */
#define INTREQR 0x01E
#define INTREQ 0x09C
#define CIAA(r) (*(volatile UBYTE *)(0xBFE001 + 0x100 * (r)))
enum { TALO = 4, TAHI = 5, TODLO = 8, TODMID = 9, TODHI = 10, ICR = 13, CRA = 14 };
#define ICR_TA 0x01
#define CRA_START 0x01
#define CRA_RUNMODE 0x08    /* una vez */
#define CRA_LOAD 0x10       /* carga forzada (no se guarda) */
#define CRA_DEL_SISTEMA 0xC0  /* TODIN y SPMODE: los del sistema, sin tocar */
#define INTF_SETCLR 0x8000
#define INTF_SOFT 0x0004    /* la interrupción de software de exec: con el sistema parado, no la atiende nadie */

/* 709 pulsos del reloj E (unos 1 ms) son 7090 ciclos de 68000 PAL: 15,6 líneas de 454 ciclos. Se cuentan los
   cambios de línea mientras espera: 15 o 16, y uno más de margen por cada lado (el de K4 en docs/CHIPS_LOOP.md). */
#define TICS 709
#define LINEAS_MIN 14
#define LINEAS_MAX 17
/* Lo más que se espera a algo (en lecturas): bastante más que un cuadro en cualquier motor. */
#define LIMITE 2000000
/* Tras el principio del cuadro, se espera a esta línea para leer el TOD: el pulso del VSYNC ya pasó. */
#define LINEA_TOD 20

static void di(const char *f, LONG a, LONG b, LONG c, LONG d)
{
    LONG v[4] = { a, b, c, d };
    VPrintf((CONST_STRPTR)f, v);
}

static LONG fallos;

static void mira(int bien, const char *que)
{
    if (!bien) fallos++;
    di("chips: %s: %s\n", (LONG)que, (LONG)(bien ? "bien" : "MAL"), 0, 0);
}

/* La línea del haz. En el Amiga, el acceso largo son dos lecturas de 16 bits: si entre VPOSR y VHPOSR la línea pasa
   de la 255 a la 256, sale V8 = 0 con V7-V0 = 0, la línea 0 (lo vio el oráculo, K4: un cuadro de más). Se lee hasta
   que dos lecturas seguidas dan la misma línea. */
static UWORD linea(void)
{
    UWORD a = (UWORD)((VPOSR_VHPOSR >> 8) & 0x1FF), b;
    while ((b = (UWORD)((VPOSR_VHPOSR >> 8) & 0x1FF)) != a) a = b;
    return a;
}

/* Espera al principio de un cuadro (la línea vuelve atrás) y después a LINEA_TOD. 0 si no llega. */
static int espera_cuadro(void)
{
    ULONG n = 0;
    UWORD antes = linea(), l;
    while ((l = linea()) >= antes && ++n < LIMITE) antes = l;
    while (linea() < LINEA_TOD && ++n < LIMITE) {}
    return n < LIMITE;
}

/* El TOD del CIA-A: leer el byte alto lo congela hasta leer el bajo. */
static ULONG tod(void)
{
    ULONG h = CIAA(TODHI);
    ULONG m = CIAA(TODMID);
    ULONG l = CIAA(TODLO);
    return h << 16 | m << 8 | l;
}

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    if (!DOSBase) return 20;

    /* Todo con el sistema parado; lo que se ve se apunta y se dice al acabar. */
    Disable();

    /* 1. El haz: la línea cambia, a la siguiente (o a la 0 al empezar cuadro). Y cuántas lecturas caben en una. */
    ULONG n = 0;
    UWORD l0 = linea(), l1, l2;
    while ((l1 = linea()) == l0 && ++n < LIMITE) {}
    int haz_cambia = n < LIMITE && (l1 == l0 + 1 || l1 == 0);
    ULONG por_linea = 0;
    while ((l2 = linea()) == l1 && ++por_linea < LIMITE) {}
    haz_cambia = haz_cambia && por_linea < LIMITE && (l2 == l1 + 1 || l2 == 0);

    /* 2. El temporizador A del CIA-A, una vez: el bit TA del ICR sube tras unas 15 líneas, el temporizador se para
          solo, y leer el ICR lo borra. */
    UBYTE cra0 = CIAA(CRA);
    CIAA(CRA) = cra0 & CRA_DEL_SISTEMA;    /* parado y continuo: escribir TAHI solo carga */
    (void)CIAA(ICR);                       /* lo que hubiera */
    CIAA(TALO) = TICS & 0xFF;
    CIAA(TAHI) = TICS >> 8;
    CIAA(CRA) = (cra0 & CRA_DEL_SISTEMA) | CRA_RUNMODE | CRA_LOAD | CRA_START;
    UWORD l = linea();
    ULONG lineas = 0;
    UBYTE icr = 0;
    n = 0;
    while (!((icr = CIAA(ICR)) & ICR_TA) && ++n < LIMITE) {
        UWORD m = linea();
        if (m != l) {
            lineas++;
            l = m;
        }
    }
    int ta_sube = (icr & ICR_TA) != 0;
    int ta_parado = !(CIAA(CRA) & CRA_START);
    UBYTE icr2 = CIAA(ICR);
    CIAA(CRA) = cra0 & ~CRA_LOAD;

    /* 3. El TOD del CIA-A cuenta cuadros: 1 entre dos, 5 en cinco. */
    int tod_bien = espera_cuadro();
    ULONG t1 = tod();
    tod_bien = tod_bien && espera_cuadro();
    ULONG t2 = tod();
    for (int i = 0; i < 5; i++) tod_bien = tod_bien && espera_cuadro();
    ULONG t3 = tod();

    /* 4. INTREQ: SOFT sube con SET y baja con CLR. */
    CUSTOM16(INTREQ) = INTF_SETCLR | INTF_SOFT;
    UWORD con = CUSTOM16(INTREQR);
    CUSTOM16(INTREQ) = INTF_SOFT;
    UWORD sin = CUSTOM16(INTREQR);

    Enable();

    mira(haz_cambia, "VPOSR y VHPOSR: la linea cambia a la siguiente");
    if (!haz_cambia) di("chips: la linea %ld, luego %ld y %ld (%ld lecturas)\n", l0, l1, l2, (LONG)n);
    mira(ta_sube && lineas >= LINEAS_MIN && lineas <= LINEAS_MAX,
         "temporizador A del CIA-A, una vez (709 tics): el bit TA del ICR sube a su hora");
    if (!ta_sube || lineas < LINEAS_MIN || lineas > LINEAS_MAX)
        di("chips: TA %s tras %ld lineas (margen de %ld a %ld)\n", (LONG)(ta_sube ? "sube" : "no sube"), (LONG)lineas,
           LINEAS_MIN, LINEAS_MAX);
    mira(ta_parado, "temporizador A del CIA-A: se para solo (START a 0)");
    mira(ta_sube && !(icr2 & ICR_TA), "ICR del CIA-A: leerlo lo borra");
    if (icr2 & ICR_TA) di("chips: la segunda lectura del ICR da $%lx\n", icr2, 0, 0, 0);
    mira(tod_bien && t2 - t1 == 1 && t3 - t2 == 5, "TOD del CIA-A: 1 por cuadro");
    if (!(tod_bien && t2 - t1 == 1 && t3 - t2 == 5))
        di("chips: TOD %ld, %ld y %ld (%s)\n", (LONG)t1, (LONG)t2, (LONG)t3,
           (LONG)(tod_bien ? "los cuadros llegaron" : "sin cuadros"));
    mira((con & INTF_SOFT) && !(sin & INTF_SOFT), "INTREQ: SOFT sube con SET y baja con CLR");
    if (!((con & INTF_SOFT) && !(sin & INTF_SOFT))) di("chips: INTREQR $%lx y $%lx\n", con, sin, 0, 0);
    di("chips: medida: %ld lecturas del haz por linea; TA tras %ld lineas; TOD +%ld y +%ld\n", (LONG)por_linea,
       (LONG)lineas, (LONG)(t2 - t1), (LONG)(t3 - t2));
    di("chips: %ld fallos\n", fallos, 0, 0, 0);

    CloseLibrary((struct Library *)DOSBase);
    return fallos;
}
