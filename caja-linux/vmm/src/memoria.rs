//! La memoria del invitado, vista desde el servicio de la caja.
//!
//! El servicio tiene la RAM del invitado mapeada en su espacio (la misma memoria que la etapa 2 da al invitado). Los
//! aparatos solo la tocan por este trait, con direcciones físicas del invitado (IPA). Toda dirección se comprueba:
//! una fuera de la RAM da [`FueraDeMemoria`], nunca un pánico.
//!
//! El invitado sigue corriendo en otros vCPU mientras un aparato lee sus colas: lo que se lee se copia una vez y se
//! comprueba la copia (sin leer dos veces el mismo campo). Las barreras entre escribir un elemento del anillo usado y
//! su índice las pone [`MemoriaInvitado::barrera`].

use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FueraDeMemoria {
    pub dir: u64,
    pub largo: u64,
}

pub trait MemoriaInvitado {
    /// Copia `buf.len()` bytes desde la dirección `dir` del invitado.
    fn leer(&self, dir: u64, buf: &mut [u8]) -> Result<(), FueraDeMemoria>;
    /// Copia `datos` a la dirección `dir` del invitado.
    fn escribir(&mut self, dir: u64, datos: &[u8]) -> Result<(), FueraDeMemoria>;
    /// Barrera de memoria entre accesos que el invitado tiene que ver en orden (anillo usado y su índice). En el PC y
    /// en las pruebas no hace falta; el servicio de DashOS pone aquí un `dmb ish` (por una función segura de su
    /// biblioteca).
    fn barrera(&self) {}

    fn leer_u16(&self, dir: u64) -> Result<u16, FueraDeMemoria> {
        let mut b = [0; 2];
        self.leer(dir, &mut b)?;
        Ok(u16::from_le_bytes(b))
    }
    fn leer_u32(&self, dir: u64) -> Result<u32, FueraDeMemoria> {
        let mut b = [0; 4];
        self.leer(dir, &mut b)?;
        Ok(u32::from_le_bytes(b))
    }
    fn leer_u64(&self, dir: u64) -> Result<u64, FueraDeMemoria> {
        let mut b = [0; 8];
        self.leer(dir, &mut b)?;
        Ok(u64::from_le_bytes(b))
    }
    fn escribir_u16(&mut self, dir: u64, v: u16) -> Result<(), FueraDeMemoria> {
        self.escribir(dir, &v.to_le_bytes())
    }
    fn escribir_u32(&mut self, dir: u64, v: u32) -> Result<(), FueraDeMemoria> {
        self.escribir(dir, &v.to_le_bytes())
    }
    fn escribir_u64(&mut self, dir: u64, v: u64) -> Result<(), FueraDeMemoria> {
        self.escribir(dir, &v.to_le_bytes())
    }
}

/// Una región de RAM contigua que empieza en `base` (dirección del invitado). Vale con un `Vec<u8>` (pruebas, PC) o
/// con un `&mut [u8]` de la RAM ya mapeada (servicio de la caja).
pub struct Ram<B> {
    pub base: u64,
    pub datos: B,
}

impl Ram<Vec<u8>> {
    pub fn nueva(base: u64, tam: usize) -> Self {
        Self { base, datos: alloc::vec![0; tam] }
    }
}

impl<B: AsRef<[u8]>> Ram<B> {
    fn rango(&self, dir: u64, largo: usize) -> Result<core::ops::Range<usize>, FueraDeMemoria> {
        let err = FueraDeMemoria { dir, largo: largo as u64 };
        let desp = dir.checked_sub(self.base).ok_or(err)?;
        let fin = desp.checked_add(largo as u64).ok_or(err)?;
        if fin > self.datos.as_ref().len() as u64 {
            return Err(err);
        }
        Ok(desp as usize..fin as usize)
    }

    pub fn tam(&self) -> u64 {
        self.datos.as_ref().len() as u64
    }
}

impl<B: AsRef<[u8]> + AsMut<[u8]>> MemoriaInvitado for Ram<B> {
    fn leer(&self, dir: u64, buf: &mut [u8]) -> Result<(), FueraDeMemoria> {
        let r = self.rango(dir, buf.len())?;
        buf.copy_from_slice(&self.datos.as_ref()[r]);
        Ok(())
    }
    fn escribir(&mut self, dir: u64, datos: &[u8]) -> Result<(), FueraDeMemoria> {
        let r = self.rango(dir, datos.len())?;
        self.datos.as_mut()[r].copy_from_slice(datos);
        Ok(())
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn limites() {
        let mut m = Ram::nueva(0x1000, 0x100);
        assert!(m.escribir_u32(0x1000, 7).is_ok());
        assert_eq!(m.leer_u32(0x1000), Ok(7));
        assert!(m.leer_u32(0x10fc).is_ok());
        assert!(m.leer_u32(0x10fd).is_err());
        assert!(m.leer_u32(0xfff).is_err());
        assert!(m.leer_u64(u64::MAX - 3).is_err());
        let mut b = [0u8; 0];
        assert!(m.leer(0x1100, &mut b).is_ok());
        assert!(m.leer(u64::MAX, &mut b).is_err());
    }
}
