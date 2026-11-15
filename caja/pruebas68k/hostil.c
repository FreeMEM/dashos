/*
    H5, prueba hostil de la caja Amiga: punteros malos a OpenWindow, OpenWindowTagList, las funciones de dibujo y las
    de dos y exec, etiquetas en círculo y cosas sin soltar al acabar. Ninguna llamada debe romper la caja ni el
    compositor: cada una falla como en el Amiga (NULL, -1, DOSFALSE) y la caja lo apunta (el Enforcer). El programa no
    toca él mismo ningún puntero malo: solo los pasa. Devuelve cuántas de las llamadas malas "funcionaron" (0 es bien).
*/
#include <exec/types.h>
#include <exec/execbase.h>
#include <exec/memory.h>
#include <dos/dos.h>
#include <intuition/intuition.h>
#include <graphics/rastport.h>
#include <utility/tagitem.h>
#include <proto/exec.h>
#include <proto/dos.h>
#include <proto/intuition.h>
#include <proto/graphics.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;
struct IntuitionBase *IntuitionBase;
struct GfxBase *GfxBase;

#define MAL(x) ((void *)(ULONG)(x))

/* Una llamada mala que no falló como debía: se cuenta y se dice cuál (su línea). */
static LONG funcionan;
static void funciono(LONG linea)
{
    funcionan++;
    VPrintf((CONST_STRPTR)"  [caja-amiga] hostil: MAL: la llamada de la linea %ld no fallo\n", &linea);
}
#define FUNCIONA() funciono(__LINE__)

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    if (!DOSBase)
        return 20;
    IntuitionBase = (struct IntuitionBase *)OpenLibrary((CONST_STRPTR)"intuition.library", 37);
    GfxBase = (struct GfxBase *)OpenLibrary((CONST_STRPTR)"graphics.library", 37);
    if (!IntuitionBase || !GfxBase)
        return 21;
    LONG hechas = 0;

    /* OpenWindow y OpenWindowTagList con punteros malos: NULL, la página 0, fuera de la memoria, el final de los 4 GB. */
    static const ULONG malos[] = { 0, 4, 0x10, 0x00F00000, 0xDEAD0000, 0xFFFFFFF0 };
    for (unsigned i = 0; i < sizeof(malos) / sizeof(malos[0]); i++) {
        if (malos[i] && OpenWindow((struct NewWindow *)MAL(malos[i])))
            FUNCIONA();
        if (malos[i] && OpenWindowTagList(NULL, (struct TagItem *)MAL(malos[i])))
            FUNCIONA();
        hechas += 2;
    }
    /* Etiquetas en círculo (TAG_MORE a sí mismas) y una pantalla que no existe. */
    static struct TagItem circulo[3];
    circulo[0].ti_Tag = WA_Width;
    circulo[0].ti_Data = 100;
    circulo[1].ti_Tag = TAG_MORE;
    circulo[1].ti_Data = (ULONG)circulo;
    if (OpenWindowTagList(NULL, circulo))
        FUNCIONA();
    if (OpenWindowTags(NULL, WA_CustomScreen, (ULONG)MAL(0xDEAD0000), WA_Width, 100, WA_Height, 50, TAG_END))
        FUNCIONA();
    hechas += 2;

    /* Dibujo con RastPort malos. */
    for (unsigned i = 0; i < sizeof(malos) / sizeof(malos[0]); i++) {
        struct RastPort *rp = (struct RastPort *)MAL(malos[i]);
        SetAPen(rp, 3);
        RectFill(rp, 0, 0, 100, 100);
        Move(rp, 0, 0);
        Draw(rp, 50, 50);
        if (WritePixel(rp, 1, 1) == 0)
            FUNCIONA();
        Text(rp, (CONST_STRPTR)"x", 1);
        hechas += 6;
    }
    /* CloseWindow de lo que no es una ventana. */
    CloseWindow((struct Window *)MAL(0x00300000));
    CloseWindow((struct Window *)MAL(0xDEAD0000));
    hechas += 2;

    /* dos: nombres y búferes malos, ficheros que no son, cerrar dos veces. */
    if (Open((CONST_STRPTR)MAL(0xDEAD0000), MODE_OLDFILE))
        FUNCIONA();
    if (Open((CONST_STRPTR)"SYS:/../../fuera", MODE_OLDFILE))
        FUNCIONA();
    if (Open((CONST_STRPTR)"SYS:Nuevo.txt", MODE_NEWFILE))
        FUNCIONA();   /* SYS: es de solo lectura */
    if (Read((BPTR)0x1234, MAL(0x00300000), 10) >= 0)
        FUNCIONA();
    hechas += 4;
    BPTR fh = Open((CONST_STRPTR)"SYS:LEEME.txt", MODE_OLDFILE);
    if (fh) {
        if (Read(fh, MAL(0x10), 8) >= 0)
            FUNCIONA();
        if (Read(fh, MAL(0xDEAD0000), 8) >= 0)
            FUNCIONA();
        Close(fh);
        if (Close(fh))
            FUNCIONA();
        hechas += 3;
    }

    /* exec: soltar lo que no es suyo. */
    FreeMem(MAL(0x00300000), 64);
    FreeMem(MAL(0xDEAD0000), 8);
    hechas += 2;

    /* Lo que se deja sin soltar al acabar: memoria, un fichero abierto y un cerrojo. Lo suelta la caja. */
    AllocMem(1000, MEMF_ANY);
    Open((CONST_STRPTR)"SYS:LEEME.txt", MODE_OLDFILE);
    Lock((CONST_STRPTR)"SYS:", ACCESS_READ);

    LONG args[2] = { hechas, funcionan };
    VPrintf((CONST_STRPTR)"  [caja-amiga] hostil: %ld llamadas malas, %ld funcionaron; el programa sigue vivo\n", args);
    CloseLibrary((struct Library *)GfxBase);
    CloseLibrary((struct Library *)IntuitionBase);
    CloseLibrary((struct Library *)DOSBase);
    return funcionan;
}
