//! virtio-console (virtio 1.2 §5.3), un solo puerto: la consola `hvc0` de Linux.
//!
//! Cola 0 (receiveq): búferes vacíos del invitado que el aparato llena con lo que llega de DashOS (teclas).
//! Cola 1 (transmitq): lo que escribe el invitado.
//!
//! Se ofrecen VIRTIO_CONSOLE_F_SIZE (columnas y filas en la configuración) y VIRTIO_CONSOLE_F_EMERG_WRITE (escribir
//! un carácter en `emerg_wr` de la configuración lo saca por la consola, útil antes de tener colas).

use alloc::boxed::Box;

use super::cola::ColaPartida;
use super::{AparatoVirtio, ErrorVirtio, F_INDIRECT_DESC, ID_CONSOLA, vaciar_cola};
use crate::memoria::MemoriaInvitado;

pub const F_SIZE: u64 = 1 << 0;
pub const F_MULTIPORT: u64 = 1 << 1;
pub const F_EMERG_WRITE: u64 = 1 << 2;

const COLA_RX: u16 = 0;
const COLA_TX: u16 = 1;
/// Bytes como mucho que se sacan de una cadena de salida: una cadena enorme no tiene sentido en una consola.
const MAX_SALIDA: u64 = 64 * 1024;

/// El otro lado de la consola, en DashOS (una ventana de terminal, un registro...).
pub trait ExtremoConsola {
    /// Lo que escribió el invitado.
    fn salida(&mut self, datos: &[u8]);
    /// Hay bytes para el invitado.
    fn hay_entrada(&self) -> bool;
    /// Saca hasta `buf.len()` bytes para el invitado; devuelve cuántos.
    fn entrada(&mut self, buf: &mut [u8]) -> usize;
}

pub struct Consola {
    extremo: Box<dyn ExtremoConsola>,
    columnas: u16,
    filas: u16,
}

impl Consola {
    pub fn nueva(extremo: Box<dyn ExtremoConsola>) -> Self {
        Self { extremo, columnas: 80, filas: 25 }
    }

    /// Nuevo tamaño de la ventana. Quien tenga el transporte llama después a `config_cambiada`.
    pub fn tamaño(&mut self, columnas: u16, filas: u16) {
        self.columnas = columnas;
        self.filas = filas;
    }

    fn config(&self) -> [u8; 12] {
        let mut c = [0u8; 12];
        c[0..2].copy_from_slice(&self.columnas.to_le_bytes());
        c[2..4].copy_from_slice(&self.filas.to_le_bytes());
        c[4..8].copy_from_slice(&1u32.to_le_bytes()); // max_nr_ports (solo cuenta con MULTIPORT)
        c
    }

    fn llenar_rx(&mut self, colas: &mut [ColaPartida], mem: &mut dyn MemoriaInvitado) -> Result<bool, ErrorVirtio> {
        let cola = &mut colas[COLA_RX as usize];
        if !cola.lista {
            return Ok(false);
        }
        let mut usados = false;
        let mut buf = [0u8; 256];
        for _ in 0..cola.tam {
            if !self.extremo.hay_entrada() {
                break;
            }
            let Some(c) = cola.siguiente(mem)? else { break };
            let mut esc = c.escritor();
            loop {
                let quedan = esc.quedan().min(buf.len() as u64) as usize;
                if quedan == 0 {
                    break;
                }
                let n = self.extremo.entrada(&mut buf[..quedan]);
                if n == 0 {
                    break;
                }
                esc.escribir(mem, &buf[..n.min(quedan)])?;
            }
            cola.devolver(mem, c.cabeza, esc.hecho as u32)?;
            usados = true;
        }
        Ok(usados && cola.quiere_aviso(mem)?)
    }
}

impl AparatoVirtio for Consola {
    fn tipo(&self) -> u32 {
        ID_CONSOLA
    }

    fn caracteristicas(&self) -> u64 {
        F_SIZE | F_EMERG_WRITE | F_INDIRECT_DESC
    }

    fn colas_max(&self) -> &[u16] {
        &[256, 256]
    }

    fn leer_config(&self, desp: u64, datos: &mut [u8]) {
        copiar_config(&self.config(), desp, datos);
    }

    fn escribir_config(&mut self, desp: u64, datos: &[u8]) {
        // emerg_wr: u32 en el desplazamiento 8; el carácter va en el byte bajo.
        if desp == 8 && !datos.is_empty() {
            self.extremo.salida(&datos[..1]);
        }
    }

    fn reiniciar(&mut self) {}

    fn atender(
        &mut self,
        cola: u16,
        colas: &mut [ColaPartida],
        mem: &mut dyn MemoriaInvitado,
    ) -> Result<bool, ErrorVirtio> {
        match cola {
            COLA_TX => {
                let extremo = &mut self.extremo;
                let aviso_tx = vaciar_cola(&mut colas[COLA_TX as usize], mem, |c, mem| {
                    let mut lec = c.lector();
                    let mut buf = [0u8; 256];
                    while lec.hecho < MAX_SALIDA {
                        let n = lec.leer(mem, &mut buf)?;
                        if n == 0 {
                            break;
                        }
                        extremo.salida(&buf[..n]);
                    }
                    Ok(0)
                })?;
                Ok(aviso_tx)
            }
            COLA_RX => self.llenar_rx(colas, mem),
            _ => Ok(false),
        }
    }

    fn sondear(&mut self, colas: &mut [ColaPartida], mem: &mut dyn MemoriaInvitado) -> Result<bool, ErrorVirtio> {
        self.llenar_rx(colas, mem)
    }
}

/// Copia la parte de `config` que pide el invitado; lo que pasa del final se lee como 0.
pub(crate) fn copiar_config(config: &[u8], desp: u64, datos: &mut [u8]) {
    for (i, d) in datos.iter_mut().enumerate() {
        *d = desp
            .checked_add(i as u64)
            .and_then(|p| usize::try_from(p).ok())
            .and_then(|p| config.get(p))
            .copied()
            .unwrap_or(0);
    }
}
