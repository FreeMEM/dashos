//! El transporte de Wayland sobre canales de DashOS (W2).
//!
//! Wayland es un flujo de bytes con descriptores intercalados, como un socket de Unix. Un mensaje de canal lleva como
//! mucho 256 bytes y 8 llaves, así que el flujo va en trozos: el primer byte de cada trozo dice qué es (`TROZO_*`) y
//! el resto son bytes del flujo. Quien escribe junta mensajes enteros en cada trozo mientras quepan, y parte solo el
//! que no cabe en uno; las llaves de un mensaje van en su primer trozo. Así una llave nunca llega después del
//! mensaje que la usa, y quien lee puede leer cada mensaje en cuanto tiene todos sus bytes.
//!
//! La conexión: el cliente crea un canal, se queda un extremo y manda el otro al compositor en un trozo
//! `TROZO_CONECTAR`, por la llave marcada que le dio `inicio`. Desde ahí, lo suyo va por esa llave marcada (la marca
//! dice quién es, S10b) y lo del compositor, por el canal que mandó.

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use crate::{Escritor, Fallo, leer_cabecera};

/// Bytes de un mensaje de canal.
pub const MAX_CANAL: usize = 256;
/// Llaves de un mensaje de canal.
pub const MAX_LLAVES: usize = 8;
/// Bytes del flujo en un trozo: el resto, tras su byte de tipo.
pub const MAX_TROZO: usize = MAX_CANAL - 1;

/// Bytes del flujo de Wayland.
pub const TROZO_DATOS: u8 = 1;
/// Abre la conexión: lleva una llave, el canal por el que contestar.
pub const TROZO_CONECTAR: u8 = 2;
/// Cierra la conexión.
pub const TROZO_ADIOS: u8 = 3;

/// Un trozo listo para mandar: sus bytes (con el tipo delante) y sus llaves.
#[derive(Debug, PartialEq, Eq)]
pub struct Trozo {
    pub bytes: Vec<u8>,
    pub llaves: Vec<u32>,
}

/// Parte los mensajes de un `Escritor` en trozos, con las llaves de cada mensaje en su primer trozo.
pub fn trocear(e: &Escritor) -> Result<Vec<Trozo>, Fallo> {
    let bytes = &e.bytes[..];
    let mut trozos = Vec::new();
    let mut actual = Trozo { bytes: alloc::vec![TROZO_DATOS], llaves: Vec::new() };
    let mut llaves = e.llaves.iter().copied();
    let mut cuentas = e.cuentas.iter().copied();
    let mut i = 0;
    while i < bytes.len() {
        let (_, _, tam) = leer_cabecera(&bytes[i..])?;
        let mensaje = &bytes[i..i + tam];
        let suyas: Vec<u32> = llaves.by_ref().take(cuentas.next().unwrap_or(0)).collect();
        if suyas.len() > MAX_LLAVES {
            return Err(Fallo::SinLlave);
        }
        // Entero en el trozo actual, si cabe; si no, en uno nuevo; si no cabe en ninguno, partido.
        if actual.bytes.len() + tam > MAX_CANAL || actual.llaves.len() + suyas.len() > MAX_LLAVES {
            trozos.push(core::mem::replace(
                &mut actual,
                Trozo { bytes: alloc::vec![TROZO_DATOS], llaves: Vec::new() },
            ));
        }
        actual.llaves.extend(suyas);
        let mut resto = mensaje;
        while !resto.is_empty() {
            let cabe = MAX_CANAL - actual.bytes.len();
            if cabe == 0 {
                trozos.push(core::mem::replace(
                    &mut actual,
                    Trozo { bytes: alloc::vec![TROZO_DATOS], llaves: Vec::new() },
                ));
                continue;
            }
            let n = cabe.min(resto.len());
            actual.bytes.extend_from_slice(&resto[..n]);
            resto = &resto[n..];
        }
        i += tam;
    }
    if actual.bytes.len() > 1 || !actual.llaves.is_empty() {
        trozos.push(actual);
    }
    Ok(trozos)
}

/// Un mensaje de Wayland recibido entero: su objeto, su orden y su cuerpo (sin la cabecera).
#[derive(Debug, PartialEq, Eq)]
pub struct Recibido {
    pub objeto: u32,
    pub orden: u16,
    pub cuerpo: Vec<u8>,
}

/// Junta los trozos de una conexión y da los mensajes enteros.
#[derive(Default)]
pub struct Juntador {
    bytes: Vec<u8>,
    pub llaves: VecDeque<u32>,
}

/// Lo más que se guarda de una conexión sin un mensaje entero: más es un cliente roto o malicioso.
pub const MAX_PENDIENTE: usize = 64 * 1024;

impl Juntador {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mete los bytes de un trozo de datos (sin su byte de tipo) y sus llaves.
    pub fn meter(&mut self, bytes: &[u8], llaves: &[u32]) -> Result<(), Fallo> {
        if self.bytes.len() + bytes.len() > MAX_PENDIENTE {
            return Err(Fallo::Tamano);
        }
        self.bytes.extend_from_slice(bytes);
        self.llaves.extend(llaves.iter().copied());
        Ok(())
    }

    /// El siguiente mensaje entero, si ha llegado.
    pub fn siguiente(&mut self) -> Result<Option<Recibido>, Fallo> {
        match leer_cabecera(&self.bytes) {
            Err(Fallo::Corto) => Ok(None),
            Err(e) => Err(e),
            Ok((objeto, orden, tam)) => {
                let cuerpo = self.bytes[8..tam].to_vec();
                self.bytes.drain(..tam);
                Ok(Some(Recibido { objeto, orden, cuerpo }))
            }
        }
    }

    /// Las llaves que esperan, para leer los argumentos de un mensaje.
    pub fn tomar_llave(&mut self) -> Option<u32> {
        self.llaves.pop_front()
    }
}
