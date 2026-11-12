//! virtio-vsock (virtio 1.2 §5.10): conexiones de flujo entre el invitado y DashOS, sin red. Lo usa el puente de
//! Wayland (G1 de `docs/GRAFICOS_LOOP.md`): el puente del invitado conecta con un puerto de DashOS y por ahí pasan
//! sus tramas.
//!
//! Tres colas: 0 (rx), búferes del invitado para lo que le llega; 1 (tx), lo que manda; 2 (event), que aquí no se usa.
//! Cada paquete lleva delante una cabecera de 44 bytes (little-endian): src_cid u64, dst_cid u64, src_port u32,
//! dst_port u32, len u32, type u16, op u16, flags u32, buf_alloc u32 y fwd_cnt u32.
//!
//! El invitado es el CID 3 y DashOS el 2 (el anfitrión). Solo conecta el invitado: DashOS escucha en los puertos que
//! diga su [`ExtremoVsock`]. El control de flujo es el de la norma: cada lado dice cuánto sitio tiene (`buf_alloc`) y
//! cuánto ha consumido ya (`fwd_cnt`), y el otro no manda más de lo que cabe. Lo que el invitado manda sin crédito,
//! con cabeceras falsas o con tamaños absurdos corta su conexión (RST) o se tira y se cuenta; el aparato no se para.

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;

use super::cola::ColaPartida;
use super::consola::copiar_config;
use super::{AparatoVirtio, ErrorVirtio, F_INDIRECT_DESC, vaciar_cola};
use crate::memoria::MemoriaInvitado;

pub const ID_VSOCK: u32 = 19;
pub const CID_ANFITRION: u64 = 2;
pub const CID_INVITADO: u64 = 3;
pub const TAM_CABECERA: usize = 44;

pub const TIPO_STREAM: u16 = 1;
pub const OP_REQUEST: u16 = 1;
pub const OP_RESPONSE: u16 = 2;
pub const OP_RST: u16 = 3;
pub const OP_SHUTDOWN: u16 = 4;
pub const OP_RW: u16 = 5;
pub const OP_CREDIT_UPDATE: u16 = 6;
pub const OP_CREDIT_REQUEST: u16 = 7;
/// Para un paquete con un tamaño imposible: ninguna orden de la norma.
const OP_INVALIDA: u16 = u16::MAX;

/// El sitio que DashOS anuncia por conexión: lo que el invitado puede tener mandado y aún sin consumir.
pub const BUF_ALLOC: u32 = 256 * 1024;
/// Lo más que lleva un paquete (lo mismo que Linux): uno más grande es de un invitado roto.
pub const MAX_PAQUETE: u32 = 64 * 1024;
/// Conexiones a la vez: más, y se contesta RST.
pub const MAX_CONEXIONES: usize = 64;
/// Paquetes de control (respuestas, RST, crédito) que esperan sitio en rx: más, y se tiran los más viejos.
const MAX_CONTROL: usize = 256;

const COLA_RX: usize = 0;
const COLA_TX: usize = 1;

/// El lado de DashOS. `con` es el número que el aparato da a cada conexión (no se repite).
pub trait ExtremoVsock {
    /// El invitado quiere conectar con el puerto `puerto` de DashOS. Si nadie escucha, `false` (RST).
    fn conectar(&mut self, con: u32, puerto: u32) -> bool;
    /// Bytes del invitado. Devuelve cuántos toma; el resto espera en el aparato (y le cuenta como crédito usado).
    fn recibir(&mut self, con: u32, datos: &[u8]) -> usize;
    /// La conexión se acabó (el invitado la cerró, la rompió o reinició el aparato).
    fn cerrada(&mut self, con: u32);
    /// Saca hasta `buf.len()` bytes para el invitado por `con`; devuelve cuántos.
    fn enviar(&mut self, con: u32, buf: &mut [u8]) -> usize;
    /// Si hay algo para el invitado (bytes o cierres): lo mira el aparato al sondear.
    fn hay_algo(&self) -> bool;
    /// Una conexión que DashOS quiere cerrar.
    fn cerrar(&mut self) -> Option<u32>;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cabecera {
    pub src_cid: u64,
    pub dst_cid: u64,
    pub src_port: u32,
    pub dst_port: u32,
    pub len: u32,
    pub tipo: u16,
    pub op: u16,
    pub flags: u32,
    pub buf_alloc: u32,
    pub fwd_cnt: u32,
}

impl Cabecera {
    pub fn leer(b: &[u8; TAM_CABECERA]) -> Self {
        let u32_ = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        let u16_ = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        let u64_ = |i: usize| u32_(i) as u64 | (u32_(i + 4) as u64) << 32;
        Self {
            src_cid: u64_(0),
            dst_cid: u64_(8),
            src_port: u32_(16),
            dst_port: u32_(20),
            len: u32_(24),
            tipo: u16_(28),
            op: u16_(30),
            flags: u32_(32),
            buf_alloc: u32_(36),
            fwd_cnt: u32_(40),
        }
    }

