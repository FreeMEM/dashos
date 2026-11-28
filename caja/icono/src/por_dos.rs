//! El `Entorno` de ficheros sobre la dos.library nativa de la caja (`caja_dos`): leer, escribir, borrar y mirar un
//! objeto por las mismas funciones que llama un programa (`Open`, `Read`, `Write`, `Close`, `DeleteFile`, `Lock`,
//! `Examine`), así que pasan por su guardia, sus asignaciones y el cajón actual de la tarea, y dejan `IoErr()` como
//! lo dejaría dos. Los nombres y los tampones van en la memoria de la caja (lo que reserve el `Caja` de dos), y se
//! sueltan al acabar.
//!
//! Quien enchufa icon.library implementa `Entorno` llamando a estas funciones con su `Dos` y su `Caja` de dos.

use alloc::vec::Vec;

use caja_dos::errores::*;
use caja_dos::estructuras::{MODE_NEWFILE, MODE_OLDFILE, SHARED_LOCK, fib};
use caja_dos::{Caja, Dos, Registros, SistemaArchivos, lvo};

use crate::Clase;

/// Lo que se lee o escribe de cada vez.
const TROZO: u32 = 32 * 1024;
/// `HUNK_HEADER`: así empieza un ejecutable.
const HUNK_HEADER: u32 = 0x3F3;
const TAMANO_FIB: u32 = 260;

fn llamar<F: SistemaArchivos, C: Caja>(dos: &mut Dos<F>, c: &mut C, n: u16, d: [u32; 3]) -> u32 {
    let mut r = Registros::default();
    r.d[1..4].copy_from_slice(&d);
    dos.llamar(c, n, &mut r);
    r.d[0]
}

fn io_err<F: SistemaArchivos, C: Caja>(dos: &mut Dos<F>, c: &mut C) -> u32 {
    match llamar(dos, c, lvo::IO_ERR, [0; 3]) {
        0 => ERROR_OBJECT_NOT_FOUND,
        e => e,
    }
}

/// Un tramo de la memoria de la caja que se suelta al salir.
struct Tramo {
    dir: u32,
    largo: u32,
}

impl Tramo {
    fn nuevo<C: Caja>(c: &mut C, largo: u32) -> Result<Tramo, u32> {
        let dir = c.reservar(largo.max(4));
        if dir == 0 { Err(ERROR_NO_FREE_STORE) } else { Ok(Tramo { dir, largo: largo.max(4) }) }
    }
    fn soltar<C: Caja>(self, c: &mut C) {
        c.liberar(self.dir, self.largo);
    }
}

fn nombre<C: Caja>(c: &mut C, n: &[u8]) -> Result<Tramo, u32> {
    if n.len() > 4096 || n.contains(&0) {
        return Err(ERROR_INVALID_COMPONENT_NAME);
    }
    let t = Tramo::nuevo(c, n.len() as u32 + 1)?;
    let Some(m) = c.trozo_mut(t.dir, n.len() as u32 + 1) else {
        t.soltar(c);
        return Err(ERROR_NO_FREE_STORE);
    };
    m[..n.len()].copy_from_slice(n);
    m[n.len()] = 0;
    Ok(t)
}

/// Lee el fichero `n` entero; `ERROR_OBJECT_TOO_LARGE` si pasa de `tope`.
pub fn leer<F: SistemaArchivos, C: Caja>(dos: &mut Dos<F>, c: &mut C, n: &[u8], tope: u32) -> Result<Vec<u8>, u32> {
    let tn = nombre(c, n)?;
    let fh = llamar(dos, c, lvo::OPEN, [tn.dir, MODE_OLDFILE as u32, 0]);
    tn.soltar(c);
    if fh == 0 {
        return Err(io_err(dos, c));
    }
    let r = (|| {
        let buf = Tramo::nuevo(c, TROZO)?;
        let mut v = Vec::new();
        let r = loop {
            let leido = llamar(dos, c, lvo::READ, [fh, buf.dir, TROZO]) as i32;
            if leido < 0 {
                break Err(io_err(dos, c));
            }
            if leido == 0 {
                break Ok(());
            }
            if v.len() + leido as usize > tope as usize {
                break Err(ERROR_OBJECT_TOO_LARGE);
            }
            match c.trozo(buf.dir, leido as u32) {
                Some(b) => v.extend_from_slice(b),
                None => break Err(ERROR_NO_FREE_STORE),
            }
        };
        buf.soltar(c);
        r.map(|_| v)
    })();
    llamar(dos, c, lvo::CLOSE, [fh, 0, 0]);
    r
}

