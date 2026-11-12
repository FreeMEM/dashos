//! El puente de Wayland de la caja Linux (G1 de `docs/GRAFICOS_LOOP.md`, H9): el formato de su transporte y su lado
//! de DashOS.
//!
//! **El transporte** es un flujo de bytes (en la caja, una conexión vsock; en el PC, un socket de Unix) con tramas:
//! tipo (1 byte), conexión (u32), largo (u32) y el contenido, todo en little-endian. Una conexión es un cliente de
//! Wayland de la caja. Los descriptores no cruzan: en su lugar viajan memorias con número, que el otro lado crea y
//! rellena (`MEMORIA_NUEVA`, `MEMORIA_ESCRIBIR`), y los mensajes de Wayland llevan esos números como llaves. El lado
//! del invitado es `puente/` (con `std`, para Linux).
//!
//! **Sin copia** (`VISTAS`, `MEMORIA_VISTA`): si este lado lo anuncia, el invitado puede dar una memoria como la lista
//! de páginas físicas de su RAM donde está (un pool de `wl_shm` fijado en memoria), y este lado hace con ellas una
//! vista de solo lectura (llamada 50 del kernel) en vez de un objeto nuevo que llenar. Las páginas se comprueban
//! contra la RAM del invitado; si no valen o no se puede, hay una memoria normal y se le dice (`VISTA_NO`): copia.
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
/// Una memoria que ya está en la RAM del invitado, sin copia: número (u32), tamaño (u64), n (u32) y n direcciones
/// físicas del invitado (u64, de página entera), una por página, en orden. Solo tras `VISTAS`.
pub const MEMORIA_VISTA: u8 = 8;
/// De DashOS al invitado, al conectar: se admiten `MEMORIA_VISTA`.
pub const VISTAS: u8 = 9;
/// De DashOS al invitado: la vista de esa memoria (número, u32) no se hizo; en su lugar hay una memoria normal del
/// mismo tamaño, que se llena con `MEMORIA_ESCRIBIR` como siempre.
pub const VISTA_NO: u8 = 10;
/// H9, el portapapeles (del 12 en adelante: del 8 al 10 son las vistas sin copia de caja-linux-sincopia): un tubo (lo que en Linux es una tubería y en DashOS un canal: bytes que van en un sentido hasta
/// que se cierra). Número (u32). Lo manda el lado que tiene un extremo donde hay que escribir lo que llegue (en Linux,
/// la tubería que un programa da en `wl_data_offer.receive`; en DashOS, el canal que da un programa nativo para pegar).
/// El otro lado crea su par: da el extremo de escribir a su programa con el mensaje que lleva el número (como una
/// memoria) y manda lo que lee de él en `TUBO_DATOS`, hasta `TUBO_FIN`.
pub const TUBO_NUEVO: u8 = 12;
/// Bytes de un tubo: número (u32) y los bytes.
pub const TUBO_DATOS: u8 = 13;
/// Fin de un tubo: número (u32). En los dos sentidos: se acabó lo que había que leer, o quien escribía ya no puede (su
/// lector se fue) y el otro deja de leer.
pub const TUBO_FIN: u8 = 14;
/// Lo ya escrito en el destino de un tubo: número (u32) y cuántos bytes (u32). Quien lee de su extremo y lo manda en
/// `TUBO_DATOS` no deja más de `VENTANA_TUBO` bytes sin confirmar: así uno que pega despacio frena al que copia, en vez
/// de llenar lo que espera (y cortar el tubo).
pub const TUBO_LEIDO: u8 = 15;
/// Bytes de un tubo sin confirmar, como mucho.
pub const VENTANA_TUBO: u64 = 128 * 1024;

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
                        CONECTAR..=TUBO_LEIDO if largo > MAX_CONTROL => return Err(FalloTrama::Larga),
                        CONECTAR..=TUBO_LEIDO => {
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
    /// Una vista de solo lectura de la RAM del invitado: las páginas que empiezan en esas direcciones físicas suyas, en
    /// orden. `None` si no se puede (sin vistas, o direcciones que no son de su RAM): el invitado copiará.
    fn memoria_vista(&mut self, _direcciones: &[u64]) -> Option<u32> {
        None
    }
    /// Una copia del canal por el que contesta el compositor.
    fn copia_respuesta(&mut self) -> Option<u32>;
    fn cerrar(&mut self, llave: u32);
    /// Manda un trozo al compositor. `Err(true)` si su cola está llena (se reintenta), `Err(false)` si se fue.
    fn al_compositor(&mut self, bytes: &[u8], llaves: &[u32]) -> Result<(), bool>;
    /// H9, tubos: un canal nuevo: (el extremo que lee este lado, el que escribe, para darlo al compositor).
    fn tubo_crear(&mut self) -> Option<(u32, u32)> {
        None
    }
    /// Lee un mensaje del extremo `llave` sin esperar: `Ok(0)` si no hay nada; `Err` si el otro extremo se cerró y no
    /// queda nada (el fin).
    fn tubo_leer(&mut self, _llave: u32, _buf: &mut [u8]) -> Result<usize, ()> {
        Err(())
    }
    /// Escribe un mensaje (como mucho `MAX_CANAL` bytes) en el extremo `llave`: cuántos tomó (0 si la cola está llena),
    /// o `Err` si el otro extremo ya no está.
    fn tubo_escribir(&mut self, _llave: u32, _datos: &[u8]) -> Result<usize, ()> {
        Err(())
    }
    /// La hora en milisegundos (para el plazo de los tubos).
    fn ahora_ms(&mut self) -> u64 {
        0
    }
}

