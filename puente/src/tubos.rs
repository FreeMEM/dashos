//! Los tubos del portapapeles (H9) del lado de Linux: tuberías de verdad (`pipe`). Los usa el puente de la caja (pegar y
//! copiar desde programas de Linux) y el compositor del PC (que hace de DashOS en las pruebas). El formato de las tramas
//! está en `wayland::puente` (`TUBO_NUEVO`, `TUBO_DATOS`, `TUBO_FIN`).
//!
//! - Un descriptor que es una tubería y llega con un mensaje (en el puente, el de `wl_data_offer.receive`: el programa
//!   leerá del otro extremo) es un tubo donde **escribir**: se anuncia con `TUBO_NUEVO` y su número va en el mensaje. Lo
//!   que llega en `TUBO_DATOS` se escribe en él (sin esperar: lo que no cabe espera, con tope) y `TUBO_FIN` lo cierra.
//! - Un número anunciado por el otro lado (`TUBO_NUEVO`) que llega en un mensaje: se crea una tubería, su extremo de
//!   escribir va al programa con el mensaje y del de leer sale lo que escriba, en `TUBO_DATOS`, hasta el fin.
//!
//! Hostil: tubos a la vez, bytes por tubo y lo que espera, con tope; uno que no se mueve en `PLAZO`, fuera. Un programa
//! que escribe sin fin o que no cierra no detiene nada más.

use std::collections::{HashMap, HashSet};
use std::os::fd::RawFd;
use std::time::{Duration, Instant};

use wayland::puente::{MAX_TUBO, MAX_TUBOS, TUBO_DATOS, TUBO_FIN, TUBO_NUEVO};

/// Lo que espera a escribirse en un tubo: más, y se corta.
pub const MAX_PENDIENTE: usize = 1 << 20;
/// Un tubo sin moverse este tiempo se corta.
pub const PLAZO: Duration = Duration::from_secs(30);

struct Tubo {
    fd: RawFd,
    /// Este lado escribe en `fd` lo que llega (si no, lee de `fd` y lo manda).
    escribe: bool,
    pendiente: Vec<u8>,
    fin: bool,
    bytes: u64,
    ultimo: Instant,
}

/// Los tubos de un transporte.
#[derive(Default)]
pub struct Tubos {
    tubos: HashMap<u32, Tubo>,
    /// Anunciados por el otro lado, que aún no llegaron en un mensaje (y los que nacieron acabados).
    anunciados: HashSet<u32>,
    muertos: HashSet<u32>,
    /// Cuentas: tubos y bytes.
    pub cuantos: u64,
    pub bytes: u64,
    pub malos: u64,
}

/// Si el descriptor es una tubería (o un socket): lo que se da para pegar.
pub fn es_tuberia(fd: RawFd) -> bool {
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    let r = unsafe { libc::fstat(fd, &mut st) };
    r == 0 && matches!(st.st_mode & libc::S_IFMT, libc::S_IFIFO | libc::S_IFSOCK)
}

fn sin_esperar(fd: RawFd) {
    unsafe {
        let f = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, f | libc::O_NONBLOCK);
    }
}

impl Tubos {
    /// Un descriptor donde hay que escribir lo que llegue con el número `n` (ya elegido por quien llama): se anuncia.
    /// Si ya hay demasiados, el descriptor se cierra y el tubo nace acabado (el número va igual en el mensaje).
    pub fn escritor(&mut self, n: u32, fd: RawFd, tramas: &mut Vec<(u8, Vec<u8>)>) {
        tramas.push((TUBO_NUEVO, n.to_le_bytes().to_vec()));
        if self.tubos.len() >= MAX_TUBOS {
            unsafe { libc::close(fd) };
            self.malos += 1;
            tramas.push((TUBO_FIN, n.to_le_bytes().to_vec()));
            return;
        }
        sin_esperar(fd);
        self.cuantos += 1;
        self.tubos.insert(n, Tubo { fd, escribe: true, pendiente: Vec::new(), fin: false, bytes: 0, ultimo: Instant::now() });
    }

    /// El otro lado anuncia un tubo.
    pub fn anunciado(&mut self, n: u32) {
        if self.anunciados.len() < 4 * MAX_TUBOS {
            self.anunciados.insert(n);
        } else {
            self.malos += 1;
        }
    }

    /// Si `n` es un tubo anunciado (para repartir los números de un mensaje).
    pub fn es_anunciado(&self, n: u32) -> bool {
        self.anunciados.contains(&n)
    }

