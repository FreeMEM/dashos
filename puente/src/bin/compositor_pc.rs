//! G0 de `docs/GRAFICOS_LOOP.md`: el compositor de DashOS (`wayland/`: el servidor y la escena) corriendo en el PC,
//! detrás de un socket de Unix como el de cualquier compositor de Linux. Sirve para probarlo con clientes de verdad
//! (libwayland, `swaybg`, `swaylock`, `alacritty`) sin QEMU. Los descriptores que llegan con los mensajes hacen de
//! llaves: cada uno toma un número, y la memoria de un `wl_shm` se proyecta con `mmap`.
//!
//!     compositor-pc SOCKET [--captura FICHERO.ppm] [--salir-sin-clientes]
//!
//! Con `--captura`, al acabar guarda la pantalla (800 × 600) en PPM. Con `--salir-sin-clientes`, acaba cuando se va
//! el último cliente (para las pruebas).

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;
use std::os::fd::{AsRawFd, IntoRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::rc::Rc;
use std::time::Instant;

use wayland::escena::{self, Cliente, Escena};
use wayland::servidor::{Conexion, Global};
use wayland::shm::Memorias;
use wayland::tablas;

const ANCHO: i32 = 800;
const ALTO: i32 = 600;
const FONDO: u32 = 0xff20_4060;

static GLOBALES: [Global; 4] = [
    Global { nombre: 1, interfaz: &tablas::WL_COMPOSITOR, version: 6 },
    Global { nombre: 2, interfaz: &tablas::WL_SHM, version: 1 },
    Global { nombre: 3, interfaz: &tablas::XDG_WM_BASE, version: 6 },
    Global { nombre: 4, interfaz: &tablas::WL_SEAT, version: 7 },
];

/// Los descriptores que hacen de llaves, y lo que hay proyectado de cada uno.
#[derive(Default)]
struct Tabla {
    siguiente: u32,
    fds: HashMap<u32, RawFd>,
    proyectado: HashMap<u32, (*mut libc::c_void, usize)>,
}

impl Tabla {
    fn poner(&mut self, fd: RawFd) -> u32 {
        self.siguiente += 1;
        self.fds.insert(self.siguiente, fd);
        self.siguiente
    }

    /// Saca la llave para mandarla: el descriptor pasa al cliente y aquí se cierra después de mandarlo.
    fn sacar(&mut self, llave: u32) -> Option<RawFd> {
        self.fds.remove(&llave)
    }
}

#[derive(Clone, Default)]
struct MemoriasPc(Rc<RefCell<Tabla>>);

impl Memorias for MemoriasPc {
    fn proyectar(&mut self, llave: u32, tamano: usize) -> Option<*const u8> {
        let mut t = self.0.borrow_mut();
        let fd = *t.fds.get(&llave)?;
        if let Some((viejo, largo)) = t.proyectado.remove(&llave) {
            unsafe { libc::munmap(viejo, largo) };
        }
        let p = unsafe { libc::mmap(std::ptr::null_mut(), tamano, libc::PROT_READ, libc::MAP_SHARED, fd, 0) };
        if p == libc::MAP_FAILED {
            return None;
        }
        t.proyectado.insert(llave, (p, tamano));
        Some(p as *const u8)
    }

    fn soltar(&mut self, llave: u32, _base: *const u8, _tamano: usize) {
        let mut t = self.0.borrow_mut();
        if let Some((p, largo)) = t.proyectado.remove(&llave) {
            unsafe { libc::munmap(p, largo) };
        }
        if let Some(fd) = t.fds.remove(&llave) {
            unsafe { libc::close(fd) };
        }
    }

    fn llave_vacia(&mut self) -> Option<u32> {
        let fd = unsafe { libc::memfd_create(c"dashos-vacio".as_ptr(), libc::MFD_CLOEXEC) };
        (fd >= 0).then(|| self.0.borrow_mut().poner(fd))
    }
}

struct DelCliente {
    sock: UnixStream,
    con: Conexion,
}

/// Lee lo que haya: bytes y descriptores (SCM_RIGHTS). `None` si el cliente cerró.
fn recibir(sock: &UnixStream, bytes: &mut Vec<u8>, fds: &mut Vec<RawFd>) -> Option<()> {
    let mut buf = [0u8; 4096];
    let mut control = [0u8; 256];
    let mut iov = libc::iovec { iov_base: buf.as_mut_ptr().cast(), iov_len: buf.len() };
    let mut m: libc::msghdr = unsafe { std::mem::zeroed() };
    m.msg_iov = &mut iov;
    m.msg_iovlen = 1;
    m.msg_control = control.as_mut_ptr().cast();
    m.msg_controllen = control.len() as _;
    let n = unsafe { libc::recvmsg(sock.as_raw_fd(), &mut m, libc::MSG_DONTWAIT | libc::MSG_CMSG_CLOEXEC) };
    if n == 0 {
        return None;
    }
    if n < 0 {
        let e = std::io::Error::last_os_error();
        return (e.kind() == std::io::ErrorKind::WouldBlock).then_some(());
    }
    bytes.extend_from_slice(&buf[..n as usize]);
    let mut c = unsafe { libc::CMSG_FIRSTHDR(&m) };
    while !c.is_null() {
        let h = unsafe { &*c };
        if h.cmsg_level == libc::SOL_SOCKET && h.cmsg_type == libc::SCM_RIGHTS {
            let datos = unsafe { libc::CMSG_DATA(c) } as *const RawFd;
            let cuantos = (h.cmsg_len as usize - unsafe { libc::CMSG_LEN(0) } as usize) / size_of::<RawFd>();
            for i in 0..cuantos {
                fds.push(unsafe { datos.add(i).read_unaligned() });
            }
        }
        c = unsafe { libc::CMSG_NXTHDR(&m, c) };
    }
    Some(())
}

/// Manda bytes y descriptores (los descriptores, con el primer trozo).
fn mandar(sock: &UnixStream, bytes: &[u8], fds: &[RawFd]) -> bool {
    let mut hecho = 0;
    let mut con_fds = !fds.is_empty();
    while hecho < bytes.len() {
        let mut iov = libc::iovec { iov_base: bytes[hecho..].as_ptr() as *mut _, iov_len: bytes.len() - hecho };
        let mut control = vec![0u8; unsafe { libc::CMSG_SPACE((fds.len() * size_of::<RawFd>()) as u32) } as usize];
        let mut m: libc::msghdr = unsafe { std::mem::zeroed() };
        m.msg_iov = &mut iov;
        m.msg_iovlen = 1;
        if con_fds {
            m.msg_control = control.as_mut_ptr().cast();
            m.msg_controllen = control.len() as _;
            unsafe {
                let c = libc::CMSG_FIRSTHDR(&m);
                (*c).cmsg_level = libc::SOL_SOCKET;
                (*c).cmsg_type = libc::SCM_RIGHTS;
                (*c).cmsg_len = libc::CMSG_LEN((fds.len() * size_of::<RawFd>()) as u32) as _;
                std::ptr::copy_nonoverlapping(fds.as_ptr(), libc::CMSG_DATA(c) as *mut RawFd, fds.len());
            }
        }
        let n = unsafe { libc::sendmsg(sock.as_raw_fd(), &m, libc::MSG_NOSIGNAL) };
        if n <= 0 {
            return false;
        }
        con_fds = false;
        hecho += n as usize;
    }
    true
}

/// Lo que el servidor tiene para el cliente, con sus llaves vueltas descriptores.
fn vaciar(c: &mut DelCliente, tabla: &Rc<RefCell<Tabla>>) -> bool {
    let e = c.con.sacar();
    if e.bytes.is_empty() {
        return true;
    }
    let fds: Vec<RawFd> = e.llaves.iter().filter_map(|&l| tabla.borrow_mut().sacar(l)).collect();
    let bien = mandar(&c.sock, &e.bytes, &fds);
    for fd in fds {
        unsafe { libc::close(fd) };
    }
    bien
}

fn captura(fichero: &str, marco: &[u32]) {
    let mut f = std::fs::File::create(fichero).expect("no se crea la captura");
    write!(f, "P6\n{} {}\n255\n", ANCHO, ALTO).unwrap();
    let px: Vec<u8> = marco.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]).collect();
    f.write_all(&px).unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(ruta) = args.get(1) else {
        eprintln!("uso: compositor-pc SOCKET [--captura FICHERO.ppm] [--salir-sin-clientes]");
        std::process::exit(2);
    };
    let fichero = args.iter().position(|a| a == "--captura").and_then(|i| args.get(i + 1)).cloned();
    let salir_sin_clientes = args.iter().any(|a| a == "--salir-sin-clientes");
    let _ = std::fs::remove_file(ruta);
    let escucha = UnixListener::bind(ruta).expect("no se abre el socket");
    escucha.set_nonblocking(true).unwrap();

    let tabla = Rc::new(RefCell::new(Tabla::default()));
    let mut escena = Escena::new(MemoriasPc(tabla.clone()), FONDO);
    let mut marco = vec![0u32; (ANCHO * ALTO) as usize];
    let mut clientes: HashMap<Cliente, DelCliente> = HashMap::new();
    let mut siguiente: Cliente = 1;
    let mut alguno = false;
    let empiezo = Instant::now();
    let mut capturada = false;
    loop {
        // Esperar a algo (o a 16 ms, el cuadro siguiente).
        let mut pfd = vec![libc::pollfd { fd: escucha.as_raw_fd(), events: libc::POLLIN, revents: 0 }];
        let orden: Vec<Cliente> = clientes.keys().copied().collect();
        pfd.extend(orden.iter().map(|c| libc::pollfd { fd: clientes[c].sock.as_raw_fd(), events: libc::POLLIN, revents: 0 }));
        unsafe { libc::poll(pfd.as_mut_ptr(), pfd.len() as _, 16) };
        if let Ok((sock, _)) = escucha.accept() {
            sock.set_nonblocking(true).unwrap();
            clientes.insert(siguiente, DelCliente { sock, con: Conexion::new() });
            println!("compositor-pc: cliente {} conectado", siguiente);
            siguiente += 1;
            alguno = true;
        }
        let mut fuera = Vec::new();
        for (&id, c) in clientes.iter_mut() {
            let (mut bytes, mut fds) = (Vec::new(), Vec::new());
            if recibir(&c.sock, &mut bytes, &mut fds).is_none() {
                fuera.push(id);
                continue;
            }
            if bytes.is_empty() {
                continue;
            }
            let llaves: Vec<u32> = fds.into_iter().map(|fd| tabla.borrow_mut().poner(fd)).collect();
            if c.con.juntador.meter(&bytes, &llaves).is_err() {
                fuera.push(id);
                continue;
            }
            c.con.atender(&GLOBALES, &mut escena.de(id));
            if let Some(e) = &c.con.roto {
                println!("compositor-pc: error de protocolo del cliente {}: {}", id, e.texto);
            }
            if !vaciar(c, &tabla) || c.con.roto.is_some() {
                fuera.push(id);
            }
        }
        // La captura, antes de que se vaya el último cliente: con su ventana aún en pantalla.
        if let Some(f) = fichero.as_ref().filter(|_| !fuera.is_empty() && fuera.len() == clientes.len()) {
            escena.componer(&mut marco, ANCHO, ALTO, ANCHO as usize, empiezo.elapsed().as_millis() as u32, &mut |_, _, _| {});
            captura(f, &marco);
            println!("compositor-pc: captura en {}", f);
            capturada = true;
        }
        for id in fuera {
            if let Some(c) = clientes.remove(&id) {
                println!("compositor-pc: cliente {} se va", id);
                escena.adios(id);
                let mut j = c.con;
                while let Some(l) = j.juntador.tomar_llave() {
                    if let Some(fd) = tabla.borrow_mut().sacar(l) {
                        unsafe { libc::close(fd) };
                    }
                }
            }
        }
        // Componer, y avisar a quien esperaba su cuadro.
        let ms = empiezo.elapsed().as_millis() as u32;
        let mut avisos = Vec::new();
        escena.componer(&mut marco, ANCHO, ALTO, ANCHO as usize, ms, &mut |cl, aviso, ms| avisos.push((cl, aviso, ms)));
        for (cl, aviso, ms) in avisos {
            if let Some(c) = clientes.get_mut(&cl) {
                escena::avisar_frame(&mut c.con, aviso, ms);
                vaciar(c, &tabla);
            }
        }
        escena.pendientes.clear();
        if salir_sin_clientes && alguno && clientes.is_empty() {
            break;
        }
    }
    if let Some(f) = fichero.filter(|_| !capturada) {
        captura(&f, &marco);
        println!("compositor-pc: captura en {}", f);
    }
    let _ = std::fs::remove_file(ruta);
    let _ = escucha.into_raw_fd();
}
