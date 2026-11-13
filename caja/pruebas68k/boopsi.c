/*
    BOOPSI y ReAction en la caja Amiga (y en FS-UAE con AmigaOS 3.2, el oráculo): sin amiga.lib, todo a pelo.

    1. Una clase de usuario ("contador", subclase de gadgetclass) con su despachador en 68k: OM_NEW con
       DoSuperMethodA, un atributo propio (OM_SET, OM_GET), un método propio y GM_RENDER que pinta con graphics.
    2. Interconexión: propgclass → strgclass con ICA_TARGET e ICA_MAP (PGA_Top → STRINGA_LongVal); un icclass y un
       modelclass en medio.
    3. ReAction: window.class con un layout, un botón, una casilla, un campo de texto y un deslizador; WM_OPEN,
       GetAttr y SetGadgetAttrs, WM_HANDLEINPUT hasta WMHI_LASTMSG, WM_CLOSE y DisposeObject.
    Escribe cada comprobación y acaba con 0 si todas van bien (y con la ventana de ReAction abierta N segundos si el
    nombre del proceso lleva "-N", para las capturas).
*/
#include <exec/types.h>
#include <exec/execbase.h>
#include <intuition/intuition.h>
#include <intuition/classes.h>
#include <intuition/classusr.h>
#include <intuition/gadgetclass.h>
#include <intuition/icclass.h>
#include <intuition/imageclass.h>
#include <utility/tagitem.h>
#include <utility/hooks.h>
#include <classes/window.h>
#include <gadgets/layout.h>
#include <gadgets/button.h>
#include <gadgets/checkbox.h>
#include <gadgets/slider.h>
#include <images/label.h>
#include <proto/exec.h>
#include <proto/dos.h>
#include <proto/intuition.h>
#include <proto/graphics.h>
#include <proto/utility.h>
#include <proto/window.h>
#include <proto/layout.h>
#include <proto/button.h>
#include <proto/checkbox.h>
#include <proto/slider.h>
#include <proto/string.h>
#include <proto/label.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;
struct IntuitionBase *IntuitionBase;
struct GfxBase *GfxBase;
struct Library *UtilityBase;
struct Library *WindowBase, *LayoutBase, *ButtonBase, *CheckBoxBase, *SliderBase, *StringBase, *LabelBase;

static LONG fallos;

static void di(const char *f, LONG a, LONG b, LONG c)
{
    LONG v[3] = { a, b, c };
    VPrintf((CONST_STRPTR)f, v);
}


static void mira(const char *que, LONG tiene, LONG debe)
{
    if (tiene == debe) di("  [caja-amiga] boopsi: %s = %ld: bien\n", (LONG)que, tiene, 0);
    else {
        di("  [caja-amiga] boopsi: %s = %ld (se esperaba %ld): MAL\n", (LONG)que, tiene, debe);
        fallos++;
    }
}

/* DoMethodA y DoSuperMethodA de amiga.lib: el gancho despachador de la clase (CallHookPkt salta a su h_Entry). */
static ULONG do_method(Object *o, Msg m)
{
    return CallHookPkt((struct Hook *)OCLASS(o), o, m);
}

static ULONG do_super(Class *cl, Object *o, Msg m)
{
    return CallHookPkt((struct Hook *)cl->cl_Super, o, m);
}

/* --- 1. La clase "contador" --- */

#define CONT_Valor (TAG_USER + 0x7701)
#define CONT_SUMA  (0x7702)

struct Contador { LONG valor; LONG pintado; };

