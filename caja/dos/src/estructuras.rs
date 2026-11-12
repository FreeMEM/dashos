//! Las estructuras de dos.library 40 tal como las ve un programa 68k: desplazamientos y tamaños de los includes del
//! NDK 3.1 (`dos/dos.h`, `dos/dosextens.h`, `dos/rdargs.h`, `dos/var.h`), que son la especificación de la interfaz.
//! Todo va en big-endian y con punteros de 32 bits. `tests/estructuras.rs` fija cada número.
//!
//! BPTR y BSTR (de BCPL): un BPTR es una dirección dividida entre 4 (la estructura tiene que estar alineada a 4); un
//! BSTR es un BPTR a un byte de largo seguido de los caracteres, sin 0 al final.

/// Valores de verdad de dos.
pub const DOSTRUE: u32 = 0xFFFF_FFFF;
pub const DOSFALSE: u32 = 0;

/// Modos de `Open`.
pub const MODE_OLDFILE: i32 = 1005;
pub const MODE_NEWFILE: i32 = 1006;
pub const MODE_READWRITE: i32 = 1004;

/// Modos de `Lock` (`ACCESS_READ` y `ACCESS_WRITE` son los mismos números).
pub const SHARED_LOCK: i32 = -2;
pub const EXCLUSIVE_LOCK: i32 = -1;

/// Modos de `Seek` y `SetFileSize`.
pub const OFFSET_BEGINNING: i32 = -1;
pub const OFFSET_CURRENT: i32 = 0;
pub const OFFSET_END: i32 = 1;

/// Lo que devuelve `SameLock`.
pub const LOCK_DIFFERENT: i32 = -1;
pub const LOCK_SAME: i32 = 0;
pub const LOCK_SAME_VOLUME: i32 = 1;

/// Tics por segundo de un `DateStamp`, y días de 1970 a 1978 (el principio de las fechas de AmigaDOS).
pub const TICKS_PER_SECOND: u32 = 50;
pub const DIAS_1970_A_1978: u64 = 2922;

/// `struct DateStamp`: 12 bytes.
pub mod datestamp {
    pub const DS_DAYS: u32 = 0;
    pub const DS_MINUTE: u32 = 4;
    pub const DS_TICK: u32 = 8;
    pub const TAMANO: u32 = 12;
}

/// `struct FileInfoBlock`: 260 bytes. Desde 2.0, `fib_FileName` y `fib_Comment` salen de `Examine` como cadenas C.
pub mod fib {
    pub const FIB_DISKKEY: u32 = 0;
    pub const FIB_DIRENTRYTYPE: u32 = 4;
    pub const FIB_FILENAME: u32 = 8;
    pub const LARGO_NOMBRE: u32 = 108;
    pub const FIB_PROTECTION: u32 = 116;
    pub const FIB_ENTRYTYPE: u32 = 120;
    pub const FIB_SIZE: u32 = 124;
    pub const FIB_NUMBLOCKS: u32 = 128;
    pub const FIB_DATE: u32 = 132;
    pub const FIB_COMMENT: u32 = 144;
    pub const LARGO_COMENTARIO: u32 = 80;
    pub const FIB_OWNERUID: u32 = 224;
    pub const FIB_OWNERGID: u32 = 226;
    pub const FIB_RESERVED: u32 = 228;
    pub const TAMANO: u32 = 260;
}

/// Bits de `fib_Protection`. Los cuatro de abajo (RWED del dueño) van al revés: a 1, no se puede. Los de grupo y
/// otros, al derecho: a 1, sí se puede.
pub mod fibf {
    pub const DELETE: u32 = 1 << 0;
    pub const EXECUTE: u32 = 1 << 1;
    pub const WRITE: u32 = 1 << 2;
    pub const READ: u32 = 1 << 3;
    pub const ARCHIVE: u32 = 1 << 4;
    pub const PURE: u32 = 1 << 5;
    pub const SCRIPT: u32 = 1 << 6;
    /// H ("hold"): residente.
    pub const HOLD: u32 = 1 << 7;
    pub const GRP_DELETE: u32 = 1 << 8;
    pub const GRP_EXECUTE: u32 = 1 << 9;
    pub const GRP_WRITE: u32 = 1 << 10;
    pub const GRP_READ: u32 = 1 << 11;
    pub const OTR_DELETE: u32 = 1 << 12;
    pub const OTR_EXECUTE: u32 = 1 << 13;
    pub const OTR_WRITE: u32 = 1 << 14;
    pub const OTR_READ: u32 = 1 << 15;
}

