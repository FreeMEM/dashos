//! dos.library nativa de la caja Amiga (S4 §4, S4b, S5).
//!
//! La caja es un proceso de DashOS donde corren programas 68k con el JIT de Emu68. Esta dos.library está escrita en
//! Rust pero con la interfaz del 68k: sus estructuras (`FileHandle`, `FileLock`, `FileInfoBlock`, `InfoData`,
//! `DosList`, `RDArgs`...) viven en la memoria de la caja, en big-endian y con punteros de 32 bits (BPTR y BSTR de
//! BCPL donde AmigaOS los usa), y cada función se llama por línea A con los registros del 68k (`Dos::llamar`).
//!
//! Debajo no hay discos Amiga falsos: los ficheros son los del servicio de archivos de DashOS, por el rasgo
//! `SistemaArchivos` (`fs.rs`), que tiene la forma de su protocolo. Los volúmenes y asignaciones son los de S4b; las
//! rutas del Amiga (`Volumen:cajón/fichero`, `/` para subir, `:` para la raíz) se resuelven aquí, y al servicio solo
//! le llegan nombres limpios desde una raíz que él mismo dio: la caja no puede salir de sus raíces.
//!
//! Escrito desde la documentación pública: los autodocs de dos.library 40, el ROM Kernel Reference Manual y los
//! includes del NDK 3.1 (`dos/*.h`, `dos_lib.fd`) como especificación de la interfaz. No se ha leído ni copiado código
//! de AmigaOS, AROS, UAE ni Amiberry.
//!
//! Lo malo nunca tumba la caja: un puntero fuera de la memoria, un cerrojo o asa que dos no dio, una ruta que no vale
//! o un bucle de asignaciones dan el error del Amiga en `IoErr()` y el valor de fallo de la función, y lo que es un
//! fallo del programa se apunta además como aviso (el registro del Enforcer de S3). Cómo se enchufa:
//! `docs/bitacoras/caja-amiga-dos.md`.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod errores;
pub mod estructuras;
pub mod fs;
pub mod lvo;
pub mod memfs;
pub mod memoria;
pub mod nombres;
pub mod registros;

mod args;
mod cambios;
mod cerrojos;
mod despacho;
mod espacio;
pub mod exall;
mod ficheros;
mod patrones;
mod texto;
mod vars;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

pub use cerrojos::{de_proteccion, proteccion_de};
use errores::*;
pub use espacio::CapasS4b;
use estructuras::*;
pub use fs::{Asa, SistemaArchivos};
pub use memfs::MemFs;
pub use memoria::{Memoria, MemoriaVec};
pub use registros::Registros;

/// Lo que dos necesita de la caja, además de su memoria. En DashOS lo da exec (reservar, la tarea en marcha) y el
/// proceso de la caja (la hora, la consola).
pub trait Caja: Memoria {
    /// Reserva `largo` bytes a cero en la memoria del 68k, alineados a 4, de la tarea en marcha (`AllocVec` con
    /// `MEMF_PUBLIC | MEMF_CLEAR`). 0 si no hay.
    fn reservar(&mut self, largo: u32) -> u32;
    /// Suelta lo reservado con `reservar`.
    fn liberar(&mut self, dir: u32, largo: u32);
    /// La tarea en marcha (`SysBase->ThisTask`).
    fn tarea_actual(&self) -> u32;
    /// La hora: segundos desde 1970 y microsegundos.
    fn hora(&self) -> (u64, u32) {
        (DIAS_1970_A_1978 * 86400, 0)
    }
    /// Escribe en la consola de la caja; devuelve cuántos bytes salieron.
    fn consola_escribir(&mut self, datos: &[u8]) -> usize {
        datos.len()
    }
    /// Lee de la consola de la caja; 0 es el final.
    fn consola_leer(&mut self, _buf: &mut [u8]) -> usize {
        0
    }
    /// `LoadSeg`: carga el ejecutable `datos` (el fichero entero) en la memoria de la caja y devuelve su lista de
    /// segmentos (un BPTR), o un código de error de dos (`ERROR_BAD_HUNK`, `ERROR_NO_FREE_STORE`...). El formato
    /// hunk es del cargador (rama `caja-amiga-hunk`); sin él, `ERROR_NOT_IMPLEMENTED`.
    fn cargar_segmentos(&mut self, _datos: &[u8]) -> Result<u32, u32> {
        Err(ERROR_NOT_IMPLEMENTED)
    }
    /// `UnLoadSeg`: suelta una lista de segmentos de `cargar_segmentos`.
    fn descargar_segmentos(&mut self, _segmentos: u32) {}
    /// `CheckSignal`: las señales de `mascara` que han llegado a la tarea en marcha, que quedan borradas (como
    /// `SetSignal(0, mascara)` de exec).
    fn senales(&mut self, _mascara: u32) -> u32 {
        0
    }
}

