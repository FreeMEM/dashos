//! La memoria de la caja, vista desde dos.library: los primeros 4 GB del proceso, en big-endian como el 68k.
//!
//! El rasgo tiene la misma forma que `Memoria` de exec (rama `caja-amiga-exec`, `caja/exec/src/memoria.rs`): quien
//! enchufa las dos bibliotecas lo implementa una vez sobre la memoria del proceso y lo pasa a cada una. Aquí va
//! repetido para que este crate se pruebe solo, sin depender de exec.

use alloc::vec;
use alloc::vec::Vec;

/// Acceso a la memoria del 68k. Basta con dar los trozos; las lecturas y escrituras big-endian salen de ahí.
pub trait Memoria {
    /// Los bytes `[dir, dir + largo)`, si están todos en memoria.
    fn trozo(&self, dir: u32, largo: u32) -> Option<&[u8]>;
    /// Lo mismo, para escribir.
    fn trozo_mut(&mut self, dir: u32, largo: u32) -> Option<&mut [u8]>;

    fn leer8(&self, dir: u32) -> Option<u8> {
        Some(self.trozo(dir, 1)?[0])
    }
    fn leer16(&self, dir: u32) -> Option<u16> {
        let b = self.trozo(dir, 2)?;
        Some(u16::from_be_bytes([b[0], b[1]]))
    }
    fn leer32(&self, dir: u32) -> Option<u32> {
        let b = self.trozo(dir, 4)?;
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn escribir8(&mut self, dir: u32, v: u8) -> Option<()> {
        self.trozo_mut(dir, 1)?[0] = v;
        Some(())
    }
    fn escribir16(&mut self, dir: u32, v: u16) -> Option<()> {
        self.trozo_mut(dir, 2)?.copy_from_slice(&v.to_be_bytes());
        Some(())
    }
    fn escribir32(&mut self, dir: u32, v: u32) -> Option<()> {
        self.trozo_mut(dir, 4)?.copy_from_slice(&v.to_be_bytes());
        Some(())
    }
}

/// Memoria de prueba: `tamano` bytes desde la dirección 0.
pub struct MemoriaVec {
    pub datos: Vec<u8>,
}

impl MemoriaVec {
    pub fn nueva(tamano: usize) -> Self {
        let tamano = tamano.min(1usize << 32);
        MemoriaVec { datos: vec![0; tamano] }
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