/// Crea o reemplaza el fichero `n` con `datos`. Si no se puede escribir entero, se borra lo que quedó a medias.
pub fn escribir<F: SistemaArchivos, C: Caja>(dos: &mut Dos<F>, c: &mut C, n: &[u8], datos: &[u8]) -> Result<(), u32> {
    let tn = nombre(c, n)?;
    let fh = llamar(dos, c, lvo::OPEN, [tn.dir, MODE_NEWFILE as u32, 0]);
    if fh == 0 {
        let e = io_err(dos, c);
        tn.soltar(c);
        return Err(e);
    }
    let r = (|| {
        let buf = Tramo::nuevo(c, TROZO)?;
        let mut r = Ok(());
        for t in datos.chunks(TROZO as usize) {
            match c.trozo_mut(buf.dir, t.len() as u32) {
                Some(m) => m.copy_from_slice(t),
                None => {
                    r = Err(ERROR_NO_FREE_STORE);
                    break;
                }
            }
            let escrito = llamar(dos, c, lvo::WRITE, [fh, buf.dir, t.len() as u32]);
            if escrito != t.len() as u32 {
                r = Err(io_err(dos, c));
                break;
            }
        }
        buf.soltar(c);
        r
    })();
    let cerrado = llamar(dos, c, lvo::CLOSE, [fh, 0, 0]);
    let r = r.and_then(|_| if cerrado == 0 { Err(io_err(dos, c)) } else { Ok(()) });
    if r.is_err() {
        llamar(dos, c, lvo::DELETE_FILE, [tn.dir, 0, 0]);
    }
    tn.soltar(c);
    r
}

/// Borra el fichero `n`.
pub fn borrar<F: SistemaArchivos, C: Caja>(dos: &mut Dos<F>, c: &mut C, n: &[u8]) -> Result<(), u32> {
    let tn = nombre(c, n)?;
    let ok = llamar(dos, c, lvo::DELETE_FILE, [tn.dir, 0, 0]);
    tn.soltar(c);
    if ok == 0 { Err(io_err(dos, c)) } else { Ok(()) }
}

/// Qué es `n`: un volumen, un cajón, un ejecutable (empieza por `HUNK_HEADER`) o un fichero cualquiera.
pub fn clase<F: SistemaArchivos, C: Caja>(dos: &mut Dos<F>, c: &mut C, n: &[u8]) -> Result<Clase, u32> {
    let tn = nombre(c, n)?;
    let cerrojo = llamar(dos, c, lvo::LOCK, [tn.dir, SHARED_LOCK as u32, 0]);
    if cerrojo == 0 {
        let e = io_err(dos, c);
        tn.soltar(c);
        return Err(e);
    }
    let r = (|| {
        let f = Tramo::nuevo(c, TAMANO_FIB)?;
        let ok = llamar(dos, c, lvo::EXAMINE, [cerrojo, f.dir, 0]);
        let tipo = c.leer32(f.dir + fib::FIB_DIRENTRYTYPE).unwrap_or(0) as i32;
        f.soltar(c);
        if ok == 0 {
            return Err(io_err(dos, c));
        }
        Ok(tipo)
    })();
    llamar(dos, c, lvo::UN_LOCK, [cerrojo, 0, 0]);
    let r = match r {
        Err(e) => Err(e),
        // ST_ROOT (1): la raíz de un volumen.
        Ok(1) => Ok(Clase::Disco),
        Ok(t) if t > 0 => Ok(Clase::Cajon),
        Ok(_) => {
            let fh = llamar(dos, c, lvo::OPEN, [tn.dir, MODE_OLDFILE as u32, 0]);
            let mut ejecutable = false;
            if fh != 0 {
                if let Ok(b) = Tramo::nuevo(c, 4) {
                    if llamar(dos, c, lvo::READ, [fh, b.dir, 4]) == 4 {
                        ejecutable = c.leer32(b.dir) == Some(HUNK_HEADER);
                    }
                    b.soltar(c);
                }
                llamar(dos, c, lvo::CLOSE, [fh, 0, 0]);
            }
            Ok(if ejecutable { Clase::Programa } else { Clase::Proyecto })
        }
    };
    tn.soltar(c);
    r
}

/// Pone `IoErr()` de la tarea en marcha (`SetIoErr`).
pub fn poner_io_err<F: SistemaArchivos, C: Caja>(dos: &mut Dos<F>, c: &mut C, codigo: u32) {
    llamar(dos, c, lvo::SET_IO_ERR, [codigo, 0, 0]);
}
