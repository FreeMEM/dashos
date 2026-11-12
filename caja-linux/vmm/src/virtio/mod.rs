//! virtio 1.2: el transporte MMIO (§4.2), las colas partidas (§2.7) y los aparatos.

pub mod bloque;
pub mod cola;
pub mod consola;
pub mod gpu;
pub mod mmio;
pub mod red;
pub mod vsock;

use crate::memoria::MemoriaInvitado;
use cola::{ColaPartida, ErrorCola};

/// Bits de característica comunes (§6).
pub const F_INDIRECT_DESC: u64 = 1 << 28;
pub const F_EVENT_IDX: u64 = 1 << 29;
pub const F_VERSION_1: u64 = 1 << 32;

/// Tipos de aparato (§5).
pub const ID_RED: u32 = 1;
pub const ID_BLOQUE: u32 = 2;
pub const ID_CONSOLA: u32 = 3;

/// Un error del que el aparato no se recupera solo: el transporte lo pasa a DEVICE_NEEDS_RESET.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorVirtio {
    Cola(ErrorCola),
}

impl From<ErrorCola> for ErrorVirtio {
    fn from(e: ErrorCola) -> Self {
        ErrorVirtio::Cola(e)
    }
}

impl From<crate::memoria::FueraDeMemoria> for ErrorVirtio {
    fn from(e: crate::memoria::FueraDeMemoria) -> Self {
        ErrorVirtio::Cola(ErrorCola::Memoria(e))
    }
}

/// Un aparato virtio, sin su transporte.
pub trait AparatoVirtio {
    /// Device ID (§5).
    fn tipo(&self) -> u32;
    /// Las características que ofrece, sin VIRTIO_F_VERSION_1 (la pone el transporte).
    fn caracteristicas(&self) -> u64;
    /// Tamaño máximo de cada cola; su largo es el número de colas.
    fn colas_max(&self) -> &[u16];
    /// Lee de su espacio de configuración. Lo que pase del final se lee como 0.
    fn leer_config(&self, desp: u64, datos: &mut [u8]);
    fn escribir_config(&mut self, _desp: u64, _datos: &[u8]) {}
    /// El invitado puso DRIVER_OK con estas características.
    fn activar(&mut self, _aceptadas: u64) {}
    /// El invitado reinició el aparato (Status a 0).
    fn reiniciar(&mut self);
    /// El invitado avisó de la cola `cola`. Devuelve si hay que darle una interrupción de búfer usado.
    fn atender(&mut self, cola: u16, colas: &mut [ColaPartida], mem: &mut dyn MemoriaInvitado)
    -> Result<bool, ErrorVirtio>;
    /// El lado de DashOS tiene algo para el invitado. Devuelve si hay que darle una interrupción.
    fn sondear(&mut self, _colas: &mut [ColaPartida], _mem: &mut dyn MemoriaInvitado) -> Result<bool, ErrorVirtio> {
        Ok(false)
    }
}

/// Saca y atiende todas las cadenas de una cola (como mucho `tam`, para no quedarse atrapado si el invitado sigue
/// llenando desde otro vCPU), con `f` devolviendo los bytes escritos en cada una. Devuelve si hay que avisar.
pub(crate) fn vaciar_cola(
    cola: &mut ColaPartida,
    mem: &mut dyn MemoriaInvitado,
    mut f: impl FnMut(&cola::Cadena, &mut dyn MemoriaInvitado) -> Result<u32, ErrorVirtio>,
) -> Result<bool, ErrorVirtio> {
    let mut usados = false;
    for _ in 0..cola.tam {
        let Some(c) = cola.siguiente(mem)? else { break };
        let n = f(&c, mem)?;
        cola.devolver(mem, c.cabeza, n)?;
        usados = true;
    }
    Ok(usados && cola.quiere_aviso(mem)?)
}
