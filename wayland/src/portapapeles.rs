//! El portapapeles (H9, rama `caja-linux-apps`): la selección de siempre (`wl_data_device`, copiar y pegar) y la
//! primaria (`zwp_primary_selection_device_v1`, seleccionar y pegar con el botón central). Las dos van igual; solo
//! cambian las interfaces y los números de sus eventos.
//!
//! Lo que dice la especificación (`wayland.xml` y `primary-selection-unstable-v1.xml`), y lo que hace DashOS:
//! - Un programa crea una fuente, dice sus tipos (`offer`) y la pone como selección (`set_selection`). Solo vale si
//!   tiene el teclado: así un programa de fondo no cambia el portapapeles a escondidas (ni lo vacía).
//! - Al que tiene el teclado (al recibirlo, o cuando cambia la selección mientras lo tiene) se le manda una oferta: un
//!   objeto nuevo del compositor (`data_offer`) con los tipos y, detrás, `selection`. Sin selección, `selection` nulo.
//! - Para pegar, el programa pide un tipo de la oferta con un descriptor (`receive`): en DashOS, la llave de un extremo
//!   de canal en el que se escribe. El compositor no toca los datos: pasa la llave a la fuente (`send`), que escribe en
//!   ella y la cierra. Los datos van en mensajes de canal; el fin, al cerrarse el otro extremo (`E_CERRADO`). Así un
//!   programa que no acaba de escribir, o que escribe sin fin, no detiene al compositor.
//! - Una fuente que deja de ser la selección recibe `cancelled`; si se destruye o su programa se va, ya no hay
//!   selección.
//! - Arrastrar y soltar no está: `start_drag` cancela su fuente en el acto.
//! - S29 (`docs/diseño/S29-malware-linux-y-dashos.md`): pegar es cosa de quien está delante. Un `receive` solo vale si
//!   el programa tiene el teclado y hace menos de `PEGAR_MS` que recibió una tecla o un clic (él u otro programa de la
//!   misma caja: así funciona `wl-paste` tecleado en una terminal de Linux). Un programa que se lleva el teclado al
//!   abrir una ventana y pide lo copiado sin que nadie pegue no lo recibe: la llave se cierra, la fuente no se entera y
//!   el compositor lo dice (`Escena::pegados_negados`). Lo de Android 10 (solo lee el portapapeles quien tiene el
//!   foco), más estricto. Las ofertas (los tipos) sí le llegan: no dicen lo copiado.
//!
//! Hostil: tipos de más (pasan `MAX_TIPOS`), largos (más de `MAX_TIPO` bytes) o repetidos se ignoran; un `receive` de
//! una oferta vieja o de un tipo que no se ofreció cierra la llave sin molestar a la fuente; las ofertas vivas de un
//! programa tienen tope (`MAX_OFERTAS`): uno que no las destruye deja de recibirlas, no llena el compositor.

use alloc::collections::BTreeMap;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;

use super::{Cliente, Entrada, Escena, PorCliente};
use crate::Valor;
use crate::servidor::{Conexion, ErrorProtocolo};
use crate::shm::Memorias;
use crate::tablas::{
    self, wl_data_device, wl_data_device_manager, wl_data_offer, wl_data_source, zwp_primary_selection_device_manager_v1,
    zwp_primary_selection_device_v1, zwp_primary_selection_offer_v1, zwp_primary_selection_source_v1,
};

/// Tipos por fuente: más se ignoran.
pub const MAX_TIPOS: usize = 32;
/// Lo más largo de un tipo (en bytes): los de verdad son cortos ("text/plain;charset=utf-8").
pub const MAX_TIPO: usize = 255;
/// Ofertas vivas por programa: más, y no se le mandan otras hasta que destruya las viejas.
pub const MAX_OFERTAS: usize = 32;
/// Fuentes vivas por programa.
pub const MAX_FUENTES: usize = 32;
/// S29: cuánto vale una tecla o un clic para pegar (en QEMU, con el PC cargado, de la tecla al `receive` de un programa
/// de Linux por el puente pasa más de un segundo).
pub const PEGAR_MS: u32 = 5_000;

