//! Lo que manda el 68k, leído y escrito con las reglas del Enforcer (S3): fuera de la memoria, la página 0 o los
//! últimos 64 KB dan un `Fallo`, nunca un pánico. Y las listas de etiquetas (`utility/tagitem.h`).

use alloc::vec::Vec;

use caja_exec::{Exec, Fallo, LIMITE, Memoria, PAGINA_CERO};

pub(crate) fn comprobar(dir: u32, largo: u32, escribir: bool) -> Result<(), Fallo> {
    if dir >= LIMITE || largo > LIMITE - dir {
        return Err(Fallo::FueraDeMemoria { dir, largo });
    }
    if dir < PAGINA_CERO && !(dir == 4 && largo == 4 && !escribir) {
        return Err(Fallo::PaginaCero { dir });
    }
    Ok(())
}

/// Que `[dir, dir + largo)` exista entero (para leer o escribir).
pub(crate) fn tramo<M: Memoria>(ex: &Exec<M>, dir: u32, largo: u32) -> Result<(), Fallo> {
    if dir == 0 {
        return Err(Fallo::PunteroNulo);
    }
    comprobar(dir, largo, true)?;
    ex.mem.leer(dir, largo).map(|_| ())
}

pub(crate) fn l8<M: Memoria>(ex: &Exec<M>, d: u32) -> Result<u8, Fallo> {
    comprobar(d, 1, false)?;
    ex.mem.leer8(d)
}
pub(crate) fn l16<M: Memoria>(ex: &Exec<M>, d: u32) -> Result<u16, Fallo> {
    comprobar(d, 2, false)?;
    ex.mem.leer16(d)
}
pub(crate) fn l32<M: Memoria>(ex: &Exec<M>, d: u32) -> Result<u32, Fallo> {
    comprobar(d, 4, false)?;
    ex.mem.leer32(d)
}
pub(crate) fn e8<M: Memoria>(ex: &mut Exec<M>, d: u32, v: u8) -> Result<(), Fallo> {
    comprobar(d, 1, true)?;
    ex.mem.escribir8(d, v)
}
pub(crate) fn e16<M: Memoria>(ex: &mut Exec<M>, d: u32, v: u16) -> Result<(), Fallo> {
    comprobar(d, 2, true)?;
    ex.mem.escribir16(d, v)
}
pub(crate) fn e32<M: Memoria>(ex: &mut Exec<M>, d: u32, v: u32) -> Result<(), Fallo> {
    comprobar(d, 4, true)?;
    ex.mem.escribir32(d, v)
}

/// Escribe un LONG donde apunta un puntero del programa (los `..._Query` y `AHI_GetAudioAttrsA`).
pub(crate) fn poner32<M: Memoria>(ex: &mut Exec<M>, d: u32, v: u32) -> Result<(), Fallo> {
    if d == 0 {
        return Err(Fallo::PunteroNulo);
    }
    e32(ex, d, v)
}

/// Copia `texto` (Latin-1) en `[d, d + cabe)`, siempre con su 0 al final (recortado si no cabe). Con `cabe` 0, nada.
pub(crate) fn poner_cadena<M: Memoria>(ex: &mut Exec<M>, d: u32, texto: &[u8], cabe: u32) -> Result<(), Fallo> {
    if cabe == 0 {
        return Ok(());
    }
    if d == 0 {
        return Err(Fallo::PunteroNulo);
    }
    let n = (texto.len() as u32).min(cabe - 1);
    comprobar(d, n + 1, true)?;
    ex.mem.leer_mut(d, n + 1)?[..n as usize].copy_from_slice(&texto[..n as usize]);
    ex.mem.escribir8(d + n, 0)
}

/// La memoria del 68k para el corazón (`audiodevice`): lo que no se puede leer suena como silencio.
pub(crate) struct Lectura<'a, M: Memoria>(pub &'a M);

impl<M: Memoria> audiodevice::Memoria for Lectura<'_, M> {
    fn leer(&self, dir: u32, buf: &mut [u8]) -> bool {
        let n = buf.len() as u32;
        if comprobar(dir, n, false).is_err() {
            return false;
        }
        match self.0.trozo(dir, n) {
            Some(b) => {
                buf.copy_from_slice(b);
                true
            }
            None => false,
        }
    }
}

pub(crate) const TAG_DONE: u32 = 0;
pub(crate) const TAG_IGNORE: u32 = 1;
pub(crate) const TAG_MORE: u32 = 2;
pub(crate) const TAG_SKIP: u32 = 3;
/// Lo más que se lee de una lista de etiquetas: más es una lista sin fin (o en círculo con `TAG_MORE`).
const MAX_ETIQUETAS: usize = 1024;

/// Las etiquetas de una lista (`ti_Tag`, `ti_Data`), siguiendo `TAG_MORE` y saltando `TAG_IGNORE` y `TAG_SKIP`. Sin
/// lista (0), ninguna.
pub(crate) fn etiquetas<M: Memoria>(ex: &Exec<M>, mut d: u32) -> Result<Vec<(u32, u32)>, Fallo> {
    let mut v = Vec::new();
    let mut vueltas = 0;
    while d != 0 {
        vueltas += 1;
        if vueltas > MAX_ETIQUETAS * 2 {
            return Err(Fallo::TablaMala { dir: d });
        }
        let (t, x) = (l32(ex, d)?, l32(ex, d.checked_add(4).ok_or(Fallo::FueraDeMemoria { dir: d, largo: 8 })?)?);
        match t {
            TAG_DONE => break,
            TAG_IGNORE => d = d.wrapping_add(8),
            TAG_MORE => d = x,
            TAG_SKIP => d = d.wrapping_add(8).wrapping_add(x.min(MAX_ETIQUETAS as u32) * 8),
            _ => {
                if v.len() >= MAX_ETIQUETAS {
                    return Err(Fallo::TablaMala { dir: d });
                }
                v.push((t, x));
                d = d.wrapping_add(8);
            }
        }
    }
    Ok(v)
}
