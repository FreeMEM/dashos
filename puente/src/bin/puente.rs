//! G1 de `docs/GRAFICOS_LOOP.md`: el puente de Wayland de la caja Linux, como Sommelier en ChromeOS. Corre dentro de
//! la caja (hoy, en el PC para las pruebas): los programas de Linux se conectan a él como a cualquier compositor, y él
//! lo pasa todo a DashOS por un solo transporte (`dashos_puente`, tramas).
//!
//!     puente WAYLAND_SOCKET TRANSPORTE
//!
//! Los mensajes pasan tal cual. Lo único que no puede cruzar son los descriptores:
//! - **La memoria de `wl_shm`**: el puente proyecta el pool del programa y, en cada `wl_surface.commit`, copia al otro
//!   lado el búfer que se va a ver. Así no hace falta memoria compartida entre la máquina virtual y DashOS (llegará
//!   con virtio, y entonces sin copia).
//! - **Otros descriptores** (un mapa de teclado, por ejemplo): se copia lo que tienen, una vez.
//! - **Lo que viene de DashOS** con memorias (el mapa de teclado que da el compositor): el puente crea un `memfd`
//!   con su contenido y se lo da al programa.

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};

use dashos_puente as p;
use wayland::tablas::{self, wl_shm, wl_shm_pool, wl_surface};
use wayland::{Interface, Kind, Valor, leer_args, leer_cabecera};

struct Pool {
    base: *mut libc::c_void,
    tamano: usize,
    fd: RawFd,
    memoria: u32,
}

struct Programa {
    sock: UnixStream,
    entrada: Vec<u8>,
    fds: VecDeque<RawFd>,
    objetos: HashMap<u32, &'static Interface>,
    pools: HashMap<u32, Pool>,
    /// Búfer → (pool, desde, bytes).
    buferes: HashMap<u32, (u32, usize, usize)>,
    /// Superficie → búfer puesto.
    superficies: HashMap<u32, u32>,
}

impl Programa {
    fn nuevo(sock: UnixStream) -> Self {
        let mut objetos = HashMap::new();
        objetos.insert(1, &tablas::WL_DISPLAY);
        Self { sock, entrada: Vec::new(), fds: VecDeque::new(), objetos, pools: HashMap::new(), buferes: HashMap::new(), superficies: HashMap::new() }
    }
}

struct Puente {
    transporte: UnixStream,
    siguiente_memoria: u32,
}

impl Puente {
    fn memoria(&mut self) -> u32 {
        self.siguiente_memoria += 1;
        self.siguiente_memoria
    }

    fn trama(&mut self, tipo: u8, conexion: u32, datos: &[u8]) {
        p::escribir_trama(&mut self.transporte, tipo, conexion, datos).expect("el transporte se cerró");
    }

    fn memoria_nueva(&mut self, numero: u32, tamano: u64) {
        let mut d = numero.to_le_bytes().to_vec();
        d.extend_from_slice(&tamano.to_le_bytes());
        self.trama(p::MEMORIA_NUEVA, 0, &d);
    }

    /// Copia lo que tiene un descriptor cualquiera a una memoria nueva del otro lado, y lo cierra.
    fn copiar_descriptor(&mut self, fd: RawFd) -> u32 {
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        let tamano = if unsafe { libc::fstat(fd, &mut st) } == 0 { st.st_size.max(0) as usize } else { 0 };
        let mut datos = vec![0u8; tamano];
        if tamano > 0 {
            unsafe { libc::pread(fd, datos.as_mut_ptr().cast(), tamano, 0) };
        }
        unsafe { libc::close(fd) };
        let n = self.memoria();
        self.memoria_nueva(n, tamano as u64);
        p::escribir_memoria(&mut self.transporte, n, 0, &datos).expect("el transporte se cerró");
        n
    }
}

