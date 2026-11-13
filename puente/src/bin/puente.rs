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
//!   lado lo dañado del búfer que se va a ver.
//! - **Sin copia**, si DashOS lo dice (`VISTAS`): el puente fija el pool en memoria (`mlock`: Linux ya no lo saca ni
//!   lo mueve), busca en `/proc/self/pagemap` en qué páginas físicas del invitado está (hace falta ser root) y se las
//!   manda (`MEMORIA_VISTA`); DashOS hace con ellas una vista de solo lectura de la RAM del invitado y el compositor lee
//!   el búfer ahí mismo. En cada commit se comprueba que siguen siendo esas páginas (si Linux las hubiera movido, la
//!   ventana enseñaría lo que haya ahora en las viejas: se cierra el programa). Si algo falla (sin permiso, DashOS dice
//!   `VISTA_NO`...), se copia como siempre. Con `/run/dashos-puente-copia`, los programas que conecten después van con
//!   copia (para medir las dos cosas en la misma prueba).
//! - **Los búferes de dmabuf** (G6, los de Mesa): igual que un pool de `wl_shm`, uno por búfer; antes de copiar se
//!   espera a que la tarjeta acabe de pintar (`DMA_BUF_IOCTL_SYNC`). Solo con `create_immed`, que es lo que usa Mesa.
//! - **Otros descriptores** (un mapa de teclado, por ejemplo): se copia lo que tienen, una vez.
//! - **Lo que viene de DashOS** con memorias (el mapa de teclado que da el compositor): el puente crea un `memfd`
//!   con su contenido y se lo da al programa.
//! - **Las tuberías del portapapeles** (H9): las que da un programa para pegar (`wl_data_offer.receive`) y las que
//!   pide DashOS para que un programa de Linux escriba lo copiado (`wl_data_source.send`) van como tubos
//!   (`dashos_puente::tubos`): bytes que pasan según llegan, hasta el fin.
//!
//! Para saber qué descriptores lleva cada mensaje, el puente sigue los objetos: los que crea el programa y los que crea
//! el compositor en sus eventos (las ofertas del portapapeles).
//!
//!     puente --cliente-hostil WAYLAND_SOCKET
//!
//! Un programa de Linux hostil (H9): decoraciones rotas, tipos del portapapeles de más, enormes o repetidos, una fuente
//! que escribe sin fin y otra que nunca cierra. El compositor y el puente siguen; las demás ventanas, también.

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};

use dashos_puente as p;

#[path = "../cliente_hostil.rs"]
mod cliente_hostil;
use wayland::tablas::{self, wl_buffer, wl_shm, wl_shm_pool, wl_surface, zwp_linux_buffer_params_v1};
use wayland::{Interface, Kind, Valor, leer_args, leer_cabecera};

struct Pool {
    base: *mut libc::c_void,
    tamano: usize,
    fd: RawFd,
    /// La memoria del otro lado donde van los búferes nuevos.
    memoria: u32,
    /// La memoria de un búfer de dmabuf (G6): se suelta con su búfer.
    dmabuf: bool,
    /// Lo que mide la memoria del otro lado (en DashOS, páginas enteras): un `resize` hasta ahí vale tal cual.
    capacidad: usize,
    /// El `wl_shm` del que salió (para crear su sucesor).
    shm: u32,
    /// Un objeto de memoria de DashOS no crece: si el programa agranda el pool por encima de su memoria, el puente
    /// crea al otro lado un pool nuevo (con un número propio, `alias`) sobre una memoria más grande, y los búferes que
    /// el programa crea después van a ese. Los de antes siguen en el viejo, con su memoria (`viejas`).
    alias: Vec<u32>,
    viejas: Vec<u32>,
    /// Búferes vivos de este pool, y si el programa ya lo destruyó: sus memorias se sueltan cuando no quede ninguno.
    vivos: usize,
    destruido: bool,
}

/// Los números de objeto que pone el puente (los sucesores de los pools), desde arriba: el programa cuenta desde 1.
const PRIMERO_PROPIO: u32 = 0xfeff_ffff;

