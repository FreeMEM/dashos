//! exec.library nativa de la caja Amiga (S4, S2, S3).
//!
//! La caja es un proceso de DashOS donde corren programas 68k con el JIT de Emu68. Su exec está escrito aquí, en
//! Rust, pero con la interfaz del 68k: las estructuras (listas, tareas, puertos, bibliotecas, la propia ExecBase)
//! viven en la memoria de la caja, en big-endian y con punteros de 32 bits, porque los programas Amiga las leen y
//! las escriben a mano. Cada entrada de la tabla de saltos lleva `$A0FF n RTS`; el JIT convierte `$A0FF n` en una
//! llamada a la función nativa `n` con los registros del 68k (S4 §1), y la caja la pasa a `Exec::llamar`.
//!
//! Escrito desde la documentación pública: el ROM Kernel Reference Manual (Libraries, capítulos de exec), los
//! autodocs de exec.library y los includes del NDK 3.1 (`exec/*.h`, `exec_lib.fd`) como especificación de la
//! interfaz. No se ha leído ni copiado código de AmigaOS, AROS, UAE ni Amiberry.
//!
//! Lo que no hace un exec de verdad y este sí (S2, S3):
//! - Cada bloque de memoria lleva su tarea dueña, en una tabla nativa que el 68k no ve. `FreeMem` de algo no
//!   reservado, liberar dos veces o con otro tamaño dan un aviso y no rompen nada; matar una tarea suelta lo suyo.
//! - Todo puntero que llega del 68k se comprueba: fuera de la memoria o en la página 0 da un aviso (el Enforcer de
//!   S3), nunca un pánico. Las listas rotas se notan antes de escribir en ellas.
//!
//! Sin hilos: el planificador es la función `elegir_siguiente`, que dice qué tarea tiene el testigo de la caja.
//! Las funciones que bloquean (`Wait`, `WaitPort`) devuelven `Accion::Bloquear` y la caja las vuelve a llamar con
//! los mismos registros cuando la tarea recupera el testigo. Cómo se enchufa: `docs/bitacoras/caja-amiga-exec.md`.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod estructuras;
pub mod lvo;
pub mod memoria;
pub mod registros;

mod asignador;
pub use asignador::{DUENO_CHIPSET, HUECO_DEL_CHIPSET};
mod bibliotecas;
pub mod cambio;
mod charcos;
mod despacho;
mod dispositivos;
mod formato;
mod listas;
pub mod memoria_baja;
mod puertos;
pub mod ramlib;
pub use formato::stuff_char;
mod semaforos;
mod tareas;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

pub use bibliotecas::{Entrada, LINEA_A_NATIVA};
pub use dispositivos::io;
use estructuras::*;
pub use memoria::{Memoria, MemoriaVec};
pub use registros::Registros;
pub use tareas::Espera;
pub use cambio::{Contexto, Modo};