/// Un fallo del programa que dos apunta como aviso, además de devolver el error del Amiga.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fallo {
    /// Una dirección fuera de la memoria de la caja (el Enforcer).
    FueraDeMemoria { dir: u32, largo: u32 },
    /// Un acceso a la página 0.
    PaginaCero { dir: u32 },
    /// Un puntero que no puede ser 0.
    PunteroNulo,
    /// Una cadena sin el 0 del final en un tramo razonable.
    CadenaSinFin { dir: u32 },
    /// Un BPTR que no cabe en 32 bits al multiplicarlo por 4.
    BptrMalo { bptr: u32 },
    /// Un cerrojo que dos no dio, o que ya se soltó.
    CerrojoDesconocido { bptr: u32 },
    /// Un cerrojo que es de una asignación: no lo suelta el programa.
    CerrojoDeAsignacion { bptr: u32 },
    /// Un fichero que dos no abrió, o que ya se cerró.
    AsaDesconocida { bptr: u32 },
    /// Un objeto de `FreeDosObject`, `FreeArgs` o `UnLoadSeg` que no es de dos.
    ObjetoDesconocido { dir: u32 },
    /// Una función de dos que aún no está hecha.
    NoImplementada { n: u16 },
    /// No hay memoria en la caja para una estructura.
    SinMemoria,
}

impl Fallo {
    /// El código de `IoErr()` que deja.
    pub fn codigo(&self) -> u32 {
        match self {
            Fallo::FueraDeMemoria { .. }
            | Fallo::PaginaCero { .. }
            | Fallo::PunteroNulo
            | Fallo::CadenaSinFin { .. }
            | Fallo::BptrMalo { .. }
            | Fallo::ObjetoDesconocido { .. } => ERROR_BAD_NUMBER,
            Fallo::CerrojoDesconocido { .. } | Fallo::CerrojoDeAsignacion { .. } | Fallo::AsaDesconocida { .. } => {
                ERROR_INVALID_LOCK
            }
            Fallo::NoImplementada { .. } => ERROR_NOT_IMPLEMENTED,
            Fallo::SinMemoria => ERROR_NO_FREE_STORE,
        }
    }
}

/// Un fallo apuntado, con la tarea que lo causó y la función.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Aviso {
    pub tarea: u32,
    pub funcion: &'static str,
    pub fallo: Fallo,
}

/// Qué tiene que hacer la caja al volver de una función de dos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accion {
    /// Volver al código 68k.
    Seguir,
    /// `Delay`: dormir la tarea esos tics (50 por segundo) y después volver al 68k.
    Dormir { tics: u32 },
    /// El número no es de dos.
    Ajena,
}

/// El error de una función por dentro: un código de `IoErr()`, o un fallo del programa que además se avisa.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum E {
    Io(u32),
    Mal(Fallo),
}

impl From<Fallo> for E {
    fn from(f: Fallo) -> E {
        E::Mal(f)
    }
}

pub(crate) type R<T> = Result<T, E>;

/// Qué se hacía cuando el servicio dijo "protegido", para elegir el error del Amiga.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    Leer,
    Escribir,
    Borrar,
    Otra,
}

