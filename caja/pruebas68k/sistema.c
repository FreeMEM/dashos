/*
    La caja Amiga: SystemTagList, Execute y RunCommand de dos.library (lanzar una orden; los pide Forja, y los usan los
    programas que llaman a otros). Es una prueba del oráculo: la misma salida en la caja y en un Amiga de verdad
    (FS-UAE con el Kickstart; tools/emu_run.py de Dash, con --file eco.exe). Lanza eco.exe, que escribe sus
    argumentos y acaba con 7, y una orden que no existe.
*/
#include <exec/types.h>
#include <dos/dos.h>
#include <dos/dostags.h>
#include <utility/tagitem.h>
#include <proto/exec.h>
#include <proto/dos.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;

static void escribe(const char *f, LONG a, LONG b, LONG c, LONG d)
{
    LONG v[4] = { a, b, c, d };
    VPrintf((CONST_STRPTR)f, v);
    Flush(Output());
}

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 37);
    if (!DOSBase)
        return 20;
    STRPTR mio = GetArgStr();
    escribe("antes: mis argumentos [%s]\n", (LONG)mio, 0, 0, 0);
    LONG r = SystemTagList((CONST_STRPTR)"eco.exe uno \"dos tres\" cuatro", NULL);
    escribe("SystemTagList: %ld\n", r, 0, 0, 0);
    escribe("despues: mis argumentos [%s]\n", (LONG)GetArgStr(), 0, 0, 0);
    r = SystemTagList((CONST_STRPTR)"eco.exe", NULL);
    escribe("SystemTagList sin argumentos: %ld\n", r, 0, 0, 0);
    SetIoErr(0);
    r = SystemTagList((CONST_STRPTR)"noexiste.exe a b", NULL);
    escribe("SystemTagList de una que no existe: %ld\n", r, 0, 0, 0);
    r = Execute((CONST_STRPTR)"eco.exe por Execute", 0, Output());
    escribe("Execute: %ld\n", r, 0, 0, 0);
    BPTR seg = LoadSeg((CONST_STRPTR)"eco.exe");
    if (seg) {
        static char args[] = "con RunCommand\n";
        r = RunCommand(seg, 8192, (STRPTR)args, sizeof args - 1);
        escribe("RunCommand: %ld\n", r, 0, 0, 0);
        UnLoadSeg(seg);
    }
    escribe("al final: mis argumentos [%s]\n", (LONG)GetArgStr(), 0, 0, 0);
    return 0;
}
