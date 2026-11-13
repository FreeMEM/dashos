//! Los números de BOOPSI: métodos, atributos y estructuras de `intuition/classes.h`, `classusr.h`, `gadgetclass.h`,
//! `imageclass.h`, `icclass.h`, `cghooks.h` y `sghooks.h` del NDK 3.2 (la interfaz; nada de código).
#![allow(dead_code)]

const TAG_USER: u32 = 0x8000_0000;

/// `struct IClass` (`intuition/classes.h`): empieza por su `struct Hook` (el despachador).
pub mod iclass {
    pub const H_SUCC: u32 = 0;
    pub const H_PRED: u32 = 4;
    pub const H_ENTRY: u32 = 8;
    pub const H_SUBENTRY: u32 = 12;
    pub const H_DATA: u32 = 16;
    pub const RESERVED: u32 = 20;
    pub const SUPER: u32 = 24;
    pub const ID: u32 = 28;
    pub const INSTOFFSET: u32 = 32;
    pub const INSTSIZE: u32 = 34;
    pub const USERDATA: u32 = 36;
    pub const SUBCLASSCOUNT: u32 = 40;
    pub const OBJECTCOUNT: u32 = 44;
    pub const FLAGS: u32 = 48;
    pub const TAMANO: u32 = 52;
    pub const CLF_INLIST: u32 = 1;
}

/// `struct _Object`, delante del objeto: `o_Node` (8) y `o_Class` (4).
pub const CABECERA: u32 = 12;

/// Métodos de `rootclass` (`classusr.h`).
pub mod om {
    pub const NEW: u32 = 0x101;
    pub const DISPOSE: u32 = 0x102;
    pub const SET: u32 = 0x103;
    pub const GET: u32 = 0x104;
    pub const ADDTAIL: u32 = 0x105;
    pub const REMOVE: u32 = 0x106;
    pub const NOTIFY: u32 = 0x107;
    pub const UPDATE: u32 = 0x108;
    pub const ADDMEMBER: u32 = 0x109;
    pub const REMMEMBER: u32 = 0x10A;
    pub const OPUF_INTERIM: u32 = 1;
}

/// `icclass.h`.
pub mod ica {
    pub const TARGET: u32 = super::TAG_USER + 0x40000 + 1;
    pub const MAP: u32 = super::TAG_USER + 0x40000 + 2;
    pub const SPECIALCODE: u32 = super::TAG_USER + 0x40000 + 3;
    pub const TARGET_IDCMP: u32 = !0;
    pub const ICM_SETLOOP: u32 = 0x402;
    pub const ICM_CLEARLOOP: u32 = 0x403;
    pub const ICM_CHECKLOOP: u32 = 0x404;
}

/// Métodos de `gadgetclass` y lo que devuelven (`gadgetclass.h`).
pub mod gm {
    pub const HITTEST: u32 = 0;
    pub const RENDER: u32 = 1;
    pub const GOACTIVE: u32 = 2;
    pub const HANDLEINPUT: u32 = 3;
    pub const GOINACTIVE: u32 = 4;
    pub const HELPTEST: u32 = 5;
    pub const LAYOUT: u32 = 6;
    pub const DOMAIN: u32 = 7;
    pub const GMR_GADGETHIT: u32 = 4;
    pub const GMR_MEACTIVE: u32 = 0;
    pub const GMR_NOREUSE: u32 = 2;
    pub const GMR_REUSE: u32 = 4;
    pub const GMR_VERIFY: u32 = 8;
    pub const GMR_NEXTACTIVE: u32 = 16;
    pub const GMR_PREVACTIVE: u32 = 32;
    pub const GREDRAW_UPDATE: u32 = 2;
    pub const GREDRAW_REDRAW: u32 = 1;
    pub const GREDRAW_TOGGLE: u32 = 0;
    pub const GDOMAIN_MINIMUM: u32 = 0;
    pub const GDOMAIN_NOMINAL: u32 = 1;
    pub const GDOMAIN_MAXIMUM: u32 = 2;
}

