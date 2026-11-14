//! Disposición de las estructuras de graphics e intuition en AmigaOS 3.1 (V40): desplazamientos y tamaños en
//! bytes, y las constantes de la interfaz.
//!
//! Sacados de la interfaz pública: los includes del NDK (`graphics/gfx.h`, `rastport.h`, `text.h`, `view.h`,
//! `gfxbase.h`, `clip.h`; `intuition/intuition.h`, `screens.h`, `intuitionbase.h`; `devices/inputevent.h`) y el
//! ROM Kernel Reference Manual: Libraries. Los nombres son los de los campos en esos includes. Las pruebas
//! (`tests/estructuras.rs`) fijan los desplazamientos con los valores que da `offsetof` en el compilador del 68k.
//!
//! Todo en big-endian, punteros de 32 bits y sin relleno: el 68k alinea a 2.

/// `struct BitMap`.
pub mod bitmap {
    pub const BYTESPERROW: u32 = 0;
    pub const ROWS: u32 = 2;
    pub const FLAGS: u32 = 4;
    pub const DEPTH: u32 = 5;
    pub const PAD: u32 = 6;
    pub const PLANES: u32 = 8;
    pub const TAMANO: u32 = 40;
    /// Planos que caben en `Planes[]`.
    pub const MAX_PLANOS: u32 = 8;

    // Opciones de `AllocBitMap` (`BMF_*`).
    pub const BMF_CLEAR: u32 = 1 << 0;
    pub const BMF_DISPLAYABLE: u32 = 1 << 1;
    pub const BMF_INTERLEAVED: u32 = 1 << 2;
    pub const BMF_STANDARD: u32 = 1 << 3;
    pub const BMF_MINPLANES: u32 = 1 << 4;

    // Atributos de `GetBitMapAttr` (`BMA_*`).
    pub const BMA_HEIGHT: u32 = 0;
    pub const BMA_DEPTH: u32 = 4;
    pub const BMA_WIDTH: u32 = 8;
    pub const BMA_FLAGS: u32 = 12;
}

/// `struct Rectangle` (MinX, MinY, MaxX, MaxY, WORD).
pub mod rectangle {
    pub const MINX: u32 = 0;
    pub const MINY: u32 = 2;
    pub const MAXX: u32 = 4;
    pub const MAXY: u32 = 6;
    pub const TAMANO: u32 = 8;
}

/// `struct RastPort`.
pub mod rastport {
    pub const LAYER: u32 = 0;
    pub const BITMAP: u32 = 4;
    pub const AREAPTRN: u32 = 8;
    pub const TMPRAS: u32 = 12;
    pub const AREAINFO: u32 = 16;
    pub const GELSINFO: u32 = 20;
    pub const MASK: u32 = 24;
    pub const FGPEN: u32 = 25;
    pub const BGPEN: u32 = 26;
    pub const AOLPEN: u32 = 27;
    pub const DRAWMODE: u32 = 28;
    pub const AREAPTSZ: u32 = 29;
    pub const LINPATCNT: u32 = 30;
    pub const DUMMY: u32 = 31;
    pub const FLAGS: u32 = 32;
    pub const LINEPTRN: u32 = 34;
    pub const CP_X: u32 = 36;
    pub const CP_Y: u32 = 38;
    pub const MINTERMS: u32 = 40;
    pub const PENWIDTH: u32 = 48;
    pub const PENHEIGHT: u32 = 50;
    pub const FONT: u32 = 52;
    pub const ALGOSTYLE: u32 = 56;
    pub const TXFLAGS: u32 = 57;
    pub const TXHEIGHT: u32 = 58;
    pub const TXWIDTH: u32 = 60;
    pub const TXBASELINE: u32 = 62;
    pub const TXSPACING: u32 = 64;
    pub const RP_USER: u32 = 66;
    pub const LONGRESERVED: u32 = 70;
    pub const WORDRESERVED: u32 = 78;
    pub const RESERVED: u32 = 92;
    pub const TAMANO: u32 = 100;