/// Tubos a la vez por transporte (H9).
pub const MAX_TUBOS: usize = 16;
/// Lo más que pasa por un tubo: más, y se corta.
pub const MAX_TUBO: u64 = 16 << 20;
/// Lo que se guarda de un tubo sin poder escribirlo (quien lee no lee): más, y se corta.
pub const MAX_TUBO_PENDIENTE: usize = 256 * 1024;
/// Un tubo sin moverse este tiempo se corta (una fuente que no escribe ni cierra no se queda con el hueco).
pub const PLAZO_TUBO_MS: u64 = 30_000;

/// Un tubo vivo (H9).
struct Tubo {
    /// El extremo de este lado: uno de leer (lo de aquí va al invitado) o de escribir (lo del invitado va aquí).
    llave: u32,
    lee: bool,
    /// El extremo de escribir que espera a ir al compositor con su mensaje (los tubos que pide el invitado).
    para_compositor: Option<u32>,
    pendiente: VecDeque<u8>,
    /// El invitado dijo fin: se cierra al vaciar lo pendiente.
    fin: bool,
    bytes: u64,
    ultimo_ms: u64,
    /// Si lee: lo mandado al invitado sin confirmar. Si escribe: lo escrito que aún no se le dijo.
    en_vuelo: u64,
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
    /// Una vista de la RAM del invitado: no se escribe, y no cuenta en `MAX_MEMORIA_TOTAL` (no toma memoria nueva).
    vista: bool,
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
    /// Tubos del portapapeles (H9) y los bytes que pasaron por ellos.
    pub tubos: u64,
    pub bytes_de_tubos: u64,
    /// De ellas, vistas de la RAM del invitado (sin copia).
    pub vistas: u64,
    /// Vistas que se pidieron y no se hicieron (el invitado copia en su lugar).
    pub vistas_no: u64,
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
    /// Los tubos del portapapeles (H9), por número.
    tubos: BTreeMap<u32, Tubo>,
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
            tubos: BTreeMap::new(),
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
                Some(m) if !m.vista && desde.checked_add(bytes.len() as u64).is_some_and(|f| f <= m.tam) => {
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

    /// Dice al invitado que este lado hace vistas (`MEMORIA_VISTA`): su `Sistema` sabe hacerlas.
    pub fn anunciar_vistas(&mut self) {
        self.al_invitado(VISTAS, 0, &[]);
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
                    // H9: un tubo que pidió el invitado: su extremo de escribir va al compositor (una vez).
                    if let Some(l) = self.tubos.get_mut(n).and_then(|t| t.para_compositor.take()) {
                        llaves.push(l);
                        continue;
                    }
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
                        self.memorias.insert(n, Memoria { llave, tam, vista: false });
                        self.total += tam;
                        self.cuentas.memorias += 1;
                    }
                    None => self.cuentas.malas += 1,
                }
            }
            MEMORIA_VISTA => {
                let (Some(n), Some(tam), Some(k)) = (Self::u32_en(d, 0), Self::u64_en(d, 4), Self::u32_en(d, 12)) else {
                    self.cuentas.malas += 1;
                    return;
                };
                self.soltar_memoria(n);
                // Una página por cada 4 KB del tamaño, ni una más; tantas como dice; y como mucho `MAX_MEMORIA`.
                let k = k as usize;
                if tam == 0 || tam > MAX_MEMORIA || k as u64 != tam.div_ceil(4096) || d.len() != 16 + 8 * k || self.memorias.len() >= MAX_MEMORIAS {
                    self.cuentas.malas += 1;
                    return;
                }
                let direcciones: Vec<u64> = d[16..].chunks_exact(8).map(|b| u64::from_le_bytes(b.try_into().unwrap())).collect();
                if let Some(llave) = self.sis.memoria_vista(&direcciones) {
                    self.memorias.insert(n, Memoria { llave, tam, vista: true });
                    self.cuentas.memorias += 1;
                    self.cuentas.vistas += 1;
                    return;
                }
                // No se pudo: una memoria normal y que copie. Si tampoco cabe, no hay memoria (como `MEMORIA_NUEVA`).
                self.cuentas.vistas_no += 1;
                if self.total + tam <= MAX_MEMORIA_TOTAL
                    && let Some(llave) = self.sis.memoria_nueva(tam)
                {
                    self.memorias.insert(n, Memoria { llave, tam, vista: false });
                    self.total += tam;
                    self.cuentas.memorias += 1;
                }
                self.al_invitado(VISTA_NO, 0, &n.to_le_bytes());
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
                        if !m.vista {
                            self.total = self.total - m.tam + tam.max(1);
                        }
                        m.tam = tam.max(1);
                    } else {
                        self.cuentas.malas += 1;
                    }
                }
            }
            TUBO_NUEVO | TUBO_DATOS | TUBO_FIN | TUBO_LEIDO => {
                let Some(n) = Self::u32_en(d, 0) else {
                    self.cuentas.malas += 1;
                    return;
                };
                self.tubo_del_invitado(tipo, n, &d[4..]);
            }
            _ => self.cuentas.malas += 1,
        }
    }

    /// Lo que el invitado dice de un tubo.
    fn tubo_del_invitado(&mut self, tipo: u8, n: u32, d: &[u8]) {
        let ahora = self.sis.ahora_ms();
        match tipo {
            TUBO_NUEVO => {
                if self.tubos.contains_key(&n) || self.tubos.len() >= MAX_TUBOS {
                    self.cuentas.malas += 1;
                    self.al_invitado(TUBO_FIN, 0, &n.to_le_bytes());
                    return;
                }
                let Some((lee, escribe)) = self.sis.tubo_crear() else {
                    self.al_invitado(TUBO_FIN, 0, &n.to_le_bytes());
                    return;
                };
                self.cuentas.tubos += 1;
                self.tubos.insert(n, Tubo { llave: lee, lee: true, para_compositor: Some(escribe), pendiente: VecDeque::new(), fin: false, bytes: 0, ultimo_ms: ahora, en_vuelo: 0 });
            }
            TUBO_DATOS => {
                let Some(t) = self.tubos.get_mut(&n).filter(|t| !t.lee && !t.fin) else {
                    self.cuentas.malas += 1;
                    return;
                };
                t.bytes += d.len() as u64;
                t.ultimo_ms = ahora;
                self.cuentas.bytes_de_tubos += d.len() as u64;
                if t.bytes > MAX_TUBO || t.pendiente.len() + d.len() > MAX_TUBO_PENDIENTE {
                    self.cuentas.malas += 1;
                    self.cortar_tubo(n, true);
                    return;
                }
                t.pendiente.extend(d);
                self.escribir_tubo(n);
            }
            TUBO_LEIDO => {
                // El invitado ya escribió eso en su destino: se puede mandar más.
                let k = d.get(..4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap())) as u64;
                match self.tubos.get_mut(&n).filter(|t| t.lee) {
                    Some(t) => {
                        t.en_vuelo = t.en_vuelo.saturating_sub(k);
                        t.ultimo_ms = ahora;
                    }
                    None => self.cuentas.malas += 1,
                }
            }
            _ => {
                // TUBO_FIN: uno que leía aquí (el invitado ya no quiere), fuera; uno que escribe, al vaciar.
                match self.tubos.get_mut(&n) {
                    Some(t) if t.lee => self.cortar_tubo(n, false),
                    Some(t) => {
                        t.fin = true;
                        self.escribir_tubo(n);
                    }
                    None => {}
                }
            }
        }
    }

    /// Escribe lo pendiente de un tubo mientras quepa; si el invitado dijo fin y no queda nada, lo cierra.
    fn escribir_tubo(&mut self, n: u32) {
        let Some(t) = self.tubos.get_mut(&n) else { return };
        while !t.pendiente.is_empty() {
            let (a, _) = t.pendiente.as_slices();
            let trozo = &a[..a.len().min(MAX_CANAL)];
            match self.sis.tubo_escribir(t.llave, trozo) {
                Ok(0) => return,
                Ok(k) => {
                    t.pendiente.drain(..k.min(trozo.len()));
                    t.en_vuelo += k as u64;
                }
                Err(()) => {
                    // Quien leía se fue: que el invitado deje de mandar.
                    self.cortar_tubo(n, true);
                    return;
                }
            }
        }
        // Lo escrito, al invitado (con todo escrito, o cada 16 KB).
        if t.en_vuelo > 0 && (t.pendiente.is_empty() || t.en_vuelo >= 16 * 1024) {
            let mut d = n.to_le_bytes().to_vec();
            d.extend_from_slice(&(t.en_vuelo.min(u32::MAX as u64) as u32).to_le_bytes());
            t.en_vuelo = 0;
            self.al_invitado(TUBO_LEIDO, 0, &d);
        }
        let t = self.tubos.get_mut(&n).unwrap();
        if t.fin && t.pendiente.is_empty() {
            let t = self.tubos.remove(&n).unwrap();
            self.sis.cerrar(t.llave);
        }
    }

    /// Cierra un tubo de este lado (y, si `avisar`, se lo dice al invitado).
    fn cortar_tubo(&mut self, n: u32, avisar: bool) {
        if let Some(t) = self.tubos.remove(&n) {
            self.sis.cerrar(t.llave);
            if let Some(l) = t.para_compositor {
                self.sis.cerrar(l);
            }
            if avisar {
                self.al_invitado(TUBO_FIN, 0, &n.to_le_bytes());
            }
        }
    }

    /// Mueve los tubos (lo llama quien atiende cada poco): lo que haya para leer, al invitado; lo pendiente, a su canal;
    /// los que llevan `PLAZO_TUBO_MS` sin moverse, fuera.
    pub fn mover_tubos(&mut self) {
        let ahora = self.sis.ahora_ms();
        let numeros: Vec<u32> = self.tubos.keys().copied().collect();
        let mut buf = [0u8; MAX_CANAL];
        for n in numeros {
            let Some(t) = self.tubos.get(&n) else { continue };
            if ahora.saturating_sub(t.ultimo_ms) > PLAZO_TUBO_MS {
                self.cuentas.malas += 1;
                self.cortar_tubo(n, true);
                continue;
            }
            if !t.lee {
                self.escribir_tubo(n);
                continue;
            }
            // Lo que se lee, al invitado en tramas de hasta 64 KB, sin pasar de la mitad de lo que se le guarda.
            let mut datos = n.to_le_bytes().to_vec();
            let mut acabado = false;
            let en_vuelo = t.en_vuelo;
            while datos.len() < 64 * 1024 && self.salida.len() + datos.len() < MAX_SALIDA / 2 && en_vuelo + (datos.len() - 4 + MAX_CANAL) as u64 <= VENTANA_TUBO {
                let llave = self.tubos[&n].llave;
                match self.sis.tubo_leer(llave, &mut buf) {
                    Ok(0) => break,
                    Ok(k) => datos.extend_from_slice(&buf[..k.min(MAX_CANAL)]),
                    Err(()) => {
                        acabado = true;
                        break;
                    }
                }
            }
            let k = (datos.len() - 4) as u64;
            if k > 0 {
                let t = self.tubos.get_mut(&n).unwrap();
                t.bytes += k;
                t.en_vuelo += k;
                t.ultimo_ms = ahora;
                self.cuentas.bytes_de_tubos += k;
                if t.bytes > MAX_TUBO {
                    self.cuentas.malas += 1;
                    self.cortar_tubo(n, true);
                    continue;
                }
                self.al_invitado(TUBO_DATOS, 0, &datos);
            }
            if acabado {
                self.cortar_tubo(n, true);
            }
        }
    }

    /// Tubos vivos (para las pruebas y el registro).
    pub fn tubos_vivos(&self) -> usize {
        self.tubos.len()
    }

    fn soltar_memoria(&mut self, n: u32) {
        if let Some(m) = self.memorias.remove(&n) {
            if !m.vista {
                self.total -= m.tam;
            }
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
            // H9: lo que no es una memoria es el extremo de un canal donde escribir (el portapapeles: `send`): un tubo.
            // El invitado crea su tubería y nos manda lo que el programa escriba en ella.
            if self.sis.memoria_tamano(l).is_none() {
                self.siguiente_memoria = self.siguiente_memoria.wrapping_add(1);
                let n = 0x8000_0000 | self.siguiente_memoria;
                self.al_invitado(TUBO_NUEVO, 0, &n.to_le_bytes());
                if self.tubos.len() >= MAX_TUBOS {
                    // Sin sitio: el número va igual (los descriptores del mensaje casan) y el tubo nace acabado.
                    self.sis.cerrar(l);
                    self.cuentas.malas += 1;
                    self.al_invitado(TUBO_FIN, 0, &n.to_le_bytes());
                } else {
                    let ahora = self.sis.ahora_ms();
                    self.cuentas.tubos += 1;
                    self.tubos.insert(n, Tubo { llave: l, lee: false, para_compositor: None, pendiente: VecDeque::new(), fin: false, bytes: 0, ultimo_ms: ahora, en_vuelo: 0 });
                }
                numeros.push(n);
                continue;
            }
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
        let ts: Vec<u32> = self.tubos.keys().copied().collect();
        for n in ts {
            self.cortar_tubo(n, false);
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
        /// Vistas hechas: llave → direcciones.
        vistas: BTreeMap<u32, Vec<u64>>,
        /// Canales de mentira (H9, tubos): llave → (mensajes que esperan, el otro extremo cerrado).
        canales: BTreeMap<u32, (VecDeque<Vec<u8>>, bool)>,
        /// Extremo → su par.
        pares: BTreeMap<u32, u32>,
        ahora: u64,
    }

    impl Falso {
        /// Mete un mensaje en el canal que lee `llave` (lo que escribe un programa nativo en su extremo).
        fn escribir_en(&mut self, llave: u32, d: &[u8]) {
            self.canales.get_mut(&llave).unwrap().0.push_back(d.to_vec());
        }
    }

    /// La RAM del invitado de mentira: 64 MB desde 1 GB.
    const RAM: u64 = 0x4000_0000;

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
        fn memoria_vista(&mut self, direcciones: &[u64]) -> Option<u32> {
            if !direcciones.iter().all(|&d| d % 4096 == 0 && (RAM..RAM + (64 << 20)).contains(&d)) {
                return None;
            }
            self.siguiente += 1;
            self.vistas.insert(self.siguiente, direcciones.to_vec());
            self.memorias.insert(self.siguiente, vec![0; direcciones.len() * 4096]);
            Some(self.siguiente)
        }
        fn cerrar(&mut self, llave: u32) {
            self.cerradas.push(llave);
            if let Some(par) = self.pares.get(&llave).copied()
                && let Some(c) = self.canales.get_mut(&par)
            {
                c.1 = true;
            }
        }
        fn tubo_crear(&mut self) -> Option<(u32, u32)> {
            let (a, b) = (self.siguiente + 1, self.siguiente + 2);
            self.siguiente += 2;
            self.canales.insert(a, (VecDeque::new(), false));
            self.canales.insert(b, (VecDeque::new(), false));
            self.pares.insert(a, b);
            self.pares.insert(b, a);
            Some((a, b))
        }
        fn tubo_leer(&mut self, llave: u32, buf: &mut [u8]) -> Result<usize, ()> {
            let c = self.canales.get_mut(&llave).ok_or(())?;
            match c.0.pop_front() {
                Some(m) => {
                    buf[..m.len()].copy_from_slice(&m);
                    Ok(m.len())
                }
                None if c.1 => Err(()),
                None => Ok(0),
            }
        }
        fn tubo_escribir(&mut self, llave: u32, datos: &[u8]) -> Result<usize, ()> {
            assert!(datos.len() <= MAX_CANAL);
            // Un extremo suelto (de un programa nativo): su lector es la llave + 1000.
            let c = self.canales.entry(llave + 1000).or_insert((VecDeque::new(), false));
            if c.1 {
                return Err(());
            }
            if c.0.len() >= 16 {
                return Ok(0);
            }
            c.0.push_back(datos.to_vec());
            Ok(datos.len())
        }
        fn ahora_ms(&mut self) -> u64 {
            self.ahora
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

    fn mem_vista(n: u32, tam: u64, dirs: &[u64]) -> Vec<u8> {
        let mut d = n.to_le_bytes().to_vec();
        d.extend_from_slice(&tam.to_le_bytes());
        d.extend_from_slice(&(dirs.len() as u32).to_le_bytes());
        for x in dirs {
            d.extend_from_slice(&x.to_le_bytes());
        }
        trama(MEMORIA_VISTA, 0, &d)
    }

    /// Las tramas para el invitado, partidas a mano (`VISTAS` y `VISTA_NO` no las lee el `Lector` de este lado).
    fn tramas_al_invitado<S: Sistema>(p: &mut Puente<S>) -> Vec<(u8, u32, Vec<u8>)> {
        let mut salida = vec![0u8; 1 << 16];
        let n = p.enviar(&mut salida);
        let mut piezas = Vec::new();
        let mut i = 0;
        while i + CABECERA <= n {
            let largo = u32::from_le_bytes(salida[i + 5..i + 9].try_into().unwrap()) as usize;
            piezas.push((salida[i], u32::from_le_bytes(salida[i + 1..i + 5].try_into().unwrap()), salida[i + 9..i + 9 + largo].to_vec()));
            i += CABECERA + largo;
        }
        piezas
    }

    /// Sin copia: el puente anuncia las vistas; una memoria del invitado dada por sus páginas físicas es una vista, que
    /// va al compositor como cualquier memoria, no se escribe y no cuenta en el total. Unas páginas que no son de su
    /// RAM dan una memoria normal y `VISTA_NO` (el invitado copia). Las tramas mal hechas no hacen nada.
    #[test]
    fn vistas() {
        let mut p = Puente::nuevo(Falso::default(), 0);
        p.anunciar_vistas();
        assert_eq!(tramas_al_invitado(&mut p), vec![(VISTAS, 0, vec![])]);
        let mut f = trama(CONECTAR, 1, &[]);
        f.extend(mem_vista(5, 3 * 4096 - 100, &[RAM + 0x5000, RAM, RAM + 0x5000]));
        f.extend(mem_escribir(5, 0, b"no se escribe en una vista"));
        f.extend(trama(DATOS, 1, &juntar_datos(&[5], b"pool")));
        p.recibir(&f).unwrap();
        let llave = p.sis.copias[&p.sis.enviados[1].1[0]];
        assert_eq!(p.sis.vistas[&llave], vec![RAM + 0x5000, RAM, RAM + 0x5000]);
        assert!(p.sis.memorias[&llave].iter().all(|&b| b == 0));
        assert_eq!(p.vivos(), (1, 1, 0), "una vista no cuenta en el total");
        assert_eq!((p.cuentas.vistas, p.cuentas.malas), (1, 1));
        assert!(tramas_al_invitado(&mut p).is_empty());
        // Crecer dentro de sus páginas vale; soltarla, también, y el total sigue a cero.
        let mut d = 5u32.to_le_bytes().to_vec();
        d.extend_from_slice(&(3 * 4096u64).to_le_bytes());
        p.recibir(&trama(MEMORIA_CRECER, 0, &d)).unwrap();
        p.recibir(&trama(MEMORIA_SOLTAR, 0, &5u32.to_le_bytes())).unwrap();
        assert_eq!(p.vivos(), (1, 0, 0));
        // Páginas fuera de su RAM, sin alinear o del kernel: memoria normal, VISTA_NO, y se copia como siempre.
        for (n, dirs) in [(6u32, vec![0u64]), (7, vec![RAM + 1]), (8, vec![u64::MAX & !0xfff])] {
            p.recibir(&mem_vista(n, 4096, &dirs)).unwrap();
            assert_eq!(tramas_al_invitado(&mut p), vec![(VISTA_NO, 0, n.to_le_bytes().to_vec())]);
        }
        assert_eq!(p.vivos(), (1, 3, 3 * 4096));
        p.recibir(&mem_escribir(6, 0, b"copiado")).unwrap();
        p.recibir(&trama(DATOS, 1, &juntar_datos(&[6], b"pool"))).unwrap();
        let llave = p.sis.copias[&p.sis.enviados.last().unwrap().1[0]];
        assert_eq!(&p.sis.memorias[&llave][..7], b"copiado");
        assert_eq!(p.cuentas.vistas_no, 3);
        // Mal hechas: tamaño 0 o de más, páginas que no casan con el tamaño, una lista más corta o más larga de lo que
        // dice, un número de páginas enorme. Ninguna hace memoria.
        let antes = p.vivos();
        let malas = p.cuentas.malas;
        let mut f = mem_vista(20, 0, &[]);
        f.extend(mem_vista(21, MAX_MEMORIA + 4096, &vec![RAM; 4097]));
        f.extend(mem_vista(22, 8192, &[RAM]));
        f.extend(mem_vista(23, 4096, &[RAM, RAM]));
        let mut corta = mem_vista(24, 8192, &[RAM, RAM]);
        corta.truncate(corta.len() - 8);
        let largo = (corta.len() - CABECERA) as u32;
        corta[5..9].copy_from_slice(&largo.to_le_bytes());
        f.extend(corta);
        let mut enorme = 25u32.to_le_bytes().to_vec();
        enorme.extend_from_slice(&4096u64.to_le_bytes());
        enorme.extend_from_slice(&u32::MAX.to_le_bytes());
        f.extend(trama(MEMORIA_VISTA, 0, &enorme));
        f.extend(trama(MEMORIA_VISTA, 0, &[1, 2, 3]));
        p.recibir(&f).unwrap();
        assert_eq!(p.vivos(), antes);
        assert_eq!(p.cuentas.malas, malas + 7);
        // Más vistas que `MAX_MEMORIAS`: las de más no se hacen.
        let mut f = Vec::new();
        for i in 0..MAX_MEMORIAS as u32 + 10 {
            f.extend(mem_vista(1000 + i, 4096, &[RAM]));
        }
        p.recibir(&f).unwrap();
        assert_eq!(p.vivos().1, MAX_MEMORIAS);
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

    /// Las tramas que salieron hacia el invitado.
    fn al_invitado(p: &mut Puente<Falso>) -> Vec<(u8, u32, Vec<u8>)> {
        let mut salida = vec![0u8; 4 << 20];
        let n = p.enviar(&mut salida);
        let mut piezas = Vec::new();
        Lector::default()
            .empujar(&salida[..n], &mut |x| match x {
                Pieza::Trama { tipo, conexion, datos } => piezas.push((tipo, conexion, datos.to_vec())),
                Pieza::Memoria { bytes, .. } => piezas.push((MEMORIA_ESCRIBIR, 0, bytes.to_vec())),
            })
            .unwrap();
        piezas
    }

    fn tubo(tipo: u8, n: u32, d: &[u8]) -> Vec<u8> {
        let mut v = n.to_le_bytes().to_vec();
        v.extend_from_slice(d);
        trama(tipo, 0, &v)
    }

    /// H9, el portapapeles por tubos. Pegar en Linux (el invitado pide un tubo y lo manda con su `receive`): su extremo
    /// de escribir va al compositor; lo que el programa nativo escribe en él llega al invitado en `TUBO_DATOS`, y al
    /// cerrarlo, `TUBO_FIN`. Copiar desde Linux (el compositor manda un `send` con el canal donde escribir): sale
    /// `TUBO_NUEVO` antes del mensaje, y lo que manda el invitado se escribe en ese canal hasta su `TUBO_FIN`.
    #[test]
    fn tubos() {
        let mut p = Puente::nuevo(Falso::default(), 0);
        let mut f = trama(CONECTAR, 1, &[]);
        f.extend(tubo(TUBO_NUEVO, 7, &[]));
        f.extend(trama(DATOS, 1, &juntar_datos(&[7], b"receive!")));
        p.recibir(&f).unwrap();
        let (bytes, llaves) = p.sis.enviados.last().unwrap().clone();
        assert_eq!(&bytes[5..], b"receive!");
        let escribe = llaves[0];
        let lee = p.sis.pares[&escribe];
        assert_eq!(p.tubos_vivos(), 1);
        // El programa nativo escribe y cierra.
        p.sis.escribir_en(lee, b"hola ");
        p.sis.escribir_en(lee, b"desde DashOS");
        p.mover_tubos();
        p.sis.canales.get_mut(&lee).unwrap().1 = true;
        p.mover_tubos();
        let t = al_invitado(&mut p);
        assert_eq!(t[0], (TUBO_DATOS, 0, [&7u32.to_le_bytes()[..], b"hola desde DashOS"].concat()));
        assert_eq!(t[1], (TUBO_FIN, 0, 7u32.to_le_bytes().to_vec()));
        assert_eq!(p.tubos_vivos(), 0);
        assert!(p.sis.cerradas.contains(&lee));
        // Al revés: un `send` del compositor con el canal donde escribe la fuente de Linux.
        let mut t = canal::cabecera_sub(TROZO_SUB_DATOS, 1).to_vec();
        t.extend_from_slice(b"send....");
        p.del_compositor(&t, &[500]);
        let piezas = al_invitado(&mut p);
        assert_eq!(piezas[0].0, TUBO_NUEVO);
        let n = u32::from_le_bytes(piezas[0].2[..4].try_into().unwrap());
        let (nums, b) = partir_datos(&piezas[1].2).unwrap();
        assert_eq!((nums, b), (vec![n], &b"send...."[..]));
        p.recibir(&tubo(TUBO_DATOS, n, b"copiado en foot")).unwrap();
        p.recibir(&tubo(TUBO_FIN, n, &[])).unwrap();
        assert_eq!(p.sis.canales[&1500].0, [b"copiado en foot".to_vec()]);
        assert!(p.sis.cerradas.contains(&500));
        assert_eq!(p.tubos_vivos(), 0);
        assert_eq!(p.cuentas.tubos, 2);
    }

    /// Hostiles de los tubos: números repetidos o de más (pasan `MAX_TUBOS`), datos de tubos que no existen o que leen
    /// aquí, más de lo que cabe esperando (quien pega no lee), un tubo que no se mueve en `PLAZO_TUBO_MS`, un lector que
    /// se va (se avisa al invitado) y un tubo usado dos veces en mensajes (cierra al cliente). Todo cortado; el puente
    /// sigue y al cerrar el transporte no queda nada abierto.
    #[test]
    fn tubos_hostiles() {
        let mut p = Puente::nuevo(Falso::default(), 0);
        let mut f = trama(CONECTAR, 1, &[]);
        for n in 0..(MAX_TUBOS as u32 + 4) {
            f.extend(tubo(TUBO_NUEVO, n, &[]));
        }
        f.extend(tubo(TUBO_NUEVO, 0, &[]));
        f.extend(tubo(TUBO_DATOS, 4242, b"de nadie"));
        f.extend(tubo(TUBO_DATOS, 1, b"a uno que lee aqui"));
        f.extend(trama(TUBO_FIN, 0, &[1]));
        p.recibir(&f).unwrap();
        assert_eq!(p.tubos_vivos(), MAX_TUBOS);
        let fines = al_invitado(&mut p).iter().filter(|t| t.0 == TUBO_FIN).count();
        assert_eq!(fines, 5);
        // Un tubo dado dos veces en mensajes: la segunda no tiene extremo: fuera el cliente.
        p.recibir(&trama(DATOS, 1, &juntar_datos(&[3], b"uno"))).unwrap();
        p.recibir(&trama(DATOS, 1, &juntar_datos(&[3], b"dos"))).unwrap();
        assert_eq!(p.vivos().0, 0);
        // Quien pega desde DashOS no lee: lo de más se corta.
        let mut p = Puente::nuevo(Falso::default(), 0);
        p.recibir(&trama(CONECTAR, 1, &[])).unwrap();
        let mut t = canal::cabecera_sub(TROZO_SUB_DATOS, 1).to_vec();
        t.extend_from_slice(b"send....");
        p.del_compositor(&t, &[600]);
        let n = u32::from_le_bytes(al_invitado(&mut p)[0].2[..4].try_into().unwrap());
        let mucho = vec![0x55u8; 60_000];
        for _ in 0..6 {
            p.recibir(&tubo(TUBO_DATOS, n, &mucho)).unwrap();
        }
        assert_eq!(p.tubos_vivos(), 0);
        assert!(al_invitado(&mut p).iter().any(|t| t.0 == TUBO_FIN));
        // Un lector que se va mientras se escribe: fin al invitado.
        p.del_compositor(&t, &[700]);
        let n = u32::from_le_bytes(al_invitado(&mut p)[0].2[..4].try_into().unwrap());
        p.sis.canales.insert(1700, (VecDeque::new(), true));
        p.recibir(&tubo(TUBO_DATOS, n, b"x")).unwrap();
        assert_eq!(p.tubos_vivos(), 0);
        assert!(al_invitado(&mut p).iter().any(|t| t.0 == TUBO_FIN));
        // Un tubo que no se mueve: fuera al pasar el plazo.
        p.recibir(&tubo(TUBO_NUEVO, 9, &[])).unwrap();
        p.sis.ahora += PLAZO_TUBO_MS + 1;
        p.mover_tubos();
        assert_eq!(p.tubos_vivos(), 0);
        // Al cerrar el transporte, todos los tubos cerrados.
        p.recibir(&tubo(TUBO_NUEVO, 10, &[])).unwrap();
        p.del_compositor(&t, &[800]);
        p.cerrar_todo();
        assert_eq!(p.tubos_vivos(), 0);
        assert!(p.sis.cerradas.contains(&800));
    }

    /// La ventana de los tubos: sin `TUBO_LEIDO` del invitado, no se manda más de `VENTANA_TUBO`; con él, sigue. Y lo que
    /// se escribe en un canal se confirma al invitado.
    #[test]
    fn tubos_con_ventana() {
        let mut p = Puente::nuevo(Falso::default(), 0);
        let mut f = trama(CONECTAR, 1, &[]);
        f.extend(tubo(TUBO_NUEVO, 7, &[]));
        f.extend(trama(DATOS, 1, &juntar_datos(&[7], b"receive!")));
        p.recibir(&f).unwrap();
        let lee = p.sis.pares[&p.sis.enviados.last().unwrap().1[0]];
        for _ in 0..2000 {
            p.sis.escribir_en(lee, &[1u8; 200]);
        }
        let mandado = |p: &mut Puente<Falso>| -> u64 { al_invitado(p).iter().filter(|t| t.0 == TUBO_DATOS).map(|t| t.2.len() as u64 - 4).sum() };
        p.mover_tubos();
        p.mover_tubos();
        let a = mandado(&mut p);
        assert!(a <= VENTANA_TUBO && a > VENTANA_TUBO / 2, "{a}");
        p.recibir(&tubo(TUBO_LEIDO, 7, &(a as u32).to_le_bytes())).unwrap();
        p.mover_tubos();
        assert!(mandado(&mut p) > 0);
        // Al revés: lo escrito en el canal, confirmado.
        let mut t = canal::cabecera_sub(TROZO_SUB_DATOS, 1).to_vec();
        t.extend_from_slice(b"send....");
        p.del_compositor(&t, &[900]);
        let n = u32::from_le_bytes(al_invitado(&mut p)[0].2[..4].try_into().unwrap());
        p.recibir(&tubo(TUBO_DATOS, n, &[5u8; 1000])).unwrap();
        let leido = al_invitado(&mut p).into_iter().find(|t| t.0 == TUBO_LEIDO).unwrap();
        assert_eq!(leido.2, [n.to_le_bytes(), 1000u32.to_le_bytes()].concat());
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
                let tipo = (azar() % 17) as u8;
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