/// Atributos de `gadgetclass`.
pub mod ga {
    const D: u32 = super::TAG_USER + 0x30000;
    pub const LEFT: u32 = D + 1;
    pub const RELRIGHT: u32 = D + 2;
    pub const TOP: u32 = D + 3;
    pub const RELBOTTOM: u32 = D + 4;
    pub const WIDTH: u32 = D + 5;
    pub const RELWIDTH: u32 = D + 6;
    pub const HEIGHT: u32 = D + 7;
    pub const RELHEIGHT: u32 = D + 8;
    pub const TEXT: u32 = D + 9;
    pub const IMAGE: u32 = D + 10;
    pub const BORDER: u32 = D + 11;
    pub const SELECTRENDER: u32 = D + 12;
    pub const HIGHLIGHT: u32 = D + 13;
    pub const DISABLED: u32 = D + 14;
    pub const GZZGADGET: u32 = D + 15;
    pub const ID: u32 = D + 16;
    pub const USERDATA: u32 = D + 17;
    pub const SPECIALINFO: u32 = D + 18;
    pub const SELECTED: u32 = D + 19;
    pub const ENDGADGET: u32 = D + 20;
    pub const IMMEDIATE: u32 = D + 21;
    pub const RELVERIFY: u32 = D + 22;
    pub const FOLLOWMOUSE: u32 = D + 23;
    pub const RIGHTBORDER: u32 = D + 24;
    pub const LEFTBORDER: u32 = D + 25;
    pub const TOPBORDER: u32 = D + 26;
    pub const BOTTOMBORDER: u32 = D + 27;
    pub const TOGGLESELECT: u32 = D + 28;
    pub const SYSGADGET: u32 = D + 29;
    pub const SYSGTYPE: u32 = D + 30;
    pub const PREVIOUS: u32 = D + 31;
    pub const NEXT: u32 = D + 32;
    pub const DRAWINFO: u32 = D + 33;
    pub const INTUITEXT: u32 = D + 34;
    pub const LABELIMAGE: u32 = D + 35;
    pub const TABCYCLE: u32 = D + 36;
    pub const GADGETHELP: u32 = D + 37;
    pub const BOUNDS: u32 = D + 38;
    pub const RELSPECIAL: u32 = D + 39;
    pub const TEXTATTR: u32 = D + 40;
    pub const READONLY: u32 = D + 41;
    pub const UNDERSCORE: u32 = D + 42;
    pub const ACTIVATEKEY: u32 = D + 43;
    pub const BACKFILL: u32 = D + 44;
    pub const HIDDEN: u32 = D + 54;
}

/// Atributos de `propgclass`.
pub mod pga {
    const D: u32 = super::TAG_USER + 0x31000;
    pub const FREEDOM: u32 = D + 1;
    pub const BORDERLESS: u32 = D + 2;
    pub const HORIZPOT: u32 = D + 3;
    pub const HORIZBODY: u32 = D + 4;
    pub const VERTPOT: u32 = D + 5;
    pub const VERTBODY: u32 = D + 6;
    pub const TOTAL: u32 = D + 7;
    pub const VISIBLE: u32 = D + 8;
    pub const TOP: u32 = D + 9;
    pub const NEWLOOK: u32 = D + 10;
    pub const FREEHORIZ: u32 = 2;
    pub const FREEVERT: u32 = 4;
}

/// Atributos de `strgclass`.
pub mod stringa {
    const D: u32 = super::TAG_USER + 0x32000;
    pub const MAXCHARS: u32 = D + 1;
    pub const BUFFER: u32 = D + 2;
    pub const UNDOBUFFER: u32 = D + 3;
    pub const WORKBUFFER: u32 = D + 4;
    pub const BUFFERPOS: u32 = D + 5;
    pub const DISPPOS: u32 = D + 6;
    pub const ALTKEYMAP: u32 = D + 7;
    pub const FONT: u32 = D + 8;
    pub const PENS: u32 = D + 9;
    pub const ACTIVEPENS: u32 = D + 10;
    pub const EDITHOOK: u32 = D + 11;
    pub const EDITMODES: u32 = D + 12;
    pub const REPLACEMODE: u32 = D + 13;
    pub const FIXEDFIELDMODE: u32 = D + 14;
    pub const NOFILTERMODE: u32 = D + 15;
    pub const JUSTIFICATION: u32 = D + 16;
    pub const LONGVAL: u32 = D + 17;
    pub const TEXTVAL: u32 = D + 18;
    pub const EXITHELP: u32 = D + 19;
}

