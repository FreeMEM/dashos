//! El puente de Wayland de la caja Linux (G1 de `docs/GRAFICOS_LOOP.md`, H9): el formato de su transporte y su lado
//! de DashOS.
//!
//! **El transporte** es un flujo de bytes (en la caja, una conexión vsock; en el PC, un socket de Unix) con tramas:
//! tipo (1 byte), conexión (u32), largo (u32) y el contenido, todo en little-endian. Una conexión es un cliente de
//! Wayland de la caja. Los descriptores no cruzan: en su lugar viajan memorias con número, que el otro lado crea y
//! rellena (`MEMORIA_NUEVA`, `MEMORIA_ESCRIBIR`), y los mensajes de Wayland llevan esos números como llaves. El lado
//! del invitado es `puente/` (con `std`, para Linux).
//!
//! **El lado de DashOS** ([`Puente`]): cada conexión del invitado es una subconexión del compositor
//! (`canal::TROZO_SUB_*`, por la llave "ventanas" de la caja), y cada memoria, un objeto de memoria de DashOS que el
//! compositor lee con una copia de solo lectura de su llave. Lo que llega del invitado no se cree: tramas que no
//! cuadran cortan el transporte; números de memoria o de conexión que no existen, escrituras fuera de una memoria o
//! tamaños por encima de los topes se tiran y se cuentan. Lo que hace con el sistema va por [`Sistema`], así se prueba
//! en el PC.

use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;

use crate::canal::{self, CABECERA_SUB, MAX_CANAL, MAX_LLAVES, TROZO_SUB_ADIOS, TROZO_SUB_CONECTAR, TROZO_SUB_DATOS};

/// Un cliente nuevo.
pub const CONECTAR: u8 = 1;
/// Bytes de Wayland: n (u32), n números de memoria (u32) y los bytes.
pub const DATOS: u8 = 2;
/// El cliente se fue.
pub const ADIOS: u8 = 3;
/// Una memoria nueva: número (u32) y tamaño (u64).
pub const MEMORIA_NUEVA: u8 = 4;
/// Escribir en una memoria: número (u32), desde (u64) y los bytes.
pub const MEMORIA_ESCRIBIR: u8 = 5;
/// Soltar una memoria: número (u32).
pub const MEMORIA_SOLTAR: u8 = 6;
/// Crecer una memoria: número (u32) y tamaño nuevo (u64).
pub const MEMORIA_CRECER: u8 = 7;

pub const CABECERA: usize = 9;
/// Lo más que mide el contenido de una trama de `MEMORIA_ESCRIBIR` (en el PC, un búfer de 4K a 32 bits cabe en unas
/// pocas).
pub const MAX_TRAMA: usize = 4 << 20;
/// Lo más que mide cualquier otra trama: una lectura entera del puente (64 KB de Wayland) y sus llaves cabe de sobra.
pub const MAX_CONTROL: usize = 256 * 1024;

/// Una trama entera para mandar.
pub fn trama(tipo: u8, conexion: u32, datos: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(CABECERA + datos.len());
    v.push(tipo);
    v.extend_from_slice(&conexion.to_le_bytes());
    v.extend_from_slice(&(datos.len() as u32).to_le_bytes());
    v.extend_from_slice(datos);
    v
}

/// Los datos de una trama `DATOS`: (números de memoria, bytes de Wayland).
pub fn partir_datos(d: &[u8]) -> Option<(Vec<u32>, &[u8])> {
    let n = u32::from_le_bytes(d.get(0..4)?.try_into().ok()?) as usize;
    if n > d.len() / 4 {
        return None;
    }
    let llaves = (0..n).map(|i| d.get(4 + 4 * i..8 + 4 * i).map(|b| u32::from_le_bytes(b.try_into().unwrap()))).collect::<Option<Vec<_>>>()?;
    Some((llaves, d.get(4 + 4 * n..)?))
}

pub fn juntar_datos(llaves: &[u32], bytes: &[u8]) -> Vec<u8> {
    let mut d = Vec::with_capacity(4 + 4 * llaves.len() + bytes.len());
    d.extend_from_slice(&(llaves.len() as u32).to_le_bytes());
    for l in llaves {
        d.extend_from_slice(&l.to_le_bytes());
    }
    d.extend_from_slice(bytes);
    d
}

/// Lo que va saliendo de un flujo de tramas.
#[derive(Debug, PartialEq, Eq)]
pub enum Pieza<'a> {
    /// Una trama entera (cualquiera menos `MEMORIA_ESCRIBIR`).
    Trama { tipo: u8, conexion: u32, datos: &'a [u8] },
    /// Un trozo de una `MEMORIA_ESCRIBIR`, según llega (sin esperar a la trama entera: puede medir megas).
    Memoria { numero: u32, desde: u64, bytes: &'a [u8] },
}

/// El flujo no es de tramas: el transporte se corta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FalloTrama {
    /// Más larga que su tope.
    Larga,
    /// Una `MEMORIA_ESCRIBIR` sin su número y su desde.
    Corta,
    /// Un tipo que no existe.
    Tipo,
}

