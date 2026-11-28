//! console.device e input.device nativos, lo que piden los programas de verdad sin abrir una consola ni tocar la
//! cadena de entrada (rama amiga-apps): LightWave y Modeler abren `console.device` con la unidad `CONU_LIBRARY` (-1)
//! solo para traducir teclas con `RawKeyConvert`; Directory Opus y LightWave abren `input.device` para saber los
//! calificadores (`PeekQualifier`) y poner un manejador de entrada.
//!
//! Escrito de los autodocs de console.device e input.device (Includes and Autodocs 3.1) y `devices/console.h`,
//! `devices/input.h`. Sin código de nadie.
//!
//! - console.device: `Open` con la unidad -1 da la base sin ventana (la de las funciones); cualquier otra unidad (una
//!   consola en una ventana, `CON:`) no está todavía: `IOERR_OPENFAIL`. `CDInputHandler` devuelve los eventos tal
//!   cual (no hay consolas que los quieran). `RawKeyConvert(events, buffer, length, keyMap)` (A0/A1/D1/A2) es lo mismo
//!   que `MapRawKey` de keymap.library, con los mismos registros: la caja la pasa a la de keymap nativa.
//! - input.device: `Open` de la unidad 0. `IND_ADDHANDLER` y `IND_REMHANDLER` se apuntan (la caja todavía no pasa sus
//!   eventos por una cadena de manejadores de 68k: los recibe intuition directamente) y vuelven bien; `IND_WRITEEVENT`,
//!   `IND_SETTHRESH`, `IND_SETPERIOD`, `IND_SETMPORT`, `IND_SETMTYPE` y `IND_SETMTRIG` vuelven bien sin hacer nada.
//!   `PeekQualifier` da los calificadores que dice intuition (por ahora, ninguno pulsado: 0).

use alloc::vec::Vec;
use caja_exec::io;
use caja_exec::{Accion, Exec, Fallo, Memoria, Registros};

pub const CONSOLA: [&str; 7] =
    ["BeginIO", "AbortIO", "CDInputHandler", "RawKeyConvert", "GetConSnip", "SetConSnip", "AddConSnipHook"];
pub const ENTRADA: [&str; 3] = ["BeginIO", "AbortIO", "PeekQualifier"];

/// Las órdenes de input.device (`devices/input.h`).
const IND_ADDHANDLER: u16 = 9;
const IND_SETMTRIG: u16 = 16;

pub struct Dispositivos {
    pub consola: u32,
    pub entrada: u32,
    n_consola: u16,
    n_entrada: u16,
    /// Los manejadores de entrada apuntados (la `struct Interrupt` y la tarea).
    pub manejadores: Vec<(u32, u32)>,
}

impl Dispositivos {
    pub fn instalar<M: Memoria>(ex: &mut Exec<M>, n_consola: u16, n_entrada: u16) -> Result<Dispositivos, Fallo> {
        let nc: Vec<u16> = (1..=CONSOLA.len() as u16).map(|i| n_consola + i).collect();
        let consola = ex.crear_dispositivo_nativo("console.device", 40, 1, n_consola, &nc, 0)?;
        let ne: Vec<u16> = (1..=ENTRADA.len() as u16).map(|i| n_entrada + i).collect();
        let entrada = ex.crear_dispositivo_nativo("input.device", 40, 1, n_entrada, &ne, 0)?;
        Ok(Dispositivos { consola, entrada, n_consola, n_entrada, manejadores: Vec::new() })
    }

    pub fn es_mio(&self, n: u16) -> bool {
        (self.n_consola..=self.n_consola + CONSOLA.len() as u16).contains(&n)
            || (self.n_entrada..=self.n_entrada + ENTRADA.len() as u16).contains(&n)
    }

    /// El número de `RawKeyConvert` (la caja la pasa a `MapRawKey`).
    pub fn es_raw_key_convert(&self, n: u16) -> bool {
        n == self.n_consola + 4
    }

    pub fn tarea_muerta(&mut self, t: u32) {
        self.manejadores.retain(|&(_, d)| d != t);
    }

    /// La función nativa `n`: lo que no es de aquí, `Accion::Ajena`; un error de memoria, `Err` (la caja lo avisa).
    pub fn llamar<M: Memoria>(&mut self, ex: &mut Exec<M>, n: u16, r: &mut Registros) -> Result<Accion, Fallo> {
        if (self.n_consola..=self.n_consola + CONSOLA.len() as u16).contains(&n) {
            return self.consola(ex, n - self.n_consola, r);
        }
        if (self.n_entrada..=self.n_entrada + ENTRADA.len() as u16).contains(&n) {
            return self.entrada(ex, n - self.n_entrada, r);
        }
        Ok(Accion::Ajena)
    }