/// El programa de DashOS del que viene un cliente: el mismo, o quien lleva su subconexión (la caja Linux).
pub fn grupo(c: Cliente) -> Cliente {
    c & ((1 << crate::canal::BITS_MARCA) - 1)
}

/// Cuál de las dos selecciones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tipo {
    /// Copiar y pegar (`wl_data_device`).
    Normal = 0,
    /// La primaria (`zwp_primary_selection_device_v1`).
    Primaria = 1,
}

/// Una selección puesta: de qué programa y de qué fuente, con sus tipos.
#[derive(Debug, PartialEq, Eq)]
pub struct Seleccion {
    pub cliente: Cliente,
    pub fuente: u32,
    pub tipos: Vec<String>,
}

/// Lo del portapapeles de cada programa.
#[derive(Default, Debug)]
pub struct DeCliente {
    /// Sus fuentes: objeto → (selección, tipos).
    pub fuentes: BTreeMap<u32, (Tipo, Vec<String>)>,
    /// Sus dispositivos (de las dos clases).
    pub dispositivos: Vec<(u32, Tipo)>,
    /// Las ofertas que se le mandaron: objeto → (selección, la que era).
    pub ofertas: BTreeMap<u32, (Tipo, Rc<Seleccion>)>,
    /// Lo que hay que ofrecerle en el próximo `Entrada::Portapapeles`, por selección: `Some(None)`, que no hay.
    pub por_ofrecer: [Option<Option<Rc<Seleccion>>>; 2],
    /// S29: cuándo recibió la última tecla o el último clic (la hora de `escribir_entrada`).
    pub pulsado_ms: Option<u32>,
}

/// Lo que el compositor manda a una fuente (quien atiende lo escribe en la conexión de su programa con
/// `escribir_aviso`, y cierra la llave si el programa ya no está).
#[derive(Debug, PartialEq, Eq)]
pub enum Aviso {
    /// Que escriba lo de `tipo` en `llave` y la cierre.
    Enviar { fuente: u32, sel: Tipo, tipo: String, llave: u32 },
    /// Ya no es la selección.
    Cancelar { fuente: u32, sel: Tipo },
}

impl Aviso {
    /// La llave que lleva, si lleva.
    pub fn llave(&self) -> Option<u32> {
        match self {
            Aviso::Enviar { llave, .. } => Some(*llave),
            Aviso::Cancelar { .. } => None,
        }
    }
}

/// Escribe un aviso del portapapeles en la conexión del programa de la fuente.
pub fn escribir_aviso(con: &mut Conexion, aviso: Aviso) {
    match aviso {
        Aviso::Enviar { fuente, sel, tipo, llave } => {
            let ev = if sel == Tipo::Normal { wl_data_source::ev::SEND } else { zwp_primary_selection_source_v1::ev::SEND };
            con.salida.empezar(fuente, ev).texto(Some(&tipo)).llave(llave).fin();
        }
        Aviso::Cancelar { fuente, sel } => {
            let ev = if sel == Tipo::Normal { wl_data_source::ev::CANCELLED } else { zwp_primary_selection_source_v1::ev::CANCELLED };
            con.salida.empezar(fuente, ev).fin();
        }
    }
}