/// Lo que puede salir mal en una llamada. Ninguno tumba la caja: se apunta como aviso y la función devuelve lo que
/// devolvería al fallar (normalmente 0 en D0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fallo {
    /// Una dirección fuera de la memoria de la caja (el Enforcer).
    FueraDeMemoria { dir: u32, largo: u32 },
    /// Acceso a la página 0, salvo leer la base de exec en la dirección 4.
    PaginaCero { dir: u32 },
    /// Un puntero que no puede ser 0.
    PunteroNulo,
    /// Una lista con los enlaces mal (nodo que no está en ella, lista sin iniciar, vueltas sin fin).
    ListaRota { dir: u32 },
    /// Una cadena sin el 0 del final en un tramo razonable.
    CadenaSinFin { dir: u32 },
    /// `FreeMem` o `FreeVec` de algo que no está reservado (o ya liberado).
    NoReservado { dir: u32 },
    /// `FreeMem` de un bloque con otro tamaño del que se reservó.
    TamanoDistinto { dir: u32, reservado: u32, pedido: u32 },
    /// `FreeVec` de un bloque de `AllocMem`.
    NoEsDeAllocVec { dir: u32 },
    /// Un charco (`AllocPooled`, `FreePooled`, `DeletePool`) que no dio `CreatePool`, o ya borrado.
    NoEsCharco { dir: u32 },
    /// Liberar memoria que ya está libre en la lista de un `MemHeader`.
    YaLibre { dir: u32 },
    /// Memoria fuera del `MemHeader` en que se libera.
    FueraDelBloque { dir: u32 },
    /// Una tarea que exec no conoce.
    TareaDesconocida { dir: u32 },
    /// `AddTask` de una tarea que ya está.
    TareaRepetida { dir: u32 },
    /// La función necesita una tarea en marcha y no la hay.
    SinTarea,
    /// La tarea no tiene pila (`tc_SPReg`) o no cabe.
    PilaMala { dir: u32 },
    /// Número de señal fuera de 0..31.
    SenalMala { n: i32 },
    /// `FreeSignal` de una señal que no está reservada.
    SenalNoReservada { n: i32 },
    /// Un puerto que ya no existe (su memoria está libre).
    PuertoMuerto { dir: u32 },
    /// Un mensaje en memoria libre.
    MensajeMuerto { dir: u32 },
    /// `PutMsg` de un mensaje que aún está en la cola de un puerto.
    MensajeEnCola { dir: u32 },
    /// Una biblioteca que no está en la lista del sistema.
    BibliotecaDesconocida { dir: u32 },
    /// `CloseLibrary` de una biblioteca que nadie tiene abierta.
    CierreDeMas { dir: u32 },
    /// Un desplazamiento que no es una entrada de la tabla de saltos.
    LvoMalo { lvo: i32 },
    /// Una tabla de `InitStruct` o de vectores que no se entiende.
    TablaMala { dir: u32 },
    /// `Permit` o `Enable` sin su `Forbid` o `Disable`.
    SinPareja,
    /// Una función de exec que aún no está hecha.
    NoImplementada { n: u16 },
    /// Algo que exec deja pasar pero suele ser un error: esperar señales no reservadas, puerto de otra tarea...
    Sospechoso { dir: u32 },
    /// `Alert`: el programa (o una biblioteca suya) avisa de un fallo; con `AT_DeadEnd` (el bit 31), se acaba.
    Alerta { n: u32 },
    /// La memoria de la caja es demasiado pequeña para arrancar.
    SinSitio,
}

/// Un fallo apuntado, con la tarea que lo causó y la función. Es el registro del Enforcer (S3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Aviso {
    pub tarea: u32,
    pub funcion: &'static str,
    pub fallo: Fallo,
}

/// Qué tiene que hacer la caja al volver de una función nativa.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accion {
    /// Volver al código 68k de la tarea.
    Seguir,
    /// La llamada ha terminado, pero hay una tarea de más prioridad lista: pasar el testigo con
    /// `elegir_siguiente`. Cuando esta tarea lo recupere, vuelve al 68k sin repetir nada.
    Ceder,
    /// La tarea espera (`Wait`, `WaitPort`): pasar el testigo con `elegir_siguiente`. Cuando esta tarea lo
    /// recupere, la caja repite la misma llamada con los mismos registros.
    Bloquear,
    /// La tarea se ha quitado a sí misma: su hilo acaba. Pasar el testigo con `elegir_siguiente`.
    Terminar,
    /// Exec necesita ejecutar código que no es suyo (68k, o de otra biblioteca nativa) antes de acabar: la caja
    /// lo ejecuta con `regs` en la pila de la tarea y llama a `continuar(tras, &regs_al_volver, regs_de_la_tarea)`.
    Llamar { destino: Destino, regs: Registros, tras: Continuacion },
    /// El número no es de exec: es de otra biblioteca nativa de la caja.
    Ajena,
}

/// Código que exec pide ejecutar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Destino {
    /// Código 68k en esa dirección, hasta su `RTS`.
    Codigo68k(u32),
    /// Una función nativa de otra biblioteca de la caja.
    Nativa(u16),
    /// Código 68k en esa dirección, en modo supervisor, hasta su `RTE` (`Supervisor`): la caja le deja en la pila de
    /// supervisor un marco de excepción que vuelve a ella.
    Supervisor(u32),
}

