/*
    La caja Amiga: más de dos.library, de lo que piden los comandos de C: de AmigaOS 3.9 (Eval, Addbuffers). Es una prueba
    del oráculo: la misma salida en la caja y en un Amiga de verdad (FS-UAE con el Kickstart; tools/emu_run.py de Dash).
    VFWritef con sus formatos de BCPL y AddBuffers con un dispositivo que no existe. MakeLink no: lo que hace depende del
    sistema de ficheros (el ram-handler de 3.1 hace los fuertes y no los simbólicos; el disco de FS-UAE, ninguno).
*/
#include <exec/types.h>
#include <exec/execbase.h>
#include <dos/dos.h>
#include <dos/dosextens.h>
#include <proto/exec.h>
#include <proto/dos.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;

static void escribe(const char *f, LONG a, LONG b, LONG c, LONG d)
{
    LONG v[4] = { a, b, c, d };
    VPrintf((CONST_STRPTR)f, v);
}

/* VFWritef(fh, format, argarray)(d1/d2/d3), por su LVO (el prototipo del NDK es VOID; lo que queda en D0 no es una
   cuenta: no se mira). */
static LONG vfwritef(BPTR fh, const char *f, LONG *v)
{
    register LONG d0 __asm("d0");
    register BPTR d1 __asm("d1") = fh;
    register const char *d2 __asm("d2") = f;
    register LONG *d3 __asm("d3") = v;
    register struct DosLibrary *a6 __asm("a6") = DOSBase;
    __asm volatile ("jsr a6@(-348:W)" : "=r"(d0), "+r"(d1), "+r"(d2), "+r"(d3) : "r"(a6) : "a0", "a1", "cc", "memory");
    return d0;
}

static void writef(const char *f, LONG a, LONG b, LONG c)
{
    LONG v[3] = { a, b, c };
    escribe("[%s] -> [", (LONG)f, 0, 0, 0);
    Flush(Output());
    vfwritef(Output(), f, v);
    Flush(Output());
    PutStr((CONST_STRPTR)"]\n");
}


LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 37);
    if (!DOSBase)
        return 20;
    /* VFWritef: los formatos de BCPL. */
    writef("%N", 42, 0, 0);
    writef("%N", -7, 0, 0);
    writef("%I5|", 42, 0, 0);
    writef("%I2|", 12345, 0, 0);
    writef("%X4", 0xBEEF1, 0, 0);
    writef("%XA", 0x1F, 0, 0);
    writef("%O3", 8, 0, 0);
    writef("%U4", -1, 0, 0);
    writef("%S y %S", (LONG)"uno", (LONG)"dos", 0);
    writef("%T8|", (LONG)"ab", 0, 0);
    writef("%C%C", 'h', 'i', 0);
    writef("%$%N", 1, 2, 0);
    writef("100%% y %Z", 3, 0, 0);
    writef("sin nada", 0, 0, 0);
    writef("%N*N", 5, 0, 0);
    writef("%X2", 0x1F, 0, 0);
    writef("%I3|", -5, 0, 0);
    writef("%U6|", 42, 0, 0);
    writef("%T2|", (LONG)"largo", 0, 0);
    writef("%Z%N", 7, 8, 0);
    /* AddBuffers: un dispositivo que no existe (sin la ventana de "inserte el volumen"). */
    ((struct Process *)FindTask(NULL))->pr_WindowPtr = (APTR)-1;
    LONG r = AddBuffers((CONST_STRPTR)"NOEXISTE:", 5);
    escribe("AddBuffers(NOEXISTE:, 5): %ld, IoErr %ld\n", r, IoErr(), 0, 0);
    return 0;
}