/// `layout` de `gadgetclass.h` (para `groupgclass`).
pub mod layouta {
    const D: u32 = super::TAG_USER + 0x38000;
    pub const LAYOUTOBJ: u32 = D + 1;
    pub const SPACING: u32 = D + 2;
    pub const ORIENTATION: u32 = D + 3;
}

/// Atributos y métodos de `imageclass`.
pub mod ia {
    const D: u32 = super::TAG_USER + 0x20000;
    pub const LEFT: u32 = D + 1;
    pub const TOP: u32 = D + 2;
    pub const WIDTH: u32 = D + 3;
    pub const HEIGHT: u32 = D + 4;
    pub const FGPEN: u32 = D + 5;
    pub const BGPEN: u32 = D + 6;
    pub const DATA: u32 = D + 7;
    pub const LINEWIDTH: u32 = D + 8;
    pub const SHADOWPEN: u32 = D + 9;
    pub const HIGHLIGHTPEN: u32 = D + 10;
    pub const PENS: u32 = D + 0x0E;
    pub const RESOLUTION: u32 = D + 0x0F;
    pub const APATTERN: u32 = D + 0x10;
    pub const APATSIZE: u32 = D + 0x11;
    pub const MODE: u32 = D + 0x12;
    pub const FONT: u32 = D + 0x13;
    pub const OUTLINE: u32 = D + 0x14;
    pub const RECESSED: u32 = D + 0x15;
    pub const DOUBLEEMBOSS: u32 = D + 0x16;
    pub const EDGESONLY: u32 = D + 0x17;
    pub const SYSIA_SIZE: u32 = D + 0x0B;
    pub const SYSIA_DEPTH: u32 = D + 0x0C;
    pub const SYSIA_WHICH: u32 = D + 0x0D;
    pub const SYSIA_DRAWINFO: u32 = D + 0x18;
    pub const FRAMETYPE: u32 = D + 0x1B;
    pub const LABEL: u32 = D + 0x28;
}

pub mod im {
    pub const DRAW: u32 = 0x202;
    pub const HITTEST: u32 = 0x203;
    pub const ERASE: u32 = 0x204;
    pub const MOVE: u32 = 0x205;
    pub const DRAWFRAME: u32 = 0x206;
    pub const FRAMEBOX: u32 = 0x207;
    pub const HITFRAME: u32 = 0x208;
    pub const ERASEFRAME: u32 = 0x209;
    pub const DOMAINFRAME: u32 = 0x20A;
}

/// Estados de `IM_DRAW`.
pub mod ids {
    pub const NORMAL: u32 = 0;
    pub const SELECTED: u32 = 1;
    pub const DISABLED: u32 = 2;
    pub const SELECTEDDISABLED: u32 = 8;
}

/// `SYSIA_Which`.
pub mod sysi {
    pub const DEPTH: u32 = 0x00;
    pub const ZOOM: u32 = 0x01;
    pub const SIZE: u32 = 0x02;
    pub const CLOSE: u32 = 0x03;
    pub const SDEPTH: u32 = 0x05;
    pub const LEFT: u32 = 0x0A;
    pub const UP: u32 = 0x0B;
    pub const RIGHT: u32 = 0x0C;
    pub const DOWN: u32 = 0x0D;
    pub const CHECK: u32 = 0x0E;
    pub const MX: u32 = 0x0F;
    pub const MENUCHECK: u32 = 0x10;
    pub const AMIGAKEY: u32 = 0x11;
}

