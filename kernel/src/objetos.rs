//! Objetos del kernel, llaves (capacidades) y canales (C12, S1).
//!
//! Un proceso no nombra los objetos: tiene llaves, que son números de su propia tabla. Cada llave apunta a un objeto
//! con unos derechos. Las llaves se pueden mandar por un canal: salen de la tabla de quien manda y entran en la de
//! quien recibe, con los mismos derechos o menos, nunca más.

use crate::cerrojo::Cerrojo;
use crate::planificador::{self, HiloRef};
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

pub const LEER: u32 = 1 << 0;
pub const ESCRIBIR: u32 = 1 << 1;
pub const TRANSFERIR: u32 = 1 << 2;
pub const TODOS: u32 = LEER | ESCRIBIR | TRANSFERIR;

pub const MAX_DATOS: usize = 256;
pub const MAX_LLAVES: usize = 8;
/// Límites por proceso y por canal, para que un proceso no se coma la memoria del kernel (auditoría, hallazgo 8).
/// Son provisionales: los cupos por grupo de procesos (S16, F) los sustituyen.
pub const MAX_TABLA: usize = 1024;
pub const MAX_COLA: usize = 16;
/// Canales vivos creados por un proceso, estén donde estén: en su tabla o dentro de mensajes de otros canales. Sin
/// esto, meter canales llenos dentro de otros canales gastaba el montón del kernel sin límite (auditoría de cierre de
/// la fase 1, N1). Con 512 canales de 16 mensajes por lado, un proceso no pasa de unos 7 MB del montón.
pub const MAX_CANALES: usize = 512;

#[derive(Clone)]
pub enum Objeto {
    /// Un extremo de un canal: (canal, lado 0 o 1).
    Extremo(Arc<Canal>, usize),
    /// Un objeto de memoria (C13, S3): páginas físicas que se pueden proyectar en varios procesos.
    Memoria(Arc<Memoria>),
    /// Registros de un aparato (C20): un tramo físico de periféricos que se puede proyectar en un proceso.
    Registros { fisica: u64, tamano: u64 },
    /// Una interrupción de aparato (C20): llega como mensaje a un canal.
    Interrupcion(Arc<crate::interrupciones::Interrupcion>),
}

/// Un objeto de memoria: está en `base`, que es quien toca las páginas (C37).
pub use crate::memoria::Memoria;

#[derive(Clone)]
pub struct Llave {
    pub objeto: Objeto,
    pub derechos: u32,
}

pub struct Mensaje {
    pub datos: Vec<u8>,
    pub llaves: Vec<Llave>,
}

#[derive(Default)]
struct Lado {
    /// Mensajes que esperan a que este lado los lea.
    cola: VecDeque<Mensaje>,
    /// Hilo bloqueado leyendo en este lado.
    esperando: Option<HiloRef>,
    cerrado: bool,
}


/// Un canal entre dos lados. Un solo cerrojo para los dos: así no hay orden de cerrojos que respetar y dos lectores
/// en los dos extremos no se esperan el uno al otro (auditoría, hallazgo 7).
pub struct Canal {
    lados: Cerrojo<[Lado; 2]>,
    /// La cuenta de canales del proceso que lo creó; se descuenta al soltarlo.
    cuenta: Option<Arc<AtomicUsize>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Cerrado,
    /// La cola del otro lado tiene ya `MAX_COLA` mensajes.
    Llena,
    SinMemoria,
}

/// Soltar un canal suelta los mensajes de sus colas, y con ellos sus llaves, que pueden ser de otros canales con
/// más mensajes dentro. Hecho por recursión, una cadena de unas decenas de canales desbordaba la pila del kernel
/// (auditoría, hallazgo 3). Aquí se hace con una lista: cada canal que se queda sin dueños se vacía en ella, y lo
/// que hay dentro se suelta en el mismo bucle.
impl Drop for Canal {
    fn drop(&mut self) {
        if let Some(c) = &self.cuenta {
            c.fetch_sub(1, Ordering::Relaxed);
        }
        let mut mensajes: Vec<Mensaje> = Vec::new();
        for lado in self.lados.get_mut().iter_mut() {
            mensajes.extend(lado.cola.drain(..));
        }
        while let Some(m) = mensajes.pop() {
            for l in m.llaves {
                let canal = match l.objeto {
                    Objeto::Extremo(c, _) => Some(c),
                    Objeto::Interrupcion(i) => match Arc::try_unwrap(i) {
                        Ok(mut i) => i.quitar_destino(),
                        Err(_) => None,
                    },
                    _ => None,
                };
                // Si era el último dueño, sus mensajes pasan a la lista y el canal se suelta ya vacío.
                if let Some(c) = canal {
                    if let Ok(mut c) = Arc::try_unwrap(c) {
                        for lado in c.lados.get_mut().iter_mut() {
                            mensajes.extend(lado.cola.drain(..));
                        }
                    }
                }
            }
        }
    }
}

impl Canal {
    /// Un canal del kernel, que no cuenta para ningún proceso.
    pub fn nuevo() -> (Llave, Llave) {
        Self::con_cuenta(None)
    }

    /// Un canal creado por un proceso: cuenta para su límite de `MAX_CANALES`. `None` si ya está en el límite.
    pub fn nuevo_de(cuenta: &Arc<AtomicUsize>) -> Option<(Llave, Llave)> {
        let mut n = cuenta.load(Ordering::Relaxed);
        loop {
            if n >= MAX_CANALES {
                return None;
            }
            match cuenta.compare_exchange_weak(n, n + 1, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(visto) => n = visto,
            }
        }
        Some(Self::con_cuenta(Some(cuenta.clone())))
    }

