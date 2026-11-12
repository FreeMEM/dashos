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
//! - Con `--captura`, guarda la pantalla (800 × 600) en PPM cuando se va el primer cliente. Si el nombre lleva `%d`,
//!   cada vez que se va uno, ya sin lo suyo, numeradas desde 1 (G5). Con `--captura-tras MS`, a esos milisegundos
//!   de empezar, con todos aún dentro (G6).
//! - Con `--dri RUTA`, el dispositivo de render que se anuncia para dmabuf (G6); si no, `/dev/dri/renderD128`.
//! - Con `--escritorio`, todos los clientes tienen el poder de escritorio: `layer-shell` (G4).
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
/// `--escritorio`: todos los clientes con el poder de escritorio.
static ESCRITORIO: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Los clientes del puente, desde aquí: no chocan con los directos.
const DEL_PUENTE: Cliente = 1 << 32;

/// Con `zwp_linux_dmabuf_v1` (G6): búferes de la tarjeta gráfica, lineales, que se leen con la CPU. Solo en el PC
/// por ahora: en DashOS llegará con el puente (los dmabuf de la caja pasan a memoria).
static GLOBALES: [Global; 10] = [
    Global { nombre: 1, interfaz: &tablas::WL_COMPOSITOR, version: 6 },
    Global { nombre: 2, interfaz: &tablas::WL_SHM, version: 1 },
    Global { nombre: 3, interfaz: &tablas::XDG_WM_BASE, version: 6 },
    Global { nombre: 4, interfaz: &tablas::WL_SEAT, version: 7 },
    Global { nombre: 5, interfaz: &tablas::WL_SUBCOMPOSITOR, version: 1 },
    Global { nombre: 6, interfaz: &tablas::WL_OUTPUT, version: 4 },
    Global { nombre: 9, interfaz: &tablas::ZWP_LINUX_DMABUF_V1, version: 4 },
    // Como en el compositor de DashOS (H9): el portapapeles, la selección primaria y la barra de DashOS.
    Global { nombre: 14, interfaz: &tablas::WL_DATA_DEVICE_MANAGER, version: 3 },
    Global { nombre: 15, interfaz: &tablas::ZWP_PRIMARY_SELECTION_DEVICE_MANAGER_V1, version: 1 },
    Global { nombre: 16, interfaz: &tablas::ZXDG_DECORATION_MANAGER_V1, version: 1 },
];
/// Los de un escritorio (S10b, G4): además, las capas. Un cliente sin el poder ni los ve; si los pide igual, error.
static GLOBALES_ESCRITORIO: [Global; 12] = [
    Global { nombre: 1, interfaz: &tablas::WL_COMPOSITOR, version: 6 },
    Global { nombre: 2, interfaz: &tablas::WL_SHM, version: 1 },
    Global { nombre: 3, interfaz: &tablas::XDG_WM_BASE, version: 6 },
    Global { nombre: 4, interfaz: &tablas::WL_SEAT, version: 7 },
    Global { nombre: 5, interfaz: &tablas::WL_SUBCOMPOSITOR, version: 1 },
    Global { nombre: 6, interfaz: &tablas::WL_OUTPUT, version: 4 },
    Global { nombre: 7, interfaz: &tablas::ZWLR_LAYER_SHELL_V1, version: 4 },
    Global { nombre: 8, interfaz: &tablas::EXT_SESSION_LOCK_MANAGER_V1, version: 1 },
    Global { nombre: 9, interfaz: &tablas::ZWP_LINUX_DMABUF_V1, version: 4 },
    Global { nombre: 14, interfaz: &tablas::WL_DATA_DEVICE_MANAGER, version: 3 },
    Global { nombre: 15, interfaz: &tablas::ZWP_PRIMARY_SELECTION_DEVICE_MANAGER_V1, version: 1 },
    Global { nombre: 16, interfaz: &tablas::ZXDG_DECORATION_MANAGER_V1, version: 1 },
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

    fn llave_con(&mut self, datos: &'static [u8]) -> Option<u32> {
        let fd = p::memfd(c"dashos-datos", datos.len() as u64)?;
        unsafe { libc::pwrite(fd, datos.as_ptr().cast(), datos.len(), 0) };
        Some(self.0.borrow_mut().poner(fd))
    }

    /// G6: espera a que la tarjeta acabe con el dmabuf (DMA_BUF_IOCTL_SYNC, empezar y acabar una lectura). Si no es
    /// un dmabuf, la orden falla y no pasa nada.
    fn antes_de_leer(&mut self, llave: u32) {
        const DMA_BUF_IOCTL_SYNC: libc::c_ulong = 0x4008_6200;
        const LEER: u64 = 1;
        const ACABAR: u64 = 4;
        if let Some(&fd) = self.0.borrow().fds.get(&llave) {
            for flags in [LEER, LEER | ACABAR] {
                unsafe { libc::ioctl(fd, DMA_BUF_IOCTL_SYNC, &flags as *const u64) };
            }
        }
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
    /// Los tubos del portapapeles (H9), con tuberías: aquí hacen de los canales de DashOS.
    tubos: p::tubos::Tubos,
}

impl Transporte {
    fn tramas(&mut self, t: Vec<(u8, Vec<u8>)>) {
        for (tipo, d) in t {
            let _ = p::escribir_trama(&mut self.sock, tipo, 0, &d);
        }
    }
}

/// Lo que el servidor tiene para el cliente, con sus llaves vueltas descriptores (directo) o memorias (puente).
fn vaciar(c: &mut DelCliente, tabla: &Rc<RefCell<Tabla>>, tr: &mut Option<Transporte>) -> bool {
    let e = c.con.sacar();
    if e.bytes.is_empty() {
        return true;
    }
    let mut fds: Vec<RawFd> = e.llaves.iter().filter_map(|&l| tabla.borrow_mut().sacar(l)).collect();
    let bien = match &c.salida {
        Salida::Directo(s) => p::mandar(s.as_raw_fd(), &e.bytes, &fds),
        Salida::Puente(conexion) => match tr.as_mut() {
            None => false,
            Some(t) => {
                // Cada descriptor cruza como una memoria con su contenido; una tubería (`send` del portapapeles), como
                // un tubo (H9).
                let mut numeros = Vec::new();
                let mut tubos = Vec::new();
                for &fd in &fds {
                    if p::tubos::es_tuberia(fd) {
                        t.siguiente_memoria += 1;
                        let n = 0x8000_0000 | t.siguiente_memoria;
                        let mut tr = Vec::new();
                        t.tubos.escritor(n, fd, &mut tr);
                        t.tramas(tr);
                        numeros.push(n);
                        tubos.push(fd);
                        continue;
                    }
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
                let bien = p::escribir_trama(&mut t.sock, p::DATOS, *conexion, &p::juntar_datos(&numeros, &e.bytes)).is_ok();
                // Los de los tubos son ya de los tubos: no se cierran aquí.
                for fd in tubos {
                    if let Some(i) = fds.iter().position(|&f| f == fd) {
                        fds.remove(i);
                    }
                }
                bien
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
    // Con --escritorio, todos los clientes tienen el poder de escritorio (para probar barras y fondos de verdad).
    let globales: &[Global] = if ESCRITORIO.load(std::sync::atomic::Ordering::Relaxed) { &GLOBALES_ESCRITORIO } else { &GLOBALES };
    c.con.atender(globales, &mut escena.de(id));
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
    ESCRITORIO.store(args.iter().any(|a| a == "--escritorio"), std::sync::atomic::Ordering::Relaxed);
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
    // El dispositivo de render que usarán los clientes de dmabuf (G6): `--dri`, o el primero.
    let dri = opcion("--dri").unwrap_or_else(|| "/dev/dri/renderD128".into());
    if let Ok(m) = std::fs::metadata(&dri) {
        use std::os::unix::fs::MetadataExt;
        escena.dispositivo = m.rdev();
    }
    let mut marco = vec![0u32; (ANCHO * ALTO) as usize];
    let mut clientes: HashMap<Cliente, DelCliente> = HashMap::new();
    let mut siguiente: Cliente = 1;
    let mut alguno = false;
    let mut capturada = false;
    let numeradas = fichero.as_ref().is_some_and(|f| f.contains("%d"));
    let captura_tras: Option<u128> = opcion("--captura-tras").and_then(|t| t.parse().ok());
    let mut capturas = 0;
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
            // H9: sus tubos.
            pfd.extend(t.tubos.vigilar().into_iter().map(|(fd, ev, _)| libc::pollfd { fd, events: ev, revents: 0 }));
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
                tr = Some(Transporte { sock, entrada: Vec::new(), memorias: HashMap::new(), siguiente_memoria: 0, tubos: Default::default() });
            }
        }
        let mut fuera = Vec::new();
        if let Some(t) = tr.as_mut() {
            let mut v = Vec::new();
            t.tubos.mover(&mut v);
            t.tramas(v);
        }
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
                    p::TUBO_NUEVO if t.datos.len() == 4 => tr.as_mut().unwrap().tubos.anunciado(u32::from_le_bytes(t.datos[..4].try_into().unwrap())),
                    p::TUBO_LEIDO if t.datos.len() == 8 => {
                        let (n, k) = (u32::from_le_bytes(t.datos[..4].try_into().unwrap()), u32::from_le_bytes(t.datos[4..8].try_into().unwrap()));
                        tr.as_mut().unwrap().tubos.leido(n, k);
                    }
                    p::TUBO_DATOS | p::TUBO_FIN if t.datos.len() >= 4 => {
                        let n = u32::from_le_bytes(t.datos[..4].try_into().unwrap());
                        let tt = tr.as_mut().unwrap();
                        let mut v = Vec::new();
                        if t.tipo == p::TUBO_DATOS {
                            tt.tubos.datos(n, &t.datos[4..], &mut v);
                        } else {
                            tt.tubos.fin(n, &mut v);
                        }
                        tt.tramas(v);
                    }
                    p::DATOS => {
                        let Some((numeros, bytes)) = p::partir_datos(&t.datos) else { continue };
                        // Una memoria, o un tubo que pidió el puente (el extremo de escribir de una tubería nueva).
                        let llaves: Vec<u32> = numeros
                            .iter()
                            .filter_map(|n| {
                                let tt = tr.as_mut().unwrap();
                                tt.memorias.get(n).copied().or_else(|| tt.tubos.dar(*n).map(|fd| tabla.borrow_mut().poner(fd)))
                            })
                            .collect();
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
        // La captura, cuando se va el primer cliente, antes de quitar lo suyo: con su ventana aún en pantalla (y lo
        // de los demás, como un fondo de swaybg).
        if let Some(f) = fichero.as_ref().filter(|_| !fuera.is_empty() && !numeradas && !capturada && captura_tras.is_none()) {
            escena.componer(&mut marco, ANCHO, ALTO, ANCHO as usize, empiezo.elapsed().as_millis() as u32, &mut |_, _, _| {});
            captura(f, &marco);
            println!("compositor-pc: captura en {}", f);
            capturada = true;
        }
        let se_fue = !fuera.is_empty();
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
        if let Some(f) = fichero.as_ref().filter(|_| !capturada && captura_tras.is_some_and(|t| empiezo.elapsed().as_millis() >= t)) {
            captura(f, &marco);
            println!("compositor-pc: captura en {}", f);
            capturada = true;
        }
        if let Some(f) = fichero.as_ref().filter(|_| se_fue && numeradas) {
            capturas += 1;
            let f = f.replace("%d", &capturas.to_string());
            captura(&f, &marco);
            println!("compositor-pc: captura en {}", f);
        }
        for (cl, aviso, ms) in avisos {
            if let Some(c) = clientes.get_mut(&cl) {
                escena::avisar_frame(&mut c.con, aviso, ms);
                vaciar(c, &tabla, &mut tr);
            }
        }
        // H9: la entrada que salió de las peticiones (el teclado a una ventana nueva, las ofertas del portapapeles, los
        // configure al redimensionar) y lo del portapapeles para las fuentes.
        for (cl, ev) in core::mem::take(&mut escena.pendientes) {
            if let (Some(c), Some(de)) = (clientes.get_mut(&cl), escena.clientes.get_mut(&cl)) {
                escena::escribir_entrada(&mut c.con, de, ev, ms);
                vaciar(c, &tabla, &mut tr);
            }
        }
        for (cl, aviso) in core::mem::take(&mut escena.avisos_portapapeles) {
            match clientes.get_mut(&cl) {
                Some(c) => {
                    escena::portapapeles::escribir_aviso(&mut c.con, aviso);
                    vaciar(c, &tabla, &mut tr);
                }
                None => {
                    if let Some(fd) = aviso.llave().and_then(|l| tabla.borrow_mut().sacar(l)) {
                        unsafe { libc::close(fd) };
                    }
                }
            }
        }
        if salir_sin_clientes && alguno && clientes.is_empty() {
            break;
        }
    }
    if let Some(f) = fichero.filter(|_| !capturada && !numeradas) {
        captura(&f, &marco);
        println!("compositor-pc: captura en {}", f);
    }
    let _ = std::fs::remove_file(ruta);
}
