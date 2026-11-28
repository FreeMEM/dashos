/*
    Prueba de ciab.resource nativo y del temporizador A de la CIA-B imitado (rama amiga-apps), como lo usan los
    reproductores de módulos (ProTracker): AddICRVector del bit 0, el periodo en TALO/TAHI ($BFD400/$BFD500) y la marcha
    en CRA ($BFDE00), escribiendo en los registros. Cuenta las interrupciones en 2 s a 50 Hz (14188 cuentas del reloj E
    de 709.379 Hz) y las dice. Lo hostil: un segundo AddICRVector del mismo bit da la interrupción de antes (ocupado);
    RemICRVector y la cuenta se para. Acaba con 0 si llegaron entre 80 y 120.
*/
#include <exec/types.h>
#include <exec/interrupts.h>
#include <exec/memory.h>
#include <resources/cia.h>
#include <hardware/cia.h>
#include <dos/dos.h>
#include <proto/exec.h>
#include <proto/dos.h>
#include <proto/cia.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;
struct Library *CiabBase;

static volatile ULONG cuenta;

static void di(const char *f, LONG a, LONG b)
{
    LONG v[2] = { a, b };
    VPrintf((CONST_STRPTR)f, v);
}

/* La interrupción: A1 = is_Data. */
static ULONG __attribute__((used)) servidor(void)
{
    cuenta++;
    return 0;
}

static struct Interrupt irq, otra;

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 37);
    if (!DOSBase) return 20;
    CiabBase = OpenResource((CONST_STRPTR)CIABNAME);
    if (!CiabBase) {
        di("  [caja-amiga] cia_prueba: no hay ciab.resource\n", 0, 0);
        return 20;
    }
    irq.is_Node.ln_Type = NT_INTERRUPT;
    irq.is_Node.ln_Name = (char *)"cia_prueba";
    irq.is_Code = (void (*)(void))servidor;
    otra = irq;
    if (AddICRVector(CiabBase, CIAICRB_TA, &irq)) {
        di("  [caja-amiga] cia_prueba: el temporizador A de la CIA-B está ocupado\n", 0, 0);
        return 10;
    }
    LONG ocupado = (LONG)AddICRVector(CiabBase, CIAICRB_TA, &otra);
    volatile UBYTE *talo = (UBYTE *)0xBFD400, *tahi = (UBYTE *)0xBFD500, *cra = (UBYTE *)0xBFDE00;
    UWORD latch = 14188;
    *talo = latch & 0xFF;
    *tahi = latch >> 8;
    *cra = (*cra & ~CIACRAF_RUNMODE) | CIACRAF_START | CIACRAF_LOAD;
    Delay(100);
    ULONG en2s = cuenta;
    *cra &= ~CIACRAF_START;
    RemICRVector(CiabBase, CIAICRB_TA, &irq);
    ULONG tras = cuenta;
    Delay(25);
    LONG bien = en2s >= 80 && en2s <= 120 && ocupado == (LONG)&irq && cuenta == tras;
    di("  [caja-amiga] cia_prueba: %ld interrupciones en 2 s a 50 Hz (ocupado: %ld)\n", en2s, ocupado == (LONG)&irq);
    di("  [caja-amiga] cia_prueba: %s\n", (LONG)(bien ? "bien" : "MAL"), 0);
    CloseLibrary((struct Library *)DOSBase);
    return bien ? 0 : 5;
}