    pub fn bytes(&self) -> [u8; TAM_CABECERA] {
        let mut b = [0u8; TAM_CABECERA];
        b[0..8].copy_from_slice(&self.src_cid.to_le_bytes());
        b[8..16].copy_from_slice(&self.dst_cid.to_le_bytes());
        b[16..20].copy_from_slice(&self.src_port.to_le_bytes());
        b[20..24].copy_from_slice(&self.dst_port.to_le_bytes());
        b[24..28].copy_from_slice(&self.len.to_le_bytes());
        b[28..30].copy_from_slice(&self.tipo.to_le_bytes());
        b[30..32].copy_from_slice(&self.op.to_le_bytes());
        b[32..36].copy_from_slice(&self.flags.to_le_bytes());
        b[36..40].copy_from_slice(&self.buf_alloc.to_le_bytes());
        b[40..44].copy_from_slice(&self.fwd_cnt.to_le_bytes());
        b
    }
}

struct Conexion {
    id: u32,
    /// Lo que mandó el invitado y el extremo aún no tomó.
    pendiente: VecDeque<u8>,
    /// Bytes del invitado ya tomados por el extremo (el `fwd_cnt` de DashOS).
    consumido: u32,
    /// El último `fwd_cnt` que se le dijo al invitado.
    anunciado: u32,
    /// Bytes mandados al invitado (para su crédito).
    enviado: u32,
    /// Lo que dijo el invitado de su sitio.
    su_buf: u32,
    su_fwd: u32,
}

impl Conexion {
    /// Lo que cabe aún en el invitado.
    fn credito(&self) -> u32 {
        self.su_buf.saturating_sub(self.enviado.wrapping_sub(self.su_fwd))
    }
}

/// Cuentas para el registro y las pruebas.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Cuentas {
    pub conexiones: u64,
    pub rechazadas: u64,
    /// Paquetes del invitado que no valían (cabecera corta, CID falso, tipo o tamaño): tirados.
    pub malos: u64,
    /// Conexiones cortadas por pasarse del crédito o por un paquete sin sentido.
    pub cortadas: u64,
    pub bytes_del_invitado: u64,
    pub bytes_al_invitado: u64,
}

pub struct Vsock {
    extremo: Box<dyn ExtremoVsock>,
    /// (puerto del invitado, puerto de DashOS) → conexión.
    conexiones: BTreeMap<(u32, u32), Conexion>,
    control: VecDeque<Cabecera>,
    siguiente: u32,
    pub cuentas: Cuentas,
}

impl Vsock {
    pub fn nuevo(extremo: Box<dyn ExtremoVsock>) -> Self {
        Self { extremo, conexiones: BTreeMap::new(), control: VecDeque::new(), siguiente: 0, cuentas: Cuentas::default() }
    }

    fn respuesta(&self, op: u16, su_puerto: u32, mi_puerto: u32, flags: u32) -> Cabecera {
        let consumido = self.conexiones.get(&(su_puerto, mi_puerto)).map_or(0, |c| c.consumido);
        Cabecera {
            src_cid: CID_ANFITRION,
            dst_cid: CID_INVITADO,
            src_port: mi_puerto,
            dst_port: su_puerto,
            len: 0,
            tipo: TIPO_STREAM,
            op,
            flags,
            buf_alloc: BUF_ALLOC,
            fwd_cnt: consumido,
        }
    }

