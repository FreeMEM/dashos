//! `vhost-blk`: sirve un aparato de `caja-linux/vmm` (el disco o la red) a QEMU por vhost-user, para que lo use el
//! controlador de un Linux de verdad. Solo es un banco de pruebas en el PC; en DashOS no hay nada de esto.
//!
//! QEMU lleva el transporte (virtio-mmio) y le pasa a este proceso la memoria del invitado (descriptores de un
//! memfd), las direcciones de la cola y dos eventfd: "kick" (el invitado avisó) y "call" (dar la interrupción).
//! Las cadenas las saca y atiende el código del crate (`ColaPartida` y `atender` del aparato), sin cambios.
//!
//! Con la red, el otro lado del cable es un "router" de juguete en 10.0.2.2 que contesta ARP y ping.
//!
//! El protocolo, según la especificación de vhost-user de la documentación de QEMU (docs/interop/vhost-user.rst):
//! cabecera de 12 bytes (petición u32, banderas u32 con la versión 1 y el bit 2 de respuesta, tamaño u32), carga y,
//! en algunas peticiones, descriptores de fichero por SCM_RIGHTS.
//!
//! Aquí sí hay `unsafe` (mmap, recvmsg, poll y la copia en la memoria del invitado): es una herramienta del PC.
//!
//!     vhost-blk blk SOCKET IMAGEN
//!     vhost-blk red SOCKET

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::rc::Rc;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::fs::FileExt;
use std::os::unix::net::{UnixListener, UnixStream};

use caja_vmm::memoria::{FueraDeMemoria, MemoriaInvitado};
use caja_vmm::virtio::bloque::{Bloque, Bloques, ErrorBloques, SECTOR};
use caja_vmm::virtio::cola::ColaPartida;
use caja_vmm::virtio::red::{ExtremoRed, Red};
use caja_vmm::virtio::{AparatoVirtio, F_INDIRECT_DESC, F_VERSION_1};

const GET_FEATURES: u32 = 1;
const SET_FEATURES: u32 = 2;
const SET_OWNER: u32 = 3;
const RESET_OWNER: u32 = 4;
const SET_MEM_TABLE: u32 = 5;
const SET_VRING_NUM: u32 = 8;
const SET_VRING_ADDR: u32 = 9;
const SET_VRING_BASE: u32 = 10;
const GET_VRING_BASE: u32 = 11;
const SET_VRING_KICK: u32 = 12;
const SET_VRING_CALL: u32 = 13;
const SET_VRING_ERR: u32 = 14;
const GET_PROTOCOL_FEATURES: u32 = 15;
const SET_PROTOCOL_FEATURES: u32 = 16;
const GET_QUEUE_NUM: u32 = 17;
const SET_VRING_ENABLE: u32 = 18;
const GET_CONFIG: u32 = 24;
const SET_CONFIG: u32 = 25;

const F_PROTOCOL_FEATURES: u64 = 1 << 30;
const PF_CONFIG: u64 = 1 << 9;
const BANDERA_RESPUESTA: u32 = 1 << 2;

struct DiscoFichero(File, u64);

impl Bloques for DiscoFichero {
    fn sectores(&self) -> u64 {
        self.1 / SECTOR
    }
    fn leer(&mut self, s: u64, b: &mut [u8]) -> Result<(), ErrorBloques> {
        self.0.read_exact_at(b, s * SECTOR).map_err(|_| ErrorBloques)
    }
    fn escribir(&mut self, s: u64, d: &[u8]) -> Result<(), ErrorBloques> {
        self.0.write_all_at(d, s * SECTOR).map_err(|_| ErrorBloques)
    }
    fn vaciar(&mut self) -> Result<(), ErrorBloques> {
        self.0.sync_data().map_err(|_| ErrorBloques)
    }
}

struct Region {
    gpa: u64,
    tam: u64,
    /// Dirección en el proceso de QEMU (las de SET_VRING_ADDR vienen así).
    ua: u64,
    ptr: *mut u8,
}

