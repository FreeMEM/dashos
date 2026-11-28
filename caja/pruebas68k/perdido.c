/*
    La caja Amiga, prueba hostil: el programa sale de su memoria. Con "salta", salta a $20000010; con "lee", lee de
    $30000000; con "escribe", escribe en $30000000; con "pila", deja A7 en $30000000 y llama a una función (el JSR
    escribe ahí la vuelta); con "cero", escribe en la dirección 4 (ExecBase), en la página 0 de solo lectura. Fuera de la memoria del 68k no hay nada: el programa cae (como en DashOS: "cae el
    programa") y la caja sigue con el siguiente. Si llega al final, escribe "MAL: sigue" y devuelve 10.
*/
#include <exec/types.h>
#include <exec/execbase.h>
#include <dos/dos.h>
#include <proto/exec.h>
#include <proto/dos.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;

static void nada(void) {}

LONG inicio(register STRPTR args __asm("a0"), register LONG largo __asm("d0"))
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    if (!DOSBase || largo < 2)
        return 20;
    PutStr((CONST_STRPTR)"perdido: sale de su memoria\n");
    Flush(Output());
    volatile ULONG *lejos = (volatile ULONG *)0x30000000UL;
    switch (args[0]) {
    case 's':
        ((void (*)(void))0x20000010UL)();
        break;
    case 'l': {
        ULONG v = *lejos;
        (void)v;
        break;
    }
    case 'e':
        *lejos = 0x12345678;
        break;
    case 'p':
        __asm__ volatile("move.l a7,a2\n move.l #0x30000000,a7\n jsr (%0)\n move.l a2,a7" : : "a"(nada) : "a2", "memory");
        break;
    case 'c':
        *(volatile ULONG *)4 = 0;
        break;
    default:
        return 20;
    }
    PutStr((CONST_STRPTR)"MAL: sigue\n");
    return 10;
}