/// Saca tramas de un flujo de bytes que llega a trozos, sin guardar las de memoria enteras.
#[derive(Default)]
pub struct Lector {
    cab: [u8; CABECERA + 12],
    tengo: usize,
    /// Bytes del contenido que faltan por llegar de la trama actual (tras la cabecera).
    resto: usize,
    buf: Vec<u8>,
    desde: u64,
}

impl Lector {
    fn tipo(&self) -> u8 {
        self.cab[0]
    }

    /// Mete bytes del flujo; por cada pieza completa, `f`. Un error deja el lector inservible.
    pub fn empujar(&mut self, mut datos: &[u8], f: &mut dyn FnMut(Pieza)) -> Result<(), FalloTrama> {
        while !datos.is_empty() {
            // La cabecera (y, en las de memoria, número y desde).
            let quiere = if self.tengo >= CABECERA && self.tipo() == MEMORIA_ESCRIBIR { CABECERA + 12 } else { CABECERA };
            if self.tengo < quiere {
                let n = (quiere - self.tengo).min(datos.len());
                self.cab[self.tengo..self.tengo + n].copy_from_slice(&datos[..n]);
                self.tengo += n;
                datos = &datos[n..];
                if self.tengo == CABECERA {
                    let largo = u32::from_le_bytes(self.cab[5..9].try_into().unwrap()) as usize;
                    match self.tipo() {
                        MEMORIA_ESCRIBIR if largo < 12 => return Err(FalloTrama::Corta),
                        MEMORIA_ESCRIBIR if largo > MAX_TRAMA => return Err(FalloTrama::Larga),
                        MEMORIA_ESCRIBIR => {
                            self.resto = largo - 12;
                            continue;
                        }
                        CONECTAR..=MEMORIA_CRECER if largo > MAX_CONTROL => return Err(FalloTrama::Larga),
                        CONECTAR..=MEMORIA_CRECER => {
                            self.resto = largo;
                            self.buf.clear();
                        }
                        _ => return Err(FalloTrama::Tipo),
                    }
                } else if self.tengo == CABECERA + 12 {
                    self.desde = u64::from_le_bytes(self.cab[13..21].try_into().unwrap());
                }
                if self.tengo < quiere {
                    continue;
                }
            }
            let conexion = u32::from_le_bytes(self.cab[1..5].try_into().unwrap());
            let n = self.resto.min(datos.len());
            if self.tipo() == MEMORIA_ESCRIBIR {
                if n > 0 {
                    let numero = u32::from_le_bytes(self.cab[9..13].try_into().unwrap());
                    f(Pieza::Memoria { numero, desde: self.desde, bytes: &datos[..n] });
                    self.desde = self.desde.wrapping_add(n as u64);
                }
            } else {
                self.buf.extend_from_slice(&datos[..n]);
            }
            datos = &datos[n..];
            self.resto -= n;
            if self.resto == 0 {
                if self.tipo() != MEMORIA_ESCRIBIR {
                    f(Pieza::Trama { tipo: self.tipo(), conexion, datos: &self.buf });
                }
                self.tengo = 0;
            }
        }
        Ok(())
    }
}

/// Lo que el puente necesita del sistema (en DashOS, llamadas al kernel; en las pruebas, de mentira).
pub trait Sistema {
    /// Un objeto de memoria nuevo de `tam` bytes (como mucho `MAX_MEMORIA`), que este lado puede escribir: su llave.
    fn memoria_nueva(&mut self, tam: u64) -> Option<u32>;
    /// Escribe en el objeto; `desde + datos.len()` ya está dentro de su tamaño.
    fn memoria_escribir(&mut self, llave: u32, desde: u64, datos: &[u8]);
    /// Copia `n` bytes del objeto `llave` desde el principio (lo que llega del compositor con un mensaje), si mide eso.
    fn memoria_leer(&mut self, llave: u32, n: usize) -> Option<Vec<u8>>;
    /// Lo que mide un objeto de memoria (para las llaves que llegan del compositor).
    fn memoria_tamano(&mut self, llave: u32) -> Option<u64>;
    /// Una copia de solo lectura de la llave, para dársela al compositor.
    fn copia_lectura(&mut self, llave: u32) -> Option<u32>;
    /// Una copia del canal por el que contesta el compositor.
    fn copia_respuesta(&mut self) -> Option<u32>;
    fn cerrar(&mut self, llave: u32);
    /// Manda un trozo al compositor. `Err(true)` si su cola está llena (se reintenta), `Err(false)` si se fue.
    fn al_compositor(&mut self, bytes: &[u8], llaves: &[u32]) -> Result<(), bool>;
}

