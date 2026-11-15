/*
    aviso-cajas: un programa con un manejador de memoria baja (AddMemHandler, V39), como los de los programas que
    guardan cachés. Guarda 2 MB de "caché" y espera, llamando a exec (es en una llamada a exec donde la caja llama a los
    manejadores cuando DashOS va justo de memoria). Cuando le llaman, suelta la caché. Acaba con 0 si le llamaron y la
    soltó, o con 5 si no llegó ningún aviso en 90 s.
*/
#include <exec/types.h>
#include <exec/memory.h>
#include <exec/interrupts.h>
#include <exec/nodes.h>
#include <proto/exec.h>
#include <proto/dos.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;

static void di(const char *f, LONG a, LONG b, LONG c)
{
    LONG v[3] = { a, b, c };
    VPrintf((CONST_STRPTR)f, v);
}

#define CACHE (2 << 20)

static APTR cache;
static volatile LONG llamadas;
static volatile LONG reciclado;
static volatile ULONG soltado;
static struct Interrupt manejador_is;

/* El manejador: A0 = MemHandlerData, A1 = is_Data. Suelta la caché, si la tiene. */
static LONG manejador(struct MemHandlerData *d __asm("a0"), APTR datos __asm("a1"))
{
    llamadas++;
    if (d->memh_Flags & MEMHF_RECYCLE) reciclado++;
    if (datos != &manejador_is || !cache) return MEM_DID_NOTHING;
    FreeMem(cache, CACHE);
    cache = 0;
    soltado += CACHE;
    return MEM_ALL_DONE;
}

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    if (!DOSBase) return 20;
    cache = AllocMem(CACHE, MEMF_ANY | MEMF_CLEAR);
    if (!cache) {
        di("  [caja-amiga] memh: sin memoria para la cache\n", 0, 0, 0);
        CloseLibrary((struct Library *)DOSBase);
        return 10;
    }
    manejador_is.is_Node.ln_Type = NT_INTERRUPT;
    manejador_is.is_Node.ln_Pri = 0;
    manejador_is.is_Node.ln_Name = (char *)"memh";
    manejador_is.is_Data = &manejador_is;
    manejador_is.is_Code = (VOID (*)())manejador;
    AddMemHandler(&manejador_is);
    di("  [caja-amiga] memh: guarda %ld KB de cache, con su manejador de memoria baja, y espera el aviso\n", CACHE >> 10, 0, 0);
    ULONG i;
    for (i = 0; i < 900 && !llamadas; i++) {
        (void)AvailMem(MEMF_ANY);
        Delay(5);
    }
    RemMemHandler(&manejador_is);
    if (cache) FreeMem(cache, CACHE);
    if (llamadas)
        di("  [caja-amiga] memh: su manejador se llamo %ld veces y solto %ld KB de cache\n", llamadas, soltado >> 10, 0);
    else
        di("  [caja-amiga] memh: no llego el aviso de memoria baja en 90 s\n", 0, 0, 0);
    CloseLibrary((struct Library *)DOSBase);
    return llamadas && soltado == CACHE ? 0 : 5;
}
