//! Disposición de las estructuras de exec en AmigaOS 3.1 (V40): desplazamientos y tamaños en bytes.
//!
//! Sacados de la interfaz pública: los includes del NDK 3.1 (`exec/nodes.h`, `lists.h`, `tasks.h`, `ports.h`,
//! `libraries.h`, `memory.h`, `semaphores.h`, `interrupts.h`, `execbase.h` y sus `.i`), que hacen de
//! especificación, y el ROM Kernel Reference Manual: Libraries. Los nombres de las constantes son los de los
//! campos en esos includes, para poder cotejarlos.
//!
//! En la memoria de la caja todo va en big-endian, los punteros son de 32 bits y no hay relleno: el 68k alinea a 2.

/// `struct Node`.
pub mod node {
    pub const LN_SUCC: u32 = 0;
    pub const LN_PRED: u32 = 4;
    pub const LN_TYPE: u32 = 8;
    pub const LN_PRI: u32 = 9;
    pub const LN_NAME: u32 = 10;
    pub const TAMANO: u32 = 14;
}

/// `struct MinNode`.
pub mod minnode {
    pub const MLN_SUCC: u32 = 0;
    pub const MLN_PRED: u32 = 4;
    pub const TAMANO: u32 = 8;
}

/// `struct List`.
pub mod list {
    pub const LH_HEAD: u32 = 0;
    pub const LH_TAIL: u32 = 4;
    pub const LH_TAILPRED: u32 = 8;
    pub const LH_TYPE: u32 = 12;
    pub const L_PAD: u32 = 13;
    pub const TAMANO: u32 = 14;
}

/// `struct MinList`.
pub mod minlist {
    pub const MLH_HEAD: u32 = 0;
    pub const MLH_TAIL: u32 = 4;
    pub const MLH_TAILPRED: u32 = 8;
    pub const TAMANO: u32 = 12;
}

/// Tipos de nodo (`ln_Type`).
pub mod nt {
    pub const UNKNOWN: u8 = 0;
    pub const TASK: u8 = 1;
    pub const INTERRUPT: u8 = 2;
    pub const DEVICE: u8 = 3;
    pub const MSGPORT: u8 = 4;
    pub const MESSAGE: u8 = 5;
    pub const FREEMSG: u8 = 6;
    pub const REPLYMSG: u8 = 7;
    pub const RESOURCE: u8 = 8;
    pub const LIBRARY: u8 = 9;
    pub const MEMORY: u8 = 10;
    pub const SOFTINT: u8 = 11;
    pub const FONT: u8 = 12;
    pub const PROCESS: u8 = 13;
    pub const SEMAPHORE: u8 = 14;
    pub const SIGNALSEM: u8 = 15;
    pub const BOOTNODE: u8 = 16;
    pub const KICKMEM: u8 = 17;
    pub const GRAPHICS: u8 = 18;
    pub const DEATHMESSAGE: u8 = 19;
    pub const USER: u8 = 254;
    pub const EXTENDED: u8 = 255;
}

/// `struct Task`.
pub mod task {
    pub const TC_NODE: u32 = 0;
    pub const TC_FLAGS: u32 = 14;
    pub const TC_STATE: u32 = 15;
    pub const TC_IDNESTCNT: u32 = 16;
    pub const TC_TDNESTCNT: u32 = 17;
    pub const TC_SIGALLOC: u32 = 18;
    pub const TC_SIGWAIT: u32 = 22;
    pub const TC_SIGRECVD: u32 = 26;
    pub const TC_SIGEXCEPT: u32 = 30;
    pub const TC_TRAPALLOC: u32 = 34;
    pub const TC_TRAPABLE: u32 = 36;
    pub const TC_EXCEPTDATA: u32 = 38;
    pub const TC_EXCEPTCODE: u32 = 42;
    pub const TC_TRAPDATA: u32 = 46;
    pub const TC_TRAPCODE: u32 = 50;
    pub const TC_SPREG: u32 = 54;
    pub const TC_SPLOWER: u32 = 58;
    pub const TC_SPUPPER: u32 = 62;
    pub const TC_SWITCH: u32 = 66;
    pub const TC_LAUNCH: u32 = 70;
    pub const TC_MEMENTRY: u32 = 74;
    pub const TC_USERDATA: u32 = 88;
    pub const TAMANO: u32 = 92;
}

/// Estados de una tarea (`tc_State`).
pub mod ts {
    pub const INVALID: u8 = 0;
    pub const ADDED: u8 = 1;
    pub const RUN: u8 = 2;
    pub const READY: u8 = 3;
    pub const WAIT: u8 = 4;
    pub const EXCEPT: u8 = 5;
    pub const REMOVED: u8 = 6;
}

