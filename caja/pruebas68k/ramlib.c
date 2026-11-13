/*
    ramlib en la caja: abre bibliotecas que no son nativas y están en el disco (Caja:Libs, que es LIBS:), las usa, las
    cierra (y se echan) y prueba lo malo: un RomTag falso, una que miente con su versión, un fichero que no es un
    ejecutable, una que no existe, nombres con caminos raros y las que nunca se cargan de disco (lowlevel, que habla
    con el hardware; layers ya es nativa). Escribe cada comprobación y acaba con 0 si todas van bien.
*/
#include <exec/types.h>
#include <exec/execbase.h>
#include <exec/libraries.h>
#include <proto/exec.h>
#include <proto/dos.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;
static LONG fallos;

static void di(const char *f, LONG a, LONG b, LONG c)
{
    LONG v[3] = { a, b, c };
    VPrintf((CONST_STRPTR)f, v);
}

static void mira(const char *que, LONG tiene, LONG debe)
{
    if (tiene == debe) di("  [caja-amiga] ramlib: %s = %ld: bien\n", (LONG)que, tiene, 0);
    else {
        di("  [caja-amiga] ramlib: %s = %ld (se esperaba %ld): MAL\n", (LONG)que, tiene, debe);
        fallos++;
    }
}

/* Suma(a, b)(d0/d1), la función 5 (-30) de prueba.library. */
static LONG suma(struct Library *base, LONG x, LONG y)
{
    register LONG d0 __asm("d0") = x;
    register LONG d1 __asm("d1") = y;
    register struct Library *a6 __asm("a6") = base;
    __asm volatile("jsr -30(a6)" : "+r"(d0), "+r"(d1) : "r"(a6) : "a0", "a1", "cc", "memory");
    return d0;
}

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    if (!DOSBase) return 20;
    struct Library *b = OpenLibrary((CONST_STRPTR)"prueba.library", 3);
    mira("OpenLibrary(prueba.library) de disco", b != NULL, 1);
    if (b) {
        mira("su versión", b->lib_Version, 3);
        mira("Suma(40, 2)", suma(b, 40, 2), 42);
        struct Library *b2 = OpenLibrary((CONST_STRPTR)"prueba.library", 0);
        mira("otra vez: la misma (ya en memoria)", b2 == b, 1);
        mira("abierta dos veces", b->lib_OpenCnt, 2);
        CloseLibrary(b2);
        CloseLibrary(b);
    }
    Forbid();
    struct Library *sigue = (struct Library *)FindName(&SysBase->LibList, (CONST_STRPTR)"prueba.library");
    Permit();
    mira("al cerrarla el último, se echa", sigue == NULL, 1);
    /* Por su camino entero. */
    b = OpenLibrary((CONST_STRPTR)"Caja:Libs/prueba.library", 1);
    mira("por su camino (Caja:Libs/prueba.library)", b != NULL, 1);
    if (b) CloseLibrary(b);
    /* Lo malo. */
    mira("una versión más nueva de la que hay (99)", OpenLibrary((CONST_STRPTR)"prueba.library", 99) == NULL, 1);
    mira("RomTag falso (falsa.library)", OpenLibrary((CONST_STRPTR)"falsa.library", 0) == NULL, 1);
    mira("versión que miente (miente.library, se pide la 2)", OpenLibrary((CONST_STRPTR)"miente.library", 2) == NULL, 1);
    mira("un fichero que no es un ejecutable (rota.library)", OpenLibrary((CONST_STRPTR)"rota.library", 0) == NULL, 1);
    mira("una que no existe", OpenLibrary((CONST_STRPTR)"noexiste.library", 0) == NULL, 1);
    mira("camino raro (LIBS://../S/Startup-Sequence)", OpenLibrary((CONST_STRPTR)"LIBS://../S/Startup-Sequence", 0) == NULL, 1);
    mira("lowlevel.library, nunca de disco", OpenLibrary((CONST_STRPTR)"lowlevel.library", 0) == NULL, 1);
    {
        struct Library *l = OpenLibrary((CONST_STRPTR)"layers.library", 0);
        mira("layers.library, la nativa", l != NULL, 1);
        if (l) CloseLibrary(l);
    }
    struct Library *g = OpenLibrary((CONST_STRPTR)"graphics.library", 0);
    mira("graphics.library, la nativa", g != NULL, 1);
    if (g) CloseLibrary(g);
    Forbid();
    sigue = (struct Library *)FindName(&SysBase->LibList, (CONST_STRPTR)"prueba.library");
    Permit();
    mira("ninguna se queda en memoria", sigue == NULL, 1);
    di("  [caja-amiga] ramlib: %ld fallos\n", fallos, 0, 0);
    CloseLibrary((struct Library *)DOSBase);
    return fallos ? 10 : 0;
}
