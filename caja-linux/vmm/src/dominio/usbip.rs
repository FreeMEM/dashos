//! USB/IP entre un dominio de controladores y DashOS (S35, paso 2; `docs/bitacoras/dominio-usb.md`).
//!
//! El aparato USB es de DashOS (lo lleva su xHCI) y se presta al dominio. El Linux del dominio lo engancha con su
//! controlador anfitrión virtual de USB/IP (`vhci-hcd`, de serie): habla USB/IP por un socket, y su `/init` pasa los
//! bytes de ese socket al **tubo**, una memoria compartida con el servicio del dominio (dos anillos de bytes). El
//! servicio es el **servidor** de USB/IP: lee las peticiones (`CMD_SUBMIT`, `CMD_UNLINK`), las comprueba y se las pasa
//! al xHCI; con lo que este contesta, escribe las respuestas (`RET_SUBMIT`, `RET_UNLINK`).
//!
//! El protocolo, de `Documentation/usb/usbip_protocol.rst` de Linux (leído como especificación): una cabecera de 48 bytes
//! en orden de red y, detrás, los datos de salida (en `CMD_SUBMIT`) o de entrada (en `RET_SUBMIT`). Solo la parte de
//! después de enganchar (sin `OP_REQ_IMPORT`: el `/init` engancha el socket él mismo, por sysfs). Sin isócronas.
//!
//! **Lo que llega del dominio es hostil.** [`Servidor`] no se fía de nada:
//!
//! - un aparato que no es el prestado (`devid`), un punto que no existe, un largo de más de [`MAX_DATOS`], una
//!   preparación que no cuadra con el largo o isócronas: la petición se contesta con un error y no sale de aquí;
//! - lo que rompe el flujo (una orden desconocida, unos datos de salida que no caben, un anillo con índices imposibles)
//!   es un [`Roto`]: el servicio da el dominio por roto y lo relanza;
//! - lo que tiene en marcha está acotado: [`RANURAS`] en el xHCI a la vez (y por punto y dirección, menos), y en espera
//!   [`MAX_ESPERA`] peticiones y [`MAX_ESPERA_BYTES`] de datos; con eso lleno, no se lee más del tubo (el dominio
//!   espera: solo se frena a sí mismo).
//!
//! Sin `unsafe` ni nada de DashOS: el tubo y los búferes del xHCI se tocan por [`MemoriaTubo`] y [`Buferes`], y se
//! prueba en el PC.

use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;

pub const CMD_SUBMIT: u32 = 1;
pub const CMD_UNLINK: u32 = 2;
pub const RET_SUBMIT: u32 = 3;
pub const RET_UNLINK: u32 = 4;
/// Lo que mide la cabecera de las cuatro órdenes.
pub const CABECERA: usize = 48;
/// Lo más que mide una transferencia (lo que cabe en una ranura de búfer del xHCI).
pub const MAX_DATOS: usize = 16 * 1024;
/// Transferencias en el xHCI a la vez (las ranuras de búfer que presta el xHCI), repartidas: de entrada, de salida y
/// una de control. Cada clase tiene las suyas: las de entrada pueden esperar sin fin (un punto de interrupción que no
/// tiene nada que decir), y las de salida y la de control tienen que poder seguir igual.
pub const RANURAS: usize = 16;
pub const MAX_ENTRADA: usize = 12;
pub const MAX_SALIDA: usize = RANURAS - MAX_ENTRADA - 1;
/// Por punto y dirección; el de control (0), de una en una.
pub const MAX_POR_PUNTO: usize = 4;
/// En espera (aún sin mandar al xHCI): peticiones y bytes de datos de salida guardados.
pub const MAX_ESPERA: usize = 256;
pub const MAX_ESPERA_BYTES: usize = 256 * 1024;
/// Respuestas escritas pero sin sitio en el tubo: con tanto, no se manda nada más al xHCI.
pub const MAX_SALIDA_BYTES: usize = 256 * 1024;

/// `transfer_flags` de Linux que se miran (`include/uapi/linux/usbip.h`).
pub const URB_SHORT_NOT_OK: u32 = 0x0001;
pub const URB_ZERO_PACKET: u32 = 0x0040;

/// Los errores que ve el dominio, como los de Linux (`status` de `RET_SUBMIT`).
pub const ENOMEM: i32 = -12;
pub const ENODEV: i32 = -19;
pub const EINVAL: i32 = -22;
pub const EPIPE: i32 = -32;
pub const EPROTO: i32 = -71;
pub const EOVERFLOW: i32 = -75;
pub const ECONNRESET: i32 = -104;
pub const ESHUTDOWN: i32 = -108;
pub const EREMOTEIO: i32 = -121;

// ---- el tubo ----

/// La cabecera del tubo (una página) y sus dos anillos de bytes: la ida (del dominio a DashOS) y la vuelta.
pub const TUBO_CABECERA: u64 = 4096;
pub const TAM_ANILLO: u32 = 128 * 1024;
pub const TAM_TUBO: u64 = TUBO_CABECERA + 2 * TAM_ANILLO as u64;
/// En la cabecera (u32, en el orden de la máquina): el préstamo que sirve el servicio (lo escribe él), el que abrió
/// el dominio (lo escribe el dominio cuando ha dejado sus índices a cero), y los cuatro índices: cuántos bytes se han
/// escrito y leído en cada anillo desde que se abrió (dan la vuelta a los 4 GB).
pub const T_CUENTA: u64 = 0x00;
pub const T_ABIERTO: u64 = 0x04;
pub const T_IDA_ESCRITO: u64 = 0x08;
pub const T_IDA_LEIDO: u64 = 0x0c;
pub const T_VUELTA_ESCRITO: u64 = 0x10;
pub const T_VUELTA_LEIDO: u64 = 0x14;
pub const T_IDA: u64 = TUBO_CABECERA;
pub const T_VUELTA: u64 = TUBO_CABECERA + TAM_ANILLO as u64;

