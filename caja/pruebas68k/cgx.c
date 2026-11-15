/*
    cybergraphics.library y Picasso96API.library nativas de la caja: un mapa de color verdadero (AllocBitMap de 32
    bits), sus atributos, escribir y leer píxeles (WritePixelArray, FillPixelArray, ReadRGBPixel, ReadPixelArray), el
    acceso directo (LockBitMapTags / UnLockBitMap) y lo mismo por Picasso96 (p96WritePixelArray con su RenderInfo,
    p96ReadPixel, p96LockBitMap). Lo hostil: rectángulos fuera, negativos y enormes, un formato que no existe, un mapa
    planar en las funciones de color verdadero, un mapa que se suelta sin soltar antes su cerrojo.
    Mide pintar 640 x 480 en RGB y coger y soltar ese mapa. Acaba con 0 si todo sale como debe; si no, con el número
    del primer caso que falló.
*/
#include <exec/types.h>
#include <exec/memory.h>
#include <dos/dos.h>
#include <graphics/gfx.h>
#include <graphics/rastport.h>
#include <cybergraphx/cybergraphics.h>
#include <proto/exec.h>
#include <proto/dos.h>
#include <proto/graphics.h>
#include <proto/cybergraphics.h>
#include <intuition/screens.h>
#include <intuition/intuitionbase.h>
#include <proto/intuition.h>

struct ExecBase *SysBase;
struct DosLibrary *DOSBase;
struct GfxBase *GfxBase;
struct Library *CyberGfxBase;
struct IntuitionBase *IntuitionBase;
static struct Library *P96Base;

static void di(const char *f, LONG a, LONG b, LONG c)
{
    LONG v[3] = { a, b, c };
    VPrintf((CONST_STRPTR)f, v);
}

/* Picasso96API sin sus includes (como la biblioteca de Dash): RenderInfo y las llamadas por su desplazamiento. */
struct RenderInfo { APTR Memory; WORD BytesPerRow; WORD pad; ULONG RGBFormat; };
#define RGBFB_R8G8B8 2
#define RGBFB_A8R8G8B8 6
#define P96BMA_RGBFORMAT 7

static void p96_pixel_array(struct RenderInfo *ri, struct RastPort *rp, LONG x, LONG y, LONG w, LONG h)
{
    register struct RenderInfo *a0 __asm("a0") = ri;
    register ULONG d0 __asm("d0") = 0;
    register ULONG d1 __asm("d1") = 0;
    register struct RastPort *a1 __asm("a1") = rp;
    register ULONG d2 __asm("d2") = x;
    register ULONG d3 __asm("d3") = y;
    register ULONG d4 __asm("d4") = w;
    register ULONG d5 __asm("d5") = h;
    register struct Library *a6 __asm("a6") = P96Base;
    __asm volatile ("jsr a6@(-102:W)" : "+r"(a0), "+r"(d0), "+r"(d1), "+r"(a1), "+r"(d2), "+r"(d3), "+r"(d4), "+r"(d5)
                    : "r"(a6) : "cc", "memory");
}

static ULONG p96_read_pixel(struct RastPort *rp, LONG x, LONG y)
{
    register ULONG d0 __asm("d0") = x;
    register ULONG d1 __asm("d1") = y;
    register struct RastPort *a1 __asm("a1") = rp;
    register struct Library *a6 __asm("a6") = P96Base;
    __asm volatile ("jsr a6@(-120:W)" : "+r"(d0), "+r"(d1), "+r"(a1) : "r"(a6) : "a0", "cc", "memory");
    return d0;
}

static ULONG p96_bm_attr(struct BitMap *bm, ULONG attr)
{
    register struct BitMap *a0 __asm("a0") = bm;
    register ULONG d0 __asm("d0") = attr;
    register struct Library *a6 __asm("a6") = P96Base;
    __asm volatile ("jsr a6@(-42:W)" : "+r"(a0), "+r"(d0) : "r"(a6) : "d1", "a1", "cc", "memory");
    return d0;
}

static LONG p96_lock(struct BitMap *bm, struct RenderInfo *ri)
{
    register struct BitMap *a0 __asm("a0") = bm;
    register struct RenderInfo *a1 __asm("a1") = ri;
    register ULONG d0 __asm("d0") = sizeof(*ri);
    register struct Library *a6 __asm("a6") = P96Base;
    __asm volatile ("jsr a6@(-48:W)" : "+r"(a0), "+r"(a1), "+r"(d0) : "r"(a6) : "d1", "cc", "memory");
    return d0;
}