    // `rp_Flags`.
    pub const FRST_DOT: u16 = 0x01;
    pub const ONE_DOT: u16 = 0x02;
    pub const DBUFFER: u16 = 0x04;
    pub const AREAOUTLINE: u16 = 0x08;
    pub const NOCROSSFILL: u16 = 0x20;
}

/// `struct AreaInfo` (de `graphics/rastport.h`): la tabla de vértices de `AreaMove`/`AreaDraw`/`AreaEllipse`.
pub mod areainfo {
    pub const VCTRTBL: u32 = 0;
    pub const VCTRPTR: u32 = 4;
    pub const FLAGTBL: u32 = 8;
    pub const FLAGPTR: u32 = 12;
    pub const COUNT: u32 = 16;
    pub const MAXCOUNT: u32 = 18;
    pub const FIRSTX: u32 = 20;
    pub const FIRSTY: u32 = 22;
    pub const TAMANO: u32 = 24;
}

/// `struct TmpRas`.
pub mod tmpras {
    pub const RASPTR: u32 = 0;
    pub const SIZE: u32 = 4;
    pub const TAMANO: u32 = 8;
}

/// Modos de dibujo (`rp_DrawMode`).
pub mod drmd {
    pub const JAM1: u8 = 0;
    pub const JAM2: u8 = 1;
    pub const COMPLEMENT: u8 = 2;
    pub const INVERSVID: u8 = 4;
}

/// `struct TextAttr`.
pub mod textattr {
    pub const TA_NAME: u32 = 0;
    pub const TA_YSIZE: u32 = 4;
    pub const TA_STYLE: u32 = 6;
    pub const TA_FLAGS: u32 = 7;
    pub const TAMANO: u32 = 8;
}

/// `struct TextFont`. Empieza con un `struct Message` (`tf_Message`), cuyo nodo lleva el nombre.
pub mod textfont {
    pub const TF_MESSAGE: u32 = 0;
    pub const TF_YSIZE: u32 = 20;
    pub const TF_STYLE: u32 = 22;
    pub const TF_FLAGS: u32 = 23;
    pub const TF_XSIZE: u32 = 24;
    pub const TF_BASELINE: u32 = 26;
    pub const TF_BOLDSMEAR: u32 = 28;
    pub const TF_ACCESSORS: u32 = 30;
    pub const TF_LOCHAR: u32 = 32;
    pub const TF_HICHAR: u32 = 33;
    pub const TF_CHARDATA: u32 = 34;
    pub const TF_MODULO: u32 = 38;
    pub const TF_CHARLOC: u32 = 40;
    pub const TF_CHARSPACE: u32 = 44;
    pub const TF_CHARKERN: u32 = 48;
    pub const TAMANO: u32 = 52;

    // `tf_Flags`.
    pub const FPF_ROMFONT: u8 = 0x01;
    pub const FPF_DISKFONT: u8 = 0x02;
    pub const FPF_REVPATH: u8 = 0x04;
    pub const FPF_TALLDOT: u8 = 0x08;
    pub const FPF_WIDEDOT: u8 = 0x10;
    pub const FPF_PROPORTIONAL: u8 = 0x20;
    pub const FPF_DESIGNED: u8 = 0x40;
    pub const FPF_REMOVED: u8 = 0x80;
}

/// `struct Region` (de `graphics/regions.h`): sus límites y la lista de sus rectángulos.
pub mod region {
    pub const BOUNDS: u32 = 0;
    pub const REGIONRECTANGLE: u32 = 8;
    pub const TAMANO: u32 = 12;
}

/// `struct RegionRectangle`: un nodo de la lista doble de una región; sus límites son relativos a la esquina de arriba
/// a la izquierda de los de la región.
pub mod region_rectangle {
    pub const NEXT: u32 = 0;
    pub const PREV: u32 = 4;
    pub const BOUNDS: u32 = 8;
    pub const TAMANO: u32 = 16;
}

