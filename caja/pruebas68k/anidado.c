/*
    H4, prueba de las llamadas anidadas de la caja Amiga (código 68k que la caja ejecuta desde una función nativa):
    una biblioteca 68k hecha con MakeLibrary (exec llama a su Open, que es 68k), una excepción de tarea (SetExcept y
    tc_ExceptCode), un gancho (CallHookPkt) y un tc_TrapCode propio. El modo sale de la última letra del nombre del
    proceso (la caja lo lanza como "anidado-a", "anidado-b"...):

      a  normal, y hostiles de las que el programa sale vivo: Open que se llama a sí mismo sin fin (la caja corta en su
         tope de niveles y da 0), Open que vuelve con la pila más abajo de lo debido; la excepción de tarea dos veces;
         un tc_TrapCode que recupera una instrucción ilegal. Devuelve 0 si todo fue como debía.
      b  un tc_ExceptCode que vuelve con la pila más arriba (ha deshecho marcos de fuera): la caja corta el programa.
      c  una instrucción ilegal dentro de tc_ExceptCode, sin tc_TrapCode propio: el programa acaba por el vector 4
         dentro de una llamada anidada, y la caja deshace los niveles.
      d  excepción dentro de excepción: un tc_TrapCode que provoca otra, sin fin; la pila de supervisor se acaba y el
         programa acaba por el vector 4.
*/
#include <exec/types.h>
#include <exec/execbase.h>
#include <exec/libraries.h>
#include <exec/tasks.h>
#include <utility/hooks.h>
#include <proto/exec.h>
#include <proto/dos.h>
#include <proto/utility.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;
struct Library *UtilityBase;

static volatile LONG modo, nivel, hondo, abiertas, excepciones, recuperado;
static struct Library *milib;
static const char nombre[] = "anidado.library";

static void di(const char *f, LONG a, LONG b)
{
    LONG v[2] = { a, b };
    VPrintf((CONST_STRPTR)f, v);
}

/* Las funciones de la biblioteca: 68k con los registros de exec (A6 = la biblioteca). Open llama a una de C según el
   modo; las que no vuelven normal van en ensamblador. */
LONG abrir_c(void) __attribute__((used));
LONG abrir_c(void)
{
    if (modo == 1) {
        /* Se abre a sí misma otra vez: exec vuelve a llamar a Open, anidado, hasta el tope de la caja. */
        nivel++;
        if (nivel > hondo) hondo = nivel;
        struct Library *l = OpenLibrary((CONST_STRPTR)nombre, 0);
        if (l) abiertas++;
        nivel--;
    }
    milib->lib_OpenCnt++;
    return (LONG)milib;
}

void lib_abrir(void);
void lib_cerrar(void);
void lib_nada(void);
__asm__(
"       .text\n"
"       .even\n"
"_lib_abrir:\n"
"       movem.l d1/a0-a1,-(sp)\n"
"       cmp.l   #2,_modo\n"
"       beq.s   2f\n"
"       jsr     _abrir_c\n"
"       movem.l (sp)+,d1/a0-a1\n"
"       rts\n"
/* 2: vuelve con la pila 4 bytes más abajo (repite su dirección de vuelta) */
"2:     movem.l (sp)+,d1/a0-a1\n"
"       addq.w  #1,32(a6)\n"
"       move.l  a6,d0\n"
"       move.l  (sp),-(sp)\n"
"       rts\n"
"_lib_cerrar:\n"
"       subq.w  #1,32(a6)\n"
"       moveq   #0,d0\n"
"       rts\n"
"_lib_nada:\n"
"       moveq   #0,d0\n"
"       rts\n"
);

/* tc_ExceptCode: D0 = las señales, A1 = tc_ExceptData. Cuenta y devuelve las mismas (siguen habilitadas). */
void excepcion(void);
__asm__(
"       .text\n"
"       .even\n"
"_excepcion:\n"
"       addq.l  #1,_excepciones\n"
"       move.l  a1,_recuperado\n"
"       rts\n"
/* b: vuelve con la pila 8 bytes más arriba */
"_excepcion_b:\n"
"       move.l  (sp)+,a0\n"
"       addq.l  #8,sp\n"
"       jmp     (a0)\n"
/* c: una instrucción ilegal dentro */
"_excepcion_c:\n"
"       illegal\n"
"       rts\n"
);
extern void excepcion_b(void), excepcion_c(void);

/* tc_TrapCode que recupera: [n.l][marco 68010+]. Cambia la PC del marco a `tras_ilegal` y vuelve con RTE. */
void trampa_buena(void);
void tras_ilegal(void);
__asm__(
"       .text\n"
"       .even\n"
"_trampa_buena:\n"
"       addq.l  #4,sp\n"
"       move.l  #_tras_ilegal,2(sp)\n"
"       rte\n"
/* tc_TrapCode que provoca otra excepción: sin fin */
"_trampa_mala:\n"
"       illegal\n"
"       rte\n"
);
extern void trampa_mala(void);

/* El gancho: A0 = el gancho, A2 = el objeto, A1 = el mensaje. Devuelve objeto + mensaje + h_Data. */
ULONG gancho(void);
__asm__(
"       .text\n"
"       .even\n"
"_gancho:\n"
"       move.l  a2,d0\n"
"       add.l   a1,d0\n"
"       add.l   16(a0),d0\n"
"       rts\n"
);

