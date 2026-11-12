//! virtio-blk (virtio 1.2 §5.2), respaldado por un [`Bloques`]: en DashOS, el servicio de archivos (un fichero de
//! imagen en un volumen).
//!
//! Una petición es una cadena con una cabecera de 16 bytes que el aparato lee (tipo u32, reservado u32, sector u64),
//! los datos (de lectura para OUT, de escritura para IN) y un byte de estado al final, de escritura.
//!
//! Tipos atendidos: IN, OUT, FLUSH y GET_ID. Los demás dan VIRTIO_BLK_S_UNSUPP. Un sector fuera del disco, unos datos
//! que no son múltiplo de 512 o una petición de más de [`MAX_PETICION`] dan VIRTIO_BLK_S_IOERR. Sin byte de estado no
//! hay forma de contestar: la cadena se devuelve con 0 bytes escritos.
//!
//! Los datos se copian en trozos de [`TROZO`] bytes, sin reservar memoria según lo que pida el invitado.

use alloc::boxed::Box;
use alloc::vec::Vec;

use super::cola::{Cadena, ColaPartida};
use super::consola::copiar_config;
use super::{AparatoVirtio, ErrorVirtio, F_INDIRECT_DESC, ID_BLOQUE, vaciar_cola};
use crate::memoria::MemoriaInvitado;

pub const F_SIZE_MAX: u64 = 1 << 1;
pub const F_SEG_MAX: u64 = 1 << 2;
pub const F_RO: u64 = 1 << 5;
pub const F_BLK_SIZE: u64 = 1 << 6;
pub const F_FLUSH: u64 = 1 << 9;

pub const T_IN: u32 = 0;
pub const T_OUT: u32 = 1;
pub const T_FLUSH: u32 = 4;
pub const T_GET_ID: u32 = 8;

pub const S_OK: u8 = 0;
pub const S_IOERR: u8 = 1;
pub const S_UNSUPP: u8 = 2;

pub const SECTOR: u64 = 512;
/// Datos como mucho en una petición (lo que se anuncia en size_max × seg_max queda por debajo).
pub const MAX_PETICION: u64 = 4 << 20;
pub const TROZO: usize = 4096;
const TAM_COLA: u16 = 256;
/// Segmentos de datos como mucho (seg_max). Con size_max da 4 MiB, lo mismo que MAX_PETICION.
const SEG_MAX: u32 = 64;
const SIZE_MAX: u32 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorBloques;

/// Un disco de sectores de 512 bytes.
pub trait Bloques {
    fn sectores(&self) -> u64;
    fn solo_lectura(&self) -> bool {
        false
    }
    /// Lee desde `sector`; `buf.len()` es múltiplo de 512 y no pasa del final (ya comprobado).
    fn leer(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), ErrorBloques>;
    fn escribir(&mut self, sector: u64, datos: &[u8]) -> Result<(), ErrorBloques>;
    fn vaciar(&mut self) -> Result<(), ErrorBloques> {
        Ok(())
    }
    /// El número de serie que ve el invitado (20 bytes, GET_ID).
    fn id(&self) -> [u8; 20] {
        *b"dashos-caja-linux\0\0\0"
    }
}

/// Un disco en memoria, para pruebas y para discos pequeños de arranque.
pub struct DiscoEnMemoria {
    pub datos: Vec<u8>,
    pub solo_lectura: bool,
}

impl Bloques for DiscoEnMemoria {
    fn sectores(&self) -> u64 {
        self.datos.len() as u64 / SECTOR
    }
    fn solo_lectura(&self) -> bool {
        self.solo_lectura
    }
    fn leer(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), ErrorBloques> {
        let ini = sector.checked_mul(SECTOR).ok_or(ErrorBloques)? as usize;
        let fin = ini.checked_add(buf.len()).ok_or(ErrorBloques)?;
        buf.copy_from_slice(self.datos.get(ini..fin).ok_or(ErrorBloques)?);
        Ok(())
    }
    fn escribir(&mut self, sector: u64, datos: &[u8]) -> Result<(), ErrorBloques> {
        if self.solo_lectura {
            return Err(ErrorBloques);
        }
        let ini = sector.checked_mul(SECTOR).ok_or(ErrorBloques)? as usize;
        let fin = ini.checked_add(datos.len()).ok_or(ErrorBloques)?;
        self.datos.get_mut(ini..fin).ok_or(ErrorBloques)?.copy_from_slice(datos);
        Ok(())
    }
}

pub struct Bloque {
    disco: Box<dyn Bloques>,
    trozo: Vec<u8>,
}

impl Bloque {
    pub fn nuevo(disco: Box<dyn Bloques>) -> Self {
        Self { disco, trozo: alloc::vec![0; TROZO] }
    }

