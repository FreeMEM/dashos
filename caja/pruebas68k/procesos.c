/*
    Varias tareas 68k a la vez en la caja Amiga: tareas de exec (AddTask, como CreateTask de amiga.lib, con su
    estructura y su pila en tc_MemEntry), Wait y Signal entre ellas, puertos de mensajes, prioridades, Forbid/Permit,
    Delay, el turno, RemTask y el fin de una tarea. El modo sale de la última letra del nombre del proceso (la caja lo
    lanza como "procesos-a", "procesos-b"...):

      a  lo normal: acaba con 0 si todo fue como debía, sin avisos del Enforcer. También procesos de dos
         (CreateNewProc con NP_Entry y con NP_Seglist, CreateProc).
      b  hostil, y sale vivo: CreateNewProc con NULL y con etiquetas absurdas, miles de tareas hasta el tope, tareas que no acaban (la caja las quita al acabar el
         programa), un puerto borrado con mensajes dentro, una tarea que quita a otra que espera, una que se señala a
         sí misma sin fin, AddTask con basura. Acaba con 0; los avisos del Enforcer se esperan.
      c  una tarea quita la del programa (RemTask): el programa acaba ahí (la caja lo corta) y su tarea con él.
      d  tareas y un proceso que caen (instrucción ilegal, división por cero, sin tc_TrapCode): la caja los quita y el
         programa sigue; acaba con 0.
*/
#include <exec/types.h>
#include <exec/execbase.h>
#include <exec/memory.h>
#include <exec/tasks.h>
#include <exec/ports.h>
#include <dos/dos.h>
#include <dos/dosextens.h>
#include <dos/dostags.h>
#include <utility/tagitem.h>
#include <proto/exec.h>
#include <proto/dos.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;

static struct Task *principal;
static BYTE sig_p = -1;
static volatile LONG modo, fallos;

static void di(const char *f, LONG a, LONG b, LONG c)
{
    LONG v[3] = { a, b, c };
    VPrintf((CONST_STRPTR)f, v);
}

static void mira(int bien, const char *que)
{
    if (!bien) fallos++;
    di("  [caja-amiga] procesos: %s: %s\n", (LONG)que, (LONG)(bien ? "bien" : "MAL"), 0);
}

/* Como CreateTask de amiga.lib: la estructura y la pila en un MemList de tc_MemEntry, así RemTask (o el fin de la
   tarea) las suelta. */
struct ListaTarea {
    struct Node ml_Node;
    UWORD ml_NumEntries;
    struct MemEntry ml_ME[2];
};

static struct Task *crear(const char *nombre, LONG pri, void (*entrada)(void), ULONG pila)
{
    struct Task *t = AllocMem(sizeof(struct Task), MEMF_PUBLIC | MEMF_CLEAR);
    if (!t) return 0;
    UBYTE *p = AllocMem(pila, MEMF_CLEAR);
    struct ListaTarea *ml = AllocMem(sizeof(struct ListaTarea), MEMF_PUBLIC | MEMF_CLEAR);
    if (!p || !ml) {
        if (p) FreeMem(p, pila);
        if (ml) FreeMem(ml, sizeof(struct ListaTarea));
        FreeMem(t, sizeof(struct Task));
        return 0;
    }
    t->tc_Node.ln_Type = NT_TASK;
    t->tc_Node.ln_Pri = pri;
    t->tc_Node.ln_Name = (char *)nombre;
    t->tc_SPLower = p;
    t->tc_SPUpper = p + pila;
    t->tc_SPReg = p + pila;
    ml->ml_NumEntries = 2;
    ml->ml_ME[0].me_Addr = t;
    ml->ml_ME[0].me_Length = sizeof(struct Task);
    ml->ml_ME[1].me_Addr = p;
    ml->ml_ME[1].me_Length = pila;
    t->tc_MemEntry.lh_Head = (struct Node *)&t->tc_MemEntry.lh_Tail;
    t->tc_MemEntry.lh_Tail = 0;
    t->tc_MemEntry.lh_TailPred = (struct Node *)&t->tc_MemEntry.lh_Head;
    t->tc_MemEntry.lh_Type = NT_MEMORY;
    AddHead(&t->tc_MemEntry, &ml->ml_Node);
    if (!AddTask(t, (APTR)entrada, 0)) return 0;
    return t;
}

