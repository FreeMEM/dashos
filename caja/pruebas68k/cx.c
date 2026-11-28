/*
    La caja Amiga: commodities.library, de lo que pide MUI al arrancar (su broker, su filtro de tecla) y Exchange. Es una
    prueba del oráculo: la misma salida en la caja y en un Amiga de verdad (FS-UAE con el Kickstart; tools/emu_run.py
    de Dash). CxBroker (bien, repetido con NBU_UNIQUE y NBU_NOTIFY, versión mala), los objetos y sus errores, la
    prioridad, activar, el mensaje CXCMD_UNIQUE que recibe el primero, y ParseIX con muchas descripciones.
*/
#include <exec/types.h>
#include <exec/ports.h>
#include <exec/execbase.h>
#include <dos/dos.h>
#include <libraries/commodities.h>
#include <devices/inputevent.h>
#include <proto/exec.h>
#include <proto/dos.h>
#include <proto/commodities.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;
struct Library *CxBase;

static void escribe(const char *f, LONG a, LONG b, LONG c, LONG d)
{
    LONG v[4] = { a, b, c, d };
    VPrintf((CONST_STRPTR)f, v);
}

static void ix(const char *texto)
{
    IX x;
    UBYTE *p = (UBYTE *)&x;
    for (unsigned k = 0; k < sizeof x; k++) p[k] = 0xAA;
    LONG r = ParseIX((CONST_STRPTR)texto, &x);
    escribe("ParseIX [%s]: %ld", (LONG)texto, r, 0, 0);
    escribe(" v%ld clase %ld codigo %lx/%lx", x.ix_Version, x.ix_Class, x.ix_Code, x.ix_CodeMask);
    escribe(" cualif %lx/%lx igual %lx\n", x.ix_Qualifier, x.ix_QualMask, x.ix_QualSame, 0);
}

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 37);
    CxBase = OpenLibrary((CONST_STRPTR)"commodities.library", 37);
    if (!DOSBase || !CxBase)
        return 20;
    struct MsgPort *puerto = CreateMsgPort();
    if (!puerto)
        return 20;
    struct NewBroker nb = { NB_VERSION, (BYTE *)(UBYTE *)"cx-prueba", (BYTE *)(UBYTE *)"Prueba de la caja", (BYTE *)(UBYTE *)"commodities.library",
                            NBU_UNIQUE | NBU_NOTIFY, COF_SHOW_HIDE, 0, puerto, 0 };
    LONG err = -1;
    CxObj *b = CxBroker(&nb, &err);
    escribe("CxBroker: %ld, error %ld, tipo %ld, CxObjError %ld\n", b != NULL, err, b ? CxObjType(b) : -1,
            b ? CxObjError(b) : -1);
    if (!b)
        return 20;
    /* El mismo nombre otra vez: NBU_UNIQUE lo rechaza y NBU_NOTIFY avisa al primero. */
    struct MsgPort *otro = CreateMsgPort();
    nb.nb_Port = otro;
    err = -1;
    CxObj *b2 = CxBroker(&nb, &err);
    escribe("CxBroker repetido: %ld, error %ld\n", b2 != NULL, err, 0, 0);
    if (b2)
        DeleteCxObjAll(b2);
    CxMsg *m = (CxMsg *)GetMsg(puerto);
    escribe("aviso al primero: %ld", m != NULL, 0, 0, 0);
    if (m) {
        escribe(" tipo %ld id %ld", CxMsgType(m), CxMsgID(m), 0, 0);
        ReplyMsg((struct Message *)m);
    }
    PutStr((CONST_STRPTR)"\n");
    escribe("y nada mas en el otro puerto: %ld\n", GetMsg(otro) == NULL, 0, 0, 0);
    /* Versión mala. */
    nb.nb_Version = 99;
    nb.nb_Name = (BYTE *)"cx-otra";
    err = -1;
    b2 = CxBroker(&nb, &err);
    escribe("CxBroker version 99: %ld, error %ld\n", b2 != NULL, err, 0, 0);
    if (b2)
        DeleteCxObjAll(b2);
    /* Sin error: un puntero nulo vale. */
    nb.nb_Version = NB_VERSION;
    b2 = CxBroker(&nb, NULL);
    escribe("CxBroker sin error: %ld\n", b2 != NULL, 0, 0, 0);
    if (b2)
        DeleteCxObjAll(b2);
    /* Objetos. */
    CxObj *f = CxFilter("ctrl alt x");
    escribe("CxFilter bueno: %ld, tipo %ld, error %ld\n", f != NULL, f ? CxObjType(f) : -1, f ? CxObjError(f) : -1, 0);
    CxObj *fm = CxFilter("esto no es nada");
    escribe("CxFilter malo: %ld, tipo %ld, error %ld\n", fm != NULL, fm ? CxObjType(fm) : -1, fm ? CxObjError(fm) : -1, 0);
    CxObj *s = CxSender(puerto, 7);
    escribe("CxSender: %ld, tipo %ld\n", s != NULL, s ? CxObjType(s) : -1, 0, 0);
    CxObj *t = CxTranslate(NULL);
    escribe("CxTranslate: %ld, tipo %ld\n", t != NULL, t ? CxObjType(t) : -1, 0, 0);
    CxObj *sg = CxSignal(FindTask(NULL), SIGBREAKB_CTRL_F);
    escribe("CxSignal: %ld, tipo %ld\n", sg != NULL, sg ? CxObjType(sg) : -1, 0, 0);
    AttachCxObj(f, s);
    AttachCxObj(f, t);
    AttachCxObj(b, f);
    escribe("tras enganchar: error del broker %ld, del filtro %ld\n", CxObjError(b), CxObjError(f), 0, 0);
    AttachCxObj(b, NULL);
    escribe("enganchar NULL: error del broker %ld\n", CxObjError(b), 0, 0, 0);
    ClearCxObjError(b);
    escribe("ClearCxObjError: %ld\n", CxObjError(b), 0, 0, 0);
    AttachCxObj(b, fm);
    escribe("enganchar el filtro malo: error del broker %ld\n", CxObjError(b), 0, 0, 0);
    ClearCxObjError(b);
    /* Una llamada por línea: GCC evalúa los argumentos de derecha a izquierda. */
    LONG p1 = SetCxObjPri(b, 5);
    LONG p2 = SetCxObjPri(b, -3);
    escribe("SetCxObjPri: %ld, %ld\n", p1, p2, 0, 0);
    LONG a1 = ActivateCxObj(b, FALSE);
    LONG a2 = ActivateCxObj(b, TRUE);
    LONG a3 = ActivateCxObj(b, TRUE);
    LONG a4 = ActivateCxObj(b, FALSE);
    escribe("ActivateCxObj: %ld, %ld, %ld, %ld\n", a1, a2, a3, a4);
    escribe("CxObjType(NULL) %ld, CxObjError(NULL) %ld\n", CxObjType(NULL), CxObjError(NULL), 0, 0);
    /* SetFilter en uno que ya hay. */
    SetFilter(f, "lshift f1");
    escribe("SetFilter bueno: error %ld\n", CxObjError(f), 0, 0, 0);
    SetFilter(f, "nada de nada");
    escribe("SetFilter malo: error %ld\n", CxObjError(f), 0, 0, 0);
    DeleteCxObjAll(b);
    DeleteCxObj(sg);
    escribe("sin mensajes en el puerto: %ld\n", GetMsg(puerto) == NULL, 0, 0, 0);
    /* ParseIX. */
    ix("ctrl alt x");
    ix("lshift f1");
    ix("rawkey control a");
    ix("-repeat lcommand j");
    ix("help");
    ix("upstroke a");
    ix("-upstroke a");
    ix("shift caps tab");
    ix("alt esc");
    ix("numericpad 5");
    ix("rawmouse leftbutton");
    ix("rawmouse mouse_leftpress");
    ix("diskinserted");
    ix("newprefs");
    ix("rawkey 0x40");
    ix("rawkey space");
    ix("A");
    ix("CTRL ALT X");
    ix("");
    ix("bogus");
    ix("ctrl");
    ix("ctrl alt");
    ix("f11");
    ix("rbutton midbutton f10");
    ix("relativemouse up");
    /* MatchIX. */
    IX x;
    ParseIX((CONST_STRPTR)"ctrl alt x", &x);
    static const UWORD cualifs[] = { 0x18, 0x28, 0x38, 0x08, 0x218, 0x19, 0x8018, 0x418 };
    for (unsigned k = 0; k < sizeof cualifs / 2; k++) {
        struct InputEvent ie = { 0 };
        ie.ie_Class = IECLASS_RAWKEY;
        ie.ie_Code = 0x32;
        ie.ie_Qualifier = cualifs[k];
        escribe("MatchIX ctrl alt x, cualif %lx: %ld\n", cualifs[k], MatchIX(&ie, &x), 0, 0);
    }
    {
        struct InputEvent ie = { 0 };
        ie.ie_Class = IECLASS_RAWKEY;
        ie.ie_Code = 0xB2;
        ie.ie_Qualifier = 0x18;
        escribe("MatchIX con la tecla soltada: %ld\n", MatchIX(&ie, &x), 0, 0, 0);
        ie.ie_Class = IECLASS_RAWMOUSE;
        ie.ie_Code = 0x32;
        escribe("MatchIX con otra clase: %ld\n", MatchIX(&ie, &x), 0, 0, 0);
    }
    ix("-upstroke shift a");
    ParseIX((CONST_STRPTR)"shift -upstroke a", &x);
    {
        struct InputEvent ie = { 0 };
        ie.ie_Class = IECLASS_RAWKEY;
        ie.ie_Code = 0xA0;
        ie.ie_Qualifier = 0x2;
        escribe("MatchIX shift -upstroke a, soltada con rshift: %ld\n", MatchIX(&ie, &x), 0, 0, 0);
        ie.ie_Qualifier = 0;
        escribe("MatchIX shift -upstroke a, sin shift: %ld\n", MatchIX(&ie, &x), 0, 0, 0);
    }
    /* InvertKeyMap con el mapa de serie. */
    /* Sin letras con acento: en el mapa de serie van con tecla muerta (InvertKeyMap da la letra sola). */
    static const char letras[] = "xX5a!~ \n\r\t\033\177\001";
    for (unsigned k = 0; k < sizeof letras - 1; k++) {
        struct InputEvent ie = { 0 };
        LONG r = InvertKeyMap((ULONG)(UBYTE)letras[k], &ie, NULL);
        escribe("InvertKeyMap %ld: %ld clase %ld", (UBYTE)letras[k], r, ie.ie_Class, 0);
        escribe(" codigo %lx cualif %lx\n", ie.ie_Code, ie.ie_Qualifier, 0, 0);
    }
    DeleteMsgPort(otro);
    DeleteMsgPort(puerto);
    CloseLibrary(CxBase);
    return 0;
}
