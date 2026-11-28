/*
    La caja Amiga en Linux (docs/CAJA_LINUX_LOOP.md, L4): los cajones de Linux vistos por la dos.library.

    Desde el cajón actual (el de Linux al lanzar la caja): su nombre (NameFromLock), lo que hay dentro (Examine y
    ExNext, sin orden: el script los ordena), y crear un cajón, un fichero dentro, renombrarlo y borrarlo todo. Escribe
    "MAL: ..." y devuelve 10 si algo falla; 0 si todo fue bien.
*/
#include <exec/types.h>
#include <exec/execbase.h>
#include <dos/dos.h>
#include <proto/exec.h>
#include <proto/dos.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;

static LONG fallos;

static void mal(const char *que)
{
    LONG a[1];
    a[0] = (LONG)que;
    VPrintf((CONST_STRPTR)"MAL: %s\n", a);
    fallos++;
}

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    if (!DOSBase)
        return 20;
    static char nombre[256];
    static struct FileInfoBlock fib __attribute__((aligned(4)));
    LONG a[3];

    BPTR aqui = Lock((CONST_STRPTR)"", SHARED_LOCK);
    if (!aqui) {
        mal("Lock del cajon actual");
        return 10;
    }
    if (NameFromLock(aqui, (STRPTR)nombre, sizeof nombre)) {
        a[0] = (LONG)nombre;
        VPrintf((CONST_STRPTR)"cajon actual: %s\n", a);
    } else
        mal("NameFromLock");
    if (Examine(aqui, &fib)) {
        LONG n = 0;
        while (ExNext(aqui, &fib)) {
            a[0] = (LONG)(fib.fib_DirEntryType > 0 ? "cajon  " : "fichero");
            a[1] = fib.fib_Size;
            a[2] = (LONG)fib.fib_FileName;
            VPrintf((CONST_STRPTR)"  %s %6ld %s\n", a);
            n++;
        }
        if (IoErr() != ERROR_NO_MORE_ENTRIES) mal("ExNext no acaba con ERROR_NO_MORE_ENTRIES");
        a[0] = n;
        VPrintf((CONST_STRPTR)"%ld entradas\n", a);
    } else
        mal("Examine");

    /* Crear, escribir, renombrar y borrar. */
    BPTR c = CreateDir((CONST_STRPTR)"cajon_nuevo");
    if (!c) mal("CreateDir");
    else UnLock(c);
    BPTR f = Open((CONST_STRPTR)"cajon_nuevo/dentro.txt", MODE_NEWFILE);
    if (!f) mal("Open MODE_NEWFILE");
    else {
        if (Write(f, (APTR)"desde el 68k\n", 13) != 13) mal("Write");
        Close(f);
    }
    if (!Rename((CONST_STRPTR)"cajon_nuevo/dentro.txt", (CONST_STRPTR)"cajon_nuevo/renombrado.txt")) mal("Rename");
    BPTR r = Lock((CONST_STRPTR)"cajon_nuevo/renombrado.txt", SHARED_LOCK);
    if (!r) mal("Lock del renombrado");
    else {
        if (!Examine(r, &fib) || fib.fib_Size != 13) mal("el renombrado no mide 13");
        UnLock(r);
    }
    if (DeleteFile((CONST_STRPTR)"cajon_nuevo")) mal("DeleteFile borra un cajon lleno");
    if (!DeleteFile((CONST_STRPTR)"cajon_nuevo/renombrado.txt")) mal("DeleteFile del fichero");
    if (!DeleteFile((CONST_STRPTR)"cajon_nuevo")) mal("DeleteFile del cajon vacio");
    if (Lock((CONST_STRPTR)"cajon_nuevo", SHARED_LOCK)) mal("el cajon sigue ahi");
    if (!fallos) PutStr((CONST_STRPTR)"crear, escribir, renombrar y borrar: bien\n");
    UnLock(aqui);
    CloseLibrary((struct Library *)DOSBase);
    return fallos ? 10 : 0;
}