/// Lo que queda por hacer cuando vuelve una llamada pedida con `Accion::Llamar`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Continuacion {
    /// Volvió el `Open` de una biblioteca: D0 es la base abierta, o 0.
    AbrirBiblioteca { lib: u32 },
    /// Volvió el `Close`.
    CerrarBiblioteca { lib: u32 },
    /// Volvió la función de inicio de `MakeLibrary`: D0 es la biblioteca.
    IniciarBiblioteca,
    /// Pasar D0 tal cual.
    Devolver,
    /// Volvió la función de `Supervisor`: todos los registros como los dejó, salvo A7 (la pila de quien llamó).
    Supervisor,
    /// Volvió el `Open` de un dispositivo (`OpenDevice`): su error está en `io_Error`.
    AbrirDispositivo { dev: u32, io: u32 },
    /// Volvió el `Close` de un dispositivo.
    CerrarDispositivo { io: u32, dev: u32 },
    /// Volvió el `BeginIO` de `DoIO`: si no acabó en el acto, hay que esperar.
    HacerIO { io: u32 },
    /// ramlib: volvió la carga de la caja (D0, la lista de segmentos).
    Cargada(ramlib::Apertura),
    /// ramlib: la apertura falla (tras soltar lo cargado).
    Fallida(ramlib::Apertura),
    /// ramlib: volvió el `InitResident` de lo cargado (D0, la base).
    Residente { a: ramlib::Apertura, seglist: u32 },
    /// `InitResident` con `RTF_AUTOINIT`: volvió su `init`; se añade a su lista y se sigue con la guardada.
    AutoIniciada { tipo: u8, tras: u32 },
    /// Volvió el `Expunge` de una de disco (D0, su lista); `d0` es lo que devolverá la llamada.
    Echada { lib: u32, d0: u32 },
    /// D0 = ese valor.
    Valor(u32),
    /// `RawDoFmt`: volvió su `PutChProc` con un carácter; el siguiente (con el A3 que dejó).
    Entrega { id: u32 },
    /// Memoria baja: volvió un manejador (D0, lo que dice); después, la llamada `n` del programa, con su D0 de antes.
    Manejador { n: u16, d0: u32 },
    /// Volvió código 68k que pidió la caja (no exec; `RunCommand`, `SystemTagList`): D0 tal cual, y la caja sigue con
    /// lo suyo (`tomar_vuelta_caja`).
    DeLaCaja { id: u32 },
}

/// Un trozo de memoria para el reparto: `[inicio, fin)` con sus atributos (`MEMF_CHIP`...).
#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub inicio: u32,
    pub fin: u32,
    pub atributos: u16,
    pub prioridad: i8,
    pub nombre: &'static str,
}

/// Las cachés que dice tener la caja al arrancar (`CacheControl`): las de instrucciones y datos de un 68040, encendidas
/// (`CACRF_EnableI`, `CACRF_EnableD`, `CACRF_CopyBack`).
pub const CACR_DE_SERIE: u32 = 1 | 1 << 8 | 1 << 31;

/// Cómo arrancar exec.
#[derive(Clone, Debug)]
pub struct Config {
    /// La zona del sistema: `[inicio, fin)`, donde van ExecBase, su tabla de saltos, los trampolines y las cadenas de
    /// exec. Nunca se reparte. En las pruebas, `ZONA_SISTEMA..FIN_ZONA_SISTEMA`; en la caja de DashOS, dentro de la
    /// memoria del 68k (el kernel no proyecta nada por debajo de 64 KB, salvo la página 0).
    pub sistema: (u32, u32),
    pub regiones: Vec<Region>,
    pub attn_flags: u16,
    pub vblank: u8,
    pub eclock: u32,
}

/// Lo que va antes de la memoria repartible: la página 0 y la zona del sistema (ExecBase, su tabla de saltos y
/// los trampolines de las funciones nativas). Estos son los de `Config::para`; `Config::sistema` los cambia.
pub const PAGINA_CERO: u32 = 0x1000;
pub const ZONA_SISTEMA: u32 = 0x1000;
pub const FIN_ZONA_SISTEMA: u32 = 0x4000;
/// Los últimos 64 KB de los 4 GB nunca son memoria de la caja: un puntero ahí es un fallo. Así una dirección
/// válida más el desplazamiento de un campo (siempre menos de 64 KB) no da la vuelta.
pub const LIMITE: u32 = 0xFFFF_0000;
/// Dónde acaba la memoria "chip" por defecto: 2 MB, como un A1200.
pub const FIN_CHIP: u32 = 0x0020_0000;

