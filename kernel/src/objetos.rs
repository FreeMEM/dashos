//! Objetos del kernel, llaves (capacidades) y canales (C12, S1).
//!
//! Un proceso no nombra los objetos: tiene llaves, que son números de su propia tabla. Cada llave apunta a un objeto
//! con unos derechos. Las llaves se pueden mandar por un canal: salen de la tabla de quien manda y entran en la de
//! quien recibe, con los mismos derechos o menos, nunca más.

use crate::cerrojo::Cerrojo;
use crate::planificador::{self, ColaEspera, HiloRef};
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
/// Son límites de forma; lo que cuesta cada cosa se cobra además al cupo del grupo (C42).
pub const MAX_TABLA: usize = 1024;
pub const MAX_COLA: usize = 16;

#[derive(Clone)]
pub enum Objeto {
    /// Un extremo de un canal. Cuando se suelta su última llave, ese lado se cierra (C40).
    Extremo(Arc<Punta>),
    /// Un objeto de memoria (C13, S3): páginas físicas que se pueden proyectar en varios procesos.
    Memoria(Arc<Memoria>),
    /// Registros de un aparato (C20): un tramo físico de periféricos que se puede proyectar en un proceso.
    Registros { fisica: u64, tamano: u64 },
    /// Una interrupción de aparato (C20): llega como mensaje a un canal.
    Interrupcion(Arc<crate::interrupciones::Interrupcion>),
    /// Un proceso visto desde fuera (C39): con escribir se mata, con leer se espera a que acabe.
    Proceso(Arc<Control>),
    /// Un grupo de procesos con su cupo (C42): con escribir se crean procesos en él y se mata; con leer se mira el uso.
    Grupo(Arc<crate::cupo::Cupo>),
    /// Una llave revocable (C41): apunta al objeto a través de un intermediario que se puede vaciar.
    Revocable(Arc<Revocable>),
    /// La llave con la que se revoca: vacía el intermediario, y todas las llaves que pasan por él dejan de valer.
    Revocador(Arc<Revocable>),
}

/// El intermediario de las llaves revocables (C41).
pub struct Revocable {
    objeto: Cerrojo<Option<Objeto>>,
    /// Cuántos intermediarios hay por debajo, contando este: como mucho `MAX_REVOCABLES` (C43, auditoría C1).
    profundidad: u32,
    _cargo: crate::cupo::Cargo,
}

/// Intermediarios dentro de intermediarios, como mucho. Soltar una cadena va por recursión, y `ver` la recorre.
pub const MAX_REVOCABLES: u32 = 4;

impl Revocable {
    /// Un intermediario para `objeto`, cobrado a `cupo`. `None` si ya hay `MAX_REVOCABLES` por debajo (contando los
    /// revocadores: un revocador también lleva su intermediario) o si no cabe en el cupo.
    pub fn nuevo(objeto: Objeto, cupo: &Arc<crate::cupo::Cupo>) -> Option<Self> {
        let debajo = match &objeto {
            Objeto::Revocable(r) | Objeto::Revocador(r) => r.profundidad,
            _ => 0,
        };
        if debajo + 1 > MAX_REVOCABLES {
            return None;
        }
        let cargo = cupo.cobrar(crate::cupo::COSTE_REVOCABLE)?;
        Some(Self { objeto: Cerrojo::new(Some(objeto)), profundidad: debajo + 1, _cargo: cargo })
    }

    /// Revoca: suelta el objeto (fuera del cerrojo). Si era lo último que lo tenía, se libera; un lado de canal se
    /// cierra.
    pub fn revocar(&self) {
        let objeto = self.objeto.tomar().take();
        drop(objeto);
    }
}

/// Cómo acabó un proceso (C39).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fin {
    /// Llamó a `salir` con este código.
    Salida(u64),
    /// Lo mató alguien con su llave.
    Matado,
    /// Murió por un fallo (una excepción en EL0).
    Fallo,
}

