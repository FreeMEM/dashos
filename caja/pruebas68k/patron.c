/*
    La caja Amiga: lo que deja ParsePattern (y ParsePatternNoCase) en el búfer, byte a byte, y lo que devuelve. Es una
    prueba del oráculo: la misma salida en la caja y en un Amiga de verdad (FS-UAE con el Kickstart; tools/emu_run.py
    de Dash). Los programas que arman el patrón a mano (Search) o usan el búfer como nombre (Rename) cuentan con él.
*/
#include <exec/types.h>
#include <exec/execbase.h>
#include <dos/dos.h>
#include <proto/exec.h>
#include <proto/dos.h>
struct ExecBase *SysBase;
struct DosLibrary *DOSBase;
static void escribe(const char *f, LONG a, LONG b, LONG c, LONG d) { LONG v[4] = { a, b, c, d }; VPrintf((CONST_STRPTR)f, v); }
static void vuelca(const char *pat, int nc)
{
    static UBYTE buf[128];
    for (int i = 0; i < 128; i++) buf[i] = 0xEE;
    LONG r = nc ? ParsePatternNoCase((CONST_STRPTR)pat, (STRPTR)buf, 100) : ParsePattern((CONST_STRPTR)pat, (STRPTR)buf, 100);
    escribe("%s [%s] %ld:", (LONG)(nc ? "NC" : "PP"), (LONG)pat, r, 0);
    for (int i = 0; i < 40; i++) {
        escribe(" %02lx", buf[i], 0, 0, 0);
        if (buf[i] == 0) break;
    }
    PutStr((CONST_STRPTR)"\n");
}
LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 37);
    const char *p[] = { "abc", "aBc", "a?c", "#?", "*", "a#?b", "(a|b)", "(ab|cd|e)", "~(x)", "~x", "[a-c]", "[~a-c]",
                        "[abc]", "#a", "#(ab)", "'?", "a'#b", "%", "a%b", "#?.info", "~(#?.info)", "Querido", "", 0 };
    for (int i = 0; p[i]; i++) { vuelca(p[i], 0); vuelca(p[i], 1); }
    return 0;
}
