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

/// Subconexiones (la caja Linux, H9): un proceso con una sola marca lleva varios clientes de Wayland (los programas de
/// Linux que pasan por su puente), cada uno con su número. Tras el byte de tipo, el número (u32 little-endian) y lo
/// demás como en los trozos de siempre. Solo para los programas a los que el compositor se lo permite por su nombre.
/// Abre la subconexión: una llave, el canal por el que contestar (puede ser el mismo para todas: las respuestas
/// llevan también el número).
pub const TROZO_SUB_CONECTAR: u8 = 4;
/// Bytes de una subconexión (y sus llaves), en los dos sentidos.
pub const TROZO_SUB_DATOS: u8 = 5;
/// Cierra una subconexión. El compositor también lo manda por el canal de respuesta cuando la suelta.
pub const TROZO_SUB_ADIOS: u8 = 6;
/// Bytes de la cabecera de un trozo de subconexión: tipo y número.
pub const CABECERA_SUB: usize = 5;
/// Un cliente de una subconexión es la marca de quien la lleva (en los 40 bits bajos) con el número de la subconexión más
/// 1 encima (lo pone el compositor). Así se sabe de qué programa de DashOS viene cada cliente (S29: el portapapeles).
pub const BITS_MARCA: u32 = 40;

/// La cabecera de los trozos de datos de la subconexión `sub`.
pub fn cabecera_sub(tipo: u8, sub: u32) -> [u8; CABECERA_SUB] {
    let n = sub.to_le_bytes();
    [tipo, n[0], n[1], n[2], n[3]]
}

/// El número de subconexión de un trozo de subconexión, y lo que va detrás.
pub fn partir_sub(trozo: &[u8]) -> Option<(u32, &[u8])> {
    let n = u32::from_le_bytes(trozo.get(1..5)?.try_into().ok()?);
    Some((n, &trozo[5..]))
}

/// Un trozo listo para mandar: sus bytes (con el tipo delante) y sus llaves.
#[derive(Debug, PartialEq, Eq)]
pub struct Trozo {
    pub bytes: Vec<u8>,
    pub llaves: Vec<u32>,
}

/// Parte los mensajes de un `Escritor` en trozos, con las llaves de cada mensaje en su primer trozo.
pub fn trocear(e: &Escritor) -> Result<Vec<Trozo>, Fallo> {
    trocear_con(e, &[TROZO_DATOS])
}