/// Lo más que mide una memoria del invitado (el tope de `memoria_crear`).
pub const MAX_MEMORIA: u64 = 16 << 20;
/// Lo más de memoria del invitado a la vez, en total (lo paga el grupo de la caja: entra en su margen,
/// `dashos::maquina::MARGEN_SERVICIO`). Una ventana de 1920 × 1080 con dos búferes cabe justa.
pub const MAX_MEMORIA_TOTAL: u64 = 24 << 20;
/// Memorias del invitado a la vez.
pub const MAX_MEMORIAS: usize = 256;
/// Clientes de Wayland del invitado a la vez.
pub const MAX_CONEXIONES: usize = 32;
/// Lo que se guarda para el compositor sin poder mandarlo: más, y se deja de leer del invitado (el crédito de vsock
/// le hace esperar).
pub const MAX_PENDIENTE: usize = 64 * 1024;
/// Lo que se guarda para el invitado sin que lo lea: más, y se cierra el cliente que lo causa.
pub const MAX_SALIDA: usize = 4 << 20;
/// Lo más que mide una memoria que el compositor da a un cliente (el mapa de teclado, la tabla de formatos).
pub const MAX_DEL_COMPOSITOR: u64 = 1 << 20;

struct Memoria {
    llave: u32,
    tam: u64,
}

/// Cuentas para el registro y las pruebas.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Cuentas {
    pub clientes: u64,
    /// Bytes de Wayland del invitado al compositor y al revés.
    pub bytes_al_compositor: u64,
    pub bytes_al_invitado: u64,
    /// Bytes de memoria (búferes) copiados del invitado.
    pub bytes_de_memoria: u64,
    pub memorias: u64,
    /// Lo que se tiró por no cuadrar: memorias o conexiones que no existen, escrituras fuera, tamaños de más.
    pub malas: u64,
    /// Clientes cerrados por algo suyo (una memoria que no existe, salida de más...).
    pub cerrados: u64,
}

/// El lado de DashOS del puente, para una conexión del transporte.
pub struct Puente<S: Sistema> {
    pub sis: S,
    lector: Lector,
    memorias: BTreeMap<u32, Memoria>,
    total: u64,
    conexiones: BTreeMap<u32, ()>,
    /// Trozos para el compositor que aún no cupieron en su canal.
    pendiente: VecDeque<canal::Trozo>,
    pendiente_bytes: usize,
    /// Bytes de tramas para el invitado.
    salida: VecDeque<u8>,
    siguiente_memoria: u32,
    /// El compositor se fue: no hay nada que hacer.
    pub sin_compositor: bool,
    /// Los números de subconexión del compositor son `prefijo << 16 | cliente`: varios transportes a la vez (un
    /// puente y otro, o uno hostil) comparten el canal de respuesta sin chocar.
    prefijo: u32,
    pub cuentas: Cuentas,
}

impl<S: Sistema> Puente<S> {
    pub fn nuevo(sis: S, prefijo: u32) -> Self {
        Self {
            sis,
            lector: Lector::default(),
            memorias: BTreeMap::new(),
            total: 0,
            conexiones: BTreeMap::new(),
            pendiente: VecDeque::new(),
            pendiente_bytes: 0,
            salida: VecDeque::new(),
            siguiente_memoria: 0,
            sin_compositor: false,
            prefijo,
            cuentas: Cuentas::default(),
        }
    }

    /// Bytes del transporte. Devuelve cuántos toma (0 si el compositor va atrasado: el invitado esperará), o un fallo
    /// si el flujo no es de tramas (hay que cortar el transporte y llamar a `cerrar_todo`).
    pub fn recibir(&mut self, datos: &[u8]) -> Result<usize, FalloTrama> {
        self.vaciar();
        if self.pendiente_bytes > MAX_PENDIENTE {
            return Ok(0);
        }
        let mut lector = core::mem::take(&mut self.lector);
        let r = lector.empujar(datos, &mut |p| self.pieza(p));
        self.lector = lector;
        self.vaciar();
        r.map(|_| datos.len())
    }

    fn pieza(&mut self, p: Pieza) {
        match p {
            Pieza::Memoria { numero, desde, bytes } => match self.memorias.get(&numero) {
                Some(m) if desde.checked_add(bytes.len() as u64).is_some_and(|f| f <= m.tam) => {
                    let llave = m.llave;
                    self.sis.memoria_escribir(llave, desde, bytes);
                    self.cuentas.bytes_de_memoria += bytes.len() as u64;
                }
                _ => self.cuentas.malas += 1,
            },
            Pieza::Trama { tipo, conexion, datos } => self.trama(tipo, conexion, datos),
        }
    }

    fn u32_en(d: &[u8], i: usize) -> Option<u32> {
        Some(u32::from_le_bytes(d.get(i..i + 4)?.try_into().ok()?))
    }

    fn u64_en(d: &[u8], i: usize) -> Option<u64> {
        Some(u64::from_le_bytes(d.get(i..i + 8)?.try_into().ok()?))
    }

