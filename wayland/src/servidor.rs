//! El lado del compositor de una conexión de Wayland (W3): los objetos de cada cliente y lo que es igual para todos
//! los compositores. `wl_display` (sync, get_registry, errores), `wl_registry` (globales y bind), versiones, objetos
//! nuevos y destructores. Lo propio de cada interfaz lo hace quien implemente `Servicio`.
//!
//! Las reglas, de `wayland.xml`: un objeto nuevo creado por una petición tiene la versión del objeto que lo creó; una
//! petición más nueva que la versión del objeto es un error; los números que pone el cliente van de 1 a 0xFEFFFFFF y
//! los del servidor desde 0xFF000000; al destruir un objeto del cliente, el servidor responde `delete_id`. Un error
//! de protocolo manda `wl_display.error` y la conexión se corta.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::canal::Juntador;
use crate::tablas::{self, wl_callback, wl_display, wl_registry};
use crate::{Escritor, Interface, Kind, Valor, leer_args};

/// Primer número de los objetos que crea el servidor.
pub const PRIMERO_DEL_SERVIDOR: u32 = 0xFF00_0000;

/// Los errores de `wl_display`.
pub const INVALID_OBJECT: u32 = 0;
pub const INVALID_METHOD: u32 = 1;
pub const NO_MEMORY: u32 = 2;
pub const IMPLEMENTATION: u32 = 3;

/// Un global que se anuncia: su nombre (número), su interfaz y la versión más alta que se sirve.
#[derive(Clone, Copy, Debug)]
pub struct Global {
    pub nombre: u32,
    pub interfaz: &'static Interface,
    pub version: u32,
}

/// Un error de protocolo: el objeto, el código (de la interfaz del objeto) y un texto.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorProtocolo {
    pub objeto: u32,
    pub codigo: u32,
    pub texto: String,
}

impl ErrorProtocolo {
    pub fn new(objeto: u32, codigo: u32, texto: &str) -> Self {
        Self { objeto, codigo, texto: texto.into() }
    }
}

/// Un objeto vivo de una conexión.
#[derive(Clone, Copy, Debug)]
pub struct Objeto {
    pub interfaz: &'static Interface,
    pub version: u32,
}

/// Lo propio de cada compositor: lo llama `Conexion::atender` con cada petición que no es de `wl_display` ni de
/// `wl_registry`, ya leída y comprobada. Los objetos nuevos de la petición ya están en la tabla.
pub trait Servicio {
    fn peticion(
        &mut self,
        con: &mut Conexion,
        objeto: u32,
        orden: u16,
        args: &[Valor],
    ) -> Result<(), ErrorProtocolo>;

    /// Un cliente enlazó un global (`wl_registry.bind`): `id` es el objeto nuevo. Por ejemplo, `wl_shm` anuncia aquí
    /// sus formatos.
    fn enlazado(&mut self, _con: &mut Conexion, _global: &Global, _id: u32) {}

    /// Un objeto se destruyó (por su petición destructora): el servicio suelta lo suyo.
    fn destruido(&mut self, _con: &mut Conexion, _objeto: u32) {}
}

/// Una conexión: sus objetos, lo que llega a medias y lo que hay que mandarle.
pub struct Conexion {
    pub objetos: BTreeMap<u32, Objeto>,
    pub juntador: Juntador,
    pub salida: Escritor,
    /// Se cortó por un error de protocolo: no se atiende más.
    pub roto: Option<ErrorProtocolo>,
    siguiente_propio: u32,
    pub serie: u32,
}

impl Default for Conexion {
    fn default() -> Self {
        let mut objetos = BTreeMap::new();
        objetos.insert(1, Objeto { interfaz: &tablas::WL_DISPLAY, version: 1 });
        Self {
            objetos,
            juntador: Juntador::new(),
            salida: Escritor::new(),
            roto: None,
            siguiente_propio: PRIMERO_DEL_SERVIDOR,
            serie: 0,
        }
    }
}

impl Conexion {
    pub fn new() -> Self {
        Self::default()
    }

    /// Un número de serie nuevo (para configure, ping, eventos de entrada).
    pub fn serie_nueva(&mut self) -> u32 {
        self.serie = self.serie.wrapping_add(1);
        self.serie
    }

    /// Crea un objeto del servidor (por ejemplo, el `wl_data_offer` que se manda en un evento).
    pub fn objeto_propio(&mut self, interfaz: &'static Interface, version: u32) -> u32 {
        let id = self.siguiente_propio;
        self.siguiente_propio += 1;
        self.objetos.insert(id, Objeto { interfaz, version });
        id
    }