/// `struct ColorMap` (V39).
pub mod colormap {
    pub const FLAGS: u32 = 0;
    pub const TYPE: u32 = 1;
    pub const COUNT: u32 = 2;
    pub const COLORTABLE: u32 = 4;
    pub const CM_VPE: u32 = 8;
    pub const LOWCOLORBITS: u32 = 12;
    pub const TRANSPARENCYPLANE: u32 = 16;
    pub const SPRITERESOLUTION: u32 = 17;
    pub const SPRITERESDEFAULT: u32 = 18;
    pub const AUXFLAGS: u32 = 19;
    pub const CM_VP: u32 = 20;
    pub const NORMALDISPLAYINFO: u32 = 24;
    pub const COERCEDISPLAYINFO: u32 = 28;
    pub const CM_BATCH_ITEMS: u32 = 32;
    pub const VPMODEID: u32 = 36;
    pub const PALEXTRA: u32 = 40;
    pub const SPRITEBASE_EVEN: u32 = 44;
    pub const SPRITEBASE_ODD: u32 = 46;
    pub const BP_0_BASE: u32 = 48;
    pub const BP_1_BASE: u32 = 50;
    pub const TAMANO: u32 = 52;

    pub const COLORMAP_TYPE_V39: u8 = 2;
}

/// `struct ViewPort`.
pub mod viewport {
    pub const NEXT: u32 = 0;
    pub const COLORMAP: u32 = 4;
    pub const DSPINS: u32 = 8;
    pub const SPRINS: u32 = 12;
    pub const CLRINS: u32 = 16;
    pub const UCOPINS: u32 = 20;
    pub const DWIDTH: u32 = 24;
    pub const DHEIGHT: u32 = 26;
    pub const DXOFFSET: u32 = 28;
    pub const DYOFFSET: u32 = 30;
    pub const MODES: u32 = 32;
    pub const SPRITEPRIORITIES: u32 = 34;
    pub const EXTENDEDMODES: u32 = 35;
    pub const RASINFO: u32 = 36;
    pub const TAMANO: u32 = 40;

    // `Modes`.
    pub const HIRES: u16 = 0x8000;
}

/// `struct Layer` (de `graphics/clip.h`). Se rellenan `rp`, `bounds`, `Flags`, `Window`, `LayerInfo`, `Width`,
/// `Height`, una sola `ClipRect` (la capa entera: una ventana nunca está tapada en su búfer) y la `DamageList` (lo que
/// falta por pintar de una ventana de refresco simple).
pub mod layer {
    pub const FRONT: u32 = 0;
    pub const BACK: u32 = 4;
    pub const CLIPRECT: u32 = 8;
    pub const RP: u32 = 12;
    pub const BOUNDS: u32 = 16;
    pub const PRIORITY: u32 = 28;
    pub const FLAGS: u32 = 30;
    pub const SUPERBITMAP: u32 = 32;
    pub const WINDOW: u32 = 40;
    pub const SCROLL_X: u32 = 44;
    pub const SCROLL_Y: u32 = 46;
    pub const LAYERINFO: u32 = 68;
    pub const LOCK: u32 = 72;
    pub const BACKFILL: u32 = 118;
    pub const CLIPREGION: u32 = 126;
    pub const WIDTH: u32 = 134;
    pub const HEIGHT: u32 = 136;
    pub const DAMAGELIST: u32 = 156;
    pub const TAMANO: u32 = 160;

    // `Flags`.
    pub const LAYERSIMPLE: u16 = 1;
    pub const LAYERSMART: u16 = 2;
    pub const LAYERSUPER: u16 = 4;
    pub const LAYERREFRESH: u16 = 0x80;
}

/// `struct ClipRect` (de `graphics/clip.h`).
pub mod cliprect {
    pub const NEXT: u32 = 0;
    pub const PREV: u32 = 4;
    pub const LOBS: u32 = 8;
    pub const BITMAP: u32 = 12;
    pub const BOUNDS: u32 = 16;
    pub const TAMANO: u32 = 40;
}

/// `struct Layer_Info` (solo su tamaño: va dentro de `struct Screen`).
pub mod layer_info {
    pub const TAMANO: u32 = 102;
}