    /// El número de subconexión del cliente `c` del invitado.
    fn sub(&self, c: u32) -> u32 {
        self.prefijo << 16 | c
    }

    /// El prefijo de este transporte (para repartir lo que llega del compositor).
    pub fn prefijo(&self) -> u32 {
        self.prefijo
    }

    fn trama(&mut self, tipo: u8, c: u32, d: &[u8]) {
        match tipo {
            CONECTAR => {
                if c > 0xffff || self.conexiones.contains_key(&c) || self.conexiones.len() >= MAX_CONEXIONES {
                    self.cuentas.malas += 1;
                    self.al_invitado(ADIOS, c, &[]);
                    return;
                }
                let Some(r) = self.sis.copia_respuesta() else {
                    self.al_invitado(ADIOS, c, &[]);
                    return;
                };
                self.conexiones.insert(c, ());
                self.cuentas.clientes += 1;
                self.encolar(canal::cabecera_sub(TROZO_SUB_CONECTAR, self.sub(c)).to_vec(), alloc::vec![r]);
            }
            DATOS => {
                if !self.conexiones.contains_key(&c) {
                    self.cuentas.malas += 1;
                    return;
                }
                let Some((numeros, bytes)) = partir_datos(d) else {
                    self.cuentas.malas += 1;
                    self.cerrar_cliente(c, true);
                    return;
                };
                // Cada número, una copia de solo lectura de su memoria; uno que no existe cierra al cliente (si no, sus
                // llaves y sus mensajes ya no casarían en el compositor).
                let mut llaves = Vec::with_capacity(numeros.len());
                for n in &numeros {
                    match self.memorias.get(n).and_then(|m| self.sis.copia_lectura(m.llave)) {
                        Some(l) => llaves.push(l),
                        None => {
                            for l in llaves {
                                self.sis.cerrar(l);
                            }
                            self.cuentas.malas += 1;
                            self.cerrar_cliente(c, true);
                            return;
                        }
                    }
                }
                self.cuentas.bytes_al_compositor += bytes.len() as u64;
                // Las llaves, lo antes posible (como mucho 8 por trozo): nunca llegan después del mensaje que las usa.
                let cab = canal::cabecera_sub(TROZO_SUB_DATOS, self.sub(c));
                let mut llaves = llaves.into_iter().peekable();
                let mut resto = bytes;
                while llaves.peek().is_some() || !resto.is_empty() {
                    let suyas: Vec<u32> = llaves.by_ref().take(MAX_LLAVES).collect();
                    let n = resto.len().min(MAX_CANAL - CABECERA_SUB);
                    let mut t = cab.to_vec();
                    t.extend_from_slice(&resto[..n]);
                    resto = &resto[n..];
                    self.encolar(t, suyas);
                }
            }
            ADIOS => {
                if self.conexiones.remove(&c).is_some() {
                    self.encolar(canal::cabecera_sub(TROZO_SUB_ADIOS, self.sub(c)).to_vec(), Vec::new());
                }
            }
            MEMORIA_NUEVA => {
                let (Some(n), Some(tam)) = (Self::u32_en(d, 0), Self::u64_en(d, 4)) else {
                    self.cuentas.malas += 1;
                    return;
                };
                self.soltar_memoria(n);
                let tam = tam.max(1);
                if tam > MAX_MEMORIA || self.total + tam > MAX_MEMORIA_TOTAL || self.memorias.len() >= MAX_MEMORIAS {
                    self.cuentas.malas += 1;
                    return;
                }
                match self.sis.memoria_nueva(tam) {
                    Some(llave) => {
                        self.memorias.insert(n, Memoria { llave, tam });
                        self.total += tam;
                        self.cuentas.memorias += 1;
                    }
                    None => self.cuentas.malas += 1,
                }
            }
            MEMORIA_SOLTAR => match Self::u32_en(d, 0) {
                Some(n) => self.soltar_memoria(n),
                None => self.cuentas.malas += 1,
            },
            MEMORIA_CRECER => {
                // Un objeto de memoria de DashOS no crece: vale si el nuevo tamaño cabe en lo que ya mide (las páginas
                // de más). Si no, lo que el compositor lea más allá le dará error al cliente, no al puente.
                let (Some(n), Some(tam)) = (Self::u32_en(d, 0), Self::u64_en(d, 4)) else {
                    self.cuentas.malas += 1;
                    return;
                };
                if let Some(m) = self.memorias.get_mut(&n) {
                    let cabe = m.tam.next_multiple_of(4096);
                    if tam <= cabe {
                        self.total = self.total - m.tam + tam.max(1);
                        m.tam = tam.max(1);
                    } else {
                        self.cuentas.malas += 1;
                    }
                }
            }
            _ => self.cuentas.malas += 1,
        }
    }

    fn soltar_memoria(&mut self, n: u32) {
        if let Some(m) = self.memorias.remove(&n) {
            self.total -= m.tam;
            self.sis.cerrar(m.llave);
        }
    }