#[derive(Default)]
struct Memoria {
    regiones: Vec<Region>,
}

impl Memoria {
    fn trozo(&self, dir: u64, largo: usize) -> Result<*mut u8, FueraDeMemoria> {
        let err = FueraDeMemoria { dir, largo: largo as u64 };
        for r in &self.regiones {
            if dir >= r.gpa && dir - r.gpa <= r.tam && largo as u64 <= r.tam - (dir - r.gpa) {
                return Ok(unsafe { r.ptr.add((dir - r.gpa) as usize) });
            }
        }
        Err(err)
    }

    fn ua_a_gpa(&self, ua: u64) -> Option<u64> {
        self.regiones.iter().find(|r| ua >= r.ua && ua - r.ua < r.tam).map(|r| r.gpa + (ua - r.ua))
    }
}

impl MemoriaInvitado for Memoria {
    fn leer(&self, dir: u64, buf: &mut [u8]) -> Result<(), FueraDeMemoria> {
        let p = self.trozo(dir, buf.len())?;
        unsafe { std::ptr::copy_nonoverlapping(p, buf.as_mut_ptr(), buf.len()) };
        Ok(())
    }
    fn escribir(&mut self, dir: u64, datos: &[u8]) -> Result<(), FueraDeMemoria> {
        let p = self.trozo(dir, datos.len())?;
        unsafe { std::ptr::copy_nonoverlapping(datos.as_ptr(), p, datos.len()) };
        Ok(())
    }
    fn barrera(&self) {
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
    }
}

/// Lee un mensaje con sus descriptores de fichero.
fn recibir(s: &UnixStream) -> Option<(u32, u32, Vec<u8>, Vec<RawFd>)> {
    let mut cab = [0u8; 12];
    let mut fds = Vec::new();
    let mut ctrl = [0u8; 256];
    let mut iov = libc::iovec { iov_base: cab.as_mut_ptr().cast(), iov_len: 12 };
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = ctrl.as_mut_ptr().cast();
    msg.msg_controllen = ctrl.len() as _;
    let n = unsafe { libc::recvmsg(s.as_raw_fd(), &mut msg, libc::MSG_CMSG_CLOEXEC) };
    if n != 12 {
        return None;
    }
    unsafe {
        let mut c = libc::CMSG_FIRSTHDR(&msg);
        while !c.is_null() {
            if (*c).cmsg_level == libc::SOL_SOCKET && (*c).cmsg_type == libc::SCM_RIGHTS {
                let datos = libc::CMSG_DATA(c) as *const RawFd;
                let k = ((*c).cmsg_len as usize - libc::CMSG_LEN(0) as usize) / 4;
                for i in 0..k {
                    fds.push(*datos.add(i));
                }
            }
            c = libc::CMSG_NXTHDR(&msg, c);
        }
    }
    let pet = u32::from_le_bytes(cab[0..4].try_into().unwrap());
    let banderas = u32::from_le_bytes(cab[4..8].try_into().unwrap());
    let tam = u32::from_le_bytes(cab[8..12].try_into().unwrap()) as usize;
    let mut carga = vec![0u8; tam];
    (&*s).read_exact(&mut carga).ok()?;
    Some((pet, banderas, carga, fds))
}

fn responder(s: &UnixStream, pet: u32, carga: &[u8]) {
    let mut m = Vec::with_capacity(12 + carga.len());
    m.extend_from_slice(&pet.to_le_bytes());
    m.extend_from_slice(&(1 | BANDERA_RESPUESTA).to_le_bytes());
    m.extend_from_slice(&(carga.len() as u32).to_le_bytes());
    m.extend_from_slice(carga);
    (&*s).write_all(&m).expect("respuesta");
}