/// La memoria del tubo vista desde el servicio. El dominio la cambia cuando quiere: cada lectura puede dar otra cosa.
pub trait MemoriaTubo {
    fn leer32(&self, off: u64) -> u32;
    fn escribir32(&mut self, off: u64, v: u32);
    fn leer(&self, off: u64, buf: &mut [u8]);
    fn escribir(&mut self, off: u64, datos: &[u8]);
    /// Que lo escrito antes se vea antes que lo de después (una barrera).
    fn barrera(&self);
}

/// Los búferes que presta el xHCI (`RANURAS` de `MAX_DATOS` bytes): se escriben antes de mandar una transferencia de
/// salida y se leen al acabar una de entrada.
pub trait Buferes {
    fn escribir(&mut self, ranura: usize, datos: &[u8]);
    fn leer(&self, ranura: usize, buf: &mut [u8]);
}

/// Lo que hace que el servicio dé el dominio por roto: el flujo ya no se puede seguir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Roto(pub &'static str);

/// Lo que hay en el anillo de ida, sin sacarlo: bytes disponibles y dónde se empieza a leer.
fn disponibles(m: &dyn MemoriaTubo, leido: u32) -> Result<u32, Roto> {
    let escrito = m.leer32(T_IDA_ESCRITO);
    // Lo que escribió el dominio antes de mover el índice, después de leerlo.
    m.barrera();
    let n = escrito.wrapping_sub(leido);
    if n > TAM_ANILLO {
        return Err(Roto("el índice de la ida del tubo es imposible"));
    }
    Ok(n)
}

fn copiar_de_anillo(m: &dyn MemoriaTubo, base: u64, pos: u32, buf: &mut [u8]) {
    let i = pos % TAM_ANILLO;
    let primero = ((TAM_ANILLO - i) as usize).min(buf.len());
    m.leer(base + i as u64, &mut buf[..primero]);
    if primero < buf.len() {
        m.leer(base, &mut buf[primero..]);
    }
}

fn copiar_a_anillo(m: &mut dyn MemoriaTubo, base: u64, pos: u32, datos: &[u8]) {
    let i = pos % TAM_ANILLO;
    let primero = ((TAM_ANILLO - i) as usize).min(datos.len());
    m.escribir(base + i as u64, &datos[..primero]);
    if primero < datos.len() {
        m.escribir(base, &datos[primero..]);
    }
}

