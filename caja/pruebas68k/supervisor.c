/*
    La caja Amiga: Supervisor y CacheControl de exec. Es una prueba del oráculo: la misma salida en la caja y en un
    Amiga de verdad (FS-UAE con el Kickstart; tools/emu_run.py de Dash). Lo que sale no depende de la CPU: que la
    función corre en modo supervisor (el bit S de su SR), que Supervisor vuelve con lo que deja en D0 y el programa
    sigue, y que CacheControl cambia solo los bits de la máscara y devuelve los de antes.
*/
#include <exec/types.h>
#include <exec/execbase.h>
#include <dos/dos.h>
#include <proto/exec.h>
#include <proto/dos.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;

UWORD sr_dentro;
ULONG en_super(void);
/* La función de Supervisor: guarda su SR, deja 0x1234 en D0 y vuelve con RTE. */
__asm__(".text\n.even\n_en_super:\n move.w sr,_sr_dentro\n move.l #0x1234,d0\n rte\n");

static void escribe(const char *f, LONG a, LONG b, LONG c, LONG d)
{
    LONG v[4] = { a, b, c, d };
    VPrintf((CONST_STRPTR)f, v);
}

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 37);
    if (!DOSBase)
        return 20;
    for (int i = 0; i < 3; i++) {
        sr_dentro = 0;
        ULONG v = Supervisor((ULONG (*)())en_super);
        escribe("Supervisor %ld: D0 $%lx, S dentro %ld\n", i, v, (sr_dentro & 0x2000) != 0, 0);
    }
    ULONG c0 = CacheControl(0, 0);
    ULONG c1 = CacheControl(0, CACRF_EnableI);
    ULONG c2 = CacheControl(0, 0);
    ULONG c3 = CacheControl(c0, CACRF_EnableI);
    ULONG c4 = CacheControl(0, 0);
    escribe("CacheControl: igual %ld, sin EnableI %ld, otra vez igual %ld, y vuelve %ld\n", c1 == c0,
            (c2 & CACRF_EnableI) == 0 && (c2 & ~CACRF_EnableI) == (c0 & ~CACRF_EnableI), (c3 & ~CACRF_EnableI) == (c0 & ~CACRF_EnableI),
            c4 == c0);
    PutStr((CONST_STRPTR)"el programa sigue\n");
    return 0;
}
