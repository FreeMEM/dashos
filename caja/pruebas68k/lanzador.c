/*
    El lanzador de programas del usuario en la caja Amiga (rama amiga-apps): lo que haría una persona en un Shell del
    Amiga, para probar programas de verdad (ProTracker, Directory Opus, LightWave...) con sus argumentos, varios a la
    vez y con sus asignaciones.

    Lee un guion de PROGDIR:lanzador/<su nombre>.txt (su nombre: el de su tarea sin el camino; así una copia del
    lanzador llamada "escena-pt" lee lanzador/escena-pt.txt). Una orden por línea; '#' empieza un comentario:

      asigna NOMBRE: camino        AssignLock (el nombre de antes se quita)
      asignaadd NOMBRE: camino     AssignAdd (detrás de lo que ya haya)
      variable nombre valor        SetVar global (ENV:)
      cajon camino                 el cajón actual de lo que se lance después (y del propio lanzador)
      lanza programa argumentos    LoadSeg y CreateNewProc con su CLI, sus argumentos y su cajón (el de la orden
                                   cajon; si no hay, el del programa); no espera a que acabe
      espera N                     N décimas de segundo
      marca texto                  escribe "lanzador: marca texto" (el guion de las capturas lo espera para
                                   sacar la captura)
      hasta N                      espera a que acaben todos los lanzados, como mucho N décimas
      ventana texto N              espera, como mucho N décimas, a que haya una ventana cuyo título lleve `texto` (en
                                   cualquier pantalla) y dice a los cuántos segundos salió: lo que tarda en abrir

    Al final espera a que acaben todos los que lanzó (si acabara antes, se los llevaría con él: en la caja, las tareas
    de un programa mueren con él). Cada cosa que hace la dice por la consola, con su tiempo desde que empezó, para
    medir cuánto tarda cada programa en abrir.

    No es una prueba de la caja: no va en caja.c. Se compila aquí (make lanzador.exe) y se pasa con
    DASHOS_PROGRAMAS_AMIGA (tools/disco_archivos.py), como los programas del usuario.
*/
#include <exec/types.h>
#include <exec/memory.h>
#include <exec/tasks.h>
#include <dos/dos.h>
#include <dos/dosextens.h>
#include <dos/dostags.h>
#include <dos/var.h>
#include <intuition/intuitionbase.h>
#include <intuition/screens.h>
#include <proto/exec.h>
#include <proto/dos.h>
#include <proto/intuition.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;
struct IntuitionBase *IntuitionBase;

#define MAX_HIJOS 16
#define MAX_GUION 16384

static struct Task *hijos[MAX_HIJOS];
static char nombres[MAX_HIJOS][40];
static int nhijos;
static ULONG t0_tics;

static ULONG ahora_decimas(void)
{
    struct DateStamp d;
    DateStamp(&d);
    ULONG t = (ULONG)d.ds_Minute * 60 * 50 + (ULONG)d.ds_Tick;
    return (t - t0_tics) / 5;
}

static void di(const char *f, LONG a, LONG b, LONG c)
{
    LONG v[4] = { (LONG)ahora_decimas() / 10, (LONG)ahora_decimas() % 10, 0, 0 };
    VPrintf((CONST_STRPTR)"  [caja-amiga] lanzador %ld.%ld s: ", v);
    LONG w[3] = { a, b, c };
    VPrintf((CONST_STRPTR)f, w);
}

static int es_blanco(char c) { return c == ' ' || c == '\t'; }

/* Corta la primera palabra de *p (con comillas dobles, como el Shell) y deja *p tras ella. */
static char *palabra(char **p)
{
    char *s = *p;
    while (es_blanco(*s)) s++;
    if (!*s) { *p = s; return 0; }
    char *ini;
    if (*s == '"') {
        ini = ++s;
        while (*s && *s != '"') s++;
    } else {
        ini = s;
        while (*s && !es_blanco(*s)) s++;
    }
    if (*s) *s++ = 0;
    *p = s;
    return ini;
}

static LONG numero(const char *s)
{
    LONG n = 0;
    if (s) StrToLong((CONST_STRPTR)s, &n);
    return n;
}

static int igual(const char *a, const char *b)
{
    while (*a && *a == *b) a++, b++;
    return *a == *b;
}

static void quitar_dos_puntos(char *n)
{
    int l = 0;
    while (n[l]) l++;
    if (l && n[l - 1] == ':') n[l - 1] = 0;
}

static int vivo(int i)
{
    /* La tarea sigue si está en las listas de exec (FindTask por su nombre devuelve esta misma). */
    Forbid();
    struct Task *t = FindTask((CONST_STRPTR)nombres[i]);
    Permit();
    return t == hijos[i];
}