fn be32(b: &[u8], i: usize) -> u32 {
    u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// Una respuesta: la cabecera de 48 bytes (`RET_SUBMIT` o `RET_UNLINK`) y los datos de entrada, si los hay.
fn respuesta(orden: u32, seqnum: u32, estado: i32, largo: u32, datos: &[u8]) -> Vec<u8> {
    let mut r = vec![0u8; CABECERA + datos.len()];
    r[0..4].copy_from_slice(&orden.to_be_bytes());
    r[4..8].copy_from_slice(&seqnum.to_be_bytes());
    // devid, dirección y punto: 0 en las respuestas.
    r[20..24].copy_from_slice(&(estado as u32).to_be_bytes());
    if orden == RET_SUBMIT {
        r[24..28].copy_from_slice(&largo.to_be_bytes());
        // start_frame 0, number_of_packets 0 (sin isócronas), error_count 0.
    }
    r[CABECERA..].copy_from_slice(datos);
    r
}

// ---- lo que se pide al xHCI ----

/// Lo que el servicio pide al xHCI para el aparato prestado (`protocolo::usb_prestado` de DashOS).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Peticion {
    /// Por el punto 0, con su preparación; los datos (si `largo` > 0), en la ranura `ranura`.
    Control { etiqueta: u32, ranura: u8, setup: [u8; 8], largo: u32 },
    /// Por el punto `punto` (dirección USB: número y 0x80 si es de entrada), `largo` bytes en la ranura `ranura`; `cero`:
    /// un paquete vacío detrás si el largo es múltiplo del paquete (`URB_ZERO_PACKET`).
    Transferir { etiqueta: u32, ranura: u8, punto: u8, largo: u32, cero: bool },
    /// Cancela la de `etiqueta`, si sigue en marcha.
    Cancelar { etiqueta: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Estado {
    Esperando,
    /// En el xHCI, con su ranura de búfer.
    EnMarcha(u8),
    /// En el xHCI, con su ranura y el `seqnum` del `CMD_UNLINK` que la cancela.
    Cancelando(u8, u32),
}

#[derive(Debug)]
struct Urb {
    seqnum: u32,
    etiqueta: u32,
    /// Dirección USB del punto (0x80 si es de entrada); el de control es 0 en las dos direcciones.
    punto: u8,
    entrada: bool,
    largo: u32,
    flags: u32,
    setup: [u8; 8],
    salida: Vec<u8>,
    estado: Estado,
}

impl Urb {
    fn clave(&self) -> u8 {
        if self.punto & 0x7f == 0 { 0 } else { self.punto }
    }
}

/// Cuentas para el registro y las pruebas: lo raro que mandó el dominio.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Cuentas {
    pub peticiones: u64,
    pub hechas: u64,
    pub canceladas: u64,
    /// Peticiones para un aparato que no es el prestado.
    pub ajenas: u64,
    /// Con un largo de más, un punto que no existe, isócronas, una preparación que no cuadra, un `seqnum` repetido.
    pub malas: u64,
    /// Veces que no se leyó del tubo por tener la espera llena (el dominio va más deprisa de lo que se atiende).
    pub frenadas: u64,
}

/// El servidor de USB/IP de un préstamo: un aparato prestado a un dominio, desde que el dominio abre el tubo.
pub struct Servidor {
    devid: u32,
    leido: u32,
    escrito: u32,
    urbs: VecDeque<Urb>,
    /// Ranuras de búfer libres.
    libres: u32,
    siguiente: u32,
    salida: VecDeque<Vec<u8>>,
    /// Lo que ya se escribió de la primera de `salida`.
    salida_hecho: usize,
    salida_bytes: usize,
    espera_bytes: usize,
    /// Las etiquetas por cancelar en el xHCI (de `CMD_UNLINK` de peticiones en marcha), para el próximo `despachar`.
    cancelar_pedidas: Vec<u32>,
    pub cuentas: Cuentas,
}

impl Servidor {
    /// Un servidor para el aparato `devid` (`busnum << 16 | devnum`, el que se dio al dominio para enganchar).
    pub fn nuevo(devid: u32) -> Self {
        Servidor {
            devid,
            leido: 0,
            escrito: 0,
            urbs: VecDeque::new(),
            libres: (1u32 << RANURAS) - 1,
            siguiente: 1,
            salida: VecDeque::new(),
            salida_hecho: 0,
            salida_bytes: 0,
            espera_bytes: 0,
            cancelar_pedidas: Vec::new(),
            cuentas: Cuentas::default(),
        }
    }

    /// Peticiones sin acabar (en espera o en el xHCI).
    pub fn pendientes(&self) -> usize {
        self.urbs.len()
    }

    /// En el xHCI ahora.
    pub fn en_marcha(&self) -> usize {
        self.urbs.iter().filter(|u| u.estado != Estado::Esperando).count()
    }

    fn contestar(&mut self, r: Vec<u8>) {
        self.salida_bytes += r.len();
        self.salida.push_back(r);
    }

    /// Lee del tubo las peticiones enteras que haya (y caben en la espera). Las que no valen se contestan aquí con un
    /// error. `Err` si el flujo está roto.
    pub fn leer_tubo(&mut self, m: &mut dyn MemoriaTubo) -> Result<(), Roto> {
        loop {
            if self.urbs.len() >= MAX_ESPERA || self.espera_bytes >= MAX_ESPERA_BYTES {
                self.cuentas.frenadas += 1;
                return Ok(());
            }
            let hay = disponibles(m, self.leido)?;
            if (hay as usize) < CABECERA {
                return Ok(());
            }
            let mut c = [0u8; CABECERA];
            copiar_de_anillo(m, T_IDA, self.leido, &mut c);
            let (orden, seqnum, devid, dir, ep) = (be32(&c, 0), be32(&c, 4), be32(&c, 8), be32(&c, 12), be32(&c, 16));
            match orden {
                CMD_SUBMIT => {
                    let (flags, largo, paquetes) = (be32(&c, 20), be32(&c, 24), be32(&c, 32));
                    if dir > 1 {
                        return Err(Roto("una petición sin dirección"));
                    }
                    let entrada = dir == 1;
                    // Lo que viene detrás en el flujo: los datos de salida y los descriptores de isócronas.
                    let iso = if paquetes == 0 || paquetes == u32::MAX { 0 } else { paquetes as u64 };
                    if iso > 1024 {
                        return Err(Roto("demasiadas isócronas"));
                    }
                    let datos = if entrada { 0 } else { largo as u64 };
                    if datos > MAX_DATOS as u64 {
                        return Err(Roto("datos de salida de más"));
                    }
                    let total = CABECERA as u64 + datos + 16 * iso;
                    if total > hay as u64 {
                        return Ok(()); // aún no ha llegado entera
                    }
                    let mut salida = vec![0u8; datos as usize];
                    copiar_de_anillo(m, T_IDA, self.leido.wrapping_add(CABECERA as u32), &mut salida);
                    self.leido = self.leido.wrapping_add(total as u32);
                    self.cuentas.peticiones += 1;
                    let mut setup = [0u8; 8];
                    setup.copy_from_slice(&c[40..48]);
                    let error = if devid != self.devid {
                        self.cuentas.ajenas += 1;
                        Some(ENODEV)
                    } else if ep > 15 || iso > 0 || largo as usize > MAX_DATOS || self.urbs.iter().any(|u| u.seqnum == seqnum) {
                        self.cuentas.malas += 1;
                        Some(EINVAL)
                    } else if ep == 0 && (((setup[0] & 0x80) != 0) != entrada || u16::from_le_bytes([setup[6], setup[7]]) as u32 != largo) {
                        self.cuentas.malas += 1;
                        Some(EINVAL)
                    } else {
                        None
                    };
                    if let Some(e) = error {
                        self.contestar(respuesta(RET_SUBMIT, seqnum, e, 0, &[]));
                        continue;
                    }
                    let etiqueta = self.siguiente;
                    self.siguiente = self.siguiente.wrapping_add(1).max(1);
                    let punto = if ep == 0 { 0 } else { ep as u8 | if entrada { 0x80 } else { 0 } };
                    self.espera_bytes += salida.len();
                    self.urbs.push_back(Urb { seqnum, etiqueta, punto, entrada, largo, flags, setup, salida, estado: Estado::Esperando });
                }
                CMD_UNLINK => {
                    self.leido = self.leido.wrapping_add(CABECERA as u32);
                    let cual = be32(&c, 20);
                    match self.urbs.iter().position(|u| u.seqnum == cual) {
                        Some(i) => match self.urbs[i].estado {
                            Estado::Esperando => {
                                // Aún no salió: se quita y basta (sin RET_SUBMIT, como dice el protocolo).
                                let u = self.urbs.remove(i).unwrap();
                                self.espera_bytes -= u.salida.len();
                                self.cuentas.canceladas += 1;
                                self.contestar(respuesta(RET_UNLINK, seqnum, ECONNRESET, 0, &[]));
                            }
                            Estado::EnMarcha(r) => {
                                self.urbs[i].estado = Estado::Cancelando(r, seqnum);
                                self.cancelar_pedidas.push(self.urbs[i].etiqueta);
                            }
                            // Otra vez la misma: la primera ya se contestará.
                            Estado::Cancelando(..) => self.contestar(respuesta(RET_UNLINK, seqnum, 0, 0, &[])),
                        },
                        // Ya acabó (o nunca existió): status 0.
                        None => self.contestar(respuesta(RET_UNLINK, seqnum, 0, 0, &[])),
                    }
                }
                _ => return Err(Roto("una orden de USB/IP que no existe")),
            }
            m.escribir32(T_IDA_LEIDO, self.leido);
        }
    }

    /// Lo que se puede mandar ya al xHCI, sin pasar de lo que se deja en marcha. Los datos de salida van a su ranura
    /// de búfer. Las cancelaciones pedidas desde la última vez, también.
    pub fn despachar(&mut self, b: &mut dyn Buferes) -> Vec<Peticion> {
        let mut r = Vec::new();
        r.extend(self.cancelar_pedidas.drain(..).map(|etiqueta| Peticion::Cancelar { etiqueta }));
        if self.salida_bytes >= MAX_SALIDA_BYTES {
            return r;
        }
        let en_marcha = |u: &&Urb| u.estado != Estado::Esperando && u.clave() != 0;
        let mut entrada = self.urbs.iter().filter(|u| u.entrada).filter(en_marcha).count();
        let mut salida = self.urbs.iter().filter(|u| !u.entrada).filter(en_marcha).count();
        for i in 0..self.urbs.len() {
            if self.libres == 0 {
                break;
            }
            let u = &self.urbs[i];
            if u.estado != Estado::Esperando {
                continue;
            }
            // En orden por punto: si hay otra del mismo punto esperando antes, esta espera.
            let clave = u.clave();
            if self.urbs.iter().take(i).any(|v| v.clave() == clave && v.estado == Estado::Esperando) {
                continue;
            }
            let del_punto = self.urbs.iter().filter(|v| v.clave() == clave && v.estado != Estado::Esperando).count();
            let tope = if clave == 0 { 1 } else { MAX_POR_PUNTO };
            if del_punto >= tope || (clave != 0 && ((u.entrada && entrada >= MAX_ENTRADA) || (!u.entrada && salida >= MAX_SALIDA))) {
                continue;
            }
            let ranura = self.libres.trailing_zeros() as u8;
            self.libres &= !(1 << ranura);
            let u = &mut self.urbs[i];
            if !u.salida.is_empty() {
                b.escribir(ranura as usize, &u.salida);
                self.espera_bytes -= u.salida.len();
                u.salida = Vec::new();
            }
            u.estado = Estado::EnMarcha(ranura);
            if clave != 0 && u.entrada {
                entrada += 1;
            } else if clave != 0 {
                salida += 1;
            }
            r.push(if u.punto & 0x7f == 0 {
                Peticion::Control { etiqueta: u.etiqueta, ranura, setup: u.setup, largo: u.largo }
            } else {
                let cero = u.flags & URB_ZERO_PACKET != 0 && !u.entrada;
                Peticion::Transferir { etiqueta: u.etiqueta, ranura, punto: u.punto, largo: u.largo, cero }
            });
        }
        r
    }

    /// Acabó en el xHCI la de `etiqueta`: con `estado` (0, o un error de Linux: [`EPIPE`], [`ECONNRESET`]...) y `hecho`
    /// bytes. Los de entrada se leen de su ranura. Una etiqueta que no es de nada se ignora (falso).
    pub fn hecho(&mut self, etiqueta: u32, estado: i32, hecho: u32, b: &dyn Buferes) -> bool {
        let Some(i) = self.urbs.iter().position(|u| u.etiqueta == etiqueta && u.estado != Estado::Esperando) else { return false };
        let u = self.urbs.remove(i).unwrap();
        let ranura = match u.estado {
            Estado::EnMarcha(r) | Estado::Cancelando(r, _) => r,
            Estado::Esperando => unreachable!(),
        };
        self.libres |= 1 << ranura;
        let hecho = hecho.min(u.largo);
        // Corta con URB_SHORT_NOT_OK: un error, con lo que llegó.
        let estado = if estado == 0 && u.entrada && u.flags & URB_SHORT_NOT_OK != 0 && hecho < u.largo { EREMOTEIO } else { estado };
        match u.estado {
            Estado::Cancelando(_, unlink) if estado == ECONNRESET => {
                // Cancelada: solo RET_UNLINK.
                self.cuentas.canceladas += 1;
                self.contestar(respuesta(RET_UNLINK, unlink, ECONNRESET, 0, &[]));
            }
            _ => {
                let mut datos = vec![0u8; if u.entrada { hecho as usize } else { 0 }];
                if !datos.is_empty() {
                    b.leer(ranura as usize, &mut datos);
                }
                self.cuentas.hechas += 1;
                self.contestar(respuesta(RET_SUBMIT, u.seqnum, estado, hecho, &datos));
                if let Estado::Cancelando(_, unlink) = u.estado {
                    // Acabó antes de que llegara la cancelación: RET_SUBMIT y, después, RET_UNLINK con 0.
                    self.contestar(respuesta(RET_UNLINK, unlink, 0, 0, &[]));
                }
            }
        }
        true
    }

    /// Todo lo que había en el xHCI se dio por acabado sin contestar (el aparato se fue o se devolvió): las ranuras
    /// vuelven y cada una se contesta con `estado`.
    pub fn todo_acabado(&mut self, estado: i32) {
        let etiquetas: Vec<u32> = self.urbs.iter().filter(|u| u.estado != Estado::Esperando).map(|u| u.etiqueta).collect();
        struct Nada;
        impl Buferes for Nada {
            fn escribir(&mut self, _: usize, _: &[u8]) {}
            fn leer(&self, _: usize, _: &mut [u8]) {}
        }
        for e in etiquetas {
            self.hecho(e, estado, 0, &Nada);
        }
        self.cancelar_pedidas.clear();
    }

    /// Escribe en la vuelta del tubo las respuestas que quepan (enteras o a trozos). Si escribió algo (hay que avisar
    /// al dominio).
    pub fn escribir_tubo(&mut self, m: &mut dyn MemoriaTubo) -> Result<bool, Roto> {
        let leido = m.leer32(T_VUELTA_LEIDO);
        let ocupado = self.escrito.wrapping_sub(leido);
        if ocupado > TAM_ANILLO {
            return Err(Roto("el índice de la vuelta del tubo es imposible"));
        }
        let mut sitio = (TAM_ANILLO - ocupado) as usize;
        let mut algo = false;
        while sitio > 0 {
            let Some(r) = self.salida.front() else { break };
            let resto = &r[self.salida_hecho..];
            let n = resto.len().min(sitio);
            copiar_a_anillo(m, T_VUELTA, self.escrito, &resto[..n]);
            self.escrito = self.escrito.wrapping_add(n as u32);
            sitio -= n;
            algo = true;
            if n == resto.len() {
                self.salida_bytes -= r.len();
                self.salida.pop_front();
                self.salida_hecho = 0;
            } else {
                self.salida_hecho += n;
            }
        }
        if algo {
            m.barrera();
            m.escribir32(T_VUELTA_ESCRITO, self.escrito);
        }
        Ok(algo)
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    struct Tubo(Vec<u8>);
    impl MemoriaTubo for Tubo {
        fn leer32(&self, off: u64) -> u32 {
            u32::from_le_bytes(self.0[off as usize..off as usize + 4].try_into().unwrap())
        }
        fn escribir32(&mut self, off: u64, v: u32) {
            self.0[off as usize..off as usize + 4].copy_from_slice(&v.to_le_bytes());
        }
        fn leer(&self, off: u64, buf: &mut [u8]) {
            buf.copy_from_slice(&self.0[off as usize..off as usize + buf.len()]);
        }
        fn escribir(&mut self, off: u64, d: &[u8]) {
            self.0[off as usize..off as usize + d.len()].copy_from_slice(d);
        }
        fn barrera(&self) {}
    }

    /// El cliente de las pruebas: escribe en la ida y lee de la vuelta, como el /init del dominio.
    struct Cliente {
        t: Tubo,
        ida: u32,
        vuelta: u32,
        /// Los seqnum de entrada (su RET_SUBMIT trae datos), como sabe vhci-hcd.
        entradas: Vec<u32>,
    }

    impl Cliente {
        fn nuevo() -> Self {
            Cliente { t: Tubo(vec![0; TAM_TUBO as usize]), ida: 0, vuelta: 0, entradas: Vec::new() }
        }
        fn mandar(&mut self, b: &[u8]) -> bool {
            let leido = self.t.leer32(T_IDA_LEIDO);
            if (TAM_ANILLO - self.ida.wrapping_sub(leido)) < b.len() as u32 {
                return false;
            }
            if b.len() >= CABECERA && be32(b, 0) == CMD_SUBMIT && be32(b, 12) == 1 {
                self.entradas.push(be32(b, 4));
            }
            copiar_a_anillo(&mut self.t, T_IDA, self.ida, b);
            self.ida = self.ida.wrapping_add(b.len() as u32);
            self.t.escribir32(T_IDA_ESCRITO, self.ida);
            true
        }
        /// Las respuestas que hay: (orden, seqnum, estado, largo, datos).
        fn respuestas(&mut self) -> Vec<(u32, u32, i32, u32, Vec<u8>)> {
            let mut r = Vec::new();
            loop {
                let escrito = self.t.leer32(T_VUELTA_ESCRITO);
                let hay = escrito.wrapping_sub(self.vuelta);
                if (hay as usize) < CABECERA {
                    break;
                }
                let mut c = [0u8; CABECERA];
                copiar_de_anillo(&self.t, T_VUELTA, self.vuelta, &mut c);
                let largo = be32(&c, 24);
                let datos = if be32(&c, 0) == RET_SUBMIT && self.entradas.contains(&be32(&c, 4)) { largo as usize } else { 0 };
                if (hay as usize) < CABECERA + datos {
                    break;
                }
                let mut d = vec![0u8; datos];
                copiar_de_anillo(&self.t, T_VUELTA, self.vuelta.wrapping_add(CABECERA as u32), &mut d);
                self.vuelta = self.vuelta.wrapping_add((CABECERA + datos) as u32);
                self.t.escribir32(T_VUELTA_LEIDO, self.vuelta);
                r.push((be32(&c, 0), be32(&c, 4), be32(&c, 20) as i32, largo, d));
            }
            r
        }
    }

    fn submit(seq: u32, devid: u32, entrada: bool, ep: u32, largo: u32, flags: u32, setup: [u8; 8], datos: &[u8]) -> Vec<u8> {
        let mut c = vec![0u8; CABECERA];
        for (i, v) in [CMD_SUBMIT, seq, devid, entrada as u32, ep, flags, largo, 0, u32::MAX, 0].iter().enumerate() {
            c[4 * i..4 * i + 4].copy_from_slice(&v.to_be_bytes());
        }
        c[40..48].copy_from_slice(&setup);
        c.extend_from_slice(datos);
        c
    }

    fn unlink(seq: u32, devid: u32, cual: u32) -> Vec<u8> {
        let mut c = vec![0u8; CABECERA];
        for (i, v) in [CMD_UNLINK, seq, devid, 0, 0, cual].iter().enumerate() {
            c[4 * i..4 * i + 4].copy_from_slice(&v.to_be_bytes());
        }
        c
    }

    const DEV: u32 = 0x0001_0002;
    const DESCRIPTOR: [u8; 8] = [0x80, 6, 0, 1, 0, 0, 18, 0];

    struct Buf(Vec<Vec<u8>>);
    impl Buf {
        fn nuevo() -> Self {
            Buf(vec![vec![0; MAX_DATOS]; RANURAS])
        }
    }
    impl Buferes for Buf {
        fn escribir(&mut self, r: usize, d: &[u8]) {
            self.0[r][..d.len()].copy_from_slice(d);
        }
        fn leer(&self, r: usize, b: &mut [u8]) {
            b.copy_from_slice(&self.0[r][..b.len()]);
        }
    }

    #[test]
    fn de_punta_a_punta() {
        let mut c = Cliente::nuevo();
        let mut s = Servidor::nuevo(DEV);
        let mut b = Buf::nuevo();
        c.mandar(&submit(7, DEV, true, 0, 18, 0, DESCRIPTOR, &[]));
        c.mandar(&submit(8, DEV, false, 2, 5, 0, [0; 8], b"hola!"));
        s.leer_tubo(&mut c.t).unwrap();
        let p = s.despachar(&mut b);
        assert_eq!(p.len(), 2);
        let Peticion::Control { etiqueta: e1, ranura: r1, setup, largo: 18 } = p[0] else { panic!("{p:?}") };
        assert_eq!(setup, DESCRIPTOR);
        let Peticion::Transferir { etiqueta: e2, ranura: r2, punto: 2, largo: 5, cero: false } = p[1] else { panic!("{p:?}") };
        assert_eq!(&b.0[r2 as usize][..5], b"hola!");
        b.0[r1 as usize][..18].copy_from_slice(&[0x12; 18]);
        assert!(s.hecho(e1, 0, 18, &b));
        assert!(s.hecho(e2, 0, 5, &b));
        assert!(!s.hecho(e2, 0, 5, &b), "otra vez la misma: no es de nada");
        assert!(s.escribir_tubo(&mut c.t).unwrap());
        let r = c.respuestas();
        assert_eq!(r, vec![(RET_SUBMIT, 7, 0, 18, vec![0x12; 18]), (RET_SUBMIT, 8, 0, 5, vec![])]);
        assert_eq!(s.pendientes(), 0);
    }

    #[test]
    fn peticiones_que_no_valen() {
        let mut c = Cliente::nuevo();
        let mut s = Servidor::nuevo(DEV);
        let mut b = Buf::nuevo();
        // Otro aparato (también con datos de salida, que se saltan), un punto que no existe, un largo enorme, una
        // preparación que no cuadra con el largo, isócronas y un seqnum repetido.
        c.mandar(&submit(1, 0x0001_0001, true, 0, 18, 0, DESCRIPTOR, &[]));
        c.mandar(&submit(2, 0xdead_beef, false, 1, 3, 0, [0; 8], b"abc"));
        c.mandar(&submit(3, DEV, true, 16, 64, 0, [0; 8], &[]));
        c.mandar(&submit(4, DEV, true, 1, u32::MAX, 0, [0; 8], &[]));
        c.mandar(&submit(5, DEV, true, 0, 255, 0, DESCRIPTOR, &[]));
        c.mandar(&submit(6, DEV, false, 0, 18, 0, DESCRIPTOR, &[0; 18]));
        let mut iso = submit(7, DEV, false, 1, 4, 0, [0; 8], b"isoc");
        iso[32..36].copy_from_slice(&2u32.to_be_bytes());
        iso.extend_from_slice(&[0xee; 32]);
        c.mandar(&iso);
        c.mandar(&submit(8, DEV, true, 1, 64, 0, [0; 8], &[]));
        c.mandar(&submit(8, DEV, true, 1, 64, 0, [0; 8], &[]));
        s.leer_tubo(&mut c.t).unwrap();
        let p = s.despachar(&mut b);
        assert_eq!(p.len(), 1, "solo la 8: {p:?}");
        s.escribir_tubo(&mut c.t).unwrap();
        let r = c.respuestas();
        let estados: Vec<(u32, i32)> = r.iter().map(|x| (x.1, x.2)).collect();
        assert_eq!(estados, vec![(1, ENODEV), (2, ENODEV), (3, EINVAL), (4, EINVAL), (5, EINVAL), (6, EINVAL), (7, EINVAL), (8, EINVAL)]);
        assert_eq!(s.cuentas.ajenas, 2);
        assert_eq!(s.cuentas.malas, 6);
        // El flujo siguió en orden: lo siguiente se lee bien.
        c.mandar(&submit(9, DEV, true, 0, 18, 0, DESCRIPTOR, &[]));
        s.leer_tubo(&mut c.t).unwrap();
        assert_eq!(s.pendientes(), 2);
    }

    #[test]
    fn lo_que_rompe_el_flujo() {
        let roto = |m: Vec<u8>| {
            let mut c = Cliente::nuevo();
            let mut s = Servidor::nuevo(DEV);
            c.mandar(&m);
            s.leer_tubo(&mut c.t)
        };
        let mut orden = submit(1, DEV, true, 0, 18, 0, DESCRIPTOR, &[]);
        orden[3] = 0x77;
        assert!(roto(orden).is_err());
        let mut sin_dir = submit(1, DEV, true, 1, 8, 0, [0; 8], &[]);
        sin_dir[15] = 2;
        assert!(roto(sin_dir).is_err());
        assert!(roto(submit(1, DEV, false, 1, MAX_DATOS as u32 + 1, 0, [0; 8], &[])).is_err());
        let mut iso = submit(1, DEV, true, 1, 8, 0, [0; 8], &[]);
        iso[32..36].copy_from_slice(&5000u32.to_be_bytes());
        assert!(roto(iso).is_err());
        // Un índice imposible.
        let mut c = Cliente::nuevo();
        let mut s = Servidor::nuevo(DEV);
        c.t.escribir32(T_IDA_ESCRITO, TAM_ANILLO + 1);
        assert!(s.leer_tubo(&mut c.t).is_err());
        c.t.escribir32(T_IDA_ESCRITO, 0);
        c.t.escribir32(T_VUELTA_LEIDO, 5);
        s.contestar(respuesta(RET_UNLINK, 1, 0, 0, &[]));
        assert!(s.escribir_tubo(&mut c.t).is_err());
    }

    #[test]
    fn lo_que_se_deja_en_marcha() {
        let mut c = Cliente::nuevo();
        let mut s = Servidor::nuevo(DEV);
        let mut b = Buf::nuevo();
        // 10 de entrada en cada uno de cuatro puntos: 4 por punto y 12 de entrada en total.
        let mut seq = 0;
        for _ in 0..10 {
            for ep in 1..=4 {
                seq += 1;
                c.mandar(&submit(seq, DEV, true, ep, 512, 0, [0; 8], &[]));
            }
        }
        // Y de salida y control, que tienen que poder seguir.
        for i in 0..6 {
            c.mandar(&submit(100 + i, DEV, false, 5, 4, 0, [0; 8], b"sale"));
            c.mandar(&submit(200 + i, DEV, true, 0, 18, 0, DESCRIPTOR, &[]));
        }
        s.leer_tubo(&mut c.t).unwrap();
        let p = s.despachar(&mut b);
        let entrada = p.iter().filter(|x| matches!(x, Peticion::Transferir { punto, .. } if punto & 0x80 != 0)).count();
        let salida = p.iter().filter(|x| matches!(x, Peticion::Transferir { punto: 5, .. })).count();
        let control = p.iter().filter(|x| matches!(x, Peticion::Control { .. })).count();
        assert_eq!((entrada, salida, control), (MAX_ENTRADA, MAX_SALIDA, 1), "{p:?}");
        assert_eq!(p.len(), RANURAS);
        assert!(s.despachar(&mut b).is_empty());
        // Acaba la de control: sale la siguiente de control, en orden.
        let e = p.iter().find_map(|x| if let Peticion::Control { etiqueta, .. } = x { Some(*etiqueta) } else { None }).unwrap();
        s.hecho(e, EPIPE, 0, &b);
        let p2 = s.despachar(&mut b);
        assert!(matches!(p2[..], [Peticion::Control { .. }]), "{p2:?}");
        s.escribir_tubo(&mut c.t).unwrap();
        assert_eq!(c.respuestas(), vec![(RET_SUBMIT, 200, EPIPE, 0, vec![])]);
    }

    #[test]
    fn cancelar() {
        let mut c = Cliente::nuevo();
        let mut s = Servidor::nuevo(DEV);
        let mut b = Buf::nuevo();
        for i in 1..=6 {
            c.mandar(&submit(i, DEV, true, 1, 64, 0, [0; 8], &[]));
        }
        s.leer_tubo(&mut c.t).unwrap();
        let p = s.despachar(&mut b);
        assert_eq!(p.len(), MAX_POR_PUNTO);
        let etiqueta = |p: &Peticion| if let Peticion::Transferir { etiqueta, .. } = p { *etiqueta } else { 0 };
        // La 6 aún espera: se quita sin más. La 1 está en marcha: se pide cancelar. La 99 no existe.
        c.mandar(&unlink(50, DEV, 6));
        c.mandar(&unlink(51, DEV, 1));
        c.mandar(&unlink(52, DEV, 2));
        c.mandar(&unlink(53, DEV, 99));
        s.leer_tubo(&mut c.t).unwrap();
        let q = s.despachar(&mut b);
        assert_eq!(q, vec![Peticion::Cancelar { etiqueta: etiqueta(&p[0]) }, Peticion::Cancelar { etiqueta: etiqueta(&p[1]) }]);
        // La 1 se cancela; la 2 acaba antes de que llegue la cancelación.
        s.hecho(etiqueta(&p[0]), ECONNRESET, 0, &b);
        s.hecho(etiqueta(&p[1]), 0, 64, &b);
        s.escribir_tubo(&mut c.t).unwrap();
        let r: Vec<(u32, u32, i32)> = c.respuestas().iter().map(|x| (x.0, x.1, x.2)).collect();
        assert_eq!(
            r,
            vec![
                (RET_UNLINK, 50, ECONNRESET),
                (RET_UNLINK, 53, 0),
                (RET_UNLINK, 51, ECONNRESET),
                (RET_SUBMIT, 2, 0),
                (RET_UNLINK, 52, 0),
            ]
        );
        // El aparato se va: lo que quedaba en marcha (3, 4 y la 5, que entró al acabar la 1) se contesta con ENODEV.
        s.despachar(&mut b);
        s.todo_acabado(ENODEV);
        s.escribir_tubo(&mut c.t).unwrap();
        let r: Vec<(u32, i32)> = c.respuestas().iter().map(|x| (x.1, x.2)).collect();
        assert_eq!(r, vec![(3, ENODEV), (4, ENODEV), (5, ENODEV)]);
        assert_eq!(s.pendientes(), 0);
    }

    #[test]
    fn tormenta_y_vuelta_llena() {
        let mut c = Cliente::nuevo();
        let mut s = Servidor::nuevo(DEV);
        let mut b = Buf::nuevo();
        // Miles de peticiones: se leen hasta llenar la espera y el resto se queda en el tubo (frenado).
        let mut seq = 0u32;
        let mut mandadas = 0;
        for _ in 0..20 {
            while c.mandar(&submit(seq, DEV, true, 1 + seq % 15, 4096, 0, [0; 8], &[])) {
                seq += 1;
                mandadas += 1;
            }
            s.leer_tubo(&mut c.t).unwrap();
        }
        assert!(mandadas > MAX_ESPERA);
        assert_eq!(s.pendientes(), MAX_ESPERA);
        assert!(s.cuentas.frenadas > 0);
        // Todas acaban con 4 KB de datos: las respuestas no caben de golpe en la vuelta; se escriben a trozos según el
        // dominio lee, y no se manda nada más al xHCI mientras haya demasiado por escribir.
        let mut total = 0;
        let mut vueltas = 0;
        while total < mandadas && vueltas < 100_000 {
            vueltas += 1;
            for p in s.despachar(&mut b) {
                if let Peticion::Transferir { etiqueta, .. } = p {
                    s.hecho(etiqueta, 0, 4096, &b);
                }
            }
            assert!(s.salida_bytes <= MAX_SALIDA_BYTES + RANURAS * (CABECERA + MAX_DATOS));
            s.escribir_tubo(&mut c.t).unwrap();
            total += c.respuestas().len();
            s.leer_tubo(&mut c.t).unwrap();
        }
        assert_eq!(total, mandadas);
    }

    #[test]
    fn basura_al_azar_sin_panico() {
        let mut x = 0x1234_5678_9abc_def0u64;
        let mut azar = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for _ in 0..300 {
            let mut c = Cliente::nuevo();
            let mut s = Servidor::nuevo(DEV);
            let mut b = Buf::nuevo();
            for _ in 0..50 {
                let n = (azar() % 200) as usize;
                let mut m: Vec<u8> = (0..n).map(|_| azar() as u8).collect();
                if azar() % 2 == 0 && m.len() >= CABECERA {
                    // Que parezca una orden.
                    m[..4].copy_from_slice(&((azar() % 3) as u32).to_be_bytes());
                    m[8..12].copy_from_slice(&DEV.to_be_bytes());
                    m[12..16].copy_from_slice(&((azar() % 2) as u32).to_be_bytes());
                    m[24..28].copy_from_slice(&((azar() % 100) as u32).to_be_bytes());
                }
                c.mandar(&m);
                if s.leer_tubo(&mut c.t).is_err() {
                    break;
                }
                for p in s.despachar(&mut b) {
                    match p {
                        Peticion::Control { etiqueta, .. } | Peticion::Transferir { etiqueta, .. } => {
                            s.hecho(etiqueta, 0, azar() as u32, &b);
                        }
                        Peticion::Cancelar { etiqueta } => {
                            s.hecho(etiqueta, ECONNRESET, 0, &b);
                        }
                    }
                }
                let _ = s.escribir_tubo(&mut c.t);
                c.respuestas();
            }
        }
    }
}