    /// La interfaz de un objeto vivo.
    pub fn interfaz(&self, objeto: u32) -> Option<&'static Interface> {
        self.objetos.get(&objeto).map(|o| o.interfaz)
    }

    /// Atiende todos los mensajes enteros que hayan llegado. Tras un error de protocolo, deja el evento `error` en
    /// la salida y no atiende más: quien lo llama manda la salida y cierra.
    pub fn atender(&mut self, globales: &[Global], servicio: &mut impl Servicio) {
        while self.roto.is_none() {
            let m = match self.juntador.siguiente() {
                Ok(Some(m)) => m,
                Ok(None) => break,
                Err(_) => {
                    self.romper(ErrorProtocolo::new(1, INVALID_METHOD, "mensaje mal formado"));
                    break;
                }
            };
            if let Err(e) = self.una(m.objeto, m.orden, &m.cuerpo, globales, servicio) {
                self.romper(e);
            }
        }
    }

    fn romper(&mut self, e: ErrorProtocolo) {
        self.salida
            .empezar(1, wl_display::ev::ERROR)
            .uint(e.objeto)
            .uint(e.codigo)
            .texto(Some(&e.texto))
            .fin();
        self.roto = Some(e);
    }

    fn una(
        &mut self,
        objeto: u32,
        orden: u16,
        cuerpo: &[u8],
        globales: &[Global],
        servicio: &mut impl Servicio,
    ) -> Result<(), ErrorProtocolo> {
        let Some(&Objeto { interfaz, version }) = self.objetos.get(&objeto) else {
            return Err(ErrorProtocolo::new(1, INVALID_OBJECT, "objeto que no existe"));
        };
        let Some(mensaje) = interfaz.requests.get(orden as usize) else {
            return Err(ErrorProtocolo::new(objeto, INVALID_METHOD, "petición que no existe"));
        };
        if mensaje.since > version {
            return Err(ErrorProtocolo::new(objeto, INVALID_METHOD, "petición más nueva que el objeto"));
        }
        let juntador = &mut self.juntador;
        let args = leer_args(mensaje, cuerpo, &mut core::iter::from_fn(|| juntador.tomar_llave()))
            .map_err(|_| ErrorProtocolo::new(1, INVALID_METHOD, "argumentos mal formados"))?;

        // Los objetos que la petición crea, con la versión del que los crea.
        for (arg, valor) in mensaje.args.iter().zip(&args) {
            if let (Kind::NewId, Valor::NewId(id), Some(nombre)) = (arg.kind, valor, arg.interface) {
                self.nuevo(*id, nombre, version)?;
            }
        }

        if core::ptr::eq(interfaz, &tablas::WL_DISPLAY) {
            match orden {
                wl_display::req::SYNC => {
                    let id = args[0].uint();
                    let serie = self.serie_nueva();
                    self.salida.empezar(id, wl_callback::ev::DONE).uint(serie).fin();
                    self.quitar(id);
                }
                _ => {
                    // get_registry: los globales, uno por evento.
                    let id = args[0].uint();
                    for g in globales {
                        self.salida
                            .empezar(id, wl_registry::ev::GLOBAL)
                            .uint(g.nombre)
                            .texto(Some(g.interfaz.name))
                            .uint(g.version)
                            .fin();
                    }
                }
            }
            return Ok(());
        }
        if core::ptr::eq(interfaz, &tablas::WL_REGISTRY) {
            let nombre = args[0].uint();
            let Valor::NewIdDe { interface, version: pedida, id } = args[1] else {
                return Err(ErrorProtocolo::new(objeto, INVALID_METHOD, "bind sin objeto"));
            };
            let Some(g) = globales.iter().find(|g| g.nombre == nombre) else {
                return Err(ErrorProtocolo::new(objeto, INVALID_METHOD, "global que no existe"));
            };
            if g.interfaz.name != interface || pedida == 0 || pedida > g.version {
                return Err(ErrorProtocolo::new(objeto, INVALID_METHOD, "bind con otra interfaz o versión"));
            }
            self.nuevo(id, interface, pedida)?;
            let g = *g;
            servicio.enlazado(self, &g, id);
            return Ok(());
        }

        servicio.peticion(self, objeto, orden, &args)?;
        if mensaje.destructor && self.objetos.contains_key(&objeto) {
            self.quitar(objeto);
            servicio.destruido(self, objeto);
        }
        Ok(())
    }

    fn nuevo(&mut self, id: u32, interfaz: &str, version: u32) -> Result<(), ErrorProtocolo> {
        let Some(i) = crate::interfaz(interfaz) else {
            return Err(ErrorProtocolo::new(1, IMPLEMENTATION, "interfaz sin servir"));
        };
        if id == 0 || id >= PRIMERO_DEL_SERVIDOR || self.objetos.contains_key(&id) {
            return Err(ErrorProtocolo::new(1, INVALID_OBJECT, "número de objeto nuevo malo"));
        }
        self.objetos.insert(id, Objeto { interfaz: i, version });
        Ok(())
    }

    /// Quita un objeto; si era del cliente, se lo dice con `delete_id` para que pueda usar el número otra vez.
    pub fn quitar(&mut self, id: u32) {
        if self.objetos.remove(&id).is_some() && id < PRIMERO_DEL_SERVIDOR {
            self.salida.empezar(1, wl_display::ev::DELETE_ID).uint(id).fin();
        }
    }

    /// Mensajes que quedan para el cliente, y vacía la salida.
    pub fn sacar(&mut self) -> Escritor {
        core::mem::take(&mut self.salida)
    }
}

/// Lee los eventos que el servidor mandó (para pruebas y clientes): objeto, orden y cuerpo de cada uno.
pub fn eventos(e: &Escritor) -> Vec<(u32, u16, Vec<u8>)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < e.bytes.len() {
        let (o, orden, tam) = crate::leer_cabecera(&e.bytes[i..]).unwrap();
        out.push((o, orden, e.bytes[i + 8..i + tam].to_vec()));
        i += tam;
    }
    out
}