static int sigue_viva(const char *nombre)
{
    Forbid();
    struct Task *t = FindTask((CONST_STRPTR)nombre);
    Permit();
    return t != 0;
}

/* --- a: lo normal --- */

static struct Task *hija;
static volatile BYTE sig_h = -1;
static volatile LONG cuenta, bandera, vueltas, parar, suma;
#define IDAS 1000

static void hija_ping(void)
{
    sig_h = AllocSignal(-1);
    Signal(principal, 1L << sig_p);
    for (int i = 0; i < IDAS; i++) {
        Wait(1L << sig_h);
        cuenta++;
        Signal(principal, 1L << sig_p);
    }
}

struct MiMensaje {
    struct Message m;
    LONG valor;
};

static void hija_puerto(void)
{
    struct MsgPort *p = CreateMsgPort();
    if (!p) { Signal(principal, 1L << sig_p); return; }
    p->mp_Node.ln_Name = (char *)"procesos.puerto";
    p->mp_Node.ln_Pri = 0;
    AddPort(p);
    Signal(principal, 1L << sig_p);
    for (;;) {
        WaitPort(p);
        struct MiMensaje *m = (struct MiMensaje *)GetMsg(p);
        if (!m) continue;
        LONG v = m->valor;
        suma += v;
        ReplyMsg(&m->m);
        if (v < 0) break;
    }
    RemPort(p);
    DeleteMsgPort(p);
}

static void hija_alta(void)
{
    bandera = 1;
}

static void hija_dormilona(void)
{
    Delay(5);
    bandera = 2;
    Signal(principal, 1L << sig_p);
}

static void hija_ocupada(void)
{
    while (!parar) {
        vueltas++;
        SetSignal(0, 0);
    }
    Signal(principal, 1L << sig_p);
}

/* Un proceso de dos: empieza con A0 = sus argumentos y D0 = su largo (la entrada, en ensamblador, los guarda). */
static volatile ULONG args_vistos, largo_visto;
static volatile LONG es_proceso, con_ficheros, escrito;
void proc_hijo(void) __attribute__((used));
void proc_entrada(void);
__asm__(
"       .text\n"
"       .globl  _proc_entrada\n"
"_proc_entrada:\n"
"       move.l  a0,_args_vistos\n"
"       move.l  d0,_largo_visto\n"
"       jmp     _proc_hijo\n");

void proc_hijo(void)
{
    struct Process *yo = (struct Process *)FindTask(0);
    es_proceso = yo->pr_Task.tc_Node.ln_Type == NT_PROCESS && yo->pr_StackSize >= 8000;
    con_ficheros = yo->pr_CIS != 0 && yo->pr_COS != 0 && Output() == yo->pr_COS;
    escrito = Write(Output(), (APTR)"nada\n", 5);
    bandera = 4;
    Signal(principal, 1L << sig_p);
}

/* Una lista de segmentos hecha a mano: [siguiente = 0][código]. El código pone `bandera_seg` a 1 y vuelve. */
static volatile LONG bandera_seg;
static ULONG *segmento_falso(void)
{
    ULONG *m = AllocMem(32, MEMF_PUBLIC | MEMF_CLEAR);
    if (!m) return 0;
    UWORD *c = (UWORD *)(m + 1);
    ULONG dir = (ULONG)&bandera_seg;
    c[0] = 0x52B9; c[1] = dir >> 16; c[2] = dir & 0xFFFF;   /* addq.l #1,bandera_seg */
    c[3] = 0x4E75;                                           /* rts */
    CacheClearU();
    return m;
}

