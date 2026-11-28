/*
    La caja Amiga: un programa que escribe lo que recibe (GetArgStr y ReadArgs) y acaba con 7. Lo lanza sistema.c con
    SystemTagList, Execute y RunCommand.
*/
#include <exec/types.h>
#include <dos/dos.h>
#include <dos/rdargs.h>
#include <proto/exec.h>
#include <proto/dos.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 37);
    if (!DOSBase)
        return 20;
    STRPTR a = GetArgStr();
    LONG n = 0;
    while (a && a[n] && a[n] != '\n') n++;
    static char copia[128];
    LONG k;
    for (k = 0; k < n && k < 127; k++) copia[k] = a[k];
    copia[k] = 0;
    LONG v[2] = { n, (LONG)copia };
    VPrintf((CONST_STRPTR)"  eco: GetArgStr %ld [%s]\n", v);
    LONG arr[1] = { 0 };
    struct RDArgs *rd = ReadArgs((CONST_STRPTR)"PALABRAS/M", arr, NULL);
    if (rd) {
        STRPTR *p = (STRPTR *)arr[0];
        PutStr((CONST_STRPTR)"  eco: ReadArgs");
        while (p && *p) {
            LONG w[1] = { (LONG)*p++ };
            VPrintf((CONST_STRPTR)" [%s]", w);
        }
        PutStr((CONST_STRPTR)"\n");
        FreeArgs(rd);
    }
    Flush(Output());
    return 7;
}
