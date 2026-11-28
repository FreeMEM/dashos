//! La caja Amiga en Linux (docs/CAJA_LINUX_LOOP.md): intuition sin pantalla todavía, como `CompositorWayland` sin
//! conectar en DashOS: no se abre ninguna ventana (`crear_ventana` da `None`) y el escritorio mide 640 x 480. L5 pone
//! aquí el cliente de Wayland por el socket de Linux.

use alloc::string::String;
use alloc::vec::Vec;

use caja_intuition::compositor::{Compositor, NuevaVentana, Superficie};
use caja_intuition::lienzo::Rect;
use caja_intuition::nitido::TextoNitido;
use wayland::aspecto::Caja;

use crate::tema::TemaCaja;

pub struct CompositorWayland {
    pub conectado: bool,
    pub dichos: Vec<String>,
    pub tema: TemaCaja,
    pantalla: (u32, u32),
}

impl CompositorWayland {
    pub fn sin_conectar() -> CompositorWayland {
        CompositorWayland { conectado: false, dichos: Vec::new(), tema: TemaCaja::default(), pantalla: (640, 480) }
    }

    /// En Linux aún no hay compositor: se queda sin conectar.
    pub fn conectar(_llave: u32) -> CompositorWayland {
        CompositorWayland::sin_conectar()
    }

    pub fn sincronizar(&mut self, _us: u64) -> bool {
        false
    }

    pub fn abiertas(&self) -> usize {
        0
    }
}

impl Compositor for CompositorWayland {
    fn tamano_escritorio(&self) -> (u32, u32) {
        let f = self.tema.ajuste.escala_de_caja(Caja::Amiga).factor(self.pantalla.0, self.pantalla.1);
        ((self.pantalla.0 / f).min(1024), (self.pantalla.1 / f).min(768))
    }

    fn crear_ventana(&mut self, _v: &NuevaVentana) -> Option<Superficie> {
        None
    }

    fn cerrar_ventana(&mut self, _s: Superficie) {}

    fn presentar(&mut self, _s: Superficie, _pix: &[u32], _ancho: u32, _alto: u32, _dano: Rect, _textos: &[TextoNitido]) {}
}