/// `IA_FrameType` de `frameiclass`.
pub mod frame {
    pub const DEFAULT: u32 = 0;
    pub const BUTTON: u32 = 1;
    pub const RIDGE: u32 = 2;
    pub const ICONDROPBOX: u32 = 3;
}

/// `struct Image` (`intuition/intuition.h`): el principio de los objetos de `imageclass`.
pub mod image {
    pub const LEFTEDGE: u32 = 0;
    pub const TOPEDGE: u32 = 2;
    pub const WIDTH: u32 = 4;
    pub const HEIGHT: u32 = 6;
    pub const DEPTH: u32 = 8;
    pub const IMAGEDATA: u32 = 10;
    pub const PLANEPICK: u32 = 14;
    pub const PLANEONOFF: u32 = 15;
    pub const NEXTIMAGE: u32 = 16;
    pub const TAMANO: u32 = 20;
    /// `Depth` de un objeto de imagen (`CUSTOMIMAGEDEPTH`).
    pub const CUSTOMIMAGEDEPTH: u16 = 0xFFFF;
}

/// `struct GadgetInfo` (`intuition/cghooks.h`).
pub mod ginfo {
    pub const SCREEN: u32 = 0;
    pub const WINDOW: u32 = 4;
    pub const REQUESTER: u32 = 8;
    pub const RASTPORT: u32 = 12;
    pub const LAYER: u32 = 16;
    pub const DOMAIN: u32 = 20;
    pub const PENS: u32 = 28;
    pub const DRINFO: u32 = 30;
    pub const TAMANO: u32 = 58;
}

/// `struct InputEvent` (`devices/inputevent.h`).
pub mod ievent {
    pub const NEXTEVENT: u32 = 0;
    pub const CLASS: u32 = 4;
    pub const SUBCLASS: u32 = 5;
    pub const CODE: u32 = 6;
    pub const QUALIFIER: u32 = 8;
    pub const X: u32 = 10;
    pub const Y: u32 = 12;
    pub const TIMESTAMP: u32 = 14;
    pub const TAMANO: u32 = 22;
    pub const CLASS_RAWKEY: u8 = 1;
    pub const CLASS_RAWMOUSE: u8 = 2;
    pub const CLASS_TIMER: u8 = 6;
    pub const LBUTTON: u16 = 0x68;
    pub const UP_PREFIX: u16 = 0x80;
    pub const NOBUTTON: u16 = 0xFF;
}

/// `struct SGWork` y lo de los ganchos de edición (`intuition/sghooks.h`).
pub mod sgw {
    pub const GADGET: u32 = 0;
    pub const STRINGINFO: u32 = 4;
    pub const WORKBUFFER: u32 = 8;
    pub const PREVBUFFER: u32 = 12;
    pub const MODES: u32 = 16;
    pub const IEVENT: u32 = 20;
    pub const CODE: u32 = 24;
    pub const BUFFERPOS: u32 = 26;
    pub const NUMCHARS: u32 = 28;
    pub const ACTIONS: u32 = 30;
    pub const LONGINT: u32 = 34;
    pub const GADGETINFO: u32 = 38;
    pub const EDITOP: u32 = 42;
    pub const TAMANO: u32 = 44;
    pub const SGH_KEY: u32 = 1;
    pub const SGA_USE: u32 = 1;
    pub const SGA_END: u32 = 2;
    pub const SGA_BEEP: u32 = 4;
    pub const SGA_REUSE: u32 = 8;
    pub const SGA_REDISPLAY: u32 = 0x10;
    pub const SGM_LONGINT: u32 = 1 << 6;
    pub const EO_NOOP: u16 = 1;
    pub const EO_DELBACKWARD: u16 = 2;
    pub const EO_DELFORWARD: u16 = 3;
    pub const EO_MOVECURSOR: u16 = 4;
    pub const EO_ENTER: u16 = 5;
    pub const EO_INSERTCHAR: u16 = 8;
    pub const EO_BADFORMAT: u16 = 9;
    pub const EO_CLEAR: u16 = 0xC;
}

