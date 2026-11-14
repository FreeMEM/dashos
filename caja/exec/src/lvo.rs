//! Las funciones de exec.library 40 y su sitio en la tabla de saltos (LVO), según `exec_lib.fd` del NDK 3.1.
//!
//! La entrada número `n` (desde 1) está en `-6 * n` desde la base. Las cuatro primeras son las de toda biblioteca
//! (Open, Close, Expunge y la reservada); desde la 5 (-30), las de exec. Cada fila lleva los registros de los
//! argumentos, como en el `.fd`, para quien escriba los puentes.
//!
//! En DashOS, el número de función nativa de una función de exec es su número de entrada `n`: el JIT recibe ese
//! número de la entrada `$A0FF n RTS` y la caja lo pasa a `Exec::llamar`.

/// Nombre y registros de cada entrada, desde la 1. Las privadas van como en el `.fd`.
pub const FUNCIONES: [(&str, &str); 137] = [
    ("Open", "d0/a6"),
    ("Close", "a6"),
    ("Expunge", "a6"),
    ("Reserved", ""),
    ("Supervisor", "a5"),
    ("ExitIntr", ""),
    ("Schedule", ""),
    ("Reschedule", ""),
    ("Switch", ""),
    ("Dispatch", ""),
    ("Exception", ""),
    ("InitCode", "d0/d1"),
    ("InitStruct", "a1/a2/d0"),
    ("MakeLibrary", "a0/a1/a2/d0/d1"),
    ("MakeFunctions", "a0/a1/a2"),
    ("FindResident", "a1"),
    ("InitResident", "a1/d1"),
    ("Alert", "d7"),
    ("Debug", "d0"),
    ("Disable", ""),
    ("Enable", ""),
    ("Forbid", ""),
    ("Permit", ""),
    ("SetSR", "d0/d1"),
    ("SuperState", ""),
    ("UserState", "d0"),
    ("SetIntVector", "d0/a1"),
    ("AddIntServer", "d0/a1"),
    ("RemIntServer", "d0/a1"),
    ("Cause", "a1"),
    ("Allocate", "a0/d0"),
    ("Deallocate", "a0/a1/d0"),
    ("AllocMem", "d0/d1"),
    ("AllocAbs", "d0/a1"),
    ("FreeMem", "a1/d0"),
    ("AvailMem", "d1"),
    ("AllocEntry", "a0"),
    ("FreeEntry", "a0"),
    ("Insert", "a0/a1/a2"),
    ("AddHead", "a0/a1"),
    ("AddTail", "a0/a1"),
    ("Remove", "a1"),
    ("RemHead", "a0"),
    ("RemTail", "a0"),
    ("Enqueue", "a0/a1"),
    ("FindName", "a0/a1"),
    ("AddTask", "a1/a2/a3"),
    ("RemTask", "a1"),
    ("FindTask", "a1"),
    ("SetTaskPri", "a1/d0"),
    ("SetSignal", "d0/d1"),
    ("SetExcept", "d0/d1"),
    ("Wait", "d0"),
    ("Signal", "a1/d0"),
    ("AllocSignal", "d0"),
    ("FreeSignal", "d0"),
    ("AllocTrap", "d0"),
    ("FreeTrap", "d0"),
    ("AddPort", "a1"),
    ("RemPort", "a1"),
    ("PutMsg", "a0/a1"),
    ("GetMsg", "a0"),
    ("ReplyMsg", "a1"),
    ("WaitPort", "a0"),
    ("FindPort", "a1"),
    ("AddLibrary", "a1"),
    ("RemLibrary", "a1"),
    ("OldOpenLibrary", "a1"),
    ("CloseLibrary", "a1"),
    ("SetFunction", "a1/a0/d0"),
    ("SumLibrary", "a1"),
    ("AddDevice", "a1"),
    ("RemDevice", "a1"),
    ("OpenDevice", "a0/d0/a1/d1"),
    ("CloseDevice", "a1"),
    ("DoIO", "a1"),
    ("SendIO", "a1"),
    ("CheckIO", "a1"),
    ("WaitIO", "a1"),
    ("AbortIO", "a1"),
    ("AddResource", "a1"),
    ("RemResource", "a1"),
    ("OpenResource", "a1"),
    ("RawIOInit", ""),
    ("RawMayGetChar", ""),
    ("RawPutChar", "d0"),
    ("RawDoFmt", "a0/a1/a2/a3"),
    ("GetCC", ""),
    ("TypeOfMem", "a1"),
    ("Procure", "a0/a1"),
    ("Vacate", "a0/a1"),
    ("OpenLibrary", "a1/d0"),
    ("InitSemaphore", "a0"),
    ("ObtainSemaphore", "a0"),
    ("ReleaseSemaphore", "a0"),
    ("AttemptSemaphore", "a0"),
    ("ObtainSemaphoreList", "a0"),
    ("ReleaseSemaphoreList", "a0"),
    ("FindSemaphore", "a1"),
    ("AddSemaphore", "a1"),
    ("RemSemaphore", "a1"),
    ("SumKickData", ""),
    ("AddMemList", "d0/d1/d2/a0/a1"),
    ("CopyMem", "a0/a1/d0"),
    ("CopyMemQuick", "a0/a1/d0"),
    ("CacheClearU", ""),
    ("CacheClearE", "a0/d0/d1"),
    ("CacheControl", "d0/d1"),
    ("CreateIORequest", "a0/d0"),
    ("DeleteIORequest", "a0"),
    ("CreateMsgPort", ""),
    ("DeleteMsgPort", "a0"),
    ("ObtainSemaphoreShared", "a0"),
    ("AllocVec", "d0/d1"),
    ("FreeVec", "a1"),
    ("CreatePool", "d0/d1/d2"),
    ("DeletePool", "a0"),
    ("AllocPooled", "a0/d0"),
    ("FreePooled", "a0/a1/d0"),
    ("AttemptSemaphoreShared", "a0"),
    ("ColdReboot", ""),
    ("StackSwap", "a0"),
    ("ChildFree", "d0"),
    ("ChildOrphan", "d0"),
    ("ChildStatus", "d0"),
    ("ChildWait", "d0"),
    ("CachePreDMA", "a0/a1/d0"),
    ("CachePostDMA", "a0/a1/d0"),
    ("AddMemHandler", "a1"),
    ("RemMemHandler", "a1"),
    ("ObtainQuickVector", "a0"),
    ("execPrivate14", ""),
    ("execPrivate15", ""),
    ("execPrivate16", ""),
    ("execPrivate17", ""),
    ("execPrivate18", ""),
    ("execPrivate19", ""),
];

