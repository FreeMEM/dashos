/*
    La caja Amiga: DrawImage, DrawBorder y PrintIText de intuition, de lo que piden los programas de sistema de otros
    (AMines). Es una prueba del oráculo: la misma salida en la caja y en un Amiga de verdad (FS-UAE con el Kickstart;
    tools/emu_run.py de Dash). Pinta en un RastPort propio de 3 planos, sin pantalla, y escribe los píxeles (ReadPixel)
    y el estado del RastPort (plumas, modo, posición) después de cada llamada.
*/
#include <exec/types.h>
#include <exec/memory.h>
#include <dos/dos.h>
#include <graphics/gfx.h>
#include <graphics/rastport.h>
#include <graphics/text.h>
#include <intuition/intuition.h>
#include <proto/exec.h>
#include <proto/dos.h>
#include <proto/graphics.h>
#include <proto/intuition.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;
struct GfxBase *GfxBase;
struct IntuitionBase *IntuitionBase;

#define ANCHO 48
#define ALTO 20
#define PLANOS 3

static struct BitMap bm;
static struct RastPort rp;

static void escribe(const char *f, LONG a, LONG b, LONG c, LONG d)
{
    LONG v[4] = { a, b, c, d };
    VPrintf((CONST_STRPTR)f, v);
}

static void vuelca(const char *que)
{
    static char fila[ANCHO + 2];
    escribe("%s: plumas %ld/%ld modo %ld", (LONG)que, rp.FgPen, rp.BgPen, rp.DrawMode);
    escribe(" en %ld,%ld\n", rp.cp_x, rp.cp_y, 0, 0);
    for (int y = 0; y < ALTO; y++) {
        for (int x = 0; x < ANCHO; x++)
            fila[x] = '0' + ReadPixel(&rp, x, y);
        fila[ANCHO] = '\n';
        fila[ANCHO + 1] = 0;
        PutStr((CONST_STRPTR)fila);
    }
}

/* Lo que ocupa cada pluma (el rectángulo que la contiene): para el texto, que no depende de cómo es cada letra de la
   fuente (la topaz de la caja no es la de la ROM). */
static void cajas(const char *que)
{
    escribe("%s: plumas %ld/%ld modo %ld", (LONG)que, rp.FgPen, rp.BgPen, rp.DrawMode);
    escribe(" en %ld,%ld\n", rp.cp_x, rp.cp_y, 0, 0);
    for (int pen = 1; pen < 8; pen++) {
        int x0 = ANCHO, y0 = ALTO, x1 = -1, y1 = -1;
        for (int y = 0; y < ALTO; y++)
            for (int x = 0; x < ANCHO; x++)
                if (ReadPixel(&rp, x, y) == pen) {
                    if (x < x0) x0 = x;
                    if (x > x1) x1 = x;
                    if (y < y0) y0 = y;
                    if (y > y1) y1 = y;
                }
        /* Las columnas de las letras dependen de la fuente: solo las del fondo de JAM2 (pluma 6), la celda. */
        if (x1 >= 0)
            escribe("  pluma %ld: filas %ld a %ld", pen, y0, y1, 0);
        if (x1 >= 0 && pen == 6)
            escribe(", columnas %ld a %ld", x0, x1, 0, 0);
        if (x1 >= 0)
            PutStr((CONST_STRPTR)"\n");
    }
}

static void limpia(UBYTE pluma)
{
    SetRast(&rp, pluma);
    SetAPen(&rp, 1);
    SetBPen(&rp, 0);
    SetDrMd(&rp, JAM1);
    Move(&rp, 0, 0);
}

/* Dos imágenes encadenadas: una de 16x4 con 2 planos que van a los planos 0 y 2 (el 1, siempre encendido) y otra de
   8x2 con un plano y la mitad derecha que no se ve. Las de los datos tienen que estar en memoria chip: se copian. */