static void normal(void)
{
    /* 1. Ping-pong con Wait y Signal. */
    cuenta = 0;
    hija = crear("ping", 0, hija_ping, 4096);
    Wait(1L << sig_p);
    for (int i = 0; i < IDAS; i++) {
        Signal(hija, 1L << sig_h);
        Wait(1L << sig_p);
    }
    di("  [caja-amiga] procesos: ping-pong: %ld idas y vueltas con Wait y Signal\n", cuenta, 0, 0);
    mira(cuenta == IDAS, "Wait y Signal entre dos tareas");
    /* La hija acaba al volver de su función (su última Signal no cede: es de la misma prioridad). */
    Delay(1);
    mira(!sigue_viva("ping"), "una tarea que vuelve de su funcion acaba");

    /* 2. Un puerto con nombre entre dos tareas: 100 mensajes y uno de fin. */
    suma = 0;
    crear("puerto", 0, hija_puerto, 4096);
    Wait(1L << sig_p);
    Forbid();
    struct MsgPort *suyo = FindPort((CONST_STRPTR)"procesos.puerto");
    Permit();
    struct MsgPort *vuelta = CreateMsgPort();
    LONG esperado = 0, contestados = 0;
    if (suyo && vuelta) {
        struct MiMensaje m;
        for (LONG i = 1; i <= 101; i++) {
            m.m.mn_Node.ln_Type = NT_MESSAGE;
            m.m.mn_ReplyPort = vuelta;
            m.m.mn_Length = sizeof(m);
            m.valor = i <= 100 ? i : -1;
            if (i <= 100) esperado += i;
            PutMsg(suyo, &m.m);
            WaitPort(vuelta);
            if (GetMsg(vuelta) == &m.m) contestados++;
        }
    }
    if (vuelta) DeleteMsgPort(vuelta);
    di("  [caja-amiga] procesos: puerto: %ld mensajes contestados, suma %ld\n", contestados, suma + 1, 0);
    mira(suyo && contestados == 101 && suma + 1 == esperado, "mensajes por un puerto entre dos tareas");
    Delay(1);
    mira(!sigue_viva("puerto"), "la tarea del puerto acaba");

    /* 3. Una de más prioridad corre en cuanto se añade. */
    bandera = 0;
    crear("alta", 5, hija_alta, 2048);
    mira(bandera == 1, "una tarea de mas prioridad corre en cuanto se anade");

    /* 4. Con Forbid no corre hasta Permit. */
    bandera = 0;
    Forbid();
    crear("alta2", 5, hija_alta, 2048);
    LONG dentro = bandera;
    Permit();
    mira(dentro == 0 && bandera == 1, "Forbid retiene el testigo hasta Permit");

    /* 5. Una tarea que se señala a sí misma no espera. */
    BYTE s = AllocSignal(-1);
    Signal(FindTask(0), 1L << s);
    ULONG r = Wait(1L << s);
    FreeSignal(s);
    mira(r == (1UL << s), "Signal a si misma y Wait");

    /* 6. Delay en una hija: duerme sin parar a las demás. */
    bandera = 0;
    crear("dormilona", 0, hija_dormilona, 2048);
    Wait(1L << sig_p);
    mira(bandera == 2, "Delay en una hija");

    /* 7. El turno: una hija que no espera nunca (llama a exec en un bucle) no se queda con la caja. */
    vueltas = 0;
    parar = 0;
    crear("ocupada", 0, hija_ocupada, 2048);
    Delay(10);
    LONG v1 = vueltas;
    parar = 1;
    Wait(1L << sig_p);
    di("  [caja-amiga] procesos: turno: la hija ocupada dio %ld vueltas mientras la principal dormia\n", v1, 0, 0);
    mira(v1 > 100, "el turno entre dos tareas de la misma prioridad");

    /* 8. CreateNewProc con NP_Entry: un proceso de más prioridad corre enseguida, con sus argumentos, NIL: como
       entrada y salida, y acaba al volver de su función (la caja cierra sus NIL:). */
    bandera = 0;
    static const char args[] = "uno dos\n";
    struct TagItem et[] = {
        { NP_Entry, (ULONG)proc_entrada }, { NP_Name, (ULONG)"hijo" }, { NP_Priority, 1 },
        { NP_StackSize, 8000 }, { NP_Arguments, (ULONG)args }, { TAG_DONE, 0 },
    };
    struct Process *pr = CreateNewProc(et);
    di("  [caja-amiga] procesos: CreateNewProc dio un proceso (%ld); argumentos de %ld bytes; escribio %ld en NIL:\n",
       pr != 0, largo_visto, escrito);
    mira(pr && bandera == 4 && es_proceso && con_ficheros && escrito == 5 && largo_visto == 8 && args_vistos != 0,
         "CreateNewProc con NP_Entry");
    Delay(1);
    mira(!sigue_viva("hijo"), "el proceso acaba al volver de su funcion");

    /* 9. CreateNewProc con NP_Seglist (una lista de segmentos hecha a mano) y CreateProc con la misma. */
    ULONG *seg = segmento_falso();
    bandera_seg = 0;
    struct TagItem es[] = { { NP_Seglist, MKBADDR(seg) }, { NP_Name, (ULONG)"segmento" }, { TAG_DONE, 0 } };
    struct Process *ps = seg ? CreateNewProc(es) : 0;
    Delay(2);
    struct MsgPort *mp = seg ? CreateProc((CONST_STRPTR)"viejo", 0, MKBADDR(seg), 4000) : 0;
    Delay(2);
    mira(ps && mp && bandera_seg == 2 && !sigue_viva("segmento") && !sigue_viva("viejo"), "NP_Seglist y CreateProc");
    if (seg) FreeMem(seg, 32);
}

