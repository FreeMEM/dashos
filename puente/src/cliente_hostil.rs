//! Un programa de Linux hostil (H9, rama `caja-linux-apps`): `puente --cliente-hostil SOCKET MODO`. Habla Wayland a
//! mano (sin libwayland, que no deja mandar cosas rotas) con el compositor que haya en `SOCKET` (en la caja, el puente:
//! así lo roto cruza el puente y llega al compositor de DashOS). Los modos:
//!
//! - `roto`: decoraciones con un modo que no existe, dos para la misma ventana, y un tipo del portapapeles que no es
//!   UTF-8. Cada uno en su conexión; el compositor la corta (error de protocolo) y sigue.
//! - `sin-fin`: una ventana; con el teclado, una selección con 200 tipos (enormes, repetidos) que, cuando alguien pega,
//!   escribe sin parar (hasta que le cierran la tubería o 32 MB).
//! - `nunca`: lo mismo, pero no escribe ni cierra nunca la tubería.
//!
//! Dice por la salida lo que pasa (para el guion) y acaba solo a los `plazo` segundos.

use std::io::Write;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use dashos_puente as p;
use wayland::tablas::{
    wl_compositor, wl_data_device, wl_data_device_manager, wl_data_source, wl_display, wl_keyboard, wl_registry, wl_seat, wl_shm,
    wl_shm_pool, wl_surface, xdg_surface, xdg_toplevel, xdg_wm_base, zxdg_decoration_manager_v1, zxdg_toplevel_decoration_v1,
};
use wayland::{Escritor, leer_cabecera};

struct Con {
    s: UnixStream,
    entrada: Vec<u8>,
    fds: Vec<RawFd>,
    globales: Vec<(u32, String)>,
}

impl Con {
    fn nueva(ruta: &str) -> Option<Con> {
        let s = UnixStream::connect(ruta).ok()?;
        s.set_nonblocking(true).ok()?;
        Some(Con { s, entrada: Vec::new(), fds: Vec::new(), globales: Vec::new() })
    }

    fn mandar(&mut self, e: &Escritor) -> bool {
        let fds: Vec<RawFd> = e.llaves.iter().map(|&l| l as RawFd).collect();
        p::mandar(self.s.as_raw_fd(), &e.bytes, &fds)
    }

