/*
    La caja Amiga en Linux (docs/CAJA_LINUX_LOOP.md, L5): el teclado de Wayland en una ventana de Intuition. Abre una
    ventana con IDCMP_VANILLAKEY y escribe cada tecla que le llega ("tecla: x"). Acaba con "q" (devuelve 0), con su
    gadget de cerrar (0) o a los 20 s (5).
*/
#include <exec/types.h>
#include <exec/execbase.h>
#include <dos/dos.h>
#include <intuition/intuition.h>
#include <proto/exec.h>
#include <proto/dos.h>
#include <proto/intuition.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;
struct IntuitionBase *IntuitionBase;

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    IntuitionBase = (struct IntuitionBase *)OpenLibrary((CONST_STRPTR)"intuition.library", 36);
    if (!DOSBase || !IntuitionBase)
        return 20;
    struct Window *w = OpenWindowTags(NULL, WA_Title, (ULONG)"Teclas: escribe (q acaba)", WA_Width, 300, WA_Height, 60,
                                      WA_IDCMP, IDCMP_VANILLAKEY | IDCMP_CLOSEWINDOW, WA_CloseGadget, TRUE,
                                      WA_DragBar, TRUE, WA_Activate, TRUE, TAG_END);
    if (!w)
        return 20;
    PutStr((CONST_STRPTR)"ventana abierta\n");
    Flush(Output());
    LONG r = 5;
    for (int i = 0; i < 400 && r == 5; i++) {
        struct IntuiMessage *m;
        while ((m = (struct IntuiMessage *)GetMsg(w->UserPort))) {
            ULONG clase = m->Class;
            UWORD codigo = m->Code;
            ReplyMsg((struct Message *)m);
            if (clase == IDCMP_CLOSEWINDOW) r = 0;
            else if (clase == IDCMP_VANILLAKEY) {
                LONG a[1];
                a[0] = codigo;
                VPrintf((CONST_STRPTR)"tecla: %lc\n", a);
                Flush(Output());
                if (codigo == 'q') r = 0;
            }
        }
        Delay(2);
    }
    CloseWindow(w);
    CloseLibrary((struct Library *)IntuitionBase);
    CloseLibrary((struct Library *)DOSBase);
    return r;
}