/* --- b: hostil --- */

static volatile LONG vivas, empezadas;
#define MUCHAS 4000

static void hija_muchas(void)
{
    vivas++;
    empezadas++;
    Wait(SIGBREAKF_CTRL_C);
    vivas--;
}

static void hija_eterna(void)
{
    BYTE s = AllocSignal(-1);
    vivas++;
    Wait(1L << s);
}

static struct MsgPort *borrado;

static void hija_borra(void)
{
    borrado = CreateMsgPort();
    Signal(principal, 1L << sig_p);
    Wait(SIGBREAKF_CTRL_D);
    DeleteMsgPort(borrado);   /* con mensajes dentro */
    Signal(principal, 1L << sig_p);
}

static void hija_se_senala(void)
{
    BYTE s = AllocSignal(-1);
    for (int i = 0; i < 10000; i++) {
        Signal(FindTask(0), 1L << s);
        Wait(1L << s);
    }
    bandera = 3;
    Signal(principal, 1L << sig_p);
}

static void proc_eterno(void)
{
    vivas++;
    Wait(1L << 20);
}

static void hostil_procesos(void)
{
    /* CreateNewProc con NULL, sin código, con los dos, con una lista en círculo, una pila absurda, entradas malas: 0. */
    struct TagItem sin[] = { { NP_Name, (ULONG)"nada" }, { TAG_DONE, 0 } };
    struct TagItem dos_codigos[] = { { NP_Entry, (ULONG)proc_eterno }, { NP_Seglist, 0x1000 }, { TAG_DONE, 0 } };
    struct TagItem circulo[2] = { { NP_Name, (ULONG)"circulo" }, { TAG_MORE, 0 } };
    circulo[1].ti_Data = (ULONG)circulo;
    struct TagItem pila[] = { { NP_Entry, (ULONG)proc_eterno }, { NP_StackSize, 0xFFFFFFFF }, { TAG_DONE, 0 } };
    struct TagItem impar[] = { { NP_Entry, (ULONG)proc_eterno + 1 }, { TAG_DONE, 0 } };
    struct TagItem cero[] = { { NP_Entry, 0x10 }, { TAG_DONE, 0 } };
    struct TagItem nombre[] = { { NP_Entry, (ULONG)proc_eterno }, { NP_Name, 0xFFFFFF00 }, { TAG_DONE, 0 } };
    struct TagItem *malas[] = { 0, sin, dos_codigos, circulo, pila, impar, cero, nombre, (struct TagItem *)0x7FFFFFF0,
                                (struct TagItem *)3 };
    LONG dados = 0;
    for (unsigned i = 0; i < sizeof(malas) / sizeof(malas[0]); i++) dados += CreateNewProc(malas[i]) != 0;
    mira(dados == 0, "CreateNewProc con etiquetas absurdas o NULL da 0");
    /* Basura que se ignora (TAG_IGNORE, TAG_SKIP, etiquetas desconocidas, NP_NotifyOnDeath...): sale un proceso. */
    vivas = 0;
    struct TagItem raras[] = { { TAG_IGNORE, 7 }, { TAG_SKIP, 1 }, { NP_Name, 0x12345678 }, { 0x8FFFFFFF, 1 },
                               { NP_NotifyOnDeath, 1 }, { NP_Synchronous, 1 }, { NP_Entry, (ULONG)proc_eterno },
                               { NP_Name, (ULONG)"raro" }, { NP_Priority, 1000 }, { NP_CloseInput, 0 },
                               { NP_Input, 0x77777777 }, { TAG_DONE, 0 } };
    struct Process *p = CreateNewProc(raras);
    mira(p && vivas == 1 && p->pr_Task.tc_Node.ln_Pri == 127, "CreateNewProc con etiquetas raras pero validas");
    if (p) RemTask(&p->pr_Task);
    /* 200 procesos que no acaban: la caja los quita al acabar el programa. */
    struct TagItem et[] = { { NP_Entry, (ULONG)proc_eterno }, { NP_Name, (ULONG)"eterno" }, { TAG_DONE, 0 } };
    LONG n = 0;
    for (int i = 0; i < 200; i++) n += CreateNewProc(et) != 0;
    Delay(1);
    mira(n == 200, "200 procesos que no acaban");
}