impl Config {
    /// La configuración normal para una caja de `tamano` bytes: memoria "chip" hasta los 2 MB y "fast" encima.
    /// En la caja no hay chipset; la chip existe porque los programas la piden y miran cuánta queda.
    pub fn para(tamano: u32) -> Config {
        let mut regiones = Vec::new();
        let fin_chip = tamano.min(FIN_CHIP);
        regiones.push(Region {
            inicio: FIN_ZONA_SISTEMA,
            fin: fin_chip,
            atributos: (memf::PUBLIC | memf::CHIP | memf::LOCAL | memf::DMA24BIT) as u16,
            prioridad: -10,
            nombre: "chip memory",
        });
        if tamano > FIN_CHIP + 0x10000 {
            regiones.push(Region {
                inicio: FIN_CHIP,
                fin: tamano & !7,
                atributos: (memf::PUBLIC | memf::FAST) as u16,
                prioridad: 0,
                nombre: "expansion memory",
            });
        }
        Config {
            sistema: (ZONA_SISTEMA, FIN_ZONA_SISTEMA),
            regiones,
            attn_flags: execbase::AFF_68010
                | execbase::AFF_68020
                | execbase::AFF_68030
                | execbase::AFF_68040
                | execbase::AFF_68881
                | execbase::AFF_68882
                | execbase::AFF_FPU40,
            vblank: 50,
            eclock: 709_379,
        }
    }

    /// La de la caja de DashOS: la memoria del 68k es `[desde, hasta)`, no empieza en 0. La zona del sistema va en
    /// `sistema`; la memoria "chip", de `chip` a los 2 MB (o a `hasta`), y la "fast", el resto.
    pub fn caja(sistema: (u32, u32), chip: u32, hasta: u32) -> Config {
        let mut c = Config::para(hasta);
        c.sistema = sistema;
        for r in &mut c.regiones {
            r.inicio = r.inicio.max(chip);
        }
        c.regiones.retain(|r| r.inicio < r.fin);
        c
    }
}

/// Un bloque reservado con `AllocMem`, `AllocVec` o `AllocAbs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bloque {
    /// Bytes, ya redondeados a 8.
    pub largo: u32,
    /// La tarea dueña; 0 es el sistema.
    pub dueno: u32,
    /// Si es de `AllocVec` (la dirección de la tabla es la de antes del tamaño guardado).
    pub vec: bool,
}

/// Lo que exec sabe de cada tarea fuera de la memoria del 68k.
#[derive(Clone, Debug, Default)]
pub(crate) struct InfoTarea {
    /// Dónde empieza y con qué pila, para que la caja cree su hilo.
    pc: u32,
    sp: u32,
    /// La tarea se bloqueó en `Wait` o `WaitPort` y la próxima llamada es la repetición.
    esperando: bool,
    /// Está dentro de su código de excepción (`tc_ExceptCode`): no se le entrega otra hasta que vuelva.
    en_excepcion: bool,
    /// Bibliotecas que tiene abiertas (una vez por cada `OpenLibrary`).
    bibliotecas: Vec<u32>,
    /// Puertos que creó con `CreateMsgPort`.
    puertos: Vec<u32>,
    /// Quién la creó: la tarea que hizo su `AddTask`, o 0 (el sistema, una biblioteca de disco o la caja).
    creadora: u32,
    /// Lo que guarda la caja mientras no corre (`cambio.rs`).
    contexto: Option<cambio::Contexto>,
    /// La llamada que repite al volver (el trampolín `REPETIR`).
    repetir: Option<u16>,
    /// Espera sin señales hasta que la caja la deje lista (`aparcar`).
    aparcada: bool,
    /// Ha corrido (la caja la retomó o la puso en marcha): sin contexto guardado ya no se puede retomar.
    corrido: bool,
}

const MAX_AVISOS: usize = 1024;
/// Tope de vueltas al recorrer una lista: más es una lista en círculo.
pub(crate) const MAX_VUELTAS: u32 = 1 << 20;
/// Tope de una cadena (nombres de nodos, bibliotecas...).
pub(crate) const MAX_CADENA: u32 = 4096;

