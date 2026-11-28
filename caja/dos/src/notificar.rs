//! StartNotify y EndNotify: avisar a un programa cuando cambia un fichero o lo de dentro de un cajón (dos/notify.h). Lo
//! comprobado contra el oráculo (`pruebas68k/notify.c`, en RAM: con el Kickstart 40.68):
//!
//! - Avisa al cerrar un fichero en el que se escribió (o que se creó o vació), al borrarlo y al crearlo: a quien vigila
//!   ese objeto y a quien vigila su cajón. Abrir y cerrar sin escribir no avisa, ni SetProtection.
//! - Con `NRF_SEND_SIGNAL`, la señal a la tarea; con `NRF_SEND_MESSAGE`, un `NotifyMessage` (clase `NOTIFY_CLASS`,
//!   código `NOTIFY_CODE`, `nm_NReq` la petición) por cada cambio.
//! - `NRF_WAIT_REPLY`: mientras el último mensaje no se conteste, los cambios se juntan en uno, que sale al contestarlo.
//! - `NRF_NOTIFY_INITIAL`: un aviso al empezar, si el objeto ya existe.
//! - Se puede vigilar lo que aún no existe. `nr_FullName`, el camino entero (memoria de dos, hasta `EndNotify`).
//!
//! Solo se ven los cambios que pasan por la dos de la caja (no los que haga otro programa de Linux en ese cajón).

use alloc::vec::Vec;

use crate::fs::SistemaArchivos;
use crate::*;

/// `struct NotifyRequest`.
mod nr {
    pub const NAME: u32 = 0;
    pub const FULLNAME: u32 = 4;
    pub const FLAGS: u32 = 12;
    pub const PORT: u32 = 16;
    pub const TASK: u32 = 16;
    pub const SIGNALNUM: u32 = 20;
    pub const TAMANO: u32 = 48;
}

const NRF_SEND_MESSAGE: u32 = 1;
const NRF_SEND_SIGNAL: u32 = 2;
const NRF_WAIT_REPLY: u32 = 8;
const NRF_NOTIFY_INITIAL: u32 = 16;
const NOTIFY_CLASS: u32 = 0x4000_0000;
const NOTIFY_CODE: u16 = 0x1234;
/// `struct NotifyMessage`: un `Message` (20 bytes), `nm_Class`, `nm_Code`, `nm_NReq` y dos palabras de dos.
const NM_CLASS: u32 = 20;
const NM_CODE: u32 = 24;
const NM_NREQ: u32 = 26;
const NM_TAMANO: u32 = 38;
const MAX_PETICIONES: usize = 512;
const MAX_MENSAJES: usize = 1024;

#[derive(Clone, Debug)]
pub(crate) struct Peticion {
    nr: u32,
    clave: (usize, Vec<alloc::string::String>),
    banderas: u32,
    destino: u32,
    senal: u8,
    dueno: u32,
    nombre: u32,
    largo_nombre: u32,
    /// Con `NRF_WAIT_REPLY`: un mensaje sin contestar, y si hubo cambios mientras tanto.
    fuera: bool,
    pendiente: bool,
}

impl<F: SistemaArchivos> Dos<F> {
    /// StartNotify(notify).
    pub(crate) fn start_notify<C: Caja>(&mut self, c: &mut C, p: u32) -> R<u32> {
        Self::tramo(c, p, nr::TAMANO)?;
        if self.peticiones.len() >= MAX_PETICIONES {
            return Err(E::Io(ERROR_NO_FREE_STORE));
        }
        let ruta = Self::cadena(c, Self::l32(c, p + nr::NAME)?)?;
        let sitio = self.sitios(c, &ruta)?.into_iter().next().ok_or(E::Io(ERROR_OBJECT_NOT_FOUND))?;
        let banderas = Self::l32(c, p + nr::FLAGS)?;
        let destino = Self::l32(c, p + if banderas & NRF_SEND_SIGNAL != 0 { nr::TASK } else { nr::PORT })?;
        let senal = Self::l8(c, p + nr::SIGNALNUM)?;
        // nr_FullName: el camino entero, en memoria de dos.
        let mut t = self.nombre_de_sitio(&sitio)?;
        t.push(0);
        let largo = t.len() as u32;
        let nombre = c.reservar_sistema(largo);
        if nombre != 0 {
            Self::poner(c, nombre, &t)?;
        }
        Self::e32(c, p + nr::FULLNAME, nombre)?;
        let existe = self.mirar_sitio(&sitio).is_ok();
        self.peticiones.push(Peticion {
            nr: p,
            clave: sitio.clave(),
            banderas,
            destino,
            senal,
            dueno: c.tarea_actual(),
            nombre,
            largo_nombre: largo,
            fuera: false,
            pendiente: false,
        });
        if banderas & NRF_NOTIFY_INITIAL != 0 && existe {
            let k = self.peticiones.len() - 1;
            self.entregar(c, k);
        }
        Ok(DOSTRUE)
    }