/// Cuántas entradas tiene la tabla de saltos de exec 40.
pub const ENTRADAS: u16 = FUNCIONES.len() as u16;

/// El LVO (desplazamiento desde la base, negativo) de la entrada `n`.
pub const fn lvo(n: u16) -> i32 {
    -6 * n as i32
}

/// El número de entrada de un LVO, si es de exec.
pub fn numero(lvo: i32) -> Option<u16> {
    if lvo < 0 && lvo % 6 == 0 && -lvo / 6 <= ENTRADAS as i32 { Some((-lvo / 6) as u16) } else { None }
}

/// El número de entrada de una función por su nombre.
pub fn por_nombre(nombre: &str) -> Option<u16> {
    FUNCIONES.iter().position(|(n, _)| *n == nombre).map(|i| i as u16 + 1)
}

/// El nombre de la entrada `n`.
pub fn nombre(n: u16) -> &'static str {
    match n {
        1..=ENTRADAS => FUNCIONES[n as usize - 1].0,
        FIN_DE_TAREA => "(fin de tarea)",
        REPETIR => "(repetir)",
        _ => "(desconocida)",
    }
}

/// Número nativo privado de exec: el código al que vuelve una tarea cuando acaba su función (`RemTask(NULL)`).
pub const FIN_DE_TAREA: u16 = 200;
/// Número nativo privado de exec: el trampolín con que la caja retoma una tarea que se bloqueó en una llamada: la
/// repite con los registros de entonces (`cambio.rs`).
pub const REPETIR: u16 = 201;
/// Los números nativos de exec van del 1 a este; los demás son de otras bibliotecas nativas de la caja.
pub const ULTIMO_DE_EXEC: u16 = 255;
