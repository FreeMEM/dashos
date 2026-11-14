/*
    La memoria del 68k crece según la pidan los programas, hasta un tope (paso 1 de la tanda de Tachyon). Hostil: pide
    1 MB tras otro sin soltar nada hasta que no le dan más, y algo absurdo (2 GB). No pasa del tope (la caja le pone
    uno de prueba, 48 MB, o su cupo si es menos) y nada se cuelga; al soltarlo todo, un bloque de la mitad cabe sin
    crecer (exec lo vuelve a repartir). AvailMem cuenta lo que queda por crecer. Acaba con 0 si todo cuadra.
*/
#include <exec/types.h>
#include <exec/memory.h>
#include <proto/exec.h>
#include <proto/dos.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;

static void di(const char *f, LONG a, LONG b, LONG c)
{
    LONG v[3] = { a, b, c };
    VPrintf((CONST_STRPTR)f, v);
}

#define MAX_BLOQUES 1024
static APTR bloques[MAX_BLOQUES];

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    if (!DOSBase) return 20;
    LONG fallos = 0;
    ULONG antes = AvailMem(MEMF_ANY);
    di("  [caja-amiga] glotona: AvailMem antes, %ld MB (con lo que puede crecer)\n", antes >> 20, 0, 0);
    ULONG n = 0;
    while (n < MAX_BLOQUES) {
        APTR p = AllocMem(1 << 20, MEMF_ANY);
        if (!p) break;
        ((volatile UBYTE *)p)[(1 << 20) - 1] = 0x5A;   /* que se pueda usar de verdad, hasta el final */
        bloques[n++] = p;
    }
    di("  [caja-amiga] glotona: %ld MB antes de quedarse sin sitio\n", n, 0, 0);
    /* Para en el tope (48 MB desde el MB 1) o antes, en el cupo de la caja (en una máquina pequeña, mucho antes). */
    if (n >= 48) { di("  [caja-amiga] glotona: MAL (pasó del tope)\n", 0, 0, 0); fallos++; }
    if (AllocMem(0x7FFF0000, MEMF_ANY) || AllocMem(1 << 20, MEMF_ANY)) { di("  [caja-amiga] glotona: MAL (le dieron más)\n", 0, 0, 0); fallos++; }
    ULONG mayor = AvailMem(MEMF_ANY | MEMF_LARGEST);
    di("  [caja-amiga] glotona: el mayor libre, %ld KB\n", mayor >> 10, 0, 0);
    for (ULONG i = 0; i < n; i++) FreeMem(bloques[i], 1 << 20);
    /* Soltado todo, la mitad de lo que tuvo, de una vez: cabe sin crecer (exec reparte otra vez lo crecido). */
    ULONG mitad = (n / 2) << 20;
    APTR grande = mitad ? AllocMem(mitad, MEMF_ANY | MEMF_CLEAR) : (APTR)1;
    di("  [caja-amiga] glotona: soltado todo, %ld MB de una vez %s\n", mitad >> 20, (LONG)(grande ? "caben: bien" : "no caben: MAL"), 0);
    if (!grande) fallos++; else if (mitad) FreeMem(grande, mitad);
    di("  [caja-amiga] glotona: %ld fallos\n", fallos, 0, 0);
    CloseLibrary((struct Library *)DOSBase);
    return fallos ? 10 : 0;
}