    /// EndNotify(notify).
    pub(crate) fn end_notify<C: Caja>(&mut self, c: &mut C, p: u32) {
        if self.soltar_peticion(c, p) {
            let _ = Self::e32(c, p + nr::FULLNAME, 0);
        }
    }

    /// Quita la petición `p` y suelta su `nr_FullName`. Si la había.
    fn soltar_peticion<C: Caja>(&mut self, c: &mut C, p: u32) -> bool {
        let Some(k) = self.peticiones.iter().position(|x| x.nr == p) else { return false };
        let x = self.peticiones.remove(k);
        if x.nombre != 0 {
            c.liberar(x.nombre, x.largo_nombre);
        }
        true
    }

    /// Ha cambiado lo de `sitio`: aviso a quien lo vigila y a quien vigila su cajón.
    pub(crate) fn notificar<C: Caja>(&mut self, c: &mut C, sitio: &Sitio) {
        if self.peticiones.is_empty() {
            return;
        }
        let (vol, camino) = sitio.clave();
        let a_quien: Vec<usize> = (0..self.peticiones.len())
            .filter(|&k| {
                let (pv, pc) = &self.peticiones[k].clave;
                *pv == vol && (*pc == camino || (!camino.is_empty() && *pc == camino[..camino.len() - 1]))
            })
            .collect();
        for k in a_quien {
            self.entregar(c, k);
        }
    }

    fn entregar<C: Caja>(&mut self, c: &mut C, k: usize) {
        let x = &mut self.peticiones[k];
        if x.banderas & NRF_SEND_SIGNAL != 0 {
            if x.destino != 0 && x.senal < 32 {
                c.senalar(x.destino, 1 << x.senal);
            }
            return;
        }
        if x.banderas & NRF_SEND_MESSAGE == 0 || x.destino == 0 {
            return;
        }
        if x.banderas & NRF_WAIT_REPLY != 0 && x.fuera {
            x.pendiente = true;
            return;
        }
        if self.mensajes_aviso.len() >= MAX_MENSAJES {
            return;
        }
        let (puerto, peticion, espera) = (x.destino, x.nr, x.banderas & NRF_WAIT_REPLY != 0);
        let m = c.reservar_sistema(NM_TAMANO);
        if m == 0 {
            return;
        }
        let bien = (|| -> R<()> {
            Self::e8(c, m + 8, 5)?; // ln_Type = NT_MESSAGE
            Self::poner(c, m + 18, &(NM_TAMANO as u16).to_be_bytes())?;
            Self::e32(c, m + NM_CLASS, NOTIFY_CLASS)?;
            Self::poner(c, m + NM_CODE, &NOTIFY_CODE.to_be_bytes())?;
            Self::e32(c, m + NM_NREQ, peticion)
        })();
        if bien.is_err() {
            c.liberar(m, NM_TAMANO);
            return;
        }
        self.mensajes_aviso.push((m, peticion));
        if espera {
            self.peticiones[k].fuera = true;
        }
        c.mandar_mensaje(puerto, m);
    }

    /// Si `m` es un aviso de StartNotify mandado por dos y aún sin contestar.
    pub fn es_aviso(&self, m: u32) -> bool {
        self.mensajes_aviso.iter().any(|&(x, _)| x == m)
    }

    /// La caja: el programa contestó (ReplyMsg) el mensaje `m`. Si era un aviso de dos, se suelta, y si su petición
    /// esperaba la respuesta con cambios pendientes, sale el siguiente.
    pub fn aviso_contestado<C: Caja>(&mut self, c: &mut C, m: u32) {
        let Some(i) = self.mensajes_aviso.iter().position(|&(x, _)| x == m) else { return };
        let (_, peticion) = self.mensajes_aviso.remove(i);
        c.liberar(m, NM_TAMANO);
        if let Some(k) = self.peticiones.iter().position(|x| x.nr == peticion) {
            self.peticiones[k].fuera = false;
            if self.peticiones[k].pendiente {
                self.peticiones[k].pendiente = false;
                self.entregar(c, k);
            }
        }
    }

    /// Una tarea acabó: sus peticiones se quitan (sin tocar su `NotifyRequest`, que puede no existir ya).
    pub(crate) fn quitar_peticiones<C: Caja>(&mut self, c: &mut C, tarea: u32) {
        let suyas: Vec<u32> = self.peticiones.iter().filter(|x| x.dueno == tarea).map(|x| x.nr).collect();
        for p in suyas {
            self.soltar_peticion(c, p);
        }
    }
}