pub struct Exec<M: Memoria> {
    pub mem: M,
    base: u32,
    /// Lo libre de la zona del sistema: `[libre, fin)`.
    sistema: (u32, u32),
    bloques: BTreeMap<u32, Bloque>,
    regiones: Vec<(u32, u32)>,
    tareas: BTreeMap<u32, InfoTarea>,
    /// El trampolín de cada función nativa: `$A0FF n RTS` en un sitio fijo, para lo que devuelve `SetFunction`.
    trampolines: BTreeMap<u16, u32>,
    /// Funciones nativas de otras bibliotecas: número → (biblioteca, LVO).
    ajenas: BTreeMap<u16, (u32, i32)>,
    avisos: Vec<Aviso>,
    /// Una tarea de más prioridad quedó lista dentro de `Forbid`/`Disable`: replanificar al salir.
    replanificar: bool,
    /// La tarea en marcha ha gastado su turno.
    turno_acabado: bool,
    /// Tareas quitadas cuyo hilo tiene que acabar la caja.
    quitadas: Vec<u32>,
    /// Tramos de código cambiados (`SetFunction`, `MakeFunctions`...) que el JIT debe olvidar. `(0, 0)` es todo.
    codigo_cambiado: Vec<(u32, u32)>,
    fin_de_tarea: u32,
    /// Dónde acaba la zona del sistema: las regiones repartibles empiezan después.
    fin_sistema: u32,
    /// Los charcos de memoria (`CreatePool`): cabecera → lo que exec sabe de él.
    charcos: BTreeMap<u32, charcos::Charco>,
    /// Quién espera qué semáforo: (semáforo, tarea), por orden de llegada.
    esperas_semaforo: Vec<(u32, u32)>,
    /// Peticiones de `DoIO` que ya salieron y esperan su vuelta (la caja repite la llamada al despertar).
    doio_en_espera: Vec<u32>,
    /// Los dispositivos de `AddDevice` (y los nativos): así no hay que recorrer `DeviceList`, que un programa puede
    /// haber estropeado, en cada petición.
    dispositivos: alloc::collections::BTreeSet<u32>,
    /// ramlib (`ramlib.rs`).
    pub(crate) ramlib: ramlib::Ramlib,
    /// `RawDoFmt` con un `PutChProc` de 68k: lo que queda por entregar (`formato.rs`).
    pub(crate) entregas: alloc::collections::BTreeMap<u32, formato::Entrega>,
    pub(crate) n_entrega: u32,
    /// Lo que se reserva ahora es del sistema (código de una biblioteca de disco): `memoria_del_sistema`.
    pub(crate) memoria_del_sistema: bool,
    /// La última `Continuacion::DeLaCaja` que volvió (`tomar_vuelta_caja`).
    pub(crate) vuelta_caja: Option<u32>,
    /// Lo que dice `CacheControl` de las cachés: no hay ninguna (el JIT), pero los programas las miran y las cambian.
    pub(crate) cacr: u32,
    /// El aviso de memoria baja de DashOS y los manejadores de los programas (`memoria_baja.rs`).
    pub(crate) memoria_baja: memoria_baja::MemoriaBaja,
    /// Bases propias de cada apertura de una biblioteca nativa (bsdsocket.library da una a cada `OpenLibrary`):
    /// base → biblioteca de `LibList`. No están en `LibList`, pero `CloseLibrary` las acepta.
    pub(crate) bases_propias: BTreeMap<u32, u32>,
}

/// Las funciones de exec con número nativo propio, en el orden de la tabla.
const NEGSIZE_EXEC: u32 = (lvo::ENTRADAS as u32 * 6 + 3) & !3;