static ULONG disp_contador(Class *cl __asm("a0"), Object *o __asm("a2"), Msg msg __asm("a1"))
{
    struct Contador *c;
    switch (msg->MethodID) {
    case OM_NEW: {
        Object *n = (Object *)do_super(cl, o, msg);
        if (n) {
            c = INST_DATA(cl, n);
            c->valor = GetTagData(CONT_Valor, 1, ((struct opSet *)msg)->ops_AttrList);
            c->pintado = 0;
        }
        return (ULONG)n;
    }
    case OM_SET: {
        ULONG r = do_super(cl, o, msg);
        struct TagItem *t = FindTagItem(CONT_Valor, ((struct opSet *)msg)->ops_AttrList);
        if (t) { c = INST_DATA(cl, o); c->valor = t->ti_Data; r = 1; }
        return r;
    }
    case OM_GET:
        if (((struct opGet *)msg)->opg_AttrID == CONT_Valor) {
            c = INST_DATA(cl, o);
            *((struct opGet *)msg)->opg_Storage = c->valor;
            return 1;
        }
        return do_super(cl, o, msg);
    case CONT_SUMA:
        c = INST_DATA(cl, o);
        return (ULONG)(c->valor += 1);
    case GM_RENDER: {
        struct gpRender *r = (struct gpRender *)msg;
        struct Gadget *g = (struct Gadget *)o;
        c = INST_DATA(cl, o);
        c->pintado++;
        SetAPen(r->gpr_RPort, 3);
        RectFill(r->gpr_RPort, g->LeftEdge, g->TopEdge, g->LeftEdge + g->Width - 1, g->TopEdge + g->Height - 1);
        return 0;
    }
    }
    return do_super(cl, o, msg);
}

/* --- La entrada --- */

static struct TagItem mapa_prop[] = { { PGA_Top, STRINGA_LongVal }, { TAG_DONE, 0 } };

