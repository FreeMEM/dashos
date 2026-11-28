/*
    La caja Amiga: ramlib con una biblioteca de disco sin RTF_AUTOINIT (manual.library) cuya Init se añade a mano y
    devuelve otra cosa que su base, como mathieeedoubtrans.library de AmigaOS 3.9 (que abre MUI). Es una prueba del
    oráculo: la misma salida en la caja y en un Amiga de verdad (FS-UAE con el Kickstart; tools/emu_run.py de Dash, con
    --lib manual.library). Abre, llama a su función, la abre otra vez y cierra.
*/
#include <exec/types.h>
#include <exec/libraries.h>
#include <exec/execbase.h>
#include <dos/dos.h>
#include <proto/exec.h>
#include <proto/dos.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;

static void escribe(const char *f, LONG a, LONG b, LONG c, LONG d)
{
    LONG v[4] = { a, b, c, d };
    VPrintf((CONST_STRPTR)f, v);
}

static LONG suma(struct Library *b, LONG x, LONG y)
{
    register LONG d0 __asm("d0") = x;
    register LONG d1 __asm("d1") = y;
    register struct Library *a6 __asm("a6") = b;
    __asm volatile ("jsr a6@(-30:W)" : "+r"(d0), "+r"(d1) : "r"(a6) : "a0", "a1", "cc", "memory");
    return d0;
}

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 37);
    if (!DOSBase)
        return 20;
    struct Library *b = OpenLibrary((CONST_STRPTR)"manual.library", 0);
    escribe("OpenLibrary(manual.library): %ld\n", b != NULL, 0, 0, 0);
    if (!b)
        return 0;
    struct Node *n = FindName(&SysBase->LibList, (CONST_STRPTR)"manual.library");
    escribe("es la de la lista: %ld; version %ld; abierta %ld veces\n", n == &b->lib_Node, b->lib_Version, b->lib_OpenCnt, 0);
    escribe("Suma(2, 40): %ld\n", suma(b, 2, 40), 0, 0, 0);
    struct Library *b2 = OpenLibrary((CONST_STRPTR)"manual.library", 0);
    escribe("otra vez: la misma %ld, abierta %ld veces\n", b2 == b, b->lib_OpenCnt, 0, 0);
    CloseLibrary(b2);
    CloseLibrary(b);
    return 0;
}
