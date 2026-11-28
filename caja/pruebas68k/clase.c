/*
    La caja Amiga: MakeClass con un tamaño de instancia con basura en la mitad alta de D0 (lo hace Directory Opus 5). Es
    una prueba del oráculo: la misma salida en la caja y en un Amiga de verdad (FS-UAE con el Kickstart; tools/emu_run.py
    de Dash). Lo que devuelve y lo que queda en cl_InstSize y cl_InstOffset.
*/
#include <exec/types.h>
#include <dos/dos.h>
#include <intuition/classes.h>
#include <proto/exec.h>
#include <proto/dos.h>
#include <proto/intuition.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;
struct IntuitionBase *IntuitionBase;

/* MakeClass por su LVO con D0 entero: el inline del NDK lo pasa como UWORD (sin la mitad alta). */
static struct IClass *make_class(CONST_STRPTR id, CONST_STRPTR sup, ULONG tam)
{
    register struct IClass *d0r __asm("d0");
    register CONST_STRPTR a0 __asm("a0") = id;
    register CONST_STRPTR a1 __asm("a1") = sup;
    register APTR a2 __asm("a2") = 0;
    register ULONG d0 __asm("d0") = tam;
    register ULONG d1 __asm("d1") = 0;
    register struct IntuitionBase *a6 __asm("a6") = IntuitionBase;
    __asm volatile ("jsr a6@(-678:W)" : "=d"(d0r), "+a"(a0), "+a"(a1), "+d"(d1) : "a"(a6), "a"(a2), "0"(d0)
                    : "cc", "memory");
    return d0r;
}

static void prueba(ULONG tam)
{
    struct IClass *cl = make_class(NULL, (CONST_STRPTR)"rootclass", tam);
    LONG v[4] = { (LONG)tam, cl != NULL, cl ? cl->cl_InstSize : -1, cl ? cl->cl_InstOffset : -1 };
    VPrintf((CONST_STRPTR)"MakeClass(tam %lx): %ld, InstSize %ld, InstOffset %ld\n", v);
    if (cl)
        FreeClass(cl);
}

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 37);
    IntuitionBase = (struct IntuitionBase *)OpenLibrary((CONST_STRPTR)"intuition.library", 37);
    if (!DOSBase || !IntuitionBase)
        return 20;
    prueba(10);
    prueba(0x30000A);
    prueba(0xFFFF);
    prueba(0x10000);
    prueba(0);
    return 0;
}
