//! Lo común del puente de Wayland (G1 de `docs/GRAFICOS_LOOP.md`) y del compositor del PC (G0): sockets de Unix con
//! descriptores, y las tramas del transporte entre la caja Linux y DashOS.
//!
//! **El transporte** es un flujo de bytes (hoy un socket de Unix; en la caja Linux, `vsock`) con tramas:
//! tipo (1 byte), conexión (u32), largo (u32) y el contenido, todo en little-endian. Una conexión es un cliente de
//! Wayland de la caja. Los descriptores no cruzan: en su lugar viajan memorias con número, que el otro lado crea y
//! rellena (`MEMORIA_NUEVA`, `MEMORIA_ESCRIBIR`), y los mensajes de Wayland llevan esos números como llaves.

use std::io::{self, Read, Write};
use std::os::fd::RawFd;

/// Un cliente nuevo.
pub const CONECTAR: u8 = 1;
/// Bytes de Wayland: n (u32), n números de memoria (u32) y los bytes.
pub const DATOS: u8 = 2;
/// El cliente se fue.
pub const ADIOS: u8 = 3;
/// Una memoria nueva: número (u32) y tamaño (u64).
pub const MEMORIA_NUEVA: u8 = 4;
/// Escribir en una memoria: número (u32), desde (u64) y los bytes.
pub const MEMORIA_ESCRIBIR: u8 = 5;
/// Soltar una memoria: número (u32).
pub const MEMORIA_SOLTAR: u8 = 6;
/// Crecer una memoria: número (u32) y tamaño nuevo (u64).
pub const MEMORIA_CRECER: u8 = 7;

/// Lo más que mide el contenido de una trama: un búfer de 4K a 32 bits cabe en unas pocas.
pub const MAX_TRAMA: usize = 4 << 20;

pub struct Trama {
    pub tipo: u8,
    pub conexion: u32,
    pub datos: Vec<u8>,
}

pub fn escribir_trama(w: &mut impl Write, tipo: u8, conexion: u32, datos: &[u8]) -> io::Result<()> {
    let mut cab = [0u8; 9];
    cab[0] = tipo;
    cab[1..5].copy_from_slice(&conexion.to_le_bytes());
    cab[5..9].copy_from_slice(&(datos.len() as u32).to_le_bytes());
    w.write_all(&cab)?;
    w.write_all(datos)
}

/// Escribe en varias tramas lo que no cabe en una (para `MEMORIA_ESCRIBIR`).
pub fn escribir_memoria(w: &mut impl Write, numero: u32, desde: u64, bytes: &[u8]) -> io::Result<()> {
    for (i, trozo) in bytes.chunks(MAX_TRAMA - 12).enumerate() {
        let mut d = Vec::with_capacity(12 + trozo.len());
        d.extend_from_slice(&numero.to_le_bytes());
        d.extend_from_slice(&(desde + (i * (MAX_TRAMA - 12)) as u64).to_le_bytes());
        d.extend_from_slice(trozo);
        escribir_trama(w, MEMORIA_ESCRIBIR, 0, &d)?;
    }
    Ok(())
}

/// Saca las tramas enteras que haya en `buf` (lo que sobra se queda para la siguiente lectura).
pub fn sacar_tramas(buf: &mut Vec<u8>) -> io::Result<Vec<Trama>> {
    let mut out = Vec::new();
    loop {
        if buf.len() < 9 {
            break;
        }
        let largo = u32::from_le_bytes(buf[5..9].try_into().unwrap()) as usize;
        if largo > MAX_TRAMA {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "trama demasiado larga"));
        }
        if buf.len() < 9 + largo {
            break;
        }
        let t = Trama { tipo: buf[0], conexion: u32::from_le_bytes(buf[1..5].try_into().unwrap()), datos: buf[9..9 + largo].to_vec() };
        buf.drain(..9 + largo);
        out.push(t);
    }
    Ok(out)
}

/// Lee lo que haya del transporte sin esperar. `None` si se cerró.
pub fn leer_transporte(r: &mut impl Read, buf: &mut Vec<u8>) -> Option<()> {
    let mut b = [0u8; 65536];
    match r.read(&mut b) {
        Ok(0) => None,
        Ok(n) => {
            buf.extend_from_slice(&b[..n]);
            Some(())
        }
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => Some(()),
        Err(_) => None,
    }
}

/// Los datos de una trama `DATOS`: (números de memoria, bytes de Wayland).
pub fn partir_datos(d: &[u8]) -> Option<(Vec<u32>, &[u8])> {
    let n = u32::from_le_bytes(d.get(0..4)?.try_into().ok()?) as usize;
    let llaves = (0..n).map(|i| d.get(4 + 4 * i..8 + 4 * i).map(|b| u32::from_le_bytes(b.try_into().unwrap()))).collect::<Option<Vec<_>>>()?;
    Some((llaves, d.get(4 + 4 * n..)?))
}

pub fn juntar_datos(llaves: &[u32], bytes: &[u8]) -> Vec<u8> {
    let mut d = Vec::with_capacity(4 + 4 * llaves.len() + bytes.len());
    d.extend_from_slice(&(llaves.len() as u32).to_le_bytes());
    for l in llaves {
        d.extend_from_slice(&l.to_le_bytes());
    }
    d.extend_from_slice(bytes);
    d
}

/// Lee lo que haya en un socket de Unix: bytes y descriptores (SCM_RIGHTS). `None` si se cerró.
pub fn recibir(fd: RawFd, bytes: &mut Vec<u8>, fds: &mut Vec<RawFd>) -> Option<()> {
    let mut buf = [0u8; 65536];
    let mut control = [0u8; 512];
    let mut iov = libc::iovec { iov_base: buf.as_mut_ptr().cast(), iov_len: buf.len() };
    let mut m: libc::msghdr = unsafe { std::mem::zeroed() };
    m.msg_iov = &mut iov;
    m.msg_iovlen = 1;
    m.msg_control = control.as_mut_ptr().cast();
    m.msg_controllen = control.len() as _;
    let n = unsafe { libc::recvmsg(fd, &mut m, libc::MSG_DONTWAIT | libc::MSG_CMSG_CLOEXEC) };
    if n == 0 {
        return None;
    }
    if n < 0 {
        let e = io::Error::last_os_error();
        return (e.kind() == io::ErrorKind::WouldBlock).then_some(());
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

/// Manda bytes y descriptores (los descriptores, con el primer trozo). Falso si no se pudo.
pub fn mandar(fd: RawFd, bytes: &[u8], fds: &[RawFd]) -> bool {
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
        let n = unsafe { libc::sendmsg(fd, &m, libc::MSG_NOSIGNAL) };
        if n <= 0 {
            if n < 0 && io::Error::last_os_error().kind() == io::ErrorKind::WouldBlock {
                // Cliente lento: se espera un poco (el puente y el compositor del PC son para pruebas y la caja).
                std::thread::sleep(std::time::Duration::from_millis(1));
                continue;
            }
            return false;
        }
        con_fds = false;
        hecho += n as usize;
    }
    true
}

/// Una memoria compartida de Linux (`memfd`) de `tamano` bytes.
pub fn memfd(nombre: &std::ffi::CStr, tamano: u64) -> Option<RawFd> {
    let fd = unsafe { libc::memfd_create(nombre.as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return None;
    }
    if unsafe { libc::ftruncate(fd, tamano as libc::off_t) } < 0 {
        unsafe { libc::close(fd) };
        return None;
    }
    Some(fd)
}
