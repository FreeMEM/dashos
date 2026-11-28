/*
    prueba.library: una biblioteca de disco para ramlib (caja/exec/src/ramlib.rs), escrita a mano con su RomTag y la
    tabla de RTF_AUTOINIT, como las del RKRM (Libraries, "Exec Libraries"). Funciones: Open, Close, Expunge, la
    reservada y Suma(a, b)(d0/d1). Expunge con la biblioteca abierta solo la marca (LIBF_DELEXP); cerrada, sale de la
    lista, suelta su memoria y devuelve su lista de segmentos. Con -DVERSION=n y -DFALSA (el RomTag no se apunta a sí
    mismo) salen las variantes hostiles.
*/
#include <exec/types.h>
#include <exec/libraries.h>
#include <exec/resident.h>
#include <exec/nodes.h>
#include <exec/initializers.h>
#include <dos/dos.h>
#include <proto/exec.h>

#ifndef VERSION
#define VERSION 3
#endif

struct Base {
    struct Library lib;
    BPTR segmentos;
    struct ExecBase *sys;
};

#define SysBase (b->sys)

#ifdef MANUAL
static const char nombre[] = "manual.library";
#else
static const char nombre[] = "prueba.library";
#endif
static const char id[] = "prueba.library 3.0 (13.11.2026) de la caja de DashOS";

static struct Base *b_init(struct Base *b __asm("d0"), BPTR seg __asm("a0"), struct ExecBase *sys __asm("a6"))
{
    b->sys = sys;
    b->segmentos = seg;
    b->lib.lib_Node.ln_Type = NT_LIBRARY;
    b->lib.lib_Node.ln_Name = (char *)nombre;
    b->lib.lib_Flags = LIBF_SUMUSED | LIBF_CHANGED;
    b->lib.lib_Version = VERSION;
    b->lib.lib_Revision = 0;
    b->lib.lib_IdString = (APTR)id;
    return b;
}

static BPTR b_expunge(struct Base *b __asm("a6"))
{
    if (b->lib.lib_OpenCnt) {
        b->lib.lib_Flags |= LIBF_DELEXP;
        return 0;
    }
    BPTR seg = b->segmentos;
    Remove(&b->lib.lib_Node);
    ULONG neg = b->lib.lib_NegSize, total = neg + b->lib.lib_PosSize;
    FreeMem((UBYTE *)b - neg, total);
    return seg;
}

static struct Base *b_open(struct Base *b __asm("a6"))
{
    b->lib.lib_OpenCnt++;
    b->lib.lib_Flags &= ~LIBF_DELEXP;
    return b;
}

static BPTR b_close(struct Base *b __asm("a6"))
{
    if (--b->lib.lib_OpenCnt == 0 && (b->lib.lib_Flags & LIBF_DELEXP)) return b_expunge(b);
    return 0;
}

static LONG b_nada(void) { return 0; }

static LONG b_suma(LONG x __asm("d0"), LONG y __asm("d1")) { return x + y; }

static const APTR vectores[] = { (APTR)b_open, (APTR)b_close, (APTR)b_expunge, (APTR)b_nada, (APTR)b_suma, (APTR)-1 };
#ifndef MANUAL
static const ULONG tabla[4] = { sizeof(struct Base), (ULONG)vectores, 0, (ULONG)b_init };
#endif

#ifdef MANUAL
/* Sin RTF_AUTOINIT: su Init hace la biblioteca y la añade a mano, y devuelve otra cosa que la base (como
   mathieeedoubtrans.library de AmigaOS 3.9, que devuelve lo que AddLibrary deja en D0). ramlib la busca por su nombre. */
static ULONG m_init(BPTR seg __asm("a0"), struct ExecBase *sys __asm("a6"))
{
    struct Base *b;
    {
        register APTR a0 __asm("a0") = (APTR)vectores;
        register APTR a1 __asm("a1") = 0;
        register APTR a2 __asm("a2") = 0;
        register ULONG d0 __asm("d0") = sizeof(struct Base);
        register ULONG d1 __asm("d1") = 0;
        register struct ExecBase *a6 __asm("a6") = sys;
        __asm volatile ("jsr a6@(-84:W)" : "+r"(d0), "+r"(d1), "+r"(a0), "+r"(a1), "+r"(a2) : "r"(a6) : "cc", "memory");
        b = (struct Base *)d0;
    }
    if (!b)
        return 0;
    b_init(b, seg, sys);
    {
        register struct Library *a1 __asm("a1") = &b->lib;
        register ULONG d0 __asm("d0") = (ULONG)b + 8;
        register struct ExecBase *a6 __asm("a6") = sys;
        __asm volatile ("jsr a6@(-396:W)" : "+r"(d0), "+r"(a1) : "r"(a6) : "d1", "a0", "cc", "memory");
        return d0;
    }
}
#endif

const struct Resident romtag = {
    RTC_MATCHWORD,
#ifdef FALSA
    (struct Resident *)0x12345678,
#else
    (struct Resident *)&romtag,
#endif
    (APTR)(&romtag + 1),
#ifdef MANUAL
    0,
#else
    RTF_AUTOINIT,
#endif
    VERSION,
    NT_LIBRARY,
    0,
    (char *)nombre,
    (char *)id,
#ifdef MANUAL
    (APTR)m_init,
#else
    (APTR)tabla,
#endif
};
