//! Lo que comparten las pruebas: una red de mentira y una caja con exec, tareas y bsdsocket.library.
#![allow(dead_code)]

use std::collections::{BTreeMap, VecDeque};

use caja_bsdsocket::definiciones::{errno, sbt};
use caja_bsdsocket::*;
use caja_exec::estructuras::{memf, task, ts};
use caja_exec::{Accion, Config, Destino, Exec, Memoria, MemoriaVec, Registros, lvo};

pub const TAMANO: u32 = 8 << 20;
pub const PRIMERO: u16 = 0x900;

/// Una conexión TCP de mentira.
#[derive(Debug)]
pub struct Conn {
    pub fase: Fase,
    /// Lo que le llega al programa.
    pub entrada: VecDeque<u8>,
    /// Lo que mandó el programa.
    pub salida: Vec<u8>,
    pub cabe: usize,
    pub ext: Extremos,
    pub cerrada: bool,
}

#[derive(Debug, Default)]
pub struct Udp {
    pub puerto: u16,
    pub entrada: VecDeque<(Vec<u8>, [u8; 4], u16)>,
    pub salida: Vec<(Vec<u8>, [u8; 4], u16)>,
    pub cerrado: bool,
}

/// Cómo contesta `conectar`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modo {
    Abierta,
    Pendiente,
    Rechaza,
}

pub struct RedFalsa {
    pub siguiente: Id,
    pub conns: BTreeMap<Id, Conn>,
    pub udps: BTreeMap<Id, Udp>,
    /// Oído → (puerto, conexión que entró).
    pub oidos: BTreeMap<Id, (u16, Option<Id>)>,
    pub oidos_cerrados: Vec<Id>,
    pub consultas: BTreeMap<Id, (Vec<u8>, bool)>,
    pub consultas_cerradas: Vec<Id>,
    pub dns: BTreeMap<Vec<u8>, Result<Vec<[u8; 4]>, ErrorRed>>,
    pub modo: Modo,
    pub ocupados: Vec<u16>,
    pub sin_permiso: bool,
    pub ip: [u8; 4],
    pub conexiones_hechas: Vec<([u8; 4], u16)>,
}

impl RedFalsa {
    pub fn nueva() -> RedFalsa {
        RedFalsa {
            siguiente: 100,
            conns: BTreeMap::new(),
            udps: BTreeMap::new(),
            oidos: BTreeMap::new(),
            oidos_cerrados: Vec::new(),
            consultas: BTreeMap::new(),
            consultas_cerradas: Vec::new(),
            dns: BTreeMap::new(),
            modo: Modo::Abierta,
            ocupados: Vec::new(),
            sin_permiso: false,
            ip: [192, 168, 1, 50],
            conexiones_hechas: Vec::new(),
        }
    }
    fn id(&mut self) -> Id {
        self.siguiente += 1;
        self.siguiente
    }
    pub fn conn(&mut self, id: Id) -> &mut Conn {
        self.conns.get_mut(&id).unwrap()
    }
    /// La última conexión abierta.
    pub fn ultima(&self) -> Id {
        *self.conns.keys().last().unwrap()
    }
    pub fn llega(&mut self, id: Id, datos: &[u8]) {
        self.conn(id).entrada.extend(datos);
    }
    /// Alguien entra por un oído de `puerto`: la conexión nueva.
    pub fn entra(&mut self, puerto: u16, ip: [u8; 4], su_puerto: u16) -> Id {
        let o = *self.oidos.iter().find(|(_, (p, c))| *p == puerto && c.is_none()).expect("un oído libre").0;
        let c = self.id();
        self.conns.insert(
            c,
            Conn {
                fase: Fase::Abierta,
                entrada: VecDeque::new(),
                salida: Vec::new(),
                cabe: 1 << 16,
                ext: Extremos { local_puerto: puerto, remoto: ip, remoto_puerto: su_puerto },
                cerrada: false,
            },
        );
        self.oidos.get_mut(&o).unwrap().1 = Some(c);
        c
    }
    /// Los oídos vivos de un puerto.
    pub fn oidos_de(&self, puerto: u16) -> usize {
        self.oidos.values().filter(|(p, _)| *p == puerto).count()
    }
    pub fn contestar_dns(&mut self) {
        for (_, l) in self.consultas.values_mut() {
            *l = true;
        }
    }
}