/// Tipos de entrada (`fib_DirEntryType`): mayor que 0, un cajón; menor, un fichero.
pub mod st {
    pub const ROOT: i32 = 1;
    pub const USERDIR: i32 = 2;
    pub const SOFTLINK: i32 = 3;
    pub const LINKDIR: i32 = 4;
    pub const FILE: i32 = -3;
    pub const LINKFILE: i32 = -4;
}

/// `struct FileHandle`: 44 bytes. `Open` devuelve un BPTR a ella.
pub mod filehandle {
    pub const FH_LINK: u32 = 0;
    /// `fh_Interactive` en los includes de 3.1: distinto de 0 si es una consola.
    pub const FH_PORT: u32 = 4;
    /// El puerto del gestor; 0 en `NIL:`.
    pub const FH_TYPE: u32 = 8;
    pub const FH_BUF: u32 = 12;
    pub const FH_POS: u32 = 16;
    pub const FH_END: u32 = 20;
    pub const FH_FUNCS: u32 = 24;
    pub const FH_FUNC2: u32 = 28;
    pub const FH_FUNC3: u32 = 32;
    pub const FH_ARGS: u32 = 36;
    pub const FH_ARG2: u32 = 40;
    pub const TAMANO: u32 = 44;
}

/// `struct FileLock`: 20 bytes. `Lock` devuelve un BPTR a ella.
pub mod filelock {
    pub const FL_LINK: u32 = 0;
    pub const FL_KEY: u32 = 4;
    pub const FL_ACCESS: u32 = 8;
    pub const FL_TASK: u32 = 12;
    pub const FL_VOLUME: u32 = 16;
    pub const TAMANO: u32 = 20;
}

/// `struct InfoData`: 36 bytes.
pub mod infodata {
    pub const ID_NUMSOFTERRORS: u32 = 0;
    pub const ID_UNITNUMBER: u32 = 4;
    pub const ID_DISKSTATE: u32 = 8;
    pub const ID_NUMBLOCKS: u32 = 12;
    pub const ID_NUMBLOCKSUSED: u32 = 16;
    pub const ID_BYTESPERBLOCK: u32 = 20;
    pub const ID_DISKTYPE: u32 = 24;
    pub const ID_VOLUMENODE: u32 = 28;
    pub const ID_INUSE: u32 = 32;
    pub const TAMANO: u32 = 36;

    pub const ID_WRITE_PROTECTED: u32 = 80;
    pub const ID_VALIDATING: u32 = 81;
    pub const ID_VALIDATED: u32 = 82;
    /// `DOS\3`: FFS internacional (sin distinguir mayúsculas, también con acentos), lo más parecido a DashFS.
    pub const ID_INTER_FFS_DISK: u32 = 0x444F_5303;
}

/// `struct DosList`: 44 bytes. La unión de 16 a 39 cambia con el tipo.
pub mod doslist {
    pub const DOL_NEXT: u32 = 0;
    pub const DOL_TYPE: u32 = 4;
    pub const DOL_TASK: u32 = 8;
    pub const DOL_LOCK: u32 = 12;
    // Dispositivo.
    pub const DOL_HANDLER: u32 = 16;
    pub const DOL_STACKSIZE: u32 = 20;
    pub const DOL_PRIORITY: u32 = 24;
    pub const DOL_STARTUP: u32 = 28;
    pub const DOL_SEGLIST: u32 = 32;
    pub const DOL_GLOBVEC: u32 = 36;
    // Volumen.
    pub const DOL_VOLUMEDATE: u32 = 16;
    pub const DOL_LOCKLIST: u32 = 28;
    pub const DOL_DISKTYPE: u32 = 32;
    // Asignación.
    pub const DOL_ASSIGNNAME: u32 = 16;
    pub const DOL_LIST: u32 = 20;
    pub const DOL_NAME: u32 = 40;
    pub const TAMANO: u32 = 44;

    pub const DLT_DEVICE: u32 = 0;
    pub const DLT_DIRECTORY: u32 = 1;
    pub const DLT_VOLUME: u32 = 2;
    pub const DLT_LATE: u32 = 3;
    pub const DLT_NONBINDING: u32 = 4;
}

/// `struct AssignList`: 8 bytes, los cajones de más de una multiasignación.
pub mod assignlist {
    pub const AL_NEXT: u32 = 0;
    pub const AL_LOCK: u32 = 4;
    pub const TAMANO: u32 = 8;
}

/// Marcas de `LockDosList`, `NextDosEntry` y `FindDosEntry`.
pub mod ldf {
    pub const READ: u32 = 1;
    pub const WRITE: u32 = 2;
    pub const DEVICES: u32 = 4;
    pub const VOLUMES: u32 = 8;
    pub const ASSIGNS: u32 = 16;
    pub const ENTRIES: u32 = 32;
    pub const DELETE: u32 = 64;
    pub const ALL: u32 = DEVICES | VOLUMES | ASSIGNS;
}

