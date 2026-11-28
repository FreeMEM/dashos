/*
    La caja Amiga: MatchFirst y MatchNext con el patrón vacío y entrando en los cajones (APF_DODIR, APF_DIDDIR), y
    DateToStr. Es una prueba del oráculo: la misma salida en la caja y en un Amiga de verdad (FS-UAE con el
    Kickstart; tools/emu_run.py de Dash). Hace su árbol en el cajón actual, con un solo objeto por cajón donde importa
    el orden (el de ExNext depende del sistema de archivos), y lo borra al acabar. Los textos, en ASCII.

        zona/pdir/sub/c.txt
*/
#include <exec/types.h>
#include <exec/execbase.h>
#include <exec/memory.h>
#include <dos/dos.h>
#include <dos/dosasl.h>
#include <dos/datetime.h>
#include <proto/exec.h>
#include <proto/dos.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;

#define TAM_BUF 200

static void escribe(const char *formato, LONG a, LONG b, LONG c, LONG d)
{
    LONG v[4];
    v[0] = a;
    v[1] = b;
    v[2] = c;
    v[3] = d;
    VPrintf((CONST_STRPTR)formato, v);
}

static void fichero(const char *nombre, LONG bytes)
{
    BPTR f = Open((CONST_STRPTR)nombre, MODE_NEWFILE);
    if (f) {
        Write(f, (APTR)"0123456789", bytes);
        Close(f);
    }
}

/* Lo que dio MatchFirst o MatchNext: el código, el nombre de ap_Info, si es cajón, las banderas que importan y
   ap_Buf. */
static void dice(const char *quien, LONG r, struct AnchorPath *ap)
{
    if (r) {
        escribe("%s: %ld\n", (LONG)quien, r, 0, 0);
        return;
    }
    escribe("%s: %s (%s)", (LONG)quien, (LONG)ap->ap_Info.fib_FileName,
            (LONG)(ap->ap_Info.fib_DirEntryType > 0 ? "cajon" : "fichero"), 0);
    escribe("%s%s buf [%s]\n", (LONG)(ap->ap_Flags & APF_DIDDIR ? " DIDDIR" : ""),
            (LONG)(ap->ap_Flags & APF_ITSWILD ? " ITSWILD" : ""), (LONG)ap->ap_Buf, 0);
}

/* Recorre `patron` entrando en los cajones (todos, o solo el primero si `uno`), como Dir y List con ALL. */
static void recorre(const char *patron, struct AnchorPath *ap, int uno)
{
    escribe("== %s%s\n", (LONG)patron, (LONG)(uno ? " (solo el primer cajon)" : ""), 0, 0);
    ap->ap_Flags = 0;
    ap->ap_Strlen = TAM_BUF;
    LONG r = MatchFirst((CONST_STRPTR)patron, ap);
    int entrados = 0;
    for (int i = 0; i < 20; i++) {
        dice(i ? "MatchNext" : "MatchFirst", r, ap);
        if (r)
            break;
        if (ap->ap_Flags & APF_DIDDIR)
            ap->ap_Flags &= ~APF_DIDDIR;
        else if (ap->ap_Info.fib_DirEntryType > 0 && (!uno || !entrados)) {
            ap->ap_Flags |= APF_DODIR;
            entrados++;
        }
        r = MatchNext(ap);
    }
    MatchEnd(ap);
}

static void fecha(LONG dias, LONG minutos, LONG tics, UBYTE formato)
{
    static char dia[LEN_DATSTRING], fec[LEN_DATSTRING], hora[LEN_DATSTRING];
    struct DateTime dt;
    dt.dat_Stamp.ds_Days = dias;
    dt.dat_Stamp.ds_Minute = minutos;
    dt.dat_Stamp.ds_Tick = tics;
    dt.dat_Format = formato;
    dt.dat_Flags = 0;
    dt.dat_StrDay = (STRPTR)dia;
    dt.dat_StrDate = (STRPTR)fec;
    dt.dat_StrTime = (STRPTR)hora;
    dia[0] = fec[0] = hora[0] = 0;
    LONG r = DateToStr(&dt);
    escribe("DateToStr %ld/%ld/%ld", dias, minutos, tics, 0);
    escribe(" formato %ld: %ld [%s] [%s]", formato, r != 0, (LONG)dia, (LONG)fec);
    escribe(" [%s]\n", (LONG)hora, 0, 0, 0);
}