fn u32_en(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u64_en(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

#[derive(Default)]
struct Vring {
    kick: Option<File>,
    call: Option<File>,
    activa: bool,
    /// Direcciones de la cola en el espacio de QEMU (desc, usado, disponible).
    dirs_ua: (u64, u64, u64),
}

struct Estado {
    aparato: Box<dyn AparatoVirtio>,
    colas: Vec<ColaPartida>,
    vrings: Vec<Vring>,
    mem: Memoria,
    proto: bool,
    avisos: u64,
}

impl Estado {
    fn nuevo(aparato: Box<dyn AparatoVirtio>) -> Self {
        let n = aparato.colas_max().len();
        Estado {
            aparato,
            colas: vec![ColaPartida::default(); n],
            vrings: (0..n).map(|_| Vring::default()).collect(),
            mem: Memoria::default(),
            proto: false,
            avisos: 0,
        }
    }

    fn todas_activas(&self) -> bool {
        self.vrings.iter().zip(&self.colas).all(|(v, c)| v.activa && c.lista)
    }

    fn resultado(&mut self, r: Result<bool, caja_vmm::virtio::ErrorVirtio>) {
        match r {
            // No se sabe de qué cola fue el búfer usado: se avisa por todas (una interrupción de más no hace daño).
            Ok(true) => {
                for v in &mut self.vrings {
                    if let Some(c) = &mut v.call {
                        let _ = c.write_all(&1u64.to_le_bytes());
                    }
                }
            }
            Ok(false) => {}
            Err(e) => {
                eprintln!("vhost-blk: error en una cola: {e:?}");
                for v in &mut self.vrings {
                    v.activa = false;
                }
            }
        }
    }

    fn atender_kick(&mut self, i: usize) {
        let Some(k) = &mut self.vrings[i].kick else { return };
        let mut b = [0u8; 8];
        let _ = k.read(&mut b);
        if !self.todas_activas() {
            return;
        }
        self.avisos += 1;
        let r = self.aparato.atender(i as u16, &mut self.colas, &mut self.mem);
        self.resultado(r);
        self.sondear();
    }

    fn sondear(&mut self) {
        if self.todas_activas() {
            let r = self.aparato.sondear(&mut self.colas, &mut self.mem);
            self.resultado(r);
        }
    }

    fn preparar_cola(&mut self, i: usize) {
        let (d, u, a) = self.vrings[i].dirs_ua;
        if let (Some(d), Some(u), Some(a)) = (self.mem.ua_a_gpa(d), self.mem.ua_a_gpa(u), self.mem.ua_a_gpa(a)) {
            let c = &mut self.colas[i];
            c.desc = d;
            c.usado = u;
            c.disponible = a;
            c.lista = true;
        }
    }

    fn mensaje(&mut self, s: &UnixStream, pet: u32, carga: &[u8], fds: Vec<RawFd>) {
        let fd0 = || fds.first().map(|&f| unsafe { File::from_raw_fd(f) });
        // Índice de cola: en SET_VRING_KICK/CALL/ERR va en los 8 bits bajos de un u64; en las demás, un u32.
        let idx = || (u32_en(carga, 0) & 0xff) as usize;
        let n = self.colas.len();
        match pet {
            GET_FEATURES => {
                let f = self.aparato.caracteristicas() | F_VERSION_1 | F_PROTOCOL_FEATURES;
                responder(s, pet, &f.to_le_bytes());
            }
            SET_FEATURES => {
                let f = u64_en(carga, 0);
                for c in &mut self.colas {
                    c.indirectas = f & F_INDIRECT_DESC != 0;
                }
                self.proto = f & F_PROTOCOL_FEATURES != 0;
                self.aparato.activar(f);
                eprintln!("vhost-blk: características aceptadas {f:#x}");
            }
            GET_PROTOCOL_FEATURES => responder(s, pet, &PF_CONFIG.to_le_bytes()),
            SET_PROTOCOL_FEATURES | SET_OWNER | RESET_OWNER | SET_CONFIG => {}
            GET_QUEUE_NUM => responder(s, pet, &(n as u64).to_le_bytes()),
            GET_CONFIG => {
                let (off, tam) = (u32_en(carga, 0) as u64, u32_en(carga, 4) as usize);
                let mut r = carga[..12].to_vec();
                let mut datos = vec![0u8; tam];
                self.aparato.leer_config(off, &mut datos);
                r.extend_from_slice(&datos);
                responder(s, pet, &r);
            }
            SET_MEM_TABLE => {
                let k = u32_en(carga, 0) as usize;
                self.mem.regiones.clear();
                for i in 0..k {
                    let o = 8 + 32 * i;
                    let (gpa, tam, ua, off) =
                        (u64_en(carga, o), u64_en(carga, o + 8), u64_en(carga, o + 16), u64_en(carga, o + 24));
                    let p = unsafe {
                        libc::mmap(
                            std::ptr::null_mut(),
                            (tam + off) as usize,
                            libc::PROT_READ | libc::PROT_WRITE,
                            libc::MAP_SHARED,
                            fds[i],
                            0,
                        )
                    };
                    assert_ne!(p, libc::MAP_FAILED, "mmap");
                    unsafe { libc::close(fds[i]) };
                    eprintln!("vhost-blk: región {gpa:#x}+{tam:#x}");
                    self.mem.regiones.push(Region { gpa, tam, ua, ptr: unsafe { (p as *mut u8).add(off as usize) } });
                }
                for i in 0..n {
                    if self.vrings[i].dirs_ua.0 != 0 {
                        self.preparar_cola(i);
                    }
                }
            }
            SET_VRING_NUM if idx() < n => self.colas[idx()].tam = u32_en(carga, 4) as u16,
            SET_VRING_ADDR if idx() < n => {
                let i = idx();
                self.vrings[i].dirs_ua = (u64_en(carga, 8), u64_en(carga, 16), u64_en(carga, 24));
                self.preparar_cola(i);
            }
            SET_VRING_BASE if idx() < n => {
                let b = u32_en(carga, 4) as u16;
                self.colas[idx()].reanudar(b, b);
            }
            GET_VRING_BASE if idx() < n => {
                let i = idx();
                self.vrings[i].activa = false;
                let mut r = carga[..4].to_vec();
                r.extend_from_slice(&(self.colas[i].ultimo_disponible() as u32).to_le_bytes());
                responder(s, pet, &r);
                eprintln!("vhost-blk: cola {i} parada; {} avisos atendidos", self.avisos);
            }
            SET_VRING_KICK if idx() < n => {
                let i = idx();
                self.vrings[i].kick = fd0();
                // Sin el protocolo extendido, la cola empieza al recibir el kick.
                if !self.proto {
                    self.vrings[i].activa = true;
                }
            }
            SET_VRING_CALL if idx() < n => self.vrings[idx()].call = fd0(),
            SET_VRING_ERR => drop(fd0()),
            SET_VRING_ENABLE if idx() < n => {
                self.vrings[idx()].activa = u32_en(carga, 4) != 0;
                eprintln!("vhost-blk: cola {} {}", idx(), if self.vrings[idx()].activa { "activa" } else { "parada" });
            }
            otra => {
                eprintln!("vhost-blk: petición {otra} sin atender");
                for f in fds {
                    unsafe { libc::close(f) };
                }
            }
        }
    }
}

const MAC_ROUTER: [u8; 6] = [0x52, 0x54, 0x00, 0x00, 0x00, 0x02];
const IP_ROUTER: [u8; 4] = [10, 0, 2, 2];

/// El otro lado del cable: contesta ARP por 10.0.2.2 y los ping que le llegan.
#[derive(Clone, Default)]
struct Router(Rc<RefCell<(VecDeque<Vec<u8>>, u64)>>);

fn suma_ip(b: &[u8]) -> u16 {
    let mut s: u32 = 0;
    for c in b.chunks(2) {
        s += u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)]) as u32;
    }
    while s > 0xffff {
        s = (s & 0xffff) + (s >> 16);
    }
    !(s as u16)
}