/// Desde aquí, los números de pool de los búferes de dmabuf (no chocan con objetos del programa).
const POOL_DMABUF: u32 = 0x8000_0000;
/// Y desde aquí, los pools apartados: destruidos por el programa, que siguen por sus búferes, cuando su número vuelve.
const POOL_APARTADO: u32 = 0x4000_0000;

/// Espera a que la tarjeta acabe con un dmabuf antes de leerlo (empezar y acabar una lectura).
fn esperar_tarjeta(fd: RawFd) {
    const DMA_BUF_IOCTL_SYNC: libc::c_ulong = 0x4008_6200;
    for flags in [1u64, 1 | 4] {
        unsafe { libc::ioctl(fd, DMA_BUF_IOCTL_SYNC as _, &flags as *const u64) };
    }
}

struct Programa {
    sock: UnixStream,
    /// Con copia aunque DashOS admita vistas (`FORZAR_COPIA`).
    copia: bool,
    /// Memorias del otro lado que son vistas (sin copia) → las páginas físicas del invitado que se mandaron.
    vistas: HashMap<u32, Vec<u64>>,
    /// Cuadros que se vieron sin copiar nada.
    sin_copia: u64,
    /// Tiempo de CPU del puente (µs) cuando conectó, para medir lo que gasta con él.
    cpu0: u64,
    /// Hay que cerrarlo (sus páginas se movieron).
    cerrar: bool,
    /// Pools apartados hasta ahora (para sus números).
    apartados: u32,
    entrada: Vec<u8>,
    /// Lo que llegó de DashOS para él y aún no es un mensaje entero (para seguir los objetos que crea el compositor).
    eventos: Vec<u8>,
    fds: VecDeque<RawFd>,
    objetos: HashMap<u32, &'static Interface>,
    pools: HashMap<u32, Pool>,
    /// Búfer → (pool, desde, bytes, bytes por fila, memoria del otro lado).
    buferes: HashMap<u32, (u32, usize, usize, usize, u32)>,
    /// El siguiente número de objeto propio (cuenta hacia abajo desde `PRIMERO_PROPIO`).
    propio: u32,
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
        let copia = std::path::Path::new(FORZAR_COPIA).exists();
        Self { sock, copia, vistas: HashMap::new(), sin_copia: 0, cpu0: cpu_us(), cerrar: false, apartados: 0, entrada: Vec::new(), eventos: Vec::new(), fds: VecDeque::new(), objetos, pools: HashMap::new(), buferes: HashMap::new(), superficies: HashMap::new(), planos: HashMap::new(), danos: HashMap::new(), enviado: HashMap::new(), copiado: 0, cuadros: 0, propio: PRIMERO_PROPIO }
    }
}

struct Puente {
    transporte: UnixStream,
    siguiente_memoria: u32,
    /// Los tubos del portapapeles (H9).
    tubos: p::tubos::Tubos,
    /// DashOS admite vistas (dijo `VISTAS`).
    vistas: bool,
    /// `/proc/self/pagemap`, para saber las páginas físicas de un pool.
    pagemap: Option<std::fs::File>,
}

/// Si existe este fichero, los programas que conecten van con copia aunque DashOS admita vistas.
const FORZAR_COPIA: &str = "/run/dashos-puente-copia";
/// Lo más que mide una vista (el tope de una memoria de DashOS).
const MAX_VISTA: usize = 16 << 20;

/// Tiempo de CPU del puente (de usuario y de sistema), en µs.
fn cpu_us() -> u64 {
    let mut r: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut r) };
    let us = |t: libc::timeval| t.tv_sec as u64 * 1_000_000 + t.tv_usec as u64;
    us(r.ru_utime) + us(r.ru_stime)
}

