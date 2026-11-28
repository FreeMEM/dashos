//! La salida de ahi.device y audio.device nativos (`caja/sonido`) en la caja como programa de Linux: de momento,
//! callada (el sonido de la caja de Linux espera a S19). Sin hueco: los programas ven un dispositivo que no avanza
//! cuando no hay salida, como en DashOS sin la llave "sonido". La de DashOS es `../sonido.rs`.

pub(crate) struct SalidaCaja;

impl SalidaCaja {
    pub fn nueva() -> SalidaCaja {
        SalidaCaja
    }

    pub fn hay_servicio(&self) -> bool {
        false
    }
}

impl caja_sonido::Salida for SalidaCaja {
    fn hueco(&self) -> usize {
        0
    }

    fn escribir(&mut self, _cuadros: &[[i16; 2]]) {}

    fn pendientes(&self) -> Option<usize> {
        None
    }
}
