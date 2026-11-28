/*
    La caja Amiga en Linux (docs/CAJA_LINUX_LOOP.md, L6): la consola. Sin argumentos, lee la entrada (Read de
    Input()) hasta el final y la escribe en mayúsculas (las letras de Latin-1 también) con el número de bytes leídos.
    Con "espera", espera Ctrl-C (SIGBREAKF_CTRL_C, CheckSignal) como mucho 10 s: escribe "***Break" y devuelve 5 si
    llegó, 20 si no.
*/
#include <exec/types.h>
#include <exec/execbase.h>
#include <dos/dos.h>
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
    if (largo >= 6 && args[0] == 'e' && args[1] == 's' && args[2] == 'p') {
        PutStr((CONST_STRPTR)"esperando Ctrl-C\n");
        Flush(Output());
        for (int i = 0; i < 500; i++) {
            if (CheckSignal(SIGBREAKF_CTRL_C)) {
                PutStr((CONST_STRPTR)"***Break\n");
                return 5;
            }
            Delay(1);
        }
        return 20;
    }
    static UBYTE buf[256];
    LONG total = 0, n;
    while ((n = Read(Input(), buf, sizeof buf)) > 0) {
        for (LONG i = 0; i < n; i++) {
            UBYTE c = buf[i];
            if ((c >= 'a' && c <= 'z') || (c >= 0xE0 && c <= 0xFE && c != 0xF7)) buf[i] = c - 32;
        }
        Write(Output(), buf, n);
        total += n;
    }
    LONG a[1];
    a[0] = total;
    VPrintf((CONST_STRPTR)"%ld bytes leidos\n", a);
    CloseLibrary((struct Library *)DOSBase);
    return n < 0 ? 10 : 0;
}
