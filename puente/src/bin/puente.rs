//! G1 de `docs/GRAFICOS_LOOP.md`: el puente de Wayland de la caja Linux, como Sommelier en ChromeOS. Corre dentro de
//! la caja (hoy, en el PC para las pruebas): los programas de Linux se conectan a él como a cualquier compositor, y él
//! lo pasa todo a DashOS por un solo transporte (`dashos_puente`, tramas).
//!
//!     puente WAYLAND_SOCKET TRANSPORTE
//!     puente --hostil TRANSPORTE
//!
//! `TRANSPORTE` es un socket de Unix (en el PC) o `vsock:PUERTO` (en la caja Linux: el puerto de DashOS, CID 2).
//! `--hostil` manda por el transporte tramas rotas, memorias que no existen, escrituras fuera y tamaños absurdos, y
//! acaba: DashOS tiene que seguir como si nada (H9).
//!
//! Los mensajes pasan tal cual. Lo único que no puede cruzar son los descriptores:
//! - **La memoria de `wl_shm`**: el puente proyecta el pool del programa y, en cada `wl_surface.commit`, copia al otro
//!   lado el búfer que se va a ver. Así no hace falta memoria compartida entre la máquina virtual y DashOS (llegará
//!   con virtio, y entonces sin copia).
//! - **Los búferes de dmabuf** (G6, los de Mesa): igual que un pool de `wl_shm`, uno por búfer; antes de copiar se
//!   espera a que la tarjeta acabe de pintar (`DMA_BUF_IOCTL_SYNC`). Solo con `create_immed`, que es lo que usa Mesa.
//! - **Otros descriptores** (un mapa de teclado, por ejemplo): se copia lo que tienen, una vez.
//! - **Lo que viene de DashOS** con memorias (el mapa de teclado que da el compositor): el puente crea un `memfd`
//!   con su contenido y se lo da al programa.

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};

use dashos_puente as p;
use wayland::tablas::{self, wl_buffer, wl_shm, wl_shm_pool, wl_surface, zwp_linux_buffer_params_v1};
use wayland::{Interface, Kind, Valor, leer_args, leer_cabecera};

struct Pool {
    base: *mut libc::c_void,
    tamano: usize,
    fd: RawFd,
    memoria: u32,
    /// La memoria de un búfer de dmabuf (G6): se suelta con su búfer.
    dmabuf: bool,
}

/// Desde aquí, los números de pool de los búferes de dmabuf (no chocan con objetos del programa).
const POOL_DMABUF: u32 = 0x8000_0000;

/// Espera a que la tarjeta acabe con un dmabuf antes de leerlo (empezar y acabar una lectura).
fn esperar_tarjeta(fd: RawFd) {
    const DMA_BUF_IOCTL_SYNC: libc::c_ulong = 0x4008_6200;
    for flags in [1u64, 1 | 4] {
        unsafe { libc::ioctl(fd, DMA_BUF_IOCTL_SYNC as _, &flags as *const u64) };
    }
}

struct Programa {
    sock: UnixStream,
    entrada: Vec<u8>,
    fds: VecDeque<RawFd>,
    objetos: HashMap<u32, &'static Interface>,
    pools: HashMap<u32, Pool>,
    /// Búfer → (pool, desde, bytes, bytes por fila).
    buferes: HashMap<u32, (u32, usize, usize, usize)>,
    /// Superficie → búfer puesto.
    superficies: HashMap<u32, u32>,
    /// Lo dañado de cada superficie: el pendiente (filas, del próximo commit) y la historia (cuadro, filas).
    danos: HashMap<u32, Danos>,
    /// Params de dmabuf → (pool, desde, bytes por fila) de su plano 0 (G6).
    planos: HashMap<u32, (u32, usize, usize)>,
    /// Búfer → cuadro de su superficie con el que se mandó por última vez.
    enviado: HashMap<u32, u64>,
    /// Bytes copiados al otro lado, para la medida.
    copiado: u64,
    cuadros: u64,
}

/// Cuántos cuadros de daño se recuerdan: un programa con más búferes que esto recibe la copia entera.
const HISTORIA: usize = 8;

#[derive(Default)]
struct Danos {
    cuadro: u64,
    pendiente: Vec<(usize, usize)>,
    todo: bool,
    historia: VecDeque<(u64, Vec<(usize, usize)>, bool)>,
}

