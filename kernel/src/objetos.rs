//! Objetos del kernel, llaves (capacidades) y canales (C12, S1).
//!
//! Un proceso no nombra los objetos: tiene llaves, que son números de su propia tabla. Cada llave apunta a un objeto
//! con unos derechos. Las llaves se pueden mandar por un canal: salen de la tabla de quien manda y entran en la de
//! quien recibe, con los mismos derechos o menos, nunca más.

use crate::cerrojo::Cerrojo;
use crate::planificador::{self, Hilo};
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;

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

/// Un objeto de memoria. Sus páginas se devuelven cuando no queda ni una llave ni una proyección (C18): cada
/// espacio que lo proyecta guarda un `Arc`.
pub struct Memoria {
    pub paginas: Vec<u64>,
}

impl Drop for Memoria {
    fn drop(&mut self) {
        for &p in &self.paginas {
            crate::memoria::paginas().liberar(p, 1);
        }
    }
}

impl Memoria {
    /// Páginas nuevas, a cero. `None` si no hay memoria fuera de la reserva del kernel; entonces las páginas que ya
    /// se habían tomado vuelven al repartidor (al soltar el objeto a medias).
    pub fn nueva(tamano: usize) -> Option<Self> {
        let n = tamano.div_ceil(crate::memoria::PAGINA as usize);
        let mut m = Self { paginas: Vec::new() };
        m.paginas.try_reserve_exact(n).ok()?;
        for _ in 0..n {
            let p = crate::memoria::paginas().pagina_usuario()?;
            unsafe { core::ptr::write_bytes(crate::mmu::virt(p as usize) as *mut u8, 0, crate::memoria::PAGINA as usize) };
            m.paginas.push(p);
        }
        Some(m)
    }
}

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
    esperando: Option<*mut Hilo>,
    cerrado: bool,
}

unsafe impl Send for Lado {}

/// Un canal entre dos lados. Un solo cerrojo para los dos: así no hay orden de cerrojos que respetar y dos lectores
/// en los dos extremos no se esperan el uno al otro (auditoría, hallazgo 7).
pub struct Canal {
    lados: Cerrojo<[Lado; 2]>,
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
    pub fn nuevo() -> (Llave, Llave) {
        let c = Arc::new(Canal { lados: Cerrojo::new([Lado::default(), Lado::default()]) });
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
            planificador::despertar(h);
        }
        Ok(())
    }

    /// Lee en el lado `lado`, esperando si no hay nada.
    pub fn leer(&self, lado: usize) -> Result<Mensaje, Error> {
        loop {
            let mut l = self.lados.tomar();
            if let Some(m) = l[lado].cola.pop_front() {
                return Ok(m);
            }
            if l[1 - lado].cerrado {
                return Err(Error::Cerrado);
            }
            l[lado].esperando = Some(planificador::actual());
            // El cerrojo se suelta con el hilo ya bloqueado: así quien escriba no puede despertarlo antes.
            planificador::bloquear(move || drop(l));
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
            planificador::despertar(h);
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
}

/// El proceso del hilo actual, si lo hay.
pub fn proceso_actual<'a>() -> Option<&'a Proceso> {
    let p = planificador::proceso_actual();
    if p == 0 { None } else { Some(unsafe { &*(p as *const Proceso) }) }
}
