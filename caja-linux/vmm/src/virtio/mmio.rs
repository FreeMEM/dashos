//! El transporte virtio-mmio, versión 2 (virtio 1.2 §4.2): una ventana de 0x200 bytes con los registros de control
//! y, desde 0x100, el espacio de configuración del aparato.
//!
//! Los registros de control solo se leen y escriben de 32 bits y alineados (§4.2.2.2); otro acceso se lee como 0 y
//! se ignora. El espacio de configuración admite 1, 2, 4 y 8 bytes.
//!
//! Si el aparato falla por algo que hizo el invitado (una cola mal formada), el transporte pone DEVICE_NEEDS_RESET
//! en Status, da una interrupción de cambio de configuración (§2.1.2) y deja de atender colas hasta que el invitado
//! lo reinicie.

use alloc::vec::Vec;

use super::cola::ColaPartida;
use super::{AparatoVirtio, ErrorVirtio, F_INDIRECT_DESC, F_VERSION_1};
use crate::memoria::MemoriaInvitado;
use crate::mmio::{AparatoMmio, Clase, tam_valido};

pub const MAGIA: u32 = 0x7472_6976; // "virt"
pub const VERSION: u32 = 2;
/// Vendor ID: el de DashOS para la caja ("DSH\0"). Linux no lo mira.
pub const VENDEDOR: u32 = 0x0048_5344;
pub const TAM_REGION: u64 = 0x200;

pub const R_MAGIA: u64 = 0x000;
pub const R_VERSION: u64 = 0x004;
pub const R_DEVICE_ID: u64 = 0x008;
pub const R_VENDOR_ID: u64 = 0x00c;
pub const R_DEVICE_FEATURES: u64 = 0x010;
pub const R_DEVICE_FEATURES_SEL: u64 = 0x014;
pub const R_DRIVER_FEATURES: u64 = 0x020;
pub const R_DRIVER_FEATURES_SEL: u64 = 0x024;
pub const R_QUEUE_SEL: u64 = 0x030;
pub const R_QUEUE_NUM_MAX: u64 = 0x034;
pub const R_QUEUE_NUM: u64 = 0x038;
pub const R_QUEUE_READY: u64 = 0x044;
pub const R_QUEUE_NOTIFY: u64 = 0x050;
pub const R_INTERRUPT_STATUS: u64 = 0x060;
pub const R_INTERRUPT_ACK: u64 = 0x064;
pub const R_STATUS: u64 = 0x070;
pub const R_QUEUE_DESC_LOW: u64 = 0x080;
pub const R_QUEUE_DESC_HIGH: u64 = 0x084;
pub const R_QUEUE_DRIVER_LOW: u64 = 0x090;
pub const R_QUEUE_DRIVER_HIGH: u64 = 0x094;
pub const R_QUEUE_DEVICE_LOW: u64 = 0x0a0;
pub const R_QUEUE_DEVICE_HIGH: u64 = 0x0a4;
pub const R_SHM_SEL: u64 = 0x0ac;
pub const R_SHM_LEN_LOW: u64 = 0x0b0;
pub const R_SHM_LEN_HIGH: u64 = 0x0b4;
pub const R_QUEUE_RESET: u64 = 0x0c0;
pub const R_CONFIG_GENERATION: u64 = 0x0fc;
pub const R_CONFIG: u64 = 0x100;

pub const S_ACKNOWLEDGE: u32 = 1;
pub const S_DRIVER: u32 = 2;
pub const S_DRIVER_OK: u32 = 4;
pub const S_FEATURES_OK: u32 = 8;
pub const S_DEVICE_NEEDS_RESET: u32 = 64;
pub const S_FAILED: u32 = 128;

pub const INT_USADO: u32 = 1;
pub const INT_CONFIG: u32 = 2;

pub struct TransporteMmio<A> {
    aparato: A,
    colas: Vec<ColaPartida>,
    estado: u32,
    sel_caract: u32,
    sel_caract_inv: u32,
    aceptadas: u64,
    sel_cola: u32,
    interrupcion: u32,
    generacion: u32,
    /// El último error, si el aparato quedó en DEVICE_NEEDS_RESET.
    pub fallo: Option<ErrorVirtio>,
}

impl<A: AparatoVirtio> TransporteMmio<A> {
    pub fn nuevo(aparato: A) -> Self {
        let n = aparato.colas_max().len();
        Self {
            aparato,
            colas: alloc::vec![ColaPartida::default(); n],
            estado: 0,
            sel_caract: 0,
            sel_caract_inv: 0,
            aceptadas: 0,
            sel_cola: 0,
            interrupcion: 0,
            generacion: 0,
            fallo: None,
        }
    }

    pub fn aparato(&mut self) -> &mut A {
        &mut self.aparato
    }

    pub fn estado(&self) -> u32 {
        self.estado
    }