    fn encolar(&mut self, c: Cabecera) {
        if self.control.len() >= MAX_CONTROL {
            self.control.pop_front();
        }
        self.control.push_back(c);
    }

    fn rst(&mut self, su_puerto: u32, mi_puerto: u32) {
        let c = self.respuesta(OP_RST, su_puerto, mi_puerto, 0);
        self.encolar(c);
    }

    /// Quita una conexión y avisa al extremo.
    fn quitar(&mut self, clave: (u32, u32)) {
        if let Some(c) = self.conexiones.remove(&clave) {
            self.extremo.cerrada(c.id);
        }
    }

    /// Da al extremo lo que espera de cada conexión, y pide anunciar el crédito que se liberó.
    fn entregar(&mut self) {
        let mut anunciar = Vec::new();
        for (clave, c) in self.conexiones.iter_mut() {
            while !c.pendiente.is_empty() {
                let (a, _) = c.pendiente.as_slices();
                let n = self.extremo.recibir(c.id, a).min(a.len());
                if n == 0 {
                    break;
                }
                c.pendiente.drain(..n);
                c.consumido = c.consumido.wrapping_add(n as u32);
            }
            // El crédito liberado se dice si es mucho, o si el invitado cree que le queda poco sitio (puede estar
            // esperándolo para seguir). Así no va un paquete de crédito por cada paquete de datos.
            let libre = c.consumido.wrapping_sub(c.anunciado);
            let en_vuelo = libre as usize + c.pendiente.len();
            if libre >= BUF_ALLOC / 4 || (libre > 0 && en_vuelo + (BUF_ALLOC / 4) as usize >= BUF_ALLOC as usize) {
                anunciar.push(*clave);
            }
        }
        for (su, mio) in anunciar {
            let cab = self.respuesta(OP_CREDIT_UPDATE, su, mio, 0);
            if let Some(c) = self.conexiones.get_mut(&(su, mio)) {
                c.anunciado = c.consumido;
            }
            self.encolar(cab);
        }
    }

    /// Un paquete del invitado (cabecera y datos ya copiados).
    fn paquete(&mut self, h: &Cabecera, datos: &[u8]) {
        if h.src_cid != CID_INVITADO || h.dst_cid != CID_ANFITRION {
            self.cuentas.malos += 1;
            return;
        }
        let clave = (h.src_port, h.dst_port);
        if h.tipo != TIPO_STREAM {
            self.cuentas.malos += 1;
            if h.op != OP_RST {
                self.rst(h.src_port, h.dst_port);
            }
            return;
        }
        if let Some(c) = self.conexiones.get_mut(&clave) {
            c.su_buf = h.buf_alloc;
            c.su_fwd = h.fwd_cnt;
        }
        match h.op {
            OP_REQUEST => {
                if self.conexiones.contains_key(&clave) {
                    // Pedir otra vez una conexión que ya está: se corta.
                    self.cuentas.cortadas += 1;
                    self.quitar(clave);
                    self.rst(h.src_port, h.dst_port);
                    return;
                }
                self.siguiente = self.siguiente.wrapping_add(1);
                let id = self.siguiente;
                if self.conexiones.len() >= MAX_CONEXIONES || !self.extremo.conectar(id, h.dst_port) {
                    self.cuentas.rechazadas += 1;
                    self.rst(h.src_port, h.dst_port);
                    return;
                }
                self.cuentas.conexiones += 1;
                self.conexiones.insert(
                    clave,
                    Conexion {
                        id,
                        pendiente: VecDeque::new(),
                        consumido: 0,
                        anunciado: 0,
                        enviado: 0,
                        su_buf: h.buf_alloc,
                        su_fwd: h.fwd_cnt,
                    },
                );
                let r = self.respuesta(OP_RESPONSE, h.src_port, h.dst_port, 0);
                self.encolar(r);
            }
            OP_RW => {
                let Some(c) = self.conexiones.get_mut(&clave) else {
                    self.rst(h.src_port, h.dst_port);
                    return;
                };
                // Sin crédito: lo que hay sin consumir más esto no puede pasar de lo anunciado.
                if c.pendiente.len() + datos.len() > BUF_ALLOC as usize {
                    self.cuentas.cortadas += 1;
                    self.quitar(clave);
                    self.rst(h.src_port, h.dst_port);
                    return;
                }
                c.pendiente.extend(datos.iter().copied());
                self.cuentas.bytes_del_invitado += datos.len() as u64;
                self.entregar();
            }
            OP_CREDIT_UPDATE => {}
            OP_CREDIT_REQUEST => {
                if self.conexiones.contains_key(&clave) {
                    let r = self.respuesta(OP_CREDIT_UPDATE, h.src_port, h.dst_port, 0);
                    if let Some(c) = self.conexiones.get_mut(&clave) {
                        c.anunciado = c.consumido;
                    }
                    self.encolar(r);
                } else {
                    self.rst(h.src_port, h.dst_port);
                }
            }
            OP_SHUTDOWN | OP_RST => {
                if self.conexiones.contains_key(&clave) {
                    // Lo que quede, al extremo si lo toma; después, fuera.
                    self.entregar();
                    self.quitar(clave);
                    if h.op == OP_SHUTDOWN {
                        self.rst(h.src_port, h.dst_port);
                    }
                }
            }
            _ => {
                // RESPONSE (DashOS nunca pide conexiones) o una orden que no existe.
                self.cuentas.malos += 1;
                if self.conexiones.contains_key(&clave) {
                    self.cuentas.cortadas += 1;
                    self.quitar(clave);
                }
                self.rst(h.src_port, h.dst_port);
            }
        }
    }

