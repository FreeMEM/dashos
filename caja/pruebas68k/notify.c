/*
    La caja Amiga: StartNotify y EndNotify de dos.library (los piden Forja y AmigaAMP para ver si cambian sus ficheros). Es
    una prueba del oráculo: la misma salida en la caja y en un Amiga de verdad (FS-UAE con el Kickstart; tools/emu_run.py
    de Dash). En RAM:, con señal y con mensaje: cambiar, borrar, crear lo que no estaba, NRF_NOTIFY_INITIAL,
    NRF_WAIT_REPLY, nr_FullName y EndNotify.
*/
#include <exec/types.h>
#include <exec/ports.h>
#include <dos/dos.h>
#include <dos/notify.h>
#include <proto/exec.h>
#include <proto/dos.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;

static void escribe(const char *f, LONG a, LONG b, LONG c, LONG d)
{
    LONG v[4] = { a, b, c, d };
    VPrintf((CONST_STRPTR)f, v);
}

/* nr_FullName tras los dos puntos: delante, el Amiga pone el dispositivo (RAM:) y la caja el volumen (Ram Disk:). */
static const char *tras_dos_puntos(const UBYTE *n)
{
    if (!n)
        return "-";
    const char *p = (const char *)n;
    for (const char *q = p; *q; q++)
        if (*q == ':') p = q + 1;
    return p;
}

static void pon(const char *nombre, const char *texto)
{
    BPTR f = Open((CONST_STRPTR)nombre, MODE_NEWFILE);
    if (f) {
        LONG n = 0;
        while (texto[n]) n++;
        Write(f, (APTR)texto, n);
        Close(f);
    }
}

/* Las señales de CTRL_E que llegaron (y las borra). */
static LONG senal(void)
{
    Delay(2);
    return (SetSignal(0, SIGBREAKF_CTRL_E) & SIGBREAKF_CTRL_E) != 0;
}

/* Los mensajes que llegaron al puerto: cuántos, y la clase, el código y si es de `nr` el primero. Los contesta. */
static void mensajes(const char *que, struct MsgPort *p, struct NotifyRequest *nr)
{
    Delay(2);
    struct NotifyMessage *m;
    LONG n = 0;
    while ((m = (struct NotifyMessage *)GetMsg(p))) {
        if (n == 0)
            escribe("%s: clase %lx codigo %lx suyo %ld", (LONG)que, m->nm_Class, m->nm_Code, m->nm_NReq == nr);
        ReplyMsg((struct Message *)m);
        n++;
    }
    if (n == 0)
        escribe("%s: ninguno", (LONG)que, 0, 0, 0);
    escribe(" (%ld)\n", n, 0, 0, 0);
}

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 37);
    if (!DOSBase)
        return 20;
    pon("RAM:notif.txt", "uno");
    SetSignal(0, SIGBREAKF_CTRL_E);
    /* Con señal. */
    struct NotifyRequest nr = { 0 };
    nr.nr_Name = (STRPTR)"RAM:notif.txt";
    nr.nr_Flags = NRF_SEND_SIGNAL;
    nr.nr_stuff.nr_Signal.nr_Task = FindTask(NULL);
    nr.nr_stuff.nr_Signal.nr_SignalNum = SIGBREAKB_CTRL_E;
    LONG r = StartNotify(&nr);
    escribe("StartNotify con senal: %ld, nombre completo [...:%s]\n", r != 0, (LONG)tras_dos_puntos(nr.nr_FullName), 0, 0);
    escribe("sin cambios: %ld\n", senal(), 0, 0, 0);
    pon("RAM:notif.txt", "dos");
    escribe("tras escribir: %ld\n", senal(), 0, 0, 0);
    BPTR f = Open((CONST_STRPTR)"RAM:notif.txt", MODE_OLDFILE);
    Close(f);
    escribe("tras abrir y cerrar sin escribir: %ld\n", senal(), 0, 0, 0);
    pon("RAM:otro.txt", "x");
    escribe("tras escribir otro fichero: %ld\n", senal(), 0, 0, 0);
    SetProtection((CONST_STRPTR)"RAM:notif.txt", FIBF_ARCHIVE);
    escribe("tras SetProtection: %ld\n", senal(), 0, 0, 0);
    DeleteFile((CONST_STRPTR)"RAM:notif.txt");
    escribe("tras borrarlo: %ld\n", senal(), 0, 0, 0);
    pon("RAM:notif.txt", "tres");
    escribe("tras crearlo otra vez: %ld\n", senal(), 0, 0, 0);
    EndNotify(&nr);
    pon("RAM:notif.txt", "cuatro");
    escribe("tras EndNotify: %ld\n", senal(), 0, 0, 0);
    /* Con mensaje, y uno que aún no existe. */
    struct MsgPort *p = CreateMsgPort();
    struct NotifyRequest nm = { 0 };
    nm.nr_Name = (STRPTR)"RAM:nuevo.txt";
    nm.nr_Flags = NRF_SEND_MESSAGE;
    nm.nr_stuff.nr_Msg.nr_Port = p;
    r = StartNotify(&nm);
    escribe("StartNotify con mensaje de uno que no existe: %ld, [...:%s]\n", r != 0, (LONG)tras_dos_puntos(nm.nr_FullName), 0, 0);
    mensajes("sin cambios", p, &nm);
    pon("RAM:nuevo.txt", "hola");
    mensajes("al crearlo", p, &nm);
    pon("RAM:nuevo.txt", "otra");
    pon("RAM:nuevo.txt", "y otra");
    mensajes("dos cambios seguidos", p, &nm);
    EndNotify(&nm);
    /* NRF_NOTIFY_INITIAL y NRF_WAIT_REPLY. */
    struct NotifyRequest ni = { 0 };
    ni.nr_Name = (STRPTR)"RAM:nuevo.txt";
    ni.nr_Flags = NRF_SEND_MESSAGE | NRF_NOTIFY_INITIAL | NRF_WAIT_REPLY;
    ni.nr_stuff.nr_Msg.nr_Port = p;
    r = StartNotify(&ni);
    escribe("StartNotify con NOTIFY_INITIAL y WAIT_REPLY: %ld\n", r != 0, 0, 0, 0);
    Delay(2);
    struct NotifyMessage *m = (struct NotifyMessage *)GetMsg(p);
    escribe("el aviso inicial: %ld\n", m != NULL, 0, 0, 0);
    pon("RAM:nuevo.txt", "sin contestar");
    pon("RAM:nuevo.txt", "sin contestar 2");
    mensajes("cambios sin contestar el primero", p, &ni);
    if (m)
        ReplyMsg((struct Message *)m);
    pon("RAM:nuevo.txt", "contestado");
    mensajes("tras contestar", p, &ni);
    EndNotify(&ni);
    /* Un cajón. */
    struct NotifyRequest nc = { 0 };
    nc.nr_Name = (STRPTR)"RAM:";
    nc.nr_Flags = NRF_SEND_MESSAGE;
    nc.nr_stuff.nr_Msg.nr_Port = p;
    r = StartNotify(&nc);
    escribe("StartNotify de RAM: %ld\n", r != 0, 0, 0, 0);
    pon("RAM:dentro.txt", "x");
    mensajes("un fichero nuevo dentro", p, &nc);
    EndNotify(&nc);
    DeleteFile((CONST_STRPTR)"RAM:dentro.txt");
    DeleteFile((CONST_STRPTR)"RAM:nuevo.txt");
    DeleteFile((CONST_STRPTR)"RAM:otro.txt");
    DeleteFile((CONST_STRPTR)"RAM:notif.txt");
    DeleteMsgPort(p);
    return 0;
}
