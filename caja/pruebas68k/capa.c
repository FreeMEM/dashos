/*
    El AmigaOS del usuario como capa (paso 2 de las bibliotecas de disco): si la caja montó uno (AmigaOS:), abre las
    bibliotecas de software que trae y escribe cuáles abren y con qué versión; las que tocan el hardware (lowlevel,
    audio.device) tienen que fallar limpias. Sin AmigaOS del usuario, lo dice y acaba. Acaba con 0 si nada se colgó.
*/
#include <exec/types.h>
#include <exec/execbase.h>
#include <exec/libraries.h>
#include <exec/io.h>
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

static const char *const libs[] = {
    "iffparse.library", "locale.library", "mathieeedoubbas.library", "mathieeedoubtrans.library",
    "mathieeesingtrans.library", "mathtrans.library", "diskfont.library", "commodities.library", "icon.library",
    "datatypes.library", "asl.library", "workbench.library", "rexxsyslib.library", "muimaster.library",
    "gadgets/texteditor.gadget", "images/bitmap.image",
};

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    if (!DOSBase) return 20;
    BPTR l = Lock((CONST_STRPTR)"AmigaOS:", SHARED_LOCK);
    if (!l) {
        di("  [caja-amiga] capa: sin AmigaOS del usuario\n", 0, 0, 0);
        CloseLibrary((struct Library *)DOSBase);
        return 0;
    }
    UnLock(l);
    LONG abren = 0;
    for (ULONG i = 0; i < sizeof(libs) / sizeof(libs[0]); i++) {
        struct Library *b = OpenLibrary((CONST_STRPTR)libs[i], 0);
        if (b) {
            di("  [caja-amiga] capa: %s abre, versión %ld.%ld\n", (LONG)libs[i], b->lib_Version, b->lib_Revision);
            abren++;
            CloseLibrary(b);
        } else {
            di("  [caja-amiga] capa: %s no abre\n", (LONG)libs[i], 0, 0);
        }
    }
    /* Las del hardware: limpias, a 0. */
    struct Library *ll = OpenLibrary((CONST_STRPTR)"lowlevel.library", 0);
    di("  [caja-amiga] capa: lowlevel.library (hardware) %s\n", (LONG)(ll ? "abre: MAL" : "no abre: bien"), 0, 0);
    if (ll) CloseLibrary(ll);
    struct IORequest io;
    struct MsgPort *mp = CreateMsgPort();
    if (mp) {
        for (ULONG k = 0; k < sizeof(io); k++) ((UBYTE *)&io)[k] = 0;
        io.io_Message.mn_ReplyPort = mp;
        io.io_Message.mn_Length = sizeof(io);
        LONG e = OpenDevice((CONST_STRPTR)"audio.device", 0, &io, 0);
        di("  [caja-amiga] capa: audio.device (hardware) %s\n", (LONG)(e ? "no abre: bien" : "abre: MAL"), 0, 0);
        if (!e) CloseDevice(&io);
        DeleteMsgPort(mp);
    }
    di("  [caja-amiga] capa: %ld de %ld abren del AmigaOS del usuario\n", abren, sizeof(libs) / sizeof(libs[0]), 0);
    CloseLibrary((struct Library *)DOSBase);
    return (ll) ? 10 : 0;
}