    fn encolar(&mut self, bytes: Vec<u8>, llaves: Vec<u32>) {
        self.pendiente_bytes += bytes.len();
        self.pendiente.push_back(canal::Trozo { bytes, llaves });
    }

    fn al_invitado(&mut self, tipo: u8, c: u32, d: &[u8]) {
        self.salida.extend(trama(tipo, c, d));
    }

    /// Cierra un cliente del invitado: en el compositor (si `aviso`) y en el invitado.
    fn cerrar_cliente(&mut self, c: u32, aviso: bool) {
        if self.conexiones.remove(&c).is_some() {
            self.cuentas.cerrados += 1;
            if aviso {
                self.encolar(canal::cabecera_sub(TROZO_SUB_ADIOS, self.sub(c)).to_vec(), Vec::new());
            }
        }
        self.al_invitado(ADIOS, c, &[]);
    }

    /// Manda al compositor lo que espera, en orden, mientras quepa.
    pub fn vaciar(&mut self) {
        while let Some(t) = self.pendiente.front() {
            if self.sin_compositor {
                let t = self.pendiente.pop_front().unwrap();
                for l in t.llaves {
                    self.sis.cerrar(l);
                }
                continue;
            }
            match self.sis.al_compositor(&t.bytes, &t.llaves) {
                Ok(()) => {
                    let t = self.pendiente.pop_front().unwrap();
                    self.pendiente_bytes -= t.bytes.len();
                }
                Err(true) => break,
                Err(false) => self.sin_compositor = true,
            }
        }
        if self.pendiente.is_empty() {
            self.pendiente_bytes = 0;
        }
    }

    /// Si queda algo para el compositor.
    pub fn atrasado(&self) -> bool {
        !self.pendiente.is_empty()
    }

    /// Un trozo del compositor por el canal de respuesta: bytes para un cliente del invitado (con sus llaves, que se
    /// copian como memorias nuevas) o su cierre. Lo que no es de un cliente que exista se tira, con sus llaves.
    pub fn del_compositor(&mut self, trozo: &[u8], llaves: &[u32]) {
        let tipo = trozo.first().copied();
        let Some((c, bytes)) = canal::partir_sub(trozo)
            .filter(|(sub, _)| matches!(tipo, Some(TROZO_SUB_DATOS | TROZO_SUB_ADIOS)) && sub >> 16 == self.prefijo)
            .map(|(sub, b)| (sub & 0xffff, b))
        else {
            for &l in llaves {
                self.sis.cerrar(l);
            }
            return;
        };
        if !self.conexiones.contains_key(&c) {
            for &l in llaves {
                self.sis.cerrar(l);
            }
            return;
        }
        if tipo == Some(TROZO_SUB_ADIOS) {
            for &l in llaves {
                self.sis.cerrar(l);
            }
            self.cerrar_cliente(c, false);
            return;
        }
        let mut numeros = Vec::new();
        for &l in llaves {
            let datos = self.sis.memoria_tamano(l).filter(|&t| t <= MAX_DEL_COMPOSITOR).and_then(|t| self.sis.memoria_leer(l, t as usize));
            self.sis.cerrar(l);
            let datos = datos.unwrap_or_default();
            // Los números de las memorias de este lado van desde arriba: no chocan con los del invitado.
            self.siguiente_memoria = self.siguiente_memoria.wrapping_add(1);
            let n = 0x8000_0000 | self.siguiente_memoria;
            let mut d = n.to_le_bytes().to_vec();
            d.extend_from_slice(&(datos.len() as u64).to_le_bytes());
            self.al_invitado(MEMORIA_NUEVA, 0, &d);
            if !datos.is_empty() {
                let mut d = n.to_le_bytes().to_vec();
                d.extend_from_slice(&0u64.to_le_bytes());
                d.extend_from_slice(&datos);
                self.al_invitado(MEMORIA_ESCRIBIR, 0, &d);
            }
            numeros.push(n);
        }
        self.cuentas.bytes_al_invitado += bytes.len() as u64;
        self.al_invitado(DATOS, c, &juntar_datos(&numeros, bytes));
        if self.salida.len() > MAX_SALIDA {
            // El invitado no lee lo suyo: fuera ese cliente, y lo pendiente no se manda (salvo su ADIOS).
            self.salida.clear();
            self.cerrar_cliente(c, true);
        }
    }

    /// Saca bytes para el invitado.
    pub fn enviar(&mut self, buf: &mut [u8]) -> usize {
        let n = buf.len().min(self.salida.len());
        for (b, x) in buf.iter_mut().zip(self.salida.drain(..n)) {
            *b = x;
        }
        n
    }

    pub fn hay_salida(&self) -> bool {
        !self.salida.is_empty()
    }