static ULONG get(Object *o, ULONG attr)
{
    ULONG v = 0xDEAD;
    GetAttr(attr, o, &v);
    return v;
}

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    if (!DOSBase) return 20;
    IntuitionBase = (struct IntuitionBase *)OpenLibrary((CONST_STRPTR)"intuition.library", 39);
    GfxBase = (struct GfxBase *)OpenLibrary((CONST_STRPTR)"graphics.library", 37);
    UtilityBase = OpenLibrary((CONST_STRPTR)"utility.library", 37);
    if (!IntuitionBase || !GfxBase || !UtilityBase) {
        di("  [caja-amiga] boopsi: falta una biblioteca\n", 0, 0, 0);
        return 21;
    }
    LONG segundos = 0;
    {
        /* Los segundos, tras el último '-' del nombre ("boopsi-90", "boopsi-orig-90"). */
        const char *p = FindTask(NULL)->tc_Node.ln_Name, *u = NULL;
        for (; *p; p++) if (*p == '-') u = p;
        if (u) for (u++; *u >= '0' && *u <= '9'; u++) segundos = segundos * 10 + (*u - '0');
    }

    /* 1. La clase de usuario. */
    Class *cl = MakeClass(NULL, (ClassID)"gadgetclass", NULL, sizeof(struct Contador), 0);
    mira("MakeClass", cl != NULL, 1);
    if (!cl) return 22;
    *(APTR *)&cl->cl_Dispatcher.h_Entry = (APTR)disp_contador;
    struct TagItem t1[] = { { GA_Left, 200 }, { GA_Top, 30 }, { GA_Width, 40 }, { GA_Height, 12 }, { GA_ID, 9 },
                            { CONT_Valor, 40 }, { TAG_DONE, 0 } };
    Object *cont = NewObjectA(cl, NULL, t1);
    mira("NewObject(contador)", cont != NULL, 1);
    mira("OCLASS(contador) es la suya", cont && OCLASS(cont) == cl, 1);
    mira("GA_ID por la superclase", get(cont, GA_ID), 9);
    mira("CONT_Valor", get(cont, CONT_Valor), 40);
    ULONG suma = CONT_SUMA;
    mira("CONT_SUMA", do_method(cont, (Msg)&suma), 41);
    struct TagItem t2[] = { { CONT_Valor, 7 }, { TAG_DONE, 0 } };
    SetAttrsA(cont, t2);
    mira("CONT_Valor tras SetAttrs", get(cont, CONT_Valor), 7);
    mira("FreeClass con un objeto vivo", FreeClass(cl), 0);

    /* 2. Interconexión. */
    struct TagItem ts[] = { { GA_Left, 20 }, { GA_Top, 50 }, { GA_Width, 100 }, { GA_Height, 14 }, { GA_ID, 2 },
                            { STRINGA_MaxChars, 12 }, { STRINGA_LongVal, 0 }, { TAG_DONE, 0 } };
    Object *cad = NewObjectA(NULL, (ClassID)"strgclass", ts);
    struct TagItem tp[] = { { GA_Left, 20 }, { GA_Top, 30 }, { GA_Width, 150 }, { GA_Height, 12 }, { GA_ID, 1 },
                            { PGA_Freedom, FREEHORIZ }, { PGA_Total, 100 }, { PGA_Visible, 10 },
                            { ICA_TARGET, (ULONG)cad }, { ICA_MAP, (ULONG)mapa_prop }, { GA_Previous, 0 },
                            { TAG_DONE, 0 } };
    Object *prop = NewObjectA(NULL, (ClassID)"propgclass", tp);
    mira("propgclass y strgclass", prop && cad, 1);
    /* OM_UPDATE al deslizador (lo que mandaría otro objeto) lo cambia, pero propgclass no lo notifica: solo notifica
       lo que mueve el usuario (el oráculo, 3.2, dice lo mismo). */
    struct TagItem tu[] = { { PGA_Top, 33 }, { TAG_DONE, 0 } };
    struct opUpdate up = { OM_UPDATE, tu, NULL, 0 };
    do_method(prop, (Msg)&up);
    mira("PGA_Top tras OM_UPDATE", get(prop, PGA_Top), 33);
    mira("STRINGA_LongVal sin notificar", get(cad, STRINGA_LongVal), 0);
    /* icclass: lo que le llega pasa a su objetivo (con su mapa). */
    struct TagItem ti[] = { { ICA_TARGET, (ULONG)cad }, { ICA_MAP, (ULONG)mapa_prop }, { TAG_DONE, 0 } };
    Object *ic = NewObjectA(NULL, (ClassID)"icclass", ti);
    struct TagItem tu2[] = { { PGA_Top, 51 }, { TAG_DONE, 0 } };
    struct opUpdate up2 = { OM_UPDATE, tu2, NULL, 0 };
    do_method(ic, (Msg)&up2);
    mira("icclass → strgclass", get(cad, STRINGA_LongVal), 51);
    /* modelclass: a sus miembros. */
    struct TagItem tn[] = { { TAG_DONE, 0 } };
    Object *modelo = NewObjectA(NULL, (ClassID)"modelclass", tn);
    struct TagItem ti2[] = { { ICA_TARGET, (ULONG)cad }, { ICA_MAP, (ULONG)mapa_prop }, { TAG_DONE, 0 } };
    Object *ic2 = NewObjectA(NULL, (ClassID)"icclass", ti2);
    struct opMember mm = { OM_ADDMEMBER, ic2 };
    do_method(modelo, (Msg)&mm);
    struct TagItem tu3[] = { { PGA_Top, 64 }, { TAG_DONE, 0 } };
    struct opUpdate up3 = { OM_UPDATE, tu3, NULL, 0 };
    do_method(modelo, (Msg)&up3);
    mira("modelclass → icclass → strgclass", get(cad, STRINGA_LongVal), 64);
    /* En una ventana: el contador se pinta (GM_RENDER del 68k) con RefreshGList. */
    struct TagItem tw[] = { { WA_Width, 300 }, { WA_Height, 90 }, { WA_Title, (ULONG)"BOOPSI" }, { WA_DragBar, TRUE },
                            { WA_CloseGadget, TRUE }, { WA_IDCMP, IDCMP_GADGETUP | IDCMP_CLOSEWINDOW },
                            { TAG_DONE, 0 } };
    struct Window *w = OpenWindowTagList(NULL, tw);
    mira("OpenWindowTagList", w != NULL, 1);
    if (w) {
        ((struct Gadget *)prop)->NextGadget = (struct Gadget *)cad;
        ((struct Gadget *)cad)->NextGadget = (struct Gadget *)cont;
        AddGList(w, (struct Gadget *)prop, -1, 3, NULL);
        RefreshGList((struct Gadget *)prop, w, NULL, 3);
        Delay(5);
        mira("GM_RENDER del contador", ((struct Contador *)INST_DATA(cl, cont))->pintado > 0, 1);
        mira("el contador pintado", ReadPixel(w->RPort, 210, 35), 3);
        struct TagItem tg[] = { { PGA_Top, 80 }, { TAG_DONE, 0 } };
        SetGadgetAttrsA((struct Gadget *)prop, w, NULL, tg);
        mira("PGA_Top tras SetGadgetAttrs", get(prop, PGA_Top), 80);
        mira("SetGadgetAttrs del deslizador no notifica", get(cad, STRINGA_LongVal), 64);
        RemoveGList(w, (struct Gadget *)prop, 3);
        CloseWindow(w);
    }
    DisposeObject(modelo);
    DisposeObject(prop);
    DisposeObject(cad);
    DisposeObject(ic);
    DisposeObject(cont);
    mira("FreeClass sin objetos", FreeClass(cl), 1);

    /* 3. ReAction. */
    WindowBase = OpenLibrary((CONST_STRPTR)"window.class", 44);
    LayoutBase = OpenLibrary((CONST_STRPTR)"gadgets/layout.gadget", 44);
    ButtonBase = OpenLibrary((CONST_STRPTR)"gadgets/button.gadget", 44);
    CheckBoxBase = OpenLibrary((CONST_STRPTR)"gadgets/checkbox.gadget", 44);
    SliderBase = OpenLibrary((CONST_STRPTR)"gadgets/slider.gadget", 44);
    StringBase = OpenLibrary((CONST_STRPTR)"gadgets/string.gadget", 44);
    LabelBase = OpenLibrary((CONST_STRPTR)"images/label.image", 44);
    mira("clases de ReAction", WindowBase && LayoutBase && ButtonBase && CheckBoxBase && SliderBase && StringBase
                                   && LabelBase, 1);
    if (WindowBase && LayoutBase && ButtonBase && CheckBoxBase && SliderBase && StringBase && LabelBase) {
        struct TagItem tl[] = { { LAYOUT_Orientation, LAYOUT_ORIENT_VERT }, { LAYOUT_SpaceOuter, TRUE },
                                { LAYOUT_BevelStyle, BVS_GROUP }, { LAYOUT_Label, (ULONG)"BOOPSI y ReAction" },
                                { TAG_DONE, 0 } };
        Object *layout = NewObjectA(LAYOUT_GetClass(), NULL, tl);
        struct TagItem tc[] = { { GA_ID, 11 }, { GA_RelVerify, TRUE }, { GA_Text, (ULONG)"Sonido" },
                                { GA_Selected, TRUE }, { TAG_DONE, 0 } };
        Object *casilla = NewObjectA(CHECKBOX_GetClass(), NULL, tc);
        struct TagItem tst[] = { { GA_ID, 12 }, { GA_RelVerify, TRUE }, { STRINGA_TextVal, (ULONG)"Amiga" },
                                 { STRINGA_MaxChars, 40 }, { TAG_DONE, 0 } };
        Object *campo = NewObjectA(STRING_GetClass(), NULL, tst);
        struct TagItem tla[] = { { LABEL_Text, (ULONG)"_Nombre" }, { TAG_DONE, 0 } };
        Object *etiqueta = NewObjectA(LABEL_GetClass(), NULL, tla);
        struct TagItem tsl[] = { { GA_ID, 13 }, { GA_RelVerify, TRUE }, { SLIDER_Min, 0 }, { SLIDER_Max, 10 },
                                 { SLIDER_Level, 4 }, { SLIDER_Orientation, SLIDER_HORIZONTAL }, { TAG_DONE, 0 } };
        Object *desl = NewObjectA(SLIDER_GetClass(), NULL, tsl);
        struct TagItem tb[] = { { GA_ID, 14 }, { GA_RelVerify, TRUE }, { GA_Text, (ULONG)"_Aceptar" }, { TAG_DONE, 0 } };
        Object *boton = NewObjectA(BUTTON_GetClass(), NULL, tb);
        mira("layout.gadget", layout != NULL, 1);
        mira("checkbox.gadget", casilla != NULL, 1);
        mira("string.gadget", campo != NULL, 1);
        mira("label.image", etiqueta != NULL, 1);
        mira("slider.gadget", desl != NULL, 1);
        mira("button.gadget", boton != NULL, 1);
        struct TagItem ta[] = { { LAYOUT_AddChild, (ULONG)campo }, { CHILD_Label, (ULONG)etiqueta },
                                { LAYOUT_AddChild, (ULONG)casilla }, { LAYOUT_AddChild, (ULONG)desl },
                                { LAYOUT_AddChild, (ULONG)boton }, { CHILD_WeightedHeight, 0 }, { TAG_DONE, 0 } };
        SetAttrsA(layout, ta);
        struct TagItem tv[] = { { WA_Title, (ULONG)"ReAction" }, { WA_Width, 240 }, { WA_Height, 60 },
                                { WA_DragBar, TRUE }, { WA_CloseGadget, TRUE }, { WA_DepthGadget, TRUE },
                                { WA_SizeGadget, TRUE }, { WA_Activate, TRUE },
                                { WINDOW_Position, WPOS_CENTERSCREEN }, { WINDOW_Layout, (ULONG)layout },
                                { TAG_DONE, 0 } };
        Object *ventana = NewObjectA(WINDOW_GetClass(), NULL, tv);
        ULONG abrir = WM_OPEN;
        struct Window *rw = ventana ? (struct Window *)do_method(ventana, (Msg)&abrir) : NULL;
        mira("WM_OPEN", rw != NULL, 1);
        if (rw) {
            mira("WINDOW_Window", get(ventana, WINDOW_Window) == (ULONG)rw, 1);
            mira("WINDOW_SigMask", get(ventana, WINDOW_SigMask) == (1UL << rw->UserPort->mp_SigBit), 1);
            struct Gadget *b = (struct Gadget *)boton, *s = (struct Gadget *)campo;
            mira("el botón dentro de la ventana", b->LeftEdge >= 0 && b->TopEdge >= 0
                 && b->LeftEdge + b->Width <= rw->Width && b->TopEdge + b->Height <= rw->Height, 1);
            mira("el botón cabe su texto", b->Width >= 7 * 8, 1);
            mira("el campo a la derecha de su etiqueta", s->LeftEdge >= rw->BorderLeft + 6 * 8, 1);
            mira("el campo encima del botón", s->TopEdge + s->Height <= b->TopEdge, 1);
            mira("la ventana crece hasta el layout", rw->Height > 60, 1);
            mira("GA_Selected de la casilla", get(casilla, GA_Selected), 1);
            struct TagItem tx[] = { { GA_Selected, FALSE }, { TAG_DONE, 0 } };
            SetGadgetAttrsA((struct Gadget *)casilla, rw, NULL, tx);
            mira("GA_Selected tras SetGadgetAttrs", get(casilla, GA_Selected), 0);
            mira("SLIDER_Level", get(desl, SLIDER_Level), 4);
            STRPTR texto = (STRPTR)get(campo, STRINGA_TextVal);
            mira("STRINGA_TextVal", texto && texto[0] == 'A' && texto[4] == 'a' && texto[5] == 0, 1);
            /* Lo que haya llegado (activar la ventana...): hasta WMHI_LASTMSG, sin colgarse. */
            WORD code;
            struct wmHandle h = { WM_HANDLEINPUT, &code };
            LONG n = 0;
            while (do_method(ventana, (Msg)&h) != WMHI_LASTMSG && n < 100) n++;
            mira("WM_HANDLEINPUT acaba", n < 100, 1);
            /* Abierta un rato (las capturas): cerrar con su botón de cerrar acaba antes. */
            for (LONG k = 0; k < segundos * 10; k++) {
                ULONG r, fin = 0;
                Delay(5);
                while ((r = do_method(ventana, (Msg)&h)) != WMHI_LASTMSG)
                    if ((r & WMHI_CLASSMASK) == WMHI_CLOSEWINDOW) fin = 1;
                if (fin) break;
            }
            ULONG cerrar = WM_CLOSE;
            do_method(ventana, (Msg)&cerrar);
            mira("WM_CLOSE", get(ventana, WINDOW_Window), 0);
        }
        DisposeObject(ventana);   /* con el layout y todo lo suyo */
    }
    if (LabelBase) CloseLibrary(LabelBase);
    if (StringBase) CloseLibrary(StringBase);
    if (SliderBase) CloseLibrary(SliderBase);
    if (CheckBoxBase) CloseLibrary(CheckBoxBase);
    if (ButtonBase) CloseLibrary(ButtonBase);
    if (LayoutBase) CloseLibrary(LayoutBase);
    if (WindowBase) CloseLibrary(WindowBase);

    di("  [caja-amiga] boopsi: %ld fallos\n", fallos, 0, 0);
    CloseLibrary(UtilityBase);
    CloseLibrary((struct Library *)GfxBase);
    CloseLibrary((struct Library *)IntuitionBase);
    CloseLibrary((struct Library *)DOSBase);
    return fallos ? 10 : 0;
}