/// El error del Amiga de un error del servicio.
pub(crate) fn de_fs(e: fs::ErrorFs, op: Op) -> E {
    use fs::ErrorFs::*;
    E::Io(match e {
        MalFormado => ERROR_INVALID_COMPONENT_NAME,
        NoExiste => ERROR_OBJECT_NOT_FOUND,
        NoEsCajon | EsCajon => ERROR_OBJECT_WRONG_TYPE,
        AsaMala => ERROR_INVALID_LOCK,
        SinSitio => ERROR_NO_FREE_STORE,
        ErrorDelDisco => ERROR_NOT_A_DOS_DISK,
        SoloLectura => ERROR_DISK_WRITE_PROTECTED,
        YaExiste => ERROR_OBJECT_EXISTS,
        NoVacio => ERROR_DIRECTORY_NOT_EMPTY,
        Lleno => ERROR_DISK_FULL,
        Protegido => match op {
            Op::Leer => ERROR_READ_PROTECTED,
            Op::Borrar => ERROR_DELETE_PROTECTED,
            Op::Escribir | Op::Otra => ERROR_WRITE_PROTECTED,
        },
        EnUso => ERROR_OBJECT_IN_USE,
        SinAzar => ERROR_NOT_IMPLEMENTED,
        NoSoportado => ERROR_ACTION_NOT_KNOWN,
    })
}

/// Lo que va antes de la memoria de la caja: la página 0. Y los últimos 64 KB de los 4 GB, que nunca son de la caja
/// (como en exec): así una dirección válida más un desplazamiento de campo no da la vuelta.
pub const PAGINA_CERO: u32 = 0x1000;
pub const LIMITE: u32 = 0xFFFF_0000;
/// Lo más largo que dos lee de una cadena C del programa.
pub const MAX_CADENA: u32 = 4096;
/// Profundidad máxima al resolver asignaciones que apuntan a otras (más es un bucle).
pub const MAX_NIVELES: u32 = 16;
const MAX_AVISOS: usize = 1024;

/// Un sitio de un volumen: el volumen y las partes desde su raíz (UTF-8, como las guarda el servicio).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Sitio {
    pub vol: usize,
    pub camino: Vec<String>,
}

impl Sitio {
    /// La clave para ver si dos sitios son el mismo objeto (sin distinguir mayúsculas).
    pub fn clave(&self) -> (usize, Vec<String>) {
        (self.vol, self.camino.iter().map(|s| nombres::clave(s)).collect())
    }
}

/// Un volumen montado.
#[derive(Clone, Debug)]
pub(crate) struct Volumen {
    /// El nombre en Latin-1, sin `:`.
    pub nombre: Vec<u8>,
    /// La raíz que dio el servicio. Es de dos para siempre: nunca se cierra.
    pub raiz: Asa,
    pub solo_lectura: bool,
    /// Su `DosList` en la memoria de la caja.
    pub doslist: u32,
    /// Un `MsgPort` vacío que hace de puerto del gestor (`fl_Task`, `fh_Type`): no recibe paquetes.
    pub puerto: u32,
}