static void p96_unlock(struct BitMap *bm, LONG lock)
{
    register struct BitMap *a0 __asm("a0") = bm;
    register ULONG d0 __asm("d0") = lock;
    register struct Library *a6 __asm("a6") = P96Base;
    __asm volatile ("jsr a6@(-54:W)" : "+r"(a0), "+r"(d0) : "r"(a6) : "d1", "a1", "cc", "memory");
}

static LONG ms_desde(struct DateStamp *a)
{
    struct DateStamp b;
    DateStamp(&b);
    return ((b.ds_Minute - a->ds_Minute) * 60 * 50 + (b.ds_Tick - a->ds_Tick)) * 20;
}

#define AN 640
#define AL 480

LONG inicio(void)
{
    SysBase = *(struct ExecBase **)4;
    DOSBase = (struct DosLibrary *)OpenLibrary((CONST_STRPTR)"dos.library", 36);
    if (!DOSBase) return 20;
    GfxBase = (struct GfxBase *)OpenLibrary((CONST_STRPTR)"graphics.library", 39);
    CyberGfxBase = OpenLibrary((CONST_STRPTR)"cybergraphics.library", 41);
    P96Base = OpenLibrary((CONST_STRPTR)"Picasso96API.library", 2);
    IntuitionBase = (struct IntuitionBase *)OpenLibrary((CONST_STRPTR)"intuition.library", 39);
    LONG fallo = 0;
    struct BitMap *bm = 0, *pl = 0;
    UBYTE *pix = 0;
    struct RastPort rp;

    /* 1. Las dos abren; un mapa de 32 bits es de cybergraphics, en ARGB32. */
    if (!GfxBase || !CyberGfxBase || !P96Base || !IntuitionBase) fallo = 1;
    if (!fallo) {
        bm = AllocBitMap(AN, AL, 32, BMF_DISPLAYABLE, 0);
        pl = AllocBitMap(64, 16, 4, BMF_CLEAR, 0);
        pix = AllocMem(AN * AL * 3, MEMF_ANY);
        if (!bm || !pl || !pix || !GetCyberMapAttr(bm, CYBRMATTR_ISCYBERGFX)
            || GetCyberMapAttr(bm, CYBRMATTR_PIXFMT) != PIXFMT_ARGB32 || GetCyberMapAttr(bm, CYBRMATTR_WIDTH) != AN
            || GetCyberMapAttr(pl, CYBRMATTR_ISCYBERGFX) || p96_bm_attr(bm, P96BMA_RGBFORMAT) != RGBFB_A8R8G8B8
            || !IsCyberModeID(BestCModeIDTags(CYBRBIDTG_Depth, 24, TAG_DONE)))
            fallo = 1;
    }
    di("  [caja-amiga] cgx: un mapa de 32 bits de cybergraphics (ARGB32): %s\n", (LONG)(fallo ? "MAL" : "bien"), 0, 0);

    /* 2. Escribir y leer: FillPixelArray, WritePixelArray en RGB, ReadRGBPixel y ReadPixelArray. */
    if (!fallo) {
        InitRastPort(&rp);
        rp.BitMap = bm;
        static UBYTE dos[] = { 0x11, 0x22, 0x33, 0x44, 0x55, 0x66 };
        UBYTE leido[8] = { 0 };
        FillPixelArray(&rp, 0, 0, AN, AL, 0x00102030);
        WritePixelArray(dos, 0, 0, 6, &rp, 100, 50, 2, 1, RECTFMT_RGB);
        ReadPixelArray(leido, 0, 0, 8, &rp, 100, 50, 2, 1, RECTFMT_ARGB);
        if ((ReadRGBPixel(&rp, 0, 0) & 0xFFFFFF) != 0x102030 || (ReadRGBPixel(&rp, 101, 50) & 0xFFFFFF) != 0x445566
            || leido[1] != 0x11 || leido[7] != 0x66)
            fallo = 2;
    }
    /* 3. Acceso directo: lo escrito en la dirección que da LockBitMapTags llega al mapa al soltarlo. */
    if (!fallo) {
        ULONG base = 0, bpr = 0, fmt = 99;
        APTR h = LockBitMapTags(bm, LBMI_BASEADDRESS, (ULONG)&base, LBMI_BYTESPERROW, (ULONG)&bpr, LBMI_PIXFMT, (ULONG)&fmt,
                                TAG_DONE);
        if (!h || !base || bpr != AN * 4 || fmt != PIXFMT_ARGB32 || ((ULONG *)base)[0] << 8 != 0x10203000) fallo = 3;
        if (h) {
            ((ULONG *)(base + 10 * bpr))[20] = 0x00ABCDEF;
            UnLockBitMap(h);
        }
        if (!fallo && (ReadRGBPixel(&rp, 20, 10) & 0xFFFFFF) != 0xABCDEF) fallo = 3;
    }
    /* 4. Por Picasso96: p96WritePixelArray con un RenderInfo en R8G8B8, p96ReadPixel y p96LockBitMap. */
    if (!fallo) {
        static UBYTE uno[] = { 0x77, 0x88, 0x99 };
        struct RenderInfo ri = { uno, 3, 0, RGBFB_R8G8B8 };
        p96_pixel_array(&ri, &rp, 5, 6, 1, 1);
        if ((p96_read_pixel(&rp, 5, 6) & 0xFFFFFF) != 0x778899) fallo = 4;
        struct RenderInfo cog = { 0, 0, 0, 0 };
        LONG l = p96_lock(bm, &cog);
        if (!l || !cog.Memory || cog.BytesPerRow != AN * 4 || cog.RGBFormat != RGBFB_A8R8G8B8) fallo = 4;
        if (l) p96_unlock(bm, l);
    }
    if (!fallo) di("  [caja-amiga] cgx: pixeles, LockBitMap y Picasso96 (RenderInfo, p96LockBitMap): bien\n", 0, 0, 0);

    /* 4b. Formatos de verdad: un mapa RGB16 guarda dos bytes por punto (el rojo puro, $F800) y uno RGB24 de 7 de ancho
       lleva filas de 24 bytes (21 y 3 de relleno); LockBitMap da su memoria de verdad (la de CYBRMATTR_DISPADR). */
    if (!fallo) {
        struct BitMap *m16 = AllocBitMap(32, 4, 16, BMF_SPECIALFMT | SHIFT_PIXFMT(PIXFMT_RGB16), 0);
        struct BitMap *m24 = AllocBitMap(7, 3, 24, BMF_SPECIALFMT | SHIFT_PIXFMT(PIXFMT_RGB24), 0);
        if (!m16 || !m24 || GetCyberMapAttr(m16, CYBRMATTR_PIXFMT) != PIXFMT_RGB16 || GetCyberMapAttr(m16, CYBRMATTR_BPPIX) != 2
            || GetCyberMapAttr(m24, CYBRMATTR_XMOD) != 24 || GetBitMapAttr(m16, BMA_DEPTH) != 16)
            fallo = 6;
        if (!fallo) {
            struct RastPort r16;
            static UBYTE rojo[] = { 0xFF, 0x00, 0x00 };
            ULONG base = 0;
            InitRastPort(&r16);
            r16.BitMap = m16;
            WritePixelArray(rojo, 0, 0, 3, &r16, 0, 0, 1, 1, RECTFMT_RGB);
            APTR h = LockBitMapTags(m16, LBMI_BASEADDRESS, (ULONG)&base, TAG_DONE);
            if (!h || base != GetCyberMapAttr(m16, CYBRMATTR_DISPADR) || *(UWORD *)base != 0xF800
                || (ReadRGBPixel(&r16, 0, 0) & 0xFFFFFF) != 0xFF0000)
                fallo = 6;
            if (h) UnLockBitMap(h);
        }
        if (m16) FreeBitMap(m16);
        if (m24) FreeBitMap(m24);
    }
    if (!fallo) di("  [caja-amiga] cgx: formatos (RGB16 de dos bytes, RGB24 de ancho impar con relleno): bien\n", 0, 0, 0);

    /* 4c. Modos RTG: el del Workbench es de la caja (no del chipset); una pantalla propia en el mejor modo de 16 bits
       para 640 x 480 lleva su mapa en RGB16, y CloseScreen la suelta. */
    if (!fallo) {
        struct Screen *wb = LockPubScreen(0);
        ULONG id = BestCModeIDTags(CYBRBIDTG_Depth, 16, CYBRBIDTG_NominalWidth, 640, CYBRBIDTG_NominalHeight, 480, TAG_DONE);
        ULONG err = 0;
        struct Screen *s = 0;
        if (!wb || !IsCyberModeID(GetVPModeID(&wb->ViewPort)) || IsCyberModeID(0x29000) || id == INVALID_ID
            || GetCyberIDAttr(CYBRIDATTR_DEPTH, id) != 16)
            fallo = 8;
        if (wb) UnlockPubScreen(0, wb);
        if (!fallo) {
            s = OpenScreenTags(0, SA_DisplayID, id, SA_Title, (ULONG)"cgx", SA_ErrorCode, (ULONG)&err, TAG_DONE);
            if (!s || GetVPModeID(&s->ViewPort) != id || GetCyberMapAttr(s->RastPort.BitMap, CYBRMATTR_PIXFMT) != PIXFMT_RGB16)
                fallo = 8;
            /* Una ventana dentro de ella (en el compositor, una subsuperficie de la pantalla), y la pantalla detrás y
               delante de las demás: va primera o última en la lista de IntuitionBase. */
            if (s && !fallo) {
                struct Window *w = OpenWindowTags(0, WA_CustomScreen, (ULONG)s, WA_Left, 20, WA_Top, 30, WA_Width, 200,
                                                  WA_Height, 100, WA_Title, (ULONG)"cgx", WA_Flags, WFLG_DRAGBAR, TAG_DONE);
                if (!w || w->WScreen != s) fallo = 8;
                ScreenToBack(s);
                if (IntuitionBase->FirstScreen == s) fallo = 8;
                ScreenToFront(s);
                if (IntuitionBase->FirstScreen != s) fallo = 8;
                if (w) {
                    WindowToFront(w);
                    CloseWindow(w);
                }
            }
            if (s && !CloseScreen(s)) fallo = 8;
        }
    }
    if (!fallo) di("  [caja-amiga] cgx: modos RTG (Workbench de la caja, pantalla propia de 16 bits, con su ventana dentro): bien\n", 0, 0, 0);

    /* 5. Lo hostil: fuera, negativo, enorme, un formato que no existe, un mapa planar. */
    if (!fallo) {
        if (WritePixelArray(pix, 0, 0, 3, &rp, AN + 5, 0, 10, 10, RECTFMT_RGB) != 0) fallo = 5;
        else if (WritePixelArray(pix, 0, 0, 3 * 64, &rp, -32, -32, 64, 64, RECTFMT_RGB) != 32 * 32) fallo = 5;
        else if (FillPixelArray(&rp, 0, 0, 0xFFFF, 0xFFFF, 0x00102030) != AN * AL) fallo = 5;
        else if (WritePixelArray(pix, 0, 0, 3, &rp, 0, 0, 4, 4, 99) != 0) fallo = 5;
        else if (LockBitMapTags(pl, TAG_DONE) != 0) fallo = 5;
        else {
            struct RastPort rpp;
            InitRastPort(&rpp);
            rpp.BitMap = pl;
            FillPixelArray(&rpp, 0, 0, 8, 8, 3);
            if (ReadPixel(&rpp, 7, 7) != 3) fallo = 5;
        }
    }
    if (!fallo) di("  [caja-amiga] cgx: fuera, negativo, enorme, formato raro y planar: bien\n", 0, 0, 0);

    /* 6. Medidas: 640 x 480 en RGB por WritePixelArray, y coger y soltar el mapa entero. */
    if (!fallo) {
        struct DateStamp t;
        LONG i, n = 10;
        for (i = 0; i < AN * AL * 3; i++) pix[i] = (UBYTE)i;
        DateStamp(&t);
        for (i = 0; i < n; i++) WritePixelArray(pix, 0, 0, AN * 3, &rp, 0, 0, AN, AL, RECTFMT_RGB);
        LONG ms_wpa = ms_desde(&t);
        DateStamp(&t);
        for (i = 0; i < n; i++) {
            APTR h = LockBitMapTags(bm, TAG_DONE);
            if (h) UnLockBitMap(h);
        }
        LONG ms_lock = ms_desde(&t);
        di("  [caja-amiga] cgx: medida: 10 WritePixelArray de 640x480 RGB en %ld ms, 10 LockBitMap+UnLockBitMap en %ld ms\n",
           ms_wpa, ms_lock, 0);
    }

    /* 7. Un cerrojo sin soltar: lo suelta FreeBitMap (ningún bloque sin soltar). */
    if (!fallo && !LockBitMapTags(bm, TAG_DONE)) fallo = 7;

    if (fallo) di("  [caja-amiga] cgx: el caso %ld no sale como debe\n", fallo, 0, 0);
    if (pix) FreeMem(pix, AN * AL * 3);
    if (pl) FreeBitMap(pl);
    if (bm) FreeBitMap(bm);
    if (P96Base) CloseLibrary(P96Base);
    if (IntuitionBase) CloseLibrary((struct Library *)IntuitionBase);
    if (CyberGfxBase) CloseLibrary(CyberGfxBase);
    if (GfxBase) CloseLibrary((struct Library *)GfxBase);
    CloseLibrary((struct Library *)DOSBase);
    return fallo;
}