impl ExtremoRed for Router {
    fn enviar(&mut self, t: &[u8]) {
        let mut r = self.0.borrow_mut();
        r.1 += 1;
        if t.len() < 42 {
            return;
        }
        let tipo = u16::from_be_bytes([t[12], t[13]]);
        if tipo == 0x0806 && t[20..22] == [0, 1] && t[38..42] == IP_ROUTER {
            let mut a = t[..42].to_vec();
            a[0..6].copy_from_slice(&t[6..12]);
            a[6..12].copy_from_slice(&MAC_ROUTER);
            a[21] = 2;
            a[22..28].copy_from_slice(&MAC_ROUTER);
            a[28..32].copy_from_slice(&IP_ROUTER);
            a[32..42].copy_from_slice(&t[22..32]);
            r.0.push_back(a);
        } else if tipo == 0x0800 && t[23] == 1 && t[30..34] == IP_ROUTER {
            let ihl = (t[14] & 0xf) as usize * 4;
            if t.len() < 14 + ihl + 8 || t[14 + ihl] != 8 {
                return;
            }
            let mut a = t.to_vec();
            a[0..6].copy_from_slice(&t[6..12]);
            a[6..12].copy_from_slice(&MAC_ROUTER);
            a[26..30].copy_from_slice(&t[30..34]);
            a[30..34].copy_from_slice(&t[26..30]);
            let icmp = 14 + ihl;
            a[icmp] = 0;
            a[icmp + 2..icmp + 4].fill(0);
            let total = u16::from_be_bytes([t[16], t[17]]) as usize;
            let fin = (14 + total).min(a.len());
            let c = suma_ip(&a[icmp..fin]);
            a[icmp + 2..icmp + 4].copy_from_slice(&c.to_be_bytes());
            r.0.push_back(a);
        }
    }
    fn hay_trama(&self) -> bool {
        !self.0.borrow().0.is_empty()
    }
    fn recibir(&mut self) -> Option<Vec<u8>> {
        self.0.borrow_mut().0.pop_front()
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (aparato, socket): (Box<dyn AparatoVirtio>, &str) = match args.get(1).map(String::as_str) {
        Some("blk") if args.len() == 4 => {
            let fichero = OpenOptions::new().read(true).write(true).open(&args[3]).expect("imagen");
            let tam = fichero.metadata().unwrap().len();
            (Box::new(Bloque::nuevo(Box::new(DiscoFichero(fichero, tam)))), &args[2])
        }
        Some("red") if args.len() == 3 => {
            (Box::new(Red::nueva(Box::new(Router::default()), [0x52, 0x54, 0, 0x12, 0x34, 0x56])), &args[2])
        }
        _ => {
            eprintln!("uso: vhost-blk blk SOCKET IMAGEN | vhost-blk red SOCKET");
            std::process::exit(2);
        }
    };
    let _ = std::fs::remove_file(socket);
    let escucha = UnixListener::bind(socket).expect("socket");
    eprintln!("vhost-blk: esperando a QEMU en {socket}");
    let (s, _) = escucha.accept().expect("accept");
    let mut e = Estado::nuevo(aparato);
    loop {
        let mut pfd = vec![libc::pollfd { fd: s.as_raw_fd(), events: libc::POLLIN, revents: 0 }];
        let mut de_cola = Vec::new();
        for (i, v) in e.vrings.iter().enumerate() {
            if let Some(k) = &v.kick {
                pfd.push(libc::pollfd { fd: k.as_raw_fd(), events: libc::POLLIN, revents: 0 });
                de_cola.push(i);
            }
        }
        if unsafe { libc::poll(pfd.as_mut_ptr(), pfd.len() as _, -1) } < 0 {
            continue;
        }
        for (k, &i) in de_cola.iter().enumerate() {
            if pfd[k + 1].revents & libc::POLLIN != 0 {
                e.atender_kick(i);
            }
        }
        if pfd[0].revents & (libc::POLLIN | libc::POLLHUP) != 0 {
            let Some((pet, _banderas, carga, fds)) = recibir(&s) else {
                eprintln!("vhost-blk: QEMU cerró; {} avisos atendidos", e.avisos);
                return;
            };
            e.mensaje(&s, pet, &carga, fds);
        }
    }
}
