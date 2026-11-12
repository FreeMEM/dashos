//! virtio-net (virtio 1.2 §5.1), un par de colas y sin descargas: tramas Ethernet enteras hacia un [`ExtremoRed`].
//! En DashOS, el extremo es la pila de red (S13): un puerto de un conmutador virtual con NAT hacia fuera.
//!
//! Cola 0 (receiveq1): búferes del invitado para tramas que llegan. Cola 1 (transmitq1): tramas que envía.
//! Cada búfer lleva delante la cabecera `virtio_net_hdr` de 12 bytes (con VERSION_1 siempre incluye `num_buffers`).
//! Sin VIRTIO_NET_F_MRG_RXBUF, una trama va en un solo búfer: si no cabe, se descarta y el búfer se devuelve vacío.
//!
//! Se ofrecen MAC, STATUS (enlace arriba) y MTU (1500). Sin sumas de comprobación ni GSO: el invitado manda tramas
//! acabadas y recibe tramas acabadas.

use alloc::boxed::Box;
use alloc::vec::Vec;

use super::cola::ColaPartida;
use super::consola::copiar_config;
use super::{AparatoVirtio, ErrorVirtio, F_INDIRECT_DESC, ID_RED, vaciar_cola};
use crate::memoria::MemoriaInvitado;

pub const F_MTU: u64 = 1 << 3;
pub const F_MAC: u64 = 1 << 5;
pub const F_STATUS: u64 = 1 << 16;

pub const TAM_CABECERA: usize = 12;
pub const MTU: u16 = 1500;
/// Una trama más grande que esto (cabecera Ethernet + MTU + VLAN) no es de este enlace: se descarta.
pub const MAX_TRAMA: usize = 14 + MTU as usize + 4;
const COLA_RX: usize = 0;
const COLA_TX: usize = 1;

/// El otro lado del cable.
pub trait ExtremoRed {
    /// Una trama del invitado.
    fn enviar(&mut self, trama: &[u8]);
    fn hay_trama(&self) -> bool;
    /// La siguiente trama para el invitado.
    fn recibir(&mut self) -> Option<Vec<u8>>;
}

pub struct Red {
    extremo: Box<dyn ExtremoRed>,
    mac: [u8; 6],
    /// Tramas que el invitado intentó mandar demasiado grandes o sin cabecera, y que llegaron sin sitio.
    pub descartadas: u64,
}

impl Red {
    pub fn nueva(extremo: Box<dyn ExtremoRed>, mac: [u8; 6]) -> Self {
        Self { extremo, mac, descartadas: 0 }
    }

    fn config(&self) -> [u8; 12] {
        let mut c = [0u8; 12];
        c[0..6].copy_from_slice(&self.mac);
        c[6..8].copy_from_slice(&1u16.to_le_bytes()); // status: VIRTIO_NET_S_LINK_UP
        c[8..10].copy_from_slice(&1u16.to_le_bytes()); // max_virtqueue_pairs
        c[10..12].copy_from_slice(&MTU.to_le_bytes());
        c
    }

    fn llenar_rx(&mut self, colas: &mut [ColaPartida], mem: &mut dyn MemoriaInvitado) -> Result<bool, ErrorVirtio> {
        let cola = &mut colas[COLA_RX];
        if !cola.lista {
            return Ok(false);
        }
        let mut usados = false;
        for _ in 0..cola.tam {
            if !self.extremo.hay_trama() {
                break;
            }
            let Some(c) = cola.siguiente(mem)? else { break };
            let escrito = match self.extremo.recibir() {
                Some(t) if t.len() <= MAX_TRAMA && (TAM_CABECERA + t.len()) as u64 <= c.bytes_escritura() => {
                    let mut cab = [0u8; TAM_CABECERA];
                    cab[10..12].copy_from_slice(&1u16.to_le_bytes()); // num_buffers
                    let mut esc = c.escritor();
                    esc.escribir(mem, &cab)?;
                    esc.escribir(mem, &t)?;
                    esc.hecho as u32
                }
                Some(_) => {
                    self.descartadas += 1;
                    0
                }
                None => 0,
            };
            cola.devolver(mem, c.cabeza, escrito)?;
            usados = true;
        }
        Ok(usados && cola.quiere_aviso(mem)?)
    }
}

impl AparatoVirtio for Red {
    fn tipo(&self) -> u32 {
        ID_RED
    }

    fn caracteristicas(&self) -> u64 {
        F_MAC | F_STATUS | F_MTU | F_INDIRECT_DESC
    }

    fn colas_max(&self) -> &[u16] {
        &[256, 256]
    }

    fn leer_config(&self, desp: u64, datos: &mut [u8]) {
        copiar_config(&self.config(), desp, datos);
    }

    fn reiniciar(&mut self) {}

    fn atender(
        &mut self,
        cola: u16,
        colas: &mut [ColaPartida],
        mem: &mut dyn MemoriaInvitado,
    ) -> Result<bool, ErrorVirtio> {
        match cola as usize {
            COLA_TX => {
                let extremo = &mut self.extremo;
                let descartadas = &mut self.descartadas;
                let mut trama = Vec::with_capacity(MAX_TRAMA);
                vaciar_cola(&mut colas[COLA_TX], mem, |c, mem| {
                    let total = c.bytes_lectura();
                    if total < TAM_CABECERA as u64 || total - TAM_CABECERA as u64 > MAX_TRAMA as u64 {
                        *descartadas += 1;
                        return Ok(0);
                    }
                    let mut lec = c.lector();
                    lec.saltar(TAM_CABECERA as u64);
                    trama.resize((total - TAM_CABECERA as u64) as usize, 0);
                    lec.leer(mem, &mut trama)?;
                    extremo.enviar(&trama);
                    Ok(0)
                })
            }
            COLA_RX => self.llenar_rx(colas, mem),
            _ => Ok(false),
        }
    }

    fn sondear(&mut self, colas: &mut [ColaPartida], mem: &mut dyn MemoriaInvitado) -> Result<bool, ErrorVirtio> {
        self.llenar_rx(colas, mem)
    }
}
