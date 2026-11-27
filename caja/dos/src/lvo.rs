//! Las funciones de dos.library 40 y su sitio en la tabla de saltos (LVO), según `dos_lib.fd` del NDK 3.1.
//!
//! La entrada número `n` (desde 1) está en `-6 * n` desde la base: las cuatro primeras son Open, Close, Expunge y la
//! reservada de toda biblioteca; desde la 5 (-30), las de dos. Cada fila lleva los registros de los argumentos, como en
//! el `.fd`. `Dos::llamar` recibe el número de entrada; la caja lo saca de su número nativo con `exec.ajena(n)`.

/// Nombre y registros de las entradas de dos, desde la 5 (-30). Las privadas van como `dosPrivateN`.
pub const FUNCIONES: [(&str, &str); 162] = [
    ("Open", "d1/d2"),
    ("Close", "d1"),
    ("Read", "d1/d2/d3"),
    ("Write", "d1/d2/d3"),
    ("Input", ""),
    ("Output", ""),
    ("Seek", "d1/d2/d3"),
    ("DeleteFile", "d1"),
    ("Rename", "d1/d2"),
    ("Lock", "d1/d2"),
    ("UnLock", "d1"),
    ("DupLock", "d1"),
    ("Examine", "d1/d2"),
    ("ExNext", "d1/d2"),
    ("Info", "d1/d2"),
    ("CreateDir", "d1"),
    ("CurrentDir", "d1"),
    ("IoErr", ""),
    ("CreateProc", "d1/d2/d3/d4"),
    ("Exit", "d1"),
    ("LoadSeg", "d1"),
    ("UnLoadSeg", "d1"),
    ("dosPrivate1", ""),
    ("dosPrivate2", ""),
    ("DeviceProc", "d1"),
    ("SetComment", "d1/d2"),
    ("SetProtection", "d1/d2"),
    ("DateStamp", "d1"),
    ("Delay", "d1"),
    ("WaitForChar", "d1/d2"),
    ("ParentDir", "d1"),
    ("IsInteractive", "d1"),
    ("Execute", "d1/d2/d3"),
    ("AllocDosObject", "d1/d2"),
    ("FreeDosObject", "d1/d2"),
    ("DoPkt", "d1/d2/d3/d4/d5/d6/d7"),
    ("SendPkt", "d1/d2/d3"),
    ("WaitPkt", ""),
    ("ReplyPkt", "d1/d2/d3"),
    ("AbortPkt", "d1/d2"),
    ("LockRecord", "d1/d2/d3/d4/d5"),
    ("LockRecords", "d1/d2"),
    ("UnLockRecord", "d1/d2/d3"),
    ("UnLockRecords", "d1"),
    ("SelectInput", "d1"),
    ("SelectOutput", "d1"),
    ("FGetC", "d1"),
    ("FPutC", "d1/d2"),
    ("UnGetC", "d1/d2"),
    ("FRead", "d1/d2/d3/d4"),
    ("FWrite", "d1/d2/d3/d4"),
    ("FGets", "d1/d2/d3"),
    ("FPuts", "d1/d2"),
    ("VFWritef", "d1/d2/d3"),
    ("VFPrintf", "d1/d2/d3"),
    ("Flush", "d1"),
    ("SetVBuf", "d1/d2/d3/d4"),
    ("DupLockFromFH", "d1"),
    ("OpenFromLock", "d1"),
    ("ParentOfFH", "d1"),
    ("ExamineFH", "d1/d2"),
    ("SetFileDate", "d1/d2"),
    ("NameFromLock", "d1/d2/d3"),
    ("NameFromFH", "d1/d2/d3"),
    ("SplitName", "d1/d2/d3/d4/d5"),
    ("SameLock", "d1/d2"),
    ("SetMode", "d1/d2"),
    ("ExAll", "d1/d2/d3/d4/d5"),
    ("ReadLink", "d1/d2/d3/d4/d5"),
    ("MakeLink", "d1/d2/d3"),
    ("ChangeMode", "d1/d2/d3"),
    ("SetFileSize", "d1/d2/d3"),
    ("SetIoErr", "d1"),
    ("Fault", "d1/d2/d3/d4"),
    ("PrintFault", "d1/d2"),
    ("ErrorReport", "d1/d2/d3/d4"),
    ("dosPrivate3", ""),
    ("Cli", ""),
    ("CreateNewProc", "d1"),
    ("RunCommand", "d1/d2/d3/d4"),
    ("GetConsoleTask", ""),
    ("SetConsoleTask", "d1"),
    ("GetFileSysTask", ""),
    ("SetFileSysTask", "d1"),
    ("GetArgStr", ""),
    ("SetArgStr", "d1"),
    ("FindCliProc", "d1"),
    ("MaxCli", ""),
    ("SetCurrentDirName", "d1"),
    ("GetCurrentDirName", "d1/d2"),
    ("SetProgramName", "d1"),
    ("GetProgramName", "d1/d2"),
    ("SetPrompt", "d1"),
    ("GetPrompt", "d1/d2"),
    ("SetProgramDir", "d1"),
    ("GetProgramDir", ""),
    ("SystemTagList", "d1/d2"),
    ("AssignLock", "d1/d2"),
    ("AssignLate", "d1/d2"),
    ("AssignPath", "d1/d2"),
    ("AssignAdd", "d1/d2"),
    ("RemAssignList", "d1/d2"),
    ("GetDeviceProc", "d1/d2"),
    ("FreeDeviceProc", "d1"),
    ("LockDosList", "d1"),
    ("UnLockDosList", "d1"),
    ("AttemptLockDosList", "d1"),
    ("RemDosEntry", "d1"),
    ("AddDosEntry", "d1"),
    ("FindDosEntry", "d1/d2/d3"),
    ("NextDosEntry", "d1/d2"),
    ("MakeDosEntry", "d1/d2"),
    ("FreeDosEntry", "d1"),
    ("IsFileSystem", "d1"),
    ("Format", "d1/d2/d3"),
    ("Relabel", "d1/d2"),
    ("Inhibit", "d1/d2"),
    ("AddBuffers", "d1/d2"),
    ("CompareDates", "d1/d2"),
    ("DateToStr", "d1"),
    ("StrToDate", "d1"),
    ("InternalLoadSeg", "d0/a0/a1/a2"),
    ("InternalUnLoadSeg", "d1/a1"),
    ("NewLoadSeg", "d1/d2"),
    ("AddSegment", "d1/d2/d3"),
    ("FindSegment", "d1/d2/d3"),
    ("RemSegment", "d1"),
    ("CheckSignal", "d1"),
    ("ReadArgs", "d1/d2/d3"),
    ("FindArg", "d1/d2"),
    ("ReadItem", "d1/d2/d3"),
    ("StrToLong", "d1/d2"),
    ("MatchFirst", "d1/d2"),
    ("MatchNext", "d1"),
    ("MatchEnd", "d1"),
    ("ParsePattern", "d1/d2/d3"),
    ("MatchPattern", "d1/d2"),
    ("dosPrivate4", ""),
    ("FreeArgs", "d1"),
    ("dosPrivate5", ""),
    ("FilePart", "d1"),
    ("PathPart", "d1"),
    ("AddPart", "d1/d2/d3"),
    ("StartNotify", "d1"),
    ("EndNotify", "d1"),
    ("SetVar", "d1/d2/d3/d4"),
    ("GetVar", "d1/d2/d3/d4"),
    ("DeleteVar", "d1/d2"),
    ("FindVar", "d1/d2"),
    ("dosPrivate6", ""),
    ("CliInitNewcli", "a0"),
    ("CliInitRun", "a0"),
    ("WriteChars", "d1/d2"),
    ("PutStr", "d1"),
    ("VPrintf", "d1/d2"),
    ("dosPrivate7", ""),
    ("ParsePatternNoCase", "d1/d2/d3"),
    ("MatchPatternNoCase", "d1/d2"),
    ("dosPrivate8", ""),
    ("SameDevice", "d1/d2"),
    ("ExAllEnd", "d1/d2/d3/d4/d5"),
    ("SetOwner", "d1/d2"),
];