/// `struct GfxBase` (V40). Solo los campos que se rellenan.
pub mod gfxbase {
    pub const ACTIVIEW: u32 = 34;
    pub const TEXTFONTS: u32 = 140;
    pub const DEFAULTFONT: u32 = 154;
    pub const MODES: u32 = 158;
    pub const VBLANK: u32 = 160;
    pub const DISPLAYFLAGS: u32 = 206;
    pub const MAXDISPLAYROW: u32 = 212;
    pub const MAXDISPLAYCOLUMN: u32 = 214;
    pub const NORMALDISPLAYROWS: u32 = 216;
    pub const NORMALDISPLAYCOLUMNS: u32 = 218;
    pub const CHIPREVBITS0: u32 = 236;
    pub const MEMTYPE: u32 = 237;
    pub const MONITORLIST: u32 = 384;
    pub const BP3BITS: u32 = 483;
    pub const GFXFLAGS: u32 = 498;
    pub const TAMANO: u32 = 552;

    // `DisplayFlags`.
    pub const PAL: u16 = 4;
    // `ChipRevBits0`.
    pub const GFXF_HR_AGNUS: u8 = 1 << 0;
    pub const GFXF_HR_DENISE: u8 = 1 << 1;
    pub const GFXF_AA_ALICE: u8 = 1 << 2;
    pub const GFXF_AA_LISA: u8 = 1 << 3;
}

/// `struct Screen`.
pub mod screen {
    pub const NEXTSCREEN: u32 = 0;
    pub const FIRSTWINDOW: u32 = 4;
    pub const LEFTEDGE: u32 = 8;
    pub const TOPEDGE: u32 = 10;
    pub const WIDTH: u32 = 12;
    pub const HEIGHT: u32 = 14;
    pub const MOUSEY: u32 = 16;
    pub const MOUSEX: u32 = 18;
    pub const FLAGS: u32 = 20;
    pub const TITLE: u32 = 22;
    pub const DEFAULTTITLE: u32 = 26;
    pub const BARHEIGHT: u32 = 30;
    pub const BARVBORDER: u32 = 31;
    pub const BARHBORDER: u32 = 32;
    pub const MENUVBORDER: u32 = 33;
    pub const MENUHBORDER: u32 = 34;
    pub const WBORTOP: u32 = 35;
    pub const WBORLEFT: u32 = 36;
    pub const WBORRIGHT: u32 = 37;
    pub const WBORBOTTOM: u32 = 38;
    pub const FONT: u32 = 40;
    pub const VIEWPORT: u32 = 44;
    pub const RASTPORT: u32 = 84;
    pub const BITMAP: u32 = 184;
    pub const LAYERINFO: u32 = 224;
    pub const FIRSTGADGET: u32 = 326;
    pub const DETAILPEN: u32 = 330;
    pub const BLOCKPEN: u32 = 331;
    pub const SAVECOLOR0: u32 = 332;
    pub const BARLAYER: u32 = 334;
    pub const EXTDATA: u32 = 338;
    pub const USERDATA: u32 = 342;
    pub const TAMANO: u32 = 346;

    // `Flags`.
    pub const WBENCHSCREEN: u16 = 0x0001;
    pub const PUBLICSCREEN: u16 = 0x0002;
    pub const CUSTOMSCREEN: u16 = 0x000F;
    pub const SHOWTITLE: u16 = 0x0010;
    pub const BEEPING: u16 = 0x0020;
    pub const CUSTOMBITMAP: u16 = 0x0040;
    pub const SCREENBEHIND: u16 = 0x0080;
    pub const SCREENQUIET: u16 = 0x0100;
    pub const SCREENHIRES: u16 = 0x0200;
    pub const AUTOSCROLL: u16 = 0x4000;

    // Tipo de pantalla en `GetScreenData`.
    pub const TIPO_WBENCH: u32 = 1;
}