    fn ofrecidas(&self) -> u64 {
        self.aparato.caracteristicas() | F_VERSION_1
    }

    /// El aparato cambió su configuración (tamaño del disco, de la consola...): §4.2.2 pide subir ConfigGeneration y
    /// avisar.
    pub fn config_cambiada(&mut self) {
        self.generacion = self.generacion.wrapping_add(1);
        if self.estado & S_DRIVER_OK != 0 {
            self.interrupcion |= INT_CONFIG;
        }
    }

    fn reiniciar(&mut self) {
        self.aparato.reiniciar();
        for c in &mut self.colas {
            c.reiniciar();
        }
        self.estado = 0;
        self.sel_caract = 0;
        self.sel_caract_inv = 0;
        self.aceptadas = 0;
        self.sel_cola = 0;
        self.interrupcion = 0;
        self.fallo = None;
    }

    fn romper(&mut self, e: ErrorVirtio) {
        self.fallo = Some(e);
        self.estado |= S_DEVICE_NEEDS_RESET;
        if self.estado & S_DRIVER_OK != 0 {
            self.interrupcion |= INT_CONFIG;
        }
    }

    fn activo(&self) -> bool {
        self.estado & S_DRIVER_OK != 0 && self.estado & (S_DEVICE_NEEDS_RESET | S_FAILED) == 0
    }

    fn resultado(&mut self, r: Result<bool, ErrorVirtio>) {
        match r {
            Ok(true) => self.interrupcion |= INT_USADO,
            Ok(false) => {}
            Err(e) => self.romper(e),
        }
    }

    fn cola_sel(&mut self) -> Option<&mut ColaPartida> {
        self.colas.get_mut(self.sel_cola as usize)
    }

    fn escribir_estado(&mut self, v: u32) {
        if v == 0 {
            self.reiniciar();
            return;
        }
        let nuevos = v & !self.estado;
        let mut v = v;
        if nuevos & S_FEATURES_OK != 0 {
            // §2.2.2: si no puede con las que aceptó el invitado, FEATURES_OK no se queda puesto.
            let ok = self.aceptadas & !self.ofrecidas() == 0 && self.aceptadas & F_VERSION_1 != 0;
            if !ok {
                v &= !S_FEATURES_OK;
            }
        }
        if nuevos & S_DRIVER_OK != 0 {
            if v & S_FEATURES_OK == 0 {
                v &= !S_DRIVER_OK;
            } else {
                let ind = self.aceptadas & F_INDIRECT_DESC != 0;
                for c in &mut self.colas {
                    c.indirectas = ind;
                }
                self.aparato.activar(self.aceptadas);
            }
        }
        // DEVICE_NEEDS_RESET lo pone el aparato, no el invitado.
        self.estado = (v & !S_DEVICE_NEEDS_RESET) | (self.estado & S_DEVICE_NEEDS_RESET);
    }

    fn leer_reg(&mut self, desp: u64) -> u32 {
        match desp {
            R_MAGIA => MAGIA,
            R_VERSION => VERSION,
            R_DEVICE_ID => self.aparato.tipo(),
            R_VENDOR_ID => VENDEDOR,
            R_DEVICE_FEATURES => match self.sel_caract {
                0 => self.ofrecidas() as u32,
                1 => (self.ofrecidas() >> 32) as u32,
                _ => 0,
            },
            R_QUEUE_NUM_MAX => self.aparato.colas_max().get(self.sel_cola as usize).copied().unwrap_or(0) as u32,
            R_QUEUE_NUM => self.cola_sel().map_or(0, |c| c.tam as u32),
            R_QUEUE_READY => self.cola_sel().map_or(0, |c| c.lista as u32),
            R_INTERRUPT_STATUS => self.interrupcion,
            R_STATUS => self.estado,
            R_QUEUE_DESC_LOW => self.cola_sel().map_or(0, |c| c.desc as u32),
            R_QUEUE_DESC_HIGH => self.cola_sel().map_or(0, |c| (c.desc >> 32) as u32),
            R_QUEUE_DRIVER_LOW => self.cola_sel().map_or(0, |c| c.disponible as u32),
            R_QUEUE_DRIVER_HIGH => self.cola_sel().map_or(0, |c| (c.disponible >> 32) as u32),
            R_QUEUE_DEVICE_LOW => self.cola_sel().map_or(0, |c| c.usado as u32),
            R_QUEUE_DEVICE_HIGH => self.cola_sel().map_or(0, |c| (c.usado >> 32) as u32),
            // Sin memoria compartida: largo "todo unos" = la región no existe (§4.2.2).
            R_SHM_LEN_LOW | R_SHM_LEN_HIGH => 0xffff_ffff,
            R_CONFIG_GENERATION => self.generacion,
            _ => 0,
        }
    }

