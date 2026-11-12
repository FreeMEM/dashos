//! timer.device nativo de la caja Amiga (S4), versión 40.
//!
//! Escrito desde los autodocs de timer.device y `devices/timer.h`, `timer_lib.fd` del NDK 3.1. No se ha leído código
//! de AmigaOS ni de AROS. Las cinco unidades (`UNIT_MICROHZ`, `UNIT_VBLANK`, `UNIT_ECLOCK`, `UNIT_WAITUNTIL` y
//! `UNIT_WAITECLOCK`) miden con el mismo reloj, el de DashOS (en microsegundos): en la caja no hay CIA ni haz de
//! vídeo, y lo que cambia entre ellas en un Amiga es la precisión.
//!
//! - `TR_ADDREQUEST`: la petición vuelve (`ReplyMsg`) cuando pasa su tiempo. La caja llama a `vencidos` mientras la
//!   tarea espera y en cada llamada nativa; las pendientes van en una tabla nativa, por orden de vencimiento.
//! - `TR_GETSYSTIME` y `TR_SETSYSTIME`: la hora del sistema (segundos desde 1978), la de la caja más lo que se ponga.
//! - Las funciones de la biblioteca: `AddTime`, `SubTime`, `CmpTime`, `ReadEClock` (a 709.379 Hz, la de un Amiga
//!   PAL) y `GetSysTime`.
//!
//! Lo que manda el 68k se comprueba (el Enforcer de S3): una petición fuera de la memoria, una orden que no existe o
//! una que ya está esperando dan un aviso o su error de dos (`IOERR_NOCMD`), nunca un pánico. Si su tarea muere, sus
//! peticiones se olvidan.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;

use caja_exec::estructuras::{message, node, nt};
use caja_exec::io;
use caja_exec::{Accion, Aviso, Exec, Fallo, LIMITE, Memoria, PAGINA_CERO, Registros};

/// Las funciones propias, en el orden de la tabla de saltos desde -30: `BeginIO`, `AbortIO` y las de `timer_lib.fd`.
pub const FUNCIONES: [&str; 7] = ["BeginIO", "AbortIO", "AddTime", "SubTime", "CmpTime", "ReadEClock", "GetSysTime"];

/// Unidades y órdenes (`devices/timer.h`).
pub const UNIT_MICROHZ: u32 = 0;
pub const UNIT_VBLANK: u32 = 1;
pub const UNIT_ECLOCK: u32 = 2;
pub const UNIT_WAITUNTIL: u32 = 3;
pub const UNIT_WAITECLOCK: u32 = 4;
pub const TR_ADDREQUEST: u16 = io::CMD_NONSTD;
pub const TR_GETSYSTIME: u16 = io::CMD_NONSTD + 1;
pub const TR_SETSYSTIME: u16 = io::CMD_NONSTD + 2;

/// `struct timerequest`: la `IORequest` y un `timeval` (segundos y microsegundos), o un `EClockVal` (alto y bajo).
pub const TR_SECS: u32 = io::TAMANO;
pub const TR_MICRO: u32 = io::TAMANO + 4;
pub const TR_TAMANO: u32 = io::TAMANO + 8;

/// La frecuencia del EClock: la de un Amiga PAL (la de `ExecBase->ex_EClockFrequency` de la caja).
pub const ECLOCK: u64 = 709_379;
const MILLON: u64 = 1_000_000;
const MAX_AVISOS: usize = 1024;
/// Lo más que se espera: unos 136 años (lo que cabe en los segundos de un `timeval`).
const MAX_US: u64 = u32::MAX as u64 * MILLON;

/// El reloj que da la caja: microsegundos desde que arrancó (para las esperas y el EClock) y la hora del sistema,
/// en microsegundos desde el 1 de enero de 1978.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reloj {
    pub monotono_us: u64,
    pub sistema_us: u64,
}

#[derive(Clone, Copy, Debug)]
struct Pendiente {
    io: u32,
    vence_us: u64,
    tarea: u32,
}

/// timer.device de una caja.
pub struct Temporizador {
    base: u32,
    /// Número nativo de su `Open`; `BeginIO` es el siguiente, y así.
    primero: u16,
    pendientes: Vec<Pendiente>,
    /// Lo que se suma a la hora del sistema de la caja (`TR_SETSYSTIME`), en microsegundos.
    ajuste_us: i64,
    avisos: Vec<Aviso>,
}

