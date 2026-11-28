/*
    La caja Amiga: ExtendFont y StripFont de graphics.library, de lo que pide la diskfont.library de AmigaOS 3.9 al cargar
    una fuente de disco. Es una prueba del oráculo: la misma salida en la caja y en un Amiga de verdad (FS-UAE con el
    Kickstart; tools/emu_run.py de Dash). Una TextFont hecha a mano (sin extensión), con y sin etiquetas, y topaz 8.
*/
#include <exec/types.h>
#include <exec/memory.h>
#include <dos/dos.h>
#include <graphics/text.h>
#include <utility/tagitem.h>
#include <proto/exec.h>
#include <proto/dos.h>
#include <proto/graphics.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;
struct GfxBase *GfxBase;

static void escribe(const char *f, LONG a, LONG b, LONG c, LONG d)
{
    LONG v[4] = { a, b, c, d };
    VPrintf((CONST_STRPTR)f, v);
}

static void mira(const char *que, struct TextFont *tf, struct MsgPort *puerto)
{
    struct TextFontExtension *e = (struct TextFontExtension *)tf->tf_Message.mn_ReplyPort;
    escribe("%s: estilo %lx, puerto cambiado %ld", (LONG)que, tf->tf_Style, (LONG)e != (LONG)puerto, 0);
    if ((LONG)e != (LONG)puerto && e) {
        escribe(", marca %lx banderas %lx/%lx", e->tfe_MatchWord, e->tfe_Flags0, e->tfe_Flags1, 0);
        escribe(", atras %ld, puerto de antes %ld, etiquetas %ld\n", e->tfe_BackPtr == tf,
                e->tfe_OrigReplyPort == puerto, e->tfe_Tags != NULL, 0);
        if (e->tfe_Tags)
            escribe("  etiqueta %lx = %lx\n", e->tfe_Tags[0].ti_Tag, e->tfe_Tags[0].ti_Data, 0, 0);
    } else {
        PutStr((CONST_STRPTR)"\n");
    }
}

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 37);
    GfxBase = (struct GfxBase *)OpenLibrary((CONST_STRPTR)"graphics.library", 37);
    if (!DOSBase || !GfxBase)
        return 20;
    /* Una TextFont a mano, como la arma diskfont al leer un fichero de fuente. */
    struct TextFont *tf = AllocMem(sizeof *tf, MEMF_PUBLIC | MEMF_CLEAR);
    struct MsgPort *puerto = AllocMem(sizeof(struct MsgPort), MEMF_PUBLIC | MEMF_CLEAR);
    if (!tf || !puerto)
        return 20;
    tf->tf_Message.mn_ReplyPort = puerto;
    tf->tf_YSize = 8;
    tf->tf_XSize = 8;
    tf->tf_Baseline = 6;
    tf->tf_LoChar = 32;
    tf->tf_HiChar = 126;
    mira("sin extender", tf, puerto);
    ULONG r = ExtendFont(tf, NULL);
    escribe("ExtendFont: %ld\n", r != 0, 0, 0, 0);
    mira("extendida", tf, puerto);
    struct TextFontExtension *e1 = (struct TextFontExtension *)tf->tf_Message.mn_ReplyPort;
    r = ExtendFont(tf, NULL);
    escribe("ExtendFont otra vez: %ld, la misma extension %ld\n", r != 0,
            (struct TextFontExtension *)tf->tf_Message.mn_ReplyPort == e1, 0, 0);
    StripFont(tf);
    mira("StripFont", tf, puerto);
    /* Con etiquetas. */
    struct TagItem tags[] = { { TA_DeviceDPI, (75 << 16) | 75 }, { TAG_DONE, 0 } };
    r = ExtendFont(tf, tags);
    escribe("ExtendFont con etiquetas: %ld\n", r != 0, 0, 0, 0);
    mira("con etiquetas", tf, puerto);
    StripFont(tf);
    mira("StripFont otra vez", tf, puerto);
    StripFont(tf);
    mira("StripFont de una sin extension", tf, puerto);
    /* topaz 8, la de la ROM. */
    struct TextAttr ta = { (STRPTR)"topaz.font", 8, 0, 0 };
    struct TextFont *tp = OpenFont(&ta);
    if (tp) {
        struct MsgPort *antes = tp->tf_Message.mn_ReplyPort;
        r = ExtendFont(tp, NULL);
        escribe("topaz: ExtendFont %ld, puerto cambiado %ld\n", r != 0, tp->tf_Message.mn_ReplyPort != antes, 0, 0);
        CloseFont(tp);
    }
    FreeMem(puerto, sizeof(struct MsgPort));
    FreeMem(tf, sizeof *tf);
    return 0;
}
