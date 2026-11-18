//! Lo que la biblioteca pide a la caja: la red de DashOS, sin esperar nunca.
//!
//! Todo es por sondeo: cada orden vuelve en el acto y lo que tarda (conectar, resolver un nombre, que entre alguien)
//! se mira después (`fase`, `resuelto`, `aceptar`). La biblioteca espera ella, dejando correr a las demás tareas 68k,
//! y la caja la despierta con `Bsdsocket::atender`. Encaja con `usuario/src/red_cliente.rs`: una `Conexion` (TCP), un
//! `Udp`, un `Oido` (escuchar: cada uno atiende a una entrada) y `resolver`, todos con un número (`Id`) que da quien
//! implementa el rasgo.

use alloc::vec::Vec;

use crate::definiciones::errno::*;

/// Lo que da la red a cada cosa abierta (conexión, zócalo UDP, oído o consulta de nombre). Lo elige quien implementa
/// `Red`; la biblioteca solo lo guarda y lo devuelve.
pub type Id = u32;

/// Cómo va una conexión TCP (la palabra `FASE` de su memoria en DashOS) o un zócalo UDP.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fase {
    /// Aún no se sabe (el `R_CONECTAR` no ha llegado).
    Conectando,
    Abierta,
    /// El otro cerró: lo que queda por leer se puede leer; enviar ya no.
    Fin,
    Error(ErrorRed),
}

/// Los errores del servicio de red (`protocolo::red`), con lo que significan para un programa BSD.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorRed {
    /// `SIN_DIRECCION`: no hay tarjeta o aún no hay dirección (DHCP).
    SinDireccion,
    /// `LIMITE`: demasiadas conexiones (del programa o del servicio), o preguntas de nombres en curso.
    Limite,
    /// `RECHAZADA`: el otro lado la rechazó o no contestó.
    Rechazada,
    /// No contestó en el plazo (si quien implementa el rasgo lo distingue de `Rechazada`).
    Plazo,
    /// `CORTADA`: el otro la reinició, o venció el plazo con la conexión abierta.
    Cortada,
    /// `NO_EXISTE`: no es de este programa o ya no está.
    NoExiste,
    /// `NO_SOPORTADA`: una familia que no se sabe.
    NoSoportada,
    /// `OCUPADO`: el puerto lo tiene otro.
    Ocupado,
    /// `SIN_NOMBRE`: el nombre no existe o no tiene direcciones IPv4.
    SinNombre,
    /// `SIN_RESPUESTA`: el servidor de nombres no contestó.
    SinRespuesta,
    /// `FALLO_SERVIDOR` o `TRUNCADA`: el servidor contestó que no puede, o algo que no se entiende.
    FalloServidor,
    /// `SIN_PERMISO`: escuchar (o UDP en un puerto elegido) sin el permiso `red-escuchar`.
    SinPermiso,
    /// Cualquier otro código del servicio.
    Otro(i32),
}

impl ErrorRed {
    /// De un código de estado de `protocolo::red` (negativo).
    pub fn de_dashos(codigo: i32) -> ErrorRed {
        match codigo {
            -2 => ErrorRed::SinDireccion,
            -3 => ErrorRed::Limite,
            -4 => ErrorRed::Rechazada,
            -5 => ErrorRed::Cortada,
            -6 => ErrorRed::NoExiste,
            -7 => ErrorRed::NoSoportada,
            -8 => ErrorRed::Ocupado,
            -9 => ErrorRed::SinNombre,
            -10 => ErrorRed::SinRespuesta,
            -11 | -12 => ErrorRed::FalloServidor,
            -13 => ErrorRed::SinPermiso,
            c => ErrorRed::Otro(c),
        }
    }

    /// El `errno` de BSD que le toca. `conectando`: el error llega a un `connect` (el otro no contestó o la rechazó).
    pub fn errno(self, conectando: bool) -> i32 {
        match self {
            ErrorRed::SinDireccion => ENETUNREACH,
            ErrorRed::Limite => ENOBUFS,
            ErrorRed::Rechazada => ECONNREFUSED,
            ErrorRed::Plazo => ETIMEDOUT,
            ErrorRed::Cortada if conectando => ECONNREFUSED,
            ErrorRed::Cortada => ECONNRESET,
            ErrorRed::NoExiste => ENOTCONN,
            ErrorRed::NoSoportada => EAFNOSUPPORT,
            ErrorRed::Ocupado => EADDRINUSE,
            ErrorRed::SinPermiso => EACCES,
            ErrorRed::SinNombre | ErrorRed::SinRespuesta | ErrorRed::FalloServidor => EHOSTUNREACH,
            ErrorRed::Otro(_) => EIO,
        }
    }