/// La primera entrada propia de dos (Open, en -30).
pub const PRIMERA: u16 = 5;
/// La última (SetOwner, en -996).
pub const ULTIMA: u16 = PRIMERA + FUNCIONES.len() as u16 - 1;

/// El número de entrada de un LVO (-30 → 5), si es de dos.
pub fn entrada(lvo: i32) -> Option<u16> {
    if lvo < 0 && lvo % 6 == 0 && (-lvo / 6) as u16 >= PRIMERA && -lvo / 6 <= ULTIMA as i32 {
        Some((-lvo / 6) as u16)
    } else {
        None
    }
}

/// El LVO de una entrada (5 → -30).
pub fn lvo(n: u16) -> i32 {
    -6 * n as i32
}

/// El nombre de una entrada, para los avisos.
pub fn nombre(n: u16) -> &'static str {
    match n {
        1 => "Open (de biblioteca)",
        2 => "Close (de biblioteca)",
        3 => "Expunge",
        4 => "Reserved",
        PRIMERA..=ULTIMA => FUNCIONES[(n - PRIMERA) as usize].0,
        _ => "(no es de dos)",
    }
}

/// Las constantes con el número de entrada de cada función, para el despacho y las pruebas.
pub const OPEN: u16 = 5;
pub const CLOSE: u16 = 6;
pub const READ: u16 = 7;
pub const WRITE: u16 = 8;
pub const INPUT: u16 = 9;
pub const OUTPUT: u16 = 10;
pub const SEEK: u16 = 11;
pub const DELETE_FILE: u16 = 12;
pub const RENAME: u16 = 13;
pub const LOCK: u16 = 14;
pub const UN_LOCK: u16 = 15;
pub const DUP_LOCK: u16 = 16;
pub const EXAMINE: u16 = 17;
pub const EX_NEXT: u16 = 18;
pub const INFO: u16 = 19;
pub const CREATE_DIR: u16 = 20;
pub const CURRENT_DIR: u16 = 21;
pub const IO_ERR: u16 = 22;
pub const CREATE_PROC: u16 = 23;
pub const EXIT: u16 = 24;
pub const LOAD_SEG: u16 = 25;
pub const UN_LOAD_SEG: u16 = 26;
pub const DEVICE_PROC: u16 = 29;
pub const SET_COMMENT: u16 = 30;
pub const SET_PROTECTION: u16 = 31;
pub const DATE_STAMP: u16 = 32;
pub const DELAY: u16 = 33;
pub const WAIT_FOR_CHAR: u16 = 34;
pub const PARENT_DIR: u16 = 35;
pub const IS_INTERACTIVE: u16 = 36;
pub const EXECUTE: u16 = 37;
pub const ALLOC_DOS_OBJECT: u16 = 38;
pub const FREE_DOS_OBJECT: u16 = 39;
pub const DO_PKT: u16 = 40;
pub const SEND_PKT: u16 = 41;
pub const WAIT_PKT: u16 = 42;
pub const REPLY_PKT: u16 = 43;
pub const ABORT_PKT: u16 = 44;
pub const LOCK_RECORD: u16 = 45;
pub const LOCK_RECORDS: u16 = 46;
pub const UN_LOCK_RECORD: u16 = 47;
pub const UN_LOCK_RECORDS: u16 = 48;
pub const SELECT_INPUT: u16 = 49;
pub const SELECT_OUTPUT: u16 = 50;
pub const FGET_C: u16 = 51;
pub const FPUT_C: u16 = 52;
pub const UN_GET_C: u16 = 53;
pub const FREAD: u16 = 54;
pub const FWRITE: u16 = 55;
pub const FGETS: u16 = 56;
pub const FPUTS: u16 = 57;
pub const VFWRITEF: u16 = 58;
pub const VFPRINTF: u16 = 59;
pub const FLUSH: u16 = 60;
pub const SET_VBUF: u16 = 61;
pub const DUP_LOCK_FROM_FH: u16 = 62;
pub const OPEN_FROM_LOCK: u16 = 63;
pub const PARENT_OF_FH: u16 = 64;
pub const EXAMINE_FH: u16 = 65;
pub const SET_FILE_DATE: u16 = 66;
pub const NAME_FROM_LOCK: u16 = 67;
pub const NAME_FROM_FH: u16 = 68;
pub const SPLIT_NAME: u16 = 69;
pub const SAME_LOCK: u16 = 70;
pub const SET_MODE: u16 = 71;
pub const EXALL: u16 = 72;
pub const READ_LINK: u16 = 73;
pub const MAKE_LINK: u16 = 74;
pub const CHANGE_MODE: u16 = 75;
pub const SET_FILE_SIZE: u16 = 76;
pub const SET_IO_ERR: u16 = 77;
pub const FAULT: u16 = 78;
pub const PRINT_FAULT: u16 = 79;
pub const ERROR_REPORT: u16 = 80;
pub const CLI: u16 = 82;
pub const CREATE_NEW_PROC: u16 = 83;
pub const RUN_COMMAND: u16 = 84;
pub const GET_CONSOLE_TASK: u16 = 85;
pub const SET_CONSOLE_TASK: u16 = 86;
pub const GET_FILE_SYS_TASK: u16 = 87;
pub const SET_FILE_SYS_TASK: u16 = 88;
pub const GET_ARG_STR: u16 = 89;
pub const SET_ARG_STR: u16 = 90;
pub const FIND_CLI_PROC: u16 = 91;
pub const MAX_CLI: u16 = 92;
pub const SET_CURRENT_DIR_NAME: u16 = 93;
pub const GET_CURRENT_DIR_NAME: u16 = 94;
pub const SET_PROGRAM_NAME: u16 = 95;
pub const GET_PROGRAM_NAME: u16 = 96;
pub const SET_PROMPT: u16 = 97;
pub const GET_PROMPT: u16 = 98;
pub const SET_PROGRAM_DIR: u16 = 99;
pub const GET_PROGRAM_DIR: u16 = 100;
pub const SYSTEM_TAG_LIST: u16 = 101;
pub const ASSIGN_LOCK: u16 = 102;
pub const ASSIGN_LATE: u16 = 103;
pub const ASSIGN_PATH: u16 = 104;
pub const ASSIGN_ADD: u16 = 105;
pub const REM_ASSIGN_LIST: u16 = 106;
pub const GET_DEVICE_PROC: u16 = 107;
pub const FREE_DEVICE_PROC: u16 = 108;
pub const LOCK_DOS_LIST: u16 = 109;
pub const UN_LOCK_DOS_LIST: u16 = 110;
pub const ATTEMPT_LOCK_DOS_LIST: u16 = 111;
pub const REM_DOS_ENTRY: u16 = 112;
pub const ADD_DOS_ENTRY: u16 = 113;
pub const FIND_DOS_ENTRY: u16 = 114;
pub const NEXT_DOS_ENTRY: u16 = 115;
pub const MAKE_DOS_ENTRY: u16 = 116;
pub const FREE_DOS_ENTRY: u16 = 117;
pub const IS_FILE_SYSTEM: u16 = 118;
pub const FORMAT: u16 = 119;
pub const RELABEL: u16 = 120;
pub const INHIBIT: u16 = 121;
pub const ADD_BUFFERS: u16 = 122;
pub const COMPARE_DATES: u16 = 123;
pub const DATE_TO_STR: u16 = 124;
pub const STR_TO_DATE: u16 = 125;
pub const INTERNAL_LOAD_SEG: u16 = 126;
pub const INTERNAL_UN_LOAD_SEG: u16 = 127;
pub const NEW_LOAD_SEG: u16 = 128;
pub const ADD_SEGMENT: u16 = 129;
pub const FIND_SEGMENT: u16 = 130;
pub const REM_SEGMENT: u16 = 131;
pub const CHECK_SIGNAL: u16 = 132;
pub const READ_ARGS: u16 = 133;
pub const FIND_ARG: u16 = 134;
pub const READ_ITEM: u16 = 135;
pub const STR_TO_LONG: u16 = 136;
pub const MATCH_FIRST: u16 = 137;
pub const MATCH_NEXT: u16 = 138;
pub const MATCH_END: u16 = 139;
pub const PARSE_PATTERN: u16 = 140;
pub const MATCH_PATTERN: u16 = 141;
pub const FREE_ARGS: u16 = 143;
pub const FILE_PART: u16 = 145;
pub const PATH_PART: u16 = 146;
pub const ADD_PART: u16 = 147;
pub const START_NOTIFY: u16 = 148;
pub const END_NOTIFY: u16 = 149;
pub const SET_VAR: u16 = 150;
pub const GET_VAR: u16 = 151;
pub const DELETE_VAR: u16 = 152;
pub const FIND_VAR: u16 = 153;
pub const CLI_INIT_NEWCLI: u16 = 155;
pub const CLI_INIT_RUN: u16 = 156;
pub const WRITE_CHARS: u16 = 157;
pub const PUT_STR: u16 = 158;
pub const VPRINTF: u16 = 159;
pub const PARSE_PATTERN_NO_CASE: u16 = 161;
pub const MATCH_PATTERN_NO_CASE: u16 = 162;
pub const SAME_DEVICE: u16 = 164;
pub const EXALL_END: u16 = 165;
pub const SET_OWNER: u16 = 166;