/// Un cerrojo dado por dos. La clave del mapa es la dirección de su `FileLock`.
#[derive(Clone, Debug)]
pub(crate) struct Cerrojo {
    pub sitio: Sitio,
    pub asa: Asa,
    pub exclusivo: bool,
    pub cajon: bool,
    /// La tarea que lo pidió (0: dos, para las asignaciones).
    pub dueno: u32,
    /// Lo tiene una asignación: no lo suelta el programa.
    pub de_asignacion: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum Flujo {
    Disco { sitio: Sitio, asa: Asa, pos: u64, exclusivo: bool },
    Consola,
    Nil,
}

/// Un fichero abierto. La clave del mapa es la dirección de su `FileHandle`.
#[derive(Clone, Debug)]
pub(crate) struct Fichero {
    pub flujo: Flujo,
    pub dueno: u32,
    /// El carácter que devolvió `UnGetC` y aún no se ha vuelto a leer.
    pub devuelto: Option<u8>,
    /// El último carácter que dio `FGetC` (para `UnGetC(fh, -1)`).
    pub ultimo: Option<u8>,
}

/// Una asignación (`DLT_DIRECTORY`, `DLT_LATE` o `DLT_NONBINDING`).
#[derive(Clone, Debug)]
pub(crate) enum Destino {
    /// Uno o más cerrojos (direcciones de `FileLock`), en orden: una multiasignación.
    Cerrojos(Vec<u32>),
    /// `AssignLate`: una ruta que se convierte en cerrojo la primera vez que se usa.
    Tarde(Vec<u8>),
    /// `AssignPath`: una ruta que se resuelve cada vez.
    Ruta(Vec<u8>),
}

#[derive(Clone, Debug)]
pub(crate) struct Asignacion {
    /// El nombre en Latin-1, sin `:`, como se escribió.
    pub nombre: Vec<u8>,
    pub destino: Destino,
    /// Su `DosList` en la memoria de la caja, y el BSTR de su nombre.
    pub doslist: u32,
    pub bstr: u32,
    /// Los `AssignList` de los cerrojos de más (de una multiasignación).
    pub listas: Vec<u32>,
    /// La cadena C de `dol_AssignName` (`DLT_LATE`, `DLT_NONBINDING`) y su largo reservado.
    pub cadena: (u32, u32),
}

/// Lo que hay detrás de un nombre de dispositivo.
#[derive(Clone, Debug)]
pub(crate) enum Nombre {
    Volumen(usize),
    Asignacion(Asignacion),
}

/// La dos.library de una caja. `F` es el servicio de archivos.
pub struct Dos<F: SistemaArchivos> {
    pub fs: F,
    pub(crate) volumenes: Vec<Volumen>,
    /// Volúmenes y asignaciones, por su nombre en mayúsculas.
    pub(crate) nombres: BTreeMap<Vec<u8>, Nombre>,
    /// El volumen de arranque: el del cerrojo 0.
    pub(crate) arranque: Option<usize>,
    pub(crate) cerrojos: BTreeMap<u32, Cerrojo>,
    pub(crate) ficheros: BTreeMap<u32, Fichero>,
    /// Campos del `Process` de las tareas que no son procesos (dos no debería llamarse desde ellas, pero no se cae).
    pub(crate) campos: BTreeMap<(u32, u32), u32>,
    /// Lo reservado por `ReadArgs` para cada `RDArgs`: (dirección, largo), y si el `RDArgs` es de dos.
    pub(crate) argumentos: BTreeMap<u32, (Vec<(u32, u32)>, bool)>,
    /// Variables locales: (tarea, tipo, nombre en mayúsculas) → valor. No se ven en `pr_LocalVars`.
    pub(crate) locales: BTreeMap<(u32, u32, Vec<u8>), Vec<u8>>,
    /// Objetos de `AllocDosObject`: dirección → (tipo, largo).
    pub(crate) objetos: BTreeMap<u32, (u32, u32)>,
    /// Listas de segmentos de `LoadSeg`, para no descargar lo que no se cargó.
    pub(crate) segmentos: BTreeMap<u32, u32>,
    /// La cabeza que da `LockDosList` (un `DosList` vacío; su `dol_Next` es el primero).
    pub(crate) cabeza_doslist: u32,
    /// El puerto de la consola (`fh_Type` de sus ficheros).
    pub(crate) puerto_consola: u32,
    /// Lo que se reservó en la caja para siempre (puertos, nombres...): dirección → largo.
    pub(crate) fijos: BTreeMap<u32, u32>,
    pub(crate) avisos: Vec<Aviso>,
}

impl<F: SistemaArchivos> Dos<F> {
    pub fn nuevo(fs: F) -> Dos<F> {
        Dos {
            fs,
            volumenes: Vec::new(),
            nombres: BTreeMap::new(),
            arranque: None,
            cerrojos: BTreeMap::new(),
            ficheros: BTreeMap::new(),
            campos: BTreeMap::new(),
            argumentos: BTreeMap::new(),
            locales: BTreeMap::new(),
            objetos: BTreeMap::new(),
            segmentos: BTreeMap::new(),
            cabeza_doslist: 0,
            puerto_consola: 0,
            fijos: BTreeMap::new(),
            avisos: Vec::new(),
        }
    }