    /// El `h_errno` de `gethostbyname` (`netdb.h`).
    pub fn h_errno(self) -> i32 {
        use crate::definiciones::netdb::*;
        match self {
            ErrorRed::SinNombre => HOST_NOT_FOUND,
            ErrorRed::SinRespuesta | ErrorRed::Limite | ErrorRed::SinDireccion | ErrorRed::Plazo => TRY_AGAIN,
            _ => NO_RECOVERY,
        }
    }
}

/// Los extremos de una conexión TCP o de un zócalo UDP.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Extremos {
    /// El puerto de este lado.
    pub local_puerto: u16,
    /// Quién es el otro (TCP; en UDP, 0).
    pub remoto: [u8; 4],
    pub remoto_puerto: u16,
}

/// La red de DashOS, como la ve bsdsocket.library. Ningún método espera: lo que no está, se mira después.
pub trait Red {
    /// Empieza una conexión TCP a `ip:puerto`. Vuelve en el acto; `fase` dice cuándo queda abierta.
    fn conectar(&mut self, ip: [u8; 4], puerto: u16) -> Result<Id, ErrorRed>;
    /// Cómo va una conexión o un zócalo UDP. De algo que no existe, `Error(NoExiste)`.
    fn fase(&mut self, id: Id) -> Fase;
    /// Pone lo que quepa de `datos` en el anillo de envío; cuánto.
    fn enviar(&mut self, id: Id, datos: &[u8]) -> usize;
    /// Saca lo que haya hasta llenar `buf`; cuánto.
    fn recibir(&mut self, id: Id, buf: &mut [u8]) -> usize;
    /// Bytes por leer y sitio para enviar (de una conexión o de un zócalo UDP).
    fn pendiente(&mut self, id: Id) -> (usize, usize);
    /// El puerto propio y quién es el otro.
    fn extremos(&mut self, id: Id) -> Option<Extremos>;
    /// Un oído en el puerto TCP `puerto`: espera una entrada. Pide el permiso `red-escuchar`.
    fn escuchar(&mut self, puerto: u16) -> Result<Id, ErrorRed>;
    /// Si alguien entró por el oído: la conexión nueva (y el oído ya no sirve: se pide otro). `Ok(None)` si aún no.
    fn aceptar(&mut self, oido: Id) -> Result<Option<Id>, ErrorRed>;
    /// Un zócalo UDP en `puerto` (0: el que toque; otro pide el permiso de escuchar).
    fn udp_abrir(&mut self, puerto: u16) -> Result<Id, ErrorRed>;
    /// Pone un datagrama para `ip:puerto`; falso si no cabe ahora.
    fn udp_enviar(&mut self, id: Id, datos: &[u8], ip: [u8; 4], puerto: u16) -> bool;
    /// El siguiente datagrama que llegó, si lo hay: cuánto medía (lo que no cupo en `buf` se pierde) y de quién.
    fn udp_recibir(&mut self, id: Id, buf: &mut [u8]) -> Option<(usize, [u8; 4], u16)>;
    /// El datagrama más grande que se puede mandar.
    fn max_datagrama(&self) -> usize {
        1472
    }
    /// Empieza a resolver `nombre` (direcciones IPv4).
    fn resolver(&mut self, nombre: &[u8]) -> Result<Id, ErrorRed>;
    /// La respuesta, si ya llegó (entonces la consulta se olvida sola).
    fn resuelto(&mut self, consulta: Id) -> Option<Result<Vec<[u8; 4]>, ErrorRed>>;
    /// Suelta lo que sea: una conexión (manda lo que quede y cierra), un zócalo UDP, un oído o una consulta.
    fn cerrar(&mut self, id: Id);
    /// La dirección IPv4 de la caja (0.0.0.0 si no hay).
    fn direccion_local(&mut self) -> [u8; 4];
    /// El nombre de la máquina (`gethostname`).
    fn nombre_maquina(&mut self) -> Vec<u8> {
        Vec::from(&b"amiga"[..])
    }
}
