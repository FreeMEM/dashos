/*
    InternalLoadSeg con un flujo propio, como lo usa AddDataTypes para el código de un descriptor: el ejecutable está
    en memoria y lo lee una función de leer del programa (D1 = fh, D2 = búfer, D3 = largo, A6 = DOSBase), no dos.
    Comprueba que carga y que su código está donde dice la lista de segmentos, y lo hostil: la función de leer que da
    -1, que da más de lo pedido, datos que no son un ejecutable, sin función de leer. Acaba con 0 si todo sale como
    debe; si no, con el número del primer caso que falló.
*/
#include <exec/types.h>
#include <exec/memory.h>
#include <dos/dos.h>
#include <proto/exec.h>
#include <proto/dos.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;

static void di(const char *f, LONG a, LONG b, LONG c)
{
    LONG v[3] = { a, b, c };
    VPrintf((CONST_STRPTR)f, v);
}

/* Un ejecutable mínimo: un hunk de código de una palabra larga (RTS, RTS). */
static const ULONG ejecutable[] = {
    0x000003F3, 0, 1, 0, 0, 1,   /* HUNK_HEADER: sin nombres, 1 hunk, del 0 al 0, de 1 palabra larga */
    0x000003E9, 1, 0x4E754E75,   /* HUNK_CODE de 1 palabra larga */
    0x000003F2,                  /* HUNK_END */
};

/* El flujo: de dónde lee y qué hace la función de leer. */
struct flujo {
    const UBYTE *datos;
    LONG largo;
    LONG pos;
    LONG modo;                   /* 0 normal, 1 da -1, 2 da más de lo pedido */
};

static LONG leer(BPTR fh __asm("d1"), APTR buf __asm("d2"), LONG largo __asm("d3"))
{
    struct flujo *f = (struct flujo *)fh;
    if (f->modo == 1) return -1;
    if (f->modo == 2) return largo + 16;
    LONG n = f->largo - f->pos;
    if (n > largo) n = largo;
    if (n > 0) CopyMem((APTR)(f->datos + f->pos), buf, n);
    f->pos += n;
    return n;
}

static APTR reservar(ULONG largo __asm("d0"), ULONG req __asm("d1")) { return AllocMem(largo, req); }
static void soltar(APTR d __asm("a1"), ULONG largo __asm("d0")) { FreeMem(d, largo); }

static BPTR cargar(const void *datos, LONG largo, LONG modo, LONG *funciones)
{
    struct flujo f = { (const UBYTE *)datos, largo, 0, modo };
    LONG pila = 0;
    return InternalLoadSeg((BPTR)&f, 0, funciones, &pila);
}

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    if (!DOSBase) return 20;
    LONG funciones[3] = { (LONG)leer, (LONG)reservar, (LONG)soltar };
    LONG fallo = 0;

    /* 1. Carga, y su código está detrás del enlace del primer segmento. */
    BPTR seg = cargar(ejecutable, sizeof(ejecutable), 0, funciones);
    ULONG *s = (ULONG *)BADDR(seg);
    if (!seg || s[1] != 0x4E754E75) fallo = 1;
    else if (!InternalUnLoadSeg(seg, (VOID (*)())soltar)) fallo = 1;
    di("  [caja-amiga] ils: un flujo propio leido por su funcion: %s\n", (LONG)(fallo ? "MAL" : "carga"), 0, 0);

    /* 2. La función de leer da -1. */
    if (!fallo && cargar(ejecutable, sizeof(ejecutable), 1, funciones)) fallo = 2;
    /* 3. Da más de lo que se le pidió. */
    if (!fallo && cargar(ejecutable, sizeof(ejecutable), 2, funciones)) fallo = 3;
    /* 4. Lo leído no es un ejecutable. */
    static const ULONG basura[] = { 0x12345678, 0x9ABCDEF0 };
    if (!fallo && cargar(basura, sizeof(basura), 0, funciones)) fallo = 4;
    /* 5. Sin función de leer (la tabla a cero). */
    LONG vacias[3] = { 0, 0, 0 };
    if (!fallo && cargar(ejecutable, sizeof(ejecutable), 0, vacias)) fallo = 5;
    /* 6. Un flujo vacío. */
    if (!fallo && cargar(ejecutable, 0, 0, funciones)) fallo = 6;

    if (fallo) di("  [caja-amiga] ils: el caso %ld no sale como debe\n", fallo, 0, 0);
    else di("  [caja-amiga] ils: -1, de mas, basura, sin funcion y vacio dan 0: bien\n", 0, 0, 0);
    CloseLibrary((struct Library *)DOSBase);
    return fallo;
}
