/*
    La velocidad de la red de la caja Amiga (rama amiga-apps): baja un fichero por HTTP con la bsdsocket.library nativa
    (gethostbyname, socket, connect, send, recv) y dice cuánto tardó y a cuántos KB/s. Argumentos: máquina, puerto y
    camino (p. ej. "10.0.2.2 8099 /grande.bin", el servidor de `tools/amiga_apps.py correr --red`). Acaba con 0 si bajó
    algo, 10 si no.
*/
#include <exec/types.h>
#include <sys/types.h>
#include <dos/dos.h>
#include <dos/dosextens.h>
#include <proto/exec.h>
#include <proto/dos.h>
#include <proto/bsdsocket.h>
#include <netinet/in.h>
#include <sys/socket.h>
#include <netdb.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;
struct Library *SocketBase;

static void di(const char *f, LONG a, LONG b, LONG c, LONG d)
{
    LONG v[4] = { a, b, c, d };
    VPrintf((CONST_STRPTR)f, v);
}

static ULONG ms(void)
{
    struct DateStamp d;
    DateStamp(&d);
    return ((ULONG)d.ds_Minute * 60 * 50 + (ULONG)d.ds_Tick) * 20;
}

static char buf[32768];

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 37);
    if (!DOSBase) return 20;
    LONG r = 10;
    char *args = (char *)GetArgStr();
    char *maquina = args, *puerto_t = 0, *camino = 0;
    for (char *p = args; *p; p++) {
        if (*p == ' ' || *p == '\n') {
            *p = 0;
            if (!puerto_t) puerto_t = p + 1;
            else if (!camino) camino = p + 1;
        }
    }
    LONG puerto = 80;
    if (puerto_t) StrToLong((CONST_STRPTR)puerto_t, &puerto);
    if (!camino || !*camino) camino = "/";
    SocketBase = OpenLibrary((CONST_STRPTR)"bsdsocket.library", 4);
    if (!SocketBase) {
        di("  [caja-amiga] red_prueba: no hay bsdsocket.library\n", 0, 0, 0, 0);
        return 20;
    }
    ULONG t0 = ms();
    struct hostent *h = gethostbyname((STRPTR)maquina);
    if (!h) {
        di("  [caja-amiga] red_prueba: %s no se resuelve\n", (LONG)maquina, 0, 0, 0);
        goto fuera;
    }
    ULONG t1 = ms();
    LONG s = socket(AF_INET, SOCK_STREAM, 0);
    struct sockaddr_in a = { 0 };
    a.sin_len = sizeof(a);
    a.sin_family = AF_INET;
    a.sin_port = puerto;
    CopyMem(h->h_addr_list[0], &a.sin_addr, 4);
    if (connect(s, (struct sockaddr *)&a, sizeof(a)) < 0) {
        di("  [caja-amiga] red_prueba: connect da error %ld\n", Errno(), 0, 0, 0);
        CloseSocket(s);
        goto fuera;
    }
    ULONG t2 = ms();
    char *pet = buf;
    int n = 0;
    const char *partes[] = { "GET ", camino, " HTTP/1.0\r\nHost: ", maquina, "\r\nUser-Agent: caja-amiga\r\n\r\n" };
    for (int i = 0; i < 5; i++)
        for (const char *q = partes[i]; *q && n < 1000; q++) pet[n++] = *q;
    send(s, pet, n, 0);
    ULONG total = 0;
    for (;;) {
        LONG k = recv(s, buf, sizeof(buf), 0);
        if (k <= 0) break;
        total += k;
    }
    ULONG t3 = ms();
    CloseSocket(s);
    ULONG dur = t3 - t2 ? t3 - t2 : 1;
    di("  [caja-amiga] red_prueba: %s resuelto en %ld ms, conectado en %ld ms\n", (LONG)maquina, t1 - t0, t2 - t1, 0);
    di("  [caja-amiga] red_prueba: %ld bytes en %ld ms: %ld KB/s\n", total, dur, total / dur * 1000 / 1024, 0);
    r = total > 0 ? 0 : 10;
fuera:
    CloseLibrary(SocketBase);
    CloseLibrary((struct Library *)DOSBase);
    return r;
}