/// Señales del sistema (`exec/tasks.h` y `dos/dos.h`).
pub mod sig {
    pub const SIGF_ABORT: u32 = 1 << 0;
    pub const SIGF_CHILD: u32 = 1 << 1;
    pub const SIGF_BLIT: u32 = 1 << 4;
    pub const SIGF_SINGLE: u32 = 1 << 4;
    pub const SIGF_INTUITION: u32 = 1 << 5;
    pub const SIGF_NET: u32 = 1 << 7;
    pub const SIGF_DOS: u32 = 1 << 8;
    pub const SIGBREAKF_CTRL_C: u32 = 1 << 12;
    pub const SIGBREAKF_CTRL_D: u32 = 1 << 13;
    pub const SIGBREAKF_CTRL_E: u32 = 1 << 14;
    pub const SIGBREAKF_CTRL_F: u32 = 1 << 15;
    /// Las 16 de abajo son del sistema: una tarea nueva las tiene ya reservadas.
    pub const DEL_SISTEMA: u32 = 0xFFFF;
}

/// `struct MsgPort`.
pub mod msgport {
    pub const MP_NODE: u32 = 0;
    pub const MP_FLAGS: u32 = 14;
    pub const MP_SIGBIT: u32 = 15;
    /// También `mp_SoftInt`, con `PA_SOFTINT`.
    pub const MP_SIGTASK: u32 = 16;
    pub const MP_MSGLIST: u32 = 20;
    pub const TAMANO: u32 = 34;

    pub const PF_ACTION: u8 = 3;
    pub const PA_SIGNAL: u8 = 0;
    pub const PA_SOFTINT: u8 = 1;
    pub const PA_IGNORE: u8 = 2;
}

/// `struct Message`.
pub mod message {
    pub const MN_NODE: u32 = 0;
    pub const MN_REPLYPORT: u32 = 14;
    pub const MN_LENGTH: u32 = 18;
    pub const TAMANO: u32 = 20;
}

/// `struct Library`.
pub mod library {
    pub const LIB_NODE: u32 = 0;
    pub const LIB_FLAGS: u32 = 14;
    pub const LIB_PAD: u32 = 15;
    pub const LIB_NEGSIZE: u32 = 16;
    pub const LIB_POSSIZE: u32 = 18;
    pub const LIB_VERSION: u32 = 20;
    pub const LIB_REVISION: u32 = 22;
    pub const LIB_IDSTRING: u32 = 24;
    pub const LIB_SUM: u32 = 28;
    pub const LIB_OPENCNT: u32 = 32;
    pub const TAMANO: u32 = 34;

    pub const LIBF_SUMMING: u8 = 1 << 0;
    pub const LIBF_CHANGED: u8 = 1 << 1;
    pub const LIBF_SUMUSED: u8 = 1 << 2;
    pub const LIBF_DELEXP: u8 = 1 << 3;

    /// Tamaño de una entrada de la tabla de saltos.
    pub const LIB_VECTSIZE: u32 = 6;
    /// Las cuatro entradas de toda biblioteca: Open, Close, Expunge y la reservada.
    pub const LIB_RESERVED: u32 = 4;
    /// La primera entrada (Open), en -6.
    pub const LIB_BASE: i32 = -(LIB_VECTSIZE as i32);
    /// La primera función propia de una biblioteca, en -30.
    pub const LIB_USERDEF: i32 = LIB_BASE - (LIB_RESERVED as i32) * (LIB_VECTSIZE as i32);
}

/// `struct MemChunk`.
pub mod memchunk {
    pub const MC_NEXT: u32 = 0;
    pub const MC_BYTES: u32 = 4;
    pub const TAMANO: u32 = 8;
}

/// `struct MemHeader`.
pub mod memheader {
    pub const MH_NODE: u32 = 0;
    pub const MH_ATTRIBUTES: u32 = 14;
    pub const MH_FIRST: u32 = 16;
    pub const MH_LOWER: u32 = 20;
    pub const MH_UPPER: u32 = 24;
    pub const MH_FREE: u32 = 28;
    pub const TAMANO: u32 = 32;
}

/// `struct MemEntry` y `struct MemList`.
pub mod memlist {
    pub const ME_REQS: u32 = 0;
    pub const ME_ADDR: u32 = 0;
    pub const ME_LENGTH: u32 = 4;
    pub const ME_TAMANO: u32 = 8;

    pub const ML_NODE: u32 = 0;
    pub const ML_NUMENTRIES: u32 = 14;
    pub const ML_ME: u32 = 16;
    /// `sizeof(struct MemList)`, con una entrada.
    pub const TAMANO: u32 = 24;
}

/// Atributos y opciones de memoria (`exec/memory.h`).
pub mod memf {
    pub const ANY: u32 = 0;
    pub const PUBLIC: u32 = 1 << 0;
    pub const CHIP: u32 = 1 << 1;
    pub const FAST: u32 = 1 << 2;
    pub const LOCAL: u32 = 1 << 8;
    pub const DMA24BIT: u32 = 1 << 9;
    pub const KICK: u32 = 1 << 10;
    pub const CLEAR: u32 = 1 << 16;
    pub const LARGEST: u32 = 1 << 17;
    pub const REVERSE: u32 = 1 << 18;
    pub const TOTAL: u32 = 1 << 19;
    pub const NO_EXPUNGE: u32 = 1 << 31;
    /// Los bits que son atributos de la memoria (los de abajo); el resto son opciones de la petición.
    pub const ATRIBUTOS: u32 = 0xFFFF;
    /// Todo se reparte en bloques de 8 bytes.
    pub const MEM_BLOCKSIZE: u32 = 8;
}