/// `struct Window`.
pub mod window {
    pub const NEXTWINDOW: u32 = 0;
    pub const LEFTEDGE: u32 = 4;
    pub const TOPEDGE: u32 = 6;
    pub const WIDTH: u32 = 8;
    pub const HEIGHT: u32 = 10;
    pub const MOUSEY: u32 = 12;
    pub const MOUSEX: u32 = 14;
    pub const MINWIDTH: u32 = 16;
    pub const MINHEIGHT: u32 = 18;
    pub const MAXWIDTH: u32 = 20;
    pub const MAXHEIGHT: u32 = 22;
    pub const FLAGS: u32 = 24;
    pub const MENUSTRIP: u32 = 28;
    pub const TITLE: u32 = 32;
    pub const FIRSTREQUEST: u32 = 36;
    pub const DMREQUEST: u32 = 40;
    pub const REQCOUNT: u32 = 44;
    pub const WSCREEN: u32 = 46;
    pub const RPORT: u32 = 50;
    pub const BORDERLEFT: u32 = 54;
    pub const BORDERTOP: u32 = 55;
    pub const BORDERRIGHT: u32 = 56;
    pub const BORDERBOTTOM: u32 = 57;
    pub const BORDERRPORT: u32 = 58;
    pub const FIRSTGADGET: u32 = 62;
    pub const PARENT: u32 = 66;
    pub const DESCENDANT: u32 = 70;
    pub const POINTER: u32 = 74;
    pub const PTRHEIGHT: u32 = 78;
    pub const PTRWIDTH: u32 = 79;
    pub const XOFFSET: u32 = 80;
    pub const YOFFSET: u32 = 81;
    pub const IDCMPFLAGS: u32 = 82;
    pub const USERPORT: u32 = 86;
    pub const WINDOWPORT: u32 = 90;
    pub const MESSAGEKEY: u32 = 94;
    pub const DETAILPEN: u32 = 98;
    pub const BLOCKPEN: u32 = 99;
    pub const CHECKMARK: u32 = 100;
    pub const SCREENTITLE: u32 = 104;
    pub const GZZMOUSEX: u32 = 108;
    pub const GZZMOUSEY: u32 = 110;
    pub const GZZWIDTH: u32 = 112;
    pub const GZZHEIGHT: u32 = 114;
    pub const EXTDATA: u32 = 116;
    pub const USERDATA: u32 = 120;
    pub const WLAYER: u32 = 124;
    pub const IFONT: u32 = 128;
    pub const MOREFLAGS: u32 = 132;
    pub const TAMANO: u32 = 136;
}

/// `struct NewWindow` y `struct ExtNewWindow`.
pub mod newwindow {
    pub const LEFTEDGE: u32 = 0;
    pub const TOPEDGE: u32 = 2;
    pub const WIDTH: u32 = 4;
    pub const HEIGHT: u32 = 6;
    pub const DETAILPEN: u32 = 8;
    pub const BLOCKPEN: u32 = 9;
    pub const IDCMPFLAGS: u32 = 10;
    pub const FLAGS: u32 = 14;
    pub const FIRSTGADGET: u32 = 18;
    pub const CHECKMARK: u32 = 22;
    pub const TITLE: u32 = 26;
    pub const SCREEN: u32 = 30;
    pub const BITMAP: u32 = 34;
    pub const MINWIDTH: u32 = 38;
    pub const MINHEIGHT: u32 = 40;
    pub const MAXWIDTH: u32 = 42;
    pub const MAXHEIGHT: u32 = 44;
    pub const TYPE: u32 = 46;
    pub const TAMANO: u32 = 48;
    /// `ExtNewWindow`: la lista de etiquetas, si `Flags` lleva `WFLG_NW_EXTENDED`.
    pub const EXTENSION: u32 = 48;
    pub const TAMANO_EXT: u32 = 52;
}

/// `struct IntuiMessage` y `struct ExtIntuiMessage`.
pub mod intuimessage {
    pub const EXECMESSAGE: u32 = 0;
    pub const CLASS: u32 = 20;
    pub const CODE: u32 = 24;
    pub const QUALIFIER: u32 = 26;
    pub const IADDRESS: u32 = 28;
    pub const MOUSEX: u32 = 32;
    pub const MOUSEY: u32 = 34;
    pub const SECONDS: u32 = 36;
    pub const MICROS: u32 = 40;
    pub const IDCMPWINDOW: u32 = 44;
    pub const SPECIALLINK: u32 = 48;
    pub const TAMANO: u32 = 52;
    /// `ExtIntuiMessage` (V39): lleva `eim_TabletData` detrás. Es lo que reparte intuition 3.1.
    pub const TAMANO_EXT: u32 = 56;
}

