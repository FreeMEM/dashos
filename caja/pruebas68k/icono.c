/*
    La caja Amiga: icon.library. Es una prueba del oráculo: la misma salida en la caja y en un Amiga de verdad (FS-UAE
    con el Kickstart; tools/emu_run.py de Dash). FindToolType, MatchToolValue y BumpRevision; un icono por defecto
    (GetDefDiskObject), cambiado, escrito (PutDiskObject), leído otra vez (GetDiskObject) y borrado
    (DeleteDiskObject); y FreeFreeList con AddFreeList.
*/
#include <exec/types.h>
#include <exec/execbase.h>
#include <exec/memory.h>
#include <dos/dos.h>
#include <workbench/workbench.h>
#include <workbench/icon.h>
#include <proto/exec.h>
#include <proto/dos.h>
#include <proto/icon.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;
struct Library *IconBase;

static void escribe(const char *f, LONG a, LONG b, LONG c, LONG d)
{
    LONG v[4] = { a, b, c, d };
    VPrintf((CONST_STRPTR)f, v);
}

static const char *o(const char *s) { return s ? s : "(nulo)"; }

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 37);
    IconBase = OpenLibrary((CONST_STRPTR)"icon.library", 37);
    if (!DOSBase)
        return 20;
    if (!IconBase) {
        PutStr((CONST_STRPTR)"sin icon.library\n");
        return 20;
    }
    escribe("icon.library %ld\n", IconBase->lib_Version >= 40, 0, 0, 0);

    /* FindToolType: el valor tras "=", "" si no lleva, nulo si no está; sin distinguir mayúsculas. */
    static STRPTR tt[] = { (STRPTR)"DONOTWAIT", (STRPTR)"Pubscreen=Workbench", (STRPTR)"(COMENTADO=1)", (STRPTR)"FILETYPE=text|ascii",
                           (STRPTR)"VACIO=", (STRPTR)"donotwaitmas=2", NULL };
    const char *nombres[] = { "DONOTWAIT", "PUBSCREEN", "pubscreen", "COMENTADO", "(COMENTADO", "FILETYPE", "VACIO", "NOESTA",
                              "DONOT", 0 };
    for (int i = 0; nombres[i]; i++) {
        STRPTR v = FindToolType((CONST_STRPTR *)tt, (CONST_STRPTR)nombres[i]);
        escribe("FindToolType %s: [%s]\n", (LONG)nombres[i], (LONG)o((const char *)v), 0, 0);
    }
    const char *valores[] = { "text", "TEXT", "ascii", "tex", "bin", "text|ascii", "", 0 };
    for (int i = 0; valores[i]; i++)
        escribe("MatchToolValue [text|ascii] %s: %ld\n", (LONG)valores[i],
                MatchToolValue((CONST_STRPTR) "text|ascii", (CONST_STRPTR)valores[i]) != 0, 0, 0);
    escribe("MatchToolValue [text] text: %ld\n", MatchToolValue((CONST_STRPTR) "text", (CONST_STRPTR) "text") != 0, 0, 0, 0);

    /* BumpRevision. */
    const char *viejos[] = { "Fichero", "copy of Fichero", "copy 2 of Fichero", "copy 9 of F", "copy of", "copy x of F",
                             "Un nombre muy muy largo de treinta", 0 };
    static char nuevo[64];
    for (int i = 0; viejos[i]; i++) {
        for (int k = 0; k < 64; k++) nuevo[k] = 0;
        BumpRevision((STRPTR)nuevo, (CONST_STRPTR)viejos[i]);
        escribe("BumpRevision [%s]: [%s]\n", (LONG)viejos[i], (LONG)nuevo, 0, 0);
    }

    /* Un icono de herramienta por defecto, cambiado, escrito y leído otra vez. */
    struct DiskObject *d = GetDefDiskObject(WBTOOL);
    if (!d) {
        PutStr((CONST_STRPTR)"sin icono por defecto\n");
        return 20;
    }
    escribe("por defecto: magic $%lx version %ld tipo %ld pila %ld\n", d->do_Magic, d->do_Version, d->do_Type, d->do_StackSize);
    escribe("  gadget %ldx%ld render %ld select %ld\n", d->do_Gadget.Width, d->do_Gadget.Height, d->do_Gadget.GadgetRender != 0,
            d->do_Gadget.SelectRender != 0);
    STRPTR *viejas = d->do_ToolTypes;
    STRPTR viejo_tool = d->do_DefaultTool;
    LONG vx = d->do_CurrentX, vy = d->do_CurrentY, vs = d->do_StackSize;
    static STRPTR nuevas[] = { (STRPTR)"UNO=1", (STRPTR)"DOS", NULL };
    d->do_ToolTypes = nuevas;
    d->do_DefaultTool = (STRPTR)"SYS:Herramienta";
    d->do_CurrentX = 33;
    d->do_CurrentY = 44;
    d->do_StackSize = 12345;
    escribe("PutDiskObject: %ld\n", PutDiskObject((CONST_STRPTR) "prueba_icono", d) != 0, 0, 0, 0);
    d->do_ToolTypes = viejas;
    d->do_DefaultTool = viejo_tool;
    d->do_CurrentX = vx;
    d->do_CurrentY = vy;
    d->do_StackSize = vs;
    FreeDiskObject(d);
    BPTR l = Lock((CONST_STRPTR) "prueba_icono.info", SHARED_LOCK);
    escribe("prueba_icono.info existe: %ld\n", l != 0, 0, 0, 0);
    if (l) UnLock(l);
    d = GetDiskObject((CONST_STRPTR) "prueba_icono");
    if (d) {
        escribe("leido: magic $%lx version %ld tipo %ld pila %ld\n", d->do_Magic, d->do_Version, d->do_Type, d->do_StackSize);
        escribe("  x %ld y %ld herramienta [%s]\n", d->do_CurrentX, d->do_CurrentY, (LONG)o((const char *)d->do_DefaultTool), 0);
        for (int i = 0; d->do_ToolTypes && d->do_ToolTypes[i] && i < 10; i++)
            escribe("  tooltype %ld: [%s]\n", i, (LONG)d->do_ToolTypes[i], 0, 0);
        escribe("  FindToolType UNO: [%s]\n", (LONG)o((const char *)FindToolType((CONST_STRPTR *)d->do_ToolTypes, (CONST_STRPTR) "UNO")), 0, 0, 0);
        escribe("  gadget %ldx%ld render %ld\n", d->do_Gadget.Width, d->do_Gadget.Height, d->do_Gadget.GadgetRender != 0, 0);
        FreeDiskObject(d);
    } else
        escribe("GetDiskObject: nulo, IoErr %ld\n", IoErr(), 0, 0, 0);
    escribe("DeleteDiskObject: %ld\n", DeleteDiskObject((CONST_STRPTR) "prueba_icono") != 0, 0, 0, 0);
    d = GetDiskObject((CONST_STRPTR) "prueba_icono");
    escribe("GetDiskObject tras borrar: %ld, IoErr %ld\n", d != 0, IoErr(), 0, 0);
    if (d) FreeDiskObject(d);
    d = GetDiskObjectNew((CONST_STRPTR) "no_existe");
    escribe("GetDiskObjectNew de lo que no existe: %ld tipo %ld\n", d != 0, d ? d->do_Type : -1, 0, 0);
    if (d) FreeDiskObject(d);

    /* FreeList. */
    struct FreeList fl;
    struct List *lista = (struct List *)&fl.fl_MemList;   /* NewList, que es de amiga.lib */
    lista->lh_Head = (struct Node *)&lista->lh_Tail;
    lista->lh_Tail = NULL;
    lista->lh_TailPred = (struct Node *)&lista->lh_Head;
    fl.fl_NumFree = 0;
    ULONG antes = AvailMem(MEMF_ANY);
    for (int i = 0; i < 5; i++) {
        APTR m = AllocMem(1000, MEMF_ANY);
        AddFreeList(&fl, m, 1000);
    }
    FreeFreeList(&fl);
    escribe("FreeFreeList: vuelve la memoria %ld\n", AvailMem(MEMF_ANY) >= antes, 0, 0, 0);
    CloseLibrary(IconBase);
    return 0;
}
