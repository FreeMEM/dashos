//! G0 y G1 de `docs/GRAFICOS_LOOP.md`: el compositor de DashOS (`wayland/`: el servidor y la escena) corriendo en el
//! PC, detrás de un socket de Unix como el de cualquier compositor de Linux, y con el transporte del puente de la caja
//! Linux. Sirve para probarlo con clientes de verdad (libwayland, `swaybg`, `swaylock`, `alacritty`) sin QEMU.
//!
//!     compositor-pc SOCKET [--transporte FICHERO] [--captura FICHERO.ppm] [--salir-sin-clientes]
//!
//! - Por `SOCKET` llegan clientes de Wayland directos: los descriptores que traen hacen de llaves (cada uno toma un
//!   número) y la memoria de un `wl_shm` se proyecta con `mmap`.
//! - Por `--transporte` llega el puente (`puente`): tramas con clientes, bytes de Wayland y memorias con número, que
//!   aquí son `memfd`. Es lo que hará en DashOS el lado de la caja Linux, con canales y objetos de memoria.
//! - Con `--captura`, guarda la pantalla (800 × 600) en PPM antes de que se vaya el último cliente.
//! - Con `--salir-sin-clientes`, acaba cuando se va el último cliente (para las pruebas).

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::rc::Rc;
use std::time::Instant;

use dashos_puente as p;
use wayland::escena::{self, Cliente, Escena};
use wayland::servidor::{Conexion, Global};
use wayland::shm::Memorias;
use wayland::tablas;

const ANCHO: i32 = 800;
const ALTO: i32 = 600;
const FONDO: u32 = 0xff20_4060;
/// Los clientes del puente, desde aquí: no chocan con los directos.
const DEL_PUENTE: Cliente = 1 << 32;

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

    /// Saca la llave para mandarla: quien la saca cierra el descriptor después.
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
        let q = unsafe { libc::mmap(std::ptr::null_mut(), tamano, libc::PROT_READ, libc::MAP_SHARED, fd, 0) };
        if q == libc::MAP_FAILED {
            return None;
        }
        t.proyectado.insert(llave, (q, tamano));
        Some(q as *const u8)
    }

    fn soltar(&mut self, llave: u32, _base: *const u8, _tamano: usize) {
        let mut t = self.0.borrow_mut();
        if let Some((q, largo)) = t.proyectado.remove(&llave) {
            unsafe { libc::munmap(q, largo) };
        }
        if let Some(fd) = t.fds.remove(&llave) {
            unsafe { libc::close(fd) };
        }
    }

    fn llave_vacia(&mut self) -> Option<u32> {
        let fd = p::memfd(c"dashos-vacio", 0)?;
        Some(self.0.borrow_mut().poner(fd))
    }
}

/// Por dónde se contesta a un cliente.
enum Salida {
    Directo(UnixStream),
    Puente(u32),
}

struct DelCliente {
    salida: Salida,
    con: Conexion,
}

/// El lado de DashOS del transporte del puente.
struct Transporte {
    sock: UnixStream,
    entrada: Vec<u8>,
    /// Memoria del puente (su número) → llave de aquí.
    memorias: HashMap<u32, u32>,
    siguiente_memoria: u32,
}

/// Lo que el servidor tiene para el cliente, con sus llaves vueltas descriptores (directo) o memorias (puente).
fn vaciar(c: &mut DelCliente, tabla: &Rc<RefCell<Tabla>>, tr: &mut Option<Transporte>) -> bool {
    let e = c.con.sacar();
    if e.bytes.is_empty() {
        return true;
    }
    let fds: Vec<RawFd> = e.llaves.iter().filter_map(|&l| tabla.borrow_mut().sacar(l)).collect();
    let bien = match &c.salida {
        Salida::Directo(s) => p::mandar(s.as_raw_fd(), &e.bytes, &fds),
        Salida::Puente(conexion) => match tr.as_mut() {
            None => false,
            Some(t) => {
                // Cada descriptor cruza como una memoria con su contenido.
                let mut numeros = Vec::new();
                for &fd in &fds {
                    let mut st: libc::stat = unsafe { std::mem::zeroed() };
                    let tam = if unsafe { libc::fstat(fd, &mut st) } == 0 { st.st_size.max(0) as usize } else { 0 };
                    let mut datos = vec![0u8; tam];
                    if tam > 0 {
                        unsafe { libc::pread(fd, datos.as_mut_ptr().cast(), tam, 0) };
                    }
                    t.siguiente_memoria += 1;
                    let n = t.siguiente_memoria;
                    let mut d = n.to_le_bytes().to_vec();
                    d.extend_from_slice(&(tam as u64).to_le_bytes());
                    let _ = p::escribir_trama(&mut t.sock, p::MEMORIA_NUEVA, 0, &d);
                    let _ = p::escribir_memoria(&mut t.sock, n, 0, &datos);
                    numeros.push(n);
                }
                p::escribir_trama(&mut t.sock, p::DATOS, *conexion, &p::juntar_datos(&numeros, &e.bytes)).is_ok()
            }
        },
    };
    for fd in fds {
        unsafe { libc::close(fd) };
    }
    bien
}