static int quedan(void)
{
    int n = 0;
    for (int i = 0; i < nhijos; i++)
        if (hijos[i] && vivo(i)) n++;
        else if (hijos[i]) {
            di("acaba %s\n", (LONG)nombres[i], 0, 0);
            hijos[i] = 0;
        }
    return n;
}

static BPTR cajon_actual;

/* Si `aguja` está dentro de `pajar` (sin mirar mayúsculas de ASCII). */
static int contiene(const char *pajar, const char *aguja)
{
    if (!pajar) return 0;
    for (; *pajar; pajar++) {
        const char *a = pajar, *b = aguja;
        while (*a && *b && ((*a | 32) == (*b | 32))) a++, b++;
        if (!*b) return 1;
    }
    return 0;
}

/* Una ventana con ese texto en el título, en alguna pantalla (las listas de IntuitionBase, con LockIBase). */
static int hay_ventana(const char *texto)
{
    if (!IntuitionBase) return 0;
    int si = 0;
    ULONG l = LockIBase(0);
    for (struct Screen *s = IntuitionBase->FirstScreen; s && !si; s = s->NextScreen)
        for (struct Window *w = s->FirstWindow; w && !si; w = w->NextWindow)
            si = contiene((const char *)w->Title, texto);
    UnlockIBase(l);
    return si;
}

