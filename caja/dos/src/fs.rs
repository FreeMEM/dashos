//! El sistema de archivos que hay debajo de dos.library: un rasgo con la forma del protocolo del servicio
//! `archivos` (`usuario/src/protocolo/archivos.rs`, `docs/referencia/servicios.md`). En DashOS lo implementa un
//! adaptador que manda cada orden por el canal del servicio; en las pruebas, `MemFs` (`memfs.rs`).
//!
//! Lo que manda dos aquí es siempre limpio: nombres en UTF-8 sin `/`, `:` ni 0, nunca vacíos, ni `.` ni `..`, y
//! caminos de esas partes juntadas con `/` de hasta `LARGO_CAMINO` bytes. Todo se busca desde una asa que el
//! servicio dio (una raíz de la caja o algo abierto desde ella): así la caja no sale de sus raíces.

use alloc::string::String;
use alloc::vec::Vec;

/// Una asa del servicio: 0 a 15 son raíces del cliente; las que se dan al buscar o crear, desde la 16.
pub type Asa = u32;

/// Lo más largo que puede ser un nombre en un mensaje al servicio (bytes de UTF-8).
pub const LARGO_CAMINO: usize = 224;

/// Los estados de error de las respuestas del servicio, con sus números.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorFs {
    MalFormado,
    NoExiste,
    NoEsCajon,
    EsCajon,
    AsaMala,
    SinSitio,
    ErrorDelDisco,
    SoloLectura,
    YaExiste,
    NoVacio,
    Lleno,
    Protegido,
    EnUso,
    SinAzar,
    /// Lo que el servicio aún no hace (por ejemplo, cambiar la fecha).
    NoSoportado,
}

impl ErrorFs {
    /// El estado de una respuesta del servicio (negativo); `None` si es 0 o no se conoce.
    pub fn de_estado(e: i32) -> Option<ErrorFs> {
        use ErrorFs::*;
        Some(match e {
            -1 => MalFormado,
            -2 => NoExiste,
            -3 => NoEsCajon,
            -4 => EsCajon,
            -5 => AsaMala,
            -6 => SinSitio,
            -7 => ErrorDelDisco,
            -8 => SoloLectura,
            -9 => YaExiste,
            -10 => NoVacio,
            -11 => Lleno,
            -12 => Protegido,
            -13 => EnUso,
            -14 => SinAzar,
            _ => return None,
        })
    }
}

/// Tipo de una entrada (en `DATOS` y `LISTAR`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tipo {
    Fichero,
    Cajon,
    Enlace,
}

impl Tipo {
    pub fn de_byte(b: u8) -> Tipo {
        match b {
            2 => Tipo::Cajon,
            3 => Tipo::Enlace,
            _ => Tipo::Fichero,
        }
    }
}

/// Bits del Amiga que guarda el servicio (DashFS 3): leer, escribir y ejecutar son los permisos de Unix.
pub mod bits {
    pub const ARCHIVADO: u16 = 1;
    pub const PURO: u16 = 2;
    pub const GUION: u16 = 4;
    pub const RESIDENTE: u16 = 8;
    pub const NO_BORRAR_DUENO: u16 = 16;
    pub const NO_BORRAR_GRUPO: u16 = 32;
    pub const NO_BORRAR_OTROS: u16 = 64;
}

/// Lo que dicen `BUSCAR`, `DATOS` y `CREAR` de un objeto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Datos {
    pub tamano: u64,
    /// Segundos desde 1970.
    pub fecha: u64,
    /// Modo de Unix: permisos en los 12 bits de abajo.
    pub modo: u16,
    pub tipo: Tipo,
    /// Bits del Amiga (`bits`).
    pub amiga: u16,
}

impl Datos {
    /// Desempaqueta la `c` de la respuesta: modo | tipo << 16 | bits del Amiga << 32.
    pub fn de_respuesta(tamano: u64, fecha: u64, c: u64) -> Datos {
        Datos {
            tamano,
            fecha,
            modo: (c & 0xFFFF) as u16,
            tipo: Tipo::de_byte(((c >> 16) & 0xFF) as u8),
            amiga: ((c >> 32) & 0xFFFF) as u16,
        }
    }
}

