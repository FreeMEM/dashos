/*
    S28: un programa honrado que actualiza otro, como un instalador: reescribe Caja:C/viejo (un ejecutable de AmigaOS
    que ya existe) con la versión siguiente. La guardia de ficheros de la caja no lo deja sin preguntar al usuario
    (por `sesiones` y `guardian`); en la prueba, el guion contesta "solo esta vez" la primera vuelta y "siempre" la
    segunda, y la tercera ya lo recuerda `sesiones`.

    Caja:C/viejo es una cabecera hunk vacía (HUNK_HEADER sin hunks) de 32 bytes con la versión en los 4 últimos. Lo
    que escribe es lo mismo con la versión más uno: nada que se ejecute.

    Devuelve 0 si lo actualizó y 1 si no le dejaron (o no está). Textos en ASCII.
*/
#include <exec/types.h>
#include <exec/execbase.h>
#include <dos/dos.h>
#include <proto/exec.h>
#include <proto/dos.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;

static void decir(const char *f, LONG a, LONG b)
{
    LONG v[2] = { a, b };
    VPrintf((CONST_STRPTR)f, v);
}

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    if (!DOSBase)
        return 20;
    ULONG viejo[8] = { 0 };
    LONG r = 1;
    BPTR f = Open((CONST_STRPTR)"Caja:C/viejo", MODE_OLDFILE);
    if (f) {
        LONG n = Read(f, viejo, 32);
        Close(f);
        if (n != 32 || viejo[0] != 0x3F3) {
            decir("actualiza: Caja:C/viejo no es lo que esperaba (%ld bytes)\n", n, 0);
            viejo[0] = 0;
        }
    } else {
        decir("actualiza: Caja:C/viejo no esta (IoErr %ld)\n", IoErr(), 0);
    }
    if (viejo[0] == 0x3F3) {
        ULONG nuevo[8] = { 0x3F3, 0, 0, 0, 0, 0, 0, viejo[7] + 1 };
        f = Open((CONST_STRPTR)"Caja:C/viejo", MODE_NEWFILE);
        LONG w = f ? Write(f, nuevo, 32) : -1;
        LONG e = IoErr();
        if (f)
            Close(f);
        if (w == 32) {
            decir("actualiza: Caja:C/viejo de la version %ld a la %ld: hecho\n", (LONG)viejo[7], (LONG)nuevo[7]);
            r = 0;
        } else {
            decir("actualiza: Caja:C/viejo de la version %ld: no le dejan (IoErr %ld)\n", (LONG)viejo[7], e);
        }
    }
    CloseLibrary((struct Library *)DOSBase);
    return r;
}