static void lanzar(char *prog, char *resto)
{
    if (nhijos >= MAX_HIJOS) { di("demasiados programas\n", 0, 0, 0); return; }
    ULONG t = ahora_decimas();
    BPTR seg = LoadSeg((CONST_STRPTR)prog);
    if (!seg) { di("%s no carga (IoErr %ld)\n", (LONG)prog, IoErr(), 0); return; }
    /* Sus argumentos, con el salto de línea del final (como los da el Shell). */
    static char args[MAX_HIJOS][512];
    char *a = args[nhijos];
    int l = 0;
    while (es_blanco(*resto)) resto++;
    while (resto[l] && l < 510) { a[l] = resto[l]; l++; }
    a[l++] = '\n';
    a[l] = 0;
    /* Su cajón: el de la orden cajon, o el del programa. */
    BPTR dir = 0;
    if (cajon_actual) dir = DupLock(cajon_actual);
    else {
        BPTR f = Lock((CONST_STRPTR)prog, SHARED_LOCK);
        if (f) { dir = ParentDir(f); UnLock(f); }
    }
    BPTR casa = 0;
    {
        BPTR f = Lock((CONST_STRPTR)prog, SHARED_LOCK);
        if (f) { casa = ParentDir(f); UnLock(f); }
    }
    const char *corto = (const char *)FilePart((CONST_STRPTR)prog);
    char *nom = nombres[nhijos];
    int k = 0;
    while (corto[k] && k < 39) { nom[k] = corto[k]; k++; }
    nom[k] = 0;
    struct Process *p = CreateNewProcTags(NP_Seglist, (ULONG)seg, NP_FreeSeglist, TRUE, NP_Name, (ULONG)nom,
                                          NP_CommandName, (ULONG)nom, NP_Arguments, (ULONG)a, NP_Cli, TRUE,
                                          NP_StackSize, 65536, NP_CurrentDir, (ULONG)dir, NP_HomeDir, (ULONG)casa,
                                          NP_Input, (ULONG)Open((CONST_STRPTR)"NIL:", MODE_OLDFILE),
                                          NP_CloseInput, TRUE, NP_Output, (ULONG)Output(), NP_CloseOutput, FALSE,
                                          TAG_DONE);
    if (!p) {
        di("%s: CreateNewProc da 0 (IoErr %ld)\n", (LONG)prog, IoErr(), 0);
        UnLoadSeg(seg);
        if (dir) UnLock(dir);
        if (casa) UnLock(casa);
        return;
    }
    hijos[nhijos++] = &p->pr_Task;
    di("lanza %s (en %ld ms)\n", (LONG)prog, (LONG)(ahora_decimas() - t) * 100, 0);
}

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 37);
    if (!DOSBase) return 20;
    IntuitionBase = (struct IntuitionBase *)OpenLibrary((CONST_STRPTR)"intuition.library", 39);
    {
        struct DateStamp d;
        DateStamp(&d);
        t0_tics = (ULONG)d.ds_Minute * 60 * 50 + (ULONG)d.ds_Tick;
    }
    struct Task *yo = FindTask(0);
    const char *minombre = (const char *)FilePart((CONST_STRPTR)yo->tc_Node.ln_Name);
    char camino[160] = "PROGDIR:lanzador/";
    int l = 17;
    for (int i = 0; minombre[i] && l < 150; i++) camino[l++] = minombre[i];
    camino[l++] = '.', camino[l++] = 't', camino[l++] = 'x', camino[l++] = 't', camino[l] = 0;
    BPTR f = Open((CONST_STRPTR)camino, MODE_OLDFILE);
    if (!f) {
        di("no hay guion %s (IoErr %ld)\n", (LONG)camino, IoErr(), 0);
        CloseLibrary((struct Library *)DOSBase);
        return 10;
    }
    char *guion = AllocVec(MAX_GUION, MEMF_CLEAR);
    LONG n = guion ? Read(f, guion, MAX_GUION - 1) : -1;
    Close(f);
    if (n < 0) { di("no lee el guion\n", 0, 0, 0); return 10; }
    di("guion %s, %ld bytes\n", (LONG)camino, n, 0);
    char *p = guion;
    while (*p) {
        char *lin = p;
        while (*p && *p != '\n') p++;
        if (*p) *p++ = 0;
        char *q = lin;
        char *orden = palabra(&q);
        if (!orden || orden[0] == '#') continue;
        if (igual(orden, "asigna") || igual(orden, "asignaadd")) {
            char *nom = palabra(&q), *donde = palabra(&q);
            if (!nom || !donde) continue;
            quitar_dos_puntos(nom);
            BPTR k = Lock((CONST_STRPTR)donde, SHARED_LOCK);
            if (!k) { di("asigna %s: %s no está (IoErr %ld)\n", (LONG)nom, (LONG)donde, IoErr()); continue; }
            BOOL bien = orden[6] ? AssignAdd((CONST_STRPTR)nom, k) : AssignLock((CONST_STRPTR)nom, k);
            if (!bien) { di("asigna %s: falla (IoErr %ld)\n", (LONG)nom, IoErr(), 0); UnLock(k); }
            else di("%s: -> %s\n", (LONG)nom, (LONG)donde, 0);
        } else if (igual(orden, "variable")) {
            char *nom = palabra(&q);
            while (es_blanco(*q)) q++;
            if (nom && !SetVar((CONST_STRPTR)nom, (CONST_STRPTR)q, -1, GVF_GLOBAL_ONLY))
                di("variable %s: falla (IoErr %ld)\n", (LONG)nom, IoErr(), 0);
        } else if (igual(orden, "cajon")) {
            char *donde = palabra(&q);
            BPTR k = donde ? Lock((CONST_STRPTR)donde, SHARED_LOCK) : 0;
            if (!k) { di("cajon %s: no está (IoErr %ld)\n", (LONG)(donde ? donde : ""), IoErr(), 0); continue; }
            if (cajon_actual) UnLock(cajon_actual);
            cajon_actual = k;
            UnLock(CurrentDir(DupLock(k)));
        } else if (igual(orden, "lanza")) {
            char *prog = palabra(&q);
            if (prog) lanzar(prog, q);
        } else if (igual(orden, "espera")) {
            Delay(numero(palabra(&q)) * 5);
        } else if (igual(orden, "marca")) {
            while (es_blanco(*q)) q++;
            di("marca %s\n", (LONG)q, 0, 0);
        } else if (igual(orden, "ventana")) {
            char *texto = palabra(&q);
            ULONG tope = ahora_decimas() + (ULONG)numero(palabra(&q));
            ULONG t = ahora_decimas();
            while (texto && !hay_ventana(texto) && ahora_decimas() < tope) Delay(5);
            if (texto && hay_ventana(texto)) di("ventana \"%s\" a los %ld ms de pedirla\n", (LONG)texto, (LONG)(ahora_decimas() - t) * 100, 0);
            else di("ventana \"%s\": no sale\n", (LONG)(texto ? texto : ""), 0, 0);
        } else if (igual(orden, "hasta")) {
            ULONG tope = ahora_decimas() + (ULONG)numero(palabra(&q));
            while (quedan() && ahora_decimas() < tope) Delay(10);
        } else {
            di("orden desconocida: %s\n", (LONG)orden, 0, 0);
        }
    }
    /* Que no se lleve a sus hijos: espera a que acaben (los cierra la caja, o Ctrl-C). */
    while (quedan()) {
        if (SetSignal(0, 0) & SIGBREAKF_CTRL_C) {
            for (int i = 0; i < nhijos; i++)
                if (hijos[i]) Signal(hijos[i], SIGBREAKF_CTRL_C);
            SetSignal(0, SIGBREAKF_CTRL_C);
        }
        Delay(10);
    }
    di("todos acabaron\n", 0, 0, 0);
    if (cajon_actual) UnLock(cajon_actual);
    FreeVec(guion);
    if (IntuitionBase) CloseLibrary((struct Library *)IntuitionBase);
    CloseLibrary((struct Library *)DOSBase);
    return 0;
}