    fn consola<M: Memoria>(&mut self, ex: &mut Exec<M>, i: u16, r: &mut Registros) -> Result<Accion, Fallo> {
        match i {
            // Open(ioRequest, unit, flags)(a1/d0/d1)
            0 => {
                let ior = r.a[1];
                comprobar(ior, io::TAMANO)?;
                let e = if r.d[0] == u32::MAX { 0 } else { io::IOERR_OPENFAIL };
                ex.mem.escribir32(ior + io::IO_UNIT, 0)?;
                ex.mem.escribir8(ior + io::IO_ERROR, e as u8)?;
                if e != 0 {
                    return Err(Fallo::NoImplementada { n: 0 });
                }
                Ok(Accion::Seguir)
            }
            1 => acabar(ex, r.a[1], io::IOERR_NOCMD),
            2 => {
                r.d[0] = 0;
                Ok(Accion::Seguir)
            }
            // CDInputHandler(events, consoleDevice)(a0/a1): los eventos, tal cual.
            3 => {
                r.d[0] = r.a[0];
                Ok(Accion::Seguir)
            }
            // RawKeyConvert: la pasa la caja a MapRawKey antes de llegar aquí.
            4 => Ok(Accion::Ajena),
            // GetConSnip: no hay texto marcado en ninguna consola (0); SetConSnip y AddConSnipHook, nada.
            _ => {
                r.d[0] = 0;
                Ok(Accion::Seguir)
            }
        }
    }

    fn entrada<M: Memoria>(&mut self, ex: &mut Exec<M>, i: u16, r: &mut Registros) -> Result<Accion, Fallo> {
        match i {
            0 => {
                let ior = r.a[1];
                comprobar(ior, io::TAMANO)?;
                let e = if r.d[0] == 0 { 0 } else { io::IOERR_OPENFAIL };
                ex.mem.escribir32(ior + io::IO_UNIT, 0)?;
                ex.mem.escribir8(ior + io::IO_ERROR, e as u8)?;
                Ok(Accion::Seguir)
            }
            // BeginIO
            1 => {
                let ior = r.a[1];
                comprobar(ior, io::STD_TAMANO)?;
                let orden = ex.mem.leer16(ior + io::IO_COMMAND)?;
                match orden {
                    IND_ADDHANDLER => {
                        let h = ex.mem.leer32(ior + io::IO_DATA)?;
                        if h != 0 && self.manejadores.len() < 64 {
                            self.manejadores.push((h, ex.tarea_actual()));
                        }
                        acabar(ex, ior, 0)
                    }
                    10 => {
                        let h = ex.mem.leer32(ior + io::IO_DATA)?;
                        self.manejadores.retain(|&(x, _)| x != h);
                        acabar(ex, ior, 0)
                    }
                    11..=IND_SETMTRIG | io::CMD_RESET | io::CMD_FLUSH | io::CMD_START | io::CMD_STOP => acabar(ex, ior, 0),
                    _ => acabar(ex, ior, io::IOERR_NOCMD),
                }
            }
            2 => {
                r.d[0] = 0;
                Ok(Accion::Seguir)
            }
            // PeekQualifier(): ningún calificador pulsado.
            _ => {
                r.d[0] = 0;
                Ok(Accion::Seguir)
            }
        }
    }
}

fn comprobar(dir: u32, largo: u32) -> Result<(), Fallo> {
    if dir < caja_exec::PAGINA_CERO || dir.checked_add(largo).is_none_or(|f| f > caja_exec::LIMITE) {
        return Err(Fallo::FueraDeMemoria { dir, largo });
    }
    Ok(())
}

/// Acaba una petición: con `IOF_QUICK` (de `DoIO`) se queda así; si no, vuelve a su puerto.
fn acabar<M: Memoria>(ex: &mut Exec<M>, ior: u32, error: i8) -> Result<Accion, Fallo> {
    comprobar(ior, io::TAMANO)?;
    ex.mem.escribir8(ior + io::IO_ERROR, error as u8)?;
    if ex.mem.leer8(ior + io::IO_FLAGS)? & io::IOF_QUICK != 0 {
        return Ok(Accion::Seguir);
    }
    ex.reply_msg(ior)
}