impl Red for RedFalsa {
    fn conectar(&mut self, ip: [u8; 4], puerto: u16) -> Result<Id, ErrorRed> {
        if self.modo == Modo::Rechaza {
            return Err(ErrorRed::Rechazada);
        }
        let id = self.id();
        self.conexiones_hechas.push((ip, puerto));
        let fase = if self.modo == Modo::Abierta { Fase::Abierta } else { Fase::Conectando };
        self.conns.insert(
            id,
            Conn {
                fase,
                entrada: VecDeque::new(),
                salida: Vec::new(),
                cabe: 1 << 16,
                ext: Extremos { local_puerto: 40000 + id as u16, remoto: ip, remoto_puerto: puerto },
                cerrada: false,
            },
        );
        Ok(id)
    }
    fn fase(&mut self, id: Id) -> Fase {
        if let Some(c) = self.conns.get(&id) {
            return c.fase;
        }
        if self.udps.contains_key(&id) {
            return Fase::Abierta;
        }
        Fase::Error(ErrorRed::NoExiste)
    }
    fn enviar(&mut self, id: Id, datos: &[u8]) -> usize {
        let Some(c) = self.conns.get_mut(&id) else { return 0 };
        let n = datos.len().min(c.cabe.saturating_sub(c.salida.len()));
        c.salida.extend_from_slice(&datos[..n]);
        n
    }
    fn recibir(&mut self, id: Id, buf: &mut [u8]) -> usize {
        let Some(c) = self.conns.get_mut(&id) else { return 0 };
        let n = buf.len().min(c.entrada.len());
        for b in buf.iter_mut().take(n) {
            *b = c.entrada.pop_front().unwrap();
        }
        n
    }
    fn pendiente(&mut self, id: Id) -> (usize, usize) {
        if let Some(c) = self.conns.get(&id) {
            return (c.entrada.len(), c.cabe.saturating_sub(c.salida.len()));
        }
        if let Some(u) = self.udps.get(&id) {
            return (u.entrada.iter().map(|d| d.0.len() + 24).sum(), 4096);
        }
        (0, 0)
    }
    fn extremos(&mut self, id: Id) -> Option<Extremos> {
        if let Some(c) = self.conns.get(&id) {
            return Some(c.ext);
        }
        self.udps.get(&id).map(|u| Extremos { local_puerto: u.puerto, ..Default::default() })
    }
    fn escuchar(&mut self, puerto: u16) -> Result<Id, ErrorRed> {
        if self.sin_permiso {
            return Err(ErrorRed::SinPermiso);
        }
        if self.ocupados.contains(&puerto) {
            return Err(ErrorRed::Ocupado);
        }
        let id = self.id();
        self.oidos.insert(id, (puerto, None));
        Ok(id)
    }
    fn aceptar(&mut self, oido: Id) -> Result<Option<Id>, ErrorRed> {
        match self.oidos.get(&oido) {
            None => Err(ErrorRed::NoExiste),
            Some((_, None)) => Ok(None),
            Some((_, Some(c))) => {
                let c = *c;
                self.oidos.remove(&oido);
                Ok(Some(c))
            }
        }
    }
    fn udp_abrir(&mut self, puerto: u16) -> Result<Id, ErrorRed> {
        if puerto != 0 && self.sin_permiso {
            return Err(ErrorRed::SinPermiso);
        }
        if self.ocupados.contains(&puerto) {
            return Err(ErrorRed::Ocupado);
        }
        let id = self.id();
        let puerto = if puerto == 0 { 50000 + id as u16 } else { puerto };
        self.udps.insert(id, Udp { puerto, ..Default::default() });
        Ok(id)
    }
    fn udp_enviar(&mut self, id: Id, datos: &[u8], ip: [u8; 4], puerto: u16) -> bool {
        let Some(u) = self.udps.get_mut(&id) else { return false };
        u.salida.push((datos.to_vec(), ip, puerto));
        true
    }
    fn udp_recibir(&mut self, id: Id, buf: &mut [u8]) -> Option<(usize, [u8; 4], u16)> {
        let u = self.udps.get_mut(&id)?;
        let (d, ip, p) = u.entrada.pop_front()?;
        let n = d.len().min(buf.len());
        buf[..n].copy_from_slice(&d[..n]);
        Some((d.len(), ip, p))
    }
    fn resolver(&mut self, nombre: &[u8]) -> Result<Id, ErrorRed> {
        let id = self.id();
        self.consultas.insert(id, (nombre.to_vec(), false));
        Ok(id)
    }
    fn resuelto(&mut self, c: Id) -> Option<Result<Vec<[u8; 4]>, ErrorRed>> {
        let (n, listo) = self.consultas.get(&c)?;
        if !listo {
            return None;
        }
        let r = self.dns.get(n).cloned().unwrap_or(Err(ErrorRed::SinNombre));
        self.consultas.remove(&c);
        Some(r)
    }
    fn cerrar(&mut self, id: Id) {
        if let Some(c) = self.conns.get_mut(&id) {
            c.cerrada = true;
        } else if let Some(u) = self.udps.get_mut(&id) {
            u.cerrado = true;
        } else if self.oidos.remove(&id).is_some() {
            self.oidos_cerrados.push(id);
        } else if self.consultas.remove(&id).is_some() {
            self.consultas_cerradas.push(id);
        }
    }
    fn direccion_local(&mut self) -> [u8; 4] {
        self.ip
    }
}