    fn escribir_reg(&mut self, desp: u64, v: u32, mem: &mut dyn MemoriaInvitado) {
        match desp {
            R_DEVICE_FEATURES_SEL => self.sel_caract = v,
            R_DRIVER_FEATURES_SEL => self.sel_caract_inv = v,
            R_DRIVER_FEATURES if self.estado & S_FEATURES_OK == 0 => match self.sel_caract_inv {
                0 => self.aceptadas = (self.aceptadas & !0xffff_ffff) | v as u64,
                1 => self.aceptadas = (self.aceptadas & 0xffff_ffff) | ((v as u64) << 32),
                _ => {}
            },
            R_QUEUE_SEL => self.sel_cola = v,
            R_QUEUE_NUM => {
                if let Some(c) = self.cola_sel().filter(|c| !c.lista) {
                    c.tam = v.min(u16::MAX as u32) as u16;
                }
            }
            R_QUEUE_READY => self.escribir_ready(v),
            R_QUEUE_DESC_LOW | R_QUEUE_DESC_HIGH | R_QUEUE_DRIVER_LOW | R_QUEUE_DRIVER_HIGH | R_QUEUE_DEVICE_LOW
            | R_QUEUE_DEVICE_HIGH => {
                if let Some(c) = self.cola_sel().filter(|c| !c.lista) {
                    let campo = match desp & !0xf {
                        R_QUEUE_DESC_LOW => &mut c.desc,
                        R_QUEUE_DRIVER_LOW => &mut c.disponible,
                        _ => &mut c.usado,
                    };
                    if desp & 4 == 0 {
                        *campo = (*campo & !0xffff_ffff) | v as u64;
                    } else {
                        *campo = (*campo & 0xffff_ffff) | ((v as u64) << 32);
                    }
                }
            }
            R_QUEUE_NOTIFY => self.notificar(v, mem),
            R_INTERRUPT_ACK => self.interrupcion &= !v,
            R_STATUS => self.escribir_estado(v),
            // QueueReset solo con VIRTIO_F_RING_RESET, que no se ofrece; SHMSel no tiene regiones.
            R_QUEUE_RESET | R_SHM_SEL => {}
            _ => {}
        }
    }

    fn escribir_ready(&mut self, v: u32) {
        let sel = self.sel_cola as usize;
        let Some(max) = self.aparato.colas_max().get(sel).copied() else { return };
        let roto = {
            let c = &mut self.colas[sel];
            if v == 0 {
                c.lista = false;
                false
            } else if c.lista {
                false
            } else if c.valida(max) {
                c.lista = true;
                false
            } else {
                true
            }
        };
        if roto {
            // Una cola imposible: el invitado no la verá lista y el aparato pide reinicio.
            self.romper(ErrorVirtio::Cola(super::cola::ErrorCola::NoLista));
        }
    }

    fn notificar(&mut self, cola: u32, mem: &mut dyn MemoriaInvitado) {
        if !self.activo() || cola as usize >= self.colas.len() || !self.colas[cola as usize].lista {
            return;
        }
        let r = self.aparato.atender(cola as u16, &mut self.colas, mem);
        self.resultado(r);
    }
}

impl<A: AparatoVirtio> AparatoMmio for TransporteMmio<A> {
    fn tam_region(&self) -> u64 {
        TAM_REGION
    }

    fn clase(&self) -> Clase {
        Clase::VirtioMmio
    }

    fn leer(&mut self, desp: u64, tam: u8, _mem: &mut dyn MemoriaInvitado) -> u64 {
        if !tam_valido(tam) || desp % tam as u64 != 0 {
            return 0;
        }
        if desp >= R_CONFIG {
            let mut b = [0u8; 8];
            let n = tam as usize;
            if desp.saturating_add(tam as u64) <= TAM_REGION {
                self.aparato.leer_config(desp - R_CONFIG, &mut b[..n]);
            }
            return u64::from_le_bytes(b);
        }
        if tam != 4 {
            return 0;
        }
        self.leer_reg(desp) as u64
    }

    fn escribir(&mut self, desp: u64, tam: u8, valor: u64, mem: &mut dyn MemoriaInvitado) {
        if !tam_valido(tam) || desp % tam as u64 != 0 {
            return;
        }
        if desp >= R_CONFIG {
            if desp.saturating_add(tam as u64) <= TAM_REGION {
                let b = valor.to_le_bytes();
                self.aparato.escribir_config(desp - R_CONFIG, &b[..tam as usize]);
            }
            return;
        }
        if tam == 4 {
            self.escribir_reg(desp, valor as u32, mem);
        }
    }

    fn irq(&self) -> bool {
        self.interrupcion != 0
    }

    fn sondear(&mut self, mem: &mut dyn MemoriaInvitado) {
        if !self.activo() {
            return;
        }
        let r = self.aparato.sondear(&mut self.colas, mem);
        self.resultado(r);
    }
}