/// Como `trocear`, con `cabecera` delante de cada trozo en vez del byte de tipo (las subconexiones llevan también su
/// número).
pub fn trocear_con(e: &Escritor, cabecera: &[u8]) -> Result<Vec<Trozo>, Fallo> {
    let bytes = &e.bytes[..];
    let mut trozos = Vec::new();
    let nuevo = || Trozo { bytes: cabecera.to_vec(), llaves: Vec::new() };
    let mut actual = nuevo();
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
            trozos.push(core::mem::replace(&mut actual, nuevo()));
        }
        actual.llaves.extend(suyas);
        let mut resto = mensaje;
        while !resto.is_empty() {
            let cabe = MAX_CANAL - actual.bytes.len();
            if cabe == 0 {
                trozos.push(core::mem::replace(&mut actual, nuevo()));
                continue;
            }
            let n = cabe.min(resto.len());
            actual.bytes.extend_from_slice(&resto[..n]);
            resto = &resto[n..];
        }
        i += tam;
    }
    if actual.bytes.len() > cabecera.len() || !actual.llaves.is_empty() {
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
/// Lo más de llaves que esperan a un mensaje que las use (E2). Ningún mensaje de Wayland lleva más de unas pocas; sin
/// tope, un cliente llenaría la tabla de llaves del servidor, que dejaría de poder leer.
pub const MAX_LLAVES_PENDIENTES: usize = 32;

impl Juntador {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mete los bytes de un trozo de datos (sin su byte de tipo) y sus llaves. Si no caben, no se queda nada: las
    /// llaves siguen siendo de quien llama.
    pub fn meter(&mut self, bytes: &[u8], llaves: &[u32]) -> Result<(), Fallo> {
        if self.bytes.len() + bytes.len() > MAX_PENDIENTE {
            return Err(Fallo::Tamano);
        }
        if self.llaves.len() + llaves.len() > MAX_LLAVES_PENDIENTES {
            return Err(Fallo::Llaves);
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

/// Lo más que se guarda para un cliente que no lee, en bytes (E8): más, y se le suelta.
pub const MAX_COLA: usize = 64 * 1024;

/// Lo que el compositor tiene por mandar a un cliente cuya cola de canal está llena (E8). En vez de esperarle (y parar
/// a todos mientras), se guarda y se reintenta en cada vuelta; se le suelta si pasa de `MAX_COLA` o si lleva más de su
/// plazo sin leer nada.
#[derive(Debug, Default)]
pub struct ColaSalida {
    trozos: VecDeque<Trozo>,
    bytes: usize,
    /// Desde cuándo (ms) no ha leído nada, si hay algo esperando.
    parado: Option<u32>,
}

impl ColaSalida {
    pub fn meter(&mut self, trozos: Vec<Trozo>) {
        for t in trozos {
            self.bytes += t.bytes.len();
            self.trozos.push_back(t);
        }
    }

    pub fn pendiente(&self) -> bool {
        !self.trozos.is_empty()
    }

    /// Manda lo que pueda, en orden, con `escribir` (que dice si cupo). `false` si hay que soltar al cliente: guarda
    /// más de `MAX_COLA` o lleva `plazo_ms` sin leer.
    pub fn vaciar(&mut self, ahora_ms: u32, plazo_ms: u32, mut escribir: impl FnMut(&Trozo) -> bool) -> bool {
        let mut alguno = false;
        while let Some(t) = self.trozos.front() {
            if !escribir(t) {
                break;
            }
            self.bytes -= t.bytes.len();
            self.trozos.pop_front();
            alguno = true;
        }
        if self.trozos.is_empty() {
            self.parado = None;
            return true;
        }
        if alguno || self.parado.is_none() {
            self.parado = Some(ahora_ms);
        }
        self.bytes <= MAX_COLA && ahora_ms.wrapping_sub(self.parado.unwrap()) <= plazo_ms
    }

    /// Las llaves de lo que no llegó a mandarse (al soltar al cliente, para cerrarlas).
    pub fn llaves(&mut self) -> impl Iterator<Item = u32> + '_ {
        self.bytes = 0;
        self.parado = None;
        self.trozos.drain(..).flat_map(|t| t.llaves)
    }
}

#[cfg(test)]
mod tests_cola {
    use super::*;
    use alloc::vec;

    fn trozo(n: usize, llave: Option<u32>) -> Trozo {
        Trozo { bytes: vec![TROZO_DATOS; n], llaves: llave.into_iter().collect() }
    }

    /// E8: un cliente que lee tarde no para a nadie ni pierde nada: lo suyo espera en orden. Uno que lee poco a poco
    /// sigue; uno que no lee nada en su plazo, o que deja más de `MAX_COLA`, se suelta, y sus llaves salen para
    /// cerrarlas.
    #[test]
    fn cola_de_salida() {
        let mut c = ColaSalida::default();
        let mut llegado: Vec<usize> = Vec::new();
        // Cabe todo: nada pendiente.
        c.meter(vec![trozo(10, None), trozo(20, None)]);
        assert!(c.vaciar(0, 100, |t| { llegado.push(t.bytes.len()); true }));
        assert!(!c.pendiente());
        // Cola llena: espera, y el orden se guarda.
        c.meter(vec![trozo(30, Some(7)), trozo(40, None)]);
        assert!(c.vaciar(10, 100, |_| false));
        assert!(c.pendiente());
        c.meter(vec![trozo(50, None)]);
        // Lee uno de vez en cuando: no se le suelta aunque pase el plazo desde el principio.
        let mut uno = true;
        assert!(c.vaciar(90, 100, |t| { let r = uno; uno = false; if r { llegado.push(t.bytes.len()) } r }));
        assert!(c.vaciar(180, 100, |_| false));
        // Ya no lee: a los 100 ms de lo último, fuera.
        assert!(c.vaciar(190, 100, |_| false));
        assert!(!c.vaciar(191, 100, |_| false));
        assert_eq!(llegado, [10, 20, 30]);
        assert_eq!(c.llaves().count(), 0, "la llave 7 iba en el trozo que llegó");
        assert!(!c.pendiente());
        // Hostil: no lee nada y le llega mucho; al pasar de MAX_COLA, fuera, con sus llaves.
        let mut c = ColaSalida::default();
        c.meter(vec![trozo(100, Some(9))]);
        let mut ok = true;
        for _ in 0..(MAX_COLA / MAX_CANAL) {
            c.meter(vec![trozo(MAX_CANAL, None)]);
            ok = c.vaciar(0, 1_000_000, |_| false);
        }
        assert!(!ok);
        assert_eq!(c.llaves().collect::<Vec<_>>(), [9]);
    }
}