    fn config(&self) -> [u8; 36] {
        let mut c = [0u8; 36];
        c[0..8].copy_from_slice(&self.disco.sectores().to_le_bytes()); // capacity
        c[8..12].copy_from_slice(&SIZE_MAX.to_le_bytes()); // size_max
        c[12..16].copy_from_slice(&SEG_MAX.to_le_bytes()); // seg_max
        // geometry (16..20) a 0: no se ofrece VIRTIO_BLK_F_GEOMETRY.
        c[20..24].copy_from_slice(&(SECTOR as u32).to_le_bytes()); // blk_size
        c
    }

    /// Atiende una petición; devuelve los bytes escritos en la cadena (datos + estado).
    fn peticion(&mut self, c: &Cadena, mem: &mut dyn MemoriaInvitado) -> Result<u32, ErrorVirtio> {
        let escribibles = c.bytes_escritura();
        if escribibles == 0 {
            return Ok(0);
        }
        let mut cab = [0u8; 16];
        let mut lec = c.lector();
        let leidos = lec.leer(mem, &mut cab)?;
        let (estado, datos_escritos) = if leidos < 16 {
            (S_IOERR, 0)
        } else {
            let tipo = u32::from_le_bytes([cab[0], cab[1], cab[2], cab[3]]);
            let sector = u64::from_le_bytes(cab[8..16].try_into().unwrap_or([0; 8]));
            // Los datos de IN son lo escribible menos el byte de estado.
            let datos_in = escribibles - 1;
            match tipo {
                T_IN => self.leer_disco(c, mem, sector, datos_in)?,
                T_OUT => (self.escribir_disco(&mut lec, mem, sector)?, 0),
                T_FLUSH => (if self.disco.vaciar().is_ok() { S_OK } else { S_IOERR }, 0),
                T_GET_ID => {
                    let id = self.disco.id();
                    let mut esc = c.escritor();
                    let n = (datos_in as usize).min(id.len());
                    esc.escribir(mem, &id[..n])?;
                    (S_OK, n as u64)
                }
                _ => (S_UNSUPP, 0),
            }
        };
        // El estado va en el último byte escribible.
        let mut esc = c.escritor();
        esc.saltar(escribibles - 1);
        esc.escribir(mem, &[estado])?;
        Ok((datos_escritos + 1) as u32)
    }

    fn rango_valido(&self, sector: u64, largo: u64) -> bool {
        largo % SECTOR == 0
            && largo <= MAX_PETICION
            && sector.checked_add(largo / SECTOR).is_some_and(|fin| fin <= self.disco.sectores())
    }

    fn leer_disco(
        &mut self,
        c: &Cadena,
        mem: &mut dyn MemoriaInvitado,
        sector: u64,
        largo: u64,
    ) -> Result<(u8, u64), ErrorVirtio> {
        if !self.rango_valido(sector, largo) {
            return Ok((S_IOERR, 0));
        }
        let mut esc = c.escritor();
        let mut hecho = 0u64;
        while hecho < largo {
            let k = ((largo - hecho) as usize).min(TROZO);
            if self.disco.leer(sector + hecho / SECTOR, &mut self.trozo[..k]).is_err() {
                return Ok((S_IOERR, hecho));
            }
            esc.escribir(mem, &self.trozo[..k])?;
            hecho += k as u64;
        }
        Ok((S_OK, hecho))
    }

    fn escribir_disco(
        &mut self,
        lec: &mut super::cola::Cursor<'_>,
        mem: &mut dyn MemoriaInvitado,
        sector: u64,
    ) -> Result<u8, ErrorVirtio> {
        let largo = lec.quedan();
        if self.disco.solo_lectura() || !self.rango_valido(sector, largo) {
            return Ok(S_IOERR);
        }
        let mut hecho = 0u64;
        while hecho < largo {
            let k = ((largo - hecho) as usize).min(TROZO);
            let n = lec.leer(mem, &mut self.trozo[..k])?;
            if n != k || self.disco.escribir(sector + hecho / SECTOR, &self.trozo[..k]).is_err() {
                return Ok(S_IOERR);
            }
            hecho += k as u64;
        }
        Ok(S_OK)
    }
}

impl AparatoVirtio for Bloque {
    fn tipo(&self) -> u32 {
        ID_BLOQUE
    }

    fn caracteristicas(&self) -> u64 {
        let ro = if self.disco.solo_lectura() { F_RO } else { 0 };
        F_SIZE_MAX | F_SEG_MAX | F_BLK_SIZE | F_FLUSH | F_INDIRECT_DESC | ro
    }

    fn colas_max(&self) -> &[u16] {
        &[TAM_COLA]
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
        if cola != 0 {
            return Ok(false);
        }
        vaciar_cola(&mut colas[0], mem, |c, mem| self.peticion(c, mem))
    }
}