    // --- Lo que mira la caja ---

    pub fn avisos(&self) -> &[Aviso] {
        &self.avisos
    }

    /// Saca los avisos apuntados (para el registro del Enforcer o Forja).
    pub fn tomar_avisos(&mut self) -> Vec<Aviso> {
        core::mem::take(&mut self.avisos)
    }

    /// Cerrojos dados y no soltados (los de las asignaciones incluidos).
    pub fn cerrojos_abiertos(&self) -> usize {
        self.cerrojos.len()
    }

    /// Ficheros abiertos y no cerrados.
    pub fn ficheros_abiertos(&self) -> usize {
        self.ficheros.len()
    }

    pub(crate) fn avisar<C: Caja>(&mut self, c: &C, funcion: &'static str, fallo: Fallo) {
        if self.avisos.len() >= MAX_AVISOS {
            self.avisos.remove(0);
        }
        self.avisos.push(Aviso { tarea: c.tarea_actual(), funcion, fallo });
    }

    // --- La memoria del 68k, con las reglas del Enforcer ---

    pub(crate) fn comprobar(dir: u32, largo: u32) -> R<()> {
        if dir < PAGINA_CERO {
            return Err(Fallo::PaginaCero { dir }.into());
        }
        if dir >= LIMITE || largo > LIMITE - dir {
            return Err(Fallo::FueraDeMemoria { dir, largo }.into());
        }
        Ok(())
    }

    pub(crate) fn l8<C: Caja>(c: &C, dir: u32) -> R<u8> {
        Self::comprobar(dir, 1)?;
        c.leer8(dir).ok_or(Fallo::FueraDeMemoria { dir, largo: 1 }.into())
    }
    pub(crate) fn l16<C: Caja>(c: &C, dir: u32) -> R<u16> {
        Self::comprobar(dir, 2)?;
        c.leer16(dir).ok_or(Fallo::FueraDeMemoria { dir, largo: 2 }.into())
    }
    pub(crate) fn l32<C: Caja>(c: &C, dir: u32) -> R<u32> {
        Self::comprobar(dir, 4)?;
        c.leer32(dir).ok_or(Fallo::FueraDeMemoria { dir, largo: 4 }.into())
    }
    pub(crate) fn e8<C: Caja>(c: &mut C, dir: u32, v: u8) -> R<()> {
        Self::comprobar(dir, 1)?;
        c.escribir8(dir, v).ok_or(Fallo::FueraDeMemoria { dir, largo: 1 }.into())
    }
    pub(crate) fn e32<C: Caja>(c: &mut C, dir: u32, v: u32) -> R<()> {
        Self::comprobar(dir, 4)?;
        c.escribir32(dir, v).ok_or(Fallo::FueraDeMemoria { dir, largo: 4 }.into())
    }
    /// Lee `[dir, dir + largo)`.
    pub(crate) fn bytes<C: Caja>(c: &C, dir: u32, largo: u32) -> R<Vec<u8>> {
        if largo == 0 {
            return Ok(Vec::new());
        }
        Self::comprobar(dir, largo)?;
        c.trozo(dir, largo).map(|b| b.to_vec()).ok_or(Fallo::FueraDeMemoria { dir, largo }.into())
    }
    /// Escribe `datos` en `dir`.
    pub(crate) fn poner<C: Caja>(c: &mut C, dir: u32, datos: &[u8]) -> R<()> {
        if datos.is_empty() {
            return Ok(());
        }
        let largo = u32::try_from(datos.len()).map_err(|_| Fallo::FueraDeMemoria { dir, largo: u32::MAX })?;
        Self::comprobar(dir, largo)?;
        c.trozo_mut(dir, largo).ok_or(Fallo::FueraDeMemoria { dir, largo })?.copy_from_slice(datos);
        Ok(())
    }
    /// Comprueba que se puede escribir todo `[dir, dir + largo)`.
    pub(crate) fn tramo<C: Caja>(c: &mut C, dir: u32, largo: u32) -> R<()> {
        if largo == 0 {
            return Ok(());
        }
        Self::comprobar(dir, largo)?;
        c.trozo_mut(dir, largo).map(|_| ()).ok_or(Fallo::FueraDeMemoria { dir, largo }.into())
    }
    pub(crate) fn cero<C: Caja>(c: &mut C, dir: u32, largo: u32) -> R<()> {
        if largo == 0 {
            return Ok(());
        }
        Self::comprobar(dir, largo)?;
        c.trozo_mut(dir, largo).ok_or(Fallo::FueraDeMemoria { dir, largo })?.fill(0);
        Ok(())
    }
    /// Lee una cadena C (sin el 0).
    pub(crate) fn cadena<C: Caja>(c: &C, dir: u32) -> R<Vec<u8>> {
        if dir == 0 {
            return Err(Fallo::PunteroNulo.into());
        }
        let mut v = Vec::new();
        for i in 0..MAX_CADENA {
            let d = dir.checked_add(i).ok_or(Fallo::FueraDeMemoria { dir, largo: i })?;
            let b = Self::l8(c, d)?;
            if b == 0 {
                return Ok(v);
            }
            v.push(b);
        }
        Err(Fallo::CadenaSinFin { dir }.into())
    }
    /// Escribe una cadena C en un hueco de `largo` bytes (con el 0). `false` si no cabe (y no escribe nada).
    pub(crate) fn poner_cadena<C: Caja>(c: &mut C, dir: u32, largo: u32, s: &[u8]) -> R<bool> {
        if (s.len() as u64) >= largo as u64 {
            return Ok(false);
        }
        Self::poner(c, dir, s)?;
        Self::e8(c, dir + s.len() as u32, 0)?;
        Ok(true)
    }