/// `struct DrawInfo` (V39: `DRI_VERSION` 2).
pub mod drawinfo {
    pub const DRI_VERSION: u32 = 0;
    pub const DRI_NUMPENS: u32 = 2;
    pub const DRI_PENS: u32 = 4;
    pub const DRI_FONT: u32 = 8;
    pub const DRI_DEPTH: u32 = 12;
    pub const DRI_RESOLUTION: u32 = 14;
    pub const DRI_FLAGS: u32 = 18;
    pub const DRI_CHECKMARK: u32 = 22;
    pub const DRI_AMIGAKEY: u32 = 26;
    pub const DRI_RESERVED: u32 = 34;
    pub const TAMANO: u32 = 50;

    pub const VERSION: u16 = 2;
    pub const DRIF_NEWLOOK: u32 = 1;

    // Índices de `dri_Pens`.
    pub const DETAILPEN: usize = 0;
    pub const BLOCKPEN: usize = 1;
    pub const TEXTPEN: usize = 2;
    pub const SHINEPEN: usize = 3;
    pub const SHADOWPEN: usize = 4;
    pub const FILLPEN: usize = 5;
    pub const FILLTEXTPEN: usize = 6;
    pub const BACKGROUNDPEN: usize = 7;
    pub const HIGHLIGHTTEXTPEN: usize = 8;
    pub const BARDETAILPEN: usize = 9;
    pub const BARBLOCKPEN: usize = 10;
    pub const BARTRIMPEN: usize = 11;
    pub const NUMDRIPENS: usize = 12;
}

/// `struct IntuitionBase` (la parte pública).
pub mod intuitionbase {
    pub const VIEWLORD: u32 = 34;
    pub const ACTIVEWINDOW: u32 = 52;
    pub const ACTIVESCREEN: u32 = 56;
    pub const FIRSTSCREEN: u32 = 60;
    pub const FLAGS: u32 = 64;
    pub const MOUSEY: u32 = 68;
    pub const MOUSEX: u32 = 70;
    pub const SECONDS: u32 = 72;
    pub const MICROS: u32 = 76;
    pub const TAMANO: u32 = 80;
}

/// `struct TagItem` y las etiquetas de control de `utility/tagitem.h`.
pub mod tag {
    pub const TI_TAG: u32 = 0;
    pub const TI_DATA: u32 = 4;
    pub const TAMANO: u32 = 8;

    pub const TAG_DONE: u32 = 0;
    pub const TAG_IGNORE: u32 = 1;
    pub const TAG_MORE: u32 = 2;
    pub const TAG_SKIP: u32 = 3;
    pub const TAG_USER: u32 = 0x8000_0000;
}

