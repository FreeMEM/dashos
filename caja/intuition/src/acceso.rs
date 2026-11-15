//! Acceso a la memoria del 68k con las reglas del Enforcer de exec (S3): nada en la página 0 (salvo leer la base
//! en la dirección 4) ni en los últimos 64 KB. Un fallo es un `Err`, nunca un pánico.

use alloc::vec::Vec;

use caja_exec::estructuras::memf;
use caja_exec::{Exec, LIMITE, Memoria, PAGINA_CERO};

use crate::Fallo;

type F = caja_exec::Fallo;

/// Tope de una cadena (títulos, nombres de fuente).
pub(crate) const MAX_CADENA: u32 = 4096;

pub(crate) fn comprobar(dir: u32, largo: u32, escribir: bool) -> Result<(), Fallo> {
    if dir >= LIMITE || largo > LIMITE - dir {
        return Err(F::FueraDeMemoria { dir, largo }.into());
    }
    if dir < PAGINA_CERO && !(dir == 4 && largo == 4 && !escribir) {
        return Err(F::PaginaCero { dir }.into());
    }
    Ok(())
}

pub(crate) trait Acceso {
    fn l8(&self, d: u32) -> Result<u8, Fallo>;
    fn l16(&self, d: u32) -> Result<u16, Fallo>;
    fn l32(&self, d: u32) -> Result<u32, Fallo>;
    fn e8(&mut self, d: u32, v: u8) -> Result<(), Fallo>;
    fn e16(&mut self, d: u32, v: u16) -> Result<(), Fallo>;
    fn e32(&mut self, d: u32, v: u32) -> Result<(), Fallo>;
    /// Comprueba que `[d, d + largo)` se puede leer y escribir.
    fn tramo(&self, d: u32, largo: u32) -> Result<(), Fallo>;
    /// Copia `[d, d + largo)`.
    fn bytes(&self, d: u32, largo: u32) -> Result<Vec<u8>, Fallo>;
    fn escribir_bytes(&mut self, d: u32, b: &[u8]) -> Result<(), Fallo>;
    fn cero(&mut self, d: u32, largo: u32) -> Result<(), Fallo>;
    /// Una cadena C (sin el 0).
    fn cadena(&self, d: u32) -> Result<Vec<u8>, Fallo>;
    /// Memoria del sistema (dueño 0: matar la tarea que llamó no la suelta). 0 da `SinMemoria`.
    fn reservar(&mut self, largo: u32, req: u32) -> Result<u32, Fallo>;
    /// Suelta lo de `reservar`.
    fn soltar(&mut self, d: u32, largo: u32) -> Result<(), Fallo>;
    /// Una cadena C nueva en memoria del sistema.
    fn cadena_nueva(&mut self, s: &[u8]) -> Result<u32, Fallo>;
}

impl<M: Memoria> Acceso for Exec<M> {
    fn l8(&self, d: u32) -> Result<u8, Fallo> {
        comprobar(d, 1, false)?;
        Ok(self.mem.leer8(d)?)
    }
    fn l16(&self, d: u32) -> Result<u16, Fallo> {
        comprobar(d, 2, false)?;
        Ok(self.mem.leer16(d)?)
    }
    fn l32(&self, d: u32) -> Result<u32, Fallo> {
        comprobar(d, 4, false)?;
        Ok(self.mem.leer32(d)?)
    }
    fn e8(&mut self, d: u32, v: u8) -> Result<(), Fallo> {
        comprobar(d, 1, true)?;
        Ok(self.mem.escribir8(d, v)?)
    }
    fn e16(&mut self, d: u32, v: u16) -> Result<(), Fallo> {
        comprobar(d, 2, true)?;
        Ok(self.mem.escribir16(d, v)?)
    }
    fn e32(&mut self, d: u32, v: u32) -> Result<(), Fallo> {
        comprobar(d, 4, true)?;
        Ok(self.mem.escribir32(d, v)?)
    }
    fn tramo(&self, d: u32, largo: u32) -> Result<(), Fallo> {
        comprobar(d, largo, true)?;
        self.mem.leer(d, largo)?;
        Ok(())
    }
    fn bytes(&self, d: u32, largo: u32) -> Result<Vec<u8>, Fallo> {
        comprobar(d, largo, false)?;
        Ok(self.mem.leer(d, largo)?.to_vec())
    }
    fn escribir_bytes(&mut self, d: u32, b: &[u8]) -> Result<(), Fallo> {
        let largo = u32::try_from(b.len()).map_err(|_| F::FueraDeMemoria { dir: d, largo: u32::MAX })?;
        comprobar(d, largo, true)?;
        self.mem.leer_mut(d, largo)?.copy_from_slice(b);
        Ok(())
    }
    fn cero(&mut self, d: u32, largo: u32) -> Result<(), Fallo> {
        comprobar(d, largo, true)?;
        self.mem.leer_mut(d, largo)?.fill(0);
        Ok(())
    }
    fn cadena(&self, d: u32) -> Result<Vec<u8>, Fallo> {
        if d == 0 {
            return Err(F::PunteroNulo.into());
        }
        let mut v = Vec::new();
        for i in 0..MAX_CADENA {
            let c = self.l8(d.checked_add(i).ok_or(F::FueraDeMemoria { dir: d, largo: i })?)?;
            if c == 0 {
                return Ok(v);
            }
            v.push(c);
        }
        Err(F::CadenaSinFin { dir: d }.into())
    }
    fn reservar(&mut self, largo: u32, req: u32) -> Result<u32, Fallo> {
        let d = self.alloc_mem(largo, req | memf::PUBLIC)?;
        if d == 0 {
            return Err(Fallo::SinMemoria);
        }
        self.cambiar_dueno(d, 0);
        Ok(d)
    }
    fn soltar(&mut self, d: u32, largo: u32) -> Result<(), Fallo> {
        if d == 0 {
            return Ok(());
        }
        Ok(self.free_mem(d, largo)?)
    }
    fn cadena_nueva(&mut self, s: &[u8]) -> Result<u32, Fallo> {
        let d = self.reservar(s.len() as u32 + 1, memf::CLEAR)?;
        self.escribir_bytes(d, s)?;
        Ok(d)
    }
}

/// Un puntero que se va a usar como base de una estructura: tiene que estar por debajo de `LIMITE`, para que
/// sumarle el desplazamiento de un campo (menos de 64 KB) no dé la vuelta.
pub(crate) fn puntero(d: u32) -> Result<u32, Fallo> {
    if d >= LIMITE { Err(F::FueraDeMemoria { dir: d, largo: 0 }.into()) } else { Ok(d) }
}

/// Un WORD con signo de un registro (las coordenadas de graphics son WORD).
pub(crate) fn palabra(v: u32) -> i32 {
    v as u16 as i16 as i32
}