    /// Llena los búferes del invitado: primero el control, después los datos de cada conexión con crédito.
    fn llenar_rx(&mut self, colas: &mut [ColaPartida], mem: &mut dyn MemoriaInvitado) -> Result<bool, ErrorVirtio> {
        // Lo que DashOS quiere cerrar.
        while let Some(id) = self.extremo.cerrar() {
            if let Some(&clave) = self.conexiones.iter().find(|(_, c)| c.id == id).map(|(k, _)| k) {
                self.conexiones.remove(&clave);
                self.rst(clave.0, clave.1);
            }
        }
        self.entregar();
        let cola = &mut colas[COLA_RX];
        if !cola.lista {
            return Ok(false);
        }
        let mut usados = false;
        let mut buf = alloc::vec![0u8; MAX_PAQUETE as usize];
        let mut claves: Vec<(u32, u32)> = self.conexiones.keys().copied().collect();
        loop {
            // ¿Hay algo que mandar? Control, o datos de alguna conexión con crédito.
            if self.control.is_empty() && (claves.is_empty() || !self.extremo.hay_algo()) {
                break;
            }
            let Some(c) = cola.siguiente(mem)? else { break };
            let sitio = c.bytes_escritura();
            if sitio < TAM_CABECERA as u64 {
                // Un búfer donde no cabe ni la cabecera: vuelve vacío.
                cola.devolver(mem, c.cabeza, 0)?;
                usados = true;
                self.cuentas.malos += 1;
                continue;
            }
            let mut esc = c.escritor();
            if let Some(h) = self.control.pop_front() {
                esc.escribir(mem, &h.bytes())?;
            } else {
                // Datos: la primera conexión que tenga algo y crédito.
                let max = ((sitio - TAM_CABECERA as u64).min(MAX_PAQUETE as u64)) as usize;
                let mut hecho = false;
                while let Some(clave) = claves.first().copied() {
                    let Some(con) = self.conexiones.get_mut(&clave) else {
                        claves.remove(0);
                        continue;
                    };
                    let n = max.min(con.credito() as usize);
                    let n = if n == 0 { 0 } else { self.extremo.enviar(con.id, &mut buf[..n]).min(n) };
                    if n == 0 {
                        claves.remove(0);
                        continue;
                    }
                    con.enviado = con.enviado.wrapping_add(n as u32);
                    con.anunciado = con.consumido;
                    let h = Cabecera {
                        src_cid: CID_ANFITRION,
                        dst_cid: CID_INVITADO,
                        src_port: clave.1,
                        dst_port: clave.0,
                        len: n as u32,
                        tipo: TIPO_STREAM,
                        op: OP_RW,
                        flags: 0,
                        buf_alloc: BUF_ALLOC,
                        fwd_cnt: con.consumido,
                    };
                    esc.escribir(mem, &h.bytes())?;
                    esc.escribir(mem, &buf[..n])?;
                    self.cuentas.bytes_al_invitado += n as u64;
                    hecho = true;
                    break;
                }
                if !hecho {
                    // Nadie tenía nada: la cadena vuelve al principio (no se ha devuelto, se reusa la próxima vez).
                    cola.retroceder();
                    break;
                }
            }
            cola.devolver(mem, c.cabeza, esc.hecho as u32)?;
            usados = true;
        }
        Ok(usados && cola.quiere_aviso(mem)?)
    }