fn captura(fichero: &str, marco: &[u32]) {
    let mut f = std::fs::File::create(fichero).expect("no se crea la captura");
    write!(f, "P6\n{} {}\n255\n", ANCHO, ALTO).unwrap();
    let px: Vec<u8> = marco.iter().flat_map(|q| [(q >> 16) as u8, (q >> 8) as u8, *q as u8]).collect();
    f.write_all(&px).unwrap();
}

/// Mete bytes de un cliente en su conexión y atiende. Falso si hay que soltarlo.
fn atender(id: Cliente, c: &mut DelCliente, bytes: &[u8], llaves: &[u32], escena: &mut Escena<MemoriasPc>, tabla: &Rc<RefCell<Tabla>>, tr: &mut Option<Transporte>) -> bool {
    if c.con.juntador.meter(bytes, llaves).is_err() {
        return false;
    }
    c.con.atender(&GLOBALES, &mut escena.de(id));
    if let Some(e) = &c.con.roto {
        println!("compositor-pc: error de protocolo del cliente {}: {}", id, e.texto);
    }
    vaciar(c, tabla, tr) && c.con.roto.is_none()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(ruta) = args.get(1) else {
        eprintln!("uso: compositor-pc SOCKET [--transporte FICHERO] [--captura FICHERO.ppm] [--salir-sin-clientes]");
        std::process::exit(2);
    };
    let opcion = |n: &str| args.iter().position(|a| a == n).and_then(|i| args.get(i + 1)).cloned();
    let fichero = opcion("--captura");
    let salir_sin_clientes = args.iter().any(|a| a == "--salir-sin-clientes");
    let _ = std::fs::remove_file(ruta);
    let escucha = UnixListener::bind(ruta).expect("no se abre el socket");
    escucha.set_nonblocking(true).unwrap();
    let escucha_tr = opcion("--transporte").map(|f| {
        let _ = std::fs::remove_file(&f);
        let l = UnixListener::bind(&f).expect("no se abre el transporte");
        l.set_nonblocking(true).unwrap();
        l
    });
    let mut tr: Option<Transporte> = None;

    let tabla = Rc::new(RefCell::new(Tabla::default()));
    let mut escena = Escena::new(MemoriasPc(tabla.clone()), FONDO);
    let mut marco = vec![0u32; (ANCHO * ALTO) as usize];
    let mut clientes: HashMap<Cliente, DelCliente> = HashMap::new();
    let mut siguiente: Cliente = 1;
    let mut alguno = false;
    let mut capturada = false;
    let empiezo = Instant::now();
    loop {
        // Esperar a algo (o a 16 ms, el cuadro siguiente).
        let mut pfd = vec![libc::pollfd { fd: escucha.as_raw_fd(), events: libc::POLLIN, revents: 0 }];
        if let Some(l) = &escucha_tr {
            pfd.push(libc::pollfd { fd: l.as_raw_fd(), events: libc::POLLIN, revents: 0 });
        }
        let i_tr = pfd.len();
        if let Some(t) = &tr {
            pfd.push(libc::pollfd { fd: t.sock.as_raw_fd(), events: libc::POLLIN, revents: 0 });
        }
        for c in clientes.values() {
            if let Salida::Directo(s) = &c.salida {
                pfd.push(libc::pollfd { fd: s.as_raw_fd(), events: libc::POLLIN, revents: 0 });
            }
        }
        unsafe { libc::poll(pfd.as_mut_ptr(), pfd.len() as _, 16) };
        if let Ok((sock, _)) = escucha.accept() {
            sock.set_nonblocking(true).unwrap();
            clientes.insert(siguiente, DelCliente { salida: Salida::Directo(sock), con: Conexion::new() });
            println!("compositor-pc: cliente {} conectado", siguiente);
            siguiente += 1;
            alguno = true;
        }
        if tr.is_none() {
            if let Some(Ok((sock, _))) = escucha_tr.as_ref().map(|l| l.accept()) {
                // Bloqueante: solo se lee cuando `poll` dice que hay algo.
                sock.set_nonblocking(false).unwrap();
                println!("compositor-pc: puente conectado");
                tr = Some(Transporte { sock, entrada: Vec::new(), memorias: HashMap::new(), siguiente_memoria: 0 });
            }
        }
        let mut fuera = Vec::new();
        // Lo del puente.
        let listo_tr = pfd.get(i_tr).is_some_and(|f| f.revents & (libc::POLLIN | libc::POLLHUP) != 0);
        if listo_tr && tr.is_some() {
            let cerrado = {
                let t = tr.as_mut().unwrap();
                p::leer_transporte(&mut t.sock, &mut t.entrada).is_none()
            };
            let tramas = p::sacar_tramas(&mut tr.as_mut().unwrap().entrada).unwrap_or_default();
            for t in tramas {
                let id = DEL_PUENTE + t.conexion as Cliente;
                match t.tipo {
                    p::CONECTAR => {
                        clientes.insert(id, DelCliente { salida: Salida::Puente(t.conexion), con: Conexion::new() });
                        println!("compositor-pc: cliente {} del puente conectado", t.conexion);
                        alguno = true;
                    }
                    p::MEMORIA_NUEVA if t.datos.len() == 12 => {
                        let n = u32::from_le_bytes(t.datos[0..4].try_into().unwrap());
                        let tam = u64::from_le_bytes(t.datos[4..12].try_into().unwrap());
                        if let Some(fd) = p::memfd(c"dashos-puente", tam) {
                            let l = tabla.borrow_mut().poner(fd);
                            tr.as_mut().unwrap().memorias.insert(n, l);
                        }
                    }
                    p::MEMORIA_CRECER if t.datos.len() == 12 => {
                        let n = u32::from_le_bytes(t.datos[0..4].try_into().unwrap());
                        let tam = u64::from_le_bytes(t.datos[4..12].try_into().unwrap());
                        let l = tr.as_ref().unwrap().memorias.get(&n).copied();
                        if let Some(&fd) = l.and_then(|l| tabla.borrow().fds.get(&l).copied()).as_ref() {
                            unsafe { libc::ftruncate(fd, tam as libc::off_t) };
                        }
                    }
                    p::MEMORIA_ESCRIBIR if t.datos.len() >= 12 => {
                        let n = u32::from_le_bytes(t.datos[0..4].try_into().unwrap());
                        let desde = u64::from_le_bytes(t.datos[4..12].try_into().unwrap());
                        let l = tr.as_ref().unwrap().memorias.get(&n).copied();
                        if let Some(fd) = l.and_then(|l| tabla.borrow().fds.get(&l).copied()) {
                            unsafe { libc::pwrite(fd, t.datos[12..].as_ptr().cast(), t.datos.len() - 12, desde as libc::off_t) };
                        }
                    }
                    p::DATOS => {
                        let Some((numeros, bytes)) = p::partir_datos(&t.datos) else { continue };
                        let llaves: Vec<u32> = numeros.iter().filter_map(|n| tr.as_ref().unwrap().memorias.get(n).copied()).collect();
                        if let Some(c) = clientes.get_mut(&id) {
                            if !atender(id, c, bytes, &llaves, &mut escena, &tabla, &mut tr) {
                                fuera.push(id);
                            }
                        }
                    }
                    p::ADIOS => fuera.push(id),
                    _ => {}
                }
            }
            if cerrado {
                println!("compositor-pc: el puente se fue");
                fuera.extend(clientes.iter().filter(|(_, c)| matches!(c.salida, Salida::Puente(_))).map(|(&id, _)| id));
                tr = None;
            }
        }
        // Los directos.
        let ids: Vec<Cliente> = clientes.keys().copied().filter(|id| *id < DEL_PUENTE).collect();
        for id in ids {
            let c = clientes.get_mut(&id).unwrap();
            let Salida::Directo(s) = &c.salida else { continue };
            let (mut bytes, mut fds) = (Vec::new(), Vec::new());
            if p::recibir(s.as_raw_fd(), &mut bytes, &mut fds).is_none() {
                fuera.push(id);
                continue;
            }
            if bytes.is_empty() {
                continue;
            }
            let llaves: Vec<u32> = fds.into_iter().map(|fd| tabla.borrow_mut().poner(fd)).collect();
            if !atender(id, c, &bytes, &llaves, &mut escena, &tabla, &mut tr) {
                fuera.push(id);
            }
        }
        fuera.sort();
        fuera.dedup();
        // La captura, antes de que se vaya el último cliente: con su ventana aún en pantalla.
        if let Some(f) = fichero.as_ref().filter(|_| !fuera.is_empty() && fuera.len() == clientes.len() && !capturada) {
            escena.componer(&mut marco, ANCHO, ALTO, ANCHO as usize, empiezo.elapsed().as_millis() as u32, &mut |_, _, _| {});
            captura(f, &marco);
            println!("compositor-pc: captura en {}", f);
            capturada = true;
        }
        for id in fuera {
            if let Some(c) = clientes.remove(&id) {
                println!("compositor-pc: cliente {} se va", id);
                if let (Salida::Puente(conexion), Some(t)) = (&c.salida, tr.as_mut()) {
                    let _ = p::escribir_trama(&mut t.sock, p::ADIOS, *conexion, &[]);
                }
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
                vaciar(c, &tabla, &mut tr);
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
}