/// Las direcciones físicas de las `n` páginas proyectadas desde `base`, por `/proc/self/pagemap` (64 bits por página:
/// el 63, presente; del 0 al 54, el número de página física, que solo se ve siendo root). `None` si alguna no está o
/// no se ve.
fn leer_pagemap(pagemap: &std::fs::File, base: usize, n: usize) -> Option<Vec<u64>> {
    use std::os::unix::fs::FileExt;
    let mut b = vec![0u8; n * 8];
    pagemap.read_exact_at(&mut b, (base / 4096 * 8) as u64).ok()?;
    b.chunks_exact(8)
        .map(|e| {
            let e = u64::from_le_bytes(e.try_into().unwrap());
            let pfn = e & ((1 << 55) - 1);
            (e >> 63 == 1 && pfn != 0).then_some(pfn << 12)
        })
        .collect()
}
impl Puente {
    fn memoria(&mut self) -> u32 {
        self.siguiente_memoria += 1;
        self.siguiente_memoria
    }

    fn trama(&mut self, tipo: u8, conexion: u32, datos: &[u8]) {
        p::escribir_trama(&mut self.transporte, tipo, conexion, datos).expect("el transporte se cerró");
    }

    /// Manda las tramas que dejaron los tubos.
    fn tramas(&mut self, t: Vec<(u8, Vec<u8>)>) {
        for (tipo, d) in t {
            self.trama(tipo, 0, &d);
        }
    }

    fn memoria_nueva(&mut self, numero: u32, tamano: u64) {
        let mut d = numero.to_le_bytes().to_vec();
        d.extend_from_slice(&tamano.to_le_bytes());
        self.trama(p::MEMORIA_NUEVA, 0, &d);
    }

    fn memoria_vista(&mut self, numero: u32, tamano: u64, paginas: &[u64]) {
        let mut d = numero.to_le_bytes().to_vec();
        d.extend_from_slice(&tamano.to_le_bytes());
        d.extend_from_slice(&(paginas.len() as u32).to_le_bytes());
        for x in paginas {
            d.extend_from_slice(&x.to_le_bytes());
        }
        self.trama(p::MEMORIA_VISTA, 0, &d);
    }

    /// Las páginas físicas del invitado de los `tamano` bytes del pool `fd`, proyectado en `base`, fijadas en memoria
    /// (`mlock`) para que Linux no las saque ni las mueva mientras la proyección viva. `None` si no se puede (DashOS no
    /// admite vistas, sin permiso para verlas, más de `MAX_VISTA`...): entonces se copia.
    fn fisicas(&self, fd: RawFd, base: *mut libc::c_void, tamano: usize) -> Option<Vec<u64>> {
        let pagemap = self.pagemap.as_ref().filter(|_| self.vistas)?;
        if base == libc::MAP_FAILED || tamano == 0 || tamano > MAX_VISTA {
            return None;
        }
        // Páginas de verdad en los huecos del memfd (sin cambiar lo que tiene), y fijadas: así cada página del pool es
        // una página física suya y no se va a otra.
        unsafe { libc::fallocate(fd, libc::FALLOC_FL_KEEP_SIZE, 0, tamano as libc::off_t) };
        if unsafe { libc::mlock(base, tamano) } != 0 {
            return None;
        }
        let n = tamano.div_ceil(4096);
        let paginas = leer_pagemap(pagemap, base as usize, n)?;
        // Dos páginas del pool en la misma física sería la página de ceros compartida, no la del memfd: con copia.
        let mut vistas = paginas.clone();
        vistas.sort_unstable();
        vistas.dedup();
        (vistas.len() == n).then_some(paginas)
    }

