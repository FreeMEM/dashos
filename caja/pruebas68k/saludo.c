/*
    H4: el primer ejecutable hunk de verdad de la caja Amiga, compilado con m68k-amigaos-gcc (la cadena de Dash).

    Lo que haría un programa del Amiga sin biblioteca de arranque: toma ExecBase de la dirección 4, abre la dos.library
    con OpenLibrary y escribe con PutStr, VPrintf y Write(Output(), ...). Usa variables globales (hunk de datos y BSS),
    así que el cargador tiene que reubicar entre hunks. Devuelve 0 si todo fue bien.

    La misma salida en la caja y en un Amiga de verdad (FS-UAE con el Kickstart): es el oráculo. Los textos, en ASCII.
*/
#include <exec/types.h>
#include <exec/execbase.h>
#include <dos/dos.h>
#include <proto/exec.h>
#include <proto/dos.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;

static LONG suma_tabla(void);
static void criba(void);

/* La primera función del hunk de código: por aquí entra el programa (-nostartfiles, -fno-toplevel-reorder). */
LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    if (!DOSBase)
        return 20;
    PutStr((CONST_STRPTR)"  [caja-amiga] hunk: hola desde un ejecutable de AmigaOS (m68k-amigaos-gcc)\n");
    criba();
    LONG args[3];
    args[0] = suma_tabla();
    args[1] = (LONG)"dos.library";
    args[2] = DOSBase->dl_lib.lib_Version >= 36 ? 1 : 0;
    VPrintf((CONST_STRPTR)"  [caja-amiga] hunk: tabla %ld, %s abierta (v36 o mas: %ld)\n", args);
    static const char escrito[] = "  [caja-amiga] hunk: Write() en Output(): bien\n";
    LONG n = Write(Output(), (APTR)escrito, sizeof(escrito) - 1);
    CloseLibrary((struct Library *)DOSBase);
    return n == sizeof(escrito) - 1 ? 0 : 10;
}

/* Datos con punteros dentro (reubicados) y una tabla que se suma. */
static const char *const nombres[] = { "uno", "dos", "tres" };
static LONG tabla[8] = { 3, 1, 4, 1, 5, 9, 2, 6 };

static LONG suma_tabla(void)
{
    LONG s = 0;
    for (int i = 0; i < 8; i++)
        s = s * 10 + tabla[i];
    return s;
}

/* La criba de Eratóstenes en el BSS: los primos por debajo de 1000. */
static UBYTE compuesto[1000];

static void criba(void)
{
    LONG cuantos = 0, ultimo = 0;
    for (int i = 2; i < 1000; i++) {
        if (compuesto[i])
            continue;
        cuantos++;
        ultimo = i;
        for (int j = i * i; j < 1000; j += i)
            compuesto[j] = 1;
    }
    LONG args[3] = { cuantos, ultimo, (LONG)nombres[2] };
    VPrintf((CONST_STRPTR)"  [caja-amiga] hunk: %ld primos por debajo de 1000, el ultimo %ld (%s hunks)\n", args);
}