impl Fin {
    /// Para `proceso_esperar`: (clase, código). Clase 0 salida, 1 matado, 2 fallo.
    pub fn valores(self) -> (u64, u64) {
        match self {
            Fin::Salida(c) => (0, c),
            Fin::Matado => (1, 0),
            Fin::Fallo => (2, 0),
        }
    }
}

/// ¿Mandar `l` por `canal` haría un ciclo? Un lado del mismo canal, o una interrupción que avisa a ese canal.
pub fn forma_ciclo(l: &Llave, canal: &Arc<Canal>) -> bool {
    match &l.objeto {
        Objeto::Extremo(p) => Arc::ptr_eq(p.canal(), canal),
        Objeto::Interrupcion(i) => i.destino().is_some_and(|c| Arc::ptr_eq(&c, canal)),
        // Un revocador lleva dentro su objeto: si es un lado de este canal, también es un ciclo (auditoría, M1).
        Objeto::Revocador(r) => r.objeto.tomar().as_ref().is_some_and(|o| forma_ciclo(&Llave::nueva(o.clone(), 0), canal)),
        _ => false,
    }
}

/// Lo que se sabe de un proceso desde fuera (C39). La llave de proceso apunta aquí y no al proceso, así que tenerla
/// no mantiene viva su memoria: el proceso se libera al salir su último hilo, haya llaves o no.
#[derive(Default)]
pub struct Control {
    pub estado: Cerrojo<EstadoProceso>,
}

#[derive(Default)]
pub struct EstadoProceso {
    /// Sus hilos vivos. Cada hilo se quita de aquí al morir (`planificador::terminar_actual`), con el cerrojo
    /// tomado; mientras alguien lo tiene, ningún hilo de la lista puede liberarse.
    pub hilos: Vec<HiloRef>,
    /// Cómo acabó, cuando ya no le quedan hilos.
    pub fin: Option<Fin>,
    /// Hilos bloqueados en `proceso_esperar`.
    pub esperando: ColaEspera,
    /// Lo han matado (C43, M2): los hilos que se apunten después nacen marcados.
    pub matado: bool,
}

/// Un objeto de memoria: está en `base`, que es quien toca las páginas (C37).
pub use crate::memoria::Memoria;

#[derive(Clone)]
pub struct Llave {
    pub objeto: Objeto,
    pub derechos: u32,
    /// La identidad que lleva (C41): la pone quien la duplica, una sola vez, y llega con cada mensaje que se escribe
    /// por ella. 0 es sin marca. Así un servidor sabe quién le habla sin fiarse de lo que diga el mensaje.
    pub marca: u64,
}

impl Llave {
    pub fn nueva(objeto: Objeto, derechos: u32) -> Self {
        Self { objeto, derechos, marca: 0 }
    }
}

pub struct Mensaje {
    pub datos: Vec<u8>,
    pub llaves: Vec<Llave>,
    /// La marca de la llave por la que se escribió (C41); 0 si no tenía o lo escribió el kernel.
    pub marca: u64,
}

#[derive(Default)]
struct Lado {
    /// Mensajes que esperan a que este lado los lea.
    cola: VecDeque<Mensaje>,
    /// Hilo bloqueado leyendo en este lado.
    esperando: ColaEspera,
    cerrado: bool,
}


/// Un canal entre dos lados. Un solo cerrojo para los dos: así no hay orden de cerrojos que respetar y dos lectores
/// en los dos extremos no se esperan el uno al otro (auditoría, hallazgo 7).
pub struct Canal {
    lados: Cerrojo<[Lado; 2]>,
    /// Lo cobrado al grupo del proceso que lo creó (C42); se devuelve al soltarlo.
    _cargo: Option<crate::cupo::Cargo>,
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
/// Un lado de un canal, como objeto (C40). Lo tienen las llaves de ese lado. Al soltarse el último `Arc`, sea porque
/// se cerró la llave, porque murió el proceso que la tenía o porque se perdió dentro de un mensaje, el lado se cierra:
/// el otro lado se entera (`E_CERRADO`) y los mensajes que esperaban a este lado, que ya nadie leerá, se sueltan.
pub struct Punta {
    /// Siempre `Some` salvo dentro de `soltar`, que lo saca para cerrar sin recursión.
    canal: Option<Arc<Canal>>,
    lado: usize,
}

impl Punta {
    pub fn canal(&self) -> &Arc<Canal> {
        self.canal.as_ref().expect("punta sin canal fuera de soltar")
    }