/// Manda al programa las ofertas que tiene pendientes (`por_ofrecer`), en cada uno de sus dispositivos: `data_offer`,
/// los tipos y `selection`. Lo llama `escribir_entrada` con `Entrada::Portapapeles`, justo antes de darle el teclado.
pub fn escribir_ofertas(con: &mut Conexion, pp: &mut DeCliente) {
    for sel in [Tipo::Normal, Tipo::Primaria] {
        let Some(ofrecer) = pp.por_ofrecer[sel as usize].take() else { continue };
        let (oferta_i, ev_data_offer, ev_offer, ev_selection) = match sel {
            Tipo::Normal => (&tablas::WL_DATA_OFFER, wl_data_device::ev::DATA_OFFER, wl_data_offer::ev::OFFER, wl_data_device::ev::SELECTION),
            Tipo::Primaria => (
                &tablas::ZWP_PRIMARY_SELECTION_OFFER_V1,
                zwp_primary_selection_device_v1::ev::DATA_OFFER,
                zwp_primary_selection_offer_v1::ev::OFFER,
                zwp_primary_selection_device_v1::ev::SELECTION,
            ),
        };
        let dispositivos: Vec<u32> = pp.dispositivos.iter().filter(|d| d.1 == sel).map(|d| d.0).collect();
        for d in dispositivos {
            let Some(version) = con.objetos.get(&d).map(|o| o.version) else { continue };
            let oferta = match &ofrecer {
                Some(s) if pp.ofertas.len() < MAX_OFERTAS => {
                    let id = con.objeto_propio(oferta_i, version);
                    con.salida.empezar(d, ev_data_offer).uint(id).fin();
                    for t in &s.tipos {
                        con.salida.empezar(id, ev_offer).texto(Some(t)).fin();
                    }
                    pp.ofertas.insert(id, (sel, s.clone()));
                    id
                }
                _ => 0,
            };
            con.salida.empezar(d, ev_selection).uint(oferta).fin();
        }
    }
}

impl<M: Memorias> Escena<M> {
    /// La selección de ahora (`sel`).
    pub fn seleccion(&self, sel: Tipo) -> Option<&Seleccion> {
        self.selecciones[sel as usize].as_deref()
    }

    /// Prepara las ofertas para `cliente` (las dos selecciones) y añade el evento que las manda.
    pub(super) fn ofrecer(&mut self, cliente: Cliente, out: &mut Vec<(Cliente, Entrada)>) {
        let sel = [self.selecciones[0].clone(), self.selecciones[1].clone()];
        if let Some(c) = self.clientes.get_mut(&cliente) {
            if c.pp.dispositivos.is_empty() {
                return;
            }
            c.pp.por_ofrecer = [Some(sel[0].clone()), Some(sel[1].clone())];
            out.push((cliente, Entrada::Portapapeles));
        }
    }

    /// Cambia la selección `sel` (o la quita). La fuente de antes, si es otra, recibe `cancelled`; quien tiene el
    /// teclado, la oferta nueva.
    fn poner_seleccion(&mut self, sel: Tipo, nueva: Option<Seleccion>) {
        let vieja = core::mem::replace(&mut self.selecciones[sel as usize], nueva.map(Rc::new));
        if let Some(v) = vieja {
            let sigue = self.selecciones[sel as usize].as_ref().is_some_and(|n| n.cliente == v.cliente && n.fuente == v.fuente);
            let viva = self.clientes.get(&v.cliente).is_some_and(|c| c.pp.fuentes.contains_key(&v.fuente));
            if !sigue && viva {
                self.avisos_portapapeles.push((v.cliente, Aviso::Cancelar { fuente: v.fuente, sel }));
            }
        }
        if let Some((c, _)) = self.teclado {
            let mut ev = Vec::new();
            self.ofrecer(c, &mut ev);
            self.pendientes.extend(ev);
        }
    }