static void hostil(void)
{
    /* 1. Miles de tareas, hasta MUCHAS o hasta que no quepan: todas llegan a su Wait; con Ctrl-C acaban. */
    vivas = empezadas = 0;
    static struct Task *ts[MUCHAS];
    LONG n = 0;
    while (n < MUCHAS && (ts[n] = crear("muchas", 0, hija_muchas, 1024)) != 0) n++;
    Delay(2);
    LONG llegaron = empezadas;
    for (LONG i = 0; i < n; i++) Signal(ts[i], SIGBREAKF_CTRL_C);
    Delay(2);
    di("  [caja-amiga] procesos: %ld tareas creadas, %ld llegaron a su Wait; tras Ctrl-C quedan %ld\n", n, llegaron, vivas);
    mira(n > 500 && llegaron == n && vivas == 0, "miles de tareas");

    /* 2. Un puerto borrado con mensajes dentro: lo de después se niega, nada se rompe. */
    struct MiMensaje *m[3];
    crear("borra", 0, hija_borra, 2048);
    Wait(1L << sig_p);
    struct MsgPort *vuelta = CreateMsgPort();
    for (int i = 0; i < 3; i++) {
        m[i] = AllocMem(sizeof(struct MiMensaje), MEMF_PUBLIC | MEMF_CLEAR);
        m[i]->m.mn_ReplyPort = vuelta;
        m[i]->m.mn_Length = sizeof(struct MiMensaje);
        PutMsg(borrado, &m[i]->m);
    }
    struct Task *b = FindTask((CONST_STRPTR)"borra");
    if (b) Signal(b, SIGBREAKF_CTRL_D);
    Wait(1L << sig_p);
    PutMsg(borrado, &m[0]->m);   /* a un puerto borrado: se niega */
    for (int i = 0; i < 3; i++) FreeMem(m[i], sizeof(struct MiMensaje));
    DeleteMsgPort(vuelta);
    mira(1, "un puerto borrado con mensajes dentro");

    /* 3. Una tarea quita a otra que espera; señalarla después no rompe nada. */
    vivas = 0;
    struct Task *e = crear("victima", 0, hija_eterna, 2048);
    Delay(1);
    RemTask(e);
    Signal(e, 1);
    mira(vivas == 1 && !sigue_viva("victima"), "una tarea quita a otra");

    /* 4. Una que se señala a sí misma 10.000 veces. */
    bandera = 0;
    crear("se-senala", 0, hija_se_senala, 2048);
    Wait(1L << sig_p);
    mira(bandera == 3, "una tarea que se senala a si misma sin fin");

    /* 5. AddTask con basura: NULL, sin pila, con la pila fuera de su sitio. */
    struct Task *t = AllocMem(sizeof(struct Task), MEMF_PUBLIC | MEMF_CLEAR);
    APTR r1 = AddTask(0, (APTR)hija_alta, 0);
    APTR r2 = AddTask(t, (APTR)hija_alta, 0);
    t->tc_SPReg = (APTR)0x7ffffff0;
    t->tc_SPLower = (APTR)0x100;
    t->tc_SPUpper = (APTR)0x200;
    APTR r3 = AddTask(t, (APTR)hija_alta, 0);
    FreeMem(t, sizeof(struct Task));
    mira(!r1 && !r2 && !r3, "AddTask con basura da 0");

    /* 6. Procesos de dos con etiquetas malas, y 200 que no acaban. */
    hostil_procesos();

    /* 7. Tareas que no acaban: se quedan esperando; la caja las quita al acabar el programa. */
    for (int i = 0; i < 20; i++) crear("eterna", 0, hija_eterna, 1024);
    Delay(1);
}

