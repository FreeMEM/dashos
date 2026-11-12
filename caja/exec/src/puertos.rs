//! Puertos y mensajes: `CreateMsgPort`, `DeleteMsgPort`, `AddPort`, `RemPort`, `FindPort`, `PutMsg`, `GetMsg`,
//! `ReplyMsg` y `WaitPort` (RKRM: Libraries, "Exec Messages and Ports").
//!
//! Un puerto es una lista de mensajes y lo que pasa al llegar uno (`mp_Flags`): con `PA_SIGNAL`, la señal
//! `mp_SigBit` a la tarea `mp_SigTask`. Los mensajes no se copian: va el puntero.
//!
//! Lo que se comprueba aquí y exec no: que el puerto y el mensaje estén en memoria viva (reservada o del sistema;
//! un puerto borrado está en memoria libre) y que el mensaje no esté ya en la cola de algún puerto.

use crate::estructuras::{list, memf, message, msgport, node, nt, task};
use crate::tareas::Espera;
use crate::{Accion, Exec, Fallo, Memoria, execbase};

impl<M: Memoria> Exec<M> {
    /// `CreateMsgPort`: un puerto con una señal nueva de la tarea actual. 0 si no hay memoria o señal.
    pub fn create_msg_port(&mut self) -> Result<u32, Fallo> {
        let t = self.tarea_actual();
        if t == 0 || !self.tareas.contains_key(&t) {
            return Err(Fallo::SinTarea);
        }
        let s = self.alloc_signal(-1)?;
        if s < 0 {
            return Ok(0);
        }
        let p = self.alloc_mem(msgport::TAMANO, memf::PUBLIC | memf::CLEAR)?;
        if p == 0 {
            self.free_signal(s as i32)?;
            return Ok(0);
        }
        self.e8(p + node::LN_TYPE, nt::MSGPORT)?;
        self.e8(p + msgport::MP_FLAGS, msgport::PA_SIGNAL)?;
        self.e8(p + msgport::MP_SIGBIT, s as u8)?;
        self.e32(p + msgport::MP_SIGTASK, t)?;
        self.new_list(p + msgport::MP_MSGLIST, nt::MESSAGE)?;
        if let Some(i) = self.tareas.get_mut(&t) {
            i.puertos.push(p);
        }
        Ok(p)
    }

    /// `DeleteMsgPort`: suelta su señal y su memoria. Con 0 no hace nada. Si aún está en la lista pública o le
    /// quedan mensajes, avisa (y lo saca de la lista, para no dejarla rota).
    pub fn delete_msg_port(&mut self, p: u32) -> Result<(), Fallo> {
        if p == 0 {
            return Ok(());
        }
        match self.bloques.get(&p) {
            Some(b) if !b.vec => {}
            _ => return Err(Fallo::NoReservado { dir: p }),
        }
        if self.tipo(p)? != nt::MSGPORT {
            return Err(Fallo::Sospechoso { dir: p });
        }
        if self.nodos(self.base + execbase::PORTLIST)?.contains(&p) {
            self.avisar("DeleteMsgPort", Fallo::Sospechoso { dir: p });
            self.remove(p)?;
        }
        if !self.lista_vacia(p + msgport::MP_MSGLIST)? {
            self.avisar("DeleteMsgPort", Fallo::Sospechoso { dir: p });
        }
        let dueno = self.l32(p + msgport::MP_SIGTASK)?;
        let s = self.l8(p + msgport::MP_SIGBIT)? as i32;
        if self.tareas.contains_key(&dueno) {
            if let Err(f) = self.soltar_senal(dueno, s) {
                self.avisar("DeleteMsgPort", f);
            }
            if let Some(i) = self.tareas.get_mut(&dueno) {
                i.puertos.retain(|&x| x != p);
            }
        }
        self.free_mem(p, msgport::TAMANO)
    }

    /// `AddPort`: a la lista pública, por prioridad. Deja su cola de mensajes vacía.
    pub fn add_port(&mut self, p: u32) -> Result<(), Fallo> {
        if p == 0 {
            return Err(Fallo::PunteroNulo);
        }
        self.tramo(p, msgport::TAMANO)?;
        self.e8(p + node::LN_TYPE, nt::MSGPORT)?;
        self.new_list(p + msgport::MP_MSGLIST, nt::MESSAGE)?;
        self.enqueue(self.base + execbase::PORTLIST, p)
    }

    /// `RemPort`.
    pub fn rem_port(&mut self, p: u32) -> Result<(), Fallo> {
        self.remove(p)
    }

    /// `FindPort`.
    pub fn find_port(&self, nombre: u32) -> Result<u32, Fallo> {
        let n = self.cadena(nombre)?;
        self.find_name(self.base + execbase::PORTLIST, &n)
    }

    fn comprobar_puerto(&mut self, p: u32, funcion: &'static str) -> Result<(), Fallo> {
        if p == 0 {
            return Err(Fallo::PunteroNulo);
        }
        if !self.memoria_viva(p, msgport::TAMANO) {
            return Err(Fallo::PuertoMuerto { dir: p });
        }
        if self.tipo(p)? != nt::MSGPORT {
            // Exec no lo mira; algunos programas montan puertos a mano sin poner el tipo.
            self.avisar(funcion, Fallo::Sospechoso { dir: p });
        }
        Ok(())
    }