/// Etiquetas de `OpenWindowTagList` (`WA_*`).
pub mod wa {
    const D: u32 = super::tag::TAG_USER + 99;
    pub const LEFT: u32 = D + 0x01;
    pub const TOP: u32 = D + 0x02;
    pub const WIDTH: u32 = D + 0x03;
    pub const HEIGHT: u32 = D + 0x04;
    pub const DETAILPEN: u32 = D + 0x05;
    pub const BLOCKPEN: u32 = D + 0x06;
    pub const IDCMP: u32 = D + 0x07;
    pub const FLAGS: u32 = D + 0x08;
    pub const GADGETS: u32 = D + 0x09;
    pub const CHECKMARK: u32 = D + 0x0A;
    pub const TITLE: u32 = D + 0x0B;
    pub const SCREENTITLE: u32 = D + 0x0C;
    pub const CUSTOMSCREEN: u32 = D + 0x0D;
    pub const SUPERBITMAP: u32 = D + 0x0E;
    pub const MINWIDTH: u32 = D + 0x0F;
    pub const MINHEIGHT: u32 = D + 0x10;
    pub const MAXWIDTH: u32 = D + 0x11;
    pub const MAXHEIGHT: u32 = D + 0x12;
    pub const INNERWIDTH: u32 = D + 0x13;
    pub const INNERHEIGHT: u32 = D + 0x14;
    pub const PUBSCREENNAME: u32 = D + 0x15;
    pub const PUBSCREEN: u32 = D + 0x16;
    pub const PUBSCREENFALLBACK: u32 = D + 0x17;
    pub const WINDOWNAME: u32 = D + 0x18;
    pub const COLORS: u32 = D + 0x19;
    pub const ZOOM: u32 = D + 0x1A;
    pub const MOUSEQUEUE: u32 = D + 0x1B;
    pub const BACKFILL: u32 = D + 0x1C;
    pub const RPTQUEUE: u32 = D + 0x1D;
    pub const SIZEGADGET: u32 = D + 0x1E;
    pub const DRAGBAR: u32 = D + 0x1F;
    pub const DEPTHGADGET: u32 = D + 0x20;
    pub const CLOSEGADGET: u32 = D + 0x21;
    pub const BACKDROP: u32 = D + 0x22;
    pub const REPORTMOUSE: u32 = D + 0x23;
    pub const NOCAREREFRESH: u32 = D + 0x24;
    pub const BORDERLESS: u32 = D + 0x25;
    pub const ACTIVATE: u32 = D + 0x26;
    pub const RMBTRAP: u32 = D + 0x27;
    pub const WBENCHWINDOW: u32 = D + 0x28;
    pub const SIMPLEREFRESH: u32 = D + 0x29;
    pub const SMARTREFRESH: u32 = D + 0x2A;
    pub const SIZEBRIGHT: u32 = D + 0x2B;
    pub const SIZEBBOTTOM: u32 = D + 0x2C;
    pub const AUTOADJUST: u32 = D + 0x2D;
    pub const GIMMEZEROZERO: u32 = D + 0x2E;
    pub const MENUHELP: u32 = D + 0x2F;
    pub const NEWLOOKMENUS: u32 = D + 0x30;
    pub const AMIGAKEY: u32 = D + 0x31;
    pub const NOTIFYDEPTH: u32 = D + 0x32;
}

/// `Window->Flags` (`WFLG_*`).
pub mod wflg {
    pub const SIZEGADGET: u32 = 0x0000_0001;
    pub const DRAGBAR: u32 = 0x0000_0002;
    pub const DEPTHGADGET: u32 = 0x0000_0004;
    pub const CLOSEGADGET: u32 = 0x0000_0008;
    pub const SIZEBRIGHT: u32 = 0x0000_0010;
    pub const SIZEBBOTTOM: u32 = 0x0000_0020;
    pub const REFRESHBITS: u32 = 0x0000_00C0;
    pub const SMART_REFRESH: u32 = 0x0000_0000;
    pub const SIMPLE_REFRESH: u32 = 0x0000_0040;
    pub const SUPER_BITMAP: u32 = 0x0000_0080;
    pub const OTHER_REFRESH: u32 = 0x0000_00C0;
    pub const BACKDROP: u32 = 0x0000_0100;
    pub const REPORTMOUSE: u32 = 0x0000_0200;
    pub const GIMMEZEROZERO: u32 = 0x0000_0400;
    pub const BORDERLESS: u32 = 0x0000_0800;
    pub const ACTIVATE: u32 = 0x0000_1000;
    pub const WINDOWACTIVE: u32 = 0x0000_2000;
    pub const INREQUEST: u32 = 0x0000_4000;
    pub const MENUSTATE: u32 = 0x0000_8000;
    pub const RMBTRAP: u32 = 0x0001_0000;
    pub const NOCAREREFRESH: u32 = 0x0002_0000;
    pub const NW_EXTENDED: u32 = 0x0004_0000;
    pub const NEWLOOKMENUS: u32 = 0x0020_0000;
    pub const WINDOWREFRESH: u32 = 0x0100_0000;
    pub const WBENCHWINDOW: u32 = 0x0200_0000;
    pub const WINDOWTICKED: u32 = 0x0400_0000;
    pub const VISITOR: u32 = 0x0800_0000;
    pub const ZOOMED: u32 = 0x1000_0000;
    pub const HASZOOM: u32 = 0x2000_0000;
    /// Lo que puede pedir un programa (el resto lo lleva intuition).
    pub const DEL_PROGRAMA: u32 = 0x003F_1FFF;
}

