//! La red de DashOS para la bsdsocket.library nativa (`caja/bsdsocket`): su rasgo `Red` sobre el cliente del
//! servicio de red (`dashos::red_cliente`), sin esperar nunca (la caja corre todas las tareas 68k en un hilo).
//!
//! - La llave: el canal "red" de su tabla de llaves (`kernel/src/inicio.rs`, rama amiga-apps). Sin ella, todo da
//!   `SinDireccion` (un programa ve "la red no está", no se cuelga).
//! - Cada conexión TCP o zócalo UDP es memoria compartida con el servicio (64 bytes y dos anillos), proyectada en un
//!   hueco propio desde `VA_RED` (40 GB, fuera de lo que ve el 68k y de lo que usa la caja: `caja.c`), de
//!   `HUECO` bytes; como mucho `MAX_HUECOS` a la vez.
//! - Escuchar (servidores) pide el permiso `red-escuchar`, que la caja no tiene: `SinPermiso`.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use caja_bsdsocket::{ErrorRed, Extremos, Fase, Id, Red};
use dashos::protocolo::red as p;
use dashos::red_cliente::{Conexion, Pendiente, Resolucion, Udp};

/// Dónde se proyectan las conexiones (`caja.c`: 40 GB está libre; el JIT empieza en 4 y 68 GB, las ventanas en 20).
const VA_RED: usize = 0xA_0000_0000;
/// Los anillos de cada conexión (una potencia de dos) y el hueco de cada una (su memoria cabe de sobra).
const ANILLO: usize = 64 * 1024;
const HUECO: usize = 256 * 1024;
const MAX_HUECOS: usize = 256;

enum Cosa {
    Conectando(Pendiente, usize),
    Tcp(Conexion, usize),
    UdpAbriendo(Pendiente, usize),
    Udp(Udp, usize),
    Fallida(ErrorRed),
    Consulta(Resolucion),
}

pub(crate) struct RedCaja {
    servicio: Option<u32>,
    cosas: BTreeMap<Id, Cosa>,
    siguiente: Id,
    libres: Vec<usize>,
    ip: [u8; 4],
}

impl RedCaja {
    pub fn nueva() -> RedCaja {
        RedCaja {
            servicio: dashos::llaves::buscar("red"),
            cosas: BTreeMap::new(),
            siguiente: 1,
            libres: (0..MAX_HUECOS).rev().collect(),
            ip: [0; 4],
        }
    }

    pub fn hay_servicio(&self) -> bool {
        self.servicio.is_some()
    }

    fn id(&mut self, c: Cosa) -> Id {
        let id = self.siguiente;
        self.siguiente = self.siguiente.wrapping_add(1).max(1);
        self.cosas.insert(id, c);
        id
    }

    fn hueco(&mut self) -> Result<(usize, usize), ErrorRed> {
        let h = self.libres.pop().ok_or(ErrorRed::Limite)?;
        Ok((h, VA_RED + h * HUECO))
    }

    /// Lo pendiente de `id` que ya llegó pasa a abierto (o a fallido).
    fn avanzar(&mut self, id: Id) {
        let Some(c) = self.cosas.remove(&id) else { return };
        let nueva = match c {
            Cosa::Conectando(mut q, h) => match q.mirar() {
                None => Cosa::Conectando(q, h),
                Some(Ok(con)) => Cosa::Tcp(con, h),
                Some(Err(e)) => {
                    self.libres.push(h);
                    Cosa::Fallida(ErrorRed::de_dashos(e))
                }
            },
            Cosa::UdpAbriendo(mut q, h) => match q.mirar() {
                None => Cosa::UdpAbriendo(q, h),
                Some(Ok(con)) => Cosa::Udp(Udp::de_conexion(con), h),
                Some(Err(e)) => {
                    self.libres.push(h);
                    Cosa::Fallida(ErrorRed::de_dashos(e))
                }
            },
            otra => otra,
        };
        self.cosas.insert(id, nueva);
    }
}

fn de_fase(f: i32) -> Fase {
    match f {
        p::CONECTANDO => Fase::Conectando,
        p::ABIERTA => Fase::Abierta,
        p::FIN => Fase::Fin,
        e => Fase::Error(ErrorRed::de_dashos(e)),
    }
}

impl Red for RedCaja {
    fn conectar(&mut self, ip: [u8; 4], puerto: u16) -> Result<Id, ErrorRed> {
        let s = self.servicio.ok_or(ErrorRed::SinDireccion)?;
        let (h, va) = self.hueco()?;
        match Conexion::empezar(s, ip, puerto, va, ANILLO) {
            Ok(q) => Ok(self.id(Cosa::Conectando(q, h))),
            Err(e) => {
                self.libres.push(h);
                Err(ErrorRed::de_dashos(e))
            }
        }
    }

    fn fase(&mut self, id: Id) -> Fase {
        self.avanzar(id);
        match self.cosas.get(&id) {
            Some(Cosa::Conectando(..)) | Some(Cosa::UdpAbriendo(..)) => Fase::Conectando,
            Some(Cosa::Tcp(c, _)) => de_fase(c.fase()),
            Some(Cosa::Udp(u, _)) => de_fase(u.fase()),
            Some(Cosa::Fallida(e)) => Fase::Error(*e),
            _ => Fase::Error(ErrorRed::NoExiste),
        }
    }