    /// La dirección de un BPTR (0 es 0).
    pub(crate) fn baddr(b: u32) -> R<u32> {
        if b > 0x3FFF_FFFF {
            return Err(Fallo::BptrMalo { bptr: b }.into());
        }
        Ok(b << 2)
    }

    /// Reserva una estructura en la memoria de la caja.
    pub(crate) fn reservar<C: Caja>(c: &mut C, largo: u32) -> R<u32> {
        let d = c.reservar(largo);
        if d == 0 || d & 3 != 0 {
            if d != 0 {
                c.liberar(d, largo);
            }
            return Err(E::Io(ERROR_NO_FREE_STORE));
        }
        Ok(d)
    }

    /// Reserva para siempre (puertos de volumen, nombres de las `DosList`).
    pub(crate) fn reservar_fijo<C: Caja>(&mut self, c: &mut C, largo: u32) -> R<u32> {
        let d = Self::reservar(c, largo)?;
        self.fijos.insert(d, largo);
        Ok(d)
    }

    pub(crate) fn soltar_fijo<C: Caja>(&mut self, c: &mut C, dir: u32) {
        if let Some(l) = self.fijos.remove(&dir) {
            c.liberar(dir, l);
        }
    }

    /// Un BSTR nuevo, reservado para siempre. Devuelve su dirección (no el BPTR).
    pub(crate) fn bstr_fijo<C: Caja>(&mut self, c: &mut C, s: &[u8]) -> R<u32> {
        let s = &s[..s.len().min(255)];
        let d = self.reservar_fijo(c, s.len() as u32 + 1)?;
        Self::e8(c, d, s.len() as u8)?;
        Self::poner(c, d + 1, s)?;
        Ok(d)
    }

    // --- El proceso en marcha (`struct Process`) ---

