//! La memoria de la caja: los primeros 4 GB del proceso, que el 68k ve como su espacio de direcciones (S3).
//!
//! Todo va en big-endian, como en el 68k. Una dirección fuera de lo que hay da `Fallo::FueraDeMemoria`, nunca un
//! pánico: es el Enforcer de S3. En la caja de verdad, quien implementa `Memoria` mira la tabla de páginas del
//! proceso; en las pruebas, `MemoriaVec` es un `Vec<u8>` que empieza en la dirección 0.

use alloc::vec;
use alloc::vec::Vec;

use crate::Fallo;

/// Acceso a la memoria del 68k. Basta con dar los trozos; las lecturas y escrituras big-endian salen de ahí.
pub trait Memoria {
    /// Los bytes `[dir, dir + largo)`, si están todos en memoria.
    fn trozo(&self, dir: u32, largo: u32) -> Option<&[u8]>;
    /// Lo mismo, para escribir.
    fn trozo_mut(&mut self, dir: u32, largo: u32) -> Option<&mut [u8]>;

    fn leer(&self, dir: u32, largo: u32) -> Result<&[u8], Fallo> {
        self.trozo(dir, largo).ok_or(Fallo::FueraDeMemoria { dir, largo })
    }
    fn leer_mut(&mut self, dir: u32, largo: u32) -> Result<&mut [u8], Fallo> {
        self.trozo_mut(dir, largo).ok_or(Fallo::FueraDeMemoria { dir, largo })
    }
    fn leer8(&self, dir: u32) -> Result<u8, Fallo> {
        Ok(self.leer(dir, 1)?[0])
    }
    fn leer16(&self, dir: u32) -> Result<u16, Fallo> {
        let b = self.leer(dir, 2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn leer32(&self, dir: u32) -> Result<u32, Fallo> {
        let b = self.leer(dir, 4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn escribir8(&mut self, dir: u32, v: u8) -> Result<(), Fallo> {
        self.leer_mut(dir, 1)?[0] = v;
        Ok(())
    }
    fn escribir16(&mut self, dir: u32, v: u16) -> Result<(), Fallo> {
        self.leer_mut(dir, 2)?.copy_from_slice(&v.to_be_bytes());
        Ok(())
    }
    fn escribir32(&mut self, dir: u32, v: u32) -> Result<(), Fallo> {
        self.leer_mut(dir, 4)?.copy_from_slice(&v.to_be_bytes());
        Ok(())
    }
}

/// Memoria de prueba: `tamano` bytes desde la dirección 0.
pub struct MemoriaVec {
    datos: Vec<u8>,
}

impl MemoriaVec {
    /// `tamano` se recorta a 4 GB, lo que ve el 68k.
    pub fn nueva(tamano: usize) -> Self {
        let tamano = tamano.min(1usize << 32);
        MemoriaVec { datos: vec![0; tamano] }
    }
    pub fn tamano(&self) -> usize {
        self.datos.len()
    }
}

impl Memoria for MemoriaVec {
    fn trozo(&self, dir: u32, largo: u32) -> Option<&[u8]> {
        let i = dir as usize;
        self.datos.get(i..i.checked_add(largo as usize)?)
    }
    fn trozo_mut(&mut self, dir: u32, largo: u32) -> Option<&mut [u8]> {
        let i = dir as usize;
        self.datos.get_mut(i..i.checked_add(largo as usize)?)
    }
}
