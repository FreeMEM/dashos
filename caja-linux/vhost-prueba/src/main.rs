//! `vhost-blk`: sirve el virtio-blk de `caja-linux/vmm` a QEMU por vhost-user, para que lo use el controlador
//! virtio-blk de un Linux de verdad. Solo es un banco de pruebas en el PC; en DashOS no hay nada de esto.
//!
//! QEMU lleva el transporte (virtio-mmio) y le pasa a este proceso la memoria del invitado (descriptores de un
//! memfd), las direcciones de la cola y dos eventfd: "kick" (el invitado avisó) y "call" (dar la interrupción).
//! Las cadenas las saca y atiende el código del crate (`ColaPartida` y `Bloque::atender`), sin cambios.
//!
//! El protocolo, según la especificación de vhost-user de la documentación de QEMU (docs/interop/vhost-user.rst):
//! cabecera de 12 bytes (petición u32, banderas u32 con la versión 1 y el bit 2 de respuesta, tamaño u32), carga y,
//! en algunas peticiones, descriptores de fichero por SCM_RIGHTS.
//!
//! Aquí sí hay `unsafe` (mmap, recvmsg, poll y la copia en la memoria del invitado): es una herramienta del PC.
//!
//!     vhost-blk SOCKET IMAGEN

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::fs::FileExt;
use std::os::unix::net::{UnixListener, UnixStream};

use caja_vmm::memoria::{FueraDeMemoria, MemoriaInvitado};
use caja_vmm::virtio::bloque::{Bloque, Bloques, ErrorBloques, SECTOR};
use caja_vmm::virtio::cola::ColaPartida;
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

struct Estado {
    bloque: Bloque,
    cola: ColaPartida,
    mem: Memoria,
    kick: Option<File>,
    call: Option<File>,
    activa: bool,
    proto: bool,
    /// Direcciones de la cola en el espacio de QEMU, hasta tener la tabla de memoria.
    dirs_ua: (u64, u64, u64),
    peticiones: u64,
}

impl Estado {
    fn atender_kick(&mut self) {
        let Some(k) = &mut self.kick else { return };
        let mut b = [0u8; 8];
        let _ = k.read(&mut b);
        if !self.activa || !self.cola.lista {
            return;
        }
        let mut colas = [std::mem::take(&mut self.cola)];
        let r = self.bloque.atender(0, &mut colas, &mut self.mem);
        self.cola = std::mem::take(&mut colas[0]);
        self.peticiones += 1;
        match r {
            Ok(true) => {
                if let Some(c) = &mut self.call {
                    let _ = c.write_all(&1u64.to_le_bytes());
                }
            }
            Ok(false) => {}
            Err(e) => {
                eprintln!("vhost-blk: error en la cola: {e:?}");
                self.activa = false;
            }
        }
    }

    fn preparar_cola(&mut self) {
        let (d, u, a) = self.dirs_ua;
        if let (Some(d), Some(u), Some(a)) = (self.mem.ua_a_gpa(d), self.mem.ua_a_gpa(u), self.mem.ua_a_gpa(a)) {
            self.cola.desc = d;
            self.cola.usado = u;
            self.cola.disponible = a;
            self.cola.lista = true;
        }
    }