pub struct Caja {
    pub e: Exec<MemoriaVec>,
    pub s: Bsdsocket,
    pub red: RedFalsa,
    pub ahora: u64,
    pub tarea: u32,
    /// La base con la que se llama (A6).
    pub base: u32,
}

/// Una caja con una tarea en marcha y una base abierta por ella.
pub fn caja() -> Caja {
    let mut e = Exec::arrancar(MemoriaVec::nueva(TAMANO as usize), Config::para(TAMANO)).unwrap();
    let tarea = e.crear_tarea("principal", 0, 4096, 0x10_0000).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(tarea));
    let s = Bsdsocket::instalar(&mut e, PRIMERO).unwrap();
    let mut c = Caja { e, s, red: RedFalsa::nueva(), ahora: 1_000_000, tarea, base: 0 };
    c.base = c.abrir();
    assert_ne!(c.base, 0);
    c
}

impl Caja {
    /// Una función de exec por su nombre, siguiendo lo que pida (el Open o el Close de bsdsocket), como la caja.
    pub fn exec(&mut self, f: &str, r: &mut Registros) -> Accion {
        let n = lvo::por_nombre(f).unwrap();
        let mut a = self.e.llamar(n, r);
        for _ in 0..8 {
            match a {
                Accion::Llamar { destino: Destino::Nativa(m), regs, tras } => {
                    let mut rr = regs;
                    if self.s.es_mio(m) {
                        self.s.llamar(&mut self.e, &mut self.red, m, &mut rr, self.ahora);
                    } else {
                        self.e.llamar(m, &mut rr);
                    }
                    a = self.e.continuar(tras, &rr, r);
                }
                Accion::Llamar { .. } => panic!("68k"),
                _ => return a,
            }
        }
        a
    }

    /// `OpenLibrary("bsdsocket.library", 4)` desde la tarea en marcha.
    pub fn abrir(&mut self) -> u32 {
        let nombre = self.cadena("bsdsocket.library");
        let mut r = Registros::default();
        r.a[1] = nombre;
        r.d[0] = 4;
        assert_eq!(self.exec("OpenLibrary", &mut r), Accion::Seguir);
        r.d[0]
    }

    pub fn cerrar(&mut self, base: u32) {
        let mut r = Registros::default();
        r.a[1] = base;
        assert_eq!(self.exec("CloseLibrary", &mut r), Accion::Seguir);
    }

    pub fn cadena(&mut self, s: &str) -> u32 {
        let d = self.e.alloc_mem(s.len() as u32 + 1, memf::CLEAR).unwrap();
        self.e.mem.leer_mut(d, s.len() as u32).unwrap().copy_from_slice(s.as_bytes());
        d
    }
    pub fn bloque(&mut self, largo: u32) -> u32 {
        let d = self.e.alloc_mem(largo, memf::CLEAR).unwrap();
        assert_ne!(d, 0);
        d
    }
    pub fn datos(&mut self, b: &[u8]) -> u32 {
        let d = self.bloque(b.len() as u32 + 4);
        self.e.mem.leer_mut(d, b.len() as u32).unwrap().copy_from_slice(b);
        d
    }
    pub fn leer(&self, d: u32, n: u32) -> Vec<u8> {
        self.e.mem.leer(d, n).unwrap().to_vec()
    }
    pub fn l32(&self, d: u32) -> u32 {
        self.e.mem.leer32(d).unwrap()
    }
    pub fn e32(&mut self, d: u32, v: u32) {
        self.e.mem.escribir32(d, v).unwrap()
    }
    pub fn texto(&self, d: u32) -> String {
        self.e.leer_cadena(d).unwrap()
    }
    /// Una `sockaddr_in`.
    pub fn sockaddr(&mut self, ip: [u8; 4], puerto: u16) -> u32 {
        let p = puerto.to_be_bytes();
        self.datos(&[16, 2, p[0], p[1], ip[0], ip[1], ip[2], ip[3], 0, 0, 0, 0, 0, 0, 0, 0])
    }

