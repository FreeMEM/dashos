//! Lo común del puente de Wayland (G1 de `docs/GRAFICOS_LOOP.md`) y del compositor del PC (G0): sockets de Unix con
//! descriptores, y las tramas del transporte entre la caja Linux y DashOS.
//!
//! **El transporte** es un flujo de bytes (en el PC, un socket de Unix; en la caja Linux, `vsock`) con tramas:
//! tipo (1 byte), conexión (u32), largo (u32) y el contenido, todo en little-endian. Una conexión es un cliente de
//! Wayland de la caja. Los descriptores no cruzan: en su lugar viajan memorias con número, que el otro lado crea y
//! rellena (`MEMORIA_NUEVA`, `MEMORIA_ESCRIBIR`), y los mensajes de Wayland llevan esos números como llaves.

use std::io::{self, Read, Write};
use std::os::fd::RawFd;

// El formato de las tramas es el de `wayland::puente` (sin std): el mismo para los dos lados.
pub use wayland::puente::{ADIOS, CONECTAR, DATOS, MAX_TRAMA, MEMORIA_CRECER, MEMORIA_ESCRIBIR, MEMORIA_NUEVA, MEMORIA_SOLTAR, TUBO_DATOS, TUBO_FIN, TUBO_LEIDO, TUBO_NUEVO, juntar_datos, partir_datos};

/// Los tubos del portapapeles con tuberías de Linux (H9).
pub mod tubos;

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

/// Conecta por vsock con el puerto `puerto` del anfitrión (CID 2: DashOS, visto desde la caja Linux). El descriptor
/// vale como un socket de flujo cualquiera.
pub fn vsock(puerto: u32) -> io::Result<std::os::unix::net::UnixStream> {
    use std::os::fd::FromRawFd;
    let fd = unsafe { libc::socket(libc::AF_VSOCK, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut dir: libc::sockaddr_vm = unsafe { std::mem::zeroed() };
    dir.svm_family = libc::AF_VSOCK as _;
    dir.svm_cid = 2;
    dir.svm_port = puerto;
    let r = unsafe { libc::connect(fd, (&dir as *const libc::sockaddr_vm).cast(), size_of::<libc::sockaddr_vm>() as u32) };
    if r < 0 {
        let e = io::Error::last_os_error();
        unsafe { libc::close(fd) };
        return Err(e);
    }
    // SAFETY: `fd` es un socket de flujo conectado y solo nuestro: el `UnixStream` lo cierra al soltarse (read, write
    // y shutdown no miran la familia del socket).
    Ok(unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd) })
}