/// `struct SignalSemaphore`.
pub mod signalsemaphore {
    pub const SS_LINK: u32 = 0;
    pub const SS_NESTCOUNT: u32 = 14;
    pub const SS_WAITQUEUE: u32 = 16;
    pub const SS_MULTIPLELINK: u32 = 28;
    pub const SS_OWNER: u32 = 40;
    pub const SS_QUEUECOUNT: u32 = 44;
    pub const TAMANO: u32 = 46;
}

/// `struct Interrupt`, `struct IntVector` y `struct SoftIntList`.
pub mod interrupt {
    pub const IS_NODE: u32 = 0;
    pub const IS_DATA: u32 = 14;
    pub const IS_CODE: u32 = 18;
    pub const TAMANO: u32 = 22;

    pub const IV_DATA: u32 = 0;
    pub const IV_CODE: u32 = 4;
    pub const IV_NODE: u32 = 8;
    pub const IV_TAMANO: u32 = 12;

    pub const SH_LIST: u32 = 0;
    pub const SH_PAD: u32 = 14;
    pub const SH_TAMANO: u32 = 16;
}

/// `struct ExecBase` de la V39/V40.
pub mod execbase {
    pub const LIBNODE: u32 = 0;
    pub const SOFTVER: u32 = 34;
    pub const LOWMEMCHKSUM: u32 = 36;
    pub const CHKBASE: u32 = 38;
    pub const COLDCAPTURE: u32 = 42;
    pub const COOLCAPTURE: u32 = 46;
    pub const WARMCAPTURE: u32 = 50;
    pub const SYSSTKUPPER: u32 = 54;
    pub const SYSSTKLOWER: u32 = 58;
    pub const MAXLOCMEM: u32 = 62;
    pub const DEBUGENTRY: u32 = 66;
    pub const DEBUGDATA: u32 = 70;
    pub const ALERTDATA: u32 = 74;
    pub const MAXEXTMEM: u32 = 78;
    pub const CHKSUM: u32 = 82;
    /// 16 `IntVector` de 12 bytes.
    pub const INTVECTS: u32 = 84;
    pub const THISTASK: u32 = 276;
    pub const IDLECOUNT: u32 = 280;
    pub const DISPCOUNT: u32 = 284;
    pub const QUANTUM: u32 = 288;
    pub const ELAPSED: u32 = 290;
    pub const SYSFLAGS: u32 = 292;
    pub const IDNESTCNT: u32 = 294;
    pub const TDNESTCNT: u32 = 295;
    pub const ATTNFLAGS: u32 = 296;
    pub const ATTNRESCHED: u32 = 298;
    pub const RESMODULES: u32 = 300;
    pub const TASKTRAPCODE: u32 = 304;
    pub const TASKEXCEPTCODE: u32 = 308;
    pub const TASKEXITCODE: u32 = 312;
    pub const TASKSIGALLOC: u32 = 316;
    pub const TASKTRAPALLOC: u32 = 320;
    pub const MEMLIST: u32 = 322;
    pub const RESOURCELIST: u32 = 336;
    pub const DEVICELIST: u32 = 350;
    pub const INTRLIST: u32 = 364;
    pub const LIBLIST: u32 = 378;
    pub const PORTLIST: u32 = 392;
    pub const TASKREADY: u32 = 406;
    pub const TASKWAIT: u32 = 420;
    /// 5 `SoftIntList` de 16 bytes.
    pub const SOFTINTS: u32 = 434;
    /// 4 LONG.
    pub const LASTALERT: u32 = 514;
    pub const VBLANKFREQUENCY: u32 = 530;
    pub const POWERSUPPLYFREQUENCY: u32 = 531;
    pub const SEMAPHORELIST: u32 = 532;
    pub const KICKMEMPTR: u32 = 546;
    pub const KICKTAGPTR: u32 = 550;
    pub const KICKCHECKSUM: u32 = 554;
    pub const EX_PAD0: u32 = 558;
    pub const EX_LAUNCHPOINT: u32 = 560;
    pub const EX_RAMLIBPRIVATE: u32 = 564;
    pub const EX_ECLOCKFREQUENCY: u32 = 568;
    pub const EX_CACHECONTROL: u32 = 572;
    pub const EX_TASKID: u32 = 576;
    pub const EX_RESERVED1: u32 = 580;
    pub const EX_MMULOCK: u32 = 600;
    pub const EX_RESERVED2: u32 = 604;
    pub const EX_MEMHANDLERS: u32 = 616;
    pub const EX_MEMHANDLER: u32 = 628;
    pub const TAMANO: u32 = 632;

    /// `AttnFlags`.
    pub const AFF_68010: u16 = 1 << 0;
    pub const AFF_68020: u16 = 1 << 1;
    pub const AFF_68030: u16 = 1 << 2;
    pub const AFF_68040: u16 = 1 << 3;
    pub const AFF_68881: u16 = 1 << 4;
    pub const AFF_68882: u16 = 1 << 5;
    pub const AFF_FPU40: u16 = 1 << 6;
}