    fn enviar(&mut self, id: Id, datos: &[u8]) -> usize {
        self.avanzar(id);
        match self.cosas.get(&id) {
            Some(Cosa::Tcp(c, _)) => c.enviar(datos),
            _ => 0,
        }
    }

    fn recibir(&mut self, id: Id, buf: &mut [u8]) -> usize {
        self.avanzar(id);
        match self.cosas.get(&id) {
            Some(Cosa::Tcp(c, _)) => c.recibir(buf),
            _ => 0,
        }
    }

    fn pendiente(&mut self, id: Id) -> (usize, usize) {
        self.avanzar(id);
        match self.cosas.get(&id) {
            Some(Cosa::Tcp(c, _)) => c.pendiente(),
            Some(Cosa::Udp(u, _)) => u.pendiente(),
            _ => (0, 0),
        }
    }

    fn extremos(&mut self, id: Id) -> Option<Extremos> {
        self.avanzar(id);
        match self.cosas.get(&id)? {
            Cosa::Tcp(c, _) => {
                let (d, _, puerto) = c.remoto();
                Some(Extremos { local_puerto: c.puerto(), remoto: [d[0], d[1], d[2], d[3]], remoto_puerto: puerto })
            }
            Cosa::Udp(u, _) => Some(Extremos { local_puerto: u.puerto(), remoto: [0; 4], remoto_puerto: 0 }),
            _ => None,
        }
    }

    fn escuchar(&mut self, _puerto: u16) -> Result<Id, ErrorRed> {
        // La caja no tiene el permiso de escuchar (`red-escuchar`).
        Err(if self.servicio.is_some() { ErrorRed::SinPermiso } else { ErrorRed::SinDireccion })
    }

    fn aceptar(&mut self, _oido: Id) -> Result<Option<Id>, ErrorRed> {
        Err(ErrorRed::NoExiste)
    }

    fn udp_abrir(&mut self, puerto: u16) -> Result<Id, ErrorRed> {
        let s = self.servicio.ok_or(ErrorRed::SinDireccion)?;
        if puerto != 0 {
            return Err(ErrorRed::SinPermiso);
        }
        let (h, va) = self.hueco()?;
        match Udp::empezar(s, 0, va, ANILLO) {
            Ok(q) => Ok(self.id(Cosa::UdpAbriendo(q, h))),
            Err(e) => {
                self.libres.push(h);
                Err(ErrorRed::de_dashos(e))
            }
        }
    }

    fn udp_enviar(&mut self, id: Id, datos: &[u8], ip: [u8; 4], puerto: u16) -> bool {
        self.avanzar(id);
        match self.cosas.get(&id) {
            Some(Cosa::Udp(u, _)) => u.enviar4(datos, ip, puerto),
            _ => false,
        }
    }

    fn udp_recibir(&mut self, id: Id, buf: &mut [u8]) -> Option<(usize, [u8; 4], u16)> {
        self.avanzar(id);
        match self.cosas.get(&id) {
            Some(Cosa::Udp(u, _)) => u.recibir(buf).map(|(n, d, _, puerto)| (n, [d[0], d[1], d[2], d[3]], puerto)),
            _ => None,
        }
    }

    fn max_datagrama(&self) -> usize {
        p::MAX_DATAGRAMA
    }

    fn resolver(&mut self, nombre: &[u8]) -> Result<Id, ErrorRed> {
        let s = self.servicio.ok_or(ErrorRed::SinDireccion)?;
        let q = dashos::red_cliente::resolver_empezar(s, nombre, p::IPV4, ([0; 4], 0)).map_err(ErrorRed::de_dashos)?;
        Ok(self.id(Cosa::Consulta(q)))
    }

    fn resuelto(&mut self, consulta: Id) -> Option<Result<Vec<[u8; 4]>, ErrorRed>> {
        let Some(Cosa::Consulta(q)) = self.cosas.get_mut(&consulta) else { return Some(Err(ErrorRed::NoExiste)) };
        let r = q.mirar()?;
        self.cosas.remove(&consulta);
        Some(match r {
            Ok(d) => Ok((0..d.n).map(|i| [d.dirs[i][0], d.dirs[i][1], d.dirs[i][2], d.dirs[i][3]]).collect()),
            Err(e) => Err(ErrorRed::de_dashos(e)),
        })
    }

    fn cerrar(&mut self, id: Id) {
        match self.cosas.remove(&id) {
            Some(Cosa::Conectando(q, h)) | Some(Cosa::UdpAbriendo(q, h)) => {
                q.abandonar();
                self.libres.push(h);
            }
            Some(Cosa::Tcp(c, h)) => {
                c.cerrar();
                self.libres.push(h);
            }
            Some(Cosa::Udp(u, h)) => {
                u.cerrar();
                self.libres.push(h);
            }
            Some(Cosa::Consulta(mut q)) => q.soltar(),
            _ => {}
        }
    }

    fn direccion_local(&mut self) -> [u8; 4] {
        if self.ip == [0; 4]
            && let Some(s) = self.servicio
            && let Ok(e) = dashos::red_cliente::estado(s, 200_000)
            && e[p::E_CONFIGURADA] != 0
        {
            self.ip = e[p::E_IP].to_le_bytes();
        }
        self.ip
    }
}