    pub fn lado(&self) -> usize {
        self.lado
    }
}

impl Drop for Punta {
    fn drop(&mut self) {
        if let Some(c) = self.canal.take() {
            let mut pendientes = Vec::new();
            cerrar_lado(c, self.lado, &mut pendientes);
            soltar(pendientes);
        }
    }
}

/// Cierra un lado y, si era el último dueño del canal, lo vacía. Lo que sale va a `pendientes`.
fn cerrar_lado(c: Arc<Canal>, lado: usize, pendientes: &mut Vec<Mensaje>) {
    pendientes.extend(c.cerrar_y_vaciar(lado));
    if let Ok(mut canal) = Arc::try_unwrap(c) {
        pendientes.extend(canal.vaciar());
    }
}

/// Suelta mensajes. Sus llaves pueden ser de lados de otros canales con más mensajes dentro, y así en cadena: por
/// recursión, una cadena larga desbordaba la pila del kernel (auditoría, hallazgo 3). Aquí va con una lista: cada lado
/// que se queda sin dueños se cierra y lo que contenía pasa a la lista.
fn soltar(mut pendientes: Vec<Mensaje>) {
    while let Some(m) = pendientes.pop() {
        let mut objetos: Vec<Objeto> = m.llaves.into_iter().map(|l| l.objeto).collect();
        while let Some(objeto) = objetos.pop() {
            match objeto {
                Objeto::Revocable(r) | Objeto::Revocador(r) => {
                    // Si nadie más tiene el intermediario, lo de dentro se suelta en este mismo bucle.
                    if let Ok(mut r) = Arc::try_unwrap(r) {
                        if let Some(o) = r.objeto.get_mut().take() {
                            objetos.push(o);
                        }
                    }
                }
                Objeto::Extremo(p) => {
                    if let Ok(mut p) = Arc::try_unwrap(p) {
                        if let Some(c) = p.canal.take() {
                            cerrar_lado(c, p.lado, &mut pendientes);
                        }
                    }
                }
                Objeto::Interrupcion(i) => {
                    if let Ok(mut i) = Arc::try_unwrap(i) {
                        if let Some(c) = i.quitar_destino() {
                            if let Ok(mut canal) = Arc::try_unwrap(c) {
                                pendientes.extend(canal.vaciar());
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

impl Drop for Canal {
    fn drop(&mut self) {
        let mensajes = self.vaciar();
        soltar(mensajes);
    }
}

impl Canal {
    /// Un canal del kernel, que no se cobra a ningún grupo.
    pub fn nuevo() -> (Llave, Llave) {
        Self::con_cargo(None)
    }

    /// Un canal creado por un proceso: se cobra `COSTE_CANAL` a su grupo (C42). `None` si no cabe.
    pub fn nuevo_de(cupo: &Arc<crate::cupo::Cupo>) -> Option<(Llave, Llave)> {
        Some(Self::con_cargo(Some(cupo.cobrar(crate::cupo::COSTE_CANAL)?)))
    }

    fn con_cargo(cargo: Option<crate::cupo::Cargo>) -> (Llave, Llave) {
        let c = Arc::new(Canal { lados: Cerrojo::new([Lado::default(), Lado::default()]), _cargo: cargo });
        (
            Llave::nueva(Objeto::Extremo(Arc::new(Punta { canal: Some(c.clone()), lado: 0 })), TODOS),
            Llave::nueva(Objeto::Extremo(Arc::new(Punta { canal: Some(c), lado: 1 })), TODOS),
        )
    }

    /// Manda desde el lado `lado`: el mensaje va a la cola del otro. Si no puede (cerrado, cola llena o sin
    /// memoria), devuelve el mensaje para que quien manda recupere sus llaves.
    pub fn escribir(&self, lado: usize, m: Mensaje) -> Result<(), (Error, Mensaje)> {
        {
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
            // Con el cerrojo tomado: los que esperan siguen vivos mientras lo tengamos (auditoría de la fase 3, A2).
            otro.esperando.despertar_todos();
        };
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
            // Si lo despertaron para matarlo (C39), nadie lo quitó de la cola: se quita él, para que el canal no guarde
            // un hilo que va a morir.
            l[lado].esperando.quitarme();
            if planificador::debe_morir() {
                return Err(Error::Cerrado);
            }
            if let Some(m) = l[lado].cola.pop_front() {
                return Ok(m);
            }
            if l[1 - lado].cerrado {
                return Err(Error::Cerrado);
            }
            if !l[lado].esperando.apuntarme() {
                return Err(Error::SinMemoria);
            }
            // El cerrojo se suelta con el hilo ya bloqueado: así quien escriba no puede despertarlo antes.
            planificador::bloquear_soltando(l);
        }
    }

    /// Devuelve al principio de la cola de `lado` un mensaje que se leyó pero no se pudo entregar (C31).
    pub fn devolver(&self, lado: usize, m: Mensaje) {
        self.lados.tomar()[lado].cola.push_front(m);
    }

    /// Cierra el lado `lado`: el otro se entera, y los mensajes que esperaban a este lado se devuelven para soltarlos
    /// fuera del cerrojo.
    fn cerrar_y_vaciar(&self, lado: usize) -> Vec<Mensaje> {
        let mut l = self.lados.tomar();
        l[lado].cerrado = true;
        l[1 - lado].esperando.despertar_todos();
        core::mem::take(&mut l[lado].cola).into()
    }

    /// Saca todos los mensajes de las dos colas (cuando ya nadie más tiene el canal).
    fn vaciar(&mut self) -> Vec<Mensaje> {
        let mut v = Vec::new();
        for lado in self.lados.get_mut().iter_mut() {
            v.extend(lado.cola.drain(..));
        }
        v
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

    /// Reserva memoria para `n` llaves más, para que `instalar` no tenga que pedirla. Falso si no hay.
    pub fn reservar(&mut self, n: usize) -> bool {
        self.llaves.try_reserve(n).is_ok()
    }

    /// Cuántas llaves caben todavía sin pasar de `MAX_TABLA`.
    pub fn huecos(&self) -> usize {
        let libres = self.llaves.iter().skip(1).filter(|x| x.is_none()).count();
        libres + (MAX_TABLA + 1).saturating_sub(self.llaves.len().max(1))
    }

    /// La llave `h` lista para usarse: si es revocable, con el objeto de detrás (y `None` si está revocada).
    pub fn ver(&self, h: u32) -> Option<Llave> {
        let mut l = self.ver_crudo(h)?;
        while let Objeto::Revocable(r) = &l.objeto {
            let dentro = r.objeto.tomar().clone()?;
            l.objeto = dentro;
        }
        Some(l)
    }

    /// La llave `h` tal cual, con su intermediario si es revocable (para duplicarla o hacerla revocable otra vez).
    pub fn ver_crudo(&self, h: u32) -> Option<Llave> {
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
    /// Su grupo (C42): a él se cobra lo que crea.
    pub cupo: Arc<crate::cupo::Cupo>,
    /// Lo que cuesta el proceso en sí (`COSTE_PROCESO`).
    pub _cargo: crate::cupo::Cargo,
    /// Sus hilos y cómo acabó (C39). Lo comparte con las llaves de proceso que lo nombran.
    pub control: Arc<Control>,
}

/// El proceso del hilo actual, si lo hay.
pub fn proceso_actual() -> Option<Arc<Proceso>> {
    planificador::proceso_actual_arc()
}