    /// Suelta un pool: su proyección, su descriptor y sus memorias del otro lado.
    fn soltar_pool(&mut self, pl: Pool) {
        unsafe {
            if pl.base != libc::MAP_FAILED {
                libc::munmap(pl.base, pl.tamano.max(1));
            }
            libc::close(pl.fd);
        }
        for m in std::iter::once(pl.memoria).chain(pl.viejas) {
            self.trama(p::MEMORIA_SOLTAR, 0, &m.to_le_bytes());
        }
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
    let mut sueltas = Vec::new();
    while let Ok((objeto, orden, tam)) = leer_cabecera(&pr.entrada) {
        let mut msg: Vec<u8> = pr.entrada.drain(..tam).collect();
        // Mensajes que el puente añade detrás de este (destruir los sucesores de un pool), el que va en su lugar (el
        // `create_pool` del sucesor) y otro objeto para él (un búfer que va al sucesor).
        let mut extra: Vec<u8> = Vec::new();
        let mut en_su_lugar: Option<Vec<u8>> = None;
        let mut otro_objeto: Option<u32> = None;
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
                match (!pr.copia).then(|| pu.fisicas(fds[0], base, tamano)).flatten() {
                    Some(paginas) => {
                        pu.memoria_vista(memoria, tamano as u64, &paginas);
                        pr.vistas.insert(memoria, paginas);
                    }
                    None => pu.memoria_nueva(memoria, tamano as u64),
                }
                // Un número de pool que vuelve (Mesa crea y destruye un pool por cuadro, y libwayland repite el número) con el
                // de antes aún vivo por sus búferes: el viejo pasa a un número propio, con sus búferes. Si no, esos búferes
                // se leerían en el pool nuevo (con copia, otro dibujo; sin copia, otras páginas: el cliente se cerraba).
                if let Some(viejo) = pr.pools.remove(&id) {
                    pr.apartados = pr.apartados.wrapping_add(1);
                    let k = POOL_APARTADO | (pr.apartados & 0x0fff_ffff);
                    for b in pr.buferes.values_mut().filter(|b| b.0 == id) {
                        b.0 = k;
                    }
                    pr.pools.insert(k, viejo);
                }
                if base != libc::MAP_FAILED {
                    let capacidad = tamano.max(1).next_multiple_of(4096);
                    pr.pools.insert(id, Pool { base, tamano, fd: fds[0], memoria, dmabuf: false, capacidad, shm: objeto, alias: Vec::new(), viejas: Vec::new(), vivos: 0, destruido: false });
                } else {
                    unsafe { libc::close(fds[0]) };
                }
                suyas.push(memoria);
            }
            ("wl_shm_pool", wl_shm_pool::req::RESIZE) if !args.is_empty() => {
                if let Some(pool) = pr.pools.get_mut(&objeto) {
                    let nuevo = args[0].uint() as usize;
                    // La proyección nueva antes de quitar la vieja: las páginas de una vista no quedan sin fijar ni un
                    // momento.
                    let base = unsafe { libc::mmap(std::ptr::null_mut(), nuevo.max(1), libc::PROT_READ, libc::MAP_SHARED, pool.fd, 0) };
                    if base != libc::MAP_FAILED && pr.vistas.contains_key(&pool.memoria) {
                        unsafe { libc::mlock(base, nuevo.max(1)) };
                    }
                    unsafe { libc::munmap(pool.base, pool.tamano.max(1)) };
                    pool.base = base;
                    pool.tamano = nuevo;
                    if nuevo <= pool.capacidad {
                        let mut d = pool.memoria.to_le_bytes().to_vec();
                        d.extend_from_slice(&(nuevo as u64).to_le_bytes());
                        pu.trama(p::MEMORIA_CRECER, 0, &d);
                    } else {
                        // No cabe: una memoria nueva con todo lo del pool y, en vez del `resize`, un `create_pool` sobre
                        // ella con un número propio. El viejo sigue para los búferes que ya tenía.
                        let memoria = pu.memoria();
                        match (!pr.copia).then(|| pu.fisicas(pool.fd, pool.base, nuevo)).flatten() {
                            Some(paginas) => {
                                pu.memoria_vista(memoria, nuevo as u64, &paginas);
                                pr.vistas.insert(memoria, paginas);
                            }
                            None => {
                                pu.memoria_nueva(memoria, nuevo as u64);
                                if pool.base != libc::MAP_FAILED {
                                    let bytes = unsafe { std::slice::from_raw_parts(pool.base as *const u8, nuevo) };
                                    p::escribir_memoria(&mut pu.transporte, memoria, 0, bytes).expect("el transporte se cerró");
                                    pr.copiado += nuevo as u64;
                                }
                            }
                        }
                        let id = pr.propio;
                        pr.propio -= 1;
                        println!("puente: un pool crece de {} a {} bytes, más que su memoria: sigue en otro ({:#x})", pool.capacidad, nuevo, id);
                        let mut m = Vec::with_capacity(16);
                        for w in [pool.shm, 16 << 16 | wl_shm::req::CREATE_POOL as u32, id, nuevo as u32] {
                            m.extend_from_slice(&w.to_le_bytes());
                        }
                        en_su_lugar = Some(m);
                        suyas.push(memoria);
                        pool.viejas.push(std::mem::replace(&mut pool.memoria, memoria));
                        pool.alias.push(id);
                        pool.capacidad = nuevo.next_multiple_of(4096);
                    }
                }
            }
            ("wl_shm_pool", wl_shm_pool::req::DESTROY) => {
                if let Some(pool) = pr.pools.get_mut(&objeto) {
                    for &a in &pool.alias {
                        for w in [a, 8 << 16 | wl_shm_pool::req::DESTROY as u32] {
                            extra.extend_from_slice(&w.to_le_bytes());
                        }
                    }
                    pool.destruido = true;
                    if pool.vivos == 0 {
                        let pl = pr.pools.remove(&objeto).unwrap();
                        pu.soltar_pool(pl);
                    }
                }
            }
            ("wl_shm_pool", wl_shm_pool::req::CREATE_BUFFER) if args.len() == 6 => {
                let (alto, linea) = (args[3].uint() as usize, args[4].uint() as usize);
                if let Some(pool) = pr.pools.get_mut(&objeto) {
                    pool.vivos += 1;
                    pr.buferes.insert(args[0].uint(), (objeto, args[1].uint() as usize, alto.saturating_mul(linea), linea, pool.memoria));
                    // Con sucesor, el búfer se crea en él (el último: el de la memoria grande).
                    otro_objeto = pool.alias.last().copied();
                }
                pr.enviado.remove(&args[0].uint());
            }
            ("zwp_linux_buffer_params_v1", zwp_linux_buffer_params_v1::req::ADD) if fds.len() == 1 && args.len() == 6 => {
                // El plano entero se proyecta y va a una memoria del otro lado de su tamaño.
                let tamano = unsafe { libc::lseek(fds[0], 0, libc::SEEK_END) }.max(0) as usize;
                let base = unsafe { libc::mmap(std::ptr::null_mut(), tamano.max(1), libc::PROT_READ, libc::MAP_SHARED, fds[0], 0) };
                let memoria = pu.memoria();
                pu.memoria_nueva(memoria, tamano as u64);
                if base != libc::MAP_FAILED && args[1].uint() == 0 {
                    pr.pools.insert(POOL_DMABUF | memoria, Pool { base, tamano, fd: fds[0], memoria, dmabuf: true, capacidad: tamano, shm: 0, alias: Vec::new(), viejas: Vec::new(), vivos: 0, destruido: false });
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
                    let memoria = pr.pools.get(&pool).map_or(0, |pl| pl.memoria);
                    pr.buferes.insert(args[0].uint(), (pool, desde, alto.saturating_mul(linea), linea, memoria));
                    pr.enviado.remove(&args[0].uint());
                }
            }
            ("wl_buffer", wl_buffer::req::DESTROY) => {
                // Un búfer de dmabuf se lleva su memoria; uno de wl_shm, la de su pool si era el último de un pool ya
                // destruido.
                if let Some((pool, ..)) = pr.buferes.remove(&objeto) {
                    let soltar = match pr.pools.get_mut(&pool) {
                        Some(pl) if pl.dmabuf => true,
                        Some(pl) => {
                            pl.vivos = pl.vivos.saturating_sub(1);
                            pl.destruido && pl.vivos == 0
                        }
                        None => false,
                    };
                    if soltar {
                        let pl = pr.pools.remove(&pool).unwrap();
                        pu.soltar_pool(pl);
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
                    if let Some(&(pool, desde, largo, linea, memoria)) = pr.buferes.get(&b) {
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
                        if let (Some(pl), Some(paginas)) = (pr.pools.get(&pool), pr.vistas.get(&memoria)) {
                            // Sin copia: el compositor lo lee en la RAM del invitado. Solo se comprueba que el búfer
                            // sigue en las páginas que se mandaron.
                            let (a, z) = (desde / 4096, (desde + largo).div_ceil(4096).min(paginas.len()));
                            let iguales = a < z
                                && pl.base != libc::MAP_FAILED
                                && pu.pagemap.as_ref().and_then(|f| leer_pagemap(f, pl.base as usize + a * 4096, z - a)).is_some_and(|v| v[..] == paginas[a..z]);
                            if iguales {
                                pr.sin_copia += 1;
                                pr.cuadros += 1;
                                if pr.sin_copia == 1 {
                                    println!("puente: cliente {}: primer cuadro sin copia", conexion);
                                }
                            } else {
                                println!("puente: cliente {}: Linux movió las páginas de un búfer sin copia: se cierra", conexion);
                                pr.cerrar = true;
                            }
                        } else if let Some(pl) = pr.pools.get(&pool) {
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
                                    p::escribir_memoria(&mut pu.transporte, memoria, ini as u64, bytes).expect("el transporte se cerró");
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
                    // H9: una tubería (para pegar: `receive`) es un tubo; lo demás se copia una vez.
                    if p::tubos::es_tuberia(fd) {
                        let n = pu.memoria();
                        let mut t = Vec::new();
                        pu.tubos.escritor(n, fd, &mut t);
                        pu.tramas(t);
                        suyas.push(n);
                        continue;
                    }
                    let n = pu.copiar_descriptor(fd);
                    sueltas.push(n);
                    suyas.push(n);
                }
            }
        }
        drop(args);
        if let Some(m) = en_su_lugar {
            msg = m;
        }
        if let Some(a) = otro_objeto {
            msg[0..4].copy_from_slice(&a.to_le_bytes());
        }
        salida.extend_from_slice(&msg);
        salida.extend_from_slice(&extra);
        llaves.extend(suyas);
    }
    if !salida.is_empty() {
        pu.trama(p::DATOS, conexion, &p::juntar_datos(&llaves, &salida));
    }
    // Las copias de un descriptor cualquiera: el otro lado ya tiene la suya, con el mensaje.
    for n in sueltas {
        pu.trama(p::MEMORIA_SOLTAR, 0, &n.to_le_bytes());
    }
}

/// Sigue los eventos que manda DashOS a un programa: los objetos que crea el compositor (las ofertas del portapapeles)
/// pasan a la tabla del programa, para saber qué descriptores llevan sus peticiones.
fn seguir_eventos(pr: &mut Programa, bytes: &[u8]) {
    pr.eventos.extend_from_slice(bytes);
    let mut hecho = 0;
    while let Ok((objeto, orden, tam)) = leer_cabecera(&pr.eventos[hecho..]) {
        let msg = &pr.eventos[hecho + 8..hecho + tam];
        if let Some(m) = pr.objetos.get(&objeto).and_then(|i| i.events.get(orden as usize)) {
            if let Ok(args) = leer_args(m, msg, &mut std::iter::repeat(0)) {
                for (a, v) in m.args.iter().zip(args.iter()) {
                    if let (Valor::NewId(id), Some(nombre)) = (v, a.interface)
                        && let Some(i) = wayland::interfaz(nombre)
                    {
                        pr.objetos.insert(*id, i);
                    }
                }
            }
        }
        hecho += tam;
    }
    pr.eventos.drain(..hecho);
    // Un evento que no cuadra (más de lo que puede medir uno): se deja de seguir.
    if pr.eventos.len() > 1 << 16 {
        pr.eventos.clear();
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
    // Vistas (sin copia): páginas fuera de la RAM del invitado, sin alinear, listas que no casan con el tamaño o enormes,
    // tamaños absurdos, más de las que caben, escribir en una vista y un cliente que la usa. Y una buena: una página
    // propia, fijada, con su dirección física de verdad (si se ve).
    let vista = |n: u32, tam: u64, k: u32, dirs: &[u64]| {
        let mut d = n.to_le_bytes().to_vec();
        d.extend_from_slice(&tam.to_le_bytes());
        d.extend_from_slice(&k.to_le_bytes());
        for x in dirs {
            d.extend_from_slice(&x.to_le_bytes());
        }
        wayland::puente::trama(p::MEMORIA_VISTA, 0, &d)
    };
    let propia = unsafe { libc::mmap(std::ptr::null_mut(), 4096, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_PRIVATE | libc::MAP_ANONYMOUS, -1, 0) };
    let fisica = (propia != libc::MAP_FAILED && unsafe { libc::mlock(propia, 4096) } == 0)
        .then(|| std::fs::File::open("/proc/self/pagemap").ok().and_then(|f| leer_pagemap(&f, propia as usize, 1)))
        .flatten()
        .map(|v| v[0]);
    let mut t = vista(1, 4096, 1, &[0]);
    t.extend(vista(2, 4096, 1, &[u64::MAX & !0xfff]));
    t.extend(vista(3, 4096, 1, &[0x4000_0001]));
    t.extend(vista(4, 4096, 1, &[0x4000_0000 + (1 << 40)]));
    t.extend(vista(5, 8192, 1, &[0x4000_0000]));
    t.extend(vista(6, 4096, u32::MAX, &[0x4000_0000]));
    t.extend(vista(7, 1 << 40, 1, &[0x4000_0000]));
    t.extend(vista(8, 0, 0, &[]));
    for i in 0..300 {
        t.extend(vista(100 + i, 4096, 1, &[fisica.unwrap_or(0x4000_0000)]));
    }
    if let Some(f) = fisica {
        t.extend(vista(9, 4096, 1, &[f]));
        t.extend(mem(p::MEMORIA_ESCRIBIR, 9, 0, b"en una vista no se escribe"));
        t.extend(wayland::puente::trama(p::CONECTAR, 1, &[]));
        t.extend(wayland::puente::trama(p::DATOS, 1, &p::juntar_datos(&[9, 9], &[1, 0, 0, 0, 1, 0, 12, 0])));
    }
    tandas.push(("vistas", t));
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
        let mut recibido = Vec::new();
        loop {
            match std::io::Read::read(&mut s, &mut b) {
                Ok(0) => {
                    cortada = true;
                    break;
                }
                Ok(n) => recibido.extend_from_slice(&b[..n]),
                Err(_) => break,
            }
        }
        cortadas += cortada as u32;
        let tramas = p::sacar_tramas(&mut recibido).unwrap_or_default();
        let vistas_no = tramas.iter().filter(|t| t.tipo == p::VISTA_NO).count();
        println!(
            "puente hostil: {}: {} bytes, {}{}",
            nombre,
            t.len(),
            if cortada { "DashOS cortó" } else { "sigue abierta" },
            if nombre == "vistas" { format!(" ({} VISTA_NO; la vista de una página propia {})", vistas_no, if fisica.is_some() { "pedida" } else { "no se pudo pedir" }) } else { String::new() }
        );
    }
    println!("puente hostil: hecho, {} conexiones cortadas por DashOS", cortadas);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--cliente-hostil") {
        let plazo = args.get(4).and_then(|x| x.parse().ok()).unwrap_or(60);
        cliente_hostil::principal(args.get(2).map_or("/run/wayland-0", String::as_str), args.get(3).map_or("roto", String::as_str), plazo);
        return;
    }
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
    // Sin compactación de páginas fijadas (si el kernel la tiene): las de un pool sin copia no se mueven.
    let _ = std::fs::write("/proc/sys/vm/compact_unevictable_allowed", "0");
    let pagemap = std::fs::File::open("/proc/self/pagemap").ok();
    let mut pu = Puente { transporte: t, siguiente_memoria: 0, vistas: false, pagemap, tubos: p::tubos::Tubos::default() };
    let mut programas: HashMap<u32, Programa> = HashMap::new();
    let mut siguiente = 0u32;
    // Las memorias que manda DashOS: número → memfd.
    let mut de_dashos: HashMap<u32, RawFd> = HashMap::new();
    let mut entrada = Vec::new();
    loop {
        let mut pfd = vec![libc::pollfd { fd: escucha.as_raw_fd(), events: libc::POLLIN, revents: 0 }, libc::pollfd { fd: lectura.as_raw_fd(), events: libc::POLLIN, revents: 0 }];
        let orden: Vec<u32> = programas.keys().copied().collect();
        pfd.extend(orden.iter().map(|c| libc::pollfd { fd: programas[c].sock.as_raw_fd(), events: libc::POLLIN, revents: 0 }));
        // H9: los tubos del portapapeles.
        pfd.extend(pu.tubos.vigilar().into_iter().map(|(fd, ev, _)| libc::pollfd { fd, events: ev, revents: 0 }));
        unsafe { libc::poll(pfd.as_mut_ptr(), pfd.len() as _, 100) };
        let mut t = Vec::new();
        pu.tubos.mover(&mut t);
        pu.tramas(t);
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
            if pr.cerrar {
                fuera.push(c);
            }
        }
        for c in fuera {
            if let Some(mut pr) = programas.remove(&c) {
                if pr.cuadros > 0 {
                    let cpu = cpu_us() - pr.cpu0;
                    println!(
                        "puente: cliente {}: {} cuadros ({} sin copia), {} bytes copiados, {} por cuadro; CPU del puente {} ms, {} µs por cuadro",
                        c, pr.cuadros, pr.sin_copia, pr.copiado, pr.copiado / pr.cuadros, cpu / 1000, cpu / pr.cuadros
                    );
                }
                // Sus memorias, fuera también al otro lado (si no, el tope de memoria de DashOS se llenaría).
                for (_, pl) in pr.pools.drain() {
                    pu.soltar_pool(pl);
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
                p::TUBO_NUEVO if t.datos.len() == 4 => pu.tubos.anunciado(u32::from_le_bytes(t.datos[..4].try_into().unwrap())),
                p::TUBO_LEIDO if t.datos.len() == 8 => {
                    pu.tubos.leido(u32::from_le_bytes(t.datos[..4].try_into().unwrap()), u32::from_le_bytes(t.datos[4..8].try_into().unwrap()))
                }
                p::TUBO_DATOS | p::TUBO_FIN if t.datos.len() >= 4 => {
                    let n = u32::from_le_bytes(t.datos[..4].try_into().unwrap());
                    let mut v = Vec::new();
                    if t.tipo == p::TUBO_DATOS {
                        pu.tubos.datos(n, &t.datos[4..], &mut v);
                    } else {
                        pu.tubos.fin(n, &mut v);
                    }
                    pu.tramas(v);
                }
                p::DATOS => {
                    let Some((llaves, bytes)) = p::partir_datos(&t.datos) else { continue };
                    // Cada número, una memoria (un memfd) o un tubo anunciado (el extremo de escribir de su tubería).
                    let fds: Vec<RawFd> = llaves.iter().filter_map(|n| de_dashos.remove(n).or_else(|| pu.tubos.dar(*n))).collect();
                    if let Some(pr) = programas.get_mut(&t.conexion) {
                        p::mandar(pr.sock.as_raw_fd(), bytes, &fds);
                        seguir_eventos(pr, bytes);
                    }
                    for fd in fds {
                        unsafe { libc::close(fd) };
                    }
                }
                p::ADIOS => {
                    programas.remove(&t.conexion);
                }
                p::VISTAS => {
                    pu.vistas = pu.pagemap.is_some();
                    println!("puente: DashOS lee los búferes sin copia{}", if pu.vistas { "" } else { " (pero aquí no se ve /proc/self/pagemap: con copia)" });
                }
                p::VISTA_NO if t.datos.len() == 4 => {
                    // Esa memoria es normal: los búferes que la usan se copian enteros en su próximo commit.
                    let n = u32::from_le_bytes(t.datos[0..4].try_into().unwrap());
                    for pr in programas.values_mut() {
                        if pr.vistas.remove(&n).is_some() {
                            let suyos: Vec<u32> = pr.buferes.iter().filter(|(_, b)| b.4 == n).map(|(&b, _)| b).collect();
                            for b in suyos {
                                pr.enviado.remove(&b);
                            }
                            println!("puente: DashOS no hizo la vista de la memoria {}: con copia", n);
                        }
                    }
                }
                _ => {}
            }
        }
        let _ = pu.transporte.flush();
    }
}