    /// Si el mensaje está enlazado en la cola de algún puerto.
    fn en_cola(&self, m: u32) -> bool {
        let (Ok(s), Ok(a)) = (self.lp(m + node::LN_SUCC), self.lp(m + node::LN_PRED)) else { return false };
        s != 0 && a != 0 && self.l32(a + node::LN_SUCC) == Ok(m) && self.l32(s + node::LN_PRED) == Ok(m)
    }

    /// Mete el mensaje en la cola y hace lo que diga el puerto.
    fn entregar(&mut self, p: u32, m: u32, tipo: u8, funcion: &'static str) -> Result<Accion, Fallo> {
        if !self.memoria_viva(m, message::TAMANO) {
            return Err(Fallo::MensajeMuerto { dir: m });
        }
        if self.en_cola(m) {
            return Err(Fallo::MensajeEnCola { dir: m });
        }
        self.e8(m + node::LN_TYPE, tipo)?;
        self.add_tail(p + msgport::MP_MSGLIST, m)?;
        match self.l8(p + msgport::MP_FLAGS)? & msgport::PF_ACTION {
            msgport::PA_SIGNAL => {
                let t = self.l32(p + msgport::MP_SIGTASK)?;
                let s = self.l8(p + msgport::MP_SIGBIT)? as u32;
                if s > 31 || !self.tareas.contains_key(&t) {
                    self.avisar(funcion, Fallo::TareaDesconocida { dir: t });
                    return Ok(Accion::Seguir);
                }
                self.signal(t, 1 << s)
            }
            msgport::PA_SOFTINT => {
                // Las interrupciones por software de la caja aún no están: el mensaje queda en la cola.
                self.avisar(funcion, Fallo::NoImplementada { n: 30 });
                Ok(Accion::Seguir)
            }
            _ => Ok(Accion::Seguir),
        }
    }

    /// `PutMsg`.
    pub fn put_msg(&mut self, p: u32, m: u32) -> Result<Accion, Fallo> {
        if m == 0 {
            return Err(Fallo::PunteroNulo);
        }
        self.comprobar_puerto(p, "PutMsg")?;
        self.entregar(p, m, nt::MESSAGE, "PutMsg")
    }

    /// `GetMsg`: el primer mensaje, sacado de la cola, o 0.
    pub fn get_msg(&mut self, p: u32) -> Result<u32, Fallo> {
        self.comprobar_puerto(p, "GetMsg")?;
        self.rem_head(p + msgport::MP_MSGLIST)
    }

    /// `ReplyMsg`: devuelve el mensaje a su `mn_ReplyPort`. Sin puerto de vuelta, solo lo marca como libre.
    pub fn reply_msg(&mut self, m: u32) -> Result<Accion, Fallo> {
        if m == 0 {
            return Err(Fallo::PunteroNulo);
        }
        if !self.memoria_viva(m, message::TAMANO) {
            return Err(Fallo::MensajeMuerto { dir: m });
        }
        let p = self.l32(m + message::MN_REPLYPORT)?;
        if p == 0 {
            self.e8(m + node::LN_TYPE, nt::FREEMSG)?;
            return Ok(Accion::Seguir);
        }
        self.comprobar_puerto(p, "ReplyMsg")?;
        self.entregar(p, m, nt::REPLYMSG, "ReplyMsg")
    }

    /// `WaitPort`: el primer mensaje (sin sacarlo), esperando a que llegue si no hay.
    pub fn wait_port(&mut self, p: u32) -> Result<Espera, Fallo> {
        let t = self.tarea_actual();
        if t == 0 || !self.tareas.contains_key(&t) {
            return Err(Fallo::SinTarea);
        }
        let vuelve = self.vuelve_de_espera(t);
        self.comprobar_puerto(p, "WaitPort")?;
        let bit = 1u32 << (self.l8(p + msgport::MP_SIGBIT)? & 31);
        let recibidas = self.l32(t + task::TC_SIGRECVD)?;
        if vuelve {
            // El Wait de dentro de WaitPort se ha cumplido: quita la señal, como Wait.
            self.e32(t + task::TC_SIGRECVD, recibidas & !bit)?;
        }
        let l = p + msgport::MP_MSGLIST;
        if !self.lista_vacia(l)? {
            return Ok(Espera::Hecho(self.l32(l + list::LH_HEAD)?));
        }
        // Sin mensajes: una señal que ya estuviera puesta no vale (Wait la gastaría y volvería a esperar).
        let recibidas = self.l32(t + task::TC_SIGRECVD)?;
        self.e32(t + task::TC_SIGRECVD, recibidas & !bit)?;
        if self.l32(p + msgport::MP_SIGTASK)? != t {
            self.avisar("WaitPort", Fallo::Sospechoso { dir: p });
        }
        self.dormir(t, bit)?;
        Ok(Espera::Bloquea)
    }
}