    /// Una fuente o un programa que ya no está: si era la selección, ya no hay.
    pub(super) fn olvidar_fuente(&mut self, cliente: Cliente, fuente: Option<u32>) {
        for sel in [Tipo::Normal, Tipo::Primaria] {
            if self.selecciones[sel as usize].as_ref().is_some_and(|s| s.cliente == cliente && fuente.is_none_or(|f| f == s.fuente)) {
                self.selecciones[sel as usize] = None;
                // A quien tiene el teclado (si es el programa que se va, ya no).
                if let Some((c, _)) = self.teclado.filter(|t| fuente.is_some() || t.0 != cliente) {
                    let mut ev = Vec::new();
                    self.ofrecer(c, &mut ev);
                    self.pendientes.extend(ev);
                }
            }
        }
        // Lo que se iba a mandar a ese programa ya no se manda (y sus llaves se cierran).
        if fuente.is_none() {
            let (fuera, quedan): (Vec<_>, Vec<_>) = core::mem::take(&mut self.avisos_portapapeles).into_iter().partition(|a| a.0 == cliente);
            self.avisos_portapapeles = quedan;
            for (_, a) in fuera {
                if let Some(l) = a.llave() {
                    self.memorias.soltar(l, core::ptr::null(), 0);
                }
            }
        }
    }
}

/// Un tipo que vale para guardarlo.
fn tipo_bueno(tipos: &[String], t: &str) -> bool {
    tipos.len() < MAX_TIPOS && !t.is_empty() && t.len() <= MAX_TIPO && !tipos.iter().any(|x| x == t)
}

