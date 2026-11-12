/*
    H5 en la caja Amiga: una ventana de Intuition en el escritorio de DashOS, desde un ejecutable de AmigaOS hecho con
    m68k-amigaos-gcc (la cadena de Dash).

    Abre dos, intuition y graphics con OpenLibrary; lee SYS:LEEME.txt por dos (el servicio de archivos de DashOS debajo);
    abre una ventana con OpenWindowTags (título, gadgets de cerrar, arrastre y profundidad, IDCMP_CLOSEWINDOW); prueba a
    dibujar fuera de ella (no debe salir nada); dibuja rectángulos de colores (plumas de ObtainBestPen), líneas y texto;
    y espera a IDCMP_CLOSEWINDOW, mirando su UserPort con GetMsg y Delay, como mucho 40 s. Devuelve 0 si la cerraron con
    su gadget, 5 si se cerró sola por tiempo, y 20 o más si algo falló. Los textos, en ASCII.
*/
#include <exec/types.h>
#include <exec/execbase.h>
#include <dos/dos.h>
#include <intuition/intuition.h>
#include <intuition/screens.h>
#include <graphics/gfx.h>
#include <graphics/rastport.h>
#include <graphics/view.h>
#include <proto/exec.h>
#include <proto/dos.h>
#include <proto/intuition.h>
#include <proto/graphics.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;
struct IntuitionBase *IntuitionBase;
struct GfxBase *GfxBase;

static void decir(CONST_STRPTR f, LONG a, LONG b, LONG c, LONG d)
{
    LONG args[4] = { a, b, c, d };
    VPrintf(f, args);
}

static LONG largo(const char *s)
{
    LONG n = 0;
    while (s[n])
        n++;
    return n;
}

static LONG cerrar_todo(LONG r)
{
    if (GfxBase)
        CloseLibrary((struct Library *)GfxBase);
    if (IntuitionBase)
        CloseLibrary((struct Library *)IntuitionBase);
    CloseLibrary((struct Library *)DOSBase);
    return r;
}