static const UWORD datos1[] = {
    0xF00F, 0xFFFF, 0x0FF0, 0x8001,     /* plano A (va al 0) */
    0xAAAA, 0x5555, 0xFF00, 0x00FF,     /* plano B (va al 2) */
};
static const UWORD datos2[] = { 0xC3FF, 0x3CFF };

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 37);
    GfxBase = (struct GfxBase *)OpenLibrary((CONST_STRPTR)"graphics.library", 37);
    IntuitionBase = (struct IntuitionBase *)OpenLibrary((CONST_STRPTR)"intuition.library", 37);
    if (!DOSBase || !GfxBase || !IntuitionBase)
        return 20;
    InitBitMap(&bm, PLANOS, ANCHO, ALTO);
    for (int i = 0; i < PLANOS; i++) {
        bm.Planes[i] = AllocRaster(ANCHO, ALTO);
        if (!bm.Planes[i])
            return 20;
    }
    InitRastPort(&rp);
    rp.BitMap = &bm;

    UWORD *chip = AllocMem(sizeof datos1 + sizeof datos2, MEMF_CHIP);
    if (!chip)
        return 20;
    for (unsigned i = 0; i < sizeof datos1 / 2; i++) chip[i] = datos1[i];
    for (unsigned i = 0; i < sizeof datos2 / 2; i++) chip[sizeof datos1 / 2 + i] = datos2[i];

    struct Image im2 = { 0, 6, 8, 2, 1, chip + sizeof datos1 / 2, 0x01, 0x00, NULL };
    struct Image im1 = { 1, 1, 16, 4, 2, chip, 0x05, 0x02, &im2 };

    /* DrawImage: los planos que no lleva la imagen, de PlaneOnOff; la posición, la de la llamada más la suya. */
    limpia(0);
    DrawImage(&rp, &im1, 2, 3);
    vuelca("DrawImage");

    /* Con el RastPort en COMPLEMENT y JAM2, encima de un fondo de pluma 7. */
    limpia(7);
    SetDrMd(&rp, COMPLEMENT);
    SetAPen(&rp, 4);
    DrawImage(&rp, &im1, 20, 0);
    SetDrMd(&rp, JAM2);
    DrawImage(&rp, &im2, 30, 10);
    vuelca("DrawImage COMPLEMENT/JAM2");

    /* Sin datos: PlanePick 0, todo de PlaneOnOff (un rectángulo de color). */
    struct Image solida = { 0, 0, 5, 3, 0, NULL, 0x00, 0x06, NULL };
    limpia(1);
    DrawImage(&rp, &solida, 40, 15);
    vuelca("DrawImage sin datos");

    /* DrawBorder: dos encadenados, uno en JAM1 con 5 puntos y otro en COMPLEMENT. */
    static WORD xy1[] = { 0, 0, 10, 0, 10, 5, 0, 5, 0, 1 };
    static WORD xy2[] = { 3, 2, 20, 9 };
    struct Border b2 = { 0, 0, 3, 0, COMPLEMENT, 2, xy2, NULL };
    struct Border b1 = { 2, 1, 5, 2, JAM1, 5, xy1, &b2 };
    limpia(0);
    SetAPen(&rp, 6);
    DrawBorder(&rp, &b1, 4, 4);
    vuelca("DrawBorder");

    /* PrintIText: dos textos encadenados, con la fuente del RastPort (topaz 8: la de serie depende de las
       preferencias) y con la suya. */
    struct TextAttr topaz = { (STRPTR)"topaz.font", 8, 0, 0 };
    struct TextFont *tf = OpenFont(&topaz);
    if (!tf)
        return 20;
    SetFont(&rp, tf);
    struct IntuiText t2 = { 2, 0, JAM1, 0, 10, &topaz, (UBYTE *)"ab", NULL };
    struct IntuiText t1 = { 3, 6, JAM2, 1, 0, NULL, (UBYTE *)"Hi", &t2 };
    limpia(0);
    PrintIText(&rp, &t1, 5, 1);
    cajas("PrintIText");
    /* La de t1 (sin fuente) depende de la fuente de las preferencias: solo la de t2. */
    escribe("IntuiTextLength %ld\n", IntuiTextLength(&t2), 0, 0, 0);

    CloseFont(tf);
    FreeMem(chip, sizeof datos1 + sizeof datos2);
    for (int i = 0; i < PLANOS; i++)
        FreeRaster(bm.Planes[i], ANCHO, ALTO);
    return 0;
}