impl<M: Memorias> PorCliente<'_, M> {
    /// Las peticiones del portapapeles. `None` si la interfaz no es suya.
    pub(super) fn portapapeles(&mut self, _con: &mut Conexion, interfaz: &str, objeto: u32, orden: u16, args: &[Valor]) -> Option<Result<(), ErrorProtocolo>> {
        let cliente = self.cliente;
        let escena = &mut *self.escena;
        let sel = match interfaz {
            "wl_data_device_manager" | "wl_data_source" | "wl_data_device" | "wl_data_offer" => Tipo::Normal,
            "zwp_primary_selection_device_manager_v1" | "zwp_primary_selection_source_v1" | "zwp_primary_selection_device_v1" | "zwp_primary_selection_offer_v1" => Tipo::Primaria,
            _ => return None,
        };
        let yo = escena.clientes.get_mut(&cliente)?;
        match (interfaz, orden) {
            ("wl_data_device_manager", wl_data_device_manager::req::CREATE_DATA_SOURCE)
            | ("zwp_primary_selection_device_manager_v1", zwp_primary_selection_device_manager_v1::req::CREATE_SOURCE) => {
                if yo.pp.fuentes.len() >= MAX_FUENTES {
                    return Some(Err(ErrorProtocolo::new(objeto, 0, "demasiadas fuentes del portapapeles")));
                }
                yo.pp.fuentes.insert(args[0].uint(), (sel, Vec::new()));
            }
            ("wl_data_device_manager", wl_data_device_manager::req::GET_DATA_DEVICE)
            | ("zwp_primary_selection_device_manager_v1", zwp_primary_selection_device_manager_v1::req::GET_DEVICE) => {
                if yo.pp.dispositivos.len() >= MAX_FUENTES {
                    return Some(Err(ErrorProtocolo::new(objeto, 0, "demasiados dispositivos del portapapeles")));
                }
                yo.pp.dispositivos.push((args[0].uint(), sel));
                // Si ya tiene el teclado, la selección de ahora.
                if escena.teclado.is_some_and(|t| t.0 == cliente) {
                    let mut ev = Vec::new();
                    escena.ofrecer(cliente, &mut ev);
                    escena.pendientes.extend(ev);
                }
            }
            ("wl_data_source", wl_data_source::req::OFFER) | ("zwp_primary_selection_source_v1", zwp_primary_selection_source_v1::req::OFFER) => {
                let t = args[0].texto();
                if let Some((_, tipos)) = yo.pp.fuentes.get_mut(&objeto)
                    && tipo_bueno(tipos, t)
                {
                    tipos.push(t.into());
                }
            }
            ("wl_data_device", wl_data_device::req::SET_SELECTION) | ("zwp_primary_selection_device_v1", zwp_primary_selection_device_v1::req::SET_SELECTION) => {
                let fuente = args[0].uint();
                // Solo quien tiene el teclado (la serie es la de una tecla o un clic suyo). A otro, su fuente se le
                // cancela: así no cree que la suya es la selección.
                if escena.teclado.is_none_or(|t| t.0 != cliente) {
                    if fuente != 0 && yo.pp.fuentes.contains_key(&fuente) {
                        escena.avisos_portapapeles.push((cliente, Aviso::Cancelar { fuente, sel }));
                    }
                    return Some(Ok(()));
                }
                let nueva = if fuente == 0 {
                    None
                } else {
                    match yo.pp.fuentes.get(&fuente) {
                        Some((s, tipos)) if *s == sel => Some(Seleccion { cliente, fuente, tipos: tipos.clone() }),
                        _ => return Some(Err(ErrorProtocolo::new(objeto, 0, "fuente que no es de esta selección"))),
                    }
                };
                escena.poner_seleccion(sel, nueva);
            }
            ("wl_data_device", wl_data_device::req::START_DRAG) => {
                // Sin arrastrar y soltar: la fuente se cancela en el acto.
                let fuente = args[0].uint();
                if fuente != 0 && yo.pp.fuentes.contains_key(&fuente) {
                    escena.avisos_portapapeles.push((cliente, Aviso::Cancelar { fuente, sel: Tipo::Normal }));
                }
            }
            ("wl_data_offer", wl_data_offer::req::RECEIVE) | ("zwp_primary_selection_offer_v1", zwp_primary_selection_offer_v1::req::RECEIVE) => {
                let (t, llave) = (args[0].texto(), args[1].uint());
                // Solo de la selección de ahora y de un tipo que ofrece; si no, la llave se cierra (quien pega ve el
                // fin en el acto).
                let actual = escena.selecciones[sel as usize].clone();
                // S29: con el teclado y una tecla o un clic reciente suyo (o de su caja).
                let con_teclado = escena.teclado.is_some_and(|t| t.0 == cliente);
                let ahora = escena.reloj_ms;
                let reciente = escena
                    .clientes
                    .iter()
                    .filter(|(c, _)| grupo(**c) == grupo(cliente))
                    .filter_map(|(_, d)| d.pp.pulsado_ms)
                    .any(|t| ahora.wrapping_sub(t) <= PEGAR_MS);
                if escena.pegar_con_entrada && !(con_teclado && reciente) {
                    escena.memorias.soltar(llave, core::ptr::null(), 0);
                    if !escena.pegados_negados.contains(&cliente) {
                        escena.pegados_negados.push(cliente);
                    }
                    return Some(Ok(()));
                }
                let yo = escena.clientes.get_mut(&cliente)?;
                let buena = yo.pp.ofertas.get(&objeto).zip(actual.as_ref()).filter(|((_, vista), a)| Rc::ptr_eq(vista, a)).map(|(_, a)| a.clone());
                match buena {
                    Some(s) if s.tipos.iter().any(|x| x == t) && escena.clientes.contains_key(&s.cliente) => {
                        escena.avisos_portapapeles.push((s.cliente, Aviso::Enviar { fuente: s.fuente, sel, tipo: t.into(), llave }));
                    }
                    _ => escena.memorias.soltar(llave, core::ptr::null(), 0),
                }
            }
            // accept, finish, set_actions: de arrastrar y soltar; release y destroy, en `destruido`.
            _ => {}
        }
        Some(Ok(()))
    }

    /// Un objeto del portapapeles destruido.
    pub(super) fn portapapeles_destruido(&mut self, objeto: u32) {
        let cliente = self.cliente;
        let escena = &mut *self.escena;
        let Some(yo) = escena.clientes.get_mut(&cliente) else { return };
        yo.pp.dispositivos.retain(|d| d.0 != objeto);
        yo.pp.ofertas.remove(&objeto);
        if yo.pp.fuentes.remove(&objeto).is_some() {
            escena.olvidar_fuente(cliente, Some(objeto));
        }
    }
}
