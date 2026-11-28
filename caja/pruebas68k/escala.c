/*
    La caja Amiga: ScalerDiv y BitMapScale de graphics.library, de lo que pide la diskfont.library de AmigaOS 3.9 para
    escalar una fuente. Es una prueba del oráculo: la misma salida en la caja y en un Amiga de verdad (FS-UAE con el
    Kickstart; tools/emu_run.py de Dash). ScalerDiv con varios números y BitMapScale de un mapa de 2 planos a otros
    tamaños: los puntos que salen y lo que deja en bsa_DestWidth y bsa_DestHeight.
*/
#include <exec/types.h>
#include <exec/memory.h>
#include <dos/dos.h>
#include <graphics/gfx.h>
#include <graphics/rastport.h>
#include <graphics/scale.h>
#include <proto/exec.h>
#include <proto/dos.h>
#include <proto/graphics.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;
struct GfxBase *GfxBase;

static void escribe(const char *f, LONG a, LONG b, LONG c, LONG d)
{
    LONG v[4] = { a, b, c, d };
    VPrintf((CONST_STRPTR)f, v);
}

#define AN 32
#define AL 16

static struct BitMap *nuevo(void)
{
    struct BitMap *bm = AllocMem(sizeof(struct BitMap), MEMF_PUBLIC | MEMF_CLEAR);
    if (!bm)
        return NULL;
    InitBitMap(bm, 2, AN, AL);
    for (int i = 0; i < 2; i++)
        if (!(bm->Planes[i] = AllocRaster(AN, AL)))
            return NULL;
    return bm;
}

static void vuelca(const char *que, struct BitMap *bm, int an, int al)
{
    struct RastPort rp;
    InitRastPort(&rp);
    rp.BitMap = bm;
    static char fila[AN + 2];
    escribe("%s:\n", (LONG)que, 0, 0, 0);
    for (int y = 0; y < al; y++) {
        for (int x = 0; x < an; x++)
            fila[x] = '0' + ReadPixel(&rp, x, y);
        fila[an] = '\n';
        fila[an + 1] = 0;
        PutStr((CONST_STRPTR)fila);
    }
}

static void escala(struct BitMap *src, struct BitMap *dst, UWORD sx, UWORD sy, UWORD sw, UWORD sh, UWORD xs, UWORD ys,
                   UWORD xd, UWORD yd)
{
    struct RastPort rp;
    InitRastPort(&rp);
    rp.BitMap = dst;
    SetRast(&rp, 0);
    struct BitScaleArgs a = { 0 };
    a.bsa_SrcX = sx;
    a.bsa_SrcY = sy;
    a.bsa_SrcWidth = sw;
    a.bsa_SrcHeight = sh;
    a.bsa_XSrcFactor = xs;
    a.bsa_YSrcFactor = ys;
    a.bsa_DestX = 1;
    a.bsa_DestY = 1;
    a.bsa_XDestFactor = xd;
    a.bsa_YDestFactor = yd;
    a.bsa_SrcBitMap = src;
    a.bsa_DestBitMap = dst;
    BitMapScale(&a);
    escribe("BitMapScale %ldx%ld", sw, sh, 0, 0);
    escribe(" de %ld/%ld en x y %ld/%ld en y:", xs, xd, ys, yd);
    escribe(" sale %ldx%ld\n", a.bsa_DestWidth, a.bsa_DestHeight, 0, 0);
    vuelca("  destino", dst, AN, AL);
}

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 37);
    GfxBase = (struct GfxBase *)OpenLibrary((CONST_STRPTR)"graphics.library", 37);
    if (!DOSBase || !GfxBase)
        return 20;
    static const UWORD nums[][3] = { { 10, 3, 4 }, { 7, 1, 2 }, { 5, 2, 3 }, { 1, 1, 3 }, { 100, 13, 7 }, { 0, 5, 5 },
                                     { 16383, 2, 1 }, { 9, 2, 4 }, { 11, 2, 4 }, { 3, 3, 2 } };
    for (unsigned k = 0; k < sizeof nums / sizeof nums[0]; k++)
        escribe("ScalerDiv(%ld, %ld, %ld) = %ld\n", nums[k][0], nums[k][1], nums[k][2],
                ScalerDiv(nums[k][0], nums[k][1], nums[k][2]));
    struct BitMap *src = nuevo(), *dst = nuevo();
    if (!src || !dst)
        return 20;
    /* Un dibujo de 6x4 en la esquina: plumas 1, 2 y 3 en diagonal y bordes. */
    struct RastPort rp;
    InitRastPort(&rp);
    rp.BitMap = src;
    SetRast(&rp, 0);
    static const char *dibujo[] = { "123012", "230123", "301230", "012301" };
    for (int y = 0; y < 4; y++)
        for (int x = 0; x < 6; x++) {
            SetAPen(&rp, dibujo[y][x] - '0');
            WritePixel(&rp, x, y);
        }
    vuelca("origen", src, 8, 5);
    escala(src, dst, 0, 0, 6, 4, 1, 1, 2, 2);
    escala(src, dst, 0, 0, 6, 4, 2, 2, 3, 3);
    escala(src, dst, 0, 0, 6, 4, 2, 2, 1, 1);
    escala(src, dst, 0, 0, 6, 4, 3, 1, 5, 2);
    escala(src, dst, 1, 1, 4, 3, 1, 1, 3, 1);
    return 0;
}