impl<M: Memoria> Exec<M> {
    /// Monta exec en la memoria de la caja: ExecBase con su tabla de saltos, las listas del sistema, los
    /// `MemHeader` de cada región y la base en la dirección 4.
    pub fn arrancar(mem: M, config: Config) -> Result<Exec<M>, Fallo> {
        let mut e = Exec {
            mem,
            base: 0,
            sistema: config.sistema,
            bloques: BTreeMap::new(),
            regiones: Vec::new(),
            tareas: BTreeMap::new(),
            trampolines: BTreeMap::new(),
            ajenas: BTreeMap::new(),
            avisos: Vec::new(),
            replanificar: false,
            turno_acabado: false,
            quitadas: Vec::new(),
            codigo_cambiado: Vec::new(),
            fin_de_tarea: 0,
            fin_sistema: FIN_ZONA_SISTEMA,
            charcos: BTreeMap::new(),
            esperas_semaforo: Vec::new(),
            doio_en_espera: Vec::new(),
            dispositivos: alloc::collections::BTreeSet::new(),
            ramlib: ramlib::Ramlib::default(),
            entregas: alloc::collections::BTreeMap::new(),
            n_entrega: 0,
            memoria_del_sistema: false,
            vuelta_caja: None,
            cacr: CACR_DE_SERIE,
            memoria_baja: memoria_baja::MemoriaBaja::default(),
            bases_propias: BTreeMap::new(),
        };
        // La zona del sistema tiene que existir entera, fuera de la página 0.
        let (zs, fz) = config.sistema;
        if zs < PAGINA_CERO || fz <= zs {
            return Err(Fallo::SinSitio);
        }
        e.mem.leer(zs, fz - zs).map_err(|_| Fallo::SinSitio)?;
        e.cero(zs, fz - zs)?;
        e.fin_sistema = fz;

        let tabla = e.de_sistema(NEGSIZE_EXEC + execbase::TAMANO)?;
        let b = tabla + NEGSIZE_EXEC;
        e.base = b;
        for n in 1..=lvo::ENTRADAS {
            e.escribir_entrada_nativa(b - 6 * n as u32, n)?;
        }
        let nombre = e.cadena_de_sistema("exec.library")?;
        let id = e.cadena_de_sistema("exec 40.1 (12.11.2026) DashOS\r\n")?;
        e.e8(b + node::LN_TYPE, nt::LIBRARY)?;
        e.e8(b + node::LN_PRI, 126)?;
        e.e32(b + node::LN_NAME, nombre)?;
        e.e8(b + library::LIB_FLAGS, library::LIBF_SUMUSED | library::LIBF_CHANGED)?;
        e.e16(b + library::LIB_NEGSIZE, NEGSIZE_EXEC as u16)?;
        e.e16(b + library::LIB_POSSIZE, execbase::TAMANO as u16)?;
        e.e16(b + library::LIB_VERSION, 40)?;
        e.e16(b + library::LIB_REVISION, 1)?;
        e.e32(b + library::LIB_IDSTRING, id)?;
        e.e16(b + execbase::SOFTVER, 40)?;
        e.e32(b + execbase::CHKBASE, !b)?;
        e.e16(b + execbase::QUANTUM, 4)?;
        e.e8(b + execbase::IDNESTCNT, 0xFF)?;
        e.e8(b + execbase::TDNESTCNT, 0xFF)?;
        e.e16(b + execbase::ATTNFLAGS, config.attn_flags)?;
        e.e32(b + execbase::TASKSIGALLOC, sig::DEL_SISTEMA)?;
        e.e16(b + execbase::TASKTRAPALLOC, 0x8000)?;
        e.e8(b + execbase::VBLANKFREQUENCY, config.vblank)?;
        e.e8(b + execbase::POWERSUPPLYFREQUENCY, 50)?;
        e.e32(b + execbase::EX_ECLOCKFREQUENCY, config.eclock)?;
        for (lista, tipo) in [
            (execbase::MEMLIST, nt::MEMORY),
            (execbase::RESOURCELIST, nt::RESOURCE),
            (execbase::DEVICELIST, nt::DEVICE),
            (execbase::INTRLIST, nt::INTERRUPT),
            (execbase::LIBLIST, nt::LIBRARY),
            (execbase::PORTLIST, nt::MSGPORT),
            (execbase::TASKREADY, nt::TASK),
            (execbase::TASKWAIT, nt::TASK),
            (execbase::SEMAPHORELIST, nt::SIGNALSEM),
        ] {
            e.new_list(b + lista, tipo)?;
        }
        for i in 0..5 {
            e.new_list(b + execbase::SOFTINTS + i * interrupt::SH_TAMANO, nt::INTERRUPT)?;
        }
        e.new_list(b + execbase::EX_MEMHANDLERS, 0)?;

        // Trampolines de las funciones de exec y el código de fin de tarea.
        let tr = e.de_sistema(6 * lvo::ENTRADAS as u32)?;
        for n in 1..=lvo::ENTRADAS {
            let d = tr + 6 * (n as u32 - 1);
            e.escribir_entrada_nativa(d, n)?;
            e.trampolines.insert(n, d);
        }
        let fin = e.de_sistema(6)?;
        e.escribir_entrada_nativa(fin, lvo::FIN_DE_TAREA)?;
        e.trampolines.insert(lvo::FIN_DE_TAREA, fin);
        e.fin_de_tarea = fin;
        let rep = e.de_sistema(6)?;
        e.escribir_entrada_nativa(rep, lvo::REPETIR)?;
        e.trampolines.insert(lvo::REPETIR, rep);
        e.e32(b + execbase::TASKEXITCODE, fin)?;

        // La memoria repartible: un MemHeader al principio de cada región.
        for r in &config.regiones {
            e.anadir_region(r)?;
        }
        if e.regiones.is_empty() {
            return Err(Fallo::SinSitio);
        }

        e.enqueue(b + execbase::LIBLIST, b)?;
        e.sum_library(b)?;
        // La base de exec, en la dirección 4: lo único que se escribe en la página 0.
        e.mem.escribir32(4, b)?;
        Ok(e)
    }