static volatile LONG paso_ilegal;

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    if (!DOSBase) return 20;
    struct Task *yo = FindTask(NULL);
    const char *n = yo->tc_Node.ln_Name;
    while (n[1]) n++;
    char m = *n;
    LONG mal = 0;

    if (m == 'a') {
        static APTR vectores[] = { (APTR)lib_abrir, (APTR)lib_cerrar, (APTR)lib_nada, (APTR)lib_nada, (APTR)-1 };
        milib = MakeLibrary(vectores, NULL, NULL, sizeof(struct Library), 0);
        if (!milib) return 22;
        milib->lib_Node.ln_Name = (char *)nombre;
        milib->lib_Node.ln_Type = NT_LIBRARY;
        AddLibrary(milib);

        /* 1. Open normal: exec llama al 68k de la biblioteca. */
        modo = 0;
        struct Library *l = OpenLibrary((CONST_STRPTR)nombre, 0);
        di("  [caja-amiga] anidado: Open 68k desde exec: %s (abierta %ld vez)\n", (LONG)(l == milib ? "bien" : "MAL"),
           milib->lib_OpenCnt);
        if (l != milib || milib->lib_OpenCnt != 1) mal++;
        if (l) CloseLibrary(l);

        /* 2. Hostil: Open que se abre a sí misma sin fin. */
        modo = 1;
        nivel = hondo = abiertas = 0;
        l = OpenLibrary((CONST_STRPTR)nombre, 0);
        di("  [caja-amiga] anidado: Open que se abre a si misma: %ld niveles, %ld abiertas dentro\n", hondo, abiertas);
        if (!l || hondo < 8 || hondo > 32 || abiertas != hondo - 1) mal++;
        for (LONG i = 0; i < abiertas; i++) CloseLibrary(milib);
        if (l) CloseLibrary(l);

        /* 3. Hostil: vuelve con la pila más abajo. La caja lo acepta (la de fuera está intacta) y lo dice. */
        modo = 2;
        l = OpenLibrary((CONST_STRPTR)nombre, 0);
        di("  [caja-amiga] anidado: Open que vuelve con la pila mas abajo: %s\n", (LONG)(l == milib ? "sigue" : "MAL"), 0);
        if (l != milib) mal++;
        if (l) CloseLibrary(l);

        /* 4. Una excepción de tarea, dos veces: tc_ExceptCode corre anidado y en modo de usuario. */
        BYTE s = AllocSignal(-1);
        yo->tc_ExceptCode = (APTR)excepcion;
        yo->tc_ExceptData = (APTR)0x1234;
        SetExcept(1UL << s, 1UL << s);
        Signal(yo, 1UL << s);
        Signal(yo, 1UL << s);
        ULONG quedan = SetSignal(0, 0) & (1UL << s);
        di("  [caja-amiga] anidado: tc_ExceptCode %ld veces, A1 = $%lx\n", excepciones, recuperado);
        if (excepciones != 2 || recuperado != 0x1234 || quedan) mal++;
        SetExcept(0, 1UL << s);
        FreeSignal(s);
        recuperado = 0;

        /* 5. Un gancho por CallHookPkt (su entrada es 68k: salta a h_Entry). */
        UtilityBase = OpenLibrary((CONST_STRPTR)"utility.library", 37);
        struct Hook h = { { 0, 0 }, (HOOKFUNC)(APTR)gancho, 0, (APTR)1000 };
        ULONG g = UtilityBase ? CallHookPkt(&h, (APTR)20, (APTR)300) : 0;
        di("  [caja-amiga] anidado: CallHookPkt da %ld (esperado 1320)\n", g, 0);
        if (g != 1320) mal++;
        if (UtilityBase) CloseLibrary(UtilityBase);

        /* 6. Un tc_TrapCode que recupera una instrucción ilegal: sigue después. */
        APTR antes = yo->tc_TrapCode;
        yo->tc_TrapCode = (APTR)trampa_buena;
        __asm__ volatile("illegal\n"
                         "_tras_ilegal:\n"
                         "addq.l #1,_paso_ilegal" ::: "memory");
        yo->tc_TrapCode = antes;
        di("  [caja-amiga] anidado: tc_TrapCode propio recupera la instruccion ilegal: %s\n",
           (LONG)(paso_ilegal == 1 ? "bien" : "MAL"), 0);
        if (paso_ilegal != 1) mal++;

        /* La biblioteca fuera de la lista del sistema y su memoria devuelta (es de la tarea, pero la lista no). */
        Forbid();
        Remove(&milib->lib_Node);
        Permit();
        FreeMem((UBYTE *)milib - milib->lib_NegSize, milib->lib_NegSize + milib->lib_PosSize);
    } else if (m == 'b' || m == 'c') {
        BYTE s = AllocSignal(-1);
        yo->tc_ExceptCode = (APTR)(m == 'b' ? excepcion_b : excepcion_c);
        SetExcept(1UL << s, 1UL << s);
        Signal(yo, 1UL << s);
        di("  [caja-amiga] anidado: MAL: siguio tras su tc_ExceptCode (modo %ld)\n", m, 0);
        mal++;
    } else if (m == 'd') {
        yo->tc_TrapCode = (APTR)trampa_mala;
        __asm__ volatile("illegal");
        di("  [caja-amiga] anidado: MAL: siguio tras la excepcion dentro de la excepcion\n", 0, 0);
        mal++;
    }
    CloseLibrary((struct Library *)DOSBase);
    return mal;
}