/* Con DTF_SUBST, lo que sale en la fecha según lo lejos que esté de hoy: "Today", "Yesterday", "Tomorrow", el día
   de la semana o la fecha (el día de hoy cambia, así que se dice la clase, no el texto). */
static void sustituida(LONG desde_hoy)
{
    static char dia[LEN_DATSTRING], fec[LEN_DATSTRING], hora[LEN_DATSTRING];
    struct DateTime dt;
    DateStamp(&dt.dat_Stamp);
    dt.dat_Stamp.ds_Days += desde_hoy;
    dt.dat_Format = FORMAT_DOS;
    dt.dat_Flags = DTF_SUBST;
    dt.dat_StrDay = (STRPTR)dia;
    dt.dat_StrDate = (STRPTR)fec;
    dt.dat_StrTime = (STRPTR)hora;
    DateToStr(&dt);
    const char *clase = "fecha";
    const char *nombres[] = { "Today", "Yesterday", "Tomorrow" };
    for (int i = 0; i < 3; i++) {
        const char *a = nombres[i], *b = fec;
        while (*a && *a == *b) a++, b++;
        if (!*a && !*b) clase = nombres[i];
    }
    const char *a = dia, *b = fec;
    while (*a && *a == *b) a++, b++;
    if (!*a && !*b) clase = "dia de la semana";
    escribe("DateToStr DTF_SUBST hoy%s%ld: %s\n", (LONG)(desde_hoy >= 0 ? "+" : ""), desde_hoy, (LONG)clase, 0);
}

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 37);
    if (!DOSBase)
        return 20;
    struct AnchorPath *ap = AllocVec(sizeof(struct AnchorPath) + TAM_BUF, MEMF_CLEAR);
    if (!ap)
        return 20;
    BPTR l;
    if ((l = CreateDir((CONST_STRPTR)"zona"))) UnLock(l);
    if ((l = CreateDir((CONST_STRPTR)"zona/pdir"))) UnLock(l);
    if ((l = CreateDir((CONST_STRPTR)"zona/pdir/sub"))) UnLock(l);
    fichero("zona/pdir/sub/c.txt", 3);
    BPTR zona = Lock((CONST_STRPTR)"zona", SHARED_LOCK);
    if (!zona) {
        PutStr((CONST_STRPTR)"sin zona\n");
        return 20;
    }
    BPTR antes = CurrentDir(zona);

    /* 1. El patrón vacío: el cajón actual (pdir). */
    BPTR pdir = Lock((CONST_STRPTR)"pdir", SHARED_LOCK);
    BPTR antes2 = CurrentDir(pdir);
    PutStr((CONST_STRPTR)"== \"\" en pdir\n");
    ap->ap_Flags = 0;
    ap->ap_Strlen = TAM_BUF;
    LONG r = MatchFirst((CONST_STRPTR)"", ap);
    dice("MatchFirst", r, ap);
    if (!r) dice("MatchNext", MatchNext(ap), ap);
    MatchEnd(ap);
    recorre("", ap, 0);
    CurrentDir(antes2);
    UnLock(pdir);

    /* 2. Un cajón sin comodines, entrando en todo (Dir ALL). */
    recorre("pdir", ap, 0);
    /* 3. Lo mismo, entrando solo en el primero (Dir). */
    recorre("pdir", ap, 1);
    /* 4. Con comodines, entrando: dentro sale todo, case o no con el patrón (sub no casa con p#?). */
    recorre("p#?", ap, 0);

    /* 5. DateToStr: los cuatro formatos, sin sustituir (DTF_SUBST lo haría depender del día de hoy). */
    for (UBYTE f = FORMAT_DOS; f <= FORMAT_CDN; f++) fecha(7000, 750, 1234, f);
    fecha(0, 0, 0, FORMAT_DOS);
    fecha(13148, 1439, 2999, FORMAT_INT);
    fecha(366 + 365 * 3, 61, 49, FORMAT_USA);
    fecha(7000, 750, 1234, 4);
    fecha(7000, 750, 1234, 9);
    for (LONG d = -9; d <= 9; d++) sustituida(d);

    CurrentDir(antes);
    UnLock(zona);
    DeleteFile((CONST_STRPTR)"zona/pdir/sub/c.txt");
    DeleteFile((CONST_STRPTR)"zona/pdir/sub");
    DeleteFile((CONST_STRPTR)"zona/pdir");
    DeleteFile((CONST_STRPTR)"zona");
    FreeVec(ap);
    CloseLibrary((struct Library *)DOSBase);
    return 0;
}
