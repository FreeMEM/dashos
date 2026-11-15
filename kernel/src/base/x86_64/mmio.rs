//! Registros de los aparatos que toca el kernel en x86-64 (X5): el espacio de configuración del PCIe (ECAM) y las
//! tablas de MSI-X de las BAR. Una `Ventana` es un tramo de física fuera de la RAM proyectado sin caché en el GB de los
//! aparatos (`mapa::proyectar_aparato`); los accesos son de 32 bits (o de 64, X5b), alineados y dentro del tramo. Así, como
//! `RegLe32` en ARM, un registro nunca es memoria normal del kernel.

use core::ptr::{read_volatile, write_volatile};

#[derive(Clone, Copy, Debug)]
pub struct Ventana {
    va: usize,
    tamano: usize,
}

impl Ventana {
    /// Proyecta `tamano` bytes de registros desde `fisica`. `None` si no es de aparatos o no cabe.
    pub fn nueva(fisica: u64, tamano: u64) -> Option<Self> {
        let va = super::mapa::proyectar_aparato(fisica, tamano)?;
        Some(Self { va, tamano: tamano as usize })
    }

    pub fn tamano(&self) -> usize {
        self.tamano
    }

    fn vale(&self, off: usize) -> bool {
        off % 4 == 0 && off.checked_add(4).is_some_and(|f| f <= self.tamano)
    }

    /// Lee el registro de 32 bits en `off`; fuera del tramo o sin alinear, todo unos (como un aparato que no está).
    pub fn leer32(&self, off: usize) -> u32 {
        if !self.vale(off) {
            return u32::MAX;
        }
        unsafe { read_volatile((self.va + off) as *const u32) }
    }

    /// Escribe el registro de 32 bits en `off`; fuera del tramo o sin alinear, no hace nada.
    pub fn escribir32(&self, off: usize, v: u32) {
        if self.vale(off) {
            unsafe { write_volatile((self.va + off) as *mut u32, v) }
        }
    }

    fn vale64(&self, off: usize) -> bool {
        off % 8 == 0 && off.checked_add(8).is_some_and(|f| f <= self.tamano)
    }

    /// Lee el registro de 64 bits en `off` de una vez (X5b: los de las IOMMU); fuera o sin alinear, todo unos.
    pub fn leer64(&self, off: usize) -> u64 {
        if !self.vale64(off) {
            return u64::MAX;
        }
        unsafe { read_volatile((self.va + off) as *const u64) }
    }

    /// Escribe el registro de 64 bits en `off` de una vez; fuera del tramo o sin alinear, no hace nada.
    pub fn escribir64(&self, off: usize, v: u64) {
        if self.vale64(off) {
            unsafe { write_volatile((self.va + off) as *mut u64, v) }
        }
    }
}
