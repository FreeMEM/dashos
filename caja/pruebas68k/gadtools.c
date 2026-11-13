/*
    H4, GadTools en la caja Amiga: una ventana con un gadget de cada tipo (botón, casilla, ciclo, mx, texto, número,
    cadena, entero, deslizador, lista y paleta), un marco de DrawBevelBoxA y menús (CreateMenus, LayoutMenus,
    SetMenuStrip). Comprueba sola GT_SetGadgetAttrs, GT_GetGadgetAttrs e ItemAddress, y después escribe cada evento
    que llega (GT_GetIMsg) hasta que la cierran: con su gadget de cerrar, con "Salir" o con el menú Proyecto > Salir.
    Sin nadie que pulse, se cierra sola a los 40 s (devuelve 5). El guion de la prueba captura la ventana, pulsa la
    casilla, el ciclo, un elemento de la lista y "Aceptar", y elige Proyecto > Salir con el botón derecho.
*/
#include <exec/types.h>
#include <exec/execbase.h>
#include <exec/memory.h>
#include <exec/lists.h>
#include <exec/nodes.h>
#include <intuition/intuition.h>
#include <libraries/gadtools.h>
#include <proto/exec.h>
#include <proto/dos.h>
#include <proto/intuition.h>
#include <proto/graphics.h>
#include <proto/gadtools.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;
struct IntuitionBase *IntuitionBase;
struct GfxBase *GfxBase;
struct Library *GadToolsBase;

enum { G_ACEPTAR, G_SALIR, G_CASILLA, G_CICLO, G_MX, G_TEXTO, G_NUMERO, G_CADENA, G_ENTERO, G_NIVEL, G_LISTA,
       G_PALETA, G_TOTAL };

static void di(const char *f, LONG a, LONG b, LONG c)
{
    LONG v[3] = { a, b, c };
    VPrintf((CONST_STRPTR)f, v);
}

static STRPTR ciclo[] = { (STRPTR)"Uno", (STRPTR)"Dos", (STRPTR)"Tres", NULL };
static STRPTR radio[] = { (STRPTR)"Rojo", (STRPTR)"Verde", (STRPTR)"Azul", NULL };
static struct NewMenu menus[] = {
    { NM_TITLE, (STRPTR)"Proyecto", NULL, 0, 0, NULL },
    { NM_ITEM, (STRPTR)"Abrir...", (STRPTR)"A", 0, 0, (APTR)1 },
    { NM_ITEM, NM_BARLABEL, NULL, 0, 0, NULL },
    { NM_ITEM, (STRPTR)"Salir", (STRPTR)"Q", 0, 0, (APTR)2 },
    { NM_TITLE, (STRPTR)"Editar", NULL, 0, 0, NULL },
    { NM_ITEM, (STRPTR)"Marcado", NULL, CHECKIT | MENUTOGGLE, 0, (APTR)3 },
    { NM_ITEM, (STRPTR)"Apagado", NULL, NM_ITEMDISABLED, 0, (APTR)4 },
    { NM_END, NULL, NULL, 0, 0, NULL },
};
static struct List elementos;
static struct Node nodos[5];
static const char *nombres[5] = { "Amiga 500", "Amiga 1200", "Amiga 4000", "CDTV", "CD32" };

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    if (!DOSBase) return 20;
    IntuitionBase = (struct IntuitionBase *)OpenLibrary((CONST_STRPTR)"intuition.library", 37);
    GfxBase = (struct GfxBase *)OpenLibrary((CONST_STRPTR)"graphics.library", 37);
    GadToolsBase = OpenLibrary((CONST_STRPTR)"gadtools.library", 37);
    if (!IntuitionBase || !GfxBase || !GadToolsBase) {
        di("  [caja-amiga] gadtools: falta una biblioteca\n", 0, 0, 0);
        return 21;
    }
    LONG mal = 0, rc = 5;
    struct Screen *pant = LockPubScreen(NULL);
    APTR vi = pant ? GetVisualInfo(pant, TAG_END) : NULL;
    struct Gadget *lista = NULL, *g, *gad[G_TOTAL];
    g = CreateContext(&lista);
    elementos.lh_Head = (struct Node *)&elementos.lh_Tail;   /* NewList, de amiga.lib */
    elementos.lh_Tail = NULL;
    elementos.lh_TailPred = (struct Node *)&elementos.lh_Head;
    for (int i = 0; i < 5; i++) {
        nodos[i].ln_Name = (char *)nombres[i];
        AddTail(&elementos, &nodos[i]);
    }

    struct NewGadget ng = { 0 };
    ng.ng_VisualInfo = vi;