/// Clases de mensaje IDCMP (`IDCMP_*`).
pub mod idcmp {
    pub const SIZEVERIFY: u32 = 0x0000_0001;
    pub const NEWSIZE: u32 = 0x0000_0002;
    pub const REFRESHWINDOW: u32 = 0x0000_0004;
    pub const MOUSEBUTTONS: u32 = 0x0000_0008;
    pub const MOUSEMOVE: u32 = 0x0000_0010;
    pub const GADGETDOWN: u32 = 0x0000_0020;
    pub const GADGETUP: u32 = 0x0000_0040;
    pub const REQSET: u32 = 0x0000_0080;
    pub const MENUPICK: u32 = 0x0000_0100;
    pub const CLOSEWINDOW: u32 = 0x0000_0200;
    pub const RAWKEY: u32 = 0x0000_0400;
    pub const REQVERIFY: u32 = 0x0000_0800;
    pub const REQCLEAR: u32 = 0x0000_1000;
    pub const MENUVERIFY: u32 = 0x0000_2000;
    pub const NEWPREFS: u32 = 0x0000_4000;
    pub const DISKINSERTED: u32 = 0x0000_8000;
    pub const DISKREMOVED: u32 = 0x0001_0000;
    pub const WBENCHMESSAGE: u32 = 0x0002_0000;
    pub const ACTIVEWINDOW: u32 = 0x0004_0000;
    pub const INACTIVEWINDOW: u32 = 0x0008_0000;
    pub const DELTAMOVE: u32 = 0x0010_0000;
    pub const VANILLAKEY: u32 = 0x0020_0000;
    pub const INTUITICKS: u32 = 0x0040_0000;
    pub const IDCMPUPDATE: u32 = 0x0080_0000;
    pub const MENUHELP: u32 = 0x0100_0000;
    pub const CHANGEWINDOW: u32 = 0x0200_0000;
    pub const GADGETHELP: u32 = 0x0400_0000;
}

/// Códigos y calificadores de `devices/inputevent.h` (`IECODE_*`, `IEQUALIFIER_*`).
pub mod ie {
    pub const IECODE_UP_PREFIX: u16 = 0x80;
    pub const IECODE_LBUTTON: u16 = 0x68;
    pub const IECODE_RBUTTON: u16 = 0x69;
    pub const IECODE_MBUTTON: u16 = 0x6A;
    pub const IECODE_NOBUTTON: u16 = 0xFF;
    pub const SELECTDOWN: u16 = IECODE_LBUTTON;
    pub const SELECTUP: u16 = IECODE_LBUTTON | IECODE_UP_PREFIX;
    pub const MENUDOWN: u16 = IECODE_RBUTTON;
    pub const MENUUP: u16 = IECODE_RBUTTON | IECODE_UP_PREFIX;
    pub const MIDDLEDOWN: u16 = IECODE_MBUTTON;
    pub const MIDDLEUP: u16 = IECODE_MBUTTON | IECODE_UP_PREFIX;

    pub const LSHIFT: u16 = 0x0001;
    pub const RSHIFT: u16 = 0x0002;
    pub const CAPSLOCK: u16 = 0x0004;
    pub const CONTROL: u16 = 0x0008;
    pub const LALT: u16 = 0x0010;
    pub const RALT: u16 = 0x0020;
    pub const LCOMMAND: u16 = 0x0040;
    pub const RCOMMAND: u16 = 0x0080;
    pub const NUMERICPAD: u16 = 0x0100;
    pub const REPEAT: u16 = 0x0200;
    pub const INTERRUPT: u16 = 0x0400;
    pub const MULTIBROADCAST: u16 = 0x0800;
    pub const MIDBUTTON: u16 = 0x1000;
    pub const RBUTTON: u16 = 0x2000;
    pub const LEFTBUTTON: u16 = 0x4000;
    pub const RELATIVEMOUSE: u16 = 0x8000;
}

/// Tipos de `NewWindow->Type`.
pub mod tipo_pantalla {
    pub const WBENCHSCREEN: u16 = 0x0001;
    pub const PUBLICSCREEN: u16 = 0x0002;
    pub const CUSTOMSCREEN: u16 = 0x000F;
}