/// Atiende los mensajes enteros que haya de un programa: los manda al otro lado, con lo que haga falta de memoria.
fn atender(pr: &mut Programa, conexion: u32, pu: &mut Puente) {
    let mut salida = Vec::new();
    let mut llaves = Vec::new();
    while let Ok((objeto, orden, tam)) = leer_cabecera(&pr.entrada) {
        let msg: Vec<u8> = pr.entrada.drain(..tam).collect();
        let Some(&iface) = pr.objetos.get(&objeto) else {
            salida.extend_from_slice(&msg); // un objeto que no conocemos: que lo rechace el compositor
            continue;
        };
        let Some(mensaje) = iface.requests.get(orden as usize) else {
            salida.extend_from_slice(&msg);
            continue;
        };
        // Los descriptores de este mensaje, en orden, de los que llegaron con los bytes.
        let cuantos = mensaje.args.iter().filter(|a| matches!(a.kind, Kind::Fd)).count();
        let fds: Vec<RawFd> = (0..cuantos).filter_map(|_| pr.fds.pop_front()).collect();
        let mut marcadores = (0..cuantos as u32).map(|i| i + 1);
        let args = leer_args(mensaje, &msg[8..], &mut marcadores).unwrap_or_default();
        // Los objetos nuevos y su interfaz.
        for (a, v) in mensaje.args.iter().zip(args.iter()) {
            match (v, a.interface) {
                (Valor::NewId(id), Some(nombre)) => {
                    if let Some(i) = wayland::interfaz(nombre) {
                        pr.objetos.insert(*id, i);
                    }
                }
                (Valor::NewIdDe { interface, id, .. }, _) => {
                    if let Some(i) = wayland::interfaz(interface) {
                        pr.objetos.insert(*id, i);
                    }
                }
                _ => {}
            }
        }
        let mut suyas = Vec::new();
        match (iface.name, orden) {
            ("wl_shm", wl_shm::req::CREATE_POOL) if fds.len() == 1 && args.len() == 3 => {
                let (id, tamano) = (args[0].uint(), args[2].uint() as usize);
                let base = unsafe { libc::mmap(std::ptr::null_mut(), tamano.max(1), libc::PROT_READ, libc::MAP_SHARED, fds[0], 0) };
                let memoria = pu.memoria();
                pu.memoria_nueva(memoria, tamano as u64);
                if base != libc::MAP_FAILED {
                    pr.pools.insert(id, Pool { base, tamano, fd: fds[0], memoria });
                } else {
                    unsafe { libc::close(fds[0]) };
                }
                suyas.push(memoria);
            }
            ("wl_shm_pool", wl_shm_pool::req::RESIZE) if !args.is_empty() => {
                if let Some(pool) = pr.pools.get_mut(&objeto) {
                    let nuevo = args[0].uint() as usize;
                    unsafe { libc::munmap(pool.base, pool.tamano.max(1)) };
                    pool.base = unsafe { libc::mmap(std::ptr::null_mut(), nuevo.max(1), libc::PROT_READ, libc::MAP_SHARED, pool.fd, 0) };
                    pool.tamano = nuevo;
                    let mut d = pool.memoria.to_le_bytes().to_vec();
                    d.extend_from_slice(&(nuevo as u64).to_le_bytes());
                    pu.trama(p::MEMORIA_CRECER, 0, &d);
                }
            }
            ("wl_shm_pool", wl_shm_pool::req::CREATE_BUFFER) if args.len() == 6 => {
                let largo = (args[4].uint() as usize).saturating_mul(args[3].uint() as usize);
                pr.buferes.insert(args[0].uint(), (objeto, args[1].uint() as usize, largo));
            }
            ("wl_surface", wl_surface::req::ATTACH) if !args.is_empty() => {
                pr.superficies.insert(objeto, args[0].uint());
            }
            ("wl_surface", wl_surface::req::COMMIT) => {
                // Lo que se va a ver, al otro lado antes que el commit.
                if let Some(&(pool, desde, largo)) = pr.superficies.get(&objeto).and_then(|b| pr.buferes.get(b)) {
                    if let Some(pl) = pr.pools.get(&pool) {
                        let fin = (desde + largo).min(pl.tamano);
                        if desde < fin && pl.base != libc::MAP_FAILED {
                            let bytes = unsafe { std::slice::from_raw_parts((pl.base as *const u8).add(desde), fin - desde) };
                            p::escribir_memoria(&mut pu.transporte, pl.memoria, desde as u64, bytes).expect("el transporte se cerró");
                        }
                    }
                }
            }
            _ => {
                for fd in fds {
                    suyas.push(pu.copiar_descriptor(fd));
                }
            }
        }
        salida.extend_from_slice(&msg);
        llaves.extend(suyas);
    }
    if !salida.is_empty() {
        pu.trama(p::DATOS, conexion, &p::juntar_datos(&llaves, &salida));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(ruta), Some(transporte)) = (args.get(1), args.get(2)) else {
        eprintln!("uso: puente WAYLAND_SOCKET TRANSPORTE");
        std::process::exit(2);
    };
    let _ = std::fs::remove_file(ruta);
    let escucha = UnixListener::bind(ruta).expect("no se abre el socket de Wayland");
    escucha.set_nonblocking(true).unwrap();
    let t = UnixStream::connect(transporte).expect("no hay transporte a DashOS");
    // Bloqueante (la copia comparte el modo con la de escribir): se lee solo cuando `poll` dice que hay algo.
    let mut lectura = t.try_clone().unwrap();
    let mut pu = Puente { transporte: t, siguiente_memoria: 0 };
    let mut programas: HashMap<u32, Programa> = HashMap::new();
    let mut siguiente = 0u32;
    // Las memorias que manda DashOS: número → memfd.
    let mut de_dashos: HashMap<u32, RawFd> = HashMap::new();
    let mut entrada = Vec::new();
    loop {
        let mut pfd = vec![libc::pollfd { fd: escucha.as_raw_fd(), events: libc::POLLIN, revents: 0 }, libc::pollfd { fd: lectura.as_raw_fd(), events: libc::POLLIN, revents: 0 }];
        let orden: Vec<u32> = programas.keys().copied().collect();
        pfd.extend(orden.iter().map(|c| libc::pollfd { fd: programas[c].sock.as_raw_fd(), events: libc::POLLIN, revents: 0 }));
        unsafe { libc::poll(pfd.as_mut_ptr(), pfd.len() as _, 100) };
        if let Ok((sock, _)) = escucha.accept() {
            sock.set_nonblocking(true).unwrap();
            siguiente += 1;
            pu.trama(p::CONECTAR, siguiente, &[]);
            programas.insert(siguiente, Programa::nuevo(sock));
        }
        // De los programas hacia DashOS.
        let mut fuera = Vec::new();
        for (&c, pr) in programas.iter_mut() {
            let mut fds = Vec::new();
            if p::recibir(pr.sock.as_raw_fd(), &mut pr.entrada, &mut fds).is_none() {
                fuera.push(c);
                continue;
            }
            pr.fds.extend(fds);
            atender(pr, c, &mut pu);
        }
        for c in fuera {
            if let Some(pr) = programas.remove(&c) {
                for pl in pr.pools.values() {
                    unsafe {
                        libc::munmap(pl.base, pl.tamano.max(1));
                        libc::close(pl.fd);
                    }
                }
                pu.trama(p::ADIOS, c, &[]);
            }
        }
        // De DashOS hacia los programas.
        if pfd[1].revents & (libc::POLLIN | libc::POLLHUP) != 0 && p::leer_transporte(&mut lectura, &mut entrada).is_none() {
            eprintln!("puente: DashOS cerró el transporte");
            break;
        }
        for t in p::sacar_tramas(&mut entrada).expect("trama mala de DashOS") {
            match t.tipo {
                p::MEMORIA_NUEVA if t.datos.len() == 12 => {
                    let n = u32::from_le_bytes(t.datos[0..4].try_into().unwrap());
                    let tam = u64::from_le_bytes(t.datos[4..12].try_into().unwrap());
                    if let Some(fd) = p::memfd(c"dashos", tam) {
                        de_dashos.insert(n, fd);
                    }
                }
                p::MEMORIA_ESCRIBIR if t.datos.len() >= 12 => {
                    let n = u32::from_le_bytes(t.datos[0..4].try_into().unwrap());
                    let desde = u64::from_le_bytes(t.datos[4..12].try_into().unwrap());
                    if let Some(&fd) = de_dashos.get(&n) {
                        unsafe { libc::pwrite(fd, t.datos[12..].as_ptr().cast(), t.datos.len() - 12, desde as libc::off_t) };
                    }
                }
                p::DATOS => {
                    let Some((llaves, bytes)) = p::partir_datos(&t.datos) else { continue };
                    let fds: Vec<RawFd> = llaves.iter().filter_map(|n| de_dashos.remove(n)).collect();
                    if let Some(pr) = programas.get(&t.conexion) {
                        p::mandar(pr.sock.as_raw_fd(), bytes, &fds);
                    }
                    for fd in fds {
                        unsafe { libc::close(fd) };
                    }
                }
                p::ADIOS => {
                    programas.remove(&t.conexion);
                }
                _ => {}
            }
        }
        let _ = pu.transporte.flush();
    }
}