/// `struct StringInfo` (`intuition/intuition.h`).
pub mod sinfo {
    pub const BUFFER: u32 = 0;
    pub const UNDOBUFFER: u32 = 4;
    pub const BUFFERPOS: u32 = 8;
    pub const MAXCHARS: u32 = 10;
    pub const DISPPOS: u32 = 12;
    pub const UNDOPOS: u32 = 14;
    pub const NUMCHARS: u32 = 16;
    pub const DISPCOUNT: u32 = 18;
    pub const CLEFT: u32 = 20;
    pub const CTOP: u32 = 22;
    pub const EXTENSION: u32 = 24;
    pub const LONGINT: u32 = 28;
    pub const ALTKEYMAP: u32 = 32;
    pub const TAMANO: u32 = 36;
}

/// `struct PropInfo`.
pub mod pinfo {
    pub const FLAGS: u32 = 0;
    pub const HORIZPOT: u32 = 2;
    pub const VERTPOT: u32 = 4;
    pub const HORIZBODY: u32 = 6;
    pub const VERTBODY: u32 = 8;
    pub const TAMANO: u32 = 22;
}

/// Lo de `struct Gadget` que no está en `gadtools::gadget`.
pub mod gflg {
    pub const GADGHNONE: u16 = 0x0003;
    pub const GADGIMAGE: u16 = 0x0004;
    pub const RELBOTTOM: u16 = 0x0008;
    pub const RELRIGHT: u16 = 0x0010;
    pub const RELWIDTH: u16 = 0x0020;
    pub const RELHEIGHT: u16 = 0x0040;
    pub const SELECTED: u16 = 0x0080;
    pub const DISABLED: u16 = 0x0100;
    pub const TABCYCLE: u16 = 0x0200;
    pub const LABELMASK: u16 = 0x3000;
    pub const LABELITEXT: u16 = 0x0000;
    pub const LABELSTRING: u16 = 0x1000;
    pub const LABELIMAGE: u16 = 0x2000;
    pub const RELSPECIAL: u16 = 0x4000;
    pub const EXTENDED: u16 = 0x8000;
}

pub mod gact {
    pub const RELVERIFY: u16 = 0x0001;
    pub const IMMEDIATE: u16 = 0x0002;
    pub const ENDGADGET: u16 = 0x0004;
    pub const FOLLOWMOUSE: u16 = 0x0008;
    pub const RIGHTBORDER: u16 = 0x0010;
    pub const LEFTBORDER: u16 = 0x0020;
    pub const TOPBORDER: u16 = 0x0040;
    pub const BOTTOMBORDER: u16 = 0x0080;
    pub const TOGGLESELECT: u16 = 0x0100;
    pub const LONGINT: u16 = 0x0800;
    pub const ACTIVEGADGET: u16 = 0x4000;
}

pub mod gtyp {
    pub const BOOLGADGET: u16 = 1;
    pub const PROPGADGET: u16 = 3;
    pub const STRGADGET: u16 = 4;
    pub const CUSTOMGADGET: u16 = 5;
    pub const GTYPEMASK: u16 = 7;
    pub const GZZGADGET: u16 = 0x2000;
    pub const SYSGADGET: u16 = 0x8000;
}

/// `struct ExtGadget`: la `struct Gadget` (44) más `MoreFlags` y los `Bounds`.
pub const TAMANO_GADGET: u32 = 56;

/// `struct IntuiText`.
pub mod itext {
    pub const FRONTPEN: u32 = 0;
    pub const BACKPEN: u32 = 1;
    pub const DRAWMODE: u32 = 2;
    pub const LEFTEDGE: u32 = 4;
    pub const TOPEDGE: u32 = 6;
    pub const ITEXTFONT: u32 = 8;
    pub const ITEXT: u32 = 12;
    pub const NEXTTEXT: u32 = 16;
}