#define NUEVO(id, tipo, x, y, an, al, texto, lugar, ...)                                                          \
    do {                                                                                                         \
        ng.ng_LeftEdge = x; ng.ng_TopEdge = y; ng.ng_Width = an; ng.ng_Height = al;                               \
        ng.ng_GadgetText = (STRPTR)texto; ng.ng_GadgetID = id; ng.ng_Flags = lugar;                              \
        g = gad[id] = CreateGadget(tipo, g, &ng, __VA_ARGS__);                                                    \
    } while (0)
    NUEVO(G_CASILLA, CHECKBOX_KIND, 90, 20, 26, 11, "Casilla", PLACETEXT_LEFT, GTCB_Checked, TRUE, TAG_END);
    NUEVO(G_CICLO, CYCLE_KIND, 90, 36, 100, 14, "Ciclo", PLACETEXT_LEFT, GTCY_Labels, (ULONG)ciclo, TAG_END);
    NUEVO(G_MX, MX_KIND, 90, 56, 17, 9, "Color", PLACETEXT_LEFT, GTMX_Labels, (ULONG)radio, GTMX_Active, 1,
          GTMX_Spacing, 2, TAG_END);
    NUEVO(G_CADENA, STRING_KIND, 90, 92, 120, 14, "Texto", PLACETEXT_LEFT, GTST_String, (ULONG)"hola",
          GTST_MaxChars, 40, TAG_END);
    NUEVO(G_ENTERO, INTEGER_KIND, 90, 110, 80, 14, "Entero", PLACETEXT_LEFT, GTIN_Number, 42, TAG_END);
    NUEVO(G_NIVEL, SLIDER_KIND, 130, 130, 110, 11, "Nivel", PLACETEXT_LEFT, GTSL_Min, 0, GTSL_Max, 100,
          GTSL_Level, 25, GTSL_LevelFormat, (ULONG)"%ld", GTSL_MaxLevelLen, 3, TAG_END);
    NUEVO(G_TEXTO, TEXT_KIND, 90, 148, 150, 12, "Estado", PLACETEXT_LEFT, GTTX_Text, (ULONG)"Listo",
          GTTX_Border, TRUE, TAG_END);
    NUEVO(G_NUMERO, NUMBER_KIND, 90, 164, 60, 12, "Numero", PLACETEXT_LEFT, GTNM_Number, 1985, GTNM_Border, TRUE,
          TAG_END);
    NUEVO(G_LISTA, LISTVIEW_KIND, 260, 30, 130, 60, "Modelos", PLACETEXT_ABOVE, GTLV_Labels, (ULONG)&elementos,
          GTLV_ShowSelected, 0, GTLV_Selected, 1, TAG_END);
    NUEVO(G_PALETA, PALETTE_KIND, 260, 110, 130, 30, "Paleta", PLACETEXT_ABOVE, GTPA_Depth, 2, GTPA_Color, 1,
          GTPA_IndicatorWidth, 24, TAG_END);
    NUEVO(G_ACEPTAR, BUTTON_KIND, 260, 160, 60, 16, "Aceptar", PLACETEXT_IN, TAG_END);
    NUEVO(G_SALIR, BUTTON_KIND, 330, 160, 60, 16, "Salir", PLACETEXT_IN, TAG_END);
    if (!g) {
        di("  [caja-amiga] gadtools: CreateGadget fallo\n", 0, 0, 0);
        return 22;
    }
    struct Menu *menu = CreateMenus(menus, TAG_END);
    if (!menu || !LayoutMenus(menu, vi, GTMN_NewLookMenus, TRUE, TAG_END)) {
        di("  [caja-amiga] gadtools: CreateMenus o LayoutMenus fallo\n", 0, 0, 0);
        return 23;
    }
    struct Window *v = OpenWindowTags(NULL, WA_Left, 80, WA_Top, 60, WA_Width, 410, WA_Height, 192,
        WA_Title, (ULONG)"GadTools en DashOS", WA_Gadgets, (ULONG)lista, WA_DragBar, TRUE, WA_DepthGadget, TRUE,
        WA_CloseGadget, TRUE, WA_Activate, TRUE, WA_PubScreen, (ULONG)pant,
        WA_IDCMP, IDCMP_CLOSEWINDOW | IDCMP_GADGETUP | IDCMP_GADGETDOWN | IDCMP_MOUSEMOVE | IDCMP_MENUPICK
                  | IDCMP_REFRESHWINDOW | IDCMP_VANILLAKEY, TAG_END);
    if (!v) {
        di("  [caja-amiga] gadtools: OpenWindowTags fallo\n", 0, 0, 0);
        return 24;
    }
    /* Por si WA_Gadgets no los ha puesto: AddGList, como hacen muchos programas. */
    if (!v->FirstGadget) AddGList(v, lista, -1, -1, NULL);
    SetMenuStrip(v, menu);
    RefreshGList(lista, v, NULL, -1);
    GT_RefreshWindow(v, NULL);
    DrawBevelBox(v->RPort, 250, 16, 150, 132, GT_VisualInfo, (ULONG)vi, GTBB_Recessed, TRUE, TAG_END);

    /* Lo que el programa comprueba solo: atributos de ida y vuelta, ItemAddress y GTMENUITEM_USERDATA. */
    LONG nivel = -1, activo = -1, marcado = -1, numero = -1;
    GT_SetGadgetAttrs(gad[G_NIVEL], v, NULL, GTSL_Level, 60, TAG_END);
    GT_SetGadgetAttrs(gad[G_CICLO], v, NULL, GTCY_Active, 2, TAG_END);
    GT_GetGadgetAttrs(gad[G_NIVEL], v, NULL, GTSL_Level, (ULONG)&nivel, TAG_END);
    GT_GetGadgetAttrs(gad[G_CICLO], v, NULL, GTCY_Active, (ULONG)&activo, TAG_END);
    GT_GetGadgetAttrs(gad[G_CASILLA], v, NULL, GTCB_Checked, (ULONG)&marcado, TAG_END);
    GT_GetGadgetAttrs(gad[G_ENTERO], v, NULL, GTIN_Number, (ULONG)&numero, TAG_END);
    struct MenuItem *salir = ItemAddress(menu, FULLMENUNUM(0, 2, NOSUB));
    LONG ud = salir ? (LONG)GTMENUITEM_USERDATA(salir) : -1;
    struct StringInfo *si = (struct StringInfo *)gad[G_CADENA]->SpecialInfo;
    BOOL bien = nivel == 60 && activo == 2 && marcado == 1 && numero == 42 && ud == 2 && si
                && si->Buffer[0] == 'h' && gad[G_ACEPTAR]->GadgetID == G_ACEPTAR;
    di("  [caja-amiga] gadtools: atributos (nivel %ld, ciclo %ld, casilla %ld), ", nivel, activo, marcado);
    di("entero %ld, ItemAddress y UserData del menu %ld: %s\n", numero, ud, (LONG)(bien ? "bien" : "MAL"));
    if (!bien) mal++;
    GT_SetGadgetAttrs(gad[G_NIVEL], v, NULL, GTSL_Level, 25, TAG_END);
    GT_SetGadgetAttrs(gad[G_CICLO], v, NULL, GTCY_Active, 0, TAG_END);
    di("  [caja-amiga] gadtools: ventana abierta y dibujada (%ld gadgets); espera clics\n", G_TOTAL, 0, 0);

    BOOL seguir = TRUE;
    LONG vueltas = 0;
    while (seguir && vueltas < 40 * 25) {
        struct IntuiMessage *m;
        while ((m = GT_GetIMsg(v->UserPort))) {
            ULONG clase = m->Class;
            UWORD cod = m->Code;
            struct Gadget *ga = (struct Gadget *)m->IAddress;
            GT_ReplyIMsg(m);
            if (clase == IDCMP_CLOSEWINDOW) {
                di("  [caja-amiga] gadtools: IDCMP_CLOSEWINDOW\n", 0, 0, 0);
                seguir = FALSE;
                rc = 0;
            } else if (clase == IDCMP_GADGETUP || clase == IDCMP_GADGETDOWN) {
                di("  [caja-amiga] gadtools: %s del gadget %ld, codigo %ld\n",
                   (LONG)(clase == IDCMP_GADGETUP ? "GADGETUP" : "GADGETDOWN"), ga->GadgetID, cod);
                if (ga->GadgetID == G_ACEPTAR)
                    GT_SetGadgetAttrs(gad[G_TEXTO], v, NULL, GTTX_Text, (ULONG)"Aceptado", TAG_END);
                if (ga->GadgetID == G_SALIR) {
                    seguir = FALSE;
                    rc = 0;
                }
                if (ga->GadgetID == G_CADENA)
                    di("  [caja-amiga] gadtools: el texto es \"%s\"\n", (LONG)si->Buffer, 0, 0);
            } else if (clase == IDCMP_MENUPICK) {
                if (cod == MENUNULL) {
                    di("  [caja-amiga] gadtools: MENUPICK sin elegir nada\n", 0, 0, 0);
                }
                while (cod != MENUNULL) {
                    struct MenuItem *it = ItemAddress(menu, cod);
                    LONG id = it ? (LONG)GTMENUITEM_USERDATA(it) : -1;
                    di("  [caja-amiga] gadtools: MENUPICK menu %ld elemento %ld (%ld)\n", MENUNUM(cod), ITEMNUM(cod), id);
                    if (id == 2) {
                        seguir = FALSE;
                        rc = 0;
                    }
                    cod = it ? it->NextSelect : MENUNULL;
                }
            } else if (clase == IDCMP_REFRESHWINDOW) {
                GT_BeginRefresh(v);
                GT_EndRefresh(v, TRUE);
            }
        }
        if (seguir) {
            WaitTOF();
            Delay(1);
            vueltas++;
        }
    }
    if (seguir) di("  [caja-amiga] gadtools: nadie la cerro en 40 s; se cierra sola\n", 0, 0, 0);
    ClearMenuStrip(v);
    CloseWindow(v);
    FreeMenus(menu);
    FreeGadgets(lista);
    FreeVisualInfo(vi);
    UnlockPubScreen(NULL, pant);
    CloseLibrary(GadToolsBase);
    CloseLibrary((struct Library *)GfxBase);
    CloseLibrary((struct Library *)IntuitionBase);
    CloseLibrary((struct Library *)DOSBase);
    return mal ? 10 : rc;
}