/* La primera función del hunk de código: por aquí entra el programa (saludo_entrada.s). */
LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    if (!DOSBase)
        return 20;
    IntuitionBase = (struct IntuitionBase *)OpenLibrary((CONST_STRPTR)"intuition.library", 37);
    GfxBase = (struct GfxBase *)OpenLibrary((CONST_STRPTR)"graphics.library", 37);
    if (!IntuitionBase || !GfxBase) {
        PutStr((CONST_STRPTR)"  [caja-amiga] ventana: sin intuition.library o graphics.library\n");
        return cerrar_todo(21);
    }

    /* Un fichero del disco de DashOS, por dos.library. */
    static char texto[64];
    LONG n = -1;
    BPTR fh = Open((CONST_STRPTR)"SYS:LEEME.txt", MODE_OLDFILE);
    if (fh) {
        n = Read(fh, texto, sizeof(texto) - 1);
        Close(fh);
    }
    if (n < 0) {
        decir((CONST_STRPTR)"  [caja-amiga] ventana: SYS:LEEME.txt no se abre (IoErr %ld)\n", IoErr(), 0, 0, 0);
        n = 0;
    }
    texto[n] = 0;
    while (n > 0 && (texto[n - 1] == '\n' || texto[n - 1] == '\r'))
        texto[--n] = 0;
    decir((CONST_STRPTR)"  [caja-amiga] ventana: SYS:LEEME.txt, leido por dos.library: \"%s\" (%ld bytes)\n", (LONG)texto,
          n, 0, 0);

    struct Window *w = OpenWindowTags(NULL, WA_Left, 40, WA_Top, 40, WA_InnerWidth, 320, WA_InnerHeight, 150,
                                      WA_Title, (ULONG) "Ventana Amiga: 68k en DashOS", WA_CloseGadget, TRUE,
                                      WA_DragBar, TRUE, WA_DepthGadget, TRUE, WA_Activate, TRUE,
                                      WA_IDCMP, IDCMP_CLOSEWINDOW, TAG_END);
    if (!w) {
        PutStr((CONST_STRPTR)"  [caja-amiga] ventana: OpenWindowTags dio NULL\n");
        return cerrar_todo(22);
    }
    struct RastPort *rp = w->RPort;
    LONG x0 = w->BorderLeft, y0 = w->BorderTop;
    LONG an = w->Width - w->BorderLeft - w->BorderRight, al = w->Height - w->BorderTop - w->BorderBottom;

    /* Fuera de la ventana: no debe salir nada, ni romper nada. */
    SetAPen(rp, 1);
    RectFill(rp, 5000, 5000, 9000, 9000);
    RectFill(rp, -9000, -9000, -5000, -5000);
    RectFill(rp, 200, 100, 10, 10);
    Move(rp, -30000, -30000);
    Draw(rp, -20000, 30000);
    WritePixel(rp, -1, -1);
    WritePixel(rp, 100000, 3);
    Move(rp, 30000, 20);
    Text(rp, (CONST_STRPTR)"fuera", 5);

    /* Colores: las plumas más parecidas de la paleta del Workbench. */
    struct ColorMap *cm = w->WScreen->ViewPort.ColorMap;
    static const ULONG rgb[4][3] = { { 0xEE, 0x22, 0x22 }, { 0x22, 0xBB, 0x22 }, { 0x22, 0x55, 0xEE }, { 0xFF, 0xCC, 0x00 } };
    LONG pluma[4];
    for (int i = 0; i < 4; i++)
        pluma[i] = ObtainBestPen(cm, rgb[i][0] * 0x01010101u, rgb[i][1] * 0x01010101u, rgb[i][2] * 0x01010101u,
                                 OBP_Precision, PRECISION_EXACT, TAG_END);

    SetAPen(rp, 0);
    RectFill(rp, x0, y0, x0 + an - 1, y0 + al - 1);
    for (int i = 0; i < 4; i++) {
        SetAPen(rp, pluma[i] >= 0 ? pluma[i] : 3);
        RectFill(rp, x0 + 12 + i * 76, y0 + 10, x0 + 12 + i * 76 + 63, y0 + 55);
    }
    /* Líneas: un marco y dos diagonales, en negro (pluma 1). */
    SetAPen(rp, 1);
    Move(rp, x0 + 6, y0 + 64);
    Draw(rp, x0 + an - 7, y0 + 64);
    Draw(rp, x0 + an - 7, y0 + al - 7);
    Draw(rp, x0 + 6, y0 + al - 7);
    Draw(rp, x0 + 6, y0 + 64);
    Move(rp, x0 + 6, y0 + 64);
    Draw(rp, x0 + an - 7, y0 + al - 7);
    Move(rp, x0 + an - 7, y0 + 64);
    Draw(rp, x0 + 6, y0 + al - 7);
    /* Texto, en blanco (pluma 2) sobre su fondo. */
    SetAPen(rp, 2);
    SetBPen(rp, 1);
    SetDrMd(rp, JAM2);
    static const char hola[] = "Hola desde el 68k";
    Move(rp, x0 + 14, y0 + 80 + rp->TxBaseline);
    Text(rp, (CONST_STRPTR)hola, sizeof(hola) - 1);
    Move(rp, x0 + 14, y0 + 96 + rp->TxBaseline);
    Text(rp, (CONST_STRPTR)texto, n);

    decir((CONST_STRPTR)"  [caja-amiga] ventana: bordes %ld, %ld, %ld y %ld (izquierda, arriba, derecha, abajo)\n",
          w->BorderLeft, w->BorderTop, w->BorderRight, w->BorderBottom);
    decir((CONST_STRPTR)"  [caja-amiga] ventana: pantalla %ldx%ld, su fuente de %ld puntos, WBorTop %ld\n", w->WScreen->Width,
          w->WScreen->Height, w->WScreen->Font ? w->WScreen->Font->ta_YSize : 0, w->WScreen->WBorTop);
    decir((CONST_STRPTR)"  [caja-amiga] ventana: abierta (%ldx%ld, dentro %ldx%ld), dibujada; espera IDCMP_CLOSEWINDOW\n",
          w->Width, w->Height, an, al);

    /* Espera: los mensajes del UserPort, cada 100 ms (Delay(5)), como mucho 40 s. */
    BOOL cerrada = FALSE;
    LONG vueltas = 0;
    while (!cerrada && vueltas < 400) {
        struct IntuiMessage *m;
        while ((m = (struct IntuiMessage *)GetMsg(w->UserPort))) {
            if (m->Class == IDCMP_CLOSEWINDOW)
                cerrada = TRUE;
            ReplyMsg((struct Message *)m);
        }
        if (!cerrada) {
            Delay(5);
            vueltas++;
        }
    }
    if (cerrada)
        decir((CONST_STRPTR)"  [caja-amiga] ventana: IDCMP_CLOSEWINDOW recibido tras unos %ld ms\n", vueltas * 100, 0, 0,
              0);
    else
        PutStr((CONST_STRPTR)"  [caja-amiga] ventana: nadie la cerro en 40 s; se cierra sola\n");
    CloseWindow(w);
    for (int i = 0; i < 4; i++)
        if (pluma[i] >= 0)
            ReleasePen(cm, pluma[i]);
    (void)largo;
    return cerrar_todo(cerrada ? 0 : 5);
}