fn comprobar(dir: u32, largo: u32, escribir: bool) -> Result<(), Fallo> {
    if dir >= LIMITE || largo > LIMITE - dir {
        return Err(Fallo::FueraDeMemoria { dir, largo });
    }
    if dir < PAGINA_CERO && !(dir == 4 && largo == 4 && !escribir) {
        return Err(Fallo::PaginaCero { dir });
    }
    Ok(())
}

fn l8<M: Memoria>(ex: &Exec<M>, d: u32) -> Result<u8, Fallo> {
    comprobar(d, 1, false)?;
    ex.mem.leer8(d)
}
fn l16<M: Memoria>(ex: &Exec<M>, d: u32) -> Result<u16, Fallo> {
    comprobar(d, 2, false)?;
    ex.mem.leer16(d)
}
fn l32<M: Memoria>(ex: &Exec<M>, d: u32) -> Result<u32, Fallo> {
    comprobar(d, 4, false)?;
    ex.mem.leer32(d)
}
fn e8<M: Memoria>(ex: &mut Exec<M>, d: u32, v: u8) -> Result<(), Fallo> {
    comprobar(d, 1, true)?;
    ex.mem.escribir8(d, v)
}
fn e32<M: Memoria>(ex: &mut Exec<M>, d: u32, v: u32) -> Result<(), Fallo> {
    comprobar(d, 4, true)?;
    ex.mem.escribir32(d, v)
}

/// Lee un `timeval` como microsegundos.
fn leer_tiempo<M: Memoria>(ex: &Exec<M>, d: u32) -> Result<u64, Fallo> {
    if d == 0 {
        return Err(Fallo::PunteroNulo);
    }
    Ok(l32(ex, d)? as u64 * MILLON + l32(ex, d + 4)? as u64)
}

/// Escribe microsegundos como `timeval` (los segundos, recortados a 32 bits).
fn poner_tiempo<M: Memoria>(ex: &mut Exec<M>, d: u32, us: u64) -> Result<(), Fallo> {
    if d == 0 {
        return Err(Fallo::PunteroNulo);
    }
    comprobar(d, 8, true)?;
    e32(ex, d, (us / MILLON).min(u32::MAX as u64) as u32)?;
    e32(ex, d + 4, (us % MILLON) as u32)
}

/// Microsegundos → ciclos del EClock, y al revés.
pub fn a_eclock(us: u64) -> u64 {
    (us as u128 * ECLOCK as u128 / MILLON as u128) as u64
}
pub fn de_eclock(ciclos: u64) -> u64 {
    (ciclos as u128 * MILLON as u128 / ECLOCK as u128).min(u64::MAX as u128) as u64
}

impl Temporizador {
    /// Crea timer.device en la caja, con sus números nativos desde `primero` (`Open`, y después las de `FUNCIONES`).
    pub fn instalar<M: Memoria>(ex: &mut Exec<M>, primero: u16) -> Result<Temporizador, Fallo> {
        let numeros: Vec<u16> = (1..=FUNCIONES.len() as u16).map(|i| primero + i).collect();
        let base = ex.crear_dispositivo_nativo("timer.device", 40, 1, primero, &numeros, 0)?;
        Ok(Temporizador { base, primero, pendientes: Vec::new(), ajuste_us: 0, avisos: Vec::new() })
    }

    pub fn base(&self) -> u32 {
        self.base
    }

    pub fn es_mio(&self, n: u16) -> bool {
        n >= self.primero && n <= self.primero + FUNCIONES.len() as u16
    }

    pub fn avisos(&self) -> &[Aviso] {
        &self.avisos
    }

    pub fn tomar_avisos(&mut self) -> Vec<Aviso> {
        core::mem::take(&mut self.avisos)
    }

    fn avisar<M: Memoria>(&mut self, ex: &Exec<M>, funcion: &'static str, fallo: Fallo) {
        if self.avisos.len() >= MAX_AVISOS {
            self.avisos.remove(0);
        }
        self.avisos.push(Aviso { tarea: ex.tarea_actual(), funcion, fallo });
    }

    /// Cuántas peticiones esperan.
    pub fn pendientes(&self) -> usize {
        self.pendientes.len()
    }

    /// Cuándo vence la primera que espera (en el reloj monótono), si hay alguna.
    pub fn proximo(&self) -> Option<u64> {
        self.pendientes.iter().map(|p| p.vence_us).min()
    }

    fn hora(&self, reloj: Reloj) -> u64 {
        (reloj.sistema_us as i64).saturating_add(self.ajuste_us).max(0) as u64
    }