impl Programa {
    fn nuevo(sock: UnixStream) -> Self {
        let mut objetos = HashMap::new();
        objetos.insert(1, &tablas::WL_DISPLAY);
        Self { sock, entrada: Vec::new(), fds: VecDeque::new(), objetos, pools: HashMap::new(), buferes: HashMap::new(), superficies: HashMap::new(), planos: HashMap::new(), danos: HashMap::new(), enviado: HashMap::new(), copiado: 0, cuadros: 0 }
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
                    pr.pools.insert(id, Pool { base, tamano, fd: fds[0], memoria, dmabuf: false });
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
                let (alto, linea) = (args[3].uint() as usize, args[4].uint() as usize);
                pr.buferes.insert(args[0].uint(), (objeto, args[1].uint() as usize, alto.saturating_mul(linea), linea));
                pr.enviado.remove(&args[0].uint());
            }
            ("zwp_linux_buffer_params_v1", zwp_linux_buffer_params_v1::req::ADD) if fds.len() == 1 && args.len() == 6 => {
                // El plano entero se proyecta y va a una memoria del otro lado de su tamaño.
                let tamano = unsafe { libc::lseek(fds[0], 0, libc::SEEK_END) }.max(0) as usize;
                let base = unsafe { libc::mmap(std::ptr::null_mut(), tamano.max(1), libc::PROT_READ, libc::MAP_SHARED, fds[0], 0) };
                let memoria = pu.memoria();
                pu.memoria_nueva(memoria, tamano as u64);
                if base != libc::MAP_FAILED && args[1].uint() == 0 {
                    pr.pools.insert(POOL_DMABUF | memoria, Pool { base, tamano, fd: fds[0], memoria, dmabuf: true });
                    pr.planos.insert(objeto, (POOL_DMABUF | memoria, args[2].uint() as usize, args[3].uint() as usize));
                } else {
                    if base != libc::MAP_FAILED {
                        unsafe { libc::munmap(base, tamano.max(1)) };
                    }
                    unsafe { libc::close(fds[0]) };
                }
                suyas.push(memoria);
            }
            ("zwp_linux_buffer_params_v1", zwp_linux_buffer_params_v1::req::CREATE_IMMED) if args.len() == 5 => {
                if let Some((pool, desde, linea)) = pr.planos.remove(&objeto) {
                    let alto = args[2].uint() as usize;
                    pr.buferes.insert(args[0].uint(), (pool, desde, alto.saturating_mul(linea), linea));
                    pr.enviado.remove(&args[0].uint());
                }
            }
            ("wl_buffer", wl_buffer::req::DESTROY) => {
                // Un búfer de dmabuf se lleva su memoria.
                if let Some((pool, ..)) = pr.buferes.remove(&objeto).filter(|b| b.0 & POOL_DMABUF != 0) {
                    if let Some(pl) = pr.pools.remove(&pool) {
                        unsafe {
                            libc::munmap(pl.base, pl.tamano.max(1));
                            libc::close(pl.fd);
                        }
                        pu.trama(p::MEMORIA_SOLTAR, 0, &pl.memoria.to_le_bytes());
                    }
                }
            }
            ("wl_surface", wl_surface::req::ATTACH) if !args.is_empty() => {
                pr.superficies.insert(objeto, args[0].uint());
            }
            ("wl_surface", wl_surface::req::DAMAGE | wl_surface::req::DAMAGE_BUFFER) if args.len() == 4 => {
                // Por filas: lo que importa para copiar son las filas del búfer (escala 1).
                let (y, alto) = (args[1].uint() as i32, args[3].uint() as i32);
                let d = pr.danos.entry(objeto).or_default();
                if y < 0 || alto <= 0 || y > 1 << 16 || alto > 1 << 16 {
                    d.todo = true;
                } else {
                    d.pendiente.push((y as usize, (y + alto) as usize));
                }
            }
            ("wl_surface", wl_surface::req::COMMIT) => {
                // Lo que se va a ver, al otro lado antes que el commit: lo cambiado desde la última vez que se mandó
                // este búfer (el programa puede ir alternando búferes, y su daño es frente al cuadro anterior).
                let d = pr.danos.entry(objeto).or_default();
                d.cuadro += 1;
                let cuadro = d.cuadro;
                let pendiente = std::mem::take(&mut d.pendiente);
                let todo_ahora = std::mem::take(&mut d.todo);
                d.historia.push_back((cuadro, pendiente, todo_ahora));
                if d.historia.len() > HISTORIA {
                    d.historia.pop_front();
                }
                if let Some(&b) = pr.superficies.get(&objeto) {
                    if let Some(&(pool, desde, largo, linea)) = pr.buferes.get(&b) {
                        let filas: Option<Vec<(usize, usize)>> = match pr.enviado.get(&b) {
                            Some(&ultimo) if d.historia.front().is_some_and(|h| h.0 <= ultimo + 1) => {
                                let mut f = Vec::new();
                                let mut todo = false;
                                for (c, r, t) in d.historia.iter().filter(|h| h.0 > ultimo) {
                                    let _ = c;
                                    todo |= *t;
                                    f.extend(r.iter().copied());
                                }
                                (!todo).then_some(f)
                            }
                            _ => None,
                        };
                        if let Some(pl) = pr.pools.get(&pool) {
                            if pl.dmabuf {
                                esperar_tarjeta(pl.fd);
                            }
                            let tramos: Vec<(usize, usize)> = match filas {
                                None => vec![(desde, (desde + largo).min(pl.tamano))],
                                Some(mut f) => {
                                    f.sort();
                                    let mut juntas: Vec<(usize, usize)> = Vec::new();
                                    for (a, z) in f {
                                        match juntas.last_mut() {
                                            Some(u) if a <= u.1 => u.1 = u.1.max(z),
                                            _ => juntas.push((a, z)),
                                        }
                                    }
                                    juntas.into_iter().map(|(a, z)| (desde + a * linea, (desde + z * linea).min(desde + largo).min(pl.tamano))).collect()
                                }
                            };
                            for (ini, fin) in tramos {
                                if ini < fin && pl.base != libc::MAP_FAILED {
                                    let bytes = unsafe { std::slice::from_raw_parts((pl.base as *const u8).add(ini), fin - ini) };
                                    p::escribir_memoria(&mut pu.transporte, pl.memoria, ini as u64, bytes).expect("el transporte se cerró");
                                    pr.copiado += bytes.len() as u64;
                                }
                            }
                            pr.cuadros += 1;
                        }
                        pr.enviado.insert(b, cuadro);
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

/// Conecta con el transporte: `vsock:PUERTO` o la ruta de un socket de Unix.
fn conectar(transporte: &str) -> std::io::Result<UnixStream> {
    match transporte.strip_prefix("vsock:") {
        Some(puerto) => p::vsock(puerto.parse().map_err(|_| std::io::Error::other("puerto de vsock que no vale"))?),
        None => UnixStream::connect(transporte),
    }
}

/// Lo de `--hostil`: cada tanda por una conexión nueva del transporte (las que rompen el flujo hacen que DashOS la
/// corte). Cuenta las que DashOS cortó.
fn hostil(transporte: &str) {
    let mem = |tipo: u8, n: u32, a: u64, resto: &[u8]| {
        let mut d = n.to_le_bytes().to_vec();
        d.extend_from_slice(&a.to_le_bytes());
        d.extend_from_slice(resto);
        wayland::puente::trama(tipo, 0, &d)
    };
    let mut tandas: Vec<(&str, Vec<u8>)> = Vec::new();
    // Memorias: que no existen, escrituras fuera, tamaños absurdos.
    let mut t = mem(p::MEMORIA_ESCRIBIR, 4242, 0, b"no existe");
    t.extend(mem(p::MEMORIA_NUEVA, 1, 4096, &[]));
    t.extend(mem(p::MEMORIA_ESCRIBIR, 1, 4000, &[0xee; 200]));
    t.extend(mem(p::MEMORIA_ESCRIBIR, 1, u64::MAX - 8, &[0xee; 16]));
    t.extend(mem(p::MEMORIA_NUEVA, 2, u64::MAX, &[]));
    t.extend(mem(p::MEMORIA_NUEVA, 3, 1 << 40, &[]));
    for i in 0..300 {
        t.extend(mem(p::MEMORIA_NUEVA, 100 + i, 1 << 20, &[]));
    }
    t.extend(mem(p::MEMORIA_CRECER, 1, 1 << 30, &[]));
    tandas.push(("memorias", t));
    // Clientes: datos de uno que no existe, números de memoria que no existen, uno repetido, un montón.
    let mut t = wayland::puente::trama(p::DATOS, 99, &p::juntar_datos(&[], &[1, 2, 3, 4, 5, 6, 7, 8]));
    t.extend(wayland::puente::trama(p::CONECTAR, 1, &[]));
    t.extend(wayland::puente::trama(p::DATOS, 1, &p::juntar_datos(&[7777, 8888], &[0xff; 64])));
    t.extend(wayland::puente::trama(p::CONECTAR, 2, &[]));
    t.extend(wayland::puente::trama(p::CONECTAR, 2, &[]));
    t.extend(wayland::puente::trama(p::CONECTAR, 3, &[]));
    // Wayland roto para el compositor: un objeto que no existe y un tamaño que no es múltiplo de 4.
    t.extend(wayland::puente::trama(p::DATOS, 3, &p::juntar_datos(&[], &[9, 9, 9, 9, 7, 0, 0, 0, 1, 2, 3])));
    t.extend(wayland::puente::trama(p::DATOS, 3, &[0xff, 0xff, 0xff, 0x7f]));
    for c in 10..100 {
        t.extend(wayland::puente::trama(p::CONECTAR, c, &[]));
    }
    tandas.push(("clientes", t));
    // Tramas que rompen el flujo: un tipo que no existe, una de 4 GB, una de memoria sin su número.
    tandas.push(("tipo", wayland::puente::trama(200, 0, &[0; 32])));
    let mut t = vec![p::DATOS, 0, 0, 0, 0];
    t.extend_from_slice(&u32::MAX.to_le_bytes());
    t.extend_from_slice(&[0x55; 4096]);
    tandas.push(("larga", t));
    tandas.push(("corta", wayland::puente::trama(p::MEMORIA_ESCRIBIR, 0, &[1, 2, 3])));
    let mut cortadas = 0;
    for (nombre, t) in tandas {
        let mut s = match conectar(transporte) {
            Ok(s) => s,
            Err(e) => {
                println!("puente hostil: {}: no conecta ({})", nombre, e);
                continue;
            }
        };
        let escrito = s.write_all(&t).is_ok();
        // DashOS corta la conexión (o contesta ADIOS): se lee lo que diga durante un momento.
        s.set_read_timeout(Some(std::time::Duration::from_millis(1500))).ok();
        let mut b = [0u8; 4096];
        let mut cortada = !escrito;
        loop {
            match std::io::Read::read(&mut s, &mut b) {
                Ok(0) => {
                    cortada = true;
                    break;
                }
                Ok(_) => continue,
                Err(_) => break,
            }
        }
        cortadas += cortada as u32;
        println!("puente hostil: {}: {} bytes, {}", nombre, t.len(), if cortada { "DashOS cortó" } else { "sigue abierta" });
    }
    println!("puente hostil: hecho, {} conexiones cortadas por DashOS", cortadas);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--hostil") {
        hostil(args.get(2).map_or("vsock:5000", String::as_str));
        return;
    }
    let (Some(ruta), Some(transporte)) = (args.get(1), args.get(2)) else {
        eprintln!("uso: puente WAYLAND_SOCKET TRANSPORTE | puente --hostil TRANSPORTE");
        std::process::exit(2);
    };
    let _ = std::fs::remove_file(ruta);
    let escucha = UnixListener::bind(ruta).expect("no se abre el socket de Wayland");
    escucha.set_nonblocking(true).unwrap();
    // En la caja, el puente puede arrancar antes de que DashOS escuche: se reintenta un rato.
    let mut t = conectar(transporte);
    for _ in 0..50 {
        if t.is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
        t = conectar(transporte);
    }
    let t = t.expect("no hay transporte a DashOS");
    println!("puente: Wayland en {}, hacia DashOS por {}", ruta, transporte);
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
                if pr.cuadros > 0 {
                    println!("puente: cliente {}: {} cuadros, {} bytes copiados, {} por cuadro", c, pr.cuadros, pr.copiado, pr.copiado / pr.cuadros);
                }
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