    /// El transporte se cortó: cada cliente, fuera del compositor; las memorias, soltadas. Queda como nuevo.
    pub fn cerrar_todo(&mut self) {
        let cs: Vec<u32> = self.conexiones.keys().copied().collect();
        for c in cs {
            self.conexiones.remove(&c);
            self.encolar(canal::cabecera_sub(TROZO_SUB_ADIOS, self.sub(c)).to_vec(), Vec::new());
        }
        let ns: Vec<u32> = self.memorias.keys().copied().collect();
        for n in ns {
            self.soltar_memoria(n);
        }
        self.lector = Lector::default();
        self.salida.clear();
        self.vaciar();
    }

    /// Clientes y memorias vivos (para las pruebas y el registro).
    pub fn vivos(&self) -> (usize, usize, u64) {
        (self.conexiones.len(), self.memorias.len(), self.total)
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use alloc::vec;

    #[derive(Default)]
    struct Falso {
        siguiente: u32,
        memorias: BTreeMap<u32, Vec<u8>>,
        cerradas: Vec<u32>,
        enviados: Vec<(Vec<u8>, Vec<u32>)>,
        lleno: bool,
        copias: BTreeMap<u32, u32>,
    }

    impl Sistema for Falso {
        fn memoria_nueva(&mut self, tam: u64) -> Option<u32> {
            self.siguiente += 1;
            self.memorias.insert(self.siguiente, vec![0; tam as usize]);
            Some(self.siguiente)
        }
        fn memoria_escribir(&mut self, llave: u32, desde: u64, datos: &[u8]) {
            let m = self.memorias.get_mut(&llave).unwrap();
            m[desde as usize..desde as usize + datos.len()].copy_from_slice(datos);
        }
        fn memoria_leer(&mut self, llave: u32, n: usize) -> Option<Vec<u8>> {
            self.memorias.get(&llave).map(|m| m[..n.min(m.len())].to_vec())
        }
        fn memoria_tamano(&mut self, llave: u32) -> Option<u64> {
            self.memorias.get(&llave).map(|m| m.len() as u64)
        }
        fn copia_lectura(&mut self, llave: u32) -> Option<u32> {
            self.siguiente += 1;
            self.copias.insert(self.siguiente, llave);
            Some(self.siguiente)
        }
        fn copia_respuesta(&mut self) -> Option<u32> {
            self.siguiente += 1;
            Some(self.siguiente)
        }
        fn cerrar(&mut self, llave: u32) {
            self.cerradas.push(llave);
        }
        fn al_compositor(&mut self, bytes: &[u8], llaves: &[u32]) -> Result<(), bool> {
            if self.lleno {
                return Err(true);
            }
            assert!(bytes.len() <= MAX_CANAL && llaves.len() <= MAX_LLAVES);
            self.enviados.push((bytes.to_vec(), llaves.to_vec()));
            Ok(())
        }
    }

    fn mem_nueva(n: u32, tam: u64) -> Vec<u8> {
        let mut d = n.to_le_bytes().to_vec();
        d.extend_from_slice(&tam.to_le_bytes());
        trama(MEMORIA_NUEVA, 0, &d)
    }

    fn mem_escribir(n: u32, desde: u64, b: &[u8]) -> Vec<u8> {
        let mut d = n.to_le_bytes().to_vec();
        d.extend_from_slice(&desde.to_le_bytes());
        d.extend_from_slice(b);
        trama(MEMORIA_ESCRIBIR, 0, &d)
    }

    /// Las tramas sacadas a trozos de cualquier tamaño dan lo mismo que de una vez; la memoria sale según llega.
    #[test]
    fn lector_a_trozos() {
        let mut flujo = trama(CONECTAR, 7, &[]);
        flujo.extend(mem_escribir(3, 100, &[9u8; 1000]));
        flujo.extend(trama(DATOS, 7, &juntar_datos(&[3], b"hola")));
        flujo.extend(trama(ADIOS, 7, &[]));
        for paso in [1, 2, 5, 9, 13, 21, 100, 5000] {
            let mut l = Lector::default();
            let mut tramas = Vec::new();
            let mut memoria = Vec::new();
            for t in flujo.chunks(paso) {
                l.empujar(t, &mut |p| match p {
                    Pieza::Trama { tipo, conexion, datos } => tramas.push((tipo, conexion, datos.to_vec())),
                    Pieza::Memoria { numero, desde, bytes } => {
                        assert_eq!((numero, desde), (3, 100 + memoria.len() as u64));
                        memoria.extend_from_slice(bytes)
                    }
                })
                .unwrap();
            }
            assert_eq!(memoria, vec![9u8; 1000], "paso {}", paso);
            assert_eq!(tramas.len(), 3, "paso {}", paso);
            assert_eq!(tramas[0], (CONECTAR, 7, vec![]));
            assert_eq!(tramas[2], (ADIOS, 7, vec![]));
        }
        // Tramas que no valen.
        let mut l = Lector::default();
        assert_eq!(l.empujar(&trama(99, 0, &[]), &mut |_| {}), Err(FalloTrama::Tipo));
        let mut l = Lector::default();
        assert_eq!(l.empujar(&trama(MEMORIA_ESCRIBIR, 0, &[1, 2, 3]), &mut |_| {}), Err(FalloTrama::Corta));
        let mut l = Lector::default();
        let mut larga = vec![DATOS, 0, 0, 0, 0];
        larga.extend_from_slice(&(MAX_CONTROL as u32 + 1).to_le_bytes());
        assert_eq!(l.empujar(&larga, &mut |_| {}), Err(FalloTrama::Larga));
    }

    /// Un cliente de punta a punta: conecta, crea su memoria, la escribe, la manda con un mensaje (llave de solo
    /// lectura en el primer trozo, mensajes de más de 251 bytes partidos) y se va. Lo del compositor vuelve con su
    /// memoria copiada.
    #[test]
    fn cliente_entero() {
        let mut p = Puente::nuevo(Falso::default(), 0);
        let mut flujo = trama(CONECTAR, 1, &[]);
        flujo.extend(mem_nueva(5, 4096));
        flujo.extend(mem_escribir(5, 10, b"pixeles"));
        let wl = vec![0xabu8; 600];
        flujo.extend(trama(DATOS, 1, &juntar_datos(&[5], &wl)));
        assert_eq!(p.recibir(&flujo), Ok(flujo.len()));
        let e = &p.sis.enviados;
        assert_eq!(e[0].0, vec![TROZO_SUB_CONECTAR, 1, 0, 0, 0]);
        assert_eq!(e[0].1.len(), 1);
        assert_eq!(&e[1].0[..5], &[TROZO_SUB_DATOS, 1, 0, 0, 0]);
        assert_eq!(e[1].1.len(), 1, "la llave de la memoria, en el primer trozo");
        let llave = p.sis.copias[&e[1].1[0]];
        assert_eq!(&p.sis.memorias[&llave][10..17], b"pixeles");
        let juntos: Vec<u8> = e[1..].iter().flat_map(|t| t.0[5..].to_vec()).collect();
        assert_eq!(juntos, wl);
        // Del compositor: un evento con una memoria (el mapa de teclado).
        let mapa = p.sis.memoria_nueva(100).unwrap();
        p.sis.memoria_escribir(mapa, 0, b"xkb_keymap {");
        let mut t = canal::cabecera_sub(TROZO_SUB_DATOS, 1).to_vec();
        t.extend_from_slice(b"evento!!");
        p.del_compositor(&t, &[mapa]);
        assert!(p.sis.cerradas.contains(&mapa));
        let mut salida = vec![0u8; 1000];
        let n = p.enviar(&mut salida);
        let mut piezas = Vec::new();
        Lector::default()
            .empujar(&salida[..n], &mut |x| match x {
                Pieza::Trama { tipo, conexion, datos } => piezas.push((tipo, conexion, datos.to_vec())),
                Pieza::Memoria { bytes, .. } => piezas.push((MEMORIA_ESCRIBIR, 0, bytes.to_vec())),
            })
            .unwrap();
        assert_eq!(piezas[0].0, MEMORIA_NUEVA);
        assert_eq!(&piezas[1].2[..12], b"xkb_keymap {");
        let (nums, b) = partir_datos(&piezas[2].2).unwrap();
        assert_eq!((piezas[2].0, piezas[2].1, b), (DATOS, 1, &b"evento!!"[..]));
        assert_eq!(nums.len(), 1);
        // Se va: ADIOS al compositor; su memoria sigue hasta MEMORIA_SOLTAR.
        let mut flujo = trama(ADIOS, 1, &[]);
        let d = 5u32.to_le_bytes();
        flujo.extend(trama(MEMORIA_SOLTAR, 0, &d));
        p.recibir(&flujo).unwrap();
        assert_eq!(p.sis.enviados.last().unwrap().0, vec![TROZO_SUB_ADIOS, 1, 0, 0, 0]);
        assert_eq!(p.vivos(), (0, 0, 0));
    }

    /// Con el canal del compositor lleno, lo suyo espera en orden; con demasiado esperando, no se toma más del
    /// invitado; cuando se vacía, sigue.
    #[test]
    fn compositor_lleno() {
        let mut p = Puente::nuevo(Falso::default(), 0);
        p.recibir(&trama(CONECTAR, 1, &[])).unwrap();
        p.sis.lleno = true;
        let datos = trama(DATOS, 1, &juntar_datos(&[], &[1u8; 60_000]));
        assert_eq!(p.recibir(&datos), Ok(datos.len()));
        assert_eq!(p.recibir(&datos), Ok(datos.len()));
        assert_eq!(p.recibir(&datos), Ok(0), "más de MAX_PENDIENTE esperando: no se toma");
        assert!(p.atrasado());
        p.sis.lleno = false;
        p.vaciar();
        assert!(!p.atrasado());
        assert_eq!(p.recibir(&datos), Ok(datos.len()));
        let total: usize = p.sis.enviados.iter().skip(1).map(|t| t.0.len() - 5).sum();
        assert_eq!(total, 3 * 60_000);
    }

    /// Hostiles: memorias que no existen, escrituras fuera, tamaños absurdos, demasiadas memorias y clientes, números
    /// de memoria en un mensaje que no existen, conexiones que no existen y basura del compositor. El puente sigue, y
    /// un cliente bueno funciona después.
    #[test]
    fn hostil() {
        let mut p = Puente::nuevo(Falso::default(), 0);
        let mut f = Vec::new();
        f.extend(mem_escribir(77, 0, b"no existe"));
        f.extend(mem_nueva(1, 100));
        f.extend(mem_escribir(1, 90, &[0u8; 20])); // se sale
        f.extend(mem_escribir(1, u64::MAX - 3, &[0u8; 8])); // desborda
        f.extend(mem_nueva(2, MAX_MEMORIA + 1)); // demasiado grande
        f.extend(mem_nueva(3, u64::MAX));
        f.extend(trama(MEMORIA_NUEVA, 0, &[1, 2])); // corta
        f.extend(trama(DATOS, 55, &juntar_datos(&[], b"de nadie")));
        f.extend(trama(CONECTAR, 4, &[]));
        f.extend(trama(CONECTAR, 4, &[])); // repetida
        f.extend(trama(DATOS, 4, &[0xff, 0xff, 0xff, 0xff])); // dice 4 mil millones de números
        p.recibir(&f).unwrap();
        assert!(p.sis.enviados.iter().any(|t| t.0 == vec![TROZO_SUB_ADIOS, 4, 0, 0, 0]));
        assert_eq!(p.vivos().0, 0);
        assert!(p.cuentas.malas >= 8, "{:?}", p.cuentas);
        // Un mensaje con un número de memoria que no existe cierra a ese cliente.
        let mut f = trama(CONECTAR, 5, &[]);
        f.extend(trama(DATOS, 5, &juntar_datos(&[1, 999], b"x")));
        p.recibir(&f).unwrap();
        assert_eq!(p.vivos().0, 0);
        // Demasiada memoria en total y demasiados clientes.
        let mut f = Vec::new();
        for i in 0..10 {
            f.extend(mem_nueva(100 + i, MAX_MEMORIA));
        }
        for c in 0..(MAX_CONEXIONES as u32 + 5) {
            f.extend(trama(CONECTAR, 1000 + c, &[]));
        }
        p.recibir(&f).unwrap();
        let (clientes, _, total) = p.vivos();
        assert!(total <= MAX_MEMORIA_TOTAL);
        assert_eq!(clientes, MAX_CONEXIONES);
        // Basura del compositor: otro tipo, un cliente que no existe (sus llaves se cierran), un trozo corto.
        let antes = p.sis.cerradas.len();
        p.del_compositor(&[9, 9], &[]);
        p.del_compositor(&canal::cabecera_sub(TROZO_SUB_DATOS, 4242), &[7777]);
        p.del_compositor(&[TROZO_SUB_DATOS], &[7778]);
        assert_eq!(p.sis.cerradas.len(), antes + 2);
        // El transporte se corta: todo fuera.
        p.cerrar_todo();
        assert_eq!(p.vivos(), (0, 0, 0));
        // Y uno bueno después.
        let mut f = trama(CONECTAR, 9, &[]);
        f.extend(mem_nueva(1, 64));
        f.extend(trama(DATOS, 9, &juntar_datos(&[1], b"bien")));
        p.recibir(&f).unwrap();
        assert_eq!(p.vivos().0, 1);
        assert_eq!(&p.sis.enviados.last().unwrap().0[5..], b"bien");
    }

    /// Basura al azar por el transporte: o error de trama (se corta) o nada; nunca pánico.
    #[test]
    fn a_lo_loco() {
        let mut s: u64 = 0x1234_5678_9abc_def1;
        let mut azar = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        for _ in 0..300 {
            let mut p = Puente::nuevo(Falso::default(), 0);
            for _ in 0..50 {
                // Cabeceras casi buenas con contenido al azar.
                let tipo = (azar() % 9) as u8;
                let largo = (azar() % 64) as usize;
                let d: Vec<u8> = (0..largo).map(|_| azar() as u8).collect();
                let t = trama(tipo, (azar() % 4) as u32, &d);
                if p.recibir(&t).is_err() {
                    p.cerrar_todo();
                }
                if azar() % 5 == 0 {
                    let mut t = canal::cabecera_sub(TROZO_SUB_DATOS, (azar() % 4) as u32).to_vec();
                    t.extend((0..azar() % 20).map(|_| azar() as u8));
                    p.del_compositor(&t, &[]);
                }
                let mut b = [0u8; 64];
                while p.enviar(&mut b) > 0 {}
            }
        }
    }
}
