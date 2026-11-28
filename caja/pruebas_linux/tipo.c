/*
    La caja Amiga en Linux (docs/CAJA_LINUX_LOOP.md, L6): los argumentos. Escribe la línea de argumentos tal como le
    llega ("args: [...]") y, como Type, el fichero que dice ReadArgs("FICHERO/A"). Devuelve 0, 10 si no lo abre o 20
    si los argumentos no valen.
*/
#include <exec/types.h>
#include <exec/execbase.h>
#include <dos/dos.h>
#include <dos/rdargs.h>
#include <proto/exec.h>
#include <proto/dos.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;

LONG inicio(register STRPTR args __asm("a0"), register LONG largo __asm("d0"))
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    if (!DOSBase)
        return 20;
    static UBYTE linea[256];
    LONG n = largo > 0 ? largo - 1 : 0;   /* sin el "\n" del final */
    if (n > 255) n = 255;
    for (LONG i = 0; i < n; i++) linea[i] = args[i];
    linea[n] = 0;
    LONG a[1];
    a[0] = (LONG)linea;
    VPrintf((CONST_STRPTR)"args: [%s]\n", a);
    LONG op[1] = { 0 };
    struct RDArgs *rd = ReadArgs((CONST_STRPTR)"FICHERO/A", op, NULL);
    if (!rd) {
        PrintFault(IoErr(), (CONST_STRPTR)"tipo");
        return 20;
    }
    a[0] = op[0];
    VPrintf((CONST_STRPTR)"fichero: %s\n", a);
    BPTR f = Open((CONST_STRPTR)op[0], MODE_OLDFILE);
    LONG r = 10;
    if (f) {
        static UBYTE buf[256];
        LONG n;
        while ((n = Read(f, buf, sizeof buf)) > 0) Write(Output(), buf, n);
        Close(f);
        r = 0;
    } else
        PrintFault(IoErr(), (CONST_STRPTR)"tipo");
    FreeArgs(rd);
    CloseLibrary((struct Library *)DOSBase);
    return r;
}
