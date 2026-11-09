//! Wayland para DashOS (W1): el formato de los mensajes, sin bibliotecas de C ni sockets de Unix.
//!
//! Un mensaje de Wayland es: el objeto (u32), el tamaño en bytes y el número de la orden (u32: tamaño << 16 | orden),
//! y los argumentos, cada uno en múltiplos de 4 bytes y en el orden de bytes de la máquina (little-endian en DashOS).
//! Los descriptores de fichero no van en los bytes: en DashOS son llaves y viajan con el mensaje del canal, en orden.
//! Las firmas de cada mensaje salen de los XML oficiales (`protocolos/`, MIT), por `tools/wayland_tablas.py`.
//!
//! Sirve al compositor y a los clientes: el que escribe usa `Escritor`, el que lee `leer_cabecera` y `leer_args` con
//! la firma del mensaje.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;

pub mod canal;
pub mod servidor;
pub mod tablas;

/// El tipo de un argumento.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Int,
    Uint,
    /// Número con signo de 24.8 bits.
    Fixed,
    Str,
    Object,
    /// Un objeto nuevo. Sin interfaz en la firma (`wl_registry.bind`), lleva antes su interfaz y su versión.
    NewId,
    Array,
    /// Un descriptor de fichero: en DashOS, una llave.
    Fd,
}

#[derive(Debug)]
pub struct Arg {
    pub name: &'static str,
    pub kind: Kind,
    pub nullable: bool,
    pub interface: Option<&'static str>,
}

#[derive(Debug)]
pub struct Message {
    pub name: &'static str,
    pub since: u32,
    pub destructor: bool,
    pub args: &'static [Arg],
}

#[derive(Debug)]
pub struct Interface {
    pub name: &'static str,
    pub version: u32,
    pub requests: &'static [Message],
    pub events: &'static [Message],
}

/// La interfaz que se llama `name`.
pub fn interfaz(name: &str) -> Option<&'static Interface> {
    tablas::INTERFACES.iter().copied().find(|i| i.name == name)
}

/// Un argumento leído.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Valor<'a> {
    Int(i32),
    Uint(u32),
    Fixed(i32),
    Str(Option<&'a str>),
    Object(u32),
    NewId(u32),
    /// Un objeto nuevo de una interfaz que dice el mensaje (`wl_registry.bind`).
    NewIdDe { interface: &'a str, version: u32, id: u32 },
    Array(&'a [u8]),
    /// Una llave de DashOS.
    Fd(u32),
}

impl Valor<'_> {
    pub fn uint(&self) -> u32 {
        match *self {
            Valor::Uint(v) | Valor::Object(v) | Valor::NewId(v) | Valor::Fd(v) => v,
            Valor::Int(v) | Valor::Fixed(v) => v as u32,
            Valor::NewIdDe { id, .. } => id,
            _ => 0,
        }
    }

    pub fn int(&self) -> i32 {
        self.uint() as i32
    }
}

/// Lo que puede estar mal en un mensaje.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fallo {
    /// Más corto que su cabecera o que lo que dicen sus argumentos.
    Corto,
    /// Tamaño que no es múltiplo de 4, o menor que la cabecera.
    Tamano,
    /// Un texto sin el 0 final, que no es UTF-8, o nulo sin permitirlo.
    Texto,
    /// Un objeto nulo donde no se permite.
    Nulo,
    /// Faltan llaves para sus descriptores.
    SinLlave,
}

/// La cabecera de un mensaje: objeto, orden y tamaño (con la cabecera).
pub fn leer_cabecera(bytes: &[u8]) -> Result<(u32, u16, usize), Fallo> {
    if bytes.len() < 8 {
        return Err(Fallo::Corto);
    }
    let objeto = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    let segundo = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let tamano = (segundo >> 16) as usize;
    if tamano < 8 || tamano % 4 != 0 {
        return Err(Fallo::Tamano);
    }
    if tamano > bytes.len() {
        return Err(Fallo::Corto);
    }
    Ok((objeto, segundo as u16, tamano))
}