/// Una entrada de `LISTAR`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entrada {
    pub nombre: String,
    pub comentario: String,
    pub datos: Datos,
}

/// Una página de `LISTAR`: las entradas desde la pedida y el número de la siguiente (`None` si no hay más).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pagina {
    pub entradas: Vec<Entrada>,
    pub siguiente: Option<u64>,
}

/// Datos de un volumen (`INFO`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InfoVolumen {
    pub tamano: u64,
    pub libre: u64,
    pub bloque: u32,
}

/// El servicio de archivos, orden por orden. Cada método es una orden del protocolo; el número va en su comentario.
pub trait SistemaArchivos {
    /// 1 buscar: abre lo que hay en `camino` (partes separadas por `/`; vacío, lo mismo) desde `asa`.
    fn buscar(&mut self, asa: Asa, camino: &str) -> Result<(Asa, Datos), ErrorFs>;
    /// 1 buscar con a = 1: solo sus datos, sin abrirlo.
    fn mirar(&mut self, asa: Asa, camino: &str) -> Result<Datos, ErrorFs>;
    /// 2 leer: desde el byte `desde`; devuelve cuántos bytes dejó en `buf`.
    fn leer(&mut self, asa: Asa, desde: u64, buf: &mut [u8]) -> Result<usize, ErrorFs>;
    /// 3 escribir: desde el byte `desde`; devuelve cuántos escribió.
    fn escribir(&mut self, asa: Asa, desde: u64, datos: &[u8]) -> Result<usize, ErrorFs>;
    /// 4 listar: las entradas del cajón desde la número `desde`.
    fn listar(&mut self, asa: Asa, desde: u64) -> Result<Pagina, ErrorFs>;
    /// 5 datos.
    fn datos(&mut self, asa: Asa) -> Result<Datos, ErrorFs>;
    /// 6 cerrar.
    fn cerrar(&mut self, asa: Asa) -> Result<(), ErrorFs>;
    /// 7 crear: `nombre` (una parte) en el cajón de `asa`, y lo deja abierto.
    fn crear(&mut self, asa: Asa, nombre: &str, cajon: bool) -> Result<(Asa, Datos), ErrorFs>;
    /// 8 borrar: `nombre` del cajón de `asa`; un cajón, solo vacío.
    fn borrar(&mut self, asa: Asa, nombre: &str) -> Result<(), ErrorFs>;
    /// 9 renombrar: `viejo` del cajón de `asa` a `nuevo` en el cajón de `asa_nueva`. No sobrescribe.
    fn renombrar(&mut self, asa: Asa, viejo: &str, asa_nueva: Asa, nuevo: &str) -> Result<(), ErrorFs>;
    /// 10 tamaño: recorta o alarga; devuelve el tamaño.
    fn cambiar_tamano(&mut self, asa: Asa, tamano: u64) -> Result<u64, ErrorFs>;
    /// 11 vaciar.
    fn vaciar(&mut self) -> Result<(), ErrorFs>;
    /// 12 info, del volumen de `asa`.
    fn info(&mut self, asa: Asa) -> Result<InfoVolumen, ErrorFs>;
    /// 13 comentario (UTF-8, hasta 255 bytes).
    fn comentario(&mut self, asa: Asa) -> Result<String, ErrorFs>;
    /// 14 comentar; vacío, lo quita.
    fn comentar(&mut self, asa: Asa, comentario: &str) -> Result<(), ErrorFs>;
    /// 15 proteger: bits del Amiga y permisos de Unix (hasta 0o7777).
    fn proteger(&mut self, asa: Asa, amiga: u16, modo: u16) -> Result<(), ErrorFs>;
    /// Cambiar la fecha. El servicio aún no tiene orden para esto (servicios.md, "Pendiente").
    fn fechar(&mut self, _asa: Asa, _fecha: u64) -> Result<(), ErrorFs> {
        Err(ErrorFs::NoSoportado)
    }
}