    /// La tarea en marcha, si es un proceso (`NT_PROCESS`).
    pub(crate) fn proceso<C: Caja>(c: &C) -> Option<u32> {
        let t = c.tarea_actual();
        if !(PAGINA_CERO..LIMITE - process::TAMANO).contains(&t) {
            return None;
        }
        match c.leer8(t + process::LN_TYPE) {
            Some(process::NT_PROCESS) if c.trozo(t, process::TAMANO).is_some() => Some(t),
            _ => None,
        }
    }

    /// Un campo LONG del `Process` en marcha (`pr_Result2`, `pr_CurrentDir`...). Si la tarea no es un proceso, dos
    /// lo guarda aparte.
    pub(crate) fn campo<C: Caja>(&self, c: &C, desp: u32) -> u32 {
        match Self::proceso(c) {
            Some(p) => c.leer32(p + desp).unwrap_or(0),
            None => self.campos.get(&(c.tarea_actual(), desp)).copied().unwrap_or(0),
        }
    }

    pub(crate) fn poner_campo<C: Caja>(&mut self, c: &mut C, desp: u32, v: u32) {
        match Self::proceso(c) {
            Some(p) => {
                c.escribir32(p + desp, v);
            }
            None => {
                self.campos.insert((c.tarea_actual(), desp), v);
            }
        }
    }

    pub(crate) fn io_err<C: Caja>(&self, c: &C) -> u32 {
        self.campo(c, process::PR_RESULT2)
    }

    pub(crate) fn poner_io_err<C: Caja>(&mut self, c: &mut C, v: u32) {
        self.poner_campo(c, process::PR_RESULT2, v)
    }

    // --- Lo que la caja llama por su cuenta ---

    /// Una tarea ha acabado (o exec la ha matado): dos suelta sus cerrojos, cierra sus ficheros y libera sus
    /// argumentos. Exec suelta después su memoria.
    pub fn acabar_tarea<C: Caja>(&mut self, c: &mut C, tarea: u32) {
        let ficheros: Vec<u32> = self.ficheros.iter().filter(|(_, f)| f.dueno == tarea).map(|(d, _)| *d).collect();
        for d in ficheros {
            let _ = self.cerrar_fichero(c, d);
        }
        let cerrojos: Vec<u32> =
            self.cerrojos.iter().filter(|(_, l)| l.dueno == tarea && !l.de_asignacion).map(|(d, _)| *d).collect();
        for d in cerrojos {
            self.soltar_cerrojo(c, d);
        }
        self.campos.retain(|(t, _), _| *t != tarea);
        self.locales.retain(|(t, _, _), _| *t != tarea);
    }

    /// Escribe en un `DateStamp` de la caja una fecha en segundos desde 1970.
    pub(crate) fn poner_fecha<C: Caja>(c: &mut C, ds: u32, segundos: u64, micros: u32) -> R<()> {
        let s = segundos.saturating_sub(DIAS_1970_A_1978 * 86400);
        let dias = (s / 86400).min(u32::MAX as u64) as u32;
        Self::e32(c, ds + datestamp::DS_DAYS, dias)?;
        Self::e32(c, ds + datestamp::DS_MINUTE, ((s % 86400) / 60) as u32)?;
        Self::e32(c, ds + datestamp::DS_TICK, (s % 60) as u32 * TICKS_PER_SECOND + micros / 20_000)?;
        Ok(())
    }

    /// Lee un `DateStamp` de la caja como segundos desde 1970.
    pub(crate) fn leer_fecha<C: Caja>(c: &C, ds: u32) -> R<u64> {
        Self::comprobar(ds, datestamp::TAMANO)?;
        let dias = Self::l32(c, ds + datestamp::DS_DAYS)? as u64;
        let min = Self::l32(c, ds + datestamp::DS_MINUTE)? as u64;
        let tic = Self::l32(c, ds + datestamp::DS_TICK)? as u64;
        Ok((DIAS_1970_A_1978 + dias) * 86400 + min * 60 + tic / TICKS_PER_SECOND as u64)
    }
}
