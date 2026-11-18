//! La red de la bsdsocket.library nativa (`caja/bsdsocket`) en la caja como programa de Linux: de momento, sin red.
//! Todo da `SinDireccion` (un programa ve "la red no está" y no se cuelga), como en DashOS sin la llave "red". La de
//! DashOS es `../red.rs`; la de Linux con zócalos de verdad queda pendiente (docs/CAJA_LINUX_LOOP.md).

use alloc::vec::Vec;

use caja_bsdsocket::{ErrorRed, Extremos, Fase, Id, Red};

pub(crate) struct RedCaja;

impl RedCaja {
    pub fn nueva() -> RedCaja {
        RedCaja
    }

    pub fn hay_servicio(&self) -> bool {
        false
    }
}

impl Red for RedCaja {
    fn conectar(&mut self, _ip: [u8; 4], _puerto: u16) -> Result<Id, ErrorRed> {
        Err(ErrorRed::SinDireccion)
    }
    fn fase(&mut self, _id: Id) -> Fase {
        Fase::Error(ErrorRed::NoExiste)
    }
    fn enviar(&mut self, _id: Id, _datos: &[u8]) -> usize {
        0
    }
    fn recibir(&mut self, _id: Id, _buf: &mut [u8]) -> usize {
        0
    }
    fn pendiente(&mut self, _id: Id) -> (usize, usize) {
        (0, 0)
    }
    fn extremos(&mut self, _id: Id) -> Option<Extremos> {
        None
    }
    fn escuchar(&mut self, _puerto: u16) -> Result<Id, ErrorRed> {
        Err(ErrorRed::SinDireccion)
    }
    fn aceptar(&mut self, _oido: Id) -> Result<Option<Id>, ErrorRed> {
        Err(ErrorRed::NoExiste)
    }
    fn udp_abrir(&mut self, _puerto: u16) -> Result<Id, ErrorRed> {
        Err(ErrorRed::SinDireccion)
    }
    fn udp_enviar(&mut self, _id: Id, _datos: &[u8], _ip: [u8; 4], _puerto: u16) -> bool {
        false
    }
    fn udp_recibir(&mut self, _id: Id, _buf: &mut [u8]) -> Option<(usize, [u8; 4], u16)> {
        None
    }
    fn resolver(&mut self, _nombre: &[u8]) -> Result<Id, ErrorRed> {
        Err(ErrorRed::SinDireccion)
    }
    fn resuelto(&mut self, _consulta: Id) -> Option<Result<Vec<[u8; 4]>, ErrorRed>> {
        Some(Err(ErrorRed::NoExiste))
    }
    fn cerrar(&mut self, _id: Id) {}
    fn direccion_local(&mut self) -> [u8; 4] {
        [0; 4]
    }
}
