//! La caja Amiga en Linux (docs/CAJA_LINUX_LOOP.md, L4): las llamadas al sistema de Linux que usa el pegamento (los
//! ficheros), a mano, sin libc. Los números y las estructuras son los del ABI de Linux (sus cabeceras uapi): en x86-64
//! los suyos; en AArch64, los genéricos.

use core::arch::asm;

#[cfg(target_arch = "x86_64")]
mod nr {
    pub const CLOSE: usize = 3;
    pub const LSEEK: usize = 8;
    pub const PREAD64: usize = 17;
    pub const PWRITE64: usize = 18;
    pub const FCNTL: usize = 72;
    pub const FTRUNCATE: usize = 77;
    pub const GETCWD: usize = 79;
    pub const FCHMOD: usize = 91;
    pub const FSTATFS: usize = 138;
    pub const GETDENTS64: usize = 217;
    pub const OPENAT: usize = 257;
    pub const MKDIRAT: usize = 258;
    pub const UNLINKAT: usize = 263;
    pub const UTIMENSAT: usize = 280;
    pub const RENAMEAT2: usize = 316;
    pub const STATX: usize = 332;
    pub const MMAP: usize = 9;
    pub const MUNMAP: usize = 11;
    pub const SOCKET: usize = 41;
    pub const CONNECT: usize = 42;
    pub const SENDMSG: usize = 46;
    pub const RECVMSG: usize = 47;
    pub const PPOLL: usize = 271;
    pub const MEMFD_CREATE: usize = 319;
}

#[cfg(target_arch = "aarch64")]
mod nr {
    pub const GETCWD: usize = 17;
    pub const FCNTL: usize = 25;
    pub const MKDIRAT: usize = 34;
    pub const UNLINKAT: usize = 35;
    pub const FSTATFS: usize = 44;
    pub const FTRUNCATE: usize = 46;
    pub const FCHMOD: usize = 52;
    pub const OPENAT: usize = 56;
    pub const CLOSE: usize = 57;
    pub const GETDENTS64: usize = 61;
    pub const LSEEK: usize = 62;
    pub const PREAD64: usize = 67;
    pub const PWRITE64: usize = 68;
    pub const UTIMENSAT: usize = 88;
    pub const RENAMEAT2: usize = 276;
    pub const STATX: usize = 291;
    pub const PPOLL: usize = 73;
    pub const SOCKET: usize = 198;
    pub const CONNECT: usize = 203;
    pub const SENDMSG: usize = 211;
    pub const RECVMSG: usize = 212;
    pub const MUNMAP: usize = 215;
    pub const MMAP: usize = 222;
    pub const MEMFD_CREATE: usize = 279;
}