    /// Conexiones abiertas (para las pruebas y el registro).
    pub fn abiertas(&self) -> usize {
        self.conexiones.len()
    }
}

impl AparatoVirtio for Vsock {
    fn tipo(&self) -> u32 {
        ID_VSOCK
    }

    fn caracteristicas(&self) -> u64 {
        // Sin bits: solo flujo (§5.10.3). Con descriptores indirectos.
        F_INDIRECT_DESC
    }

    fn colas_max(&self) -> &[u16] {
        &[256, 256, 16]
    }

    fn leer_config(&self, desp: u64, datos: &mut [u8]) {
        copiar_config(&CID_INVITADO.to_le_bytes(), desp, datos);
    }

    fn reiniciar(&mut self) {
        let claves: Vec<(u32, u32)> = self.conexiones.keys().copied().collect();
        for c in claves {
            self.quitar(c);
        }
        self.control.clear();
    }

    fn atender(
        &mut self,
        cola: u16,
        colas: &mut [ColaPartida],
        mem: &mut dyn MemoriaInvitado,
    ) -> Result<bool, ErrorVirtio> {
        match cola as usize {
            COLA_TX => {
                let mut paquetes: Vec<(Cabecera, Vec<u8>)> = Vec::new();
                let mut malos = 0u64;
                let aviso_tx = vaciar_cola(&mut colas[COLA_TX], mem, |c, mem| {
                    let total = c.bytes_lectura();
                    let mut lec = c.lector();
                    let mut cab = [0u8; TAM_CABECERA];
                    if total < TAM_CABECERA as u64 || lec.leer(mem, &mut cab)? != TAM_CABECERA {
                        malos += 1;
                        return Ok(0);
                    }
                    let h = Cabecera::leer(&cab);
                    let resto = total - TAM_CABECERA as u64;
                    // El largo tiene que caber en lo que trae la cadena, y no pasar de un paquete.
                    if h.len > MAX_PAQUETE || h.len as u64 > resto {
                        // Se trata como un paquete sin datos con una orden que no existe: se cuenta como malo y corta
                        // la conexión, si la hay.
                        let mut falso = h;
                        falso.op = OP_INVALIDA;
                        falso.len = 0;
                        paquetes.push((falso, Vec::new()));
                        return Ok(0);
                    }
                    let mut datos = alloc::vec![0u8; h.len as usize];
                    lec.leer(mem, &mut datos)?;
                    paquetes.push((h, datos));
                    Ok(0)
                })?;
                self.cuentas.malos += malos;
                for (h, d) in paquetes {
                    self.paquete(&h, &d);
                }
                let aviso_rx = self.llenar_rx(colas, mem)?;
                Ok(aviso_tx || aviso_rx)
            }
            COLA_RX => self.llenar_rx(colas, mem),
            _ => Ok(false),
        }
    }

    fn sondear(&mut self, colas: &mut [ColaPartida], mem: &mut dyn MemoriaInvitado) -> Result<bool, ErrorVirtio> {
        self.llenar_rx(colas, mem)
    }
}
