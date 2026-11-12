/*
    DashOS, H4: una exec.library mínima y PROVISIONAL para la caja Amiga: lo justo para que un ejecutable de AmigaOS
    tome ExecBase de la dirección 4 y abra la dos.library. La exec de verdad (tareas, señales, puertos, memoria con
    dueño, bibliotecas) es el crate caja/exec de la rama caja-amiga-exec; cuando se enchufe, este fichero se va.

    - ExecBase en EXEC_BASE: la cabecera de biblioteca (exec.library, versión 40) y AttnFlags (68010 a 68040 y FPU:
      lo que emula Emu68). El resto de la estructura, a 0.
    - OpenLibrary (LVO -552): solo "dos.library" hasta la versión 40; lo demás, 0.
    - CloseLibrary (LVO -414): baja la cuenta de aperturas.
    Escrito de los autodocs de exec y de exec/execbase.h y exec/libraries.h del NDK (desplazamientos comprobados con
    m68k-amigaos-gcc: lib_Version 20, lib_OpenCnt 32, AttnFlags 296, sizeof(struct ExecBase) 632).
*/

#include "caja.h"

enum { N_OPENLIBRARY = N_EXEC, N_CLOSELIBRARY };

#define LIB_OPENCNT  32
#define ATTNFLAGS    296
#define EXECBASE_LEN 632
#define AFF_TODOS    0x3F   /* AFF_68010 | AFF_68020 | AFF_68030 | AFF_68040 | AFF_68881 | AFF_68882 */

uint32_t exec_minima_preparar(void)
{
    for (uintptr_t a = EXEC_BASE; a < EXEC_BASE + EXECBASE_LEN; a++) pon8(a, 0);
    pon_biblioteca(EXEC_BASE, "exec.library", EXEC_BASE + EXECBASE_LEN, 558, EXECBASE_LEN, 40);
    pon16(EXEC_BASE + ATTNFLAGS, AFF_TODOS);
    pon_salto(EXEC_BASE, 414, N_CLOSELIBRARY);
    pon_salto(EXEC_BASE, 552, N_OPENLIBRARY);
    return EXEC_BASE;
}

static int igual68k(uint32_t a, const char *s)
{
    long n = cadena68k(a);
    if (n < 0) return 0;
    for (long i = 0; i <= n; i++)
        if (*(const char *)(uintptr_t)(a + i) != s[i]) return 0;
    return 1;
}

int exec_minima_llamada(uint64_t *regs, uint32_t n)
{
    switch (n) {
        case N_OPENLIBRARY: {   /* OpenLibrary(nombre A1, versión D0): la base, o 0 */
            uint32_t base = 0;
            if (igual68k(lee_r(regs, R_A(1)), "dos.library") && lee_r(regs, R_D(0)) <= 40) {
                base = DOS_BASE;
                pon16(DOS_BASE + LIB_OPENCNT, lee16(DOS_BASE + LIB_OPENCNT) + 1);
            }
            pon_r(regs, R_D(0), base);
            return 1;
        }
        case N_CLOSELIBRARY:    /* CloseLibrary(base A1) */
            if (lee_r(regs, R_A(1)) == DOS_BASE && lee16(DOS_BASE + LIB_OPENCNT) > 0)
                pon16(DOS_BASE + LIB_OPENCNT, lee16(DOS_BASE + LIB_OPENCNT) - 1);
            return 1;
        default:
            return 0;
    }
}
