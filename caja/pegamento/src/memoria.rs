//! La memoria del 68k vista desde las bibliotecas nativas: los rasgos `Memoria` de exec y de dos sobre la memoria de
//! verdad del proceso de la caja.
//!
//! El 68k ve sus direcciones como direcciones reales del proceso (`caja/caja.h`): la RAM del 68k está proyectada en
//! `[ram, ram + largo)` y la página 0 en la dirección 0, de solo lectura. Para escribirla (ExecBase en la dirección 4)
//! se usa su otra proyección, por encima de 4 GB (`pagina0`), donde el 68k no llega. Lo demás de los primeros 4 GB no
//! es memoria del 68k: `trozo` da `None` y las bibliotecas lo apuntan como un fallo del programa (el Enforcer), sin
//! tocarlo nunca.

use core::slice;

/// La RAM del 68k y la página 0 de una caja.
pub struct MemoriaCaja {
    ram: u32,
    largo: u32,
    /// La dirección por encima de 4 GB donde la caja escribe la página 0.
    pagina0: usize,
}

const PAGINA: u64 = 0x1000;

impl MemoriaCaja {
    /// `ram` y `largo` tienen que ser la memoria del 68k que la caja ya ha proyectado para leer y escribir, y `pagina0`
    /// la proyección de escribir de la página 0 (4 KB). Lo garantiza `caja_main` (caja.c) antes de llamar a
    /// `caja_rust_arrancar`.
    pub fn nueva(ram: u32, largo: u32, pagina0: usize) -> MemoriaCaja {
        MemoriaCaja { ram, largo, pagina0 }
    }

    /// Dónde está `[dir, dir + largo)` en el proceso, si es todo memoria del 68k.
    fn sitio(&self, dir: u32, largo: u32) -> Option<usize> {
        let fin = dir as u64 + largo as u64;
        if fin <= PAGINA {
            return Some(self.pagina0 + dir as usize);
        }
        if dir >= self.ram && fin <= self.ram as u64 + self.largo as u64 {
            return Some(dir as usize);
        }
        None
    }

    pub fn trozo(&self, dir: u32, largo: u32) -> Option<&[u8]> {
        let p = self.sitio(dir, largo)?;
        // SEGURIDAD: `sitio` solo da tramos dentro de la RAM del 68k o de la página 0 por su alias de escribir, que la
        // caja proyectó antes de crear este valor y no quita nunca (viven lo que el proceso). Mientras corre una
        // función nativa, el 68k está parado (el JIT ha llamado a `dashos_native_call`) y el hilo vigilante no toca
        // esta memoria: nadie más la escribe durante el préstamo.
        Some(unsafe { slice::from_raw_parts(p as *const u8, largo as usize) })
    }

    pub fn trozo_mut(&mut self, dir: u32, largo: u32) -> Option<&mut [u8]> {
        let p = self.sitio(dir, largo)?;
        // SEGURIDAD: como en `trozo`; el préstamo es exclusivo porque lo es el de `self`.
        Some(unsafe { slice::from_raw_parts_mut(p as *mut u8, largo as usize) })
    }
}

impl caja_exec::Memoria for MemoriaCaja {
    fn trozo(&self, dir: u32, largo: u32) -> Option<&[u8]> {
        MemoriaCaja::trozo(self, dir, largo)
    }
    fn trozo_mut(&mut self, dir: u32, largo: u32) -> Option<&mut [u8]> {
        MemoriaCaja::trozo_mut(self, dir, largo)
    }
}
