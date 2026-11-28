//! La caja Amiga en Linux (docs/CAJA_LINUX_LOOP.md, L5): el transporte de Wayland del compositor (`compositor.rs`)
//! sobre el socket de Unix del compositor de Linux (`$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY`). El flujo va tal cual, con
//! los descriptores aparte (SCM_RIGHTS); las "llaves" son descriptores. La misma forma que `transporte.rs` de DashOS.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use wayland::Escritor;
use wayland::canal::Juntador;

use crate::sys;

pub enum Recogido {
    Nada,
    Algo,
    Cerrado(i64),
}

pub struct Transporte {
    fd: i32,
    juntador: Juntador,
    /// Lo proyectado para los búferes: dirección y tamaño (munmap quiere el tamaño).
    memorias: BTreeMap<usize, usize>,
}

impl Transporte {
    pub fn nada() -> Transporte {
        Transporte { fd: -1, juntador: Juntador::new(), memorias: BTreeMap::new() }
    }

    /// Se conecta al compositor de Wayland de la sesión. La llave de DashOS no cuenta aquí.
    pub fn conectar(_llave: u32) -> Option<Transporte> {
        let pantalla = sys::entorno(b"WAYLAND_DISPLAY\0").filter(|p| !p.is_empty()).unwrap_or(b"wayland-0");
        let mut camino = Vec::new();
        if pantalla[0] != b'/' {
            camino.extend_from_slice(sys::entorno(b"XDG_RUNTIME_DIR\0")?);
            camino.push(b'/');
        }
        camino.extend_from_slice(pantalla);
        let fd = sys::conectar_unix(&camino).ok()?;
        Some(Transporte { fd, juntador: Juntador::new(), memorias: BTreeMap::new() })
    }

    /// Manda los mensajes de `e`; sus descriptores van con los primeros bytes y después se cierran (el compositor ya
    /// tiene los suyos). `false` si no se pudo.
    pub fn mandar(&mut self, e: &Escritor) -> bool {
        let fds: Vec<i32> = e.llaves.iter().map(|&l| l as i32).collect();
        let mut resto = &e.bytes[..];
        let mut con_fds = &fds[..];
        let mut bien = true;
        while !resto.is_empty() {
            match sys::sendmsg(self.fd, resto, con_fds) {
                Ok(n) => {
                    resto = &resto[n.min(resto.len())..];
                    con_fds = &[];
                }
                Err(sys::EINTR) | Err(sys::EAGAIN) => {}
                Err(_) => {
                    bien = false;
                    break;
                }
            }
        }
        for &f in &fds {
            sys::close(f);
        }
        bien
    }

    /// Lee lo que haya, esperando como mucho `us` (por milisegundos enteros: menos de uno, sin esperar).
    pub fn recoger(&mut self, us: u64) -> Recogido {
        match sys::esperar_lectura(self.fd, us / 1000) {
            Ok(true) => {}
            Ok(false) | Err(sys::EINTR) => return Recogido::Nada,
            Err(e) => return Recogido::Cerrado(-(e as i64)),
        }
        let mut buf = [0u8; 4096];
        let mut fds = [-1i32; sys::MAX_FDS];
        match sys::recvmsg(self.fd, &mut buf, &mut fds) {
            Ok((0, _)) => Recogido::Cerrado(0),
            Ok((n, k)) => {
                let llaves: Vec<u32> = fds[..k].iter().map(|&f| f as u32).collect();
                if self.juntador.meter(&buf[..n], &llaves).is_err() {
                    for &f in &fds[..k] {
                        sys::close(f);
                    }
                }
                Recogido::Algo
            }
            Err(sys::EINTR) | Err(sys::EAGAIN) => Recogido::Nada,
            Err(e) => Recogido::Cerrado(-(e as i64)),
        }
    }

    pub fn siguiente(&mut self) -> Option<(u32, u16, Vec<u8>)> {
        match self.juntador.siguiente() {
            Ok(Some(m)) => Some((m.objeto, m.orden, m.cuerpo)),
            _ => None,
        }
    }

    pub fn tomar_llave(&mut self) -> Option<u32> {
        self.juntador.tomar_llave()
    }

    pub fn cerrar(l: u32) {
        sys::close(l as i32);
    }

    /// Un memfd de `tam` bytes proyectado en `va`. La llave local no hace falta (0); la que se manda es el memfd, que
    /// `mandar` cierra al mandarlo.
    pub fn memoria(&mut self, tam: usize, va: usize) -> Option<(u32, u32)> {
        let fd = sys::memoria_compartida(tam, va).ok()?;
        self.memorias.insert(va, tam);
        Some((0, fd as u32))
    }

    pub fn soltar_memoria(&mut self, _memoria: u32, va: usize) {
        if let Some(tam) = self.memorias.remove(&va) {
            sys::munmap(va, tam);
        }
    }
}