    /// Llama una vez: (acción, D0).
    pub fn una(&mut self, f: &str, d: &[u32], a: &[u32]) -> (Accion, u32) {
        let mut r = self.regs(d, a);
        let n = self.s.numero(f).unwrap_or_else(|| panic!("{f}"));
        let acc = self.s.llamar(&mut self.e, &mut self.red, n, &mut r, self.ahora);
        (acc, r.d[0])
    }

    pub fn regs(&self, d: &[u32], a: &[u32]) -> Registros {
        let mut r = Registros::default();
        r.d[..d.len()].copy_from_slice(d);
        r.a[..a.len()].copy_from_slice(a);
        r.a[6] = self.base;
        r
    }

    /// Una llamada que no debe esperar: su D0.
    pub fn f(&mut self, f: &str, d: &[u32], a: &[u32]) -> u32 {
        let (acc, v) = self.una(f, d, a);
        assert_eq!(acc, Accion::Seguir, "{f} no debía esperar");
        v
    }

    /// Una llamada que debe esperar: la deja bloqueada y devuelve sus registros para repetirla.
    pub fn bloquea(&mut self, f: &str, d: &[u32], a: &[u32]) -> Registros {
        let (acc, _) = self.una(f, d, a);
        assert_eq!(acc, Accion::Bloquear, "{f} debía esperar");
        self.regs(d, a)
    }

    /// Lo que hace la caja mientras la tarea espera: `atender` y, si la despertó, repetir la llamada. `None` si sigue
    /// esperando.
    pub fn repetir(&mut self, f: &str, r: &Registros) -> Option<(Accion, u32)> {
        let t = self.e.tarea_actual();
        self.s.atender(&mut self.e, &mut self.red, self.ahora);
        if self.estado(t) == ts::WAIT {
            return None;
        }
        // Lista otra vez: el planificador le da el testigo (la saca de TaskReady).
        if self.estado(t) == ts::READY {
            assert_eq!(self.e.elegir_siguiente(), Some(t));
        }
        let mut rr = *r;
        let n = self.s.numero(f).unwrap();
        let acc = self.s.llamar(&mut self.e, &mut self.red, n, &mut rr, self.ahora);
        Some((acc, rr.d[0]))
    }

    /// Una señal de otra tarea (o de fuera) a `t`; si la despierta, vuelve a tener el testigo.
    pub fn senal(&mut self, t: u32, m: u32) {
        self.e.signal(t, m).unwrap();
        if self.estado(t) == ts::READY {
            assert_eq!(self.e.elegir_siguiente(), Some(t));
        }
    }

    pub fn estado(&self, t: u32) -> u8 {
        self.e.mem.leer8(t + task::TC_STATE).unwrap()
    }

    pub fn errno(&mut self) -> i32 {
        self.f("Errno", &[], &[]) as i32
    }

    /// `socket(AF_INET, tipo, 0)`.
    pub fn zocalo(&mut self, tipo: u32) -> u32 {
        let fd = self.f("socket", &[2, tipo, 0], &[]);
        assert!((fd as i32) >= 0, "socket: errno {}", self.errno());
        fd
    }

    /// Un cliente TCP conectado (la red contesta en el acto).
    pub fn conectado(&mut self, ip: [u8; 4], puerto: u16) -> (u32, Id) {
        let fd = self.zocalo(1);
        let sa = self.sockaddr(ip, puerto);
        assert_eq!(self.f("connect", &[fd, 16], &[sa]), 0);
        (fd, self.red.ultima())
    }

    /// `SocketBaseTags` de una sola etiqueta: (resultado, el dato tras la llamada).
    pub fn etiqueta(&mut self, tag: u32, dato: u32) -> (u32, u32) {
        let t = self.bloque(16);
        self.e32(t, tag);
        self.e32(t + 4, dato);
        self.e32(t + 8, 0);
        let r = self.f("SocketBaseTagList", &[], &[t]);
        (r, self.l32(t + 4))
    }

    pub fn avisos_de(&self, f: &str) -> usize {
        self.s.avisos().iter().filter(|a| a.funcion == f).count()
    }

    pub fn sin_avisos(&self) {
        assert!(self.s.avisos().is_empty(), "{:?}", self.s.avisos());
        assert!(self.e.avisos().is_empty(), "{:?}", self.e.avisos());
    }
}

pub use errno::*;
pub const ERRNO_GET: u32 = sbt::getval(sbt::SBTC_ERRNO);