/* --- c: una tarea quita la del programa --- */

static void hija_mata(void)
{
    RemTask(principal);
    bandera = 9;   /* no llega: el programa acaba con la principal */
}

static void mata(void)
{
    crear("mata", 5, hija_mata, 2048);
    di("  [caja-amiga] procesos: MAL: la principal sigue tras quitarla\n", 0, 0, 0);
}

/* --- d: tareas que caen --- */

static void hija_ilegal(void)
{
    __asm__ volatile("illegal");
    bandera = 99;
}

static void hija_divcero(void)
{
    volatile LONG z = 0;
    bandera = 10 / z;
}

static void cae(void)
{
    bandera = 0;
    crear("cae-ilegal", 0, hija_ilegal, 2048);
    Delay(2);
    mira(!sigue_viva("cae-ilegal") && bandera == 0, "una tarea que cae por una instruccion ilegal se quita");
    crear("cae-div", 0, hija_divcero, 2048);
    Delay(2);
    mira(!sigue_viva("cae-div") && bandera == 0, "una tarea que divide por cero se quita");
    struct TagItem et[] = { { NP_Entry, (ULONG)hija_ilegal }, { NP_Name, (ULONG)"cae-proceso" }, { TAG_DONE, 0 } };
    struct Process *p = CreateNewProc(et);
    Delay(2);
    mira(p && !sigue_viva("cae-proceso") && bandera == 0, "un proceso que cae se quita");
    /* Y la principal sigue bien: otra tarea normal. */
    crear("alta", 5, hija_alta, 2048);
    mira(bandera == 1, "tras las caidas, todo sigue");
}

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    if (!DOSBase) return 20;
    principal = FindTask(0);
    const char *n = principal->tc_Node.ln_Name;
    while (n[0] && n[1]) n++;
    modo = n[0];
    sig_p = AllocSignal(-1);
    if (modo == 'a') normal();
    else if (modo == 'b') hostil();
    else if (modo == 'c') mata();
    else if (modo == 'd') cae();
    di("  [caja-amiga] procesos: %ld fallos (modo %lc)\n", fallos, modo, 0);
    FreeSignal(sig_p);
    CloseLibrary((struct Library *)DOSBase);
    return fallos ? 10 : 0;
}