    fn anadir_region(&mut self, r: &Region) -> Result<(), Fallo> {
        let inicio = (r.inicio.max(self.fin_sistema) + 7) & !7;
        let fin = r.fin & !7;
        let nombre = self.cadena_de_sistema(r.nombre)?;
        let primero = inicio + memheader::TAMANO;
        if fin <= primero + 64 {
            return Ok(());
        }
        self.mem.leer(inicio, fin - inicio).map_err(|_| Fallo::SinSitio)?;
        let mh = inicio;
        self.cero(mh, memheader::TAMANO)?;
        self.e8(mh + node::LN_TYPE, nt::MEMORY)?;
        self.e8(mh + node::LN_PRI, r.prioridad as u8)?;
        self.e32(mh + node::LN_NAME, nombre)?;
        self.e16(mh + memheader::MH_ATTRIBUTES, r.atributos)?;
        self.e32(mh + memheader::MH_FIRST, primero)?;
        self.e32(mh + memheader::MH_LOWER, primero)?;
        self.e32(mh + memheader::MH_UPPER, fin)?;
        self.e32(mh + memheader::MH_FREE, fin - primero)?;
        self.e32(primero + memchunk::MC_NEXT, 0)?;
        self.e32(primero + memchunk::MC_BYTES, fin - primero)?;
        self.enqueue(self.base + execbase::MEMLIST, mh)?;
        self.regiones.push((primero, fin));
        Ok(())
    }

    /// Reserva en la zona del sistema (ExecBase, cadenas, trampolines). Nunca se libera.
    fn de_sistema(&mut self, largo: u32) -> Result<u32, Fallo> {
        let d = (self.sistema.0 + 3) & !3;
        if d + largo > self.sistema.1 {
            return Err(Fallo::SinSitio);
        }
        self.sistema.0 = d + largo;
        Ok(d)
    }

    fn cadena_de_sistema(&mut self, s: &str) -> Result<u32, Fallo> {
        let d = self.de_sistema(s.len() as u32 + 1)?;
        self.mem.leer_mut(d, s.len() as u32)?.copy_from_slice(s.as_bytes());
        self.e8(d + s.len() as u32, 0)?;
        Ok(d)
    }

    // --- Lo que mira la caja ---

    /// La dirección de ExecBase (la que hay en la dirección 4).
    pub fn base(&self) -> u32 {
        self.base
    }

    /// La tarea que tiene el testigo (`SysBase->ThisTask`), o 0.
    pub fn tarea_actual(&self) -> u32 {
        self.mem.leer32(self.base + execbase::THISTASK).unwrap_or(0)
    }

    pub fn avisos(&self) -> &[Aviso] {
        &self.avisos
    }

    /// Saca los avisos apuntados (para el registro del Enforcer o Forja).
    pub fn tomar_avisos(&mut self) -> Vec<Aviso> {
        core::mem::take(&mut self.avisos)
    }

    /// Tareas quitadas desde la última vez: la caja acaba sus hilos.
    pub fn tomar_quitadas(&mut self) -> Vec<u32> {
        core::mem::take(&mut self.quitadas)
    }

    /// Tramos de código que el JIT debe olvidar desde la última vez; `(0, 0)` es toda la caché.
    pub fn tomar_codigo_cambiado(&mut self) -> Vec<(u32, u32)> {
        core::mem::take(&mut self.codigo_cambiado)
    }

    /// El bloque reservado que empieza en `dir`, si lo hay.
    pub fn bloque(&self, dir: u32) -> Option<Bloque> {
        self.bloques.get(&dir).copied()
    }

    /// Los bloques de una tarea.
    pub fn bloques_de(&self, tarea: u32) -> Vec<(u32, Bloque)> {
        self.bloques.iter().filter(|(_, b)| b.dueno == tarea).map(|(d, b)| (*d, *b)).collect()
    }