    /// Un número anunciado llega en un mensaje: el extremo de escribir para el programa (quien llama lo cierra tras
    /// mandarlo). El de leer se queda aquí.
    pub fn dar(&mut self, n: u32) -> Option<RawFd> {
        if !self.anunciados.remove(&n) {
            return None;
        }
        let mut p = [0 as RawFd; 2];
        if unsafe { libc::pipe2(p.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
            return None;
        }
        if self.muertos.remove(&n) || self.tubos.len() >= MAX_TUBOS {
            unsafe { libc::close(p[0]) };
        } else {
            sin_esperar(p[0]);
            self.cuantos += 1;
            self.tubos.insert(n, Tubo { fd: p[0], escribe: false, pendiente: Vec::new(), fin: false, bytes: 0, ultimo: Instant::now() });
        }
        Some(p[1])
    }

    /// Bytes del otro lado para el tubo `n`.
    pub fn datos(&mut self, n: u32, d: &[u8], tramas: &mut Vec<(u8, Vec<u8>)>) {
        let Some(t) = self.tubos.get_mut(&n).filter(|t| t.escribe && !t.fin) else {
            self.malos += 1;
            return;
        };
        t.bytes += d.len() as u64;
        t.ultimo = Instant::now();
        self.bytes += d.len() as u64;
        if t.bytes > MAX_TUBO || t.pendiente.len() + d.len() > MAX_PENDIENTE {
            self.malos += 1;
            self.cortar(n, Some(tramas));
            return;
        }
        t.pendiente.extend_from_slice(d);
        self.escribir(n, tramas);
    }

    /// El otro lado dice fin del tubo `n`.
    pub fn fin(&mut self, n: u32, tramas: &mut Vec<(u8, Vec<u8>)>) {
        if self.anunciados.contains(&n) {
            self.muertos.insert(n);
            return;
        }
        match self.tubos.get_mut(&n) {
            Some(t) if t.escribe => {
                t.fin = true;
                self.escribir(n, tramas);
            }
            Some(_) => self.cortar(n, None),
            None => {}
        }
    }

    fn escribir(&mut self, n: u32, tramas: &mut Vec<(u8, Vec<u8>)>) {
        let Some(t) = self.tubos.get_mut(&n) else { return };
        while !t.pendiente.is_empty() {
            let k = unsafe { libc::write(t.fd, t.pendiente.as_ptr().cast(), t.pendiente.len()) };
            if k > 0 {
                t.pendiente.drain(..k as usize);
                continue;
            }
            if k < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock {
                return;
            }
            // Quien leía se fue (EPIPE): que el otro lado deje de mandar.
            self.cortar(n, Some(tramas));
            return;
        }
        if t.fin {
            let t = self.tubos.remove(&n).unwrap();
            unsafe { libc::close(t.fd) };
        }
    }

    /// Cierra un tubo de este lado; con `tramas`, se avisa al otro.
    fn cortar(&mut self, n: u32, tramas: Option<&mut Vec<(u8, Vec<u8>)>>) {
        if let Some(t) = self.tubos.remove(&n) {
            unsafe { libc::close(t.fd) };
            if let Some(tr) = tramas {
                tr.push((TUBO_FIN, n.to_le_bytes().to_vec()));
            }
        }
    }

    /// Lo que hay que vigilar con `poll`: (descriptor, eventos, número).
    pub fn vigilar(&self) -> Vec<(RawFd, i16, u32)> {
        self.tubos
            .iter()
            .filter(|(_, t)| !t.escribe || !t.pendiente.is_empty())
            .map(|(&n, t)| (t.fd, if t.escribe { libc::POLLOUT } else { libc::POLLIN }, n))
            .collect()
    }

    /// Tras `poll`: lee lo que haya (a `tramas`, como `TUBO_DATOS`), escribe lo pendiente y corta los que llevan
    /// `PLAZO` quietos.
    pub fn mover(&mut self, tramas: &mut Vec<(u8, Vec<u8>)>) {
        let numeros: Vec<u32> = self.tubos.keys().copied().collect();
        let mut buf = vec![0u8; 64 * 1024];
        for n in numeros {
            let Some(t) = self.tubos.get_mut(&n) else { continue };
            if t.ultimo.elapsed() > PLAZO {
                self.malos += 1;
                self.cortar(n, Some(tramas));
                continue;
            }
            if t.escribe {
                self.escribir(n, tramas);
                continue;
            }
            let k = unsafe { libc::read(t.fd, buf.as_mut_ptr().cast(), buf.len()) };
            if k > 0 {
                t.bytes += k as u64;
                t.ultimo = Instant::now();
                self.bytes += k as u64;
                if t.bytes > MAX_TUBO {
                    self.malos += 1;
                    self.cortar(n, Some(tramas));
                    continue;
                }
                let mut d = n.to_le_bytes().to_vec();
                d.extend_from_slice(&buf[..k as usize]);
                tramas.push((TUBO_DATOS, d));
            } else if k == 0 || std::io::Error::last_os_error().kind() != std::io::ErrorKind::WouldBlock {
                // El programa cerró (el fin) o algo falló.
                self.cortar(n, Some(tramas));
            }
        }
    }

    pub fn vivos(&self) -> usize {
        self.tubos.len()
    }
}

impl Drop for Tubos {
    fn drop(&mut self) {
        for t in self.tubos.values() {
            unsafe { libc::close(t.fd) };
        }
    }
}