#[cfg(target_arch = "x86_64")]
fn sys6(n: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> isize {
    let r: isize;
    // SEGURIDAD: una llamada al sistema; lo que lee o escribe lo dicen sus argumentos, que comprueba cada envoltorio.
    unsafe {
        asm!("syscall", inlateout("rax") n as isize => r, in("rdi") a, in("rsi") b, in("rdx") c, in("r10") d,
             in("r8") e, in("r9") f, lateout("rcx") _, lateout("r11") _, options(nostack));
    }
    r
}

#[cfg(target_arch = "aarch64")]
fn sys6(n: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> isize {
    let r: isize;
    // SEGURIDAD: como en x86-64.
    unsafe {
        asm!("svc #0", in("x8") n, inlateout("x0") a as isize => r, in("x1") b, in("x2") c, in("x3") d, in("x4") e,
             in("x5") f, options(nostack));
    }
    r
}

pub const AT_FDCWD: i32 = -100;
pub const O_RDONLY: u32 = 0;
pub const O_RDWR: u32 = 2;
pub const O_CREAT: u32 = 0o100;
pub const O_EXCL: u32 = 0o200;
#[cfg(target_arch = "x86_64")]
pub const O_DIRECTORY: u32 = 0o200000;
#[cfg(target_arch = "aarch64")]
pub const O_DIRECTORY: u32 = 0o40000;
pub const O_CLOEXEC: u32 = 0o2000000;
pub const AT_REMOVEDIR: u32 = 0x200;
pub const AT_EMPTY_PATH: u32 = 0x1000;
pub const AT_SYMLINK_NOFOLLOW: u32 = 0x100;
pub const RENAME_NOREPLACE: u32 = 1;
const F_DUPFD_CLOEXEC: usize = 1030;
const STATX_BASICO: u32 = 0x7ff;

pub const ENOENT: isize = 2;
pub const EBADF: isize = 9;
pub const ENOMEM: isize = 12;
pub const EACCES: isize = 13;
pub const EBUSY: isize = 16;
pub const EEXIST: isize = 17;
pub const EXDEV: isize = 18;
pub const ENOTDIR: isize = 20;
pub const EISDIR: isize = 21;
pub const EINVAL: isize = 22;
pub const ETXTBSY: isize = 26;
pub const ENOSPC: isize = 28;
pub const EROFS: isize = 30;
pub const EPERM: isize = 1;
pub const ENAMETOOLONG: isize = 36;
pub const ENOTEMPTY: isize = 39;
pub const ELOOP: isize = 40;

/// Un camino de Linux, terminado en 0 (en la pila).
pub struct Camino {
    b: [u8; 512],
}

impl Camino {
    /// `None` si no cabe o lleva un 0.
    pub fn de(s: &str) -> Option<Camino> {
        let s = s.as_bytes();
        if s.len() >= 512 || s.contains(&0) {
            return None;
        }
        let mut b = [0u8; 512];
        b[..s.len()].copy_from_slice(s);
        Some(Camino { b })
    }
    fn p(&self) -> usize {
        self.b.as_ptr() as usize
    }
}

fn res(r: isize) -> Result<usize, isize> {
    if r < 0 { Err(-r) } else { Ok(r as usize) }
}

pub fn openat(dir: i32, c: &Camino, flags: u32, modo: u32) -> Result<i32, isize> {
    res(sys6(nr::OPENAT, dir as usize, c.p(), (flags | O_CLOEXEC) as usize, modo as usize, 0, 0)).map(|f| f as i32)
}

pub fn close(fd: i32) {
    sys6(nr::CLOSE, fd as usize, 0, 0, 0, 0, 0);
}

pub fn dup(fd: i32) -> Result<i32, isize> {
    res(sys6(nr::FCNTL, fd as usize, F_DUPFD_CLOEXEC, 0, 0, 0, 0)).map(|f| f as i32)
}

pub fn pread(fd: i32, buf: &mut [u8], desde: u64) -> Result<usize, isize> {
    res(sys6(nr::PREAD64, fd as usize, buf.as_mut_ptr() as usize, buf.len(), desde as usize, 0, 0))
}

pub fn pwrite(fd: i32, datos: &[u8], desde: u64) -> Result<usize, isize> {
    res(sys6(nr::PWRITE64, fd as usize, datos.as_ptr() as usize, datos.len(), desde as usize, 0, 0))
}

pub fn lseek0(fd: i32) -> Result<usize, isize> {
    res(sys6(nr::LSEEK, fd as usize, 0, 0, 0, 0, 0))
}

pub fn ftruncate(fd: i32, tam: u64) -> Result<usize, isize> {
    res(sys6(nr::FTRUNCATE, fd as usize, tam as usize, 0, 0, 0, 0))
}

pub fn fchmod(fd: i32, modo: u32) -> Result<usize, isize> {
    res(sys6(nr::FCHMOD, fd as usize, modo as usize, 0, 0, 0, 0))
}

pub fn mkdirat(dir: i32, c: &Camino, modo: u32) -> Result<usize, isize> {
    res(sys6(nr::MKDIRAT, dir as usize, c.p(), modo as usize, 0, 0, 0))
}

pub fn unlinkat(dir: i32, c: &Camino, flags: u32) -> Result<usize, isize> {
    res(sys6(nr::UNLINKAT, dir as usize, c.p(), flags as usize, 0, 0, 0))
}

pub fn renameat2(dir: i32, viejo: &Camino, dir_nuevo: i32, nuevo: &Camino, flags: u32) -> Result<usize, isize> {
    res(sys6(nr::RENAMEAT2, dir as usize, viejo.p(), dir_nuevo as usize, nuevo.p(), flags as usize, 0))
}

/// La fecha de modificación (segundos desde 1970) de lo abierto en `fd`; el acceso, sin tocar.
pub fn fechar(fd: i32, segundos: u64) -> Result<usize, isize> {
    const UTIME_OMIT: i64 = (1 << 30) - 2;
    let t: [i64; 4] = [0, UTIME_OMIT, segundos as i64, 0];
    res(sys6(nr::UTIMENSAT, fd as usize, 0, t.as_ptr() as usize, 0, 0, 0))
}

pub fn getcwd(buf: &mut [u8]) -> Result<usize, isize> {
    res(sys6(nr::GETCWD, buf.as_mut_ptr() as usize, buf.len(), 0, 0, 0, 0))
}

pub fn getdents64(fd: i32, buf: &mut [u8]) -> Result<usize, isize> {
    res(sys6(nr::GETDENTS64, fd as usize, buf.as_mut_ptr() as usize, buf.len(), 0, 0, 0))
}

/// Lo que se usa de `struct statx` (256 bytes).
pub struct Estado {
    pub modo: u16,
    pub tamano: u64,
    pub mtime: i64,
}

/// statx de `c` desde `dir` (camino vacío: lo abierto en `dir`), sin seguir el último enlace si se pide.
pub fn statx(dir: i32, c: &Camino, sin_seguir: bool) -> Result<Estado, isize> {
    let mut b = [0u64; 32];
    let vacio = c.b[0] == 0;
    let flags = if vacio { AT_EMPTY_PATH } else { 0 } | if sin_seguir { AT_SYMLINK_NOFOLLOW } else { 0 };
    res(sys6(nr::STATX, dir as usize, c.p(), flags as usize, STATX_BASICO as usize, b.as_mut_ptr() as usize, 0))?;
    let p = b.as_ptr() as *const u8;
    // SEGURIDAD: `b` son 256 bytes escritos por el kernel con la forma de `struct statx`: stx_mode (u16) en el byte
    // 28, stx_size (u64) en el 40 y stx_mtime (tv_sec i64) en el 112.
    unsafe {
        Ok(Estado {
            modo: core::ptr::read_unaligned(p.add(28) as *const u16),
            tamano: core::ptr::read_unaligned(p.add(40) as *const u64),
            mtime: core::ptr::read_unaligned(p.add(112) as *const i64),
        })
    }
}

/// Lo que se usa de `struct statfs` (en las dos arquitecturas, palabras de 64 bits): f_bsize, f_blocks y f_bavail.
pub fn fstatfs(fd: i32) -> Result<(u64, u64, u64), isize> {
    let mut b = [0u64; 15];
    res(sys6(nr::FSTATFS, fd as usize, b.as_mut_ptr() as usize, 0, 0, 0, 0))?;
    Ok((b[1], b[2], b[4]))
}

// --- El socket de Wayland y la memoria compartida (linux/transporte.rs) ---

const AF_UNIX: usize = 1;
const SOCK_STREAM: usize = 1;
const SOCK_CLOEXEC: usize = 0o2000000;
const MSG_NOSIGNAL: usize = 0x4000;
const MSG_CMSG_CLOEXEC: usize = 0x40000000;
const SOL_SOCKET: i32 = 1;
const SCM_RIGHTS: i32 = 1;
const POLLIN: i16 = 1;
pub const EAGAIN: isize = 11;
pub const EINTR: isize = 4;

/// Un socket de Unix conectado a `camino`.
pub fn conectar_unix(camino: &[u8]) -> Result<i32, isize> {
    if camino.len() >= 108 {
        return Err(ENAMETOOLONG);
    }
    let fd = res(sys6(nr::SOCKET, AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0, 0, 0, 0))? as i32;
    // struct sockaddr_un: sun_family (u16) y sun_path (108 bytes).
    let mut dir = [0u8; 110];
    dir[0..2].copy_from_slice(&(AF_UNIX as u16).to_ne_bytes());
    dir[2..2 + camino.len()].copy_from_slice(camino);
    if let Err(e) = res(sys6(nr::CONNECT, fd as usize, dir.as_ptr() as usize, 2 + camino.len() + 1, 0, 0, 0)) {
        close(fd);
        return Err(e);
    }
    Ok(fd)
}

/// Lo más de descriptores que van o vienen con un mensaje.
pub const MAX_FDS: usize = 28;

#[repr(C)]
struct Iovec {
    base: usize,
    largo: usize,
}

#[repr(C)]
struct Msghdr {
    nombre: usize,
    nombre_largo: u32,
    iov: *const Iovec,
    iov_n: usize,
    control: usize,
    control_largo: usize,
    flags: i32,
}

/// El control de un mensaje con descriptores: cmsghdr (largo u64, nivel i32, tipo i32) y los descriptores.
#[repr(C, align(8))]
struct Control {
    largo: u64,
    nivel: i32,
    tipo: i32,
    fds: [i32; MAX_FDS],
}

/// Manda `bytes` por `fd`, con los descriptores `fds` (van con el primer byte). Lo que mandó.
pub fn sendmsg(fd: i32, bytes: &[u8], fds: &[i32]) -> Result<usize, isize> {
    let iov = Iovec { base: bytes.as_ptr() as usize, largo: bytes.len() };
    let mut c = Control { largo: 0, nivel: SOL_SOCKET, tipo: SCM_RIGHTS, fds: [0; MAX_FDS] };
    let n = fds.len().min(MAX_FDS);
    c.fds[..n].copy_from_slice(&fds[..n]);
    c.largo = (16 + 4 * n) as u64;
    let m = Msghdr {
        nombre: 0,
        nombre_largo: 0,
        iov: &iov,
        iov_n: 1,
        control: if n > 0 { &c as *const Control as usize } else { 0 },
        control_largo: if n > 0 { (16 + 4 * n).next_multiple_of(8) } else { 0 },
        flags: 0,
    };
    res(sys6(nr::SENDMSG, fd as usize, &m as *const Msghdr as usize, MSG_NOSIGNAL, 0, 0, 0))
}

/// Recibe en `buf` lo que haya por `fd`, y los descriptores que vengan en `fds`: (bytes, descriptores).
pub fn recvmsg(fd: i32, buf: &mut [u8], fds: &mut [i32; MAX_FDS]) -> Result<(usize, usize), isize> {
    let iov = Iovec { base: buf.as_mut_ptr() as usize, largo: buf.len() };
    let mut c = Control { largo: 0, nivel: 0, tipo: 0, fds: [-1; MAX_FDS] };
    let mut m = Msghdr {
        nombre: 0,
        nombre_largo: 0,
        iov: &iov,
        iov_n: 1,
        control: &mut c as *mut Control as usize,
        control_largo: core::mem::size_of::<Control>(),
        flags: 0,
    };
    let n = res(sys6(nr::RECVMSG, fd as usize, &mut m as *mut Msghdr as usize, MSG_CMSG_CLOEXEC, 0, 0, 0))?;
    let mut k = 0;
    if m.control_largo >= 16 && c.nivel == SOL_SOCKET && c.tipo == SCM_RIGHTS {
        k = ((c.largo as usize).saturating_sub(16) / 4).min(MAX_FDS);
        fds[..k].copy_from_slice(&c.fds[..k]);
    }
    Ok((n, k))
}

/// Espera hasta `ms` milisegundos a que haya algo que leer en `fd`. `true` si lo hay.
pub fn esperar_lectura(fd: i32, ms: u64) -> Result<bool, isize> {
    #[repr(C)]
    struct Pollfd {
        fd: i32,
        eventos: i16,
        devueltos: i16,
    }
    let mut p = Pollfd { fd, eventos: POLLIN, devueltos: 0 };
    let t: [i64; 2] = [(ms / 1000) as i64, ((ms % 1000) * 1_000_000) as i64];
    let n = res(sys6(nr::PPOLL, &mut p as *mut Pollfd as usize, 1, t.as_ptr() as usize, 0, 8, 0))?;
    Ok(n > 0)
}

/// Un memfd de `tam` bytes, proyectado para escribir en `va` (ahí o nada). Su descriptor.
pub fn memoria_compartida(tam: usize, va: usize) -> Result<i32, isize> {
    const PROT_RW: usize = 3;
    const MAP_SHARED: usize = 1;
    const MAP_FIXED_NOREPLACE: usize = 0x100000;
    let nombre = b"caja-ventana\0";
    let fd = res(sys6(nr::MEMFD_CREATE, nombre.as_ptr() as usize, 1, 0, 0, 0, 0))? as i32; // MFD_CLOEXEC
    let r = ftruncate(fd, tam as u64).and_then(|_| res(sys6(nr::MMAP, va, tam, PROT_RW, MAP_SHARED | MAP_FIXED_NOREPLACE, fd as usize, 0)));
    match r {
        Ok(d) if d == va => Ok(fd),
        Ok(d) => {
            sys6(nr::MUNMAP, d, tam, 0, 0, 0, 0);
            close(fd);
            Err(EEXIST)
        }
        Err(e) => {
            close(fd);
            Err(e)
        }
    }
}

pub fn munmap(va: usize, tam: usize) {
    sys6(nr::MUNMAP, va, tam, 0, 0, 0, 0);
}

unsafe extern "C" {
    /// caja_linux.c: el valor de una variable del entorno de Linux, o NULL.
    fn plat_entorno(nombre: *const u8) -> *const u8;
}

/// El valor de la variable del entorno `nombre` (terminado en 0).
pub fn entorno(nombre: &[u8]) -> Option<&'static [u8]> {
    if nombre.last() != Some(&0) {
        return None;
    }
    // SEGURIDAD: `nombre` termina en 0; lo devuelto es del entorno del proceso (en la pila del arranque, para
    // siempre), terminado en 0.
    unsafe {
        let p = plat_entorno(nombre.as_ptr());
        if p.is_null() {
            return None;
        }
        let mut n = 0;
        while *p.add(n) != 0 {
            n += 1;
        }
        Some(core::slice::from_raw_parts(p, n))
    }
}