/// `struct Process`: 228 bytes. Empieza con una `Task` (92) y su `MsgPort` (34).
pub mod process {
    pub const LN_TYPE: u32 = 8;
    pub const NT_PROCESS: u8 = 13;
    pub const PR_MSGPORT: u32 = 92;
    pub const PR_PAD: u32 = 126;
    pub const PR_SEGLIST: u32 = 128;
    pub const PR_STACKSIZE: u32 = 132;
    pub const PR_GLOBVEC: u32 = 136;
    pub const PR_TASKNUM: u32 = 140;
    pub const PR_STACKBASE: u32 = 144;
    pub const PR_RESULT2: u32 = 148;
    pub const PR_CURRENTDIR: u32 = 152;
    pub const PR_CIS: u32 = 156;
    pub const PR_COS: u32 = 160;
    pub const PR_CONSOLETASK: u32 = 164;
    pub const PR_FILESYSTEMTASK: u32 = 168;
    pub const PR_CLI: u32 = 172;
    pub const PR_RETURNADDR: u32 = 176;
    pub const PR_PKTWAIT: u32 = 180;
    pub const PR_WINDOWPTR: u32 = 184;
    pub const PR_HOMEDIR: u32 = 188;
    pub const PR_FLAGS: u32 = 192;
    pub const PR_EXITCODE: u32 = 196;
    pub const PR_EXITDATA: u32 = 200;
    pub const PR_ARGUMENTS: u32 = 204;
    pub const PR_LOCALVARS: u32 = 208;
    pub const PR_SHELLPRIVATE: u32 = 220;
    pub const PR_CES: u32 = 224;
    pub const TAMANO: u32 = 228;
}

/// `struct CommandLineInterface`: 64 bytes.
pub mod cli {
    pub const CLI_RESULT2: u32 = 0;
    pub const CLI_SETNAME: u32 = 4;
    pub const CLI_COMMANDDIR: u32 = 8;
    pub const CLI_RETURNCODE: u32 = 12;
    pub const CLI_COMMANDNAME: u32 = 16;
    pub const CLI_FAILLEVEL: u32 = 20;
    pub const CLI_PROMPT: u32 = 24;
    pub const CLI_STANDARDINPUT: u32 = 28;
    pub const CLI_CURRENTINPUT: u32 = 32;
    pub const CLI_COMMANDFILE: u32 = 36;
    pub const CLI_INTERACTIVE: u32 = 40;
    pub const CLI_BACKGROUND: u32 = 44;
    pub const CLI_CURRENTOUTPUT: u32 = 48;
    pub const CLI_DEFAULTSTACK: u32 = 52;
    pub const CLI_STANDARDOUTPUT: u32 = 56;
    pub const CLI_MODULE: u32 = 60;
    pub const TAMANO: u32 = 64;
}

/// `struct CSource` (12 bytes) y `struct RDArgs` (32 bytes), de `dos/rdargs.h`.
pub mod rdargs {
    pub const CS_BUFFER: u32 = 0;
    pub const CS_LENGTH: u32 = 4;
    pub const CS_CURCHR: u32 = 8;
    pub const CSOURCE_TAMANO: u32 = 12;
    pub const RDA_SOURCE: u32 = 0;
    pub const RDA_DALIST: u32 = 12;
    pub const RDA_BUFFER: u32 = 16;
    pub const RDA_BUFSIZ: u32 = 20;
    pub const RDA_EXTHELP: u32 = 24;
    pub const RDA_FLAGS: u32 = 28;
    pub const TAMANO: u32 = 32;

    pub const RDAF_STDIN: u32 = 1;
    pub const RDAF_NOALLOC: u32 = 2;
    pub const RDAF_NOPROMPT: u32 = 4;
}

/// Tipos de `AllocDosObject`.
pub mod dos_obj {
    pub const DOS_FILEHANDLE: u32 = 0;
    pub const DOS_EXALLCONTROL: u32 = 1;
    pub const DOS_FIB: u32 = 2;
    pub const DOS_STDPKT: u32 = 3;
    pub const DOS_CLI: u32 = 4;
    pub const DOS_RDARGS: u32 = 5;
}

/// Marcas de `SetVar` y `GetVar` (`dos/var.h`).
pub mod gvf {
    pub const LV_VAR: u32 = 0;
    pub const LV_ALIAS: u32 = 1;
    pub const LVF_IGNORE: u32 = 0x80;
    pub const GLOBAL_ONLY: u32 = 0x100;
    pub const LOCAL_ONLY: u32 = 0x200;
    pub const BINARY_VAR: u32 = 0x400;
    pub const DONT_NULL_TERM: u32 = 0x800;
    pub const SAVE_VAR: u32 = 0x1000;
}