/// Los argumentos de un mensaje (sin la cabecera) según su firma. Las llaves de sus descriptores se toman de
/// `llaves`, en orden.
pub fn leer_args<'a>(
    mensaje: &Message,
    mut cuerpo: &'a [u8],
    llaves: &mut impl Iterator<Item = u32>,
) -> Result<Vec<Valor<'a>>, Fallo> {
    fn palabra(cuerpo: &mut &[u8]) -> Result<u32, Fallo> {
        if cuerpo.len() < 4 {
            return Err(Fallo::Corto);
        }
        let v = u32::from_le_bytes(cuerpo[..4].try_into().unwrap());
        *cuerpo = &cuerpo[4..];
        Ok(v)
    }
    fn bloque<'a>(cuerpo: &mut &'a [u8], largo: usize) -> Result<&'a [u8], Fallo> {
        let relleno = largo.div_ceil(4) * 4;
        if cuerpo.len() < relleno {
            return Err(Fallo::Corto);
        }
        let b = &cuerpo[..largo];
        *cuerpo = &cuerpo[relleno..];
        Ok(b)
    }
    fn texto<'a>(cuerpo: &mut &'a [u8], nullable: bool) -> Result<Option<&'a str>, Fallo> {
        let largo = palabra(cuerpo)? as usize;
        if largo == 0 {
            return if nullable { Ok(None) } else { Err(Fallo::Texto) };
        }
        let b = bloque(cuerpo, largo)?;
        if b[largo - 1] != 0 {
            return Err(Fallo::Texto);
        }
        core::str::from_utf8(&b[..largo - 1]).map(Some).map_err(|_| Fallo::Texto)
    }

    let mut out = Vec::with_capacity(mensaje.args.len());
    for arg in mensaje.args {
        let v = match arg.kind {
            Kind::Int => Valor::Int(palabra(&mut cuerpo)? as i32),
            Kind::Uint => Valor::Uint(palabra(&mut cuerpo)?),
            Kind::Fixed => Valor::Fixed(palabra(&mut cuerpo)? as i32),
            Kind::Str => Valor::Str(texto(&mut cuerpo, arg.nullable)?),
            Kind::Object => {
                let id = palabra(&mut cuerpo)?;
                if id == 0 && !arg.nullable {
                    return Err(Fallo::Nulo);
                }
                Valor::Object(id)
            }
            Kind::NewId if arg.interface.is_none() => {
                let interface = texto(&mut cuerpo, false)?.unwrap_or("");
                let version = palabra(&mut cuerpo)?;
                let id = palabra(&mut cuerpo)?;
                Valor::NewIdDe { interface, version, id }
            }
            Kind::NewId => {
                let id = palabra(&mut cuerpo)?;
                if id == 0 {
                    return Err(Fallo::Nulo);
                }
                Valor::NewId(id)
            }
            Kind::Array => {
                let largo = palabra(&mut cuerpo)? as usize;
                Valor::Array(bloque(&mut cuerpo, largo)?)
            }
            Kind::Fd => Valor::Fd(llaves.next().ok_or(Fallo::SinLlave)?),
        };
        out.push(v);
    }
    if !cuerpo.is_empty() {
        // Sobran bytes: el mensaje no es el que dice su firma.
        return Err(Fallo::Tamano);
    }
    Ok(out)
}

/// Escribe mensajes seguidos en un mismo búfer, con sus llaves aparte.
#[derive(Default)]
pub struct Escritor {
    pub bytes: Vec<u8>,
    pub llaves: Vec<u32>,
    /// Cuántas llaves usa cada mensaje, en orden: para trocear el flujo (`canal::trocear`).
    pub cuentas: Vec<usize>,
    inicio: usize,
    llaves_al_empezar: usize,
}

impl Escritor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Empieza un mensaje para `objeto` con la orden `orden`; se cierra con `fin`.
    pub fn empezar(&mut self, objeto: u32, orden: u16) -> &mut Self {
        self.inicio = self.bytes.len();
        self.llaves_al_empezar = self.llaves.len();
        self.bytes.extend_from_slice(&objeto.to_le_bytes());
        self.bytes.extend_from_slice(&(orden as u32).to_le_bytes());
        self
    }

    pub fn uint(&mut self, v: u32) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub fn int(&mut self, v: i32) -> &mut Self {
        self.uint(v as u32)
    }

    /// Un número de 24.8 bits desde un entero.
    pub fn fixed(&mut self, v: i32) -> &mut Self {
        self.int(v.wrapping_mul(256))
    }

    pub fn texto(&mut self, v: Option<&str>) -> &mut Self {
        match v {
            None => self.uint(0),
            Some(t) => {
                self.uint(t.len() as u32 + 1);
                self.bytes.extend_from_slice(t.as_bytes());
                self.bytes.push(0);
                self.rellenar()
            }
        }
    }

    pub fn array(&mut self, v: &[u8]) -> &mut Self {
        self.uint(v.len() as u32);
        self.bytes.extend_from_slice(v);
        self.rellenar()
    }

    /// Un descriptor: la llave va aparte, nada en los bytes.
    pub fn llave(&mut self, llave: u32) -> &mut Self {
        self.llaves.push(llave);
        self
    }

    fn rellenar(&mut self) -> &mut Self {
        while self.bytes.len() % 4 != 0 {
            self.bytes.push(0);
        }
        self
    }

    /// Pone el tamaño en la cabecera del mensaje empezado.
    pub fn fin(&mut self) {
        let tamano = (self.bytes.len() - self.inicio) as u32;
        let i = self.inicio + 4;
        let orden = u32::from_le_bytes(self.bytes[i..i + 4].try_into().unwrap()) & 0xFFFF;
        self.bytes[i..i + 4].copy_from_slice(&(tamano << 16 | orden).to_le_bytes());
        self.cuentas.push(self.llaves.len() - self.llaves_al_empezar);
    }

    pub fn vaciar(&mut self) {
        self.bytes.clear();
        self.llaves.clear();
        self.cuentas.clear();
    }
}

#[cfg(test)]
mod tests;