    fn mensaje(&mut self, s: &UnixStream, pet: u32, carga: &[u8], fds: Vec<RawFd>) {
        let fd0 = || fds.first().map(|&f| unsafe { File::from_raw_fd(f) });
        match pet {
            GET_FEATURES => {
                let f = self.bloque.caracteristicas() | F_VERSION_1 | F_PROTOCOL_FEATURES;
                responder(s, pet, &f.to_le_bytes());
            }
            SET_FEATURES => {
                let f = u64_en(carga, 0);
                self.cola.indirectas = f & F_INDIRECT_DESC != 0;
                self.proto = f & F_PROTOCOL_FEATURES != 0;
                eprintln!("vhost-blk: características aceptadas {f:#x}");
            }
            GET_PROTOCOL_FEATURES => responder(s, pet, &PF_CONFIG.to_le_bytes()),
            SET_PROTOCOL_FEATURES | SET_OWNER | RESET_OWNER | SET_CONFIG => {}
            GET_QUEUE_NUM => responder(s, pet, &1u64.to_le_bytes()),
            GET_CONFIG => {
                let (off, tam) = (u32_en(carga, 0) as u64, u32_en(carga, 4) as usize);
                let mut r = carga[..12].to_vec();
                let mut datos = vec![0u8; tam];
                self.bloque.leer_config(off, &mut datos);
                r.extend_from_slice(&datos);
                responder(s, pet, &r);
            }
            SET_MEM_TABLE => {
                let n = u32_en(carga, 0) as usize;
                self.mem.regiones.clear();
                for i in 0..n {
                    let o = 8 + 32 * i;
                    let (gpa, tam, ua, off) = (u64_en(carga, o), u64_en(carga, o + 8), u64_en(carga, o + 16), u64_en(carga, o + 24));
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
                if self.dirs_ua.0 != 0 {
                    self.preparar_cola();
                }
            }
            SET_VRING_NUM => self.cola.tam = u32_en(carga, 4) as u16,
            SET_VRING_ADDR => {
                self.dirs_ua = (u64_en(carga, 8), u64_en(carga, 16), u64_en(carga, 24));
                self.preparar_cola();
            }
            SET_VRING_BASE => {
                let b = u32_en(carga, 4) as u16;
                self.cola.reanudar(b, b);
            }
            GET_VRING_BASE => {
                self.activa = false;
                let mut r = carga[..4].to_vec();
                r.extend_from_slice(&(self.cola.ultimo_disponible() as u32).to_le_bytes());
                responder(s, pet, &r);
                eprintln!("vhost-blk: parada; {} avisos atendidos", self.peticiones);
            }
            SET_VRING_KICK => {
                self.kick = fd0();
                // Sin el protocolo extendido, la cola empieza al recibir el kick.
                if !self.proto {
                    self.activa = true;
                }
            }
            SET_VRING_CALL => self.call = fd0(),
            SET_VRING_ERR => drop(fd0()),
            SET_VRING_ENABLE => {
                self.activa = u32_en(carga, 4) != 0;
                eprintln!("vhost-blk: cola {}", if self.activa { "activa" } else { "parada" });
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("uso: vhost-blk SOCKET IMAGEN");
        std::process::exit(2);
    }
    let fichero = OpenOptions::new().read(true).write(true).open(&args[2]).expect("imagen");
    let tam = fichero.metadata().unwrap().len();
    let _ = std::fs::remove_file(&args[1]);
    let escucha = UnixListener::bind(&args[1]).expect("socket");
    eprintln!("vhost-blk: esperando a QEMU en {}", args[1]);
    let (s, _) = escucha.accept().expect("accept");
    let mut e = Estado {
        bloque: Bloque::nuevo(Box::new(DiscoFichero(fichero, tam))),
        cola: ColaPartida::default(),
        mem: Memoria::default(),
        kick: None,
        call: None,
        activa: false,
        proto: false,
        dirs_ua: (0, 0, 0),
        peticiones: 0,
    };
    loop {
        let mut pfd = vec![libc::pollfd { fd: s.as_raw_fd(), events: libc::POLLIN, revents: 0 }];
        if let Some(k) = &e.kick {
            pfd.push(libc::pollfd { fd: k.as_raw_fd(), events: libc::POLLIN, revents: 0 });
        }
        if unsafe { libc::poll(pfd.as_mut_ptr(), pfd.len() as _, -1) } < 0 {
            continue;
        }
        if pfd.len() > 1 && pfd[1].revents & libc::POLLIN != 0 {
            e.atender_kick();
        }
        if pfd[0].revents & (libc::POLLIN | libc::POLLHUP) != 0 {
            let Some((pet, _banderas, carga, fds)) = recibir(&s) else {
                eprintln!("vhost-blk: QEMU cerró; {} avisos atendidos", e.peticiones);
                return;
            };
            e.mensaje(&s, pet, &carga, fds);
        }
    }
}
