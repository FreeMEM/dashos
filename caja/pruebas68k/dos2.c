/*
    La caja Amiga: más de dos.library, de lo que piden los comandos de C: de AmigaOS 3.9 (Wait, Setdate, Break,
    Status, Which, Execute). Es una prueba del oráculo: la misma salida en la caja y en un Amiga de verdad (FS-UAE con el
    Kickstart; tools/emu_run.py de Dash). ReadArgs con un argumento sin nombre y con alias; StrToDate; MaxCli y
    FindCliProc; FindSegment; ReadItem. Los textos, en ASCII.
*/
#include <exec/types.h>
#include <exec/execbase.h>
#include <dos/dos.h>
#include <dos/dosextens.h>
#include <dos/rdargs.h>
#include <dos/datetime.h>
#include <proto/exec.h>
#include <proto/dos.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;

static void escribe(const char *f, LONG a, LONG b, LONG c, LONG d)
{
    LONG v[4] = { a, b, c, d };
    VPrintf((CONST_STRPTR)f, v);
}

static void lee(const char *plantilla, const char *linea)
{
    static char buf[128];
    int n = 0;
    while (linea[n] && n < 120) { buf[n] = linea[n]; n++; }
    buf[n++] = '\n';
    buf[n] = 0;
    struct RDArgs *rd = AllocDosObject(DOS_RDARGS, NULL);
    rd->RDA_Source.CS_Buffer = (STRPTR)buf;
    rd->RDA_Source.CS_Length = n;
    rd->RDA_Source.CS_CurChr = 0;
    LONG a[4] = { 0, 0, 0, 0 };
    struct RDArgs *r = ReadArgs((CONST_STRPTR)plantilla, a, rd);
    if (!r) {
        escribe("ReadArgs [%s] [%s]: nulo, IoErr %ld\n", (LONG)plantilla, (LONG)linea, IoErr(), 0);
    } else {
        escribe("ReadArgs [%s] [%s]: %ld", (LONG)plantilla, (LONG)linea, a[0] ? *(LONG *)a[0] : -1, 0);
        escribe(" %ld %ld [%s]\n", a[1], a[2], (LONG)(a[3] ? (char *)a[3] : "-"), 0);
        FreeArgs(r);
    }
    FreeDosObject(DOS_RDARGS, rd);
}

static void fecha(const char *f, const char *h, UBYTE formato)
{
    struct DateTime dt;
    dt.dat_Format = formato;
    dt.dat_Flags = 0;
    dt.dat_StrDay = NULL;
    dt.dat_StrDate = (STRPTR)f;
    dt.dat_StrTime = (STRPTR)h;
    dt.dat_Stamp.ds_Days = dt.dat_Stamp.ds_Minute = dt.dat_Stamp.ds_Tick = -1;
    LONG r = StrToDate(&dt);
    escribe("StrToDate [%s] [%s] formato %ld: ", (LONG)(f ? f : "-"), (LONG)(h ? h : "-"), formato, 0);
    escribe("%ld: %ld/%ld/%ld\n", r != 0, dt.dat_Stamp.ds_Days, dt.dat_Stamp.ds_Minute, dt.dat_Stamp.ds_Tick);
}

static void item(const char *texto)
{
    static char buf[128], sal[64];
    int n = 0;
    while (texto[n] && n < 120) { buf[n] = texto[n]; n++; }
    struct CSource cs;
    cs.CS_Buffer = (STRPTR)buf;
    cs.CS_Length = n;
    cs.CS_CurChr = 0;
    escribe("ReadItem [%s]:", (LONG)texto, 0, 0, 0);
    for (int i = 0; i < 6; i++) {
        for (int k = 0; k < 64; k++) sal[k] = 0;
        LONG r = ReadItem((STRPTR)sal, sizeof sal, &cs);
        escribe(" %ld[%s]", r, (LONG)sal, 0, 0);
        if (r <= 0) break;
    }
    escribe(" fin en %ld\n", cs.CS_CurChr, 0, 0, 0);
}

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 37);
    if (!DOSBase)
        return 20;
    /* ReadArgs: un primero sin nombre (/N), alias y claves. */
    const char *pl = "/N,SEC=SECS/S,MIN=MINS/S,UNTIL/K";
    lee(pl, "5");
    lee(pl, "5 MIN");
    lee(pl, "7 SECS");
    lee(pl, "UNTIL 10:30");
    lee(pl, "");
    lee("NOMBRE/A,=X/S", "hola");
    /* StrToDate. */
    fecha("02-Mar-97", "12:30:24", FORMAT_DOS);
    fecha("97-03-02", NULL, FORMAT_INT);
    fecha("03-02-97", "01:02", FORMAT_USA);
    fecha("02-03-97", NULL, FORMAT_CDN);
    fecha(NULL, "23:59:59", FORMAT_DOS);
    fecha("01-Jan-78", "00:00:00", FORMAT_DOS);
    fecha("31-Dec-13", NULL, FORMAT_DOS);
    fecha("29-Feb-00", NULL, FORMAT_DOS);
    fecha("30-Feb-00", NULL, FORMAT_DOS);
    fecha("mal", NULL, FORMAT_DOS);
    fecha("02-Mar-97", "25:00", FORMAT_DOS);
    /* MaxCli y FindCliProc: el CLI de este programa existe y es el mismo proceso que FindTask(0). */
    ULONG mx = MaxCli();
    struct Process *yo = (struct Process *)FindTask(NULL);
    LONG mio = 0;
    for (ULONG i = 1; i <= mx; i++) if (FindCliProc(i) == yo) mio = i;
    escribe("MaxCli >= 1: %ld; su CLI esta entre ellos: %ld; FindCliProc(0) %ld, FindCliProc(1000) %ld\n", mx >= 1,
            mio > 0, (LONG)(FindCliProc(0) != 0), (LONG)(FindCliProc(1000) != 0));
    /* FindSegment: sin residentes de usuario. */
    escribe("FindSegment(\"Dir\", 0, 0): %ld; (\"NoExiste\", 0, 1): %ld\n", FindSegment((CONST_STRPTR) "Dir", NULL, 0) != 0,
            FindSegment((CONST_STRPTR) "NoExiste", NULL, 1) != 0, 0, 0);
    /* ReadItem. */
    item("uno dos tres");
    item("  \"con espacio\" = fin");
    item("a=b");
    item("\"*\"escape*N\" x");
    item("");
    return 0;
}