    /// Los eventos que lleguen en `ms` milisegundos: (objeto, orden, cuerpo). `None` si se cortó.
    fn eventos(&mut self, ms: u64) -> Option<Vec<(u32, u16, Vec<u8>)>> {
        let hasta = Instant::now() + Duration::from_millis(ms);
        let mut out = Vec::new();
        let mut cortada = false;
        while Instant::now() < hasta {
            let mut pfd = libc::pollfd { fd: self.s.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            unsafe { libc::poll(&mut pfd, 1, 20) };
            if p::recibir(self.s.as_raw_fd(), &mut self.entrada, &mut self.fds).is_none() {
                cortada = true;
                break;
            }
            while let Ok((o, orden, tam)) = leer_cabecera(&self.entrada) {
                out.push((o, orden, self.entrada[8..tam].to_vec()));
                self.entrada.drain(..tam);
            }
            if !out.is_empty() && ms == 0 {
                break;
            }
        }
        // Los globales, al vuelo.
        for (o, orden, c) in &out {
            if *o == 2 && *orden == wl_registry::ev::GLOBAL {
                let n = u32::from_le_bytes(c[0..4].try_into().unwrap());
                let largo = u32::from_le_bytes(c[4..8].try_into().unwrap()) as usize;
                let nombre = String::from_utf8_lossy(&c[8..8 + largo - 1]).into_owned();
                self.globales.push((n, nombre));
            }
        }
        if cortada && out.is_empty() { None } else { Some(out) }
    }

    fn global(&self, n: &str) -> u32 {
        self.globales.iter().find(|g| g.1 == n).map_or(0, |g| g.0)
    }

    /// Registro (2), compositor (3), xdg_wm_base (4), decoraciones (5), asiento (6), portapapeles (7), shm (8).
    fn empezar(&mut self) {
        let mut e = Escritor::new();
        e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
        e.empezar(1, wl_display::req::SYNC).uint(99).fin();
        self.mandar(&e);
        self.eventos(1000);
        let mut e = Escritor::new();
        for (nombre, v, id) in [
            ("wl_compositor", 4, 3),
            ("xdg_wm_base", 1, 4),
            ("zxdg_decoration_manager_v1", 1, 5),
            ("wl_seat", 5, 6),
            ("wl_data_device_manager", 3, 7),
            ("wl_shm", 1, 8),
        ] {
            let g = self.global(nombre);
            if g != 0 {
                e.empezar(2, wl_registry::req::BIND).uint(g).texto(Some(nombre)).uint(v).uint(id).fin();
            }
        }
        self.mandar(&e);
    }

    /// Una ventana (superficie 10, xdg_surface 11, toplevel 12), con decoración 13 si `deco`.
    fn ventana(&mut self, e: &mut Escritor, deco: bool) {
        e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(10).fin();
        e.empezar(4, xdg_wm_base::req::GET_XDG_SURFACE).uint(11).uint(10).fin();
        e.empezar(11, xdg_surface::req::GET_TOPLEVEL).uint(12).fin();
        e.empezar(12, xdg_toplevel::req::SET_TITLE).texto(Some("Programa hostil")).fin();
        if deco {
            e.empezar(5, zxdg_decoration_manager_v1::req::GET_TOPLEVEL_DECORATION).uint(13).uint(12).fin();
        }
    }

    /// Si el compositor cortó (un `wl_display.error`, o la conexión cerrada), en `ms`.
    fn cortada(&mut self, ms: u64) -> Option<String> {
        let hasta = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < hasta {
            match self.eventos(100) {
                None => return Some("conexión cerrada".into()),
                Some(ev) => {
                    if let Some((_, _, c)) = ev.iter().find(|x| x.0 == 1 && x.1 == wl_display::ev::ERROR) {
                        let largo = u32::from_le_bytes(c[8..12].try_into().unwrap()) as usize;
                        return Some(String::from_utf8_lossy(&c[12..12 + largo.saturating_sub(1)]).into_owned());
                    }
                }
            }
        }
        None
    }
}

fn decir(t: &str) {
    println!("cliente hostil: {}", t);
    let _ = std::io::stdout().flush();
}

/// `roto`: cada cosa rota en su conexión; el compositor tiene que cortarla.
fn roto(ruta: &str) {
    let casos: [(&str, &dyn Fn(&mut Escritor)); 3] = [
        ("modo de decoración que no existe", &|e: &mut Escritor| {
            e.empezar(13, zxdg_toplevel_decoration_v1::req::SET_MODE).uint(99).fin();
        }),
        ("dos decoraciones para una ventana", &|e: &mut Escritor| {
            e.empezar(5, zxdg_decoration_manager_v1::req::GET_TOPLEVEL_DECORATION).uint(14).uint(12).fin();
        }),
        ("tipo del portapapeles que no es UTF-8", &|e: &mut Escritor| {
            e.empezar(7, wl_data_device_manager::req::CREATE_DATA_SOURCE).uint(20).fin();
            // Un texto con bytes que no son UTF-8, a mano.
            e.empezar(20, wl_data_source::req::OFFER).uint(5).uint(u32::from_le_bytes([0xff, 0xfe, b'x', b'y'])).uint(0).fin();
        }),
    ];
    for (nombre, caso) in casos {
        let Some(mut c) = Con::nueva(ruta) else {
            decir("no conecta");
            return;
        };
        c.empezar();
        let mut e = Escritor::new();
        c.ventana(&mut e, true);
        caso(&mut e);
        c.mandar(&e);
        match c.cortada(5000) {
            Some(por) => decir(&format!("{}: el compositor lo cortó ({})", nombre, por)),
            None => decir(&format!("{}: el compositor NO lo cortó", nombre)),
        }
    }
    decir("roto: hecho");
}

/// `sin-fin` y `nunca`: una ventana, el teclado, una selección con 200 tipos; y lo que hace al pegar.
fn fuente(ruta: &str, sin_fin: bool, plazo: u64) {
    let modo = if sin_fin { "sin-fin" } else { "nunca" };
    let Some(mut c) = Con::nueva(ruta) else {
        decir("no conecta");
        return;
    };
    c.empezar();
    let (w, h) = (160, 60);
    let tam = (w * h * 4) as u64;
    let Some(fd) = p::memfd(c"hostil", tam) else { return };
    let base = unsafe { libc::mmap(std::ptr::null_mut(), tam as usize, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0) };
    if base != libc::MAP_FAILED {
        let px = unsafe { std::slice::from_raw_parts_mut(base as *mut u32, (w * h) as usize) };
        px.fill(0xff80_2020);
    }
    let mut e = Escritor::new();
    e.empezar(6, wl_seat::req::GET_KEYBOARD).uint(30).fin();
    e.empezar(7, wl_data_device_manager::req::GET_DATA_DEVICE).uint(31).uint(6).fin();
    e.empezar(8, wl_shm::req::CREATE_POOL).uint(32).llave(fd as u32).int(tam as i32).fin();
    e.empezar(32, wl_shm_pool::req::CREATE_BUFFER).uint(33).int(0).int(w).int(h).int(w * 4).uint(1).fin();
    c.ventana(&mut e, false);
    e.empezar(10, wl_surface::req::COMMIT).fin();
    c.mandar(&e);
    unsafe { libc::close(fd) };
    let hasta = Instant::now() + Duration::from_secs(plazo);
    let mut serie_teclado = None;
    let mut mapeada = false;
    let mut tuberias: Vec<RawFd> = Vec::new();
    let mut escritos = 0u64;
    let trozo = vec![b'x'; 64 * 1024];
    while Instant::now() < hasta {
        let Some(ev) = c.eventos(50) else {
            decir(&format!("{}: el compositor cortó la conexión", modo));
            return;
        };
        for (o, orden, cuerpo) in ev {
            let palabra = |i: usize| u32::from_le_bytes(cuerpo[4 * i..4 * i + 4].try_into().unwrap());
            match (o, orden) {
                (11, xdg_surface::ev::CONFIGURE) if !mapeada => {
                    let mut e = Escritor::new();
                    e.empezar(11, xdg_surface::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                    e.empezar(10, wl_surface::req::ATTACH).uint(33).int(0).int(0).fin();
                    e.empezar(10, wl_surface::req::COMMIT).fin();
                    c.mandar(&e);
                    mapeada = true;
                }
                (4, xdg_wm_base::ev::PING) => {
                    let mut e = Escritor::new();
                    e.empezar(4, xdg_wm_base::req::PONG).uint(palabra(0)).fin();
                    c.mandar(&e);
                }
                (30, wl_keyboard::ev::KEYMAP) => {
                    // Su descriptor, fuera.
                    if !c.fds.is_empty() {
                        unsafe { libc::close(c.fds.remove(0)) };
                    }
                }
                (30, wl_keyboard::ev::ENTER) if serie_teclado.is_none() => {
                    let serie = palabra(0);
                    serie_teclado = Some(serie);
                    // 200 tipos: largos (4000 bytes), repetidos y los de texto.
                    let mut e = Escritor::new();
                    e.empezar(7, wl_data_device_manager::req::CREATE_DATA_SOURCE).uint(40).fin();
                    let largo = "y".repeat(4000);
                    for i in 0..200 {
                        // Por tandas: lo de un programa que no cabe en lo que el compositor junta de una vez (64 KB)
                        // lo corta, y aquí se quiere que la selección llegue.
                        if i % 10 == 9 {
                            c.mandar(&e);
                            e = Escritor::new();
                            std::thread::sleep(Duration::from_millis(20));
                        }
                        let t = match i % 4 {
                            0 => "text/plain;charset=utf-8".to_string(),
                            1 => largo.clone(),
                            2 => format!("tipo/hostil-{}", i),
                            _ => "UTF8_STRING".to_string(),
                        };
                        e.empezar(40, wl_data_source::req::OFFER).texto(Some(&t)).fin();
                    }
                    e.empezar(31, wl_data_device::req::SET_SELECTION).uint(40).uint(serie).fin();
                    c.mandar(&e);
                    decir(&format!("{}: selección puesta, con 200 tipos", modo));
                }
                (40, wl_data_source::ev::SEND) => {
                    if !c.fds.is_empty() {
                        let t = c.fds.remove(0);
                        unsafe {
                            let f = libc::fcntl(t, libc::F_GETFL);
                            libc::fcntl(t, libc::F_SETFL, f | libc::O_NONBLOCK);
                        }
                        decir(&format!("{}: alguien pega", modo));
                        tuberias.push(t);
                    }
                }
                (40, wl_data_source::ev::CANCELLED) => decir(&format!("{}: ya no es la selección", modo)),
                (1, wl_display::ev::ERROR) => {
                    decir(&format!("{}: error de protocolo", modo));
                    return;
                }
                _ => {}
            }
        }
        // Lo de los que pegan: sin fin, se escribe hasta que cortan; si no, nada (y no se cierra).
        if sin_fin {
            tuberias.retain(|&t| {
                for _ in 0..16 {
                    let k = unsafe { libc::write(t, trozo.as_ptr().cast(), trozo.len()) };
                    if k > 0 {
                        escritos += k as u64;
                        if escritos > 32 << 20 {
                            decir(&format!("sin-fin: {} bytes escritos sin que corten", escritos));
                            unsafe { libc::close(t) };
                            return false;
                        }
                        continue;
                    }
                    if k < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock {
                        return true;
                    }
                    decir(&format!("sin-fin: le cortaron la tubería tras {} bytes", escritos));
                    unsafe { libc::close(t) };
                    return false;
                }
                true
            });
        }
    }
    decir(&format!("{}: hecho", modo));
}

pub fn principal(ruta: &str, modo: &str, plazo: u64) {
    match modo {
        "roto" => roto(ruta),
        "sin-fin" => fuente(ruta, true, plazo),
        "nunca" => fuente(ruta, false, plazo),
        _ => decir("modo que no existe (roto, sin-fin, nunca)"),
    }
}