    fn con_cuenta(cuenta: Option<Arc<AtomicUsize>>) -> (Llave, Llave) {
        let c = Arc::new(Canal { lados: Cerrojo::new([Lado::default(), Lado::default()]), cuenta });
        (
            Llave { objeto: Objeto::Extremo(c.clone(), 0), derechos: TODOS },
            Llave { objeto: Objeto::Extremo(c, 1), derechos: TODOS },
        )
    }

    /// Manda desde el lado `lado`: el mensaje va a la cola del otro. Si no puede (cerrado, cola llena o sin
    /// memoria), devuelve el mensaje para que quien manda recupere sus llaves.
    pub fn escribir(&self, lado: usize, m: Mensaje) -> Result<(), (Error, Mensaje)> {
        let despertar = {
            let mut l = self.lados.tomar();
            let otro = &mut l[1 - lado];
            if otro.cerrado {
                return Err((Error::Cerrado, m));
            }
            if otro.cola.len() >= MAX_COLA {
                return Err((Error::Llena, m));
            }
            if otro.cola.try_reserve(1).is_err() {
                return Err((Error::SinMemoria, m));
            }
            otro.cola.push_back(m);
            otro.esperando.take()
        };
        if let Some(h) = despertar {
            h.despertar();
        }
        Ok(())
    }

    /// Lee en el lado `lado`, esperando si no hay nada.
    pub fn leer(&self, lado: usize) -> Result<Mensaje, Error> {
        // Con las IRQ enmascaradas desde antes de tomar el cerrojo: se suelta dentro del planificador al bloquearse,
        // y ahí no deben abrirse (cerrojo.rs).
        let estado = crate::cerrojo::enmascarar_irq();
        let r = self.leer_enmascarado(lado);
        crate::cerrojo::restaurar_irq(estado);
        r
    }

    fn leer_enmascarado(&self, lado: usize) -> Result<Mensaje, Error> {
        loop {
            let mut l = self.lados.tomar();
            if let Some(m) = l[lado].cola.pop_front() {
                return Ok(m);
            }
            if l[1 - lado].cerrado {
                return Err(Error::Cerrado);
            }
            l[lado].esperando = Some(planificador::yo());
            // El cerrojo se suelta con el hilo ya bloqueado: así quien escriba no puede despertarlo antes.
            planificador::bloquear_soltando(l);
        }
    }

    /// Devuelve al principio de la cola de `lado` un mensaje que se leyó pero no se pudo entregar (C31).
    pub fn devolver(&self, lado: usize, m: Mensaje) {
        self.lados.tomar()[lado].cola.push_front(m);
    }

    pub fn cerrar(&self, lado: usize) {
        let despertar = {
            let mut l = self.lados.tomar();
            l[lado].cerrado = true;
            l[1 - lado].esperando.take()
        };
        if let Some(h) = despertar {
            h.despertar();
        }
    }
}

/// La tabla de llaves de un proceso. La llave 0 no existe nunca.
#[derive(Default)]
pub struct TablaLlaves {
    llaves: Vec<Option<Llave>>,
}

impl TablaLlaves {
    /// Pone la llave en el primer hueco libre y devuelve su número. Si la tabla está llena (`MAX_TABLA`) o no hay
    /// memoria para crecer, devuelve la llave.
    pub fn instalar(&mut self, l: Llave) -> Result<u32, Llave> {
        if self.llaves.is_empty() {
            if self.llaves.try_reserve(1).is_err() {
                return Err(l);
            }
            self.llaves.push(None);
        }
        if let Some(i) = self.llaves.iter().skip(1).position(|x| x.is_none()) {
            self.llaves[i + 1] = Some(l);
            return Ok((i + 1) as u32);
        }
        if self.llaves.len() > MAX_TABLA || self.llaves.try_reserve(1).is_err() {
            return Err(l);
        }
        self.llaves.push(Some(l));
        Ok((self.llaves.len() - 1) as u32)
    }

    /// Cuántas llaves caben todavía sin pasar de `MAX_TABLA`.
    pub fn huecos(&self) -> usize {
        let libres = self.llaves.iter().skip(1).filter(|x| x.is_none()).count();
        libres + (MAX_TABLA + 1).saturating_sub(self.llaves.len().max(1))
    }

    pub fn ver(&self, h: u32) -> Option<Llave> {
        self.llaves.get(h as usize).and_then(|x| x.clone())
    }

    pub fn quitar(&mut self, h: u32) -> Option<Llave> {
        self.llaves.get_mut(h as usize).and_then(|x| x.take())
    }

    /// Vuelve a poner una llave en el hueco `h`, que tiene que estar vacío (se quitó en la misma llamada).
    pub fn reponer(&mut self, h: u32, l: Llave) {
        if let Some(x @ None) = self.llaves.get_mut(h as usize) {
            *x = Some(l);
        }
    }
}

/// Un proceso: su espacio de direcciones y su tabla de llaves.
pub struct Proceso {
    pub ttbr0: u64,
    /// Orden de bytes de sus datos en EL0 (SCTLR_EL1.E0E). Lo nativo de DashOS es little-endian; la caja Amiga y
    /// sus bibliotecas, big-endian como el 68k (decisión del usuario, 03/11/2026).
    pub big_endian: bool,
    pub espacio: Cerrojo<crate::espacio::Espacio>,
    pub llaves: Cerrojo<TablaLlaves>,
    /// Cuántos canales suyos siguen vivos (`MAX_CANALES`).
    pub canales: Arc<AtomicUsize>,
}

/// El proceso del hilo actual, si lo hay.
pub fn proceso_actual() -> Option<Arc<Proceso>> {
    planificador::proceso_actual_arc()
}