    /// La función nativa `n` (con `A6` = la base del dispositivo). `Accion::Ajena` si no es suya.
    pub fn llamar<M: Memoria>(&mut self, ex: &mut Exec<M>, n: u16, r: &mut Registros, reloj: Reloj) -> Accion {
        if !self.es_mio(n) {
            return Accion::Ajena;
        }
        let nombre = if n == self.primero { "Open" } else { FUNCIONES[(n - self.primero - 1) as usize] };
        match self.ejecutar(ex, nombre, r, reloj) {
            Ok(a) => a,
            Err(f) => {
                self.avisar(ex, nombre, f);
                r.d[0] = 0;
                Accion::Seguir
            }
        }
    }

    fn ejecutar<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        nombre: &'static str,
        r: &mut Registros,
        reloj: Reloj,
    ) -> Result<Accion, Fallo> {
        let (d, a) = (r.d, r.a);
        match nombre {
            // Open(ioRequest, unit, flags)(a1/d0/d1): la unidad tiene que existir.
            "Open" => {
                let ior = a[1];
                comprobar(ior, io::TAMANO, true)?;
                if d[0] > UNIT_WAITECLOCK {
                    e8(ex, ior + io::IO_ERROR, io::IOERR_OPENFAIL as u8)?;
                } else {
                    e32(ex, ior + io::IO_UNIT, d[0])?;
                    e8(ex, ior + io::IO_ERROR, 0)?;
                }
                Ok(Accion::Seguir)
            }
            "BeginIO" => self.begin_io(ex, a[1], reloj),
            "AbortIO" => {
                r.d[0] = self.abort_io(ex, a[1])?;
                Ok(Accion::Seguir)
            }
            // AddTime y SubTime(dest, src)(a0/a1): dest = dest ± src, con los microsegundos en 0..999.999.
            "AddTime" | "SubTime" => {
                let (x, y) = (leer_tiempo(ex, a[0])?, leer_tiempo(ex, a[1])?);
                let v = if nombre == "AddTime" { x.saturating_add(y) } else { x.saturating_sub(y) };
                poner_tiempo(ex, a[0], v)?;
                Ok(Accion::Seguir)
            }
            // CmpTime(dest, src)(a0/a1): 0 iguales, -1 si dest es más, 1 si dest es menos.
            "CmpTime" => {
                let (x, y) = (leer_tiempo(ex, a[0])?, leer_tiempo(ex, a[1])?);
                r.d[0] = match x.cmp(&y) {
                    core::cmp::Ordering::Equal => 0,
                    core::cmp::Ordering::Greater => u32::MAX,
                    core::cmp::Ordering::Less => 1,
                };
                Ok(Accion::Seguir)
            }
            // ReadEClock(dest)(a0): el EClock en dest (alto y bajo); D0, su frecuencia.
            "ReadEClock" => {
                if a[0] == 0 {
                    return Err(Fallo::PunteroNulo);
                }
                comprobar(a[0], 8, true)?;
                let c = a_eclock(reloj.monotono_us);
                e32(ex, a[0], (c >> 32) as u32)?;
                e32(ex, a[0] + 4, c as u32)?;
                r.d[0] = ECLOCK as u32;
                Ok(Accion::Seguir)
            }
            "GetSysTime" => {
                poner_tiempo(ex, a[0], self.hora(reloj))?;
                Ok(Accion::Seguir)
            }
            _ => Err(Fallo::NoImplementada { n: 0 }),
        }
    }

    /// Acaba una petición: con `IOF_QUICK` (de `DoIO`) se queda así; si no, vuelve a su puerto.
    fn acabar<M: Memoria>(&mut self, ex: &mut Exec<M>, ior: u32, error: i8) -> Result<Accion, Fallo> {
        e8(ex, ior + io::IO_ERROR, error as u8)?;
        if l8(ex, ior + io::IO_FLAGS)? & io::IOF_QUICK != 0 {
            return Ok(Accion::Seguir);
        }
        ex.reply_msg(ior)
    }

    fn begin_io<M: Memoria>(&mut self, ex: &mut Exec<M>, ior: u32, reloj: Reloj) -> Result<Accion, Fallo> {
        if ior == 0 {
            return Err(Fallo::PunteroNulo);
        }
        comprobar(ior, TR_TAMANO, true)?;
        ex.mem.leer(ior, TR_TAMANO)?;
        if self.pendientes.iter().any(|p| p.io == ior) {
            // Mandarla otra vez mientras espera rompería la cola del puerto: se queda la de antes.
            return Err(Fallo::MensajeEnCola { dir: ior });
        }
        let orden = l16(ex, ior + io::IO_COMMAND)?;
        let unidad = l32(ex, ior + io::IO_UNIT)?;
        match orden {
            TR_ADDREQUEST => {
                let (s, us) = (l32(ex, ior + TR_SECS)? as u64, l32(ex, ior + TR_MICRO)? as u64);
                let ahora = reloj.monotono_us;
                let espera = match unidad {
                    UNIT_MICROHZ | UNIT_VBLANK => s * MILLON + us,
                    UNIT_ECLOCK => de_eclock(s << 32 | us),
                    UNIT_WAITUNTIL => (s * MILLON + us).saturating_sub(self.hora(reloj)),
                    UNIT_WAITECLOCK => de_eclock(s << 32 | us).saturating_sub(ahora),
                    _ => return self.acabar(ex, ior, io::IOERR_NOCMD),
                };
                let f = l8(ex, ior + io::IO_FLAGS)?;
                e8(ex, ior + io::IO_FLAGS, f & !io::IOF_QUICK)?;
                e8(ex, ior + io::IO_ERROR, 0)?;
                let vence_us = ahora.saturating_add(espera.min(MAX_US));
                self.pendientes.push(Pendiente { io: ior, vence_us, tarea: ex.tarea_actual() });
                if espera == 0 {
                    return self.vencidos(ex, reloj);
                }
                Ok(Accion::Seguir)
            }
            TR_GETSYSTIME => {
                poner_tiempo(ex, ior + TR_SECS, self.hora(reloj))?;
                self.acabar(ex, ior, 0)
            }
            TR_SETSYSTIME => {
                let nueva = leer_tiempo(ex, ior + TR_SECS)?;
                self.ajuste_us = nueva as i64 - reloj.sistema_us as i64;
                self.acabar(ex, ior, 0)
            }
            _ => self.acabar(ex, ior, io::IOERR_NOCMD),
        }
    }

    /// `AbortIO`: si la petición espera, vuelve ya con `IOERR_ABORTED`. D0: 0 si se abortó, -1 si no esperaba.
    fn abort_io<M: Memoria>(&mut self, ex: &mut Exec<M>, ior: u32) -> Result<u32, Fallo> {
        let Some(i) = self.pendientes.iter().position(|p| p.io == ior) else { return Ok(u32::MAX) };
        self.pendientes.remove(i);
        self.acabar(ex, ior, io::IOERR_ABORTED)?;
        Ok(0)
    }

    /// Devuelve a su puerto las peticiones que ya vencieron, por orden. `Accion::Ceder` si alguna despertó a una
    /// tarea de más prioridad que la que corre.
    pub fn vencidos<M: Memoria>(&mut self, ex: &mut Exec<M>, reloj: Reloj) -> Result<Accion, Fallo> {
        let mut accion = Accion::Seguir;
        loop {
            let Some(i) = self
                .pendientes
                .iter()
                .enumerate()
                .filter(|(_, p)| p.vence_us <= reloj.monotono_us)
                .min_by_key(|(_, p)| p.vence_us)
                .map(|(i, _)| i)
            else {
                return Ok(accion);
            };
            let p = self.pendientes.remove(i);
            // La memoria de la petición puede haberse ido (el programa la soltó sin AbortIO): se olvida.
            match self.acabar(ex, p.io, 0) {
                Ok(Accion::Ceder) => accion = Accion::Ceder,
                Ok(_) => {}
                Err(f) => self.avisar(ex, "(timer.device: vence)", f),
            }
        }
    }

    /// Olvida las peticiones de una tarea que muere.
    pub fn tarea_muerta(&mut self, t: u32) {
        self.pendientes.retain(|p| p.tarea != t);
    }

    /// Las direcciones de lo que espera, para las pruebas.
    pub fn peticiones(&self) -> Vec<u32> {
        self.pendientes.iter().map(|p| p.io).collect()
    }

    /// Si `io` es una petición del 68k que se puede leer como `timerequest`. Para las pruebas.
    pub fn es_peticion<M: Memoria>(ex: &Exec<M>, ior: u32) -> bool {
        comprobar(ior, TR_TAMANO, false).is_ok()
            && ex.mem.leer(ior, TR_TAMANO).is_ok()
            && l8(ex, ior + node::LN_TYPE).is_ok_and(|t| t == nt::REPLYMSG || t == nt::MESSAGE)
            && l32(ex, ior + message::MN_REPLYPORT).is_ok()
    }
}
