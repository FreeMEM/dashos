/*
    La caja Amiga: RawDoFmt y VPrintf con %lx (en AmigaOS, en mayúsculas), %04lx, %08lx y %ld. Es una prueba del
    oráculo: la misma salida en la caja y en un Amiga de verdad (FS-UAE con el Kickstart; tools/emu_run.py de Dash).
*/
#include <exec/types.h>
#include <exec/execbase.h>
#include <dos/dos.h>
#include <proto/exec.h>
#include <proto/dos.h>
struct ExecBase *SysBase;
struct DosLibrary *DOSBase;
static void pon(register UBYTE c __asm("d0"), register UBYTE **p __asm("a3")) { *(*p)++ = c; }
LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 37);
    static UBYTE buf[200];
    UBYTE *p = buf;
    LONG v[6] = { 0x2a, 0xbeef, 0xc0de, 255, -1, 0x1234abcd };
    RawDoFmt((CONST_STRPTR)"raw [%lx] [%04lx] [%lX] [%08lx] [%lx] [%ld]\n", v, (void (*)())pon, &p);
    PutStr((CONST_STRPTR)buf);
    VPrintf((CONST_STRPTR)"vpr [%lx] [%04lx] [%lX] [%08lx] [%lx] [%ld]\n", v);
    return 0;
}