    /// Pasa un bloque a otro dueño (por ejemplo, la pila de una tarea nueva pasa a la tarea).
    pub fn cambiar_dueno(&mut self, dir: u32, dueno: u32) -> bool {
        match self.bloques.get_mut(&dir) {
            Some(b) => {
                b.dueno = dueno;
                true
            }
            None => false,
        }
    }

    /// Las tareas que exec conoce (lista, esperando o en marcha).
    pub fn tareas(&self) -> Vec<u32> {
        self.tareas.keys().copied().collect()
    }

    pub(crate) fn avisar(&mut self, funcion: &'static str, fallo: Fallo) {
        if self.avisos.len() >= MAX_AVISOS {
            self.avisos.remove(0);
        }
        let tarea = self.tarea_actual();
        self.avisos.push(Aviso { tarea, funcion, fallo });
    }

    // --- Acceso a la memoria del 68k con las reglas del Enforcer ---

    fn comprobar(&self, dir: u32, largo: u32, escribir: bool) -> Result<(), Fallo> {
        if dir >= LIMITE || largo > LIMITE - dir {
            return Err(Fallo::FueraDeMemoria { dir, largo });
        }
        if dir < PAGINA_CERO && !(dir == 4 && largo == 4 && !escribir) {
            return Err(Fallo::PaginaCero { dir });
        }
        Ok(())
    }
    pub(crate) fn l8(&self, dir: u32) -> Result<u8, Fallo> {
        self.comprobar(dir, 1, false)?;
        self.mem.leer8(dir)
    }
    pub(crate) fn l16(&self, dir: u32) -> Result<u16, Fallo> {
        self.comprobar(dir, 2, false)?;
        self.mem.leer16(dir)
    }
    pub(crate) fn l32(&self, dir: u32) -> Result<u32, Fallo> {
        self.comprobar(dir, 4, false)?;
        self.mem.leer32(dir)
    }
    /// Lee un puntero de la memoria del 68k. Uno de los últimos 64 KB no vale: así sumarle el desplazamiento de
    /// un campo nunca da la vuelta.
    pub(crate) fn lp(&self, dir: u32) -> Result<u32, Fallo> {
        let v = self.l32(dir)?;
        if v >= LIMITE {
            return Err(Fallo::FueraDeMemoria { dir: v, largo: 0 });
        }
        Ok(v)
    }
    pub(crate) fn e8(&mut self, dir: u32, v: u8) -> Result<(), Fallo> {
        self.comprobar(dir, 1, true)?;
        self.mem.escribir8(dir, v)
    }
    pub(crate) fn e16(&mut self, dir: u32, v: u16) -> Result<(), Fallo> {
        self.comprobar(dir, 2, true)?;
        self.mem.escribir16(dir, v)
    }
    pub(crate) fn e32(&mut self, dir: u32, v: u32) -> Result<(), Fallo> {
        self.comprobar(dir, 4, true)?;
        self.mem.escribir32(dir, v)
    }
    /// Comprueba que se puede leer y escribir todo `[dir, dir + largo)`.
    pub(crate) fn tramo(&self, dir: u32, largo: u32) -> Result<(), Fallo> {
        self.comprobar(dir, largo, true)?;
        self.mem.leer(dir, largo).map(|_| ())
    }
    pub(crate) fn cero(&mut self, dir: u32, largo: u32) -> Result<(), Fallo> {
        self.comprobar(dir, largo, true)?;
        self.mem.leer_mut(dir, largo)?.fill(0);
        Ok(())
    }
    /// Lee una cadena C (sin el 0).
    pub(crate) fn cadena(&self, dir: u32) -> Result<Vec<u8>, Fallo> {
        if dir == 0 {
            return Err(Fallo::PunteroNulo);
        }
        let mut v = Vec::new();
        for i in 0..MAX_CADENA {
            let c = self.l8(dir.checked_add(i).ok_or(Fallo::FueraDeMemoria { dir, largo: i })?)?;
            if c == 0 {
                return Ok(v);
            }
            v.push(c);
        }
        Err(Fallo::CadenaSinFin { dir })
    }

    /// Lee una cadena C como texto (Latin-1), para las pruebas y los registros.
    pub fn leer_cadena(&self, dir: u32) -> Result<String, Fallo> {
        Ok(self.cadena(dir)?.iter().map(|&c| c as char).collect())
    }
}
