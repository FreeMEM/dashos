//! icon.library y workbench.library nativas (`caja/icono`) enchufadas en la caja: sus ficheros, por la dos de la
//! caja (con su guardia, el cajón de la tarea e `IoErr`), y su dibujo, por graphics, intuition y cybergraphics nativas.

use alloc::vec::Vec;
use caja_exec::{Accion, Exec, Registros};
use caja_icono::{Clase, Entorno};

use crate::memoria::MemoriaCaja;
use crate::{CajaDos, Consola};

pub(crate) struct EntornoCaja<'a> {
    pub dos: &'a mut caja_dos::Dos<caja_dos::guardia::Guardia<crate::archivos::Fs>>,
    pub consola: &'a mut Consola,
    pub gr: &'a mut caja_intuition::CajaGrafica<crate::compositor::CompositorWayland>,
}

impl Entorno<MemoriaCaja> for EntornoCaja<'_> {
    fn leer(&mut self, ex: &mut Exec<MemoriaCaja>, nombre: &[u8], tope: u32) -> Result<Vec<u8>, u32> {
        let mut cd = CajaDos { ex, consola: self.consola, sistema: false };
        caja_icono::por_dos::leer(self.dos, &mut cd, nombre, tope)
    }
    fn escribir(&mut self, ex: &mut Exec<MemoriaCaja>, nombre: &[u8], datos: &[u8]) -> Result<(), u32> {
        let mut cd = CajaDos { ex, consola: self.consola, sistema: false };
        caja_icono::por_dos::escribir(self.dos, &mut cd, nombre, datos)
    }
    fn borrar(&mut self, ex: &mut Exec<MemoriaCaja>, nombre: &[u8]) -> Result<(), u32> {
        let mut cd = CajaDos { ex, consola: self.consola, sistema: false };
        caja_icono::por_dos::borrar(self.dos, &mut cd, nombre)
    }
    fn clase(&mut self, ex: &mut Exec<MemoriaCaja>, nombre: &[u8]) -> Result<Clase, u32> {
        let mut cd = CajaDos { ex, consola: self.consola, sistema: false };
        caja_icono::por_dos::clase(self.dos, &mut cd, nombre)
    }
    fn poner_io_err(&mut self, ex: &mut Exec<MemoriaCaja>, codigo: u32) {
        let mut cd = CajaDos { ex, consola: self.consola, sistema: false };
        caja_icono::por_dos::poner_io_err(self.dos, &mut cd, codigo)
    }
    fn nativa(&mut self, ex: &mut Exec<MemoriaCaja>, n: u16, r: &mut Registros) -> bool {
        !matches!(self.gr.llamar(ex, n, r), Accion::Ajena)
    }
}
