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
    pub fn nueva(tamano: usize) -> Option<Self> {
        let n = tamano.div_ceil(crate::memoria::PAGINA as usize);
        let mut paginas = Vec::with_capacity(n);
        for _ in 0..n {
            let p = crate::memoria::paginas().pagina(crate::memoria::Zona::Normal)?;
            unsafe { core::ptr::write_bytes(crate::mmu::virt(p as usize) as *mut u8, 0, crate::memoria::PAGINA as usize) };
            paginas.push(p);
        }
        Some(Self { paginas })
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

pub struct Canal {
    lados: [Cerrojo<Lado>; 2],
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Cerrado,
}

impl Canal {
    pub fn nuevo() -> (Llave, Llave) {
        let c = Arc::new(Canal { lados: [Cerrojo::new(Lado::default()), Cerrojo::new(Lado::default())] });
        (
            Llave { objeto: Objeto::Extremo(c.clone(), 0), derechos: TODOS },
            Llave { objeto: Objeto::Extremo(c, 1), derechos: TODOS },
        )
    }

    /// Manda desde el lado `lado`: el mensaje va a la cola del otro.
    pub fn escribir(&self, lado: usize, m: Mensaje) -> Result<(), Error> {
        let despertar = {
            let mut otro = self.lados[1 - lado].tomar();
            if otro.cerrado {
                return Err(Error::Cerrado);
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
            let mut yo = self.lados[lado].tomar();
            if let Some(m) = yo.cola.pop_front() {
                return Ok(m);
            }
            if self.lados[1 - lado].tomar().cerrado {
                return Err(Error::Cerrado);
            }
            yo.esperando = Some(planificador::actual());
            // El cerrojo del lado se suelta con el hilo ya bloqueado: así quien escriba no puede despertarlo antes.
            planificador::bloquear(move || drop(yo));
        }
    }

    /// Devuelve al principio de la cola de `lado` un mensaje que se leyó pero no se pudo entregar (C31).
    pub fn devolver(&self, lado: usize, m: Mensaje) {
        self.lados[lado].tomar().cola.push_front(m);
    }

    pub fn cerrar(&self, lado: usize) {
        self.lados[lado].tomar().cerrado = true;
        let despertar = self.lados[1 - lado].tomar().esperando.take();
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
    pub fn instalar(&mut self, l: Llave) -> u32 {
        if self.llaves.is_empty() {
            self.llaves.push(None);
        }
        if let Some(i) = self.llaves.iter().skip(1).position(|x| x.is_none()) {
            self.llaves[i + 1] = Some(l);
            return (i + 1) as u32;
        }
        self.llaves.push(Some(l));
        (self.llaves.len() - 1) as u32
    }

    pub fn ver(&self, h: u32) -> Option<Llave> {
        self.llaves.get(h as usize).and_then(|x| x.clone())
    }

    pub fn quitar(&mut self, h: u32) -> Option<Llave> {
        self.llaves.get_mut(h as usize).and_then(|x| x.take())
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
